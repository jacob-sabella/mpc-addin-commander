#!/usr/bin/env bash
# Offline tests on the build machine (x86): the hook, the poll thread and the server, end to end through a fake host
# and a fake plugin, under ASan+UBSan and under TSan; a preload smoke test of the real .so (it starts in a process
# named MPC and stays out of every other one, and MPC_COMMANDER_ADDIN_DISABLE keeps it off); the exports; and the
# installer against a scratch systemd layout (BUSYBOX=/path/to/busybox runs it in the device's shell).
set -euo pipefail
cd "$(dirname "$0")/.."
B=build/host
mkdir -p "$B"
SRC="src/commander.c src/hook.c src/poll.c src/ws.c src/conf.c src/json.c src/daw.c src/project.c"
VER="-DADDIN_VERSION=\"$(cat VERSION)\""
W="-std=gnu11 -O1 -g -Wall -Wextra -Werror -fno-omit-frame-pointer $VER"

cc -std=gnu11 -O1 -g -Wall -Wextra -Werror -fPIC -shared -o "$B/fake_plugin.so" tests/fake_plugin.c -ldl
for san in address,undefined thread; do
  name=${san%%,*}
  cc $W -fsanitize=$san -DHOOK_DLSYM=commander_dlsym -DHOST_DLSYM=commander_dlsym -DSTOCK_TEST_ROOT="\"$PWD/$B/stock\"" \
    -o "$B/host_$name" tests/fake_host.c tests/fake_midi.c $SRC -ldl -lpthread
  rm -rf "$B/$name"; mkdir -p "$B/$name"; cp "$B/fake_plugin.so" "$B/$name/"
  SK="$B/stock/Test Vendor - MPC - Fake Verb/Plugin Skins"; rm -rf "$B/stock"; mkdir -p "$SK"
  printf '{"pageData":{}}' > "$SK/TUI.json"; printf 'png' > "$SK/knob.png"; printf 'secret' > "$B/stock/secret.txt"
  mkdir -p "$B/stock/AIR Components"; printf 'shared' > "$B/stock/AIR Components/shared.png"
  STOCK_TEST=1 TSAN_OPTIONS="halt_on_error=1" python3 tests/test_ws.py "$B/host_$name" "$B/$name/fake_plugin.so"
  echo "ok   end to end under $name"
done

# the real (non-test) .so, preloaded
cc -std=gnu11 -O2 -Wall -Wextra -Werror -fPIC -shared -fvisibility=hidden $VER -o "$B/mpc_commander_addin.so" $SRC src/midi.c -ldl -lpthread
printf '#include <unistd.h>\nint main(void){for(;;)pause();}\n' > "$B/idle.c"
cc -O2 -o "$B/MPC" "$B/idle.c"
cp "$B/MPC" "$B/other"
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
printf 'port=%s\nbind=127.0.0.1\n' "$PORT" > "$B/smoke.conf"
check_port() { python3 -c "import socket,sys; s=socket.socket(); s.settimeout(1); sys.exit(s.connect_ex(('127.0.0.1',$PORT)))"; }
MPC_COMMANDER_ADDIN_CONF="$B/smoke.conf" LD_PRELOAD="$PWD/$B/mpc_commander_addin.so" "$B/other" & P=$!
sleep 0.5
if check_port; then echo "FAIL the addin started in a process not named MPC"; kill $P; exit 1; fi
kill $P; wait $P 2>/dev/null || true
echo "ok   preload: stays out of other processes"
MPC_COMMANDER_ADDIN_CONF="$B/smoke.conf" LD_PRELOAD="$PWD/$B/mpc_commander_addin.so" "$B/MPC" 2>"$B/smoke.log" & P=$!
for _ in $(seq 50); do check_port && break; sleep 0.1; done
INFO=$(python3 -c "import urllib.request; print(urllib.request.urlopen('http://127.0.0.1:$PORT/info').read().decode())")
kill $P; wait $P 2>/dev/null || true
case "$INFO" in *"\"version\":\"$(cat VERSION)\""*) echo "ok   preload: serves inside MPC: $INFO" ;; *) echo "FAIL preload: $INFO"; cat "$B/smoke.log"; exit 1 ;; esac
MPC_COMMANDER_ADDIN_DISABLE=1 MPC_COMMANDER_ADDIN_CONF="$B/smoke.conf" LD_PRELOAD="$PWD/$B/mpc_commander_addin.so" "$B/MPC" & P=$!
sleep 0.5
if check_port; then echo "FAIL MPC_COMMANDER_ADDIN_DISABLE ignored"; kill $P; exit 1; fi
kill $P; wait $P 2>/dev/null || true
echo "ok   preload: MPC_COMMANDER_ADDIN_DISABLE keeps it off"
# the real .so's exported dlsym, binding the fake host's versioned dlsym reference, in a process named MPC
rm -rf "$B/real"; mkdir -p "$B/real"; cc -O2 -Wall -o "$B/real/MPC" tests/fake_host.c "$PWD/$B/mpc_commander_addin.so" -ldl -lpthread
cp "$B/fake_plugin.so" "$B/real/"
LD_PRELOAD="$PWD/$B/mpc_commander_addin.so" python3 tests/test_ws.py "$B/real/MPC" "$B/real/fake_plugin.so"
echo "ok   preload: the exported dlsym hooks the plugin inside MPC"
if [ -e /dev/snd/seq ] && command -v aconnect >/dev/null; then   # the real sequencer port, where this machine has one
  MPC_COMMANDER_ADDIN_CONF="$B/smoke.conf" LD_PRELOAD="$PWD/$B/mpc_commander_addin.so" "$B/MPC" 2>"$B/smoke.log" & P=$!
  for _ in $(seq 50); do check_port && break; sleep 0.1; done
  sleep 0.3
  PORTS=$(aconnect -l | grep -A2 "'MPC Commander'" | tr '\n' ' ')
  kill $P; wait $P 2>/dev/null || true
  case "$PORTS" in *"'Out "*"'In "*) echo "ok   the sequencer client MPC Commander with ports Out and In" ;; *) echo "FAIL sequencer: $PORTS"; cat "$B/smoke.log"; exit 1 ;; esac
else
  echo "skip the real sequencer port (no /dev/snd/seq or aconnect here)"
fi
EXPORTS=$(nm -D --defined-only "$B/mpc_commander_addin.so" | awk '$2 == "T" {print $3}' | sort | tr '\n' ' ')
[ "$EXPORTS" = "dlsym mpc_commander_addin_start " ] || { echo "FAIL exports: $EXPORTS"; exit 1; }
echo "ok   exports only dlsym and mpc_commander_addin_start"
tests/test_install.sh
echo "all tests passed"
