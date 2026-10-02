# Multi-stage Dockerfile for Rust F1r3fly Node
# Equivalent to build.sbt Docker configuration

FROM --platform=$BUILDPLATFORM tonistiigi/xx:1.9.0@sha256:c64defb9ed5a91eacb37f96ccc3d4cd72521c4bd18d5442905b95e2226b0e707 AS xx

# ============================================================================
# Stage 1: Builder - Build node binary (Cargo handles all dependencies)
# ============================================================================
FROM --platform=$BUILDPLATFORM rust:bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS builder

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
COPY scripts/supply-chain/ ./scripts/supply-chain/

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

RUN xx-cargo --setup-target-triple && \
    cargo fetch --locked --target "$(xx-cargo --print-target-triple)"

RUN --network=none \
    TARGET_TRIPLE="$(xx-cargo --print-target-triple)" && \
    export TARGET_TRIPLE && \
    PKG_CONFIG_SYSROOT_DIR="$(xx-info sysroot)" && \
    export PKG_CONFIG_SYSROOT_DIR && \
    export PKG_CONFIG_PATH="${PKG_CONFIG_SYSROOT_DIR}/usr/lib/$(xx-info triple)/pkgconfig:${PKG_CONFIG_SYSROOT_DIR}/usr/share/pkgconfig" && \
    export PKG_CONFIG_ALLOW_CROSS=1 && \
    case "${TARGET_TRIPLE}" in \
        aarch64*) export RUSTFLAGS="-C target-feature=+aes" ;; \
        *) export RUSTFLAGS="-C target-feature=+aes,+sse2" ;; \
    esac && \
    if [ "${PROFILING_BUILD}" = "1" ]; then export RUSTFLAGS="${RUSTFLAGS} -C force-frame-pointers=yes"; fi && \
    xx-cargo build --frozen --release -p node --target-dir ./build && \
    xx-verify "./build/${TARGET_TRIPLE}/release/node" && \
    cp "./build/${TARGET_TRIPLE}/release/node" /build/node_binary && \
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
FROM swipl:stable@sha256:9d44cac3f4235e297db87217e59aa81cdf9847970bd27ddb447072d5466ea8ae AS swipl-extract

# ============================================================================
# Optional stage: petta-test - Run the PeTTa/MeTTa test suite in-container
# ============================================================================
# NOT part of the default build (the runtime stage below stays the default
# target). It reuses the Rust toolchain, the workspace source, and the pinned
# PeTTa already present in the builder; only `swipl` is added on top. Build and
# run it explicitly:
#   docker build --target petta-test -t f1r3fly-petta-test -f node/Dockerfile .
#   docker run --rm f1r3fly-petta-test
# Add `-v petta-cargo:/usr/local/cargo/registry -v petta-target:/build/target`
# to the run to cache crates and compiled artifacts across repeated runs.
FROM builder AS petta-test

