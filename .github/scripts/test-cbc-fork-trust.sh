#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP="$(mktemp -d)"
trap 'rm -rf "$TEMP"' EXIT

awk '
    /^      - name: Configure cleanup acceptance trust$/ { step=1; next }
    step && /^      - / { exit }
    step && /^        run: \|$/ { body=1; next }
    step && /^        run: / { sub(/^        run: /, ""); print; exit }
    body { sub(/^          /, ""); print }
' "$ROOT/.github/workflows/ci.yml" >"$TEMP/step.sh"
test -s "$TEMP/step.sh"

ssh-keygen -q -t ed25519 -N '' -f "$TEMP/approved-key"
ssh-keygen -q -t ed25519 -N '' -f "$TEMP/other-key"
printf 'fixture %s\n' "$(cat "$TEMP/approved-key.pub")" >"$TEMP/approved-signers"
printf 'fixture %s\n' "$(cat "$TEMP/other-key.pub")" >"$TEMP/other-signers"
approved_hash="$(sha256sum "$TEMP/approved-signers" | awk '{print $1}')"
real_sha256="$(command -v sha256sum)"
mkdir "$TEMP/bin"
cat >"$TEMP/bin/sha256sum" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
"$CBC_TEST_SHA256" "$@"
cp "$CBC_TEST_OTHER_SIGNERS" .github/cbc-allowed-signers
SH
chmod +x "$TEMP/bin/sha256sum"
printf 'fixture acceptance\n' >"$TEMP/approval"
ssh-keygen -Y sign -f "$TEMP/approved-key" -n cbc-acceptance "$TEMP/approval" >/dev/null 2>&1

check_case() {
    local name="$1" fork="$2" policy="$3" fallback="$4" digest="$5" expected="$6"
    local directory="$TEMP/$name" code=0 trust test_path="$PATH"
    if [ "$name" = fork-read-once ]; then test_path="$TEMP/bin:$PATH"; fi
    mkdir -p "$directory/.github" "$directory/scripts/ci" "$TEMP/runner-$name"
    git -C "$directory" init -q
    cp "$ROOT/scripts/ci/configure-cbc-trust.sh" "$directory/scripts/ci/"
    if [ "$fallback" != missing ]; then
        cp "$TEMP/$fallback" "$directory/.github/cbc-allowed-signers"
    fi
    (
        cd "$directory"
        CBC_ACCEPTANCE_ALLOWED_SIGNERS="$policy" \
        CBC_FORK_PULL_REQUEST="$fork" \
        CBC_FORK_SIGNERS_SHA256="$digest" \
        CBC_TEST_SHA256="$real_sha256" \
        CBC_TEST_OTHER_SIGNERS="$TEMP/other-signers" \
        PATH="$test_path" \
        RUNNER_TEMP="$TEMP/runner-$name" \
            bash -euo pipefail "$TEMP/step.sh"
    ) >"$directory/output" 2>&1 || code=$?
    if [ "$expected" = pass ]; then
        if [ "$code" -ne 0 ]; then
            printf 'FAIL: %s exited %s\n' "$name" "$code" >&2
            cat "$directory/output" >&2
            exit 1
        fi
        trust="$(git -C "$directory" config --local cbc.acceptanceAllowedSigners)"
        cmp "$TEMP/approved-signers" "$trust"
        test "$(stat -c %a "$trust")" = 600
        ssh-keygen -Y verify -f "$trust" -I fixture -n cbc-acceptance \
            -s "$TEMP/approval.sig" <"$TEMP/approval" >/dev/null 2>&1
    else
        if [ "$code" -eq 0 ] || git -C "$directory" config --local --get cbc.acceptanceAllowedSigners >/dev/null; then
            printf 'FAIL: %s accepted missing or unapproved trust\n' "$name" >&2
            exit 1
        fi
    fi
    printf 'PASS: %s\n' "$name"
}

: >"$TEMP/empty-signers"
check_case fork-approved true '' approved-signers "$approved_hash" pass
check_case fork-read-once true '' approved-signers "$approved_hash" pass
check_case fork-altered true '' other-signers "$approved_hash" fail
check_case fork-missing true '' missing "$approved_hash" fail
check_case fork-empty true '' empty-signers "$approved_hash" fail
check_case fork-no-pin true '' approved-signers '' fail
check_case ordinary-missing false '' approved-signers "$approved_hash" fail
check_case ordinary-policy false "$(cat "$TEMP/approved-signers")" missing '' pass
check_case policy-precedence true "$(cat "$TEMP/approved-signers")" other-signers "$approved_hash" pass

if ssh-keygen -Y verify -f "$TEMP/other-signers" -I fixture -n cbc-acceptance \
    -s "$TEMP/approval.sig" <"$TEMP/approval" >/dev/null 2>&1; then
    printf 'FAIL: an unapproved key verified the acceptance\n' >&2
    exit 1
fi
printf 'PASS: unapproved signature rejected\n'

grep -Fq 'CBC_FORK_PULL_REQUEST: ${{ github.event_name == '\''pull_request'\'' && github.event.pull_request.head.repo.full_name != github.repository }}' \
    "$ROOT/.github/workflows/ci.yml"
pinned="$(awk '/CBC_FORK_SIGNERS_SHA256:/ { print $2 }' "$ROOT/.github/workflows/ci.yml")"
test "${#pinned}" -eq 64
printf '%s  %s\n' "$pinned" "$ROOT/.github/cbc-allowed-signers" | sha256sum --check --status
printf 'PASS: workflow restricts and pins the fork fallback\n'
