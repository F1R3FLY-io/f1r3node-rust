#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
P="$ROOT/docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01"
OUT="${1:?Supply a new validation directory.}"
[[ ! -e "$OUT" ]]
mkdir -p "$OUT/source" "$OUT/evidence"
OUT="$(cd "$OUT" && pwd)"
(cd "$P"; shasum -a 256 -c artifacts.sha256; shasum -a 256 -c ledgers.sha256) >"$OUT/package-hashes.txt"
for kind in source evidence; do
 gtar -xzf "$P/$kind.tar.gz" -C "$OUT/$kind"
 (cd "$OUT/$kind"; shasum -a 256 -c "$P/$kind-files.sha256") >"$OUT/$kind-hashes.txt"
done
while IFS=$'\t' read -r path target; do [[ -L "$OUT/$path" && "$(readlink "$OUT/$path")" == "$target" ]]; done <"$P/symlinks.txt"
shasum -a 256 -c "$P/profile-artifacts.sha256" >"$OUT/current-source.txt"
[[ "$(shasum -a 256 docs/claims/casper-soak-merge-accounting.md | awk '{print $1}')" == "$(jq -r .specification_sha256 "$P/report.json")" ]]
jq -e '.claim_discharge=="pending" and .binding=="pending" and .node_execution==false and .policy_activation==false and .soak=="pending" and .post_merge_execution=="blocked" and .workflow_tag=="proposed-not-applied"' "$P/report.json" >/dev/null
while IFS= read -r path; do
 slug="$(printf '%s' "$path" | awk '{sub(/^\./, ""); gsub(/[\/._]/,"-"); print}')"
 cmp "$P/ledgers/$slug.md" "docs/casper/cbc-evidence/$slug.md"
 [[ "$(readlink "docs/cbc-evidence/$slug.md")" == "../casper/cbc-evidence/$slug.md" ]]
done <"$P/artifact-paths.txt"
for n in 001 002 003 004 005; do
 expected=0; [[ "$n" != 005 ]] || expected=4
 status=0
 target/debug/check-casper-claims --root "$ROOT" --output "$OUT/claim-$n.json" --strict --claim "CLAIM-CASPER-SOAK-$n" >"$OUT/claim-$n.txt" 2>&1 || status=$?
 [[ "$status" == "$expected" ]]
done
[[ "$(git check-attr cbc -- .github/workflows/casper-merge-accounting.yml)" == '.github/workflows/casper-merge-accounting.yml: cbc: unspecified' ]]
NESTED=0
check_hash() {
 local relative="$1" expected="$2" actual
 [[ "$relative" != /* && "$relative" != *../* ]]
 actual="$(shasum -a 256 "$OUT/evidence/$relative" | awk '{print $1}')"
 if [[ "$actual" != "$expected" ]]; then
  awk -F '\t' -v p="evidence/$relative" -v b="$expected" -v a="$actual" '$1==p && $2==b && $3==a {found++} END {exit found!=1}' "$P/redactions.tsv"
 fi
 NESTED=$((NESTED+1))
}
for lane in native-recorded/fixtures linux-final/evidence/fixtures; do
 cases="$(find "$OUT/evidence/$lane" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
 [[ "$cases" == 68 ]]
 count=0
 while IFS= read -r invocation; do
  jq -e '.actual_exit==.expected_exit and (.stdout|fromjson|.scenario_verdict)==.expected_verdict and (.stdout|fromjson|.node_launch_count)==0 and (.stdout|fromjson|.soak_verdict)=="non_passing"' "$invocation" >/dev/null
  count=$((count+1))
 done < <(find "$OUT/evidence/$lane" -name 'invocation-*.json' | LC_ALL=C sort)
 [[ "$count" == 73 ]]
 while IFS= read -r report; do
  jq -e --slurpfile own "$OUT/evidence/native-recorded/identity.json" '.profile_identity.source_digests==$own[0].source_digests and .node_launch_count==0 and .soak_verdict=="non_passing"' "$report" >/dev/null
  while IFS=$'\t' read -r retained digest; do
   check_hash "$(dirname "${report#"$OUT/evidence/"}")/$retained" "$digest"
  done < <(jq -r '(.retained_sources[],.retained_inputs[]) | [.retained,.sha256] | @tsv' "$report")
 done < <(find "$OUT/evidence/$lane" -path '*/output-*/report.json' | LC_ALL=C sort)
done
MODEL="$OUT/evidence/native-recorded/models/report.json"
jq -e '.status=="passed" and (.results|length)==4 and all(.results[]; .outcome=="passed" and .exit==.expected_exit) and .results[0].states_generated==841 and .results[0].distinct_states==441' "$MODEL" >/dev/null
while IFS=$'\t' read -r log digest; do check_hash "native-recorded/models/$log" "$digest"; done < <(jq -r '.results[] | [.log,.log_sha256] | @tsv' "$MODEL")
for file in native-recorded/fixtures.txt linux-final/evidence/tests.txt; do grep -F 'test result: ok. 7 passed; 0 failed; 0 ignored;' "$OUT/evidence/$file" >/dev/null; done
[[ "$(<"$OUT/evidence/independent-red.exit")" == 101 && "$(<"$OUT/evidence/independent-green.exit")" == 0 ]]
[[ "$(<"$OUT/evidence/shared.exit")" == 0 && "$(<"$OUT/evidence/ste.exit")" == 0 ]]
jq -e '.[0] | .Config.User=="65534:65534" and .HostConfig.NetworkMode=="none" and .HostConfig.Privileged==false and (.HostConfig.CapDrop|index("ALL"))!=null and (.HostConfig.SecurityOpt|index("no-new-privileges"))!=null and .HostConfig.PidsLimit==128 and .HostConfig.Memory==536870912 and .HostConfig.NanoCpus==2000000000 and (.Mounts|length)==0' "$OUT/evidence/linux-final/container.json" >/dev/null
jq -e '.[0].State.ExitCode==0' "$OUT/evidence/linux-final/finished.json" >/dev/null
jq -n --argjson nested "$NESTED" '{status:"pending-binding-evidence-verified",nested_references:$nested,platforms:2,tests_per_platform:7,cases_per_platform:68,invocations_per_platform:73,models:4,claim_discharge:"pending",node_execution:false}' >"$OUT/validation.json"
printf 'The source-bound evidence is verified. Binding acceptance remains pending.\n'
