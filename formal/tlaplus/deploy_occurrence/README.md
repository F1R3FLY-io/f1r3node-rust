# Deploy occurrence model

| Artifact | Purpose |
| --- | --- |
| `DeployOccurrence.tla` | Models independent validators learning source-distinct deploy occurrences in arbitrary orders and exchanging observations. |
| `MC_DeployOccurrence.cfg` | Requires exact occurrence tombstones, one canonical winner per observed deploy, and convergence after equal observation sets. |
| `MC_DeployOccurrence_sig_only_pre_fix.cfg` | Reproduces the signature-only rejection defect; it must violate `Inv_OneWinnerPreserved`. |

The model abstracts a source block hash as a natural number. Its total order
stands for the Rust comparator's node-identical rank, including the source hash
as its final tie-break. It does not claim that the Rho calculus chooses that
rank; the rank is a protocol policy required to make the merge projection a
function.

| Model expression | Rust realization |
| --- | --- |
| `Occurrences` | `(deploy signature, source block hash)` |
| `Canonical` | `DeployChainIndex::cmp` and the keep-one merge |
| `Rejected` | `RejectedDeploy { sig, source_block_hash, reason }` |
| `Active` | source-aware deploy disposition reduction |
| `Observe`, `Share` | asynchronous block arrival and DAG synchronization |
