From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment LexicographicMinimax.
Import ListNotations.

Fixpoint funding_vectors {A : Type} (count : nat) (alphabet : list A) : list (list A) :=
  match count with
  | 0 => [[]]
  | S rest => flat_map (fun head => map (cons head) (funding_vectors rest alphabet)) alphabet
  end.

Theorem funding_vectors_exact : forall A count (alphabet : list A) values,
  In values (funding_vectors count alphabet) <->
  length values = count /\ Forall (fun value => In value alphabet) values.
Proof.
  intros A count. induction count as [|count IH]; intros alphabet values; simpl.
  - split.
    + intros [<-|[]]. split; [reflexivity|constructor].
    + intros [empty _]. apply length_zero_iff_nil in empty. subst. auto.
  - rewrite in_flat_map. split.
    + intros [head [allowed included]]. apply in_map_iff in included.
      destruct included as [tail [<- included]]. apply IH in included.
      destruct included as [size valid]. simpl. split; [lia|now constructor].
    + destruct values as [|head tail]; simpl; intros [size valid]; [lia|].
      inversion valid; subst. exists head. split; [assumption|].
      apply in_map. apply IH. split; [lia|assumption].
Qed.

Definition funding_table := list (list nat).
Definition table_flow (table : funding_table) : funding_flow :=
  fun source obligation => nth obligation (nth source table []) 0.

Definition rectangular_funding_table sources obligations (table : funding_table) : Prop :=
  length table = sources /\ Forall (fun row => length row = obligations) table.

Definition bounded_funding_table sources obligations bound (table : funding_table) : Prop :=
  length table = sources /\
  Forall (fun row => length row = obligations /\ Forall (fun amount => amount <= bound) row) table.

Definition funding_tables sources obligations bound : list funding_table :=
  funding_vectors sources (funding_vectors obligations (seq 0 (S bound))).

Theorem funding_tables_exact : forall sources obligations bound table,
  In table (funding_tables sources obligations bound) <->
  bounded_funding_table sources obligations bound table.
Proof.
  intros sources obligations bound table. unfold funding_tables, bounded_funding_table.
  rewrite (funding_vectors_exact (list nat) sources (funding_vectors obligations (seq 0 (S bound))) table).
  split; intros [size rows]; split; [exact size| |exact size|].
  - rewrite Forall_forall in *. intros row included. specialize (rows row included).
    apply funding_vectors_exact in rows. destruct rows as [width allowed]. split; [exact width|].
    rewrite Forall_forall in *. intros amount inside. specialize (allowed amount inside).
    apply in_seq in allowed. lia.
  - rewrite Forall_forall in *. intros row included. specialize (rows row included).
    apply funding_vectors_exact. destruct rows as [width allowed]. split; [exact width|].
    rewrite Forall_forall in *. intros amount inside. apply in_seq. specialize (allowed amount inside). lia.
Qed.

Lemma funding_sum_contains_entry : forall count amount index,
  index < count -> amount index <= funding_sum count amount.
Proof.
  induction count; intros amount index inside; simpl; [lia|].
  destruct (Nat.eq_dec index count) as [->|different]; [lia|].
  specialize (IHcount amount index ltac:(lia)). lia.
Qed.

Theorem valid_funding_entry_is_bounded_by_total : forall sources obligations eligible capacity demand flow source obligation,
  assignment_valid sources obligations eligible capacity demand flow ->
  source < sources -> obligation < obligations ->
  flow source obligation <= funding_sum obligations demand.
Proof.
  intros sources obligations eligible capacity demand flow source obligation [_ columns] source_inside obligation_inside.
  pose proof (funding_sum_contains_entry sources (fun index => flow index obligation) source source_inside) as bound.
  unfold obligation_draw in columns. rewrite columns in bound by assumption.
  pose proof (funding_sum_contains_entry obligations demand obligation obligation_inside). lia.
Qed.

Theorem valid_rectangular_table_fits_search_bound : forall sources obligations eligible capacity demand table,
  rectangular_funding_table sources obligations table ->
  assignment_valid sources obligations eligible capacity demand (table_flow table) ->
  bounded_funding_table sources obligations (funding_sum obligations demand) table.
Proof.
  intros sources obligations eligible capacity demand table [height widths] valid.
  split; [exact height|]. rewrite Forall_forall in *. intros row included.
  split; [now apply widths|]. rewrite Forall_forall. intros amount inside.
  destruct (In_nth table row [] included) as [source [source_inside source_at]].
  destruct (In_nth row amount 0 inside) as [obligation [obligation_inside obligation_at]].
  assert (source_bound : source < sources) by lia.
  assert (obligation_bound : obligation < obligations) by (rewrite <- (widths row included); exact obligation_inside).
  pose proof (valid_funding_entry_is_bounded_by_total _ _ _ _ _ _ source obligation valid source_bound obligation_bound) as bounded.
  unfold table_flow in bounded. rewrite source_at, obligation_at in bounded. exact bounded.
Qed.

Definition complete_funding_candidates sources obligations eligible capacity demand : list funding_table :=
  filter (fun table => assignment_check sources obligations eligible capacity demand (table_flow table))
    (funding_tables sources obligations (funding_sum obligations demand)).

Theorem complete_funding_candidates_exact : forall sources obligations eligible capacity demand table,
  In table (complete_funding_candidates sources obligations eligible capacity demand) <->
  rectangular_funding_table sources obligations table /\
  assignment_valid sources obligations eligible capacity demand (table_flow table).
