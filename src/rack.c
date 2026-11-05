// See rack.h. The rack addin exports mpc_sample_rack_api/_state/_apply; they are looked up on each call (the addin may
// be loaded after this one, and a missing one is just "unavailable").
#define _GNU_SOURCE
#include "rack.h"
#include "conf.h"
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern struct conf C;

typedef int (*api_fn)(void);
typedef char *(*state_fn)(void);
typedef int (*apply_fn)(const char *, char *, size_t);

static const char *why_not(api_fn *api, state_fn *state, apply_fn *apply)
{
    if (!C.rack) return "rack=0 in the settings";
    *api = (api_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_api");
    *state = (state_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_state");
    *apply = (apply_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_apply");
    if (!*api || !*state || !*apply) return "the MPC Sample rack addin isn't loaded";
    if ((*api)() != 1) return "the rack addin's interface is not one this addin knows";
    return NULL;
}

void rack_json(struct sb *b)
{
    api_fn api; state_fn state; apply_fn apply;
    const char *why = why_not(&api, &state, &apply);
    char *doc = why ? NULL : state();
    if (!doc || doc[0] != '{') {
        sb_puts(b, "{\"t\":\"rack\",\"available\":false,\"reason\":");
        sb_jstr(b, why ? why : "the rack could not be read");
        sb_puts(b, "}");
    } else {
        sb_puts(b, "{\"t\":\"rack\",\"available\":true,");
        sb_puts(b, doc + 1);   // the rack addin's document, minus its opening brace
    }
    free(doc);
}

int rack_apply(const char *json, char *why, size_t wn)
{
    api_fn api; state_fn state; apply_fn apply;
    const char *no = why_not(&api, &state, &apply);
    if (no) { snprintf(why, wn, "%s", no); return -1; }
    return apply(json, why, wn);
}
