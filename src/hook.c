// dlsym interposition and the instance registry. MPC's plugin host loads a plugin with dlopen and
// dlsym("VSTPluginMain") (or "main"). This file exports dlsym: everything passes through to libc's except those two
// names in a .so, which get a per-module trampoline. The trampoline calls the plugin's real entry with a per-module
// host callback, then patches the returned AEffect in place: its dispatcher and setParameter become ours (the
// originals go in the registry slot), so effClose unregisters, and the host's and the plugin's own parameter traffic
// marks the slot dirty for the poll thread. The audio-thread paths (setParameter, the host callback) touch atomics
// only; registration and close take registry_lock.
#define _GNU_SOURCE
#include "hook.h"
#include <dlfcn.h>
#include <link.h>
#include <link.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#define LOG(...) fprintf(stderr, "mpc-commander-addin: " __VA_ARGS__)

struct inst insts[MAX_INST];
struct module modules[MAX_MODULES];
pthread_mutex_t registry_lock = PTHREAD_MUTEX_INITIALIZER;
atomic_int instance_count;
static atomic_int enabled;
static int next_id = 1;

typedef void *(*dlsym_fn)(void *, const char *);
static dlsym_fn real_dlsym;

// The sanitizers' runtimes call dlsym while they initialise, before their shadow memory exists, so the lookup path
// is not instrumented and uses no lock (a racing double resolution finds the same function).
#define NO_SANITIZE __attribute__((no_sanitize("address", "thread", "undefined")))

static NO_SANITIZE void resolve(void)
{
    if (real_dlsym) return;
    // dlvsym is a different symbol, so this never re-enters our dlsym. The versions: glibc 2.34 moved dlsym into
    // libc (MPC references that one); older binaries bind the libdl versions.
    // The names are assembled here so the .so carries no "GLIBC_2.34" text: release tools that scan a library for
    // the glibc it needs would read it as a requirement.
    static const char *const vers[] = { "2.34", "2.4", "2.2.5", "2.0" };
    for (size_t i = 0; i < sizeof vers / sizeof *vers && !real_dlsym; i++) {
        char v[16] = "GLIBC_";
        strcat(v, vers[i]);
        real_dlsym = (dlsym_fn)dlvsym(RTLD_NEXT, "dlsym", v);
    }
    if (!real_dlsym) LOG("can't find libc's dlsym: plugins stay unhooked\n");
}

static long long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

// The slot of a live instance (audio-thread safe: atomics only), or NULL.
static struct inst *slot_of(AEffect *fx)
{
    if (!fx) return NULL;
    for (int k = 0; k < MAX_INST; k++)
        if (atomic_load_explicit(&insts[k].fx, memory_order_acquire) == fx) return &insts[k];
    return NULL;
}

// ---- the patched AEffect entry points ----
static void hooked_setParameter(AEffect *fx, int32_t index, float value)
{
    struct inst *in = slot_of(fx);
    if (!in) return;                   // closed under us: the host should not be here
    atomic_fetch_or(&in->dirty, DIRTY_VALUES);
    inst_force(in, index);
    in->setParameter(fx, index, value);
}

static intptr_t hooked_dispatcher(AEffect *fx, int32_t op, int32_t index, intptr_t value, void *ptr, float opt)
{
    struct inst *in = slot_of(fx);
    if (!in) return 0;
    if (op == effOpen) atomic_store(&in->opened, 1);
    else if (op == effSetSampleRate) atomic_store(&in->sample_rate, (int)opt);
    else if (op == effSetChunk) atomic_fetch_or(&in->dirty, DIRTY_VALUES | DIRTY_TEXT);
    else if (op == effClose) {
        pthread_mutex_lock(&registry_lock);
        hook_on_close(in);
        atomic_store_explicit(&in->fx, NULL, memory_order_release);
        atomic_fetch_sub(&instance_count, 1);
        pthread_mutex_unlock(&registry_lock);
        vst_dispatcher d = in->dispatcher;
        return d(fx, op, index, value, ptr, opt);   // the plugin frees itself here: nothing of ours after it
    }
    return in->dispatcher(fx, op, index, value, ptr, opt);
}

// ---- per-module host callback and entry trampoline ----
static intptr_t master_common(int m, AEffect *fx, int32_t op, int32_t index, intptr_t value, void *ptr, float opt)
{
    struct inst *in = slot_of(fx);
    if (in) {
        if (op == audioMasterAutomate) { atomic_fetch_or(&in->dirty, DIRTY_VALUES); inst_force(in, index); }
        else if (op == audioMasterUpdateDisplay) atomic_fetch_or(&in->dirty, DIRTY_VALUES | DIRTY_TEXT);
    }
    audioMasterCallback real = atomic_load_explicit(&modules[m].master, memory_order_acquire);
    return real ? real(fx, op, index, value, ptr, opt) : 0;
}

