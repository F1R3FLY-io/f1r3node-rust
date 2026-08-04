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
