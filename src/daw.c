#include "daw.h"
#include "addin.h"
#include "midi.h"
#include "ws.h"
#include <fcntl.h>
#include <pthread.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

enum { PPQ = 960, CLOCKS_PER_BEAT = 24, TICKS_PER_CLOCK = PPQ / CLOCKS_PER_BEAT, BEATS_PER_BAR = 4, TEMPO_WINDOW = 48 };

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static struct {
    int playing, recording;
    double tempo;        // 0 = unknown (no clock seen yet)
    long clocks;         // MIDI clocks since the start of the song (song position)
    const char *source;  // what last changed the state: "clock", "mmc", or "" before anything arrived
} T = { .source = "" };
static long long clock_at[TEMPO_WINDOW];   // when the last clocks arrived (ms), a ring
static int clock_n;

void daw_transport_json(struct sb *b)
{
    pthread_mutex_lock(&lock);
    long beats = T.clocks / CLOCKS_PER_BEAT;
    sb_printf(b, "{\"t\":\"transport\",\"playing\":%s,\"recording\":%s,\"tempo\":", T.playing ? "true" : "false",
              T.recording ? "true" : "false");
    if (T.tempo > 0) sb_printf(b, "%.2f", T.tempo);
    else sb_puts(b, "null");
    sb_printf(b, ",\"bar\":%ld,\"beat\":%ld,\"tick\":%ld,\"source\":", beats / BEATS_PER_BAR + 1, beats % BEATS_PER_BAR + 1,
              (T.clocks % CLOCKS_PER_BEAT) * TICKS_PER_CLOCK);
    sb_jstr(b, T.source);
    sb_puts(b, "}");
    pthread_mutex_unlock(&lock);
}

static void push_transport(void)
{
    struct sb b = { 0 };
    daw_transport_json(&b);
    if (!b.oom) ws_broadcast(b.p, b.n);
    sb_free(&b);
}

static void push_midi_in(const struct midi_msg *m, long long at)
{
    struct sb b = { 0 };
    sb_puts(&b, "{\"t\":\"midi_in\",\"bytes\":[");
    int n = m->len > (int)sizeof m->b ? (int)sizeof m->b : m->len;
    for (int k = 0; k < n; k++) sb_printf(&b, "%s%d", k ? "," : "", m->b[k]);
    sb_printf(&b, "],\"ms\":%lld}", at);
    if (!b.oom) ws_broadcast(b.p, b.n);
    sb_free(&b);
}

// MMC locate: F0 7F <device> 06 44 06 01 <hr> <mn> <sc> <fr> <ff> F7, a time code position. The hour byte's bits 5-6
// are the frame rate (24, 25, 29.97 drop, 30). The position in clocks needs the tempo; -1 while it is unknown.
static long locate_clocks(const struct midi_msg *m)
{
    if (m->len != 13 || m->b[1] != 0x7F || m->b[3] != 0x06 || m->b[4] != 0x44 || m->b[5] != 0x06 || m->b[6] != 0x01) return -1;
    static const double fps[] = { 24, 25, 29.97, 30 };
    double sec = (m->b[7] & 31) * 3600.0 + (m->b[8] & 63) * 60.0 + (m->b[9] & 63) + ((m->b[10] & 31) + (m->b[11] & 127) / 100.0) / fps[(m->b[7] >> 5) & 3];
    if (T.tempo <= 0) return sec == 0 ? 0 : -1;
    return (long)(sec * T.tempo / 60.0 * CLOCKS_PER_BEAT + 0.5);
}

static int is_mmc(const struct midi_msg *m)
{
    // MMC: F0 7F <device> 06 <command> F7
    return m->b[0] == 0xF0 && m->len == 6 && m->b[1] == 0x7F && m->b[3] == 0x06 && m->b[5] == 0xF7;
}

