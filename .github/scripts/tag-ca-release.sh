#!/usr/bin/env bash
# Tags the current ca-release head as ca-v<version> for the cost-accounting
# release train (issue #791). `just ca-tag <version>` runs it.
#
# The tag is created only when the head commit has a successful
# "Soak Gate (60h)" commit status, so every ca-v* tag passed the 60-hour soak.
# The tag push starts CI, which runs the heavy pipeline and publishes the
# :ca-v<version> and :ca-latest images after a reviewer approves the
# ephemeral-launch environment. The rust-client master branch pins this tag.
#
# The tag is created through the GitHub API, so no local checkout or push is
# needed. An existing tag is never moved.
set -euo pipefail

version="${1:-}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]] ||
	{ echo "ERROR: give the version as MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-rc.N, for example 0.5.0-rc.1." >&2; exit 2; }
tag="ca-v$version"
gate="Soak Gate (60h)"

repo="${CA_TAG_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"

sha="$(gh api "repos/$repo/git/ref/heads/ca-release" --jq .object.sha)"
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: ca-release does not resolve to a commit." >&2; exit 2; }

if gh api "repos/$repo/git/ref/tags/$tag" >/dev/null 2>&1; then
	echo "ERROR: $tag already exists. A release train tag is never moved." >&2
	exit 1
fi

state="$(gh api "repos/$repo/commits/$sha/status" --jq ".statuses[] | select(.context == \"$gate\") | .state")"
if [ "$state" != success ]; then
	echo "ERROR: ca-release ${sha:0:9} has no successful \"$gate\" status (state: ${state:-none}). Tag only a soaked commit." >&2
	exit 1
fi

tag_object="$(gh api "repos/$repo/git/tags" -f tag="$tag" -f object="$sha" -f type=commit \
	-f message="Release train $tag at ca-release ${sha:0:9}, after \"$gate\" passed." --jq .sha)"
[[ "$tag_object" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: the tag object for $tag was not created." >&2; exit 1; }
gh api "repos/$repo/git/refs" -f ref="refs/tags/$tag" -f sha="$tag_object" >/dev/null
echo "Created $tag at ca-release ${sha:0:9}."
