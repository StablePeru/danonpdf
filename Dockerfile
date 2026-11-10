# Build image: the Rust toolchain, plus MinGW to cross-compile the Windows
# .exe. The source is bind-mounted at run time (see compose.yaml).
FROM rust:1.99.1-slim-trixie

# gcc-mingw-w64-x86-64: the linker and C runtime for x86_64-pc-windows-gnu.
#   It is the Rust target's default linker (x86_64-w64-mingw32-gcc), so no
#   extra Cargo configuration is needed.
# file: lets scripts and humans check what kind of binary came out.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends gcc-mingw-w64-x86-64 file \
    && rm -rf /var/lib/apt/lists/*

# Installed as root into the image's RUSTUP_HOME, so every container gets
# them without downloading anything at run time.
RUN rustup target add x86_64-pc-windows-gnu \
    && rustup component add rustfmt clippy

# Run as a non-root user. UID/GID default to 1000 so that files written
# into a bind-mounted checkout on Linux belong to the usual first user.
# compose.yaml passes HOST_UID/HOST_GID here when they are set.
ARG UID=1000
ARG GID=1000
RUN groupadd --gid "${GID}" builder \
    && useradd --uid "${UID}" --gid "${GID}" --create-home builder \
    && mkdir -p /home/builder/.cargo /home/builder/target /workspace \
    && chown builder:builder /home/builder/.cargo /home/builder/target /workspace

# Downloaded crates and build output live outside the checkout, in named
# volumes (compose.yaml): bind mounts on Docker Desktop for Windows are slow,
# and a fresh container would otherwise refetch and rebuild everything.
# The rustup proxies (cargo, rustc...) stay on PATH from /usr/local/cargo/bin.
ENV CARGO_HOME=/home/builder/.cargo \
    CARGO_TARGET_DIR=/home/builder/target

USER builder
WORKDIR /workspace
