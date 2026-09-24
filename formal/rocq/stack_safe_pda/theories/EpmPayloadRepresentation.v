From Stdlib Require Import List Arith Bool Lia.
Import ListNotations.
Set Implicit Arguments.

Module EpmPayloadRepresentation.

Definition bindings (V : Type) := list (nat * V).
Definition erase {V} key (items : bindings V) : bindings V :=
  filter (fun item => negb (Nat.eqb (fst item) key)) items.
Definition put {V} key (value : V) items := (key, value) :: erase key items.
Definition denote {V W} (f : V -> W) (items : bindings V) : bindings W :=
  map (fun item => (fst item, f (snd item))) items.

Record state (V : Type) := State {
  topology : list nat;
  contents : bindings V;
  failure : option nat
}.
Arguments State {V}.
Arguments topology {V}.
Arguments contents {V}.
Arguments failure {V}.

Inductive action (V : Type) :=
| Install (key : nat) (value : V)
| Remove (key : nat)
| Refuse (code : nat).
Arguments Install {V}.
Arguments Remove {V}.
Arguments Refuse {V}.

Definition step {V} (current : state V) (op : action V) : state V :=
  match failure current with
  | Some _ => current
  | None => match op with
    | Install key value => State (topology current) (put key value (contents current)) None
    | Remove key => State (topology current) (erase key (contents current)) None
    | Refuse code => State (topology current) [] (Some code)
    end
  end.

Fixpoint run {V} (ops : list (action V)) (current : state V) :=
  match ops with [] => current | op :: tail => run tail (step current op) end.

Definition map_action {V W} (f : V -> W) (op : action V) : action W :=
  match op with
  | Install key value => Install key (f value)
  | Remove key => Remove key
  | Refuse code => Refuse code
  end.
Definition map_state {V W} (f : V -> W) (current : state V) : state W :=
  State (topology current) (denote f (contents current)) (failure current).
Definition publish {V} (current : state V) : option (bindings V) :=
  match failure current with None => Some (contents current) | Some _ => None end.

Theorem erase_commutes_with_denotation {V W} (f : V -> W) key items :
  denote f (erase key items) = erase key (denote f items).
Proof.
  unfold denote, erase.
  induction items as [|[k v] tail IH]; cbn; [reflexivity|].
  destruct (negb (k =? key)); cbn; now rewrite IH.
Qed.

Theorem install_commutes_with_denotation {V W} (f : V -> W) key value items :
  denote f (put key value items) = put key (f value) (denote f items).
Proof.
  change ((key, f value) :: denote f (erase key items) =
    (key, f value) :: erase key (denote f items)).
  now rewrite erase_commutes_with_denotation.
Qed.

Local Opaque denote erase put.

Theorem each_writer_step_commutes {V W} (f : V -> W) current op :
  map_state f (step current op) = step (map_state f current) (map_action f op).
Proof.
  destruct current as [shape items error]; destruct error; [reflexivity|].
  destruct op; cbn; unfold map_state; cbn;
    try rewrite install_commutes_with_denotation;
    try rewrite erase_commutes_with_denotation; reflexivity.
Qed.

Theorem writer_trace_commutes {V W} (f : V -> W) ops current :
  map_state f (run ops current) = run (map (map_action f) ops) (map_state f current).
Proof.
  revert current. induction ops as [|op tail IH]; intros current; cbn; [reflexivity|].
  rewrite IH, each_writer_step_commutes. reflexivity.
Qed.

Theorem publication_commutes {V W} (f : V -> W) current :
  publish (map_state f current) = option_map (denote f) (publish current).
Proof. destruct current as [shape items [error|]]; reflexivity. Qed.

Theorem operations_preserve_topology {V} ops (current : state V) :
  topology (run ops current) = topology current.
Proof.
  revert current. induction ops as [|op tail IH]; intros current; cbn; [reflexivity|].
  rewrite IH. destruct current as [shape items [error|]]; [reflexivity|].
  destruct op; reflexivity.
Qed.

Theorem refusal_cannot_publish {V} ops shape (items : bindings V) code :
  publish (run ops (State shape items (Some code))) = None.