static void register_instance(int m, AEffect *fx)
{
    if (!fx || fx->magic != VST_MAGIC) return;
    pthread_mutex_lock(&registry_lock);
    struct inst *in = NULL;
    for (int k = 0; k < MAX_INST && !in; k++)
        if (!atomic_load(&insts[k].fx) && !insts[k].params) in = &insts[k];
    if (!in) {
        pthread_mutex_unlock(&registry_lock);
        LOG("%d instances already: %s stays unhooked\n", MAX_INST, modules[m].path);
        return;
    }
    in->id = next_id++;
    in->module = m;
    in->created_ms = now_ms();
    in->announced = 0;
    in->nparams = fx->numParams;
    in->synth = (fx->flags & effFlagsIsSynth) != 0;
    in->uid = (uint32_t)fx->uniqueID;
    in->name[0] = in->vendor[0] = in->product[0] = 0;
    in->params = NULL;
    in->dispatcher = fx->dispatcher;
    in->setParameter = fx->setParameter;
    atomic_store(&in->dirty, 0);
    for (int k = 0; k < 32; k++) atomic_store(&in->force[k], 0);
    atomic_store(&in->opened, 0);
    atomic_store(&in->sample_rate, 0);
    fx->dispatcher = hooked_dispatcher;
    fx->setParameter = hooked_setParameter;
    atomic_store_explicit(&in->fx, fx, memory_order_release);
    atomic_fetch_add(&instance_count, 1);
    pthread_mutex_unlock(&registry_lock);
}

static AEffect *main_common(int m, audioMasterCallback master)
{
    if (atomic_load(&modules[m].master) != master) atomic_store(&modules[m].master, master);   // one host: one callback
    vst_entry entry = modules[m].entry;
    if (!entry) return NULL;
    AEffect *fx = entry(modules[m].hooked_master);
    register_instance(m, fx);
    return fx;
}

#define MODULE_FNS(N) \
    static intptr_t master_##N(AEffect *fx, int32_t op, int32_t i, intptr_t v, void *p, float o) \
    { return master_common(N, fx, op, i, v, p, o); } \
    static AEffect *main_##N(audioMasterCallback master) { return main_common(N, master); }
#define MODULE_ROW(N) { master_##N, main_##N },

MODULE_FNS(0) MODULE_FNS(1) MODULE_FNS(2) MODULE_FNS(3) MODULE_FNS(4) MODULE_FNS(5) MODULE_FNS(6) MODULE_FNS(7)
MODULE_FNS(8) MODULE_FNS(9) MODULE_FNS(10) MODULE_FNS(11) MODULE_FNS(12) MODULE_FNS(13) MODULE_FNS(14) MODULE_FNS(15)
MODULE_FNS(16) MODULE_FNS(17) MODULE_FNS(18) MODULE_FNS(19) MODULE_FNS(20) MODULE_FNS(21) MODULE_FNS(22) MODULE_FNS(23)
MODULE_FNS(24) MODULE_FNS(25) MODULE_FNS(26) MODULE_FNS(27) MODULE_FNS(28) MODULE_FNS(29) MODULE_FNS(30) MODULE_FNS(31)

static const struct { audioMasterCallback master; vst_entry entry; } module_fns[MAX_MODULES] = {
    MODULE_ROW(0) MODULE_ROW(1) MODULE_ROW(2) MODULE_ROW(3) MODULE_ROW(4) MODULE_ROW(5) MODULE_ROW(6) MODULE_ROW(7)
    MODULE_ROW(8) MODULE_ROW(9) MODULE_ROW(10) MODULE_ROW(11) MODULE_ROW(12) MODULE_ROW(13) MODULE_ROW(14) MODULE_ROW(15)
    MODULE_ROW(16) MODULE_ROW(17) MODULE_ROW(18) MODULE_ROW(19) MODULE_ROW(20) MODULE_ROW(21) MODULE_ROW(22) MODULE_ROW(23)
    MODULE_ROW(24) MODULE_ROW(25) MODULE_ROW(26) MODULE_ROW(27) MODULE_ROW(28) MODULE_ROW(29) MODULE_ROW(30) MODULE_ROW(31)
};

