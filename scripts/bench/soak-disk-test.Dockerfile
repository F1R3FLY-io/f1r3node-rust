FROM ruby@sha256:1a41ebabaa0e2d4f3383cdd32170423fb9f7d3c641f9de21745bc33a0f9ab957
RUN apt-get update && apt-get install -y --no-install-recommends jq && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /case/repo/scripts/bench && chown -R 65534:65534 /case
USER 65534:65534
WORKDIR /case
