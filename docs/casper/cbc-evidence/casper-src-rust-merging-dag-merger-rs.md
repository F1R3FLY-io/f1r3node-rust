# CbC Evidence: casper/src/rust/merging/dag_merger.rs

- **Status:** waived
- **Adapter:** waiver
- **Commit:** fd72f2aae
- **Verified:** 2026-08-22T03:59:59Z

Claim:

> Every adjudication site uses complete authenticated counts and maximum member priority with deterministic equal-count ties.

```json
{
  "artifact": {
    "path": "casper/src/rust/merging/dag_merger.rs",
    "commit": "fd72f2aae",
    "id": "casper-src-rust-merging-dag-merger-rs"
  },
  "claim": "Every adjudication site uses complete authenticated counts and maximum member priority with deterministic equal-count ties.",
  "adapter": "waiver",
  "status": "waived",
  "evidence": {
    "kind": "waiver",
    "ref": null,
    "counterexample": null,
    "detail": "explicit waiver"
  },
  "waiver": {
    "reason": "PR #299 records phase-1 implementation evidence. PR #311 will discharge the formal Rust-conformance claims.",
    "by": "maintainer",
    "at": "2026-08-22T03:59:59Z"
  },
  "verified_at": "2026-08-22T03:59:59Z"
}
```

---

## 2026-11-25 — R-CREATOR (number-cell writes need their creator)

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** b94b3cb40
- **Verified:** 2026-11-25T01:23:39Z

Claim:

> R-CREATOR (docs/casper/theory/merge-algebra/merge-algebra-specification.md): `split_unavailable_branch_consumes` rejects a chain that holds a value on a number cell it did not create when the base holds no value there and no surviving chain creates it, so no merge makes a number cell from nothing (S9, issue #1040).

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "R-CREATOR",
  "artifact": {
    "path": "casper/src/rust/merging/dag_merger.rs",
    "commit": "b94b3cb40",
    "id": "casper-src-rust-merging-dag-merger-rs"
  },
  "claim": "R-CREATOR: a chain's number-cell delta is applied only when the base holds the cell, the chain created it, or a surviving chain of the merge creates it; otherwise the chain is unavailable and rejected inside the retry loop.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test",
    "ref": "casper merging::number_cell_creator_spec (a_write_to_a_number_cell_whose_creator_an_earlier_merge_rejected_is_rejected; a_write_to_a_number_cell_whose_creator_the_merge_keeps_is_kept)",
    "counterexample": null,
    "detail": "The first test fails when the check is disabled (the merge applies E); the second fails when the check ignores cells that surviving chains create (E is wrongly rejected). A 3-validator probe that reproduced the issue-1040 stall on dev (33 of 33 proposals failed on a number cell holding 2 values) shows 0 failed proposals with the check. Under capacity load at 4/s (two runs) and the CI-shaped test_load and capacity ramp, the check fired 0 times (dag.merge.rejected.number-cell-without-creator), so normal merges are unchanged. Inputs are the chains' recorded state changes and the base state, so the verdict is a pure function of the merge inputs (R-DET)."
  },
  "verified_at": "2026-11-25T01:23:39Z"
}
```

---

## 2026-11-25 — R-CREATOR (a creator and its users in one block)

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** 24cde6d68
- **Verified:** 2026-11-25T16:00:19Z

Claim:

> R-CREATOR (docs/casper/theory/merge-algebra/merge-algebra-specification.md): a deploy that uses the value another deploy of its block left on a number cell the block's pre-state lacks is in that deploy's chain (`depends_within_block`, `block_index.rs`), so no merge keeps a write through a cell whose creator in the same block it rejects.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "R-CREATOR",
  "artifact": {
    "path": "casper/src/rust/merging/dag_merger.rs",
    "commit": "24cde6d68",
    "id": "casper-src-rust-merging-dag-merger-rs"
  },
  "claim": "R-CREATOR, same-block case: the creator of a number cell and the deploys of its block that use the cell's new value form one chain, so the merge keeps or rejects them together.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test",
    "ref": "casper merging::number_cell_creator_spec::a_writer_in_the_creators_block_is_rejected_with_the_creator",
    "counterexample": null,
    "detail": "D creates a TreeHashMap node and E inserts through it in one block; a sibling block makes the merge reject D. With plain depends, D and E are separate chains, E counts as the node's creator, and the merge keeps E. With depends_within_block, D and E are one chain and the merge rejects both. A 3-validator probe of this shape on the same tree without the link failed 33 of 33 proposals with 'pre-state values; single-value invariant violated' after D's recovery, and failed none with it. The link adds only cells the block's pre-state lacks, so writers of existing cells such as the PoS vault stay independent; the full casper suite passes."
  },
  "verified_at": "2026-11-25T16:00:19Z"
}
```
