From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat.
From CostAccountedRho Require Import NativeOperationJournal.
Import ListNotations.

Record producer := { producer_channel : nat; producer_hash : nat; producer_persistent : bool }.
Record consumer := { consumer_channels : list nat; consumer_hash : nat; consumer_persistent : bool }.
Record comm_source := {
  comm_consumer : consumer;
  comm_producers : list producer;
  comm_peeks : list nat;
  comm_repetitions : list (producer * nat)
}.

Definition comm_eq_dec : forall a b : comm_source, {a = b} + {a <> b}.
Proof. repeat decide equality. Defined.

Definition authenticated_comm actual expected :=
  if comm_eq_dec actual expected then true else false.

Theorem authenticated_comm_exact : forall actual expected,
  authenticated_comm actual expected = true <-> actual = expected.
Proof. intros. unfold authenticated_comm. destruct (comm_eq_dec actual expected); split; congruence. Qed.

Theorem authenticated_comm_preserves_every_field : forall actual expected,
  authenticated_comm actual expected = true ->
  comm_consumer actual = comm_consumer expected /\
  comm_producers actual = comm_producers expected /\
  comm_peeks actual = comm_peeks expected /\
  comm_repetitions actual = comm_repetitions expected.
Proof. intros. apply authenticated_comm_exact in H. subst. repeat split. Qed.

Definition authenticated_footprint (actual expected : list nat) :=
  if list_eq_dec Nat.eq_dec actual expected then true else false.

Theorem authenticated_footprint_exact : forall actual expected,
  authenticated_footprint actual expected = true <-> actual = expected.
Proof. intros. unfold authenticated_footprint. destruct (list_eq_dec Nat.eq_dec actual expected); split; congruence. Qed.

Theorem authenticated_footprint_preserves_actual_conflicts : forall actual declared last supplied,
  authenticated_footprint actual declared = true ->
  (forall predecessor, In predecessor supplied <->
    In predecessor (journal_declared_predecessors declared last)) ->
  forall channel predecessor, In channel actual -> last channel = Some predecessor ->
  In predecessor supplied.
Proof.
  intros actual declared last supplied checked exact channel predecessor present previous.
  apply authenticated_footprint_exact in checked. subst declared.
  eapply journal_exact_predecessors_preserve_declared_conflicts; eauto.
Qed.

Theorem omitted_actual_channel_rejected : forall actual declared channel,
  In channel actual -> ~In channel declared -> authenticated_footprint actual declared = false.
Proof.
  intros. destruct (authenticated_footprint actual declared) eqn:checked; auto.
  apply authenticated_footprint_exact in checked. subst. contradiction.
Qed.

Theorem extra_declared_channel_rejected : forall actual declared channel,
  ~In channel actual -> In channel declared -> authenticated_footprint actual declared = false.
Proof.
  intros. destruct (authenticated_footprint actual declared) eqn:checked; auto.
  apply authenticated_footprint_exact in checked. subst. contradiction.
Qed.

Definition advance_authenticated {State : Type} source observation (before after : State) :=
  if source && observation then after else before.

Theorem source_rejection_preserves_state : forall State observation (before after : State),
  advance_authenticated false observation before after = before.
Proof. reflexivity. Qed.

Theorem observation_rejection_preserves_state : forall State source (before after : State),
  advance_authenticated source false before after = before.
Proof. intros State [] before after; reflexivity. Qed.

Record candidate_event := {
  candidate_source : comm_source;
  candidate_telemetry : list nat;
  candidate_guard : bool;
  candidate_counter_valid : bool
}.

Definition eligible_candidate expected candidate :=
  authenticated_comm (candidate_source candidate) expected &&
  candidate_guard candidate && candidate_counter_valid candidate.

Definition select_candidate expected candidates := find (eligible_candidate expected) candidates.

Theorem selected_candidate_authenticates_source : forall expected candidates selected,
  select_candidate expected candidates = Some selected ->
  candidate_source selected = expected /\
  candidate_guard selected = true /\ candidate_counter_valid selected = true.
Proof.
  intros expected candidates selected checked.
  apply find_some in checked. destruct checked as [_ checked].
  unfold eligible_candidate in checked.
  repeat rewrite andb_true_iff in checked.
  destruct checked as [[source guard] counter].
  apply authenticated_comm_exact in source. auto.
Qed.

Theorem selected_candidate_is_actual : forall expected candidates selected,
  select_candidate expected candidates = Some selected -> In selected candidates.
Proof. intros. apply find_some in H. tauto. Qed.

Definition replace_telemetry candidate telemetry :=
  {| candidate_source := candidate_source candidate;
     candidate_telemetry := telemetry;
     candidate_guard := candidate_guard candidate;
     candidate_counter_valid := candidate_counter_valid candidate |}.

