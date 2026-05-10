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

Theorem er_key_injective :
  forall r1 r2,
    er_key r1 = er_key r2 ->
    er_validator r1 = er_validator r2 /\ er_baseSeq r1 = er_baseSeq r2.
Proof.
  intros r1 r2 H.
  unfold er_key in H.
  inversion H. split; reflexivity.
Qed.

Theorem canonical_key_pair_injective :
  forall (v1 v2 : Validator) (s1 s2 : nat),
    (v1, s1) = (v2, s2) ->
    v1 = v2 /\ s1 = s2.
Proof.
  intros v1 v2 s1 s2 H. inversion H. split; reflexivity.
Qed.

Definition naive_record_key_projection (v : Validator) (seq : nat) : nat :=
  v + seq.

Example naive_record_key_projection_collision :
  (1, 23) <> (12, 12) /\
  naive_record_key_projection 1 23 =
  naive_record_key_projection 12 12.
Proof.
  split; [intro H; inversion H; lia | reflexivity].
Qed.

Definition delimiter_free_record_key_projection
  (validator_digits seq_digits : list nat) : list nat :=
  validator_digits ++ seq_digits.

Example delimiter_free_record_key_projection_collision :
  ([1], [2; 3]) <> ([1; 2], [3]) /\
  delimiter_free_record_key_projection [1] [2; 3] =
  delimiter_free_record_key_projection [1; 2] [3].
Proof.
  split.
  - intro H. injection H as Hdigits _. discriminate Hdigits.
  - reflexivity.
Qed.

Example delimiter_free_record_key_projection_hypothesis_collision :
  ([1], [1; 0]) <> ([1; 1], [0]) /\
  delimiter_free_record_key_projection [1] [1; 0] =
  delimiter_free_record_key_projection [1; 1] [0].
Proof.
  split.
  - intro H. injection H as Hdigits _. discriminate Hdigits.
  - reflexivity.
Qed.

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

Fixpoint delete_record (s : EqStore) (k : Validator * nat) : EqStore :=
  match s with
  | [] => []
  | r :: rest =>
      if same_key r k then delete_record rest k else r :: delete_record rest k
  end.

Definition hashes_at_key (s : EqStore) (k : Validator * nat) : list BlockHash :=
  match find_by_key s k with
  | Some r => er_hashes r
  | None   => []
  end.

Definition hashes_equiv (xs ys : list BlockHash) : Prop :=
  forall h, In h xs <-> In h ys.

Theorem hashes_equiv_refl :
  forall xs, hashes_equiv xs xs.
Proof.
  intros xs h. split; intro H; assumption.
Qed.

Theorem hashes_equiv_sym :
  forall xs ys, hashes_equiv xs ys -> hashes_equiv ys xs.
Proof.
  intros xs ys H h. destruct (H h) as [Hxy Hyx]. split; assumption.
Qed.

Theorem hashes_equiv_trans :
  forall xs ys zs,
    hashes_equiv xs ys ->
    hashes_equiv ys zs ->
    hashes_equiv xs zs.
Proof.
  intros xs ys zs Hxy Hyz h.
  destruct (Hxy h) as [Hxy1 Hxy2].
  destruct (Hyz h) as [Hyz1 Hyz2].
  split; intro H.
  - apply Hyz1. apply Hxy1. assumption.
  - apply Hxy2. apply Hyz2. assumption.
Qed.

Theorem hashes_equiv_from_incl :
  forall xs ys,
    incl xs ys ->
    incl ys xs ->
    hashes_equiv xs ys.
Proof.
  intros xs ys Hxy Hyx h. split; intro H.
  - apply Hxy. assumption.
  - apply Hyx. assumption.
Qed.

Theorem hashes_equiv_duplicate_cons :
  forall h xs,
    hashes_equiv (h :: h :: xs) (h :: xs).
Proof.
  intros h xs x. split; intro H.
  - destruct H as [H | [H | H]].
    + left. assumption.
    + left. assumption.
    + right. assumption.
  - destruct H as [H | H].
    + left. assumption.
    + right. right. assumption.
Qed.

Theorem hashes_at_key_in_has_key :
  forall s k h,
    In h (hashes_at_key s k) ->
    has_key s k = true.
