---
doc_type: independent_review
task: pr216-pruning-design
criterion_id: 3663
verification_revision: 1
status: review_complete
date: 2026-09-08
reviewer: /root/casper_comparison_plan
---

# Independent pruning-plan review

The following report records the authorized plan agent's review.
The main agent checked the listed artifact hashes before submission.
This report is not upstream Casper approval.

## Reviewer report

**Criterion3663: PASS for the decision-packet review.** This does not approve the storage amendment or qualify a production repair.

I read the complete packet. It distinguishes demonstrated pruning loss, source-derived caller hazards, proposed storage changes, and remaining qualification.

### Independent assessment of smaller options

- **Restore upstream victim selection:** insufficient. The pinned upstream module reproduction records the same durable-row loss. Current `dev` retains the reproduced buffer blob.
- **Skip unresolved entries or remove only terminal entries:** smaller preservation repair, but not a complete memory-bound repair. The constructor still materializes all durable rows.
- **Remove only leaves:** insufficient. A leaf can retain the last discoverable retry obligation.
- **Transfer ownership to BlockRetriever:** unproven. The current acknowledgment path deletes retriever tracking. Transfer also needs refusal, restart, and capacity semantics.
- **Reject at capacity:** potentially smaller than persistent indexes, but requires explicit ownership and dependency-progress rules. A blanket rejection can block the parent needed to release capacity.
- **Page existing durable rows:** addresses resident retention without necessarily requiring monotonic tickets. It still needs correct cold lookups, reverse queries, startup capture, and an agreed scan-service guarantee.

The persistent-ticket namespace is therefore **not established as necessary** by the pruning reproductions. Its stronger selection guarantee and migration costs require separate approval. The packet states this adequately.

### Material findings

The concrete caller hazard is supported by current source:

- `block_processor_instance.rs:474` acknowledges a dependency-processing error.
- `block_processor.rs:1075` writes dependency relations individually.
- `block_processor.rs:1780` delegates acknowledgment to the retriever.
- `block_retriever.rs:1475` removes tracking after acknowledgment.

This supports the packet’s first-publication ownership concern. It does **not** replace the specified worker-level fault-injection regression.

The packet correctly requires preservation of upstream mutation ordering and orphan cleanup. It also identifies feature failure atomicity, explicit empty rows, and certificate waiters as retained obligations.

No material correction is required for the design-review criterion. Add the current-dev blob-equality observation to permanent provenance without presenting it as a new runtime run.

### Evidence limits and assumptions

- I inspected the original feature reproduction log. It records two preservation failures and one passing control.
- The upstream `result.txt` is an explicitly labeled summary, not a verbatim log.
- Matching module blobs extend source applicability. They do not establish a complete current-dev build, network reproduction, or LMDB crash result.
- I did not rerun tests or formal tools. I did not validate every backend claim against dependency source.
- Durable retry guarantees and service bounds remain policy assumptions for upstream disposition. Neither safety nor uptime follows solely from the proposed index.

### Reviewed hashes

```text
a6c104b179edd006e8bc6c9671e7c1a0fda8b1d281aac0adad9ff8c59e08b714  buffer-pruning-preservation.md
46bab7945cda1a7a20241acaccec0e32f349913b9aa8a6e0f49be913248bc443  casper_buffer_key_value_storage.rs
60e5f43cd9ba3d16daf08fe31b2b9611096bef8adea24aadbe1f8083f46e51d1  doubly_linked_dag_operations.rs
9a5bb3eefdcf1cad569f09d771f7930f5c80944f89880a6811e9a70f5b57051f  buffer_pruning_preservation.rs
b07dd8d723c6b5ef0a8f7a44569eccded4c6fe153d0b951e9ab99657ae9a5111  block_processor_instance.rs
025282a1f443089ad726ce17184b60da0d1ab73082a27914e6e0e64af7dd9b77  block_processor.rs
59fb47044c73773d249f04bbc32022cc17fdd410170ffd877874753bdf7dc7d6  block_retriever.rs
b5bbbc04116f6b939844203c0aaf6905c268a03742379f27b4f691d90dfcafb0  key_value_store.rs
```

Current `dev` commit: `cdf447ac18710d9702a27379bce6c946f421be46`. Buffer blob: `1c15061f86aaecf2515ac96cb28c552830263bfd`.

**Independence:** I did not implement production changes or write this pruning packet. I performed read-only review. This review is not upstream signoff.

## Provenance supplement

The [M1 producer audit](../task-pr216-verification-producers-2026-09-08.md) records the current-dev module equality observation.
It also distinguishes the retained test log from the summarized upstream result.
The original decision packet remains unchanged at the reviewed hash.
