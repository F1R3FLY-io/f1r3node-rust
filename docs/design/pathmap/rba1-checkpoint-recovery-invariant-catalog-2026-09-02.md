# RBA1 checkpoint recovery invariant catalog

Date: 2026-09-02

Status: formal authority verified; executable property registry established; production RBA1 codec and recovery adapter open

Scope: direct checkpoints of the selected two-level RSpace layout

## Purpose

This living engineering report is the traceability authority for recovery of a direct two-level PathMap checkpoint. The checkpoint format is named **RBA1**: RSpace Bag Authority version 1. It contains one canonical outer PathMap image and the canonical image of every independently guarded inner Bag in outer-cursor order.

The formal model is verified, but RBA1 is not yet a production persistence authority. The existing canonical content-addressed directed-acyclic-graph (DAG) backend remains the durable implementation until the selected two-level layout, direct codec, and recovery adapter satisfy every executable obligation below. A formal proof row marked **GAP** is therefore a mandatory implementation target, not a claim that current Rust code already discharges it.

The required development order is:

1. verify the recovery authority and causal controls;
2. extract every invariant into this catalog and the mechanical Rust registry;
3. implement the two-level PathMap authority;
4. implement the direct RBA1 codec and recovery adapter;
5. require proofs, property tests, corruption differentials, restart tests, and bounded-resource measurements to pass before enabling RBA1 authority.

## Vocabulary

- **Outer authority**: the PathMap keyed by exact canonical channel identity. Each real endpoint stores one independently guarded channel-relative Bag.
- **Inner Bag**: a `PathMap<Multiplicity>` relative to one channel. A loaded channel with no data has a present outer endpoint whose value is an empty Bag.
- **Loaded-empty**: the semantically observable state in which the outer endpoint exists and its inner Bag is empty. It is different from an unloaded channel, whose outer endpoint is absent.
- **Physical index generation**: the generation of the on-disk index selected by a receipt. Maintenance may advance it without changing RSpace semantics.
- **Semantic state generation**: the generation of the admitted RSpace state. An accepted match advances it exactly once; a refused match does not advance it.
- **Exact validation subject**: the complete canonical bytes that recovery proposes to admit. Validation of a projection, cardinality, alternate serialization, or same-sized substitute is insufficient.
- **Readmission**: reconstruction and semantic admission of the exact store represented by the validated RBA1 image. Merely parsing bytes or selecting a catalog slot is not readmission.

## Authority relation

Let `C` be a recovery candidate, `A` its generation artifact, `S` the admitted transactional store, `I` the decoded RBA1 image, `g_i` the physical index generation, and `g_s` the semantic state generation. New authority is observable only when all of the following facts hold simultaneously:

```math
\begin{aligned}
\operatorname{bytes}(C) &= \operatorname{bytes}(A), \\
\operatorname{validate}(\operatorname{bytes}(A)) &= I, \\
\operatorname{receipt}_{expected} &= \operatorname{receipt}_{observed}, \\
\operatorname{receipt}.g_i &= A.g_i, \\
A.g_s &= \operatorname{descriptor}.g_s = S.g_s, \\
I &= \operatorname{encode}_{RBA1}(S), \\
\operatorname{semanticAfter} &= S.
\end{aligned}
```

The physical and semantic generations are intentionally separate. A maintenance checkpoint may change `g_i` while preserving `g_s`; an accepted match must change `g_s` even when a physical index identifier happens to remain otherwise valid.

## Formal evidence

- Recovery authority: `formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceCheckpointRecovery.v`
- Causal controls: `formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceCheckpointRecoveryMutants.v`
- Integrated gate: `scripts/check-stack-safe-pda-proofs.sh`
- Stable implementation checkpoint: `d7dd385de`

The recovery theory closes 14 reported capstones under the global context. The causal-control theory closes 6 reported capstones. `coqchk` accepts both compiled modules. The non-vacuity fixtures construct a real content object and execute the complete affine publication path through exact validation, physical recovery, semantic readmission, and acknowledgement for one loaded-empty channel.

## Exhaustive recovery property catalog

| ID | Required property | Executable property target | Current status |
|---|---|---|---|
| RBA1-REC-01 | Recovery validates the complete canonical RBA1 bytes and recovers exactly their image. A decodable but invalid current-format image is rejected and never reinterpreted as legacy data. | `rba1_exact_validation_rejects_every_subject_mutation` | GAP |
| RBA1-REC-02 | The authoritative content object is constructed from the domain-separated checkpoint preimage and exact canonical length, inserted into the expected inventory, and recovered from that same inventory. | `rba1_object_inventory_binding_is_exact` | GAP |
| RBA1-REC-03 | The expected receipt equals the observed receipt; its physical index generation equals the selected artifact index generation; descriptor, artifact, and store semantic generations agree independently. | `rba1_physical_and_semantic_generations_bind_independently` | GAP |
| RBA1-REC-04 | An accepted match binds the checkpoint to the exact successor semantic state. A refused match cannot publish a successor semantic generation. Physical maintenance generations cannot mask a stale semantic generation. | `rba1_match_publication_uses_exact_semantic_successor` | GAP |
| RBA1-REC-05 | New authority remains unobservable until exact bytes, exact receipts, exact image-to-store refinement, physical recovery, semantic readmission, catalog-directory durability, and unique-writer transfer all hold. | `rba1_authority_is_unobservable_before_exact_readmission` | GAP |
| RBA1-REC-06 | Restart selects old, exact complete new, or rejection according to durability and validity. Loaded-empty survives exactly, and accepted legacy migration becomes authority only after canonical re-encoding and validation. | `rba1_restart_matrix_preserves_loaded_empty_exactly` | GAP |