Proof.
  intros s k h Hin.
  unfold hashes_at_key, has_key in *.
  destruct (find_by_key s k); simpl in *.
  - reflexivity.
  - inversion Hin.
Qed.

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

Theorem record_lifecycle_update_retains_detected_hash :
  forall s k h_old h_new,
    In h_old (hashes_at_key s k) ->
    In h_old (hashes_at_key (update_record s k h_new) k).
Proof.
  intros s k h_old h_new Hin.
  apply t_4_record_monotone_update. assumption.
Qed.

Theorem current_rust_record_update_retains_all_detected_hashes :
  forall s k h_new,
    incl (hashes_at_key s k) (hashes_at_key (update_record s k h_new) k).
Proof.
  intros s k h_new x Hin.
  apply (t_4_record_monotone_update s k h_new k). assumption.
Qed.

Example record_lifecycle_early_delete_loses_detected_hash :
  forall v seq h,
    hashes_at_key
      (delete_record [mkEqRec v seq [h]] (v, seq))
      (v, seq) = [].
Proof.
  intros v seq h.
  unfold hashes_at_key, find_by_key. simpl.
  destruct (same_key (mkEqRec v seq [h]) (v, seq)) eqn:E.
  - reflexivity.
  - assert (Htrue : same_key (mkEqRec v seq [h]) (v, seq) = true).
    { apply same_key_true_iff. reflexivity. }
    congruence.
Qed.

Theorem update_record_contains_hash :
  forall s k h,
    has_key s k = true ->
    In h (hashes_at_key (update_record s k h) k).
Proof.
  intros s k h Hkey.
  unfold has_key in Hkey.
  unfold hashes_at_key.
  unfold find_by_key in Hkey |- *.
  induction s as [| r rest IH]; simpl in Hkey |- *.
  - discriminate.
  - destruct (same_key r k) eqn:Esk.
    + destruct (existsb (Nat.eqb h) (er_hashes r)) eqn:Eh.
      * simpl. rewrite Esk.
        apply existsb_exists in Eh.
        destruct Eh as [h' [Hin Heq]].
        apply Nat.eqb_eq in Heq. subst h'. assumption.
      * simpl. rewrite same_key_mkEqRec_eq. rewrite Esk.
        simpl. left. reflexivity.
    + simpl. rewrite Esk. apply IH. assumption.
Qed.

Theorem update_record_hashes_subset :
  forall s k h,
    incl (hashes_at_key (update_record s k h) k) (h :: hashes_at_key s k).
Proof.
  intros s k h x Hin.
  induction s as [| r rest IH].
  - simpl in Hin. inversion Hin.
  - destruct (same_key r k) eqn:Esk.
    + assert (Hold : hashes_at_key (r :: rest) k = er_hashes r).
      { unfold hashes_at_key, find_by_key. cbn [find]. rewrite Esk. reflexivity. }
      destruct (existsb (Nat.eqb h) (er_hashes r)) eqn:Eh.
      * assert (Hnew : hashes_at_key (update_record (r :: rest) k h) k = er_hashes r).
        { unfold hashes_at_key, find_by_key. cbn [update_record find].
          rewrite Esk. rewrite Eh. cbn [find]. rewrite Esk. reflexivity. }
        rewrite Hnew in Hin. rewrite Hold. right. assumption.
      * assert (Hnew :
          hashes_at_key (update_record (r :: rest) k h) k = h :: er_hashes r).
        { unfold hashes_at_key, find_by_key. cbn [update_record find].
          rewrite Esk. rewrite Eh. cbn [find].
          rewrite same_key_mkEqRec_eq. rewrite Esk. reflexivity. }
        rewrite Hnew in Hin. rewrite Hold. assumption.
    + assert (Hold : hashes_at_key (r :: rest) k = hashes_at_key rest k).
      { unfold hashes_at_key, find_by_key. cbn [find]. rewrite Esk. reflexivity. }
      assert (Hnew :
        hashes_at_key (update_record (r :: rest) k h) k =
        hashes_at_key (update_record rest k h) k).
      { unfold hashes_at_key, find_by_key. cbn [update_record].
        rewrite Esk. cbn [find]. rewrite Esk. reflexivity. }
      rewrite Hnew in Hin. rewrite Hold. apply IH. assumption.
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
