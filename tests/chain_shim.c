// Two copies of this are preloaded ahead of the addin (-DSHIM_VAL=1 and 2): chain_probe() asks dlsym(RTLD_NEXT) for the
// next chain_probe, as the ALSA wrappers of the other MPC addins do for snd_rawmidi_open. 21 means the addin's dlsym
// let the first shim reach the second.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>

__attribute__((visibility("default"))) int chain_probe(void)
{
    int (*next)(void) = (int (*)(void))dlsym(RTLD_NEXT, "chain_probe");
    return next ? next() * 10 + SHIM_VAL : SHIM_VAL;
}
