(* ===========================================================================
   IntegerAdd.v - The IntegerAdd mergeable channel: wrapping-add group laws
   (T-ALG c), checked apply-to-base (T-ALG d), and the overflow-launder
   (EXHIBIT it is real → the checked-combine FIX is launder-free → a supply-cap
   bound under which it cannot arise).

   Two arithmetics meet in the merger:
     * COMBINE (rspace++/merging_logic.rs:42): folds a branch's per-channel
       IntegerAdd diffs with `wrapping_add` (silent mod-2^64).
     * APPLY-to-base (conflict_set_merger.rs:760): `checked_add`, rejecting the
       branch on overflow OR a negative vault result.

   The asymmetry is the launder: a branch whose diffs sum past i64::MAX wraps in
   COMBINE to an in-range value, which then passes the APPLY `checked_add >= 0`
   gate with a WRONG result. We (1) EXHIBIT a concrete launder, confirming it is
   real; (2) prove the fix — a `checked_combine` that rejects on overflow in the
   combine phase too — is launder-free (if it accepts, it returns the TRUE sum,
   never a wrapped value); and (3) prove a supply-cap bound: while every partial
   sum stays in range, wrapping = checked = true sum, so the launder cannot
   arise (defense in depth). BitmaskOr is unaffected (bitwise OR never overflows;
   modeled as a semilattice in Merge.v).

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                          | Rust
   ------------------------------+-----------------------------------------------
   wadd (= wrap (a+b))           | combine_mergeable_value IntegerAdd (merging_logic.rs:42)
   checked_add / checked_apply   | cal_merged_result IntegerAdd arm (conflict_set_merger.rs:760)
   wsum                          | fold of a branch's diffs via combine (the buggy path)
   checked_combine               | the fail-loudly FIX (W3): checked fold in combine
   launder_exhibit               | the confirmed defect
   checked_combine_sound         | fix is launder-free
   supply_cap_no_launder         | defense-in-depth bound
   =========================================================================== *)

From Stdlib Require Import ZArith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
Import ListNotations.
Open Scope Z_scope.

(* Signed 64-bit envelope. *)
Definition P63 : Z := 2 ^ 63.
Definition P64 : Z := 2 ^ 64.
Definition IMIN : Z := - P63.
Definition IMAX : Z := P63 - 1.
Definition in_range (z : Z) : Prop := IMIN <= z <= IMAX.

Lemma P63_pos : 0 < P63. Proof. unfold P63. lia. Qed.
Lemma P64_eq : P64 = 2 * P63. Proof. unfold P64, P63. lia. Qed.
Lemma P64_pos : P64 <> 0. Proof. unfold P64. lia. Qed.

(* ===========================================================================
   Section 1 - wrapping add (mod 2^64 into the signed window) and its group laws
   =========================================================================== *)

Definition wrap (z : Z) : Z := (z + P63) mod P64 - P63.
Definition wadd (a b : Z) : Z := wrap (a + b).

(* wrap is the identity on the representable range. *)
Lemma wrap_id : forall a, in_range a -> wrap a = a.
Proof.
  intros a Hr. unfold in_range, IMIN, IMAX in Hr. unfold wrap.
  rewrite Z.mod_small; [ring | rewrite P64_eq; lia].
Qed.

(* Pushing a wrap through an addition: wrap absorbs an already-wrapped operand. *)
Lemma wrap_wadd_l : forall x y, wrap (wrap x + y) = wrap (x + y).
Proof.
  intros x y. unfold wrap.
  replace ((x + P63) mod P64 - P63 + y + P63) with ((x + P63) mod P64 + y) by ring.
  rewrite Z.add_mod_idemp_l by exact P64_pos.
  replace (x + P63 + y) with (x + y + P63) by ring.
  reflexivity.
Qed.

Lemma wadd_comm : forall a b, wadd a b = wadd b a.
Proof. intros a b. unfold wadd. rewrite Z.add_comm. reflexivity. Qed.

