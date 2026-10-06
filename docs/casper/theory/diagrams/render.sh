#!/usr/bin/env bash
# ============================================================================
# Cost-Accounted Rho — diagram renderer
# ----------------------------------------------------------------------------
# Renders every diagram source in this directory to a paired .svg, dispatching
# by file extension. Shared colours live in palette.{iuml,d2,tex} +
# palette.mermaid.json (one concept = one colour).
#
# Usage:
#   ./render.sh                 # render every source whose .svg is stale/missing
#   ./render.sh -f              # force re-render everything
#   ./render.sh foo.puml bar.d2 # render only the named sources (always forced)
#
# Toolchains (verified versions): PlantUML, Mermaid (mmdc 11.x, incl.
# sankey-beta), D2 0.7.x, Graphviz 14.x (dot+tred), TikZ (lualatex+dvisvgm),
# svgbob 0.7.x. Exits non-zero if any render fails.
# ============================================================================
set -uo pipefail

cd "$(dirname "$0")" || exit 2

FORCE=0
declare -a TARGETS=()
for arg in "$@"; do
  case "$arg" in
    -f|--force) FORCE=1 ;;
    *) TARGETS+=("$arg"); FORCE=1 ;;
  esac
done

# Sources never rendered on their own (shared includes / config).
is_include() {
  case "$1" in
    palette.iuml|palette.d2|palette.tex|palette.mermaid.json|puppeteer.json) return 0 ;;
    *) return 1 ;;
  esac
}

# stale <source> <svg>  → 0 (true) if svg missing or older than source
stale() {
  [[ "$FORCE" -eq 1 ]] && return 0
  [[ ! -f "$2" ]] && return 0
  [[ "$1" -nt "$2" ]] && return 0
  return 1
}

FAIL=0
RENDERED=0
SKIPPED=0

render_one() {
  local f="$1"
  local base="${f%.*}"
  local svg="${base}.svg"
  is_include "$f" && return 0
  if ! stale "$f" "$svg"; then
    SKIPPED=$((SKIPPED + 1)); return 0
  fi
  printf '  ▸ %-48s' "$f"
  local log; log="$(mktemp)"
  local ok=0
  case "$f" in
    *.puml)
      plantuml -tsvg "$f" >"$log" 2>&1 && ok=1 ;;
    *.mmd)
      mmdc -i "$f" -o "$svg" -b transparent -c palette.mermaid.json -p puppeteer.json >"$log" 2>&1 && ok=1 ;;
    *.d2)
      d2 --layout elk "$f" "$svg" >"$log" 2>&1 && ok=1 ;;
    *.dot)
      tred "$f" 2>"$log" | dot -Tsvg -o "$svg" 2>>"$log" && ok=1 ;;
    *.tex)
      lualatex --output-format=dvi --interaction=nonstopmode --halt-on-error "$f" >"$log" 2>&1 \
        && dvisvgm --font-format=woff --exact --optimize "${base}.dvi" -o "$svg" >>"$log" 2>&1 && ok=1
      rm -f "${base}.dvi" "${base}.aux" "${base}.log" ;;
    *.bob)
      svgbob_cli "$f" -o "$svg" >"$log" 2>&1 && ok=1 ;;
    *)
      echo "SKIP (unknown ext)"; rm -f "$log"; SKIPPED=$((SKIPPED + 1)); return 0 ;;
  esac
  if [[ "$ok" -eq 1 && -s "$svg" ]]; then
    echo "ok"
    RENDERED=$((RENDERED + 1))
  else
    echo "FAILED"
    echo "----- $f -----" >&2
    cat "$log" >&2
    FAIL=$((FAIL + 1))
  fi
  rm -f "$log"
}

if [[ "${#TARGETS[@]}" -gt 0 ]]; then
  for f in "${TARGETS[@]}"; do render_one "$f"; done
else
  shopt -s nullglob
  for f in *.puml *.mmd *.d2 *.dot *.tex *.bob; do render_one "$f"; done
fi

echo "render.sh: ${RENDERED} rendered, ${SKIPPED} up-to-date, ${FAIL} failed"
exit $(( FAIL > 0 ? 1 : 0 ))
