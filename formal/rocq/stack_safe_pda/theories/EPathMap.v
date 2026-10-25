From Stdlib Require Import List Bool Arith FunctionalExtensionality.

Import ListNotations.
Set Implicit Arguments.

Section PointwiseTrie.

Context {Key Value : Type}.
Variable key_eq_dec : forall left right : Key, {left = right} + {left <> right}.
Variable value_eq_dec : forall left right : Value, {left = right} + {left <> right}.

Inductive value_cell : Type :=
| Absent : value_cell
| Present : Value -> value_cell
| Conflict : value_cell.

Definition value_join (left right : value_cell) : value_cell :=
  match left, right with
  | Conflict, _ | _, Conflict => Conflict
  | Absent, cell | cell, Absent => cell
  | Present left_value, Present right_value =>
      if value_eq_dec left_value right_value then Present left_value else Conflict
  end.

Definition value_meet (left right : value_cell) : value_cell :=
  match left, right with
  | Conflict, _ | _, Conflict => Conflict
  | Absent, _ | _, Absent => Absent
  | Present left_value, Present right_value =>
      if value_eq_dec left_value right_value then Present left_value else Conflict
  end.

Definition value_subtract (left right : value_cell) : value_cell :=
  match left, right with
  | Conflict, _ | _, Conflict => Conflict
  | Absent, _ => Absent
  | cell, Absent => cell
  | Present _, Present _ => Absent
  end.

Lemma value_join_absent_left : forall cell, value_join Absent cell = cell.
Proof. destruct cell; reflexivity. Qed.

Lemma value_join_absent_right : forall cell, value_join cell Absent = cell.
Proof. destruct cell; reflexivity. Qed.

Lemma value_join_idempotent : forall cell, value_join cell cell = cell.
Proof.
  destruct cell; simpl; try reflexivity.
  destruct (value_eq_dec v v); congruence.
Qed.

Lemma value_join_commutative :
  forall left right, value_join left right = value_join right left.
Proof.
  destruct left, right; simpl; try reflexivity.
  destruct (value_eq_dec v v0) as [same | different].
  - subst. destruct (value_eq_dec v0 v0); congruence.
  - destruct (value_eq_dec v0 v); [congruence | reflexivity].
Qed.

Lemma value_meet_commutative :
  forall left right, value_meet left right = value_meet right left.
Proof.
  destruct left, right; simpl; try reflexivity.
  destruct (value_eq_dec v v0) as [same | different].
  - subst. destruct (value_eq_dec v0 v0); congruence.
  - destruct (value_eq_dec v0 v); [congruence | reflexivity].
Qed.

Lemma value_meet_idempotent : forall cell, value_meet cell cell = cell.
Proof.
  destruct cell; simpl; try reflexivity.
  destruct (value_eq_dec v v); congruence.
Qed.

Lemma value_subtract_absent_right :
  forall cell, value_subtract cell Absent = cell.
Proof. destruct cell; reflexivity. Qed.

Lemma value_subtract_self :
  forall value, value_subtract (Present value) (Present value) = Absent.
Proof. reflexivity. Qed.

Lemma value_subtract_present_mask :
  forall left right, value_subtract (Present left) (Present right) = Absent.
Proof. reflexivity. Qed.

Definition trie_map := Key -> value_cell.
Definition empty_map : trie_map := fun _ => Absent.
Definition map_join (left right : trie_map) : trie_map :=
  fun key => value_join (left key) (right key).
Definition map_meet (left right : trie_map) : trie_map :=
  fun key => value_meet (left key) (right key).
Definition map_subtract (left right : trie_map) : trie_map :=
  fun key => value_subtract (left key) (right key).
Definition conflict_free (map : trie_map) : Prop :=
  forall key, map key <> Conflict.

Theorem map_join_empty_left : forall map, map_join empty_map map = map.
Proof.
  intro map. apply functional_extensionality. intro key.
  apply value_join_absent_left.
Qed.

Theorem map_join_empty_right : forall map, map_join map empty_map = map.
Proof.
  intro map. apply functional_extensionality. intro key.
  apply value_join_absent_right.
Qed.

Theorem map_join_idempotent : forall map, map_join map map = map.
Proof.
  intro map. apply functional_extensionality. intro key.
  apply value_join_idempotent.
Qed.

Theorem map_join_commutative : forall left right, map_join left right = map_join right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  apply value_join_commutative.
Qed.

