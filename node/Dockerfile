# Multi-stage Dockerfile for Rust F1r3fly Node
# Equivalent to build.sbt Docker configuration

FROM --platform=$BUILDPLATFORM tonistiigi/xx AS xx

# ============================================================================
# Stage 1: Builder - Build node binary (Cargo handles all dependencies)
# ============================================================================
FROM --platform=$BUILDPLATFORM rust:bookworm AS builder

# Declare build arguments at the top to ensure consistent caching
ARG TARGETARCH

# Install build dependencies for Debian/glibc cross-compilation
# Include cross-compiler toolchains for both amd64 and arm64 targets
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libclang-dev \
    clang \
    build-essential \
    protobuf-compiler \
    curl \
    cmake \
    gcc-x86-64-linux-gnu \
    g++-x86-64-linux-gnu \
    gcc-aarch64-linux-gnu \
    g++-aarch64-linux-gnu \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY --from=xx / /

# Install library dependencies for target architectures using xx-apt
# This ensures cross-compilers can find necessary headers and libraries for the target
RUN xx-apt-get update && xx-apt-get install -y \
    libc6-dev \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files first for better caching
COPY Cargo.toml Cargo.lock ./
COPY rust-toolchain.toml ./

# Create directory structure and copy only Cargo.toml files from all workspace members
# This allows us to cache dependencies separately from source code
RUN mkdir -p models crypto rholang rholang/src/main/tree_sitter shared casper comm rspace++ rspace++/libs/rspace_rhotypes block-storage graphz node

# Now copy all source code (this layer invalidates when source changes)
# Copy in dependency order to maximize cache hits - base dependencies first
COPY shared/ ./shared/
COPY crypto/ ./crypto/
COPY models/ ./models/
COPY graphz/ ./graphz/
COPY block-storage/ ./block-storage/
COPY rspace++/ ./rspace++/
COPY casper/ ./casper/
COPY comm/ ./comm/
COPY rho-pure-eval/ ./rho-pure-eval/
COPY rholang/ ./rholang/
COPY node/ ./node/

# Configure pkg-config for cross-compilation to find target OpenSSL libraries
# xx-apt installs target libs in the xx sysroot, we need to tell pkg-config where they are
# Enable AES CPU features for gxhash dependency (required by PathMap crate).
# +sse2 is x86-only; on aarch64 only +aes is valid (avoids warning spam on ARM hosts).
# PROFILING_BUILD=1 keeps frame pointers in the release binary so `perf
# record -g`'s default fp-based unwinder can resolve user-space (Rust) call
# stacks above libc frames instead of stopping at the kernel/libc boundary.
# Off by default: frame pointers cost a register + minor codegen overhead,
# not wanted in the image everyone actually runs.
ARG PROFILING_BUILD=0

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/build \
    export TARGET_TRIPLE=$(xx-cargo --print-target-triple) && \
    export PKG_CONFIG_SYSROOT_DIR=$(xx-info sysroot) && \
    export PKG_CONFIG_PATH=$(xx-info sysroot)/usr/lib/$(xx-info triple)/pkgconfig:$(xx-info sysroot)/usr/share/pkgconfig && \
    export PKG_CONFIG_ALLOW_CROSS=1 && \
    case "${TARGET_TRIPLE}" in \
        aarch64*) export RUSTFLAGS="-C target-feature=+aes" ;; \
        *) export RUSTFLAGS="-C target-feature=+aes,+sse2" ;; \
    esac && \
    if [ "${PROFILING_BUILD}" = "1" ]; then export RUSTFLAGS="${RUSTFLAGS} -C force-frame-pointers=yes"; fi && \
    xx-cargo build --release -p node --target-dir ./build && \
    xx-verify ./build/${TARGET_TRIPLE}/release/node && \
    cp ./build/${TARGET_TRIPLE}/release/node /build/node_binary && \
    chmod +x /build/node_binary

# Fetch PeTTa (MeTTa interpreter, required at runtime for the `rho:petta:execute`
# system service).
#
# PeTTa is pure Prolog and is not needed to compile the node, so cloning after
# the cargo build keeps the build cache intact.
# 
# Bump PETTA_REF to upgrade the bundled interpreter.
ARG PETTA_REPO=https://github.com/trueagi-io/PeTTa.git
ARG PETTA_REF=52e85c6d02d016a4734559a64b2a69e8fdc385fc
RUN git clone "${PETTA_REPO}" /build/PeTTa && \
    git -C /build/PeTTa checkout --detach "${PETTA_REF}" && \
    rm -rf /build/PeTTa/.git

