From Stdlib Require Import Bool Arith PeanoNat List Permutation.
From StackSafePDA Require Import RSpaceEagerReference.
Import ListNotations RSpaceEagerReference.RSpaceEagerReference.

(**
  Nonvacuity witnesses and negative controls for the eager tuple-space
  reference of [RSpaceEagerReference.v].

  The instance.  Channels, values, bindings, bodies and roots are natural
  numbers.  A pattern is [None], which accepts every value, or [Some v], which
  accepts [v] only; a match binds the value.  The guard of every continuation
  accepts every list of bindings.  The root of a contents is its number of
  resting items.  The channels [c], [a] and [b] are 0, 1 and 2.  The comments
  write a datum as [c!(1)] and a continuation as [for(@1 <- a)] or
  [for(@y <- c)], as Rholang would.

  The enabled set of the instance is decided.  [enabled_check] decides the
  positions, the part the arriving datum takes and the availability of the
  data; [candidates] lists every rendezvous whose continuation is available
  and whose data are drawn from the readable pool.  Every member of the
  enabled set is a listed candidate that passes the check, and every listed
  candidate that passes the check is a member.  So [enumeration_choice], the
  first listed candidate that passes the check, picks a member of every
  non-empty enabled set, and an empty enabled set is shown by computation
  ([enabled_set_is_empty_by_enumeration]).

  Witnesses (the premises of each theorem hold):
  - [enumeration_choice_chooses_members]: the premise of
    [selector_resolution_is_refinement] holds for [enumeration_choice];
  - [reference_is_branching_bisimilar_where_two_candidates_wait]: the
    reference meets O1 to O4 from empty contents and reaches the state where
    [c!(1)] and [c!(2)] rest, which [obligations_give_branching_bisimulation]
    makes branching bisimilar to itself;
  - [enumeration_selector_is_a_refinement]:
    [selector_resolution_is_refinement] applies to [enumeration_choice],
    which fires [c!(1)] for an arriving [for(@y <- c)] where [c!(1)] and
    [c!(2)] rest;
  - [two_candidates_separate_the_selector_from_the_reference]: both data give
    a member of the enabled set of that operation, so
    [selector_resolution_is_not_bisimilar] applies to [enumeration_choice];
  - [replay_follows_the_member_the_selector_skips]: replay of a log that names
    the COMM with [c!(2)] follows it although the selector takes [c!(1)]
    ([replay_follows_any_enabled_choice]);
  - [replay_performs_the_logged_event_in_either_position_order]: for an
    arriving [for(@x <- c & @y <- c)], a log that names the data in the order
    [c!(2)], [c!(1)] is replayed by the selection [c!(1)], [c!(2)], and the
    reference step carries the logged label;
  - [replay_refuses_a_value_no_datum_supplies]: a log that names a COMM with
    [c!(3)] is refused, and the reference has no step with its label
    ([replay_refuses_a_non_enabled_choice]);
  - [reached_rest_states_hold_no_enabled_rendezvous]: the reference reaches
    from empty contents the state where [c!(1)] and [for(@2 <- c)] rest, and
    both parts of [rest_states_have_no_enabled_rendezvous] apply to it;
  - [install_rests_only_where_nothing_matches]: at empty contents the install
    of [for(@y <= c)], persistent and catch-all like a system process, has an
    empty enabled set and rests; where [c!(1)] rests its enabled set has a
    member and the reference has no step for it
    ([install_rests_exactly_when_nothing_is_enabled]).

  Negative controls.  Each proves that the conclusion of the designated
  theorem fails for the mutant, that the premises the mutant meets hold (O1
  wherever the theorem asks for it, O4 for the sorting reader, O3 and O4 for
  the implementation that fires an install), and that the premise named below
  fails:
  - [pattern_blind_selector_fires_a_non_member]
    ([selector_resolution_is_refinement], premise [chooses_members]): a
    choice function that takes the first resting data, one per position,
    without matching, fires [c!(1)] for an arriving [for(@2 <- c)]; a step of
    its selector is no reference step, and the reference does not simulate
    it;
  - [careless_rest_holds_an_enabled_rendezvous]
    ([rest_states_have_no_enabled_rendezvous], premise O2): the reference
    without the emptiness premise of a rest starts at empty contents; after
    [c!(1)] and [for(@1 <- c)] rest, its state holds an enabled rendezvous
    among resting items;
  - [stale_reset_is_not_bisimilar_to_its_abstraction]
    ([obligations_give_branching_bisimulation], premise O4): a reset that
    keeps the current contents instead of the recorded ones; after [a!(1)]
    rests, a checkpoint records it and [for(@1 <- a)] consumes it, the
    reference resets and fires that COMM again, which the mutant cannot;
  - [sorting_reader_is_not_bisimilar_to_its_abstraction]
    ([obligations_give_branching_bisimulation], premise O2): the reference's
    own steps read back through a contents function that sorts the channel
    list of each continuation and leaves its patterns in place; where
    [for(@1 <- b & @2 <- a)] and [b!(2)] rest, the implementation rests an
    arriving [a!(1)], while the continuation it reads back,
    [for(@1 <- a & @2 <- b)], fires with [a!(1)] and [b!(2)].  Its resets
    read back the recorded contents, so it meets O4;
  - [install_firing_is_not_bisimilar_to_its_abstraction]
    ([obligations_give_branching_bisimulation], premise O2): an
    implementation that takes every reference step, so that it meets O3 and
    O4, and besides fires an install as the consume of the same continuation
    would; where [c!(1)] rests it fires the install of [for(@y <= c)] with
    [c!(1)], which no reference step does ([install_never_fires]), so the
    reference cannot answer that step.
*)

Module RSpaceEagerReferenceMutants.

(** * The instance *)

Notation item_datum := (@datum nat nat).
Notation item_continuation := (@continuation nat (option nat) unit nat).
Notation store_contents := (@contents nat nat (option nat) unit nat).
Notation store_configuration := (@configuration nat nat (option nat) unit nat nat).
Notation store_operation := (@operation nat nat (option nat) unit nat).
Notation store_rendezvous := (@rendezvous nat nat (option nat) unit nat).
Notation store_result := (@result nat nat (option nat) unit nat).
Notation store_label := (@label nat nat (option nat) unit nat nat).
Notation store_implementation := (@implementation nat nat (option nat) unit nat nat).

(** [pattern_accepts pattern value]: [None] accepts every value and [Some v]
    accepts [v] only. *)
Definition pattern_accepts (pattern : option nat) (value : nat) : bool :=
  match pattern with
  | None => true
  | Some expected => Nat.eqb expected value
  end.

(** A pattern that accepts a value binds the value. *)
Definition concrete_matches (pattern : option nat) (value binding : nat) : Prop :=
  binding = value /\ pattern_accepts pattern value = true.

Definition concrete_guard_accepts (guard : unit) (bindings : list nat) : Prop := True.

(** The root of a contents is its number of resting items. *)
Definition concrete_root_of (contents_now : store_contents) : nat :=
  length (resting_data contents_now) + length (resting_continuations contents_now).

Notation concrete_enabled := (enabled concrete_matches concrete_guard_accepts).
Notation concrete_ref_step :=
  (ref_step concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec).
Notation concrete_action_step :=
  (ref_action_step concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec).
Notation concrete_ref_reachable :=
  (ref_reachable concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec).
Notation concrete_replay_step := (replay_step concrete_matches concrete_guard_accepts).
Notation concrete_reference :=
  (reference_implementation concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec).
Notation concrete_selector choose :=
  (selector_implementation concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
     choose).
Notation concrete_initial := (@initial_configuration nat nat (option nat) unit nat nat).

(** * Deciding the enabled set *)

Definition datum_eq_dec (one other : item_datum) : {one = other} + {one <> other}.
Proof.
  destruct one as [channel_one value_one persistent_one].
  destruct other as [channel_two value_two persistent_two].
  destruct (Nat.eq_dec channel_one channel_two) as [same_channel|different_channel].
  - destruct (Nat.eq_dec value_one value_two) as [same_value|different_value].
    + destruct (bool_dec persistent_one persistent_two) as [same_flag|different_flag].
      * left. rewrite same_channel, same_value, same_flag. reflexivity.
      * right. intros same. apply different_flag. exact (f_equal datum_persistent same).
    + right. intros same. apply different_value. exact (f_equal datum_value same).
  - right. intros same. apply different_channel. exact (f_equal datum_channel same).
Defined.

(** [member wanted pool] decides [In wanted pool]. *)
Fixpoint member (wanted : item_datum) (pool : list item_datum) : bool :=
  match pool with
  | [] => false
  | head :: tail => if datum_eq_dec wanted head then true else member wanted tail
  end.

Lemma member_spec :
  forall (wanted : item_datum) (pool : list item_datum),
    member wanted pool = true <-> In wanted pool.
Proof.
  intros wanted pool.
  induction pool as [|head tail induction_hypothesis].
  - split; [intros absent; discriminate absent|intros inside; destruct inside].
  - change (member wanted (head :: tail)) with
      (if datum_eq_dec wanted head then true else member wanted tail).
    destruct (datum_eq_dec wanted head) as [same|different].
    + split; [intros _; left; exact (eq_sym same)|intros _; reflexivity].
    + split.
      * intros found. right. exact (proj1 induction_hypothesis found).
      * intros [same|deeper].
        -- destruct (different (eq_sym same)).
        -- exact (proj2 induction_hypothesis deeper).
