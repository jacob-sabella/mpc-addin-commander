#!/usr/bin/env bash
# The addin for 32-bit ARM MPC OS devices: build/mpc_commander_addin.so, with addin.manifest and the default settings
# beside it. Built against glibc 2.31 so it loads on older MPC OS too. Needs Docker with QEMU for arm32v7.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build
VER=$(cat VERSION)
docker run --rm --platform linux/arm/v7 -u "$(id -u):$(id -g)" -v "$PWD":/b -w /b -e VER="$VER" arm32v7/gcc:11-bullseye sh -c '
  set -e
  F="-std=gnu11 -O2 -Wall -Wextra -Werror -march=armv7-a -mfpu=neon-vfpv4 -mfloat-abi=hard -DADDIN_VERSION=\"$VER\""
  gcc $F -fPIC -shared -fvisibility=hidden -Wl,-z,defs -Wl,--as-needed -o build/mpc_commander_addin.so \
    src/commander.c src/hook.c src/poll.c src/ws.c src/conf.c src/json.c src/daw.c src/project.c src/midi.c -ldl -lpthread
  strip --strip-unneeded build/mpc_commander_addin.so
  max=$(objdump -T build/mpc_commander_addin.so | grep -o "GLIBC_[0-9.]*" | sort -t. -k2 -n -u | tail -n 1)
  echo "highest glibc symbol version: $max"
  case "$max" in GLIBC_2.[0-9]|GLIBC_2.[12][0-9]|GLIBC_2.3[01]) ;; *) echo "too new for older MPC OS: $max" >&2; exit 1 ;; esac
  readelf -d build/mpc_commander_addin.so | grep NEEDED
  echo "exports:"; nm -D --defined-only build/mpc_commander_addin.so | awk "\$2 == \"T\" {print \"  \" \$3}"
'
cp -f mpc_commander_addin.conf addin.manifest build/   # tools/release.sh packages build/ with the installer
ls -l build/mpc_commander_addin.so
