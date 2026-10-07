(* D-D4 (epic 8946, Phase D; decision record DR-106): native replay records
   the position of each channel's entry when it builds the channel data, and
   the selection reads that position instead of searching for it.

   metered_channel_data walks the channels of a join in order. For each
   channel it searches the entries built so far for the first entry equal to
   the channel (channel_position). A found entry j takes the channel's
   values. A channel with no equal entry appends a new entry at the end.
   Before D-D4, the selection (metered_match_data) searched the finished
   entries again for each pattern. D-D4 records the found or appended
   position during the build, and the selection reads it.

   The model keeps only the channels of the entries, because the positions
   do not depend on the values. Entries never move once appended, so the
   proofs need only that the equality is reflexive.

   Results:
   - positions_length: one recorded position for each channel.
   - recorded_position_equals_first_equal_search: the recorded position of
     channel i is the position that the first-equal search finds in the
     finished entries.
   - first_equal_search_equals_recorded_positions: the same result for the
     whole channel list.
   - selection_inputs_equal: the selection reads the same (channel, pattern,
     entry) inputs from the recorded positions as from the searches, so it
     selects the same data.
   - recorded_position_names_equal_entry: a recorded position names an entry,
     and the channel equals that entry.
   - entries_pairwise_unequal: no entry equals an earlier entry.
   Charges, in one dimension of the meter at a time:
   - legacy_search_visits_position_plus_one: the search for a channel whose
     first equal entry is j visits j + 1 entries.
   - legacy_attempt_charge_uniform: when every channel has the same
     inspection charge v, that search charges (j + 1) (2 v + cmp).
   - legacy_attempt_trace_covered, legacy_trace_reserves_search_charge: each
     visit reserves before it reads, and the search reserves its charge.
   - recorded_attempt_trace_covered, recorded_attempt_charge: the recorded
     read reserves one word before it reads, and charges one word.
   - recorded_charge_le_legacy: when a word is at most the charge of one
     visit, the recorded read charges no more than the search.
   Negative controls:
   - read_before_reservation_is_not_covered: the coverage predicate rejects
     a read before its reservation.
   - legacy_position_search_charge_example: the search charge grows with the
     position of the entry; the recorded read does not.
   - small_channel_search_cheaper_than_word_example: without the premise of
     recorded_charge_le_legacy, a search at position 0 over channels with a
     small inspection charge costs less than one word.
   - pattern_index_names_another_channel_example: with repeated channels,
     the pattern's own index names another channel's entry.
   - non_reflexive_equality_breaks_recorded_positions: without reflexivity,
     the search does not find the position that the build recorded.

   No axiom, parameter or admission is used. Section variables are
   universally quantified when the sections close.

   Rust correspondence:
   rspace++/src/rspace/replay_rspace/native_candidate/metered.rs
   (channel_position, metered_channel_data, metered_match_data,
   POSITION_BYTES). *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia.
Import ListNotations.

(* A metered trace: reservations and the work that they pay for. *)
Inductive step := Reserve (amount : nat) | Read (amount : nat).

Fixpoint reserved (trace : list step) : nat :=
  match trace with
  | [] => 0
  | Reserve amount :: rest => amount + reserved rest
  | Read _ :: rest => reserved rest
  end.

(* [credit] is the reserved amount that no read has used yet. A trace is
   covered when every read fits in the credit before it. *)
Fixpoint covered_from (credit : nat) (trace : list step) : Prop :=
  match trace with
  | [] => True
  | Reserve amount :: rest => covered_from (credit + amount) rest
  | Read amount :: rest => amount <= credit /\ covered_from (credit - amount) rest
  end.

Definition covered (trace : list step) : Prop := covered_from 0 trace.

(* The recorded read: one reservation of a word, then the read of the word. *)
Definition recorded_trace (word : nat) : list step := [Reserve word; Read word].

Lemma reserved_app : forall left right,
  reserved (left ++ right) = reserved left + reserved right.
Proof.
  induction left as [| [amount | amount] rest IH]; intros right; simpl;
    [reflexivity | rewrite IH; lia | apply IH].
Qed.

Theorem recorded_attempt_trace_covered : forall word, covered (recorded_trace word).
Proof. intros word. unfold covered. simpl. split; [lia | exact I]. Qed.

Theorem recorded_attempt_charge : forall word, reserved (recorded_trace word) = word.
Proof. intros word. simpl. lia. Qed.

Theorem read_before_reservation_is_not_covered : ~ covered [Read 1; Reserve 1].
Proof. unfold covered. simpl. intros [Hread _]. lia. Qed.

Section Positions.
  Variable C : Type.
  Variable eqb : C -> C -> bool.

  Fixpoint first_equal (c : C) (entries : list C) : option nat :=
    match entries with
    | [] => None
    | e :: rest => if eqb c e then Some 0 else option_map S (first_equal c rest)
    end.

  (* The finished entries and one recorded position for each channel. *)
  Fixpoint build_from (entries : list C) (channels : list C) : list C * list nat :=
    match channels with
    | [] => (entries, [])
    | c :: rest =>
        match first_equal c entries with
        | Some j => (fst (build_from entries rest), j :: snd (build_from entries rest))
        | None =>
            (fst (build_from (entries ++ [c]) rest),
             length entries :: snd (build_from (entries ++ [c]) rest))
        end
    end.

  Definition build (channels : list C) : list C * list nat := build_from [] channels.

  Lemma build_from_positions_length : forall channels entries,
    length (snd (build_from entries channels)) = length channels.
  Proof.
    induction channels as [| c rest IH]; intros entries; simpl; [reflexivity |].
    destruct (first_equal c entries); simpl; rewrite IH; reflexivity.
  Qed.

  Theorem positions_length : forall channels,
    length (snd (build channels)) = length channels.
  Proof. intros channels. apply build_from_positions_length. Qed.

  Lemma build_from_extends : forall channels entries,
    exists suffix, fst (build_from entries channels) = entries ++ suffix.
  Proof.
    induction channels as [| c rest IH]; intros entries; simpl.
    - exists []. rewrite app_nil_r. reflexivity.
    - destruct (first_equal c entries); simpl.
      + apply IH.
      + destruct (IH (entries ++ [c])) as [suffix Hsuffix].
        exists (c :: suffix). rewrite Hsuffix, <- app_assoc. reflexivity.
  Qed.

  Lemma first_equal_app_found : forall c entries more j,
    first_equal c entries = Some j -> first_equal c (entries ++ more) = Some j.
  Proof.
    induction entries as [| e rest IH]; intros more j Hfound; simpl in *; [discriminate |].
    destruct (eqb c e); [exact Hfound |].
    destruct (first_equal c rest) as [k |] eqn:Hrest; simpl in Hfound; [| discriminate].
    injection Hfound as <-. rewrite (IH more k eq_refl). reflexivity.
  Qed.

  Lemma first_equal_app_missing : forall c entries more,
    first_equal c entries = None ->
    first_equal c (entries ++ more)
    = option_map (Nat.add (length entries)) (first_equal c more).
  Proof.
    induction entries as [| e rest IH]; intros more Hmissing; simpl in *.
    - destruct (first_equal c more); reflexivity.
    - destruct (eqb c e); [discriminate |].
      destruct (first_equal c rest) eqn:Hrest; [discriminate |].
      rewrite (IH more eq_refl). destruct (first_equal c more); reflexivity.
  Qed.

  Lemma first_equal_entry : forall c entries j,
    first_equal c entries = Some j ->
    exists e, nth_error entries j = Some e /\ eqb c e = true.
  Proof.
    induction entries as [| e rest IH]; intros j Hfound; simpl in Hfound; [discriminate |].
    destruct (eqb c e) eqn:Hequal.
    - injection Hfound as <-. exists e. split; [reflexivity | exact Hequal].
    - destruct (first_equal c rest) as [k |] eqn:Hrest; simpl in Hfound; [| discriminate].
      injection Hfound as <-. exact (IH k eq_refl).
  Qed.

  Lemma first_equal_missing_unequal : forall c entries,
    first_equal c entries = None ->
    forall j e, nth_error entries j = Some e -> eqb c e = false.
  Proof.
    induction entries as [| e rest IH]; intros Hmissing j x Hx.
    - destruct j; discriminate.
    - simpl in Hmissing. destruct (eqb c e) eqn:Hequal; [discriminate |].
      destruct (first_equal c rest) eqn:Hrest; [discriminate |].
      destruct j as [| j]; simpl in Hx.
      + injection Hx as <-. exact Hequal.
      + exact (IH eq_refl j x Hx).
  Qed.

  Section Reflexive.
    Hypothesis eqb_refl : forall c, eqb c c = true.

    Lemma found_stays_found : forall rest entries c j,
      first_equal c entries = Some j ->
      first_equal c (fst (build_from entries rest)) = Some j.
    Proof.
      intros rest entries c j Hfound.
      destruct (build_from_extends rest entries) as [suffix Hsuffix].
      rewrite Hsuffix. apply first_equal_app_found. exact Hfound.
    Qed.

    Lemma appended_is_found : forall rest entries c,
      first_equal c entries = None ->
      first_equal c (fst (build_from (entries ++ [c]) rest)) = Some (length entries).
    Proof.
      intros rest entries c Hmissing.
      destruct (build_from_extends rest (entries ++ [c])) as [suffix Hsuffix].
      rewrite Hsuffix, <- app_assoc, (first_equal_app_missing c entries ([c] ++ suffix) Hmissing).
      simpl. rewrite eqb_refl. simpl. rewrite Nat.add_0_r. reflexivity.
    Qed.

    Lemma build_from_records_first_equal : forall channels entries i c,
      nth_error channels i = Some c ->
      nth_error (snd (build_from entries channels)) i
      = first_equal c (fst (build_from entries channels)).
    Proof.
      induction channels as [| d rest IH]; intros entries i c Hc.
      - destruct i; discriminate.
      - simpl. destruct (first_equal d entries) as [j |] eqn:Hd; simpl.
        + destruct i as [| i]; simpl in Hc |- *.
          * injection Hc as <-. symmetry. apply found_stays_found. exact Hd.
          * apply IH. exact Hc.
        + destruct i as [| i]; simpl in Hc |- *.
          * injection Hc as <-. symmetry. apply appended_is_found. exact Hd.
          * apply IH. exact Hc.
    Qed.

    Theorem recorded_position_equals_first_equal_search : forall channels i c,
      nth_error channels i = Some c ->
      nth_error (snd (build channels)) i = first_equal c (fst (build channels)).
    Proof. intros channels i c Hc. apply build_from_records_first_equal. exact Hc. Qed.

    Lemma build_from_map_search : forall channels entries,
      map (fun c => first_equal c (fst (build_from entries channels))) channels
      = map Some (snd (build_from entries channels)).
    Proof.
      induction channels as [| c rest IH]; intros entries; simpl; [reflexivity |].
      destruct (first_equal c entries) as [j |] eqn:Hc; simpl; f_equal.
      - apply found_stays_found. exact Hc.
      - apply IH.
      - apply appended_is_found. exact Hc.
      - apply IH.
    Qed.

    Theorem first_equal_search_equals_recorded_positions : forall channels,
      map (fun c => first_equal c (fst (build channels))) channels
      = map Some (snd (build channels)).
    Proof. intros channels. apply build_from_map_search. Qed.

    Theorem recorded_position_names_equal_entry : forall channels i c j,
      nth_error channels i = Some c ->
      nth_error (snd (build channels)) i = Some j ->
      exists e, nth_error (fst (build channels)) j = Some e /\ eqb c e = true.
    Proof.
      intros channels i c j Hc Hj.
      rewrite (recorded_position_equals_first_equal_search channels i c Hc) in Hj.
      exact (first_equal_entry c (fst (build channels)) j Hj).
    Qed.

    Variable P : Type.

    (* The inputs of the selection: for each (channel, pattern) pair, the
       entry that the selection reads. *)
    Definition legacy_inputs (final channels : list C) (patterns : list P)
      : list (C * P * option nat) :=
      map (fun pair => (fst pair, snd pair, first_equal (fst pair) final))
        (combine channels patterns).

    Definition recorded_inputs (channels : list C) (positions : list nat) (patterns : list P)
      : list (C * P * option nat) :=
      map (fun triple => (fst (fst triple), snd (fst triple), Some (snd triple)))
        (combine (combine channels patterns) positions).

    Lemma inputs_equal_of_searches : forall final channels positions patterns,
      map (fun c => first_equal c final) channels = map Some positions ->
      legacy_inputs final channels patterns = recorded_inputs channels positions patterns.
    Proof.
      intros final channels.
      induction channels as [| c rest IH]; intros positions patterns Hsearch; [reflexivity |].
      destruct positions as [| j positions]; [discriminate |].
      simpl in Hsearch. injection Hsearch as Hhead Htail.
      destruct patterns as [| p patterns]; [reflexivity |].
      unfold legacy_inputs, recorded_inputs in *. simpl. rewrite Hhead. f_equal.
      apply IH. exact Htail.
    Qed.

    Theorem selection_inputs_equal : forall channels patterns,
      legacy_inputs (fst (build channels)) channels patterns
      = recorded_inputs channels (snd (build channels)) patterns.
    Proof.
      intros channels patterns. apply inputs_equal_of_searches.
      apply first_equal_search_equals_recorded_positions.
    Qed.
  End Reflexive.

  Definition pairwise_unequal (entries : list C) : Prop :=
    forall j k earlier later, j < k ->
      nth_error entries j = Some earlier -> nth_error entries k = Some later ->
      eqb later earlier = false.

  Lemma append_missing_keeps_unequal : forall entries c,
    pairwise_unequal entries -> first_equal c entries = None ->
    pairwise_unequal (entries ++ [c]).
  Proof.
    intros entries c Hunequal Hmissing j k earlier later Hjk Hj Hk.
    assert (Hjlen : j < length entries).
    { destruct (Nat.lt_ge_cases j (length entries)) as [Hlt | Hge]; [exact Hlt |].
      destruct (Nat.lt_ge_cases k (length entries)) as [Hklt | Hkge]; [lia |].
      rewrite nth_error_app2 in Hk by exact Hkge.
      destruct (k - length entries) as [| n] eqn:Hn; [lia |].
      simpl in Hk. destruct n; discriminate. }
    rewrite nth_error_app1 in Hj by exact Hjlen.
    destruct (Nat.lt_ge_cases k (length entries)) as [Hklt | Hkge].
    - rewrite nth_error_app1 in Hk by exact Hklt.
      exact (Hunequal j k earlier later Hjk Hj Hk).
    - rewrite nth_error_app2 in Hk by exact Hkge.
      destruct (k - length entries) as [| n]; simpl in Hk.
      + injection Hk as <-. exact (first_equal_missing_unequal c entries Hmissing j earlier Hj).
      + destruct n; discriminate.
  Qed.

  Lemma build_from_keeps_unequal : forall channels entries,
    pairwise_unequal entries -> pairwise_unequal (fst (build_from entries channels)).
  Proof.
    induction channels as [| c rest IH]; intros entries Hunequal; simpl; [exact Hunequal |].
    destruct (first_equal c entries) eqn:Hc; simpl.
    - apply IH. exact Hunequal.
    - apply IH. apply append_missing_keeps_unequal; assumption.
  Qed.

  Theorem entries_pairwise_unequal : forall channels,
    pairwise_unequal (fst (build channels)).
  Proof.
    intros channels. apply build_from_keeps_unequal.
    intros j k earlier later _ Hj. destruct j; discriminate.
  Qed.

  (* Charges, in one dimension of the meter. A visit of entry e by the search
     for channel c inspects both channels and compares them. *)
  Variable inspection : C -> nat.
  Variable comparison : nat.

  Fixpoint search_charge (c : C) (entries : list C) : nat :=
    match entries with
    | [] => 0
    | e :: rest =>
        inspection c + inspection e + comparison
        + (if eqb c e then 0 else search_charge c rest)
    end.

  Fixpoint search_visits (c : C) (entries : list C) : nat :=
    match entries with
    | [] => 0
    | e :: rest => S (if eqb c e then 0 else search_visits c rest)
    end.

  Fixpoint legacy_trace (c : C) (entries : list C) : list step :=
    match entries with
    | [] => []
    | e :: rest =>
        [Reserve (inspection c); Reserve (inspection e); Reserve comparison;
         Read (inspection c + inspection e + comparison)]
        ++ (if eqb c e then [] else legacy_trace c rest)
    end.

  Theorem legacy_search_visits_position_plus_one : forall c entries j,
    first_equal c entries = Some j -> search_visits c entries = S j.
  Proof.
    induction entries as [| e rest IH]; intros j Hfound; simpl in Hfound |- *; [discriminate |].
    destruct (eqb c e).
    - injection Hfound as <-. reflexivity.
    - destruct (first_equal c rest) as [k |] eqn:Hrest; simpl in Hfound; [| discriminate].
      injection Hfound as <-. rewrite (IH k eq_refl). reflexivity.
  Qed.

  Theorem legacy_attempt_charge_uniform : forall v c entries j,
    (forall x, inspection x = v) -> first_equal c entries = Some j ->
    search_charge c entries = S j * (2 * v + comparison).
  Proof.
    intros v c entries. induction entries as [| e rest IH]; intros j Hv Hfound;
      simpl in Hfound |- *; [discriminate |].
    rewrite !Hv. destruct (eqb c e).
    - injection Hfound as <-. lia.
    - destruct (first_equal c rest) as [k |] eqn:Hrest; simpl in Hfound; [| discriminate].
      injection Hfound as <-. rewrite (IH k Hv eq_refl). nia.
  Qed.

  Theorem legacy_trace_reserves_search_charge : forall c entries,
    reserved (legacy_trace c entries) = search_charge c entries.
  Proof.
    induction entries as [| e rest IH]; simpl; [reflexivity |].
    destruct (eqb c e); simpl; [lia |]. rewrite IH. lia.
  Qed.

  Lemma legacy_trace_covered_from : forall c entries credit,
    covered_from credit (legacy_trace c entries).
  Proof.
    induction entries as [| e rest IH]; intros credit; simpl; [exact I |].
    replace (credit + inspection c + inspection e + comparison
             - (inspection c + inspection e + comparison)) with credit by lia.
    split; [lia |]. destruct (eqb c e); [exact I | apply IH].
  Qed.

  Theorem legacy_attempt_trace_covered : forall c entries,
    covered (legacy_trace c entries).
  Proof. intros c entries. apply legacy_trace_covered_from. Qed.

  Theorem recorded_charge_le_legacy : forall word c entries j,
    first_equal c entries = Some j ->
    (forall e, word <= inspection c + inspection e + comparison) ->
    reserved (recorded_trace word) <= search_charge c entries.
  Proof.
    intros word c [| e rest] j Hfound Hword; simpl in Hfound; [discriminate |].
    rewrite recorded_attempt_charge. simpl. specialize (Hword e). lia.
  Qed.
End Positions.

(* Negative control: eight channels with an inspection charge of 100 each.
   The search for the entry at position 7 charges eight visits of 200, the
   search for position 0 one visit, and the recorded read one word. *)
Example legacy_position_search_charge_example :
  search_charge nat Nat.eqb (fun _ => 100) 0 7 [0; 1; 2; 3; 4; 5; 6; 7] = 1600
  /\ search_charge nat Nat.eqb (fun _ => 100) 0 0 [0; 1; 2; 3; 4; 5; 6; 7] = 200
  /\ reserved (recorded_trace 8) = 8.
Proof. repeat split; reflexivity. Qed.

(* Negative control: the premise of recorded_charge_le_legacy is needed. A
   channel whose inspection charges 1 byte makes a search at position 0
   charge 2 bytes, less than the 8-byte word. *)
Example small_channel_search_cheaper_than_word_example :
  search_charge nat Nat.eqb (fun _ => 1) 0 0 [0] = 2
  /\ search_charge nat Nat.eqb (fun _ => 1) 0 0 [0] < reserved (recorded_trace 8).
Proof. split; [reflexivity | simpl; lia]. Qed.

(* Negative control: with the channels [5; 5; 6], the pattern at index 1 has
   channel 5, but entry 1 holds channel 6. Its recorded position is 0. *)
Example pattern_index_names_another_channel_example :
  build nat Nat.eqb [5; 5; 6] = ([5; 6], [0; 0; 1])
  /\ nth_error (fst (build nat Nat.eqb [5; 5; 6])) 1 = Some 6.
Proof. split; reflexivity. Qed.

(* Negative control: an equality that is never true is not reflexive. The
   build appends the channel and records position 0, but the search finds
   no entry. *)
Theorem non_reflexive_equality_breaks_recorded_positions :
  nth_error (snd (build bool (fun _ _ => false) [true])) 0 = Some 0
  /\ first_equal bool (fun _ _ => false) true (fst (build bool (fun _ _ => false) [true])) = None.
Proof. split; reflexivity. Qed.