Qed.

(** [remove_one removed pool] removes one occurrence of [removed]. *)
Fixpoint remove_one (removed : item_datum) (pool : list item_datum)
    : option (list item_datum) :=
  match pool with
  | [] => None
  | head :: tail =>
      if datum_eq_dec removed head then Some tail
      else
        match remove_one removed tail with
        | Some remaining => Some (head :: remaining)
        | None => None
        end
  end.

Lemma remove_one_found :
  forall (removed : item_datum) (pool remaining : list item_datum),
    remove_one removed pool = Some remaining -> Permutation pool (removed :: remaining).
Proof.
  intros removed pool.
  induction pool as [|head tail induction_hypothesis]; intros remaining removal.
  - simpl in removal. discriminate removal.
  - change (remove_one removed (head :: tail)) with
      (if datum_eq_dec removed head then Some tail
       else
         match remove_one removed tail with
         | Some tail_remaining => Some (head :: tail_remaining)
         | None => None
         end) in removal.
    revert removal.
    destruct (datum_eq_dec removed head) as [same|different]; intros removal.
    + injection removal as same_remaining.
      rewrite <- same_remaining, same.
      apply Permutation_refl.
    + revert removal. revert induction_hypothesis.
      destruct (remove_one removed tail) as [tail_remaining|];
        intros induction_hypothesis removal.
      * injection removal as same_remaining.
        rewrite <- same_remaining.
        exact (Permutation_trans
          (perm_skip head (induction_hypothesis tail_remaining eq_refl))
          (perm_swap removed head tail_remaining)).
      * discriminate removal.
Qed.

Lemma remove_one_missing :
  forall (removed : item_datum) (pool : list item_datum),
    remove_one removed pool = None -> ~ In removed pool.
Proof.
  intros removed pool.
  induction pool as [|head tail induction_hypothesis]; intros removal inside.
  - exact inside.
  - change (remove_one removed (head :: tail)) with
      (if datum_eq_dec removed head then Some tail
       else
         match remove_one removed tail with
         | Some tail_remaining => Some (head :: tail_remaining)
         | None => None
         end) in removal.
    revert removal.
    destruct (datum_eq_dec removed head) as [same|different]; intros removal.
    + discriminate removal.
    + revert removal. revert induction_hypothesis.
      destruct (remove_one removed tail) as [tail_remaining|];
        intros induction_hypothesis removal.
      * discriminate removal.
      * destruct inside as [same|deeper].
        -- exact (different (eq_sym same)).
        -- exact (induction_hypothesis eq_refl deeper).
Qed.

(** [included needed pool] decides that [needed] is a sub-multiset of
    [pool]. *)
Fixpoint included (needed pool : list item_datum) : bool :=
  match needed with
  | [] => true
  | wanted :: others =>
      match remove_one wanted pool with
      | Some remaining => included others remaining
      | None => false
      end
  end.

Lemma included_sound :
  forall needed pool : list item_datum,
    included needed pool = true -> exists remainder, Permutation pool (needed ++ remainder).
Proof.
  intros needed.
  induction needed as [|wanted others induction_hypothesis]; intros pool found.
  - exists pool. exact (Permutation_refl pool).
  - change (included (wanted :: others) pool) with
      (match remove_one wanted pool with
       | Some remaining => included others remaining
       | None => false
       end) in found.
    revert found.
    destruct (remove_one wanted pool) as [remaining|] eqn:removal; intros found.
    + destruct (induction_hypothesis remaining found) as [remainder split_remaining].
      exists remainder.
      exact (Permutation_trans (remove_one_found wanted pool remaining removal)
        (perm_skip wanted split_remaining)).
    + discriminate found.
Qed.

Lemma included_complete :
  forall needed pool remainder : list item_datum,
    Permutation pool (needed ++ remainder) -> included needed pool = true.
Proof.
  intros needed.
  induction needed as [|wanted others induction_hypothesis]; intros pool remainder split_pool.
  - reflexivity.
  - change (included (wanted :: others) pool) with
      (match remove_one wanted pool with
       | Some remaining => included others remaining
       | None => false
       end).
    destruct (remove_one wanted pool) as [remaining|] eqn:removal.
    + exact (induction_hypothesis remaining remainder
        (Permutation_cons_inv
           (Permutation_trans (Permutation_sym (remove_one_found wanted pool remaining removal))
              split_pool))).
    + exfalso.
      exact (remove_one_missing wanted pool removal
        (Permutation_in wanted (Permutation_sym split_pool) (in_eq wanted (others ++ remainder)))).
Qed.

