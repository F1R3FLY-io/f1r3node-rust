From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool.
Require Import Epm1CursorAuthorityV2 Epm1CursorPositiveV3
  Epm1CursorPositiveV4 Epm1ActConcreteV5 Epm1PositionalAssociationV6.

Import ListNotations.
Import PositiveV3 PositiveV4 ActConcreteV5 PositionalAssociationV6.

(** Causal negative controls for positional EPM1 association.  Every mutant
    changes one named decision and is killed by a concrete two-occurrence
    witness.  These are not alternate specifications. *)
Module PositionalAssociationV6Mutants.

Definition two_occurrence_source : canonical_tree :=
  CNode (Some 17) (FCons 1 (CNode (Some 29) FNil) FNil).

Definition two_occurrence_paths : list path := [[]; [1]].
Definition two_occurrence_bindings : list (path * nat) :=
  [([], 0); ([1], 0)].

Theorem two_occurrence_fixture_is_the_accepted_structural_view :
  source_body_cursor_paths two_occurrence_source = two_occurrence_paths /\
  writer_value_bindings (canonical_zero_writer_tree two_occurrence_source) =
    two_occurrence_bindings.
Proof. vm_compute. auto. Qed.

(** Mutant: physical value addresses are used as occurrence identities.  CoW
    sharing then collapses two logical endpoints into one key. *)
Definition mutant_address_keyed_occurrences (address : path -> nat)
    (endpoints : list path) : list nat :=
  nodup Nat.eq_dec (map address endpoints).

Definition shared_address (_ : path) : nat := 41.

Theorem kill_pointer_or_address_collapse :
  length (mutant_address_keyed_occurrences shared_address
    two_occurrence_paths) <> length two_occurrence_paths.
Proof. vm_compute. discriminate. Qed.

(** Mutant: ACT values regain the removed sequential-ordinal meaning. *)
Definition mutant_sequential_act_bindings : list (path * nat) :=
  combine two_occurrence_paths (seq 0 (length two_occurrence_paths)).

Theorem kill_sequential_act_ordinal :
  mutant_sequential_act_bindings <> two_occurrence_bindings.
Proof. vm_compute. discriminate. Qed.

(** Mutant: a logical endpoint carries a nonzero occurrence sentinel. *)
Definition mutant_nonzero_sentinel_bindings : list (path * nat) :=
  [([], 0); ([1], 9)].

Theorem kill_nonzero_sentinel :
  mutant_nonzero_sentinel_bindings <> two_occurrence_bindings.
Proof. discriminate. Qed.

(** Mutant: the child endpoint is skipped while the root is retained. *)
Definition mutant_skipped_child_endpoint : list (path * nat) :=
  firstn 1 two_occurrence_bindings.

Theorem kill_skipped_endpoint :
  mutant_skipped_child_endpoint <> two_occurrence_bindings.
Proof. vm_compute. discriminate. Qed.

(** Mutant: an endpoint is emitted twice. *)
Definition mutant_duplicated_endpoint : list (path * nat) :=
  ([], 0) :: two_occurrence_bindings.

Theorem kill_duplicated_endpoint :
  mutant_duplicated_endpoint <> two_occurrence_bindings.
Proof. discriminate. Qed.

(** Mutant: the second body is associated with the wrong structural path. *)
Definition mutant_wrong_endpoint : list (path * nat) :=
  [([], 0); ([2], 0)].

Theorem kill_wrong_endpoint :
  mutant_wrong_endpoint <> two_occurrence_bindings.
Proof. discriminate. Qed.

(** Mutant: root-value handling incorrectly assumes every endpoint has a
    nonempty path. *)
Definition mutant_root_omission : list (path * nat) :=
  tl two_occurrence_bindings.

Theorem kill_root_omission :
  mutant_root_omission <> two_occurrence_bindings.
Proof. vm_compute. discriminate. Qed.

Inductive publication_phase : Type :=
| BeforeAllBodies
| AfterAllBodies.

Record publication_observation : Type := {
  observed_publications : nat;
  observed_publication_phase : publication_phase
}.

Definition accepted_publication : publication_observation :=
  {| observed_publications := 1;
     observed_publication_phase := AfterAllBodies |}.

Definition mutant_early_publication : publication_observation :=
  {| observed_publications := 1;
     observed_publication_phase := BeforeAllBodies |}.

Definition mutant_double_publication : publication_observation :=
  {| observed_publications := 2;
     observed_publication_phase := AfterAllBodies |}.

Theorem kill_early_publication :
  mutant_early_publication <> accepted_publication.
Proof. discriminate. Qed.

Theorem kill_double_publication :
  mutant_double_publication <> accepted_publication.
Proof. discriminate. Qed.

(** Mutant: ACT failure is returned before the already-framed child PDA has
    had the opportunity to report its earlier failure. *)
Definition malformed_child_packet : body_packet :=
  {| packet_wire_bytes := 1;
     packet_decoded := None;
     packet_canonical := true |}.

