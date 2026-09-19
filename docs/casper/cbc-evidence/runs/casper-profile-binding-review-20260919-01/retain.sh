#!/usr/bin/env bash
set -euo pipefail
P=docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01
E=target/casper-profile-completion
S=target/casper-profile-review-retention
[[ ! -e "$P/report.json" && ! -e "$S" ]]
mkdir -p "$S/source" "$S/evidence" "$S/ledgers" "$P/candidate-ledgers"
for p in authority-finality publication recovery; do
 [[ "$(<"$E/$p-final.exit")" == 0 ]]
 cmp "$E/$p-final/source-before.sha256" "$E/$p-final/source-after.sha256"
 shasum -a 256 -c "$E/$p-final/source-after.sha256" >"$E/$p-current-source.txt"
done
for check in publication-green linux-profiles linux-build shared-regressions fmt ste; do
 [[ "$(<"$E/$check.exit")" == 0 ]]
done
[[ "$(<"$E/publication-red.exit")" == 101 ]]
[[ "$(<"$E/accepted-source-check.exit")" != 0 ]]
[[ "$(<"$E/audits/ordinary.exit")" == 0 ]]
for id in 001 002 003 004; do [[ "$(<"$E/audits/claim-$id.exit")" == 4 ]]; done
[[ "$(<"$E/audits/bundle.exit")" == 4 ]]
git rev-parse HEAD >"$E/final-head.txt"
git status --porcelain=v1 >"$E/status-after.txt"
git diff --check >"$E/diff-check.txt"
git diff --cached --check >"$E/index-check.txt"
git diff "$(<"$E/base.txt")" HEAD --stat >"$E/external-tree-diff.txt"
git log --format='%H %s' "$(<"$E/base.txt")..HEAD" >"$E/external-history.txt"
{
 awk '{print $2}' docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01/source-files.sha256
 for p in authority-finality publication recovery; do awk '{print $2}' "$E/$p-final/source-before.sha256"; done
 printf '%s\n' docs/ToDos.md docs/casper/design/decision-ledger/07-deploy-recovery-custody.md docs/casper/design/soak-candidate-matrix.jsonc docs/work-logs/task-017-{5-authority-finality,6-publication,7-recovery,5-7-binding-review}.md "$P/retain.sh" "$P/validate.sh" "$P/redact.rs"
} | LC_ALL=C sort -u >"$S/source-paths.txt"
while IFS= read -r path; do
 [[ -f "$path" && ! -L "$path" && "$path" != /* && "$path" != *..* ]]
 mkdir -p "$S/source/$(dirname "$path")"
 cp "$path" "$S/source/$path"
done <"$S/source-paths.txt"
cp -R "$E/." "$S/evidence/"
rustc --edition 2021 "$P/redact.rs" -o "$S/redact-bin"
"$S/redact-bin" "$S/evidence" >"$P/redactions.tsv"
if grep -rIlE '/Users/|/home/[[:alnum:]_-]+/|/var/folders/' "$S/evidence" >"$P/private-path-scan.txt"; then exit 2; fi
for claim in authority-finality publication recovery; do
 awk '/^artifacts:$/ {on=1; next} on && /^  - / {print $2; next} on {exit}' "docs/claims/casper-soak-$claim.md" |
 while IFS= read -r artifact; do
  id="${artifact//\//-}"; id="${id//./-}"; id="${id//_/-}"
  path="docs/casper/cbc-evidence/$id.md"
  [[ -f "$path" && ! -L "$path" ]]
  mkdir -p "$S/ledgers/$(dirname "$path")"
  cp "$path" "$S/ledgers/$path"
 done
done
[[ "$(find "$S/ledgers" -type f | wc -l | tr -d ' ')" == 36 ]]
for kind in source evidence ledgers; do
 (cd "$S/$kind"; find . -type f | LC_ALL=C sort | while IFS= read -r file; do printf '%s\0' "${file#./}"; done | xargs -0 shasum -a 256) >"$P/$kind-files.sha256"
 COPYFILE_DISABLE=1 gtar --owner=0 --group=0 --numeric-owner --no-xattrs --no-acls -czf "$P/$kind.tar.gz" -C "$S/$kind" .
done
jq -n --arg base "$(<"$E/base.txt")" --arg head "$(<"$E/final-head.txt")" --arg time "$(date -u +%FT%TZ)" --argjson sources "$(wc -l <"$P/source-files.sha256")" --argjson evidence "$(wc -l <"$P/evidence-files.sha256")" '{
 schema_version:1,status:"verified-binding-acceptance-pending",scope:"bounded-controlled-transcript-profiles",phase:"pre_pr216_merge",base:$base,observed_head:$head,verified_at:$time,
 claims:["CLAIM-CASPER-SOAK-002","CLAIM-CASPER-SOAK-003","CLAIM-CASPER-SOAK-004"],claim_discharge:"pending",binding_acceptance:null,tasks_complete:false,
 profiles:[{profile:"authority-finality",tests:8,cases:58,invocations:65,threshold_cases:2000,generated_states:3281,distinct_states:1681},{profile:"publication",tests:6,cases:79,invocations:83,new_regressions:22,generated_states:3281,distinct_states:1681},{profile:"recovery",tests:7,cases:52,invocations:55,generated_states:841,distinct_states:441}],
 platforms:["native-macos","isolated-linux-aarch64-musl"],models:{clean:3,named_negatives:9,scenarios:2,observation_slots:3,negative_exit:12},shared_native_tests:11,
 publication_red:{exit:101,expected_profile_exit:2,observed_profile_exit:0,observed_verdict:"passed"},publication_green:true,
 audits:{ordinary:0,strict_001:4,strict_002:4,strict_003:4,strict_004:4,strict_bundle:4},
 lifecycle:{status:"pending",cause:"prior-host-control-function-move",changed_by_this_review:false,prior_67_source_check:"failed-four-declared-differences"},
 pending_workflow_tags:[".github/workflows/casper-authority-finality.yml",".github/workflows/casper-publication.yml",".github/workflows/casper-recovery.yml"],tags_applied:false,
 limits:["D-07 interpretation unresolved","Live adapters unqualified","Binary bytes not archived; executable hashes retained","Bounded models do not prove node correctness","Containment assumptions remain","Post-merge execution blocked"],
 construction:"not-applicable",node_execution:false,policy_activation:false,soak_verdict:"non_passing",source_files:$sources,evidence_files:$evidence,previous_ledgers:36,candidate_ledgers:36
}' >"$P/report.json"
report_hash="$(shasum -a 256 "$P/report.json" | awk '{print $1}')"
ledger_hash="$(shasum -a 256 "$P/ledgers.tar.gz" | awk '{print $1}')"
for old in "$S/ledgers/docs/casper/cbc-evidence/"*.md; do
 data="$(awk '/^```json$/ {on=1;next} on && /^```$/ {exit} on {print}' "$old")"
 artifact="$(jq -r '.artifact.path' <<<"$data")"
 claim="$(jq -r '.claim' <<<"$data")"
 sha="$(shasum -a 256 "$artifact" | awk '{print $1}')"
 spec="$(shasum -a 256 "$claim" | awk '{print $1}')"
 member="docs/casper/cbc-evidence/$(basename "$old")"
 {
  printf '# Pending CbC Candidate: %s\n\nHuman binding acceptance remains pending. This record does not discharge a claim.\n\n```json\n' "$artifact"
  jq --arg sha "$sha" --arg spec "$spec" --arg commit "$(<"$E/final-head.txt")" --arg ref "$P/report.json" --arg report "$report_hash" --arg archive "$P/ledgers.tar.gz" --arg ledger "$ledger_hash" --arg member "$member" --arg time "$(jq -r '.verified_at' "$P/report.json")" '.artifact.sha256=$sha | .artifact.commit=$commit | .artifact.commit_is_base=true | .claim_digests[.claim]=$spec | .status="pending" | .evidence.ref=$ref | .evidence.sha256=$report | .tiers.binding="pending-review" | .phase_status.pre_pr216_merge="pending" | .phase_status.post_pr216_merge="blocked" | .soak="pending" | .waiver=null | .verified_at=$time | .previous_ledger={archive:$archive,sha256:$ledger,member:$member}' <<<"$data"
  printf '```\n'
 } >"$P/candidate-ledgers/$(basename "$old")"
done
(cd "$P"; find candidate-ledgers -type f | LC_ALL=C sort | while IFS= read -r path; do shasum -a 256 "$path"; done) >"$P/candidate-ledgers.sha256"
(cd "$P"; shasum -a 256 report.json source.tar.gz evidence.tar.gz ledgers.tar.gz source-files.sha256 evidence-files.sha256 ledgers-files.sha256 candidate-ledgers.sha256 redactions.tsv private-path-scan.txt retain.sh validate.sh redact.rs >artifacts.sha256)
printf 'The pending review package is retained.\n'
