(* DR-114: recorded external-service replies in native funded execution.

   Play asks an external service once for each recorded call, and it records
   the reply on the triggering produce of the call. Replay never asks a
   service. It gives each recorded call the record that has the identity of
   the call. The identity of a call is the hash of its produce. The hash
   covers the random state of the call, so two calls never share one.

   - A service is any function of a node-local moment and a request. Two
     nodes, or one node at two moments, can give different answers.
   - A success takes room for its encoded bytes plus a fixed overhead
     ([take_native_record_room]). A success that does not fit the remaining
     room becomes the fixed failure reply EXT_OUTPUT_TOO_LARGE. A failed
     service gives EXT_FAILED. A malformed request gives EXT_BAD_ARG and calls
     no service. Thus every recorded call has exactly one reply, and a failure
     is a paid reply, not an abort.
   - Replay accepts the records only when their placement is exact: every
     recorded call has a record, and every record belongs to a recorded call.
     The checks are E1 ([record_placement]), E2 (the produce check of the
     native replay backend) and the completeness check of the replay session.

   The negative controls replace one rule with a plausible wrong rule, and
   they prove that the wrong rule breaks a required property. *)

From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation.
Import ListNotations.

(** * Calls, services and replies *)

Record call := {
  call_id : nat;
  call_recorded : bool;
  call_request : nat
}.

Inductive failure_code := ExtBadArg | ExtFailed | ExtOutputTooLarge.

Inductive reply :=
| Success (value : nat)
| Failure (code : failure_code).

(** A service answers a request at a moment. [None] is a failed call. *)
Definition service := nat -> nat -> option nat.

(** A record: the identity of a call and its reply. *)
Definition record := (nat * reply)%type.

(** The fixed room overhead of one success. *)
Definition overhead := 64.

Section Recording.

(** The encoded length of a success value, and the shape check of a request. *)
Variable size : nat -> nat.
Variable well_formed : nat -> bool.

Definition room_cost (value : nat) : nat := size value + overhead.

(** Native play ([native_recorded_call] in play). A plain call records
    nothing. A recorded call records exactly one reply. *)
Fixpoint play (answer : service) (moment room : nat) (calls : list call) : list record :=
  match calls with
  | [] => []
  | c :: rest =>
      if call_recorded c then
        if well_formed (call_request c) then
          match answer moment (call_request c) with
          | Some value =>
              if room_cost value <=? room
              then (call_id c, Success value) :: play answer (S moment) (room - room_cost value) rest
              else (call_id c, Failure ExtOutputTooLarge) :: play answer (S moment) room rest
          | None => (call_id c, Failure ExtFailed) :: play answer (S moment) room rest
          end
        else (call_id c, Failure ExtBadArg) :: play answer moment room rest
      else play answer moment room rest
  end.

(** The room that the successes of a record list take. *)
Fixpoint success_bytes (records : list record) : nat :=
  match records with
  | [] => 0
  | (_, Success value) :: rest => room_cost value + success_bytes rest
  | (_, Failure _) :: rest => success_bytes rest
  end.

(** The encoded bytes of the successes of a record list. *)
Fixpoint record_bytes (records : list record) : nat :=
  match records with
  | [] => 0
  | (_, Success value) :: rest => size value + record_bytes rest
  | (_, Failure _) :: rest => record_bytes rest
  end.

Lemma play_plain_step : forall answer moment room c rest,
  call_recorded c = false ->
  play answer moment room (c :: rest) = play answer moment room rest.
Proof. intros answer moment room c rest plain. simpl. now rewrite plain. Qed.

Lemma play_recorded_step : forall answer moment room c rest,
  call_recorded c = true ->
  exists r moment' room',
    play answer moment room (c :: rest) = (call_id c, r) :: play answer moment' room' rest.
Proof.
  intros answer moment room c rest recorded. simpl. rewrite recorded.
  destruct (well_formed (call_request c)).
  - destruct (answer moment (call_request c)) as [value|].
    + destruct (room_cost value <=? room); eexists _, _, _; reflexivity.
    + eexists _, _, _; reflexivity.
  - eexists _, _, _; reflexivity.
Qed.

Lemma play_ids : forall answer moment room calls,
  map fst (play answer moment room calls) = map call_id (filter call_recorded calls).
Proof.
  intros answer moment room calls. revert moment room.
  induction calls as [|c rest IH]; intros moment room; [reflexivity|].
  destruct (call_recorded c) eqn:recorded.
  - destruct (play_recorded_step answer moment room c rest recorded)
      as (r & moment' & room' & ->).
    simpl. rewrite recorded. simpl. f_equal. apply IH.
  - rewrite (play_plain_step answer moment room c rest recorded).
    simpl. rewrite recorded. apply IH.
