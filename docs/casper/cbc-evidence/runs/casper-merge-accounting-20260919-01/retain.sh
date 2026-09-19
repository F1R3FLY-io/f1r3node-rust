#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
P=docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01
WORK=target/task-017-8
STAGE="${1:?Supply a new staging directory.}"
[[ ! -e "$STAGE" && ! -e "$P/report.json" ]]
mkdir -p "$STAGE/source" "$STAGE/evidence"
STAGE="$(cd "$STAGE" && pwd)"
for f in native-recorded linux-run linux-build shared independent-green ste; do [[ "$(<"$WORK/$f.exit")" == 0 ]]; done
[[ "$(<"$WORK/independent-red.exit")" == 101 ]]
for n in 001 002 003 004; do [[ "$(<"$WORK/claim-$n.exit")" == 0 ]]; done
[[ "$(<"$WORK/claim-005.exit")" == 4 ]]
shasum -a 256 -c "$WORK/native-recorded/source-before.sha256" >"$WORK/retention-source-check.txt"
cp -R "$WORK/." "$STAGE/evidence/"
awk '/^artifacts:/{active=1;next} active && /^  - /{print substr($0,5);next} active {exit}' docs/claims/casper-soak-merge-accounting.md >"$P/artifact-paths.txt"
[[ "$(wc -l <"$P/artifact-paths.txt" | tr -d ' ')" == 12 ]]
{ awk '{print $2}' "$WORK/native-recorded/source-before.sha256"; printf '%s\n' docs/work-logs/task-017-8-merge-accounting.md "$P/retain.sh" "$P/validate.sh" "$P/redact.rs"; } | LC_ALL=C sort -u >"$P/source-paths.txt"
while IFS= read -r path; do mkdir -p "$STAGE/source/$(dirname "$path")"; cp "$path" "$STAGE/source/$path"; done <"$P/source-paths.txt"
while IFS= read -r path; do shasum -a 256 "$path"; done <"$P/artifact-paths.txt" >"$P/profile-artifacts.sha256"
cp "$P/profile-artifacts.sha256" "$STAGE/evidence/"
rustc --edition 2021 "$P/redact.rs" -o "$STAGE/redact"
"$STAGE/redact" "$STAGE/source" | awk '{print "source/" $0}' >"$P/redactions.tsv"
"$STAGE/redact" "$STAGE/evidence" | awk '{print "evidence/" $0}' >>"$P/redactions.tsv"
find "$STAGE/source" "$STAGE/evidence" -type l -print | LC_ALL=C sort | while IFS= read -r path; do
 target="$(readlink "$path")"
 [[ "$target" != /* ]]
 printf '%s\t%s\n' "${path#"$STAGE/"}" "$target"
done >"$P/symlinks.txt"
if grep -RIlF -e "$HOME" -e "$ROOT" "$STAGE/source" "$STAGE/evidence" >"$P/private-path-scan.txt"; then exit 2; fi
for kind in source evidence; do
 (cd "$STAGE/$kind"; find . -type f | LC_ALL=C sort | while IFS= read -r file; do shasum -a 256 "$file"; done) >"$P/$kind-files.sha256"
 COPYFILE_DISABLE=1 gtar --owner=0 --group=0 --numeric-owner --no-acls --no-xattrs -czf "$P/$kind.tar.gz" -C "$STAGE/$kind" .
done
SHA="$(shasum -a 256 docs/claims/casper-soak-merge-accounting.md | awk '{print $1}')"
DIGESTS="$(awk '{print $1 "\t" $2}' "$P/profile-artifacts.sha256" | jq -Rn '[inputs | split("\t") | {key:.[1],value:.[0]}] | from_entries')"
jq -n --arg spec "$SHA" --arg head "$(git rev-parse HEAD)" --argjson sources "$DIGESTS" --argjson source_count "$(wc -l <"$P/source-files.sha256")" --argjson evidence_count "$(wc -l <"$P/evidence-files.sha256")" '{schema_version:1,claim:"CLAIM-CASPER-SOAK-005",status:"verified-pending-binding-review",claim_discharge:"pending",binding:"pending",workflow_tag:"proposed-not-applied",phase:"pre_pr216_merge",scope:"bounded-controlled-transcript-merge-accounting",specification:"docs/claims/casper-soak-merge-accounting.md",specification_sha256:$spec,source_digests:$sources,source_files:$source_count,evidence_files:$evidence_count,start_revision:"09b0a60063815b750979864225a0a56387d6ed80",packaging_revision:$head,revision_is_base:true,platforms:{native:{tests:7,cases:68,invocations:73,optimization:0},isolated_linux_arm64:{tests:7,cases:68,invocations:73,optimization:1}},shared_tests:11,refutation:{status:"bounded-safety-pass",scenarios:2,observations:3,controls:4,clean:{generated:841,distinct:441},negative_exits:[12,12,12]},construction:"not-applicable",strict_audits:{claims_001_004:0,claim_005:4},regressions:{independent_failure_red:101,focused_green:0},failures_retained:["initial-model-configuration-refusal","absolute-path-retry-same-configuration-refusal","independent-failure-regression"],initial_model_archive_provenance:"reconstructed-from-442e93faa-not-runtime-capture",live_adapters:"unqualified",post_merge_execution:"blocked",policy_activation:false,node_execution:false,soak:"pending",waiver:null}' >"$P/report.json"
REPORT="$(shasum -a 256 "$P/report.json" | awk '{print $1}')"
mkdir "$P/ledgers"
while IFS= read -r path; do
 slug="$(printf '%s' "$path" | awk '{sub(/^\./, ""); gsub(/[\/._]/,"-"); print}')"
 ledger="docs/casper/cbc-evidence/$slug.md"
 [[ ! -e "$ledger" && ! -L "$ledger" && ! -e "docs/cbc-evidence/$slug.md" && ! -L "docs/cbc-evidence/$slug.md" ]]
 digest="$(shasum -a 256 "$path" | awk '{print $1}')"
 { printf '# CbC Evidence: %s\n\nThe bounded checks passed. Human binding acceptance remains pending.\n\n```json\n' "$path"; jq -n --arg path "$path" --arg slug "$slug" --arg sha "$digest" --arg spec "$SHA" --arg report "$REPORT" --arg ref "$P/report.json" --arg commit "$(git rev-parse HEAD)" --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '{artifact:{path:$path,id:$slug,commit:$commit,commit_is_base:true,sha256:$sha},claim:"docs/claims/casper-soak-merge-accounting.md",claim_ids:["CLAIM-CASPER-SOAK-005"],claim_digests:{"docs/claims/casper-soak-merge-accounting.md":$spec},adapter:"embedded",status:"pending",scope:"bounded-merge-accounting-profile",evidence:{kind:"bounded-refutation-and-executable-fixtures",ref:$ref,sha256:$report},tiers:{refutation:"bounded-safety-pass",construction:"not-applicable",binding:"pending"},phase_status:{pre_pr216_merge:"pending",post_pr216_merge:"blocked"},soak:"pending",waiver:null,verified_at:$timestamp}'; printf '\n```\n'; } >"$ledger"
 cp "$ledger" "$P/ledgers/$slug.md"
 ln -s "../casper/cbc-evidence/$slug.md" "docs/cbc-evidence/$slug.md"
done <"$P/artifact-paths.txt"
(cd "$P"; find ledgers -type f | LC_ALL=C sort | while IFS= read -r file; do shasum -a 256 "$file"; done) >"$P/ledgers.sha256"
(cd "$P"; find . -maxdepth 1 -type f ! -name artifacts.sha256 | LC_ALL=C sort | while IFS= read -r file; do shasum -a 256 "$file"; done) >"$P/artifacts.sha256"
printf 'The pending review package is retained.\n'