Proof.
  intros. unfold complete_funding_candidates.
  rewrite filter_In, funding_tables_exact, assignment_check_exact. split.
  - intros [[height rows] valid]. split; [|exact valid]. split; [exact height|].
    rewrite Forall_forall in *. intros row inside. now destruct (rows row inside).
  - intros [shape valid]. split; [eapply valid_rectangular_table_fits_search_bound; eassumption|exact valid].
Qed.

Theorem empty_candidate_set_iff_infeasible : forall sources obligations eligible capacity demand,
  complete_funding_candidates sources obligations eligible capacity demand = [] <->
  ~ exists table, rectangular_funding_table sources obligations table /\
      assignment_valid sources obligations eligible capacity demand (table_flow table).
Proof.
  intros. split.
  - intros empty [table valid]. apply complete_funding_candidates_exact in valid.
    rewrite empty in valid. contradiction.
  - intros impossible. destruct (complete_funding_candidates sources obligations eligible capacity demand) as [|table rest] eqn:found; auto.
    exfalso. apply impossible. exists table. apply complete_funding_candidates_exact.
    rewrite found. simpl; auto.
Qed.

Definition funding_table_contributions sources obligations table : list nat :=
  map (source_draw obligations (table_flow table)) (seq 0 sources).

Theorem complete_table_minimax_has_a_valid_assignment : forall sources obligations eligible capacity demand seed rest,
  map (funding_table_contributions sources obligations)
    (complete_funding_candidates sources obligations eligible capacity demand) = seed :: rest ->
  exists selected,
    rectangular_funding_table sources obligations selected /\
    assignment_valid sources obligations eligible capacity demand (table_flow selected) /\
    funding_table_contributions sources obligations selected = least_burden seed rest /\
    forall competing,
      rectangular_funding_table sources obligations competing ->
      assignment_valid sources obligations eligible capacity demand (table_flow competing) ->
      burden_le (funding_table_contributions sources obligations selected)
                (funding_table_contributions sources obligations competing).
Proof.
  intros sources obligations eligible capacity demand seed rest cover.
  pose proof (finite_minimax_is_a_supplied_candidate rest seed) as included.
  rewrite <- cover in included. apply in_map_iff in included.
  destruct included as [selected [selected_rank selected_valid]].
  apply complete_funding_candidates_exact in selected_valid.
  destruct selected_valid as [shape valid]. exists selected.
  split; [exact shape|]. split; [exact valid|]. split; [exact selected_rank|].
  intros competing competing_shape competing_valid. rewrite selected_rank.
  apply finite_minimax_is_optimal. rewrite <- cover. apply in_map.
  apply complete_funding_candidates_exact. auto.
Qed.

Definition candidate_source_capacity (left right source : nat) :=
  if Nat.eqb source 0 then left else right.

Definition candidate_unit_demand (_ : nat) := 1.

Definition candidate_graph (a b c d : bool) source obligation :=
  if Nat.eqb source 0 then (if Nat.eqb obligation 0 then a else b)
  else (if Nat.eqb obligation 0 then c else d).

Example source_restrictions_survive_complete_enumeration :
  complete_funding_candidates 2 2 (candidate_graph true true false true)
    (candidate_source_capacity 2 10) (fun column => if Nat.eqb column 0 then 2 else 1) =
    [[[2; 0]; [0; 1]]].
Proof. vm_compute. reflexivity. Qed.

Example aggregate_capacity_does_not_prove_feasibility :
  complete_funding_candidates 2 2 (candidate_graph true true false false)
    (candidate_source_capacity 1 100) candidate_unit_demand = [].
Proof. vm_compute. reflexivity. Qed.

Example three_sources_and_zero_capacity_are_supported :
  In [[0; 0]; [1; 0]; [0; 1]]
    (complete_funding_candidates 3 2 (fun _ _ => true)
      (fun source => if Nat.eqb source 0 then 0 else 1) candidate_unit_demand).
Proof. vm_compute. auto. Qed.

Example zero_obligations_preserve_source_positions :
  complete_funding_candidates 3 0 (fun _ _ => false) (fun _ => 0) (fun _ => 0) = [[[]; []; []]].
Proof. vm_compute. reflexivity. Qed.

Definition two_obligation_oracle left right a b c d : bool :=
  ((2 <=? left) && a && b) || ((2 <=? right) && c && d) ||
  ((1 <=? left) && (1 <=? right) && ((a && d) || (b && c))).

Example generated_complete_candidate_regression :
  forallb (fun left => forallb (fun right => forallb (fun a => forallb (fun b =>
    forallb (fun c => forallb (fun d =>
      Bool.eqb
        (negb (Nat.eqb (length (complete_funding_candidates 2 2 (candidate_graph a b c d)
          (candidate_source_capacity left right) candidate_unit_demand)) 0))
        (two_obligation_oracle left right a b c d))
      [false; true]) [false; true]) [false; true]) [false; true]) [0; 1; 2]) [0; 1; 2] = true.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions funding_vectors_exact.
Print Assumptions funding_tables_exact.
Print Assumptions valid_funding_entry_is_bounded_by_total.
Print Assumptions valid_rectangular_table_fits_search_bound.
Print Assumptions complete_funding_candidates_exact.
Print Assumptions empty_candidate_set_iff_infeasible.
Print Assumptions complete_table_minimax_has_a_valid_assignment.
