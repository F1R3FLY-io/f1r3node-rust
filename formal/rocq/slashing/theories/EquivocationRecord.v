(* ═══════════════════════════════════════════════════════════════════════════
   EquivocationRecord.v — Equivocation evidence persistence

   Models the EquivocationRecord type from
     models/src/rust/equivocation_record.rs
   and the corresponding Scala
     models/src/main/scala/coop/rchain/models/EquivocationRecord.scala

   A record is a triple (validator, baseSeqNum, witnessHashes) where
   witnessHashes is the monotonically growing set of block hashes that
   witness the equivocation.

   Theorems proved here:
     T-4 (Record monotonicity) — append-only growth of witnessHashes
     T-5 (Record uniqueness)   — at most one record per (validator, baseSeqNum)
     (T-6 / Bug-fix #2 lives in BugFixAtomicTracker.v.)

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition   │ Paper Notation              │ Rust Implementation
   ──────────────────┼─────────────────────────────┼─────────────────────
   EqRec             │ E ⊆ V × ℕ × P(H)            │ EquivocationRecord
   er_validator      │ validator                   │ .equivocator
   er_baseSeq        │ baseSeqNum                  │ .equivocation_base_block_seq_num
   er_hashes         │ equivocationDetectedHashes  │ .equivocation_detected_block_hashes
   record_monotone   │ T-4                          │ (verified property)
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §5.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Record type
   ═══════════════════════════════════════════════════════════════════════════ *)

Record EqRec : Type := mkEqRec {
  er_validator : Validator;
  er_baseSeq   : nat;
  er_hashes    : list BlockHash
}.

Definition er_key (r : EqRec) : Validator * nat :=
  (er_validator r, er_baseSeq r).

Definition key_eq_dec :
  forall (k1 k2 : Validator * nat), {k1 = k2} + {k1 <> k2}.
Proof.
  decide equality. apply Nat.eq_dec. apply validator_eq_dec.
Defined.

Definition same_key (r : EqRec) (k : Validator * nat) : bool :=
  if validator_eq_dec (er_validator r) (fst k)
  then if Nat.eq_dec (er_baseSeq r) (snd k) then true else false
  else false.

Lemma same_key_true_iff :
  forall r k, same_key r k = true <-> er_key r = k.
Proof.
  intros r k. unfold same_key, er_key. split.
  - destruct (validator_eq_dec (er_validator r) (fst k)) as [Hv | Hv]; [|discriminate].
    destruct (Nat.eq_dec (er_baseSeq r) (snd k)) as [Hs | Hs]; [|discriminate].
    intros _. destruct k as [a b]; simpl in *; subst; reflexivity.
  - intros Heq. destruct k as [a b]; simpl in *. inversion Heq; subst.
    destruct (validator_eq_dec (er_validator r) (er_validator r)) as [_ | C]; [|contradiction].
    destruct (Nat.eq_dec (er_baseSeq r) (er_baseSeq r)) as [_ | C]; [|contradiction].
    reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Record store (set keyed by er_key)
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition EqStore := list EqRec.

Definition find_by_key (s : EqStore) (k : Validator * nat) : option EqRec :=
  find (fun r => same_key r k) s.

Definition has_key (s : EqStore) (k : Validator * nat) : bool :=
  match find_by_key s k with Some _ => true | None => false end.

Definition insert_cond (s : EqStore) (r : EqRec) : EqStore :=
  if has_key s (er_key r) then s else r :: s.

Fixpoint update_record (s : EqStore) (k : Validator * nat) (h : BlockHash) : EqStore :=
  match s with
  | []        => []
  | r :: rest =>
      if same_key r k
      then if existsb (Nat.eqb h) (er_hashes r)
           then r :: rest
           else mkEqRec (er_validator r) (er_baseSeq r) (h :: er_hashes r) :: rest
      else r :: update_record rest k h
  end.

Definition hashes_at_key (s : EqStore) (k : Validator * nat) : list BlockHash :=
  match find_by_key s k with
  | Some r => er_hashes r
  | None   => []
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — find_by_key, hashes_at_key on insert_cond and update_record
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma find_insert_cond_other :
  forall s r k,
    er_key r <> k ->
    find_by_key (insert_cond s r) k = find_by_key s k.
Proof.
  intros s r k Hne. unfold insert_cond.
  destruct (has_key s (er_key r)); [reflexivity|].
  unfold find_by_key. simpl.
  destruct (same_key r k) eqn:E.
  - apply same_key_true_iff in E. contradiction.
  - reflexivity.
Qed.

Lemma find_insert_cond_same_absent :
  forall s r,
    has_key s (er_key r) = false ->
    find_by_key (insert_cond s r) (er_key r) = Some r.
Proof.
  intros s r H. unfold insert_cond. rewrite H.
  unfold find_by_key. simpl.
  destruct (same_key r (er_key r)) eqn:E.
  - reflexivity.
  - exfalso. assert (Heq : er_key r = er_key r) by reflexivity.
    apply same_key_true_iff in Heq. rewrite Heq in E. discriminate.
Qed.

Lemma find_insert_cond_same_present :
  forall s r,
    has_key s (er_key r) = true ->
    find_by_key (insert_cond s r) (er_key r) = find_by_key s (er_key r).
Proof.
  intros s r H. unfold insert_cond. rewrite H. reflexivity.
Qed.

Lemma find_update_other_record :
  forall s k h k',
    k' <> k ->
    find_by_key (update_record s k h) k' = find_by_key s k'.
Proof.
  intros s k h k' Hne.
  unfold find_by_key.
  induction s as [| r rest IH]; simpl.
  - reflexivity.
  - destruct (same_key r k) eqn:Esk.
    + destruct (existsb (Nat.eqb h) (er_hashes r)) eqn:Eh.
      * simpl. reflexivity.
      * simpl.
        destruct (same_key
          (mkEqRec (er_validator r) (er_baseSeq r) (h :: er_hashes r)) k')
          eqn:Esk'.
        -- apply same_key_true_iff in Esk'.
           apply same_key_true_iff in Esk.
           unfold er_key in *. simpl in Esk', Esk.
           destruct k as [a b]. destruct k' as [a' b']. simpl in *.
           inversion Esk; inversion Esk'; subst.
           contradiction.
        -- assert (Hsk : same_key r k' = false).
           { unfold same_key in Esk' |- *. simpl in Esk'. exact Esk'. }
           rewrite Hsk. reflexivity.
    + simpl.
      destruct (same_key r k') eqn:Esk'.
      * reflexivity.
      * exact IH.
Qed.

Lemma find_update_same_absent :
  forall s k h,
    has_key s k = false ->
    find_by_key (update_record s k h) k = None.
Proof.
  intros s k h Hk. unfold has_key in Hk.
  unfold find_by_key in Hk |- *.
  induction s as [| r rest IH]; simpl in Hk |- *.
  - reflexivity.
  - destruct (same_key r k) eqn:Esk.
    + discriminate.
    + simpl. rewrite Esk. apply IH. assumption.
Qed.

Lemma same_key_mkEqRec_eq :
  forall r h k,
    same_key (mkEqRec (er_validator r) (er_baseSeq r) h) k = same_key r k.
Proof.
  intros r h k. unfold same_key. simpl. reflexivity.
Qed.

Lemma find_update_same_present :
  forall s k h r0,
    find_by_key s k = Some r0 ->
    exists r1, find_by_key (update_record s k h) k = Some r1
            /\ er_validator r1 = er_validator r0
            /\ er_baseSeq r1   = er_baseSeq r0
            /\ incl (er_hashes r0) (er_hashes r1).
Proof.
  intros s k h r0 Hf.
  unfold find_by_key in Hf |- *.
  induction s as [| r rest IH]; simpl in Hf |- *.
  - discriminate.
  - destruct (same_key r k) eqn:Esk.
    + (* r is the matching record. *)
      inversion Hf; subst r0.
      destruct (existsb (Nat.eqb h) (er_hashes r)) eqn:Eh.
      * simpl. rewrite Esk. exists r. repeat split.
        intros x Hx. assumption.
      * simpl.
        rewrite same_key_mkEqRec_eq. rewrite Esk.
        eexists (mkEqRec (er_validator r) (er_baseSeq r) (h :: er_hashes r)).
        repeat split.
        intros x Hx. simpl. right. assumption.
    + simpl. rewrite Esk.
      destruct (IH Hf) as [r1 [Hf1 [Hv1 [Hb1 Hi1]]]].
      exists r1. tauto.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-4 Record monotonicity
   ═══════════════════════════════════════════════════════════════════════════

   For every key k', the hashes recorded at k' grow monotonically under
   both insert_cond and update_record. *)

Theorem t_4_record_monotone_insert_cond :
  forall s r k',
    incl (hashes_at_key s k') (hashes_at_key (insert_cond s r) k').
Proof.
  intros s r k' x Hin. unfold hashes_at_key in *.
  destruct (key_eq_dec (er_key r) k') as [Heq | Hne].
  - subst k'.
    destruct (find_by_key s (er_key r)) as [r0 |] eqn:E.
    + assert (Hk : has_key s (er_key r) = true).
      { unfold has_key. rewrite E. reflexivity. }
      rewrite (find_insert_cond_same_present s r Hk).
      rewrite E. assumption.
    + inversion Hin.
  - assert (Hf : find_by_key (insert_cond s r) k' = find_by_key s k')
      by (apply find_insert_cond_other; assumption).
    rewrite Hf. assumption.
Qed.

Theorem t_4_record_monotone_update :
  forall s k h k',
    incl (hashes_at_key s k') (hashes_at_key (update_record s k h) k').
Proof.
  intros s k h k' x Hin. unfold hashes_at_key in *.
  destruct (key_eq_dec k' k) as [Heq | Hne].
  - subst k'. destruct (find_by_key s k) as [r0 |] eqn:E.
    + destruct (find_update_same_present s k h E) as [r1 [Hf1 [_ [_ Hi]]]].
      rewrite Hf1. apply Hi. assumption.
    + inversion Hin.
  - assert (Hf : find_by_key (update_record s k h) k' = find_by_key s k')
      by (apply find_update_other_record; assumption).
    rewrite Hf. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — T-5 Record uniqueness
   ═══════════════════════════════════════════════════════════════════════════

   insert_cond preserves the predicate that find_by_key has at most one
   answer (i.e., the underlying list has no two records with equal keys). *)

Definition unique_keys (s : EqStore) : Prop :=
  NoDup (map er_key s).

Lemma in_with_key_implies_has_key :
  forall s r0 k,
    In r0 s ->
    er_key r0 = k ->
    has_key s k = true.
Proof.
  induction s as [| r rest IH]; intros r0 k Hin Hkey.
  - inversion Hin.
  - simpl in Hin. unfold has_key, find_by_key. simpl.
    destruct (same_key r k) eqn:Esk.
    + reflexivity.
    + destruct Hin as [E | Hin'].
      * subst r. apply same_key_true_iff in Hkey.
        rewrite Hkey in Esk. discriminate.
      * apply (IH r0 k Hin' Hkey).
Qed.

Theorem t_5_insert_cond_preserves_unique :
  forall s r,
    unique_keys s ->
    unique_keys (insert_cond s r).
Proof.
  intros s r Hu. unfold insert_cond.
  destruct (has_key s (er_key r)) eqn:E.
  - assumption.
  - unfold unique_keys in *. simpl.
    constructor; [|assumption].
    intros Hin.
    apply in_map_iff in Hin.
    destruct Hin as [r0 [Hkey Hin0]].
    assert (Hh : has_key s (er_key r) = true).
    { apply (@in_with_key_implies_has_key s r0 (er_key r) Hin0 Hkey). }
    rewrite Hh in E. discriminate.
Qed.

Lemma in_update_record_implies_in :
  forall (s : EqStore) (k : Validator * nat) (h : BlockHash) (r0 : EqRec),
    In r0 (update_record s k h) ->
    In (er_key r0) (map er_key s).
Proof.
  induction s as [| r rest IH]; intros k h r0 Hin; simpl in Hin |- *.
  - inversion Hin.
  - destruct (same_key r k) eqn:Esk1.
    + destruct (existsb (Nat.eqb h) (er_hashes r)).
      * destruct Hin as [Eh | Ht].
        -- left. subst r0. reflexivity.
        -- right. apply in_map. assumption.
      * simpl in Hin.
        destruct Hin as [Eh | Ht].
        -- left. subst r0. unfold er_key. simpl. reflexivity.
        -- right. apply in_map. assumption.
    + simpl in Hin.
      destruct Hin as [Eh | Ht].
      * left. subst r0. reflexivity.
      * right. apply (IH k h r0 Ht).
Qed.

Theorem t_5_update_record_preserves_unique :
  forall s k h,
    unique_keys s ->
    unique_keys (update_record s k h).
Proof.
  intros s k h Hu.
  induction s as [| r rest IH]; simpl.
  - constructor.
  - destruct (same_key r k) eqn:Esk.
    + destruct (existsb (Nat.eqb h) (er_hashes r)).
      * assumption.
      * unfold unique_keys in Hu |- *. simpl.
        inversion Hu; subst.
        constructor; [|assumption].
        unfold er_key. simpl. assumption.
    + unfold unique_keys in Hu |- *. simpl.
      inversion Hu; subst.
      constructor.
      * intros Hin.
        apply in_map_iff in Hin.
        destruct Hin as [r0 [Hk Hin0]].
        apply H1.
        rewrite <- Hk.
        apply (@in_update_record_implies_in rest k h r0 Hin0).
      * apply IH; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Slash-relevant idempotence
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem update_record_dup_noop :
  forall s k h,
    (exists r0, find_by_key s k = Some r0
              /\ In h (er_hashes r0)) ->
    update_record s k h = s.
Proof.
  intros s k h [r0 [Hf Hin]].
  induction s as [| r rest IH]; simpl in Hf |- *.
  - discriminate.
  - destruct (same_key r k) eqn:Esk.
    + inversion Hf; subst r0.
      assert (Hex : existsb (Nat.eqb h) (er_hashes r) = true).
      { apply existsb_exists. exists h.
        split; [assumption | apply Nat.eqb_refl]. }
      rewrite Hex. reflexivity.
    + f_equal. apply IH. assumption.
Qed.

Theorem insert_cond_dup_noop :
  forall s r,
    has_key s (er_key r) = true ->
    insert_cond s r = s.
Proof.
  intros s r H. unfold insert_cond. rewrite H. reflexivity.
Qed.
