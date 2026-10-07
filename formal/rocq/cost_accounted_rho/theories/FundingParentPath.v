From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Fixpoint extract_ranked_parent_path fuel root (parent : nat -> nat) node :=
  if Nat.eqb node root then Some [node]
  else match fuel with
  | 0 => None
  | S rest => option_map (cons node) (extract_ranked_parent_path rest root parent (parent node))
  end.

Fixpoint backward_path_links (edge : nat -> nat -> bool) path : Prop :=
  match path with
  | [] => True
  | node :: tail =>
      match tail with
      | [] => True
      | previous :: _ => edge previous node = true /\ backward_path_links edge tail
      end
  end.

Record ranked_parent_path_certificate root (known : nat -> bool) rank edge node path : Prop := {
  parent_path_starts_at_node : exists tail, path = node :: tail;
  parent_path_ends_at_root : last path root = root;
  parent_path_is_simple : NoDup path;
  parent_path_nodes_known : Forall (fun vertex => known vertex = true) path;
  parent_path_edges_valid : backward_path_links edge path;
  parent_path_rank_bound : Forall (fun vertex => rank vertex <= rank node) path;
  parent_path_length_bound : length path <= S (rank node)
}.

Lemma root_parent_path_certificate : forall root known rank edge,
  known root = true -> ranked_parent_path_certificate root known rank edge root [root].
Proof.
  intros. constructor; simpl.
  - exists []. reflexivity.
  - reflexivity.
  - constructor; [intro absent; contradiction|constructor].
  - constructor; [assumption|constructor].
  - exact I.
  - constructor; [lia|constructor].
  - lia.
Qed.

Theorem strictly_ranked_parents_extract_a_simple_path : forall fuel root known parent rank edge node,
  (forall current, known current = true -> current <> root ->
    known (parent current) = true /\ rank (parent current) < rank current /\ edge (parent current) current = true) ->
  known node = true -> rank node <= fuel ->
  exists path, extract_ranked_parent_path fuel root parent node = Some path /\
    ranked_parent_path_certificate root known rank edge node path.
Proof.
  induction fuel as [|fuel IH]; intros root known parent rank edge node parents known_node bounded.
  - destruct (Nat.eq_dec node root) as [same|different].
    + subst node. exists [root]. split.
      * simpl. now rewrite Nat.eqb_refl.
      * now apply root_parent_path_certificate.
    + destruct (parents node known_node different) as [_ [ranked _]]. lia.
  - destruct (Nat.eq_dec node root) as [same|different].
    + subst node. exists [root]. split.
      * simpl. now rewrite Nat.eqb_refl.
      * now apply root_parent_path_certificate.
    + destruct (parents node known_node different) as [known_parent [ranked linked]].
      destruct (IH root known parent rank edge (parent node) parents known_parent ltac:(lia))
        as [path [extracted certificate]].
      destruct certificate as [[tail path_at] ending simple all_known links ranks length_bound].
      subst path. exists (node :: parent node :: tail). split.
      * simpl. assert (Nat.eqb node root = false) by (apply Nat.eqb_neq; exact different).
        rewrite H, extracted. reflexivity.
      * constructor.
        -- exists (parent node :: tail). reflexivity.
        -- change (last (parent node :: tail) root = root). exact ending.
        -- constructor; [|exact simple]. intros repeated.
           rewrite Forall_forall in ranks. specialize (ranks node repeated). lia.
        -- constructor; assumption.
        -- change (edge (parent node) node = true /\ backward_path_links edge (parent node :: tail)). auto.
        -- constructor; [lia|]. rewrite Forall_forall in *. intros vertex included.
           specialize (ranks vertex included). lia.
        -- simpl in *. lia.
Qed.

Theorem parent_path_has_no_cycle : forall root known rank edge node path,
  ranked_parent_path_certificate root known rank edge node path -> NoDup path.
Proof. intros. exact (parent_path_is_simple _ _ _ _ _ _ H). Qed.

Definition residual_path_bottleneck remaining capacities := fold_right Nat.min remaining capacities.

Theorem residual_bottleneck_respects_every_bound : forall capacities remaining,
  residual_path_bottleneck remaining capacities <= remaining /\
  forall capacity, In capacity capacities -> residual_path_bottleneck remaining capacities <= capacity.
Proof.
  induction capacities as [|head tail IH]; intros remaining; simpl.
  - split; [lia|intros capacity absent; contradiction].
  - destruct (IH remaining) as [remaining_bound all_bounds]. split.
    + eapply Nat.le_trans; [apply Nat.le_min_r|exact remaining_bound].
    + intros capacity [same|inside].
      * subst capacity. apply Nat.le_min_l.
      * eapply Nat.le_trans; [apply Nat.le_min_r|now apply all_bounds].
Qed.

Theorem residual_bottleneck_is_greatest_feasible_amount : forall capacities remaining amount,
  amount <= remaining -> (forall capacity, In capacity capacities -> amount <= capacity) ->
  amount <= residual_path_bottleneck remaining capacities.
Proof.
  induction capacities as [|head tail IH]; intros remaining amount bounded all_bounds; simpl; [exact bounded|].
  apply Nat.min_glb.
  - apply all_bounds. now left.
  - apply IH; [exact bounded|]. intros capacity inside. apply all_bounds. now right.
Qed.

Theorem positive_residual_path_has_positive_bottleneck : forall capacities remaining,
  0 < remaining -> Forall (fun capacity => 0 < capacity) capacities ->
  0 < residual_path_bottleneck remaining capacities.
Proof.
  intros capacities remaining positive all_positive.
  assert (1 <= residual_path_bottleneck remaining capacities).
  { apply residual_bottleneck_is_greatest_feasible_amount; [lia|].
    rewrite Forall_forall in all_positive. intros. specialize (all_positive capacity H). lia. }
  lia.
Qed.

Theorem residual_bottleneck_completes_or_saturates : forall capacities remaining,
  residual_path_bottleneck remaining capacities = remaining \/
  In (residual_path_bottleneck remaining capacities) capacities.
Proof.
  induction capacities as [|head tail IH]; intros remaining; simpl; [auto|].
  destruct (Nat.le_ge_cases head (residual_path_bottleneck remaining tail)) as [small|large].
  - rewrite Nat.min_l by exact small. right. now left.
  - rewrite Nat.min_r by exact large. destruct (IH remaining) as [done|saturated].
    + now left.
    + right. now right.
Qed.

Print Assumptions strictly_ranked_parents_extract_a_simple_path.
Print Assumptions parent_path_has_no_cycle.
Print Assumptions residual_bottleneck_respects_every_bound.
Print Assumptions residual_bottleneck_is_greatest_feasible_amount.
Print Assumptions positive_residual_path_has_positive_bottleneck.
Print Assumptions residual_bottleneck_completes_or_saturates.
