From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import PeanoNat.
From Stdlib Require Import ZArith.
Import ListNotations.
Open Scope Z_scope.

Inductive snapshot_runtime : Type :=
  | OrdinarySnapshotRuntime
  | ReplaySnapshotRuntime.

Record validator_fuel_snapshot : Type := {
  snapshot_root : nat;
  snapshot_proposer : nat;
  snapshot_balance : Z;
  snapshot_source : snapshot_runtime;
  snapshot_before_rig : bool
}.

Definition capture_validator_fuel
  (root proposer : nat)
  (fuel : Z) : validator_fuel_snapshot :=
  {| snapshot_root := root;
     snapshot_proposer := proposer;
     snapshot_balance := fuel;
     snapshot_source := OrdinarySnapshotRuntime;
     snapshot_before_rig := true |}.

Definition snapshot_is_valid
  (expected_root expected_proposer : nat)
  (actual_fuel : Z)
  (snapshot : validator_fuel_snapshot) : bool :=
  Nat.eqb (snapshot_root snapshot) expected_root
  && Nat.eqb (snapshot_proposer snapshot) expected_proposer
  && Z.eqb (snapshot_balance snapshot) actual_fuel
  && match snapshot_source snapshot with
     | OrdinarySnapshotRuntime => true
     | ReplaySnapshotRuntime => false
     end
  && snapshot_before_rig snapshot.

Definition settlement_gate (fuel cost : Z) : bool :=
  (0 <=? cost) && (cost <=? fuel).

Definition play_handler
  (fuel cost : Z) : option Z :=
  if settlement_gate fuel cost
  then Some (fuel - cost)
  else None.

Definition replay_handler
  (expected_root expected_proposer : nat)
  (actual_fuel cost : Z)
  (snapshot : validator_fuel_snapshot) : option Z :=
  if snapshot_is_valid
       expected_root expected_proposer actual_fuel snapshot
     && settlement_gate actual_fuel cost
  then Some (actual_fuel - cost)
  else None.

Theorem captured_snapshot_is_valid : forall root proposer fuel,
  snapshot_is_valid root proposer fuel
    (capture_validator_fuel root proposer fuel) = true.
Proof.
  intros root proposer fuel.
  unfold snapshot_is_valid, capture_validator_fuel.
  simpl.
  now rewrite !Nat.eqb_refl, Z.eqb_refl.
Qed.

Theorem root_mismatch_rejects : forall expected_root actual_root proposer fuel cost,
  expected_root <> actual_root ->
  replay_handler expected_root proposer fuel cost
    {| snapshot_root := actual_root;
       snapshot_proposer := proposer;
       snapshot_balance := fuel;
       snapshot_source := OrdinarySnapshotRuntime;
       snapshot_before_rig := true |} = None.
Proof.
  intros expected_root actual_root proposer fuel cost mismatch.
  unfold replay_handler, snapshot_is_valid.
  simpl.
  destruct (Nat.eqb actual_root expected_root) eqn:equal.
  - apply Nat.eqb_eq in equal. congruence.
  - reflexivity.
Qed.

Theorem proposer_mismatch_rejects : forall root expected actual fuel cost,
  expected <> actual ->
  replay_handler root expected fuel cost
    {| snapshot_root := root;
       snapshot_proposer := actual;
       snapshot_balance := fuel;
       snapshot_source := OrdinarySnapshotRuntime;
       snapshot_before_rig := true |} = None.
Proof.
  intros root expected actual fuel cost mismatch.
  unfold replay_handler, snapshot_is_valid.
  simpl.
  rewrite Nat.eqb_refl.
  destruct (Nat.eqb actual expected) eqn:equal.
  - apply Nat.eqb_eq in equal. congruence.
  - reflexivity.
Qed.

Theorem stale_balance_rejects : forall root proposer actual recorded cost,
  actual <> recorded ->
  replay_handler root proposer actual cost
    {| snapshot_root := root;
       snapshot_proposer := proposer;
       snapshot_balance := recorded;
       snapshot_source := OrdinarySnapshotRuntime;
       snapshot_before_rig := true |} = None.
Proof.
  intros root proposer actual recorded cost mismatch.
  unfold replay_handler, snapshot_is_valid.
  simpl.
  rewrite !Nat.eqb_refl.
  destruct (Z.eqb recorded actual) eqn:equal.
  - apply Z.eqb_eq in equal. congruence.
  - reflexivity.
Qed.

Theorem replay_runtime_snapshot_rejects : forall root proposer fuel cost,
  replay_handler root proposer fuel cost
    {| snapshot_root := root;
       snapshot_proposer := proposer;
       snapshot_balance := fuel;
       snapshot_source := ReplaySnapshotRuntime;
       snapshot_before_rig := true |} = None.
Proof.
  intros root proposer fuel cost.
  unfold replay_handler, snapshot_is_valid.
  simpl.
  rewrite !Nat.eqb_refl, Z.eqb_refl.
  reflexivity.
Qed.

