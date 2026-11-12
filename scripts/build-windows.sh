#!/usr/bin/env bash
# Builds the two Windows programs from Linux and copies them to dist/,
# with the PDFium library that drawing a page needs:
#   dist/danonpdf.exe      the desktop app (a GUI program: no console window)
#   dist/danonpdf-cli.exe  the command-line tool (a console program)
#   dist/pdfium.dll        PDFium, prebuilt (not compiled here: see Dockerfile)
#   dist/licenses/pdfium/  the licence texts that must travel with that DLL
# The Pages view of danonpdf.exe and `danonpdf-cli.exe render` load
# pdfium.dll from the folder they are in, which is why the DLL sits next to
# the programs. Nothing else loads it: the other views and the other
# commands work without it.
# Meant to run inside the build container:
#   docker compose run --rm build bash scripts/build-windows.sh
#
# The script runs in four phases, and their order is the point of it:
#   Phase 1. Everything that does not come out of the build is checked
#            first, so a missing tool or a wrong PDFium folder stops it in a
#            second and not after a long build.
#   Phase 2. The build.
#   Phase 3. The two programs are checked.
#   Phase 4. Only then is dist/ changed, in an order that never leaves
#            pdfium.dll there without its licence texts (steps a to d,
#            further down).
# A run that stops in phases 1 to 3 leaves dist/ exactly as it was. A run
# that stops in phase 4 may leave it incomplete, and says so (see
# `report_incomplete_dist`).
set -euo pipefail

target=x86_64-pc-windows-gnu

# Run from the repository root, wherever the script is called from.
cd "$(dirname "$0")/.."

# `file` checks the binaries below. The build image installs it (see the
# Dockerfile); outside it, say what is missing before a long build instead
# of failing with "command not found" at the end.
if ! command -v file > /dev/null; then
    echo "error: the 'file' command is missing; install it (Debian: apt install file)" >&2
    exit 1
fi

# Where the Windows build of PDFium was unpacked. The build image puts it
# here (see the Dockerfile); the variable is for running the script anywhere
# else. The list is everything that phase 4 copies from that folder: the
# library, and the three things that make up its licence folder. Checked
# before the build for the same reason as `file`.
pdfium="${DANONPDF_PDFIUM_WINDOWS_DIR:-/opt/pdfium/windows}"
for needed in bin/pdfium.dll LICENSE VERSION licenses; do
    if [[ ! -e "$pdfium/$needed" ]]; then
        echo "error: $pdfium/$needed is missing; rebuild the image (docker compose build) or set DANONPDF_PDFIUM_WINDOWS_DIR" >&2
        exit 1
    fi
done

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
#
# $3 is optional: words to put in front of "<file> does not exist", for a
# file whose absence says more than its name does. The two programs pass
# "cargo finished but ": cargo reported success, so a missing .exe means
# that the build wrote somewhere else (a target folder or a package name
# that changed), not that the build failed.
verify() {
    local binary=$1 kind=$2 context=${3:-}

    if [[ ! -f "$binary" ]]; then
        echo "error: ${context}$binary does not exist" >&2
        exit 1
    fi

    description=$(file --brief "$binary")
    if [[ "$description" != PE32+\ executable*\("$kind"\)*x86-64* ]]; then
        echo "error: $binary is not a 64-bit Windows $kind binary: $description" >&2
        exit 1
    fi
}

# The DLL does not come out of the build, so it is checked before it.
verify "$pdfium/bin/pdfium.dll" DLL
dll_description=$description

# One cargo call for both packages, so what they share (the core, lopdf) is
# compiled once.
cargo build --release --locked --target "$target" -p danonpdf-desktop -p danonpdf-cli

# Inside Docker, CARGO_TARGET_DIR points to a named volume outside the
# checkout; elsewhere Cargo uses ./target.
release="${CARGO_TARGET_DIR:-target}/$target/release"