# ============================================================================
# Stage: swipl-extract - Upstream SWI-Prolog with the janus (Python) bridge
# ============================================================================
# PeTTa's `src/metta.pl` requires `library(janus)` and the full JSON libraries.
# Debian bookworm only ships SWI-Prolog 9.0.4, which predates janus and omits
# those libraries, so `rho:petta:execute` fails on the packaged interpreter.
# The official `swipl` image ships a recent build (>= 9.3.x, currently 10.x) with
# janus included; we copy its interpreter and library into the images that need
# to actually execute MeTTa (the runtime image and the petta-test stage).
FROM swipl:stable AS swipl-extract

# ============================================================================
# Stage: sandbox-builder - Build Cloudflare libsandbox.so (seccomp)
# ============================================================================
# Defined before petta-test (and the runtime stages) so both can COPY the built
# libsandbox.so: a COPY --from must reference an earlier stage.
FROM --platform=$BUILDPLATFORM debian:bookworm-slim AS sandbox-builder

RUN apt-get update && apt-get install -y \
    gcc \
    make \
    curl \
    ca-certificates \
    git \
    gperf \
    && rm -rf /var/lib/apt/lists/*

# Clone Cloudflare sandbox (LD_PRELOAD seccomp library)
RUN git clone --depth 1 https://github.com/cloudflare/sandbox /tmp/sandbox

# Build libseccomp as a static library (required by sandbox Makefile)
WORKDIR /tmp/sandbox
RUN curl -L -o libseccomp.tar.gz \
    https://github.com/seccomp/libseccomp/releases/download/v2.5.5/libseccomp-2.5.5.tar.gz && \
    tar xf libseccomp.tar.gz && \
    mv libseccomp-2.5.5 libseccomp

WORKDIR /tmp/sandbox/libseccomp
RUN ./configure --enable-shared=no && make

# Build libsandbox.so
WORKDIR /tmp/sandbox
RUN make && \
    cp libsandbox.so /libsandbox.so

# ============================================================================
# Optional stage: petta-test - Run the de-pythonified MeTTa suites in-container
# ============================================================================
# NOT part of the default build (the runtime stage below stays the default
# target). It reuses the Rust toolchain and the workspace source from the
# builder, and runs the sandboxed-PeTTa test harness against three de-pythonified
# MeTTa suites: PLN (swipl_petta_ecan_spec) and metta-moses + metta-attention
# (swipl_petta_workspace_spec). Build and run it explicitly:
#   docker build --target petta-test -t f1r3fly-petta-test -f node/Dockerfile .
#   docker run --rm --privileged f1r3fly-petta-test
# `--privileged` (or `--security-opt seccomp=unconfined --security-opt
# apparmor=unconfined`) is required because petta.sh runs each test inside a
# bubblewrap user namespace, which the default Docker sandbox blocks.
# Add `-v petta-cargo:/usr/local/cargo/registry -v petta-target:/build/target`
# to the run to cache crates and compiled artifacts across repeated runs.
FROM builder AS petta-test

# The MeTTa libraries are de-pythonified: random/time/math come from PeTTa
# builtins (lib/pyrand.pl), so no py-call is ever invoked. We therefore do NOT
# install the python3 interpreter (the old petta-test did). It is still present
# in the image, but only as a transitive dependency of the rust:bookworm base
# toolchain (clang/llvm), never as something this stage adds or the sandbox
# binds — petta.sh does not bind python3 into the bubblewrap sandbox.
# libpython3.11 is kept only because SWI-Prolog's janus.so is dynamically linked
# against it (library(janus) fails to load without it); it is never exercised.
# bubblewrap + patchelf drive petta.sh's seccomp sandbox; swi-prolog supplies
# the interpreter's shared libraries (the binary itself is overlaid below);
# ncurses-term provides the terminfo tree petta.sh binds into the sandbox.
RUN apt-get update && apt-get install -y --no-install-recommends \
    swi-prolog \
    libpython3.11 \
    bubblewrap \
    patchelf \
    ncurses-term \
    && rm -rf /var/lib/apt/lists/*

# Overlay the packaged 9.0.4 with the upstream janus-capable SWI-Prolog so the
# tests exercise the same interpreter the runtime image ships.
COPY --from=swipl-extract /usr/lib/swipl /usr/lib/swipl
COPY --from=swipl-extract /usr/bin/swipl /usr/bin/swipl

# Cloudflare libsandbox.so (seccomp) and the sandbox driver scripts petta.sh
# invokes at runtime.
COPY --from=sandbox-builder /libsandbox.so /usr/lib/libsandbox.so
COPY petta.sh functions.sh /build/

# De-pythonified PeTTa fork + the three MeTTa suites, pinned to the same revs the
# devshell (f1r3fly-rust-devshell flake.lock) locks. Full clones so the pinned
# commit (on a non-default branch) is reachable. Bump the *_REF args to upgrade.
ARG PETTA_NOPYTHON_REPO=https://github.com/mlabs-haskell/PeTTa.git
ARG PETTA_NOPYTHON_REF=fcc22e8a2e5623870c87b6033ecb3f33d72a7338
ARG PLN_REPO=https://github.com/dylon/PLN.git
ARG PLN_REF=fbb2ae0f43824106102cb7de4085699dedab0db7
ARG METTA_MOSES_REPO=https://github.com/mlabs-haskell/metta-moses.git
ARG METTA_MOSES_REF=47a3c740f8006d66880d578811bd84896a47e3f8
ARG METTA_ATTENTION_REPO=https://github.com/mlabs-haskell/metta-attention.git
ARG METTA_ATTENTION_REF=1b61232cecdb4d71c02969e229a20124a6716a62
RUN git clone "${PETTA_NOPYTHON_REPO}" /build/PeTTa-nopython && \
    git -C /build/PeTTa-nopython checkout --detach "${PETTA_NOPYTHON_REF}" && \
    rm -rf /build/PeTTa-nopython/.git && \
    git clone "${PLN_REPO}" /build/PLN && \
    git -C /build/PLN checkout --detach "${PLN_REF}" && \
    rm -rf /build/PLN/.git && \
    git clone "${METTA_MOSES_REPO}" /build/metta-moses && \
    git -C /build/metta-moses checkout --detach "${METTA_MOSES_REF}" && \
    rm -rf /build/metta-moses/.git && \
    git clone "${METTA_ATTENTION_REPO}" /build/metta-attention && \
    git -C /build/metta-attention checkout --detach "${METTA_ATTENTION_REF}" && \
    rm -rf /build/metta-attention/.git

# Point the harness at the fork + the suite sources. REQUIRE_PETTA_TESTS=1 makes
# a missing prerequisite fail the run loudly instead of silently skipping.
ENV PETTA_PATH=/build/PeTTa-nopython \
    PETTA_DIR=/build/PeTTa-nopython \
    PETTA_SCRIPT_PATH=/build/petta.sh \
    PLN_DIR=/build/PLN \
    METTA_MOSES_DIR=/build/metta-moses \
    METTA_ATTENTION_DIR=/build/metta-attention \
    CACHE_DIR=/build/.petta-cache \
    SANDBOX_LIB_PATH=/usr/lib/libsandbox.so \
    PETTA_BLOCKED_PREDS=readln! \
    REQUIRE_PETTA_TESTS=1 \
    TERMINFO=/usr/share/terminfo \
    TERMINFO_DIRS=/usr/share/terminfo

WORKDIR /build

# Create SWI-Prolog paths petta.sh expects but the Debian package omits (mirror
# the runtime stage), and the writable cache dir petta.sh patches libraries into.
RUN SWI_HOME=$(swipl --home 2>/dev/null || echo "/usr/lib/swi-prolog") && \
    { [ -f "$SWI_HOME/ABI" ] || echo "9.0.4" > "$SWI_HOME/ABI"; } && \
    mkdir -p "$SWI_HOME/app" "$SWI_HOME/doc" /build/.petta-cache

# gxhash (via the PathMap crate) needs AES CPU features to compile, matching the
# release build above. +sse2 is x86-only; on aarch64 only +aes is valid. Runs
# the PLN suite (swipl_petta_ecan_spec) and the metta-moses + metta-attention
# suites (swipl_petta_workspace_spec); exits non-zero if either suite fails.
ENTRYPOINT ["bash", "-c", "case \"$(uname -m)\" in aarch64) export RUSTFLAGS='-C target-feature=+aes' ;; *) export RUSTFLAGS='-C target-feature=+aes,+sse2' ;; esac; rc=0; for spec in swipl_petta_ecan_spec swipl_petta_workspace_spec; do echo \"=== running $spec ===\"; cargo test --test \"$spec\" -p rholang -- --nocapture --test-threads=1 || rc=1; done; exit $rc"]

# ============================================================================
# Stage 4: Runtime base - Install runtime dependencies (cached independently)
# ============================================================================
FROM debian:bookworm-slim AS runtime-base

# Re-declared: ARGs don't cross FROM boundaries. Gates the `perf`/`procps`
# install below the same way it gates frame pointers in the builder stage —
# keeps profiling tooling out of the image everyone actually runs.
ARG PROFILING_BUILD=0

# Install runtime dependencies
# Note: grpcurl and jq for healthcheck (matching build.sbt healthcheck)
# Note: swi-prolog is required for PeTTa/MeTTa contract execution. The Debian
#       package provides the interpreter's shared-library dependencies; the
#       interpreter binary/library are overlaid with the janus-capable upstream
#       build in the final stage. python3/libpython3.11 back the janus bridge.
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libc6 \
    libjemalloc2 \
    curl \
    python3 \
    libpython3.11 \
    swi-prolog \
    bubblewrap \
    patchelf \
    ncurses-term \
    && if [ "${PROFILING_BUILD}" = "1" ]; then apt-get install -y linux-perf procps binutils; fi \
    && rm -rf /var/lib/apt/lists/*

# Install grpcurl and jq for healthcheck (matching build.sbt)
# Note: These are installed in the runtime stage, architecture will be detected at runtime
RUN GRPCURL_VERSION="1.8.9" && \
    ARCH=$(dpkg --print-architecture) && \
    case "$ARCH" in \
        amd64) GRPCURL_ARCH="x86_64" ;; \
        arm64) GRPCURL_ARCH="arm64" ;; \
        *) GRPCURL_ARCH="x86_64" ;; \
    esac && \
    curl -L "https://github.com/fullstorydev/grpcurl/releases/download/v${GRPCURL_VERSION}/grpcurl_${GRPCURL_VERSION}_linux_${GRPCURL_ARCH}.tar.gz" -o /tmp/grpcurl.tar.gz && \
    tar -xzf /tmp/grpcurl.tar.gz -C /tmp && \
    find /tmp -name grpcurl -type f -executable -exec mv {} /usr/local/bin/ \; && \
    chmod +x /usr/local/bin/grpcurl && \
    rm -rf /tmp/grpcurl* || true && \
    ARCH=$(dpkg --print-architecture) && \
    case "$ARCH" in \
        amd64) JQ_ARCH="linux64" ;; \
        arm64) JQ_ARCH="linuxarm64" ;; \
        *) JQ_ARCH="linux64" ;; \
    esac && \
    curl -L "https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-${JQ_ARCH}" -o /usr/local/bin/jq && \
    chmod +x /usr/local/bin/jq || true

# ============================================================================
# Stage 5: Runtime - Final image, starts at WORKDIR with artifacts from builder
# ============================================================================
FROM runtime-base

# Use existing daemon user (matching build.sbt: daemonUser in Docker := "daemon")
# Note: daemon user already exists in debian:12-slim, so we just ensure it has the right UID
# RUN id -u daemon >/dev/null 2>&1 || useradd -r -s /bin/false -u 1 daemon

# Set up directory structure (matching build.sbt defaultLinuxInstallLocation)
WORKDIR /opt/docker

# Create bin directory first
RUN mkdir -p /opt/docker/bin

# Copy node binary (using node_binary from builder to avoid conflict with node/ directory).
# The Rust binary is fully self-contained: `defaults.conf` and the genesis-ceremony
# `.rho` / `.rhox` resources are baked in via `include_str!` at compile time, so no
# resource files need to be staged into the runtime image.

COPY --from=builder --chmod=755 /build/node_binary /opt/docker/bin/node

# Copy PeTTa (cloned in the builder stage) for MeTTa contract execution at runtime
COPY --from=builder /build/PeTTa ./PeTTa

# Overlay the packaged SWI-Prolog with the upstream janus-capable build so that
# PeTTa's `use_module(library(janus))` (and the JSON libraries) resolve. The
# Debian swi-prolog installed in runtime-base still supplies the required shared
# libraries; here we only replace the interpreter binary and its library home.
COPY --from=swipl-extract /usr/lib/swipl /usr/lib/swipl
COPY --from=swipl-extract /usr/bin/swipl /usr/bin/swipl

# Build-time self-test: fail the build if janus or the JSON libraries are missing.
RUN /usr/bin/swipl -g "use_module(library(janus)), py_call(builtins:int('5'), X), X == 5, halt." -t "halt(1)" && \
    /usr/bin/swipl -g "use_module(library(http/json)), halt." -t "halt(1)" && \
    echo "=== SWI-Prolog/janus self-test PASSED ==="

# Copy Cloudflare libsandbox.so for seccomp sandbox
COPY --from=sandbox-builder /libsandbox.so /usr/lib/libsandbox.so

# Copy petta.sh and functions.sh (required by PeTTa sandboxed execution)
COPY petta.sh functions.sh /opt/docker/

# Create entrypoint script for Rust (adapted from docker-entrypoint.sh)
# Copy the Rust-specific entrypoint script from node directory
# Note: This Dockerfile should be built from workspace root
COPY node/docker-entrypoint-rust.sh /opt/docker/bin/docker-entrypoint.sh

# Create SWI-Prolog paths that petta.sh expects but Debian package omits
RUN SWI_HOME=$(swipl --home 2>/dev/null || echo "/usr/lib/swi-prolog") && \
    [ -f "$SWI_HOME/ABI" ] || echo "9.0.4" > "$SWI_HOME/ABI" && \
    [ -d "$SWI_HOME/app" ] || mkdir -p "$SWI_HOME/app" && \
    [ -d "$SWI_HOME/doc" ] || mkdir -p "$SWI_HOME/doc"

# Set permissions (matching build.sbt: withUser "daemon" withGroup "daemon")
RUN if [ ! -f /opt/docker/bin/node ]; then \
        echo "ERROR: /opt/docker/bin/node is not a file!" >&2 && \
        exit 1; \
    fi && \
    mkdir -p /opt/docker/.petta-cache && \
    chmod +x /opt/docker/petta.sh && \
    chmod +x /opt/docker/functions.sh && \
    chown -R daemon:daemon /opt/docker && \
    chmod +x /opt/docker/bin/node && \
    chmod +x /opt/docker/bin/docker-entrypoint.sh

# Expose ports (matching build.sbt: dockerExposedPorts)
EXPOSE 40400 40401 40402 40403 40404

# Healthcheck (matching build.sbt healthcheck, adapted for Rust)
# Note: The original uses grpcurl and curl with jq
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD grpcurl -plaintext 127.0.0.1:40401 casper.v1.DeployService.status | jq -e && \
        curl -s 127.0.0.1:40403/api/status | jq -e || exit 1

# Image version label. Populate via `--build-arg VERSION=...` from
# docker-commands.sh / CI so the label tracks the actual build instead of
# drifting from node/Cargo.toml.
ARG VERSION=unknown
LABEL maintainer="F1r3fly.io LCA https://f1r3fly.io/"
LABEL version="0.4.45"

# Set environment variables for PeTTa sandboxed execution
# PETTA_PATH is used for local testing only; PETTA_SCRIPT_PATH and PETTA_DIR are consumed by swi_prolog_service.rs
ENV PETTA_PATH=/opt/docker/PeTTa \
    PETTA_SCRIPT_PATH=/opt/docker/petta.sh \
    PETTA_DIR=/opt/docker/PeTTa \
    CACHE_DIR=/opt/docker/.petta-cache \
    SANDBOX_LIB_PATH=/usr/lib/libsandbox.so \
    PETTA_BLOCKED_PREDS=readln! \
    TERMINFO=/usr/share/terminfo \
    TERMINFO_DIRS=/usr/share/terminfo \
    HOME=/opt/docker

# Switch to daemon user (matching build.sbt: daemonUser in Docker := "daemon")
USER daemon

# Entrypoint (matching build.sbt: dockerEntrypoint)
ENTRYPOINT ["/opt/docker/bin/docker-entrypoint.sh"]

# Default command (matching build.sbt: ExecCmd("CMD", "run"))
CMD ["run"]
