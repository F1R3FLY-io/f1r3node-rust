#!/usr/bin/env bash
set -euo pipefail
[[ ( $# == 1 || ( $# == 2 && "$2" == --historical ) ) && ! -e "$1" && ! -L "$1" ]]
ROOT="$PWD"
HISTORICAL="${2:-}"
P=docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01
mkdir -p "$1"
OUT="$(cd "$1" && pwd)"
(cd "$P"; shasum -a 256 -c artifacts.sha256; shasum -a 256 -c candidate-ledgers.sha256) >"$OUT/artifacts.txt"
for kind in source evidence ledgers; do
 mkdir "$OUT/$kind"
 gtar --no-same-owner -xzf "$P/$kind.tar.gz" -C "$OUT/$kind"
 (cd "$OUT/$kind"; shasum -a 256 -c "$ROOT/$P/$kind-files.sha256") >"$OUT/$kind.txt"
done
if [[ -z "$HISTORICAL" ]]; then
 shasum -a 256 -c "$P/source-files.sha256" >"$OUT/current-source.txt"
 shasum -a 256 -c "$P/ledgers-files.sha256" >"$OUT/previous-ledgers.txt"
fi
jq -e '.status=="verified-binding-accepted-pending-recording" and .claim_discharge=="pending" and .binding_acceptance.confirmation=="yes - I authorized acceptance" and .tasks_complete==false and .tags_applied==true and .node_execution==false and .policy_activation==false and .soak_verdict=="non_passing"' "$P/report.json" >/dev/null
E="$OUT/evidence"
[[ "$(<"$E/publication-red.exit")" == 101 && "$(<"$E/publication-green.exit")" == 0 ]]
grep -F 'left: Some(0)' "$E/publication-red.txt" >/dev/null
grep -F 'right: Some(2)' "$E/publication-red.txt" >/dev/null
[[ "$(grep -c FAILED "$E/accepted-source-check.txt")" == 4 ]]
for kind in native linux; do
 for p in authority-finality publication recovery; do
  if [[ "$kind" == native ]]; then
   fixtures="$E/$p-final/fixtures"
  else
   name="$p"; [[ "$p" != authority-finality ]] || name=authority
   fixtures="$E/linux/evidence/$name-fixtures"
  fi
  expected="$(jq -r --arg p "$p" '.profiles[]|select(.profile==$p)|.invocations' "$P/report.json")"
  cases="$(jq -r --arg p "$p" '.profiles[]|select(.profile==$p)|.cases' "$P/report.json")"
  [[ "$(find "$fixtures" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" == "$cases" ]]
  find "$fixtures" -name 'invocation-*.json' -print0 | xargs -0 jq -s --argjson expected "$expected" -e 'length==$expected and all(.actual_exit==.expected_exit and (.stdout|fromjson|.node_launch_count==0 and .soak_verdict=="non_passing")) and all(. as $v|($v.stdout|fromjson|.scenario_verdict)==$v.expected_verdict)' >"$OUT/$kind-$p-invocations.txt"
 done
done
check_reference() {
 local relative="$1" expected="$2" stored
 stored="$(awk -F '\t' -v path="$relative" -v before="$expected" '$1==path && $2==before {n++; after=$3} END {if(n>1) exit 2; print (n==1 ? after : before)}' "$P/redactions.tsv")"
 printf '%s  %s\n' "$stored" "$E/$relative"
}
for p in authority-finality publication recovery; do
 [[ "$(<"$E/$p-final/exit.txt")" == 0 ]]
 jq -e '.results|length==4 and all(.outcome=="passed" and .exit==.expected_exit) and ([.[]|select(.exit==0)]|length)==1 and ([.[]|select(.exit==12 and .expected_property!=null)]|length)==3' "$E/$p-final/models/report.json" >/dev/null
 jq -r '.results[]|.log_sha256+"  "+.log' "$E/$p-final/models/report.json" |
 while read -r sha log; do check_reference "$p-final/models/$log" "$sha"; done |
 shasum -a 256 -c - >"$OUT/$p-model-logs.txt"
 (cd "$OUT/source"; shasum -a 256 -c "$E/$p-final/source-after.sha256") >"$OUT/$p-run-source.txt"
done
[[ "$(<"$E/linux/exit.txt")" == 0 ]]
jq -e '.[0]|.State.ExitCode==0 and .HostConfig.NetworkMode=="none" and .Config.User=="65534:65534" and (.HostConfig.CapDrop|index("ALL"))!=null and (.HostConfig.SecurityOpt|index("no-new-privileges"))!=null and (.HostConfig.Binds==null or .HostConfig.Binds==[]) and (.Mounts|length)==0' "$E/linux/finished.json" >/dev/null
grep -F '7 passed; 0 failed;' "$E/linux/evidence/authority_finality.txt" >/dev/null
grep -F '6 passed; 0 failed;' "$E/linux/evidence/publication.txt" >/dev/null
grep -F '7 passed; 0 failed;' "$E/linux/evidence/recovery.txt" >/dev/null
grep -F '1 passed; 0 failed;' "$E/linux/evidence/authority-unit.txt" >/dev/null
for code in 001 002 003 004; do [[ "$(<"$E/audits/claim-$code.exit")" == 4 ]]; done
[[ "$(<"$E/audits/ordinary.exit")" == 0 && "$(<"$E/audits/bundle.exit")" == 4 ]]
: >"$OUT/nested.sha256"
find "$E" -name report.json -path '*/fixtures/*' -o -name report.json -path '*-fixtures/*' | LC_ALL=C sort |
while IFS= read -r report; do
 jq -r '(.retained_sources[]?,.retained_inputs[]?)|.sha256+"  "+.retained' "$report" |
 while read -r sha relative; do
  [[ "$relative" != /* && "$relative" != *..* ]]
  parent="$(dirname "${report#"$E/"}")"
  check_reference "$parent/$relative" "$sha"
 done
done >"$OUT/nested.sha256"
shasum -a 256 -c "$OUT/nested.sha256" >"$OUT/nested.txt"
report_hash="$(shasum -a 256 "$P/report.json" | awk '{print $1}')"
count=0
for candidate in "$P/candidate-ledgers/"*.md; do
 data="$(awk '/^```json$/ {on=1;next} on && /^```$/ {exit} on {print}' "$candidate")"
 artifact="$(jq -r '.artifact.path' <<<"$data")"
 spec="$(jq -r '.claim' <<<"$data")"
 sha="$(shasum -a 256 "$OUT/source/$artifact" | awk '{print $1}')"
 spec_sha="$(shasum -a 256 "$OUT/source/$spec" | awk '{print $1}')"
 jq -e --arg sha "$sha" --arg spec "$spec_sha" --arg report "$report_hash" '.artifact.sha256==$sha and .claim_digests[.claim]==$spec and .evidence.sha256==$report and .status=="pending" and .tiers.binding=="pending-review" and .phase_status.pre_pr216_merge=="pending" and .phase_status.post_pr216_merge=="blocked" and .soak=="pending" and .waiver==null' <<<"$data" >/dev/null
 count=$((count+1))
done
[[ "$count" == 36 ]]
for p in authority-finality publication recovery; do
 [[ "$(git check-attr cbc -- ".github/workflows/casper-$p.yml")" == *': mandatory' ]]
done
jq -n --arg report "$report_hash" --argjson nested "$(wc -l <"$OUT/nested.sha256")" --arg mode "${HISTORICAL:-current}" '{status:"review-package-integrity-passed",report_sha256:$report,current_sources:($mode=="current"),source_archives:true,evidence_archive:true,previous_ledgers:true,pending_candidates:36,nested_references:$nested,invocations_per_platform:203,models:12,claim_discharge:"pending",strict_001:4,strict_bundle:4,node_execution:false}' >"$OUT/validation.json"
printf 'The source-bound review package is valid. Ledger recording remains pending in this snapshot.\n'
