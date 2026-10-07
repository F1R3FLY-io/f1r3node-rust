From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate CompleteFundingCandidates FundingBox FundingCellBound LexicographicMinimax.
Import ListNotations.

Lemma funding_sum_and_constant : forall count chosen left right flag,
  funding_sum count (fun i => if chosen i && flag then left i else right i) =
  if flag then funding_sum count (fun i => if chosen i then left i else right i) else funding_sum count right.
Proof.
  intros count chosen left right [|]; apply funding_sum_ext; intros i _; destruct (chosen i); reflexivity.
Qed.

Definition erase_funding_entry (flow : funding_flow) source target i j :=
  if Nat.eqb i source && Nat.eqb j target then 0 else flow i j.

Definition restore_funding_entry (flow : funding_flow) source target amount i j :=
  if Nat.eqb i source && Nat.eqb j target then amount else flow i j.

Definition funding_entry_residual_values values index amount i :=
  if Nat.eqb i index then values i - amount else values i.

Definition funding_entry_residual_edges eligible source target i j :=
  eligible i j && negb (Nat.eqb i source && Nat.eqb j target).

Lemma erased_entry_row_balance : forall obligations flow source target,
  target < obligations ->
  source_draw obligations (erase_funding_entry flow source target) source + flow source target =
    source_draw obligations flow source.
Proof.
  intros obligations flow source target inside. unfold source_draw, erase_funding_entry.
  rewrite Nat.eqb_refl. simpl. pose proof (funding_sum_replace obligations (flow source) target 0 inside). lia.
Qed.

Lemma erased_entry_column_balance : forall sources flow source target,
  source < sources ->
  obligation_draw sources (erase_funding_entry flow source target) target + flow source target =
    obligation_draw sources flow target.
Proof.
  intros sources flow source target inside. unfold obligation_draw, erase_funding_entry.
  rewrite Nat.eqb_refl, funding_sum_and_constant.
  pose proof (funding_sum_replace sources (fun i => flow i target) source 0 inside). lia.
Qed.

Theorem erasing_funding_entry_preserves_residual_assignment : forall sources obligations eligible capacity demand flow source target,
  source < sources -> target < obligations ->
  assignment_valid sources obligations eligible capacity demand flow ->
  assignment_valid sources obligations (funding_entry_residual_edges eligible source target)
    (funding_entry_residual_values capacity source (flow source target))
    (funding_entry_residual_values demand target (flow source target))
    (erase_funding_entry flow source target).
Proof.
  intros sources obligations eligible capacity demand flow source target si ti [rows columns].
  split.
  - intros i ii. destruct (rows i ii) as [bound edges]. split.
    + unfold funding_entry_residual_values. destruct (Nat.eqb i source) eqn:same.
      * apply Nat.eqb_eq in same. subst i. pose proof (erased_entry_row_balance obligations flow source target ti). lia.
      * unfold source_draw, erase_funding_entry. rewrite same. exact bound.
    + intros j ji excluded. unfold funding_entry_residual_edges in excluded.
      unfold erase_funding_entry. destruct (Nat.eqb i source && Nat.eqb j target) eqn:chosen; [reflexivity|].
      simpl in excluded. rewrite andb_true_r in excluded. now apply edges.
  - intros j ji. unfold funding_entry_residual_values. destruct (Nat.eqb j target) eqn:same.
    + apply Nat.eqb_eq in same. subst j. pose proof (erased_entry_column_balance sources flow source target si).
      rewrite columns in H by exact ti. lia.
    + unfold obligation_draw, erase_funding_entry. rewrite same, funding_sum_and_constant. now apply columns.
Qed.

Theorem entry_residual_totals_remain_equal : forall sources obligations eligible capacity demand flow source target,
  source < sources -> target < obligations ->
  assignment_valid sources obligations eligible capacity demand flow ->
  funding_sum sources capacity = funding_sum obligations demand ->
  funding_sum sources (funding_entry_residual_values capacity source (flow source target)) =
    funding_sum obligations (funding_entry_residual_values demand target (flow source target)).