Theorem late_snapshot_rejects : forall root proposer fuel cost,
  replay_handler root proposer fuel cost
    {| snapshot_root := root;
       snapshot_proposer := proposer;
       snapshot_balance := fuel;
       snapshot_source := OrdinarySnapshotRuntime;
       snapshot_before_rig := false |} = None.
Proof.
  intros root proposer fuel cost.
  unfold replay_handler, snapshot_is_valid.
  simpl.
  rewrite !Nat.eqb_refl, Z.eqb_refl.
  reflexivity.
Qed.

Theorem replay_capture_refines_play : forall root proposer fuel cost,
  replay_handler root proposer fuel cost
    (capture_validator_fuel root proposer fuel) =
  play_handler fuel cost.
Proof.
  intros root proposer fuel cost.
  unfold replay_handler, play_handler.
  rewrite captured_snapshot_is_valid.
  reflexivity.
Qed.

Theorem successful_replay_settles_actual_fuel : forall root proposer fuel cost snapshot remaining,
  replay_handler root proposer fuel cost snapshot = Some remaining ->
  remaining = fuel - cost /\ cost <= fuel.
Proof.
  intros root proposer fuel cost snapshot remaining replayed.
  unfold replay_handler in replayed.
  destruct
    (snapshot_is_valid root proposer fuel snapshot
     && settlement_gate fuel cost) eqn:gate;
    try discriminate.
  inversion replayed. subst remaining.
  apply andb_true_iff in gate as [_ settlement].
  unfold settlement_gate in settlement.
  apply andb_true_iff in settlement as [_ bounded].
  apply Z.leb_le in bounded.
  split; reflexivity || exact bounded.
Qed.

Record play_selection : Type := {
  selected_handlers : list Z;
  deferred_handlers : list Z;
  play_remaining_fuel : Z
}.

Fixpoint select_play_prefix
  (fuel : Z)
  (costs : list Z) : play_selection :=
  match costs with
  | [] =>
      {| selected_handlers := [];
         deferred_handlers := [];
         play_remaining_fuel := fuel |}
  | cost :: tail =>
      match play_handler fuel cost with
      | None =>
          {| selected_handlers := [];
             deferred_handlers := cost :: tail;
             play_remaining_fuel := fuel |}
      | Some next_fuel =>
          let suffix := select_play_prefix next_fuel tail in
          {| selected_handlers := cost :: selected_handlers suffix;
             deferred_handlers := deferred_handlers suffix;
             play_remaining_fuel := play_remaining_fuel suffix |}
      end
  end.

Lemma select_play_prefix_cons : forall fuel cost tail,
  select_play_prefix fuel (cost :: tail) =
  match play_handler fuel cost with
  | None =>
      {| selected_handlers := [];
         deferred_handlers := cost :: tail;
         play_remaining_fuel := fuel |}
  | Some next_fuel =>
      let suffix := select_play_prefix next_fuel tail in
      {| selected_handlers := cost :: selected_handlers suffix;
         deferred_handlers := deferred_handlers suffix;
         play_remaining_fuel := play_remaining_fuel suffix |}
  end.
Proof.
  reflexivity.
Qed.

Record replay_result : Type := {
  replay_final_root : nat;
  replay_remaining_fuel : Z;
  replay_consumed_certificates : nat
}.

Fixpoint replay_selected_handlers
  (root proposer : nat)
  (fuel : Z)
  (costs : list Z) : option replay_result :=
  match costs with
  | [] =>
      Some
        {| replay_final_root := root;
           replay_remaining_fuel := fuel;
           replay_consumed_certificates := 0 |}
  | cost :: tail =>
      match replay_handler root proposer fuel cost
              (capture_validator_fuel root proposer fuel) with
      | None => None
      | Some next_fuel =>
          match replay_selected_handlers (S root) proposer next_fuel tail with
          | None => None
          | Some suffix =>
              Some
                {| replay_final_root := replay_final_root suffix;
                   replay_remaining_fuel := replay_remaining_fuel suffix;
                   replay_consumed_certificates :=
                     S (replay_consumed_certificates suffix) |}
          end
      end
  end.

Lemma replay_selected_handlers_cons : forall root proposer fuel cost tail,
  replay_selected_handlers root proposer fuel (cost :: tail) =
  match play_handler fuel cost with
  | None => None
  | Some next_fuel =>
      match replay_selected_handlers (S root) proposer next_fuel tail with
      | None => None
      | Some suffix =>
          Some
            {| replay_final_root := replay_final_root suffix;
               replay_remaining_fuel := replay_remaining_fuel suffix;
               replay_consumed_certificates :=
                 S (replay_consumed_certificates suffix) |}
      end
  end.
Proof.
  intros root proposer fuel cost tail.
  unfold replay_selected_handlers, replay_handler, capture_validator_fuel.
  unfold snapshot_is_valid, play_handler.
  simpl.
  rewrite !Nat.eqb_refl, Z.eqb_refl.
  destruct (settlement_gate fuel cost); reflexivity.
Qed.

