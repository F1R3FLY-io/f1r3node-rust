#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
	echo "Usage: check-ci-parity.sh <candidate> <master> <dev>" >&2
	exit 2
fi

for revision in "$@"; do
	if ! git rev-parse --quiet --verify "${revision}^{commit}" >/dev/null; then
		echo "ERROR: $revision is not a commit." >&2
		exit 2
	fi
done

list() {
	git ls-tree -r --full-tree "$2" -- .github .mergify.yml |
		awk -v tag="$1" -F '\t' '{ split($1, meta, " "); print tag "\t" meta[1] " " meta[3] "\t" $2 }'
}

drift="$(
	{
		list candidate "$1"
		list master "$2"
		list dev "$3"
	} | awk -F '\t' '
		{ entry[$1, $3] = $2; paths[$3] = 1 }
		END {
			for (path in paths) {
				c = entry["candidate", path]; m = entry["master", path]; d = entry["dev", path]
				if (c == m || c == d) continue
				if (c == "") reason = "is deleted only in the candidate"
				else if (m == "" && d == "") reason = "exists only in the candidate"
				else reason = "differs from master and from dev"
				print path " " reason
			}
		}' | LC_ALL=C sort
)"

if [ -n "$drift" ]; then
	sed 's/^/DRIFT: /' <<<"$drift" >&2
	echo "ERROR: a CI control file did not come from master or dev. Make the change on dev, promote it, and sync ca-canary." >&2
	exit 1
fi
echo "PASS: every CI control file of the candidate matches master or dev."
