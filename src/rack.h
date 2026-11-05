// The MPC Sample's plugin rack (mpc-addin-sample-rack), reached through the symbols that addin exports into MPC's
// process: no socket, nothing installed beside this addin. Without that addin, or with rack=0, the rack is unavailable.
#ifndef RACK_H
#define RACK_H
#include "json.h"
#include <stddef.h>

// The "rack" message: {"t":"rack","available":true,...} with the channels, their inserts and the plugins MPC knows, or
// {"t":"rack","available":false,"reason":...}. Always appends one JSON message.
void rack_json(struct sb *b);
// Apply edits ({"edits":[...]}, the rack addin's format): MPC is stopped, the project edited and MPC started again.
// 0 when that has started, else -1 with the reason in why.
int rack_apply(const char *json, char *why, size_t wn);

#endif
