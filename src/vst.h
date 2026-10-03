// The VST2 ABI the addin needs, hand-written (no Steinberg SDK), the same shapes as mpc-vst-plugins' wrapper.
#ifndef VST_H
#define VST_H
#include <stdint.h>

typedef struct AEffect AEffect;
typedef intptr_t (*audioMasterCallback)(AEffect *, int32_t, int32_t, intptr_t, void *, float);
typedef intptr_t (*vst_dispatcher)(AEffect *, int32_t, int32_t, intptr_t, void *, float);
typedef AEffect *(*vst_entry)(audioMasterCallback);

struct AEffect {
    int32_t magic;                              // 'VstP', 0x56737450
    vst_dispatcher dispatcher;
    void (*process)(AEffect *, float **, float **, int32_t);
    void (*setParameter)(AEffect *, int32_t, float);
    float (*getParameter)(AEffect *, int32_t);
    int32_t numPrograms, numParams, numInputs, numOutputs, flags;
    intptr_t resvd1, resvd2;
    int32_t initialDelay, realQualities, offQualities;
    float ioRatio;
    void *object, *user;
    int32_t uniqueID, version;
    void (*processReplacing)(AEffect *, float **, float **, int32_t);
    void (*processDoubleReplacing)(AEffect *, double **, double **, int32_t);
    char future[56];
};

#define VST_MAGIC 0x56737450

enum {
    effOpen = 0, effClose = 1, effGetParamLabel = 6, effGetParamDisplay = 7, effGetParamName = 8,
    effSetSampleRate = 10, effSetBlockSize = 11, effMainsChanged = 12, effGetChunk = 23, effSetChunk = 24,
    effProcessEvents = 25, effCanBeAutomated = 26, effGetPlugCategory = 35, effGetEffectName = 45,
    effGetVendorString = 47, effGetProductString = 48, effGetVendorVersion = 49, effCanDo = 51,
    effGetVstVersion = 58,
};
enum { audioMasterAutomate = 0, audioMasterVersion = 1, audioMasterGetTime = 7, audioMasterUpdateDisplay = 42 };
enum { effFlagsCanReplacing = 1 << 4, effFlagsProgramChunks = 1 << 5, effFlagsIsSynth = 1 << 8 };

#endif
