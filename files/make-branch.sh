#!/usr/bin/env bash
#
# make-branch.sh - construct the Plausible Fiction prototype branch.
#
# Base is 91b5c70a, the where-clauses-and-match-guards merge. That is the
# EXACT commit Embers@dylon/embers-demo-fixes pins for f1r3node-models, so
# the branch starts at parity with the node the SF and August demos ran
# against. It is also the last commit before 0f6ee989 introduced a root-level
# path dependency and a [patch] stanza pointing at
# ../rholang-rs-cost-accounting-transpiler/ - a directory that exists on
# exactly one machine. Basing here means never having to remove them.
#
# The DDL work (9c57a82b) is cherry-picked on top. It touches only new files
# under module-syntax/, so the pick is clean.
#
#   usage:  ./make-branch.sh /path/to/f1r3node-rust [branch-name] [parser-rev]
#
# If you have already run `git checkout -b <branch> 91b5c70a` yourself, pass
# the same name and the script will use the branch you are on.
#
# The parser rev is the rholang-rs commit carrying the MeTTaIL DDL grammar
# delta - see rholang-rs/README.md. Pass it once it exists; until then the
# script leaves the existing pin (c163755, public) alone and says so.
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${1:?usage: make-branch.sh /path/to/f1r3node-rust [branch] [parser-rev]}"
BRANCH="${2:-feature/theory-syntax}"
PARSER_REV="${3:-}"

# The where-clauses merge: clean, public, and what Embers pins.
BASE_REV="${BASE_REV:-91b5c70a0740f91c3ba2a414af3e29fe830263c3}"
# "documentation and initial implementation for module syntax"
DDL_COMMIT="${DDL_COMMIT:-9c57a82b9ceaf798b361918369056312a33b051c}"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[ -d "$REPO/.git" ] || die "$REPO is not a git repository"
[ -f "$HERE/files/mettail-elab/examples/modules/PFLam.module" ] \
  || die "run this from the bundle directory; files/ is missing"

cd "$REPO"

# ---------------------------------------------------------------- 0. checks
git diff --quiet && git diff --cached --quiet \
  || die "working tree is dirty; commit or stash first"

ON_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
REUSE=no
if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  # Support the "I already made the branch" workflow, but only if it is
  # actually at the base -- otherwise we would be layering onto something
  # unknown.
  [ "$ON_BRANCH" = "$BRANCH" ] \
    || die "branch $BRANCH exists but is not checked out; switch to it or pick another name"
  [ "$(git rev-parse HEAD)" = "$BASE_REV" ] \
    || die "$BRANCH is not at $BASE_REV; reset it there or start a fresh branch"
  REUSE=yes
fi

# Do not reuse feature/mettail: that name is Dylon's July RSpace and
# substitution-performance work, which has nothing to do with the DDL.
[ "$BRANCH" = "feature/mettail" ] \
  && die "feature/mettail is taken by unrelated RSpace work; pick another name"

say "fetching"
git fetch -q origin

git cat-file -e "$BASE_REV^{commit}" 2>/dev/null \
  || die "base commit $BASE_REV not found; fetch more history"
git cat-file -e "$DDL_COMMIT^{commit}" 2>/dev/null \
  || git fetch -q origin feature/module-syntax \
  || die "cannot reach $DDL_COMMIT; fetch feature/module-syntax"

if [ "$REUSE" = "yes" ]; then
  say "using the branch you are on ($BRANCH, at the base)"
else
  say "branching $BRANCH off $BASE_REV"
  git checkout -q -b "$BRANCH" "$BASE_REV"
fi

# Sanity: the base must NOT carry the dev-only wiring 0f6ee989 introduced.
if grep -q 'rholang-rs-cost-accounting-transpiler' Cargo.toml 2>/dev/null; then
  die "the base carries a path dependency on a sibling worktree; wrong base"
fi

say "cherry-picking the DDL commit $DDL_COMMIT"
git cherry-pick -n "$DDL_COMMIT"

[ -d module-syntax/mettail-elab ] \
  || die "module-syntax/mettail-elab not found after the pick"

# --------------------------------------------- 1. promote the elaborator
say "promoting module-syntax/mettail-elab -> mettail-elab"
git mv module-syntax/mettail-elab mettail-elab
rm -f mettail-elab/Cargo.toml~
git rm --cached --ignore-unmatch -q 'mettail-elab/Cargo.toml~' || true

say "moving documentation -> docs/mettail"
mkdir -p docs/mettail
git mv module-syntax/documentation/mettail-ddl-and-modules-2026-08-19.md docs/mettail/
git mv module-syntax/documentation/mettail-for-developers.pdf docs/mettail/
git mv module-syntax/documentation/mettail-for-developers-source.tar.gz docs/mettail/

