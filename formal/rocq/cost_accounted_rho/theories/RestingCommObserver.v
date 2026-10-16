(**
  Additive native observer lifecycle for a CostV2 resting rendezvous.

  A RestingCOMM addresses a previously stored waiter and an ordered roster of
  stored data. It has no arriving Produce/Consume introduction. This model
  deliberately gives it a separate native evidence constructor and stage
  machine, so the old introduction-bearing record remains unchanged. It
  abstracts the RSpace lock, guard, and host-work checks into [preflight], and
  the atomic data retirement/event append into [RetireAndAppend]. The RSpace
  RestingCommCommit TLA model checks the ordered address under locks; this
  model checks the observer and native-recording boundary.
*)

From Stdlib Require Import List Bool PeanoNat.
Import ListNotations.

Module RestingCommObserver.

Record RestingEvent := {
  waiter_source : nat;
  continuation_index : nat;
  ordered_producers : list nat;
  ordered_data_indices : list nat;
  peeks : list nat
}.

Fixpoint nat_list_equal (left right : list nat) : bool :=
  match left, right with
  | [], [] => true
  | l :: ls, r :: rs => Nat.eqb l r && nat_list_equal ls rs
  | _, _ => false
  end.

Lemma nat_list_equal_sound : forall left right,
  nat_list_equal left right = true -> left = right.
Proof.
  induction left as [|l ls IH]; destruct right as [|r rs]; simpl;
    try discriminate; auto.
  intros H. apply andb_true_iff in H as [Hhead Htail].
  apply Nat.eqb_eq in Hhead. apply IH in Htail. now subst.
Qed.

Definition event_equal (left right : RestingEvent) : bool :=
  Nat.eqb (waiter_source left) (waiter_source right) &&
  Nat.eqb (continuation_index left) (continuation_index right) &&
  nat_list_equal (ordered_producers left) (ordered_producers right) &&
  nat_list_equal (ordered_data_indices left) (ordered_data_indices right) &&
  nat_list_equal (peeks left) (peeks right).

Inductive RestingPhase :=
| Idle
| Started (event : RestingEvent)
| Observed (event : RestingEvent)
| Committed (event : RestingEvent)
| FinishedMatched (event : RestingEvent)
| FinishedRejected (event : RestingEvent).

Inductive RestingAction :=
| Start (approved_cost_v2 preflight : bool) (event : RestingEvent)
| Observe (event : RestingEvent)
| RetireAndAppend
| FinishMatch
| Refuse.

Definition advance (phase : RestingPhase) (action : RestingAction)
    : option RestingPhase :=
  match phase, action with
  | Idle, Start true true event => Some (Started event)
  | Started expected, Observe actual =>
      if event_equal expected actual then Some (Observed expected) else None
  | Observed event, RetireAndAppend => Some (Committed event)
  | Committed event, FinishMatch => Some (FinishedMatched event)
  | Started event, Refuse | Observed event, Refuse =>
      Some (FinishedRejected event)
  | _, _ => None
  end.

Theorem start_requires_approved_cost_v2_and_preflight :
  forall action event,
    advance Idle action = Some (Started event) ->
    action = Start true true event.
Proof.
  intros action event H.
  destruct action; simpl in H; try discriminate.
  destruct approved_cost_v2, preflight; simpl in H; try discriminate.
  inversion H. reflexivity.
Qed.

Theorem observation_preserves_exact_ordered_event :
  forall expected actual observed,
    advance (Started expected) (Observe actual) = Some (Observed observed) ->
    expected = actual /\ observed = expected.
Proof.
  intros expected actual observed H.
  simpl in H. destruct (event_equal expected actual) eqn:Heq; try discriminate.
  inversion H; subst observed. split; try reflexivity.
  unfold event_equal in Heq.
  repeat match goal with
  | H : (_ && _) = true |- _ => apply andb_true_iff in H as [? ?]
  end.
  destruct expected as [ew ei ep ed ek], actual as [aw ai ap ad ak]; simpl in *.
  repeat match goal with
  | H : Nat.eqb _ _ = true |- _ => apply Nat.eqb_eq in H
  | H : nat_list_equal _ _ = true |- _ =>
      apply nat_list_equal_sound in H
  end.
  subst. reflexivity.
Qed.

Theorem matched_result_requires_atomic_retirement_and_append :
  forall phase action event,
    advance phase action = Some (FinishedMatched event) ->
    phase = Committed event /\ action = FinishMatch.