# The Debian swi-prolog package pulls in the shared libraries the interpreter
# needs; python3/libpython3.11 back the janus bridge. The interpreter itself is
# then overlaid with the janus-capable build below.
RUN apt-get update && apt-get install -y --no-install-recommends \
    swi-prolog \
    python3 \
    libpython3.11 \
    && rm -rf /var/lib/apt/lists/*

# Overlay the packaged 9.0.4 with the upstream janus-capable SWI-Prolog so the
# tests exercise the same interpreter the runtime image ships.
COPY --from=swipl-extract /usr/lib/swipl /usr/lib/swipl
COPY --from=swipl-extract /usr/bin/swipl /usr/bin/swipl

# The test script defaults PETTA_PATH to <repo-root>/PeTTa (i.e. /build/PeTTa),
# which is exactly where the builder cloned it; set it explicitly anyway.
ENV PETTA_PATH=/build/PeTTa
WORKDIR /build

# gxhash (via the PathMap crate) needs AES CPU features to compile, matching the
# release build above. +sse2 is x86-only; on aarch64 only +aes is valid.
ENTRYPOINT ["bash", "-c", "case \"$(uname -m)\" in aarch64) export RUSTFLAGS='-C target-feature=+aes' ;; *) export RUSTFLAGS='-C target-feature=+aes,+sse2' ;; esac; exec rholang/tests/RUN_PETTA_TESTS.sh"]

# ============================================================================
# Stage 2: Runtime base - Install runtime dependencies (cached independently)
# ============================================================================
FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime-base

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Re-declared: ARGs don't cross FROM boundaries. Gates the `perf`/`procps`
# install below the same way it gates frame pointers in the builder stage —
# keeps profiling tooling out of the image everyone actually runs.
ARG PROFILING_BUILD=0

# Install runtime dependencies.
# curl is the only tool the container HEALTHCHECK further below needs (GET /api/ready).
# swi-prolog is required for PeTTa/MeTTa contract execution. The Debian package
# provides the interpreter's shared-library dependencies; the interpreter
# binary/library are overlaid with the janus-capable upstream build in the final
# stage. python3/libpython3.11 back the janus bridge.
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libc6 \
    libjemalloc2 \
    curl \
    jq \
    python3 \
    libpython3.11 \
    swi-prolog \
    && if [ "${PROFILING_BUILD}" = "1" ]; then apt-get install -y linux-perf procps binutils; fi \
    && rm -rf /var/lib/apt/lists/*

RUN GRPCURL_VERSION="1.8.9" && \
    ARCH=$(dpkg --print-architecture) && \
    case "$ARCH" in \
        amd64) GRPCURL_ARCH="x86_64"; GRPCURL_SHA256="a422d1e8ad854a305c0dd53f2f2053da242211d3d1810e7addb40a041e309516" ;; \
        arm64) GRPCURL_ARCH="arm64"; GRPCURL_SHA256="1303f4c1c6667f31b80efbe483875c749c94c8cb0d8b631bd64179f0b140714d" ;; \
        *) exit 1 ;; \
    esac && \
    curl --fail --location --proto '=https' --proto-redir '=https' \
        "https://github.com/fullstorydev/grpcurl/releases/download/v${GRPCURL_VERSION}/grpcurl_${GRPCURL_VERSION}_linux_${GRPCURL_ARCH}.tar.gz" \
        -o /tmp/grpcurl.tar.gz && \
    echo "${GRPCURL_SHA256}  /tmp/grpcurl.tar.gz" | sha256sum --check --strict && \
    tar -xzf /tmp/grpcurl.tar.gz -C /usr/local/bin grpcurl && \
    chmod +x /usr/local/bin/grpcurl && \
    rm /tmp/grpcurl.tar.gz

# ============================================================================
# Stage 3: Runtime - Final image, starts at WORKDIR with artifacts from builder
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

# Create entrypoint script for Rust (adapted from docker-entrypoint.sh)
# Copy the Rust-specific entrypoint script from node directory
# Note: This Dockerfile should be built from workspace root
COPY node/docker-entrypoint-rust.sh /opt/docker/bin/docker-entrypoint.sh

# Set permissions (matching build.sbt: withUser "daemon" withGroup "daemon")
RUN if [ ! -f /opt/docker/bin/node ]; then \
        echo "ERROR: /opt/docker/bin/node is not a file!" >&2 && \
        exit 1; \
    fi && \
    chown -R daemon:daemon /opt/docker && \
    chmod +x /opt/docker/bin/node && \
    chmod +x /opt/docker/bin/docker-entrypoint.sh

# Expose ports (matching build.sbt: dockerExposedPorts)
EXPOSE 40400 40401 40402 40403 40404

# Healthcheck: report healthy only once Casper has entered the Running state
# and can serve deploys. /api/ready returns 503 while Casper is still
# initializing (genesis ceremony, LFS sync), so `docker compose up --wait`
# and `depends_on: condition: service_healthy` line up with deploy readiness.
HEALTHCHECK --interval=30s --timeout=10s --start-period=120s --retries=3 \
    CMD curl -fsS 127.0.0.1:40403/api/ready || exit 1

# Image version label. Populate via `--build-arg VERSION=...` from
# docker-commands.sh / CI so the label tracks the actual build instead of
# drifting from node/Cargo.toml.
ARG VERSION=unknown
LABEL maintainer="F1r3fly.io LCA https://f1r3fly.io/"
LABEL version="0.4.46"

# Set PETTA_PATH environment variable for MeTTa contract execution
ENV PETTA_PATH=/opt/docker/PeTTa

# Switch to daemon user (matching build.sbt: daemonUser in Docker := "daemon")
USER daemon

# Entrypoint (matching build.sbt: dockerEntrypoint)
ENTRYPOINT ["/opt/docker/bin/docker-entrypoint.sh"]

# Default command (matching build.sbt: ExecCmd("CMD", "run"))
CMD ["run"]
