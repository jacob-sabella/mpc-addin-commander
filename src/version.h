// The addin version, in /info and the hello message. build.sh and tests/test.sh pass -DADDIN_VERSION from the VERSION
// file; the fallback is for an ad hoc compile.
#ifndef VERSION_H
#define VERSION_H
#ifndef ADDIN_VERSION
#define ADDIN_VERSION "0.0.0-dev"
#endif
#define COMMANDER_PROTOCOL 1
#endif