Proof.
  intros sources obligations eligible capacity demand flow source target si ti valid total.
  pose proof (funding_sum_contains_entry obligations (flow source) target ti) as cell_row.
  pose proof (funding_sum_contains_entry sources (fun i => flow i target) source si) as cell_column.
  pose proof (proj1 (proj1 valid source si)) as row.
  pose proof (proj2 valid target ti) as column.
  unfold source_draw in row. unfold obligation_draw in column.
  assert (left : funding_sum sources (funding_entry_residual_values capacity source (flow source target)) + capacity source =
    funding_sum sources capacity + (capacity source - flow source target)).
  { unfold funding_entry_residual_values.
    transitivity (funding_sum sources (fun i => if i =? source then capacity source - flow source target else capacity i) + capacity source).
    - f_equal. apply funding_sum_ext. intros i _. destruct (Nat.eqb_spec i source); subst; reflexivity.
    - apply funding_sum_replace. exact si. }
  assert (right : funding_sum obligations (funding_entry_residual_values demand target (flow source target)) + demand target =
    funding_sum obligations demand + (demand target - flow source target)).
  { unfold funding_entry_residual_values.
    transitivity (funding_sum obligations (fun j => if j =? target then demand target - flow source target else demand j) + demand target).
    - f_equal. apply funding_sum_ext. intros j _. destruct (Nat.eqb_spec j target); subst; reflexivity.
    - apply funding_sum_replace. exact ti. }
  lia.
Qed.

Theorem restoring_erased_funding_entry_is_identity : forall flow source target i j,
  restore_funding_entry (erase_funding_entry flow source target) source target (flow source target) i j = flow i j.
Proof.
  intros. unfold restore_funding_entry, erase_funding_entry.
  destruct (Nat.eqb i source && Nat.eqb j target) eqn:same; [|reflexivity].
  apply andb_true_iff in same. destruct same as [row column]. apply Nat.eqb_eq in row, column. now subst.
Qed.

Theorem restoring_funding_entry_preserves_assignment : forall sources obligations eligible capacity demand flow source target amount,
  source < sources -> target < obligations -> amount <= capacity source -> amount <= demand target ->
  (amount = 0 \/ eligible source target = true) ->
  assignment_valid sources obligations (funding_entry_residual_edges eligible source target)
    (funding_entry_residual_values capacity source amount) (funding_entry_residual_values demand target amount) flow ->
  assignment_valid sources obligations eligible capacity demand (restore_funding_entry flow source target amount).
Proof.
  intros sources obligations eligible capacity demand flow source target amount si ti row_bound column_bound permitted [rows columns].
  assert (empty : flow source target = 0).
  { apply (proj2 (rows source si) target ti). unfold funding_entry_residual_edges. rewrite !Nat.eqb_refl. simpl. apply andb_false_r. }
  split.
  - intros i ii. destruct (rows i ii) as [bound edges]. split.
    + unfold funding_entry_residual_values in bound. destruct (Nat.eqb i source) eqn:same.
      * apply Nat.eqb_eq in same. subst i. unfold source_draw, restore_funding_entry. rewrite Nat.eqb_refl. simpl.
        pose proof (funding_sum_replace obligations (flow source) target amount ti).
        unfold source_draw in bound. lia.
      * unfold source_draw, restore_funding_entry. rewrite same. exact bound.
    + intros j ji excluded. unfold restore_funding_entry.
      destruct (Nat.eqb i source && Nat.eqb j target) eqn:same.
      * apply andb_true_iff in same. destruct same as [row column]. apply Nat.eqb_eq in row, column. subst.
        destruct permitted; congruence.
      * apply edges; [exact ji|]. unfold funding_entry_residual_edges. now rewrite excluded.
  - intros j ji. specialize (columns j ji). unfold funding_entry_residual_values in columns.
    destruct (Nat.eqb j target) eqn:same.
    + apply Nat.eqb_eq in same. subst j. unfold obligation_draw, restore_funding_entry.
      rewrite Nat.eqb_refl, funding_sum_and_constant.
      pose proof (funding_sum_replace sources (fun i => flow i target) source amount si).
      unfold obligation_draw in columns. lia.
    + unfold obligation_draw, restore_funding_entry. rewrite same, funding_sum_and_constant. exact columns.
Qed.

Definition funding_entry := (nat * nat)%type.
Definition funding_entry_value (flow : funding_flow) (entry : funding_entry) := flow (fst entry) (snd entry).

