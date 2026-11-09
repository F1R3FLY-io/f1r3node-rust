(** RPB3AdmissionGeneratedReferenceEquivalence: the executable source-indexed
    PDA over raw wire bytes against the independent typed reference, over the
    concrete generated descriptor tables.

    [generated_descriptor_raw_wire_pda_typed_reference_equivalence] composes
    the results the admission argument rests on:

    - for every type id and byte string, the source-indexed frame machine and
      the independent oracle report the same summary, through the oracle's
      projection, and the same admission verdict
      ([generated_root_matches_oracle] and the frame theorems of
      [RPB3SourceIndexedFrames]);
    - every generated program type has exactly one score class
      ([score_class_total]);
    - the semantic reducer returns a result on every typed-valid input
      ([semantic_reducer_total_on_typed_valid]);
    - the root locally-free quotient ([root_locally_free_quotient] and
      [root_locally_free_quotient_default]).

    The type id ranges over the concrete generated tables through
    [Schema.lookup_projected_program]; a type id with no generated program is
    refused by both sides.  Nothing is parameterised over a local validity
    function, an abstract Boolean fold or an externally supplied authorization.

    The red control mutates the close step.  [close_without_refusal] closes a
    message without consulting the reducer's refusal or the New key order, the
    analogue of an authorization that admits a malformed frame, and on a New with
    reversed map keys the verdict it produces differs from the oracle's. *)

From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3SemanticReducer RPB3RecursiveAuxiliarySourceReference
  RPB3SourceIndexedFrames RPB3IndependentOracleCore
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursiveProjection
  RPB3ScoreClass RPB3TypedValid RPB3SemanticReducerTotal
  RPB3RootLocallyFreeQuotient RPB3IndependentOracleRootMatch.

Import ListNotations.

Theorem generated_descriptor_raw_wire_pda_typed_reference_equivalence :
  (forall hash_fn type_id bytes,
    SourceIndexedFrames.source_indexed_root_summary hash_fn type_id bytes =
    option_map (fun summary =>
        [RPB3IndependentOracleProjection.project_source_summary summary])
      (RPB3IndependentOracleRecursiveProjection.source_unrejected_child
        (oracle_root hash_fn type_id bytes)) /\
    SourceIndexedFrames.source_indexed_root_accepts hash_fn type_id bytes =
    oracle_root_admits hash_fn type_id bytes) /\
  (length generated_type_ids = 64 /\
   NoDup generated_type_ids /\
   unclassified_types = []) /\
  (forall subject,
    typed_valid_tree subject ->
    reducer_result_acceptable (semantic_reduce subject) = true) /\
  (forall field_with field_without prefix suffix lf,
    generated_wire_program field_with = Some par_program ->
    generated_wire_program field_without = Some par_program ->
    valid_raw_body field_with = valid_raw_body field_without ->
    root_locally_free_child lf = true ->
    scan_valid (fold_left (scan_child par_program)
      (prefix ++ lf :: suffix) empty_scan) = true ->
    let w := reduce_semantic_field field_with (prefix ++ lf :: suffix) in
    let o := reduce_semantic_field field_without (prefix ++ suffix) in
    sem_score_evidence w = sem_score_evidence o /\
    sem_complete w = sem_complete o /\
    sem_definitely_refused w = sem_definitely_refused o) /\
  (forall bytes prefix suffix node,
    resolve_tag (program_fields par_program) 9 =
      Some (RPB3RawRegionTree.source_field node) ->
    RPB3RawRegionTree.source_par_semantic_default bytes (prefix ++ node :: suffix) =
    RPB3RawRegionTree.source_par_semantic_default bytes (prefix ++ suffix)).
Proof.
  refine (conj _ (conj score_class_total
    (conj semantic_reducer_total_on_typed_valid
      (conj root_locally_free_quotient root_locally_free_quotient_default)))).
  intros hash_fn type_id bytes.
  exact (conj (source_indexed_root_summary_matches_oracle hash_fn type_id bytes)
    (source_indexed_root_accepts_matches_oracle hash_fn type_id bytes)).
Qed.

(** * Non-vacuity *)

Definition constant_hash (_ : list nat) : nat := 0.

Definition ordered_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 97; 34; 3; 10; 1; 98].

Definition reversed_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 98; 34; 3; 10; 1; 97].

Example both_sides_admit_the_ordered_new :
  SourceIndexedFrames.source_indexed_root_accepts constant_hash 20
    ordered_new_body = true /\
  oracle_root_admits constant_hash 20 ordered_new_body = true.
Proof. vm_compute. split; reflexivity. Qed.

Example both_sides_refuse_the_reversed_new :
  SourceIndexedFrames.source_indexed_root_accepts constant_hash 20
    reversed_new_body = false /\
  oracle_root_admits constant_hash 20 reversed_new_body = false.
Proof. vm_compute. split; reflexivity. Qed.

(** * Red control: a close step that admits a malformed frame *)

Definition close_without_refusal (raw : raw_field) (type_id : nat)
    (children : list recursive_mixed_result)
    : option recursive_mixed_result :=
  let semantic := reduce_semantic_field raw
    (map recursive_result_semantic children) in
  Some (RecursiveMessageResult raw semantic children).

Definition accepts_with_mutated_close (hash_fn : list nat -> nat) (type_id : nat)
    (bytes : list nat) : bool :=
  match recursive_mixed_root_reference hash_fn type_id bytes with
  | Some results =>
      match close_without_refusal
          (SourceIndexedFrames.source_generated_root_field type_id bytes)
          type_id results with
      | Some _ => true
      | None => false
      end
  | None => false
  end.

Example mutated_close_admits_the_reversed_new :
  accepts_with_mutated_close constant_hash 20 reversed_new_body = true.
Proof. vm_compute. reflexivity. Qed.

Theorem mutated_close_breaks_the_equivalence :
  ~ (forall hash_fn type_id bytes,
      accepts_with_mutated_close hash_fn type_id bytes =
      oracle_root_admits hash_fn type_id bytes).
Proof.
  intros claimed.
  pose proof (claimed constant_hash 20 reversed_new_body) as agreed.
  rewrite mutated_close_admits_the_reversed_new in agreed.
  rewrite (proj2 both_sides_refuse_the_reversed_new) in agreed.
  discriminate agreed.
Qed.

Print Assumptions generated_descriptor_raw_wire_pda_typed_reference_equivalence.
Print Assumptions both_sides_admit_the_ordered_new.
Print Assumptions both_sides_refuse_the_reversed_new.
Print Assumptions mutated_close_admits_the_reversed_new.
Print Assumptions mutated_close_breaks_the_equivalence.
