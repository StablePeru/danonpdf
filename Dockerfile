# Build image: the Rust toolchain, plus MinGW to cross-compile the Windows
# .exe. The source is bind-mounted at run time (see compose.yaml).
FROM rust:1.99.1-slim-trixie

# gcc-mingw-w64-x86-64: the linker and C runtime for x86_64-pc-windows-gnu.
#   It is the Rust target's default linker (x86_64-w64-mingw32-gcc), so no
#   extra Cargo configuration is needed.
# file: lets scripts and humans check what kind of binary came out.
# curl, ca-certificates: download PDFium below, over HTTPS. Nothing else in
#   the image uses curl.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        gcc-mingw-w64-x86-64 file curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# PDFium, the library that draws PDF pages. DanonPDF does not compile it
# (that takes Chromium's build system and a long time): the core loads a
# prebuilt library at the moment a page is drawn, libpdfium.so on Linux and
# pdfium.dll on Windows. What draws pages is `danonpdf-cli render` and the
# tests; the other commands and the desktop app never load the library.
# The binaries come from https://github.com/bblanchon/pdfium-binaries, one
# release per PDFium build. The build number must be the one that the
# pdfium-render crate is told to bind to (its `pdfium_8125` feature in
# Cargo.toml): the two change together.
#
# The release publishes no checksum file, so the SHA-256 of each archive is
# pinned here. A download that does not match stops the build before
# anything is unpacked. The README says how to move to another build
# ("Bumping PDFium").
#
# .dockerignore keeps the build context empty, so the hashes cannot come
# from a file in the repository: they are build arguments with defaults.
ARG PDFIUM_BUILD=8125
ARG PDFIUM_LINUX_X64_SHA256=e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659
ARG PDFIUM_WIN_X64_SHA256=9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b

# /opt/pdfium/linux    lib/libpdfium.so: what the tests load in this image.
# /opt/pdfium/windows  bin/pdfium.dll: what scripts/build-windows.sh ships
#                      next to the .exe files.
# Both keep LICENSE, licenses/ and VERSION from their archive. Only the
# members named are unpacked (the archives also hold C headers and an import
# library that nothing here uses), and tar fails if one of them is missing.
# Everything is owned by root and readable by all: the "builder" user below
# only reads it.
#
# The last lines check what was unpacked, not only what was downloaded: that
# VERSION names the build asked for, and that each library is of the kind
# its folder says (`file` exits 0 whatever it finds, hence the grep).
#
# The download itself is allowed to stumble, within limits:
# - --proto and --proto-redir: HTTPS only, for the address given and for
#   every address a redirect leads to (GitHub answers with a redirect to
#   its file servers). Without the second one, a redirect could step down
#   to plain HTTP.
# - --connect-timeout, --max-time: 30 seconds to connect and 5 minutes for
#   one attempt (the archives are about 4 MB each), so a connection that
#   stalls ends as an error instead of hanging the build for ever.
# - --retry: an attempt that fails is made again, up to 3 more times, 5
#   seconds apart. Each attempt starts the file from scratch.
#   --retry-max-time ends the retrying after 15 minutes, whatever the count
#   says. By itself --retry only covers what curl calls transient (a
#   timeout, a server error of the 5xx kind), so two more flags widen it:
#   - --retry-connrefused: a connection that the server refuses, which is
#     what a server being restarted looks like.
#   - --retry-all-errors: every other failure, such as a connection cut in
#     the middle of the file. It also retries failures that will not pass
#     (a 404 for a release that does not exist): that costs 15 seconds
#     before the same error is reported, and nothing else.
# None of this decides whether a file is trusted: the SHA-256 check below
# is still the only gate.
RUN set -eu; \
    base="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_BUILD}"; \
    cd /tmp; \
    for archive in pdfium-linux-x64.tgz pdfium-win-x64.tgz; do \
        curl --fail --silent --show-error --location \
            --proto '=https' --proto-redir '=https' \
            --connect-timeout 30 --max-time 300 \
            --retry 3 --retry-delay 5 --retry-max-time 900 \
            --retry-connrefused --retry-all-errors \
            --output "$archive" "$base/$archive"; \
    done; \
    printf '%s  %s\n' \
        "${PDFIUM_LINUX_X64_SHA256}" pdfium-linux-x64.tgz \
        "${PDFIUM_WIN_X64_SHA256}" pdfium-win-x64.tgz \
        | sha256sum --check --strict -; \
    mkdir -p /opt/pdfium/linux /opt/pdfium/windows; \
    tar --extract --gzip --no-same-owner --file pdfium-linux-x64.tgz \
        --directory /opt/pdfium/linux LICENSE VERSION licenses lib/libpdfium.so; \
    tar --extract --gzip --no-same-owner --file pdfium-win-x64.tgz \
        --directory /opt/pdfium/windows LICENSE VERSION licenses bin/pdfium.dll; \
    rm pdfium-linux-x64.tgz pdfium-win-x64.tgz; \
    chmod -R a+rX /opt/pdfium; \
    grep --quiet --line-regexp "BUILD=${PDFIUM_BUILD}" /opt/pdfium/linux/VERSION; \
    grep --quiet --line-regexp "BUILD=${PDFIUM_BUILD}" /opt/pdfium/windows/VERSION; \
    file --brief /opt/pdfium/linux/lib/libpdfium.so \
        | grep --quiet 'ELF 64-bit.*shared object.*x86-64'; \
    file --brief /opt/pdfium/windows/bin/pdfium.dll \
        | grep --quiet 'PE32+.*(DLL).*x86-64'