Fixpoint prefix_minimal_funding_witness order (feasible : funding_flow -> Prop) candidate :=
  match order with
  | [] => True
  | entry :: rest =>
    (forall alternative, feasible alternative -> funding_entry_value candidate entry <= funding_entry_value alternative entry) /\
    prefix_minimal_funding_witness rest
      (fun alternative => feasible alternative /\ funding_entry_value alternative entry = funding_entry_value candidate entry) candidate
  end.

Theorem prefix_minimal_funding_witness_is_lexicographically_least : forall order feasible candidate,
  prefix_minimal_funding_witness order feasible candidate ->
  forall alternative, feasible alternative ->
    funding_lex_le (map (funding_entry_value candidate) order) (map (funding_entry_value alternative) order).
Proof.
  induction order as [|entry rest IH]; intros feasible candidate certificate alternative valid; simpl in *; [trivial|].
  destruct certificate as [head tail]. specialize (head alternative valid).
  destruct (Nat.lt_ge_cases (funding_entry_value candidate entry) (funding_entry_value alternative entry)).
  - now left.
  - right. assert (same : funding_entry_value candidate entry = funding_entry_value alternative entry) by lia.
    split; [exact same|]. apply (IH _ candidate tail alternative). auto.
Qed.

Theorem least_funding_witness_is_unique_on_complete_order : forall sources obligations order left right,
  (forall i j, i < sources -> j < obligations -> In (i,j) order) ->
  funding_lex_le (map (funding_entry_value left) order) (map (funding_entry_value right) order) ->
  funding_lex_le (map (funding_entry_value right) order) (map (funding_entry_value left) order) ->
  forall i j, i < sources -> j < obligations -> left i j = right i j.
Proof.
  intros sources obligations order left right complete lr rl i j ii jj.
  pose proof (funding_lex_antisymmetric _ _ lr rl) as same.
  specialize (complete i j ii jj). clear lr rl ii jj. revert same complete.
  induction order as [|entry rest IH]; simpl; intros same included; [contradiction|].
  inversion same. destruct included as [equal|tail]; [subst entry; assumption|]. now apply IH.
Qed.

Lemma erasing_entry_preserves_unselected_order : forall order flow source target,
  ~ In (source,target) order ->
  map (funding_entry_value (erase_funding_entry flow source target)) order = map (funding_entry_value flow) order.
Proof.
  intros order flow source target absent. apply map_ext_in. intros [i j] included.
  unfold funding_entry_value, erase_funding_entry. simpl.
  destruct (Nat.eqb i source && Nat.eqb j target) eqn:same; [|reflexivity].
  apply andb_true_iff in same. destruct same as [row column]. apply Nat.eqb_eq in row, column.
  subst. contradiction.
Qed.

Fixpoint funding_witness_prefix_certificate sources obligations eligible capacity demand candidate order :=
  match order with
  | [] => True
  | (source,target) :: rest =>
    source < sources /\ target < obligations /\ ~ In (source,target) rest /\
    (candidate source target = 0 \/ exists selected,
      funding_deficit_check (S sources) obligations (funding_cell_edges sources eligible source target)
        (funding_cell_cap sources capacity source (candidate source target - 1)) demand selected = true) /\
    funding_witness_prefix_certificate sources obligations
      (funding_entry_residual_edges eligible source target)
      (funding_entry_residual_values capacity source (candidate source target))
      (funding_entry_residual_values demand target (candidate source target))
      (erase_funding_entry candidate source target) rest
  end.

Theorem checked_witness_prefix_is_lexicographically_least : forall order sources obligations eligible capacity demand candidate,
  assignment_valid sources obligations eligible capacity demand candidate ->
  funding_sum sources capacity = funding_sum obligations demand ->
  funding_witness_prefix_certificate sources obligations eligible capacity demand candidate order ->
  forall alternative, assignment_valid sources obligations eligible capacity demand alternative ->
    funding_lex_le (map (funding_entry_value candidate) order) (map (funding_entry_value alternative) order).
