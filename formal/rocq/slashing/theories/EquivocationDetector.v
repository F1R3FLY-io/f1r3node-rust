(* ═══════════════════════════════════════════════════════════════════════════
   EquivocationDetector.v — Detection semantics and soundness/completeness

   Models the detection function of f1r3node's EquivocationDetector at
     casper/src/rust/equivocation_detector.rs:24-104
   and the Scala counterpart
     coop/rchain/casper/EquivocationDetector.scala:25-100

   Theorems:
     T-1 (detection_sound)    — emitted equivocations are real
     T-2 (detection_complete) — real equivocations get detected

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition         │ Paper / Spec Notation       │ Rust Implementation
   ────────────────────────┼─────────────────────────────┼─────────────────────
   detect                  │ detect(S, b)                │ check_equivocations
   DetectorStatus          │ ib ∈ InvalidBlock taxonomy  │ BlockStatus value
   DSValid                 │ Valid                        │ Right(ValidBlock::Valid)
   DSAdmissible            │ AdmissibleEquivocation       │ Left(InvalidBlock::AdmissibleEquivocation)
   DSIgnorable             │ IgnorableEquivocation        │ Left(InvalidBlock::IgnorableEquivocation)
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §4.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block InvalidBlock EquivocationRecord DAGState.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — DetectorStatus
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive DetectorStatus : Type :=
  | DSValid       : DetectorStatus
  | DSAdmissible  : DetectorStatus
  | DSIgnorable   : DetectorStatus
  | DSNeglected   : DetectorStatus.

(* Map a DetectorStatus to its corresponding InvalidBlock variant when
   the status is non-valid. *)
Definition status_to_invalid (st : DetectorStatus) : option InvalidBlock :=
  match st with
  | DSValid       => None
  | DSAdmissible  => Some IBAdmissibleEquivocation
  | DSIgnorable   => Some IBIgnorableEquivocation
  | DSNeglected   => Some IBNeglectedEquivocation
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Detection function (POINTER detector, faithful to the Rust)
   ═══════════════════════════════════════════════════════════════════════════

   f1r3node's `check_equivocations` (equivocation_detector.rs:86-89) does NOT
   count same-seq/distinct-hash blocks. It compares two `Option<BlockHash>`
   pointers:

       let maybe_latest_message_of_creator_hash = dag.latest_message_hash(&block.sender);
       let maybe_creator_justification         = Self::creator_justification_hash(block);
       let is_not_equivocation =
             maybe_creator_justification == maybe_latest_message_of_creator_hash;

   i.e. an arriving block equivocates iff its creator-justification (self-parent)
   pointer disagrees with the sender's current latest-message pointer in the
   DAG. We model that pointer comparison directly. This is a DIFFERENT fact
   from DAGState's same-seq/distinct-hash count `equivocates_b`; the two are
   provably distinct (see `equivocates_ptr_diverges_from_seq_count` below), so
   the detector is modelled over the pointer notion the code actually computes,
   not the seq-count notion (which remains available in DAGState §3 as a
   separate mathematical characterization of an on-DAG fork). *)

(* Optional block-hash pointer equality (mirrors `Option<BlockHash>` `==`). *)
Definition ptr_eqb (p q : option BlockHash) : bool :=
  match p, q with
  | None, None => true
  | Some a, Some b => Nat.eqb a b
  | _, _ => false
  end.

Lemma ptr_eqb_refl : forall p, ptr_eqb p p = true.
Proof. destruct p; simpl; [apply Nat.eqb_refl | reflexivity]. Qed.

Lemma ptr_eqb_true_iff : forall p q, ptr_eqb p q = true <-> p = q.
Proof.
  intros p q. destruct p as [a|]; destruct q as [b|]; simpl; split; intro H;
    try discriminate; try reflexivity.
  - apply Nat.eqb_eq in H. subst. reflexivity.
  - inversion H. subst. apply Nat.eqb_refl.
Qed.

(* Pointer-based equivocation (equivocation_detector.rs:88-89): the arriving
   block's creator-justification pointer disagrees with the sender's current
   latest-message pointer. *)
Definition equivocates_ptr
  (creator_justification latest_message : option BlockHash) : bool :=
  negb (ptr_eqb creator_justification latest_message).

Definition detect
  (creator_justification latest_message : option BlockHash) (requestedAsDep : bool)
  : DetectorStatus :=
  if equivocates_ptr creator_justification latest_message
  then if requestedAsDep then DSAdmissible else DSIgnorable
  else DSValid.

