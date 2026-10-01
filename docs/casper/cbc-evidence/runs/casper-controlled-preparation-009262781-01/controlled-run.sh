#!/usr/bin/env bash
# Controlled preparation run for TASK-017-12 inside the Linux tools container.
# Inputs: /t (release binaries), /env (environment), /run/soak (tmpfs, mode 700).
set -u
NODE=/t/release/node
OWNER=/t/release/casper-authority-process
P2P=/t/release/casper-authority-p2p
RUN=/env/run
python3 -c 'import shutil; shutil.rmtree("/env/run", ignore_errors=True)'
mkdir -p "$RUN" /run/soak/observer
chmod 700 /run/soak/observer
python3 -c 'import shutil; shutil.rmtree("/env/target-data", ignore_errors=True); shutil.copytree("/env/genesis-snapshot", "/env/target-data", symlinks=True)'

REV=$(cat /env/revision.txt)
NODE_SHA=$(sha256sum "$NODE" | cut -c1-64)
CANON='import json,sys; print(json.dumps(json.load(sys.stdin), sort_keys=True, separators=(",",":")))'

# 1. The approved authority request. Its canonical digest is the observer's approved-request-sha256.
python3 - > "$RUN/authority.json" <<'PY'
import json
print(json.dumps({
  "capture": {"max_value_bytes":1048576,"max_total_bytes":16777216,"max_records":4096,"max_operations":100000,
              "max_compressed_bytes":1048576,"max_decompressed_bytes":1048576,"max_expansion_ratio":4096,
              "max_blocks":128,"max_validators":64,"max_edges":4096,"max_work":2000000,"lock_wait_ms":100},
  "evaluation": {"operations":2000000,"allocated_bytes":268435456,"clique_expansions":100000,"recursion_depth":64},
  "targets": [json.load(open("/env/target.json"))["hash"]],
  "body_hashes": [],
  "floor": None,
  "original": True, "reference": True, "strict": False,
  "fork_choice": {"reference": True},
  "display": {"max_equivocation_records": 64}
}, sort_keys=True, separators=(",",":")))
PY
APPROVED=$(sha256sum "$RUN/authority.json" | cut -c1-64)
echo "approved_request_sha256=$APPROVED"

# 2. The process configuration: the node under the owner with the observer bound to the owner identity.
OBS_CFG="{\"directory\":\"/run/soak/observer\",\"source-revision\":\"$REV\",\"approved-request-sha256\":\"$APPROVED\",\"peer-pid\":{owner_pid},\"peer-start-ticks\":{owner_start_ticks},\"session-timeout-ms\":500,\"max-sessions\":128}"
python3 - "$NODE" "$NODE_SHA" "$OBS_CFG" > "$RUN/process.json" <<'PY'
import json,sys
node, sha, obs = sys.argv[1:4]
print(json.dumps({"path": node, "sha256": sha,
  "arguments": ["--log-sink=stdout","run","-s","--config-file","/env/conf/rnode.conf","--data-dir","/env/target-data",
                "--host","127.0.0.1","--no-upnp","--allow-private-addresses",
                "--soak-observer", obs],
  "working_directory": "/env/target-data", "observer_socket": "/run/soak/observer/observer.sock"}, indent=1))
PY
chmod 600 "$RUN/process.json"

# 3. Launch the owner. It starts the node and serves requests for the lifetime.
"$OWNER" --config "$RUN/process.json" --output /run/soak/owner --lifetime-ms 290000 > "$RUN/owner.log" 2>&1 &
OWNER_PID=$!
for i in $(seq 1 60); do sleep 1; [ -S /run/soak/observer/observer.sock ] && [ -f /run/soak/owner/owner.json ] && break; done
if [ ! -f /run/soak/owner/owner.json ]; then echo "owner did not start"; cat "$RUN/owner.log"; exit 1; fi
echo "owner up after ${i}s"
cp /run/soak/owner/owner.json "$RUN/owner.json"
NODE_PID=$(jq -r '.child.pid // .pid' /run/soak/owner/owner.json)
echo "owner.json keys: $(jq -c 'keys' /run/soak/owner/owner.json)"
sleep 25

# 4. The public configuration digest, computed as the node computes it (sorted keys, compact).
python3 - > "$RUN/public-config.json" <<'PY2'
import json
public = {"schema_version":1,"network_id":"standalone-dev","shard_name":"root","standalone":True,
          "max_parent_depth":15,"max_number_of_parents":100,"fault_tolerance_threshold_bits":0}
