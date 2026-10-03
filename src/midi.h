// MIDI to and from MPC through the ALSA sequencer: a client named "MPC Commander" with a port "Out" (the addin plays
// into MPC; MPC sees it as a new MIDI input and connects it without a restart) and a port "In" (what MPC sends to it
// once the user enables the port as a MIDI output: clock, start/stop, song position and MMC when sync is on).
// libasound is loaded at run time (MPC already has it), and opened only when first used.
#ifndef MIDI_H
#define MIDI_H
#include <stddef.h>
#include <stdint.h>

// Open the client and ports if they aren't yet. 0 on success, -1 (midi_error() says why).
int midi_open(void);
const char *midi_error(void);
// The client's number, or -1.
int midi_client(void);
// Send one MIDI message: a channel message (2 or 3 bytes), a song position (F2 lsb msb), a system real-time byte
// (F8, FA, FB, FC) or a whole system-exclusive message (F0 .. F7). 0 on success.
int midi_send(const uint8_t *msg, size_t n);

struct midi_msg {
    int ms;                 // milliseconds since the listen started
    uint8_t b[16];          // the message (a sysex longer than 16 bytes keeps its first 16; len is the whole length)
    int len;
};
// Throw away whatever arrived on In before now.
void midi_drain(void);
// Collect what arrives on In for ms milliseconds, at most max messages. Returns the count.
int midi_listen(int ms, struct midi_msg *out, int max);

#endif