(* T-ALG(c): wrapping add is associative. *)
Theorem wadd_assoc : forall a b c, wadd (wadd a b) c = wadd a (wadd b c).
Proof.
  intros a b c. unfold wadd.
  rewrite wrap_wadd_l.
  rewrite (Z.add_comm a (wrap (b + c))), wrap_wadd_l.
  f_equal. ring.
Qed.

(* T-ALG(c): 0 is the identity on the representable range. *)
Theorem wadd_0_l : forall a, in_range a -> wadd 0 a = a.
Proof. intros a Hr. unfold wadd. rewrite Z.add_0_l. apply wrap_id. exact Hr. Qed.

Theorem wadd_0_r : forall a, in_range a -> wadd a 0 = a.
Proof. intros a Hr. rewrite wadd_comm. apply wadd_0_l. exact Hr. Qed.

(* ===========================================================================
   Section 2 - checked apply-to-base (T-ALG d): reject on overflow OR result < 0
   =========================================================================== *)

Definition in_rangeb (z : Z) : bool := (IMIN <=? z) && (z <=? IMAX).

Lemma in_rangeb_true : forall z, in_rangeb z = true <-> in_range z.
Proof.
  intros z. unfold in_rangeb, in_range.
  rewrite andb_true_iff, Z.leb_le, Z.leb_le. reflexivity.
Qed.

(* checked_add: Some (a+b) if in range, else None (overflow). *)
Definition checked_add (a b : Z) : option Z :=
  if in_rangeb (a + b) then Some (a + b) else None.

(* Vault apply: reject on overflow OR a negative result. *)
Definition checked_apply (base diff : Z) : option Z :=
  if in_rangeb (base + diff) && (0 <=? base + diff) then Some (base + diff) else None.

Theorem checked_apply_rejects_overflow :
  forall base diff, ~ in_range (base + diff) -> checked_apply base diff = None.
Proof.
  intros base diff Hnr. unfold checked_apply.
  destruct (in_rangeb (base + diff)) eqn:E.
  - apply in_rangeb_true in E. contradiction.
  - reflexivity.
Qed.

Theorem checked_apply_rejects_negative :
  forall base diff, base + diff < 0 -> checked_apply base diff = None.
Proof.
  intros base diff Hneg. unfold checked_apply.
  replace (0 <=? base + diff) with false by (symmetry; apply Z.leb_gt; exact Hneg).
  rewrite andb_false_r. reflexivity.
Qed.

(* ===========================================================================
   Section 3 - the two combine folds and the true (unbounded) sum
   =========================================================================== *)

Definition true_sum (l : list Z) : Z := fold_right Z.add 0 l.

(* The BUGGY combine: wrapping fold (mirrors merging_logic.rs:42). *)
Definition wsum (l : list Z) : Z := fold_right wadd 0 l.

(* The FIX: checked fold — reject (None) on any overflow in the combine phase. *)
Fixpoint checked_combine (l : list Z) : option Z :=
  match l with
  | [] => Some 0
  | d :: r =>
      match checked_combine r with
      | Some a => checked_add d a
      | None => None
      end
  end.

(* ===========================================================================
   Section 4 - the FIX is launder-free: if checked_combine accepts, it returns
   the TRUE sum, in range. A wrapped value can never be laundered through it.
   =========================================================================== *)

Theorem checked_combine_sound :
  forall l c, checked_combine l = Some c -> c = true_sum l /\ in_range c.
Proof.
  induction l as [| d r IH]; intros c H; simpl in H.
  - injection H as H. subst c. split; [reflexivity | unfold in_range, IMIN, IMAX; pose proof P63_pos; lia].
  - destruct (checked_combine r) as [a |] eqn:Er; [| discriminate].
    destruct (IH a eq_refl) as [Ha _].
    unfold checked_add in H. destruct (in_rangeb (d + a)) eqn:Erange; [| discriminate].
    injection H as H. subst c. apply in_rangeb_true in Erange.
    unfold true_sum. simpl. fold (true_sum r). rewrite <- Ha. split; [reflexivity | exact Erange].
