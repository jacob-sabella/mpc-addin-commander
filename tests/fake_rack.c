// Stands in for mpc-addin-sample-rack in the host tests: the same exported symbols, canned answers. The host is linked
// with -rdynamic so that dlsym(RTLD_DEFAULT, ...) finds them, as it finds the real addin's in MPC's process.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

__attribute__((visibility("default"))) int mpc_sample_rack_api(void) { return 2; }

static int tuner_off;   // Tuner, at insert 3 of the input, is running: live on/off

__attribute__((visibility("default"))) char *mpc_sample_rack_state(void)
{
    char b[1024];
    snprintf(b, sizeof b, "{\"ok\":true,\"project\":\"Fake Project\",\"applying\":false,\"max_inserts\":4,\"slots\":[{\"id\":\"input\",\"label\":\"INPUT\","
             "\"fx\":[{\"index\":0,\"enabled\":true,\"vst\":false,\"name\":\"LP Filter\",\"live\":null},"
             "{\"index\":1,\"enabled\":false,\"vst\":true,\"name\":\"NAM\",\"live\":null},"
             "{\"index\":2,\"enabled\":true,\"vst\":true,\"name\":\"Tuner\",\"live\":\"%s\"}]}],"
             "\"plugins\":[{\"name\":\"NAM\",\"vendor\":\"j\"},{\"name\":\"Tuner\",\"vendor\":\"j\"}]}", tuner_off ? "off" : "on");
    return strdup(b);
}

__attribute__((visibility("default"))) int mpc_sample_rack_live(const char *slot, int index, const char *name, int on, char *why, size_t wn)
{
    if (strcmp(slot, "input") || index != 2 || strcmp(name, "Tuner")) { snprintf(why, wn, "MPC isn't running %s", name); return -1; }
    tuner_off = !on;
    return 0;
}

// Writes the document it was given to $FAKE_RACK_OUT; refuses one that mentions "refuse".
__attribute__((visibility("default"))) int mpc_sample_rack_apply(const char *json, char *why, size_t wn)
{
    if (strstr(json, "refuse")) { snprintf(why, wn, "refused by the fake rack"); return -1; }
    const char *out = getenv("FAKE_RACK_OUT");
    FILE *f = out ? fopen(out, "w") : NULL;
    if (f) { fputs(json, f); fclose(f); }
    return 0;
}