Proof.
  induction ops as [|op tail IH]; cbn; [reflexivity|]. exact IH.
Qed.

Local Transparent denote erase put.

Lemma erase_absent {V} key (items : bindings V) :
  Forall (fun item => fst item <> key) items -> erase key items = items.
Proof.
  intro absent. induction absent as [|[k v] tail different absent IH]; [reflexivity|].
  cbn in different. apply Nat.eqb_neq in different.
  unfold erase in *; cbn. now rewrite different; cbn; rewrite IH.
Qed.

Theorem scaffold_erasure_preserves_real_bindings {V} key (scaffold : V) before descendants :
  Forall (fun item => fst item <> key) before ->
  Forall (fun item => fst item <> key) descendants ->
  erase key (descendants ++ put key scaffold before) = descendants ++ before.
Proof.
  intros oldAbsent childAbsent. unfold put.
  rewrite (erase_absent oldAbsent).
  unfold erase at 1. rewrite filter_app.
  change (erase key descendants ++
    (if negb (key =? key) then (key, scaffold) :: erase key before
     else erase key before) = descendants ++ before).
  rewrite Nat.eqb_refl; cbn.
  now rewrite (erase_absent childAbsent), (erase_absent oldAbsent).
Qed.

Theorem equal_payloads_preserve_distinct_keys {V} (value : V) first second :
  first <> second ->
  put first value (put second value []) = [(first, value); (second, value)].
Proof.
  intro different. unfold put, erase; cbn.
  assert (second <> first) by congruence.
  apply Nat.eqb_neq in H. now rewrite H.
Qed.

Theorem scaffold_leak_mutant_is_observable {V} key (scaffold : V) :
  publish (State [] [(key, scaffold)] None) <> Some [].
Proof. discriminate. Qed.

Record consuming_cursor (V : Type) := Cursor {
  cursor_topology : list nat;
  cursor_remaining : bindings V
}.
Arguments Cursor {V}.
Arguments cursor_topology {V}.
Arguments cursor_remaining {V}.

Definition cursor_next {V} (current : consuming_cursor V)
    : option (nat * V) * consuming_cursor V :=
  match cursor_remaining current with
  | [] => (None, current)
  | item :: rest => (Some item, Cursor (cursor_topology current) rest)
  end.

Fixpoint cursor_take {V} count (current : consuming_cursor V)
    : bindings V * consuming_cursor V :=
  match count with
  | 0 => ([], current)
  | S fuel =>
      let '(item, next) := cursor_next current in
      match item with
      | None => ([], next)
      | Some value =>
          let '(taken, final) := cursor_take fuel next in
          (value :: taken, final)
      end
  end.

Theorem cursor_next_preserves_topology {V} (current : consuming_cursor V) :
  cursor_topology (snd (cursor_next current)) = cursor_topology current.
Proof. destruct current as [shape [|item rest]]; reflexivity. Qed.

Theorem cursor_next_conserves_bindings {V} (current : consuming_cursor V) :
  (match fst (cursor_next current) with None => [] | Some item => [item] end) ++
    cursor_remaining (snd (cursor_next current)) = cursor_remaining current.
Proof. destruct current as [shape [|item rest]]; reflexivity. Qed.

Theorem cursor_take_is_ordered_partition {V} count shape (items : bindings V) :
  cursor_take count (Cursor shape items) =
    (firstn count items, Cursor shape (skipn count items)).
Proof.
  revert items. induction count as [|count IH]; intros [|item rest]; cbn;
    try reflexivity. now rewrite IH.
Qed.

Theorem cursor_take_conserves_bindings {V} count (current : consuming_cursor V) :
  fst (cursor_take count current) ++
    cursor_remaining (snd (cursor_take count current)) = cursor_remaining current.
Proof.
  destruct current as [shape items]. rewrite cursor_take_is_ordered_partition.
  cbn. apply firstn_skipn.
Qed.

Theorem cursor_take_preserves_topology {V} count (current : consuming_cursor V) :
  cursor_topology (snd (cursor_take count current)) = cursor_topology current.
Proof.
  destruct current as [shape items]. now rewrite cursor_take_is_ordered_partition.
Qed.

