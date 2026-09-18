#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT=""
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
while [[ $# -gt 0 ]]; do
	case "$1" in
		--output) OUTPUT="$2"; shift 2 ;;
		--image) IMAGE="$2"; shift 2 ;;
		*) printf 'Unknown argument.\n' >&2; exit 2 ;;
	esac
done
[[ -n "$OUTPUT" && ! -e "$OUTPUT" ]] || exit 2
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
cd "$ROOT"
case "$(uname -m)" in
	arm64|aarch64) TARGET=aarch64-unknown-linux-musl ;;
	x86_64) TARGET=x86_64-unknown-linux-musl ;;
	*) exit 2 ;;
esac
CONTAINER=""
finish() {
	status=$?
	trap - EXIT
	if [[ -n "$CONTAINER" ]]; then
		docker rm -f "$CONTAINER" >"$OUTPUT/cleanup.txt" 2>&1 || status=2
	fi
	jq -n --argjson exit "$status" --arg image "$IMAGE" --arg target "$TARGET" \
		'{schema_version:1,status:(if $exit==0 then "passed" else "failed" end),exit_code:$exit,image:$image,target:$target,evidence_kind:"synthetic_fixture",node_execution:false,claim_discharge:"pending"}' >"$OUTPUT/report.json"
	if [[ -d "$OUTPUT/evidence" ]]; then
		(cd "$OUTPUT" && find evidence -type f -exec shasum -a 256 {} \;) >"$OUTPUT/artifacts.sha256"
		COPYFILE_DISABLE=1 tar -czf "$OUTPUT/evidence.tar.gz" -C "$OUTPUT" evidence artifacts.sha256 sources.sha256 report.json
	fi
	exit "$status"
}
trap finish EXIT
rustup target add "$TARGET" >"$OUTPUT/target-setup.txt" 2>&1
RUSTFLAGS='-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static' \
	cargo test --locked -p casper-soak --target "$TARGET" --no-run --message-format=json \
	>"$OUTPUT/build.jsonl" 2>"$OUTPUT/build.txt"
BIN="$(jq -rs '[.[] | select(.reason=="compiler-artifact" and .target.name=="casper-soak" and .profile.test==false and .executable!=null) | .executable] | last' "$OUTPUT/build.jsonl")"
[[ -x "$BIN" ]] || exit 2
if [[ -z "$IMAGE" ]]; then
	docker build --iidfile "$OUTPUT/image-id.txt" - <scripts/bench/soak-disk-test.Dockerfile >"$OUTPUT/image-build.txt" 2>&1
	IMAGE="$(<"$OUTPUT/image-id.txt")"
fi
[[ "$IMAGE" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 2
CONTAINER="$(docker create --pull=never --network none --cap-drop ALL --security-opt no-new-privileges \
	--pids-limit 192 --memory 512m --cpus 2 --user 65534:65534 --entrypoint bash "$IMAGE" -euc '
mkdir -p /case/evidence
export SOAK_SOURCE_ROOT=/case/repo SOAK_HARNESS_BIN=/case/casper-soak SOAK_TEST_ARTIFACTS=/case/evidence
for suite in manifest models driver; do
 /case/test-$suite --test-threads=1 > /case/evidence/$suite.txt 2>&1
done
')"
docker inspect "$CONTAINER" >"$OUTPUT/container.json"
jq -e '.[0] | (.Mounts|length)==0 and .HostConfig.NetworkMode=="none" and .HostConfig.Privileged==false and .HostConfig.PidMode=="" and .Config.User=="65534:65534"' "$OUTPUT/container.json" >/dev/null
FILES=(Cargo.toml Cargo.lock scripts/run-merge-recovery-soak.sh scripts/casper-soak scripts/bench/casper-soak.sh scripts/bench/fixtures/casper-lifecycle-executor.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json scripts/bench/run-bench-segment.sh)
find "${FILES[@]}" -type f -exec shasum -a 256 {} \; >"$OUTPUT/sources.sha256"
TAR=tar
command -v gtar >/dev/null && TAR=gtar
COPYFILE_DISABLE=1 "$TAR" -cf - "${FILES[@]}" | docker cp - "$CONTAINER:/case/repo/"
docker cp "$BIN" "$CONTAINER:/case/casper-soak" >/dev/null
for suite in manifest models driver; do
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
find "$OUTPUT/evidence" -name 'invocation-*.json' -type f -exec jq -c '{expected_exit,actual_exit}' {} \; | \
	jq -s 'length as $count | {driver_invocations:$count,matched_exits:all(.expected_exit==.actual_exit)}' >"$OUTPUT/evidence/counts.json"
jq -e '.driver_invocations==88 and .matched_exits==true' "$OUTPUT/evidence/counts.json" >/dev/null
for suite in manifest models driver; do
	grep -Eq 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored;' "$OUTPUT/evidence/$suite.txt"
done
rustc --version >"$OUTPUT/evidence/rustc-version.txt"
cargo --version >"$OUTPUT/evidence/cargo-version.txt"
shasum -a 256 -c "$OUTPUT/sources.sha256" >"$OUTPUT/source-check.txt"
