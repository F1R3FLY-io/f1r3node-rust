(* D-D5 (epic 8946, Phase D; decision record DR-107): the incoming datum of
   a produce is borrowed during the selection and moved into the result.

   Before D-D5, metered native replay copied the produce's value and source
   into the channel data at each occurrence of the produced channel. It also
   copied the value again as the removed value of each candidate that took
   the incoming datum. D-D5 borrows the incoming datum in the channel data
   and leaves the removed value of an incoming candidate at the default
   value. After the selection, the produce preparation moves the value into
   the first incoming candidate and copies it for each further one (a
   persistent datum matched through a repeated channel).

   The model is a list of candidate slots: an incoming slot, or a stored
   slot with its value.

   Results:
   - incoming_fill_equals_copies: after the fill, the removed values equal
     the legacy copies, for any number of incoming slots.
   - fill_gives_every_incoming_slot_the_value: no incoming slot keeps the
     default value.
   - selection_differs_only_at_incoming: before the fill, the selection
     differs from the legacy values only at incoming slots.
   - fill_moves_once, fill_copies_further: the fill moves the value once
     when there is an incoming slot, and copies it for each further one.
   - fill_without_incoming_moves_nothing: without an incoming slot, the fill
     moves and copies nothing, and it keeps the selected values.
   - borrowed_incoming_selection_equals_owned: a selection over a borrowed
     incoming datum equals the selection over an owned copy
     (NativeSharedReads.shared_selection_equals_deep_selection).
   Charges, in one dimension of the meter at a time, over the trace model of
   ChannelPositions:
   - fill_trace_covered, fill_trace_reserved: the fill reserves its pass
     before its reads, and each copy before the copy.
   - moved_charge_le_legacy, unmatched_charge_le_legacy: when one borrow and
     the pass cost no more than a copy of the value and of the source, the
     new charge is at most the legacy charge.
   - legacy_incoming_charge_counts_two_copies: for one occurrence and one
     incoming candidate, the legacy charge holds two copies of the value and
     one of the source.
   Negative controls:
   - legacy_incoming_charge_example: 210 against 28.
   - first_only_fill_differs_example: a fill of the first incoming slot only
     leaves another slot at the default value.
   - unfilled_selection_differs_example: the selection without the fill
     differs from the legacy copies.
   - copy_first_fill_is_not_covered_example: a copy before its reservation
     is not covered.
   - tiny_value_pass_exceeds_copies_example: without the premise of
     moved_charge_le_legacy, the pass can cost more than the copies.

   No axiom, parameter or admission is used. Section variables are
   universally quantified when the sections close.

   Rust correspondence:
   rspace++/src/rspace/replay_rspace/native_candidate/metered.rs
   (metered_channel_data, metered_match_data, fill_incoming,
   prepare_metered_produce_candidate, ProduceSelection) and
   rspace++/src/rspace/replay_rspace/native_session/operations.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia.
From CostAccountedRho Require ChannelPositions NativeSharedReads.
Import ListNotations.

Section Fill.
  Variable V : Type.
  Variable default : V.

  Inductive slot := Incoming | Stored (v : V).

  (* Before D-D5, each candidate held its own copy of its datum's value. *)
  Definition legacy_removed (x : V) (s : slot) : V :=
    match s with
    | Incoming => x
    | Stored v => v
    end.

  (* The selection leaves an incoming candidate at the default value. *)
  Definition selected_removed (s : slot) : V :=
    match s with
    | Incoming => default
    | Stored v => v
    end.

  Inductive origin := Moved | Copied | Kept.

  (* The fill: the first incoming slot receives the value itself, each
     further incoming slot a copy, and a stored slot keeps its value.
     [taken] says whether the value was already moved. *)
  Fixpoint fill (x : V) (taken : bool) (slots : list slot) : list (V * origin) :=
    match slots with
    | [] => []
    | Incoming :: rest => (x, if taken then Copied else Moved) :: fill x true rest
    | Stored v :: rest => (v, Kept) :: fill x taken rest
    end.

  Fixpoint incoming_count (slots : list slot) : nat :=
    match slots with
    | [] => 0
    | Incoming :: rest => S (incoming_count rest)
    | Stored _ :: rest => incoming_count rest
    end.

  Definition is_moved (o : origin) : bool := match o with Moved => true | _ => false end.
  Definition is_copied (o : origin) : bool := match o with Copied => true | _ => false end.

  Definition moves (result : list (V * origin)) : nat :=
    length (filter (fun entry => is_moved (snd entry)) result).
  Definition copies (result : list (V * origin)) : nat :=
    length (filter (fun entry => is_copied (snd entry)) result).

  Lemma fill_values : forall x slots taken,
    map fst (fill x taken slots) = map (legacy_removed x) slots.
  Proof.
    intros x slots. induction slots as [| [| v] rest IH]; intros taken; simpl;
      [reflexivity | f_equal; apply IH | f_equal; apply IH].
  Qed.

  Theorem incoming_fill_equals_copies : forall x slots,
    map fst (fill x false slots) = map (legacy_removed x) slots.
  Proof. intros x slots. apply fill_values. Qed.

  Theorem fill_gives_every_incoming_slot_the_value : forall x slots i,
    nth_error slots i = Some Incoming ->
    nth_error (map fst (fill x false slots)) i = Some x.
  Proof.
    intros x slots i Hincoming.
    rewrite incoming_fill_equals_copies, nth_error_map, Hincoming. reflexivity.
  Qed.

  Theorem selection_differs_only_at_incoming : forall x slots i,
    nth_error slots i <> Some Incoming ->
    nth_error (map selected_removed slots) i = nth_error (map (legacy_removed x) slots) i.
  Proof.
    intros x slots i Hstored. rewrite !nth_error_map.
    destruct (nth_error slots i) as [[| v] |]; simpl;
      [exfalso; apply Hstored; reflexivity | reflexivity | reflexivity].
  Qed.

  Lemma fill_counts : forall x slots taken,
    moves (fill x taken slots) = (if taken then 0 else Nat.min 1 (incoming_count slots)) /\
    copies (fill x taken slots)
    = (if taken then incoming_count slots else incoming_count slots - 1).
  Proof.
    intros x slots. induction slots as [| [| v] rest IH]; intros taken.
    - destruct taken; split; reflexivity.
    - destruct (IH true) as [Hmoves Hcopies].
      unfold moves, copies in *. destruct taken; simpl; rewrite Hmoves, Hcopies;
        split; simpl; lia.
    - destruct (IH taken) as [Hmoves Hcopies].
      unfold moves, copies in *. simpl. rewrite Hmoves, Hcopies. split; reflexivity.
  Qed.

  Theorem fill_moves_once : forall x slots,
    moves (fill x false slots) = Nat.min 1 (incoming_count slots).
  Proof. intros x slots. exact (proj1 (fill_counts x slots false)). Qed.

  Theorem fill_copies_further : forall x slots,
    copies (fill x false slots) = incoming_count slots - 1.
  Proof. intros x slots. exact (proj2 (fill_counts x slots false)). Qed.

  Lemma stored_slots_keep_selection : forall x slots,
    incoming_count slots = 0 -> map (legacy_removed x) slots = map selected_removed slots.
  Proof.
    intros x slots. induction slots as [| [| v] rest IH]; intros Hnone; simpl in *;
      [reflexivity | discriminate | f_equal; apply IH; exact Hnone].
  Qed.

  Theorem fill_without_incoming_moves_nothing : forall x slots,
    incoming_count slots = 0 ->
    moves (fill x false slots) = 0 /\ copies (fill x false slots) = 0 /\
    map fst (fill x false slots) = map selected_removed slots.
  Proof.
    intros x slots Hnone.
    rewrite fill_moves_once, fill_copies_further, incoming_fill_equals_copies, Hnone.
    split; [reflexivity | split; [reflexivity |]].
    apply stored_slots_keep_selection. exact Hnone.
  Qed.

  (* Negative control: a fill that moves the value into the first incoming
     slot and leaves the further ones at the default value. *)
  Fixpoint first_only (x : V) (taken : bool) (slots : list slot) : list V :=
    match slots with
    | [] => []
    | Incoming :: rest => (if taken then default else x) :: first_only x true rest
    | Stored v :: rest => v :: first_only x taken rest
    end.
End Fill.

(* The borrowed incoming datum is a view whose pointer resolves to the
   incoming value, so the selection over the borrowed entry equals the
   selection over an owned copy (C2, DR-82; D-D5, DR-107). *)
Corollary borrowed_incoming_selection_equals_owned :
  forall (payload : Type) (matches : payload -> bool) (heap : nat -> option payload)
    (pointer : nat) (incoming : payload) views values index,
  heap pointer = Some incoming ->
  NativeSharedReads.resolves payload heap views values ->
  NativeSharedReads.select_views payload matches heap index (pointer :: views)
  = NativeSharedReads.select_values payload matches index (incoming :: values).
Proof.
  intros payload matches heap pointer incoming views values index Hpointer Hviews.
  apply NativeSharedReads.shared_selection_equals_deep_selection.
  simpl. split; assumption.
Qed.

Section Charges.
  Variables scan move copy copy_source borrow : nat.

  (* The trace of the fill over the candidates, true for an incoming
     candidate: a read of each index, and a reservation and a copy for each
     incoming candidate after the first. *)
  Fixpoint fill_steps (taken : bool) (incoming : list bool) : list ChannelPositions.step :=
    match incoming with
    | [] => []
    | true :: rest =>
        ChannelPositions.Read scan ::
        (if taken then [ChannelPositions.Reserve copy; ChannelPositions.Read copy] else [])
        ++ fill_steps true rest
    | false :: rest => ChannelPositions.Read scan :: fill_steps taken rest
    end.

  (* The pass reservation first, then the loop, then the move. *)
  Definition fill_trace (incoming : list bool) : list ChannelPositions.step :=
    ChannelPositions.Reserve (length incoming * scan + move)
      :: fill_steps false incoming ++ [ChannelPositions.Read move].

  Fixpoint true_count (incoming : list bool) : nat :=
    match incoming with
    | [] => 0
    | true :: rest => S (true_count rest)
    | false :: rest => true_count rest
    end.

  Lemma fill_steps_covered : forall incoming taken credit,
    ChannelPositions.covered_from (credit + length incoming * scan + move)
      (fill_steps taken incoming ++ [ChannelPositions.Read move]).
  Proof.
    induction incoming as [| [|] rest IH]; intros taken credit; simpl.
    - split; [lia | exact I].
    - split; [nia |].
      replace (credit + (scan + length rest * scan) + move - scan)
        with (credit + length rest * scan + move) by lia.
      destruct taken; simpl.
      + split; [lia |].
        replace (credit + length rest * scan + move + copy - copy)
          with (credit + length rest * scan + move) by lia.
        apply IH.
      + apply IH.
    - split; [nia |].
      replace (credit + (scan + length rest * scan) + move - scan)
        with (credit + length rest * scan + move) by lia.
      apply IH.
  Qed.

  Theorem fill_trace_covered : forall incoming,
    ChannelPositions.covered (fill_trace incoming).
  Proof.
    intros incoming. unfold ChannelPositions.covered, fill_trace. simpl.
    replace (length incoming * scan + move) with (0 + length incoming * scan + move) by lia.
    apply fill_steps_covered.
  Qed.

  Lemma fill_steps_reserved : forall incoming taken,
    ChannelPositions.reserved (fill_steps taken incoming)
    = (if taken then true_count incoming else true_count incoming - 1) * copy.
  Proof.
    induction incoming as [| [|] rest IH]; intros taken; simpl.
    - destruct taken; reflexivity.
    - destruct taken; simpl; rewrite IH; simpl; lia.
    - apply IH.
  Qed.

  Theorem fill_trace_reserved : forall incoming,
    ChannelPositions.reserved (fill_trace incoming)
    = length incoming * scan + move + (true_count incoming - 1) * copy.
  Proof.
    intros incoming. unfold fill_trace. simpl.
    rewrite ChannelPositions.reserved_app, fill_steps_reserved. simpl. lia.
  Qed.

  (* The legacy charge: a copy of the value and of the source at each
     occurrence of the produced channel, and a copy of the value for each
     incoming candidate that an attempt pushed. *)
  Definition legacy_charge (occurrences pushes : nat) : nat :=
    occurrences * (copy + copy_source) + pushes * copy.

  (* The D-D5 charge: a borrow at each occurrence, and for the selected
     match with n candidates of which k are incoming, the pass and the
     further copies. *)
  Definition moved_charge (occurrences : nat) (matched : option (nat * nat)) : nat :=
    occurrences * borrow +
    match matched with
    | None => 0
    | Some (n, k) => n * scan + move + (k - 1) * copy
    end.

  Theorem moved_charge_le_legacy : forall occurrences pushes n k,
    1 <= occurrences -> k <= pushes -> borrow + n * scan + move <= copy + copy_source ->
    moved_charge occurrences (Some (n, k)) <= legacy_charge occurrences pushes.
  Proof.
    intros occurrences pushes n k Hoccurrences Hpushes Hpremise.
    unfold moved_charge, legacy_charge.
    destruct occurrences as [| others]; [lia |].
    assert (Hborrows : others * borrow <= others * (copy + copy_source))
      by (apply Nat.mul_le_mono_l; lia).
    assert (Hcopies : (k - 1) * copy <= pushes * copy) by (apply Nat.mul_le_mono_r; lia).
    simpl. nia.
  Qed.

  Theorem unmatched_charge_le_legacy : forall occurrences pushes,
    borrow <= copy + copy_source ->
    moved_charge occurrences None <= legacy_charge occurrences pushes.
  Proof.
    intros occurrences pushes Hborrow. unfold moved_charge, legacy_charge.
    assert (Hborrows : occurrences * borrow <= occurrences * (copy + copy_source))
      by (apply Nat.mul_le_mono_l; exact Hborrow).
    lia.
  Qed.

  Theorem legacy_incoming_charge_counts_two_copies :
    legacy_charge 1 1 = 2 * copy + copy_source /\
    moved_charge 1 (Some (1, 1)) = borrow + scan + move.
  Proof. unfold legacy_charge, moved_charge. split; simpl; lia. Qed.
End Charges.

(* Negative control: a value copy of 100, a source copy of 10, a borrow of
   8, a pass read of 4 and a move of 16. *)
Example legacy_incoming_charge_example :
  legacy_charge 100 10 1 1 = 210 /\ moved_charge 4 16 100 8 1 (Some (1, 1)) = 28.
Proof. split; reflexivity. Qed.

(* Negative control: with two incoming slots, a fill of the first slot only
   leaves the second one at the default value. *)
Example first_only_fill_differs_example :
  first_only nat 0 7 false [Incoming nat; Incoming nat] = [7; 0] /\
  map (legacy_removed nat 7) [Incoming nat; Incoming nat] = [7; 7].
Proof. split; reflexivity. Qed.

(* Negative control: the selection without the fill differs from the legacy
   copies at an incoming slot. *)
Example unfilled_selection_differs_example :
  map (selected_removed nat 0) [Incoming nat] <> map (legacy_removed nat 7) [Incoming nat].
Proof. simpl. discriminate. Qed.

(* Negative control: when the pass reserves exactly its reads, a copy that
   comes before its own reservation is not covered. *)
Example copy_first_fill_is_not_covered_example :
  ~ ChannelPositions.covered
    [ChannelPositions.Reserve 3; ChannelPositions.Read 1; ChannelPositions.Read 1;
     ChannelPositions.Read 5; ChannelPositions.Reserve 5; ChannelPositions.Read 1].
Proof. unfold ChannelPositions.covered. simpl. intros [_ [_ [Hcopy _]]]. lia. Qed.

(* Negative control: a value copy of 1, a source copy of 1, a borrow of 2, a
   pass read of 4 and a move of 8. Without the premise of
   moved_charge_le_legacy, the D-D5 charge exceeds the legacy charge. *)
Example tiny_value_pass_exceeds_copies_example :
  legacy_charge 1 1 1 1 = 3 /\ moved_charge 4 8 1 2 1 (Some (1, 1)) = 14.
Proof. split; reflexivity. Qed.