Theorem cursor_exhaustion_is_fused {V} shape :
  cursor_next (Cursor (V:=V) shape []) = (None, Cursor shape []).
Proof. reflexivity. Qed.

Theorem cursor_early_retirement_owns_exact_suffix {V} count shape (items : bindings V) :
  map (@snd nat V) (cursor_remaining (snd (cursor_take count (Cursor shape items)))) =
    map (@snd nat V) (skipn count items).
Proof. now rewrite cursor_take_is_ordered_partition. Qed.

Theorem cursor_root_skip_mutant_is_observable {V} (root : V) rest shape :
  fst (cursor_take 1 (Cursor shape ((0, root) :: rest))) <> [].
Proof. discriminate. Qed.

Inductive mapping_mode := MappingEmpty | MappingSet | MappingMap.

Section FallibleMapping.
Context {V W E : Type} (convert : V -> W + E).

Fixpoint mapping_reference (items : bindings V) : list nat * (bindings W + E) :=
  match items with
  | [] => ([], inl [])
  | (key, value) :: rest =>
      match convert value with
      | inr error => ([key], inr error)
      | inl mapped =>
          let '(trace, result) := mapping_reference rest in
          (key :: trace, match result with
            | inl output => inl ((key, mapped) :: output)
            | inr error => inr error
            end)
      end
  end.

Fixpoint mapping_loop (items : bindings V) (done : bindings W) (seen : list nat)
    : list nat * (bindings W + E) :=
  match items with
  | [] => (rev seen, inl (rev done))
  | (key, value) :: rest =>
      match convert value with
      | inr error => (rev (key :: seen), inr error)
      | inl mapped => mapping_loop rest ((key, mapped) :: done) (key :: seen)
      end
  end.

Theorem mapping_loop_refines_reference : forall items done seen,
  mapping_loop items done seen =
  let '(trace, result) := mapping_reference items in
  (rev seen ++ trace, match result with
    | inl output => inl (rev done ++ output)
    | inr error => inr error
    end).
Proof.
  induction items as [|[key value] rest IH]; intros done seen; cbn.
  - now rewrite !app_nil_r.
  - destruct (convert value) as [mapped|error]; cbn; [|reflexivity].
    rewrite IH. destruct (mapping_reference rest) as [trace [output|error]]; cbn;
      now rewrite <- !app_assoc.
Qed.

Theorem mapping_empty_accumulators_are_equivalent : forall items,
  mapping_loop items [] [] = mapping_reference items.
Proof.
  intros. rewrite mapping_loop_refines_reference.
  destruct (mapping_reference items) as [trace [output|error]]; reflexivity.
Qed.

Theorem mapping_success_preserves_keys_and_trace : forall items trace output,
  mapping_reference items = (trace, inl output) ->
  trace = map fst items /\ map fst output = map fst items.
Proof.
  induction items as [|[key value] rest IH]; intros trace output result; cbn in result.
  - inversion result. auto.
  - destruct (convert value) as [mapped|error]; [|discriminate].
    destruct (mapping_reference rest) as [tail [values|error]] eqn:next; [|discriminate].
    inversion result; subst. specialize (IH _ _ eq_refl).
    destruct IH as [trace_ok keys_ok]. cbn. split; congruence.
Qed.

Theorem mapping_first_error_stops : forall key value rest error,
  convert value = inr error ->
  mapping_reference ((key, value) :: rest) = ([key], inr error).
Proof. intros. cbn. now rewrite H. Qed.

Theorem mapping_failure_cannot_publish : forall items trace error,
  mapping_reference items = (trace, inr error) ->
  (match snd (mapping_loop items [] []) with
   | inl output => Some output | inr _ => None end) = None.
Proof. intros. rewrite mapping_empty_accumulators_are_equivalent, H. reflexivity. Qed.

Theorem mapping_success_exact_callback_count : forall items trace output,
  mapping_reference items = (trace, inl output) -> length trace = length items.
Proof.
  intros. apply mapping_success_preserves_keys_and_trace in H.
  rewrite (proj1 H), length_map. reflexivity.
Qed.

