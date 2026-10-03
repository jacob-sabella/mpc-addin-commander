// The project file is gzip over five header lines (product, version, class, "json", platform) and a JSON document.
// It is read with the device's own zlib (loaded at run time, never linked), tokenised whole and summarised: the
// tempo, the current sequence and every track's mixer state and plugin.
#include "project.h"
#include "addin.h"
#include <dlfcn.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

enum { MAX_FILE = 64 << 20, MAX_TOKENS = 1 << 22 };

static struct {
    void *(*open)(const char *, const char *);
    int (*read)(void *, void *, unsigned);
    int (*close)(void *);
} Z;

static int load_zlib_locked(void)
{
    if (Z.open) return 0;
    void *lib = dlopen("libz.so.1", RTLD_NOW | RTLD_LOCAL);
    if (!lib) return -1;
    *(void **)&Z.open = dlsym(lib, "gzopen");
    *(void **)&Z.read = dlsym(lib, "gzread");
    *(void **)&Z.close = dlsym(lib, "gzclose");
    if (Z.open && Z.read && Z.close) return 0;
    Z.open = NULL;
    dlclose(lib);
    return -1;
}

// Every client thread can ask for the project at once: load zlib under a lock.
static int load_zlib(void)
{
    static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
    pthread_mutex_lock(&lock);
    int r = load_zlib_locked();
    pthread_mutex_unlock(&lock);
    return r;
}

static void xml_unescape(char *s)
{
    static const struct { const char *ent; char c; } E[] = {
        { "&amp;", '&' }, { "&quot;", '"' }, { "&apos;", '\'' }, { "&lt;", '<' }, { "&gt;", '>' }
    };
    char *w = s;
    for (char *r = s; *r;) {
        int done = 0;
        if (*r == '&')
            for (size_t k = 0; k < sizeof E / sizeof E[0]; k++) {
                size_t n = strlen(E[k].ent);
                if (!strncmp(r, E[k].ent, n)) { *w++ = E[k].c; r += n; done = 1; break; }
            }
        if (!done) *w++ = *r++;
    }
    *w = 0;
}

// recentProject1 from MPC's settings. 0 when found.
static int recent_project(char *out, size_t n)
{
    FILE *f = fopen(C.settings, "re");
    if (!f) return -1;
    char line[1024];
    int found = -1;
    while (found && fgets(line, sizeof line, f)) {
        char *p = strstr(line, "name=\"recentProject1\"");
        if (!p) continue;
        p = strstr(p, "val=\"");
        if (!p) continue;
        p += 5;
        char *e = strchr(p, '"');
        if (!e || (size_t)(e - p) >= n) continue;
        memcpy(out, p, (size_t)(e - p));
        out[e - p] = 0;
        xml_unescape(out);
        found = 0;
    }
    fclose(f);
    return found;
}

// The JSON document after the header lines, NUL-terminated, malloc'd. NULL with *err set otherwise.
static char *read_project(const char *path, size_t *len, const char **err)
{
    if (load_zlib()) { *err = "zlib isn't available"; return NULL; }
    void *gz = Z.open(path, "rb");
    if (!gz) { *err = "the project file can't be opened"; return NULL; }
    size_t cap = 1 << 20, n = 0;
    char *buf = malloc(cap);
    int r = 0;
    while (buf && (r = Z.read(gz, buf + n, (unsigned)(cap - n - 1))) > 0) {
        n += (size_t)r;
        if (cap - n - 1 < 4096) {
            if (cap >= MAX_FILE) { r = -1; break; }
            char *bigger = realloc(buf, cap * 2);
            if (!bigger) { free(buf); buf = NULL; break; }
            buf = bigger;
            cap *= 2;
        }
    }
    Z.close(gz);
    if (!buf) { *err = "out of memory"; return NULL; }
    if (r < 0) { free(buf); *err = "the project file can't be read (not gzip, or too large)"; return NULL; }
    buf[n] = 0;
    char *p = buf;
    for (int k = 0; k < 5; k++) {   // ACVS, the version, SerialisableProjectData, json, the platform
        p = strchr(p, '\n');
        if (!p) { free(buf); *err = "the project file has no header"; return NULL; }
        p++;
    }
    if (strncmp(buf, "ACVS", 4) || strncmp(p, "{", 1)) { free(buf); *err = "not a project file"; return NULL; }
    *len = n - (size_t)(p - buf);
    memmove(buf, p, *len + 1);
    return buf;
}

