#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mapfile -d '' documentation < <(
  find \
    docs/casper \
    docs/crypto \
    docs/models \
    docs/node \
    docs/rholang \
    docs/casper/theory/cost-accounting-impl \
    docs/casper/theory/finalized-floor \
    docs/casper/theory/fork-choice \
    docs/casper/theory/slashing \
    -type f -name '*.md' -print0
  find docs/casper/theory -maxdepth 1 -type f -name 'cost-account*.md' -print0
  find docs/formal-verification.md formal/README.md -type f -name '*.md' -print0
)

if [[ "${#documentation[@]}" -eq 0 ]]; then
  printf 'error: no cost-accounting or Casper documentation found\n' >&2
  exit 1
fi

failures=0
for document in "${documentation[@]}"; do
  if ! perl -ne '
    if (/^```/) {
      $fenced = !$fenced;
      next;
    }
    next if $fenced;
    $line = $_;
    $line =~ s/\$`[^`]*`\$//g;
    $line =~ s/`[^`]*`//g;
    if ($line =~ /\$\$/) {
      print "$ARGV:$.: bare double-dollar math delimiter\n";
      $failed = 1;
    }
    while ($line =~ /(?<!\$)\$([^\n\$]+)\$(?!\$)/g) {
      print "$ARGV:$.: bare inline math delimiter: $&\n";
      $failed = 1;
    }
    END {
      if ($fenced) {
        print "$ARGV: unclosed fenced code block\n";
        $failed = 1;
      }
      exit($failed ? 1 : 0);
    }
  ' "$document"; then
    failures=$((failures + 1))
  fi
done

if [[ "$failures" -ne 0 ]]; then
  printf 'error: pgmcp documentation syntax failed for %s file(s)\n' "$failures" >&2
  exit 1
fi

printf 'pgmcp documentation syntax passed for %s files.\n' "${#documentation[@]}"
