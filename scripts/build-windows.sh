#!/usr/bin/env bash
# Builds the two Windows programs from Linux and copies them to dist/,
# with the PDFium library they load when they run:
#   dist/danonpdf.exe      the desktop app (a GUI program: no console window)
#   dist/danonpdf-cli.exe  the command-line tool (a console program)
#   dist/pdfium.dll        PDFium, prebuilt (not compiled here: see Dockerfile)
#   dist/licenses/pdfium/  the licence texts that must travel with that DLL
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

# Where the Windows build of PDFium was unpacked. The build image puts it
# here (see the Dockerfile); the variable is for running the script anywhere
# else. Checked before the build for the same reason as `file`.
pdfium="${DANONPDF_PDFIUM_WINDOWS_DIR:-/opt/pdfium/windows}"
for needed in bin/pdfium.dll LICENSE licenses; do
    if [[ ! -e "$pdfium/$needed" ]]; then
        echo "error: $pdfium/$needed is missing; rebuild the image (docker compose build) or set DANONPDF_PDFIUM_WINDOWS_DIR" >&2
        exit 1
    fi
done

# One cargo call for both packages, so what they share (the core, lopdf) is
# compiled once.
cargo build --release --locked --target "$target" -p danonpdf-desktop -p danonpdf-cli

# Inside Docker, CARGO_TARGET_DIR points to a named volume outside the
# checkout; elsewhere Cargo uses ./target.
release="${CARGO_TARGET_DIR:-target}/$target/release"

# Checks that the file $1 is a 64-bit Windows binary (PE32+) of the kind
# $2, "GUI", "console" or "DLL", and stores what `file` says about it in
# $description. `file` prints a line such as
# "PE32+ executable for MS Windows 5.02 (console), x86-64, 9 sections";
# older versions order the words differently, so the pattern only asks for
# the three parts that every version prints, in the order they share.
#
# The kind matters as much as the format. A Windows program is either a GUI
# program or a console one: built as the wrong kind, the app would open a
# black console next to its window, and the tool would have no stdout for a
# script to read. And a 32-bit or ARM pdfium.dll would sit next to the
# programs looking right, and fail to load only when a page is drawn.
verify() {
    local binary=$1 kind=$2

    if [[ ! -f "$binary" ]]; then
        echo "error: $binary does not exist" >&2
        exit 1
    fi

    description=$(file --brief "$binary")
    if [[ "$description" != PE32+\ executable*\("$kind"\)*x86-64* ]]; then
        echo "error: $binary is not a 64-bit Windows $kind binary: $description" >&2
        exit 1
    fi
}

# Check all three before copying any, so dist/ only ever receives verified
# files and is left untouched when one of them is wrong.
verify "$release/danonpdf.exe" GUI
gui_description=$description
verify "$release/danonpdf-cli.exe" console
cli_description=$description
verify "$pdfium/bin/pdfium.dll" DLL
dll_description=$description

mkdir -p dist
cp "$release/danonpdf.exe" "$release/danonpdf-cli.exe" "$pdfium/bin/pdfium.dll" dist/

# PDFium's licences ask for their text to go with every binary copy, so the
# DLL never reaches dist/ without them. The folder is laid out as in the
# archive it came from (LICENSE points at "the licenses/ directory"), and
# VERSION says which PDFium build the DLL is. It is removed first: a file
# that a newer PDFium no longer ships must not stay from an earlier build.
rm -rf dist/licenses/pdfium
mkdir -p dist/licenses/pdfium
cp -r "$pdfium/LICENSE" "$pdfium/VERSION" "$pdfium/licenses" dist/licenses/pdfium/

echo "Built dist/danonpdf.exe: $gui_description"
echo "Built dist/danonpdf-cli.exe: $cli_description"
echo "Copied dist/pdfium.dll: $dll_description"
echo "Copied dist/licenses/pdfium/: the licence texts of PDFium and of what it bundles"
