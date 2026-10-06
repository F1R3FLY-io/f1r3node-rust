From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Record adjacency_model := {
  adjacency_count : nat;
  adjacency_owner : nat -> nat;
  adjacency_next : nat -> option nat;
  adjacency_head : nat -> option nat
}.

Fixpoint adjacency_chain (next : nat -> option nat) (head : option nat) (indices : list nat) : Prop :=
  match indices with
  | [] => head = None
  | index :: rest => head = Some index /\ adjacency_chain next (next index) rest
  end.

Definition adjacency_coverage state : Prop :=
  forall vertex, exists indices,
    adjacency_chain (adjacency_next state) (adjacency_head state vertex) indices /\
    NoDup indices /\
    forall index, In index indices <->
      index < adjacency_count state /\ adjacency_owner state index = vertex.

Definition adjacency_empty := {|
  adjacency_count := 0;
  adjacency_owner := fun _ => 0;
  adjacency_next := fun _ => None;
  adjacency_head := fun _ => None
|}.

Definition adjacency_append state source := {|
  adjacency_count := S (adjacency_count state);
  adjacency_owner := fun index =>
    if Nat.eqb index (adjacency_count state) then source else adjacency_owner state index;
  adjacency_next := fun index =>
    if Nat.eqb index (adjacency_count state) then adjacency_head state source else adjacency_next state index;
  adjacency_head := fun vertex =>
    if Nat.eqb vertex source then Some (adjacency_count state) else adjacency_head state vertex
|}.

Definition adjacency_append_pair state source target :=
  adjacency_append (adjacency_append state source) target.

Fixpoint adjacency_build state owners :=
  match owners with
  | [] => state
  | source :: rest => adjacency_build (adjacency_append state source) rest
  end.

Lemma adjacency_chain_pointwise : forall indices next other head,
  adjacency_chain next head indices ->
  (forall index, In index indices -> next index = other index) ->
  adjacency_chain other head indices.
Proof.
  induction indices as [|index rest IH]; intros next other head chain equal; simpl in *; [exact chain|].
  destruct chain as [starts tail]. split; [exact starts|].
  rewrite <- (equal index ltac:(now left)).
  eapply IH; [exact tail|]. intros current inside. apply equal. now right.
Qed.

Theorem empty_adjacency_has_exact_coverage : adjacency_coverage adjacency_empty.
Proof.
  intros vertex. exists []. simpl. split; [reflexivity|]. split; [constructor|]. intros index. intuition lia.
Qed.

Theorem append_preserves_exact_adjacency_coverage : forall state source,
  adjacency_coverage state -> adjacency_coverage (adjacency_append state source).
Proof.
  intros state source valid vertex.
  destruct (valid vertex) as [indices [chain [distinct coverage]]].
  assert (fresh : ~ In (adjacency_count state) indices).
  { intro included. apply coverage in included. lia. }
  assert (retained : adjacency_chain (adjacency_next (adjacency_append state source))
    (adjacency_head state vertex) indices).
  { eapply adjacency_chain_pointwise; [exact chain|]. intros index included.
    apply coverage in included. cbn [adjacency_append adjacency_next].
    assert (different : Nat.eqb index (adjacency_count state) = false) by (apply Nat.eqb_neq; lia).
    now rewrite different. }
  destruct (Nat.eq_dec vertex source) as [same|different].
  - subst vertex. exists (adjacency_count state :: indices). split.
    + cbn [adjacency_append adjacency_head adjacency_chain]. rewrite Nat.eqb_refl.
      split; [reflexivity|]. cbn [adjacency_append adjacency_next]. now rewrite Nat.eqb_refl.
    + split; [constructor; assumption|]. intros index.
      cbn [adjacency_append adjacency_count adjacency_owner].
      destruct (Nat.eqb_spec index (adjacency_count state)) as [same|different].
      * subst index. simpl. intuition lia.
      * simpl. rewrite coverage. intuition lia.
  - exists indices. split.
    + cbn [adjacency_append adjacency_head]. apply Nat.eqb_neq in different. now rewrite different.
    + split; [exact distinct|]. intros index.
      cbn [adjacency_append adjacency_count adjacency_owner]. rewrite coverage.
      destruct (Nat.eqb_spec index (adjacency_count state)) as [same|unequal].
      * subst index. intuition lia.
      * intuition lia.
Qed.

Theorem paired_append_preserves_exact_adjacency_coverage : forall state source target,
  adjacency_coverage state -> adjacency_coverage (adjacency_append_pair state source target).
Proof.
  intros. apply append_preserves_exact_adjacency_coverage. now apply append_preserves_exact_adjacency_coverage.
Qed.

Theorem arbitrary_construction_preserves_exact_adjacency_coverage : forall owners state,
  adjacency_coverage state -> adjacency_coverage (adjacency_build state owners).
Proof.
  induction owners as [|source rest IH]; intros state valid; simpl; [exact valid|].
  apply IH. now apply append_preserves_exact_adjacency_coverage.
Qed.

Theorem constructed_adjacency_reaches_exactly_owned_edges : forall owners vertex,
  let state := adjacency_build adjacency_empty owners in
  exists indices,
    adjacency_chain (adjacency_next state) (adjacency_head state vertex) indices /\
    NoDup indices /\
    forall index, In index indices <->
      index < adjacency_count state /\ adjacency_owner state index = vertex.
Proof.
  intros owners vertex state. apply arbitrary_construction_preserves_exact_adjacency_coverage.
  apply empty_adjacency_has_exact_coverage.
Qed.

Print Assumptions empty_adjacency_has_exact_coverage.
Print Assumptions append_preserves_exact_adjacency_coverage.
Print Assumptions paired_append_preserves_exact_adjacency_coverage.
Print Assumptions arbitrary_construction_preserves_exact_adjacency_coverage.
Print Assumptions constructed_adjacency_reaches_exactly_owned_edges.
