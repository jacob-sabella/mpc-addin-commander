// The DAW side: MPC's transport followed over the "MPC Commander" sequencer port (clock, start/stop/continue, song
// position and MMC that MPC sends once the port is enabled as a sync output), transport commands sent back (MMC and
// MIDI real-time), MIDI from the app into MPC, and the device's own control-surface injector file.
#ifndef DAW_H
#define DAW_H
#include "json.h"
#include <stddef.h>
#include <stdint.h>

void *daw_thread(void *arg);
// The current transport state as a "transport" message.
void daw_transport_json(struct sb *b);
// play, stop, continue or record. 0 on success, -1 (and why in *err) otherwise.
int daw_transport(const char *cmd, const char **err);
// Send one MIDI message on the port. 0 on success.
int daw_midi(const uint8_t *bytes, size_t n, const char **err);
// Append raw bytes to the control-surface injector file. 0 on success.
int daw_surface(const uint8_t *bytes, size_t n, const char **err);
// The port's sequencer client number, or -1 with the reason in *err.
int daw_midi_client(const char **err);

#endif
