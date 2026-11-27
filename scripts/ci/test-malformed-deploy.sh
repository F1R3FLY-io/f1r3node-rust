#!/usr/bin/env bash
set -euo pipefail

queue_http=${1:?expected queue HTTP URL followed by the node CLI command}
shift
status=$(curl -fsS --max-time 5 "$queue_http/api/status")
height=$(jq -er '.latestBlockNumber - 1' <<<"$status")
shard=$(jq -er '.shardId' <<<"$status")
price=$(jq -er '.minPhloPrice' <<<"$status")
reply=$(printf '%s\n' 'new x in {' | "$@" deploy 100000 "$price" "$height" \
    "$(openssl rand -hex 32)" /dev/null /dev/stdin "$shard")
sig=$(sed -nE 's/^DeployId is: ([[:xdigit:]]+)$/\1/p' <<<"$reply")
if [[ -z $sig ]]; then
    echo "Queue did not acknowledge the malformed deploy: $reply" >&2
    exit 1
fi

deadline=$((SECONDS + 45))
while (( SECONDS < deadline )); do
    verdict=$(curl -fsS --max-time 5 "$queue_http/api/deploy-finalization-status/$sig")
    case $(jq -er '.state' <<<"$verdict") in
        Pending) sleep 1 ;;
        Rejected) jq -ec 'select(.detail | startswith("rholang_bad_term:"))' <<<"$verdict"; exit ;;
        *) echo "Unexpected verdict: $verdict" >&2; exit 1 ;;
    esac
done
echo "Malformed deploy did not reach Rejected: $verdict" >&2
exit 1
