From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData RPB3DescriptorLaws.

Import ListNotations.
Import GeneratedData.

Lemma find_unique_tag_binding_complete :
  forall bindings binding,
    NoDup (map binding_tag bindings) ->
    In binding bindings ->
    find (fun candidate => Nat.eqb (binding_tag binding)
      (binding_tag candidate)) bindings = Some binding.
Proof.
  induction bindings as [|head tail IH]; intros binding unique present;
    simpl in *; [contradiction|].
  inversion unique as [|? ? absent unique_tail]; subst.
  destruct present as [same|in_tail].
  - subst binding. now rewrite Nat.eqb_refl.
  - assert (Nat.eqb (binding_tag binding) (binding_tag head) = false)
      as distinct.
    { apply Nat.eqb_neq. intro equal.
      apply absent. rewrite <- equal.
      apply in_map. exact in_tail. }
    rewrite distinct.
    apply IH; assumption.
Qed.

Theorem generated_projection_resolves_every_actual_tag :
  forall program binding,
    In program generated_programs ->
    In binding (program_bindings program) ->
    resolve_actual_tag program (binding_tag binding) = Some binding.
Proof.
  intros program binding generated present.
  unfold resolve_actual_tag.
  apply find_unique_tag_binding_complete.
  - now apply generated_program_tags_unique.
  - exact present.
Qed.

Theorem generated_projection_resolution_is_exact :
  forall program tag binding,
    In program generated_programs ->
    (resolve_actual_tag program tag = Some binding <->
      In binding (program_bindings program) /\ binding_tag binding = tag).
Proof.
  intros program tag binding generated.
  split.
  - apply resolve_actual_tag_sound.
  - intros [present same].
    rewrite <- same.
    now apply generated_projection_resolves_every_actual_tag.
Qed.

Theorem generated_projection_rejects_exactly_unbound_tags :
  forall program tag,
    In program generated_programs ->
    (resolve_actual_tag program tag = None <->
      forall binding, In binding (program_bindings program) ->
        binding_tag binding <> tag).
Proof.
  intros program tag generated.
  split.
  - intros absent binding present same.
    rewrite <- same in absent.
    rewrite (generated_projection_resolves_every_actual_tag
      program binding generated present) in absent.
    discriminate.
  - intros unbound.
    destruct (resolve_actual_tag program tag) as [binding|]
      eqn:resolved; [|reflexivity].
    destruct (resolve_actual_tag_sound program tag binding resolved)
      as [present same].
    exfalso. exact (unbound binding present same).
Qed.

Example generated_par_connective_resolution_is_exact :
  resolve_actual_tag raw_par_program 10 = Some (TagBinding 10 8 None).
Proof. vm_compute. reflexivity. Qed.

Example generated_par_unbound_tag_is_refused :
  resolve_actual_tag raw_par_program 3 = None.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions generated_projection_resolves_every_actual_tag.
Print Assumptions generated_projection_resolution_is_exact.
Print Assumptions generated_projection_rejects_exactly_unbound_tags.
Print Assumptions generated_par_connective_resolution_is_exact.
Print Assumptions generated_par_unbound_tag_is_refused.
