// HTTP and WebSocket, one thread per connection. The HTTP side is the remote addin's (short requests, Connection:
// close, 5 s socket timeouts); a request upgrading to WebSocket becomes a session whose reader thread stays until
// the client leaves, and whose socket the poll thread writes through ws_broadcast under a per-client send lock.
#define _GNU_SOURCE
#include "ws.h"
#include "addin.h"
#include "hook.h"
#include "json.h"
#include "poll.h"
#include "daw.h"
#include "project.h"
#include <dirent.h>
#include "version.h"
#include <arpa/inet.h>
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

#define MAX_CLIENTS 32
#define MAX_SUBS 16
#define MAX_FRAME 65536
#define SEND_TIMEOUT_S 3          // a stalled client is dropped rather than stalling the poll thread
#define IDLE_S 60                 // no frame for this long: a ping; twice: gone

static atomic_int connections;

struct client {
    int fd;                        // -1: free
    int alive;
    int subs[MAX_SUBS];
    int nsubs;
    pthread_mutex_t send_lock;
};
static struct client clients[MAX_CLIENTS];
static pthread_mutex_t clients_lock = PTHREAD_MUTEX_INITIALIZER;

// ---- plain sends ----
static int send_all(int fd, const void *buf, size_t n)
{
    const char *p = buf;
    while (n) {
        ssize_t w = send(fd, p, n, MSG_NOSIGNAL);
        if (w < 0 && errno == EINTR) continue;
        if (w <= 0) return -1;
        p += w;
        n -= (size_t)w;
    }
    return 0;
}

static void reply_h(int fd, const char *status, const char *type, const char *extra, const void *body, size_t n)
{
    char h[512];
    int k = snprintf(h, sizeof h,
                     "HTTP/1.1 %s\r\nContent-Type: %s\r\nContent-Length: %zu\r\n%sConnection: close\r\n\r\n",
                     status, type, n, extra);
    if (send_all(fd, h, (size_t)k) == 0 && n) send_all(fd, body, n);
}

static void reply(int fd, const char *status, const char *type, const void *body, size_t n)
{
    reply_h(fd, status, type, "Cache-Control: no-store\r\n", body, n);
}

static void reply_text(int fd, const char *status, const char *text)
{
    reply(fd, status, "text/plain; charset=utf-8", text, strlen(text));
}

// Read the request head (up to the blank line), at most n-1 bytes, with the socket's receive timeout.
static int read_head(int fd, char *buf, size_t n)
{
    size_t got = 0;
    while (got < n - 1) {
        ssize_t r = recv(fd, buf + got, n - 1 - got, 0);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) break;
        got += (size_t)r;
        buf[got] = 0;
        if (strstr(buf, "\r\n\r\n") || strstr(buf, "\n\n")) return 0;
    }
    buf[got] = 0;
    return -1;
}

// A request header's value (name case-insensitive), trimmed, into out; 0 if found.
static int header(const char *req, const char *name, char *out, size_t n)
{
    size_t nl = strlen(name);
    const char *p = strchr(req, '\n');
    while (p && p[1] && p[1] != '\r' && p[1] != '\n') {
        p++;
        const char *eol = strchr(p, '\n');
        if (!eol) eol = p + strlen(p);
        if ((size_t)(eol - p) > nl && !strncasecmp(p, name, nl) && p[nl] == ':') {
            const char *v = p + nl + 1, *e = eol;
            while (v < e && (*v == ' ' || *v == '\t')) v++;
            while (e > v && isspace((unsigned char)e[-1])) e--;
            size_t l = (size_t)(e - v) < n - 1 ? (size_t)(e - v) : n - 1;
            memcpy(out, v, l);
            out[l] = 0;
            return 0;
        }
        p = *eol ? eol : NULL;
    }
    return -1;
}

// ---- SHA-1, for the WebSocket accept key ----
static uint32_t rol(uint32_t x, int n) { return (x << n) | (x >> (32 - n)); }

