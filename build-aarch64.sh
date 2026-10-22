#!/usr/bin/env bash
# The addin for Gen2 MPC OS devices (aarch64: Live III, One G2, Key(s) G2): build/aarch64/mpc_commander_addin.so,
# with addin.manifest and the default settings beside it. Gen2 only ships MPC OS 3.x (glibc 2.39), so there is no
# older-MPC-OS glibc ceiling to respect, unlike build.sh's Gen1 build. Needs Docker with QEMU for arm64.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build/aarch64
VER=$(cat VERSION)
docker run --rm --platform linux/arm64 -u "$(id -u):$(id -g)" -v "$PWD":/b -w /b -e VER="$VER" arm64v8/gcc:12-bookworm sh -c '
  set -e
  F="-std=gnu11 -O2 -Wall -Wextra -Werror -DADDIN_VERSION=\"$VER\""
  gcc $F -fPIC -shared -fvisibility=hidden -Wl,-z,defs -Wl,--as-needed -o build/aarch64/mpc_commander_addin.so \
    src/commander.c src/hook.c src/poll.c src/ws.c src/conf.c src/json.c src/daw.c src/project.c src/midi.c -ldl -lpthread
  strip --strip-unneeded build/aarch64/mpc_commander_addin.so
  max=$(objdump -T build/aarch64/mpc_commander_addin.so | grep -o "GLIBC_[0-9.]*" | sort -t. -k2 -n -u | tail -n 1)
  echo "highest glibc symbol version: $max"
  case "$max" in GLIBC_2.[0-9]|GLIBC_2.[123][0-9]) ;; *) echo "too new for MPC OS 3.x (glibc 2.39): $max" >&2; exit 1 ;; esac
  readelf -d build/aarch64/mpc_commander_addin.so | grep NEEDED
  echo "exports:"; nm -D --defined-only build/aarch64/mpc_commander_addin.so | awk "\$2 == \"T\" {print \"  \" \$3}"
'
cp -f mpc_commander_addin.conf addin.manifest build/aarch64/   # tools/release-aarch64.sh packages build/aarch64 with the installer
ls -l build/aarch64/mpc_commander_addin.so
