#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT=""
EVIDENCE=""
verify_evidence() {
	"${SOAK_BINDING_CHECKER_BIN:-$ROOT/target/debug/check-casper-bindings}" --evidence "$EVIDENCE" --output "$OUTPUT/counts.json"
}
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
while [[ $# -gt 0 ]]; do
	case "$1" in
		--output) OUTPUT="$2"; shift 2 ;;
		--image) IMAGE="$2"; shift 2 ;;
		--verify-evidence) EVIDENCE="$2"; shift 2 ;;
		*) printf 'Unknown argument.\n' >&2; exit 2 ;;
	esac
done
[[ -n "$OUTPUT" && ! -e "$OUTPUT" ]] || exit 2
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
if [[ -n "$EVIDENCE" ]]; then
	verify_evidence
	exit 0
fi
cd "$ROOT"
FILES=(Cargo.toml Cargo.lock scripts/run-merge-recovery-soak.sh scripts/casper-soak scripts/ci/check-casper-soak-bindings.sh scripts/bench/casper-soak.sh scripts/bench/fixtures/casper-lifecycle-executor.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json scripts/bench/run-bench-segment.sh docs/casper/cbc-evidence/runs/casper-rust-migration-20260917-01/bindings.tar.gz)
FILES+=(docs/claims/casper-soak*.md formal/tlaplus/casper_soak)
FILES+=(scripts/bench/test-run-merge-recovery-soak.sh scripts/ci/check-tla-invariants.sh scripts/ci/check-casper-soak-models.sh .github/workflows/merge-recovery-soak.yml .github/workflows/slashing-tests.yml)
FILES+=(docs/casper/cbc-evidence/*.md docs/casper/cbc-evidence/runs/*/report.json)
find "${FILES[@]}" -type f -exec shasum -a 256 {} \; >"$OUTPUT/sources.sha256"
case "$(uname -m)" in
	arm64|aarch64) TARGET=aarch64-unknown-linux-musl ;;
	x86_64) TARGET=x86_64-unknown-linux-musl ;;
	*) exit 2 ;;