static void sha1(const uint8_t *msg, size_t len, uint8_t out[20])
{
    uint32_t h[5] = { 0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0 };
    size_t total = ((len + 8) / 64 + 1) * 64;
    uint8_t *m = calloc(total, 1);
    if (!m) { memset(out, 0, 20); return; }
    memcpy(m, msg, len);
    m[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) m[total - 1 - i] = (uint8_t)(bits >> (8 * i));
    for (size_t off = 0; off < total; off += 64) {
        uint32_t w[80];
        for (int i = 0; i < 16; i++)
            w[i] = (uint32_t)m[off + 4 * i] << 24 | (uint32_t)m[off + 4 * i + 1] << 16
                   | (uint32_t)m[off + 4 * i + 2] << 8 | m[off + 4 * i + 3];
        for (int i = 16; i < 80; i++) w[i] = rol(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4];
        for (int i = 0; i < 80; i++) {
            uint32_t f, k;
            if (i < 20) f = (b & c) | (~b & d), k = 0x5A827999;
            else if (i < 40) f = b ^ c ^ d, k = 0x6ED9EBA1;
            else if (i < 60) f = (b & c) | (b & d) | (c & d), k = 0x8F1BBCDC;
            else f = b ^ c ^ d, k = 0xCA62C1D6;
            uint32_t t = rol(a, 5) + f + e + k + w[i];
            e = d, d = c, c = rol(b, 30), b = a, a = t;
        }
        h[0] += a, h[1] += b, h[2] += c, h[3] += d, h[4] += e;
    }
    free(m);
    for (int i = 0; i < 5; i++) {
        out[4 * i] = (uint8_t)(h[i] >> 24);
        out[4 * i + 1] = (uint8_t)(h[i] >> 16);
        out[4 * i + 2] = (uint8_t)(h[i] >> 8);
        out[4 * i + 3] = (uint8_t)h[i];
    }
}

// ---- WebSocket frames ----
// A frame (server to client, unmasked) under the client's send lock; -1 drops the client.
static int frame_send(struct client *c, int opcode, const void *payload, size_t n)
{
    uint8_t h[10];
    size_t hl = 2;
    h[0] = (uint8_t)(0x80 | opcode);
    if (n < 126) h[1] = (uint8_t)n;
    else if (n < 65536) { h[1] = 126; h[2] = (uint8_t)(n >> 8); h[3] = (uint8_t)n; hl = 4; }
    else { h[1] = 127; for (int i = 0; i < 8; i++) h[2 + i] = (uint8_t)((uint64_t)n >> (8 * (7 - i))); hl = 10; }
    pthread_mutex_lock(&c->send_lock);
    int r = -1;
    if (c->alive) {
        r = send_all(c->fd, h, hl);
        if (r == 0 && n) r = send_all(c->fd, payload, n);
        if (r) { c->alive = 0; shutdown(c->fd, SHUT_RDWR); }   // the reader thread's recv returns and it leaves
    }
    pthread_mutex_unlock(&c->send_lock);
    return r;
}

static int recv_all(int fd, void *buf, size_t n)
{
    char *p = buf;
    while (n) {
        ssize_t r = recv(fd, p, n, 0);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) return r == 0 ? 0 : -errno;
        p += r;
        n -= (size_t)r;
    }
    return 1;
}

// One frame from the client into *payload (malloc'd, NUL-terminated), its opcode in *op. 1 ok; 0 the peer left or
// the frame was bad; -1 the receive timed out with nothing read (idle).
static int frame_recv(int fd, int *op, uint8_t **payload, size_t *len)
{
    uint8_t h[2];
    int r = recv_all(fd, h, 2);
    if (r <= 0) return r == -EAGAIN || r == -EWOULDBLOCK ? -1 : 0;
    *op = h[0] & 0x0f;
    int masked = h[1] & 0x80;
    uint64_t n = h[1] & 0x7f;
    if (n == 126) { uint8_t e[2]; if (recv_all(fd, e, 2) <= 0) return 0; n = (uint64_t)e[0] << 8 | e[1]; }
    else if (n == 127) { uint8_t e[8]; if (recv_all(fd, e, 8) <= 0) return 0; n = 0; for (int i = 0; i < 8; i++) n = n << 8 | e[i]; }
    if (!masked || n > MAX_FRAME) return 0;    // a client frame must be masked
    uint8_t mask[4];
    if (recv_all(fd, mask, 4) <= 0) return 0;
    uint8_t *p = malloc((size_t)n + 1);
    if (!p) return 0;
    if (n && recv_all(fd, p, (size_t)n) <= 0) { free(p); return 0; }
    for (uint64_t i = 0; i < n; i++) p[i] ^= mask[i & 3];
    p[n] = 0;
    *payload = p;
    *len = (size_t)n;
    return 1;
}