Inductive play_prefix_relation :
  Z -> list Z -> list Z -> list Z -> Z -> Prop :=
  | play_prefix_done : forall fuel,
      play_prefix_relation fuel [] [] [] fuel
  | play_prefix_defer : forall fuel cost tail,
      play_handler fuel cost = None ->
      play_prefix_relation fuel (cost :: tail) [] (cost :: tail) fuel
  | play_prefix_accept :
      forall fuel cost tail next_fuel selected deferred remaining,
        play_handler fuel cost = Some next_fuel ->
        play_prefix_relation
          next_fuel tail selected deferred remaining ->
        play_prefix_relation
          fuel (cost :: tail) (cost :: selected) deferred remaining.

Lemma select_play_prefix_is_sound : forall fuel costs,
  play_prefix_relation
    fuel
    costs
    (selected_handlers (select_play_prefix fuel costs))
    (deferred_handlers (select_play_prefix fuel costs))
    (play_remaining_fuel (select_play_prefix fuel costs)).
Proof.
  intros fuel costs.
  revert fuel.
  induction costs as [| cost tail IH].
  - intros fuel. constructor.
  - intros fuel.
    rewrite select_play_prefix_cons.
    destruct (play_handler fuel cost) as [next_fuel |] eqn:played.
    + simpl.
      econstructor.
      * exact played.
      * apply IH.
    + simpl.
      constructor.
      exact played.
Qed.

Lemma related_play_prefix_replays :
  forall fuel costs selected deferred remaining,
    play_prefix_relation fuel costs selected deferred remaining ->
    forall root proposer,
      exists result,
        replay_selected_handlers root proposer fuel selected = Some result /\
        replay_remaining_fuel result = remaining.
Proof.
  intros fuel costs selected deferred remaining relation.
  induction relation.
  - intros root proposer.
    exists
      {| replay_final_root := root;
         replay_remaining_fuel := fuel;
         replay_consumed_certificates := 0 |}.
    split; reflexivity.
  - intros root proposer.
    exists
      {| replay_final_root := root;
         replay_remaining_fuel := fuel;
         replay_consumed_certificates := 0 |}.
    split; reflexivity.
  - intros root proposer.
    destruct (IHrelation (S root) proposer)
      as [suffix [replayed remaining_fuel]].
    exists
      {| replay_final_root := replay_final_root suffix;
         replay_remaining_fuel := replay_remaining_fuel suffix;
         replay_consumed_certificates :=
           S (replay_consumed_certificates suffix) |}.
    rewrite replay_selected_handlers_cons, H, replayed.
    split; [reflexivity | exact remaining_fuel].
Qed.

Theorem selected_play_prefix_replays : forall costs root proposer fuel,
  exists result,
    replay_selected_handlers root proposer fuel
      (selected_handlers (select_play_prefix fuel costs)) = Some result /\
    replay_remaining_fuel result =
      play_remaining_fuel (select_play_prefix fuel costs).
Proof.
  intros costs root proposer fuel.
  eapply related_play_prefix_replays.
  apply select_play_prefix_is_sound.
Qed.

Theorem replay_consumes_exact_certificate_prefix : forall costs root proposer fuel result,
  replay_selected_handlers root proposer fuel costs = Some result ->
  replay_consumed_certificates result = length costs.
Proof.
  induction costs as [| cost tail IH].
  - intros root proposer fuel result replayed.
    cbn [replay_selected_handlers] in replayed.
    inversion replayed.
    reflexivity.
  - intros root proposer fuel result replayed.
    cbn [replay_selected_handlers] in replayed.
    destruct
      (replay_handler root proposer fuel cost
        (capture_validator_fuel root proposer fuel)) as [next_fuel |]
      eqn:head;
      try discriminate.
    destruct
      (replay_selected_handlers (S root) proposer next_fuel tail) as [suffix |]
      eqn:suffix_replay;
      try discriminate.
    inversion replayed. subst result.
    cbn.
    f_equal.
    eapply IH.
    exact suffix_replay.
Qed.

Theorem independent_replay_sessions_do_not_share_cursor :
  forall costs root proposer fuel left right,
    replay_selected_handlers root proposer fuel costs = Some left ->
    replay_selected_handlers root proposer fuel costs = Some right ->
    left = right.
Proof.
  intros costs root proposer fuel left right left_run right_run.
  congruence.
Qed.

Theorem exact_depletion_play_example :
  select_play_prefix 6 [3; 3; 3] =
  {| selected_handlers := [3; 3];
     deferred_handlers := [3];
     play_remaining_fuel := 0 |}.
Proof.
  reflexivity.
Qed.

Theorem exact_depletion_replay_example :
  replay_selected_handlers 10 7 6 [3; 3] =
  Some
    {| replay_final_root := 12;
       replay_remaining_fuel := 0;
       replay_consumed_certificates := 2 |}.
Proof.
  reflexivity.
Qed.

Print Assumptions captured_snapshot_is_valid.
Print Assumptions root_mismatch_rejects.
Print Assumptions replay_capture_refines_play.
Print Assumptions successful_replay_settles_actual_fuel.
Print Assumptions selected_play_prefix_replays.
Print Assumptions replay_consumes_exact_certificate_prefix.
Print Assumptions independent_replay_sessions_do_not_share_cursor.
Print Assumptions exact_depletion_replay_example.
