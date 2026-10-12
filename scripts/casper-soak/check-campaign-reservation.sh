#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 1 ]] || exit 2
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
out="$1"
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -m 700 "$out"
out="$(cd "$out" && pwd)"
container=""
scope=native-linux
finish() {
  local code=$?
  trap - EXIT
  if [[ -n "$container" ]]; then
    docker inspect "$container" > "$out/container-final.json" 2>/dev/null || code=1
    docker rm -f "$container" > "$out/cleanup.txt" 2>&1 || code=1
  fi
  jq -n --arg scope "$scope" --argjson code "$code" \
    '{scope:$scope,exit_code:$code,status:(if $code==0 then "passed" else "failed" end),
      node_launches:0,cloud_launches:0,claim_discharge:"pending"}' > "$out/report.json"
  exit "$code"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ "$(uname -s)" == Linux ]]; then
  cargo test --locked -p casper-soak --test campaign_reservation > "$out/tests.txt" 2>&1
else
  scope=isolated-linux
  case "$(uname -m)" in
    arm64|aarch64) target=aarch64-unknown-linux-musl ;;
    x86_64) target=x86_64-unknown-linux-musl ;;
    *) exit 2 ;;
  esac
  CARGO_PROFILE_TEST_OPT_LEVEL=1 RUSTFLAGS='-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static' \
    cargo test --locked -p casper-soak --target "$target" --test campaign_reservation --no-run --message-format=json \
    > "$out/build.jsonl" 2> "$out/build.txt"
  test_binary="$(jq -sr '[.[]|select(.reason=="compiler-artifact" and .target.name=="campaign_reservation" and .profile.test and .executable!=null)|.executable]|last' "$out/build.jsonl")"
  binary="$(jq -sr '[.[]|select(.reason=="compiler-artifact" and .target.name=="casper-campaign-reservation" and .profile.test==false and .executable!=null)|.executable]|last' "$out/build.jsonl")"
  [[ -x "$binary" && -x "$test_binary" ]]
  mkdir -m 755 "$out/bin"
  cp "$binary" "$out/bin/reservation"
  cp "$test_binary" "$out/bin/test"
  chmod 555 "$out/bin/"*
  shasum -a 256 "$out/bin/"* > "$out/binaries.sha256"
  printf 'FROM scratch\nCOPY --chmod=555 reservation test /case/\n' > "$out/bin/Dockerfile"
  docker build --network none --iidfile "$out/image-id.txt" "$out/bin" > "$out/image-build.txt" 2>&1
  image="$(cat "$out/image-id.txt")"
  [[ "$image" =~ ^sha256:[a-f0-9]{64}$ ]]
  container="$(docker create --pull=never --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges --memory 512m --cpus 2 --pids-limit 128 --user 65534:65534 \
    --tmpfs /tmp:rw,nosuid,nodev,size=134217728 --env CASPER_RESERVATION_BIN=/case/reservation \
    --entrypoint /case/test "$image")"
  docker inspect "$container" > "$out/container.json"
  timeout --signal=TERM --kill-after=5 120 docker start -a "$container" > "$out/tests.txt" 2>&1
  docker inspect "$container" > "$out/stopped.json"
  jq -e '.[0].State|.Running==false and .OOMKilled==false and .ExitCode==0' "$out/stopped.json" >/dev/null
fi
grep -Fq 'test result: ok. 16 passed; 0 failed; 1 ignored;' "$out/tests.txt"
grep -Fxq 'test lock_holder_process ... ignored' "$out/tests.txt"
grep -Fxq 'test lock_contention_rejects_and_killed_holder_releases_the_lock ... ok' "$out/tests.txt"