static struct jtok *tokenise(const char *s, size_t n, int *count)
{
    for (int max = 1 << 16; max <= MAX_TOKENS; max *= 4) {
        struct jtok *t = malloc((size_t)max * sizeof *t);
        if (!t) return NULL;
        int c = json_parse(s, n, t, max);
        if (c > 0) { *count = c; return t; }
        free(t);
    }
    return NULL;
}

static double num_or(const char *s, const struct jtok *t, int obj, const char *key, double dflt)
{
    int i = json_get(s, t, obj, key);
    double v;
    return i >= 0 && json_num(s, t, i, &v) == 0 ? v : dflt;
}

static int bool_or(const char *s, const struct jtok *t, int obj, const char *key, int dflt)
{
    int i = json_get(s, t, obj, key), v;
    return i >= 0 && json_bool(s, t, i, &v) == 0 ? v : dflt;
}

static void put_str(struct sb *b, const char *s, const struct jtok *t, int obj, const char *key)
{
    char v[256] = "";
    int i = json_get(s, t, obj, key);
    if (i < 0 || json_str(s, t, i, v, sizeof v)) { sb_puts(b, "null"); return; }
    sb_jstr(b, v);
}

static const char *track_type(int kind)
{
    switch (kind) {   // program.type values seen in project files; the rest are reported by number only
    case 0: return "drum";
    case 3: return "plugin";
    case 6: return "audio";
    case 7: return "return";
    case 8: return "submix";
    case 9: return "output";
    case 10: return "input";
    default: return "other";
    }
}

static int array_item(const struct jtok *t, int arr, int index)
{
    if (t[arr].type != J_ARR || index < 0 || index >= t[arr].size) return -1;
    int k = arr + 1;
    for (int m = 0; m < index; m++) k = t[k].skip;
    return k;
}

static void track_json(struct sb *b, const char *s, const struct jtok *t, int tr, int index)
{
    int prog = json_get(s, t, tr, "program");
    int mix = prog >= 0 ? json_get(s, t, prog, "mixable") : -1;
    int kind = prog >= 0 ? (int)num_or(s, t, prog, "type", -1) : -1;
    sb_printf(b, "{\"index\":%d,\"name\":", index);
    put_str(b, s, t, tr, "name");
    sb_printf(b, ",\"kind\":%d,\"type\":\"%s\",\"colour\":%.0f,\"track_mute\":%s,\"record_arm\":%s", kind, track_type(kind),
              num_or(s, t, tr, "colour", 0), bool_or(s, t, tr, "mute", 0) ? "true" : "false",
              bool_or(s, t, tr, "recordArm", 0) ? "true" : "false");
    if (mix >= 0)
        sb_printf(b, ",\"mute\":%s,\"solo\":%s,\"volume\":%.4f,\"pan\":%.4f", bool_or(s, t, mix, "mute", 0) ? "true" : "false",
                  bool_or(s, t, mix, "solo", 0) ? "true" : "false", num_or(s, t, mix, "volume", 0), num_or(s, t, mix, "pan", 0.5));
    int plugin = prog >= 0 ? json_get(s, t, prog, "plugin") : -1;
    int inner = plugin >= 0 ? json_get(s, t, plugin, "plugin") : -1;
    int desc = inner >= 0 ? json_get(s, t, inner, "description") : -1;
    sb_puts(b, ",\"plugin\":");
    if (desc >= 0) {
        sb_puts(b, "{\"name\":");
        put_str(b, s, t, desc, "name");
        sb_puts(b, ",\"vendor\":");
        put_str(b, s, t, desc, "manufacturerName");
        sb_puts(b, ",\"format\":");
        put_str(b, s, t, desc, "pluginFormatName");
        sb_puts(b, ",\"file\":");
        put_str(b, s, t, desc, "fileOrIdentifier");
        sb_puts(b, ",\"preset\":");
        put_str(b, s, t, inner, "presetName");
        sb_puts(b, "}");
    } else sb_puts(b, "null");
    sb_puts(b, "}");
}