esac
CONTAINER=""
COMPLETED=0
finish() {
	status=$?
	trap - EXIT HUP INT TERM
	if [[ "$COMPLETED" != 1 && "$status" == 0 ]]; then status=2; fi
	if [[ -n "$CONTAINER" ]]; then
		if [[ "$COMPLETED" != 1 ]]; then
			timeout --kill-after=5 15 docker stop -t 5 "$CONTAINER" >"$OUTPUT/interrupted-stop.txt" 2>&1 || status=2
			timeout --kill-after=5 15 docker inspect "$CONTAINER" >"$OUTPUT/interrupted-state.json" 2>"$OUTPUT/interrupted-inspect.txt" || status=2
			timeout --kill-after=5 15 docker cp "$CONTAINER:/case/evidence" "$OUTPUT/interrupted-evidence" >"$OUTPUT/interrupted-capture.txt" 2>&1 || status=2
		fi
		timeout --kill-after=5 15 docker rm -f "$CONTAINER" >"$OUTPUT/cleanup.txt" 2>&1 || status=2
	fi
	jq -n --argjson exit "$status" --arg image "$IMAGE" --arg target "$TARGET" \
		'{schema_version:1,status:(if $exit==0 then "passed" else "failed" end),exit_code:$exit,image:$image,target:$target,test_opt_level:1,container_timeout_seconds:550,attach_timeout_seconds:600,evidence_kind:"synthetic_fixture",node_execution:false,claim_discharge:"pending"}' >"$OUTPUT/report.json"
	local captures=()
	for name in evidence interrupted-evidence; do
		if [[ -d "$OUTPUT/$name" ]]; then captures+=("$name"); fi
	done
	if [[ "${#captures[@]}" -gt 0 ]]; then
		(cd "$OUTPUT" && find "${captures[@]}" -type f -exec shasum -a 256 {} \;) >"$OUTPUT/artifacts.sha256"
		COPYFILE_DISABLE=1 tar -czf "$OUTPUT/evidence.tar.gz" -C "$OUTPUT" "${captures[@]}" artifacts.sha256 sources.sha256 report.json
	fi
	exit "$status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
rustup target add "$TARGET" >"$OUTPUT/target-setup.txt" 2>&1
CARGO_PROFILE_TEST_OPT_LEVEL=1 RUSTFLAGS='-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static' \
	cargo test --locked -p casper-soak --target "$TARGET" --no-run --message-format=json \
	>"$OUTPUT/build.jsonl" 2>"$OUTPUT/build.txt"
BIN="$(jq -rs '[.[] | select(.reason=="compiler-artifact" and .target.name=="casper-soak" and .profile.test==false and .executable!=null) | .executable] | last' "$OUTPUT/build.jsonl")"
[[ -x "$BIN" ]] || exit 2
CHECKER="$(jq -rs '[.[] | select(.reason=="compiler-artifact" and .target.name=="check-casper-bindings" and .profile.test==false and .executable!=null) | .executable] | last' "$OUTPUT/build.jsonl")"
[[ -x "$CHECKER" ]] || exit 2
CLAIM_CHECKER="$(jq -rs '[.[] | select(.reason=="compiler-artifact" and .target.name=="check-casper-claims" and .profile.test==false and .executable!=null) | .executable] | last' "$OUTPUT/build.jsonl")"
[[ -x "$CLAIM_CHECKER" ]] || exit 2
if [[ -z "$IMAGE" ]]; then
	docker build --iidfile "$OUTPUT/image-id.txt" - <scripts/bench/soak-disk-test.Dockerfile >"$OUTPUT/image-build.txt" 2>&1
	IMAGE="$(<"$OUTPUT/image-id.txt")"
fi
[[ "$IMAGE" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 2
CONTAINER="$(docker create --pull=never --network none --cap-drop ALL --security-opt no-new-privileges \
	--pids-limit 192 --memory 512m --cpus 2 --user 65534:65534 --entrypoint timeout "$IMAGE" --signal=TERM --kill-after=5 550 bash -euc '
mkdir -p /case/evidence
export SOAK_SOURCE_ROOT=/case/repo SOAK_HARNESS_BIN=/case/casper-soak SOAK_TEST_ARTIFACTS=/case/evidence SOAK_BINDING_CHECKER_BIN=/case/check-casper-bindings SOAK_CLAIM_CHECKER_BIN=/case/check-casper-claims
for suite in manifest models bindings interruption claims driver; do
 /case/test-$suite --test-threads=1 > /case/evidence/$suite.txt 2>&1
done
grep -Eq "test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;" /case/evidence/bindings.txt
grep -Eq "test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;" /case/evidence/interruption.txt
grep -Eq "test result: ok\. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;" /case/evidence/claims.txt
/case/check-casper-bindings --evidence /case/evidence --output /case/evidence/counts.json
')"
docker inspect "$CONTAINER" >"$OUTPUT/container.json"
jq -e '.[0] | (.Mounts|length)==0 and .HostConfig.NetworkMode=="none" and .HostConfig.Privileged==false and .HostConfig.PidMode=="" and .Config.User=="65534:65534"' "$OUTPUT/container.json" >/dev/null
TAR=tar
command -v gtar >/dev/null && TAR=gtar
COPYFILE_DISABLE=1 "$TAR" -cf - "${FILES[@]}" | docker cp - "$CONTAINER:/case/repo/"
docker cp "$BIN" "$CONTAINER:/case/casper-soak" >/dev/null
docker cp "$CHECKER" "$CONTAINER:/case/check-casper-bindings" >/dev/null
docker cp "$CLAIM_CHECKER" "$CONTAINER:/case/check-casper-claims" >/dev/null
for suite in manifest models bindings interruption claims driver; do
	TEST="$(jq -rs --arg suite "$suite" '[.[] | select(.reason=="compiler-artifact" and .target.name==$suite and .profile.test==true and .executable!=null) | .executable] | last' "$OUTPUT/build.jsonl")"
	[[ -x "$TEST" ]] || exit 2
	docker cp "$TEST" "$CONTAINER:/case/test-$suite" >/dev/null
done
status=0
timeout --signal=TERM --kill-after=5 600 docker start -a "$CONTAINER" >"$OUTPUT/run.txt" 2>&1 || status=$?
docker inspect "$CONTAINER" >"$OUTPUT/finished.json"
docker cp "$CONTAINER:/case/evidence" "$OUTPUT/evidence" >/dev/null
jq -e --argjson status "$status" '.[0].State | .Running==false and .OOMKilled==false and .ExitCode==$status' "$OUTPUT/finished.json" >/dev/null
[[ "$status" == 0 ]] || exit "$status"
jq -e '.scope=="binding-inventory-only" and .driver_invocations==91 and .registered_cases==48 and .matched_exits==true and .claim_discharge=="pending"' "$OUTPUT/evidence/counts.json" >/dev/null
rustc --version >"$OUTPUT/evidence/rustc-version.txt"
cargo --version >"$OUTPUT/evidence/cargo-version.txt"
shasum -a 256 -c "$OUTPUT/sources.sha256" >"$OUTPUT/source-check.txt"
COMPLETED=1
