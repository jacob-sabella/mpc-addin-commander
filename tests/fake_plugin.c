// A VST2 plugin for the offline tests, shaped like mpc-vst-plugins' wrapper output: six parameters (a knob, an
// option list, a toggle, a momentary trigger, a text readout, an engine-driven value), audioMasterAutomate after a
// trigger fires, audioMasterUpdateDisplay when the readout changes, and a private opcode (effEngineSet) that stands
// in for the engine changing a value on its own (the host's "engine" command).
#include "../src/vst.h"
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define NPARAMS 6
enum { P_CUTOFF, P_MODE, P_SYNC, P_TRIG, P_PATCH, P_LEVEL };
static const char *const names[NPARAMS] = { "Cutoff", "Mode", "Sync", "Trigger", "Patch", "Level" };
static const char *const labels[NPARAMS] = { "Hz", "", "", "", "", "dB" };
static const char *const modes[] = { "Lowpass", "Bandpass", "Highpass" };
static const char *const patches[] = { "Init", "Pad", "Lead", "Bass" };

enum {
    effEngineSet = 0x7001,     // index, opt: the engine moved a value by itself; reported with audioMasterAutomate
    effNextCheck = 0x7002,     // 1 when dlsym(RTLD_NEXT) from this library skips this library (the usb-audio addin's
                               // ALSA wrappers depend on that) and still finds libc
};

__attribute__((visibility("default"))) int fake_marker(void) { return 1; }

struct plug {
    AEffect fx;
    audioMasterCallback master;
    float v[NPARAMS];
    int trig_frames;
    int need_update_display;
    int open;
};

static void set_param(AEffect *fx, int32_t i, float v)
{
    struct plug *p = fx->object;
    if (i < 0 || i >= NPARAMS) return;
    if (v < 0) v = 0;
    if (v > 1) v = 1;
    p->v[i] = v;
    if (i == P_TRIG && v > 0.5f) p->trig_frames = 256;   // reported back to 0 after the hold
    if (i == P_PATCH) p->need_update_display = 1;
}

static float get_param(AEffect *fx, int32_t i)
{
    struct plug *p = fx->object;
    return i >= 0 && i < NPARAMS ? p->v[i] : 0;
}

static void process_replacing(AEffect *fx, float **in, float **out, int32_t n)
{
    struct plug *p = fx->object;
    (void)in;
    for (int c = 0; c < 2; c++) memset(out[c], 0, sizeof(float) * (size_t)n);
    if (p->trig_frames > 0 && (p->trig_frames -= n) <= 0) {
        p->trig_frames = 0;
        p->v[P_TRIG] = 0;
        p->master(fx, audioMasterAutomate, P_TRIG, 0, NULL, 0);
    }
    if (p->need_update_display) {
        p->need_update_display = 0;
        p->master(fx, audioMasterUpdateDisplay, 0, 0, NULL, 0);
    }
}

static intptr_t dispatcher(AEffect *fx, int32_t op, int32_t i, intptr_t value, void *ptr, float opt)
{
    struct plug *p = fx->object;
    (void)value;
    char *s = ptr;
    switch (op) {
    case effOpen: p->open = 1; return 0;
    case effClose: free(p); return 0;
    case effGetEffectName: snprintf(s, 32, "Fake Synth"); return 1;
    case effGetVendorString: snprintf(s, 64, "mpc-addin-commander tests"); return 1;
    case effGetProductString: snprintf(s, 64, "Fake Synth"); return 1;
    case effGetVendorVersion: return 1;
    case effGetVstVersion: return 2400;
    case effGetPlugCategory: return 2;   // kPlugCategSynth
    case effGetParamName: if (i >= 0 && i < NPARAMS) snprintf(s, 32, "%s", names[i]); return 0;
    case effGetParamLabel: if (i >= 0 && i < NPARAMS) snprintf(s, 32, "%s", labels[i]); return 0;
    case effGetParamDisplay:
        if (i < 0 || i >= NPARAMS) return 0;
        switch (i) {
        case P_CUTOFF: snprintf(s, 32, "%.0f", 20 + p->v[i] * 19980); break;
        case P_MODE: snprintf(s, 32, "%s", modes[(int)(p->v[i] * 2 + 0.5f)]); break;
        case P_SYNC: snprintf(s, 32, "%s", p->v[i] > 0.5f ? "On" : "Off"); break;
        case P_TRIG: snprintf(s, 32, "%s", p->v[i] > 0.5f ? "Fire" : "-"); break;
        case P_PATCH: snprintf(s, 32, "%s", patches[(int)(p->v[i] * 3 + 0.5f)]); break;
        default: snprintf(s, 32, "%.1f", -60 + p->v[i] * 66); break;
        }
        return 0;
    case effEngineSet:
        if (i >= 0 && i < NPARAMS) { p->v[i] = opt; p->master(fx, audioMasterAutomate, i, 0, NULL, opt); }
        return 0;
    case effNextCheck:
        return dlsym(RTLD_NEXT, "fake_marker") == NULL && dlsym(RTLD_NEXT, "memset") != NULL;
    default: return 0;
    }
}

__attribute__((visibility("default"))) AEffect *VSTPluginMain(audioMasterCallback master)
{
    if (!master(NULL, audioMasterVersion, 0, 0, NULL, 0)) return NULL;   // like real plugins: the host must answer
    struct plug *p = calloc(1, sizeof *p);
    if (!p) return NULL;
    p->master = master;
    p->v[P_CUTOFF] = 0.5f;
    p->v[P_LEVEL] = 0.9f;
    AEffect *fx = &p->fx;
    fx->magic = VST_MAGIC;
    fx->dispatcher = dispatcher;
    fx->setParameter = set_param;
    fx->getParameter = get_param;
    fx->processReplacing = process_replacing;
    fx->numParams = NPARAMS;
    fx->numPrograms = 1;
    fx->numOutputs = 2;
    fx->flags = effFlagsCanReplacing | effFlagsIsSynth;
    fx->uniqueID = 0x46616b65;   // 'Fake'
    fx->version = 1;
    fx->object = p;
    return fx;
}
