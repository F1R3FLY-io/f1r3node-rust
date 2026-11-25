#!/usr/bin/env bash
# Prints the SHAs of the pull request commits that skip the author check of
# Commit Policy (CLAUDE.md "Commit Messages"). Two kinds of commit are exempt:
#
#   1. A commit that GitHub attributes to the pull request author, when the
#      author is a human account (type User). A bot or app account that opens
#      a pull request gets no exemption.
#   2. A commit of a fork pull request that a maintainer imported. Mergify
#      queues no fork head, so a maintainer pushes the fork head unchanged to
#      a branch of this repository and opens a pull request with the body line
#      "Imports fork pull request #<N>." The source must be a pull request into
#      this repository from a fork, opened by a human account. A commit counts
#      only when GitHub lists it in the source pull request, attributes it to
#      the source author, and lists it in this pull request too. A commit SHA
#      names its content, so the import cannot change an exempt commit.
#
# Environment: GITHUB_REPOSITORY, PR_NUMBER, PR_AUTHOR, PR_AUTHOR_TYPE.
set -euo pipefail

[[ "${GITHUB_REPOSITORY:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo "ERROR: GITHUB_REPOSITORY is not owner/name." >&2; exit 2; }
[[ "${PR_NUMBER:-}" =~ ^[1-9][0-9]*$ ]] || { echo "ERROR: PR_NUMBER is not a pull request number." >&2; exit 2; }
repo="$GITHUB_REPOSITORY"

# pull_commits N: "SHA<TAB>login" for each commit of pull request N.
pull_commits() {
	gh api --paginate "repos/$repo/pulls/$1/commits?per_page=100" --jq '.[] | [.sha, (.author.login // "")] | @tsv'
}

commits="$(pull_commits "$PR_NUMBER")"
exempt=()
if [ "${PR_AUTHOR_TYPE:-}" = User ]; then
	while IFS=$'\t' read -r sha login; do
		[ -n "$sha" ] && [ "$login" = "$PR_AUTHOR" ] && exempt+=("$sha")
	done <<<"$commits"
else
	echo "Pull request author ${PR_AUTHOR:-unknown} is a ${PR_AUTHOR_TYPE:-unknown} account; no author exemption." >&2
fi

body="$(gh api "repos/$repo/pulls/$PR_NUMBER" --jq '.body // ""' | tr -d '\r')"
sources="$(grep -E '^Imports fork pull request #[1-9][0-9]*\.?[[:space:]]*$' <<<"$body" | grep -oE '[1-9][0-9]*' || true)"
if [ -n "$sources" ]; then
	if [ "$(wc -l <<<"$sources" | tr -d ' ')" -ne 1 ]; then
		echo "ERROR: the pull request body names more than one imported fork pull request." >&2
		exit 2
	fi
	source="$sources"
	IFS=$'\t' read -r head_repo base_repo source_author source_type < <(
		gh api "repos/$repo/pulls/$source" --jq '[(.head.repo.full_name // ""), .base.repo.full_name, .user.login, .user.type] | @tsv')
	if [ "$source" = "$PR_NUMBER" ]; then
		echo "Pull request #$PR_NUMBER names itself as the imported fork pull request; no import exemption." >&2
	elif [ -z "$head_repo" ] || [ "$head_repo" = "$repo" ] || [ "$base_repo" != "$repo" ]; then
		echo "Pull request #$source is not a pull request into $repo from a fork; no import exemption." >&2
	elif [ "$source_type" != User ]; then
		echo "Pull request #$source was opened by a $source_type account; no import exemption." >&2
	else
		imported=0
		while IFS=$'\t' read -r sha login; do
			[ -n "$sha" ] && [ "$login" = "$source_author" ] || continue
			if grep -q "^$sha"$'\t' <<<"$commits"; then
				exempt+=("$sha")
				imported=$((imported + 1))
			fi
		done < <(pull_commits "$source")
		echo "Imported $imported commits of $source_author from fork pull request #$source." >&2
	fi
fi

if [ "${#exempt[@]}" -gt 0 ]; then
	printf '%s\n' "${exempt[@]}" | sort -u
fi
