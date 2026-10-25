#!/usr/bin/env bash
#
# cbc-identify.sh - Resolve CbC-mandatory files (the READ side of the
# git-attribute tags). Part of the /cbc skill (EPIC-025).
#
# See docs/designs/cbc-skill.md and AItools/commands/cbc.md.
#
# Tagging lives in .gitattributes:
#     contracts/**          cbc=mandatory
#     src/consensus/**.rs   cbc=mandatory cbc-weight=high
#
# Public function:
#   cbc_identify [--scope diff|epic <EID>] [--files "a b c"] [--base <ref>] [--json]
#
# The propose side (scan codebase -> propose .gitattributes tags) is a separate
# concern tracked in TODO-025-006; this module only reads existing tags.

# Guard against double-sourcing.
[[ -n "${_CBC_IDENTIFY_SH_LOADED:-}" ]] && return 0
_CBC_IDENTIFY_SH_LOADED=1
_CBC_IDENTIFY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Default task file for epic scope; override via env for tests.
: "${TODOS_FILE:=docs/ToDos.md}"

# Extract every `files:` list entry within an epic's YAML block in TODOS_FILE.
# The block runs from `epic_id: <EID>` to the next `epic_id:`. Only list items
# indented deeper than their `files:` key are collected, so task `- id:` lines
# (shallower) are never mistaken for file paths.
_cbc_epic_files() {
    local eid="$1" todos="${TODOS_FILE}"
    [[ -f "$todos" ]] || { echo "cbc identify: TODOS_FILE not found: $todos" >&2; return 1; }
    awk -v eid="$eid" '
        function indent(s,   junk) { match(s, /^[[:space:]]*/); return RLENGTH }
        $1 == "epic_id:" { inblock = ($2 == eid); infiles = 0 }
        inblock && $0 ~ /^[[:space:]]*files:[[:space:]]*$/ { infiles = 1; fi = indent($0); next }
        inblock && infiles {
            if ($0 ~ /^[[:space:]]*-[[:space:]]+/ && indent($0) > fi) {
                line = $0
                sub(/^[[:space:]]*-[[:space:]]+/, "", line)
                print line
            } else if ($0 !~ /^[[:space:]]*$/) {
                infiles = 0
            }
        }
    ' "$todos" | sort -u
}

# Resolve the in-scope file list (one path per line) for the chosen scope.
_cbc_scope_files() {
    local scope="$1" arg="$2" base="$3" files="$4"
    case "$scope" in
        files) printf '%s\n' $files ;;
        diff)  git diff --name-only "${base:-HEAD}" 2>/dev/null ;;
        epic) _cbc_epic_files "$arg" ;;
        flow)
            echo "cbc identify: --scope flow is not yet supported (resolve via the flow's epic); use --scope epic <EID> or --files" >&2
            return 3
            ;;
        *) echo "cbc identify: unknown scope '$scope'" >&2; return 3 ;;
    esac
}

# Read paths on stdin; print "<weight>\t<path>" for those tagged cbc=mandatory,
# preserving input order and de-duplicating.
_cbc_filter_mandatory() {
    local -a paths=()
    local -A seen=()
    local f
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        [[ -n "${seen[$f]:-}" ]] && continue
        seen[$f]=1
        paths+=("$f")
    done
    [[ ${#paths[@]} -eq 0 ]] && return 0
    printf '%s\0' "${paths[@]}" | python3 "$_CBC_IDENTIFY_DIR/cbc-attributes.py" evidence-check || return 3

    # git check-attr emits "<path>: <attr>: <value>" per requested attribute.
    local out
    out=$(git check-attr cbc cbc-weight cbc-acceptance -- "${paths[@]}" 2>/dev/null) || return 3

    local -A cbc_val=() weight_val=() acceptance_val=()
    local line val rest attr path
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        val="${line##*: }"
        rest="${line%: *}"
        attr="${rest##*: }"
        path="${rest%: *}"
        case "$attr" in
            cbc)        cbc_val[$path]="$val" ;;
            cbc-weight) weight_val[$path]="$val" ;;
            cbc-acceptance) acceptance_val[$path]="$val" ;;
        esac
    done <<< "$out"

    local p w
    for p in "${paths[@]}"; do
        case "${acceptance_val[$p]:-}" in
            unspecified) ;;
            required)
                [[ "${cbc_val[$p]:-}" == mandatory ]] || { echo "cbc identify: acceptance requires cbc=mandatory: $p" >&2; return 3; } ;;
            *) echo "cbc identify: invalid cbc-acceptance policy: $p" >&2; return 3 ;;
        esac
        if [[ "${cbc_val[$p]:-}" == "mandatory" ]]; then
            w="${weight_val[$p]:-}"
            [[ "$w" == "unspecified" || -z "$w" ]] && w="medium"
            printf '%s\t%s\n' "$w" "$p"
        fi
    done
}

_cbc_rows_to_human() {
    local rows="$1"
    if [[ -z "$rows" ]]; then
        echo "No CbC-mandatory files in scope."
        return 0
    fi
    local count
    count=$(printf '%s\n' "$rows" | wc -l | tr -d ' ')
    echo "CbC-mandatory files in scope ($count):"
    printf '%s\n' "$rows" | while IFS=$'\t' read -r w p; do
        printf '  %-7s %s\n' "$w" "$p"
    done
}

_cbc_rows_to_json() {
    local rows="$1"
    if [[ -z "$rows" ]]; then echo "[]"; return 0; fi
    printf '%s\n' "$rows" | jq -R -s '
        split("\n") | map(select(length > 0)) | map(split("\t")) |
        map({weight: .[0], path: .[1]})
    '
}

# Public entry point.
cbc_identify() {
    local scope="diff" arg="" base="" files="" json=0
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --scope)
                scope="${2:-}"; shift 2 || true
                if [[ "$scope" == "epic" || "$scope" == "flow" ]]; then
                    arg="${1:-}"; [[ $# -gt 0 ]] && shift || true
                fi
                ;;
            --files) scope="files"; files="${2:-}"; shift 2 || true ;;
            --base)  base="${2:-}"; shift 2 || true ;;
            --json)  json=1; shift ;;
            *) echo "cbc identify: unknown option '$1'" >&2; return 3 ;;
        esac
    done

    local rows
    rows=$(_cbc_scope_files "$scope" "$arg" "$base" "$files" | _cbc_filter_mandatory) || return $?

    if [[ "$json" -eq 1 ]]; then
        _cbc_rows_to_json "$rows"
    else
        _cbc_rows_to_human "$rows"
    fi
}
