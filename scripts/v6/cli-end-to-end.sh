#!/usr/bin/env bash
# CLI end-to-end run against a protocol-6 standalone shard seeded from wallets.txt.
# Usage: scripts/v6/cli-end-to-end.sh [work-dir]
# Needs target/release/node (cargo build --release -p node).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
node="$root/target/release/node"
work="${1:-$(mktemp -d)}"
data="$work/data"

validator_sk=1111111111111111111111111111111111111111111111111111111111111111
validator_pk=044f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa385b6b1b8ead809ca67454d9683fcf2ba03456d6fe2c4abe2b07f0fbdbb2f1c1
validator_address=111125UED2qbF3aman9PYvfeTk81TuP4fVL5nYsEXtFyTtaLPcHW61
payer_sk=2222222222222222222222222222222222222222222222222222222222222222
payer_address=1111yoPaoSxvmNRGDpP9GUGToHaCdUFEu3KdbNgTuyydUgg6cty7g
payee_address=11112c1uzwQPPBSwCZMyE1YzAEcpZPgemPets9UcNPVgder5HHjxoV
supply=1000000000

mkdir -p "$data/genesis"
echo "$validator_pk 1000" > "$data/genesis/bonds.txt"
echo "$payer_address,$supply" > "$data/genesis/wallets.txt"
sed -e "s#run-local/data/standalone#$data#g" "$root/run-local/conf/standalone.conf" |
    awk '{ print } /pos-multi-sig-quorum = 1/ { print "    protocol-version = 6" }' > "$work/standalone-v6.conf"

"$node" run -s --config-file="$work/standalone-v6.conf" \
    --validator-private-key="$validator_sk" --host=localhost --no-upnp > "$work/node.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
    grep -q "Making a transition to Running state" "$work/node.log" && break
    sleep 2
done
grep -q "this node runs the v6 executor" "$work/node.log" || { echo "FAIL: the node is not a v6 node"; exit 1; }

balance() { "$node" wallet-balance --address "$1" | sed -n 's/^Balance of [^:]*: //p'; }
propose_rounds() { for _ in 1 2 3 4; do "$node" propose > /dev/null; sleep 15; done; }
check() { if [ "$2" = "$3" ]; then echo "ok   $1 = $2"; else echo "FAIL $1 = $2, expected $3"; exit 1; fi; }

check "genesis payer balance" "$(balance "$payer_address")" "$supply"
check "genesis payee balance" "$(balance "$payee_address")" 0

"$node" transfer --to "$payee_address" --amount 7000 --phlo-limit 100000 --phlo-price 1 \
    --private-key "$payer_sk" --shard-id root
propose_rounds
check "payee after the transfer" "$(balance "$payee_address")" 7000
paid=$(( supply - 7000 - $(balance "$payer_address") ))
echo "     payer's charges: $paid"
[ "$paid" -gt 0 ] || { echo "FAIL: the transfer was not charged"; exit 1; }

"$node" transfer --to "$payee_address" --amount 2000000000 --phlo-limit 100000 --phlo-price 1 \
    --private-key "$payer_sk" --shard-id root
propose_rounds
check "payee after an overdraft" "$(balance "$payee_address")" 7000
echo "     proposer's COMM credits: $(balance "$validator_address")"
echo "PASS"