Theorem map_meet_empty_left :
  forall map, conflict_free map -> map_meet empty_map map = empty_map.
Proof.
  intros map valid. apply functional_extensionality. intro key.
  unfold map_meet, empty_map.
  destruct (map key) eqn:cell; try reflexivity.
  exfalso. exact (valid key cell).
Qed.

Theorem map_meet_empty_right :
  forall map, conflict_free map -> map_meet map empty_map = empty_map.
Proof.
  intros map valid. apply functional_extensionality. intro key.
  unfold map_meet, empty_map.
  destruct (map key) eqn:cell; try reflexivity.
  exfalso. exact (valid key cell).
Qed.

Theorem map_meet_idempotent : forall map, map_meet map map = map.
Proof.
  intro map. apply functional_extensionality. intro key.
  apply value_meet_idempotent.
Qed.

Theorem map_meet_commutative : forall left right, map_meet left right = map_meet right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  apply value_meet_commutative.
Qed.

Theorem map_subtract_empty_right : forall map, map_subtract map empty_map = map.
Proof.
  intro map. apply functional_extensionality. intro key.
  apply value_subtract_absent_right.
Qed.

Theorem map_subtract_self_removes_present_cells :
  forall map key value, map key = Present value -> map_subtract map map key = Absent.
Proof.
  intros map key value present. unfold map_subtract. rewrite present.
  apply value_subtract_self.
Qed.

Theorem unequal_overlap_is_exactly_a_conflict :
  forall key left right,
    left <> right ->
    map_join
      (fun candidate => if key_eq_dec candidate key then Present left else Absent)
      (fun candidate => if key_eq_dec candidate key then Present right else Absent)
      key = Conflict.
Proof.
  intros key left right different.
  unfold map_join.
  destruct (key_eq_dec key key); [|congruence].
  simpl. destruct (value_eq_dec left right); congruence.
Qed.

Theorem unequal_meet_overlap_is_exactly_a_conflict :
  forall key left right,
    left <> right ->
    map_meet
      (fun candidate => if key_eq_dec candidate key then Present left else Absent)
      (fun candidate => if key_eq_dec candidate key then Present right else Absent)
      key = Conflict.
Proof.
  intros key left right different.
  unfold map_meet.
  destruct (key_eq_dec key key); [|congruence].
  simpl. destruct (value_eq_dec left right); congruence.
Qed.

Theorem subtract_overlap_is_value_independent_key_mask :
  forall key left right,
    map_subtract
      (fun candidate => if key_eq_dec candidate key then Present left else Absent)
      (fun candidate => if key_eq_dec candidate key then Present right else Absent)
      key = Absent.
Proof.
  intros key left right.
  unfold map_subtract.
  destruct (key_eq_dec key key); [|congruence].
  reflexivity.
Qed.

End PointwiseTrie.

Section SetTrie.

Context {Key : Type}.

Definition set_trie := Key -> bool.
Definition set_empty : set_trie := fun _ => false.
Definition set_join (left right : set_trie) : set_trie :=
  fun key => left key || right key.
Definition set_meet (left right : set_trie) : set_trie :=
  fun key => left key && right key.
Definition set_subtract (left right : set_trie) : set_trie :=
  fun key => left key && negb (right key).

Theorem set_join_commutative : forall left right, set_join left right = set_join right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  apply orb_comm.
Qed.

Theorem set_join_associative :
  forall first second third,
    set_join first (set_join second third) = set_join (set_join first second) third.
Proof.
  intros first second third. apply functional_extensionality. intro key.
  apply orb_assoc.
Qed.

Theorem set_join_idempotent : forall set, set_join set set = set.
Proof.
  intro set. apply functional_extensionality. intro key.
  apply orb_diag.
Qed.

Theorem set_join_empty_identity :
  forall set, set_join set_empty set = set /\ set_join set set_empty = set.
Proof.
  intro set. split; apply functional_extensionality; intro key;
    unfold set_join, set_empty; destruct (set key); reflexivity.
Qed.

Theorem set_meet_commutative : forall left right, set_meet left right = set_meet right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  apply andb_comm.
Qed.

Theorem set_meet_associative :
  forall first second third,
    set_meet first (set_meet second third) = set_meet (set_meet first second) third.
Proof.
  intros first second third. apply functional_extensionality. intro key.
  apply andb_assoc.
Qed.