Proof.
  intros phase action event H.
  destruct phase; destruct action; simpl in H; try discriminate;
    try (destruct approved_cost_v2, preflight; discriminate);
    try (destruct (event_equal event0 event1); discriminate).
  inversion H; subst. split; reflexivity.
Qed.

Theorem rejected_result_has_no_retirement_or_event_append :
  forall phase action event,
    advance phase action = Some (FinishedRejected event) ->
    action = Refuse /\ (phase = Started event \/ phase = Observed event).
Proof.
  intros phase action event H.
  destruct phase; destruct action; simpl in H; try discriminate;
    try (destruct approved_cost_v2, preflight; discriminate);
    try (destruct (event_equal event0 event1); discriminate);
    inversion H; subst; split; auto.
Qed.

(** The V1 record remains the same term and bytes under the V2 sum. A new
    Rust codec epoch must likewise keep legacy serialization on its old arm. *)
Record LegacyOperationRecord := {
  legacy_source : nat;
  legacy_introduction : nat;
  legacy_comm : option nat
}.

Inductive NativeOperationRecordV2 :=
| LegacyRecord (record : LegacyOperationRecord)
| RestingRecord (event : RestingEvent) (matched : bool).

Definition project_legacy (record : NativeOperationRecordV2)
    : option LegacyOperationRecord :=
  match record with LegacyRecord legacy => Some legacy | RestingRecord _ _ => None end.

Theorem v2_extension_preserves_v1_record :
  forall legacy, project_legacy (LegacyRecord legacy) = Some legacy.
Proof. reflexivity. Qed.

Theorem resting_record_has_no_introduction :
  forall event matched, project_legacy (RestingRecord event matched) = None.
Proof. reflexivity. Qed.

(** The historical Casper event protobuf has only IO and arriving-leg COMM.
    Conversion remains partial until a separately versioned Resting wire and
    replay rule are adopted. Returning an ordinary COMM here would invent an
    introduction and make a different event replayable under an old root. *)
Inductive RSpaceEvent :=
| HistoricalIo (identity : nat)
| HistoricalComm (identity : nat)
| RestingComm (event : RestingEvent).

Inductive CasperEventV1 :=
| CasperIo (identity : nat)
| CasperComm (identity : nat).

Definition convert_legacy_event (event : RSpaceEvent)
    : option CasperEventV1 :=
  match event with
  | HistoricalIo identity => Some (CasperIo identity)
  | HistoricalComm identity => Some (CasperComm identity)
  | RestingComm _ => None
  end.

Theorem legacy_casper_converter_refuses_resting :
  forall event, convert_legacy_event (RestingComm event) = None.
Proof. reflexivity. Qed.

Theorem legacy_casper_converter_preserves_old_identity :
  forall identity,
    convert_legacy_event (HistoricalIo identity) = Some (CasperIo identity) /\
    convert_legacy_event (HistoricalComm identity) = Some (CasperComm identity).
Proof. intros; split; reflexivity. Qed.

(** V7 adds one protobuf arm, without changing the two historical arm images.
    The outer constructor is a disjoint wire alternative, not an encoded
    ordinary COMM. [RestingEvent] names the exact ordered waiter/producer/data
    address; its two ordered lists are not sets. *)
Inductive CasperEventEpoch :=
| LegacyEpoch
| CostV1Epoch
| CostV2Epoch.

Inductive CasperEventV7 :=
| CasperHistorical (event : CasperEventV1)
| CasperResting (event : RestingEvent).

Definition convert_v7_event (event : RSpaceEvent) : CasperEventV7 :=
  match event with
  | HistoricalIo identity => CasperHistorical (CasperIo identity)
  | HistoricalComm identity => CasperHistorical (CasperComm identity)
  | RestingComm resting => CasperResting resting
  end.

Definition convert_for_epoch (epoch : CasperEventEpoch) (event : RSpaceEvent)
    : option CasperEventV7 :=
  match epoch with
  | LegacyEpoch | CostV1Epoch =>
      option_map CasperHistorical (convert_legacy_event event)
  | CostV2Epoch => Some (convert_v7_event event)
  end.

