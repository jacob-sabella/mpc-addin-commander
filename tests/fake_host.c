// A plugin host for the offline tests, loading plugins the way MPC's does (dlopen, dlsym("VSTPluginMain"), a static
// host callback that finds the instance through resvd2, effOpen, effSetSampleRate, processReplacing on a worker
// thread), driven by commands on stdin from tests/test_ws.py:
//   load <so>            -> "loaded <n>"        set <n> <i> <v>      -> "ok"
//   get <n> <i>          -> "<value>"           engine <n> <i> <v>   -> "ok" (the plugin changes it itself)
//   close <n>            -> "ok"                next <n>             -> "1" when the plugin's RTLD_NEXT is right
//   quit
// The addin is started directly (this process may not be named MPC); in the preload smoke test the constructor
// has already started it and the call is a no-op.
#define _GNU_SOURCE
#include "../src/vst.h"
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int mpc_commander_addin_start(void);
#ifdef HOST_DLSYM                    // the sanitizer builds: the addin's hook under its test name, called directly
void *HOST_DLSYM(void *, const char *);
#define dlsym HOST_DLSYM
#endif

#define MAX 16
struct slot { AEffect *fx; void *handle; int in_use; } slots[MAX];
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static intptr_t host_callback(AEffect *fx, int32_t op, int32_t index, intptr_t value, void *ptr, float opt)
{
    (void)index; (void)value; (void)ptr; (void)opt;
    if (op == audioMasterVersion) return 2400;
    if (!fx) return 0;
    struct slot *s = (struct slot *)fx->resvd2;      // like JUCE: the instance pointer lives in resvd2
    if (!s || s->fx != fx) { fprintf(stderr, "fake_host: callback with an unknown effect\n"); abort(); }
    return 0;
}

static void *worker(void *arg)
{
    (void)arg;
    float l[64], r[64], *outs[2] = { l, r };
    for (;;) {
        pthread_mutex_lock(&lock);
        for (int k = 0; k < MAX; k++)
            if (slots[k].in_use && slots[k].fx->processReplacing) slots[k].fx->processReplacing(slots[k].fx, outs, outs, 64);
        pthread_mutex_unlock(&lock);
        usleep(2000);
    }
    return NULL;
}

int main(void)
{
    if (mpc_commander_addin_start()) return 1;
    pthread_t t;
    pthread_create(&t, NULL, worker, NULL);
    char line[1024];
    setvbuf(stdout, NULL, _IOLBF, 0);
    while (fgets(line, sizeof line, stdin)) {
        char cmd[16], path[900];
        int n, i;
        float v;
        if (sscanf(line, "%15s", cmd) != 1) continue;
        if (!strcmp(cmd, "quit")) break;
        if (!strcmp(cmd, "load") && sscanf(line, "%*s %899s", path) == 1) {
            int k = 0;
            while (k < MAX && slots[k].in_use) k++;
            if (k == MAX) { puts("error full"); continue; }
            void *h = dlopen(path, RTLD_NOW | RTLD_LOCAL);
            if (!h) { printf("error %s\n", dlerror()); continue; }
            AEffect *(*entry)(audioMasterCallback) = (AEffect *(*)(audioMasterCallback))dlsym(h, "VSTPluginMain");
            if (!entry) entry = (AEffect *(*)(audioMasterCallback))dlsym(h, "main");
            AEffect *fx = entry ? entry(host_callback) : NULL;
            if (!fx || fx->magic != VST_MAGIC) { puts("error no plugin"); dlclose(h); continue; }
            fx->resvd2 = (intptr_t)&slots[k];
            fx->dispatcher(fx, effOpen, 0, 0, NULL, 0);
            fx->dispatcher(fx, effSetSampleRate, 0, 0, NULL, 44100);
            fx->dispatcher(fx, effSetBlockSize, 0, 64, NULL, 0);
            fx->dispatcher(fx, effMainsChanged, 0, 1, NULL, 0);
            pthread_mutex_lock(&lock);
            slots[k].fx = fx; slots[k].handle = h; slots[k].in_use = 1;
            pthread_mutex_unlock(&lock);
            printf("loaded %d\n", k);
        } else if (!strcmp(cmd, "set") && sscanf(line, "%*s %d %d %f", &n, &i, &v) == 3 && n >= 0 && n < MAX && slots[n].in_use) {
            slots[n].fx->setParameter(slots[n].fx, i, v);
            puts("ok");
        } else if (!strcmp(cmd, "engine") && sscanf(line, "%*s %d %d %f", &n, &i, &v) == 3 && n >= 0 && n < MAX && slots[n].in_use) {
            slots[n].fx->dispatcher(slots[n].fx, 0x7001, i, 0, NULL, v);
            puts("ok");
        } else if (!strcmp(cmd, "next") && sscanf(line, "%*s %d", &n) == 1 && n >= 0 && n < MAX && slots[n].in_use) {
            printf("%d\n", (int)slots[n].fx->dispatcher(slots[n].fx, 0x7002, 0, 0, NULL, 0));
        } else if (!strcmp(cmd, "get") && sscanf(line, "%*s %d %d", &n, &i) == 2 && n >= 0 && n < MAX && slots[n].in_use) {
            printf("%.6g\n", (double)slots[n].fx->getParameter(slots[n].fx, i));
        } else if (!strcmp(cmd, "close") && sscanf(line, "%*s %d", &n) == 1 && n >= 0 && n < MAX && slots[n].in_use) {
            pthread_mutex_lock(&lock);
            slots[n].in_use = 0;
            pthread_mutex_unlock(&lock);
            slots[n].fx->dispatcher(slots[n].fx, effMainsChanged, 0, 0, NULL, 0);
            slots[n].fx->dispatcher(slots[n].fx, effClose, 0, 0, NULL, 0);
            int others = 0;
            for (int k = 0; k < MAX; k++) if (slots[k].in_use && slots[k].handle == slots[n].handle) others++;
            if (!others) dlclose(slots[n].handle);     // like JUCE: the module goes after its last instance
            slots[n].fx = NULL; slots[n].handle = NULL;
            puts("ok");
        } else {
            puts("error bad command");
        }
    }
    _exit(0);
}