Qed.

Lemma record_bytes_within_success_bytes : forall records,
  record_bytes records <= success_bytes records.
Proof.
  induction records as [|[id [value|code]] rest IH]; simpl; unfold room_cost; lia.
Qed.

End Recording.

(** * Replay *)

Fixpoint lookup (id : nat) (records : list record) : option reply :=
  match records with
  | [] => None
  | (id', r) :: rest => if Nat.eqb id id' then Some r else lookup id rest
  end.

Definition has_record (id : nat) (records : list record) : bool :=
  match lookup id records with
  | Some _ => true
  | None => false
  end.

(** The exact placement of the records over the calls of a run. *)
Definition placed (records : list record) (calls : list call) : bool :=
  forallb (fun c => if call_recorded c then has_record (call_id c) records else true) calls &&
  forallb (fun r => existsb (fun c => call_recorded c && Nat.eqb (call_id c) (fst r)) calls)
    records.

(** Replay gives each recorded call, in the order that replay meets it, the
    record with the identity of the call. *)
Definition deliver_one (records : list record) (c : call) : list record :=
  if call_recorded c then
    match lookup (call_id c) records with
    | Some r => [(call_id c, r)]
    | None => []
    end
  else [].

Definition deliver (records : list record) (calls : list call) : list record :=
  flat_map (deliver_one records) calls.

Definition replay (records : list record) (calls : list call) : option (list record) :=
  if placed records calls then Some (deliver records calls) else None.

Lemma lookup_present : forall id records,
  In id (map fst records) -> exists r, lookup id records = Some r.
Proof.
  intros id records present.
  induction records as [|[id' r] rest IH]; [contradiction|].
  simpl in *. destruct (Nat.eqb id id') eqn:same; [now exists r|].
  apply Nat.eqb_neq in same. destruct present as [equal|present]; [congruence|].
  now apply IH.
Qed.

Lemma lookup_in : forall id records r, lookup id records = Some r -> In (id, r) records.
Proof.
  intros id records r found.
  induction records as [|[id' r'] rest IH]; [discriminate|].
  simpl in found. destruct (Nat.eqb id id') eqn:same.
  - apply Nat.eqb_eq in same. injection found as <-. subst. now left.
  - right. now apply IH.
Qed.

Lemma deliver_skip_head : forall id r records calls,
  ~ In id (map call_id (filter call_recorded calls)) ->
  deliver ((id, r) :: records) calls = deliver records calls.
Proof.
  intros id r records calls absent.
  induction calls as [|c rest IH]; [reflexivity|].
  unfold deliver in *. simpl. unfold deliver_one at 1 3.
  destruct (call_recorded c) eqn:recorded.
  - simpl in absent. rewrite recorded in absent. simpl in absent.
    apply Decidable.not_or in absent as [distinct absent].
    simpl. destruct (Nat.eqb (call_id c) id) eqn:same.
    + apply Nat.eqb_eq in same. congruence.
    + f_equal. now apply IH.
  - simpl in absent. rewrite recorded in absent. now apply IH.
Qed.

(** * Theorems *)

(** Every recorded call has exactly one reply, in the order of the calls. *)
Theorem every_call_has_one_reply : forall size well_formed answer moment room calls,
  map fst (play size well_formed answer moment room calls) =
  map call_id (filter call_recorded calls).
Proof. intros. apply play_ids. Qed.

Lemma placed_play : forall size well_formed answer moment room calls,
  placed (play size well_formed answer moment room calls) calls = true.
Proof.
  intros size well_formed answer moment room calls.
  pose proof (play_ids size well_formed answer moment room calls) as ids.
  unfold placed. apply andb_true_intro. split.
  - apply forallb_forall. intros c member.
    destruct (call_recorded c) eqn:recorded; [|reflexivity].
    unfold has_record.
    assert (present : In (call_id c) (map fst (play size well_formed answer moment room calls))).
    { rewrite ids. apply in_map. apply filter_In. now split. }
    destruct (lookup_present _ _ present) as [r found]. now rewrite found.
  - apply forallb_forall. intros [id r] member.
    apply existsb_exists.
    assert (present : In id (map call_id (filter call_recorded calls))).
    { rewrite <- ids. now apply (in_map fst) in member. }
    apply in_map_iff in present as [c [same member_c]].
    apply filter_In in member_c as [member_c recorded].
    exists c. split; [assumption|]. rewrite recorded, same. simpl. apply Nat.eqb_refl.
Qed.

Lemma deliver_play : forall size well_formed answer moment room calls,
  NoDup (map call_id calls) ->
  deliver (play size well_formed answer moment room calls) calls =
  play size well_formed answer moment room calls.
Proof.
  intros size well_formed answer moment room calls. revert moment room.
  induction calls as [|c rest IH]; intros moment room unique; [reflexivity|].
  simpl in unique. apply NoDup_cons_iff in unique as [fresh unique].
  assert (absent : ~ In (call_id c) (map call_id (filter call_recorded rest))).
  { intro present. apply fresh. apply in_map_iff in present as [c' [same member]].
    apply filter_In in member as [member _]. rewrite <- same. now apply in_map. }
  destruct (call_recorded c) eqn:recorded.
  - destruct (play_recorded_step size well_formed answer moment room c rest recorded)
      as (r & moment' & room' & ->).
    unfold deliver. simpl. unfold deliver_one at 1. rewrite recorded. simpl.
    rewrite Nat.eqb_refl. simpl. f_equal.
    fold (deliver ((call_id c, r) :: play size well_formed answer moment' room' rest) rest).
    rewrite deliver_skip_head by exact absent. now apply IH.
  - rewrite (play_plain_step size well_formed answer moment room c rest recorded).
    unfold deliver. simpl. unfold deliver_one at 1. rewrite recorded. simpl.
    now apply IH.
Qed.

(** Replay gives every recorded call the reply that play recorded, whatever
    the services of the replaying node answer. *)
Theorem replay_agrees_with_play : forall size well_formed answer moment room calls,
  NoDup (map call_id calls) ->
  replay (play size well_formed answer moment room calls) calls =
  Some (play size well_formed answer moment room calls).
Proof.
  intros size well_formed answer moment room calls unique.
  unfold replay. rewrite placed_play. f_equal. now apply deliver_play.
Qed.

Lemma forallb_permutation : forall (A : Type) (f : A -> bool) l l',
  Permutation l l' -> forallb f l = forallb f l'.
Proof.
  intros A f l l' permuted. induction permuted; simpl.
  - reflexivity.
  - now rewrite IHpermuted.
  - now rewrite !andb_assoc, (andb_comm (f y) (f x)).
  - congruence.
Qed.

Lemma existsb_permutation : forall (A : Type) (f : A -> bool) l l',
  Permutation l l' -> existsb f l = existsb f l'.
Proof.
  intros A f l l' permuted. induction permuted; simpl.
  - reflexivity.
  - now rewrite IHpermuted.
  - now rewrite !orb_assoc, (orb_comm (f y) (f x)).
  - congruence.
Qed.

Lemma placed_permutation : forall records calls calls',
  Permutation calls calls' -> placed records calls' = placed records calls.
Proof.
  intros records calls calls' permuted. unfold placed.
  rewrite (forallb_permutation _ _ _ _ permuted). f_equal.
  induction records as [|r rest IH]; [reflexivity|]. simpl.
  rewrite (existsb_permutation _ _ _ _ permuted). now rewrite IH.
Qed.

Lemma deliver_permutation : forall records calls calls',
  Permutation calls calls' -> Permutation (deliver records calls) (deliver records calls').
Proof.
  intros records calls calls' permuted. unfold deliver.
  induction permuted; simpl.
  - constructor.
  - now apply Permutation_app_head.
  - rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
  - eapply Permutation_trans; eassumption.
Qed.

(** Replay may meet the calls in another order. Each recorded call still gets
    the record with its identity, so the replies are those of play. *)
Theorem replay_order_independent : forall size well_formed answer moment room calls calls',
  NoDup (map call_id calls) -> Permutation calls calls' ->
  replay (play size well_formed answer moment room calls) calls' =
    Some (deliver (play size well_formed answer moment room calls) calls') /\
  Permutation (deliver (play size well_formed answer moment room calls) calls')
    (play size well_formed answer moment room calls).
Proof.
  intros size well_formed answer moment room calls calls' unique permuted. split.
  - unfold replay. rewrite (placed_permutation _ _ _ permuted), placed_play. reflexivity.
  - rewrite <- (deliver_play size well_formed answer moment room calls unique) at 2.
    apply Permutation_sym. now apply deliver_permutation.
Qed.

(** The recorded successes fit the room, whatever the services answer. *)
Theorem recorded_success_within_room : forall size well_formed answer moment room calls,
  success_bytes size (play size well_formed answer moment room calls) <= room.
Proof.
  intros size well_formed answer moment room calls. revert moment room.
  induction calls as [|c rest IH]; intros moment room; simpl; [lia|].
  destruct (call_recorded c); [|apply IH].
  destruct (well_formed (call_request c)); [|simpl; apply IH].
  destruct (answer moment (call_request c)) as [value|]; [|simpl; apply IH].
  destruct (room_cost size value <=? room) eqn:fits; simpl; [|apply IH].
  apply Nat.leb_le in fits. specialize (IH (S moment) (room - room_cost size value)). lia.
Qed.

(** The record room of Casper's [native_record_room]: a third of the replay
    telemetry limit, a third of the deploy-log limit and half the evidence
    limit, whichever is smallest. *)
Definition record_room (telemetry deploy_log evidence : nat) : nat :=
  Nat.min (telemetry / 3) (Nat.min (deploy_log / 3) (evidence / 2)).

Lemma times_third_within : forall total, 3 * (total / 3) <= total.
Proof.
  intros total. pose proof (Nat.div_mod_eq total 3).
  pose proof (Nat.mod_upper_bound total 3 ltac:(lia)). lia.
Qed.

Lemma times_half_within : forall total, 2 * (total / 2) <= total.
Proof.
  intros total. pose proof (Nat.div_mod_eq total 2).
  pose proof (Nat.mod_upper_bound total 2 ltac:(lia)). lia.
Qed.

(** The room charge covers every copy of a record in each carrier that counts
    the record. The trace telemetry counts three copies: the introduction, the
    COMM copy and the repetition key. The clone-byte check of the offered
    deploy log also counts three copies, and its byte check counts two. The
    replay evidence counts two copies with an overhead for each item. The
    charge of a success is its encoded bytes plus the overhead, so the copies
    of the encoded bytes fit too ([record_bytes_within_success_bytes]). *)
Theorem charge_covers_record :
  forall size well_formed answer moment telemetry deploy_log evidence calls,
  let charged := success_bytes size
    (play size well_formed answer moment (record_room telemetry deploy_log evidence) calls) in
  3 * charged <= telemetry /\ 3 * charged <= deploy_log /\ 2 * charged <= evidence.
Proof.
  intros size well_formed answer moment telemetry deploy_log evidence calls charged.
  pose proof (recorded_success_within_room size well_formed answer moment
    (record_room telemetry deploy_log evidence) calls).
  pose proof (Nat.le_min_l (telemetry / 3) (Nat.min (deploy_log / 3) (evidence / 2))).
  pose proof (Nat.le_min_r (telemetry / 3) (Nat.min (deploy_log / 3) (evidence / 2))).
  pose proof (Nat.le_min_l (deploy_log / 3) (evidence / 2)).
  pose proof (Nat.le_min_r (deploy_log / 3) (evidence / 2)).
  pose proof (times_third_within telemetry). pose proof (times_third_within deploy_log).
  pose proof (times_half_within evidence).
  unfold record_room in *. subst charged. lia.
Qed.

(** Replay rejects a run in which a recorded call has no record. *)
Theorem missing_record_rejected : forall records calls c,
  In c calls -> call_recorded c = true -> lookup (call_id c) records = None ->
  replay records calls = None.
Proof.
  intros records calls c member recorded missing.
  unfold replay. destruct (placed records calls) eqn:checked; [|reflexivity].
  unfold placed in checked. apply andb_true_iff in checked as [covered _].
  rewrite forallb_forall in covered. specialize (covered c member).
  rewrite recorded in covered. unfold has_record in covered. now rewrite missing in covered.
Qed.

(** Replay rejects a record that no recorded call uses. A record on a plain
    call is one of them: E2 rejects it. *)
Theorem unused_record_rejected : forall records calls id r,
  In (id, r) records ->
  (forall c, In c calls -> call_recorded c = true -> call_id c <> id) ->
  replay records calls = None.
Proof.
  intros records calls id r member unused.
  unfold replay. destruct (placed records calls) eqn:checked; [|reflexivity].
  unfold placed in checked. apply andb_true_iff in checked as [_ used].
  rewrite forallb_forall in used. specialize (used (id, r) member).
  apply existsb_exists in used as [c [member_c matched]].
  apply andb_true_iff in matched as [recorded same]. apply Nat.eqb_eq in same.
  exfalso. exact (unused c member_c recorded same).
Qed.

(** Every recorded call is paid: the run completes with one reply for each
    recorded call, and the deploy pays for each reply. *)
Inductive run :=
| Completed (records : list record)
| Aborted (attempted : nat).

Definition paid_calls (outcome : run) : nat :=
  match outcome with
  | Completed records => length records
  | Aborted _ => 0
  end.

Definition attempted_calls (outcome : run) : nat :=
  match outcome with
  | Completed records => length records
  | Aborted attempted => attempted
  end.

Theorem every_attempted_call_is_paid : forall size well_formed answer moment room calls,
  let outcome := Completed (play size well_formed answer moment room calls) in
  paid_calls outcome = attempted_calls outcome /\
  paid_calls outcome = length (filter call_recorded calls).
Proof.
  intros size well_formed answer moment room calls outcome. split; [reflexivity|].
  pose proof (f_equal (@length nat)
    (every_call_has_one_reply size well_formed answer moment room calls)) as lengths.
  rewrite !length_map in lengths. exact lengths.
Qed.

(** * Negative controls *)

Definition probe (id : nat) : call := {| call_id := id; call_recorded := true; call_request := 0 |}.

Definition clock : service := fun moment _ => Some moment.

(** C1: a replay that asks its own service can get another answer. *)
Theorem live_replay_can_disagree :
  play (fun _ => 0) (fun _ => true) clock 0 overhead [probe 0] <>
  play (fun _ => 0) (fun _ => true) clock 1 overhead [probe 0].
Proof. discriminate. Qed.

(** C2: a replay that hands out the records in the order that it meets the
    calls gives a call the reply of another call. *)
Definition fifo_deliver (records : list record) (calls : list call) : list record :=
  combine (map call_id (filter call_recorded calls)) (map snd records).

Theorem fifo_replay_can_disagree :
  let records := play (fun _ => 0) (fun _ => true) clock 0 (2 * overhead) [probe 0; probe 1] in
  Permutation [probe 0; probe 1] [probe 1; probe 0] /\
  In (1, Success 0) (fifo_deliver records [probe 1; probe 0]) /\
  lookup 1 records = Some (Success 1).
Proof.
  simpl. split; [apply perm_swap|]. split; [now left|reflexivity].
Qed.

(** C3: admission without the room check records more than the room. *)
Fixpoint unbounded_play (size : nat -> nat) (answer : service) (moment : nat)
    (calls : list call) : list record :=
  match calls with
  | [] => []
  | c :: rest =>
      match answer moment (call_request c) with
      | Some value => (call_id c, Success value) :: unbounded_play size answer (S moment) rest
      | None => (call_id c, Failure ExtFailed) :: unbounded_play size answer (S moment) rest
      end
  end.

Theorem unbounded_admission_exceeds_room :
  0 < success_bytes (fun _ => 100) (unbounded_play (fun _ => 100) clock 0 [probe 0]) /\
  success_bytes (fun _ => 100) (play (fun _ => 100) (fun _ => true) clock 0 0 [probe 0]) = 0.
Proof. split; [simpl; lia|reflexivity]. Qed.

(** C4: a play that aborts on a failed service leaves the calls it made
    unpaid, because an aborted run is not a classified user failure. *)
Fixpoint aborting_play (answer : service) (moment attempted : nat) (calls : list call)
    (records : list record) : run :=
  match calls with
  | [] => Completed (rev records)
  | c :: rest =>
      match answer moment (call_request c) with
      | Some value =>
          aborting_play answer (S moment) (S attempted) rest ((call_id c, Success value) :: records)
      | None => Aborted (S attempted)
      end
  end.

Theorem aborting_failure_leaves_calls_unpaid :
  let outcome := aborting_play (fun _ _ => None) 0 0 [probe 0] [] in
  paid_calls outcome = 0 /\ attempted_calls outcome = 1 /\
  paid_calls (Completed (play (fun _ => 0) (fun _ => true) (fun _ _ => None) 0 0 [probe 0])) = 1.
Proof. repeat split. Qed.

(** C5: a replay that does not check the placement hands a forged record to
    a plain call of the user's code. The checked replay rejects it. *)
Definition unchecked_deliver (records : list record) (calls : list call) : list record :=
  flat_map (fun c => match lookup (call_id c) records with
                     | Some r => [(call_id c, r)]
                     | None => []
                     end) calls.

Definition user_call : call := {| call_id := 5; call_recorded := false; call_request := 0 |}.

Theorem unchecked_replay_accepts_a_misplaced_record :
  unchecked_deliver [(5, Success 7)] [user_call] = [(5, Success 7)] /\
  replay [(5, Success 7)] [user_call] = None.
Proof. split; reflexivity. Qed.

(** C6: two calls with one identity can get the wrong record. The identity of
    a call must be unique, as the produce hash with its random state is. *)
Theorem duplicate_ids_can_disagree :
  let records := play (fun _ => 0) (fun _ => true) clock 0 (2 * overhead) [probe 0; probe 0] in
  replay records [probe 0; probe 0] <> Some records.
Proof. simpl. discriminate. Qed.