(* Constructive divergence exhibit: the pointer detector fires on a
   self-justification-regression pointer mismatch (creator justification 7 ≠
   latest message 9) for which there need be NO second block at any single seq
   number — the case DAGState's seq-count `equivocates_b` structurally cannot
   represent. Conversely, matching pointers are classified non-equivocating.
   This is why the detector is modelled over `equivocates_ptr`, not bridged to
   the seq-count notion. *)
Example equivocates_ptr_diverges_from_seq_count :
  equivocates_ptr (Some 7) (Some 9) = true /\
  equivocates_ptr (Some 9) (Some 9) = false /\
  equivocates_ptr None (Some 9) = true /\
  equivocates_ptr None None = false.
Proof. repeat split; reflexivity. Qed.

(* The Neglected case is detected separately by checkNeglectedEquivocations
   (mirroring f1r3node's check_neglected_equivocations_with_update). It
   takes a record-set witness for the prior equivocation. *)
Definition detect_neglected
  (st : DAGState) (v : Validator) (n : nat) (requestedAsDep : bool)
  (records : EqStore)
  : DetectorStatus :=
  if andb requestedAsDep (has_key records (v, pred n))
  then DSNeglected
  else DSValid.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — T-1: Detection soundness
   ═══════════════════════════════════════════════════════════════════════════

   If detect emits Admissible or Ignorable, the arriving block's creator
   justification really disagrees with the sender's latest message — a genuine
   pointer equivocation. *)

Theorem detection_sound :
  forall cj lm d s,
    detect cj lm d = s ->
    s = DSAdmissible \/ s = DSIgnorable ->
    equivocates_ptr cj lm = true.
Proof.
  intros cj lm d s Hd Hs.
  unfold detect in Hd.
  destruct (equivocates_ptr cj lm) eqn:E.
  - reflexivity.
  - subst s. destruct Hs as [Hs | Hs]; discriminate.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-2: Detection completeness
   ═══════════════════════════════════════════════════════════════════════════

   If the creator-justification pointer disagrees with the latest-message
   pointer, then detect emits Admissible or Ignorable (per the requestedAsDep
   flag). *)

Theorem detection_complete :
  forall cj lm d,
    equivocates_ptr cj lm = true ->
    detect cj lm d = DSAdmissible \/ detect cj lm d = DSIgnorable.
Proof.
  intros cj lm d He.
  unfold detect. rewrite He.
  destruct d; [left; reflexivity | right; reflexivity].
Qed.

(* Stronger version: the detected status matches the dependency flag. *)
Theorem detection_complete_strong :
  forall cj lm d,
    equivocates_ptr cj lm = true ->
    detect cj lm d = (if d then DSAdmissible else DSIgnorable).
Proof.
  intros cj lm d He. unfold detect. rewrite He. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Correspondence: Status to InvalidBlock
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem detect_status_to_invalid_slashable :
  forall cj lm d s ib,
    detect cj lm d = s ->
    status_to_invalid s = Some ib ->
    is_slashable ib = true.
Proof.
  intros cj lm d s ib Hd Hs.
  unfold detect in Hd. unfold status_to_invalid in Hs.
  destruct (equivocates_ptr cj lm).
  - destruct d; subst s; inversion Hs; subst; reflexivity.
  - subst s. discriminate.
Qed.

(* The above theorem confirms T-3 in the detection context: every
   detector-emitted invalid-block status (post-fix) is in the slashable set. *)

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — detect_neglected: soundness and completeness
   ═══════════════════════════════════════════════════════════════════════════

   detect_neglected returns DSNeglected only when the block's latest-message
   view makes the record detectable AND a record exists for (v, n-1).  The
   boolean argument abstracts Rust's is_equivocation_detectable predicate.
   We prove this is sound and complete for that boolean view witness. *)

Theorem detect_neglected_sound :
  forall st v n d records,
    detect_neglected st v n d records = DSNeglected ->
    d = true /\ has_key records (v, pred n) = true.
Proof.
  intros st v n d records H. unfold detect_neglected in H.
  destruct (andb d (has_key records (v, pred n))) eqn:E.
  - apply andb_true_iff in E. destruct E as [Hd Hk]. tauto.
  - discriminate.
Qed.

