#!/usr/bin/env bash
# Fails when a Cargo workspace member has no COPY into the builder stage of
# node/Dockerfile.
#
# The builder stage copies each member directory separately, and cargo fetch
# --locked reads every member manifest. A member without a COPY fails the image
# build only, and the image build runs only in the heavy pipeline. PR #794 added
# scripts/file-read-service to the workspace without a COPY, and the push to dev
# then failed the image build on both architectures.
#
#   check-dockerfile-workspace.sh [CARGO_TOML [DOCKERFILE]]
#
# A COPY of a parent directory covers a nested member, for example rholang/
# covers rholang/fileio-test-fixtures.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo_toml="${1:-$ROOT/Cargo.toml}"
dockerfile="${2:-$ROOT/node/Dockerfile}"

members="$(awk '
	/^members[[:space:]]*=[[:space:]]*\[/ { inside = 1; sub(/^[^[]*\[/, "") }
	inside {
		line = $0
		done = sub(/\].*$/, "", line)
		n = split(line, parts, ",")
		for (i = 1; i <= n; i++) {
			item = parts[i]
			gsub(/[[:space:]"]/, "", item)
			if (item != "") print item
		}
		if (done) exit
	}
' "$cargo_toml")"
[ -n "$members" ] || { echo "ERROR: $cargo_toml lists no workspace members." >&2; exit 2; }

# The source directories of the COPY lines without --from, as dir/ without the
# trailing slash.
copied="$(awk '
	$1 == "COPY" && $2 !~ /^--from=/ {
		for (i = 2; i < NF; i++) if ($i !~ /^--/) { src = $i; sub(/\/$/, "", src); print src }
	}
' "$dockerfile")"

missing=()
while IFS= read -r member; do
	covered=false
	while IFS= read -r dir; do
		if [ "$member" = "$dir" ] || [[ "$member" == "$dir"/* ]]; then
			covered=true
			break
		fi
	done <<<"$copied"
	[ "$covered" = true ] || missing+=("$member")
done <<<"$members"

if [ "${#missing[@]}" -gt 0 ]; then
	for member in "${missing[@]}"; do
		echo "::error file=node/Dockerfile::the workspace member $member has no COPY in node/Dockerfile. Add: COPY $member/ ./$member/"
	done
	exit 1
fi
echo "Every workspace member of $(basename "$cargo_toml") has a COPY in $(basename "$dockerfile")."
