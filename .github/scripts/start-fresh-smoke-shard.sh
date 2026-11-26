#!/usr/bin/env bash
set -euo pipefail

compose=(docker compose -f docker/shard.yml -f docker/ci-ports.shard.yml)
volumes=$("${compose[@]}" config --format json | jq -er '.volumes[].name')
while read -r volume; do
    if docker volume inspect "$volume" >/dev/null 2>&1; then
        echo "Smoke first-use requires a fresh Compose project (existing volume: $volume)" >&2
        exit 1
    fi
done <<< "$volumes"
"${compose[@]}" up -d

genesis=""
deadline=$((SECONDS + 300))
until genesis=$(curl -fsS --max-time 5 'http://localhost:14403/api/blocks/0/0?view=full' 2>/dev/null |
    jq -er 'select(length == 1) | .[0].blockInfo | select(.blockNumber == 0) | .blockHash | select(test("^[a-f0-9]{64}$"))'); do
    (( SECONDS < deadline )) || { echo "Bootstrap genesis unavailable" >&2; exit 1; }
    sleep 2
done

for n in 1 2 3; do
    role="validator$n"
    key=$(sed -n "s/^VALIDATOR${n}_PUBLIC_KEY=//p" docker/.env)
    [[ "$key" =~ ^04[0-9a-f]{128}$ ]]
    until "${compose[@]}" exec -T "$role" curl -fsS --max-time 5 http://localhost:40403/api/ready >/dev/null 2>&1; do
        (( SECONDS < deadline )) || { echo "$role did not reach Running" >&2; exit 1; }
        sleep 2
    done
    id=$("${compose[@]}" logs --no-color "$role" | sed -nE 's/.*Starting peer restore with recovery ID ([0-9a-f-]{36}).*/\1/p' | sort -u)
    [[ "$id" =~ ^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$ ]]
    printf '%s %s first-use\n' "$genesis" "$key" | "${compose[@]}" exec -T "$role" sh -ec \
        'cd /var/lib/rnode; umask 077; cat > "$1.tmp"; mv "$1.tmp" "$1"' sh "validator-recovery-$id.confirm"
    echo "Confirmed first use for $role on test genesis $genesis"
done

for role in validator1 validator2 validator3; do
    until "${compose[@]}" logs --no-color "$role" | grep -F 'Accepted operator confirmation of signing history' >/dev/null; do
        (( SECONDS < deadline )) || { echo "$role did not accept its first-use confirmation" >&2; exit 1; }
        sleep 2
    done
done
