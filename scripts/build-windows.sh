#!/usr/bin/env bash
# Builds the Windows .exe from Linux and copies it to dist/danonpdf.exe.
# Meant to run inside the build container:
#   docker compose run --rm build bash scripts/build-windows.sh
set -euo pipefail

target=x86_64-pc-windows-gnu

# Run from the repository root, wherever the script is called from.
cd "$(dirname "$0")/.."

# `file` checks the result below. The build image installs it (see the
# Dockerfile); outside it, say what is missing before a long build instead
# of failing with "command not found" at the end.
if ! command -v file > /dev/null; then
    echo "error: the 'file' command is missing; install it (Debian: apt install file)" >&2
    exit 1
fi

cargo build --release --locked --target "$target" -p danonpdf-desktop

# Inside Docker, CARGO_TARGET_DIR points to a named volume outside the
# checkout; elsewhere Cargo uses ./target.
exe="${CARGO_TARGET_DIR:-target}/$target/release/danonpdf.exe"

if [[ ! -f "$exe" ]]; then
    echo "error: cargo finished but $exe does not exist" >&2
    exit 1
fi

# Check what was built before copying it, so dist/ only ever receives a
# verified file and is left untouched otherwise. It must be a 64-bit
# Windows executable (PE32+); `file` prints a line such as
# "PE32+ executable (GUI) x86-64, for MS Windows".
description=$(file --brief "$exe")
if [[ "$description" != PE32+\ executable*x86-64* ]]; then
    echo "error: $exe is not a 64-bit Windows executable: $description" >&2
    exit 1
fi

mkdir -p dist
cp "$exe" dist/danonpdf.exe
echo "Built dist/danonpdf.exe: $description"
