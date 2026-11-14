#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/check-tracked-key-material.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

KEY="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

# fixture NAME: creates a clean repository in $TMP/NAME and sets repo.
fixture() {
	repo="$TMP/$1"
	mkdir -p "$repo/docker/helm/f1r3fly" "$repo/.github/workflows"
	git -C "$repo" init -q
	printf 'STANDALONE_PRIVATE_KEY=%s\n' "$KEY" >"$repo/docker/.env.example"
	cat >"$repo/docker/helm/f1r3fly/values.yaml" <<'YAML'
validatorKeys:
  existingSecret: ""
nodeKeys:
  - publicKey: 04ffc0
YAML
	cat >"$repo/.github/workflows/ci.yml" <<'YAML'
jobs:
  smoke:
    steps:
      - run: node run --validator-private-key="$validator_key"
YAML
	git -C "$repo" add -A
}

# check: sets status and out for $repo.
check() {
	set +e
	out="$(bash "$TOOL" "$repo" 2>&1)"
	status=$?
	set -e
}

fixture clean
check
[ "$status" -eq 0 ] || fail "a clean tree failed: $out"

fixture env
printf 'BOOTSTRAP_PRIVATE_KEY=%s\n' "$KEY" >"$repo/docker/.env"
check
[ "$status" -eq 0 ] || fail "an untracked docker/.env failed: $out"
git -C "$repo" add -f docker/.env
check
[ "$status" -eq 1 ] || fail "a tracked docker/.env passed: $out"
[[ "$out" == *"docker/.env is tracked"* ]] || fail "the tracked docker/.env is not named: $out"

fixture helm
printf '  - publicKey: 04aa\n    privateKey: %s\n' "$KEY" >>"$repo/docker/helm/f1r3fly/values.yaml"
git -C "$repo" add -A
check
[ "$status" -eq 1 ] || fail "a Helm privateKey value passed: $out"
[[ "$out" == *"values.yaml"*"privateKey: <redacted>"* ]] || fail "the Helm key is not named: $out"
[[ "$out" != *"$KEY"* ]] || fail "the Helm key is printed: $out"

fixture helm-quoted
printf '    privateKey: "%s"\n' "$KEY" >"$repo/docker/helm/f1r3fly/minikube-values.yaml"
git -C "$repo" add -A
check
[ "$status" -eq 1 ] || fail "a quoted Helm privateKey value passed: $out"

fixture helm-template
printf 'validator-private-key = {{ .Values.x.privateKey }}\n# privateKey: ""\n' >"$repo/docker/helm/f1r3fly/conf.yaml"
git -C "$repo" add -A
check
[ "$status" -eq 0 ] || fail "a Helm file without a key value failed: $out"

fixture workflow
printf '      - run: node run --validator-private-key=%s --host=localhost\n' "$KEY" >>"$repo/.github/workflows/ci.yml"
git -C "$repo" add -A
check
[ "$status" -eq 1 ] || fail "an inline workflow key passed: $out"
[[ "$out" == *"ci.yml"*"--validator-private-key=<redacted>"* ]] || fail "the workflow key is not named: $out"
[[ "$out" != *"$KEY"* ]] || fail "the workflow key is printed: $out"

for form in "=\"$KEY\"" "='$KEY'" " $KEY" " \"$KEY\""; do
	fixture workflow-form
	printf '      - run: node run --validator-private-key%s --host=localhost\n' "$form" >>"$repo/.github/workflows/ci.yml"
	git -C "$repo" add -A
	check
	[ "$status" -eq 1 ] || fail "an inline workflow key in the form --validator-private-key$form passed: $out"
	[[ "$out" != *"$KEY"* ]] || fail "the workflow key in the form --validator-private-key$form is printed: $out"
	rm -rf "$repo"
done

KEY2="fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
fixture workflow-two-keys
printf '      - run: a --validator-private-key=%s && b --validator-private-key %s\n' "$KEY" "$KEY2" >>"$repo/.github/workflows/ci.yml"
git -C "$repo" add -A
check
[ "$status" -eq 1 ] || fail "a workflow line with two keys passed: $out"
[[ "$out" == *"--validator-private-key=<redacted> && b --validator-private-key <redacted>"* ]] || fail "both keys on one line are not redacted: $out"
[[ "$out" != *"$KEY"* && "$out" != *"$KEY2"* ]] || fail "a key on a line with two keys is printed: $out"

fixture helm-two-keys
printf '  - {privateKey: %s}\n  - {publicKey: 04aa, privateKey: %s}\n' "$KEY" "$KEY2" >>"$repo/docker/helm/f1r3fly/values.yaml"
git -C "$repo" add -A
check
[ "$status" -eq 1 ] || fail "flow-style Helm keys passed: $out"
[[ "$out" != *"$KEY"* && "$out" != *"$KEY2"* ]] || fail "a flow-style Helm key is printed: $out"

fixture all
printf 'K=%s\n' "$KEY" >"$repo/docker/.env"
printf '    privateKey: %s\n' "$KEY" >>"$repo/docker/helm/f1r3fly/values.yaml"
printf '      - run: node --validator-private-key=%s\n' "$KEY" >>"$repo/.github/workflows/ci.yml"
git -C "$repo" add -A -f
check
[ "$status" -eq 1 ] || fail "three violations passed: $out"
[ "$(grep -c '^ERROR:' <<<"$out")" -eq 3 ] || fail "not every violation is reported: $out"

check_repo_status=0
bash "$TOOL" "$TMP/not-a-repo" >/dev/null 2>&1 || check_repo_status=$?
[ "$check_repo_status" -eq 2 ] || fail "a path outside a repository did not exit 2: $check_repo_status"

repo="$ROOT"
check
[ "$status" -eq 0 ] || fail "the repository has tracked validator keys: $out"

awk '
	/^  [a-z_]+:$/ { job = $1 }
	job == "static_checks:" && $0 == "        run: bash .github/scripts/check-tracked-key-material.sh" { found = 1 }
	END { exit !found }
' "$ROOT/.github/workflows/ci.yml" || fail "Static Checks does not run check-tracked-key-material.sh"

echo "check-tracked-key-material tests passed"
