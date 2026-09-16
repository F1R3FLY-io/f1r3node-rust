# Fixture image for scripts/bench/test-soak-disk-admission.sh: minimal Debian
# plus the tools the soak driver needs (bash, GNU timeout, find, awk, tar,
# perl-base, ps, jq, and python3 for the pidfd-based owner-marked stops).
# Pinned by manifest-list digest so amd64 and arm64 runners build the same
# image.
FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update && apt-get install -y --no-install-recommends jq procps python3 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /case/repo/scripts/bench && chown -R 65534:65534 /case
USER 65534:65534
WORKDIR /case
