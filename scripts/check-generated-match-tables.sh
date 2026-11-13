#!/usr/bin/env bash
# Checks that the models build script regenerates identical sources.
#
# models/build.rs compiles the repository's .proto files with protoc through
# tonic-prost-build and then runs the schema pass of models/codegen/schema.rs;
# both write into the build script's OUT_DIR. This script builds models twice
# in release, each time into a fresh target directory, and compares every file
# of the two OUT_DIRs by SHA-256.
#
#   scripts/check-generated-match-tables.sh
#       exits 0 when the two builds generate identical files; the last line
#       gives their number
#   scripts/check-generated-match-tables.sh --self-test
#       the comparison must accept an unchanged copy of an OUT_DIR and refuse
#       a copy with one changed byte, naming the changed file
#
# The two-build run writes provenance.tsv for the first build, with the
# columns kind, name, bytes and sha256:
#   commit        the checked-out commit
#   tool          the rustc and the protoc that the build runs, by version
#   generated     each file of the OUT_DIR
#   input         models/build.rs, Cargo.lock and each file that the build
#                 script declares with cargo:rerun-if-changed (the .proto
#                 files and models/codegen/schema.rs), by repository path
#   protoc-input  each compiled .proto file that protoc took from its own
#                 include directory, by its path there
# Each file that the descriptor set in the OUT_DIR names must be one of the
# declared .proto files or a file of protoc's include directory; any other
# file is refused, so every compiled .proto file has a row.
#
# Evidence goes to target/verification/generated-match-tables. The build
# directories are removed after the comparison; reference-out-dir, a copy of
# the first OUT_DIR, is kept for the self-test, which otherwise makes it with
# one fresh build.
set -euo pipefail

name=generated-match-tables
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
evidence_dir="$repo_root/target/verification/$name"
reference_dir="$evidence_dir/reference-out-dir"
descriptor_set=rhoapi_descriptor.bin

# Builds models in release into the fresh target directory $evidence_dir/$1 and
# prints the OUT_DIR of its build script.
fresh_out_dir() {
  local target_dir="$evidence_dir/$1"
  rm -rf "$target_dir"
  mkdir -p "$target_dir"
  (cd "$repo_root" &&
    cargo build --release --locked -p models --target-dir "$target_dir" --message-format=json) \
    > "$target_dir/build-messages.json"
  python3 -I - "$target_dir/build-messages.json" <<'PY'
import json
import re
import sys

out_dirs = []
with open(sys.argv[1], encoding="utf-8") as messages:
    for line in messages:
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") == "build-script-executed" and re.search(
            r"/build/models-[0-9a-f]+/out$", message.get("out_dir", "")
        ):
            out_dirs.append(message["out_dir"])
if len(out_dirs) != 1:
    sys.exit(f"expected one build-script-executed message of models, found {len(out_dirs)}")
print(out_dirs[0])
PY
}

# Prints "<sha256>  <path>" for every file under $1, sorted by path.
digests() {
  (cd "$1" && find . -type f -printf '%P\0' | LC_ALL=C sort -z | xargs -0 -r sha256sum)
}

# Compares the output directories $1 and $2 by SHA-256. Returns 0 and prints
# the number of files when both hold the same files with the same contents;
# returns 1 and names each file that differs or exists in only one of them;
# returns 2 when the comparison cannot run.
compare_dirs() {
  local left right
  left=$(digests "$1") || return 2
  right=$(digests "$2") || return 2
  if [[ -z "$left" ]]; then
    echo "$name: $1 holds no file"
    return 2
  fi
  if [[ "$left" == "$right" ]]; then
    echo "$name: $(wc -l <<< "$left") identical files"
    return 0
  fi
  { diff <(printf '%s\n' "$left") <(printf '%s\n' "$right") || true; } |
    sed -n "s/^[<>] [0-9a-f]\{64\}  /$name: differs: /p" | LC_ALL=C sort -u
  return 1
}

# Prints the directory that protoc appends to the include directories it is
# given: the first of its binary's directory, that directory's include
# subdirectory and the include subdirectory of its parent that holds
# google/protobuf/descriptor.proto. Prints nothing when none does.
protoc_include_dir() {
  local binary directory candidate
  binary=$(realpath "$(command -v "${PROTOC:-protoc}")")
  directory=$(dirname "$binary")
  for candidate in "$directory" "$directory/include" "$(dirname "$directory")/include"; do
    if [[ -f "$candidate/google/protobuf/descriptor.proto" ]]; then
      echo "$candidate"
      return 0
    fi
  done
}

