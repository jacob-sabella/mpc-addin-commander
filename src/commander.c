// mpc-addin-commander: the plugins MPC loads, on the desktop. From inside the MPC process (LD_PRELOAD), the addin
// sees every VST2 instance MPC creates (src/hook.c), polls their parameters (src/poll.c) and serves them, with the
// plugins' own skin files, over HTTP and a WebSocket (src/ws.c) for the desktop app. A sequencer port carries MPC's
// transport to the app and the app's MIDI and transport commands into MPC (src/daw.c); a project snapshot comes
// from the most recent project file (src/project.c).
// It starts only in the process whose executable is named MPC. The launch script and anything else that inherits
// LD_PRELOAD load it and do nothing. Everything runs on the addin's own threads, at normal scheduling and a low
// priority, with every signal blocked so MPC's signals still go to MPC's threads.
#define _GNU_SOURCE
#include "addin.h"
#include "daw.h"
#include "hook.h"
#include "poll.h"
#include "version.h"
#include "ws.h"
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

struct conf C;
static atomic_int started;

// ---- where the .so is: the config sits next to it ----
static void addin_dir(char *out, size_t n)
{
    out[0] = 0;
    FILE *f = fopen("/proc/self/maps", "re");
    if (!f) return;
    uintptr_t me = (uintptr_t)&addin_dir;
    char line[512];
    while (fgets(line, sizeof line, f)) {
        unsigned long a, b;
        char *path = strchr(line, '/');
        if (!path || sscanf(line, "%lx-%lx", &a, &b) != 2 || me < a || me >= b) continue;
        path[strcspn(path, "\n")] = 0;
        char *slash = strrchr(path, '/');
        if (slash) *slash = 0;
        snprintf(out, n, "%s", path);
        break;
    }
    fclose(f);
}

static int is_mpc_process(void)
{
    char exe[256];
    ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
    if (n <= 0) return 0;
    exe[n] = 0;
    const char *base = strrchr(exe, '/');
    return !strcmp(base ? base + 1 : exe, "MPC");
}

long long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

void lower_priority(void)
{
    setpriority(PRIO_PROCESS, (id_t)syscall(SYS_gettid), C.nice);   // nice is per thread on Linux
}

int spawn(void *(*fn)(void *), void *arg)
{
    pthread_attr_t a;
    pthread_attr_init(&a);
    pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
    pthread_attr_setinheritsched(&a, PTHREAD_EXPLICIT_SCHED);
    pthread_attr_setschedpolicy(&a, SCHED_OTHER);
    struct sched_param sp = { .sched_priority = 0 };
    pthread_attr_setschedparam(&a, &sp);
    pthread_attr_setstacksize(&a, 256 * 1024);
    pthread_t t;
    int r = pthread_create(&t, &a, fn, arg);
    pthread_attr_destroy(&a);
    return r;
}

// Start the hook, the poll thread and the server (once). The constructor calls it inside MPC; a test harness can
// call it directly. Returns 0 when running or already was, -1 when disabled or it failed.
__attribute__((visibility("default"))) int mpc_commander_addin_start(void)
{
    if (atomic_exchange(&started, 1)) return 0;
    conf_defaults(&C);
    char path[512];
    const char *env = getenv("MPC_COMMANDER_ADDIN_CONF");
    if (env) snprintf(path, sizeof path, "%s", env);
    else {
        char dir[400];
        addin_dir(dir, sizeof dir);
        snprintf(path, sizeof path, "%s/mpc_commander_addin.conf", dir[0] ? dir : ".");
    }
    int bad = conf_load(&C, path);
    if (bad) LOG("%d line(s) of %s not understood, ignored\n", bad, path);
    if (!C.enabled) { LOG("disabled in %s\n", path); return -1; }
    sigset_t all, old;
    sigfillset(&all);
    pthread_sigmask(SIG_BLOCK, &all, &old);   // the new threads (and every thread they create) block all signals
    int r = spawn(server_thread, NULL);
    if (!r) r = spawn(poll_thread, NULL);
    if (!r) r = spawn(daw_thread, NULL);
    pthread_sigmask(SIG_SETMASK, &old, NULL);
    if (r) { LOG("can't start: %s\n", strerror(r)); return -1; }
    hook_enable();
    return 0;
}

__attribute__((constructor)) static void addin_init(void)
{
    if (getenv("MPC_COMMANDER_ADDIN_DISABLE") || !is_mpc_process()) return;
    mpc_commander_addin_start();
}
