From Stdlib Require Import List Arith Bool Lia.

Import ListNotations.
Set Implicit Arguments.

(**
  Semantic bridge for the RMP2 opaque [ParBag] token and the canonical datum
  order used around the retained PathMap index.

  This theory begins after canonical-protobuf admission.  It deliberately does
  not model BLAKE2b-256, generated Rust, allocation failure, or arbitrary-byte
  parsing.  The executable differential markers named by
  [models/tests/formal_equivalence_manifest.rs] discharge those boundaries.
  A digest collision can therefore add a conservative candidate, but the
  authoritative spatial matcher remains the acceptance authority.
*)

Definition byte := nat.
Definition bytes := list byte.

Definition rmp2_par_bag_marker : byte := 41.

Definition be64 (value : nat) : bytes :=
  [ (value / 256 ^ 7) mod 256;
    (value / 256 ^ 6) mod 256;
    (value / 256 ^ 5) mod 256;
    (value / 256 ^ 4) mod 256;
    (value / 256 ^ 3) mod 256;
    (value / 256 ^ 2) mod 256;
    (value / 256) mod 256;
    value mod 256 ].

Record opaque_par_summary : Type := OpaqueParSummary {
  opaque_distinct : nat;
  opaque_cardinality : nat;
  opaque_digest : bytes
}.

Definition valid_opaque_par_summary (summary : opaque_par_summary) : Prop :=
  length (opaque_digest summary) = 32 /\
  opaque_distinct summary < 2 ^ 64 /\
  opaque_cardinality summary < 2 ^ 64.

Definition emit_opaque_par_token (summary : opaque_par_summary) : bytes :=
  rmp2_par_bag_marker ::
    be64 (opaque_distinct summary) ++
    be64 (opaque_cardinality summary) ++
    opaque_digest summary.

Lemma be64_length : forall value, length (be64 value) = 8.
Proof. reflexivity. Qed.

Theorem emitted_opaque_par_token_has_exactly_49_bytes :
  forall summary,
    valid_opaque_par_summary summary ->
    length (emit_opaque_par_token summary) = 49.
Proof.
  intros summary valid.
  unfold emit_opaque_par_token, valid_opaque_par_summary in *.
  destruct valid as [digest_length [_ _]].
  unfold be64. simpl. now rewrite digest_length.
Qed.

(** Only the emission boundary observes a summary.  Classification of the
    admitted Par shell is intentionally summary-free so a dynamic nested
    suffix can become a whole-token wildcard without eagerly hashing every
    suffix. *)
Inductive admitted_par_shell : Type :=
| ExactOpaqueShell : admitted_par_shell
| DynamicInteriorShell : admitted_par_shell
| UnsupportedShell : admitted_par_shell.

Inductive opaque_par_query : Type :=
| ExactOpaqueToken : bytes -> opaque_par_query
| WholeOpaqueTokenWildcard : opaque_par_query.

Definition compile_opaque_par_shell
    (shell : admitted_par_shell)
    (summary_at_emission : opaque_par_summary) : option opaque_par_query :=
  match shell with
  | ExactOpaqueShell =>
      Some (ExactOpaqueToken (emit_opaque_par_token summary_at_emission))
  | DynamicInteriorShell => Some WholeOpaqueTokenWildcard
  | UnsupportedShell => None
  end.

Definition opaque_par_query_accepts
    (query : opaque_par_query) (actual : opaque_par_summary) : Prop :=
  match query with
  | ExactOpaqueToken expected => expected = emit_opaque_par_token actual
  | WholeOpaqueTokenWildcard => True
  end.

Inductive spatial_opaque_par_accepts :
    admitted_par_shell -> opaque_par_summary -> opaque_par_summary -> Prop :=
| SpatialOpaqueExact :
    forall expected actual,
      emit_opaque_par_token expected = emit_opaque_par_token actual ->
      spatial_opaque_par_accepts ExactOpaqueShell expected actual
| SpatialOpaqueDynamic :
    forall ignored actual,
      spatial_opaque_par_accepts DynamicInteriorShell ignored actual.

Theorem exact_opaque_query_compares_the_complete_emitted_token :
  forall expected actual,
    opaque_par_query_accepts
      (ExactOpaqueToken (emit_opaque_par_token expected)) actual <->
    emit_opaque_par_token expected = emit_opaque_par_token actual.
Proof. intros expected actual. reflexivity. Qed.

Theorem whole_token_wildcard_accepts_every_dynamic_nested_interior :
  forall summary, opaque_par_query_accepts WholeOpaqueTokenWildcard summary.
Proof. intros summary. exact I. Qed.

Theorem dynamic_shell_classification_is_independent_of_summary :
  forall left right,
    compile_opaque_par_shell DynamicInteriorShell left =
    compile_opaque_par_shell DynamicInteriorShell right.
