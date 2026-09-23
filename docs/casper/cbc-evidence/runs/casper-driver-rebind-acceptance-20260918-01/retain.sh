#!/usr/bin/env bash
set -euo pipefail
RUN=docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01
REVIEW=docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01
WORK=target/casper-driver-acceptance
STAGE=target/casper-driver-acceptance-retention
[[ ! -e "$STAGE" ]]
mkdir -p "$STAGE"/{source,evidence,ledgers}
jq -r '.source_digests | keys[]' "$RUN/report.json" | while IFS= read -r path; do
 mkdir -p "$STAGE/source/$(dirname "$path")"
 cp "$path" "$STAGE/source/$path"
done
for candidate in "$REVIEW"/candidate-ledgers/*.md; do
 path="docs/casper/cbc-evidence/${candidate##*/}"
 mkdir -p "$STAGE/ledgers/$(dirname "$path")"
 cp "$path" "$STAGE/ledgers/$path"
 shasum -a 256 "$path"
done >"$RUN/ledgers.sha256"
while IFS= read -r path; do shasum -a 256 "$path"; done <"$RUN/preserved-compatibility-records.txt" >"$RUN/preserved-compatibility.sha256"
for name in accepted-state accepted-final host-repeat; do cp -R "$WORK/$name" "$STAGE/evidence/"; done
for path in "$WORK"/*; do
 [[ -f "$path" ]] || continue
 [[ "${path##*/}" != startup-probe ]] || continue
 cp "$path" "$STAGE/evidence/"
done
cp "$WORK/preapproval/validation.json" "$STAGE/evidence/preapproval-validation.json"
cp docs/work-logs/casper-driver-rebind-acceptance.md "$STAGE/evidence/acceptance.md"
find "$STAGE/evidence" -name '*.tar.gz' -type f -delete
target/casper-task-017-4-finish/redact-bin "$STAGE" "$PWD" "$HOME" >"$RUN/redactions.tsv"
(cd "$STAGE/evidence" && find . -type f | sort | while IFS= read -r path; do shasum -a 256 "${path#./}"; done) >"$RUN/evidence-files.sha256"
for tree in source evidence ledgers; do
 COPYFILE_DISABLE=1 gtar --owner=0 --group=0 --numeric-owner --no-xattrs --no-acls -czf "$RUN/$tree.tar.gz" -C "$STAGE/$tree" .
done
cp "$WORK/retain.sh" "$RUN/retain.sh"
cp "$WORK/lsp.json" "$RUN/lsp.json"
