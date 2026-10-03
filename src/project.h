// A snapshot of a project file: the most recent one in MPC's settings (or project= in ours), read and summarised.
#ifndef PROJECT_H
#define PROJECT_H
#include "json.h"

// The "project" message for the file. Returns 0, or -1 with the error message appended as an "error" message instead.
int project_json(struct sb *b);

#endif
