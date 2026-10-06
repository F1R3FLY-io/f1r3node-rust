(* C8 (epic 8946, B1 Phase C; decision record DR-87): the acquisition demand
   of a native execution holds one entry for each distinct obligation key
   instead of one entry for each located occurrence.

   A located occurrence is a counted row (key, quantity). The key is the
   located purse (its location and authority) with the measured class and the
   acquisition terms. Before C8 the demand held one row for each occurrence,
   so the witness, the obligation limits and the settlement key work grew with
   the number of occurrences. aggregate merges the rows of each key in
   first-occurrence order and sums their quantities.

   Results:
   - aggregate_preserves_counts: every key has the same counted quantity
     after aggregation, so the expansions are permutations of each other
     (aggregate_expansion_permutation).
   - aggregate_preserves_weighted_usage: the priced value of the rows does
     not change.
   - aggregate_preserves_partition and aggregate_preserves_exhaustion: the
     counted discharge (supply = used + unused, demand = used + fresh) and
     the exhaustion of supply hold for the aggregated required and fresh parts
     exactly when they hold for the per-occurrence parts.
   - aggregate_keys_distinct: the aggregated rows have distinct keys.
   - aggregated_payment_exact: draws that pay each occurrence exactly sum to
     a draw that pays the aggregated quantity exactly
     (join_counted_payments_is_exact of CountedFundingProjection).
   - occurrence_key_work_exceeds_aggregated: a negative control. The 2,899
     occurrences of eight one-node keys need 5,798 authority nodes for the
     required and fresh parts, above the limit of 4,096, and their aggregate
     needs 16.

   Rust correspondence: prepare_acquisition_demand in
   rholang/src/rust/interpreter/accounting/native_phlo_rules/acquisition.rs
   sorts the keys canonically instead of by first occurrence, which changes
   no counted quantity, and keeps a map from each occurrence to its entry.
   The extracted tests are aggregated_witness_checks_like_occurrence_witness,
   settlement_encodes_each_key_once and funding_flow_has_nine_keys
   (acquisition/tests/aggregation.rs). *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import CostAccountedSyntax EligibleFundingAssignment
  PrepaidResourceDischarge CountedFundingProjection.
Import ListNotations.

Fixpoint add_counted (key : prepaid_resource_key) (quantity : nat)
    (rows : counted_resources) : counted_resources :=
  match rows with
  | [] => [(key, quantity)]
  | (candidate, amount) :: rest =>
      if prepaid_key_eq_dec candidate key
      then (candidate, amount + quantity) :: rest
      else (candidate, amount) :: add_counted key quantity rest
  end.

Definition aggregate_from (aggregated rows : counted_resources) : counted_resources :=
  fold_left (fun aggregated row => add_counted (fst row) (snd row) aggregated) rows aggregated.

Definition aggregate (rows : counted_resources) : counted_resources := aggregate_from [] rows.

Lemma add_counted_quantity : forall key quantity rows sought,
  counted_quantity (add_counted key quantity rows) sought =
  counted_quantity rows sought + (if prepaid_key_eq_dec key sought then quantity else 0).
Proof.
  intros key quantity rows sought.
  induction rows as [|[candidate amount] rest IH]; cbn [add_counted counted_quantity].
  - destruct (prepaid_key_eq_dec key sought); lia.
  - destruct (prepaid_key_eq_dec candidate key) as [->|Different];
      cbn [counted_quantity].
    + destruct (prepaid_key_eq_dec key sought); lia.
    + rewrite IH. lia.
Qed.

Lemma aggregate_from_quantity : forall rows aggregated sought,
  counted_quantity (aggregate_from aggregated rows) sought =
  counted_quantity aggregated sought + counted_quantity rows sought.
Proof.
  induction rows as [|[key quantity] rest IH]; intros aggregated sought.
  - cbn. lia.
  - unfold aggregate_from in *. cbn [fold_left fst snd counted_quantity].
    rewrite IH, add_counted_quantity. destruct (prepaid_key_eq_dec key sought); lia.
Qed.

Theorem aggregate_preserves_counts : forall rows key,
  counted_quantity (aggregate rows) key = counted_quantity rows key.
Proof.
  intros rows key. unfold aggregate. rewrite aggregate_from_quantity. reflexivity.
Qed.

Corollary aggregate_expansion_permutation : forall rows,
  Permutation (expand_resources (aggregate rows)) (expand_resources rows).
Proof.
  intros rows. apply counted_equality_is_expanded_permutation.
  apply aggregate_preserves_counts.
Qed.

Lemma add_counted_value : forall price key quantity rows,
  counted_value price (add_counted key quantity rows) =
  counted_value price rows + quantity * price key.
Proof.
  intros price key quantity rows.
  induction rows as [|[candidate amount] rest IH]; cbn [add_counted counted_value].
  - lia.
  - destruct (prepaid_key_eq_dec candidate key) as [->|Different];
      cbn [counted_value].
    + rewrite Nat.mul_add_distr_r. lia.
    + rewrite IH. lia.
Qed.

Lemma aggregate_from_value : forall price rows aggregated,
  counted_value price (aggregate_from aggregated rows) =
  counted_value price aggregated + counted_value price rows.
Proof.
  intros price rows.
  induction rows as [|[key quantity] rest IH]; intros aggregated.
  - cbn. lia.
  - unfold aggregate_from in *. cbn [fold_left fst snd counted_value].
    rewrite IH, add_counted_value. lia.
Qed.

Theorem aggregate_preserves_weighted_usage : forall price rows,
  counted_value price (aggregate rows) = counted_value price rows.
Proof.
  intros price rows. unfold aggregate. rewrite aggregate_from_value. reflexivity.
Qed.

(* The Rust demand aggregates the required and the fresh parts. *)
Theorem aggregate_preserves_partition : forall available required used unused fresh,
  counted_discharge available required used unused fresh <->
  counted_discharge available (aggregate required) used unused (aggregate fresh).
Proof.
  intros available required used unused fresh. unfold counted_discharge.
  split; intros Discharge key; specialize (Discharge key);
    rewrite ?aggregate_preserves_counts in *; exact Discharge.
Qed.

Theorem aggregate_preserves_exhaustion : forall unused fresh,
  (forall key, counted_quantity unused key = 0 \/ counted_quantity fresh key = 0) <->
  (forall key, counted_quantity unused key = 0 \/ counted_quantity (aggregate fresh) key = 0).
Proof.
  intros unused fresh.
  split; intros Exhausted key; specialize (Exhausted key);
    rewrite ?aggregate_preserves_counts in *; exact Exhausted.
Qed.

Lemma add_counted_membership : forall key quantity rows sought,
  In sought (map fst (add_counted key quantity rows)) <->
  sought = key \/ In sought (map fst rows).
Proof.
  intros key quantity rows sought.
  induction rows as [|[candidate amount] rest IH]; cbn [add_counted map fst].
  - cbn. split; [intros [<-|[]]; left; reflexivity | intros [->|[]]; left; reflexivity].
  - destruct (prepaid_key_eq_dec candidate key) as [->|Different]; cbn [map fst In].
    + split; [intros Member; right; exact Member|].
      intros [->|Member]; [left; reflexivity | exact Member].
    + rewrite IH. tauto.
Qed.

Lemma add_counted_nodup : forall key quantity rows,
  NoDup (map fst rows) -> NoDup (map fst (add_counted key quantity rows)).
Proof.
  intros key quantity rows.
  induction rows as [|[candidate amount] rest IH]; intros Unique;
    cbn [add_counted map fst].
  - constructor; [intros []|constructor].
  - inversion Unique as [|? ? Absent Rest]; subst.
    destruct (prepaid_key_eq_dec candidate key) as [->|Different]; cbn [map fst].
    + constructor; assumption.
    + constructor; [|apply IH; exact Rest].
      rewrite add_counted_membership. intros [Same|Member]; [congruence | contradiction].
Qed.

Lemma aggregate_from_nodup : forall rows aggregated,
  NoDup (map fst aggregated) -> NoDup (map fst (aggregate_from aggregated rows)).
Proof.
  induction rows as [|[key quantity] rest IH]; intros aggregated Unique; [exact Unique|].
  unfold aggregate_from in *. cbn [fold_left fst snd].
  apply IH. apply add_counted_nodup. exact Unique.
Qed.

Theorem aggregate_keys_distinct : forall rows, NoDup (map fst (aggregate rows)).
Proof.
  intros rows. apply aggregate_from_nodup. constructor.
Qed.

Fixpoint counted_draw_sum (draws : list (nat -> nat)) : nat -> nat :=
  match draws with
  | [] => fun _ => 0
  | draw :: rest => fun source => draw source + counted_draw_sum rest source
  end.

(* Payments stay exact: when each occurrence of a key is paid exactly by its
   draw at the unit price, the summed draw pays the aggregated quantity
   exactly. *)
Theorem aggregated_payment_exact : forall sources allowed unit quantities draws,
  Forall2 (fun quantity draw => counted_payment_valid sources allowed (unit * quantity) draw)
    quantities draws ->
  counted_payment_valid sources allowed (unit * fold_right Nat.add 0 quantities)
    (counted_draw_sum draws).
Proof.
  intros sources allowed unit quantities draws Valid.
  induction Valid as [|quantity draw quantities draws First Rest IH];
    cbn [fold_right counted_draw_sum].
  - split.
    + rewrite Nat.mul_0_r. apply funding_sum_zero.
    + intros; reflexivity.
  - rewrite Nat.mul_add_distr_l.
    apply join_counted_payments_is_exact; assumption.
Qed.

(* The negative control: the authority-node work of the required and fresh
   parts, one node for each occurrence of a one-node key. *)
Definition key_work (rows : counted_resources) : nat :=
  fold_right (fun row total => authority_node_count (prepaid_authority (fst row)) + total) 0 rows.

Definition flow_key (index : nat) : prepaid_resource_key :=
  {| prepaid_location := Nat.div (Nat.modulo index 8) 4;
     prepaid_class := Nat.modulo index 4;
     prepaid_terms := 0;
     prepaid_authority := SGround [true] |}.

Definition flow_rows : counted_resources := map (fun index => (flow_key index, 1)) (seq 0 2899).

Example occurrence_key_work_exceeds_aggregated :
  4096 < 2 * key_work flow_rows /\
  2 * key_work (aggregate flow_rows) = 16 /\
  length (aggregate flow_rows) = 8.
Proof.
  split; [apply Nat.ltb_lt; vm_compute; reflexivity|].
  split; vm_compute; reflexivity.
Qed.