Theorem set_meet_idempotent : forall set, set_meet set set = set.
Proof.
  intro set. apply functional_extensionality. intro key.
  apply andb_diag.
Qed.

Theorem set_meet_empty_absorbing :
  forall set, set_meet set_empty set = set_empty /\ set_meet set set_empty = set_empty.
Proof.
  intro set. split; apply functional_extensionality; intro key;
    unfold set_meet, set_empty; destruct (set key); reflexivity.
Qed.

Theorem set_subtract_removes_members :
  forall left right key, right key = true -> set_subtract left right key = false.
Proof.
  intros left right key present. unfold set_subtract.
  rewrite present. destruct (left key); reflexivity.
Qed.

Theorem set_subtract_empty_right : forall set, set_subtract set set_empty = set.
Proof.
  intro set. apply functional_extensionality. intro key.
  unfold set_subtract, set_empty. destruct (set key); reflexivity.
Qed.

Theorem set_subtract_self_is_empty : forall set, set_subtract set set = set_empty.
Proof.
  intro set. apply functional_extensionality. intro key.
  unfold set_subtract, set_empty. destruct (set key); reflexivity.
Qed.

End SetTrie.

Inductive mode := Neutral | SetMode | MapMode.

Inductive mode_join_result := ModeResult (result : mode) | ModeMismatch.

Definition join_mode (left right : mode) : mode_join_result :=
  match left, right with
  | Neutral, other | other, Neutral => ModeResult other
  | SetMode, SetMode => ModeResult SetMode
  | MapMode, MapMode => ModeResult MapMode
  | _, _ => ModeMismatch
  end.

Definition meet_mode (left right : mode) : mode_join_result :=
  match left, right with
  | Neutral, _ | _, Neutral => ModeResult Neutral
  | SetMode, SetMode => ModeResult SetMode
  | MapMode, MapMode => ModeResult MapMode
  | _, _ => ModeMismatch
  end.

Definition subtract_mode (left right : mode) : mode_join_result :=
  match left, right with
  | Neutral, _ => ModeResult Neutral
  | other, Neutral => ModeResult other
  | SetMode, SetMode => ModeResult SetMode
  | MapMode, MapMode => ModeResult MapMode
  | _, _ => ModeMismatch
  end.

Definition restrict_mode (left right : mode) : mode_join_result := meet_mode left right.

Theorem neutral_empty_is_a_two_sided_identity :
  forall current,
    join_mode Neutral current = ModeResult current /\
    join_mode current Neutral = ModeResult current.
Proof. destruct current; split; reflexivity. Qed.

Theorem mixed_nonempty_membership_is_rejected :
  join_mode SetMode MapMode = ModeMismatch /\
  join_mode MapMode SetMode = ModeMismatch.
Proof. split; reflexivity. Qed.

Theorem neutral_empty_is_meet_and_restrict_absorbing :
  forall current,
    meet_mode Neutral current = ModeResult Neutral /\
    meet_mode current Neutral = ModeResult Neutral /\
    restrict_mode Neutral current = ModeResult Neutral /\
    restrict_mode current Neutral = ModeResult Neutral.
Proof. destruct current; repeat split; reflexivity. Qed.

Theorem neutral_empty_is_subtract_left_zero_and_right_identity :
  forall current,
    subtract_mode Neutral current = ModeResult Neutral /\
    subtract_mode current Neutral = ModeResult current.
Proof. destruct current; split; reflexivity. Qed.

Theorem every_binary_operation_rejects_mixed_nonempty_modes :
  meet_mode SetMode MapMode = ModeMismatch /\
  meet_mode MapMode SetMode = ModeMismatch /\
  subtract_mode SetMode MapMode = ModeMismatch /\
  subtract_mode MapMode SetMode = ModeMismatch /\
  restrict_mode SetMode MapMode = ModeMismatch /\
  restrict_mode MapMode SetMode = ModeMismatch.
Proof. repeat split; reflexivity. Qed.

Definition restrict_by_prefix {KeyByte : Type}
  (base selectors : list KeyByte -> bool) : list KeyByte -> bool :=
  fun path => base path &&
    existsb (fun prefix_length => selectors (firstn prefix_length path))
      (seq 0 (S (length path))).

Theorem prefix_restriction_never_invents_members :
  forall (KeyByte : Type) (base selectors : list KeyByte -> bool) path,
    restrict_by_prefix base selectors path = true -> base path = true.