Theorem detect_neglected_complete :
  forall st v n records,
    has_key records (v, pred n) = true ->
    detect_neglected st v n true records = DSNeglected.
Proof.
  intros st v n records Hk. unfold detect_neglected.
  rewrite Hk. simpl. reflexivity.
Qed.

Definition detect_neglected_rust_view
  (st : DAGState) (v : Validator) (n : nat)
  (detectableInView offenderBonded alreadyAcknowledged : bool)
  (records : EqStore)
  : DetectorStatus :=
  if andb (has_key records (v, pred n))
          (andb detectableInView
                (andb offenderBonded (negb alreadyAcknowledged)))
  then DSNeglected
  else DSValid.

Theorem detect_neglected_rust_view_sound :
  forall st v n detectable bonded acknowledged records,
    detect_neglected_rust_view st v n detectable bonded acknowledged records = DSNeglected ->
    has_key records (v, pred n) = true /\
    detectable = true /\
    bonded = true /\
    acknowledged = false.
Proof.
  intros st v n detectable bonded acknowledged records H.
  unfold detect_neglected_rust_view in H.
  destruct (andb (has_key records (v, pred n))
                 (andb detectable (andb bonded (negb acknowledged)))) eqn:E.
  - apply andb_true_iff in E. destruct E as [Hkey Hrest].
    apply andb_true_iff in Hrest. destruct Hrest as [Hdet Hrest].
    apply andb_true_iff in Hrest. destruct Hrest as [Hbond Hack].
    apply negb_true_iff in Hack.
    repeat split; assumption.
  - discriminate.
Qed.

Theorem detect_neglected_rust_view_complete :
  forall st v n records,
    has_key records (v, pred n) = true ->
    detect_neglected_rust_view st v n true true false records = DSNeglected.
Proof.
  intros st v n records Hkey.
  unfold detect_neglected_rust_view.
  rewrite Hkey. reflexivity.
Qed.

Inductive ViewContribution : Type :=
  | VCNoEvidence : ViewContribution
  | VCChild : nat -> ViewContribution
  | VCDetected : ViewContribution.

Fixpoint child_hashes (xs : list ViewContribution) : list nat :=
  match xs with
  | [] => []
  | VCNoEvidence :: rest => child_hashes rest
  | VCChild h :: rest => h :: child_hashes rest
  | VCDetected :: rest => child_hashes rest
  end.

Fixpoint detected_hash_seen (xs : list ViewContribution) : bool :=
  match xs with
  | [] => false
  | VCNoEvidence :: rest => detected_hash_seen rest
  | VCChild _ :: rest => detected_hash_seen rest
  | VCDetected :: _ => true
  end.

Definition fixed_detectable_view (xs : list ViewContribution) : bool :=
  detected_hash_seen xs ||
  Nat.leb 2 (length (nodup Nat.eq_dec (child_hashes xs))).

Theorem fixed_detectable_missing_pointer_prefix :
  forall xs,
    fixed_detectable_view (VCNoEvidence :: xs) = fixed_detectable_view xs.
Proof.
  intros xs. reflexivity.
Qed.

Theorem fixed_detectable_detected_hash_true :
  forall xs,
    fixed_detectable_view (VCDetected :: xs) = true.
Proof.
  intros xs. reflexivity.
Qed.

Theorem fixed_detectable_duplicate_single_child_false :
  forall h,
    fixed_detectable_view [VCChild h; VCChild h] = false.
Proof.
  intros h. unfold fixed_detectable_view. simpl.
  destruct (Nat.eq_dec h h) as [_ | Hneq].
  - simpl. reflexivity.
  - contradiction.
Qed.

Theorem fixed_detectable_two_distinct_children_true :
  forall h1 h2,
    h1 <> h2 ->
    fixed_detectable_view [VCChild h1; VCChild h2] = true.
