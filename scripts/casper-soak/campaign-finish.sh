#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 5 ]] || exit 2
root="$(realpath -e -- "$1")"
configuration="$(realpath -e -- "$2")"
request="$(realpath -e -- "$3")"
plan="$(realpath -e -- "$4")"
out="$5"
[[ ! -e "$out" && ! -L "$out" ]]
mkdir -m 700 -- "$out"
fetch() {
  local endpoint="$1" file="$2"
  (ulimit -f 2048; timeout --signal=TERM --kill-after=2 45 gh api --hostname github.com --method GET "$endpoint" > "$file") 2> "$file.stderr"
}
collect() {
  fetch "repos/F1R3FLY-io/f1r3node-rust/actions/runs/$GITHUB_RUN_ID/artifacts?per_page=100" "$out/artifacts.json" || return 1
  jq -e --arg name "casper-campaign-worker-$GITHUB_RUN_ID-1" \
    '.total_count==(.artifacts|length) and .total_count<=100 and ([.artifacts[]|select(.name==$name)]|length)==1' "$out/artifacts.json" >/dev/null || return 1
  local artifact_id digest
  artifact_id="$(jq -r --arg name "casper-campaign-worker-$GITHUB_RUN_ID-1" '.artifacts[]|select(.name==$name)|.id' "$out/artifacts.json")" || return 1
  [[ "$artifact_id" =~ ^[1-9][0-9]{0,19}$ ]] || return 1
  fetch "repos/F1R3FLY-io/f1r3node-rust/actions/artifacts/$artifact_id" "$out/artifact.json" || return 1
  fetch "repos/F1R3FLY-io/f1r3node-rust/actions/artifacts/$artifact_id/zip" "$out/worker.zip" || return 1
  digest="$(sha256sum "$out/worker.zip" | cut -d ' ' -f1)" || return 1
  [[ "sha256:$digest" == "$(jq -er .digest "$out/artifact.json")" ]] || return 1
  python3 - "$out/worker.zip" "$out/worker-result.json" <<'PY'
import json
import stat
import sys
import zipfile
from pathlib import Path
with zipfile.ZipFile(sys.argv[1]) as archive:
    members = archive.infolist()
    if len(members) != 1:
        raise ValueError("The result archive must have one file.")
    member = members[0]
    mode = member.external_attr >> 16
    if member.filename != "worker-result.json" or not stat.S_ISREG(mode) or member.file_size > 16384:
        raise ValueError("The result archive member is invalid.")
    data = archive.read(member)
    json.loads(data)
    with Path(sys.argv[2]).open("xb") as output:
        output.write(data)
PY
}
arguments=()
if collect; then
  arguments=(--worker-result "$out/worker-result.json" --artifact "$out/artifact.json" --archive "$out/worker.zip")
fi
"$root/target/debug/casper-campaign-control" finish --root "$root" --config "$configuration" \
  --request "$request" --plan "$plan" --run "$GITHUB_RUN_ID" --evidence "$out/control" "${arguments[@]}"
