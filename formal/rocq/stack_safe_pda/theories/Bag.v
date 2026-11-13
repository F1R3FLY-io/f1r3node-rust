From Stdlib Require Import Arith PeanoNat Lia FunctionalExtensionality Bool.Bool.
From Stdlib Require Import Lists.List Sorting.Permutation.

Set Implicit Arguments.
Import ListNotations.

(** An exact-key Bag is a finite-support function in the implementation.  The
    algebraic laws do not depend on the concrete finite-support carrier, so the
    proof model uses a total function whose zero cells are absent.  PathMap's
    prefix relation never appears in these definitions: different byte paths
    denote independent cells. *)
Section ExactKeyBag.

Context {Key : Type}.
Variable key_eq_dec : forall left right : Key, {left = right} + {left <> right}.

Definition bag := Key -> nat.
Definition empty : bag := fun _ => 0.
Definition sum (left right : bag) : bag := fun key => left key + right key.
Definition join (left right : bag) : bag := fun key => Nat.max (left key) (right key).
Definition meet (left right : bag) : bag := fun key => Nat.min (left key) (right key).
Definition subtract (left right : bag) : bag := fun key => left key - right key.
Definition subbag (left right : bag) : Prop := forall key, left key <= right key.

Definition set_count (selected : Key) (count : nat) (subject : bag) : bag :=
  fun candidate => if key_eq_dec candidate selected then count else subject candidate.

Theorem set_count_is_exact :
  forall subject selected other count,
    other <> selected -> set_count selected count subject other = subject other.
Proof.
  intros subject selected other count different.
  unfold set_count. destruct (key_eq_dec other selected); congruence.
Qed.

Theorem sum_empty_identity :
  forall subject, sum empty subject = subject /\ sum subject empty = subject.
Proof.
  intro subject. split; apply functional_extensionality; intro key;
    unfold sum, empty; lia.
Qed.

Theorem sum_commutative : forall left right, sum left right = sum right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold sum. lia.
Qed.

