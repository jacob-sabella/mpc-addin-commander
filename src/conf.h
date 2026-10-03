// Settings: key=value lines in mpc_commander_addin.conf next to the .so (or $MPC_COMMANDER_ADDIN_CONF).
#ifndef CONF_H
#define CONF_H

struct conf {
    int enabled;       // enabled=0 keeps the addin loaded but idle (plugins still load through the hook, untouched)
    char bind[46];     // listen address (0.0.0.0: every interface)
    int port;
    int max_clients;   // connections at once
    int nice;          // the addin threads' nice value, 0..19
    int poll_ms;       // how often every instance's values are read
    int text_ms;       // how often every instance's display text is re-read (changed values re-read at poll_ms)
    int skins;         // skins=0 turns the /skin/ file routes off
    int midi;          // midi=0 opens no sequencer port: no transport, no MIDI in or out
    int transport;     // what a transport command sends: TRANSPORT_MMC, TRANSPORT_REALTIME or both (bits)
    char surface[256]; // a file the device's own control-surface injector reads (raw MIDI bytes appended); "" = off
    char settings[256];// MPC's settings file, read for the most recent project
    char project[256]; // a project file to snapshot instead of the most recent one; "" = the most recent
};

enum { TRANSPORT_MMC = 1, TRANSPORT_REALTIME = 2 };

void conf_defaults(struct conf *c);
// Apply one line; 0 when it was understood (or blank or a comment), -1 otherwise.
int conf_line(struct conf *c, const char *line);
// Read a file over the defaults already in c; a missing file is not an error. Returns the bad lines' count.
int conf_load(struct conf *c, const char *path);

#endif
