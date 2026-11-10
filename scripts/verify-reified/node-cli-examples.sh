#!/usr/bin/env bash
# node-cli-examples.sh: the gate RF10 of Reified RSpaces. The node command line runs every
# example (decisions 13, 86 and 136; docs/rspace/reified-rspaces/verification.md, section 35.7).
#
# What the gate checks
#   Each example of examples/reified_rspaces/MANIFEST.toml runs on a standalone node, in the
#   order of the manifest, with one block for each file of the example:
#     node deploy      signs the file as an owner-direct offer and sends it to the node;
#     node propose     makes the block that holds that deploy;
#     node show-block  gives the outcome of the deploy: its signature, its cost and its
#                      `errored` flag.
#   The gRPC dry run casper.v1.DeployService/exploratoryDeploy then reads the result channels of
#   the example at the post-state of its last block, and the gate compares the datums with the
#   expected values of the examples harness (rholang/tests/reified_examples_execute.rs). For an
#   example that shows an abort, the dry run of its last file at the pre-state of its block
#   gives the error code. The script reads the table of the harness at run time and checks it
#   against the manifest, so the harness and this gate have one source of expected values.
#
# Why not `node eval`
#   `node eval` sends the file to the REPL service, which evaluates it outside native execution.
#   There every Reified space call fails with `reified:RuntimeUnavailable`
#   (rholang/src/rust/interpreter/reified/runtime.rs), and no block records the deploy
#   (decision 136). A block is the path of a real deploy.
#
# Two phases
#   A  A default build of node runs every example without an external service.
#   B  A build with the feature rholang/chromadb runs the examples that call rho:vectordb:embed or
#      rho:vectordb:ann, with a disposable ChromaDB server of the image that docker/shard.yml pins
#      (as scripts/verify-reified/chroma-live.sh). A node embeds with its SBERT model, and the
#      harness installs a fake model, so the gate checks the embedding by its shape:
#      [true, <MODEL_DIMENSIONS Float32 values>, MODEL] (decision 136).
#
# The reads
#   A datum is the argument list of one send. The dry run reads the datums of each result channel
#   with persistent receives of one to four arguments (or more, when an expected datum has more),
#   so a datum of an unexpected arity shows as a difference. Each datum comes back tagged with its
#   channel and its arity. The expected literals of the harness are sent in the same dry run, so
#   the node normalizes both sides. The comparison is the equality of the two multisets of
#   protobuf Pars in JSON form, without the field locallyFree, which the harness comparison of
#   Pars also ignores.
#
# Usage: bash scripts/verify-reified/node-cli-examples.sh [--plan-only] <out-dir>
#   --plan-only  writes and checks the plan of the run (plan.jsonl), and stops.
# Environment:
#   LIBTORCH                libtorch 2.4.0, for the build of phase B (as check-reified-rspaces-ALL.sh)
#   STANDALONE_PRIVATE_KEY  the bonded validator key of run-local/genesis/standalone. Without
#                           it, the script reads the key of docker/.env.example.
#   RF10_WORK               the work folder (binaries and node data); default target/rf10-work
#   RF10_PORT_BASE          the lowest base of the 6 node ports to try; default 29400
# Output in <out-dir>: steps.txt, manifest.tsv, table.jsonl, plan.jsonl, records.txt,
#   build-<phase>.log, node-<phase>.log and, for each failed example, fail-<example>.txt. The
#   last line of steps.txt is "node-cli examples: PASS=<p> FAIL=<f> (manifest <n>); control:
#   <verdict>". The script exits 0 only when every example of the manifest passes and the
#   negative control fails its comparison.
set -uo pipefail
export LC_ALL=C

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PLAN_ONLY=0
if [[ "${1:-}" == "--plan-only" ]]; then PLAN_ONLY=1; shift; fi
if [[ $# -ne 1 ]]; then
  echo "usage: node-cli-examples.sh [--plan-only] <out-dir>" >&2
  exit 2
fi
mkdir -p "$1" || exit 2
OUT=$(cd "$1" && pwd)
: > "$OUT/steps.txt"
EX=$ROOT/examples/reified_rspaces
MANIFEST=$EX/MANIFEST.toml
TABLE=$ROOT/rholang/tests/reified_examples_execute.rs
SERVICES=$ROOT/rholang/src/rust/interpreter/reified/vectordb_services.rs
WORK=${RF10_WORK:-$ROOT/target/rf10-work}

say() { printf '%s\n' "$*" | tee -a "$OUT/steps.txt"; }
die() { say "node-cli examples: $*"; exit 2; }

for tool in awk jq grep sed; do
  command -v "$tool" >/dev/null || die "the tool $tool is missing"
done

# ---------------------------------------------------------------------------------------------
# 1. The plan: the manifest, the table of the harness, and their join.
# ---------------------------------------------------------------------------------------------

# manifest.tsv: one line for each example, "<name> TAB <file> [<file> ...]", in manifest order.
awk '
  function flush() {
    if (name == "") return
    if (files == "") { print "the manifest example " name " has no file" > "/dev/stderr"; bad = 1 }
    print name "\t" files
    name = ""; files = ""
  }
  /^\[\[example\]\]$/ { flush(); next }
  /^name = "[^"]*"$/ { v = $0; sub(/^name = "/, "", v); sub(/"$/, "", v); name = v; next }
  /^file = "[^"]*"$/ { v = $0; sub(/^file = "/, "", v); sub(/"$/, "", v); files = v; next }
  /^files = \[ *"[^]]*" *\]$/ {
    v = $0; sub(/^files = \[ *"/, "", v); sub(/" *\]$/, "", v); gsub(/" *, *"/, " ", v)
    files = v; next
  }
  END { flush(); exit bad }
' "$MANIFEST" > "$OUT/manifest.tsv" || die "the manifest $MANIFEST does not parse"

# table.jsonl: one JSON object for each `const <NAME>: Example = Example { ... };` item of the
# harness, from the first such item up to `macro_rules! example_tests`. The tokens of that
# region are Rust string literals (raw `r#"..."#` and plain `"..."`), words (identifiers,
# numbers and paths such as Services::None) and single punctuation characters. A line comment
# is skipped. The parser accepts exactly the field forms of the table and fails on any other.
# Each expected datum carries its arity: the count of its top-level arguments.
awk '
  function fail(message) { print "table: " message > "/dev/stderr"; failed = 1; exit 1 }
  function push(token) { count++; T[count] = token }
  function jstr(s,    i, c, r) {
    r = ""
    for (i = 1; i <= length(s); i++) {
      c = substr(s, i, 1)
      if (c == "\\") r = r "\\\\"
      else if (c == "\"") r = r "\\\""
      else if (c == "\n") r = r "\\n"
      else if (c == "\t") r = r "\\t"
      else if (c < " ") fail("a control character in " s)
      else r = r c
    }
    return "\"" r "\""
  }
  function arity(s,    i, c, depth, commas, quote, blank) {
    depth = 0; commas = 0; quote = ""; blank = 1
    for (i = 1; i <= length(s); i++) {
      c = substr(s, i, 1)
      if (c != " " && c != "\t" && c != "\n") blank = 0
      if (quote != "") {
        if (c == "\\") i++
        else if (c == quote) quote = ""
        continue
      }
      if (c == "\"" || c == "`") quote = c
      else if (c == "(" || c == "[" || c == "{") depth++
      else if (c == ")" || c == "]" || c == "}") depth--
      else if (c == "," && depth == 0) commas++
    }
    if (depth != 0 || quote != "") fail("an unbalanced literal " s)
    return blank ? 0 : commas + 1
  }
  function tokenize(    n, i, c, d, j, rest, s) {
    n = length(text); i = 1; count = 0
    while (i <= n) {
      c = substr(text, i, 1)
      if (c == " " || c == "\t" || c == "\n" || c == "\r") { i++; continue }
      if (substr(text, i, 2) == "//") {
        j = index(substr(text, i), "\n")
        if (j == 0) break
        i += j
        continue
      }
      if (substr(text, i, 3) == "r#\"") {
        rest = substr(text, i + 3)
        j = index(rest, "\"#")
        if (j == 0) fail("an unterminated raw string")
        push("S" substr(rest, 1, j - 1))
        i += j + 4
        continue
      }
      if (c == "\"") {
        s = ""; i++
        while (1) {
          if (i > n) fail("an unterminated string")
          c = substr(text, i, 1)
          if (c == "\"") { i++; break }
          if (c == "\\") {
            d = substr(text, i + 1, 1)
            if (d == "\"" || d == "\\") { s = s d; i += 2; continue }
            fail("the escape \\" d " in a string")
          }
          s = s c; i++
        }
        push("S" s)
        continue
      }
      if (c ~ /[A-Za-z0-9_]/) {
        j = i
        while (j <= n) {
          if (substr(text, j, 1) ~ /[A-Za-z0-9_.]/) { j++; continue }
          if (substr(text, j, 2) == "::") { j += 2; continue }
          break
        }
        push("W" substr(text, i, j - i))
        i = j
        continue
      }
      push("P" c); i++
    }
  }
  function want(token) {
    if (T[p] != token) fail("expected " token " but found " T[p] " at token " p " (example " name ")")
    p++
  }
  function str() {
    if (substr(T[p], 1, 1) != "S") fail("expected a string but found " T[p] " (example " name ")")
    p++
    return substr(T[p - 1], 2)
  }
  function datums(group) {
    nd[group] = 0
    want("P[")
    while (T[p] != "P]") {
      nd[group]++
      dat[group, nd[group]] = str()
      if (T[p] == "P,") p++
    }
    want("P]")
  }
  function expectation() {
    if (T[p] == "Wvalues") {
      kind = "values"; p++
      want("P!"); want("P(")
      while (T[p] != "P)") {
        nch++
        ch[nch] = str()
        want("P="); want("P>")
        datums(nch)
        if (T[p] == "P,") p++
      }
      want("P)")
    } else if (T[p] == "WExpect::Error") {
      kind = "error"; p++
      want("P("); code = str(); want("P)")
    } else if (T[p] == "WExpect::OneOf") {
      kind = "oneof"; p++
      want("P("); nch = 1; ch[1] = str(); want("P,"); want("P&")
      datums(1)
      want("P)")
    } else fail("the expectation " T[p] " (example " name ")")
  }
  function field(    f, depth) {
    f = T[p]; p++
    want("P:")
    if (f == "Wname") name = str()
    else if (f == "Wdeploys") {
      want("P&"); want("P[")
      while (T[p] != "P]") {
        want("Wdeploy"); want("P!"); want("P(")
        nfiles++
        files[nfiles] = str()
        want("P)")
        if (T[p] == "P,") p++
      }
      want("P]")
    } else if (f == "Wservices") {
      if (T[p] == "WServices::None") services = "None"
      else if (T[p] == "WServices::Embedder") services = "Embedder"
      else if (T[p] == "WServices::Mirror") services = "Mirror"
      else fail("the services " T[p] " (example " name ")")
      p++
      if (services == "Mirror") {
        want("P{"); depth = 1
        while (depth > 0) {
          if (p > count) fail("an unterminated Services::Mirror")
          if (T[p] == "P{") depth++
          else if (T[p] == "P}") depth--
          p++
        }
      }
    } else if (f == "Wexpect") expectation()
    else fail("the field " f " (example " name ")")
    if (T[p] == "P,") p++
  }
  function emit(    i, j, line) {
    if (name == "" || nfiles == 0 || services == "" || kind == "") fail("an incomplete example " name)
    line = "{\"name\":" jstr(name) ",\"files\":["
    for (i = 1; i <= nfiles; i++) line = line (i > 1 ? "," : "") jstr(files[i])
    line = line "],\"services\":" jstr(services) ",\"kind\":" jstr(kind) ",\"code\":" jstr(code) ",\"channels\":["
    for (i = 1; i <= nch; i++) {
      line = line (i > 1 ? "," : "") "{\"channel\":" jstr(ch[i]) ",\"datums\":["
      for (j = 1; j <= nd[i]; j++) {
        line = line (j > 1 ? "," : "") "{\"text\":" jstr(dat[i, j]) ",\"arity\":" arity(dat[i, j]) "}"
      }
      line = line "]}"
    }
    print line "]}"
  }
  /^const [A-Z0-9_]+: Example = Example \{$/ { inside = 1 }
  /^macro_rules! example_tests/ { inside = 0 }
  inside { text = text $0 "\n" }
  END {
    if (failed) exit 1
    tokenize()
    p = 1
    while (p <= count) {
      want("Wconst")
      if (substr(T[p], 1, 1) != "W") fail("a const without a name at token " p)
      p++
      want("P:"); want("WExample"); want("P="); want("WExample"); want("P{")
      name = ""; nfiles = 0; services = ""; kind = ""; code = ""; nch = 0
      delete files; delete ch; delete nd; delete dat
      while (T[p] != "P}") {
        if (p > count) fail("an unterminated example " name)
        field()
      }
      want("P}"); want("P;")
      emit()
    }
  }
' "$TABLE" > "$OUT/table.jsonl" || die "the table of $TABLE does not parse"

# plan.jsonl: the examples in manifest order, each with its phase. The join checks that the
# manifest and the table name the same examples, each once, with the same files in the same
# order, and that the expectations have the shapes that the reads support.
jq -nc --slurpfile table "$OUT/table.jsonl" --rawfile manifest "$OUT/manifest.tsv" '
  def problem($message): error($message);
  ($manifest | split("\n") | map(select(length > 0) | split("\t")
    | {name: .[0], files: (.[1] | split(" "))})) as $m
  | ($m | map(.name)) as $names
  | ($table | map(.name)) as $tnames
  | if ($names | unique | length) != ($names | length) then problem("a manifest name repeats")
    elif ($tnames | unique | length) != ($tnames | length) then problem("a table name repeats")
    elif ($names | sort) != ($tnames | sort) then
      problem("the manifest and the table differ: manifest only \($names - $tnames), table only \($tnames - $names)")
    else . end
  | ($table | map({key: .name, value: .}) | from_entries) as $t
  | $m[]
  | . as $entry
  | $t[$entry.name] as $row
  | if $row.files != $entry.files then
      problem("\($entry.name): the manifest files \($entry.files) differ from the table files \($row.files)")
    elif ($row.kind == "values" or $row.kind == "oneof") and ($row.channels | length) == 0 then
      problem("\($entry.name): no result channel")
    elif $row.kind == "oneof" and ($row.channels[0].datums | length) == 0 then
      problem("\($entry.name): no choice")
    elif any($row.channels[].datums[]; .arity < 1) then
      problem("\($entry.name): an expected datum has no argument")
    elif $row.services != "None" and $row.kind != "values" then
      problem("\($entry.name): a service example must expect values")
    else $row + {phase: (if $row.services == "None" then "A" else "B" end)} end
' > "$OUT/plan.jsonl" 2> "$OUT/plan-error.txt" || die "the plan fails: $(cat "$OUT/plan-error.txt")"
rm -f "$OUT/plan-error.txt"

TOTAL=$(jq -s 'length' "$OUT/plan.jsonl")
say "plan: $TOTAL examples, $(jq -s '[.[].files[]] | length' "$OUT/plan.jsonl") files," \
  "phase A $(jq -s '[.[] | select(.phase == "A")] | length' "$OUT/plan.jsonl")," \
  "phase B $(jq -s '[.[] | select(.phase == "B")] | length' "$OUT/plan.jsonl")"

# Isolation: each result channel belongs to one example, and no file of another example names
# it as @"<channel>". Every example then reads at the block of its own last deploy, so a later
# example cannot change what an earlier example reads.
jq -r '.name as $n | .channels[] | [$n, .channel] | @tsv' "$OUT/plan.jsonl" > "$OUT/channels.tsv"
duplicates=$(cut -f2 "$OUT/channels.tsv" | sort | uniq -d)
[[ -z "$duplicates" ]] || die "result channels of two examples: $duplicates"
while IFS=$'\t' read -r example channel; do
  own=$(jq -r --arg n "$example" 'select(.name == $n) | .files[]' "$OUT/plan.jsonl")
  while IFS= read -r named; do
    grep -qxF "$(basename "$named")" <<<"$own" \
      || die "the file $(basename "$named") of another example names the result channel $channel of $example"
  done < <(grep -lF "@\"$channel\"" "$EX"/*.rho)
done < "$OUT/channels.tsv"
say "isolation: $(wc -l < "$OUT/channels.tsv") result channels, each named only by its own example"

if [[ $PLAN_ONLY -eq 1 ]]; then
  say "plan only: $OUT/plan.jsonl"
  exit 0
fi

# ---------------------------------------------------------------------------------------------
# 2. The tools, the key, and the builds.
# ---------------------------------------------------------------------------------------------

for tool in cargo grpcurl curl ss install; do
  command -v "$tool" >/dev/null || die "the tool $tool is missing"
done
NEED_B=$(jq -s '[.[] | select(.phase == "B")] | length' "$OUT/plan.jsonl")
if [[ $NEED_B -gt 0 ]]; then
  command -v docker >/dev/null || die "the tool docker is missing (phase B)"
  [[ -n "${LIBTORCH:-}" && -d "$LIBTORCH/lib" ]] || die "set LIBTORCH to libtorch 2.4.0 (phase B)"
fi

# The key never goes to steps.txt or to a log of this script.
KEY=${STANDALONE_PRIVATE_KEY:-}
if [[ -z "$KEY" ]]; then
  KEY=$(sed -n 's/^STANDALONE_PRIVATE_KEY=\([0-9a-fA-F]\{64\}\)$/\1/p' "$ROOT/docker/.env.example")
fi
[[ "$KEY" =~ ^[0-9a-fA-F]{64}$ ]] || die "no standalone validator key (STANDALONE_PRIVATE_KEY)"
MODEL=$(sed -n 's/^pub const MODEL: &str = "\(.*\)";$/\1/p' "$SERVICES")
DIMENSIONS=$(sed -n 's/^pub const MODEL_DIMENSIONS: usize = \([0-9][0-9]*\);$/\1/p' "$SERVICES")
[[ -n "$MODEL" && -n "$DIMENSIONS" ]] || die "no MODEL or MODEL_DIMENSIONS in $SERVICES"

mkdir -p "$WORK/bin" || die "cannot make $WORK"
touch "$WORK/.rf10-work"
cd "$ROOT" || die "cannot enter $ROOT"

declare -A BINARY=()
build() {  # $1 phase; $2... the extra cargo arguments
  local phase=$1
  shift
  say "build $phase: cargo build --release --locked -p node $*"
  if env BZIP2_NO_PKG_CONFIG=1 \
      LD_LIBRARY_PATH="${LIBTORCH:+$LIBTORCH/lib}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
      cargo build --release --locked -p node "$@" > "$OUT/build-$phase.log" 2>&1 \
    && install -m 755 "$ROOT/target/release/node" "$WORK/bin/node-$phase"; then
    BINARY[$phase]=$WORK/bin/node-$phase
    say "build $phase: ok"
  else
    say "build $phase: FAILED (build-$phase.log)"
  fi
}
# The build of phase B comes first, so target/release/node is the default build at the end.
[[ $NEED_B -gt 0 ]] && build B --features rholang/chromadb
build A

# ---------------------------------------------------------------------------------------------
# 3. The node and the ChromaDB server.
# ---------------------------------------------------------------------------------------------

NODE_PID=""
CHROMA_NAME=""
PORT_INTERNAL=""
FAILED=0
PASSED=0

stop_node() {
  [[ -n "$NODE_PID" ]] || return 0
  kill -TERM "$NODE_PID" 2>/dev/null
  local waited=0
  while kill -0 "$NODE_PID" 2>/dev/null && [[ $waited -lt 60 ]]; do
    sleep 1
    waited=$((waited + 1))
  done
  kill -KILL "$NODE_PID" 2>/dev/null
  wait "$NODE_PID" 2>/dev/null
  NODE_PID=""
}
stop_chroma() {
  [[ -n "$CHROMA_NAME" ]] || return 0
  docker stop "$CHROMA_NAME" >/dev/null 2>&1
  CHROMA_NAME=""
}
# shellcheck disable=SC2329  # the EXIT trap below runs finish
finish() {
  stop_node
  stop_chroma
}
trap finish EXIT

# True when the ports base .. base+5 are free for TCP and for UDP.
ports_free() {
  local used port
  used=$({ ss -H -ltn; ss -H -lun; } | awk '{ n = split($4, part, ":"); print part[n] }' | sort -u)
  for port in $(seq "$1" $(( $1 + 5 ))); do
    grep -qxF "$port" <<<"$used" && return 1
  done
  return 0
}

start_chroma() {
  local image address
  image=$(grep -o 'chromadb/chroma@sha256:[0-9a-f]*' "$ROOT/docker/shard.yml" | head -n 1)
  [[ -n "$image" ]] || { say "phase B: docker/shard.yml pins no ChromaDB image"; return 1; }
  CHROMA_NAME="rrs-rf10-chroma-$$"
  docker run -d --rm --name "$CHROMA_NAME" "$image" >/dev/null \
    || { say "phase B: docker run failed"; CHROMA_NAME=""; return 1; }
  address=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$CHROMA_NAME")
  export CHROMA_ENDPOINT="http://$address:8000"
  CHROMA_DATABASE="rrs_rf10_$$_$(date +%s)"
  export CHROMA_DATABASE
  export RUSTBERT_CACHE="$ROOT/target/rustbert-cache"
  mkdir -p "$RUSTBERT_CACHE"
  local tries
  for tries in $(seq 1 60); do
    curl -sf -m 1 "$CHROMA_ENDPOINT/api/v2/heartbeat" >/dev/null && break
    sleep 1
  done
  curl -sf -m 2 "$CHROMA_ENDPOINT/api/v2/heartbeat" >/dev/null \
    || { say "phase B: the ChromaDB server at $CHROMA_ENDPOINT does not answer after $tries s"; return 1; }
  say "phase B: ChromaDB $image at $CHROMA_ENDPOINT, database $CHROMA_DATABASE"
}

start_node() {  # $1 phase; sets NODE_PID and PORT_INTERNAL
  local phase=$1 base data conf waited=0
  base=${RF10_PORT_BASE:-29400}
  while [[ $base -le 29990 ]] && ! ports_free "$base"; do base=$((base + 10)); done
  [[ $base -le 29990 ]] || { say "phase $phase: no free port base in 29400-29990"; return 1; }
  data=$WORK/data-$phase
  # The data of an earlier run of this script in the same work folder goes, so each phase
  # starts at its genesis.
  [[ -f "$WORK/.rf10-work" && -e "$data" ]] && rm -rf -- "$data"
  mkdir -p "$data/genesis"
  cp "$ROOT/run-local/genesis/standalone/bonds.txt" "$ROOT/run-local/genesis/standalone/wallets.txt" \
    "$data/genesis/" || return 1
  conf=$WORK/standalone-$phase.conf
  awk -v from="run-local/data/standalone" -v to="$data" '
    { out = ""; rest = $0
      while ((i = index(rest, from)) > 0) { out = out substr(rest, 1, i - 1) to; rest = substr(rest, i + length(from)) }
      print out rest }
  ' "$ROOT/run-local/conf/standalone.conf" > "$conf"
  "${BINARY[$phase]}" run --standalone --config-file="$conf" --data-dir="$data" \
    --host=localhost --no-upnp --api-host=127.0.0.1 --heartbeat-disabled \
    --validator-private-key="$KEY" \
    --protocol-port="$base" --api-port-grpc-external=$((base + 1)) \
    --api-port-grpc-internal=$((base + 2)) --api-port-http=$((base + 3)) \
    --discovery-port=$((base + 4)) --api-port-admin-http=$((base + 5)) \
    > "$OUT/node-$phase.log" 2>&1 &
  NODE_PID=$!
  PORT_INTERNAL=$((base + 2))
  until curl -fsS -m 2 "http://127.0.0.1:$((base + 3))/api/ready" >/dev/null 2>&1; do
    if ! kill -0 "$NODE_PID" 2>/dev/null; then
      wait "$NODE_PID" 2>/dev/null
      NODE_PID=""
      say "phase $phase: the node exited before it was ready (node-$phase.log)"
      return 1
    fi
    if [[ $waited -ge 600 ]]; then
      say "phase $phase: the node is not ready after 600 s (node-$phase.log)"
      return 1
    fi
    sleep 2
    waited=$((waited + 2))
  done
  say "phase $phase: node ready after $waited s on ports $base-$((base + 5))"
}

# ---------------------------------------------------------------------------------------------
# 4. One file: deploy, propose, show-block.
# ---------------------------------------------------------------------------------------------

AFTER=0
cli() { "${BINARY[$PHASE]}" --grpc-host 127.0.0.1 -p "$PORT_INTERNAL" "$@"; }

# run_file <file>: sets HASH, ERRORED and COST, or NOTE on a failure (return 1).
run_file() {
  local file=$1 out id block number
  HASH=""; ERRORED=""; COST=""; NOTE=""
  out=$(cli deploy 1000000 1 "$AFTER" "$KEY" /dev/null "$EX/$file" root 2>&1)
  id=$(sed -n 's/^DeployId is: *\([0-9a-fA-F][0-9a-fA-F]*\) *$/\1/p' <<<"$out" | tr 'A-F' 'a-f')
  [[ -n "$id" ]] || { NOTE="node deploy $file: $out"; return 1; }
  out=$(cli propose 2>&1)
  HASH=$(grep -oE 'Block [0-9a-f]{64} created and added' <<<"$out" | head -n 1 | cut -d' ' -f2)
  [[ -n "$HASH" ]] || { NOTE="node propose after $file: $out"; return 1; }
  block=$(cli show-block "$HASH" 2>&1)
  if [[ $(grep -cE '^ *errored: (true|false),$' <<<"$block") -ne 1 ]]; then
    NOTE="node show-block $HASH: not exactly one deploy: $block"
    return 1
  fi
  # The id is hexadecimal (the sed pattern above), so it is safe in the regular expression.
  if ! grep -qiE "^ *sig: \"$id\",$" <<<"$block"; then
    NOTE="node show-block $HASH: no deploy with the signature $id: $block"
    return 1
  fi
  ERRORED=$(sed -n 's/^ *errored: \(true\|false\),$/\1/p' <<<"$block")
  COST=$(sed -n 's/^ *cost: \([0-9][0-9]*\),$/\1/p' <<<"$block" | head -n 1)
  number=$(sed -n 's/^ *block_number: \([0-9][0-9]*\),$/\1/p' <<<"$block" | head -n 1)
  [[ -n "$number" ]] || { NOTE="node show-block $HASH: no block_number: $block"; return 1; }
  AFTER=$number
}

# explore <term> <block hash> <true|false>: the response of the dry run, as JSON. The node
# answers UNAVAILABLE when its dry-run slots are busy, so one retry follows after 2 s.
explore() {
  local request response tries
  request=$(jq -nc --arg t "$1" --arg h "$2" --argjson p "$3" '{term: $t, blockHash: $h, usePreStateHash: $p}')
  for tries in 1 2; do
    response=$(grpcurl -plaintext -max-time 120 -d @ "127.0.0.1:$PORT_INTERNAL" \
      casper.v1.DeployService/exploratoryDeploy <<<"$request" 2>&1)
    grep -q 'Code: Unavailable' <<<"$response" || break
    [[ $tries -eq 1 ]] && sleep 2
  done
  printf '%s' "$response"
}

# The read term of an example. Mode "values" adds the expected datums (tag "e"), mode "oneof"
# adds the choices (tags "c1", "c2", ...), mode "shape" adds nothing, and mode "control" adds
# one more expected datum that the example does not send.
read_term() {  # $1 the plan object, $2 the mode
  jq -r --arg mode "$2" '
    def literal($k; $text): if $k == 1 then $text else "[" + $text + "]" end;
    def reads($i; $channel; $kmax):
      range(1; $kmax + 1) as $k
      | ([range(0; $k) | "@rf10x\(.)"] | join(", ")) as $pattern
      | (if $k == 1 then "rf10x0" else "[" + ([range(0; $k) | "rf10x\(.)"] | join(", ")) + "]" end) as $bound
      | "for (\($pattern) <= @\($channel | tojson)) { rf10Result!([\"a\", \($i), \($k), \($bound)]) }";
    def sends($i; $datums):
      $datums | to_entries[]
      | (if $mode == "oneof" then "c\(.key + 1)" else "e" end) as $tag
      | "rf10Result!([\"\($tag)\", \($i), \(.value.arity), \(literal(.value.arity; .value.text))])";
    .channels as $channels
    | ([$channels[].datums[].arity] + [4] | max) as $kmax
    | [range(0; $channels | length) as $n
       | ($n + 1) as $i
       | reads($i; $channels[$n].channel; $kmax),
         (if $mode == "shape" then empty else sends($i; $channels[$n].datums) end),
         (if $mode == "control" and $n == 0 then "rf10Result!([\"e\", 1, 1, \"rf10.control\"])" else empty end)]
    | "new rf10Result in {\n  " + join(" |\n  ") + "\n}"
  ' <<<"$1"
}

# The records of a dry-run response: each datum on rf10Result is [tag, channel, arity, value].
# shellcheck disable=SC2016  # a jq program: $ps and $r are jq variables
RECORDS='
  def canon: walk(if type == "object" then del(.locallyFree) | to_entries | sort_by(.key) | from_entries else . end) | tojson;
  def records: [.result.postBlockData[]? | .exprs[0].eListBody.ps as $ps
    | {t: $ps[0].exprs[0].gString, i: $ps[1].exprs[0].gInt, k: $ps[2].exprs[0].gInt, raw: $ps[3], v: ($ps[3] | canon)}];
'
values_match() {  # stdin: the response
  jq -e "$RECORDS"'
    has("result") and (records as $r
      | ($r | all(.t == "a" or .t == "e"))
        and (([$r[] | select(.t == "a") | [.i, .k, .v]] | sort) == ([$r[] | select(.t == "e") | [.i, .k, .v]] | sort)))
  ' >/dev/null 2>&1
}
oneof_choice() {  # stdin: the response; prints the number of the matched choice
  jq -er "$RECORDS"'
    if has("result") | not then empty else
      records as $r
      | [$r[] | select(.t == "a")] as $a
      | if ($a | length) != 1 or ($r | any(.t != "a" and (.t | startswith("c") | not))) then empty
        else [$r[] | select((.t | startswith("c")) and .i == $a[0].i and .k == $a[0].k and .v == $a[0].v) | .t[1:]] | first // empty
        end
    end
  ' 2>/dev/null
}
shape_match() {  # stdin: the response
  jq -e --arg model "$MODEL" --argjson dimensions "$DIMENSIONS" "$RECORDS"'
    has("result") and (records as $r
      | ($r | length) == 1 and $r[0].t == "a" and $r[0].k == "1"
        and ($r[0].raw.exprs[0].eListBody.ps as $x
          | ($x | length) == 3
            and $x[0].exprs[0].gBool == true
            and ($x[1].exprs[0].eListBody.ps | length) == $dimensions
            and ($x[1].exprs[0].eListBody.ps | all(.exprs[0] | has("gFloat32")))
            and $x[2].exprs[0].gString == $model))
  ' >/dev/null 2>&1
}

# The mirror publishes after each commit through an asynchronous writer. Before the last deploy
# of a mirror example, wait until the record count of the rrs- collection of the database is
# nonzero and equal on two polls 1 s apart, for at most 60 s. The value check is the verdict.
mirror_settle() {
  local base="$CHROMA_ENDPOINT/api/v2/tenants/${CHROMA_TENANT:-default_tenant}/databases/$CHROMA_DATABASE"
  local id count last="" waited=0
  while [[ $waited -lt 60 ]]; do
    id=$(curl -sf -m 2 "$base/collections" | jq -r '[.[] | select(.name | startswith("rrs-"))][0].id // empty')
    count=""
    [[ -n "$id" ]] && count=$(curl -sf -m 2 "$base/collections/$id/count")
    if [[ -n "$count" && "$count" != "0" && "$count" == "$last" ]]; then
      say "phase B: the mirror holds $count records after $waited s"
      return 0
    fi
    last=$count
    sleep 1
    waited=$((waited + 1))
  done
  say "phase B: the mirror count did not settle in 60 s (last count: ${last:-none})"
}

# ---------------------------------------------------------------------------------------------
# 5. One example.
# ---------------------------------------------------------------------------------------------

record() {  # $1 name, $2 verdict line
  printf '%s\n' "$2" >> "$OUT/records.txt"
  case "$2" in
    *"; PASS") PASSED=$((PASSED + 1)) ;;
    *) FAILED=$((FAILED + 1)) ;;
  esac
}

run_example() {  # $1 the plan object
  local plan=$1 name kind services file response code choice term outcome="" verdict="PASS"
  local -a files=() hashes=() errored=() costs=()
  name=$(jq -r .name <<<"$plan")
  kind=$(jq -r .kind <<<"$plan")
  services=$(jq -r .services <<<"$plan")
  mapfile -t files < <(jq -r '.files[]' <<<"$plan")
  local index last=$(( ${#files[@]} - 1 ))
  for index in "${!files[@]}"; do
    file=${files[$index]}
    if [[ "$services" == "Mirror" && $index -eq $last && $index -gt 0 ]]; then mirror_settle; fi
    if ! run_file "$file"; then
      verdict="FAIL: $NOTE"
      break
    fi
    hashes+=("${HASH:0:12}")
    errored+=("$ERRORED")
    costs+=("$COST")
    if [[ $index -lt $last && "$ERRORED" != "false" ]]; then
      verdict="FAIL: the deploy of $file errored"
      break
    fi
  done
  if [[ "$verdict" == "PASS" ]]; then
    case "$kind" in
      error)
        code=$(jq -r .code <<<"$plan")
        if [[ "$ERRORED" != "true" ]]; then
          verdict="FAIL: the last deploy did not error (expected $code)"
        else
          response=$(explore "$(cat "$EX/${files[$last]}")" "$HASH" true)
          outcome=$(jq -r '.error.messages[0] // empty' <<<"$response" 2>/dev/null \
            | grep -oE 'reified:[A-Za-z0-9_]+' | head -n 1)
          outcome=${outcome#reified:}
          [[ "$outcome" == "$code" ]] || verdict="FAIL: the dry run of ${files[$last]} gives '${outcome:-no code}', expected $code"
          printf '%s\n' "$response" > "$OUT/response-$name.json"
          outcome="error $code"
        fi
        ;;
      values | oneof)
        if [[ "$ERRORED" != "false" ]]; then
          verdict="FAIL: the deploy of ${files[$last]} errored"
        elif [[ "$services" == "Embedder" ]]; then
          term=$(read_term "$plan" shape)
          response=$(explore "$term" "$HASH" false)
          printf '%s\n' "$response" > "$OUT/response-$name.json"
          outcome="$(jq -r '.channels[0].channel' <<<"$plan") = [true, $DIMENSIONS x Float32, \"$MODEL\"]"
          shape_match <<<"$response" || verdict="FAIL: the embedding has not the shape $outcome"
        elif [[ "$kind" == "values" ]]; then
          term=$(read_term "$plan" values)
          response=$(explore "$term" "$HASH" false)
          printf '%s\n' "$response" > "$OUT/response-$name.json"
          outcome=$(jq -r '[.channels[] | "\(.channel) = [\([.datums[].text] | join(", "))]"] | join("; ")' <<<"$plan")
          values_match <<<"$response" || verdict="FAIL: the datums differ from $outcome"
        else
          term=$(read_term "$plan" oneof)
          response=$(explore "$term" "$HASH" false)
          printf '%s\n' "$response" > "$OUT/response-$name.json"
          choice=$(oneof_choice <<<"$response")
          if [[ -n "$choice" ]]; then
            outcome=$(jq -r --argjson c "$choice" '"\(.channels[0].channel) = \(.channels[0].datums[$c - 1].text) (one of \(.channels[0].datums | length))"' <<<"$plan")
          else
            verdict="FAIL: not exactly one datum that equals a choice"
          fi
        fi
        ;;
    esac
  fi
  local line
  line="example $name: files [$(IFS=,; printf '%s' "${files[*]}" | sed 's/,/, /g')]; blocks [$(IFS=,; printf '%s' "${hashes[*]}" | sed 's/,/, /g')]; errored [$(IFS=,; printf '%s' "${errored[*]}" | sed 's/,/, /g')]; phlo [$(IFS=,; printf '%s' "${costs[*]}" | sed 's/,/, /g')]; ${outcome:-no outcome}; $verdict"
  record "$name" "$line"
  if [[ "$verdict" != "PASS" ]]; then
    {
      printf '%s\n\nplan:\n%s\n' "$line" "$plan"
      [[ -n "${term:-}" ]] && printf '\nread term:\n%s\n' "$term"
      [[ -n "${response:-}" ]] && printf '\nresponse:\n%s\n' "$response"
    } > "$OUT/fail-$name.txt"
  fi
  say "$line"
}

# The negative control (as examples_reject_a_changed_expectation of the harness): the read of
# the first values example of phase A, with one more expected datum, must fail the comparison.
CONTROL="not run"
control() {  # $1 the plan object, $2 the block hash of its last deploy
  local response
  response=$(explore "$(read_term "$1" control)" "$2" false)
  if values_match <<<"$response"; then
    CONTROL="FAIL: a changed expectation passes the comparison"
  elif jq -e 'has("result")' <<<"$response" >/dev/null 2>&1; then
    CONTROL="PASS: a changed expectation fails the comparison"
  else
    CONTROL="FAIL: the control read got no result: $response"
  fi
  say "control ($(jq -r .name <<<"$1")): $CONTROL"
}

# ---------------------------------------------------------------------------------------------
# 6. The phases.
# ---------------------------------------------------------------------------------------------

: > "$OUT/records.txt"
for PHASE in A B; do
  mapfile -t PLANS < <(jq -c --arg phase "$PHASE" 'select(.phase == $phase)' "$OUT/plan.jsonl")
  [[ ${#PLANS[@]} -gt 0 ]] || continue
  # The binary of phase B links libtorch, so the node and each command line call load it.
  if [[ "$PHASE" == "B" ]]; then
    export LD_LIBRARY_PATH="$LIBTORCH/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  fi
  ready=1
  if [[ -z "${BINARY[$PHASE]:-}" ]]; then
    ready=0
    reason="the build of phase $PHASE failed"
  elif [[ "$PHASE" == "B" ]] && ! start_chroma; then
    ready=0
    reason="the ChromaDB server of phase B did not start"
  elif ! start_node "$PHASE"; then
    ready=0
    reason="the node of phase $PHASE did not start"
  fi
  if [[ $ready -eq 0 ]]; then
    for plan in "${PLANS[@]}"; do
      record "" "example $(jq -r .name <<<"$plan"): FAIL: $reason"
    done
    say "phase $PHASE: $reason; ${#PLANS[@]} examples fail"
    stop_node
    stop_chroma
    continue
  fi
  AFTER=0
  for plan in "${PLANS[@]}"; do
    run_example "$plan"
    if [[ "$PHASE" == "A" && "$CONTROL" == "not run" && "$(jq -r .kind <<<"$plan")" == "values" ]] \
      && [[ "$(tail -n 1 "$OUT/records.txt")" == *"; PASS" ]]; then
      control "$plan" "$HASH"
    fi
  done
  stop_node
  stop_chroma
done

summary="node-cli examples: PASS=$PASSED FAIL=$FAILED (manifest $TOTAL); control: $CONTROL"
if [[ $PASSED -eq $TOTAL && $FAILED -eq 0 && "$CONTROL" == PASS* ]]; then
  # Only the work folder of this script goes, and only after a full pass.
  [[ -f "$WORK/.rf10-work" ]] && rm -rf -- "$WORK"
  say "$summary"
  exit 0
fi
say "the work folder stays for inspection: $WORK"
say "$summary"
exit 1
