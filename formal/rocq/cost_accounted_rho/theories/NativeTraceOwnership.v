From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Inductive trace_completion := TraceStored | TraceMatched | TraceRejected.
Inductive logical_event := Introduction (source : list nat) | Communication (source : list nat).

Record trace_row := {
  row_introduction : list nat;
  row_comm : list nat;
  row_completion : trace_completion
}.

Definition row_events row :=
  match row_completion row with
  | TraceStored => [Introduction (row_introduction row)]
  | TraceMatched => [Introduction (row_introduction row); Communication (row_comm row)]
  | TraceRejected => []
  end.

Definition projected_trace rows := flat_map row_events rows.
Definition trace_accepts rows trace := trace = projected_trace rows.
Definition row_start rows index := length (projected_trace (firstn index rows)).

Definition logical_event_eq_dec : forall a b : logical_event, {a = b} + {a <> b}.
Proof. decide equality; apply list_eq_dec, Nat.eq_dec. Defined.

Fixpoint consume_events expected actual :=
  match expected, actual with
  | [], _ => Some actual
  | e :: es, a :: rest =>
      if logical_event_eq_dec e a then consume_events es rest else None
  | _, [] => None
  end.

Fixpoint consume_rows rows actual :=
  match rows with
  | [] => Some actual
  | row :: rest =>
      match consume_events (row_events row) actual with
      | Some suffix => consume_rows rest suffix
      | None => None
      end
  end.

Definition check_trace rows actual :=
  match consume_rows rows actual with Some [] => true | _ => false end.

Theorem trace_event_parser_exact : forall expected actual suffix,
  consume_events expected actual = Some suffix <-> actual = expected ++ suffix.
Proof.
  induction expected as [|e es IH]; intros actual suffix; simpl.
  - split; intros H; congruence.
  - destruct actual as [|a rest]; simpl.
    + split; discriminate.
    + destruct (logical_event_eq_dec e a) as [same|different].
      * subst. rewrite IH. split; intros H; congruence.
      * split; intros H; [discriminate|]. inversion H. subst. contradiction.
Qed.

Theorem trace_row_parser_exact : forall rows actual suffix,
  consume_rows rows actual = Some suffix <-> actual = projected_trace rows ++ suffix.
Proof.
  induction rows as [|row rows IH]; intros actual suffix; simpl.
  - split; intros H; congruence.
  - destruct (consume_events (row_events row) actual) as [tail|] eqn:parsed.
    + apply trace_event_parser_exact in parsed. rewrite IH. subst actual.
      change (tail = projected_trace rows ++ suffix <->
        row_events row ++ tail = (row_events row ++ projected_trace rows) ++ suffix).
      rewrite <- app_assoc. symmetry. apply app_inv_head_iff.
    + split; [discriminate|]. intros equal.
      change (actual = (row_events row ++ projected_trace rows) ++ suffix) in equal.
      rewrite <- app_assoc in equal.
      apply trace_event_parser_exact in equal. congruence.
Qed.

Theorem trace_checker_sound_and_complete : forall rows actual,
  check_trace rows actual = true <-> trace_accepts rows actual.
Proof.
  intros rows actual. unfold check_trace, trace_accepts.
  destruct (consume_rows rows actual) as [[|event rest]|] eqn:parsed.
  - apply trace_row_parser_exact in parsed. rewrite app_nil_r in parsed.
    split; auto.
  - split; [discriminate|]. intros equal. subst actual.
    assert (consume_rows rows (projected_trace rows) = Some []).
    { apply trace_row_parser_exact. rewrite app_nil_r. reflexivity. }
    congruence.
  - split; [discriminate|]. intros equal. subst actual.
    assert (consume_rows rows (projected_trace rows) = Some []).
    { apply trace_row_parser_exact. rewrite app_nil_r. reflexivity. }
    congruence.
Qed.

Theorem trace_projection_append : forall left right,
  projected_trace (left ++ right) = projected_trace left ++ projected_trace right.