Theorem mapping_preserves_shape_and_mode : forall (mode : mapping_mode) (shape : list nat) items,
  let result := (mode, shape, mapping_loop items [] []) in
  fst (fst result) = mode /\ snd (fst result) = shape.
Proof. intros. split; reflexivity. Qed.

End FallibleMapping.

Definition span_path (spans : list (list nat * bool)) := concat (map fst spans).
Definition span_scaffolds (spans : list (list nat * bool)) :=
  length (filter snd spans).

Theorem relative_spans_compose_exact_path : forall prefix spans,
  fold_left (@app nat) (map fst spans) prefix = prefix ++ span_path spans.
Proof.
  intros prefix spans. revert prefix. induction spans as [|[bytes scaffold] rest IH];
    intro prefix; cbn; [now rewrite app_nil_r|].
  rewrite IH. unfold span_path. cbn. now rewrite app_assoc.
Qed.

Theorem scaffold_count_is_bounded_by_event_points : forall spans,
  span_scaffolds spans <= length spans.
Proof.
  unfold span_scaffolds. induction spans as [|[bytes scaffold] rest IH]; cbn;
    [apply Nat.le_refl|]. destruct scaffold; cbn in *; lia.
Qed.

Theorem changing_span_lengths_preserves_scaffold_work : forall spans resize,
  span_scaffolds (map (fun item => (resize (fst item), snd item)) spans) = span_scaffolds spans.
Proof.
  intros spans resize. unfold span_scaffolds. induction spans as [|[bytes scaffold] rest IH];
    cbn; [reflexivity|]. destruct scaffold; cbn; now rewrite IH.
Qed.

Theorem mapping_first_error_mutant_is_observable :
  mapping_reference (fun n : nat => if n =? 1 then inr 7 else inl n)
    [(0, 1); (1, 2)] = ([0], inr 7).
Proof. reflexivity. Qed.

Inductive admission_error := ModeConflict | InvalidKey (code : nat).

Definition compatible_mode current incoming :=
  match current, incoming with
  | MappingSet, MappingMap | MappingMap, MappingSet => false
  | _, _ => true
  end.

Record snapshot (V : Type) := Snapshot {
  snapshot_mode : mapping_mode;
  snapshot_shape : list nat;
  snapshot_bindings : bindings V
}.
Arguments Snapshot {V}.
Arguments snapshot_mode {V}.
Arguments snapshot_shape {V}.
Arguments snapshot_bindings {V}.

Definition map_snapshot {V W} (f : V -> W) (s : snapshot V) :=
  Snapshot (snapshot_mode s) (snapshot_shape s) (denote f (snapshot_bindings s)).

Fixpoint check_keys {V} (validate : nat -> option nat) (items : bindings V)
    : list nat * option admission_error :=
  match items with
  | [] => ([], None)
  | (key, _) :: rest =>
      match validate key with
      | Some code => ([key], Some (InvalidKey code))
      | None => let '(trace, error) := check_keys validate rest in (key :: trace, error)
      end
  end.

Definition check_snapshot {V} validate (current incoming : snapshot V) :=
  if compatible_mode (snapshot_mode current) (snapshot_mode incoming)
  then check_keys validate (snapshot_bindings incoming)
  else ([], Some ModeConflict).

Definition admit_snapshot {V} validate (current incoming : snapshot V) :=
  let '(trace, error) := check_snapshot validate current incoming in
  (trace, (match error with None => incoming | Some _ => current end, error)).

Theorem key_validation_ignores_payload_representation {V W} (f : V -> W) validate items :
  check_keys validate (denote f items) = check_keys validate items.
Proof.
  unfold denote.
  induction items as [|[key value] rest IH]; cbn; [reflexivity|].
  destruct (validate key); [reflexivity|]. now rewrite IH.
Qed.

Theorem snapshot_admission_commutes {V W} (f : V -> W) validate current incoming :
  admit_snapshot validate (map_snapshot f current) (map_snapshot f incoming) =
  let '(trace, (final, error)) := admit_snapshot validate current incoming in
  (trace, (map_snapshot f final, error)).