Qed.

(* Contrapositive framing: an out-of-range true sum is REJECTED (fails loudly),
   never laundered. *)
Corollary checked_combine_rejects_overflow :
  forall l, ~ in_range (true_sum l) -> checked_combine l = None.
Proof.
  intros l Hnr. destruct (checked_combine l) as [c |] eqn:E; [| reflexivity].
  destruct (checked_combine_sound l c E) as [Hc Hr]. subst c. contradiction.
Qed.

(* ===========================================================================
   Section 5 - EXHIBIT: the launder is real on the BUGGY (wrapping) path

   base = 0, diffs = [IMAX; IMAX; 2].  true_sum = 2*IMAX + 2 = 2^64 (overflow),
   but wsum wraps it to 0, and checked_apply 0 0 = Some 0 — a wrong value
   accepted past the >= 0 gate. The checked_combine FIX rejects the same input.
   =========================================================================== *)

Definition launder_diffs : list Z := [IMAX; IMAX; 2].

Theorem launder_exhibit :
  checked_apply 0 (wsum launder_diffs) = Some 0
  /\ ~ in_range (0 + true_sum launder_diffs)
  /\ 0 <> 0 + true_sum launder_diffs.
Proof.
  (* the wrapping fold laundered 2^64 down to 0; the true sum is 2^64 *)
  assert (Hw : wsum launder_diffs = 0) by (vm_compute; reflexivity).
  assert (Hs : true_sum launder_diffs = P64) by (vm_compute; reflexivity).
  pose proof P63_pos as Hp. pose proof P64_eq as He.
  split; [| split].
  - rewrite Hw. vm_compute. reflexivity.
  - rewrite Hs. unfold in_range, IMIN, IMAX. lia.
  - rewrite Hs. lia.
Qed.

(* The FIX rejects exactly that laundering input. *)
Theorem checked_combine_rejects_launder :
  checked_combine launder_diffs = None.
Proof.
  unfold launder_diffs, checked_combine, checked_add, in_rangeb, IMAX, IMIN, P63.
  vm_compute. reflexivity.
Qed.

(* ===========================================================================
   Section 6 - supply-cap bound: while every partial sum stays in range,
   wrapping = checked = true sum, so the launder cannot arise (defense in depth).
   =========================================================================== *)

(* `safe l` : every suffix-sum of the fold is representable. *)
Fixpoint safe (l : list Z) : Prop :=
  match l with
  | [] => True
  | d :: r => in_range (true_sum (d :: r)) /\ safe r
  end.

(* Definitional cons-unfoldings, so the proof controls reduction precisely
   (simpl unfolds true_sum/wsum inconsistently). *)
Lemma true_sum_cons : forall d r, true_sum (d :: r) = d + true_sum r.
Proof. reflexivity. Qed.
Lemma wsum_cons : forall d r, wsum (d :: r) = wadd d (wsum r).
Proof. reflexivity. Qed.
Lemma checked_combine_cons :
  forall d r,
    checked_combine (d :: r) =
      match checked_combine r with Some a => checked_add d a | None => None end.
Proof. reflexivity. Qed.

Theorem supply_cap_no_launder :
  forall l, safe l -> checked_combine l = Some (true_sum l) /\ wsum l = true_sum l.
Proof.
  induction l as [| d r IH]; intros Hsafe.
  - split; reflexivity.
  - destruct Hsafe as [Hhead Hrest].
    rewrite true_sum_cons in Hhead.
    destruct (IH Hrest) as [Hchk Hwsum].
    split.
    + rewrite checked_combine_cons, Hchk. unfold checked_add.
      assert (E : in_rangeb (d + true_sum r) = true) by (apply in_rangeb_true; exact Hhead).
      rewrite E, true_sum_cons. reflexivity.
    + rewrite wsum_cons, Hwsum, true_sum_cons. unfold wadd. apply wrap_id. exact Hhead.
Qed.
