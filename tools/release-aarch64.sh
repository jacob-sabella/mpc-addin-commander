#!/usr/bin/env bash
# The Gen2 release zip: the build (run ./build-aarch64.sh first) packaged with mpc-vst-plugins' shared addin
# installer, then checked as the catalog will check it. Needs mpc-vst-plugins next to this repo (or
# MPC_VST=/path), docs/ADDINS.md there. Both this zip and build.sh/release.sh's armv7 one go on the same release.
# Usage: tools/release-aarch64.sh <version>   ->  dist/<Name>-<version>-mpc-aarch64.zip
set -euo pipefail
cd "$(dirname "$0")/.."
[ $# -eq 1 ] || { echo "usage: $0 <version>" >&2; exit 2; }
MPC_VST="${MPC_VST:-../mpc-vst-plugins}"
python3 "$MPC_VST/tools/release_addin.py" --dir build/aarch64 --version "$1" --repo jacob-sabella/mpc-addin-commander --license MIT \
  --about "The plugins MPC loads, with their own skins, and MPC's transport, on a desktop app over the network." -o dist
python3 "$MPC_VST/tools/catalog_check.py" dist/*-"$1"-mpc-aarch64.zip --catalog --expect-id commander --expect-repo jacob-sabella/mpc-addin-commander
