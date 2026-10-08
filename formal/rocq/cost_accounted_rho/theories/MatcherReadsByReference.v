(* D-D2 (epic 8946, Phase D; decision record DR-104): the matcher reads by
   reference instead of copying.

   (a) A predicate by reference replaces a copy that was made only to test
       whether the locally free bitset of a value is empty. The owned
       predicate builds the bitset as a union of the bitsets of the parts,
       and the Rust union has the length of its longer argument. Thus the
       bitset is empty exactly when every part is empty (union_empty_iff,
       fold_union_empty_iff_all_empty), and the predicate by reference tests
       the parts without building the union.

   (b) The owned pattern expressions are filtered in place by Vec::retain.
       The model follows alloc::vec::Vec::retain: before the first removal a
       kept element stays in place; after it, each kept element is moved back;
       a removed element is dropped in place. The result is the filter
       (retain_equals_filter). With s the size of an element and w the size
       of a word, the work of the predicate reads, the moves and the drops
       fits n * (2 s + 3 w) when 3 w <= 2 s (retain_work_within_charge). A
       charge of one element for each entry undercounts
       (one_pass_charge_undercounts_retain).

   (c, d) A ground pattern is compared by reference. match_pars and the
       PartialEq implementations that it calls compare field by field and
       element by element and stop at the first difference. The model is a
       lockstep comparison of two lists. It reads the same number of elements
       from the target and from the pattern (lockstep_reads_equal), never
       more than the pattern holds (lockstep_reads_bounded_by_pattern), and
       the elements that it reads decide the comparison
       (prefix_reads_decide). Two inspections of the pattern therefore cover
       both sides (two_pattern_inspections_cover_lockstep_reads). The charge
       before D-D2 inspected the whole target, so it grew with elements that
       the comparison never reads (target_inspection_grows_with_unread_target).

   (e) D-E2 (DR-109): a membership scan (contains, position, a B-tree
       search) compares a probe with items of a container in lockstep. It
       reads no more of the probe than the container holds
       (membership_probe_reads_le_items), so two inspections of the container
       cover both sides (two_container_inspections_cover_membership_scan).
       One inspection of the probe and one of the container undercount
       (probe_and_container_inspections_undercount_scan).

   No axiom, parameter or admission is used. Section variables are
   universally quantified when the sections close.

   Rust correspondence: rholang/src/rust/interpreter/matcher/has_locally_free.rs
   (HasLocallyFreeRef), models/src/rust/utils.rs (union, retain_no_frees),
   rholang/src/rust/interpreter/matcher/spatial_matcher.rs
   (MatcherWork::reserve_retain_no_frees, match_ground_par), list_match.rs and
   fold_match.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia.
From CostAccountedRho Require MeteredComparison.
Import ListNotations.

(* (a) Unions of bitsets, as models::rust::utils::union. *)

Fixpoint union (a b : list nat) : list nat :=
  match a, b with
  | [], _ => b
  | _, [] => a
  | x :: a', y :: b' => Nat.lor x y :: union a' b'
  end.

Theorem union_length : forall a b, length (union a b) = Nat.max (length a) (length b).
Proof.
  induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
  rewrite IH. reflexivity.
Qed.

Theorem union_empty_iff : forall a b, union a b = [] <-> a = [] /\ b = [].
Proof.
  intros a b. split.
  - destruct a as [| x a], b as [| y b]; simpl; intros H; try discriminate; auto.
  - intros [Ha Hb]. subst. reflexivity.
Qed.

Theorem fold_union_empty_iff_all_empty : forall parts acc,
  fold_left union parts acc = [] <-> acc = [] /\ Forall (fun p => p = []) parts.
Proof.
  induction parts as [| p parts IH]; intros acc; simpl.
  - split.
    + intros H. split; [exact H | constructor].
    + intros [H _]. exact H.
  - split.
    + intros H. apply IH in H as [Hunion Hrest].
      apply union_empty_iff in Hunion as [Hacc Hp].
      split; [exact Hacc | constructor; assumption].
    + intros [Hacc Hall]. inversion Hall as [| ? ? Hp Hrest]; subst.
      apply IH. split; [reflexivity | exact Hrest].
Qed.

(* (c, d) Lockstep comparison. *)

Section Lockstep.
  Context {A : Type} (cmp : A -> A -> comparison).

  (* The numbers of elements that a lockstep comparison of at most n pairs
     reads from the target and from the pattern. *)
  Fixpoint prefix_reads (n : nat) (xs ys : list A) : nat * nat :=
    match n, xs, ys with
    | S n', x :: xs', y :: ys' =>
        match cmp x y with
        | Eq => (S (fst (prefix_reads n' xs' ys')), S (snd (prefix_reads n' xs' ys')))
        | _ => (1, 1)
        end
    | _, _, _ => (0, 0)
    end.

  Theorem lockstep_reads_equal : forall n xs ys,
    fst (prefix_reads n xs ys) = snd (prefix_reads n xs ys).
  Proof.
    induction n as [| n IH]; intros [| x xs] [| y ys]; simpl; try reflexivity.
    destruct (cmp x y); simpl; try reflexivity.
    rewrite IH. reflexivity.
  Qed.

  Theorem lockstep_reads_bounded_by_pattern : forall n xs ys,
    snd (prefix_reads n xs ys) <= length ys.
  Proof.
    induction n as [| n IH]; intros [| x xs] [| y ys]; simpl; try lia.
    destruct (cmp x y); simpl; try lia.
    specialize (IH xs ys). lia.
  Qed.

  Corollary two_pattern_inspections_cover_lockstep_reads : forall n xs ys,
    fst (prefix_reads n xs ys) + snd (prefix_reads n xs ys) <= 2 * length ys.
  Proof.
    intros n xs ys. rewrite lockstep_reads_equal.
    pose proof (lockstep_reads_bounded_by_pattern n xs ys). lia.
  Qed.

  Theorem prefix_reads_decide : forall n xs ys,
    MeteredComparison.compare_prefix cmp n
      (firstn (fst (prefix_reads n xs ys)) xs) (firstn (fst (prefix_reads n xs ys)) ys)
    = MeteredComparison.compare_prefix cmp n xs ys.
  Proof.
    induction n as [| n IH]; intros [| x xs] [| y ys]; simpl; try reflexivity.
    destruct (cmp x y) eqn:Hcmp; simpl; rewrite Hcmp; try reflexivity.
    apply IH.
  Qed.

  (* The charge before D-D2: one inspection of the target and one of the
     pattern. *)
  Definition legacy_ground_charge (xs ys : list A) : nat := length xs + length ys.

  Theorem target_inspection_grows_with_unread_target : forall x y tail ys,
    cmp x y <> Eq ->
    prefix_reads (S (length ys)) (x :: tail) (y :: ys) = (1, 1)
    /\ legacy_ground_charge (x :: tail) (y :: ys) = 2 + length tail + length ys.
  Proof.
    intros x y tail ys Hdiff. split.
    - simpl. destruct (cmp x y); [contradiction | reflexivity | reflexivity].
    - unfold legacy_ground_charge. simpl. lia.
  Qed.
End Lockstep.

Example legacy_ground_charge_example :
  prefix_reads Nat.compare 2 (0 :: repeat 1 4096) [1] = (1, 1)
  /\ legacy_ground_charge (0 :: repeat 1 4096) [1] = 4098.
Proof. split; vm_compute; reflexivity. Qed.

(* (e) D-E2 (DR-109): membership scans. Vec::contains, Iterator::position
   and a B-tree search compare a probe with items of a container, each with
   one lockstep comparison. The model compares the probe with every item,
   which bounds a scan that stops early or a search that compares only some
   items. Each comparison reads as much of the probe as of the item, so the
   probe side reads no more than the container holds
   (membership_probe_reads_le_items), and two inspections of the container
   cover both sides (two_container_inspections_cover_membership_scan). One
   inspection of the probe and one of the container undercount
   (probe_and_container_inspections_undercount_scan). *)

Section MembershipScan.
  Context {A : Type} (cmp : A -> A -> comparison).

  (* The elements that a scan of probe [x] over [items] reads, from the
     probe and from the items, with at most [n] pairs per comparison. *)
  Fixpoint scan_reads (n : nat) (x : list A) (items : list (list A)) : nat * nat :=
    match items with
    | [] => (0, 0)
    | y :: rest =>
        let here := prefix_reads cmp n x y in
        let later := scan_reads n x rest in
        (fst here + fst later, snd here + snd later)
    end.

  (* The elements that the container holds. *)
  Fixpoint items_length (items : list (list A)) : nat :=
    match items with
    | [] => 0
    | y :: rest => length y + items_length rest
    end.

  Theorem membership_item_reads_le_items : forall n x items,
    snd (scan_reads n x items) <= items_length items.
  Proof.
    induction items as [| y rest IH]; simpl; [lia |].
    pose proof (lockstep_reads_bounded_by_pattern cmp n x y). lia.
  Qed.

  Theorem membership_probe_reads_le_items : forall n x items,
    fst (scan_reads n x items) <= items_length items.
  Proof.
    induction items as [| y rest IH]; simpl; [lia |].
    pose proof (lockstep_reads_equal cmp n x y) as Hequal.
    pose proof (lockstep_reads_bounded_by_pattern cmp n x y). lia.
  Qed.

  Corollary two_container_inspections_cover_membership_scan : forall n x items,
    fst (scan_reads n x items) + snd (scan_reads n x items) <= 2 * items_length items.
  Proof.
    intros n x items.
    pose proof (membership_probe_reads_le_items n x items).
    pose proof (membership_item_reads_le_items n x items). lia.
  Qed.
End MembershipScan.

(* Negative control: the probe [1; 1; 1] scanned over three items of three
   elements reads 9 elements from each side, 18 in all. One inspection of
   the probe and one of the container pay 3 + 9 = 12. *)
Example probe_and_container_inspections_undercount_scan :
  scan_reads Nat.compare 3 [1; 1; 1] [[1; 1; 2]; [1; 1; 2]; [1; 1; 1]] = (9, 9)
  /\ items_length [[1; 1; 2]; [1; 1; 2]; [1; 1; 1]] = 9
  /\ 3 + 9 < 9 + 9.
Proof. split; [| split]; [vm_compute; reflexivity | reflexivity | lia]. Qed.

(* (b) Vec::retain. *)

Section Retain.
  Context {A : Type}.

  (* The kept elements, the number of moves and the number of drops of
     retain. [shifted] holds after the first removal. *)
  Fixpoint retain_run (keep : A -> bool) (shifted : bool) (xs : list A) : list A * nat * nat :=
    match xs with
    | [] => ([], 0, 0)
    | x :: rest =>
        if keep x then
          let '(kept, moves, drops) := retain_run keep shifted rest in
          (x :: kept, (if shifted then S moves else moves), drops)
        else
          let '(kept, moves, drops) := retain_run keep true rest in
          (kept, moves, S drops)
    end.

  Theorem retain_equals_filter : forall keep shifted xs,
    fst (fst (retain_run keep shifted xs)) = filter keep xs.
  Proof.
    intros keep shifted xs. revert shifted.
    induction xs as [| x rest IH]; intros shifted; simpl; [reflexivity |].
    destruct (keep x).
    - specialize (IH shifted).
      destruct (retain_run keep shifted rest) as [[kept moves] drops].
      simpl in *. rewrite IH. reflexivity.
    - specialize (IH true).
      destruct (retain_run keep true rest) as [[kept moves] drops].
      simpl in *. exact IH.
  Qed.

  Lemma retain_moves_and_drops : forall keep shifted xs,
    let '(_, moves, drops) := retain_run keep shifted xs in moves + drops <= length xs.
  Proof.
    intros keep shifted xs. revert shifted.
    induction xs as [| x rest IH]; intros shifted; simpl; [lia |].
    destruct (keep x).
    - specialize (IH shifted).
      destruct (retain_run keep shifted rest) as [[kept moves] drops].
      destruct shifted; simpl; lia.
    - specialize (IH true).
      destruct (retain_run keep true rest) as [[kept moves] drops].
      simpl. lia.
  Qed.

  Theorem retain_work_within_charge : forall keep s w xs,
    3 * w <= 2 * s ->
    let '(_, moves, drops) := retain_run keep false xs in
    3 * w * (length xs + drops) + 2 * s * moves <= length xs * (2 * s + 3 * w).
  Proof.
    intros keep s w xs Hword.
    pose proof (retain_moves_and_drops keep false xs) as Hcount.
    destruct (retain_run keep false xs) as [[kept moves] drops].
    assert (Hdrops : 3 * w * drops <= 2 * s * drops)
      by (apply Nat.mul_le_mono_r; exact Hword).
    assert (Hshift : 2 * s * (moves + drops) <= 2 * s * length xs)
      by (apply Nat.mul_le_mono_l; exact Hcount).
    nia.
  Qed.
End Retain.

(* Negative control: removing the first of four elements moves the other
   three, so a charge of one element for each entry undercounts. *)
Example one_pass_charge_undercounts_retain :
  retain_run (fun b : bool => b) false [false; true; true; true] = ([true; true; true], 3, 1)
  /\ 4 * 600 < 2 * 600 * 3.
Proof. split; [reflexivity | lia]. Qed.