Proof. reflexivity. Qed.

Theorem summary_congruence_applies_at_emission :
  forall left right,
    left = right -> emit_opaque_par_token left = emit_opaque_par_token right.
Proof. intros left right same. now subst. Qed.

Theorem compiled_opaque_query_has_no_false_negative :
  forall shell expected actual query,
    compile_opaque_par_shell shell expected = Some query ->
    spatial_opaque_par_accepts shell expected actual ->
    opaque_par_query_accepts query actual.
Proof.
  intros shell expected actual query compiled accepted.
  inversion accepted; subst; simpl in compiled.
  - injection compiled as query_equal. subst query. simpl. assumption.
  - injection compiled as query_equal. subst query. exact I.
Qed.

Theorem unsupported_opaque_shell_fails_closed :
  forall summary query,
    compile_opaque_par_shell UnsupportedShell summary <> Some query.
Proof. intros summary query compiled. discriminate. Qed.

(** The two executable RMP2 walkers have different source representations:
    the resident walker reads generated Rust values, while the canonical-region
    walker reads provenance-admitted protobuf fields.  They meet at this
    semantic field boundary.  Child-message projections are represented by
    their already-projected event bytes; this theory does not claim to verify
    generated protobuf parsing. *)
Inductive semantic_field_value : Type :=
| SemanticScalar : bytes -> semantic_field_value
| SemanticAbsent : semantic_field_value
| SemanticSequence : list bytes -> semantic_field_value
| SemanticMap : list (bytes * bytes) -> semantic_field_value
| SemanticOneof : nat -> bytes -> semantic_field_value
| SemanticOpaquePar : opaque_par_summary -> semantic_field_value
| SemanticPathMap : nat -> nat -> bytes -> semantic_field_value.

Record resident_field_observation : Type := ResidentFieldObservation {
  resident_field_tag : nat;
  resident_field_value : semantic_field_value
}.

Record region_field_observation : Type := RegionFieldObservation {
  region_field_tag : nat;
  region_field_value : semantic_field_value
}.

Inductive semantic_projection_event : Type :=
| ProjectFieldStart : nat -> semantic_projection_event
| ProjectFieldValue : semantic_field_value -> semantic_projection_event
| ProjectFieldEnd : semantic_projection_event.

Definition project_resident_field
    (field : resident_field_observation) : list semantic_projection_event :=
  [ ProjectFieldStart (resident_field_tag field);
    ProjectFieldValue (resident_field_value field);
    ProjectFieldEnd ].

Definition project_region_field
    (field : region_field_observation) : list semantic_projection_event :=
  [ ProjectFieldStart (region_field_tag field);
    ProjectFieldValue (region_field_value field);
    ProjectFieldEnd ].

Definition semantic_field_observations_correspond
    (resident : resident_field_observation)
    (region : region_field_observation) : Prop :=
  resident_field_tag resident = region_field_tag region /\
  resident_field_value resident = region_field_value region.

Theorem corresponding_semantic_fields_have_identical_projection :
  forall resident region,
    semantic_field_observations_correspond resident region ->
    project_resident_field resident = project_region_field region.
Proof.
  intros [resident_tag resident_value] [region_tag region_value] [tags values].
  simpl in *. now subst.
Qed.

(** Canonical protobuf emits equal occurrences adjacently.  This structurally
    recursive right fold is the mathematical image of the Rust pending-run
    state: only the immediately adjacent record may absorb an occurrence. *)
Record semantic_occurrence : Type := SemanticOccurrence {
  occurrence_family : nat;
  occurrence_body : bytes
}.

Record coalesced_record : Type := CoalescedRecord {
  record_family : nat;
  record_multiplicity : nat;
  record_body : bytes
}.

Definition occurrence_matches_record
    (occurrence : semantic_occurrence) (record : coalesced_record) : bool :=
  if Nat.eq_dec (occurrence_family occurrence) (record_family record) then
    if list_eq_dec Nat.eq_dec
         (occurrence_body occurrence) (record_body record) then true else false
  else false.

Definition prepend_coalesced_occurrence
    (occurrence : semantic_occurrence)
    (records : list coalesced_record) : list coalesced_record :=
  match records with
  | [] => [CoalescedRecord
             (occurrence_family occurrence) 1 (occurrence_body occurrence)]
  | record :: tail =>
      if occurrence_matches_record occurrence record then
        CoalescedRecord
          (record_family record)
          (S (record_multiplicity record))
          (record_body record) :: tail
      else
        CoalescedRecord
          (occurrence_family occurrence) 1 (occurrence_body occurrence) :: records
  end.

