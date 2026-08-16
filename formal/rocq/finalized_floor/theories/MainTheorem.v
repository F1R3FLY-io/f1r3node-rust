(* ===========================================================================
   MainTheorem.v - Capstone: the finalized-floor multi-parent merge is correct.

   Bundles the module-level theorems into one end-to-end statement. Each conjunct
   is discharged by `exact` against the already-proven, axiom-free lemma, so the
   capstone itself introduces no new assumptions (verify with
   `Print Assumptions finalized_floor_merge_correct`).

   The conjuncts, and what each rules out (safety S-labels from the spec):

     T-TERM      spine walk terminates          -- floor derivation always halts
     T-MONO/L-ANC ancestor-monotone finalization-- floor cannot regress (¬S2);
                                                    downward-closed finalized cut
     L-SNAP      snapshot-monotone finalization -- larger justification snapshot
                                                    only ever finalizes more
     T-CACHE     frontier cache transparent     -- warm up-walk == cold down-walk,
                                                    so caching cannot fork (¬S1)
     T-DETMERGE  merge order-independent        -- no fork from parent fold order (¬S6)
     T-K1        no mergeable write lost         -- the ~400-block write-loss (¬S5)
     T-NDA       recovery not double-applied     -- effects applied at most once
     T-LINEAGE   LFB promotion preserves every committed state-base ancestor

   The H1 deterministic backstop (over-Δ merges refuse rather than substitute a
   lossy state) and its liveness are verified in TLA+ (SpecFixed:
   Inv_NoLostParentWrite + Inv_DeltaWithinCap + Liveness_Progress); this Rocq
   development supplies the determinism and algebra that keep every honest node
   in lockstep.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import ZArith.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.
From FinalizedFloor Require Import Floor.
From FinalizedFloor Require Import Merge.
From FinalizedFloor Require Import OccurrenceDisposition.
From FinalizedFloor Require Import Recovery.
From FinalizedFloor Require Import MergeRecoveryCoherence.
From FinalizedFloor Require Import RejectionReasonConfluence.
From FinalizedFloor Require Import ProtocolVersionLifecycle.
From FinalizedFloor Require Import ProtocolActivationCoherence.
From FinalizedFloor Require Import Selection.
From FinalizedFloor Require Import IntegerAdd.
From FinalizedFloor Require Import FtExact.
From FinalizedFloor Require Import FtProvenance.
From FinalizedFloor Require Import FinalizerProgress.
From FinalizedFloor Require Import BootstrapReplayContext.
From FinalizedFloor Require Import LocalFaultDeferral.
From FinalizedFloor Require Import FundingAdmissionLifecycle.
From FinalizedFloor Require Import EffectCausalClosure.
From FinalizedFloor Require Import StateLineageFinality.

Theorem finalized_floor_merge_correct :
  (* T-TERM: the main-parent spine walk always reaches genesis. *)
  (forall d, wf_spine d ->
     forall b, In b d ->
       exists g, walk_spine d b (blk_num b) = Some g /\ blk_main_parent g = None)
  /\
  (* T-MONO / L-ANC: finalization is downward-closed along ancestry. *)
  (forall d c J b b', anc_of d b' b -> Finalized d c J b -> Finalized d c J b')
  /\
  (* L-SNAP: finalization is monotone under snapshot growth. *)
  (forall d c J J' b, snap_extends J' J -> Finalized d c J b -> Finalized d c J' b)
  /\
  (* T-CACHE: the warm frontier up-walk equals the cold down-walk (no fork). *)
  (forall pivot band, AdjDC band ->
     lastTrue ((pivot, true) :: band) = Some (upgo pivot band))
  /\
  (* T-DETMERGE / T-CONV: the mergeable-channel merge is order-independent. *)
  (forall l1 l2, Permutation l1 l2 -> merge_or l1 = merge_or l2)
  /\
  (* T-K1: no mergeable write is lost (every set bit survives the merge). *)
  (forall l x i, In x l -> Nat.testbit x i = true -> Nat.testbit (merge_or l) i = true)
  /\
  (* T-NDA: recovery never double-applies an effect. *)
  (forall s d, apply_effect (apply_effect s d) d = apply_effect s d).
Proof.
  repeat split.
  - exact spine_walk_terminates.
  - exact L_ANC.
  - exact L_SNAP.
  - exact frontier_cache_transparent.
  - exact merge_or_perm.
  - exact merge_or_no_lost_bit.
  - exact apply_idem.
Qed.

Theorem finalized_floor_occurrence_correct :
  (forall records rejected,
     tombstoned (reject_occurrence records rejected) rejected)
  /\
  (forall records rejected survivor,
     deploy_id rejected = deploy_id survivor ->
     source_id rejected <> source_id survivor ->
     active records survivor ->
     active (reject_occurrence records rejected) survivor)
  /\
  (forall records left right candidate,
     tombstoned (reject_occurrence (reject_occurrence records left) right) candidate <->
     tombstoned (reject_occurrence (reject_occurrence records right) left) candidate)
  /\
  (forall winner loser,
     deploy_id winner = deploy_id loser ->
     source_id winner <> source_id loser ->
     active (reject_occurrence [] loser) winner).
Proof.
  exact (conj rejection_is_source_exact
          (conj distinct_source_survives_rejection
            (conj rejection_order_independent one_winner_preserved))).
Qed.

Theorem finalized_floor_recovery_admission_correct :
  (forall records occurrences,
     (forall candidate, In candidate occurrences -> ~ active records candidate) <->
     all_sources_tombstoned records occurrences)
  /\
  (forall records occurrences valid_after next_block lifespan,
     retry_eligible records occurrences valid_after next_block lifespan ->
     forall candidate, In candidate occurrences -> ~ active records candidate)
  /\
  (forall records occurrences candidate valid_after next_block lifespan,
     In candidate occurrences ->
     active records candidate ->
     ~ retry_eligible records occurrences valid_after next_block lifespan)
  /\
  (forall records occurrences valid_after next_block lifespan,
     valid_after + lifespan <= next_block ->
     ~ retry_eligible records occurrences valid_after next_block lifespan).
Proof.
  exact (conj no_active_iff_all_sources_tombstoned
          (conj retry_requires_no_active_source
            (conj active_source_blocks_retry expiry_closes_recovery))).
Qed.

Theorem finalized_floor_recovery_leadership_correct :
  (forall validator_count finalized_height,
     validator_count > 0 ->
     1 <= recovery_leader validator_count finalized_height <= validator_count)
  /\
  (forall validator_count finalized_height proposer_a proposer_b,
     recovery_authorized validator_count finalized_height proposer_a ->
     recovery_authorized validator_count finalized_height proposer_b ->
     proposer_a = proposer_b).
Proof.
  exact (conj recovery_leader_in_validator_set
          recovery_authorization_unique_per_finalized_view).
Qed.

Theorem finalized_floor_merge_recovery_coherence_correct :
  (forall base scope tombstones committed_receipt candidate,
    base committed_receipt ->
    same_deploy committed_receipt candidate ->
    ~ selected base scope tombstones candidate)
  /\
  (forall base scope tombstones named candidate,
    scope named ->
    tombstones (receipt_occurrence named) ->
    receipt_chain named = receipt_chain candidate ->
    ~ selected base scope tombstones candidate)
  /\
  (forall base scope tombstones,
    base_deploy_unique base ->
    effect_identity_consistent scope ->
    forall left right,
      committed base scope tombstones left ->
      committed base scope tombstones right ->
      same_deploy left right ->
      left = right)
  /\
  (forall base scope tombstones receipt,
    committed base scope tombstones receipt <->
    ordinary_applied base scope tombstones receipt /\
    merge_metadata_bound base scope tombstones receipt)
  /\
  (forall base scope tombstones receipt,
    base receipt ->
    ~ retry_allowed base scope tombstones (receipt_deploy receipt))
  /\
  (forall base contributions,
    length (materialize_number base contributions) = 1)
  /\
  (forall base left right,
    Permutation left right ->
    materialize_number base left = materialize_number base right).
Proof.
  exact (conj base_committed_dominates_scope
          (conj tombstoned_chain_is_excluded
            (conj committed_deploy_unique
              (conj state_record_effect_coherence
                (conj base_committed_blocks_retry
                  (conj materialized_number_is_singleton
                        materialized_number_permutation)))))).
Qed.

Theorem finalized_floor_rejection_reason_confluence_correct :
  (forall left right,
    canonical_reason_join left right = canonical_reason_join right left)
  /\
  (forall left middle right,
    canonical_reason_join (canonical_reason_join left middle) right =
    canonical_reason_join left (canonical_reason_join middle right))
  /\
  (forall reason,
    canonical_reason_join reason reason = reason)
  /\
  (forall left right,
    Permutation left right ->
    fold_rejection_reasons left = fold_rejection_reasons right)
  /\
  (forall reason,
    canonical_reason_join DuplicateOccurrence reason = DuplicateOccurrence)
  /\
  canonical_reason_join MergeConflict CollateralChainDrop = MergeConflict.
Proof.
  exact (conj canonical_reason_join_commutative
          (conj canonical_reason_join_associative
            (conj canonical_reason_join_idempotent
              (conj fold_rejection_reasons_permutation
                (conj duplicate_reason_dominates
                      merge_reason_dominates_collateral))))).
Qed.

(* ===========================================================================
   Phase 6 capstone extensions — the floor SELECTION (T-SOUND/T-FIN/T-LIN/
   T-PS/T-COMM/H3) and the IntegerAdd ALGEBRA (T-ALG c/d + launder-free + bound).
   Each conjunct is discharged by `exact` against its module lemma, so these add
   no assumptions (verify with `Print Assumptions`).
   =========================================================================== *)

Theorem finalized_floor_selection_correct :
  (* T-SOUND: the chosen merge base is sound. *)
  (forall d fuel parents cands f,
     select_floor d fuel parents cands = Some f ->
     is_sound d fuel parents cands f = true)
  /\
  (* T-SOUND (Err-correct) / T-PS: for ANY parent list, None ⇒ no candidate is
     sound (the incompatible-fork Err is correct, never a silent unsound base). *)
  (forall d fuel parents cands,
     select_floor d fuel parents cands = None ->
     forall c, In c cands -> is_sound d fuel parents cands c = false)
  /\
  (* T-SOUND-A / T-LIN: a Case-A base is a common DAG-ancestor of every parent. *)
  (forall d fuel parents c,
     case_a d fuel parents c = true ->
     forall p, In p parents -> c = p \/ anc_of d c p)
  /\
  (* T-FIN: the chosen base is drawn from the candidates, so it is finalized when
     they are. *)
  (forall (Fin : BlockHash -> Prop) d fuel parents cands f,
     Forall Fin cands -> select_floor d fuel parents cands = Some f -> Fin f)
  /\
  (* T-COMM: the committee is bonds_of(floor), a pure function of the floor. *)
  (forall bonds_of d fuel parents cands f,
     select_floor d fuel parents cands = Some f ->
     committee_used bonds_of d fuel parents cands = Some (bonds_of f))
  /\
  (* H3: the floor-bounded scan covers every parent write at or above the floor. *)
  (forall d parents fl p w,
     wf_dag_num d -> In p parents -> anc_of d fl w -> anc_of d w p ->
     in_scope d parents fl w).
Proof.
  (* build the tuple directly; `repeat split` would over-split `in_scope`. *)
  exact (conj select_sound
          (conj select_none_correct
            (conj case_a_common_ancestor
              (conj select_finalized
                (conj committee_is_floor_bonds scope_covers_band))))).
Qed.

Open Scope Z_scope.

Theorem finalized_floor_arithmetic_correct :
  (* T-ALG(c): wrapping-add group laws (associativity + commutativity). *)
  (forall a b c : Z, wadd (wadd a b) c = wadd a (wadd b c))
  /\ (forall a b : Z, wadd a b = wadd b a)
  /\
  (* T-ALG(d): the checked apply-to-base rejects on overflow OR a negative result. *)
  (forall base diff : Z, ~ in_range (base + diff) -> checked_apply base diff = None)
  /\ (forall base diff : Z, base + diff < 0 -> checked_apply base diff = None)
  /\
  (* The fail-loudly FIX is launder-free: if checked_combine accepts, it returns
     the TRUE sum, in range — a wrapped value can never be laundered through it. *)
  (forall (l : list Z) (c : Z), checked_combine l = Some c -> c = true_sum l /\ in_range c)
  /\
  (* Defense-in-depth: while every partial sum stays in range, wrapping = checked
     = true sum, so the launder cannot arise. *)
  (forall l : list Z, safe l -> checked_combine l = Some (true_sum l) /\ wsum l = true_sum l).
Proof.
  exact (conj wadd_assoc
          (conj wadd_comm
            (conj checked_apply_rejects_overflow
              (conj checked_apply_rejects_negative
                (conj checked_combine_sound supply_cap_no_launder))))).
Qed.

(* A9 exact-integer fault-tolerance DECISION (the f32 -> exact hardening). The exact
   test `2q·den ≥ S(den+num)` the node evaluates over i128 is bit-for-bit the rational
   test `(2q−S)/S ≥ num/den` cleared of its positive denominators, monotone in the
   clique weight (given den ≥ 0), and overflow-free in i128 for i64-bounded stake.
   The overflow envelope now covers the FULL validated ppm range num ∈ [-den, den]
   (G2 widening in FtExact.ft_exact_no_overflow), matching the runtime range-check
   and the negative-θ sentinels, not merely [0, den]. *)
Theorem finalized_floor_ftexact_correct :
  (forall q S num den : Z, ft_exact_ge q S num den <-> ft_ratio_ge q S num den)
  /\ (forall q S num den : Z, ft_exact_gt q S num den <-> ft_ratio_gt q S num den)
  /\ (forall q q' S num den : Z,
        0 <= den -> q <= q' -> ft_exact_ge q S num den -> ft_exact_ge q' S num den)
  /\ (forall q S num den : Z,
        0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
        Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127).
Proof.
  exact (conj ft_exact_iff_ratio
          (conj ft_exact_iff_ratio_strict
            (conj ft_exact_mono_q ft_exact_no_overflow))).
Qed.

Close Scope Z_scope.

(* ===========================================================================
   G2 capstone — θ_ppm PROVENANCE determinism + the widened i128 overflow envelope.

   Strengthens the A9 (ftexact) capstone at its one un-modelled seam: the SOURCING
   of the threshold numerator θ_ppm. A9 proves the decision is exact GIVEN θ_ppm;
   this proves θ_ppm is a pure function of the on-chain value (the unconditional
   override at casper.rs:266), so local config cannot drive a fork,
   AND that the exact decision is i128-overflow-free across the node's FULL
   validated ppm range num ∈ [-den, den] (the token_metadata_check.rs:105 range,
   including the negative-θ sentinels) — not just the narrower [0, den].

   Each conjunct is discharged by `exact` against its FtProvenance lemma, so the
   capstone adds NO assumptions (verify with `Print Assumptions
   finalized_floor_ftprovenance_correct`).
   =========================================================================== *)
Open Scope Z_scope.

Theorem finalized_floor_ftprovenance_correct :
  (* G2 / provenance: the θ_ppm a node finalizes with is the on-chain value,
     independent of local config (the unconditional override), so two nodes on the
     same genesis agree on θ_ppm regardless of local config — not a fork input. *)
  (forall local onchain : Z, reconcile local onchain = onchain)
  /\
  (* G2 / provenance (agreement form): agreeing on-chain ppm forces agreeing
     reconciled ppm, for ANY local configs local, local'. *)
  (forall local local' onchain : Z, reconcile local onchain = reconcile local' onchain)
  /\
  (* G2 / widened envelope: the exact decision is i128-overflow-free over the FULL
     validated ppm range num ∈ [-den, den] (not merely [0, den]). *)
  (forall q S num den : Z,
     0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
     Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127).
Proof.
  exact (conj reconcile_is_onchain
          (conj reconcile_agrees_on_onchain ppm_range_decision_no_overflow)).
Qed.

Close Scope Z_scope.

(* ===========================================================================
   Phase 7 capstone — the strengthened selection conjuncts (Case-B compatibility
   + selection maximality). The guard⇒AdjDC bridge and the frontier-is-finalized
   result live in GuardBridge.v (guard_constant_committee_transparent,
   upgo_finalized); they are checked axiom-free by the gate.
   =========================================================================== *)

Theorem finalized_floor_phase7_correct :
  (* Case-B compatibility: the precise guarantee of the all_compatible branch —
     every other candidate is `c`, in `c`'s past, or mergeable via a common
     descendant parent (no incompatible finalized fork). *)
  (forall d fuel parents cands c,
     case_b d fuel parents cands c = true ->
     forall o, In o cands ->
       o = c \/ anc_of d o c
       \/ (exists p, In p parents /\ anc_of d o p /\ anc_of d c p))
  /\
  (* Selection maximality: on descending-sorted candidates the chosen floor is the
     sound base of greatest block number — the canonical highest sound base. *)
  (forall d fuel parents cands f,
     DescSorted d cands ->
     select_floor d fuel parents cands = Some f ->
     is_sound d fuel parents cands f = true /\
     (forall c, In c cands -> is_sound d fuel parents cands c = true ->
        numof d c <= numof d f)).
Proof.
  exact (conj case_b_compatible select_highest_sound).
Qed.

(* ===========================================================================
   C1 + C5 capstone — the θ-exact finalization test and snapshot advancement.

   Strengthens the two "assumed/proxy" seams the earlier capstones rested on:

     C1  The node's REAL fault-tolerance decision is the exact-integer test
         `Finalized_ft` (2q·den ≥ S(den+num), θ = num/den), not merely strict
         majority. It is ancestor- and snapshot-monotone (L-ANC/L-SNAP for the
         exact test) and REFINES the strict-majority `Finalized` proxy for
         θ ∈ (0,1) over a positive-stake committee — so every θ-finalized block
         inherits T-CACHE (frontier_cache_transparent) and every capstone above.

     C5  Finalization is monotone under snapshot ADVANCEMENT (a validator's latest
         message moving forward to a DAG-descendant), which GENERALIZES the
         preservation-only L-SNAP: `snap_extends ⇒ snap_advances`, so the original
         L-SNAP is the reflexive-descendant corollary.

     C1' The num>0 refinement is VACUOUS at the DEFAULT θ = 0 and the negative-θ
         sentinels. The node's REAL decision additionally applies a θ-INDEPENDENT
         hard majority gate (clique_oracle.rs:79-81, `2·agreeing > S`), modelled as
         `Finalized_ft_hg`; the gate alone yields strict-majority `Finalized` for
         ALL num (`Finalized_ft_hg_refines_Finalized`), so θ ≤ 0 is covered too.
         (Cache transparency at θ ≤ 0 is independently secured by
         GuardBridge.BridgeFt over `Finalized_ft` directly, via `L_ANC_ft`.)

   Each conjunct is discharged by `exact` against its CliqueOracle lemma, so this
   capstone introduces NO new assumptions (verify with `Print Assumptions
   finalized_floor_thetaexact_advance_correct`). The pre-existing five capstones
   are unchanged; this only ADDS coverage of the real node test and the faithful
   advancement model. The num>0 refinement side-condition `0 < cweight c` (positive
   committee stake) is faithful and necessary — see the NOTE in CliqueOracle.v
   Section 7 (a zero-stake committee finalizes nothing); the C1' hard-gate
   refinement carries NO such side-condition (it holds for all num, all committees).
   =========================================================================== *)
Theorem finalized_floor_thetaexact_advance_correct :
  (* C1 / L-ANC: θ-exact finalization is downward-closed along ancestry. *)
  (forall d c J b b' num den,
     anc_of d b' b -> Finalized_ft d c J b num den -> Finalized_ft d c J b' num den)
  /\
  (* C1 / L-SNAP: θ-exact finalization is monotone under snapshot growth. *)
  (forall d c J J' b num den,
     snap_extends J' J -> Finalized_ft d c J b num den -> Finalized_ft d c J' b num den)
  /\
  (* C1 / refinement (θ > 0 ONLY): the θ-test (θ = num/den, num,den > 0, positive
     committee stake) implies the strict-majority proxy. θ CAVEAT: this conjunct is
     VACUOUS at the DEFAULT θ = 0 (num = 0 ⇒ the exact test is only the NON-strict
     2q ≥ S) and at the negative-θ sentinels (num < 0); the C1' conjunct below is
     what covers θ ≤ 0. *)
  (forall d c J b num den,
     (0 < num)%Z -> (0 < den)%Z -> 0 < cweight c ->
     Finalized_ft d c J b num den -> Finalized d c J b)
  /\
  (* C5 / advancement: finalization is monotone as latest messages advance to
     DAG-descendants (generalizes the preservation-only L-SNAP). *)
  (forall d c J J' b,
     snap_advances d J' J -> Finalized d c J b -> Finalized d c J' b)
  /\
  (* C5 / generalization: preservation ⇒ advancement, so the existing L-SNAP is
     the reflexive-descendant corollary of L_SNAP_advance. *)
  (forall d J' J, snap_extends J' J -> snap_advances d J' J)
  /\
  (* C1' / θ ≤ 0 COVERAGE: the node's REAL decision is the θ-test AND the
     θ-INDEPENDENT hard majority gate (clique_oracle.rs:79-81, `2·agreeing > S`).
     The hard gate ALONE yields the strict-majority `Finalized` for ALL num —
     including the default θ = 0 and the negative-θ sentinels — so θ-finalization
     refines `Finalized` WITHOUT the `0 < num` restriction the conjunct above
     carries. (Independently, T-CACHE holds directly over `Finalized_ft` for all
     num via GuardBridge.BridgeFt.guard_constant_committee_transparent_ft.) *)
  (forall d c J b num den,
     Finalized_ft_hg d c J b num den -> Finalized d c J b).
Proof.
  exact (conj L_ANC_ft
          (conj L_SNAP_ft
            (conj Finalized_ft_refines_Finalized
              (conj L_SNAP_advance
                (conj snap_extends_snap_advances Finalized_ft_hg_refines_Finalized))))).
Qed.

Theorem finalizer_progress_correct :
  (forall (A : Type) (decides : A -> option bool) candidates selected,
     scan decides candidates = Selected selected ->
     In selected candidates /\ decides selected = Some true)
  /\
  (forall (A : Type) (decides : A -> option bool) candidates,
     scan decides candidates = Exhausted ->
     forall candidate, In candidate candidates -> decides candidate = Some false)
  /\
  (forall (A : Type) (decides : A -> option bool) candidates,
     Forall (fun candidate => exists decision, decides candidate = Some decision) candidates ->
     (exists candidate, In candidate candidates /\ decides candidate = Some true) ->
     exists selected, scan decides candidates = Selected selected)
  /\
  scan (fun candidate => Some (Nat.eqb candidate 3)) (firstn 2 [1; 2; 3]) = Exhausted
  /\
  scan (fun candidate => Some (Nat.eqb candidate 3)) [1; 2; 3] = Selected 3
  /\
  (forall (A : Type)
          (eq_dec : forall left right : A, {left = right} + {left <> right})
          scheduled proposed,
     NoDup (schedule_once A eq_dec scheduled proposed)
     /\
     forall candidate,
       In candidate (schedule_once A eq_dec scheduled proposed) <->
       In candidate scheduled \/ In candidate proposed).
Proof.
  destruct fixed_prefix_can_starve_a_finalizable_candidate as [Hprefix Hcomplete].
  exact (conj scan_selected_sound
          (conj scan_exhausted_complete
            (conj complete_scan_selects_when_ready_candidate_exists
              (conj Hprefix
                (conj Hcomplete
                  (fun A eq_dec scheduled proposed =>
                    conj (schedule_once_has_no_duplicates A eq_dec scheduled proposed)
                      (schedule_once_preserves_exact_membership A eq_dec scheduled proposed))))))).
Qed.

Theorem finalized_floor_protocol_activation_correct :
  (forall active_version block_version record,
    scope_admissible active_version block_version record ->
    block_version = active_version)
  /\
  (forall version record,
    exact_protocol version ->
    encoding_matches version record ->
    exists provenance, record_provenance record = Some provenance)
  /\
  (forall version record,
    exact_protocol version ->
    encoding_matches version record ->
    record_reason record <> ReasonUnspecified)
  /\
  (forall version record,
    version < 2 ->
    encoding_matches version record ->
    record_provenance record = None)
  /\
  (forall version record,
    version < 2 ->
    encoding_matches version record ->
    record_reason record = ReasonUnspecified)
  /\
  (forall active_version floor_version block_version record,
   forall base scope tombstones committed_receipt candidate,
    exact_protocol active_version ->
    floor_version < 2 ->
    base committed_receipt ->
    same_deploy committed_receipt candidate ->
    ~ protocol_selected active_version block_version record
        base scope tombstones candidate).
Proof.
  exact (conj admissible_scope_uses_active_version
    (conj exact_encoding_requires_provenance
      (conj exact_encoding_requires_reason
        (conj legacy_encoding_forbids_provenance
          (conj legacy_encoding_requires_unspecified_reason
            legacy_floor_exact_activation_preserves_base_dominance))))).
Qed.

Print Assumptions finalized_floor_protocol_activation_correct.

Theorem finalized_floor_protocol_lifecycle_correct :
  (forall candidate_version approved_version local_versions,
    candidate_version = ceremony_candidate current_protocol ->
    approver_accepts current_protocol candidate_version = true ->
    approved_version = candidate_version ->
    approved_version = current_protocol /\
    adopt_network approved_version local_versions =
      repeat current_protocol (length local_versions) /\
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions))
  /\
  (forall approved_version local_versions,
    supported_protocol approved_version ->
    admit_approved approved_version = Some approved_version /\
    adopt_network approved_version local_versions =
      repeat approved_version (length local_versions) /\
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions))
  /\
  (forall version,
    ~ supported_protocol version ->
    admit_approved version = None)
  /\
  admit_approved legacy_protocol = None
  /\
  (forall configured_version candidate_version,
    candidate_version <> configured_version ->
    approver_accepts configured_version candidate_version = false)
  /\
  (forall active_version block_version record,
    scope_admissible active_version block_version record ->
    block_version = active_version).
Proof.
  exact (conj current_ceremony_end_to_end
    (conj supported_recovery_end_to_end
      (conj unsupported_approved_fails_closed
        (conj legacy_approved_fails_closed
          (conj mismatched_candidate_is_not_approved
            admissible_scope_uses_active_version))))).
Qed.

Print Assumptions finalized_floor_protocol_lifecycle_correct.

Theorem bootstrap_replay_and_local_fault_recovery_correct :
  (forall (Context Root : Type)
          (replay : Context -> Root -> Root)
          (history : list (@ConsensusBlock Context Root replay)),
    replay_history replay history = declared_history_roots replay history)
  /\
  (forall state,
    validation_disposition (defer_local_fault state) =
      validation_disposition state)
  /\
  (forall state,
    queue_state (defer_local_fault state) <> Ready)
  /\
  (forall state,
    queue_state state = Deferred ->
    queue_state (recovery_request_failed state) <> Ready)
  /\
  (forall state,
    regular_parent_satisfied state = true ->
    validation_disposition state = Accepted).
Proof.
  split.
  - intros Context Root replay history.
    exact (consensus_history_replay_matches_declared_roots replay history).
  - exact (conj local_fault_preserves_consensus_disposition
      (conj local_fault_leaves_ready_queue
        (conj failed_recovery_does_not_restore_ready_state
          regular_child_requires_valid_parent))).
Qed.

Print Assumptions bootstrap_replay_and_local_fault_recovery_correct.

Theorem terminal_funding_admission_lifecycle_correct :
  (forall supply demand,
    supply < demand ->
    recorded_decision (propose supply demand) = Reject /\
    user_effects (propose supply demand) = 0 /\
    finalize_record (propose supply demand) = RejectedFinalized)
  /\
  (forall record (later_supply : nat),
    recorded_decision record = Reject ->
    finalize_record record = RejectedFinalized /\
    user_effects record = 0)
  /\
  (forall supply demand,
    demand <= supply ->
    validate_record
      {| recorded_supply := supply;
         recorded_demand := demand;
         recorded_decision := Reject |} = false).
Proof.
  exact (conj underfunded_proposal_is_terminal_rejection
    (conj later_supply_does_not_resurrect_recorded_rejection
      fundable_deploy_cannot_be_forged_as_rejected)).
Qed.

Print Assumptions terminal_funding_admission_lifecycle_correct.

Theorem finalized_floor_effect_causal_closure_correct :
  exact_effect_causal_closure_contract.
Proof.
  exact exact_effect_causal_closure_correct.
Qed.

Print Assumptions finalized_floor_effect_causal_closure_correct.

Theorem finalized_floor_state_lineage_correct :
  state_lineage_contract /\
  promotion_preservation_contract /\
  base_lineage_promotion_contract.
Proof.
  exact (conj state_lineage_end_to_end
    (conj state_lineage_promotion_correct base_lineage_promotion_correct)).
Qed.

Print Assumptions finalized_floor_state_lineage_correct.