Proof.
  intros KeyByte base selectors path retained.
  unfold restrict_by_prefix in retained.
  apply andb_true_iff in retained. exact (proj1 retained).
Qed.

Theorem prefix_restriction_of_empty_base_is_empty :
  forall (KeyByte : Type) (selectors : list KeyByte -> bool) path,
    restrict_by_prefix (fun _ => false) selectors path = false.
Proof. reflexivity. Qed.

Definition passes_through {KeyByte : Type}
  (right : list KeyByte -> bool) (key : list KeyByte) : Prop :=
  exists byte suffix, right (key ++ byte :: suffix) = true.

Definition keeps_members_under_longer_right_keys {KeyByte : Type}
  (member : (list KeyByte -> bool) -> (list KeyByte -> bool) -> list KeyByte -> Prop) : Prop :=
  forall left right key,
    left key = true -> right key = false -> passes_through right key ->
    member left right key.

Definition passthrough_dropping_member {KeyByte : Type}
  (left right : list KeyByte -> bool) (key : list KeyByte) : Prop :=
  set_subtract left right key = true /\ ~ passes_through right key.

Theorem set_subtract_keeps_members_under_longer_right_keys :
  forall KeyByte : Type,
    @keeps_members_under_longer_right_keys KeyByte
      (fun left right key => set_subtract left right key = true).
Proof.
  intros KeyByte left right key member absent _.
  cbv beta. unfold set_subtract. rewrite member, absent. reflexivity.
Qed.

Definition prefix_overlap_left (key : list nat) : bool :=
  match key with
  | [3; 2] | [3; 4] | [3; 6] | [3; 2; 3; 4; 0] => true
  | _ => false
  end.

Definition prefix_overlap_right (key : list nat) : bool :=
  match key with
  | [3; 2; 3; 4; 0] => true
  | _ => false
  end.

Theorem prefix_overlap_witness_keeps_the_bare_member :
  set_subtract prefix_overlap_left prefix_overlap_right [3; 2] = true /\
  set_subtract prefix_overlap_left prefix_overlap_right [3; 4] = true /\
  set_subtract prefix_overlap_left prefix_overlap_right [3; 6] = true /\
  set_subtract prefix_overlap_left prefix_overlap_right [3; 2; 3; 4; 0] = false /\
  passes_through prefix_overlap_right [3; 2].
Proof.
  split; [reflexivity|]. split; [reflexivity|]. split; [reflexivity|].
  split; [reflexivity|].
  exists 3, [4; 0]. reflexivity.
Qed.

Theorem passthrough_dropping_subtract_violates_member_survival :
  ~ passthrough_dropping_member prefix_overlap_left prefix_overlap_right [3; 2] /\
  ~ @keeps_members_under_longer_right_keys nat passthrough_dropping_member.
Proof.
  assert (through : passes_through prefix_overlap_right [3; 2])
    by (exists 3, [4; 0]; reflexivity).
  split.
  - intros [_ dropped]. exact (dropped through).
  - intro survives.
    destruct (survives prefix_overlap_left prefix_overlap_right [3; 2] eq_refl eq_refl through)
      as [_ dropped].
    exact (dropped through).
Qed.

Definition keeps_members_whose_key_the_selector_holds {KeyByte : Type}
  (member : (list KeyByte -> bool) -> (list KeyByte -> bool) -> list KeyByte -> Prop) : Prop :=
  forall base selectors key,
    base key = true -> selectors key = true -> passes_through selectors key ->
    member base selectors key.

Definition child_first_restricted_member {KeyByte : Type}
  (base selectors : list KeyByte -> bool) (key : list KeyByte) : Prop :=
  restrict_by_prefix base selectors key = true /\ ~ passes_through selectors key.

Theorem restrict_by_prefix_keeps_members_whose_key_the_selector_holds :
  forall KeyByte : Type,
    @keeps_members_whose_key_the_selector_holds KeyByte
      (fun base selectors key => restrict_by_prefix base selectors key = true).
Proof.
  intros KeyByte base selectors key member held _.
  cbv beta. unfold restrict_by_prefix. rewrite member, andb_true_l.
  apply existsb_exists. exists (length key). split.
  - apply in_seq. simpl. split; [apply Nat.le_0_l | apply Nat.lt_succ_diag_r].
  - cbv beta. rewrite firstn_all. exact held.
Qed.

Definition restriction_witness_base (key : list nat) : bool :=
  match key with
  | [3; 2] => true
  | _ => false
  end.