Theorem telemetry_cannot_change_eligibility : forall expected candidate telemetry,
  eligible_candidate expected (replace_telemetry candidate telemetry) =
  eligible_candidate expected candidate.
Proof. reflexivity. Qed.

Theorem selection_preserves_order : forall expected prefix selected suffix,
  Forall (fun c => eligible_candidate expected c = false) prefix ->
  eligible_candidate expected selected = true ->
  select_candidate expected (prefix ++ selected :: suffix) = Some selected.
Proof.
  intros expected prefix selected suffix rejected accepted.
  induction rejected; simpl; unfold select_candidate in *; simpl in *.
  - rewrite accepted. reflexivity.
  - rewrite H. exact IHrejected.
Qed.

Theorem missing_candidate_is_not_authority : forall expected candidates,
  select_candidate expected candidates = None ->
  Forall (fun c => eligible_candidate expected c = false) candidates.
Proof.
  intros expected candidates. induction candidates; simpl; intro absent; constructor.
  - unfold select_candidate in absent. simpl in absent.
    destruct (eligible_candidate expected a) eqn:eligible; congruence.
  - apply IHcandidates. unfold select_candidate in *. simpl in absent.
    destruct (eligible_candidate expected a); congruence.
Qed.

Record consume_input := {
  input_source : consumer;
  input_peeks : list nat
}.

Definition consume_input_eq_dec : forall a b : consume_input, {a = b} + {a <> b}.
Proof. repeat decide equality. Defined.

Definition authenticated_consume_input actual expected :=
  if consume_input_eq_dec actual expected then true else false.

Theorem authenticated_consume_input_exact : forall actual expected,
  authenticated_consume_input actual expected = true <-> actual = expected.
Proof.
  intros. unfold authenticated_consume_input.
  destruct (consume_input_eq_dec actual expected); split; congruence.
Qed.

Theorem authenticated_consume_input_preserves_peeks : forall actual expected,
  authenticated_consume_input actual expected = true ->
  input_peeks actual = input_peeks expected.
Proof. intros. apply authenticated_consume_input_exact in H. subst. reflexivity. Qed.

Theorem changed_peeks_rejected_before_publication : forall actual expected,
  input_peeks actual <> input_peeks expected ->
  authenticated_consume_input actual expected = false.
Proof.
  intros. destruct (authenticated_consume_input actual expected) eqn:checked; auto.
  apply authenticated_consume_input_preserves_peeks in checked. contradiction.
Qed.

Definition replay_store_consume actual expected (before : list consume_input) :=
  if authenticated_consume_input actual expected then Some (actual :: before) else None.

Theorem stored_consume_has_exact_recorded_input : forall actual expected before after,
  replay_store_consume actual expected before = Some after ->
  after = expected :: before.
Proof.
  intros. unfold replay_store_consume in H.
  destruct (authenticated_consume_input actual expected) eqn:checked; [|discriminate].
  apply authenticated_consume_input_exact in checked. subst. inversion H. reflexivity.
Qed.

Theorem consume_source_alone_does_not_bind_peeks : forall source,
  let without_peek := {| input_source := source; input_peeks := [] |} in
  let with_peek := {| input_source := source; input_peeks := [0] |} in
  input_source without_peek = input_source with_peek /\ without_peek <> with_peek.
Proof. intros. simpl. split; [reflexivity|discriminate]. Qed.

Record producer_result := {
  result_source : producer;
  result_deterministic : bool;
  result_failed : bool;
  result_output : list (list nat)
}.

Definition producer_result_eq_dec : forall a b : producer_result, {a = b} + {a <> b}.
Proof. repeat decide equality. Defined.

Definition authenticate_producer_result actual expected :=
  if producer_result_eq_dec actual expected then true else false.

Theorem authenticated_producer_result_exact : forall actual expected,
  authenticate_producer_result actual expected = true <-> actual = expected.
Proof.
  intros. unfold authenticate_producer_result.
  destruct (producer_result_eq_dec actual expected); split; congruence.
Qed.

Theorem authenticated_producer_result_preserves_output : forall actual expected,
  authenticate_producer_result actual expected = true -> result_output actual = result_output expected.
Proof. intros. apply authenticated_producer_result_exact in H. now subst. Qed.

Theorem logical_source_does_not_authenticate_output : forall source,
  let first := {| result_source := source; result_deterministic := false;
                  result_failed := false; result_output := [[1]] |} in
  let second := {| result_source := source; result_deterministic := false;
                   result_failed := false; result_output := [[2]] |} in
  result_source first = result_source second /\ authenticate_producer_result first second = false.
Proof.
  intros. simpl. split; [reflexivity|]. unfold authenticate_producer_result.
  destruct (producer_result_eq_dec _ _); [discriminate|reflexivity].
Qed.
