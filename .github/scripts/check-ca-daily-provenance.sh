#!/usr/bin/env bash
set -euo pipefail

reject() {
	printf '::error::%s\n' "$*" >&2
	exit 1
}

head_ref="${HEAD_REF:-}"
head_sha="${HEAD_SHA:-}"
base_sha="${BASE_SHA:-}"
release_sha="${RELEASE_SHA:-}"
repository="${REPOSITORY:-}"

[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || reject 'The repository name is invalid.'
[ "${HEAD_REPO:-}" = "$repository" ] || reject 'A daily back-merge must come from this repository.'
[ "${PR_AUTHOR:-}" = 'f1r3node-promotion[bot]' ] || reject 'The promotion app must open a daily back-merge.'
[[ "$head_ref" =~ ^backmerge/ca-release-([0-9a-f]{12})$ ]] || reject 'The back-merge branch must name a release commit.'
name_sha="${BASH_REMATCH[1]}"
for sha in "$head_sha" "$base_sha" "$release_sha"; do
	[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || reject 'The head, base, and release must have full commit SHAs.'
	git cat-file -e "$sha^{commit}" 2>/dev/null || reject "The commit $sha is unavailable."
done
source_sha="$(git rev-parse --verify "$name_sha^{commit}" 2>/dev/null)" || reject 'The named release source is unavailable or ambiguous.'
git merge-base --is-ancestor "$source_sha" "$release_sha" || reject 'The named source does not belong to ca-release.'

current="$head_sha"
merges=0
while [ "$current" != "$source_sha" ]; do
	[ "$merges" -lt 128 ] || reject 'The back-merge has more than 128 base updates.'
	parents="$(git show --no-patch --format=%P "$current")" || reject 'The merge parents are unavailable.'
	read -r first second extra <<<"$parents"
	[[ "$first" =~ ^[0-9a-f]{40}$ && "$second" =~ ^[0-9a-f]{40}$ && -z "$extra" ]] ||
		reject 'Only two-parent base merges can follow the release source.'
	git merge-base --is-ancestor "$second" "$base_sha" || reject 'A merge parent does not belong to the captured ca-canary base.'
	if ! expected="$(git -c core.attributesFile=/dev/null -c core.hooksPath=/dev/null -c merge.renormalize=false \
		merge-tree --write-tree "$first" "$second")"; then
		reject 'A base merge has conflicts or cannot be reconstructed.'
	fi
	actual="$(git rev-parse --verify "$current^{tree}")" || reject 'The merge tree is unavailable.'
	[ "$actual" = "$expected" ] || reject 'The merge tree contains changes beyond the clean base merge.'
	current="$first"
	merges=$((merges + 1))
done

printf 'Daily provenance accepted: source=%s head=%s base_merges=%s\n' "$source_sha" "$head_sha" "$merges"
