#!/usr/bin/env bash
set -euo pipefail
P=docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01
E=target/casper-profile-acceptance
S=target/casper-profile-acceptance-retention
[[ ! -e "$P/evidence.tar.gz" && ! -e "$S" ]]
mkdir -p "$S/source" "$S/evidence"
[[ "$(<"$E/refresh.exit")" == 0 && "$(<"$E/closure.exit")" == 0 ]]
for profile in authority-finality publication recovery shared-regressions; do [[ "$(<"$E/$profile.exit")" == 0 ]]; done
cp -R "$E/." "$S/evidence/"
if [[ -L "$S/evidence/closure/lib" ]]; then
 library="$(readlink "$S/evidence/closure/lib")"
 unlink "$S/evidence/closure/lib"
 mkdir "$S/evidence/closure/lib"
 for name in integrity-walker epic-parser user-flow-parser story-parser; do cp "$library/$name.sh" "$S/evidence/closure/lib/"; done
fi
find "$S/evidence" -type l | while IFS= read -r link; do
 target="$(readlink "$link")"
 [[ "$target" != /* ]]
 printf '%s\t%s\n' "${link#"$S/evidence/"}" "$target"
done >"$P/symlinks.tsv"
target/casper-profile-review-retention/redact-bin "$S/evidence" >"$P/redactions.tsv"
if grep -rIlE '/Users/[[:alnum:]_.-]+/|/home/[[:alnum:]_.-]+/|/var/folders/[[:alnum:]_-]+/' "$S/evidence" >"$P/private-path-scan.txt"; then exit 2; fi
{
 jq -r '(.source_digests,.claim_digests)|keys[]' "$P/report.json"
 printf '%s\n' .gitattributes docs/ToDos.md docs/UserStories.md docs/User-Flows.md docs/work-logs/task-017-{5-authority-finality,6-publication,7-recovery,5-7-binding-review,5-7-acceptance}.md
 printf '%s\n' "$P/"*.sh
} | LC_ALL=C sort -u >"$S/source-paths.txt"
while IFS= read -r path; do
 mkdir -p "$S/source/$(dirname "$path")"
 cp "$path" "$S/source/$path"
done <"$S/source-paths.txt"
for kind in source evidence; do
 (cd "$S/$kind"; find . -type f | LC_ALL=C sort | while IFS= read -r file; do printf '%s\0' "${file#./}"; done | xargs -0 shasum -a 256) >"$P/$kind-files.sha256"
 COPYFILE_DISABLE=1 gtar --owner=0 --group=0 --numeric-owner --no-xattrs --no-acls -czf "$P/$kind.tar.gz" -C "$S/$kind" .
done
(cd "$P"; find accepted-ledgers -type f | LC_ALL=C sort | while IFS= read -r path; do shasum -a 256 "$path"; done) >"$P/accepted-ledgers.sha256"
(cd "$P"; shasum -a 256 report.json review-validation.json source.tar.gz evidence.tar.gz source-files.sha256 evidence-files.sha256 accepted-ledgers.sha256 redactions.tsv symlinks.tsv private-path-scan.txt *.sh >artifacts.sha256)
printf 'The acceptance and completion evidence is retained.\n'