Theorem restriction_witness_keeps_the_bare_member :
  restrict_by_prefix restriction_witness_base prefix_overlap_left [3; 2] = true /\
  passes_through prefix_overlap_left [3; 2].
Proof.
  split; [reflexivity|].
  exists 3, [4; 0]. reflexivity.
Qed.

Theorem child_first_restriction_violates_member_survival :
  ~ child_first_restricted_member restriction_witness_base prefix_overlap_left [3; 2] /\
  ~ @keeps_members_whose_key_the_selector_holds nat child_first_restricted_member.
Proof.
  assert (through : passes_through prefix_overlap_left [3; 2])
    by (exists 3, [4; 0]; reflexivity).
  split.
  - intros [_ dropped]. exact (dropped through).
  - intro survives.
    destruct (survives restriction_witness_base prefix_overlap_left [3; 2] eq_refl eq_refl through)
      as [_ dropped].
    exact (dropped through).
Qed.

Definition value_free_paths_hold_no_member {KeyByte : Type}
  (member endpoint : list KeyByte -> bool) : Prop :=
  forall key, member key = true -> endpoint key = false.

Definition set_subtract_endpoints {KeyByte : Type}
  (left_endpoint right_reaches : list KeyByte -> bool) : list KeyByte -> bool :=
  fun key => left_endpoint key && negb (right_reaches key).

Definition leaves_no_path_where_a_member_was_removed {KeyByte : Type}
  (endpoints : (list KeyByte -> bool) -> (list KeyByte -> bool) ->
    (list KeyByte -> bool) -> (list KeyByte -> bool) -> list KeyByte -> bool) : Prop :=
  forall left_member left_endpoint right_member right_reaches key,
    value_free_paths_hold_no_member left_member left_endpoint ->
    left_member key = true -> right_member key = true ->
    endpoints left_member left_endpoint right_member right_reaches key = false.

Definition location_keeping_subtract_endpoints {KeyByte : Type}
  (left_member left_endpoint right_member right_reaches : list KeyByte -> bool)
  : list KeyByte -> bool :=
  fun key => set_subtract_endpoints left_endpoint right_reaches key
    || (left_member key && right_member key).

Theorem set_subtract_leaves_no_path_where_a_member_was_removed :
  forall KeyByte : Type,
    @leaves_no_path_where_a_member_was_removed KeyByte
      (fun _ left_endpoint _ right_reaches => set_subtract_endpoints left_endpoint right_reaches).
Proof.
  intros KeyByte left_member left_endpoint right_member right_reaches key value_free member _.
  cbv beta. unfold set_subtract_endpoints. rewrite (value_free key member). reflexivity.
Qed.

Theorem removed_member_witness_leaves_no_path :
  set_subtract prefix_overlap_left prefix_overlap_left [3; 6] = false /\
  set_subtract_endpoints (fun _ => false) prefix_overlap_left [3; 6] = false.
Proof. split; reflexivity. Qed.

Theorem location_keeping_subtract_violates_path_removal :
  location_keeping_subtract_endpoints prefix_overlap_left (fun _ => false)
    prefix_overlap_left prefix_overlap_left [3; 6] = true /\
  ~ @leaves_no_path_where_a_member_was_removed nat location_keeping_subtract_endpoints.
Proof.
  split; [reflexivity|].
  intro removes.
  assert (value_free :
    value_free_paths_hold_no_member prefix_overlap_left (fun _ : list nat => false))
    by (intros key _; reflexivity).
  pose proof (removes prefix_overlap_left (fun _ => false) prefix_overlap_left
    prefix_overlap_left [3; 6] value_free eq_refl eq_refl) as kept.
  cbv in kept. discriminate kept.
Qed.

Print Assumptions set_subtract_keeps_members_under_longer_right_keys.
Print Assumptions prefix_overlap_witness_keeps_the_bare_member.
Print Assumptions passthrough_dropping_subtract_violates_member_survival.
Print Assumptions restrict_by_prefix_keeps_members_whose_key_the_selector_holds.
Print Assumptions restriction_witness_keeps_the_bare_member.
Print Assumptions child_first_restriction_violates_member_survival.
Print Assumptions set_subtract_leaves_no_path_where_a_member_was_removed.
Print Assumptions removed_member_witness_leaves_no_path.
Print Assumptions location_keeping_subtract_violates_path_removal.
