# Q-35: the `dev` merge needs rholang-rs master 4e22339 in `rev-stacks/syntax-dev` (lane A2 to lane B)

**Label:** *blocking* under Decision 88. The `dev` merge into `patch/FX-8b` (step 1 of the 2026-11-06 order) does not compile at 743752b.

## Finding

- `dev` at 3b5daabb4 pins rholang-rs master at **4e22339** ("Merge pull request #101 from F1R3FLY-io/feature/coc-lambda-pi-types"). That commit adds `Proc::Lambda`, `Proc::App`, `Proc::Pi`, `SimpleType::Type`, and `ASTBuilder::alloc_lambda` and `alloc_app`.
- `rev-stacks/syntax-dev` at **743752b** merged master only up to **dd6f611** (pull request #100).
- With the pin at 743752b, `dev`'s `normalize.rs`, `p_simple_type_normalizer.rs` and `test_utils/par_builder_util.rs` do not compile.

## Trial merge (local only, not pushed)

Lane A2 merged 4e22339 into 743752b in a scratch clone:

1. The grammar sources (`grammar.js`, `src/grammar.json`) and all Rust sources merge without conflict.
2. Three generated files conflict: `rholang-tree-sitter/src/parser.c`, `rholang-tree-sitter/src/node-types.json` and `rholang-parser/tests/corpus/golden_snapshots/golden__Cell3_bad.snap`.
3. `tree-sitter generate` with tree-sitter-cli 0.25.10 (the version in the header of `parser.c`) regenerates `parser.c` and `node-types.json` from the merged grammar.
4. The trial takes master's `golden__Cell3_bad.snap` without a review. Lane B must check this snapshot.
5. `rholang-parser` builds. f1r3node-rust (`patch/FX-8b` plus `dev`) builds against the trial commit, and `cargo test --release -p rholang` gives 2,542 passed, 0 failed, 70 ignored.

## Requests to lane B

1. Merge rholang-rs master 4e22339 into `rev-stacks/syntax-dev`, regenerate the tree-sitter parser, and review `golden__Cell3_bad.snap`.
2. Name the new hash. Lane A2 then pins it and completes the `dev` merge.

## Interim change in f1r3node-rust

743752b adds `limit: Option<u64>` to `Proc::SignedTerm` (Decision 127). Until FX-21 brings the elaboration, the normalizer refuses a signed term that declares a limit, with a parser error that names FX-21. It does not ignore the limit. FX-21 replaces this match arm.