(** [positions_ok channels patterns selection] decides that the selection
    has one datum per position, on the position's channel, whose value the
    position's pattern accepts. *)
Fixpoint positions_ok (channels : list nat) (patterns : list (option nat))
    (selection : list item_datum) : bool :=
  match channels, patterns, selection with
  | [], [], [] => true
  | channel :: other_channels, pattern :: other_patterns, selected :: others =>
      Nat.eqb (datum_channel selected) channel && pattern_accepts pattern (datum_value selected) &&
      positions_ok other_channels other_patterns others
  | _, _, _ => false
  end.

Lemma positions_ok_sound :
  forall (channels : list nat) (patterns : list (option nat)) (selection : list item_datum),
    positions_ok channels patterns selection = true ->
    exists bindings, positions_match concrete_matches channels patterns selection bindings.
Proof.
  intros channels.
  induction channels as [|channel other_channels induction_hypothesis];
    intros patterns selection checked.
  - destruct patterns as [|pattern other_patterns]; [|discriminate checked].
    destruct selection as [|selected others]; [|discriminate checked].
    exists []. apply positions_match_nil.
  - destruct patterns as [|pattern other_patterns]; [discriminate checked|].
    destruct selection as [|selected others]; [discriminate checked|].
    change (positions_ok (channel :: other_channels) (pattern :: other_patterns)
              (selected :: others))
      with (Nat.eqb (datum_channel selected) channel &&
            pattern_accepts pattern (datum_value selected) &&
            positions_ok other_channels other_patterns others) in checked.
    destruct (andb_prop _ _ checked) as [head_checked rest_checked].
    destruct (andb_prop _ _ head_checked) as [channel_checked pattern_checked].
    destruct (induction_hypothesis other_patterns others rest_checked) as [bindings matched].
    exists (datum_value selected :: bindings).
    apply positions_match_cons.
    + exact (proj1 (Nat.eqb_eq _ _) channel_checked).
    + split; [reflexivity|exact pattern_checked].
    + exact matched.
Qed.

Lemma positions_ok_complete :
  forall (channels : list nat) (patterns : list (option nat)) (selection : list item_datum)
      (bindings : list nat),
    positions_match concrete_matches channels patterns selection bindings ->
    positions_ok channels patterns selection = true.
Proof.
  intros channels patterns selection bindings matched.
  induction matched as
    [|channel pattern selected binding other_channels other_patterns others other_bindings
      same_channel matched_head _ induction_hypothesis].
  - reflexivity.
  - destruct matched_head as [_ pattern_checked].
    change (positions_ok (channel :: other_channels) (pattern :: other_patterns)
              (selected :: others))
      with (Nat.eqb (datum_channel selected) channel &&
            pattern_accepts pattern (datum_value selected) &&
            positions_ok other_channels other_patterns others).
    rewrite same_channel, Nat.eqb_refl, pattern_checked, induction_hypothesis.
    reflexivity.
Qed.

Lemma positions_match_length :
  forall (channels : list nat) (patterns : list (option nat)) (selection : list item_datum)
      (bindings : list nat),
    positions_match concrete_matches channels patterns selection bindings ->
    length selection = length channels.
Proof.
  intros channels patterns selection bindings matched.
  induction matched as
    [|channel pattern selected binding other_channels other_patterns others other_bindings
      _ _ _ induction_hypothesis].
  - reflexivity.
  - simpl. rewrite induction_hypothesis. reflexivity.
Qed.

(** The arriving datum takes part in the selection of a produce. *)
Definition participation_ok (op : store_operation) (selection : list item_datum) : bool :=
  match op with
  | Produce arriving => member arriving selection
  | Consume _ => true
  | Install _ => true
  end.

(** Every persistent datum read lies in the readable pool. *)
Definition reads_ok (readable selection : list item_datum) : bool :=
  forallb (fun selected => member selected readable) (read_persistent_data selection).

(** [enabled_check contents_now op candidate] decides every condition of
    [enabled] except the availability of the continuation. *)
Definition enabled_check (contents_now : store_contents) (op : store_operation)
    (candidate : store_rendezvous) : bool :=
  positions_ok (continuation_channels (rendezvous_continuation candidate))
    (continuation_patterns (rendezvous_continuation candidate)) (rendezvous_data candidate) &&
  participation_ok op (rendezvous_data candidate) &&
  included (consumed_data (rendezvous_data candidate))
    (arriving_consumable op ++ resting_data contents_now) &&
  reads_ok (arriving_readable op ++ resting_data contents_now) (rendezvous_data candidate).

Lemma admissible_by_computation :
  forall (waiting : item_continuation) (selection : list item_datum),
    positions_ok (continuation_channels waiting) (continuation_patterns waiting) selection = true ->
    admissible concrete_matches concrete_guard_accepts waiting selection.
Proof.
  intros waiting selection checked.
  destruct (positions_ok_sound _ _ _ checked) as [bindings matched].
  exists bindings.
  split; [exact matched|exact I].
Qed.

Lemma data_available_by_computation :
  forall consumable readable selection : list item_datum,
    included (consumed_data selection) consumable = true ->
    reads_ok readable selection = true ->
    data_available consumable readable selection.
Proof.
  intros consumable readable selection consumed_checked reads_checked.
  split.
  - exact (included_sound _ _ consumed_checked).
  - intros selected persistent_read.
    unfold reads_ok in reads_checked.
    exact (proj1 (member_spec _ _)
      (proj1 (forallb_forall _ _) reads_checked selected persistent_read)).
Qed.

Lemma enabled_check_complete :
  forall (contents_now : store_contents) (op : store_operation) (candidate : store_rendezvous),
    concrete_enabled contents_now op candidate -> enabled_check contents_now op candidate = true.
Proof.
  intros contents_now op candidate
    [_ [[bindings [matched _]] [participating [[remainder consumable_split] readable_cover]]]].
  assert (participation_true : participation_ok op (rendezvous_data candidate) = true).
  { destruct op as [arriving|arriving|arriving]; simpl in participating |- *.
    - exact (proj2 (member_spec arriving (rendezvous_data candidate)) participating).
    - reflexivity.
    - reflexivity. }
  assert (reads_true :
            reads_ok (arriving_readable op ++ resting_data contents_now)
              (rendezvous_data candidate) = true).
  { unfold reads_ok.
    apply (proj2 (forallb_forall _ _)).
    intros selected persistent_read.
    exact (proj2 (member_spec _ _) (readable_cover selected persistent_read)). }
  unfold enabled_check.
  rewrite (positions_ok_complete _ _ _ _ matched).
  rewrite (included_complete _ _ _ consumable_split).
  rewrite participation_true, reads_true.
  reflexivity.
Qed.

Lemma enabled_check_sound :
  forall (contents_now : store_contents) (op : store_operation) (candidate : store_rendezvous),
    continuation_available contents_now op (rendezvous_continuation candidate) ->
    enabled_check contents_now op candidate = true ->
    concrete_enabled contents_now op candidate.
Proof.
  intros contents_now op candidate available checked.
  unfold enabled_check in checked.
  destruct (andb_prop _ _ checked) as [checked_first reads_checked].
  destruct (andb_prop _ _ checked_first) as [checked_second included_checked].
  destruct (andb_prop _ _ checked_second) as [positions_checked participation_checked].
  split; [exact available|].
  split; [exact (admissible_by_computation _ _ positions_checked)|].
  split.
  - destruct op as [arriving|arriving|arriving]; simpl in participation_checked |- *.
    + exact (proj1 (member_spec arriving (rendezvous_data candidate)) participation_checked).
    + exact I.
    + exact I.
  - exact (data_available_by_computation _ _ _ included_checked reads_checked).
Qed.

(** [selections pool width] lists every list of [width] data drawn from
    [pool]. *)
Fixpoint selections (pool : list item_datum) (width : nat) : list (list item_datum) :=
  match width with
  | 0 => [[]]
  | S narrower => flat_map (fun selected => map (cons selected) (selections pool narrower)) pool
  end.

Lemma selections_complete :
  forall pool selection : list item_datum,
    (forall selected, In selected selection -> In selected pool) ->
    In selection (selections pool (length selection)).
Proof.
  intros pool selection.
  induction selection as [|selected others induction_hypothesis]; intros drawn.
  - left. reflexivity.
  - change (selections pool (length (selected :: others))) with
      (flat_map (fun chosen => map (cons chosen) (selections pool (length others))) pool).
    apply (proj2 (in_flat_map _ _ _)).
    exists selected.
    split; [exact (drawn selected (in_eq selected others))|].
    exact (in_map (cons selected) (selections pool (length others)) others
      (induction_hypothesis
         (fun other inside => drawn other (in_cons selected other others inside)))).
Qed.

Lemma arriving_consumable_is_readable :
  forall (op : store_operation) (selected : item_datum),
    In selected (arriving_consumable op) -> In selected (arriving_readable op).
Proof.
  intros [arriving|arriving|arriving] selected; simpl.
  - destruct (datum_persistent arriving); intros inside.
    + destruct inside.
    + exact inside.
  - intros inside. destruct inside.
  - intros inside. destruct inside.
Qed.

(** Every datum of a member of the enabled set lies in the readable pool. *)
Lemma enabled_data_are_drawn :
  forall (contents_now : store_contents) (op : store_operation) (candidate : store_rendezvous)
      (selected : item_datum),
    concrete_enabled contents_now op candidate ->
    In selected (rendezvous_data candidate) ->
    In selected (arriving_readable op ++ resting_data contents_now).
Proof.
  intros contents_now op candidate selected
    [_ [_ [_ [[remainder consumable_split] readable_cover]]]] inside.
  destruct (datum_persistent selected) eqn:persistent_flag.
  - apply readable_cover.
    apply (proj2 (read_persistent_data_spec (rendezvous_data candidate) selected)).
    split; [exact inside|exact persistent_flag].
  - assert (consumable_member :
              In selected (arriving_consumable op ++ resting_data contents_now)).
    { apply (Permutation_in selected (Permutation_sym consumable_split)).
      apply in_or_app. left.
      apply (proj2 (consumed_data_spec (rendezvous_data candidate) selected)).
      split; [exact inside|exact persistent_flag]. }
    destruct (in_app_or _ _ _ consumable_member) as [arriving_member|resting_member].
    + apply in_or_app. left.
      exact (arriving_consumable_is_readable op selected arriving_member).
    + apply in_or_app. right. exact resting_member.
Qed.

(** The continuations a member of the enabled set can have. *)
Definition candidate_continuations (contents_now : store_contents) (op : store_operation)
    : list item_continuation :=
  match op with
  | Produce _ => resting_continuations contents_now
  | Consume arriving => [arriving]
  | Install arriving => [arriving]
  end.

Definition candidates (contents_now : store_contents) (op : store_operation)
    : list store_rendezvous :=
  flat_map
    (fun waiting =>
       map (Rendezvous waiting)
         (selections (arriving_readable op ++ resting_data contents_now)
            (length (continuation_channels waiting))))
    (candidate_continuations contents_now op).

Lemma candidates_complete :
  forall (contents_now : store_contents) (op : store_operation) (candidate : store_rendezvous),
    concrete_enabled contents_now op candidate -> In candidate (candidates contents_now op).
Proof.
  intros contents_now op [waiting selection] member_now.
  unfold candidates.
  apply (proj2 (in_flat_map _ _ _)).
  exists waiting.
  split.
  - destruct member_now as [available _].
    destruct op as [arriving|arriving|arriving]; simpl in available |- *.
    + exact available.
    + left. exact (eq_sym available).
    + left. exact (eq_sym available).
  - assert (same_width : length selection = length (continuation_channels waiting)).
    { destruct member_now as [_ [[bindings [matched _]] _]].
      exact (positions_match_length _ _ _ _ matched). }
    assert (drawn : In selection
                      (selections (arriving_readable op ++ resting_data contents_now)
                         (length (continuation_channels waiting)))).
    { rewrite <- same_width.
      apply selections_complete.
      intros selected inside.
      exact (enabled_data_are_drawn contents_now op (Rendezvous waiting selection) selected
        member_now inside). }
    exact (in_map (Rendezvous waiting) _ selection drawn).
Qed.

Lemma candidates_are_available :
  forall (contents_now : store_contents) (op : store_operation) (candidate : store_rendezvous),
    In candidate (candidates contents_now op) ->
    continuation_available contents_now op (rendezvous_continuation candidate).
Proof.
  intros contents_now op candidate listed.
  unfold candidates in listed.
  destruct (proj1 (in_flat_map _ _ _) listed) as [waiting [waiting_listed candidate_listed]].
  destruct (proj1 (in_map_iff _ _ _) candidate_listed) as [selection [built _]].
  rewrite <- built.
  destruct op as [arriving|arriving|arriving]; simpl in waiting_listed |- *.
  - exact waiting_listed.
  - destruct waiting_listed as [same|nothing]; [exact (eq_sym same)|destruct nothing].
  - destruct waiting_listed as [same|nothing]; [exact (eq_sym same)|destruct nothing].
Qed.

Lemma enabled_set_is_empty_by_enumeration :
  forall (contents_now : store_contents) (op : store_operation),
    existsb (enabled_check contents_now op) (candidates contents_now op) = false ->
    forall candidate, ~ concrete_enabled contents_now op candidate.
Proof.
  intros contents_now op nothing_found candidate member_now.
  assert (found : existsb (enabled_check contents_now op) (candidates contents_now op) = true).
  { apply (proj2 (existsb_exists (enabled_check contents_now op) (candidates contents_now op))).
    exists candidate.
    split; [exact (candidates_complete contents_now op candidate member_now)
           |exact (enabled_check_complete contents_now op candidate member_now)]. }
  rewrite nothing_found in found.
  discriminate found.
Qed.

(** * A choice function that chooses members *)

Definition fallback_rendezvous : store_rendezvous :=
  Rendezvous (Continuation [] [] tt 0 false []) [].

(** [enumeration_choice] picks the first listed candidate that passes the
    check. *)
Definition enumeration_choice (contents_now : store_contents) (op : store_operation)
    : store_rendezvous :=
  match find (enabled_check contents_now op) (candidates contents_now op) with
  | Some found => found
  | None => fallback_rendezvous
  end.

Lemma enumeration_choice_chooses_members :
  chooses_members concrete_matches concrete_guard_accepts enumeration_choice.
Proof.
  intros contents_now op candidate member_now.
  unfold enumeration_choice.
  destruct (find (enabled_check contents_now op) (candidates contents_now op))
    as [found|] eqn:search.
  - destruct (find_some _ _ search) as [found_listed found_checked].
    exact (enabled_check_sound contents_now op found
      (candidates_are_available contents_now op found found_listed) found_checked).
  - pose proof (find_none _ _ search candidate
      (candidates_complete contents_now op candidate member_now)) as unchecked.
    rewrite (enabled_check_complete contents_now op candidate member_now) in unchecked.
    discriminate unchecked.
Qed.

(** * Items, contents and configurations of the examples *)

Definition channel_c : nat := 0.
Definition channel_a : nat := 1.
Definition channel_b : nat := 2.

Definition datum_c1 : item_datum := Datum channel_c 1 false.
Definition datum_c2 : item_datum := Datum channel_c 2 false.
Definition datum_c3 : item_datum := Datum channel_c 3 false.
Definition datum_a1 : item_datum := Datum channel_a 1 false.
Definition datum_b2 : item_datum := Datum channel_b 2 false.

(** [for(@y <- c)], [for(@1 <- c)], [for(@2 <- c)] and [for(@1 <- a)]. *)
Definition receive_any (channel : nat) : item_continuation :=
  Continuation [channel] [None] tt 0 false [].
Definition receive_value (channel value : nat) : item_continuation :=
  Continuation [channel] [Some value] tt 0 false [].
Definition receive_c1 : item_continuation := receive_value channel_c 1.
Definition receive_c2 : item_continuation := receive_value channel_c 2.
Definition receive_a1 : item_continuation := receive_value channel_a 1.

(** [for(@x <- c & @y <- c)] and [for(@1 <- b & @2 <- a)]. *)
Definition receive_both_of_c : item_continuation :=
  Continuation [channel_c; channel_c] [None; None] tt 0 false [].
Definition cross_join : item_continuation :=
  Continuation [channel_b; channel_a] [Some 1; Some 2] tt 0 false [].

(** [for(@y <= c)]: persistent and catch-all, the shape of a system process
    that the runtime installs. *)
Definition system_process : item_continuation := Continuation [channel_c] [None] tt 0 true [].

Definition empty_history : nat -> option store_contents := fun _ => None.

(** [c!(2)] rests; then [c!(1)] rests too. *)
Definition c2_resting : store_configuration :=
  Configuration (Contents [datum_c2] []) empty_history.
Definition two_candidate_contents : store_contents := Contents [datum_c1; datum_c2] [].
Definition two_candidate_configuration : store_configuration :=
  Configuration two_candidate_contents empty_history.

(** [for(@y <- c)] arrives where [c!(1)] and [c!(2)] rest. *)
Definition two_candidate_operation : store_operation := Consume (receive_any channel_c).
Definition member_c1 : store_rendezvous := Rendezvous (receive_any channel_c) [datum_c1].
Definition member_c2 : store_rendezvous := Rendezvous (receive_any channel_c) [datum_c2].
Definition unsupplied_comm : store_rendezvous := Rendezvous (receive_any channel_c) [datum_c3].

(** [for(@x <- c & @y <- c)] arrives where [c!(1)] and [c!(2)] rest. *)
Definition both_of_c_operation : store_operation := Consume receive_both_of_c.
Definition performed_in_bind_order : store_rendezvous :=
  Rendezvous receive_both_of_c [datum_c1; datum_c2].
Definition logged_in_other_order : store_rendezvous :=
  Rendezvous receive_both_of_c [datum_c2; datum_c1].
Definition emptied_configuration : store_configuration :=
  Configuration (Contents [] []) empty_history.

(** [c!(1)] rests; then [for(@2 <- c)] rests beside it. *)
Definition c1_resting : store_configuration :=
  Configuration (Contents [datum_c1] []) empty_history.
Definition mismatched_configuration : store_configuration :=
  Configuration (Contents [datum_c1] [receive_c2]) empty_history.

(** [for(@y <= c)] is installed where nothing rests; where [c!(1)] rests, it
    would meet [c!(1)]. *)
Definition system_process_waiting : store_configuration :=
  Configuration (Contents [] [system_process]) empty_history.
Definition system_process_comm : store_rendezvous := Rendezvous system_process [datum_c1].

(** [a!(1)] rests; a checkpoint records it under [a1_root]; [for(@1 <- a)]
    consumes it. *)
Definition a1_root : nat := concrete_root_of (Contents [datum_a1] []).
Definition a1_history : nat -> option store_contents :=
  record_root Nat.eq_dec empty_history a1_root (Contents [datum_a1] []).
Definition a1_resting : store_configuration :=
  Configuration (Contents [datum_a1] []) empty_history.
Definition a1_recorded : store_configuration :=
  Configuration (Contents [datum_a1] []) a1_history.
Definition a1_consumed : store_configuration := Configuration (Contents [] []) a1_history.
Definition a1_operation : store_operation := Consume receive_a1.
Definition a1_comm : store_rendezvous := Rendezvous receive_a1 [datum_a1].

(** [for(@1 <- b & @2 <- a)] rests; then [b!(2)] rests; then [a!(1)]
    rests. *)
Definition cross_join_waiting : store_configuration :=
  Configuration (Contents [] [cross_join]) empty_history.
Definition cross_join_configuration : store_configuration :=
  Configuration (Contents [datum_b2] [cross_join]) empty_history.
Definition a1_beside_cross_join : store_configuration :=
  Configuration (Contents [datum_a1; datum_b2] [cross_join]) empty_history.

(** * Reference steps of the examples *)

Lemma initial_rests_datum_c2 :
  concrete_ref_step concrete_initial (OpResult (Produce datum_c2) Rested) c2_resting.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    concrete_initial (Produce datum_c2) c2_resting
    (enabled_set_is_empty_by_enumeration (current concrete_initial) (Produce datum_c2) eq_refl)
    (contents_equiv_refl (current c2_resting)) (recorded_equiv_refl empty_history)).