int ws_subscribed(int id)
{
    int r = 0;
    pthread_mutex_lock(&clients_lock);
    for (int k = 0; k < MAX_CLIENTS && !r; k++)
        if (clients[k].fd >= 0)
            for (int i = 0; i < clients[k].nsubs; i++)
                if (clients[k].subs[i] == id) { r = 1; break; }
    pthread_mutex_unlock(&clients_lock);
    return r;
}

void ws_broadcast(const char *json, size_t n)
{
    pthread_mutex_lock(&clients_lock);
    for (int k = 0; k < MAX_CLIENTS; k++)
        if (clients[k].fd >= 0 && clients[k].alive) frame_send(&clients[k], 1, json, n);
    pthread_mutex_unlock(&clients_lock);
}

// ---- the session ----
static void send_json(struct client *c, struct sb *b)
{
    if (!b->oom) frame_send(c, 1, b->p, b->n);
    sb_free(b);
}

static void send_error(struct client *c, const char *msg)
{
    struct sb b = { 0 };
    sb_puts(&b, "{\"t\":\"error\",\"msg\":");
    sb_jstr(&b, msg);
    sb_puts(&b, "}");
    send_json(c, &b);
}

// A "bytes" array of 0..255 into out. The count, or -1.
static int bytes_of(const char *s, const struct jtok *t, int arr, uint8_t *out, int max)
{
    if (arr < 0 || t[arr].type != J_ARR || t[arr].size < 1 || t[arr].size > max) return -1;
    int k = arr + 1;
    for (int m = 0; m < t[arr].size; m++, k = t[k].skip) {
        double v;
        if (json_num(s, t, k, &v) || v < 0 || v > 255 || v != (int)v) return -1;
        out[m] = (uint8_t)v;
    }
    return t[arr].size;
}

static void handle_message(struct client *c, const char *s, size_t n)
{
    struct jtok t[64];
    int nt = json_parse(s, n, t, 64);
    if (nt < 1 || t[0].type != J_OBJ) { send_error(c, "not a JSON object"); return; }
    char type[32];
    int ti = json_get(s, t, 0, "t");
    if (ti < 0 || json_str(s, t, ti, type, sizeof type)) { send_error(c, "no t"); return; }
    if (!strcmp(type, "ping")) {
        struct sb b = { 0 };
        sb_puts(&b, "{\"t\":\"pong\"}");
        send_json(c, &b);
    } else if (!strcmp(type, "list")) {
        struct sb b = { 0 };
        plugins_json(&b);
        send_json(c, &b);
    } else if (!strcmp(type, "set")) {
        double id, i, v;
        int ki = json_get(s, t, 0, "id"), kk = json_get(s, t, 0, "i"), kv = json_get(s, t, 0, "value");
        if (ki < 0 || kk < 0 || kv < 0 || json_num(s, t, ki, &id) || json_num(s, t, kk, &i) || json_num(s, t, kv, &v)
            || id < 1 || id > INT_MAX || i < 0 || i > INT_MAX) { send_error(c, "set needs id, i and value"); return; }
        if (hook_set((int)id, (int)i, (float)v)) send_error(c, "no such instance or parameter");
    } else if (!strcmp(type, "subscribe")) {
        int ka = json_get(s, t, 0, "ids");
        if (ka < 0 || t[ka].type != J_ARR) { send_error(c, "subscribe needs ids"); return; }
        int subs[MAX_SUBS], nsubs = 0;
        for (int k = ka + 1, m = 0; m < t[ka].size && nsubs < MAX_SUBS; m++, k = t[k].skip) {
            double id;
            if (json_num(s, t, k, &id) == 0 && id >= 1 && id <= INT_MAX) subs[nsubs++] = (int)id;
        }
        pthread_mutex_lock(&clients_lock);
        memcpy(c->subs, subs, sizeof subs);
        c->nsubs = nsubs;
        pthread_mutex_unlock(&clients_lock);
    } else if (!strcmp(type, "transport")) {
        char cmd[16];
        int kc = json_get(s, t, 0, "cmd");
        const char *err;
        if (kc < 0 || json_str(s, t, kc, cmd, sizeof cmd)) send_error(c, "transport needs cmd");
        else if (daw_transport(cmd, &err)) send_error(c, err);
    } else if (!strcmp(type, "midi") || !strcmp(type, "surface")) {
        uint8_t bytes[256];
        int nb = bytes_of(s, t, json_get(s, t, 0, "bytes"), bytes, (int)sizeof bytes);
        const char *err;
        if (nb < 0) send_error(c, "bytes must be 1..256 integers 0..255");
        else if ((type[0] == 'm' ? daw_midi : daw_surface)(bytes, (size_t)nb, &err)) send_error(c, err);
    } else if (!strcmp(type, "project")) {
        struct sb b = { 0 };
        project_json(&b);
        send_json(c, &b);
    } else {
        send_error(c, "unknown message type");
    }
}