Fixpoint coalesce_adjacent_occurrences
    (occurrences : list semantic_occurrence) : list coalesced_record :=
  match occurrences with
  | [] => []
  | occurrence :: tail =>
      prepend_coalesced_occurrence
        occurrence (coalesce_adjacent_occurrences tail)
  end.

Fixpoint coalesced_cardinality (records : list coalesced_record) : nat :=
  match records with
  | [] => 0
  | record :: tail =>
      record_multiplicity record + coalesced_cardinality tail
  end.

Lemma prepend_coalesced_occurrence_adds_one :
  forall occurrence records,
    coalesced_cardinality (prepend_coalesced_occurrence occurrence records) =
    S (coalesced_cardinality records).
Proof.
  intros occurrence records. destruct records as [|record tail]; simpl.
  - reflexivity.
  - destruct (occurrence_matches_record occurrence record); simpl; lia.
Qed.

Theorem adjacent_coalescing_preserves_exact_cardinality :
  forall occurrences,
    coalesced_cardinality (coalesce_adjacent_occurrences occurrences) =
    length occurrences.
Proof.
  induction occurrences as [|occurrence tail IH]; simpl.
  - reflexivity.
  - rewrite prepend_coalesced_occurrence_adds_one. now rewrite IH.
Qed.

Example separated_equal_occurrences_remain_distinct_runs :
  coalesce_adjacent_occurrences
    [ SemanticOccurrence 5 [1];
      SemanticOccurrence 5 [2];
      SemanticOccurrence 5 [1] ] =
    [ CoalescedRecord 5 1 [1];
      CoalescedRecord 5 1 [2];
      CoalescedRecord 5 1 [1] ].
Proof. reflexivity. Qed.

(** A nested Par exposes exactly its opaque ParBag summary and its two root
    metadata fields.  [nested_recursive_component_tree] is retained here only
    to state and prove that changing recursive component structure cannot make
    it into the RMP2 projection. *)
Record nested_par_source : Type := NestedParSource {
  nested_recursive_component_tree : bytes;
  nested_opaque_summary : opaque_par_summary;
  nested_locally_free : bytes;
  nested_connective_used : bool
}.

Record nested_par_projection : Type := NestedParProjection {
  projected_opaque_summary : opaque_par_summary;
  projected_locally_free : bytes;
  projected_connective_used : bool
}.

Definition project_nested_par (source : nested_par_source) : nested_par_projection :=
  NestedParProjection
    (nested_opaque_summary source)
    (nested_locally_free source)
    (nested_connective_used source).

Theorem nested_par_projection_is_opaque_to_recursive_component_structure :
  forall left_tree right_tree summary locally_free connective_used,
    project_nested_par
      (NestedParSource left_tree summary locally_free connective_used) =
    project_nested_par
      (NestedParSource right_tree summary locally_free connective_used).
Proof. reflexivity. Qed.

(** Canonical datum ordering uses only the canonical identity hash and the full
    canonical identity bytes.  The derived RMP2 bytes are retained-index data,
    not a tie breaker. *)
Record indexed_candidate : Type := IndexedCandidate {
  candidate_identity_hash : nat;
  candidate_identity : bytes;
  candidate_rmp : bytes
}.

Fixpoint compare_bytes (left right : bytes) : comparison :=
  match left, right with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | left_head :: left_tail, right_head :: right_tail =>
      match Nat.compare left_head right_head with
      | Eq => compare_bytes left_tail right_tail
      | result => result
      end
  end.

Definition compare_candidates
    (left right : indexed_candidate) : comparison :=
  match Nat.compare
    (candidate_identity_hash left) (candidate_identity_hash right) with
  | Eq => compare_bytes (candidate_identity left) (candidate_identity right)
  | result => result
  end.

Definition replace_candidate_rmp
    (candidate : indexed_candidate) (replacement : bytes) : indexed_candidate :=
  IndexedCandidate
    (candidate_identity_hash candidate)
    (candidate_identity candidate)
    replacement.

Theorem canonical_candidate_order_is_independent_of_derived_rmp_bytes :
  forall left right left_rmp right_rmp,
    compare_candidates
      (replace_candidate_rmp left left_rmp)
      (replace_candidate_rmp right right_rmp) =
    compare_candidates left right.
Proof. reflexivity. Qed.

(** These capstones are queried by the bounded proof runner. *)
Print Assumptions emitted_opaque_par_token_has_exactly_49_bytes.
Print Assumptions compiled_opaque_query_has_no_false_negative.
Print Assumptions unsupported_opaque_shell_fails_closed.
Print Assumptions canonical_candidate_order_is_independent_of_derived_rmp_bytes.
Print Assumptions corresponding_semantic_fields_have_identical_projection.
Print Assumptions adjacent_coalescing_preserves_exact_cardinality.
Print Assumptions nested_par_projection_is_opaque_to_recursive_component_structure.