Qed.

Lemma c2_resting_rests_datum_c1 :
  concrete_ref_step c2_resting (OpResult (Produce datum_c1) Rested) two_candidate_configuration.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    c2_resting (Produce datum_c1) two_candidate_configuration
    (enabled_set_is_empty_by_enumeration (current c2_resting) (Produce datum_c1) eq_refl)
    (contents_equiv_refl (current two_candidate_configuration))
    (recorded_equiv_refl empty_history)).
Qed.

Lemma initial_rests_datum_c1 :
  concrete_ref_step concrete_initial (OpResult (Produce datum_c1) Rested) c1_resting.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    concrete_initial (Produce datum_c1) c1_resting
    (enabled_set_is_empty_by_enumeration (current concrete_initial) (Produce datum_c1) eq_refl)
    (contents_equiv_refl (current c1_resting)) (recorded_equiv_refl empty_history)).
Qed.

Lemma c1_resting_rests_receive_c2 :
  concrete_ref_step c1_resting (OpResult (Consume receive_c2) Rested) mismatched_configuration.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    c1_resting (Consume receive_c2) mismatched_configuration
    (enabled_set_is_empty_by_enumeration (current c1_resting) (Consume receive_c2) eq_refl)
    (contents_equiv_refl (current mismatched_configuration))
    (recorded_equiv_refl empty_history)).
Qed.

Lemma initial_rests_datum_a1 :
  concrete_ref_step concrete_initial (OpResult (Produce datum_a1) Rested) a1_resting.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    concrete_initial (Produce datum_a1) a1_resting
    (enabled_set_is_empty_by_enumeration (current concrete_initial) (Produce datum_a1) eq_refl)
    (contents_equiv_refl (current a1_resting)) (recorded_equiv_refl empty_history)).
Qed.

Lemma a1_resting_checkpoint :
  concrete_ref_step a1_resting (CheckpointReturned (concrete_root_of (current a1_resting)))
    a1_recorded.
Proof.
  exact (ref_checkpoint concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    a1_resting (current a1_resting) a1_recorded
    (contents_equiv_refl (current a1_resting)) (contents_equiv_refl (current a1_resting))
    (recorded_equiv_refl a1_history)).
Qed.

Lemma a1_comm_enabled_where_a1_rests :
  concrete_enabled (Contents [datum_a1] []) a1_operation a1_comm.
Proof.
  apply enabled_check_sound; [exact eq_refl|reflexivity].
Qed.

Lemma a1_recorded_fires :
  concrete_ref_step a1_recorded (OpResult a1_operation (Fired a1_comm)) a1_consumed.
Proof.
  apply (ref_fire concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    a1_recorded a1_operation a1_comm a1_consumed).
  - exact I.
  - exact a1_comm_enabled_where_a1_rests.
  - split; [exact (Permutation_refl [datum_a1])|exact (Permutation_refl (@nil item_continuation))].
  - exact (recorded_equiv_refl a1_history).
