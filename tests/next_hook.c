// Another addin that wraps plugin entries through dlsym, as mpc-addin-sample-rack does: preloaded after Commander, it
// must still see each plugin's entry. Says so on stderr for every instance.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

typedef void *(*entry_fn)(void *);
static entry_fn real_entry;

static void *wrapped(void *master)
{
    void *fx = real_entry(master);
    if (fx) fprintf(stderr, "next_hook: wrapped an instance\n");
    return fx;
}

__attribute__((visibility("default"))) void *dlsym(void *h, const char *name)
{
    static void *(*real)(void *, const char *);
    if (!real) real = (void *(*)(void *, const char *))dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.34");
    if (!real) real = (void *(*)(void *, const char *))dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.2.5");
    void *s = real(h, name);
    if (s && h != RTLD_NEXT && !strcmp(name, "VSTPluginMain")) { real_entry = (entry_fn)s; return (void *)wrapped; }
    return s;
}
