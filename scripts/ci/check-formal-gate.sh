#!/usr/bin/env bash
set -euo pipefail

fail() { printf 'Formal gate: %s\n' "$1" >&2; exit 1; }

snapshot() {
    local job="$1" file digest workflow_digest
    [[ "$job" == tla-model-check || "$job" == rocq-build ]] || fail 'Unknown job.'
    [[ "${GITHUB_SHA:-}" =~ ^[0-9a-f]{40}$ && "${GITHUB_WORKFLOW_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || fail 'Invalid source identity.'
    [[ "${GITHUB_RUN_ID:-}" =~ ^[1-9][0-9]*$ && "${GITHUB_RUN_ATTEMPT:-}" =~ ^[1-9][0-9]*$ ]] || fail 'Invalid run identity.'
    [[ "${GITHUB_REPOSITORY:-}" == F1R3FLY-io/f1r3node-rust ]] || fail 'Unexpected repository.'
    case "${GITHUB_EVENT_NAME:-}" in pull_request|push|schedule|workflow_dispatch) ;; *) fail 'Unexpected event.';; esac
    [[ "${GITHUB_WORKFLOW_REF:-}" == "$GITHUB_REPOSITORY/.github/workflows/slashing-tests.yml@"* ]] || fail 'Unexpected workflow.'
    [[ "$(git rev-parse HEAD)" == "$GITHUB_SHA" ]] || fail 'Checkout identity differs.'
    local -a paths=(.github/workflows/slashing-tests.yml scripts/ci/check-formal-gate.sh scripts/ci/test-check-formal-gate.sh scripts/ci/check-tla-invariants.sh scripts/ci/check-formal-invariants.sh scripts/ci/check-casper-soak-models.sh scripts/bench/casper-soak.sh scripts/casper-soak formal/tlaplus formal/rocq/slashing formal/rocq/fork_choice formal/rocq/rspace_guards Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml)
    git diff --quiet HEAD -- "${paths[@]}" || fail 'Verification sources changed.'
    local untracked inventory
    untracked=$(git ls-files --others --exclude-standard -- "${paths[@]}") || fail 'Cannot inspect untracked inputs.'
    [[ -z "$untracked" ]] || fail 'Untracked verification inputs.'
    inventory=$(git ls-files -- "${paths[@]}") || fail 'Cannot enumerate verification inputs.'
    git ls-files --error-unmatch scripts/ci/check-formal-gate.sh scripts/ci/test-check-formal-gate.sh >/dev/null || fail 'Gate sources are not tracked.'
    workflow_digest=$(git show "$GITHUB_WORKFLOW_SHA:.github/workflows/slashing-tests.yml" | shasum -a 256 | awk '{print $1}') || fail 'Cannot read workflow-control source.'
    [[ "$(shasum -a 256 .github/workflows/slashing-tests.yml | awk '{print $1}')" == "$workflow_digest" ]] || fail 'Workflow-control source differs.'
    local sources
    sources=$(while IFS= read -r file; do
        [[ "$file" =~ ^[A-Za-z0-9_./+-]+$ && -f "$file" && ! -L "$file" ]] || fail 'Invalid source path.'
        digest=$(shasum -a 256 "$file" | awk '{print $1}') || fail 'Cannot hash verification input.'
        printf '%s\t%s\n' "$file" "$digest"
    done <<<"$inventory" | jq -Rn '[inputs | split("\t") | {path:.[0],sha256:.[1]}] | sort_by(.path)') || fail 'Cannot construct source inventory.'
    jq -e 'length > 0' <<<"$sources" >/dev/null || fail 'Empty source inventory.'
    jq -Sn --arg job "$job" --arg repository "$GITHUB_REPOSITORY" --arg event "$GITHUB_EVENT_NAME" --arg run_id "$GITHUB_RUN_ID" --arg attempt "$GITHUB_RUN_ATTEMPT" --arg tested_sha "$GITHUB_SHA" --arg workflow_sha "$GITHUB_WORKFLOW_SHA" --arg workflow_ref "$GITHUB_WORKFLOW_REF" --arg workflow_control_sha256 "$workflow_digest" --argjson sources "$sources" '{schema_version:1,job:$job,repository:$repository,event:$event,run_id:$run_id,attempt:$attempt,tested_sha:$tested_sha,workflow_sha:$workflow_sha,workflow_ref:$workflow_ref,workflow_control_sha256:$workflow_control_sha256,sources:$sources}'
}

case "${1:-}" in
    status)
        [[ $# -eq 1 ]] || fail 'Invalid arguments.'
        [[ "${FORMAL_TLA_RESULT:-}" == success && "${FORMAL_ROCQ_RESULT:-}" == success ]] || fail 'Every prerequisite must succeed.'
        ;;
    record)
        [[ $# -eq 3 ]] || fail 'Invalid arguments.'
        mkdir -p "$(dirname "$3")"
        mkdir "$3"
        snapshot "$2" > "$3/inputs.json"
        ;;
    complete)
        [[ $# -eq 3 && -f "$3/inputs.json" && ! -L "$3/inputs.json" && ! -e "$3/receipt.json" ]] || fail 'Invalid input record.'
        current=$(snapshot "$2")
        [[ "$(< "$3/inputs.json")" == "$current" ]] || fail 'Inputs changed during verification.'
        jq -S '. + {result:"success"}' <<<"$current" > "$3/receipt.json"
        ;;
    verify)
        [[ $# -eq 2 ]] || fail 'Invalid arguments.'
        [[ "${FORMAL_TLA_RESULT:-}" == success && "${FORMAL_ROCQ_RESULT:-}" == success ]] || fail 'Every prerequisite must succeed.'
        for job in tla-model-check rocq-build; do
            receipt="$2/$job/receipt.json"
            [[ -f "$receipt" && ! -L "$receipt" && ! -L "$2/$job" ]] || fail 'Missing or invalid receipt.'
            [[ "$(wc -c < "$receipt")" -le 1048576 ]] || fail 'Oversized receipt.'
            expected=$(snapshot "$job" | jq -S '. + {result:"success"}')
            cmp -s "$receipt" <(printf '%s\n' "$expected") || fail 'Receipt identity or sources differ.'
        done
        ;;
    *) fail 'Use status, record, complete, or verify.' ;;
esac
