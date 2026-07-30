#!/usr/bin/env bash
# Serve the soak dashboard locally so it can be viewed and edited without a
# soak run or a Pages deploy.
#
# The dashboard fetches its data with fetch(), which browsers refuse to do over
# file://, so opening index.html directly shows a permanently empty page. This
# assembles the same tree CI assembles and serves it over HTTP.
#
#   scripts/preview-soak-dashboard.sh              # sample data, port 8770
#   scripts/preview-soak-dashboard.sh --live       # data from the published site
#   scripts/preview-soak-dashboard.sh --empty      # bootstrap (no data) state
#   scripts/preview-soak-dashboard.sh --port 9000
#   scripts/preview-soak-dashboard.sh --no-serve   # assemble only, keep the tree
#   scripts/preview-soak-dashboard.sh --keep       # serve, keep the tree on exit
#
# The server and the sample-data generator are one std-only Rust program
# (.github/dashboard/preview.rs) built with plain rustc — no crates, no
# Cargo.toml, nothing added to the workspace. The pinned nightly is the only
# toolchain guaranteed to be present here, so the preview depends on nothing
# else. --live additionally needs curl to read the published site.
#
# Everything generated lives in site/, built fresh on start and removed on exit;
# the compiled helper lives in target/. Both are gitignored, so nothing this
# writes can be committed.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DASHBOARD_SRC="$REPO_ROOT/.github/dashboard"
SITE_DIR="$REPO_ROOT/site"
PREVIEW_SRC="$DASHBOARD_SRC/preview.rs"
PREVIEW_BIN="$REPO_ROOT/target/preview-soak-dashboard"
DASHBOARD_URL="${DASHBOARD_URL:-https://f1r3fly-io.github.io/f1r3node-rust/}"

PORT=8770
MODE=sample
SERVE=true
KEEP=false

while [ $# -gt 0 ]; do
  case "$1" in
    --live)     MODE=live ;;
    --sample)   MODE=sample ;;
    --empty)    MODE=empty ;;
    --no-serve) SERVE=false; KEEP=true ;;
    --keep)     KEEP=true ;;
    --port)     PORT="${2:?--port needs a value}"; shift ;;
    -h|--help)  sed -n '2,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)          printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v rustc >/dev/null 2>&1 || {
  printf 'rustc not found — this repo pins a toolchain in rust-toolchain.toml\n' >&2
  exit 1
}

# Refuse to touch anything that is not the throwaway preview tree. SITE_DIR is
# derived, not user-supplied, but this is the one destructive operation here and
# a wrong value would delete real work.
case "$SITE_DIR" in
  "$REPO_ROOT"/site) ;;
  *) printf 'refusing to manage unexpected site dir: %s\n' "$SITE_DIR" >&2; exit 1 ;;
esac

teardown() {
  if [ "$KEEP" = "true" ]; then
    printf '\nLeaving %s in place.\n' "$SITE_DIR"
  else
    rm -rf "$SITE_DIR"
    printf '\nRemoved %s\n' "$SITE_DIR"
  fi
}
trap teardown EXIT

# Rebuilt only when the source is newer, so repeated previews start instantly.
if [ ! -x "$PREVIEW_BIN" ] || [ "$PREVIEW_SRC" -nt "$PREVIEW_BIN" ]; then
  printf 'Building preview helper\n'
  mkdir -p "$(dirname "$PREVIEW_BIN")"
  rustc --edition 2021 -O -o "$PREVIEW_BIN" "$PREVIEW_SRC"
fi

# Built fresh every run: a tree left by an earlier invocation can hold a stale
# index.html or data files from a different mode, which silently misrepresents
# what the current source renders.
rm -rf "$SITE_DIR"
printf 'Assembling %s\n' "$SITE_DIR"
mkdir -p "$SITE_DIR/data"
cp "$DASHBOARD_SRC/index.html" "$SITE_DIR/"

SEED=none
case "$MODE" in
  sample) SEED=sample; printf 'Seeding synthetic sample data\n' ;;
  empty)  SEED=empty ;;
  live)
    command -v curl >/dev/null 2>&1 || {
      printf -- '--live needs curl to read the published site\n' >&2; exit 1; }
    printf 'Fetching published data from %s\n' "$DASHBOARD_URL"
    # Every data file is optional in the page: an absent one renders as an empty
    # tab, which is also what a real bootstrap deploy looks like before the
    # first soak publishes.
    for f in history.json latest-summary.json latest-verdict.json latest-report.md \
             history-daily.json latest-summary-daily.json latest-verdict-daily.json \
             latest-report-daily.md; do
      code="$(curl -sS --max-time 30 -H 'Cache-Control: no-cache' -w '%{http_code}' \
        -o "$SITE_DIR/data/$f" "${DASHBOARD_URL}data/${f}?cb=$$" 2>/dev/null)" || code=000
      if [ "$code" = "200" ]; then
        printf '  %-28s ok\n' "$f"
      else
        rm -f "$SITE_DIR/data/$f"
        printf '  %-28s absent (HTTP %s)\n' "$f" "$code"
      fi
    done
    ;;
esac

if [ "$SERVE" != "true" ]; then
  [ "$SEED" = "none" ] || "$PREVIEW_BIN" --dir "$SITE_DIR" --port 0 --seed "$SEED"
  printf 'Assembled (not serving).\n'
  exit 0
fi

# Deliberately not exec: exec would replace this shell and discard the EXIT
# trap, leaving the preview tree behind on Ctrl-C. Ctrl-C exits 130, which is
# the normal way to stop this, so it must not read as a failure.
"$PREVIEW_BIN" --dir "$SITE_DIR" --port "$PORT" --seed "$SEED" || true
