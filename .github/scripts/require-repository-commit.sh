#!/usr/bin/env bash
# Benchmark target check. Accepts a commit only when a branch or a tag of the
# repository at URL contains it.
#
# The commits API also resolves a commit that exists only in a fork of the
# repository network, and actions/checkout can fetch that commit. A workflow
# that builds and runs the commit in the default branch context then runs
# code that no branch of this repository holds.
#
# The check fetches only the commit graph (--filter=tree:0) of refs/heads/*
# and refs/tags/* into a new bare repository. Pull request refs and other
# refs are not fetched. A fetch failure rejects the commit.
#
# The filtered fetch makes the URL a promisor remote. Git then fetches a
# missing object on demand, and GitHub serves a fork commit by its SHA.
# GIT_NO_LAZY_FETCH=1 keeps every check below local to the fetched refs.
#
# Usage: require-repository-commit.sh URL SHA
set -euo pipefail
export GIT_NO_LAZY_FETCH=1

url="${1:?repository URL}"
sha="${2:?commit SHA}"

reject() {
	echo "::error::$1"
	exit 1
}

[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || reject "The benchmark target is not a full commit SHA: $sha"

repo="$(mktemp -d)"
trap 'rm -rf "$repo"' EXIT
git init --quiet --bare "$repo"
git -C "$repo" fetch --quiet --no-tags --filter=tree:0 "$url" \
	'+refs/heads/*:refs/heads/*' '+refs/tags/*:refs/tags/*' ||
	reject "The branches and tags of $url could not be fetched."

git -C "$repo" cat-file -e "$sha^{commit}" 2>/dev/null ||
	reject "No branch or tag of $url holds $sha. Benchmark a commit of this repository."
ref="$(git -C "$repo" for-each-ref --contains "$sha" --count=1 --format='%(refname)' refs/heads refs/tags)" ||
	reject "The branches and tags of $url could not be searched for $sha."
[ -n "$ref" ] ||
	reject "No branch or tag of $url reaches $sha. Benchmark a commit of this repository."

echo "Benchmark target accepted: $ref contains $sha"