Proof.
  intros h1 h2 Hneq. unfold fixed_detectable_view. simpl.
  destruct (Nat.eq_dec h1 h2) as [Heq | _].
  - contradiction.
  - destruct (Nat.eq_dec h2 h1) as [Heq | _].
    + symmetry in Heq. contradiction.
    + reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6b — Neglected-equivocation discovery status (Bug fix #1 no-corruption)
   ═══════════════════════════════════════════════════════════════════════════

   Models `EquivocationDiscoveryStatus` and
   `get_equivocation_discovery_status_for_bonded_validator`
   (equivocation_detector.rs:285-313): a bonded (stake > 0) validator's record
   resolves to Neglected iff its equivocation is detectable in the observing
   block's latest-message view, else Oblivious; an unbonded (stake 0) validator
   resolves to Detected.

   Fix #1 mints, for an UnauthorizedSlashDeploy, the empty-witness record
   `EquivocationRecord::new(sender, seq-1, {})` (BugFixDispatcher.dispatch_post_fix
   uses `mkEqRec offender baseSeq nil`). The lemma below shows the empty record
   does NOT corrupt neglected-equivocation detection: for an honest sender —
   whose view carries no detected-hash marker (the witness set is empty, so the
   `equivocation_detected_block_hashes.contains(..)` early return never fires)
   and at most one distinct canonical child (a single self-chain) — the fixed
   detectable view is FALSE, hence the discovery status is Oblivious, never a
   spurious NeglectedEquivocation against the honest validator. *)

Inductive EquivDiscoveryStatus : Type :=
  | EDNeglected : EquivDiscoveryStatus
  | EDDetected  : EquivDiscoveryStatus
  | EDOblivious : EquivDiscoveryStatus.

(* FV audit #6 remediation (unbonded-window record pollution fork). The
   stake-0 / unbonded branch now returns EDOblivious rather than EDDetected.

   Rationale (equivocation_detector.rs:280,311). Pre-fix, an unbonded/stake-0
   offender resolved to EDDetected, and the caller's Detected arm STAMPED the
   currently-validated block's hash into the (empty) EquivocationRecord. That
   write is observation-order-dependent, so after the offender re-bonds the
   witness set drives per-node-divergent NeglectedEquivocation verdicts — a
   consensus fork. Returning EDOblivious makes the stamping arm unreachable, so
   the witness set stays empty and detectability reduces to the deterministic
   `updated_equivocation_children.len() > 1` mechanism, on which all nodes agree.
   Matches slashing-specification.md §11.6 ("stake-0 offender: detected but never
   slashed AND never recorded"). The EDDetected constructor is KEPT (it mirrors
   the still-present Rust EquivocationDiscoveryStatus::EquivocationDetected
   variant and is exercised by `stamp_on_status` below), it is simply no longer
   produced here. *)
Definition discovery_status_bonded (stake : nat) (detectable : bool) : EquivDiscoveryStatus :=
  if Nat.ltb 0 stake
  then if detectable then EDNeglected else EDOblivious
  else EDOblivious.

Lemma honest_empty_record_view_not_detectable :
  forall xs,
    detected_hash_seen xs = false ->
    Nat.leb 2 (length (nodup Nat.eq_dec (child_hashes xs))) = false ->
    fixed_detectable_view xs = false.
Proof.
  intros xs H1 H2. unfold fixed_detectable_view. rewrite H1, H2. reflexivity.
Qed.

Theorem honest_empty_record_oblivious :
  forall stake xs,
    stake > 0 ->
    detected_hash_seen xs = false ->
    Nat.leb 2 (length (nodup Nat.eq_dec (child_hashes xs))) = false ->
    discovery_status_bonded stake (fixed_detectable_view xs) = EDOblivious.
Proof.
  intros stake xs Hstake H1 H2.
  rewrite (honest_empty_record_view_not_detectable xs H1 H2).
  unfold discovery_status_bonded.
  destruct (Nat.ltb 0 stake) eqn:E.
  - reflexivity.
  - apply Nat.ltb_ge in E. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6c — FV audit #6: unbonded-window record pollution is neutralized
   ═══════════════════════════════════════════════════════════════════════════

   Mirrors the caller `check_neglected_equivocation`
   (equivocation_detector.rs:207-229): ONLY the EquivocationDetected arm mutates
   the record (it inserts the observer block's hash into
   `equivocation_detected_block_hashes`); the Neglected and Oblivious arms leave
   the record untouched. `stamp_on_status` is that caller, distilled to its
   record effect. *)

Definition stamp_on_status
  (r : EqRec) (st : EquivDiscoveryStatus) (h : BlockHash) : EqRec :=
  match st with
  | EDDetected => mkEqRec (er_validator r) (er_baseSeq r) (h :: er_hashes r)
  | _ => r
  end.