# With the DLL above, all three binaries are checked before any is copied:
# dist/ only ever receives verified files.
verify "$release/danonpdf.exe" GUI "cargo finished but "
gui_description=$description
verify "$release/danonpdf-cli.exe" console "cargo finished but "
cli_description=$description

# Where the three files end up. Each name is written once, here: the
# cleanup below and the copies at the end both read it from these
# variables, so the cleanup can never look for a file that the copies no
# longer make, or miss one that they do.
dist_dll=dist/pdfium.dll
dist_gui=dist/danonpdf.exe
dist_cli=dist/danonpdf-cli.exe

# The temporary name under which a file is copied before it gets the name
# $1. The one place that knows what such a name looks like.
temporary_name() {
    printf '%s.new' "$1"
}

# Copies the file $1 to $2 under a temporary name and renames it into
# place. A copy takes time and a rename does not: if the script is stopped
# halfway, $2 is the old file or the new one, never the first half of one.
install_file() {
    local temporary
    temporary=$(temporary_name "$2")
    cp "$1" "$temporary"
    mv -f "$temporary" "$2"
}

mkdir -p dist/licenses
# Left over from a run that was stopped during the steps below. Only the
# temporary names that this script itself gives out, which are those of
# the three files that `install_file` is called for: dist/ belongs to the
# user too, and a file of theirs that happens to end in ".new" stays.
for destination in "$dist_dll" "$dist_gui" "$dist_cli"; do
    rm -f "$(temporary_name "$destination")"
done
rm -rf dist/licenses/pdfium.new

# Says, when the script ends, that dist/ was left incomplete, if that is
# how it ends. Between the moment the old DLL is removed and the moment the
# last program is in place, dist/ lacks pdfium.dll or holds programs of two
# different builds. A failure there (a full disk, a file that Windows keeps
# locked because the program is running) would otherwise end the script
# with the error of one `cp` or `mv` and no word about what it means for
# the folder.
#
# `trap ... EXIT` runs the function whenever the script ends, however it
# ends: at the last line, at a command that fails (`set -e`), or stopped
# from outside with Ctrl+C. `$?` is still the exit code of the script
# there, and the trap does not change it.
dist_is_incomplete=false
report_incomplete_dist() {
    if [[ "$dist_is_incomplete" == true ]]; then
        echo "error: dist/ is incomplete: the script stopped while replacing its files." >&2
        echo "error: do not use or pass on what is in dist/ now; run this script again." >&2
    fi
}
trap report_incomplete_dist EXIT

# PDFium's licences ask for their text to go with every binary copy, so
# dist/ must never hold pdfium.dll without them, not even if the script is
# stopped at any line from here on:
#
# a. The new licence folder is put together under a temporary name. The
#    DLL and the licences of an earlier build are still in place, whole.
# b. The old DLL is removed, before its licences are touched.
# c. The licence folder is replaced. The old one is removed whole and not
#    copied over: a file that a newer PDFium no longer ships must not stay
#    from an earlier build.
# d. The new DLL arrives last, with its licences already there.
#
# The folder is laid out as in the archive it came from (LICENSE points at
# "the licenses/ directory"), and VERSION says which PDFium build the DLL
# is.
mkdir dist/licenses/pdfium.new
cp -r "$pdfium/LICENSE" "$pdfium/VERSION" "$pdfium/licenses" dist/licenses/pdfium.new/
# Up to here dist/ still holds an earlier build, whole. From the next line
# on it does not, until the last file is in place.
dist_is_incomplete=true
rm -f "$dist_dll"
rm -rf dist/licenses/pdfium
mv dist/licenses/pdfium.new dist/licenses/pdfium
install_file "$pdfium/bin/pdfium.dll" "$dist_dll"

install_file "$release/danonpdf.exe" "$dist_gui"
install_file "$release/danonpdf-cli.exe" "$dist_cli"
dist_is_incomplete=false

echo "Built $dist_gui: $gui_description"
echo "Built $dist_cli: $cli_description"
echo "Copied $dist_dll: $dll_description"
echo "Copied dist/licenses/pdfium/: the licence texts of PDFium and of what it bundles"
