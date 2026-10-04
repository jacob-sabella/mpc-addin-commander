#define _GNU_SOURCE
#include "poll.h"
#include "addin.h"
#include "ws.h"
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define ANNOUNCE_WAIT_MS 1000     // an instance that never gets effOpen is announced after this anyway

static int removed[MAX_INST];     // ids closed since the last poll (under registry_lock)
static int nremoved;
static long long text_due[MAX_INST];

void hook_on_close(struct inst *in)
{
    free(in->params);
    in->params = NULL;
    if (in->announced && nremoved < MAX_INST) removed[nremoved++] = in->id;
    in->announced = 0;
}

static void get_string(const struct inst *in, AEffect *fx, int op, int index, char *out, size_t n)
{
    char buf[512];
    buf[0] = 0;
    in->dispatcher(fx, op, index, 0, buf, 0);
    buf[sizeof buf - 1] = 0;
    snprintf(out, n, "%s", buf);
}

static void param_json(struct sb *b, const struct param *p, int i)
{
    sb_printf(b, "{\"i\":%d,\"name\":", i);
    sb_jstr(b, p->name);
    sb_puts(b, ",\"label\":");
    sb_jstr(b, p->label);
    sb_printf(b, ",\"value\":%.6g,\"text\":", (double)p->value);
    sb_jstr(b, p->text);
    sb_puts(b, "}");
}

void inst_json(struct sb *b, const struct inst *in)
{
    const char *so = modules[in->module].path;
    sb_printf(b, "{\"id\":%d,\"name\":", in->id);
    sb_jstr(b, in->name);
    sb_puts(b, ",\"vendor\":");
    sb_jstr(b, in->vendor);
    sb_puts(b, ",\"product\":");
    sb_jstr(b, in->product);
    sb_printf(b, ",\"uid\":\"%08x\",\"so\":", in->uid);
    sb_jstr(b, so);
    sb_printf(b, ",\"skin\":%s,\"synth\":%s,\"sample_rate\":%d,\"params\":[",
              ws_skin_exists(so, in->vendor, in->product) ? "true" : "false", in->synth ? "true" : "false", atomic_load(&in->sample_rate));
    for (int i = 0; i < in->nparams; i++) {
        if (i) sb_puts(b, ",");
        param_json(b, &in->params[i], i);
    }
    sb_puts(b, "]}");
}

void plugins_json(struct sb *b)
{
    sb_puts(b, "{\"t\":\"plugins\",\"plugins\":[");
    pthread_mutex_lock(&registry_lock);
    int n = 0;
    for (int k = 0; k < MAX_INST; k++) {
        const struct inst *in = &insts[k];
        if (!atomic_load(&in->fx) || !in->announced) continue;
        if (n++) sb_puts(b, ",");
        inst_json(b, in);
    }
    pthread_mutex_unlock(&registry_lock);
    sb_puts(b, "]}");
}

// Names, labels, values and text for a new instance (caller holds registry_lock). -1 when out of memory.
static int announce(struct inst *in, AEffect *fx, struct sb *out)
{
    int n = in->nparams;
    in->params = calloc(n > 0 ? (size_t)n : 1, sizeof *in->params);
    if (!in->params) return -1;
    get_string(in, fx, effGetEffectName, 0, in->name, sizeof in->name);
    get_string(in, fx, effGetVendorString, 0, in->vendor, sizeof in->vendor);
    get_string(in, fx, effGetProductString, 0, in->product, sizeof in->product);
    for (int i = 0; i < n; i++) {
        struct param *p = &in->params[i];
        get_string(in, fx, effGetParamName, i, p->name, sizeof p->name);
        get_string(in, fx, effGetParamLabel, i, p->label, sizeof p->label);
        p->value = fx->getParameter(fx, i);
        get_string(in, fx, effGetParamDisplay, i, p->text, sizeof p->text);
    }
    in->announced = 1;
    sb_puts(out, "{\"t\":\"plugin_added\",\"plugin\":");
    inst_json(out, in);
    sb_puts(out, "}");
    return 0;
}

// The changed values (and texts) of one instance into out as a values message; 0 when nothing changed.
static int poll_instance(struct inst *in, AEffect *fx, int full_text, struct sb *out)
{
    int changed = 0;
    uint32_t force[32];
    for (int k = 0; k < 32; k++) force[k] = atomic_exchange(&in->force[k], 0);
    for (int i = 0; i < in->nparams; i++) {
        struct param *p = &in->params[i];
        float v = fx->getParameter(fx, i);
        int forced = i < 1024 && (force[i >> 5] >> (i & 31) & 1);
        int moved = v != p->value || forced;
        int text_changed = 0;
        if (moved || full_text) {
            char text[PARAM_TEXT];
            get_string(in, fx, effGetParamDisplay, i, text, sizeof text);
            text_changed = strcmp(text, p->text) != 0;
            if (text_changed) memcpy(p->text, text, sizeof text);
        }
        if (!moved && !text_changed) continue;
        p->value = v;
        if (!changed) sb_printf(out, "{\"t\":\"values\",\"id\":%d,\"v\":[", in->id);
        else sb_puts(out, ",");
        sb_printf(out, "[%d,%.6g,", i, (double)v);
        sb_jstr(out, p->text);
        sb_puts(out, "]");
        changed++;
    }
    if (changed) sb_puts(out, "]}");
    return changed;
}

void *poll_thread(void *arg)
{
    (void)arg;
    lower_priority();
    struct sb msgs[MAX_INST + 1];     // one message per instance per tick, plus the removals
    for (;;) {
        usleep((useconds_t)C.poll_ms * 1000);
        long long now = now_ms();
        int nmsg = 0;
        memset(msgs, 0, sizeof msgs);
        pthread_mutex_lock(&registry_lock);
        if (nremoved) {
            for (int r = 0; r < nremoved; r++) {
                if (r) sb_puts(&msgs[nmsg], "\n");
                sb_printf(&msgs[nmsg], "{\"t\":\"plugin_removed\",\"id\":%d}", removed[r]);
            }
            nremoved = 0;
            nmsg++;
        }
        for (int k = 0; k < MAX_INST; k++) {
            struct inst *in = &insts[k];
            AEffect *fx = atomic_load(&in->fx);
            if (!fx) continue;
            int dirty = atomic_exchange(&in->dirty, 0);
            if (!in->announced) {
                if (!atomic_load(&in->opened) && now - in->created_ms < ANNOUNCE_WAIT_MS) continue;
                if (announce(in, fx, &msgs[nmsg]) == 0) { text_due[k] = now + C.text_ms; nmsg++; }
                continue;
            }
            int full_text = (dirty & DIRTY_TEXT) || now >= text_due[k];
            if (full_text) text_due[k] = now + (ws_subscribed(in->id) ? C.poll_ms : C.text_ms);
            if (poll_instance(in, fx, full_text, &msgs[nmsg])) nmsg++;
        }
        pthread_mutex_unlock(&registry_lock);
        for (int i = 0; i < nmsg; i++) {
            if (!msgs[i].oom && msgs[i].n) {
                // removals are several messages separated by newlines; everything else is one
                char *s = msgs[i].p, *nl;
                while ((nl = memchr(s, '\n', msgs[i].n - (size_t)(s - msgs[i].p)))) {
                    ws_broadcast(s, (size_t)(nl - s));
                    s = nl + 1;
                }
                ws_broadcast(s, msgs[i].n - (size_t)(s - msgs[i].p));
            }
            sb_free(&msgs[i]);
        }
    }
    return NULL;
}