print(json.dumps(public, sort_keys=True, separators=(",",":")), end="")
PY2
CONFIG_SHA=$(sha256sum "$RUN/public-config.json" | cut -c1-64)
echo "configuration_sha256 (computed)=$CONFIG_SHA"
echo "$CONFIG_SHA" > "$RUN/configuration_sha256.txt"

capture() {
  local name=$1
  python3 - "$name" <<'PY' > "$RUN/capture-$1.json"
import json, sys, time, uuid
name = sys.argv[1]
owner = json.load(open("/env/run/owner.json"))
child = owner["child"]
boot = open("/proc/sys/kernel/random/boot_id").read().strip()
now = time.monotonic_ns()
import hashlib
approved = hashlib.sha256(open("/env/run/authority.json","rb").read()).hexdigest()
binding = {"socket": "/run/soak/observer/observer.sock", "node_pid": child["pid"],
  "process_start_ticks": child["process_start_ticks"], "source_revision": open("/env/revision.txt").read().strip(),
  "executable_sha256": child["executable_sha256"], "configuration_sha256": open("/env/run/configuration_sha256.txt").read().strip(),
  "approved_request_sha256": approved, "request_id": str(uuid.uuid4()), "timeout_ms": 20000}
req = {"schema_version": 1, "action": "capture", "clock_id": "linux-monotonic:" + boot,
  "deadline_monotonic_ns": now + 60_000_000_000, "binding": binding,
  "authority": json.load(open("/env/run/authority.json"))}
print(json.dumps(req, sort_keys=True, separators=(",", ":")))
PY
  "$OWNER" --owner "$RUN/owner.json" --request "$RUN/capture-$name.json" > "$RUN/capture-$name.response.json" 2>&1
  echo "capture $name: $(jq -c '{status, capture_root, error}' "$RUN/capture-$name.response.json" 2>/dev/null || head -c 300 "$RUN/capture-$name.response.json")"
  local dir; dir=$(ls -d /run/soak/owner/capture-* 2>/dev/null | tail -1)
  if [ -n "$dir" ]; then
    echo "  report: $(jq -c '{status, availability, mapping_status, error}' "$dir/report.json" 2>/dev/null)"
    [ -f "$dir/hello.json" ] && echo "  hello identity.configuration_sha256: $(jq -r .identity.configuration_sha256 "$dir/hello.json")"
    [ -f "$dir/response.json" ] && echo "  response: $(jq -c '{availability: .result.availability, reason: .result.reason}' "$dir/response.json")"
  fi
}

# 5. Capture before the delivery (genesis only).
capture before