// One message from MPC. Returns 1 when the transport state changed and should be pushed.
static int on_message(const struct midi_msg *m, long long at)
{
    uint8_t st = m->b[0];
    if (st == 0xF0 && m->len == 13) {
        pthread_mutex_lock(&lock);
        long c = locate_clocks(m);
        if (c >= 0) { T.clocks = c; T.source = "mmc"; }
        pthread_mutex_unlock(&lock);
        if (c >= 0) return 1;
    }
    if (!(st == 0xF8 || st == 0xFA || st == 0xFB || st == 0xFC || st == 0xF2 || is_mmc(m))) {
        push_midi_in(m, at);   // notes, controllers, other sysex: the app's to interpret
        return 0;
    }
    int push = 0;
    pthread_mutex_lock(&lock);
    switch (st) {
    case 0xF8:
        // MPC sends clock whenever sync output is on for the port, stopped or not: the tempo always comes from it,
        // the position only moves while playing.
        if (clock_n && at - clock_at[(clock_n - 1) % TEMPO_WINDOW] > 250) clock_n = 0;   // a pause isn't a slow tempo
        clock_at[clock_n++ % TEMPO_WINDOW] = at;
        if (clock_n >= TEMPO_WINDOW) {
            long long span = at - clock_at[clock_n % TEMPO_WINDOW];   // the oldest one still in the ring
            double bpm = span > 0 ? 60000.0 * (TEMPO_WINDOW - 1) / CLOCKS_PER_BEAT / (double)span : 0;
            if (bpm > 20 && bpm < 999 && (T.tempo <= 0 || bpm > T.tempo + 0.5 || bpm < T.tempo - 0.5)) {
                T.tempo = bpm;
                push = 1;
            }
        }
        if (T.playing && ++T.clocks % CLOCKS_PER_BEAT == 0) push = 1;
        break;
    case 0xFA: T.playing = 1; T.clocks = 0; T.source = "clock"; push = 1; break;
    case 0xFB: T.playing = 1; T.source = "clock"; push = 1; break;
    case 0xFC: T.playing = 0; T.recording = 0; T.source = "clock"; push = 1; break;
    case 0xF2: T.clocks = (long)((m->b[2] & 127) << 7 | (m->b[1] & 127)) * 6; push = 1; break;   // 1/16 notes
    default:
        switch (m->b[4]) {
        case 0x01: T.playing = 0; T.recording = 0; break;   // stop
        case 0x02: case 0x03: T.playing = 1; break;          // play, deferred play
        case 0x06: T.recording = 1; break;                   // record strobe
        case 0x07: T.recording = 0; break;                   // record exit
        case 0x09: T.playing = 0; break;                     // pause
        default: pthread_mutex_unlock(&lock); push_midi_in(m, at); return 0;
        }
        T.source = "mmc";
        push = 1;
    }
    pthread_mutex_unlock(&lock);
    return push;
}

void *daw_thread(void *arg)
{
    (void)arg;
    lower_priority();
    if (!C.midi) return NULL;
    if (midi_open()) LOG("no MIDI port: %s\n", midi_error());
    else LOG("MIDI port: client %d (MPC Commander)\n", midi_client());
    struct midi_msg m[64];
    for (;;) {
        long long t0 = now_ms();
        int k = midi_listen(50, m, 64);
        if (!k && midi_open()) { nanosleep(&(struct timespec){ 1, 0 }, NULL); continue; }   // no port: idle
        for (int i = 0; i < k; i++)
            if (on_message(&m[i], t0 + m[i].ms)) push_transport();   // each change as it happened, not per batch
    }
    return NULL;
}

int daw_midi_client(const char **err)
{
    if (!C.midi) { *err = "midi=0 in the settings"; return -1; }
    if (midi_open()) { *err = midi_error(); return -1; }
    return midi_client();
}

int daw_midi(const uint8_t *bytes, size_t n, const char **err)
{
    if (!C.midi) { *err = "midi=0 in the settings"; return -1; }
    if (midi_open()) { *err = midi_error(); return -1; }
    if (midi_send(bytes, n)) { *err = "not a MIDI message the port can send"; return -1; }
    return 0;
}

static int mmc(uint8_t cmd)
{
    uint8_t m[6] = { 0xF0, 0x7F, 0x7F, 0x06, cmd, 0xF7 };
    return midi_send(m, sizeof m);
}

int daw_transport(const char *cmd, const char **err)
{
    if (!C.midi) { *err = "midi=0 in the settings"; return -1; }
    if (!C.transport) { *err = "transport=none in the settings"; return -1; }
    if (midi_open()) { *err = midi_error(); return -1; }
    int r = 0, use_mmc = C.transport & TRANSPORT_MMC, use_rt = C.transport & TRANSPORT_REALTIME;
    if (!strcmp(cmd, "play")) {
        if (use_mmc) r |= mmc(0x02);
        if (use_rt) r |= midi_send((uint8_t[]){ 0xFA }, 1);
    } else if (!strcmp(cmd, "continue")) {
        if (use_mmc) r |= mmc(0x02);
        if (use_rt) r |= midi_send((uint8_t[]){ 0xFB }, 1);
    } else if (!strcmp(cmd, "stop")) {
        if (use_mmc) r |= mmc(0x01);
        if (use_rt) r |= midi_send((uint8_t[]){ 0xFC }, 1);
    } else if (!strcmp(cmd, "record")) {
        if (!use_mmc) { *err = "record needs transport=mmc or both"; return -1; }
        r |= mmc(0x06);   // record strobe, then play: the strobe alone only arms a stopped MPC
        r |= mmc(0x02);
    } else {
        *err = "cmd must be play, stop, continue or record";
        return -1;
    }
    if (r) { *err = "the port couldn't send"; return -1; }
    return 0;
}

int daw_surface(const uint8_t *bytes, size_t n, const char **err)
{
    if (!C.surface[0]) { *err = "surface= is empty in the settings"; return -1; }
    int fd = open(C.surface, O_WRONLY | O_APPEND | O_CLOEXEC);
    if (fd < 0) { *err = "the surface file isn't there"; return -1; }
    ssize_t w = write(fd, bytes, n);
    close(fd);
    if (w != (ssize_t)n) { *err = "the surface file couldn't be written"; return -1; }
    return 0;
}