## Formal-to-executable derivation matrix

| ID | Rocq capstones | Causal control | Refinement class |
|---|---|---|---|
| RBA1-REC-01 | `canonical_checkpoint_validation_recovers_exact_image`, `exact_recovery_is_sound`, `selected_new_implies_complete_valid_exactly_bound_successor`, `invalid_current_rba1_never_falls_through_to_legacy` | RBA1-REC-M01 | P |
| RBA1-REC-02 | `constructed_rba1_object_is_exactly_inventory_bound`, `rba1_object_construction_and_inventory_binding_are_inhabited` | RBA1-REC-M02 | P |
| RBA1-REC-03 | `checkpoint_binding_eqb_reflects_exact_receipt`, `selected_new_implies_complete_valid_exactly_bound_successor` | RBA1-REC-M03 | P |
| RBA1-REC-04 | `accepted_match_binds_artifact_to_exact_successor_state_generation`, `refused_match_cannot_publish_successor_semantic_state` | RBA1-REC-M03 | P |
| RBA1-REC-05 | `runtime_new_authority_requires_acknowledged_exact_readmission`, `acknowledged_exact_rba1_readmission_is_reachable_and_inhabited` | RBA1-REC-M01, RBA1-REC-M02, RBA1-REC-M03, RBA1-REC-M04 | P |
| RBA1-REC-06 | `loaded_empty_rba1_survives_restart_exactly`, `accepted_migration_can_become_restart_authority_only_after_validation`, `rba1_recovery_invariant_catalog_is_duplicate_free_and_exact` | RBA1-REC-M04 | P |

`P` means the formal statement partially specifies a production property whose filesystem, parser, PathMap, and failure-injection refinement remains executable work. No row is marked as a completed executable refinement yet.

## Causal-control catalog

| ID | Forbidden mutation | Formal witness | Expected executable observation |
|---|---|---|---|
| RBA1-REC-M01 | Validate a same-cardinality substitute instead of the candidate bytes. | `exact_subject_validation_kills_same_cardinality_substitution` | Validation rejects before authority selection. |
| RBA1-REC-M02 | Detach the RBA1 object from the authenticated expected inventory. | `exact_inventory_binding_kills_detached_object` | Publication or recovery rejects the missing object binding. |
| RBA1-REC-M03 | Bind an accepted match to its pre-match semantic generation while leaving the physical index generation valid. | `semantic_successor_kills_pre_match_state_generation_substitution` | Semantic-generation verification rejects independently of physical index validity. |
| RBA1-REC-M04 | Erase a loaded-empty endpoint and reinterpret it as unloaded. | `restart_exactness_kills_loaded_empty_erasure` | Exact store comparison detects the endpoint loss. |

The mutant metatheorems `every_recovery_seam_mutant_has_a_causal_witness` and `recovery_seam_mutant_catalog_is_unique_complete_and_exact` establish that the four controls are distinct, complete for this seam, and observably different from the reference behavior.

## Generator and oracle discipline

The executable properties must use dependent generators rather than discard-heavy assumptions. Valid fixtures generate the store first, derive outer and inner canonical ACT bytes from it, construct the content object from those exact bytes, and derive receipts and both generation domains from the publication transcript. Invalid fixtures mutate exactly one typed fact while holding the others fixed.

The reference oracle may materialize small logical observations for comparison. Production code must not flatten a PathMap into `Vec<Par>`, sort elements, or rebuild an equivalent trie merely to encode or validate it. RBA1 production paths must stream canonical outer and inner ACT bytes in cursor order and bind the resulting bytes directly to their artifact and receipt.

## Required executable matrix

Each property target must cover:

1. zero, one, and several loaded channels;
2. loaded-empty versus unloaded;
3. empty, singleton, prefix-related, and branching inner Bags;
4. old-only, torn-new, complete-valid-new, and complete-invalid-new restart states;
5. every exact-field receipt mutation;
6. independent physical-index and semantic-generation changes;
7. accepted and refused matches;
8. every byte truncation, extension, checksum mutation, and canonicality violation;
9. arbitrary hostile bytes with typed rejection and no panic;
10. bounded native stack, bounded scratch, bounded open descriptors, and no input-sized flattened projection.

## Activation rule

RBA1 may become a production recovery authority only after every `RBA1-REC-*` row is bound to a non-ignored executable property, every `RBA1-REC-M*` control fails for its intended reason, the selected two-level PathMap layout is the sole live logical authority, and restart comparison proves exact semantic equivalence with the canonical-DAG baseline. Until then, RBA1 remains a formally specified implementation target.

## Living-document protocol

Any change to RBA1 bytes, PathMap layout, generation semantics, WAL receipts, restart selection, or semantic admission must update this catalog in the same commit. New obligations receive stable IDs; existing IDs are never silently repurposed. A status may move from GAP to VERIFIED only when its named executable target, causal control, integration evidence, and resource evidence are all present and enforced by the repository gate.