Definition decode_for_epoch (epoch : CasperEventEpoch) (event : CasperEventV7)
    : option RSpaceEvent :=
  match event with
  | CasperHistorical (CasperIo identity) => Some (HistoricalIo identity)
  | CasperHistorical (CasperComm identity) => Some (HistoricalComm identity)
  | CasperResting resting =>
      match epoch with
      | CostV2Epoch => Some (RestingComm resting)
      | LegacyEpoch | CostV1Epoch => None
      end
  end.

Theorem old_epoch_refuses_resting_conversion_and_decode :
  forall resting,
    convert_for_epoch LegacyEpoch (RestingComm resting) = None /\
    convert_for_epoch CostV1Epoch (RestingComm resting) = None /\
    decode_for_epoch LegacyEpoch (CasperResting resting) = None /\
    decode_for_epoch CostV1Epoch (CasperResting resting) = None.
Proof. intros; repeat split; reflexivity. Qed.

Theorem cost_v2_resting_roundtrip_preserves_exact_ordered_address :
  forall resting,
    convert_for_epoch CostV2Epoch (RestingComm resting) =
      Some (CasperResting resting) /\
    decode_for_epoch CostV2Epoch (CasperResting resting) =
      Some (RestingComm resting).
Proof. intros; split; reflexivity. Qed.

Theorem resting_never_aliases_historical_comm :
  forall epoch resting identity,
    convert_for_epoch epoch (RestingComm resting) <>
      Some (CasperHistorical (CasperComm identity)) /\
    decode_for_epoch epoch (CasperResting resting) <>
      Some (HistoricalComm identity).
Proof.
  intros epoch resting identity; destruct epoch; simpl;
    split; discriminate.
Qed.

Theorem v1_v6_and_v7_preserve_old_event_identity :
  forall identity,
    convert_for_epoch LegacyEpoch (HistoricalIo identity) =
      Some (CasperHistorical (CasperIo identity)) /\
    convert_for_epoch CostV1Epoch (HistoricalIo identity) =
      Some (CasperHistorical (CasperIo identity)) /\
    convert_for_epoch CostV2Epoch (HistoricalIo identity) =
      Some (CasperHistorical (CasperIo identity)) /\
    convert_for_epoch LegacyEpoch (HistoricalComm identity) =
      Some (CasperHistorical (CasperComm identity)) /\
    convert_for_epoch CostV1Epoch (HistoricalComm identity) =
      Some (CasperHistorical (CasperComm identity)) /\
    convert_for_epoch CostV2Epoch (HistoricalComm identity) =
      Some (CasperHistorical (CasperComm identity)).
Proof. intros; repeat split; reflexivity. Qed.

(** Block-level affected-channel indexing is a read-only projection of the
    stored consume's channels followed by the bind-ordered stored producer
    channels. These channel rosters are source fields, not a substitute for the
    Resting event identity: the observation returns the exact event unchanged.
    In particular, the projection does not sort or deduplicate repeated
    producer channels. *)
Definition observe_resting_affected_channels
    (event : RestingEvent) (consume_channels ordered_produce_channels : list nat)
    : RestingEvent * list nat :=
  (event, consume_channels ++ ordered_produce_channels).

Theorem resting_affected_channels_are_read_only_and_ordered :
  forall event consume_channels ordered_produce_channels,
    fst (observe_resting_affected_channels
      event consume_channels ordered_produce_channels) = event /\
    snd (observe_resting_affected_channels
      event consume_channels ordered_produce_channels) =
      consume_channels ++ ordered_produce_channels.
Proof. intros; split; reflexivity. Qed.

Example repeated_producer_channels_remain_visible_without_identity_change :
  let event := {| waiter_source := 4; continuation_index := 0;
                  ordered_producers := [9; 9];
                  ordered_data_indices := [1; 2]; peeks := [] |} in
  observe_resting_affected_channels event [3; 3] [3; 3] =
    (event, [3; 3; 3; 3]).
Proof. reflexivity. Qed.

Example reversed_repeated_channel_indices_are_not_the_same_event :
  let base := {| waiter_source := 4; continuation_index := 0;
                 ordered_producers := [9; 9];
                 ordered_data_indices := [1; 2]; peeks := [] |} in
  let reversed := {| waiter_source := 4; continuation_index := 0;
                     ordered_producers := [9; 9];
                     ordered_data_indices := [2; 1]; peeks := [] |} in
  event_equal base reversed = false.
Proof. reflexivity. Qed.

End RestingCommObserver.
