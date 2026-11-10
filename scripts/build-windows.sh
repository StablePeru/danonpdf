#!/usr/bin/env bash
# Builds the Windows .exe from Linux and copies it to dist/danonpdf.exe.
# Meant to run inside the build container:
#   docker compose run --rm build bash scripts/build-windows.sh
set -euo pipefail

target=x86_64-pc-windows-gnu

# Run from the repository root, wherever the script is called from.
cd "$(dirname "$0")/.."

cargo build --release --locked --target "$target" -p danonpdf-desktop

# Inside Docker, CARGO_TARGET_DIR points to a named volume outside the
# checkout; elsewhere Cargo uses ./target.
exe="${CARGO_TARGET_DIR:-target}/$target/release/danonpdf.exe"

mkdir -p dist
cp "$exe" dist/danonpdf.exe
echo "Built dist/danonpdf.exe"
