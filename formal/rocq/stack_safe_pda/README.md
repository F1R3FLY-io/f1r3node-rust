# Sorter equivalence proofs

This development proves weak bisimulation between two sorter models and equality with a recursive fold.
The models cover every finite tree and every total combination function.
They have no fixed depth bound.
The proof does not establish a machine-checked refinement of the Rust source.

`StackSafePDA.v` comes unchanged from `feature/mettail` commit `4d88896e7ad8f100e6c9b6fedf082b228d4a7491`.
It proves that a compiled postorder program computes the recursive fold.
`SorterSimulation.v` adds the lazy work stack that the sorter uses.
`ScoreComparison.v` proves that the comparison work stack returns the recursive comparator's first non-equal result.
`CostAccountingSorter.v` adds this branch's cost-accounting child order and metadata parameters.

Run the proof gate from the repository root:

```sh
bash scripts/check-sorter-proofs.sh
```

The gate compiles the proofs in a temporary directory and checks them with `coqchk`.
It requires 18 theorem reports with no global assumptions.
Four negative controls detect reversed sibling scheduling, unreversed child values, an omitted combination, and comparison of later siblings first.
The gate also compiles the existing [signature proof](../cost_accounted_rho/theories/StackSafeSignatureSort.v), including its negative controls.

## Relation and observations

A model configuration contains pending work and completed values.
The relation replaces each pending `Visit` with its compiled subtree program and preserves the value stack exactly.
Expanding a visit is a silent step.
Completing a node exposes its label and arity and performs the same combination on both sides.

`lazy_step_matches_reference` proves the forward direction.
`reference_step_matches_lazy` proves the reverse direction after finitely many silent expansions.
`related_final_values` proves that related machines finish with the same values.
`every_lazy_step_decreases_weight` excludes infinite silent expansion on finite trees.
`sorter_returns_recursive_result` proves termination with exactly the recursive fold's result.

The recursive specification assumes an unlimited native stack.
It does not model the old Rust process abort after a native stack overflow.
The comparison theorem preserves results for every atom comparator.
It does not assume or prove that an arbitrary atom comparator defines a total order.

## Correspondence with Rust

Model list heads correspond to Rust vector tails.
The model reverses popped values before combination, as Rust restores children to source order.
Rust completes an unforgeable value directly, which compresses the model's expansion and completion steps.
Rust also handles empty node comparisons directly, which compresses a model `Nodes` step.
These mappings use weak correspondence, not identical instruction counts.

The following table records the adapter obligations for [sort_drive.rs](../../../models/src/rust/rholang/sorter/sort_drive.rs).
It records source order, which can differ from the resulting score-field order.
Each continuation must consume the listed children and return its corresponding `SortVal` variant.
The adapter remains a source-review and Rust-test boundary.

| Work variant | Children in source order | Combination obligation |
|---|---|---|
| `Par` | Sends, receives, expressions, new bindings, matches, bundles, connectives, unforgeables, conditionals, signed terms, stacks | Preserve all 11 family boundaries and existing family sorting. |
| `Send` | Channel, data | Preserve persistence and metadata. |
| `Receive` | Binds, body, condition | Preserve flags and counts. Normalize an absent condition as before. |
| `Bind` | Patterns, source, optional cost signature | Preserve remainder, free count, and signature presence. |
| `New` | Body, injection values in map order | Preserve injection keys, URI handling, and bind count. |
| `Match` | Target, cases | Preserve case order. |
| `Case` | Pattern, source, guard | Preserve free count. Normalize an absent guard as before. |
| `If` | Condition, true branch, false branch | Preserve branch positions. |
| `Bundle` | Body | Preserve read and write flags. |
| `Connective` | The children from `connective_child_pars` | Preserve the variant and existing connective normalization. |
| `Expr` | The children from `expr_child_pars` | Preserve all 42 variants and their field order. |
| `Unforgeable` | None | Preserve every variant and its payload. |
| `Signature` | Quote/name body, or flattened compound leaves | Preserve ground, unit, bound-level, absent, quote, name, and compound behavior. |
| `Signed` | Optional body, optional signature | Preserve both presence bits and the exact `held_price: Option<u64>`. |
| `Stack` | Cells | Preserve cell order, including duplicates. |

Expression combination retains the existing `ESet` and `EMap` wrappers, `EPathMap` ordering, and `EZipper` schema.
It also retains the current numeric variants, including `GFloat32`.
The formal algebra abstracts these local combination functions.
It does not independently prove their field projections or sorting-library behavior.

The cost-accounting theorems preserve the exact label supplied to combination, including the optional price, and every ordered child result.
Their price domain includes every `u64` value and distinguishes `None` from `Some(0)`.
The [existing flattening proof](../cost_accounted_rho/theories/StackSafeSignatureSort.v) preserves every non-compound leaf in source order.
The new composition theorem preserves that order through any leaf decoder and normalization function.
Unit removal, empty/singleton collapse, and stable tie handling remain properties of the unchanged combination function.

The generic tree can represent the full `Par → Signature → Quote/Name → Par` cycle at any finite depth.
The compound-flattening proof separately treats quotes and names as opaque leaves.
No theorem substitutes flattening for traversal inside those leaves.

## Verification boundary

The Rust correspondence requires valid mandatory fields, child indices that fit `u32`, and counters that fit `usize`.
The proof does not establish allocation success, serialization correctness, compiler correctness, or native-stack bounds for every called operation.
Its decreasing measure counts model transitions, excluding local sorting, copying, and compound-flattening work.
The generic reconstruction and discard theorems do not verify Rust's `Tree::Clone`, `PartialEq`, or `Drop` implementations.

[Canonical golden tests](../../../models/tests/sorter_canonical_golden.rs) compare bytes and scores against 160 fixtures captured before the sorter change.
Those fixtures cover current expression variants, signatures, prices, and metadata.
[Comparator tests](../../../models/src/rust/rholang/sorter/score_tree.rs) compare production results and permutations with a recursive oracle.
[Depth tests](../../../models/tests/sorter_stack_safety.rs) exercise nested terms and cost signatures on a small native stack.
These tests support the Rust correspondence. They do not convert the model proof into a proof of the Rust binary.