say "keeping the grammar delta as rholang-rs/mettail-ddl.patch"
mkdir -p rholang-rs
git mv module-syntax/rholang-rs-mettail-ddl.patch rholang-rs/mettail-ddl.patch

rmdir module-syntax/documentation module-syntax 2>/dev/null || true

# ----------------------------------------- 2. confirm the tree is portable
#
# At this base there is no [patch] stanza and no path dependency to remove --
# that is the point of basing here. Assert it rather than assume it.
say "confirming the parser wiring is public"
python3 - <<'PY'
import pathlib, re, sys
bad = []
for f in ("Cargo.toml", "rholang/Cargo.toml"):
    p = pathlib.Path(f)
    if not p.exists():
        continue
    src = p.read_text()
    if re.search(r'\[patch\."https://github\.com/F1R3FLY-io/rholang-rs"\]', src):
        bad.append(f"{f}: [patch] stanza present")
    if "rholang-rs-cost-accounting-transpiler" in src:
        bad.append(f"{f}: path dependency on a sibling worktree")
if bad:
    for b in bad:
        print("    " + b, file=sys.stderr)
    sys.exit(1)
print("    clean: rholang-parser resolves from a public git rev")
PY

# --------------------------------------------- 3. repoint the parser
if [ -n "$PARSER_REV" ]; then
  say "pinning rholang-parser at $PARSER_REV"
  for f in Cargo.toml rholang/Cargo.toml; do
    [ -f "$f" ] || continue
    python3 - "$f" "$PARSER_REV" <<'PY'
import re, sys, pathlib
path, rev = sys.argv[1], sys.argv[2]
p = pathlib.Path(path)
src = p.read_text()
new, n = re.subn(
    r'(rholang-parser\s*=\s*\{[^}]*?rev\s*=\s*")[0-9a-f]+(")',
    lambda m: m.group(1) + rev + m.group(2),
    src,
)
if n:
    p.write_text(new)
    print(f"    {path}: {n} pin(s) updated")
PY
  done
else
  say "no parser rev given - leaving the existing pin (c163755, public)"
  echo "    The DDL grammar delta is NOT yet reflected in the parser."
  echo "    Theory/Module source will not parse in the node until you build"
  echo "    the rholang-rs branch and re-run with its rev. The elaborator and"
  echo "    its corpus do not depend on this and build and test now."
fi

# --------------------------------------------- 4. drop in the new files
say "installing the Plausible Fiction corpus, tests, CI and docs"
mkdir -p mettail-elab/examples/modules/bad mettail-elab/tests \
         .github/workflows docs/mettail scripts

cp "$HERE/files/mettail-elab/examples/modules/PFLam.module"       mettail-elab/examples/modules/
cp "$HERE/files/mettail-elab/examples/modules/PricedPFLam.module" mettail-elab/examples/modules/
cp "$HERE/files/mettail-elab/examples/modules/bad/CaseMotiveUnused.module" \
                                                                  mettail-elab/examples/modules/bad/
cp "$HERE/files/mettail-elab/tests/pflam.rs"                      mettail-elab/tests/
cp "$HERE/files/mettail-elab/rust-toolchain.toml"                 mettail-elab/
cp "$HERE/files/.github/workflows/mettail-elab.yml"               .github/workflows/
cp "$HERE/files/docs/mettail/README.md"                           docs/mettail/
cp "$HERE/check-modules.py"                                       scripts/
chmod +x scripts/check-modules.py

# --------------------------------------------- 5. pre-flight
say "pre-flight checking the module corpus"
python3 scripts/check-modules.py mettail-elab/examples/modules/*.module

say "staging"
git add -A

cat <<EOF

  Branch $BRANCH is staged but not committed. Review with:

      git -C "$REPO" status
      git -C "$REPO" diff --cached --stat

  Suggested commit message:

      feat(mettail): Theory-syntax elaborator and the Plausible Fiction corpus

      Base is 91b5c70a, the commit Embers pins for f1r3node-models, so the
      branch starts at parity with the demoed node and inherits neither the
      [patch] stanza nor the sibling-worktree path dependency that 0f6ee989
      introduced.

      Cherry-picks the module-syntax work and promotes mettail-elab out of its
      staging directory into a top-level, self-contained crate so Embers can
      consume it as a git dependency (D6/D7: elaboration is client-side at
      compile time). Pins the crate to stable, since rustup would otherwise
      inherit the root nightly. Adds PFLam and PricedPFLam as elaborable
      modules, their corpus tests, a G6 negative for the unreferenced case
      motive, and a CI job that keeps the dependency surface empty.

  Then, before anything downstream:

      cd "$REPO/mettail-elab" && cargo test

EOF