static void sequence_json(struct sb *b, const char *s, const struct jtok *t, int seqs, int index, double *tempo)
{
    int item = array_item(t, seqs, index);
    int v = item >= 0 ? json_get(s, t, item, "value") : -1;
    if (v < 0) { sb_puts(b, "null"); return; }
    *tempo = num_or(s, t, v, "bpm", *tempo);
    sb_printf(b, "{\"index\":%d,\"name\":", index);
    put_str(b, s, t, v, "name");
    int ts = json_get(s, t, v, "timeSignatureTrack");
    int list = ts >= 0 ? json_get(s, t, ts, "timeSignatures") : -1;
    int first = list >= 0 ? array_item(t, list, 0) : -1;
    sb_printf(b, ",\"tempo\":%.3f,\"tempo_enabled\":%s,\"bars\":%.0f,\"loop\":%s,\"loop_start\":%.0f,\"loop_end\":%.0f,"
              "\"beats_per_bar\":%.0f,\"beat_length\":%.0f}", *tempo, bool_or(s, t, v, "tempoEnable", 1) ? "true" : "false",
              num_or(s, t, v, "lengthBars", 0), bool_or(s, t, v, "loop", 0) ? "true" : "false", num_or(s, t, v, "loopStartBar", 0),
              num_or(s, t, v, "loopEndBar", 0), first >= 0 ? num_or(s, t, first, "beatsPerBar", 4) : 4,
              first >= 0 ? num_or(s, t, first, "beatLength", 960) : 960);
}

int project_json(struct sb *b)
{
    char path[512];
    const char *err = NULL;
    if (C.project[0]) snprintf(path, sizeof path, "%s", C.project);
    else if (recent_project(path, sizeof path)) err = "no recent project in MPC's settings";
    size_t len = 0;
    char *s = err ? NULL : read_project(path, &len, &err);
    int count = 0;
    struct jtok *t = s ? tokenise(s, len, &count) : NULL;
    if (s && !t) err = "the project's JSON can't be parsed";
    int data = t ? json_get(s, t, 0, "data") : -1;
    if (t && data < 0) err = "the project has no data object";
    if (err) {
        free(t);
        free(s);
        sb_puts(b, "{\"t\":\"error\",\"msg\":");
        sb_jstr(b, err);
        sb_puts(b, "}");
        return -1;
    }
    struct stat st;
    long long mtime = stat(path, &st) == 0 ? (long long)st.st_mtime : 0;
    const char *base = strrchr(path, '/');
    char name[512];
    snprintf(name, sizeof name, "%s", base ? base + 1 : path);
    char *dot = strrchr(name, '.');
    if (dot && !strcmp(dot, ".xpj")) *dot = 0;

    sb_puts(b, "{\"t\":\"project\",\"path\":");
    sb_jstr(b, path);
    sb_puts(b, ",\"name\":");
    sb_jstr(b, name);
    sb_printf(b, ",\"mtime\":%lld,\"version\":%.0f,\"key\":", mtime, num_or(s, t, data, "version", 0));
    put_str(b, s, t, data, "key");
    double master = num_or(s, t, data, "masterTempo", 0), tempo = master;
    int master_on = bool_or(s, t, data, "masterTempoEnabled", 0);
    int cur_seq = (int)num_or(s, t, data, "currentSequence", 0);
    sb_printf(b, ",\"master_tempo\":%.3f,\"master_tempo_enabled\":%s,\"current_track\":%.0f,\"sequence\":", master,
              master_on ? "true" : "false", num_or(s, t, data, "currentTrackIndex", 0));
    int seqs = json_get(s, t, data, "sequences");
    double seq_tempo = tempo;
    if (seqs >= 0) sequence_json(b, s, t, seqs, cur_seq, &seq_tempo);
    else sb_puts(b, "null");
    sb_printf(b, ",\"tempo\":%.3f,\"tracks\":[", master_on ? master : seq_tempo);
    int tracks = json_get(s, t, data, "tracks");
    if (tracks >= 0 && t[tracks].type == J_ARR) {
        int k = tracks + 1;
        for (int m = 0; m < t[tracks].size; m++, k = t[k].skip) {
            if (m) sb_puts(b, ",");
            track_json(b, s, t, k, m);
        }
    }
    sb_puts(b, "]}");
    free(t);
    free(s);
    return 0;
}
