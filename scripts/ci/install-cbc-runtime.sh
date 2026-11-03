#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$ROOT"
lock=scripts/cbc-runtime.lock.json
check=0
checkout=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --check) check=1; shift ;;
        --source-checkout) checkout="${2:?A source checkout is required}"; shift 2 ;;
        *) printf 'Unknown runtime installer option: %s\n' "$1" >&2; exit 4 ;;
    esac
done
fail() { printf '%s\n' "$1" >&2; exit 4; }
required='["cbc.sh","lib/cbc-identify.sh","lib/cbc-verify.sh","lib/cbc-gate.sh","lib/cbc-ledger.sh","lib/cbc-acceptance.py","lib/cbc-attributes.py","lib/cbc-adapters/mock.sh","lib/cbc-adapters/embedded.sh","lib/cbc-adapters/agentic.sh","lib/llm-client.sh","lib/providers/abacusai.sh","lib/providers/anthropic.sh","lib/providers/bedrock.sh","lib/providers/gemini.sh","lib/providers/local.sh","lib/providers/nano-banana.sh","lib/providers/openai.sh","lib/providers/openrouter.sh","lib/providers/xai.sh"]'
jq -e --argjson required "$required" '
    .schema=="cbc-runtime/v1" and
    .repository=="https://gitlab.com/smart-assets.io/gitlab-profile.git" and
    .source_directory=="AItools/scripts" and
    (.revision|type=="string" and test("^[0-9a-f]{40}$")) and
    (.sha256|type=="object") and (.sha256|keys)==($required|sort) and
    all(.sha256[];type=="string" and test("^[0-9a-f]{64}$"))' "$lock" >/dev/null || fail 'The CbC runtime lockfile is invalid.'
revision="$(jq -r .revision "$lock")"
identity="$revision-$(shasum -a 256 "$lock" | awk '{print $1}')"
cache="${CBC_RUNTIME_CACHE:-${RUNNER_TEMP:-${XDG_CACHE_HOME:-${HOME:?HOME is required}/.cache}}/f1r3node/cbc}"
case "$cache" in /*) ;; *) fail 'The CbC runtime cache must use an absolute path.' ;; esac
case "/$cache/" in */../*) fail 'The CbC runtime cache path must not contain parent steps.' ;; esac
ancestor="$cache"
while [ ! -d "$ancestor" ]; do ancestor="$(dirname "$ancestor")"; done
ancestor="$(cd "$ancestor" && pwd -P)"
case "$ancestor/" in "$ROOT/"*) fail 'The CbC runtime cache must be outside the repository.' ;; esac
if [ ! -d "$cache" ]; then
    [ "$check" -eq 0 ] || fail 'Install the pinned runtime with: bash scripts/ci/install-cbc-runtime.sh'
    mkdir -p "$cache"
fi
cache="$(cd "$cache" && pwd -P)"
case "$cache/" in "$ROOT/"*) fail 'The CbC runtime cache must be outside the repository.' ;; esac
destination="$cache/$identity"
checks="$(mktemp)"
staging=""
publish_lock=""
trap '
    rm -f "$checks"
    [ -z "$staging" ] || rm -rf "$staging"
    [ -z "$publish_lock" ] || rmdir "$publish_lock"
' EXIT
jq -r '.sha256|to_entries[]|"\(.value)  AItools/scripts/\(.key)"' "$lock" >"$checks"
valid() {
    local directory="$1"
    [ -d "$directory" ] && [ ! -L "$directory" ] || return 1
    [ -z "$(find "$directory" ! -type d ! -type f -print -quit)" ] || return 1
    [ "$(find "$directory" -type f | wc -l | tr -d ' ')" -eq "$(jq '.sha256|length' "$lock")" ] || return 1
    (cd "$directory" && shasum -a 256 -c "$checks") >/dev/null 2>&1
}
if [ -e "$destination" ] || [ -L "$destination" ]; then
    valid "$destination" || fail 'The cached CbC runtime differs from the lockfile.'
    printf '%s\n' "$destination/AItools/scripts"
    exit 0
fi
[ "$check" -eq 0 ] || fail 'Install the pinned runtime with: bash scripts/ci/install-cbc-runtime.sh'
umask 077
staging="$(mktemp -d "$cache/.install.XXXXXX")"
while IFS= read -r path; do
    mkdir -p "$staging/AItools/scripts/$(dirname "$path")"
    if [ -n "$checkout" ]; then
        git -C "$checkout" show "$revision:AItools/scripts/$path" >"$staging/AItools/scripts/$path" || fail 'The pinned runtime is absent from the source checkout.'
    else
        curl --fail --location --silent --show-error --retry 2 --connect-timeout 10 --max-time 60 \
            --output "$staging/AItools/scripts/$path" \
            "https://gitlab.com/smart-assets.io/gitlab-profile/-/raw/$revision/AItools/scripts/$path" || fail 'The pinned CbC runtime download failed.'
    fi
done < <(jq -r '.sha256|keys[]' "$lock")
valid "$staging" || fail 'The downloaded CbC runtime differs from the lockfile.'
find "$staging" -type f -exec chmod a-w {} +
for ((attempt=0; attempt<30; attempt++)); do
    if mkdir "$cache/.publish-$identity" 2>/dev/null; then
        publish_lock="$cache/.publish-$identity"
        if [ ! -e "$destination" ] && [ ! -L "$destination" ]; then
            mv "$staging" "$destination"
            staging=""
        fi
        valid "$destination" || fail 'The published CbC runtime differs from the lockfile.'
        printf '%s\n' "$destination/AItools/scripts"
        exit 0
    fi
    sleep 0.1
done
fail 'Another process holds the CbC runtime installation lock.'
