From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingResidualGraph FundingNetworkAugmentation FundingNetworkProjection.
From CostAccountedRho Require Import FundingParentPath EligibleFundingAssignment.
Import ListNotations.

Definition operation_vertices from to start operations :=
  start :: map (operation_to from to) operations.

Lemma linked_member_endpoints : forall operations from to start finish op,
  linked_operations from to start operations finish ->
  In op operations ->
  In (operation_from from to op) (operation_vertices from to start operations) /\
  In (operation_to from to op) (operation_vertices from to start operations).
Proof.
  induction operations as [|head rest IH]; intros from to start finish op linked member; [contradiction|].
  destruct linked as [begins tail]. destruct member as [same|member].
  - subst op. split; [left; symmetry; exact begins|right; left; reflexivity].
  - destruct (IH from to (operation_to from to head) finish op tail member) as [origin target].
    split.
    + destruct origin as [equal|included]; [right; left; exact equal|right; right; exact included].
    + destruct target as [equal|included]; [right; left; exact equal|right; right; exact included].
Qed.

Lemma identical_pair_shares_origin : forall from to first second,
  fst first = fst second ->
  operation_from from to first = operation_from from to second \/
  operation_from from to first = operation_to from to second.
Proof.
  intros from to [first direction] [second other] same. simpl in same. subst second.
  destruct direction, other; unfold operation_from, operation_to; simpl; auto.
Qed.

Theorem simple_linked_path_has_distinct_pairs : forall operations from to start finish,
  linked_operations from to start operations finish ->
  NoDup (operation_vertices from to start operations) ->
  NoDup (map fst operations).
Proof.
  induction operations as [|head rest IH]; intros from to start finish linked simple; [constructor|].
  destruct linked as [begins tail].
  apply NoDup_cons_iff in simple. destruct simple as [absent distinct].
  simpl. constructor.
  - intros repeated. apply in_map_iff in repeated. destruct repeated as [other [same member]].
    destruct (linked_member_endpoints rest from to (operation_to from to head) finish other tail member) as [origin target].
    destruct (identical_pair_shares_origin from to head other ltac:(symmetry; exact same)) as [equal|equal].
    + rewrite begins in equal. apply absent. rewrite equal. exact origin.
    + rewrite begins in equal. apply absent. rewrite equal. exact target.
  - eapply IH; [exact tail|exact distinct].
Qed.

