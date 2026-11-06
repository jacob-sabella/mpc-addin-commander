// See rack.h. The rack addin exports mpc_sample_rack_api/_state/_apply, and since its interface 2 _live; they are looked
// up on each call (the addin may be loaded after this one, and a missing one is just "unavailable").
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
typedef int (*live_fn)(const char *, int, const char *, int, char *, size_t);

static const char *why_not(api_fn *api, state_fn *state, apply_fn *apply)
{
    if (!C.rack) return "rack=0 in the settings";
    *api = (api_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_api");
    *state = (state_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_state");
    *apply = (apply_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_apply");
    if (!*api || !*state || !*apply) return "the MPC Sample rack addin isn't loaded";
    int v = (*api)();
    if (v != 1 && v != 2) return "the rack addin's interface is not one this addin knows";
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

int rack_live(const char *slot, int index, const char *name, int on, char *why, size_t wn)
{
    api_fn api; state_fn state; apply_fn apply;
    const char *no = why_not(&api, &state, &apply);
    live_fn live = no ? NULL : (live_fn)dlsym(RTLD_DEFAULT, "mpc_sample_rack_live");
    if (no) { snprintf(why, wn, "%s", no); return -1; }
    if (api() < 2 || !live) { snprintf(why, wn, "the rack addin is too old for live on/off (needs 0.3.0)"); return -1; }
    return live(slot, index, name, on, why, wn);
}
