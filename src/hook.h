// The plugin boundary: dlsym("VSTPluginMain") is interposed, every instance's AEffect is patched in place
// (dispatcher and setParameter) and kept in a fixed registry that the poll thread and the server read.
#ifndef HOOK_H
#define HOOK_H
#include "vst.h"
#include <pthread.h>
#include <stdatomic.h>

#define MAX_INST 64
#define MAX_MODULES 32
#define PARAM_NAME 64
#define PARAM_TEXT 128

enum { DIRTY_VALUES = 1, DIRTY_TEXT = 2 };

struct param {
    char name[PARAM_NAME];
    char label[PARAM_NAME];
    char text[PARAM_TEXT];
    float value;
};

struct inst {
    AEffect *_Atomic fx;            // NULL: a free slot. Published last at registration, cleared first at close.
    atomic_int dirty;               // DIRTY_* bits from the host's setParameter and the plugin's host calls
    atomic_uint force[32];          // parameters (by index, up to 1024) to report next poll even if unchanged: a
                                    // value set and reverted between two polls (a trigger) is still seen
    atomic_int opened;              // effOpen seen
    atomic_int sample_rate;
    int id;                         // unique for the process, from 1
    int module;                     // modules[] index
    long long created_ms;           // CLOCK_MONOTONIC
    // The rest belongs to the poll thread and the server, under registry_lock.
    int announced;                  // params read, plugin_added sent
    int nparams;
    int synth;
    uint32_t uid;
    char name[64], vendor[64], product[64];
    struct param *params;
    vst_dispatcher dispatcher;      // the plugin's own
    void (*setParameter)(AEffect *, int32_t, float);
};

struct module {
    char path[512];                 // "" : free
    vst_entry entry;                // the plugin's VSTPluginMain, refreshed on every lookup (dlclose and reopen)
    audioMasterCallback _Atomic master; // the host's callback, learned at the first instance (read on audio threads)
    audioMasterCallback hooked_master; // ours for this module, handed to the plugin
};

extern struct inst insts[MAX_INST];
extern struct module modules[MAX_MODULES];
extern pthread_mutex_t registry_lock;
extern atomic_int instance_count;

// Turn the interposition on (before this, lookups pass straight through). Called once by the start function.
void hook_enable(void);
// The instance with this id (caller holds registry_lock), or NULL.
struct inst *hook_find(int id);
// A client's set: setParameter on that instance from the calling thread. 0, or -1 (unknown id or index).
int hook_set(int id, int index, float value);
static inline void inst_force(struct inst *in, int i)
{
    if (i >= 0 && i < 1024) atomic_fetch_or(&in->force[i >> 5], 1u << (i & 31));
}
// Called by the hook when an instance registers or closes (holding registry_lock), implemented by poll.c.
void hook_on_close(struct inst *in);

#endif