Qed.

Lemma a1_consumed_resets : concrete_ref_step a1_consumed (ResetTo a1_root) a1_recorded.
Proof.
  exact (ref_reset concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    a1_consumed a1_root (Contents [datum_a1] []) a1_recorded eq_refl
    (contents_equiv_refl (Contents [datum_a1] [])) (recorded_equiv_refl a1_history)).
Qed.

Lemma initial_rests_cross_join :
  concrete_ref_step concrete_initial (OpResult (Consume cross_join) Rested) cross_join_waiting.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    concrete_initial (Consume cross_join) cross_join_waiting
    (enabled_set_is_empty_by_enumeration (current concrete_initial) (Consume cross_join) eq_refl)
    (contents_equiv_refl (current cross_join_waiting)) (recorded_equiv_refl empty_history)).
Qed.

Lemma cross_join_waiting_rests_datum_b2 :
  concrete_ref_step cross_join_waiting (OpResult (Produce datum_b2) Rested)
    cross_join_configuration.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    cross_join_waiting (Produce datum_b2) cross_join_configuration
    (enabled_set_is_empty_by_enumeration (current cross_join_waiting) (Produce datum_b2) eq_refl)
    (contents_equiv_refl (current cross_join_configuration))
    (recorded_equiv_refl empty_history)).
Qed.

Lemma cross_join_rests_datum_a1 :
  concrete_ref_step cross_join_configuration (OpResult (Produce datum_a1) Rested)
    a1_beside_cross_join.
Proof.
  exact (ref_rest concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    cross_join_configuration (Produce datum_a1) a1_beside_cross_join
    (enabled_set_is_empty_by_enumeration (current cross_join_configuration) (Produce datum_a1)
       eq_refl)
    (contents_equiv_refl (current a1_beside_cross_join)) (recorded_equiv_refl empty_history)).
Qed.

Lemma member_c1_enabled :
  concrete_enabled two_candidate_contents two_candidate_operation member_c1.
Proof.
  apply enabled_check_sound; [exact eq_refl|reflexivity].
Qed.

Lemma member_c2_enabled :
  concrete_enabled two_candidate_contents two_candidate_operation member_c2.
Proof.
  apply enabled_check_sound; [exact eq_refl|reflexivity].
Qed.

Lemma system_process_comm_enabled_for_a_consume :
  concrete_enabled (current c1_resting) (Consume system_process) system_process_comm.
Proof.
  apply enabled_check_sound; [exact eq_refl|reflexivity].
Qed.

Lemma system_process_comm_enabled_for_an_install :
  concrete_enabled (current c1_resting) (Install system_process) system_process_comm.
Proof.
  apply enabled_check_sound; [exact eq_refl|reflexivity].
Qed.

(** Where [c!(1)] rests, the consume of [for(@y <= c)] fires with it. *)
Lemma c1_resting_fires_the_consumed_system_process :
  concrete_ref_step c1_resting (OpResult (Consume system_process) (Fired system_process_comm))
    emptied_configuration.
Proof.
  apply (ref_fire concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    c1_resting (Consume system_process) system_process_comm emptied_configuration).
  - exact I.
  - exact system_process_comm_enabled_for_a_consume.
  - split; [exact (Permutation_refl [datum_c1])|exact (Permutation_refl (@nil item_continuation))].
  - exact (recorded_equiv_refl empty_history).
Qed.

(** * Witnesses *)

Example reference_is_branching_bisimilar_where_two_candidates_wait :
  obligation_o1 concrete_reference concrete_initial /\
  obligation_o2 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    concrete_reference concrete_initial /\
  obligation_o3 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    concrete_reference concrete_initial /\
  obligation_o4 concrete_reference concrete_initial /\
  reachable concrete_reference concrete_initial two_candidate_configuration /\
  branching_bisimilar (impl_step concrete_reference) concrete_action_step
    two_candidate_configuration two_candidate_configuration.
Proof.
  destruct (reference_implementation_meets_o1_to_o4 concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec concrete_initial) as [o1 [o2 [o3 o4]]].
  assert (reached : reachable concrete_reference concrete_initial two_candidate_configuration).
  { apply (reachable_visible concrete_reference concrete_initial c2_resting
      (OpResult (Produce datum_c1) Rested) two_candidate_configuration).
    - apply (reachable_visible concrete_reference concrete_initial concrete_initial
        (OpResult (Produce datum_c2) Rested) c2_resting).
      + apply reachable_start.
      + exact initial_rests_datum_c2.
    - exact c2_resting_rests_datum_c1. }
  split; [exact o1|].
  split; [exact o2|].
  split; [exact o3|].
  split; [exact o4|].
  split; [exact reached|].
  exact (proj2 (obligations_give_branching_bisimulation concrete_matches concrete_guard_accepts
    concrete_root_of Nat.eq_dec concrete_reference concrete_initial o1 o2 o3 o4)
    two_candidate_configuration reached).
Qed.

Example enumeration_selector_is_a_refinement :
  chooses_members concrete_matches concrete_guard_accepts enumeration_choice /\
  enumeration_choice two_candidate_contents two_candidate_operation = member_c1 /\
  (forall (origin : store_configuration) (observed : store_label)
      (successor : store_configuration),
     selector_visible concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
       enumeration_choice origin observed successor ->
     concrete_ref_step origin observed successor) /\
  (forall configuration_now : store_configuration,
     simulated_by (impl_step (concrete_selector enumeration_choice)) concrete_action_step
       configuration_now configuration_now).
Proof.
  destruct (selector_resolution_is_refinement concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec enumeration_choice enumeration_choice_chooses_members)
    as [steps_are_reference_steps [simulated _]].
  split; [exact enumeration_choice_chooses_members|].
  split; [reflexivity|].
  split; [exact steps_are_reference_steps|exact simulated].
Qed.

Example two_candidates_separate_the_selector_from_the_reference :
  concrete_enabled two_candidate_contents two_candidate_operation member_c1 /\
  concrete_enabled two_candidate_contents two_candidate_operation member_c2 /\
  member_c1 <> member_c2 /\
  ~ branching_bisimilar (impl_step (concrete_selector enumeration_choice)) concrete_action_step
      two_candidate_configuration two_candidate_configuration.
Proof.
  assert (different : member_c1 <> member_c2).
  { intros same. discriminate same. }
  split; [exact member_c1_enabled|].
  split; [exact member_c2_enabled|].
  split; [exact different|].
  exact (selector_resolution_is_not_bisimilar concrete_matches concrete_guard_accepts
    concrete_root_of Nat.eq_dec enumeration_choice two_candidate_configuration
    two_candidate_operation member_c1 member_c2 member_c1_enabled member_c2_enabled different).
Qed.

Example replay_follows_the_member_the_selector_skips :
  enumeration_choice two_candidate_contents two_candidate_operation = member_c1 /\
  (exists outcome,
     concrete_replay_step two_candidate_configuration two_candidate_operation member_c2
       outcome) /\
  (forall outcome,
     concrete_replay_step two_candidate_configuration two_candidate_operation member_c2
       outcome ->
     exists successor,
       outcome = Replayed successor /\
       concrete_ref_step two_candidate_configuration
         (OpResult two_candidate_operation (Fired member_c2)) successor).
Proof.
  split; [reflexivity|].
  destruct (replay_follows_any_enabled_choice concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec two_candidate_configuration two_candidate_operation member_c2
      member_c2_enabled) as [replay_exists replays_are_reference_steps].
  split; [exact (replay_exists I)|exact replays_are_reference_steps].
Qed.

Example replay_performs_the_logged_event_in_either_position_order :
  same_event performed_in_bind_order logged_in_other_order /\
  performed_in_bind_order <> logged_in_other_order /\
  concrete_replay_step two_candidate_configuration both_of_c_operation logged_in_other_order
    (Replayed emptied_configuration) /\
  concrete_ref_step two_candidate_configuration
    (OpResult both_of_c_operation (Fired logged_in_other_order)) emptied_configuration.
Proof.
  assert (same : same_event performed_in_bind_order logged_in_other_order).
  { split; [reflexivity|exact (perm_swap datum_c2 datum_c1 [])]. }
  assert (performed_enabled :
            concrete_enabled two_candidate_contents both_of_c_operation performed_in_bind_order).
  { apply enabled_check_sound; [exact eq_refl|reflexivity]. }
  assert (logged_enabled :
            concrete_enabled two_candidate_contents both_of_c_operation logged_in_other_order).
  { apply enabled_check_sound; [exact eq_refl|reflexivity]. }
  assert (replayed :
            concrete_replay_step two_candidate_configuration both_of_c_operation
              logged_in_other_order (Replayed emptied_configuration)).
  { apply (replay_perform concrete_matches concrete_guard_accepts two_candidate_configuration
      both_of_c_operation logged_in_other_order performed_in_bind_order emptied_configuration).
    - exact I.
    - exact same.
    - exact performed_enabled.
    - split; [exact (Permutation_refl [datum_c1; datum_c2])
             |exact (Permutation_refl (@nil item_continuation))].
    - exact (recorded_equiv_refl empty_history). }
  split; [exact same|].
  split; [intros identical; discriminate identical|].
  split; [exact replayed|].
  destruct (proj2 (replay_follows_any_enabled_choice concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec two_candidate_configuration both_of_c_operation
      logged_in_other_order logged_enabled) (Replayed emptied_configuration) replayed)
    as [successor [same_outcome reference_step]].
  injection same_outcome as same_successor.
  subst successor.
  exact reference_step.