# Writes provenance.tsv for the build script output directory $1.
write_provenance() {
  local out_dir=$1 protoc_include
  protoc_include=$(protoc_include_dir)
  {
    printf 'kind\tname\tbytes\tsha256\n'
    printf 'commit\t%s\t-\t-\n' "$(git -C "$repo_root" rev-parse HEAD)"
    printf 'tool\t%s\t-\t-\n' "$(cd "$repo_root" && "${RUSTC:-rustc}" --version)"
    printf 'tool\t%s\t-\t-\n' "$("${PROTOC:-protoc}" --version)"
    python3 -I - "$repo_root" "$out_dir" "$protoc_include" "$descriptor_set" <<'PY'
import hashlib
import os
import sys

repo_root, out_dir, protoc_include, descriptor_name = sys.argv[1:5]
prefixes = ("cargo:rerun-if-changed=", "cargo::rerun-if-changed=")


def row(kind, name, path):
    with open(path, "rb") as source:
        data = source.read()
    print(f"{kind}\t{name}\t{len(data)}\t{hashlib.sha256(data).hexdigest()}")


def varint(data, position):
    value = 0
    shift = 0
    while True:
        byte = data[position]
        position += 1
        value |= (byte & 0x7F) << shift
        if byte < 0x80:
            return value, position
        shift += 7


def length_delimited(data, number):
    position = 0
    while position < len(data):
        key, position = varint(data, position)
        wire = key & 7
        if wire == 0:
            _, position = varint(data, position)
        elif wire == 1:
            position += 8
        elif wire == 5:
            position += 4
        elif wire == 2:
            length, position = varint(data, position)
            if key >> 3 == number:
                yield data[position : position + length]
            position += length
        else:
            sys.exit(f"unsupported protobuf wire type {wire} in the descriptor set")


generated = []
for directory, _, files in os.walk(out_dir):
    for file in files:
        generated.append(os.path.relpath(os.path.join(directory, file), out_dir))
for name in sorted(generated):
    row("generated", name, os.path.join(out_dir, name))

inputs = {"models/build.rs", "Cargo.lock"}
with open(os.path.join(out_dir, os.pardir, "output"), encoding="utf-8") as output:
    for line in output:
        line = line.rstrip("\n")
        for prefix in prefixes:
            if line.startswith(prefix):
                declared = os.path.realpath(line[len(prefix) :])
                if os.path.commonpath([declared, repo_root]) != repo_root:
                    sys.exit(f"the build script declares {declared}, outside the repository")
                if not os.path.isfile(declared):
                    sys.exit(f"the build script declares {declared}, which is not a file")
                inputs.add(os.path.relpath(declared, repo_root))

with open(os.path.join(out_dir, descriptor_name), "rb") as source:
    descriptor_set = source.read()
compiled = set()
for file in length_delimited(descriptor_set, 1):
    names = list(length_delimited(file, 1))
    if len(names) != 1:
        sys.exit(f"a file of the descriptor set carries {len(names)} names")
    compiled.add(names[0].decode("utf-8"))
declared_protos = sorted(path for path in inputs if path.endswith(".proto"))
protoc_inputs = []
for compiled_name in sorted(compiled):
    matches = [path for path in declared_protos if path == compiled_name or path.endswith("/" + compiled_name)]
    if len(matches) > 1:
        sys.exit(f"protoc compiled {compiled_name}, which several declared files end with: {matches}")
    if matches:
        continue
    if protoc_include and os.path.isfile(os.path.join(protoc_include, compiled_name)):
        protoc_inputs.append(compiled_name)
        continue
    sys.exit(f"protoc compiled {compiled_name}, which the build script does not declare and protoc's include directory does not hold")

for path in sorted(inputs):
    row("input", path, os.path.join(repo_root, path))
for compiled_name in protoc_inputs:
    row("protoc-input", compiled_name, os.path.join(protoc_include, compiled_name))
PY
  } > "$evidence_dir/provenance.tsv"
}

check_two_builds() {
  local first second comparison verdict=0
  mkdir -p "$evidence_dir"
  first=$(fresh_out_dir build-a)
  second=$(fresh_out_dir build-b)
  write_provenance "$first"
  comparison=$(compare_dirs "$first" "$second") || verdict=$?
  rm -rf "$reference_dir"
  cp -r "$first" "$reference_dir"
  rm -rf "$evidence_dir/build-a" "$evidence_dir/build-b"
  echo "$name: provenance of the first build: target/verification/$name/provenance.tsv"
  echo "$comparison"
  case $verdict in
    0) ;;
    1)
      echo "$name: the two builds generated different sources" >&2
      exit 1
      ;;
    *)
      echo "$name: the comparison could not run" >&2
      exit 1
      ;;
  esac
}

self_test() {
  local copy="$evidence_dir/self-test-copy" built victim output verdict=0
  mkdir -p "$evidence_dir"
  if [[ ! -d "$reference_dir" ]]; then
    built=$(fresh_out_dir build-self-test)
    cp -r "$built" "$reference_dir"
    rm -rf "$evidence_dir/build-self-test"
  fi
  rm -rf "$copy"
  cp -r "$reference_dir" "$copy"
  output=$(compare_dirs "$reference_dir" "$copy") || verdict=$?
  echo "$output"
  if ((verdict != 0)); then
    echo "$name: self-test failed: the comparison refused an unchanged copy" >&2
    exit 1
  fi
  victim=$(cd "$copy" && find . -type f -size +0 -printf '%P\n' | LC_ALL=C sort | sed -n 1p)
  if [[ -z "$victim" ]]; then
    echo "$name: self-test failed: $reference_dir holds no file with a byte to change" >&2
    exit 1
  fi
  python3 -I - "$copy/$victim" <<'PY'
import sys

with open(sys.argv[1], "r+b") as target:
    first = target.read(1)
    target.seek(0)
    target.write(bytes([first[0] ^ 1]))
PY
  output=$(compare_dirs "$reference_dir" "$copy") || verdict=$?
  rm -rf "$copy"
  echo "$output"
  echo "$name: self-test: comparing the copy with one changed byte in $victim gives status $verdict"
  if ((verdict != 1)) || [[ "$output" != "$name: differs: $victim" ]]; then
    echo "$name: self-test failed: one changed byte in $victim was not refused by name" >&2
    exit 1
  fi
  echo "$name: self-test passed: the comparison accepted an unchanged copy and refused one changed byte in $victim"
}

case "${1:-}" in
  "") check_two_builds ;;
  --self-test) self_test ;;
  *)
    echo "usage: $0 [--self-test]" >&2
    exit 2
    ;;
esac