static void hello_json(struct sb *b)
{
    char model[128] = "", mpc[64] = "";
    FILE *f = fopen("/proc/device-tree/model", "re");
    if (f) { size_t n = fread(model, 1, sizeof model - 1, f); model[n] = 0; fclose(f); }
    f = fopen("/tmp/com.akaipro.mpc.version", "re");
    if (f) { if (!fgets(mpc, sizeof mpc, f)) mpc[0] = 0; fclose(f); }
    model[strcspn(model, "\n")] = mpc[strcspn(mpc, "\r\n")] = 0;
    sb_printf(b, "{\"t\":\"hello\",\"protocol\":%d,\"addin\":\"%s\",\"device\":{\"model\":", COMMANDER_PROTOCOL, ADDIN_VERSION);
    sb_jstr(b, model);
    sb_puts(b, ",\"mpc\":");
    sb_jstr(b, mpc);
    const char *err = "";
    int client = daw_midi_client(&err);
    sb_printf(b, "},\"poll_ms\":%d,\"text_ms\":%d,\"midi\":", C.poll_ms, C.text_ms);
    if (client >= 0) sb_printf(b, "{\"client\":%d}", client);
    else { sb_puts(b, "{\"error\":"); sb_jstr(b, err); sb_puts(b, "}"); }
    sb_puts(b, "}");
}

