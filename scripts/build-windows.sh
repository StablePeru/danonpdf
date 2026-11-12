#!/usr/bin/env bash
# Builds the two Windows programs from Linux and copies them to dist/:
#   dist/danonpdf.exe      the desktop app (a GUI program: no console window)
#   dist/danonpdf-cli.exe  the command-line tool (a console program)
# Meant to run inside the build container:
#   docker compose run --rm build bash scripts/build-windows.sh
set -euo pipefail

target=x86_64-pc-windows-gnu

# Run from the repository root, wherever the script is called from.
cd "$(dirname "$0")/.."

# `file` checks the results below. The build image installs it (see the
# Dockerfile); outside it, say what is missing before a long build instead
# of failing with "command not found" at the end.
if ! command -v file > /dev/null; then
    echo "error: the 'file' command is missing; install it (Debian: apt install file)" >&2
    exit 1
fi

# One cargo call for both packages, so what they share (the core, lopdf) is
# compiled once.
cargo build --release --locked --target "$target" -p danonpdf-desktop -p danonpdf-cli

# Inside Docker, CARGO_TARGET_DIR points to a named volume outside the
# checkout; elsewhere Cargo uses ./target.
release="${CARGO_TARGET_DIR:-target}/$target/release"

# Checks that $release/$1 is a 64-bit Windows executable (PE32+) of the
# kind $2, "GUI" or "console", and stores what `file` says about it in
# $description. `file` prints a line such as
# "PE32+ executable for MS Windows 5.02 (console), x86-64, 9 sections";
# older versions order the words differently, so the pattern only asks for
# the three parts that every version prints, in the order they share.
#
# The kind matters as much as the format. A Windows program is either a GUI
# program or a console one: built as the wrong kind, the app would open a
# black console next to its window, and the tool would have no stdout for a
# script to read.
verify() {
    local name=$1 kind=$2
    local exe="$release/$name"

    if [[ ! -f "$exe" ]]; then
        echo "error: cargo finished but $exe does not exist" >&2
        exit 1
    fi

    description=$(file --brief "$exe")
    if [[ "$description" != PE32+\ executable*\("$kind"\)*x86-64* ]]; then
        echo "error: $exe is not a 64-bit Windows $kind executable: $description" >&2
        exit 1
    fi
}

# Check both before copying either, so dist/ only ever receives verified
# files and is left untouched when one of them is wrong.
verify danonpdf.exe GUI
gui_description=$description
verify danonpdf-cli.exe console
cli_description=$description

mkdir -p dist
cp "$release/danonpdf.exe" "$release/danonpdf-cli.exe" dist/
echo "Built dist/danonpdf.exe: $gui_description"
echo "Built dist/danonpdf-cli.exe: $cli_description"