Qed.

Example replay_refuses_a_value_no_datum_supplies :
  concrete_replay_step two_candidate_configuration two_candidate_operation unsupplied_comm
    ReplayRefused /\
  (forall outcome,
     concrete_replay_step two_candidate_configuration two_candidate_operation unsupplied_comm
       outcome ->
     outcome = ReplayRefused) /\
  (forall successor,
     ~ concrete_ref_step two_candidate_configuration
         (OpResult two_candidate_operation (Fired unsupplied_comm)) successor).
Proof.
  assert (nothing_enabled :
            forall chosen : store_rendezvous,
              same_event chosen unsupplied_comm ->
              ~ concrete_enabled (current two_candidate_configuration) two_candidate_operation
                  chosen).
  { intros [waiting selection] [same_waiting same_selection] member_now.
    change (waiting = receive_any channel_c) in same_waiting.
    change (Permutation selection [datum_c3]) in same_selection.
    pose proof (Permutation_length_1_inv (Permutation_sym same_selection)) as selection_is_logged.
    subst waiting selection.
    pose proof (enabled_check_complete _ _ _ member_now) as checked.
    compute in checked.
    discriminate checked. }
  exact (replay_refuses_a_non_enabled_choice concrete_matches concrete_guard_accepts
    concrete_root_of Nat.eq_dec two_candidate_configuration two_candidate_operation
    unsupplied_comm nothing_enabled).
Qed.

Example reached_rest_states_hold_no_enabled_rendezvous :
  concrete_ref_reachable concrete_initial mismatched_configuration /\
  reachable concrete_reference concrete_initial mismatched_configuration /\
  at_rest concrete_matches concrete_guard_accepts (current mismatched_configuration) /\
  at_rest concrete_matches concrete_guard_accepts
    (current (impl_contents concrete_reference mismatched_configuration)).
Proof.
  assert (reference_reached : concrete_ref_reachable concrete_initial mismatched_configuration).
  { apply (ref_reachable_step concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      concrete_initial c1_resting (OpResult (Consume receive_c2) Rested)
      mismatched_configuration).
    - apply (ref_reachable_step concrete_matches concrete_guard_accepts concrete_root_of
        Nat.eq_dec concrete_initial concrete_initial (OpResult (Produce datum_c1) Rested)
        c1_resting).
      + apply ref_reachable_origin.
      + exact initial_rests_datum_c1.
    - exact c1_resting_rests_receive_c2. }
  assert (implementation_reached :
            reachable concrete_reference concrete_initial mismatched_configuration).
  { apply (reachable_visible concrete_reference concrete_initial c1_resting
      (OpResult (Consume receive_c2) Rested) mismatched_configuration).
    - apply (reachable_visible concrete_reference concrete_initial concrete_initial
        (OpResult (Produce datum_c1) Rested) c1_resting).
      + apply reachable_start.
      + exact initial_rests_datum_c1.
    - exact c1_resting_rests_receive_c2. }
  destruct (rest_states_have_no_enabled_rendezvous concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec) as [reference_settles implementations_settle].
  destruct (reference_implementation_meets_o1_to_o4 concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec concrete_initial) as [o1 [o2 _]].
  split; [exact reference_reached|].
  split; [exact implementation_reached|].
  split.
  - exact (proj1 (reference_settles mismatched_configuration reference_reached)).
  - exact (implementations_settle concrete_reference concrete_initial o1 o2
      (config_equiv_refl concrete_initial) mismatched_configuration implementation_reached).
Qed.

Example install_rests_only_where_nothing_matches :
  (forall candidate : store_rendezvous,
     ~ concrete_enabled (current concrete_initial) (Install system_process) candidate) /\
  (exists successor : store_configuration,
     concrete_ref_step concrete_initial (OpResult (Install system_process) Rested) successor) /\
  concrete_enabled (current c1_resting) (Install system_process) system_process_comm /\
  (forall (outcome : store_result) (successor : store_configuration),
     ~ concrete_ref_step c1_resting (OpResult (Install system_process) outcome) successor).
Proof.
  assert (nothing_enabled :
            forall candidate : store_rendezvous,
              ~ concrete_enabled (current concrete_initial) (Install system_process) candidate).
  { exact (enabled_set_is_empty_by_enumeration (current concrete_initial) (Install system_process)
      eq_refl). }
  split; [exact nothing_enabled|].
  split.
  - exact (proj2 (proj1 (install_rests_exactly_when_nothing_is_enabled concrete_matches
      concrete_guard_accepts concrete_root_of Nat.eq_dec concrete_initial system_process))
      nothing_enabled).
  - split; [exact system_process_comm_enabled_for_an_install|].
    exact (proj2 (install_rests_exactly_when_nothing_is_enabled concrete_matches
      concrete_guard_accepts concrete_root_of Nat.eq_dec c1_resting system_process)
      system_process_comm system_process_comm_enabled_for_an_install).
Qed.

(** * Negative control M1: a selector that fires a non-member *)

(** [pattern_blind_choice] takes the first resting data, as many as the
    arriving continuation has positions, without matching channels or
    patterns. *)
Definition pattern_blind_choice (contents_now : store_contents) (op : store_operation)
    : store_rendezvous :=
  match op with
  | Produce arriving => Rendezvous (Continuation [] [] tt 0 false []) [arriving]
  | Consume waiting =>
      Rendezvous waiting
        (firstn (length (continuation_channels waiting)) (resting_data contents_now))
  | Install waiting =>
      Rendezvous waiting
        (firstn (length (continuation_channels waiting)) (resting_data contents_now))
  end.

(** [for(@2 <- c)] arrives where [c!(1)] and [c!(2)] rest. *)
Definition blind_operation : store_operation := Consume receive_c2.
Definition blind_member : store_rendezvous := Rendezvous receive_c2 [datum_c2].

Example pattern_blind_selector_fires_a_non_member :
  concrete_enabled two_candidate_contents blind_operation blind_member /\
  ~ concrete_enabled two_candidate_contents blind_operation
      (pattern_blind_choice two_candidate_contents blind_operation) /\
  ~ chooses_members concrete_matches concrete_guard_accepts pattern_blind_choice /\
  ~ (forall (origin : store_configuration) (observed : store_label)
        (successor : store_configuration),
       selector_visible concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
         pattern_blind_choice origin observed successor ->
       concrete_ref_step origin observed successor) /\
  ~ simulated_by (impl_step (concrete_selector pattern_blind_choice)) concrete_action_step
      two_candidate_configuration two_candidate_configuration.
Proof.
  assert (member_enabled : concrete_enabled two_candidate_contents blind_operation blind_member).
  { apply enabled_check_sound; [exact eq_refl|reflexivity]. }
  assert (pick_not_enabled :
            ~ concrete_enabled two_candidate_contents blind_operation
                (pattern_blind_choice two_candidate_contents blind_operation)).
  { intros pick_enabled.
    pose proof (enabled_check_complete _ _ _ pick_enabled) as checked.
    compute in checked.
    discriminate checked. }
  assert (blind_performs :
            performs (current two_candidate_configuration) blind_operation
              (pattern_blind_choice (current two_candidate_configuration) blind_operation)
              (current c2_resting)).
  { split; [exact (Permutation_refl [datum_c1; datum_c2])
           |exact (Permutation_refl (@nil item_continuation))]. }
  assert (blind_fires :
            selector_visible concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
              pattern_blind_choice two_candidate_configuration
              (OpResult blind_operation
                 (Fired (pattern_blind_choice (current two_candidate_configuration)
                           blind_operation)))
              c2_resting).
  { exact (selector_fire concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      pattern_blind_choice two_candidate_configuration blind_operation c2_resting I
      (ex_intro _ blind_member member_enabled) blind_performs
      (recorded_equiv_refl empty_history)). }
  split; [exact member_enabled|].
  split; [exact pick_not_enabled|].
  split.
  - intros members.
    exact (pick_not_enabled
      (members two_candidate_contents blind_operation blind_member member_enabled)).
  - split.
    + intros claimed.
      destruct (ref_fire_label_means_enabled concrete_matches concrete_guard_accepts
          concrete_root_of Nat.eq_dec two_candidate_configuration blind_operation
          (pattern_blind_choice (current two_candidate_configuration) blind_operation)
          c2_resting (claimed _ _ _ blind_fires)) as [pick_enabled _].
      exact (pick_not_enabled pick_enabled).
    + intros [relation [forward related]].
      destruct (forward two_candidate_configuration two_candidate_configuration
          (Visible (OpResult blind_operation
             (Fired (pattern_blind_choice (current two_candidate_configuration)
                       blind_operation))))
          c2_resting related blind_fires)
        as [[visible_is_internal _]|[reference_middle [reference_next [path [reference_fires _]]]]].
      * discriminate visible_is_internal.
      * assert (same_state : two_candidate_configuration = reference_middle).
        { apply (internal_star_without_internal_steps _ _ concrete_action_step).
          - intros state state' impossible. exact impossible.
          - exact path. }
        subst reference_middle.
        destruct (ref_fire_label_means_enabled concrete_matches concrete_guard_accepts
            concrete_root_of Nat.eq_dec two_candidate_configuration blind_operation
            (pattern_blind_choice (current two_candidate_configuration) blind_operation)
            reference_next reference_fires) as [pick_enabled _].
        exact (pick_not_enabled pick_enabled).