Proof. intros. apply flat_map_app. Qed.

Theorem trace_rejection_has_no_committed_introduction : forall intro comm,
  row_events {| row_introduction := intro; row_comm := comm;
                row_completion := TraceRejected |} = [].
Proof. reflexivity. Qed.

Theorem trace_row_width_bound : forall row, length (row_events row) <= 2.
Proof. intros [intro comm []]; simpl; lia. Qed.

Theorem trace_exact_coverage : forall rows trace,
  trace_accepts rows trace -> length trace = length (projected_trace rows).
Proof. intros rows trace ->. reflexivity. Qed.

Theorem trace_rejects_extra_event : forall rows trace event,
  trace_accepts rows trace -> ~ trace_accepts rows (trace ++ [event]).
Proof.
  intros rows trace event accepted extra.
  apply trace_exact_coverage in accepted, extra. rewrite length_app in extra. simpl in extra. lia.
Qed.

Theorem trace_rejects_missing_event : forall rows left event right,
  trace_accepts rows (left ++ event :: right) ->
  ~ trace_accepts rows (left ++ right).
Proof.
  intros rows left event right accepted missing.
  apply trace_exact_coverage in accepted, missing.
  repeat rewrite length_app in *. simpl in accepted. lia.
Qed.

Theorem trace_sources_are_exact : forall rows trace index event,
  trace_accepts rows trace -> nth_error trace index = Some event ->
  nth_error (projected_trace rows) index = Some event.
Proof. intros rows trace index event ->. auto. Qed.

Theorem trace_row_local_offset : forall prefix row suffix offset event,
  nth_error (row_events row) offset = Some event ->
  nth_error (projected_trace (prefix ++ row :: suffix))
    (length (projected_trace prefix) + offset) = Some event.
Proof.
  intros prefix row suffix offset event found.
  rewrite trace_projection_append. unfold projected_trace at 2. simpl.
  rewrite nth_error_app2 by lia. replace
    (length (projected_trace prefix) + offset - length (projected_trace prefix))
    with offset by lia.
  rewrite nth_error_app1. exact found.
  apply nth_error_Some. rewrite found. discriminate.
Qed.

Theorem trace_distinct_rows_have_disjoint_offsets : forall prefix left middle right x y,
  x < length (row_events left) -> y < length (row_events right) ->
  length (projected_trace prefix) + x <
    length (projected_trace (prefix ++ left :: middle)) + y.
Proof.
  intros prefix left middle right x y hx hy.
  rewrite trace_projection_append, length_app. unfold projected_trace. simpl.
  rewrite length_app. lia.
Qed.

Inductive occurrence_state := Available | Reserved | Completed.

Definition reserve_occurrence state :=
  match state with Available => Some Reserved | _ => None end.

Definition finish_occurrence state (authenticated : bool) :=
  match state with
  | Reserved => Some (if authenticated then Completed else Available)
  | _ => None
  end.

Theorem trace_reservation_is_exclusive : forall state,
  reserve_occurrence state = Some Reserved -> state = Available.
Proof. intros []; simpl; intros H; try discriminate; reflexivity. Qed.

Theorem trace_completed_cannot_reserve : reserve_occurrence Completed = None.
Proof. reflexivity. Qed.

Theorem trace_failure_releases_only_reservation :
  finish_occurrence Reserved false = Some Available.
Proof. reflexivity. Qed.

Definition update_occurrence (states : nat -> occurrence_state) index state :=
  fun key => if Nat.eqb key index then state else states key.

Theorem trace_independent_occurrences_commute : forall states a b sa sb,
  a <> b -> forall key,
  update_occurrence (update_occurrence states a sa) b sb key =
  update_occurrence (update_occurrence states b sb) a sa key.
Proof.
  intros states a b sa sb distinct key. unfold update_occurrence.
  destruct (key =? a) eqn:ha; destruct (key =? b) eqn:hb; auto.
  apply Nat.eqb_eq in ha, hb. subst. contradiction.
Qed.