Theorem sum_associative :
  forall first second third,
    sum (sum first second) third = sum first (sum second third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold sum. lia.
Qed.

Theorem join_commutative : forall left right, join left right = join right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold join. apply Nat.max_comm.
Qed.

Theorem join_associative :
  forall first second third,
    join (join first second) third = join first (join second third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold join. symmetry. apply Nat.max_assoc.
Qed.

Theorem join_idempotent : forall subject, join subject subject = subject.
Proof.
  intro subject. apply functional_extensionality. intro key.
  unfold join. apply Nat.max_id.
Qed.

Theorem join_empty_identity :
  forall subject, join empty subject = subject /\ join subject empty = subject.
Proof.
  intro subject. split; apply functional_extensionality; intro key;
    unfold join, empty; lia.
Qed.

Theorem meet_commutative : forall left right, meet left right = meet right left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold meet. apply Nat.min_comm.
Qed.

Theorem meet_associative :
  forall first second third,
    meet (meet first second) third = meet first (meet second third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold meet. symmetry. apply Nat.min_assoc.
Qed.

Theorem meet_idempotent : forall subject, meet subject subject = subject.
Proof.
  intro subject. apply functional_extensionality. intro key.
  unfold meet. apply Nat.min_id.
Qed.

Theorem meet_empty_absorbing :
  forall subject, meet empty subject = empty /\ meet subject empty = empty.
Proof.
  intro subject. split; apply functional_extensionality; intro key;
    unfold meet, empty; lia.
Qed.

Theorem join_absorbs_meet :
  forall left right, join left (meet left right) = left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.min_max_absorption.
Qed.

Theorem meet_absorbs_join :
  forall left right, meet left (join left right) = left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.max_min_absorption.
Qed.

Theorem meet_distributes_over_join :
  forall first second third,
    meet first (join second third) =
    join (meet first second) (meet first third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.min_max_distr.
Qed.

Theorem join_distributes_over_meet :
  forall first second third,
    join first (meet second third) =
    meet (join first second) (join first third).
Proof.
  intros first second third. apply functional_extensionality. intro key.
  unfold join, meet. apply Nat.max_min_distr.
Qed.

Theorem subtraction_conservation :
  forall left right, sum (subtract left right) (meet left right) = left.
Proof.
  intros left right. apply functional_extensionality. intro key.
  unfold sum, subtract, meet.
  destruct (Nat.le_ge_cases (right key) (left key)).
  - rewrite Nat.min_r by assumption. lia.
  - rewrite Nat.min_l by assumption. lia.
Qed.

Theorem subtract_self_empty : forall subject, subtract subject subject = empty.
Proof.
  intro subject. apply functional_extensionality. intro key.
  unfold subtract, empty. lia.
Qed.

Theorem meet_is_subbag_left : forall left right, subbag (meet left right) left.
Proof.
  intros left right key. unfold meet. apply Nat.le_min_l.
Qed.

Theorem meet_is_subbag_right : forall left right, subbag (meet left right) right.
Proof.
  intros left right key. unfold meet. apply Nat.le_min_r.
Qed.

Inductive cursor_algebra := CursorJoin | CursorMeet | CursorSubtract | CursorSum.

Definition cursor_count (operation : cursor_algebra) (left right : nat) : nat :=
  match operation with
  | CursorJoin => Nat.max left right
  | CursorMeet => Nat.min left right
  | CursorSubtract => left - right
  | CursorSum => left + right
  end.

Definition cursor_result (operation : cursor_algebra) (left right : bag) : bag :=
  fun key => cursor_count operation (left key) (right key).

Definition cursor_valid
  (operation : cursor_algebra) (left right output : bag) (visited : list Key) : Prop :=
  forall key,
    (In key visited -> output key = cursor_result operation left right key) /\
    (~ In key visited -> output key = 0).

Definition cursor_step
  (operation : cursor_algebra) (left right : bag)
  (visited : list Key) (output : bag) (key : Key) : list Key * bag :=
  (key :: visited,
   set_count key (cursor_result operation left right key) output).

Theorem cursor_step_preserves_every_other_key :
  forall operation left right visited output selected other,
    other <> selected ->
    snd (cursor_step operation left right visited output selected) other = output other.
Proof.
  intros operation left right visited output selected other different.
  unfold cursor_step; simpl.
  apply set_count_is_exact. exact different.
Qed.

Theorem empty_cursor_valid :
  forall operation left right, cursor_valid operation left right empty [].
Proof.
  intros operation left right key. split.
  - intro impossible. inversion impossible.
  - intro absent. reflexivity.
Qed.

Theorem cursor_step_preserves_exact_counts :
  forall operation left right visited output key,
    cursor_valid operation left right output visited ->
    ~ In key visited ->
    cursor_valid operation left right
      (snd (cursor_step operation left right visited output key))
      (fst (cursor_step operation left right visited output key)).
Proof.
  intros operation left right visited output key valid fresh candidate.
  unfold cursor_step; simpl. split.
  - intros [same | old].
    + subst candidate. unfold set_count.
      destruct (key_eq_dec key key); [reflexivity | contradiction].
    + unfold set_count. destruct (key_eq_dec candidate key) as [equal | different].
      * subst candidate. contradiction.
      * exact (proj1 (valid candidate) old).
  - intros absent. unfold set_count.
    destruct (key_eq_dec candidate key) as [equal | different].
    + subst candidate. exfalso. apply absent. left. reflexivity.
    + apply (proj2 (valid candidate)).
      intro old. apply absent. right. exact old.
Qed.

Fixpoint cursor_walk
  (operation : cursor_algebra) (left right : bag)
  (keys : list Key) (state : list Key * bag) : list Key * bag :=
  match keys with
  | [] => state
  | key :: rest =>
      cursor_walk operation left right rest
        (cursor_step operation left right (fst state) (snd state) key)
  end.

Theorem cursor_walk_preserves_exact_counts :
  forall operation left right keys visited output,
    NoDup keys ->
    (forall key, In key keys -> ~ In key visited) ->
    cursor_valid operation left right output visited ->
    cursor_valid operation left right
      (snd (cursor_walk operation left right keys (visited, output)))
      (fst (cursor_walk operation left right keys (visited, output))).
Proof.
  intros operation left right keys.
  induction keys as [|key rest IH]; intros visited output unique fresh valid.
  - exact valid.
  - inversion unique as [|? ? absent rest_unique]; subst.
    simpl. apply IH.
    + exact rest_unique.
    + intros candidate present [same | old].
      * subst candidate. apply absent. exact present.
      * exact (fresh candidate (or_intror present) old).
    + apply cursor_step_preserves_exact_counts.
      * exact valid.
      * apply (fresh key). left. reflexivity.
Qed.

Theorem cursor_finished_is_pointwise_algebra :
  forall operation left right visited output,
    cursor_valid operation left right output visited ->
    (forall key, left key <> 0 \/ right key <> 0 -> In key visited) ->
    output = cursor_result operation left right.
Proof.
  intros operation left right visited output valid complete.
  apply functional_extensionality. intro key.
  destruct (Nat.eq_dec (left key) 0) as [left_zero | left_nonzero];
    destruct (Nat.eq_dec (right key) 0) as [right_zero | right_nonzero].
  - destruct (in_dec key_eq_dec key visited) as [present | absent].
    + apply (proj1 (valid key)). exact present.
    + rewrite (proj2 (valid key) absent).
      unfold cursor_result, cursor_count. rewrite left_zero, right_zero.
      destruct operation; reflexivity.
  - apply (proj1 (valid key)). apply complete. right. exact right_nonzero.
  - apply (proj1 (valid key)). apply complete. left. exact left_nonzero.
  - apply (proj1 (valid key)). apply complete. left. exact left_nonzero.
Qed.

Theorem cursor_walk_preserves_visited :
  forall operation left right keys visited output key,
    In key visited ->
    In key (fst (cursor_walk operation left right keys (visited, output))).
Proof.
  intros operation left right keys.
  induction keys as [|head rest IH]; intros visited output key present.
  - exact present.
  - simpl. apply IH. right. exact present.
Qed.

Theorem cursor_walk_visits_input :
  forall operation left right keys visited output key,
    In key keys ->
    In key (fst (cursor_walk operation left right keys (visited, output))).
Proof.
  intros operation left right keys.
  induction keys as [|head rest IH]; intros visited output key present.
  - inversion present.
  - simpl. destruct present as [same | later].
    + subst key. apply cursor_walk_preserves_visited.
      left. reflexivity.
    + apply IH. exact later.
Qed.

Theorem cursor_walk_refines_pointwise_algebra :
  forall operation left right keys,
    NoDup keys ->
    (forall key, left key <> 0 \/ right key <> 0 -> In key keys) ->
    snd (cursor_walk operation left right keys ([], empty)) =
      cursor_result operation left right.
Proof.
  intros operation left right keys unique complete.
  apply cursor_finished_is_pointwise_algebra with
    (visited := fst (cursor_walk operation left right keys ([], empty))).
  - apply cursor_walk_preserves_exact_counts.
    + exact unique.
    + intros key present absent. inversion absent.
    + apply empty_cursor_valid.
  - intros key supported. apply cursor_walk_visits_input.
    apply complete. exact supported.
Qed.

Theorem cursor_join_refines_bag_join :
  forall left right, cursor_result CursorJoin left right = join left right.
Proof. reflexivity. Qed.

Theorem cursor_meet_refines_bag_meet :
  forall left right, cursor_result CursorMeet left right = meet left right.
Proof. reflexivity. Qed.

Theorem cursor_subtract_refines_bag_subtract :
  forall left right, cursor_result CursorSubtract left right = subtract left right.
Proof. reflexivity. Qed.

Theorem cursor_sum_refines_bag_sum :
  forall left right, cursor_result CursorSum left right = sum left right.
Proof. reflexivity. Qed.

Definition cursor_subbag_step
  (left right : bag) (accepted : bool) (key : Key) : bool :=
  andb accepted (Nat.leb (left key) (right key)).

Theorem cursor_subbag_step_refines_conjunction :
  forall left right accepted key,
    cursor_subbag_step left right accepted key = true <->
    accepted = true /\ left key <= right key.
Proof.
  intros left right accepted key.
  unfold cursor_subbag_step. rewrite andb_true_iff, Nat.leb_le.
  reflexivity.
Qed.

Definition cursor_subbag_valid
  (left right : bag) (visited : list Key) (accepted : bool) : Prop :=
  accepted = true <-> forall key, In key visited -> left key <= right key.

Theorem empty_subbag_cursor_valid :
  forall left right, cursor_subbag_valid left right [] true.
Proof.
  intros left right. split.
  - intros yes key impossible. inversion impossible.
  - reflexivity.
Qed.

Theorem cursor_subbag_step_preserves_validity :
  forall left right visited accepted key,
    cursor_subbag_valid left right visited accepted ->
    cursor_subbag_valid left right (key :: visited)
      (cursor_subbag_step left right accepted key).
Proof.
  intros left right visited accepted key valid.
  unfold cursor_subbag_valid in *.
  rewrite cursor_subbag_step_refines_conjunction.
  split.
  - intros [old current] candidate [same | prior].
    + subst candidate. exact current.
    + apply (proj1 valid old candidate prior).
  - intro bounded. split.
    + apply (proj2 valid). intros candidate prior.
      apply bounded. right. exact prior.
    + apply bounded. left. reflexivity.
Qed.

Theorem cursor_subbag_finished_is_exact :
  forall left right visited accepted,
    cursor_subbag_valid left right visited accepted ->
    (forall key, left key <> 0 -> In key visited) ->
    accepted = true <-> subbag left right.
Proof.
  intros left right visited accepted valid complete.
  split.
  - intros yes key. destruct (Nat.eq_dec (left key) 0) as [zero | nonzero].
    + rewrite zero. lia.
    + apply (proj1 valid yes). apply complete. exact nonzero.
  - intro bounded. apply (proj2 valid).
    intros key present. apply bounded.
Qed.

Fixpoint cursor_subbag_walk
  (left right : bag) (keys : list Key) (accepted : bool) : bool :=
  match keys with
  | [] => accepted
  | key :: rest =>
      cursor_subbag_walk left right rest
        (cursor_subbag_step left right accepted key)
  end.

Theorem cursor_subbag_walk_checks_every_key :
  forall left right keys accepted,
    cursor_subbag_walk left right keys accepted = true <->
      accepted = true /\ forall key, In key keys -> left key <= right key.
Proof.
  intros left right keys.
  induction keys as [|head rest IH]; intro accepted.
  - simpl. split.
    + intros yes. split; [exact yes |].
      intros key impossible. inversion impossible.
    + intros [yes bounded]. exact yes.
  - simpl. rewrite IH, cursor_subbag_step_refines_conjunction.
    split.
    + intros [[yes head_bounded] rest_bounded].
      split; [exact yes |].
      intros key [same | later].
      * subst key. exact head_bounded.
      * apply rest_bounded. exact later.
    + intros [yes bounded]. split.
      * split; [exact yes |].
        apply bounded. left. reflexivity.
      * intros key later. apply bounded. right. exact later.
Qed.

Theorem cursor_subbag_walk_refines_finite_support :
  forall left right keys,
    (forall key, left key <> 0 -> In key keys) ->
    cursor_subbag_walk left right keys true = true <-> subbag left right.
Proof.
  intros left right keys complete.
  rewrite cursor_subbag_walk_checks_every_key.
  split.
  - intros [_ bounded] key.
    destruct (Nat.eq_dec (left key) 0) as [zero | nonzero].
    + rewrite zero. lia.
    + apply bounded. apply complete. exact nonzero.
  - intro bounded. split.
    + reflexivity.
    + intros key present. apply bounded.
Qed.

Theorem cursor_count_respects_bound :
  forall operation left right maximum,
    operation <> CursorSum ->
    left <= maximum -> right <= maximum ->
    cursor_count operation left right <= maximum.
Proof.
  intros operation left right maximum not_sum left_bound right_bound.
  destruct operation; simpl.
  - lia.
  - lia.
  - lia.
  - exfalso. apply not_sum. reflexivity.
Qed.

(** A normalized delta has disjoint addition and removal support.  This is the
    invariant maintained by `BagDelta::record_change`. *)
Definition delta_normalized (additions removals : bag) : Prop :=
  forall key, additions key = 0 \/ removals key = 0.

Definition delta_applicable (subject removals : bag) : Prop :=
  forall key, removals key <= subject key.

Definition apply_delta (subject additions removals : bag) : bag :=
  sum (subtract subject removals) additions.

Theorem normalized_delta_reversible :
  forall subject additions removals,
    delta_normalized additions removals ->
    delta_applicable subject removals ->
    apply_delta (apply_delta subject additions removals) removals additions = subject.
Proof.
  intros subject additions removals normalized applicable.
  apply functional_extensionality. intro key.
  specialize (normalized key). specialize (applicable key).
  unfold apply_delta, sum, subtract.
  destruct normalized; lia.
Qed.

Definition delta_from_to_removals (source target : bag) : bag := subtract source target.
Definition delta_from_to_additions (source target : bag) : bag := subtract target source.

Theorem canonical_delta_is_normalized :
  forall source target,
    delta_normalized
      (delta_from_to_additions source target)
      (delta_from_to_removals source target).
Proof.
  intros source target key.
  unfold delta_from_to_additions, delta_from_to_removals, subtract.
  destruct (Nat.le_ge_cases (source key) (target key)); [right | left]; lia.
Qed.

Theorem canonical_delta_is_applicable :
  forall source target,
    delta_applicable source (delta_from_to_removals source target).
Proof.
  intros source target key.
  unfold delta_from_to_removals, subtract. lia.
Qed.

Theorem canonical_delta_reaches_target :
  forall source target,
    apply_delta
      source
      (delta_from_to_additions source target)
      (delta_from_to_removals source target) = target.
Proof.
  intros source target. apply functional_extensionality. intro key.
  unfold apply_delta, delta_from_to_additions, delta_from_to_removals,
    sum, subtract.
  lia.
Qed.

Corollary canonical_delta_round_trip :
  forall source target,
    apply_delta
      (apply_delta
        source
        (delta_from_to_additions source target)
        (delta_from_to_removals source target))
      (delta_from_to_removals source target)
      (delta_from_to_additions source target) = source.
Proof.
  intros source target.
  apply normalized_delta_reversible.
  - apply canonical_delta_is_normalized.
  - apply canonical_delta_is_applicable.
Qed.

(** The live RSpace commit boundary projects accepted matcher candidates into
    one normalized removal Bag before it publishes a new PathMap root.  This
    model deliberately retains only the fields that decide datum consumption:
    a candidate is stored or fresh, and persistent or nonpersistent. *)
Inductive selected_candidate : Type :=
| StoredCandidate : Key -> bool -> selected_candidate
| FreshCandidate : bool -> selected_candidate.

Definition candidate_removal (candidate : selected_candidate) : bag :=
  match candidate with
  | StoredCandidate key false => set_count key 1 empty
  | _ => empty
  end.

Fixpoint selected_removals (candidates : list selected_candidate) : bag :=
  match candidates with
  | [] => empty
  | candidate :: rest => sum (candidate_removal candidate) (selected_removals rest)
  end.

Theorem nonpersistent_stored_candidate_removes_one :
  forall key, candidate_removal (StoredCandidate key false) key = 1.
Proof.
  intro key. unfold candidate_removal, set_count.
  destruct (key_eq_dec key key); congruence.
Qed.

Theorem nonpersistent_stored_candidate_is_exact :
  forall selected other,
    other <> selected ->
    candidate_removal (StoredCandidate selected false) other = 0.
Proof.
  intros selected other different.
  unfold candidate_removal, set_count, empty.
  destruct (key_eq_dec other selected); congruence.
Qed.

Theorem persistent_stored_candidate_is_not_removed :
  forall key, candidate_removal (StoredCandidate key true) = empty.
Proof. reflexivity. Qed.

Theorem fresh_candidate_is_not_removed :
  forall persistent, candidate_removal (FreshCandidate persistent) = empty.
Proof. destruct persistent; reflexivity. Qed.

(** Candidate order is consensus-visible to matching, but once a COMM has been
    accepted its removal transaction is a multiset.  Any play/replay
    permutation of the same accepted candidates therefore produces the same
    exact-key delta. *)
Theorem selected_removals_permutation_invariant :
  forall play replay,
    Permutation play replay -> selected_removals play = selected_removals replay.
Proof.
  intros play replay permutation.
  induction permutation as
      [|candidate left right permutation IH
       |first second rest
       |left middle right first_permutation first_IH second_permutation second_IH].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
  - simpl. apply functional_extensionality. intro key.
    unfold sum. lia.
  - rewrite first_IH. exact second_IH.
Qed.

Definition selected_commit (subject : bag) (candidates : list selected_candidate) : bag :=
  subtract subject (selected_removals candidates).

Theorem selected_commit_exact_count :
  forall subject candidates key,
    selected_commit subject candidates key =
      subject key - selected_removals candidates key.
Proof. reflexivity. Qed.

Theorem permuted_play_replay_commits_equal :
  forall subject play replay,
    Permutation play replay ->
    delta_applicable subject (selected_removals play) ->
    selected_commit subject play = selected_commit subject replay.
Proof.
  intros subject play replay permutation _.
  unfold selected_commit.
  rewrite (selected_removals_permutation_invariant permutation).
  reflexivity.
Qed.

(** `BagDelta::apply_to` works on a private COW root.  The public store is
    replaced only on `Some`; a rejected transition returns `None`, so this
    publication function exposes the original Bag and never a partial prefix. *)
Definition publish_atomic (subject : bag) (proposal : option bag) : bag :=
  match proposal with
  | Some committed => committed
  | None => subject
  end.

Theorem rejected_commit_publishes_no_partial_state :
  forall subject, publish_atomic subject None = subject.
Proof. reflexivity. Qed.

Theorem accepted_commit_publishes_exact_proposal :
  forall subject committed, publish_atomic subject (Some committed) = committed.
Proof. reflexivity. Qed.

(** `Bag::sum` checks every endpoint of the smaller operand against the larger
    one before it writes anything.  [maximum] is the largest multiplicity
    ([u64::MAX] in the implementation) and [keys] lists the endpoints that the
    cursor walk visits.  One addition above the maximum refuses the whole sum. *)
Definition cursor_sum_fits (maximum : nat) (left right : bag) (keys : list Key) : bool :=
  forallb (fun key => Nat.leb (left key + right key) maximum) keys.

Definition checked_cursor_sum
  (maximum : nat) (left right : bag) (keys : list Key) : option bag :=
  if cursor_sum_fits maximum left right keys
  then Some (snd (cursor_walk CursorSum left right keys ([], empty)))
  else None.

Theorem overflowing_endpoint_publishes_nothing :
  forall maximum subject left right keys key,
    In key keys ->
    maximum < left key + right key ->
    publish_atomic subject (checked_cursor_sum maximum left right keys) = subject.
Proof.
  intros maximum subject left right keys key present overflow.
  unfold checked_cursor_sum.
  destruct (cursor_sum_fits maximum left right keys) eqn:fits.
  - exfalso.
    unfold cursor_sum_fits in fits.
    pose proof (proj1 (forallb_forall _ keys) fits key present) as bounded.
    cbv beta in bounded.
    apply Nat.leb_le in bounded.
    lia.
  - reflexivity.
Qed.

(** The positive half: when no endpoint overflows, the checked sum publishes
    the pointwise sum.  It is stated key by key, so that the proof needs no
    functional extensionality. *)
Theorem checked_cursor_sum_publishes_the_bounded_sum :
  forall maximum subject left right keys,
    NoDup keys ->
    (forall key, left key <> 0 \/ right key <> 0 -> In key keys) ->
    (forall key, In key keys -> left key + right key <= maximum) ->
    forall key,
      publish_atomic subject (checked_cursor_sum maximum left right keys) key =
        sum left right key.
Proof.
  intros maximum subject left right keys unique complete bounded key.
  assert (fits : cursor_sum_fits maximum left right keys = true).
  { unfold cursor_sum_fits. apply forallb_forall.
    intros candidate present. cbv beta. apply Nat.leb_le.
    apply bounded. exact present. }
  unfold checked_cursor_sum. rewrite fits. cbn [publish_atomic].
  assert (valid :
    cursor_valid CursorSum left right
      (snd (cursor_walk CursorSum left right keys ([], empty)))
      (fst (cursor_walk CursorSum left right keys ([], empty)))).
  { apply cursor_walk_preserves_exact_counts.
    - exact unique.
    - intros candidate present absent. inversion absent.
    - apply empty_cursor_valid. }
  destruct (in_dec key_eq_dec key (fst (cursor_walk CursorSum left right keys ([], empty))))
    as [visited | unvisited].
  - rewrite (proj1 (valid key) visited). reflexivity.
  - rewrite (proj2 (valid key) unvisited).
    assert (absent : ~ In key keys).
    { intro present. apply unvisited. apply cursor_walk_visits_input. exact present. }
    destruct (Nat.eq_dec (left key) 0) as [left_zero | left_nonzero].
    + destruct (Nat.eq_dec (right key) 0) as [right_zero | right_nonzero].
      * unfold sum. rewrite left_zero, right_zero. reflexivity.
      * exfalso. apply absent. apply complete. right. exact right_nonzero.
    + exfalso. apply absent. apply complete. left. exact left_nonzero.
Qed.

(** Negative control: a sum that wraps each addition modulo [S maximum], and so
    publishes even when an endpoint overflows, breaks the atomicity property
    above.  The witness overflows at the one key it visits. *)
Definition wrapping_cursor_sum
  (maximum : nat) (left right : bag) (keys : list Key) : option bag :=
  Some (fun key => (left key + right key) mod (S maximum)).

Theorem wrapping_cursor_sum_violates_atomicity :
  forall key : Key,
    exists maximum subject left right,
      In key [key] /\
      maximum < left key + right key /\
      publish_atomic subject (wrapping_cursor_sum maximum left right [key]) <> subject.
Proof.
  intro key.
  exists 1, (fun _ => 1), (fun _ => 1), (fun _ => 1).
  split; [left; reflexivity |].
  split; [simpl; lia |].
  unfold publish_atomic, wrapping_cursor_sum. intro published.
  apply (f_equal (fun counts : bag => counts key)) in published.
  cbv in published. discriminate published.
Qed.

End ExactKeyBag.
