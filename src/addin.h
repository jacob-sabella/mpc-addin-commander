// Shared between the addin's files: the settings, logging, and thread creation.
#ifndef ADDIN_H
#define ADDIN_H
#include "conf.h"
#include <stdio.h>

extern struct conf C;
#define LOG(...) fprintf(stderr, "mpc-commander-addin: " __VA_ARGS__)

// A detached thread at normal scheduling (never MPC's real-time policy) with a 256 KB stack. 0 on success.
int spawn(void *(*fn)(void *), void *arg);
// The calling thread's nice value from the settings.
void lower_priority(void);
long long now_ms(void);

#endif
