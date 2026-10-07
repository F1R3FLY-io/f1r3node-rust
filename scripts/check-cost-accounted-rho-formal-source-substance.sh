#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

failures=0
checked=0

while IFS= read -r file; do
  checked=$((checked + 1))
  if [[ ! -s "$file" ]] || ! rg -q '[^[:space:]]' "$file"; then
    echo "error: empty formal repository artifact: $file" >&2
    failures=$((failures + 1))
  fi
done < <(git -c core.fsmonitor=false ls-files --cached --others --exclude-standard formal)

while IFS= read -r file; do
  if ! sed -n '1p' "$file" | rg -q '^[-]+ MODULE [A-Za-z0-9_]+ [-]+$'; then
    echo "error: malformed TLA+ module header: $file" >&2
    failures=$((failures + 1))
  fi
  if ! awk 'NF { line=$0 } END { print line }' "$file" | rg -q '^=+$'; then
    echo "error: malformed TLA+ module terminator: $file" >&2
    failures=$((failures + 1))
  fi
  lines="$(wc -l < "$file")"
  if [[ "$(basename "$file")" == MC*.tla && "$lines" -le 4 ]]; then
    base="$(awk '/^EXTENDS / { print $2; exit }' "$file")"
    if [[ -z "$base" || ! -f "$(dirname "$file")/$base.tla" ]]; then
      echo "error: unresolved thin TLA+ wrapper: $file -> ${base:-<missing>}" >&2
      failures=$((failures + 1))
    fi
  fi
done < <(git -c core.fsmonitor=false ls-files --cached --others --exclude-standard \
  'formal/tlaplus/*.tla' 'formal/tlaplus/**/*.tla')

while IFS= read -r file; do
  if ! rg -q '^[[:space:]]*(SPECIFICATION|INIT)[[:space:]]+' "$file"; then
    echo "error: TLA+ config has neither SPECIFICATION nor INIT: $file" >&2
    failures=$((failures + 1))
  fi
done < <(git -c core.fsmonitor=false ls-files --cached --others --exclude-standard \
  'formal/tlaplus/*.cfg' 'formal/tlaplus/**/*.cfg')

if rg -n '^[[:space:]]*(Admitted\.|admit\.|Axiom[[:space:]]|Parameter[[:space:]])' \
  formal/rocq --glob '*.v'; then
  echo "error: Rocq proof escape hatch detected" >&2
  failures=$((failures + 1))
fi

if rg -n '^[[:space:]]*(sorry|admit|axiom)[[:space:]]' formal/lean --glob '*.lean'; then
  echo "error: Lean proof escape hatch detected" >&2
  failures=$((failures + 1))
fi

if rg -n '(^|[[:space:]])(sorry|oops)([[:space:]]|$)|^[[:space:]]*axiomatization' \
  formal/isabelle --glob '*.thy'; then
  echo "error: Isabelle proof escape hatch detected" >&2
  failures=$((failures + 1))
fi

if [[ "$checked" -eq 0 ]]; then
  echo "error: no formal repository artifacts were checked" >&2
  exit 1
fi

if [[ "$failures" -ne 0 ]]; then
  echo "error: formal source substance gate found $failures failure(s)" >&2
  exit 1
fi

echo "Formal source substance gate passed ($checked repository artifacts)."