Theorem kill_precedence_inversion :
  retained_count_machine true true 1 0 false [malformed_child_packet] =
    Some (ChildPdaFailure 0) /\
  naive_act_first_count_free_machine false 0 [malformed_child_packet] =
    Some DeferredActFailure.
Proof. vm_compute. auto. Qed.

Definition mutant_missing_body : list body := [101].
Definition mutant_extra_body : list body := [101; 202; 303].

Theorem kill_missing_body :
  retained_count_machine true true (length mutant_missing_body)
    (length two_occurrence_paths) true
    (canonical_packets_of_bodies mutant_missing_body) =
    Some ModeOrOrdinalFailure.
Proof. vm_compute. reflexivity. Qed.

Theorem kill_extra_body :
  retained_count_machine true true (length mutant_extra_body)
    (length two_occurrence_paths) true
    (canonical_packets_of_bodies mutant_extra_body) =
    Some ModeOrOrdinalFailure.
Proof. vm_compute. reflexivity. Qed.

Inductive v6_mutant : Type :=
| PointerAddressCollapse
| SequentialActOrdinal
| NonzeroSentinel
| SkippedEndpoint
| DuplicatedEndpoint
| WrongEndpoint
| RootOmission
| EarlyPublication
| DoublePublication
| PrecedenceInversion
| MissingBody
| ExtraBody.

Definition all_v6_mutants : list v6_mutant :=
  [PointerAddressCollapse; SequentialActOrdinal; NonzeroSentinel;
   SkippedEndpoint; DuplicatedEndpoint; WrongEndpoint; RootOmission;
   EarlyPublication; DoublePublication; PrecedenceInversion;
   MissingBody; ExtraBody].

Definition v6_mutant_killed (mutant : v6_mutant) : Prop :=
  match mutant with
  | PointerAddressCollapse =>
      length (mutant_address_keyed_occurrences shared_address
        two_occurrence_paths) <> length two_occurrence_paths
  | SequentialActOrdinal =>
      mutant_sequential_act_bindings <> two_occurrence_bindings
  | NonzeroSentinel =>
      mutant_nonzero_sentinel_bindings <> two_occurrence_bindings
  | SkippedEndpoint =>
      mutant_skipped_child_endpoint <> two_occurrence_bindings
  | DuplicatedEndpoint =>
      mutant_duplicated_endpoint <> two_occurrence_bindings
  | WrongEndpoint => mutant_wrong_endpoint <> two_occurrence_bindings
  | RootOmission => mutant_root_omission <> two_occurrence_bindings
  | EarlyPublication => mutant_early_publication <> accepted_publication
  | DoublePublication => mutant_double_publication <> accepted_publication
  | PrecedenceInversion =>
      retained_count_machine true true 1 0 false [malformed_child_packet] =
        Some (ChildPdaFailure 0) /\
      naive_act_first_count_free_machine false 0 [malformed_child_packet] =
        Some DeferredActFailure
  | MissingBody =>
      retained_count_machine true true (length mutant_missing_body)
        (length two_occurrence_paths) true
        (canonical_packets_of_bodies mutant_missing_body) =
        Some ModeOrOrdinalFailure
  | ExtraBody =>
      retained_count_machine true true (length mutant_extra_body)
        (length two_occurrence_paths) true
        (canonical_packets_of_bodies mutant_extra_body) =
        Some ModeOrOrdinalFailure
  end.

Theorem every_v6_mutant_has_a_causal_counterexample : forall mutant,
  v6_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant; cbn.
  - apply kill_pointer_or_address_collapse.
  - apply kill_sequential_act_ordinal.
  - apply kill_nonzero_sentinel.
  - apply kill_skipped_endpoint.
  - apply kill_duplicated_endpoint.
  - apply kill_wrong_endpoint.
  - apply kill_root_omission.
  - apply kill_early_publication.
  - apply kill_double_publication.
  - apply kill_precedence_inversion.
  - apply kill_missing_body.
  - apply kill_extra_body.
Qed.

Theorem v6_mutant_registry_is_unique_and_complete :
  NoDup all_v6_mutants /\
  Forall v6_mutant_killed all_v6_mutants.
Proof.
  split.
  - repeat constructor; simpl; intuition congruence.
  - repeat constructor; apply every_v6_mutant_has_a_causal_counterexample.
Qed.

Print Assumptions kill_pointer_or_address_collapse.
Print Assumptions kill_sequential_act_ordinal.
Print Assumptions kill_nonzero_sentinel.
Print Assumptions kill_skipped_endpoint.
Print Assumptions kill_duplicated_endpoint.
Print Assumptions kill_wrong_endpoint.
Print Assumptions kill_root_omission.
Print Assumptions kill_early_publication.
Print Assumptions kill_double_publication.
Print Assumptions kill_precedence_inversion.
Print Assumptions kill_missing_body.
Print Assumptions kill_extra_body.
Print Assumptions every_v6_mutant_has_a_causal_counterexample.
Print Assumptions v6_mutant_registry_is_unique_and_complete.

End PositionalAssociationV6Mutants.
