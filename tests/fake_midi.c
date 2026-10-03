// midi.h over two files instead of the ALSA sequencer, for the sanitizer builds: what the addin sends goes to
// $COMMANDER_FAKE_MIDI/out.bin and what it listens to comes from $COMMANDER_FAKE_MIDI/in.bin, both as a length byte
// followed by the message. The real midi.c is exercised in the preload run (and on the device).
#include "../src/midi.h"
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static pthread_mutex_t mtx = PTHREAD_MUTEX_INITIALIZER;
static long in_offset;

static const char *dir(void) { return getenv("COMMANDER_FAKE_MIDI"); }

int midi_open(void) { return dir() ? 0 : -1; }
const char *midi_error(void) { return "COMMANDER_FAKE_MIDI isn't set"; }
int midi_client(void) { return dir() ? 99 : -1; }

int midi_send(const uint8_t *m, size_t n)
{
    if (!n || n > 255 || midi_open()) return -1;
    uint8_t st = m[0] & 0xF0;
    size_t want = st == 0xC0 || st == 0xD0 ? 2 : st < 0xF0 ? 3 : m[0] == 0xF0 ? n : m[0] == 0xF2 ? 3 : 1;
    if (n != want || (m[0] == 0xF0 && m[n - 1] != 0xF7) || (m[0] > 0xF2 && m[0] != 0xF8 && m[0] != 0xFA && m[0] != 0xFB && m[0] != 0xFC)) return -1;
    char path[512];
    snprintf(path, sizeof path, "%s/out.bin", dir());
    pthread_mutex_lock(&mtx);
    int fd = open(path, O_WRONLY | O_APPEND | O_CREAT, 0644);
    uint8_t len = (uint8_t)n;
    int ok = fd >= 0 && write(fd, &len, 1) == 1 && write(fd, m, n) == (ssize_t)n;
    if (fd >= 0) close(fd);
    pthread_mutex_unlock(&mtx);
    return ok ? 0 : -1;
}

static long long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

static int take(struct midi_msg *out, int max, int ms)
{
    char path[512];
    snprintf(path, sizeof path, "%s/in.bin", dir());
    FILE *f = fopen(path, "rb");
    if (!f) return 0;
    fseek(f, in_offset, SEEK_SET);
    int k = 0;
    for (;;) {
        int len = fgetc(f);
        if (len == EOF) break;
        uint8_t raw[256];
        if (fread(raw, 1, (size_t)len, f) != (size_t)len) break;   // a partial write: wait for the rest
        in_offset += 1 + len;
        if (!out || k >= max) continue;
        struct midi_msg m = { .ms = ms, .len = len };
        memcpy(m.b, raw, len > (int)sizeof m.b ? sizeof m.b : (size_t)len);
        out[k++] = m;
    }
    fclose(f);
    return k;
}

void midi_drain(void)
{
    pthread_mutex_lock(&mtx);
    take(NULL, 0, 0);
    pthread_mutex_unlock(&mtx);
}

int midi_listen(int ms, struct midi_msg *out, int max)
{
    if (midi_open()) return 0;
    long long t0 = now_ms(), end = t0 + ms;
    int k = 0;
    for (;;) {
        pthread_mutex_lock(&mtx);
        k += take(out + k, max - k, (int)(now_ms() - t0));
        pthread_mutex_unlock(&mtx);
        if (now_ms() >= end) break;
        nanosleep(&(struct timespec){ 0, 2000000 }, NULL);
    }
    return k;
}
