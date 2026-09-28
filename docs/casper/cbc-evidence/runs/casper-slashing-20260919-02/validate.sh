#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 ]] || { printf 'Usage: %s OUTPUT\n' "$0" >&2; exit 2; }
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
PACKAGE=docs/casper/cbc-evidence/runs/casper-slashing-20260919-02
OUT="$1"
[[ ! -e "$OUT" && ! -L "$OUT" ]]
mkdir -p "$OUT/source" "$OUT/evidence"
OUT="$(cd "$OUT" && pwd)"
check_digest() {
    local file="$1" expected="$2"
    [[ -f "$file" && ! -L "$file" ]]
    local actual mapped
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
    if [[ "$actual" == "$expected" ]]; then return; fi
    [[ "$file" == "$OUT/evidence/"* ]]
    mapped="$(awk -F '\t' -v path="${file#"$OUT/evidence/"}" -v before="$expected" '$1==path && $2==before {print $3}' "$PACKAGE/redactions.tsv")"
    [[ "$mapped" == "$actual" ]]
}
check_digest "$PACKAGE/redactions.tsv" 5f388fe40df5258169060a51dda580332ea46eeb3a66c4b40fb64de4d66e1665
for kind in source evidence; do
    check_digest "$PACKAGE/$kind.tar.gz" "$(jq -r ".archives.$kind.sha256" "$PACKAGE/report.json")"
    tar -xzf "$PACKAGE/$kind.tar.gz" -C "$OUT/$kind"
    (cd "$OUT/$kind" && shasum -a 256 -c "$ROOT/$PACKAGE/$kind.sha256") > "$OUT/$kind-check.txt"
done
shasum -a 256 -c "$PACKAGE/source.sha256" > "$OUT/current-source.txt"
shasum -a 256 -c "$PACKAGE/artifacts.sha256" > "$OUT/current-artifacts.txt"
[[ ! -s "$PACKAGE/private-path-scan.txt" ]]
claim_sha="$(shasum -a 256 docs/claims/casper-soak-slashing.md | awk '{print $1}')"
report_sha="$(shasum -a 256 "$PACKAGE/report.json" | awk '{print $1}')"
jq -e --arg claim "$claim_sha" '.claim_digests["docs/claims/casper-soak-slashing.md"]==$claim and .status=="bounded-checks-passed-binding-pending" and .tiers.binding=="pending" and .node_execution==false and .workflow_tag_ratified==false' "$PACKAGE/report.json" >/dev/null
while IFS= read -r path; do
    slug="$(printf '%s' "${path#.}" | tr '/._' '-')"
    ledger="docs/casper/cbc-evidence/$slug.md"
    compat="docs/cbc-evidence/$slug.md"
    [[ -f "$ledger" && ! -L "$ledger" && -L "$compat" && "$(readlink "$compat")" == "../casper/cbc-evidence/$slug.md" ]]
    awk '/^```json$/{a=1;next} /^```$/{a=0} a{print}' "$ledger" | jq -e --arg path "$path" --arg sha "$(shasum -a 256 "$path" | awk '{print $1}')" --arg claim "$claim_sha" --arg report "$report_sha" '.artifact.path==$path and .artifact.sha256==$sha and .claim_digests["docs/claims/casper-soak-slashing.md"]==$claim and .evidence.sha256==$report and .claim_ids==["CLAIM-CASPER-SOAK-006"] and .status=="pending" and .tiers.binding=="pending" and .waiver==null' >/dev/null
done < "$PACKAGE/artifact-files.txt"
EVIDENCE="$OUT/evidence/target/task-017-9"
nested=0
for platform in native-recorded linux-final/evidence; do
    fixtures="$EVIDENCE/$platform/fixtures"
    find "$fixtures" -mindepth 2 -maxdepth 2 -type f -name 'invocation-*.json' | while IFS= read -r path; do printf '%s\n' "${path#"$fixtures/"}"; done | LC_ALL=C sort > "$OUT/invocations.txt"
    cmp "$EVIDENCE/native-recorded/expected-invocations.txt" "$OUT/invocations.txt"
    [[ "$(wc -l < "$OUT/invocations.txt" | tr -d ' ')" == 72 ]]
    while IFS= read -r path; do
        jq -e '.actual_exit==.expected_exit and (.stdout|fromjson|.scenario_verdict)==.expected_verdict and (.stdout|fromjson|.node_launch_count==0 and .soak_verdict=="non_passing")' "$fixtures/$path" >/dev/null
    done < "$OUT/invocations.txt"
    find "$fixtures" -mindepth 3 -maxdepth 3 -type f -name report.json | LC_ALL=C sort > "$OUT/reports.txt"
    while IFS= read -r report; do
        jq -r '(.retained_sources + .retained_inputs)[] | [.retained,.sha256] | @tsv' "$report" > "$OUT/references.tsv"
        while IFS=$'\t' read -r retained digest; do
            [[ "$retained" != /* && "$retained" != *..* ]]
            check_digest "$(dirname "$report")/$retained" "$digest"
            nested=$((nested+1))
        done < "$OUT/references.tsv"
        jq -r '.profile_identity.source_digests | to_entries[] | [.key,.value] | @tsv' "$report" > "$OUT/sources.tsv"
        while IFS=$'\t' read -r source digest; do check_digest "$source" "$digest"; done < "$OUT/sources.tsv"
    done < "$OUT/reports.txt"
done
jq -e '.[0] | .Config.User=="65534:65534" and .HostConfig.NetworkMode=="none" and .HostConfig.CapDrop==["ALL"] and (.Mounts|length)==0 and .HostConfig.PidsLimit==128 and .HostConfig.Memory==536870912 and .State.ExitCode==0 and .State.OOMKilled==false' "$EVIDENCE/linux-final/finished.json" >/dev/null
jq -e '.checks=="passed" and .required_rust_tests==7 and .required_cases==66 and .required_invocations==72 and .required_model_controls==4' "$EVIDENCE/native-recorded/summary.json" >/dev/null
MODELS="$EVIDENCE/native-recorded/models"
jq -e '.status=="passed" and ([.results[].exit]==[0,12,12,12]) and ([.results[].expected_property]==[null,"EvidenceOrderRecorded","EpochCorrelationRequired","AuthorizationMismatchReported"]) and (.results|all(.outcome=="passed"))' "$MODELS/report.json" >/dev/null
jq -r '.results[] | [.log,.log_sha256] | @tsv' "$MODELS/report.json" > "$OUT/models.tsv"
while IFS=$'\t' read -r path digest; do check_digest "$MODELS/$path" "$digest"; done < "$OUT/models.tsv"
for name in rename interrupt; do
    expected=1; [[ "$name" != interrupt ]] || expected=143
    jq -e --argjson code "$expected" '.checks=="failed" and .exit==$code' "$EVIDENCE/runner-controls/$name/summary.json" >/dev/null
done
[[ "$(git check-attr cbc -- .github/workflows/casper-slashing.yml)" == '.github/workflows/casper-slashing.yml: cbc: unspecified' ]]
for claim in 001 002 003 004 005 006; do
    code=0
    target/debug/check-casper-claims --root "$ROOT" --output "$OUT/claim-$claim.json" --claim "CLAIM-CASPER-SOAK-$claim" --strict > "$OUT/claim-$claim.txt" 2>&1 || code=$?
    expected=0; [[ "$claim" != 006 ]] || expected=4
    [[ "$code" == "$expected" ]]
done
code=0
target/debug/check-casper-claims --root "$ROOT" --output "$OUT/bundle.json" --strict > "$OUT/bundle.txt" 2>&1 || code=$?
[[ "$code" == 4 ]]
jq -n --arg report "$report_sha" --arg validator "$(shasum -a 256 "$PACKAGE/validate.sh" | awk '{print $1}')" --argjson nested "$nested" --argjson sources "$(wc -l < "$PACKAGE/source.sha256" | tr -d ' ')" --argjson evidence "$(wc -l < "$PACKAGE/evidence.sha256" | tr -d ' ')" '{status:"bounded-package-verified-binding-pending",report_sha256:$report,validator_sha256:$validator,source_files:$sources,evidence_files:$evidence,artifacts:12,pending_ledgers:12,nested_references:$nested,native:{tests:7,cases:66,invocations:72},isolated_linux:{tests:7,cases:66,invocations:72},model_controls:4,strict_claims_001_005:0,strict_claim_006:4,strict_bundle:4,workflow_tag_ratified:false,node_execution:false,post_merge_execution:"blocked",soak:"pending"}' > "$OUT/validation.json"