Proof.
  unfold admit_snapshot, check_snapshot, map_snapshot; cbn.
  destruct (compatible_mode (snapshot_mode current) (snapshot_mode incoming));
    [|reflexivity].
  rewrite key_validation_ignores_payload_representation.
  destruct (check_keys validate (snapshot_bindings incoming)) as [trace [error|]];
    reflexivity.
Qed.

Theorem snapshot_rejection_preserves_parent {V} validate (current incoming : snapshot V) trace code :
  check_snapshot validate current incoming = (trace, Some code) ->
  admit_snapshot validate current incoming = (trace, (current, Some code)).
Proof. intro H. unfold admit_snapshot. now rewrite H. Qed.

Theorem snapshot_success_installs_exact_image {V} validate (current incoming : snapshot V) trace :
  check_snapshot validate current incoming = (trace, None) ->
  admit_snapshot validate current incoming = (trace, (incoming, None)).
Proof. intro H. unfold admit_snapshot. now rewrite H. Qed.

Theorem mode_conflict_precedes_key_validation {V} validate (current incoming : snapshot V) :
  compatible_mode (snapshot_mode current) (snapshot_mode incoming) = false ->
  check_snapshot validate current incoming = ([], Some ModeConflict).
Proof. intro H. unfold check_snapshot. now rewrite H. Qed.

Theorem neutral_mode_accepts_every_mode mode :
  compatible_mode MappingEmpty mode = true /\ compatible_mode mode MappingEmpty = true.
Proof. destruct mode; split; reflexivity. Qed.

Theorem first_key_failure_stops_validation {V} validate key (value : V) rest code :
  validate key = Some code ->
  check_keys validate ((key, value) :: rest) = ([key], Some (InvalidKey code)).
Proof. intro H. cbn. now rewrite H. Qed.

Theorem key_validation_before_mode_mutant_is_observable :
  check_snapshot (fun _ => Some 7)
    (Snapshot MappingSet [] ([] : bindings nat))
    (Snapshot MappingMap [] [(0, 1)]) <>
  check_keys (fun _ => Some 7) [(0, 1)].
Proof. discriminate. Qed.

Print Assumptions key_validation_ignores_payload_representation.
Print Assumptions snapshot_admission_commutes.
Print Assumptions snapshot_rejection_preserves_parent.
Print Assumptions snapshot_success_installs_exact_image.
Print Assumptions mode_conflict_precedes_key_validation.
Print Assumptions neutral_mode_accepts_every_mode.
Print Assumptions first_key_failure_stops_validation.
Print Assumptions key_validation_before_mode_mutant_is_observable.

Print Assumptions mapping_loop_refines_reference.
Print Assumptions mapping_empty_accumulators_are_equivalent.
Print Assumptions mapping_success_preserves_keys_and_trace.
Print Assumptions mapping_first_error_stops.
Print Assumptions mapping_failure_cannot_publish.
Print Assumptions mapping_success_exact_callback_count.
Print Assumptions mapping_preserves_shape_and_mode.
Print Assumptions mapping_first_error_mutant_is_observable.
Print Assumptions relative_spans_compose_exact_path.
Print Assumptions scaffold_count_is_bounded_by_event_points.
Print Assumptions changing_span_lengths_preserves_scaffold_work.

Print Assumptions erase_commutes_with_denotation.
Print Assumptions install_commutes_with_denotation.
Print Assumptions each_writer_step_commutes.
Print Assumptions writer_trace_commutes.
Print Assumptions publication_commutes.
Print Assumptions operations_preserve_topology.
Print Assumptions refusal_cannot_publish.
Print Assumptions scaffold_erasure_preserves_real_bindings.
Print Assumptions equal_payloads_preserve_distinct_keys.
Print Assumptions scaffold_leak_mutant_is_observable.
Print Assumptions cursor_next_preserves_topology.
Print Assumptions cursor_next_conserves_bindings.
Print Assumptions cursor_take_is_ordered_partition.
Print Assumptions cursor_take_conserves_bindings.
Print Assumptions cursor_take_preserves_topology.
Print Assumptions cursor_exhaustion_is_fused.
Print Assumptions cursor_early_retirement_owns_exact_suffix.
Print Assumptions cursor_root_skip_mutant_is_observable.

End EpmPayloadRepresentation.