static void session(int fd, const char *req)
{
    char key[128];
    if (header(req, "Sec-WebSocket-Key", key, sizeof key)) { reply_text(fd, "400 Bad Request", "no Sec-WebSocket-Key"); return; }
    pthread_mutex_lock(&clients_lock);
    struct client *c = NULL;
    for (int k = 0; k < MAX_CLIENTS && !c; k++) if (clients[k].fd < 0) c = &clients[k];
    if (c) { c->fd = fd; c->alive = 1; c->nsubs = 0; }
    pthread_mutex_unlock(&clients_lock);
    if (!c) { reply_text(fd, "503 Service Unavailable", "too many clients"); return; }

    char cat[192];
    int n = snprintf(cat, sizeof cat, "%s258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key);
    uint8_t digest[20];
    sha1((const uint8_t *)cat, (size_t)n, digest);
    struct sb b = { 0 };
    sb_puts(&b, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ");
    sb_base64(&b, digest, 20);
    sb_puts(&b, "\r\n\r\n");
    struct timeval tv = { .tv_sec = IDLE_S };
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    tv.tv_sec = SEND_TIMEOUT_S;
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
    int ok = !b.oom && send_all(fd, b.p, b.n) == 0;
    sb_free(&b);
    if (ok) {
        // the client is in the list before the snapshot, so a plugin_added between the two can only repeat
        struct sb h = { 0 }, p = { 0 }, tr = { 0 };
        hello_json(&h);
        send_json(c, &h);
        plugins_json(&p);
        send_json(c, &p);
        daw_transport_json(&tr);
        send_json(c, &tr);
        int idle = 0;
        while (c->alive) {
            int op;
            uint8_t *payload;
            size_t len;
            int r = frame_recv(fd, &op, &payload, &len);
            if (r < 0) {
                if (idle++ || frame_send(c, 9, "", 0)) break;   // a ping; the pong (any frame) resets idle
                continue;
            }
            if (r == 0) break;
            idle = 0;
            if (op == 1) handle_message(c, (const char *)payload, len);
            else if (op == 9) frame_send(c, 10, payload, len);
            else if (op == 8) { frame_send(c, 8, payload, len < 2 ? 0 : 2); free(payload); break; }
            free(payload);
        }
    }
    pthread_mutex_lock(&clients_lock);
    pthread_mutex_lock(&c->send_lock);
    c->alive = 0;
    c->fd = -1;
    pthread_mutex_unlock(&c->send_lock);
    pthread_mutex_unlock(&clients_lock);
}

// ---- skin files ----
static int has_tui(const char *dir)
{
    char p[1300];
    snprintf(p, sizeof p, "%s/TUI.json", dir);
    struct stat st;
    return stat(p, &st) == 0 && S_ISREG(st.st_mode);
}

// A plugin's Plugin Skins folder: next to its .so, else the one MPC finds by name, "<vendor> - VST - <product>" in the
// same Synths folder as the .so's own folder (a .so loaded from /storage/Synths/NAM/ gets its skin from
// /storage/Synths/jacob-sabella - VST - NAM/). -1 when neither has a TUI.json.
static int skin_dir(const char *so, const char *vendor, const char *product, char *out, size_t n)
{
    const char *slash = strrchr(so, '/');
    if (!slash) return -1;
    snprintf(out, n, "%.*s/Plugin Skins", (int)(slash - so), so);
    if (has_tui(out)) return 0;
    const char *up = slash;
    while (up > so && up[-1] != '/') up--;
    if (up == so || !vendor[0] || !product[0] || strchr(vendor, '/') || strchr(product, '/') ||
        !strcmp(vendor, "..") || !strcmp(product, "..")) return -1;
    snprintf(out, n, "%.*s%s - VST - %s/Plugin Skins", (int)(up - so), so, vendor, product);
    return has_tui(out) ? 0 : -1;
}

int ws_skin_exists(const char *so, const char *vendor, const char *product)
{
    char dir[600];
    return skin_dir(so, vendor, product, dir, sizeof dir) == 0;
}

static int unescape(char *s)
{
    char *w = s;
    for (; *s; s++) {
        if (*s == '%' && isxdigit((unsigned char)s[1]) && isxdigit((unsigned char)s[2])) {
            char h[3] = { s[1], s[2], 0 };
            *w++ = (char)strtol(h, NULL, 16);
            s += 2;
        } else *w++ = *s;
    }
    *w = 0;
    return 0;
}

static const char *content_type(const char *path)
{
    const char *dot = strrchr(path, '.');
    if (!dot) return "application/octet-stream";
    if (!strcasecmp(dot, ".json")) return "application/json";
    if (!strcasecmp(dot, ".png")) return "image/png";
    if (!strcasecmp(dot, ".jpg") || !strcasecmp(dot, ".jpeg")) return "image/jpeg";
    if (!strcasecmp(dot, ".svg")) return "image/svg+xml";
    if (!strcasecmp(dot, ".txt") || !strcasecmp(dot, ".md")) return "text/plain; charset=utf-8";
    return "application/octet-stream";
}

static void send_file(int fd, const char *req, const char *dir, const char *sub);

// GET /skin/<id>/<path>: a file under that plugin's Plugin Skins folder.
static void serve_skin(int fd, const char *req, const char *rest)
{
    if (!C.skins) { reply_text(fd, "404 Not Found", "skins are off (skins=0)"); return; }
    char *end;
    long id = strtol(rest, &end, 10);
    if (end == rest || *end != '/' || id < 1 || id > INT_MAX) { reply_text(fd, "404 Not Found", "not found"); return; }
    char sub[512];
    snprintf(sub, sizeof sub, "%s", end + 1);
    unescape(sub);
    if (!sub[0] || sub[0] == '/' || strstr(sub, "..") || strchr(sub, '\\')) { reply_text(fd, "404 Not Found", "not found"); return; }
    char so[512] = "", vendor[64] = "", product[64] = "";
    pthread_mutex_lock(&registry_lock);
    struct inst *in = hook_find((int)id);
    if (in) {
        snprintf(so, sizeof so, "%s", modules[in->module].path);
        snprintf(vendor, sizeof vendor, "%s", in->vendor);
        snprintf(product, sizeof product, "%s", in->product);
    }
    pthread_mutex_unlock(&registry_lock);
    char dir[600];
    if (!so[0] || skin_dir(so, vendor, product, dir, sizeof dir)) { reply_text(fd, "404 Not Found", "no skin"); return; }
    send_file(fd, req, dir, sub);
}

// sub (already unescaped and checked for "..") under dir, with an ETag; nothing outside dir, also by symlink.
static void send_file(int fd, const char *req, const char *dir, const char *sub)
{
    char path[1200], real_dir[PATH_MAX], real_path[PATH_MAX];
    if (!realpath(dir, real_dir)) { reply_text(fd, "404 Not Found", "no skin"); return; }
    snprintf(path, sizeof path, "%s/%s", dir, sub);
    size_t dl = strlen(real_dir);
    if (!realpath(path, real_path) || strncmp(real_path, real_dir, dl) || real_path[dl] != '/') {   // no escape by symlink
        reply_text(fd, "404 Not Found", "not found");
        return;
    }
    int f = open(real_path, O_RDONLY | O_CLOEXEC);
    struct stat st;
    if (f < 0 || fstat(f, &st) || !S_ISREG(st.st_mode) || st.st_size > 32 * 1024 * 1024) {
        if (f >= 0) close(f);
        reply_text(fd, "404 Not Found", "not found");
        return;
    }
    char etag[64], inm[64], extra[128];
    snprintf(etag, sizeof etag, "\"%llx-%llx\"", (unsigned long long)st.st_size, (unsigned long long)st.st_mtime);
    snprintf(extra, sizeof extra, "ETag: %s\r\nCache-Control: no-cache\r\n", etag);
    if (header(req, "If-None-Match", inm, sizeof inm) == 0 && !strcmp(inm, etag)) {
        close(f);
        reply_h(fd, "304 Not Modified", "text/plain", extra, "", 0);
        return;
    }
    char *data = malloc((size_t)st.st_size + 1);
    size_t got = 0;
    while (data && got < (size_t)st.st_size) {
        ssize_t r = read(f, data + got, (size_t)st.st_size - got);
        if (r <= 0) break;
        got += (size_t)r;
    }
    close(f);
    if (!data || got != (size_t)st.st_size) reply_text(fd, "503 Service Unavailable", "read failed");
    else reply_h(fd, "200 OK", content_type(real_path), extra, data, got);
    free(data);
}

// A stock skin's file from its folder `dir` (found under `root`), or, for a file the skin names but doesn't hold, from
// the shared component folders beside it or in the system content, where MPC finds it too.
static void send_stock(int fd, const char *req, const char *dir, const char *root, const char *sub)
{
    static const char *shared[] = { "AIR Components", "AKAI Components", NULL };
    const char *bases[] = { root, "/usr/share/Akai/Content/Synths", NULL };
    char p[1300], base[700];
    struct stat st;
    snprintf(p, sizeof p, "%s/%s", dir, sub);
    if (stat(p, &st) == 0 || strchr(sub, '/')) { send_file(fd, req, dir, sub); return; }
    for (int b = 0; bases[b]; b++)
        for (int k = 0; shared[k]; k++) {
            snprintf(base, sizeof base, "%s/%s", bases[b], shared[k]);
            snprintf(p, sizeof p, "%s/%s", base, sub);
            if (stat(p, &st) == 0) { send_file(fd, req, base, sub); return; }
        }
    send_file(fd, req, dir, sub);
}

// GET /stock/<folder>/<path>: a file under "<folder>/Plugin Skins" of one of Akai's own plugins, folder being
// "<vendor> - MPC - <name>" in the system content or a Synths folder (the AIR instruments install to /storage/Synths).
// Read from the device at run time, so Akai's skins are never copied into anything we ship.
static void serve_stock(int fd, const char *req, const char *rest)
{
    if (!C.skins) { reply_text(fd, "404 Not Found", "skins are off (skins=0)"); return; }
    char buf[512];
    snprintf(buf, sizeof buf, "%s", rest);
    unescape(buf);
    char *sub = strchr(buf, '/');
    if (!sub) { reply_text(fd, "404 Not Found", "not found"); return; }
    *sub++ = 0;
    if (!buf[0] || !strstr(buf, " - MPC - ") || !strcmp(buf, "..") || strchr(buf, '\\') || !sub[0] || sub[0] == '/' ||
        strstr(sub, "..") || strchr(sub, '\\')) { reply_text(fd, "404 Not Found", "not found"); return; }
    static const char *roots[] = {
#ifdef STOCK_TEST_ROOT
        STOCK_TEST_ROOT,
#endif
        "/usr/share/Akai/Content/Synths", "/storage/Synths", "/sdcard/Synths",
                                   "/media/az01-internal/Synths", NULL };
    char dir[1200];   // a /media name (up to 255) and the folder (up to 511), with room
    for (int r = 0; roots[r]; r++) {
        snprintf(dir, sizeof dir, "%s/%s/Plugin Skins", roots[r], buf);
        if (has_tui(dir)) { send_stock(fd, req, dir, roots[r], sub); return; }
    }
    DIR *media = opendir("/media");   // cards and drives: /media/<name>/Synths
    struct dirent *e;
    while (media && (e = readdir(media))) {
        if (e->d_name[0] == '.') continue;
        snprintf(dir, sizeof dir, "/media/%s/Synths/%s/Plugin Skins", e->d_name, buf);
        if (has_tui(dir)) {
            char root[300];
            snprintf(root, sizeof root, "/media/%s/Synths", e->d_name);
            closedir(media);
            send_stock(fd, req, dir, root, sub);
            return;
        }
    }
    if (media) closedir(media);
    reply_text(fd, "404 Not Found", "no skin");
}

static void serve_info(int fd)
{
    char b[256];
    int n = snprintf(b, sizeof b, "{\"version\":\"%s\",\"protocol\":%d,\"plugins\":%d,\"clients\":%d}", ADDIN_VERSION,
                     COMMANDER_PROTOCOL, atomic_load(&instance_count), atomic_load(&connections) - 1);
    reply(fd, "200 OK", "application/json", b, (size_t)n);
}

static const char PAGE[] =
    "<!doctype html><meta charset=utf-8><title>MPC Commander addin</title>"
    "<body style=\"font-family:sans-serif;max-width:40em;margin:3em auto;color:#ccc;background:#1e1e2e\">"
    "<h1>MPC Commander addin " ADDIN_VERSION "</h1><p>This is the device side. The desktop app connects to the WebSocket at "
    "<code>/ws</code>; <a href=\"/info\">/info</a>, <a href=\"/plugins\">/plugins</a> and <a href=\"/project\">/project</a> are JSON.</p>";

static void handle(int fd)
{
    char req[8192], method[8], target[512];
    if (read_head(fd, req, sizeof req) || sscanf(req, "%7s %511s", method, target) != 2
        || strcspn(req + strlen(method) + 1, " \r\n") > 511) {
        reply_text(fd, "400 Bad Request", "bad request");
        return;
    }
    if (strcmp(method, "GET")) { reply_text(fd, "405 Method Not Allowed", "GET only"); return; }
    char *q = strchr(target, '?');
    if (q) *q = 0;
    char up[64];
    if (!strcmp(target, "/ws")) {
        if (header(req, "Upgrade", up, sizeof up) || strcasecmp(up, "websocket")) reply_text(fd, "426 Upgrade Required", "WebSocket only");
        else session(fd, req);
    } else if (!strcmp(target, "/") || !strcmp(target, "/index.html")) {
        reply(fd, "200 OK", "text/html; charset=utf-8", PAGE, sizeof PAGE - 1);
    } else if (!strcmp(target, "/info")) {
        serve_info(fd);
    } else if (!strcmp(target, "/plugins")) {
        struct sb b = { 0 };
        plugins_json(&b);
        if (b.oom) reply_text(fd, "503 Service Unavailable", "out of memory");
        else reply(fd, "200 OK", "application/json", b.p, b.n);
        sb_free(&b);
    } else if (!strcmp(target, "/project")) {
        struct sb b = { 0 };
        int r = project_json(&b);
        if (b.oom) reply_text(fd, "503 Service Unavailable", "out of memory");
        else reply(fd, r ? "404 Not Found" : "200 OK", "application/json", b.p, b.n);
        sb_free(&b);
    } else if (!strncmp(target, "/skin/", 6)) {
        serve_skin(fd, req, target + 6);
    } else if (!strncmp(target, "/stock/", 7)) {
        serve_stock(fd, req, target + 7);
    } else {
        reply_text(fd, "404 Not Found", "not found");
    }
}

// Close after the reply: unread request bytes would turn the close into a reset that can discard the reply.
static void finish(int fd)
{
    char junk[512];
    shutdown(fd, SHUT_WR);
    while (recv(fd, junk, sizeof junk, MSG_DONTWAIT) > 0) { }
    close(fd);
}

static void *conn_thread(void *arg)
{
    int fd = (int)(intptr_t)arg;
    handle(fd);
    finish(fd);
    atomic_fetch_sub(&connections, 1);
    return NULL;
}

void *server_thread(void *arg)
{
    (void)arg;
    lower_priority();
    for (int k = 0; k < MAX_CLIENTS; k++) { clients[k].fd = -1; pthread_mutex_init(&clients[k].send_lock, NULL); }
    struct sockaddr_in sa = { .sin_family = AF_INET, .sin_port = htons((uint16_t)C.port) };
    if (inet_pton(AF_INET, C.bind, &sa.sin_addr) != 1) { LOG("bad bind address %s\n", C.bind); return NULL; }
    int s = -1, warned = 0;
    for (;;) {                    // the port may still be held (a previous MPC, another tool): keep trying
        s = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
        int one = 1;
        if (s >= 0) setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
        if (s >= 0 && bind(s, (struct sockaddr *)&sa, sizeof sa) == 0 && listen(s, 16) == 0) break;
        if (!warned++) LOG("can't listen on %s:%d (%s); retrying every 10 s\n", C.bind, C.port, strerror(errno));
        if (s >= 0) close(s);
        sleep(10);
    }
    LOG("%s on http://%s:%d/ (WebSocket at /ws)\n", ADDIN_VERSION, C.bind, C.port);
    for (;;) {
        int c = accept4(s, NULL, NULL, SOCK_CLOEXEC);
        if (c < 0) {
            if (errno != EINTR && errno != ECONNABORTED) usleep(100000);
            continue;
        }
        struct timeval tv = { .tv_sec = 5 };
        setsockopt(c, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
        setsockopt(c, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
        if (atomic_fetch_add(&connections, 1) >= C.max_clients) {
            atomic_fetch_sub(&connections, 1);
            reply_text(c, "503 Service Unavailable", "busy");
            finish(c);
            continue;
        }
        if (spawn(conn_thread, (void *)(intptr_t)c)) {
            atomic_fetch_sub(&connections, 1);
            close(c);
        }
    }
    return NULL;
}