Theorem simple_funding_path_increases_funding : forall count sources obligations kinds state eligible capacity demand operations limit amount,
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  network_bounded count limit state ->
  NoDup (operation_vertices (fun index => pair_from sources (kinds index))
    (fun index => pair_to sources obligations (kinds index)) 0 operations) ->
  (forall op, In op operations -> fst op < count /\ amount <= operation_capacity state op) ->
  linked_operations (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
    0 operations (funding_sink sources obligations) ->
  exists next,
    network_augment limit state (rev operations) amount = Some next /\
    funding_network_valid count sources obligations kinds next eligible capacity demand /\
    EligibleFundingAssignment.funding_sum obligations
      (EligibleFundingAssignment.obligation_draw sources (projected_assignment count kinds next)) =
      EligibleFundingAssignment.funding_sum obligations
        (EligibleFundingAssignment.obligation_draw sources (projected_assignment count kinds state)) + amount /\
    EligibleFundingAssignment.funding_sum obligations
      (EligibleFundingAssignment.obligation_draw sources (projected_assignment count kinds next)) <=
      EligibleFundingAssignment.funding_sum obligations demand.
Proof.
  intros count sources obligations kinds state eligible capacity demand operations limit amount valid bounded simple permitted linked.
  eapply reverse_path_increases_funding_exactly; try eassumption.
  eapply simple_linked_path_has_distinct_pairs; eassumption.
Qed.

Theorem simple_path_operation_count : forall operations from to start,
  length (operation_vertices from to start operations) = S (length operations).
Proof. intros. unfold operation_vertices. simpl. now rewrite length_map. Qed.

Lemma linked_operations_append : forall left right from to start middle finish,
  linked_operations from to start left middle -> linked_operations from to middle right finish ->
  linked_operations from to start (left ++ right) finish.
Proof.
  induction left as [|op rest IH]; intros right from to start middle finish first second; simpl in *.
  - subst middle. exact second.
  - destruct first as [begins tail]. split; [exact begins|]. eapply IH; eauto.
Qed.

Fixpoint extract_parent_operations fuel root from to (parent_op : nat -> residual_operation) node :=
  if Nat.eqb node root then Some []
  else match fuel with
  | 0 => None
  | S rest => option_map (fun ops => ops ++ [parent_op node])
      (extract_parent_operations rest root from to parent_op (operation_from from to (parent_op node)))
  end.

Theorem ranked_parents_extract_linked_simple_operations : forall fuel root from to parent_op known rank node,
  (forall current, known current = true -> current <> root ->
    operation_to from to (parent_op current) = current /\
    known (operation_from from to (parent_op current)) = true /\
    rank (operation_from from to (parent_op current)) < rank current) ->
  known node = true -> rank node <= fuel ->
  exists ops,
    extract_parent_operations fuel root from to parent_op node = Some ops /\
    linked_operations from to root ops node /\
    NoDup (operation_vertices from to root ops) /\
    Forall (fun vertex => rank vertex <= rank node) (operation_vertices from to root ops) /\
    length ops <= rank node.
Proof.
  induction fuel as [|fuel IH]; intros root from to parent_op known rank node parents known_node bounded.
  - destruct (Nat.eq_dec node root) as [same|different].
    + subst node. exists []. simpl. rewrite Nat.eqb_refl. split; [reflexivity|]. split; [reflexivity|].
      split; [constructor; [intro absent; contradiction|constructor]|]. split; [constructor; [lia|constructor]|lia].
    + destruct (parents node known_node different) as [_ [_ smaller]]. lia.
  - destruct (Nat.eq_dec node root) as [same|different].
    + subst node. exists []. simpl. rewrite Nat.eqb_refl. split; [reflexivity|]. split; [reflexivity|].
      split; [constructor; [intro absent; contradiction|constructor]|]. split; [constructor; [lia|constructor]|lia].
    + destruct (parents node known_node different) as [target [known_parent smaller]].
      destruct (IH root from to parent_op known rank (operation_from from to (parent_op node)) parents known_parent ltac:(lia))
        as [ops [extracted [linked [simple [ranks length_bound]]]]].
      exists (ops ++ [parent_op node]). split.
      * cbn [extract_parent_operations]. apply Nat.eqb_neq in different. rewrite different, extracted. reflexivity.
      * split.
        -- eapply linked_operations_append; [exact linked|]. simpl. auto.
        -- assert (vertices : operation_vertices from to root (ops ++ [parent_op node]) =
             operation_vertices from to root ops ++ [node]).
           { unfold operation_vertices. rewrite map_app. simpl. now rewrite target. }
           rewrite vertices. split.
           ++ apply NoDup_app; [exact simple|constructor; [intro absent; contradiction|constructor]|].
              intros vertex member repeated. destruct repeated as [same|absent]; [|contradiction]. subst vertex.
              rewrite Forall_forall in ranks. specialize (ranks node member). lia.
           ++ split.
              ** apply Forall_app. split; [|constructor; [lia|constructor]].
                 rewrite Forall_forall in *. intros vertex member. specialize (ranks vertex member). lia.
              ** rewrite length_app. simpl. lia.
Qed.

Definition projected_funding count sources obligations kinds state :=
  funding_sum obligations (obligation_draw sources (projected_assignment count kinds state)).

Definition funding_path_amount count sources obligations kinds state demand operations :=
  residual_path_bottleneck (funding_sum obligations demand - projected_funding count sources obligations kinds state)
    (map (operation_capacity state) operations).

Theorem positive_simple_path_makes_strict_funding_progress : forall count sources obligations kinds state eligible capacity demand operations limit,
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  network_bounded count limit state ->
  projected_funding count sources obligations kinds state < funding_sum obligations demand ->
  NoDup (operation_vertices (fun index => pair_from sources (kinds index))
    (fun index => pair_to sources obligations (kinds index)) 0 operations) ->
  (forall op, In op operations -> fst op < count /\ 0 < operation_capacity state op) ->
  linked_operations (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
    0 operations (funding_sink sources obligations) ->
  let amount := funding_path_amount count sources obligations kinds state demand operations in
  0 < amount /\ exists next,
    network_augment limit state (rev operations) amount = Some next /\
    funding_network_valid count sources obligations kinds next eligible capacity demand /\
    network_bounded count limit next /\
    projected_funding count sources obligations kinds next = projected_funding count sources obligations kinds state + amount /\
    projected_funding count sources obligations kinds next <= funding_sum obligations demand /\
    funding_sum obligations demand - projected_funding count sources obligations kinds next <
      funding_sum obligations demand - projected_funding count sources obligations kinds state.
Proof.
  intros count sources obligations kinds state eligible capacity demand operations limit valid bounded short simple permitted linked amount.
  assert (positive : 0 < amount).
  { unfold amount, funding_path_amount. apply positive_residual_path_has_positive_bottleneck; [lia|].
    apply Forall_forall. intros value member. apply in_map_iff in member.
    destruct member as [op [same included]]. subst value. exact (proj2 (permitted op included)). }
  assert (allowed : forall op, In op operations -> fst op < count /\ amount <= operation_capacity state op).
  { intros op included. split; [exact (proj1 (permitted op included))|].
    apply (proj2 (residual_bottleneck_respects_every_bound
      (map (operation_capacity state) operations)
      (funding_sum obligations demand - projected_funding count sources obligations kinds state))).
    now apply in_map. }
  destruct (simple_funding_path_increases_funding count sources obligations kinds state eligible capacity demand operations limit amount
    valid bounded simple allowed linked) as [next [history [next_valid [increase capped]]]].
  split; [exact positive|]. exists next. split; [exact history|]. split; [exact next_valid|].
  split; [eapply network_history_preserves_bounds; eassumption|].
  change (projected_funding count sources obligations kinds next = projected_funding count sources obligations kinds state + amount) in increase.
  change (projected_funding count sources obligations kinds next <= funding_sum obligations demand) in capped.
  split; [exact increase|]. split; [exact capped|]. lia.
Qed.

Definition funding_bottleneck_step count sources obligations kinds demand limit state next :=
  exists operations,
    NoDup (operation_vertices (fun index => pair_from sources (kinds index))
      (fun index => pair_to sources obligations (kinds index)) 0 operations) /\
    (forall op, In op operations -> fst op < count /\ 0 < operation_capacity state op) /\
    linked_operations (fun index => pair_from sources (kinds index)) (fun index => pair_to sources obligations (kinds index))
      0 operations (funding_sink sources obligations) /\
    projected_funding count sources obligations kinds state < funding_sum obligations demand /\
    network_augment limit state (rev operations)
      (funding_path_amount count sources obligations kinds state demand operations) = Some next.

Inductive funding_bottleneck_history count sources obligations kinds demand limit : nat -> residual_network -> residual_network -> Prop :=
| funding_history_empty : forall state,
    funding_bottleneck_history count sources obligations kinds demand limit 0 state state
| funding_history_next : forall steps state middle next,
    funding_bottleneck_step count sources obligations kinds demand limit state middle ->
    funding_bottleneck_history count sources obligations kinds demand limit steps middle next ->
    funding_bottleneck_history count sources obligations kinds demand limit (S steps) state next.

Theorem bottleneck_histories_preserve_validity_and_bound_steps : forall steps count sources obligations kinds eligible capacity demand limit state next,
  funding_bottleneck_history count sources obligations kinds demand limit steps state next ->
  funding_network_valid count sources obligations kinds state eligible capacity demand ->
  network_bounded count limit state ->
  funding_network_valid count sources obligations kinds next eligible capacity demand /\
  network_bounded count limit next /\
  projected_funding count sources obligations kinds state + steps <= projected_funding count sources obligations kinds next /\
  steps <= funding_sum obligations demand - projected_funding count sources obligations kinds state.
Proof.
  intros steps count sources obligations kinds eligible capacity demand limit state next history.
  induction history as [state|steps state middle next step history IH]; intros valid bounded.
  - split; [exact valid|]. split; [exact bounded|]. split; lia.
  - destruct step as [operations [simple [permitted [linked [short result]]]]].
    destruct (positive_simple_path_makes_strict_funding_progress count sources obligations kinds state eligible capacity demand operations limit
      valid bounded short simple permitted linked) as [positive [computed [transition [middle_valid [middle_bounded [increase [capped decrease]]]]]]].
    rewrite result in transition. inversion transition; subst computed.
    destruct (IH middle_valid middle_bounded) as [next_valid [next_bounded [growth bound]]].
    split; [exact next_valid|]. split; [exact next_bounded|]. split; lia.
Qed.

Print Assumptions positive_simple_path_makes_strict_funding_progress.
Print Assumptions bottleneck_histories_preserve_validity_and_bound_steps.
Print Assumptions ranked_parents_extract_linked_simple_operations.

Print Assumptions linked_member_endpoints.
Print Assumptions identical_pair_shares_origin.
Print Assumptions simple_linked_path_has_distinct_pairs.
Print Assumptions simple_funding_path_increases_funding.
Print Assumptions simple_path_operation_count.
