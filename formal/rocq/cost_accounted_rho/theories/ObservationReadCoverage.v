(* C12 (epic 8946, B1 Phase A; decision record DR-76): a metered COMM
   observation charges only what it reads.

   A metered computation is a sequence of events:
   - Reserve u adds u units to the host-work budget;
   - Read u is work that no other step meters, so units that are already
     reserved must cover it.
   A self-metered step reserves its own work and then performs it, so it
   appears as the balanced pair Reserve u; Read u.

   The metering rule is prefix coverage: every prefix of the run reads at
   most the units that the same prefix has reserved.

   The metered COMM observation (comm_metered) performs this work:
   - it walks each matched datum without a meter (message_bytes), so each
     datum needs a prior inspection;
   - it hashes the COMM inside cost_identity_metered, which is self-metered;
   - it merges the continuation and datum authorities inside the
     self-metered authority functions.
   It never reads the continuation body or guard.

   The legacy construction also inspected the whole COMM and the whole
   continuation before this work.

   Results:
   - trace_covered and legacy_trace_covered: both runs satisfy the rule.
   - trace_reads_equal_legacy_reads: both runs perform the same reads.
   - legacy_excess: the legacy run reserves exactly one COMM inspection and
     one continuation inspection more.
   - charge_independent_of_body_and_guard: the C12 charge does not depend on
     the continuation body or guard.
   - legacy_charge_depends_on_unread_body: a proved counterexample shows that
     the legacy charge grows with the unread body.

   Rust correspondence: rholang/src/rust/interpreter/accounting/
   observation_construction.rs (comm_metered); the extracted property tests
   are in rholang/src/rust/interpreter/accounting/native_runtime/tests/
   observation_construction.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Inductive event := Reserve (units : nat) | Read (units : nat).

Definition reserved_units (e : event) : nat :=
  match e with Reserve units => units | Read _ => 0 end.

Definition read_units (e : event) : nat :=
  match e with Read units => units | Reserve _ => 0 end.

Definition total (project : event -> nat) (trace : list event) : nat :=
  fold_right Nat.add 0 (map project trace).

Lemma total_app : forall project left right,
  total project (left ++ right) = total project left + total project right.
Proof.
  intros project left right. unfold total.
  rewrite map_app, fold_right_app.
  induction left as [| e rest IH]; simpl; [reflexivity |].
  unfold total in IH. rewrite IH. lia.
Qed.

(* Every prefix reads at most [slack] units more than it reserves. *)
Definition covered_from (slack : nat) (trace : list event) : Prop :=
  forall prefix suffix, trace = prefix ++ suffix ->
    total read_units prefix <= slack + total reserved_units prefix.

Definition covered (trace : list event) : Prop := covered_from 0 trace.

Lemma covered_from_app : forall slack left right,
  covered_from slack left ->
  total read_units left <= slack + total reserved_units left ->
  covered_from (slack + total reserved_units left - total read_units left) right ->
  covered_from slack (left ++ right).
Proof.
  intros slack left right covered_left fits covered_right prefix suffix split.
  apply app_eq_app in split as [middle [[left_prefix right_suffix] | [prefix_left right_middle]]].
  - (* left = prefix ++ middle: the prefix ends inside [left] *)
    exact (covered_left prefix middle left_prefix).
  - (* prefix = left ++ middle: the prefix covers all of [left] *)
    subst prefix.
    pose proof (covered_right middle suffix right_middle) as inner.
    rewrite !total_app. lia.
Qed.

Lemma reserves_covered : forall slack amounts,
  covered_from slack (map Reserve amounts).
Proof.
  intros slack amounts prefix suffix split.
  assert (zero : total read_units prefix = 0).
  { revert prefix suffix split.
    induction amounts as [| amount rest IH]; intros prefix suffix split.
    - destruct prefix; [reflexivity | discriminate].
    - destruct prefix as [| e prefix]; [reflexivity |].
      simpl in split. injection split as head tail. subst e.
      unfold total in *. simpl. exact (IH prefix suffix tail). }
  lia.
Qed.

Lemma read_total : forall amounts,
  total read_units (map Read amounts) = fold_right Nat.add 0 amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  unfold total in *. simpl. f_equal. exact IH.
Qed.

Lemma reads_covered : forall slack amounts,
  fold_right Nat.add 0 amounts <= slack ->
  covered_from slack (map Read amounts).
Proof.
  intros slack amounts bound prefix suffix split.
  assert (le_total : total read_units prefix <= total read_units (map Read amounts)).
  { rewrite split, total_app. lia. }
  rewrite read_total in le_total. lia.
Qed.

Definition self_metered (units : nat) : list event := [Reserve units; Read units].

Lemma self_metered_covered : forall slack units, covered_from slack (self_metered units).
Proof.
  intros slack units prefix suffix split.
  unfold self_metered in split.
  destruct prefix as [| a [| b [| c rest]]]; simpl in split.
  - unfold total. simpl. lia.
  - injection split as ha _. subst a. unfold total. simpl. lia.
  - injection split as ha hb _. subst a b. unfold total. simpl. lia.
  - discriminate.
Qed.

Lemma self_metered_balanced : forall units,
  total read_units (self_metered units) = total reserved_units (self_metered units).
Proof. intros units. unfold total, self_metered. simpl. lia. Qed.

Lemma flat_self_metered_covered : forall slack amounts,
  covered_from slack (flat_map self_metered amounts).
Proof.
  intros slack amounts. induction amounts as [| amount rest IH].
  - intros prefix suffix split. destruct prefix; [unfold total; simpl; lia | discriminate].
  - change (covered_from slack (self_metered amount ++ flat_map self_metered rest)).
    apply covered_from_app.
    + apply self_metered_covered.
    + rewrite self_metered_balanced. lia.
    + rewrite self_metered_balanced. replace (slack + _ - _) with slack by lia. exact IH.
Qed.

Lemma flat_self_metered_balanced : forall amounts,
  total read_units (flat_map self_metered amounts) =
  total reserved_units (flat_map self_metered amounts).
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  change (flat_map self_metered (amount :: rest))
    with (self_metered amount ++ flat_map self_metered rest).
  rewrite !total_app, self_metered_balanced, IH. reflexivity.
Qed.

Record continuation := { body : nat; guard : nat; authority : nat }.

Record comm_input := {
  comm_size : nat;
  continuation_of : continuation;
  data : list nat;
  data_authorities : list nat
}.

Definition continuation_inspection (k : continuation) : nat := body k + guard k + authority k.

(* The C12 run: inspect each datum, then the self-metered identity hash and
   authority merges, then the unmetered datum walks. *)
Definition c12_trace (x : comm_input) : list event :=
  map Reserve (data x)
  ++ self_metered (comm_size x)
  ++ flat_map self_metered (authority (continuation_of x) :: data_authorities x)
  ++ map Read (data x).

(* The legacy run inspected the COMM and the continuation first. *)
Definition legacy_trace (x : comm_input) : list event :=
  Reserve (comm_size x) :: Reserve (continuation_inspection (continuation_of x)) :: c12_trace x.

Lemma reserve_total : forall amounts,
  total reserved_units (map Reserve amounts) = fold_right Nat.add 0 amounts.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  unfold total in *. simpl. f_equal. exact IH.
Qed.

Lemma reserve_reads : forall amounts, total read_units (map Reserve amounts) = 0.
Proof.
  induction amounts as [| amount rest IH]; [reflexivity |].
  unfold total in *. simpl. exact IH.
Qed.

Theorem trace_covered : forall x, covered (c12_trace x).
Proof.
  intros x. unfold covered, c12_trace.
  apply covered_from_app.
  - apply reserves_covered.
  - rewrite reserve_reads. lia.
  - rewrite reserve_reads, reserve_total, Nat.sub_0_r, Nat.add_0_l.
    apply covered_from_app.
    + apply self_metered_covered.
    + rewrite self_metered_balanced. lia.
    + rewrite self_metered_balanced, Nat.add_sub.
      apply covered_from_app.
      * apply flat_self_metered_covered.
      * rewrite flat_self_metered_balanced. lia.
      * rewrite flat_self_metered_balanced, Nat.add_sub.
        apply reads_covered. lia.
Qed.

Theorem legacy_trace_covered : forall x, covered (legacy_trace x).
Proof.
  intros x. unfold covered, legacy_trace.
  change (Reserve (comm_size x) :: Reserve (continuation_inspection (continuation_of x)) :: c12_trace x)
    with (map Reserve [comm_size x; continuation_inspection (continuation_of x)] ++ c12_trace x).
  apply covered_from_app.
  - apply reserves_covered.
  - rewrite reserve_reads. lia.
  - rewrite reserve_reads, reserve_total.
    intros prefix suffix split.
    pose proof (trace_covered x prefix suffix split). lia.
Qed.

Theorem trace_reads_equal_legacy_reads : forall x,
  total read_units (legacy_trace x) = total read_units (c12_trace x).
Proof. intros x. unfold legacy_trace, total. simpl. reflexivity. Qed.

Theorem legacy_excess : forall x,
  total reserved_units (legacy_trace x) =
  total reserved_units (c12_trace x) + comm_size x + continuation_inspection (continuation_of x).
Proof. intros x. unfold legacy_trace, total. simpl. lia. Qed.

Definition with_body_and_guard (x : comm_input) (new_body new_guard : nat) : comm_input :=
  {| comm_size := comm_size x;
     continuation_of := {| body := new_body; guard := new_guard;
                           authority := authority (continuation_of x) |};
     data := data x;
     data_authorities := data_authorities x |}.

Theorem charge_independent_of_body_and_guard : forall x new_body new_guard,
  c12_trace (with_body_and_guard x new_body new_guard) = c12_trace x.
Proof. intros x new_body new_guard. reflexivity. Qed.

Theorem legacy_charge_depends_on_unread_body : exists x new_body,
  total read_units (legacy_trace (with_body_and_guard x new_body (guard (continuation_of x)))) =
    total read_units (legacy_trace x) /\
  total reserved_units (legacy_trace (with_body_and_guard x new_body (guard (continuation_of x)))) <>
    total reserved_units (legacy_trace x).
Proof.
  exists {| comm_size := 1; continuation_of := {| body := 0; guard := 0; authority := 1 |};
            data := [2]; data_authorities := [1] |}, 1000.
  split; vm_compute; [reflexivity | discriminate].
Qed.

Print Assumptions trace_covered.
Print Assumptions legacy_trace_covered.
Print Assumptions trace_reads_equal_legacy_reads.
Print Assumptions legacy_excess.
Print Assumptions charge_independent_of_body_and_guard.
Print Assumptions legacy_charge_depends_on_unread_body.