(* The post-fix discovery status for an unbonded (stake-0) offender is always
   EquivocationOblivious, independent of the observing view's detectability. *)
Theorem unbonded_offender_oblivious :
  forall d, discovery_status_bonded 0 d = EDOblivious.
Proof. intro d. unfold discovery_status_bonded. reflexivity. Qed.

(* Because the unbonded status is Oblivious (never Detected), the caller's
   stamping arm is unreachable: stamping an unbonded offender's record is a
   no-op, so the witness set can never be polluted while the offender is
   unbonded. This is the root-cause fix for the fork. *)
Theorem unbonded_stamp_noop :
  forall r d h, stamp_on_status r (discovery_status_bonded 0 d) h = r.
Proof.
  intros r d h. rewrite unbonded_offender_oblivious. reflexivity.
Qed.

(* Determinism capstone (refutes the fork). Two nodes that observe an unbonded
   offender's record in different orders — stamping candidate hashes h1 then h2,
   or h2 then h1 — reach the SAME record, and that record is exactly the
   original r (no hash was recorded). Since the result is independent of the
   observation order, no two honest nodes can derive divergent witness sets, so
   the observation-order-dependent NeglectedEquivocation divergence cannot
   arise. *)
Theorem unbonded_witness_order_independent :
  forall r d h1 h2,
    let st := discovery_status_bonded 0 d in
    stamp_on_status (stamp_on_status r st h1) st h2
    = stamp_on_status (stamp_on_status r st h2) st h1
    /\ stamp_on_status (stamp_on_status r st h1) st h2 = r.
Proof.
  intros r d h1 h2. cbv zeta.
  rewrite !(unbonded_offender_oblivious d).
  split; reflexivity.
Qed.

Fixpoint detector_traversal_fuel
  (fuel : nat) (step : nat -> option nat) (current : nat) : list nat :=
  match fuel with
  | 0 => []
  | S k =>
      current ::
      match step current with
      | Some next => detector_traversal_fuel k step next
      | None => []
      end
  end.

Theorem detector_traversal_fuel_length_bound :
  forall fuel step current,
    length (detector_traversal_fuel fuel step current) <= fuel.
Proof.
  induction fuel as [| fuel IH]; intros step current; simpl.
  - lia.
  - destruct (step current) as [next |].
    + specialize (IH step next). lia.
    + change (S 0 <= S fuel). apply le_n_S. apply Nat.le_0_l.
Qed.

Theorem detector_traversal_zero_empty :
  forall step current,
    detector_traversal_fuel 0 step current = [].
Proof.
  reflexivity.
Qed.

Definition detector_two_cycle_step (n : nat) : option nat :=
  if Nat.eq_dec n 0 then Some 1 else Some 0.

Example detector_traversal_cycle_is_fuel_bounded :
  detector_traversal_fuel 4 detector_two_cycle_step 0 = [0; 1; 0; 1].
Proof.
  vm_compute. reflexivity.
Qed.

Definition nat_member (x : nat) (xs : list nat) : bool :=
  if in_dec Nat.eq_dec x xs then true else false.

Lemma nat_member_true_iff :
  forall x xs, nat_member x xs = true <-> In x xs.
Proof.
  intros x xs. unfold nat_member.
  destruct (in_dec Nat.eq_dec x xs) as [Hin | Hnot].
  - split; intros; [assumption | reflexivity].
  - split; intros H; [discriminate | contradiction].
Qed.

Definition filter_nat_domain (domain xs : list nat) : list nat :=
  filter (fun x => nat_member x domain) xs.

Lemma filter_nat_domain_in :
  forall domain xs x,
    In x (filter_nat_domain domain xs) <-> In x xs /\ In x domain.
Proof.
  intros domain xs x. unfold filter_nat_domain.
  rewrite filter_In. split.
  - intros [Hin Hmember]. apply nat_member_true_iff in Hmember.
    split; assumption.
  - intros [Hin Hdomain]. split; [assumption |].
    apply nat_member_true_iff. assumption.
Qed.

Definition branch_successors (g : nat -> list nat) (seen : list nat) : list nat :=
  flat_map g seen.

Definition branch_traversal_step
  (domain : list nat) (g : nat -> list nat) (seen : list nat) : list nat :=
  nodup Nat.eq_dec
    (filter_nat_domain domain (seen ++ branch_successors g seen)).

Fixpoint branch_traversal_after
  (domain : list nat) (g : nat -> list nat) (seen : list nat) (fuel : nat)
  : list nat :=
  match fuel with
  | 0 => seen
  | S k => branch_traversal_after domain g
             (branch_traversal_step domain g seen) k
  end.

Definition branch_traversal_fixed
  (domain : list nat) (g : nat -> list nat) (seen : list nat) : Prop :=
  incl (branch_traversal_step domain g seen) seen.

Lemma branch_traversal_step_contains_seen :
  forall domain g seen,
    incl seen domain ->
    incl seen (branch_traversal_step domain g seen).
Proof.
  intros domain g seen Hdomain x Hin.
  unfold branch_traversal_step.
  rewrite nodup_In. apply filter_nat_domain_in. split.
  - apply in_or_app. left. assumption.
  - apply Hdomain. assumption.
Qed.

Lemma branch_traversal_step_preserves_domain :
  forall domain g seen,
    incl (branch_traversal_step domain g seen) domain.
Proof.
  intros domain g seen x Hin.
  unfold branch_traversal_step in Hin.
  rewrite nodup_In in Hin. apply filter_nat_domain_in in Hin.
  destruct Hin as [_ Hdomain]. assumption.
Qed.

Lemma branch_traversal_step_NoDup :
  forall domain g seen,
    NoDup (branch_traversal_step domain g seen).
Proof.
  intros. unfold branch_traversal_step. apply NoDup_nodup.
Qed.

Lemma branch_traversal_step_arg_monotone :
  forall domain g s1 s2,
    incl s1 s2 ->
    incl (branch_traversal_step domain g s1)
         (branch_traversal_step domain g s2).
Proof.
  intros domain g s1 s2 Hsub x Hin.
  unfold branch_traversal_step in *.
  rewrite nodup_In in *. apply filter_nat_domain_in in Hin.
  destruct Hin as [Hin Hdomain].
  apply filter_nat_domain_in. split; [|assumption].
  apply in_app_or in Hin. destruct Hin as [Hs1 | Hsucc].
  - apply in_or_app. left. apply Hsub. assumption.
  - apply in_or_app. right. unfold branch_successors in *.
    apply in_flat_map in Hsucc.
    destruct Hsucc as [v [Hv Hg]].
    apply in_flat_map. exists v. split; [apply Hsub; assumption | assumption].
Qed.

Lemma branch_traversal_after_preserves_domain :
  forall fuel domain g seen,
    incl seen domain ->
    incl (branch_traversal_after domain g seen fuel) domain.
Proof.
  induction fuel as [| fuel IH]; intros domain g seen Hdomain; simpl.
  - assumption.
  - apply IH. apply branch_traversal_step_preserves_domain.
Qed.

Lemma branch_traversal_after_arg_monotone :
  forall fuel domain g s1 s2,
    incl s1 s2 ->
    incl (branch_traversal_after domain g s1 fuel)
         (branch_traversal_after domain g s2 fuel).
Proof.
  induction fuel as [| fuel IH]; intros domain g s1 s2 Hsub; simpl.
  - assumption.
  - apply IH. apply branch_traversal_step_arg_monotone. assumption.
Qed.

Lemma branch_traversal_after_contains_start :
  forall fuel domain g seen,
    incl seen domain ->
    incl seen (branch_traversal_after domain g seen fuel).
Proof.
  induction fuel as [| fuel IH]; intros domain g seen Hdomain; simpl.
  - intros x H. assumption.
  - intros x H.
    apply IH.
    + apply branch_traversal_step_preserves_domain.
    + apply branch_traversal_step_contains_seen; assumption.
Qed.

Lemma NoDup_same_length_incl_reverse_nat :
  forall (xs ys : list nat),
    NoDup xs ->
    incl xs ys ->
    length xs = length ys ->
    incl ys xs.
Proof.
  intros xs ys Hndx Hsub Hlen y Hy.
  destruct (in_dec Nat.eq_dec y xs) as [Hin | Hnot]; [assumption |].
  exfalso.
  assert (NoDup (y :: xs)) as Hndyx.
  { constructor; assumption. }
  assert (incl (y :: xs) ys) as Hsubyx.
  { intros z Hz. destruct Hz as [Hz | Hz].
    - subst. assumption.
    - apply Hsub. assumption. }
  pose proof (NoDup_incl_length Hndyx Hsubyx) as Hle.
  simpl in Hle. lia.
Qed.

Theorem branch_traversal_fixed_stable :
  forall domain g seen n,
    incl seen domain ->
    branch_traversal_fixed domain g seen ->
    incl (branch_traversal_after domain g seen n) seen /\
    incl seen (branch_traversal_after domain g seen n).
Proof.
  intros domain g seen n Hdomain Hfixed.
  induction n as [| n IH]; simpl.
  - split; intros x H; assumption.
  - split.
    + intros x Hx.
      destruct IH as [Hto _].
      apply Hto.
      apply (@branch_traversal_after_arg_monotone n domain g
               (branch_traversal_step domain g seen) seen Hfixed).
      assumption.
    + intros x Hx.
      apply branch_traversal_after_contains_start.
      * apply branch_traversal_step_preserves_domain.
      * apply branch_traversal_step_contains_seen; assumption.
Qed.

Theorem branch_traversal_fixed_point_after_stable :
  forall domain g seen n,
    incl seen domain ->
    branch_traversal_fixed domain g seen ->
    branch_traversal_fixed domain g
      (branch_traversal_after domain g seen n).
Proof.
  intros domain g seen n Hdomain Hfixed.
  pose proof (@branch_traversal_fixed_stable domain g seen n Hdomain Hfixed)
    as [Hto Hfrom].
  unfold branch_traversal_fixed in *.
  intros x Hx.
  apply Hfrom.
  apply Hfixed.
  apply (@branch_traversal_step_arg_monotone domain g
           (branch_traversal_after domain g seen n) seen Hto).
  assumption.
Qed.

Theorem branch_traversal_fixed_after_remaining :
  forall fuel domain g seen,
    NoDup domain ->
    NoDup seen ->
    incl seen domain ->
    length domain - length seen <= fuel ->
    branch_traversal_fixed domain g
      (branch_traversal_after domain g seen fuel).
Proof.
  induction fuel as [| fuel IH]; intros domain g seen Hndd Hnds Hsub Hfuel.
  - simpl.
    pose proof (NoDup_incl_length Hnds Hsub) as Hle.
    assert (length seen = length domain) as Hlen by lia.
    assert (incl domain seen) as Hfull.
    { apply NoDup_same_length_incl_reverse_nat; assumption. }
    unfold branch_traversal_fixed.
    intros x Hx. apply Hfull.
    apply branch_traversal_step_preserves_domain in Hx. assumption.
  - simpl.
    remember (branch_traversal_step domain g seen) as step_seen eqn:Hstep.
    destruct (Nat.eq_dec (length step_seen) (length seen)) as [Hlen | Hlen].
    + assert (Hseen_step : incl seen step_seen).
      { subst. apply branch_traversal_step_contains_seen. assumption. }
      assert (Hstep_seen : incl step_seen seen).
      { apply (@NoDup_same_length_incl_reverse_nat seen step_seen); try assumption.
        symmetry. assumption. }
      assert (Hstep_domain : incl step_seen domain).
      { subst. apply branch_traversal_step_preserves_domain. }
      assert (Hfixed_step : branch_traversal_fixed domain g step_seen).
      { unfold branch_traversal_fixed.
        subst. apply branch_traversal_step_arg_monotone. assumption. }
      apply branch_traversal_fixed_point_after_stable; assumption.
    + apply IH.
      * assumption.
      * subst. apply branch_traversal_step_NoDup.
      * subst. apply branch_traversal_step_preserves_domain.
      * assert (Hseen_step : incl seen step_seen).
        { subst. apply branch_traversal_step_contains_seen. assumption. }
        assert (Hstep_domain : incl step_seen domain).
        { subst. apply branch_traversal_step_preserves_domain. }
        assert (Hstep_nd : NoDup step_seen).
        { subst. apply branch_traversal_step_NoDup. }
        pose proof (NoDup_incl_length Hnds Hseen_step) as Hseen_le_step.
        pose proof (NoDup_incl_length Hstep_nd Hstep_domain) as Hstep_le_domain.
        lia.
Qed.

Theorem branch_traversal_fixed_after_domain_bound :
  forall domain g seen,
    NoDup domain ->
    NoDup seen ->
    incl seen domain ->
    branch_traversal_fixed domain g
      (branch_traversal_after domain g seen (length domain)).
Proof.
  intros domain g seen Hndd Hnds Hsub.
  apply branch_traversal_fixed_after_remaining; try assumption.
  pose proof (NoDup_incl_length Hnds Hsub) as Hle.
  lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Detector status decidability
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition detector_status_eq_dec :
  forall (s1 s2 : DetectorStatus), {s1 = s2} + {s1 <> s2}.
Proof. decide equality. Defined.
