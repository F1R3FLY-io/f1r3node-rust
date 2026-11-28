# CbC Evidence: casper/src/rust/merging/conflict_set_merger.rs

- **Status:** waived
- **Adapter:** waiver
- **Commit:** fd72f2aae
- **Verified:** 2026-08-22T03:59:59Z

Claim:

> Rejection-option selection minimizes prior loss before cost and deterministic content order.

```json
{
  "artifact": {
    "path": "casper/src/rust/merging/conflict_set_merger.rs",
    "commit": "fd72f2aae",
    "id": "casper-src-rust-merging-conflict-set-merger-rs"
  },
  "claim": "Rejection-option selection minimizes prior loss before cost and deterministic content order.",
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

## 2026-11-26 — R-BOUND (bounded rejection selection)

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** 29425abc3
- **Verified:** 2026-11-26T05:09:03Z

Claim:

> R-BOUND (docs/casper/theory/merge-algebra/merge-algebra-specification.md): rejection selection resolves each group of at most 16 branches exactly, minimizing prior loss before cost and deterministic content order, and resolves a larger group greedily in that same preference order with pinned chains first, so its time and memory are polynomial in the merge's branches. Groups join branches through conflicts and through IntegerAdd cells on which the overdraw fold can reject a branch.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "R-BOUND",
  "artifact": {
    "path": "casper/src/rust/merging/conflict_set_merger.rs",
    "commit": "29425abc3",
    "id": "casper-src-rust-merging-conflict-set-merger-rs"
  },
  "claim": "Rejection selection is exact for groups of at most 16 branches and greedy, in the same preference order with pinned chains first, for larger groups; groups join branches through conflicts and through IntegerAdd cells where some branch's net diff is negative or the base plus all positive net diffs would pass the IntegerAdd bound.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test",
    "ref": "casper rust::merging::conflict_set_merger::component_selection_tests (component_selection_matches_exhaustive; pinned_component_selection_matches_exhaustive; disjoint_pairs_past_the_cap_resolve_in_one_greedy_pass; the_greedy_pass_keeps_pinned_branches; cells_every_branch_only_credits_do_not_join_groups; credits_that_could_pass_the_bound_join_one_group; the_greedy_pass_still_refuses_an_overdrawn_cell; large_dense_groups_resolve_greedily_and_deterministically)",
    "counterexample": null,
    "detail": "Two proptests (3000 and 2000 cases, at most 8 branches) show the grouped selection equals the exhaustive one, with and without pinned chains (when the exhaustive result keeps every pinned chain). Each of three mutations fails exactly its guarding test: coupling through every number cell fails cells_every_branch_only_credits_do_not_join_groups, dropping the pinned-first key fails the_greedy_pass_keeps_pinned_branches, and dropping the bound clause fails credits_that_could_pass_the_bound_join_one_group. dev's exhaustive search, run alone on k disjoint pairs, returns 2^k options with time about x4 and memory about x3 per pair (k=11: 3.8 s, 113 MB); on a 4-validator OCI shard, 12 pairs per burst sent to 2 validators stopped finality and grew validators to 9-17 GB, and perf put the stalled validator in compute_rejection_options. With the bound, bursts of 12, 20 and 28 pairs left finality running with validators at most 0.86 GB, and the capacity mix at 4/s finalized 2400 of 2400 with no greedy group."
  },
  "verified_at": "2026-11-26T05:09:03Z"
}
```