# 6. Deliver the block history with the p2p driver.
NODE_PEER=$(grep -o "rnode://[0-9a-f]*@127.0.0.1?protocol=40400&discovery=40404" /env/history.log | head -1)
CLIENT_PEER=$(cat /env/client-peer.txt)
python3 - "$NODE_PEER" "$CLIENT_PEER" <<'PY'
import json, hashlib, sys, os, shutil
node_peer, client_peer = sys.argv[1:3]
root = "/env/run/p2p"; os.makedirs(root, exist_ok=True)
def ref(path, name):
    data = open(path, "rb").read()
    dst = os.path.join(root, name); open(dst, "wb").write(data)
    return {"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
blocks = json.load(open("/env/blocks/blocks.json"))
order = sorted((v["block_number"], k) for k, v in blocks.items() if v["block_number"] > 0)
refs = [ref("/env/blocks/" + blocks[k]["path"], blocks[k]["path"]) for _, k in order]
cert = ref("/env/client-data/node.certificate.pem", "client.pem")
shutil.copy("/env/client-data/node.key.pem", os.path.join(root, "key.pem")); os.chmod(os.path.join(root, "key.pem"), 0o600)
fixture = {"schema_version": 1, "kind": "signed-block-sequence-v1", "members": {"bounded": {
    "local_peer": client_peer, "target_peer": node_peer, "network_id": "standalone-dev", "timeout_ms": 5000,
    "certificate": cert, "private_key_path": os.path.join(root, "key.pem"),
    "operations": {"load_fixture": refs, "replay_fixture": refs}}}}
fb = (json.dumps(fixture, sort_keys=True, separators=(",", ":")) + "\n").encode()
open(os.path.join(root, "fixture.json"), "wb").write(fb)
fref = {"path": "fixture.json", "bytes": len(fb), "sha256": hashlib.sha256(fb).hexdigest()}
import time
req = {"schema_version": 1, "step": {"index": 0, "operation": "load_fixture", "member": {"member_id": "bounded"}},
       "input_root": root, "inputs": {"fixture": fref}, "deadline_monotonic_ns": str(time.monotonic_ns() + 30_000_000_000)}
open(os.path.join(root, "request.json"), "w").write(json.dumps(req, sort_keys=True, separators=(",", ":")) + "\n")
os.makedirs(os.path.join(root, "output"), exist_ok=True)
print("p2p fixture: %d blocks" % len(refs))
PY
CASPER_AUTHORITY_STEP_REQUEST="$RUN/p2p/request.json" CASPER_AUTHORITY_STEP_OUTPUT="$RUN/p2p/output" "$P2P" > "$RUN/p2p/driver.log" 2>&1
echo "p2p driver exit $?"
jq -c '{status, deliveries: (.transport.deliveries | map(.transport_acknowledged))}' "$RUN/p2p/output/result.json" 2>/dev/null || head -c 400 "$RUN/p2p/driver.log"
sleep 20

# 7. Capture after the delivery.
capture after

# 8. Stop the owner.
kill "$OWNER_PID" 2>/dev/null; wait "$OWNER_PID" 2>/dev/null
cp -a /run/soak/owner "$RUN/owner-output"
echo "owner output: $(ls /run/soak/owner | tr '\n' ' ')"

# 9. Controlled casper-authority-live run on a fresh target node under a new owner.
echo "=== live executor"
python3 -c 'import shutil; shutil.rmtree("/env/target-data", ignore_errors=True); shutil.copytree("/env/genesis-snapshot", "/env/target-data", symlinks=True); shutil.rmtree("/run/soak/owner", ignore_errors=True); shutil.rmtree("/run/soak/observer", ignore_errors=True)'
mkdir -m 700 /run/soak/observer
"$OWNER" --config "$RUN/process.json" --output /run/soak/owner --lifetime-ms 290000 > "$RUN/owner-live.log" 2>&1 &
OWNER_PID=$!
for i in $(seq 1 60); do sleep 1; [ -S /run/soak/observer/observer.sock ] && [ -f /run/soak/owner/owner.json ] && break; done
cp /run/soak/owner/owner.json "$RUN/owner-live.json"
echo "live owner up after ${i}s"
sleep 20
LIVE="$RUN/live"; mkdir -p "$LIVE/input"
python3 - "$P2P" "$REV" "$NODE_SHA" "$CONFIG_SHA" "$APPROVED" <<'PY'
import json, hashlib, os, shutil, time, uuid
p2p, rev, node_sha, config_sha, approved = __import__("sys").argv[1:6]
root = "/env/run/live/input"
def ref(name, data):
    open(os.path.join(root, name), "wb").write(data)
    return {"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
owner = json.load(open("/env/run/owner-live.json")); child = owner["child"]
authority = json.load(open("/env/run/authority.json"))
target = json.load(open("/env/target.json"))["hash"]
# The p2p fixture, copied from the manual run.
blocks = json.load(open("/env/blocks/blocks.json"))
order = sorted((v["block_number"], k) for k, v in blocks.items() if v["block_number"] > 0)
refs = [ref(blocks[k]["path"], open("/env/blocks/" + blocks[k]["path"], "rb").read()) for _, k in order]
cert = ref("client.pem", open("/env/client-data/node.certificate.pem", "rb").read())
shutil.copy("/env/client-data/node.key.pem", os.path.join(root, "key.pem")); os.chmod(os.path.join(root, "key.pem"), 0o600)
import re
node_peer = re.search(r"rnode://[0-9a-f]+@127\.0\.0\.1\?protocol=40400&discovery=40404", open("/env/history.log").read()).group(0)
client_peer = open("/env/client-peer.txt").read().strip()
fixture = {"schema_version": 1, "kind": "signed-block-sequence-v1", "members": {m: {
    "local_peer": client_peer, "target_peer": node_peer, "network_id": "standalone-dev", "timeout_ms": 5000,
    "certificate": cert, "private_key_path": os.path.join(root, "key.pem"),
    "operations": {"load_fixture": refs, "replay_fixture": refs, "evaluate": []}} for m in ["bounded", "reference"]}}
inputs = {"fixture": ref("fixture.json", (json.dumps(fixture, sort_keys=True, separators=(",", ":")) + "\n").encode())}
for role in ["dag", "electorate", "justification"]:
    note = {"role": role, "scope": "controlled-preparation-placeholder", "node_revision": rev,
            "statement": "No node input export exists for this role. The p2p driver reports status unknown and exports nothing."}
    inputs[role] = ref(role + ".json", (json.dumps(note, sort_keys=True, separators=(",", ":")) + "\n").encode())
binding = {"socket": "/run/soak/observer/observer.sock", "node_pid": child["pid"], "process_start_ticks": child["process_start_ticks"],
           "source_revision": rev, "executable_sha256": child["executable_sha256"], "configuration_sha256": config_sha,
           "approved_request_sha256": approved, "request_id": str(uuid.uuid4()), "timeout_ms": 20000}
member_config = {"binding": binding, "authority": authority, "target": target, "configuration_sha256": config_sha,
                 "process_owner": owner}
candidate = "controlled-" + rev[:9]
members = [{"member_id": m, "evaluation_mode": m, "candidate_id": candidate, "node_id": "target-node", "node_revision": rev,
            "node_binary_digest": node_sha, "incarnation": None, "dag_digest": inputs["dag"]["sha256"],
            "electorate_digest": inputs["electorate"]["sha256"], "justification_digest": inputs["justification"]["sha256"]}
           for m in ["bounded", "reference"]]
p2p_bytes = open(p2p, "rb").read()
manifest = {"manifest_digest": hashlib.sha256(b"controlled-preparation-" + rev.encode()).hexdigest(), "run_id": "controlled-live-" + rev[:9],
            "phase": "pre_pr216_merge", "evidence_kind": "node_observation", "policy_variant": "baseline",
            "candidate_id": candidate, "node_revision": rev, "node_binary_digest": node_sha,
            "runtime": {"authority_live": {"driver": {"path": p2p, "sha256": hashlib.sha256(p2p_bytes).hexdigest(), "bytes": len(p2p_bytes), "arguments": []},
                                           "members": {"bounded": member_config, "reference": member_config}}}}
operations = []
for m in members:
    for op in ["load_fixture", "evaluate"]:
        operations.append({"index": len(operations), "operation": op, "member": m})
boot = open("/proc/sys/kernel/random/boot_id").read().strip()
request = {"manifest_digest": manifest["manifest_digest"], "run_id": manifest["run_id"], "phase": "pre_pr216_merge",
           "evidence_kind": "node_observation", "policy_variant": "baseline", "scenario_kind": "threshold_boundary",
           "scenario_id": "controlled-single-validator-history", "pair_id": "controlled-pair", "seed": "1", "segment": "1", "iteration": "1",
           "protocol_context": {}, "metadata_availability": "complete",
           "threshold_inputs": {"q": "1", "S": "1", "agreeing_stake": "1", "n": "1", "d": "1"},
           "members": members, "inputs": inputs, "fault_schedule": [],
           "observation_deadline": {"clock_id": "linux-monotonic:" + boot, "monotonic_ns": str(time.monotonic_ns() + 200_000_000_000)}}
envelope = {"schema_version": 1, "execution_nonce": hashlib.sha256(b"controlled-nonce-" + rev.encode()).hexdigest(), "manifest": manifest,
            "request": request, "operations": operations, "input_root": root, "output_root": "/env/run/live/output", "timeout_ms": 180000}
open("/env/run/live/envelope.json", "w").write(json.dumps(envelope, sort_keys=True, separators=(",", ":")) + "\n")
print("live envelope: %d operations" % len(operations))
PY
/t/release/casper-authority-live --request "$LIVE/envelope.json" --output "$LIVE/output" > "$LIVE/live.stdout.json" 2> "$LIVE/live.stderr.txt"
echo "live executor exit $?"
jq -c '{status, receipt_count, captures: (.captures|length), errors}' "$LIVE/live.stdout.json" 2>/dev/null || head -c 400 "$LIVE/live.stdout.json" "$LIVE/live.stderr.txt"
for r in "$LIVE"/output/receipts/*.json; do jq -c '{step: .step.index, operation: .step.operation, member: .step.member.member_id, status}' "$r"; done
for c in $(jq -r '.captures[].path' "$LIVE/live.stdout.json" 2>/dev/null); do
  d=$(dirname "$LIVE/output/$c")
  echo "$c: $(jq -c '.result.value | {held_blocks, head: .fork_choice.value.bounded.value.head[0:12], target_display: .targets[0].display_inputs.availability}' "$d/response.json")"
done
kill "$OWNER_PID" 2>/dev/null; wait "$OWNER_PID" 2>/dev/null
cp -a /run/soak/owner "$RUN/owner-live-output" 2>/dev/null
