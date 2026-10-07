From Stdlib Require Import Bool.Bool Lists.List.

Import ListNotations.

Section StructuralAuthorityBound.

Context {region event introduction : Type}.

Variable charged : region -> event -> bool.
Variable scoped : region -> introduction -> bool.
Variable witness : region -> event -> introduction.

Definition charged_events (selected : region) (events : list event) : list event :=
  filter (charged selected) events.

Definition scoped_introductions
  (selected : region)
  (introductions : list introduction)
  : list introduction :=
  filter (scoped selected) introductions.

Definition witness_valid
  (events : list event)
  (introductions : list introduction)
  : Prop :=
  forall selected current,
    In current events ->
    charged selected current = true ->
    In (witness selected current) introductions /\
    scoped selected (witness selected current) = true.

Definition witness_injective (events : list event) : Prop :=
  forall selected left right,
    In left events ->
    In right events ->
    charged selected left = true ->
    charged selected right = true ->
    witness selected left = witness selected right ->
    left = right.

Lemma map_nodup_by_injective :
  forall (A B : Type) (f : A -> B) (items : list A),
    NoDup items ->
    (forall left right,
      In left items ->
      In right items ->
      f left = f right ->
      left = right) ->
    NoDup (map f items).
Proof.
  intros A B f items Hnodup.
  induction Hnodup as [|head tail Habsent Htail IH]; intros Hinjective; simpl.
  - constructor.
  - constructor.
    + intro Hin.
      apply in_map_iff in Hin.
      destruct Hin as [current [Heq Hin]].
      assert (head = current) as ->.
      { apply Hinjective; simpl; auto. }
      contradiction.
    + apply IH.
      intros left right Hleft Hright Heq.
      apply Hinjective; simpl; auto.
Qed.

Theorem realized_authority_never_exceeds_structural_demand :
  forall selected events introductions,
    NoDup events ->
    witness_valid events introductions ->
    witness_injective events ->
    length (charged_events selected events) <=
    length (scoped_introductions selected introductions).
Proof.
  intros selected events introductions Hnodup Hvalid Hinjective.
  rewrite <- (length_map (witness selected) (charged_events selected events)).
  apply NoDup_incl_length with
    (l := map (witness selected) (charged_events selected events)).
  - apply map_nodup_by_injective.
    + apply NoDup_filter. exact Hnodup.
    + intros left right Hleft Hright Heq.
      apply filter_In in Hleft.
      apply filter_In in Hright.
      destruct Hleft as [Hleft Hleft_charged].
      destruct Hright as [Hright Hright_charged].
      eapply Hinjective; eauto.
  - intros current Hin.
    apply in_map_iff in Hin.
    destruct Hin as [current_event [<- Hin]].
    apply filter_In in Hin.
    apply filter_In.
    split.
    + apply (Hvalid selected current_event); tauto.
    + apply (Hvalid selected current_event); tauto.
Qed.

End StructuralAuthorityBound.

Print Assumptions realized_authority_never_exceeds_structural_demand.
