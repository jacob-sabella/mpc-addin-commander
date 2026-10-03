// The poll thread: reads every instance's values (and display text when a value moved, on the plugin's own
// request, or on the text timer), announces new instances and closed ones, and pushes the differences to the clients.
#ifndef POLL_H
#define POLL_H
#include "hook.h"
#include "json.h"

void *poll_thread(void *arg);
// One plugin object (caller holds registry_lock; the instance must be announced).
void inst_json(struct sb *b, const struct inst *in);
// The whole "plugins" message (takes registry_lock).
void plugins_json(struct sb *b);

#endif
