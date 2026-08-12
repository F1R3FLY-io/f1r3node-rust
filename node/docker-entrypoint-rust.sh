#!/usr/bin/env sh
set -e

# Preload jemalloc (installed in the runtime image via node/Dockerfile) in
# place of glibc's default allocator. glibc's arena locks measured ~40-48%
# of sampled CPU cycles stuck in futex wait/wake under this node's
# concurrent alloc/free load (asi-chain-testbed issues/13, 2026-08-12
# update, driven by frequent protobuf-decode + deep-clone in hot API paths
# like last_finalized_block); jemalloc's per-thread arenas eliminated that
# contention entirely and roughly doubled sustained TPS in the previously-
# zero-finalization cap>=50 range. A `#[global_allocator]` switch
# (`tikv-jemallocator`) was tried first but its vendored jemalloc's autoconf
# build doesn't cross-compile under this Dockerfile's `xx`/clang-based
# toolchain ("cannot determine return type of strerror_r" — a known
# autoconf-vs-lightweight-cross-compilation failure); LD_PRELOAD against the
# apt-installed binary package sidesteps that entirely, since apt already
# resolves the correct target-architecture build. Set USE_JEMALLOC=0 to opt
# out (e.g. A/B debugging against glibc malloc).
if [ "${USE_JEMALLOC:-1}" != "0" ]; then
    export LD_PRELOAD="/usr/lib/x86_64-linux-gnu/libjemalloc.so.2${LD_PRELOAD:+:$LD_PRELOAD}"
fi

# Execute the node binary with docker profile and any arguments passed to the container
# All dependencies are built into the binary by Cargo, so no library path setup needed
# This matches the Scala entrypoint behavior which automatically sets --profile=docker
exec /opt/docker/bin/node --profile=docker "$@"