// The module slot for this plugin file (made on first sight), with its entry refreshed; -1 when the table is full.
static int module_for(const char *path, vst_entry entry)
{
    pthread_mutex_lock(&registry_lock);
    int free_slot = -1, m = -1;
    for (int k = 0; k < MAX_MODULES && m < 0; k++) {
        if (!modules[k].path[0]) { if (free_slot < 0) free_slot = k; }
        else if (!strcmp(modules[k].path, path)) m = k;
    }
    if (m < 0 && free_slot >= 0) {
        m = free_slot;
        snprintf(modules[m].path, sizeof modules[m].path, "%s", path);
        modules[m].hooked_master = module_fns[m].master;
    }
    if (m >= 0) modules[m].entry = entry;
    pthread_mutex_unlock(&registry_lock);
    return m;
}

// RTLD_NEXT means "the objects after the caller's", and libc would take this library for the caller: another
// preloaded addin that wraps a function it resolves that way (the usb-audio addin does, for ALSA) would get its own
// wrapper back and recurse. libc's answer (relative to this library) is right unless it lies in the caller's own
// object; then the search goes on after the caller. A link map is not a dlsym handle unless the object was opened
// with dlopen, so each one is re-opened by name with RTLD_NOLOAD (no load, just the handle) for the lookup.
static NO_SANITIZE void *lookup_in(struct link_map *map, const char *name)
{
    if (!map->l_name || !map->l_name[0] || strstr(map->l_name, "vdso")) return NULL;
    void *h = dlopen(map->l_name, RTLD_LAZY | RTLD_NOLOAD);
    if (!h) return NULL;
    void *sym = real_dlsym(h, name);
    dlclose(h);
    return sym;
}

static NO_SANITIZE struct link_map *map_of(const void *addr)
{
    Dl_info info;
    struct link_map *map = NULL;
    return addr && dladdr1(addr, &info, (void **)&map, RTLD_DL_LINKMAP) ? map : NULL;
}

static NO_SANITIZE void *dlsym_next(const char *name, void *caller)
{
    void *sym = real_dlsym(RTLD_NEXT, name);
    struct link_map *cm = map_of(caller);
    if (!sym || !cm || map_of(sym) != cm) return sym;
    for (struct link_map *m = cm->l_next; m; m = m->l_next)           // the objects loaded after the caller
        if ((sym = lookup_in(m, name))) return sym;
    struct link_map *me = map_of((const void *)dlsym_next);
    for (struct link_map *m = me ? me->l_next : NULL; m; m = m->l_next)   // the caller came last: anything but it
        if (m != cm && (sym = lookup_in(m, name))) return sym;
    return NULL;
}

// The exported dlsym. The sanitizers' runtimes resolve their interceptors with dlsym(RTLD_NEXT) while they
// initialise, before dlopen may be called, so the test builds export the hook under another name (HOOK_DLSYM) and
// the fake host calls it; the real .so is tested preloaded, unsanitised.
#ifndef HOOK_DLSYM
#define HOOK_DLSYM dlsym
#endif
__attribute__((visibility("default"))) NO_SANITIZE void *HOOK_DLSYM(void *handle, const char *name)
{
    resolve();
    if (!real_dlsym) return NULL;
    void *sym = handle == RTLD_NEXT ? dlsym_next(name, __builtin_return_address(0)) : real_dlsym(handle, name);
    if (!sym || !atomic_load(&enabled) || (strcmp(name, "VSTPluginMain") && strcmp(name, "main")))
        return sym;
    Dl_info info;
    if (!dladdr(sym, &info) || !info.dli_fname || !strstr(info.dli_fname, ".so")) return sym;   // not a plugin
    int m = module_for(info.dli_fname, (vst_entry)sym);
    if (m < 0) {
        LOG("%d plugin files already: %s stays unhooked\n", MAX_MODULES, info.dli_fname);
        return sym;
    }
    return (void *)module_fns[m].entry;
}

void hook_enable(void)
{
    resolve();
    atomic_store(&enabled, 1);
}

struct inst *hook_find(int id)
{
    for (int k = 0; k < MAX_INST; k++)
        if (atomic_load(&insts[k].fx) && insts[k].id == id) return &insts[k];
    return NULL;
}

int hook_set(int id, int index, float value)
{
    if (value < 0) value = 0;
    if (value > 1) value = 1;
    pthread_mutex_lock(&registry_lock);
    struct inst *in = hook_find(id);
    AEffect *fx = in ? atomic_load(&in->fx) : NULL;
    int r = -1;
    if (fx && index >= 0 && index < in->nparams) {
        in->setParameter(fx, index, value);
        atomic_fetch_or(&in->dirty, DIRTY_VALUES);
        inst_force(in, index);
        r = 0;
    }
    pthread_mutex_unlock(&registry_lock);
    return r;
}