Proof.
  induction order as [|[source target] rest IH]; intros sources obligations eligible capacity demand candidate valid total certificate alternative alternative_valid;
    simpl in *; [trivial|].
  destruct certificate as [si [ti [absent [evidence tail]]]].
  assert (minimum : candidate source target <= alternative source target).
  { destruct evidence as [zero|[selected deficit]]; [lia|].
    pose proof (funding_sum_contains_entry obligations (candidate source) target ti) as entry.
    pose proof (proj1 (proj1 valid source si)) as bounded. unfold source_draw in bounded.
    pose proof (funding_cell_cut_proves_lower_bound sources obligations eligible capacity demand source target
      (candidate source target - 1) selected si ti ltac:(lia) total deficit alternative alternative_valid). lia. }
  unfold funding_entry_value at 1 2. simpl.
  destruct (Nat.lt_ge_cases (candidate source target) (alternative source target)).
  - now left.
  - right. assert (same : candidate source target = alternative source target) by lia.
    split; [exact same|].
    assert (candidate_residual := erasing_funding_entry_preserves_residual_assignment _ _ _ _ _ _ _ _ si ti valid).
    assert (alternative_residual := erasing_funding_entry_preserves_residual_assignment _ _ _ _ _ _ _ _ si ti alternative_valid).
    rewrite <- same in alternative_residual.
    pose proof (entry_residual_totals_remain_equal _ _ _ _ _ _ _ _ si ti valid total) as residual_total.
    pose proof (IH sources obligations (funding_entry_residual_edges eligible source target)
      (funding_entry_residual_values capacity source (candidate source target))
      (funding_entry_residual_values demand target (candidate source target))
      (erase_funding_entry candidate source target) candidate_residual residual_total tail
      (erase_funding_entry alternative source target) alternative_residual) as least.
    rewrite !erasing_entry_preserves_unselected_order in least by exact absent. exact least.
Qed.

Definition funding_row_major_order sources obligations : list funding_entry :=
  flat_map (fun i => map (fun j => (i,j)) (seq 0 obligations)) (seq 0 sources).

Lemma funding_entry_row_is_unique : forall (source : nat) (targets : list nat),
  NoDup targets -> NoDup (map (fun target => (source,target)) targets).
Proof.
  intros source targets distinct. induction distinct; simpl; constructor; auto.
  intro member. apply in_map_iff in member. destruct member as [target [same included]].
  inversion same. subst. contradiction.
Qed.

Lemma funding_entry_rows_are_unique : forall (sources targets : list nat),
  NoDup sources -> NoDup targets ->
  NoDup (flat_map (fun source => map (fun target => (source,target)) targets) sources).
Proof.
  intros sources targets distinct target_distinct. induction distinct; simpl; [constructor|].
  apply NoDup_app; [apply funding_entry_row_is_unique; assumption|exact IHdistinct|].
  intros entry first later. apply in_map_iff in first. apply in_flat_map in later.
  destruct first as [target [same included]]. destruct later as [source [source_in row_in]].
  apply in_map_iff in row_in. destruct row_in as [other [equal other_in]].
  rewrite <- same in equal. inversion equal. subst. contradiction.
Qed.

Theorem funding_row_major_order_visits_each_entry_once : forall sources obligations,
  NoDup (funding_row_major_order sources obligations).
Proof.
  intros. unfold funding_row_major_order. apply funding_entry_rows_are_unique; apply seq_NoDup.
Qed.

Theorem funding_row_major_order_is_complete : forall sources obligations i j,
  i < sources -> j < obligations -> In (i,j) (funding_row_major_order sources obligations).
Proof.
  intros sources obligations i j ii jj. unfold funding_row_major_order. apply in_flat_map.
  exists i. split; [apply in_seq; lia|]. apply in_map_iff. exists j. split; [reflexivity|apply in_seq; lia].
Qed.

Theorem checked_row_major_witnesses_are_identical : forall sources obligations eligible capacity demand left right,
  assignment_valid sources obligations eligible capacity demand left ->
  assignment_valid sources obligations eligible capacity demand right ->
  funding_sum sources capacity = funding_sum obligations demand ->
  funding_witness_prefix_certificate sources obligations eligible capacity demand left (funding_row_major_order sources obligations) ->
  funding_witness_prefix_certificate sources obligations eligible capacity demand right (funding_row_major_order sources obligations) ->
  forall i j, i < sources -> j < obligations -> left i j = right i j.
Proof.
  intros sources obligations eligible capacity demand left right lv rv total lc rc.
  apply (least_funding_witness_is_unique_on_complete_order sources obligations (funding_row_major_order sources obligations)).
  - apply funding_row_major_order_is_complete.
  - exact (checked_witness_prefix_is_lexicographically_least _ _ _ _ _ _ _ lv total lc right rv).
  - exact (checked_witness_prefix_is_lexicographically_least _ _ _ _ _ _ _ rv total rc left lv).
Qed.