# Installed as root into the image's RUSTUP_HOME, so every container gets
# them without downloading anything at run time.
RUN rustup target add x86_64-pc-windows-gnu \
    && rustup component add rustfmt clippy

# Run as a non-root user. UID/GID default to 1000 so that files written
# into a bind-mounted checkout on Linux belong to the usual first user.
# compose.yaml passes HOST_UID/HOST_GID here when they are set.
#
# The host's IDs may already be taken inside the Debian image (GID 100 is
# "users", for example), and groupadd/useradd refuse duplicates by default:
# - if a group with that GID exists, it is reused instead of creating one;
# - --non-unique lets the user "builder" share its UID with an existing user.
# Only the numbers matter for file ownership, not the names, so chown uses
# the numbers too (a reused group is not called "builder").
ARG UID=1000
ARG GID=1000
RUN if ! getent group "${GID}" > /dev/null; then \
        groupadd --gid "${GID}" builder; \
    fi \
    && useradd --uid "${UID}" --gid "${GID}" --non-unique --create-home builder \
    && mkdir -p /home/builder/.cargo /home/builder/target /workspace \
    && chown "${UID}:${GID}" /home/builder/.cargo /home/builder/target /workspace

# Downloaded crates and build output live outside the checkout, in named
# volumes (compose.yaml): bind mounts on Docker Desktop for Windows are slow,
# and a fresh container would otherwise refetch and rebuild everything.
# The rustup proxies (cargo, rustc...) stay on PATH from /usr/local/cargo/bin.
ENV CARGO_HOME=/home/builder/.cargo \
    CARGO_TARGET_DIR=/home/builder/target

# The folder that holds the PDFium library for this platform. A program
# that draws a page looks for the library next to its own executable, unless
# this variable names another folder: the rule is `default_library_folder`
# in the core, and the README ("PDFium"), AGENTS.md and `danonpdf-cli --help`
# document it. In this image the executable is a test binary, or a program
# started by `cargo run`, deep in the target folder, so both need the
# variable to find the library unpacked above.
ENV DANONPDF_PDFIUM_DIR=/opt/pdfium/linux/lib

USER builder
WORKDIR /workspace