Qed.

(** * Negative control M2: an implementation that rests while a COMM is
    enabled *)

(** [careless_rest_visible] is the reference with the emptiness premise of a
    rest removed: every operation may rest. *)
Definition careless_rest_visible (origin : store_configuration) (observed : store_label)
    (successor : store_configuration) : Prop :=
  match observed with
  | OpResult op Rested =>
      contents_equiv (current successor) (rest (current origin) op) /\
      recorded_equiv (recorded successor) (recorded origin)
  | other => concrete_ref_step origin other successor
  end.

Definition careless_rest_implementation : store_implementation :=
  Implementation store_configuration (fun configuration_now => configuration_now)
    careless_rest_visible (fun _ _ => False).

(** [for(@1 <- c)] rests beside [c!(1)]. *)
Definition missed_operation : store_operation := Consume receive_c1.
Definition missed_comm : store_rendezvous := Rendezvous receive_c1 [datum_c1].
Definition missed_configuration : store_configuration :=
  Configuration (Contents [datum_c1] [receive_c1]) empty_history.

Example careless_rest_holds_an_enabled_rendezvous :
  obligation_o1 careless_rest_implementation concrete_initial /\
  config_equiv (impl_contents careless_rest_implementation concrete_initial) concrete_initial /\
  reachable careless_rest_implementation concrete_initial missed_configuration /\
  ~ obligation_o2 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      careless_rest_implementation concrete_initial /\
  ~ at_rest concrete_matches concrete_guard_accepts
      (current (impl_contents careless_rest_implementation missed_configuration)).
