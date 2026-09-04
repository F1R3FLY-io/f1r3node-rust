#!/usr/bin/env bash
set -euo pipefail

capabilities_file=${1:-.github/system-integration-node-capabilities.txt}
seen='|'

while IFS= read -r capability || [ -n "$capability" ]; do
	capability=${capability%$'\r'}
	case "$capability" in
	'' | \#*) continue ;;
	esac
	if [[ ! "$capability" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]]; then
		printf 'invalid system-integration node capability: %s\n' "$capability" >&2
		exit 1
	fi
	case "$seen" in
	*"|$capability|"*)
		printf 'duplicate system-integration node capability: %s\n' "$capability" >&2
		exit 1
		;;
	esac
	seen="${seen}${capability}|"
	printf '%s\n' "$capability"
done <"$capabilities_file"
