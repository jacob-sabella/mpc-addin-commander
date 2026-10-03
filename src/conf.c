#define _GNU_SOURCE
#include "conf.h"
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void conf_defaults(struct conf *c)
{
    memset(c, 0, sizeof *c);
    c->enabled = 1;
    snprintf(c->bind, sizeof c->bind, "0.0.0.0");
    c->port = 6730;
    c->max_clients = 6;
    c->nice = 10;
    c->poll_ms = 100;
    c->text_ms = 1000;
    c->skins = 1;
    c->midi = 1;
    c->transport = TRANSPORT_MMC | TRANSPORT_REALTIME;
    snprintf(c->surface, sizeof c->surface, "/data/midi.in");
    snprintf(c->settings, sizeof c->settings, "/media/az01-internal/Settings/MPC/MPC.settings");
}

static int to_str(const char *v, char *out, size_t n)
{
    if (!strcmp(v, "\"\"")) v = "";   // "" clears a path
    size_t len = strlen(v);
    if (len >= n) return -1;
    memcpy(out, v, len + 1);
    return 0;
}

static int to_int(const char *v, int lo, int hi, int *out)
{
    char *end;
    long n = strtol(v, &end, 10);
    if (end == v || *end || n < lo || n > hi) return -1;
    *out = (int)n;
    return 0;
}

int conf_line(struct conf *c, const char *line)
{
    char k[32], v[256];
    while (isspace((unsigned char)*line)) line++;
    if (!*line || *line == '#') return 0;
    if (sscanf(line, " %31[a-z_] = %255s", k, v) != 2) return -1;
    if (!strcmp(k, "enabled")) return to_int(v, 0, 1, &c->enabled);
    if (!strcmp(k, "port")) return to_int(v, 1, 65535, &c->port);
    if (!strcmp(k, "max_clients")) return to_int(v, 1, 32, &c->max_clients);
    if (!strcmp(k, "nice")) return to_int(v, 0, 19, &c->nice);
    if (!strcmp(k, "poll_ms")) return to_int(v, 20, 5000, &c->poll_ms);
    if (!strcmp(k, "text_ms")) return to_int(v, 100, 60000, &c->text_ms);
    if (!strcmp(k, "skins")) return to_int(v, 0, 1, &c->skins);
    if (!strcmp(k, "midi")) return to_int(v, 0, 1, &c->midi);
    if (!strcmp(k, "transport")) {
        if (!strcmp(v, "mmc")) c->transport = TRANSPORT_MMC;
        else if (!strcmp(v, "realtime")) c->transport = TRANSPORT_REALTIME;
        else if (!strcmp(v, "both")) c->transport = TRANSPORT_MMC | TRANSPORT_REALTIME;
        else if (!strcmp(v, "none")) c->transport = 0;
        else return -1;
        return 0;
    }
    if (!strcmp(k, "bind")) return to_str(v, c->bind, sizeof c->bind);
    if (!strcmp(k, "surface")) return to_str(v, c->surface, sizeof c->surface);
    if (!strcmp(k, "settings")) return to_str(v, c->settings, sizeof c->settings);
    if (!strcmp(k, "project")) return to_str(v, c->project, sizeof c->project);
    return -1;
}

int conf_load(struct conf *c, const char *path)
{
    FILE *f = fopen(path, "re");
    if (!f) return 0;
    char line[512];
    int bad = 0;
    while (fgets(line, sizeof line, f)) {
        line[strcspn(line, "\r\n")] = 0;
        if (conf_line(c, line)) bad++;
    }
    fclose(f);
    return bad;
}