Proof.
  assert (c1_reached : reachable careless_rest_implementation concrete_initial c1_resting).
  { apply (reachable_visible careless_rest_implementation concrete_initial concrete_initial
      (OpResult (Produce datum_c1) Rested) c1_resting).
    - apply reachable_start.
    - split; [exact (contents_equiv_refl (current c1_resting))
             |exact (recorded_equiv_refl empty_history)]. }
  assert (careless_step :
            impl_visible careless_rest_implementation c1_resting
              (OpResult missed_operation Rested) missed_configuration).
  { split; [exact (contents_equiv_refl (current missed_configuration))
           |exact (recorded_equiv_refl empty_history)]. }
  assert (missed_enabled : concrete_enabled (current c1_resting) missed_operation missed_comm).
  { apply enabled_check_sound; [exact eq_refl|reflexivity]. }
  split; [intros state state' _ impossible; destruct impossible|].
  split; [exact (config_equiv_refl concrete_initial)|].
  split; [exact (reachable_visible careless_rest_implementation concrete_initial c1_resting
                   (OpResult missed_operation Rested) missed_configuration c1_reached
                   careless_step)|].
  split.
  - intros o2.
    exact (ref_rest_label_means_nothing_enabled concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec (impl_contents careless_rest_implementation c1_resting)
      missed_operation (impl_contents careless_rest_implementation missed_configuration)
      (o2 c1_resting (OpResult missed_operation Rested) missed_configuration c1_reached
         careless_step)
      missed_comm missed_enabled).
  - intros calm.
    apply (calm missed_comm).
    split; [left; reflexivity|].
    split; [apply admissible_by_computation; reflexivity|].
    apply data_available_by_computation; reflexivity.
Qed.

(** * Negative control M3: a reset that returns other contents *)

(** [stale_reset_visible] is the reference, except that a reset to a recorded
    root keeps the current contents instead of the recorded ones. *)
Definition stale_reset_visible (origin : store_configuration) (observed : store_label)
    (successor : store_configuration) : Prop :=
  match observed with
  | ResetTo root =>
      (exists snapshot, recorded origin root = Some snapshot) /\
      contents_equiv (current successor) (current origin) /\
      recorded_equiv (recorded successor) (recorded origin)
  | other => concrete_ref_step origin other successor
  end.

Definition stale_reset_implementation : store_implementation :=
  Implementation store_configuration (fun configuration_now => configuration_now)
    stale_reset_visible (fun _ _ => False).

Example stale_reset_is_not_bisimilar_to_its_abstraction :
  obligation_o1 stale_reset_implementation concrete_initial /\
  reachable stale_reset_implementation concrete_initial a1_consumed /\
  ~ obligation_o4 stale_reset_implementation concrete_initial /\
  ~ branching_bisimilar (impl_step stale_reset_implementation) concrete_action_step
      a1_consumed (impl_contents stale_reset_implementation a1_consumed).
Proof.
  assert (reached : reachable stale_reset_implementation concrete_initial a1_consumed).
  { apply (reachable_visible stale_reset_implementation concrete_initial a1_recorded
      (OpResult a1_operation (Fired a1_comm)) a1_consumed).
    - apply (reachable_visible stale_reset_implementation concrete_initial a1_resting
        (CheckpointReturned (concrete_root_of (current a1_resting))) a1_recorded).
      + apply (reachable_visible stale_reset_implementation concrete_initial concrete_initial
          (OpResult (Produce datum_a1) Rested) a1_resting).
        * apply reachable_start.
        * exact initial_rests_datum_a1.
      + exact a1_resting_checkpoint.
    - exact a1_recorded_fires. }
  split; [intros state state' _ impossible; destruct impossible|].
  split; [exact reached|].
  split.
  - intros o4.
    destruct (o4 a1_consumed a1_root (Contents [datum_a1] []) reached eq_refl)
      as [stale_next [stale_step restored]].
    destruct stale_step as [_ [unchanged _]].
    exact (Permutation_nil_cons
      (proj1 (contents_equiv_trans _ _ _ (contents_equiv_sym _ _ unchanged) restored))).
  - intros [relation [[_ backward] related]].
    destruct (backward a1_consumed a1_consumed (Visible (ResetTo a1_root)) a1_recorded
        related a1_consumed_resets)
      as [[reset_is_internal _]|[stale_middle [stale_next [path [stale_step [_ related_next]]]]]].
    + discriminate reset_is_internal.
    + assert (same_state : a1_consumed = stale_middle).
      { apply (internal_star_without_internal_steps _ _
          (impl_step stale_reset_implementation)).
        - intros state state' impossible. exact impossible.
        - exact path. }
      subst stale_middle.
      destruct stale_step as [_ [unchanged _]].
      destruct (backward a1_recorded stale_next (Visible (OpResult a1_operation (Fired a1_comm)))
          a1_consumed related_next a1_recorded_fires)
        as [[fire_is_internal _]|[stale_middle [stale_after [path_after [stale_fires _]]]]].
      * discriminate fire_is_internal.
      * assert (same_next : stale_next = stale_middle).
        { apply (internal_star_without_internal_steps _ _
            (impl_step stale_reset_implementation)).
          - intros state state' impossible. exact impossible.
          - exact path_after. }
        subst stale_middle.
        destruct (ref_fire_label_means_enabled concrete_matches concrete_guard_accepts
            concrete_root_of Nat.eq_dec stale_next a1_operation a1_comm stale_after stale_fires)
          as [stale_enabled _].
        pose proof (enabled_respects_contents_equiv concrete_matches concrete_guard_accepts
          (current stale_next) (current a1_consumed) a1_operation a1_comm unchanged
          stale_enabled) as emptied_enabled.
        pose proof (enabled_check_complete _ _ _ emptied_enabled) as checked.
        compute in checked.
        discriminate checked.
Qed.

(** * Negative control M4: a reader that sorts a channel list *)

Fixpoint insert_channel (channel : nat) (sorted : list nat) : list nat :=
  match sorted with
  | [] => [channel]
  | head :: tail =>
      if Nat.leb channel head then channel :: head :: tail else head :: insert_channel channel tail
  end.

Fixpoint sort_channels (channels : list nat) : list nat :=
  match channels with
  | [] => []
  | head :: tail => insert_channel head (sort_channels tail)
  end.

(** The continuation read back with its channel list sorted and its patterns
    in their stored order. *)
Definition sorted_continuation (waiting : item_continuation) : item_continuation :=
  Continuation (sort_channels (continuation_channels waiting)) (continuation_patterns waiting)
    (continuation_guard waiting) (continuation_body waiting) (continuation_persistent waiting)
    (continuation_peeks waiting).

Definition sorted_contents (contents_now : store_contents) : store_contents :=
  Contents (resting_data contents_now)
    (map sorted_continuation (resting_continuations contents_now)).

Definition sorting_reader (configuration_now : store_configuration) : store_configuration :=
  Configuration (sorted_contents (current configuration_now))
    (fun root =>
       match recorded configuration_now root with
       | Some snapshot => Some (sorted_contents snapshot)
       | None => None
       end).

(** The reference's own steps, read back by [sorting_reader]. *)
Definition sorting_reader_implementation : store_implementation :=
  Implementation store_configuration sorting_reader concrete_ref_step (fun _ _ => False).

(** A reset of the underlying state to a recorded snapshot reads back as the
    sorted snapshot, which is what the reader records under that root: the
    sorting reader meets O4. *)
Lemma sorting_reader_meets_o4 : obligation_o4 sorting_reader_implementation concrete_initial.
Proof.
  intros state root snapshot _ stored.
  change (match recorded state root with
          | Some recorded_snapshot => Some (sorted_contents recorded_snapshot)
          | None => None
          end = Some snapshot) in stored.
  revert stored.
  destruct (recorded state root) as [recorded_snapshot|] eqn:raw_stored; intros stored.
  - injection stored as same_snapshot.
    subst snapshot.
    exists (Configuration recorded_snapshot (recorded state)).
    split.
    + exact (ref_reset concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
        state root recorded_snapshot (Configuration recorded_snapshot (recorded state))
        raw_stored (contents_equiv_refl recorded_snapshot)
        (recorded_equiv_refl (recorded state))).
    + exact (contents_equiv_refl (sorted_contents recorded_snapshot)).
  - discriminate stored.
Qed.

(** In the sorted reading, [for(@1 <- a & @2 <- b)] meets [a!(1)] and
    [b!(2)]. *)
Definition sorted_comm : store_rendezvous :=
  Rendezvous (sorted_continuation cross_join) [datum_a1; datum_b2].

Example sorting_reader_is_not_bisimilar_to_its_abstraction :
  obligation_o1 sorting_reader_implementation concrete_initial /\
  obligation_o4 sorting_reader_implementation concrete_initial /\
  reachable sorting_reader_implementation concrete_initial cross_join_configuration /\
  ~ obligation_o2 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      sorting_reader_implementation concrete_initial /\
  ~ branching_bisimilar (impl_step sorting_reader_implementation) concrete_action_step
      cross_join_configuration
      (impl_contents sorting_reader_implementation cross_join_configuration).
Proof.
  assert (reached :
            reachable sorting_reader_implementation concrete_initial cross_join_configuration).
  { apply (reachable_visible sorting_reader_implementation concrete_initial cross_join_waiting
      (OpResult (Produce datum_b2) Rested) cross_join_configuration).
    - apply (reachable_visible sorting_reader_implementation concrete_initial concrete_initial
        (OpResult (Consume cross_join) Rested) cross_join_waiting).
      + apply reachable_start.
      + exact initial_rests_cross_join.
    - exact cross_join_waiting_rests_datum_b2. }
  assert (sorted_comm_enabled :
            concrete_enabled (current (sorting_reader cross_join_configuration))
              (Produce datum_a1) sorted_comm).
  { apply enabled_check_sound; [left; reflexivity|reflexivity]. }
  split; [intros state state' _ impossible; destruct impossible|].
  split; [exact sorting_reader_meets_o4|].
  split; [exact reached|].
  split.
  - intros o2.
    exact (ref_rest_label_means_nothing_enabled concrete_matches concrete_guard_accepts
      concrete_root_of Nat.eq_dec
      (impl_contents sorting_reader_implementation cross_join_configuration) (Produce datum_a1)
      (impl_contents sorting_reader_implementation a1_beside_cross_join)
      (o2 cross_join_configuration (OpResult (Produce datum_a1) Rested) a1_beside_cross_join
         reached cross_join_rests_datum_a1)
      sorted_comm sorted_comm_enabled).
  - intros [relation [[forward _] related]].
    destruct (forward cross_join_configuration
        (impl_contents sorting_reader_implementation cross_join_configuration)
        (Visible (OpResult (Produce datum_a1) Rested)) a1_beside_cross_join related
        cross_join_rests_datum_a1)
      as [[rest_is_internal _]|[reference_middle [reference_next [path [reference_rests _]]]]].
    + discriminate rest_is_internal.
    + assert (same_state :
                impl_contents sorting_reader_implementation cross_join_configuration =
                reference_middle).
      { apply (internal_star_without_internal_steps _ _ concrete_action_step).
        - intros state state' impossible. exact impossible.
        - exact path. }
      subst reference_middle.
      exact (ref_rest_label_means_nothing_enabled concrete_matches concrete_guard_accepts
        concrete_root_of Nat.eq_dec
        (impl_contents sorting_reader_implementation cross_join_configuration)
        (Produce datum_a1) reference_next reference_rests sorted_comm sorted_comm_enabled).
Qed.

(** * Negative control M5: an implementation that fires an install *)

(** [install_firing_visible] takes every reference step and, besides, fires an
    install as the consume of the same continuation would: [Install k] fires a
    candidate, with a successor, whenever [Consume k] does. *)
Definition install_firing_visible (origin : store_configuration) (observed : store_label)
    (successor : store_configuration) : Prop :=
  concrete_ref_step origin observed successor \/
  (exists (arriving : item_continuation) (candidate : store_rendezvous),
     observed = OpResult (Install arriving) (Fired candidate) /\
     concrete_ref_step origin (OpResult (Consume arriving) (Fired candidate)) successor).

Definition install_firing_implementation : store_implementation :=
  Implementation store_configuration (fun configuration_now => configuration_now)
    install_firing_visible (fun _ _ => False).

(** Every reference step is a step of the mutant, so it meets O3. *)
Lemma install_firing_meets_o3 :
  obligation_o3 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    install_firing_implementation concrete_initial.
Proof.
  intros state observed configuration_next _ step.
  exists state, configuration_next.
  split; [apply internal_star_refl|].
  split; [left; exact step|exact (config_equiv_refl configuration_next)].
Qed.

(** Its resets are the reference's, so it meets O4. *)
Lemma install_firing_meets_o4 : obligation_o4 install_firing_implementation concrete_initial.
Proof.
  intros state root snapshot _ stored.
  exists (Configuration snapshot (recorded state)).
  split.
  - left.
    exact (ref_reset concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      state root snapshot (Configuration snapshot (recorded state)) stored
      (contents_equiv_refl snapshot) (recorded_equiv_refl (recorded state))).
  - exact (contents_equiv_refl snapshot).
Qed.

Example install_firing_is_not_bisimilar_to_its_abstraction :
  obligation_o1 install_firing_implementation concrete_initial /\
  obligation_o3 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
    install_firing_implementation concrete_initial /\
  obligation_o4 install_firing_implementation concrete_initial /\
  reachable install_firing_implementation concrete_initial c1_resting /\
  impl_visible install_firing_implementation c1_resting
    (OpResult (Install system_process) (Fired system_process_comm)) emptied_configuration /\
  ~ obligation_o2 concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      install_firing_implementation concrete_initial /\
  ~ branching_bisimilar (impl_step install_firing_implementation) concrete_action_step
      c1_resting (impl_contents install_firing_implementation c1_resting).
Proof.
  assert (reached : reachable install_firing_implementation concrete_initial c1_resting).
  { apply (reachable_visible install_firing_implementation concrete_initial concrete_initial
      (OpResult (Produce datum_c1) Rested) c1_resting).
    - apply reachable_start.
    - left. exact initial_rests_datum_c1. }
  assert (install_fires :
            impl_visible install_firing_implementation c1_resting
              (OpResult (Install system_process) (Fired system_process_comm))
              emptied_configuration).
  { right. exists system_process, system_process_comm.
    split; [exact eq_refl|exact c1_resting_fires_the_consumed_system_process]. }
  split; [intros state state' _ impossible; destruct impossible|].
  split; [exact install_firing_meets_o3|].
  split; [exact install_firing_meets_o4|].
  split; [exact reached|].
  split; [exact install_fires|].
  split.
  - intros o2.
    exact (install_never_fires concrete_matches concrete_guard_accepts concrete_root_of Nat.eq_dec
      (impl_contents install_firing_implementation c1_resting) system_process system_process_comm
      (impl_contents install_firing_implementation emptied_configuration)
      (o2 c1_resting (OpResult (Install system_process) (Fired system_process_comm))
         emptied_configuration reached install_fires)).
  - intros [relation [[forward _] related]].
    destruct (forward c1_resting (impl_contents install_firing_implementation c1_resting)
        (Visible (OpResult (Install system_process) (Fired system_process_comm)))
        emptied_configuration related install_fires)
      as [[fire_is_internal _]|[reference_middle [reference_next [_ [reference_fires _]]]]].
    + discriminate fire_is_internal.
    + exact (install_never_fires concrete_matches concrete_guard_accepts concrete_root_of
        Nat.eq_dec reference_middle system_process system_process_comm reference_next
        reference_fires).
Qed.

Print Assumptions enumeration_choice_chooses_members.
Print Assumptions reference_is_branching_bisimilar_where_two_candidates_wait.
Print Assumptions enumeration_selector_is_a_refinement.
Print Assumptions two_candidates_separate_the_selector_from_the_reference.
Print Assumptions replay_follows_the_member_the_selector_skips.
Print Assumptions replay_performs_the_logged_event_in_either_position_order.
Print Assumptions replay_refuses_a_value_no_datum_supplies.
Print Assumptions reached_rest_states_hold_no_enabled_rendezvous.
Print Assumptions install_rests_only_where_nothing_matches.
Print Assumptions pattern_blind_selector_fires_a_non_member.
Print Assumptions careless_rest_holds_an_enabled_rendezvous.
Print Assumptions stale_reset_is_not_bisimilar_to_its_abstraction.
Print Assumptions sorting_reader_is_not_bisimilar_to_its_abstraction.
Print Assumptions install_firing_is_not_bisimilar_to_its_abstraction.

End RSpaceEagerReferenceMutants.
