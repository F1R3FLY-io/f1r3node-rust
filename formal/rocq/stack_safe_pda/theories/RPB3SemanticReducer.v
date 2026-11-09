From Stdlib Require Import List Bool Arith Lia NArith ZArith.
From StackSafePDA Require Import StackSafePDA RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement
  RPB3SourceScalarReduction RPB3RawRegionTree.

Import ListNotations.

(** A deliberately bounded semantic correction to RPB3GeneratedFieldRefinement.

    The generated program is looked up by type ID, with no caller-provided
    required-slot list. The component rules below follow reduce_component_send,
    receive, new, match, bundle, if, cost_token_stack, reduce_connective,
    reduce_unforgeable, and reduce_expr in protobuf_admission.rs. A missing
    PathMap field follows MessageFrame::finish. Raw Par ordering follows
    MessageFrame::accept: compare component score, then protobuf body.

    This file establishes definite refusals and component completeness for
    the stated typed-field slice; it does not establish full admission or
    protobuf byte parsing. *)

Inductive canonical_score : Type :=
| ScoreInteger : N -> canonical_score
| ScoreString : list nat -> canonical_score
| ScoreBytes : list nat -> canonical_score
| ScoreNode : canonical_score_forest -> canonical_score
with canonical_score_forest : Type :=
| ScoreNil : canonical_score_forest
| ScoreCons : canonical_score -> canonical_score_forest -> canonical_score_forest.

Fixpoint score_forest_of_list (children : list canonical_score)
    : canonical_score_forest :=
  match children with
  | [] => ScoreNil
  | score :: rest => ScoreCons score (score_forest_of_list rest)
  end.

Definition score_constructor (constructor : N)
    (children : list canonical_score) : canonical_score :=
  ScoreNode (ScoreCons (ScoreInteger constructor)
    (score_forest_of_list children)).

Inductive score_evidence :=
| ExactScore (score : canonical_score)
| ScoreNotDerived.

Inductive generated_score_contract :=
| RequiredComponentScore
| OptionalPayloadScore
| NoStandaloneScore
| OtherGeneratedType
| OutsideGeneratedRegistry.

Definition generated_par_component_type_ids : list nat :=
  match find (fun program =>
      GeneratedData.program_type_id program =? 1)
      GeneratedData.generated_programs with
  | None => []
  | Some program =>
      fold_right (fun slot ids =>
        match GeneratedData.slot_child_id slot with
        | Some type_id => type_id :: ids
        | None => ids
        end) [] (GeneratedData.program_slots program)
  end.

Definition optional_payload_type_ids : list nat :=
  [16; 21; 30; 32; 33; 35; 36; 37; 38; 39; 40; 41; 42; 43;
   44; 45; 46; 47; 48; 49; 50; 51; 52; 53; 54].

Definition no_standalone_score_type_ids : list nat :=
  [7; 25; 26; 34; 56; 57; 61; 62; 63; 64].

Definition listed_score_type (type_id : nat) (ids : list nat) : bool :=
  existsb (Nat.eqb type_id) ids.

Definition generated_score_contract_for (type_id : nat)
    : generated_score_contract :=
  if listed_score_type type_id generated_par_component_type_ids
  then RequiredComponentScore
  else match lookup_projected_program type_id [] [] with
  | None => OutsideGeneratedRegistry
  | Some _ =>
      if listed_score_type type_id optional_payload_type_ids
      then OptionalPayloadScore
      else if listed_score_type type_id no_standalone_score_type_ids
      then NoStandaloneScore
      else OtherGeneratedType
  end.

Theorem generated_par_component_type_ids_are_exact :
  generated_par_component_type_ids =
    [15; 19; 20; 24; 22; 60; 55; 8; 23; 13].
Proof. vm_compute. reflexivity. Qed.

Lemma listed_score_type_iff :
  forall type_id ids,
    listed_score_type type_id ids = true <-> In type_id ids.
Proof.
  intros type_id ids.
  unfold listed_score_type.
  rewrite existsb_exists.
  split.
  - intros [candidate [member equal]].
    apply Nat.eqb_eq in equal.
    now subst candidate.
  - intro member.
    exists type_id. split; [exact member|apply Nat.eqb_refl].
Qed.

Theorem required_component_score_contract_is_exact :
  forall type_id,
    generated_score_contract_for type_id = RequiredComponentScore <->
    In type_id [15; 19; 20; 24; 22; 60; 55; 8; 23; 13].
Proof.
  intro type_id.
  unfold generated_score_contract_for.
  rewrite <- generated_par_component_type_ids_are_exact.
  destruct (listed_score_type type_id generated_par_component_type_ids)
    eqn:listed.
  - split.
    + intro.
      exact (proj1 (listed_score_type_iff type_id
        generated_par_component_type_ids) listed).
    + intro. reflexivity.
  - split.
    + intro impossible.
      destruct (lookup_projected_program type_id [] [])
        as [program|].
      * destruct (listed_score_type type_id optional_payload_type_ids);
        destruct (listed_score_type type_id no_standalone_score_type_ids);
        discriminate impossible.
      * discriminate impossible.
    + intro member.
      apply (proj2 (listed_score_type_iff type_id
        generated_par_component_type_ids)) in member.
      congruence.
Qed.

Example generated_cost_token_stack_requires_component_score :
  generated_score_contract_for 13 = RequiredComponentScore.
Proof. vm_compute. reflexivity. Qed.

Example generated_receive_bind_has_optional_payload_score :
  generated_score_contract_for 16 = OptionalPayloadScore.
Proof. vm_compute. reflexivity. Qed.

Example generated_key_value_pair_has_no_standalone_score :
  generated_score_contract_for 34 = NoStandaloneScore.
Proof. vm_compute. reflexivity. Qed.

Example omitting_cost_token_stack_breaks_component_inventory :
  remove Nat.eq_dec 13 generated_par_component_type_ids <>
    [15; 19; 20; 24; 22; 60; 55; 8; 23; 13].
Proof. vm_compute. discriminate. Qed.

Print Assumptions generated_par_component_type_ids_are_exact.
Print Assumptions required_component_score_contract_is_exact.
Print Assumptions generated_cost_token_stack_requires_component_score.
Print Assumptions generated_receive_bind_has_optional_payload_score.
Print Assumptions generated_key_value_pair_has_no_standalone_score.
Print Assumptions omitting_cost_token_stack_breaks_component_inventory.

Record semantic_summary := SemanticSummary {
  sem_tag : nat;
  sem_wire : wire_kind;
  sem_payload : payload_kind;
  sem_type_id : nat;
  sem_body : list nat;
  sem_definitely_refused : bool;
  sem_complete : option bool;
  sem_score_evidence : score_evidence;
  sem_bind_source_present : bool;
  sem_case_required_present : bool
}.

Definition generated_wire_program (field : raw_field) : option generated_program :=
  match raw_payload field, raw_program_type_id field with
  | Message, Some type_id => lookup_projected_program type_id [] []
  | Scalar, None => Some empty_program
  | _, _ => None
  end.

Definition scan_child (program : generated_program) (state : scan_state)
    (child : semantic_summary) : scan_state :=
  if scan_valid state then
    match resolve_tag (program_fields program) (sem_tag child) with
    | None => ScanState false (scan_last_slot state) (scan_seen state)
        (scan_ordered_slots state) []
    | Some descriptor =>
        let slot := descriptor_slot descriptor in
        let type_matches :=
          match descriptor_payload descriptor with
          | Message =>
              match descriptor_child_type_id descriptor with
              | Some type_id => sem_type_id child =? type_id
              | None => false
              end
          | _ => true
          end in
        let admissible :=
          negb (sem_definitely_refused child) &&
          wire_eqb (sem_wire child) (descriptor_wire descriptor) &&
          payload_eqb (sem_payload child) (descriptor_payload descriptor) &&
          type_matches &&
          (slot <? program_slot_count program) &&
          slot_follows (scan_last_slot state) slot
            (descriptor_repeated descriptor) &&
          (descriptor_repeated descriptor ||
            negb (existsb (Nat.eqb slot) (scan_seen state))) in
        if admissible then
          ScanState true (Some slot) (slot :: scan_seen state)
            (scan_ordered_slots state ++ [slot]) []
        else ScanState false (scan_last_slot state) (scan_seen state)
          (scan_ordered_slots state) []
    end
  else state.

Definition seen_slot (state : scan_state) (slot : nat) : bool :=
  existsb (Nat.eqb slot) (scan_seen state).

(** The slot comes from the generated PathMap descriptor, not an annotation on
    [raw_field]. A present PathMap still needs its auxiliary-frame proof. *)
Definition mandatory_pathmaps_present (program : generated_program)
    (state : scan_state) : bool :=
  forallb (fun descriptor =>
    if payload_eqb (descriptor_payload descriptor) PathMap
    then seen_slot state (descriptor_slot descriptor)
    else true) (program_fields program).

Definition all_bind_sources_present (children : list semantic_summary) : bool :=
  forallb (fun child =>
    if sem_tag child =? 1 then sem_bind_source_present child else true) children.

Definition all_match_cases_required (children : list semantic_summary) : bool :=
  forallb (fun child =>
    if sem_tag child =? 2 then sem_case_required_present child else true) children.

(** These are the family-specific completeness predicates in the production
    reducers. The two auxiliary flags are computed from ReceiveBind and
    MatchCase descriptor slots below, rather than a child's completeness.
    Expr message arms need their payload reducer's optional score and fixed
    point state, so [None] records that outstanding obligation. *)
Definition component_completeness (type_id : nat) (state : scan_state)
    (children : list semantic_summary) : option bool :=
  match type_id with
  | 15 => Some (seen_slot state 0)                 (* Send.chan *)
  | 19 => Some (seen_slot state 1 && all_bind_sources_present children)
  | 20 => Some (seen_slot state 1)                 (* New.p *)
  | 22 => Some (seen_slot state 0 && all_match_cases_required children)
  | 8 => Some (seen_slot state 0)                  (* Bundle.body *)
  | 23 => Some (seen_slot state 0 && seen_slot state 1 && seen_slot state 2)
  | 13 => Some true                               (* CostTokenStack *)
  | 55 => Some true                               (* Connective *)
  | 60 => Some true                               (* GUnforgeable *)
  | 24 =>
      match children with
      | [] => Some true                           (* absent Expr arm *)
      | [child] =>
          if payload_eqb (sem_payload child) Scalar
          then Some true                         (* ground Expr arm *)
          else sem_complete child
      | _ => None
      end
  | 25 | 26 => Some true                     (* no-score ground payloads *)
  | 31 => Some (seen_slot state 3)
  | 32 => Some (seen_slot state 0)
  | _ => None
  end.

Definition incomplete_component_uses_nonsemantic_score (type_id : nat) : bool :=
  existsb (Nat.eqb type_id) [15; 19; 20; 22; 8; 23].

Definition scalar_true_or_absent (tag : nat)
    (children : list semantic_summary) : option bool :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some false
  | Some child =>
      match sem_payload child, sem_body child with
      | Scalar, [1] => Some true
      | _, _ => None
      end
  end.

Fixpoint exact_par_component_scores (children : list semantic_summary)
    : option (list canonical_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match exact_par_component_scores rest with
      | None => None
      | Some scores =>
          match sem_payload child with
          | Message =>
              match sem_score_evidence child with
              | ExactScore score => Some (score :: scores)
              | ScoreNotDerived => None
              end
          | Scalar => Some scores
          | _ => None
          end
      end
  end.

Definition exact_par_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_par_component_scores children,
      scalar_true_or_absent 10 children with
  | Some scores, Some connective_used =>
      Some (score_constructor 999
        (scores ++ [ScoreInteger (if connective_used then 1 else 0)]))
  | _, _ => None
  end.

Theorem exact_par_score_preserves_all_component_scores :
  forall children scores connective_used,
    exact_par_component_scores children = Some scores ->
    scalar_true_or_absent 10 children = Some connective_used ->
    exact_par_score children =
      Some (score_constructor 999
        (scores ++ [ScoreInteger (if connective_used then 1 else 0)])).
Proof.
  intros children scores connective_used collected connective.
  unfold exact_par_score.
  now rewrite collected, connective.
Qed.

Definition bundle_constructor (read write : bool) : N :=
  if read then if write then 308 else 306
  else if write then 307 else 305.

Definition exact_bundle_score (children : list semantic_summary)
    : option canonical_score :=
  match find (fun child => sem_tag child =? 1) children,
      scalar_true_or_absent 2 children,
      scalar_true_or_absent 3 children with
  | Some body, Some write, Some read =>
      match sem_score_evidence body with
      | ExactScore body_score =>
          Some (score_constructor (bundle_constructor read write) [body_score])
      | ScoreNotDerived => None
      end
  | _, _, _ => None
  end.

Definition signed_score_word (value : Z) : N :=
  Z.to_N (Z.modulo value 18446744073709551616).

Definition exact_child_score (tag : nat) (children : list semantic_summary)
    : option canonical_score :=
  match find (fun child => sem_tag child =? tag) children with
  | Some child =>
      match sem_score_evidence child with
      | ExactScore score => Some score
      | ScoreNotDerived => None
      end
  | None => None
  end.

Fixpoint exact_selected_scores (tag : nat) (children : list semantic_summary)
    : option (list canonical_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match exact_selected_scores tag rest with
      | None => None
      | Some scores =>
          if sem_tag child =? tag then
            match sem_score_evidence child with
            | ExactScore score => Some (score :: scores)
            | ScoreNotDerived => None
            end
          else Some scores
      end
  end.

Definition exact_optional_child_score (tag : nat)
    (children : list semantic_summary) : option (option canonical_score) :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some None
  | Some child =>
      match sem_score_evidence child with
      | ExactScore score => Some (Some score)
      | ScoreNotDerived => None
      end
  end.

Definition exact_cost_atom_score (children : list semantic_summary)
    : option canonical_score :=
  match children with
  | [] => Some (score_constructor 505 [])
  | [child] =>
      match sem_tag child, sem_payload child with
      | 1, Scalar =>
          Some (score_constructor 502 [ScoreBytes (sem_body child)])
      | 2, Scalar =>
          Some (score_constructor 503 [ScoreBytes (sem_body child)])
      | 3, Message =>
          match sem_score_evidence child with
          | ExactScore score => Some score
          | ScoreNotDerived => None
          end
      | 4, Scalar =>
          Some (score_constructor 507 [ScoreBytes (sem_body child)])
      | _, _ => None
      end
  | _ => None
  end.

Definition exact_cost_collection_score (constructor : N)
    (children : list semantic_summary) : option canonical_score :=
  option_map (score_constructor constructor)
    (exact_selected_scores 1 children).

Definition exact_i32_or_absent (tag : nat)
    (children : list semantic_summary) : option Z :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some 0%Z
  | Some child =>
      match RPB3SourceScalarReduction.scalar_semantic_from_source
          (RPB3SourceScalarReduction.ScalarRule tag
            RPB3DescriptorProjectionData.GeneratedData.PBSint32 true)
          (sem_body child) with
      | Some (RPB3SourceScalarReduction.ScalarI32 value) => Some value
      | _ => None
      end
  end.

Definition exact_pbint32_or_absent (tag : nat)
    (children : list semantic_summary) : option Z :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some 0%Z
  | Some child =>
      match sem_wire child, sem_payload child,
          sem_definitely_refused child with
      | Varint, Scalar, false =>
          match RPB3SourceScalarReduction.scalar_semantic_from_source
              (RPB3SourceScalarReduction.ScalarRule tag
                RPB3DescriptorProjectionData.GeneratedData.PBInt32 true)
              (sem_body child) with
          | Some (RPB3SourceScalarReduction.ScalarI32 value) => Some value
          | _ => None
          end
      | _, _, _ => None
      end
  end.

Definition exact_bool_or_absent (tag : nat)
    (children : list semantic_summary) : option bool :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some false
  | Some child =>
      match sem_wire child, sem_payload child,
          sem_definitely_refused child with
      | Varint, Scalar, false =>
          match RPB3SourceScalarReduction.scalar_semantic_from_source
              (RPB3SourceScalarReduction.ScalarRule tag
                RPB3DescriptorProjectionData.GeneratedData.PBBool true)
              (sem_body child) with
          | Some (RPB3SourceScalarReduction.ScalarBool value) => Some value
          | _ => None
          end
      | _, _, _ => None
      end
  end.

Definition exact_u32_or_absent (tag : nat)
    (children : list semantic_summary) : option N :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some 0%N
  | Some child =>
      match RPB3SourceScalarReduction.scalar_semantic_from_source
          (RPB3SourceScalarReduction.ScalarRule tag
            RPB3DescriptorProjectionData.GeneratedData.PBUint32 true)
          (sem_body child) with
      | Some (RPB3SourceScalarReduction.ScalarU32 value) => Some value
      | _ => None
      end
  end.

Definition exact_u64_or_absent (tag : nat)
    (children : list semantic_summary) : option N :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some 0%N
  | Some child =>
      match RPB3SourceScalarReduction.scalar_semantic_from_source
          (RPB3SourceScalarReduction.ScalarRule tag
            RPB3DescriptorProjectionData.GeneratedData.PBUint64 true)
          (sem_body child) with
      | Some (RPB3SourceScalarReduction.ScalarU64 value) => Some value
      | _ => None
      end
  end.

Definition score_u64_be_bytes (value : N) : list nat :=
  [N.to_nat (N.land (N.shiftr value 56) 255);
   N.to_nat (N.land (N.shiftr value 48) 255);
   N.to_nat (N.land (N.shiftr value 40) 255);
   N.to_nat (N.land (N.shiftr value 32) 255);
   N.to_nat (N.land (N.shiftr value 24) 255);
   N.to_nat (N.land (N.shiftr value 16) 255);
   N.to_nat (N.land (N.shiftr value 8) 255);
   N.to_nat (N.land value 255)].

Definition exact_cost_bound_score (children : list semantic_summary)
    : option canonical_score :=
  option_map (fun index =>
    score_constructor 504 [ScoreInteger (signed_score_word index)])
    (exact_i32_or_absent 1 children).

Definition exact_cost_region_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_u32_or_absent 1 children,
      exact_u64_or_absent 2 children,
      exact_u64_or_absent 3 children,
      exact_u64_or_absent 4 children,
      exact_u64_or_absent 5 children,
      exact_optional_child_score 6 children,
      exact_u32_or_absent 7 children with
  | Some version, Some start_line, Some start_column,
      Some end_line, Some end_column, Some signer, Some ordinal =>
      Some (score_constructor 500
        [ScoreInteger version;
         ScoreBytes (score_u64_be_bytes start_line);
         ScoreBytes (score_u64_be_bytes start_column);
         ScoreBytes (score_u64_be_bytes end_line);
         ScoreBytes (score_u64_be_bytes end_column);
         match signer with
         | Some score => score
         | None => score_constructor 506 []
         end;
         ScoreInteger ordinal])
  | _, _, _, _, _, _, _ => None
  end.

Definition exact_send_score (children : list semantic_summary)
    : option canonical_score :=
  match scalar_true_or_absent 3 children, exact_child_score 1 children,
      exact_selected_scores 2 children, scalar_true_or_absent 6 children,
      exact_optional_child_score 7 children with
  | Some persistent, Some channel, Some data, Some connective, Some cost =>
      Some (score_constructor 300
        (ScoreInteger (if persistent then 1 else 0) :: channel ::
          (data ++ [ScoreInteger (if connective then 1 else 0)] ++
            match cost with None => [] | Some score => [score] end)))
  | _, _, _, _, _ => None
  end.

Definition exact_if_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_child_score 1 children, exact_child_score 2 children,
      exact_child_score 3 children, scalar_true_or_absent 5 children with
  | Some condition, Some if_true, Some if_false, Some connective =>
      Some (score_constructor 309
        [condition; if_true; if_false;
         ScoreInteger (if connective then 1 else 0)])
  | _, _, _, _ => None
  end.

Definition new_bind_count_word (children : list semantic_summary) : option N :=
  match find (fun child => sem_tag child =? 1) children with
  | None => Some 0%N
  | Some child =>
      match RPB3SourceScalarReduction.compact_varint_value (sem_body child) with
      | Some encoded =>
          if N.leb encoded 4294967295 then
            Some (signed_score_word
              (RPB3SourceScalarReduction.zigzag_signed encoded))
          else None
      | None => None
      end
  end.

Definition scores_or_absent (scores : list canonical_score)
    : list canonical_score :=
  match scores with [] => [ScoreInteger 0] | _ => scores end.

Definition exact_new_score (children : list semantic_summary)
    : option canonical_score :=
  match new_bind_count_word children, exact_child_score 2 children,
      exact_selected_scores 4 children with
  | Some bind_count, Some body, Some injections =>
      let uris := map (fun child => ScoreString (sem_body child))
        (filter (fun child => sem_tag child =? 3) children) in
      Some (score_constructor 303
        (ScoreInteger bind_count ::
          (scores_or_absent uris ++ scores_or_absent injections ++ [body])))
  | _, _, _ => None
  end.

Definition pathmap_snapshot_from_score (score : canonical_score)
    : option (list nat) :=
  match score with
  | ScoreNode (ScoreCons (ScoreInteger constructor)
      (ScoreCons (ScoreBytes bytes)
        (ScoreCons (ScoreInteger _) ScoreNil))) =>
      if N.eqb constructor 13 then Some bytes else None
  | _ => None
  end.

Definition exact_epathmap_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_child_score 9 children, scalar_true_or_absent 4 children with
  | Some (ScoreBytes snapshot), Some connective =>
      Some (score_constructor 13
        [ScoreBytes snapshot; ScoreInteger (if connective then 1 else 0)])
  | _, _ => None
  end.

Definition exact_ezipper_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_child_score 1 children, scalar_true_or_absent 5 children with
  | Some pathmap, Some connective =>
      match pathmap_snapshot_from_score pathmap with
      | Some snapshot =>
          Some (score_constructor 14
            [ScoreBytes snapshot; ScoreInteger (if connective then 1 else 0)])
      | None => None
      end
  | _, _ => None
  end.

Definition generated_connective_scalar_tags : list nat :=
  match lookup_projected_program 55 [] [] with
  | None => []
  | Some program =>
      map descriptor_tag
        (filter (fun descriptor =>
          payload_eqb (descriptor_payload descriptor) Scalar &&
          wire_eqb (descriptor_wire descriptor) Varint &&
          match descriptor_oneof descriptor with
          | Some 0 => true
          | _ => false
          end) (program_fields program))
  end.

Definition connective_scalar_constructor (tag : nat) : option N :=
  if (5 <=? tag) && (tag <=? 9)
  then Some (N.of_nat (tag + 399))
  else None.

Theorem generated_connective_scalar_tags_are_exact :
  generated_connective_scalar_tags = [5; 6; 7; 8; 9].
Proof. vm_compute. reflexivity. Qed.

Theorem connective_scalar_constructor_domain_is_generated :
  forall tag,
    (exists constructor, connective_scalar_constructor tag = Some constructor) <->
    In tag generated_connective_scalar_tags.
Proof.
  intro tag.
  rewrite generated_connective_scalar_tags_are_exact.
  unfold connective_scalar_constructor.
  split.
  - intros [constructor selected].
    destruct ((5 <=? tag) && (tag <=? 9)) eqn:within;
      [|discriminate].
    apply andb_true_iff in within as [lower upper].
    apply Nat.leb_le in lower.
    apply Nat.leb_le in upper.
    simpl. lia.
  - intro member.
    exists (N.of_nat (tag + 399)).
    assert (within : (5 <=? tag) && (tag <=? 9) = true).
    { apply andb_true_iff. split; apply Nat.leb_le;
        simpl in member; lia. }
    now rewrite within.
Qed.

Theorem generated_connective_scalar_constructors_are_exact :
  map connective_scalar_constructor generated_connective_scalar_tags =
    [Some 404%N; Some 405%N; Some 406%N; Some 407%N; Some 408%N].
Proof. vm_compute. reflexivity. Qed.

Definition generated_connective_scalar_rules :=
  match lookup_projected_program 55 [] [] with
  | None => []
  | Some program =>
      map (RPB3SourceScalarReduction.generated_scalar_rule program)
        generated_connective_scalar_tags
  end.

Theorem generated_connective_scalar_rules_are_boolean :
  generated_connective_scalar_rules =
    [Some (RPB3SourceScalarReduction.ScalarRule 5
      RPB3DescriptorProjectionData.GeneratedData.PBBool false);
     Some (RPB3SourceScalarReduction.ScalarRule 6
      RPB3DescriptorProjectionData.GeneratedData.PBBool false);
     Some (RPB3SourceScalarReduction.ScalarRule 7
      RPB3DescriptorProjectionData.GeneratedData.PBBool false);
     Some (RPB3SourceScalarReduction.ScalarRule 8
      RPB3DescriptorProjectionData.GeneratedData.PBBool false);
     Some (RPB3SourceScalarReduction.ScalarRule 9
      RPB3DescriptorProjectionData.GeneratedData.PBBool false)].
Proof. vm_compute. reflexivity. Qed.

Definition exact_connective_score (children : list semantic_summary)
    : option canonical_score :=
  match children with
  | [] => Some (ScoreInteger 0)
  | [child] =>
      match sem_wire child, sem_definitely_refused child,
          sem_payload child, connective_scalar_constructor (sem_tag child) with
      | Varint, false, Scalar, Some constructor =>
          match RPB3SourceScalarReduction.scalar_semantic_from_source
              (RPB3SourceScalarReduction.ScalarRule (sem_tag child)
                RPB3DescriptorProjectionData.GeneratedData.PBBool false)
              (sem_body child) with
          | Some (RPB3SourceScalarReduction.ScalarBool value) =>
              Some (score_constructor constructor
                [ScoreInteger (if value then 1 else 0)])
          | _ => None
          end
      | LengthDelimited, false, Message, _ =>
          if (sem_tag child =? 3) && (sem_type_id child =? 1) then
            match sem_score_evidence child with
            | ExactScore score => Some (score_constructor 400 [score])
            | ScoreNotDerived => None
            end
          else None
      | _, _, _, _ => None
      end
  | _ => None
  end.

Definition generated_unforgeable_arms :
    list (nat * wire_kind * payload_kind * option nat) :=
  match lookup_projected_program 60 [] [] with
  | None => []
  | Some program =>
      map (fun descriptor =>
        (descriptor_tag descriptor, descriptor_wire descriptor,
          descriptor_payload descriptor,
          descriptor_child_type_id descriptor))
        (program_fields program)
  end.

Definition unforgeable_arm_contract (tag : nat)
    : option (nat * N * bool) :=
  match tag with
  | 1 => Some (61, 5%N, true)
  | 2 => Some (62, 11%N, true)
  | 3 => Some (63, 10%N, true)
  | 4 => Some (64, 12%N, false)
  | _ => None
  end.

Theorem generated_unforgeable_arms_are_exact :
  generated_unforgeable_arms =
    [(1, LengthDelimited, Message, Some 61);
     (2, LengthDelimited, Message, Some 62);
     (3, LengthDelimited, Message, Some 63);
     (4, LengthDelimited, Message, Some 64)].
Proof. vm_compute. reflexivity. Qed.

Theorem unforgeable_arm_contract_uses_generated_child_type :
  forall tag type_id constructor bytes_required,
    unforgeable_arm_contract tag =
      Some (type_id, constructor, bytes_required) ->
    In (tag, LengthDelimited, Message, Some type_id)
      generated_unforgeable_arms.
Proof.
  intros tag type_id constructor bytes_required arm.
  destruct tag as [|[|[|[|[|tag]]]]]; cbn in arm;
    try discriminate;
    rewrite generated_unforgeable_arms_are_exact; cbn.
  - left; congruence.
  - right; left; congruence.
  - right; right; left; congruence.
  - right; right; right; left; congruence.
Qed.

Definition exact_unforgeable_leaf_bytes (type_id : nat)
    (body : list nat) : option (list nat) :=
  match lookup_projected_program type_id [] [] with
  | None => None
  | Some program =>
      match RPB3RawRegionTree.decode_region (S (length body))
          body program 0 (length body) with
      | Some [] => Some []
      | Some [((RPB3RawRegionTree.SourceScalar _ descriptor) as node)] =>
          if descriptor_tag descriptor =? 1 then
            match RPB3SourceScalarReduction.generated_scalar_rule program 1 with
            | None => None
            | Some rule =>
                match RPB3SourceScalarReduction.scalar_semantic_from_source
                    rule (RPB3RawRegionTree.source_body body node) with
                | Some (RPB3SourceScalarReduction.ScalarBytes bytes) =>
                    Some bytes
                | _ => None
                end
            end
          else None
      | _ => None
      end
  end.

Definition exact_unforgeable_score (children : list semantic_summary)
    : option canonical_score :=
  match children with
  | [] => Some (score_constructor 0 [])
  | [child] =>
      match unforgeable_arm_contract (sem_tag child),
          sem_wire child, sem_payload child,
          sem_definitely_refused child with
      | Some (type_id, constructor, true), LengthDelimited,
          Message, false =>
          if sem_type_id child =? type_id then
            option_map (fun bytes =>
              score_constructor constructor [ScoreBytes bytes])
              (exact_unforgeable_leaf_bytes type_id (sem_body child))
          else None
      | Some (type_id, constructor, false), LengthDelimited,
          Message, false =>
          if (sem_type_id child =? type_id) &&
              (length (sem_body child) =? 0)
          then Some (score_constructor constructor [])
          else None
      | _, _, _, _ => None
      end
  | _ => None
  end.

Definition empty_par_score : canonical_score :=
  score_constructor 999 [ScoreInteger 0].

Definition exact_optional_score_or (tag : nat)
    (fallback : canonical_score) (children : list semantic_summary)
    : option canonical_score :=
  match exact_optional_child_score tag children with
  | None => None
  | Some None => Some fallback
  | Some (Some score) => Some score
  end.

Definition exact_typed_message_score (type_id : nat)
    (child : semantic_summary) : option canonical_score :=
  if (sem_type_id child =? type_id) &&
      wire_eqb (sem_wire child) LengthDelimited &&
      payload_eqb (sem_payload child) Message &&
      negb (sem_definitely_refused child)
  then match sem_score_evidence child with
       | ExactScore score => Some score
       | ScoreNotDerived => None
       end
  else None.

Definition exact_typed_child_score (tag type_id : nat)
    (children : list semantic_summary) : option canonical_score :=
  match find (fun child => sem_tag child =? tag) children with
  | None => None
  | Some child => exact_typed_message_score type_id child
  end.

Fixpoint exact_typed_selected_scores (tag type_id : nat)
    (children : list semantic_summary) : option (list canonical_score) :=
  match children with
  | [] => Some []
  | child :: rest =>
      match exact_typed_selected_scores tag type_id rest with
      | None => None
      | Some scores =>
          if sem_tag child =? tag then
            option_map (fun score => score :: scores)
              (exact_typed_message_score type_id child)
          else Some scores
      end
  end.

Definition exact_typed_optional_child_score (tag type_id : nat)
    (children : list semantic_summary) : option (option canonical_score) :=
  match find (fun child => sem_tag child =? tag) children with
  | None => Some None
  | Some child => option_map Some (exact_typed_message_score type_id child)
  end.

Definition exact_typed_optional_score_or (tag type_id : nat)
    (fallback : canonical_score) (children : list semantic_summary)
    : option canonical_score :=
  match exact_typed_optional_child_score tag type_id children with
  | None => None
  | Some None => Some fallback
  | Some (Some score) => Some score
  end.

Definition exact_receive_bind_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_typed_child_score 2 1 children,
      exact_typed_selected_scores 1 1 children,
      exact_typed_optional_score_or 3 6 (ScoreInteger 0) children,
      exact_typed_optional_child_score 5 12 children with
  | Some source, Some patterns, Some remainder, Some cost =>
      Some (ScoreNode (score_forest_of_list
        (source :: patterns ++ [remainder] ++
          match cost with None => [] | Some score => [score] end)))
  | _, _, _, _ => None
  end.

Definition exact_match_case_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_typed_child_score 1 1 children,
      exact_typed_child_score 2 1 children,
      exact_pbint32_or_absent 3 children,
      exact_typed_optional_score_or 4 1 empty_par_score children with
  | Some pattern, Some source, Some free_count, Some guard =>
      Some (ScoreNode (score_forest_of_list
        [pattern; source; ScoreInteger (signed_score_word free_count);
         guard]))
  | _, _, _, _ => None
  end.

Definition par_source_nodes_are_semantic_default
    (body : list nat) (nodes : list RPB3RawRegionTree.source_tree) : bool :=
  match RPB3RawRegionTree.source_par_semantic_default body nodes with
  | Some true => true
  | _ => false
  end.

Definition par_source_is_semantic_default (body : list nat) : bool :=
  match lookup_projected_program 1 [] [] with
  | None => false
  | Some program =>
      match RPB3RawRegionTree.decode_region (S (length body)) body
          program 0 (length body) with
      | None => false
      | Some nodes => par_source_nodes_are_semantic_default body nodes
      end
  end.

Definition receive_has_present_default_condition
    (children : list semantic_summary) : bool :=
  existsb (fun child =>
    (sem_tag child =? 8) && (sem_type_id child =? 1) &&
    wire_eqb (sem_wire child) LengthDelimited &&
    payload_eqb (sem_payload child) Message &&
    negb (sem_definitely_refused child) &&
    par_source_is_semantic_default (sem_body child)) children.

Definition complete_receive_has_default_condition
    (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : bool :=
  if type_id =? 19 then
    match complete with
    | Some true => receive_has_present_default_condition children
    | _ => false
    end
  else false.

Definition match_case_has_present_default_guard
    (child : semantic_summary) : bool :=
  if (sem_tag child =? 2) && (sem_type_id child =? 21) &&
      wire_eqb (sem_wire child) LengthDelimited &&
      payload_eqb (sem_payload child) Message &&
      negb (sem_definitely_refused child)
  then match lookup_projected_program 21 [] [] with
  | None => false
  | Some program =>
      match RPB3RawRegionTree.decode_region (S (length (sem_body child)))
          (sem_body child) program 0 (length (sem_body child)) with
      | None => false
      | Some nodes =>
          match find (fun node =>
              descriptor_tag (RPB3RawRegionTree.source_field node) =? 4)
              nodes with
          | Some (RPB3RawRegionTree.SourceMessage _ _ guard_children) =>
              par_source_nodes_are_semantic_default (sem_body child)
                guard_children
          | _ => false
          end
      end
  end
  else false.

Definition complete_match_has_default_guard
    (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : bool :=
  if type_id =? 22 then
    match complete with
    | Some true => existsb match_case_has_present_default_guard children
    | _ => false
    end
  else false.

Definition exact_receive_score (children : list semantic_summary)
    : option canonical_score :=
  if receive_has_present_default_condition children then None else
  match exact_bool_or_absent 3 children,
      exact_bool_or_absent 4 children,
      exact_typed_selected_scores 1 16 children,
      exact_typed_child_score 2 1 children,
      exact_pbint32_or_absent 5 children,
      exact_bool_or_absent 7 children,
      exact_typed_optional_score_or 8 1 empty_par_score children,
      exact_typed_optional_child_score 9 12 children with
  | Some persistent, Some peek, Some binds, Some body,
      Some bind_count, Some connective, Some condition, Some cost =>
      Some (score_constructor 301
        ([ScoreInteger (if persistent then 1 else 0);
          ScoreInteger (if peek then 1 else 0)] ++ binds ++
         [body; ScoreInteger (signed_score_word bind_count);
          ScoreInteger (if connective then 1 else 0); condition] ++
         match cost with None => [] | Some score => [score] end))
  | _, _, _, _, _, _, _, _ => None
  end.

Definition exact_match_score (children : list semantic_summary)
    : option canonical_score :=
  if existsb match_case_has_present_default_guard children then None else
  match exact_typed_child_score 1 1 children,
      exact_typed_selected_scores 2 21 children,
      exact_bool_or_absent 5 children with
  | Some target, Some cases, Some connective =>
      Some (score_constructor 304
        ([target] ++ cases ++ [ScoreInteger (if connective then 1 else 0)]))
  | _, _, _ => None
  end.

(** BigRational and FixedPoint child summaries deliberately carry no score.
    The Expr parent re-decodes their generated body and scores semantic fields.
    Decoding a region checks field shape and order; each present scalar must
    also pass its generated scalar rule to reject explicit defaults and
    noncanonical or out-of-range uint32 encodings. *)
Definition exact_ground_node_at (tag : nat)
    (nodes : list RPB3RawRegionTree.source_tree)
    : option RPB3RawRegionTree.source_tree :=
  find (fun node =>
    RPB3GeneratedFieldRefinement.descriptor_tag
      (RPB3RawRegionTree.source_field node) =? tag) nodes.

Definition exact_ground_bytes_or_absent
    (program : RPB3GeneratedFieldRefinement.generated_program)
    (tag : nat) (body : list nat)
    (nodes : list RPB3RawRegionTree.source_tree)
    : option (list nat) :=
  match exact_ground_node_at tag nodes with
  | None => Some []
  | Some ((RPB3RawRegionTree.SourceScalar _ _) as node) =>
      match RPB3SourceScalarReduction.generated_scalar_rule program tag with
      | None => None
      | Some rule =>
          match RPB3SourceScalarReduction.scalar_semantic_from_source rule
              (RPB3RawRegionTree.source_body body node) with
          | Some (RPB3SourceScalarReduction.ScalarBytes bytes) => Some bytes
          | _ => None
          end
      end
  | Some _ => None
  end.

Definition exact_ground_u32_or_absent
    (program : RPB3GeneratedFieldRefinement.generated_program)
    (tag : nat) (body : list nat)
    (nodes : list RPB3RawRegionTree.source_tree) : option N :=
  match exact_ground_node_at tag nodes with
  | None => Some 0%N
  | Some ((RPB3RawRegionTree.SourceScalar _ _) as node) =>
      match RPB3SourceScalarReduction.generated_scalar_rule program tag with
      | None => None
      | Some rule =>
          match RPB3SourceScalarReduction.scalar_semantic_from_source rule
              (RPB3RawRegionTree.source_body body node) with
          | Some (RPB3SourceScalarReduction.ScalarU32 value) => Some value
          | _ => None
          end
      end
  | Some _ => None
  end.

Definition exact_bigrat_parent_score (child : semantic_summary)
    : option canonical_score :=
  if (sem_type_id child =? 25) &&
      wire_eqb (sem_wire child) LengthDelimited &&
      payload_eqb (sem_payload child) Message &&
      negb (sem_definitely_refused child) then
    match lookup_projected_program 25 [] [] with
    | None => None
    | Some program =>
        match RPB3RawRegionTree.decode_region (S (length (sem_body child)))
            (sem_body child) program 0 (length (sem_body child)) with
        | None => None
        | Some nodes =>
            match exact_ground_bytes_or_absent program 1
                (sem_body child) nodes,
                exact_ground_bytes_or_absent program 2
                  (sem_body child) nodes with
            | Some numerator, Some denominator =>
                Some (score_constructor 16
                  [ScoreBytes numerator; ScoreBytes denominator])
            | _, _ => None
            end
        end
    end
  else None.

Definition exact_fixed_point_parent_score (child : semantic_summary)
    : option canonical_score :=
  if (sem_type_id child =? 26) &&
      wire_eqb (sem_wire child) LengthDelimited &&
      payload_eqb (sem_payload child) Message &&
      negb (sem_definitely_refused child) then
    match lookup_projected_program 26 [] [] with
    | None => None
    | Some program =>
        match RPB3RawRegionTree.decode_region (S (length (sem_body child)))
            (sem_body child) program 0 (length (sem_body child)) with
        | None => None
        | Some nodes =>
            match exact_ground_bytes_or_absent program 1
                (sem_body child) nodes,
                exact_ground_u32_or_absent program 2
                  (sem_body child) nodes with
            | Some unscaled, Some scale =>
                Some (score_constructor 17
                  [ScoreBytes unscaled;
                   ScoreNode (ScoreCons (ScoreInteger scale) ScoreNil)])
            | _, _ => None
            end
        end
    end
  else None.

Definition exact_expr_score (children : list semantic_summary)
    : option canonical_score :=
  match children with
  | [] => Some (score_constructor 0 [])
  | [child] =>
      match sem_payload child with
      | Scalar =>
          match sem_tag child with
          | 1 =>
              match sem_body child with
              | [0] => Some (score_constructor 1 [ScoreInteger 1])
              | [1] => Some (score_constructor 1 [ScoreInteger 0])
              | _ => None
              end
          | 2 =>
              option_map (fun encoded => score_constructor 2
                [ScoreInteger (signed_score_word
                  (RPB3SourceScalarReduction.zigzag_signed encoded))])
                (RPB3SourceScalarReduction.compact_varint_value (sem_body child))
          | 3 => Some (score_constructor 3 [ScoreString (sem_body child)])
          | 4 => Some (score_constructor 4 [ScoreString (sem_body child)])
          | 25 => Some (score_constructor 116 [ScoreBytes (sem_body child)])
          | 34 =>
              match RPB3SourceScalarReduction.scalar_semantic_from_source
                  (RPB3SourceScalarReduction.ScalarRule 34
                    RPB3DescriptorProjectionData.GeneratedData.PBFixed64 false)
                  (sem_body child) with
              | Some (RPB3SourceScalarReduction.ScalarU64 bits) =>
                  Some (score_constructor 14 [ScoreInteger bits])
              | _ => None
              end
          | 35 => Some (score_constructor 15 [ScoreBytes (sem_body child)])
          | _ => None
          end
      | Message =>
          match sem_tag child with
          | 36 => exact_bigrat_parent_score child
          | 37 => exact_fixed_point_parent_score child
          | _ =>
              match sem_score_evidence child with
              | ExactScore score => Some score
              | ScoreNotDerived => None
              end
          end
      | _ => None
      end
  | _ => None
  end.

Definition exact_or_unknown (score : option canonical_score) : score_evidence :=
  match score with Some value => ExactScore value | None => ScoreNotDerived end.

Definition score_evidence_for (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  match type_id, complete with
  | 1, _ =>
      match exact_par_score children with
      | Some score => ExactScore score
      | None => ScoreNotDerived
      end
  | 9, _ => exact_or_unknown (exact_cost_collection_score 501 children)
  | 10, _ => exact_or_unknown (exact_cost_atom_score children)
  | 11, _ => exact_or_unknown (exact_cost_bound_score children)
  | 12, _ => exact_or_unknown (exact_cost_region_score children)
  | 13, Some true =>
      exact_or_unknown (exact_cost_collection_score 508 children)
  | 16, _ => exact_or_unknown (exact_receive_bind_score children)
  | 21, _ => exact_or_unknown (exact_match_case_score children)
  | 15, Some true => exact_or_unknown (exact_send_score children)
  | 23, Some true => exact_or_unknown (exact_if_score children)
  | 8, Some true =>
      match exact_bundle_score children with
      | Some score => ExactScore score
      | None => ScoreNotDerived
      end
  | 20, Some true => exact_or_unknown (exact_new_score children)
  | 19, Some true => exact_or_unknown (exact_receive_score children)
  | 22, Some true => exact_or_unknown (exact_match_score children)
  | 24, Some true => exact_or_unknown (exact_expr_score children)
  | 24, Some false => ExactScore (score_constructor 2147483647 [])
  | 55, Some true => exact_or_unknown (exact_connective_score children)
  | 60, Some true => exact_or_unknown (exact_unforgeable_score children)
  | 31, Some true => exact_or_unknown (exact_epathmap_score children)
  | 32, Some true => exact_or_unknown (exact_ezipper_score children)
  | _, Some false =>
      if incomplete_component_uses_nonsemantic_score type_id
      then ExactScore (score_constructor 2147483647 [])
      else ScoreNotDerived
  | _, _ => ScoreNotDerived
  end.

Theorem absent_connective_has_exact_integer_score :
  score_evidence_for 55 (Some true) [] = ExactScore (ScoreInteger 0).
Proof. reflexivity. Qed.

Theorem connective_scalar_score_uses_checked_source_boolean :
  forall child constructor value,
    sem_wire child = Varint ->
    sem_definitely_refused child = false ->
    sem_payload child = Scalar ->
    connective_scalar_constructor (sem_tag child) = Some constructor ->
    RPB3SourceScalarReduction.scalar_semantic_from_source
      (RPB3SourceScalarReduction.ScalarRule (sem_tag child)
        RPB3DescriptorProjectionData.GeneratedData.PBBool false)
      (sem_body child) =
      Some (RPB3SourceScalarReduction.ScalarBool value) ->
    score_evidence_for 55 (Some true) [child] =
      ExactScore (score_constructor constructor
        [ScoreInteger (if value then 1 else 0)]).
Proof.
  intros child constructor value wire accepted payload selected decoded.
  cbn [score_evidence_for exact_connective_score].
  now rewrite wire, accepted, payload, selected, decoded.
Qed.

Theorem connective_not_score_uses_checked_par_child_evidence :
  forall child score,
    sem_tag child = 3 ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_type_id child = 1 ->
    sem_definitely_refused child = false ->
    sem_score_evidence child = ExactScore score ->
    score_evidence_for 55 (Some true) [child] =
      ExactScore (score_constructor 400 [score]).
Proof.
  intros child score tag wire payload type_id accepted exact.
  cbn [score_evidence_for exact_connective_score].
  now rewrite tag, wire, payload, type_id, accepted, exact.
Qed.

Theorem connective_not_without_child_score_remains_underived :
  forall child,
    sem_tag child = 3 ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_type_id child = 1 ->
    sem_definitely_refused child = false ->
    sem_score_evidence child = ScoreNotDerived ->
    score_evidence_for 55 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child tag wire payload type_id accepted underived.
  cbn [score_evidence_for exact_connective_score].
  now rewrite tag, wire, payload, type_id, accepted, underived.
Qed.

Theorem connective_invalid_scalar_source_remains_underived :
  forall child constructor,
    sem_wire child = Varint ->
    sem_definitely_refused child = false ->
    sem_payload child = Scalar ->
    connective_scalar_constructor (sem_tag child) = Some constructor ->
    RPB3SourceScalarReduction.scalar_semantic_from_source
      (RPB3SourceScalarReduction.ScalarRule (sem_tag child)
        RPB3DescriptorProjectionData.GeneratedData.PBBool false)
      (sem_body child) = None ->
    score_evidence_for 55 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child constructor wire accepted payload selected invalid.
  cbn [score_evidence_for exact_connective_score].
  now rewrite wire, accepted, payload, selected, invalid.
Qed.

Theorem connective_wrong_wire_remains_underived :
  forall child,
    sem_payload child = Scalar ->
    sem_wire child <> Varint ->
    score_evidence_for 55 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child payload wire.
  cbn [score_evidence_for exact_connective_score].
  rewrite payload.
  destruct (sem_wire child); try contradiction;
    destruct (sem_definitely_refused child); reflexivity.
Qed.

Theorem connective_refused_child_remains_underived :
  forall child,
    sem_definitely_refused child = true ->
    score_evidence_for 55 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child refused.
  cbn [score_evidence_for exact_connective_score].
  rewrite refused.
  now destruct (sem_wire child).
Qed.

Theorem connective_other_message_arm_remains_underived :
  forall child,
    sem_payload child = Message ->
    sem_tag child <> 3 ->
    score_evidence_for 55 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child payload other.
  cbn [score_evidence_for exact_connective_score].
  rewrite payload.
  apply Nat.eqb_neq in other.
  destruct (sem_wire child), (sem_definitely_refused child),
    (connective_scalar_constructor (sem_tag child));
    simpl; try reflexivity; now rewrite other.
Qed.

Example connective_bool_true_source_has_exact_score :
  score_evidence_for 55 (Some true)
    [SemanticSummary 5 Varint Scalar 0 [1] false None
      ScoreNotDerived false false] =
    ExactScore (score_constructor 404 [ScoreInteger 1]).
Proof. vm_compute. reflexivity. Qed.

Example connective_nonboolean_scalar_cannot_claim_exact_score :
  score_evidence_for 55 (Some true)
    [SemanticSummary 5 Varint Scalar 0 [2] false None
      ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example connective_bool_source_on_fixed_wire_remains_underived :
  score_evidence_for 55 (Some true)
    [SemanticSummary 5 Fixed32 Scalar 0 [1] false None
      ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions generated_connective_scalar_tags_are_exact.
Print Assumptions connective_scalar_constructor_domain_is_generated.
Print Assumptions generated_connective_scalar_constructors_are_exact.
Print Assumptions generated_connective_scalar_rules_are_boolean.
Print Assumptions absent_connective_has_exact_integer_score.
Print Assumptions connective_scalar_score_uses_checked_source_boolean.
Print Assumptions connective_not_score_uses_checked_par_child_evidence.
Print Assumptions connective_not_without_child_score_remains_underived.
Print Assumptions connective_invalid_scalar_source_remains_underived.
Print Assumptions connective_wrong_wire_remains_underived.
Print Assumptions connective_refused_child_remains_underived.
Print Assumptions connective_other_message_arm_remains_underived.
Print Assumptions connective_bool_true_source_has_exact_score.
Print Assumptions connective_nonboolean_scalar_cannot_claim_exact_score.
Print Assumptions connective_bool_source_on_fixed_wire_remains_underived.

Theorem absent_unforgeable_has_exact_constructor_score :
  score_evidence_for 60 (Some true) [] =
    ExactScore (score_constructor 0 []).
Proof. reflexivity. Qed.

Theorem unforgeable_bytes_arm_uses_checked_nested_source :
  forall child type_id constructor bytes,
    unforgeable_arm_contract (sem_tag child) =
      Some (type_id, constructor, true) ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child = type_id ->
    exact_unforgeable_leaf_bytes type_id (sem_body child) = Some bytes ->
    score_evidence_for 60 (Some true) [child] =
      ExactScore (score_constructor constructor [ScoreBytes bytes]).
Proof.
  intros child type_id constructor bytes arm wire payload accepted
    child_type decoded.
  cbn [score_evidence_for exact_unforgeable_score].
  rewrite arm, wire, payload, accepted, child_type.
  now rewrite Nat.eqb_refl, decoded.
Qed.

Theorem unforgeable_unit_arm_has_exact_constructor_score :
  forall child type_id constructor,
    unforgeable_arm_contract (sem_tag child) =
      Some (type_id, constructor, false) ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child = type_id ->
    sem_body child = [] ->
    score_evidence_for 60 (Some true) [child] =
      ExactScore (score_constructor constructor []).
Proof.
  intros child type_id constructor arm wire payload accepted
    child_type body.
  cbn [score_evidence_for exact_unforgeable_score].
  now rewrite arm, wire, payload, accepted, child_type,
    Nat.eqb_refl, body.
Qed.

Theorem unforgeable_malformed_nested_source_remains_underived :
  forall child type_id constructor,
    unforgeable_arm_contract (sem_tag child) =
      Some (type_id, constructor, true) ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child = type_id ->
    exact_unforgeable_leaf_bytes type_id (sem_body child) = None ->
    score_evidence_for 60 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child type_id constructor arm wire payload accepted
    child_type malformed.
  cbn [score_evidence_for exact_unforgeable_score].
  rewrite arm, wire, payload, accepted, child_type.
  now rewrite Nat.eqb_refl, malformed.
Qed.

Theorem unforgeable_wrong_child_type_remains_underived :
  forall child type_id constructor bytes_required,
    unforgeable_arm_contract (sem_tag child) =
      Some (type_id, constructor, bytes_required) ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_type_id child <> type_id ->
    score_evidence_for 60 (Some true) [child] = ScoreNotDerived.
Proof.
  intros child type_id constructor bytes_required arm wire
    payload accepted wrong.
  apply Nat.eqb_neq in wrong.
  destruct bytes_required;
    cbn [score_evidence_for exact_unforgeable_score];
    rewrite arm, wire, payload, accepted, wrong;
    reflexivity.
Qed.

Example unforgeable_private_source_has_exact_bytes_score :
  score_evidence_for 60 (Some true)
    [SemanticSummary 1 LengthDelimited Message 61 [10; 1; 42]
      false None ScoreNotDerived false false] =
    ExactScore (score_constructor 5 [ScoreBytes [42]]).
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_deploy_id_source_has_exact_bytes_score :
  score_evidence_for 60 (Some true)
    [SemanticSummary 2 LengthDelimited Message 62 [10; 1; 42]
      false None ScoreNotDerived false false] =
    ExactScore (score_constructor 11 [ScoreBytes [42]]).
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_deployer_id_source_has_exact_bytes_score :
  score_evidence_for 60 (Some true)
    [SemanticSummary 3 LengthDelimited Message 63 [10; 1; 42]
      false None ScoreNotDerived false false] =
    ExactScore (score_constructor 10 [ScoreBytes [42]]).
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_sys_auth_source_has_exact_unit_score :
  score_evidence_for 60 (Some true)
    [SemanticSummary 4 LengthDelimited Message 64 []
      false None ScoreNotDerived false false] =
    ExactScore (score_constructor 12 []).
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_truncated_nested_source_remains_underived :
  score_evidence_for 60 (Some true)
    [SemanticSummary 1 LengthDelimited Message 61 [10; 2; 42]
      false None ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_explicit_empty_bytes_remain_underived :
  score_evidence_for 60 (Some true)
    [SemanticSummary 1 LengthDelimited Message 61 [10; 0]
      false None ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_wrong_variant_child_type_remains_underived :
  score_evidence_for 60 (Some true)
    [SemanticSummary 1 LengthDelimited Message 62 [10; 1; 42]
      false None ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example unforgeable_nonempty_unit_source_remains_underived :
  score_evidence_for 60 (Some true)
    [SemanticSummary 4 LengthDelimited Message 64 [10; 1; 42]
      false None ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions generated_unforgeable_arms_are_exact.
Print Assumptions unforgeable_arm_contract_uses_generated_child_type.
Print Assumptions absent_unforgeable_has_exact_constructor_score.
Print Assumptions unforgeable_bytes_arm_uses_checked_nested_source.
Print Assumptions unforgeable_unit_arm_has_exact_constructor_score.
Print Assumptions unforgeable_malformed_nested_source_remains_underived.
Print Assumptions unforgeable_wrong_child_type_remains_underived.
Print Assumptions unforgeable_private_source_has_exact_bytes_score.
Print Assumptions unforgeable_deploy_id_source_has_exact_bytes_score.
Print Assumptions unforgeable_deployer_id_source_has_exact_bytes_score.
Print Assumptions unforgeable_sys_auth_source_has_exact_unit_score.
Print Assumptions unforgeable_truncated_nested_source_remains_underived.
Print Assumptions unforgeable_explicit_empty_bytes_remain_underived.
Print Assumptions unforgeable_wrong_variant_child_type_remains_underived.
Print Assumptions unforgeable_nonempty_unit_source_remains_underived.

Theorem generated_match_case_free_count_is_checked_pbint32 :
  match lookup_projected_program 21 [] [] with
  | None => None
  | Some program =>
      RPB3SourceScalarReduction.generated_scalar_rule program 3
  end = Some (RPB3SourceScalarReduction.ScalarRule 3
    RPB3DescriptorProjectionData.GeneratedData.PBInt32 true).
Proof. vm_compute. reflexivity. Qed.

Theorem receive_bind_exact_score_uses_source_patterns_and_optional_values :
  forall children complete source patterns remainder cost,
    exact_typed_child_score 2 1 children = Some source ->
    exact_typed_selected_scores 1 1 children = Some patterns ->
    exact_typed_optional_score_or 3 6 (ScoreInteger 0) children =
      Some remainder ->
    exact_typed_optional_child_score 5 12 children = Some cost ->
    score_evidence_for 16 complete children =
      ExactScore (ScoreNode (score_forest_of_list
        (source :: patterns ++ [remainder] ++
          match cost with None => [] | Some score => [score] end))).
Proof.
  intros children complete source patterns remainder cost
    source_exact patterns_exact remainder_exact cost_exact.
  cbn [score_evidence_for].
  unfold exact_receive_bind_score.
  now rewrite source_exact, patterns_exact, remainder_exact, cost_exact.
Qed.

Theorem match_case_exact_score_uses_source_checked_free_count :
  forall children complete pattern source free_count guard,
    exact_typed_child_score 1 1 children = Some pattern ->
    exact_typed_child_score 2 1 children = Some source ->
    exact_pbint32_or_absent 3 children = Some free_count ->
    exact_typed_optional_score_or 4 1 empty_par_score children = Some guard ->
    score_evidence_for 21 complete children =
      ExactScore (ScoreNode (score_forest_of_list
        [pattern; source; ScoreInteger (signed_score_word free_count);
         guard])).
Proof.
  intros children complete pattern source free_count guard
    pattern_exact source_exact count_exact guard_exact.
  cbn [score_evidence_for].
  unfold exact_match_case_score.
  now rewrite pattern_exact, source_exact, count_exact, guard_exact.
Qed.

Theorem receive_bind_without_source_score_remains_underived :
  forall children complete,
    exact_typed_child_score 2 1 children = None ->
    score_evidence_for 16 complete children = ScoreNotDerived.
Proof.
  intros children complete missing.
  cbn [score_evidence_for].
  unfold exact_receive_bind_score.
  now rewrite missing.
Qed.

Theorem match_case_without_pattern_score_remains_underived :
  forall children complete,
    exact_typed_child_score 1 1 children = None ->
    score_evidence_for 21 complete children = ScoreNotDerived.
Proof.
  intros children complete missing.
  cbn [score_evidence_for].
  unfold exact_match_case_score.
  now rewrite missing.
Qed.

Theorem match_case_without_source_score_remains_underived :
  forall children complete,
    exact_typed_child_score 2 1 children = None ->
    score_evidence_for 21 complete children = ScoreNotDerived.
Proof.
  intros children complete missing.
  cbn [score_evidence_for].
  unfold exact_match_case_score.
  destruct (exact_typed_child_score 1 1 children) as [pattern|].
  - now rewrite missing.
  - reflexivity.
Qed.

Definition auxiliary_empty_par_child (tag : nat) : semantic_summary :=
  SemanticSummary tag LengthDelimited Message 1 [] false None
    (ExactScore empty_par_score) false false.

Example source_present_receive_bind_has_exact_bare_node_score :
  score_evidence_for 16 None [auxiliary_empty_par_child 2] =
    ExactScore (ScoreNode (score_forest_of_list
      [empty_par_score; ScoreInteger 0])).
Proof. vm_compute. reflexivity. Qed.

Example required_match_case_fields_have_exact_bare_node_score :
  score_evidence_for 21 None
    [auxiliary_empty_par_child 1; auxiliary_empty_par_child 2] =
    ExactScore (ScoreNode (score_forest_of_list
      [empty_par_score; empty_par_score; ScoreInteger 0;
       empty_par_score])).
Proof. vm_compute. reflexivity. Qed.

Example malformed_match_case_free_count_remains_underived :
  score_evidence_for 21 None
    [auxiliary_empty_par_child 1; auxiliary_empty_par_child 2;
     SemanticSummary 3 Varint Scalar 0 [128; 0] false None
       ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example receive_bind_wrong_source_wire_remains_underived :
  score_evidence_for 16 None
    [SemanticSummary 2 Varint Message 1 [] false None
      (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example receive_bind_wrong_source_payload_remains_underived :
  score_evidence_for 16 None
    [SemanticSummary 2 LengthDelimited Scalar 1 [] false None
      (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example receive_bind_wrong_source_type_remains_underived :
  score_evidence_for 16 None
    [SemanticSummary 2 LengthDelimited Message 6 [] false None
      (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example receive_bind_wrong_pattern_type_remains_underived :
  score_evidence_for 16 None
    [SemanticSummary 1 LengthDelimited Message 6 [] false None
       (ExactScore empty_par_score) false false;
     auxiliary_empty_par_child 2] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example receive_bind_wrong_remainder_type_remains_underived :
  score_evidence_for 16 None
    [auxiliary_empty_par_child 2;
     SemanticSummary 3 LengthDelimited Message 1 [] false None
       (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example receive_bind_wrong_cost_type_remains_underived :
  score_evidence_for 16 None
    [auxiliary_empty_par_child 2;
     SemanticSummary 5 LengthDelimited Message 1 [] false None
       (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example match_case_wrong_pattern_wire_remains_underived :
  score_evidence_for 21 None
    [SemanticSummary 1 Varint Message 1 [] false None
       (ExactScore empty_par_score) false false;
     auxiliary_empty_par_child 2] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example match_case_wrong_pattern_payload_remains_underived :
  score_evidence_for 21 None
    [SemanticSummary 1 LengthDelimited Scalar 1 [] false None
       (ExactScore empty_par_score) false false;
     auxiliary_empty_par_child 2] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example match_case_wrong_pattern_type_remains_underived :
  score_evidence_for 21 None
    [SemanticSummary 1 LengthDelimited Message 6 [] false None
       (ExactScore empty_par_score) false false;
     auxiliary_empty_par_child 2] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example match_case_wrong_source_type_remains_underived :
  score_evidence_for 21 None
    [auxiliary_empty_par_child 1;
     SemanticSummary 2 LengthDelimited Message 6 [] false None
       (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example match_case_wrong_guard_type_remains_underived :
  score_evidence_for 21 None
    [auxiliary_empty_par_child 1; auxiliary_empty_par_child 2;
     SemanticSummary 4 LengthDelimited Message 6 [] false None
       (ExactScore empty_par_score) false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions generated_match_case_free_count_is_checked_pbint32.
Print Assumptions receive_bind_exact_score_uses_source_patterns_and_optional_values.
Print Assumptions match_case_exact_score_uses_source_checked_free_count.
Print Assumptions receive_bind_without_source_score_remains_underived.
Print Assumptions match_case_without_pattern_score_remains_underived.
Print Assumptions match_case_without_source_score_remains_underived.
Print Assumptions source_present_receive_bind_has_exact_bare_node_score.
Print Assumptions required_match_case_fields_have_exact_bare_node_score.
Print Assumptions malformed_match_case_free_count_remains_underived.
Print Assumptions receive_bind_wrong_source_wire_remains_underived.
Print Assumptions receive_bind_wrong_source_payload_remains_underived.
Print Assumptions receive_bind_wrong_source_type_remains_underived.
Print Assumptions receive_bind_wrong_pattern_type_remains_underived.
Print Assumptions receive_bind_wrong_remainder_type_remains_underived.
Print Assumptions receive_bind_wrong_cost_type_remains_underived.
Print Assumptions match_case_wrong_pattern_wire_remains_underived.
Print Assumptions match_case_wrong_pattern_payload_remains_underived.
Print Assumptions match_case_wrong_pattern_type_remains_underived.
Print Assumptions match_case_wrong_source_type_remains_underived.
Print Assumptions match_case_wrong_guard_type_remains_underived.

Theorem generated_receive_parent_scalar_rules_are_exact :
  match lookup_projected_program 19 [] [] with
  | None => []
  | Some program =>
      map (RPB3SourceScalarReduction.generated_scalar_rule program)
        [3; 4; 5; 7]
  end =
    [Some (RPB3SourceScalarReduction.ScalarRule 3
      RPB3DescriptorProjectionData.GeneratedData.PBBool true);
     Some (RPB3SourceScalarReduction.ScalarRule 4
      RPB3DescriptorProjectionData.GeneratedData.PBBool true);
     Some (RPB3SourceScalarReduction.ScalarRule 5
      RPB3DescriptorProjectionData.GeneratedData.PBInt32 true);
     Some (RPB3SourceScalarReduction.ScalarRule 7
      RPB3DescriptorProjectionData.GeneratedData.PBBool true)].
Proof. vm_compute. reflexivity. Qed.

Theorem generated_match_parent_scalar_rule_is_exact :
  match lookup_projected_program 22 [] [] with
  | None => None
  | Some program =>
      RPB3SourceScalarReduction.generated_scalar_rule program 5
  end = Some (RPB3SourceScalarReduction.ScalarRule 5
    RPB3DescriptorProjectionData.GeneratedData.PBBool true).
Proof. vm_compute. reflexivity. Qed.

Theorem absent_parent_scalars_have_default_values :
  forall tag,
    exact_bool_or_absent tag [] = Some false /\
    exact_pbint32_or_absent tag [] = Some 0%Z.
Proof. intro tag. split; reflexivity. Qed.

Theorem complete_receive_score_retains_checked_source_values :
  forall children persistent peek binds body bind_count connective
      condition cost,
    receive_has_present_default_condition children = false ->
    exact_bool_or_absent 3 children = Some persistent ->
    exact_bool_or_absent 4 children = Some peek ->
    exact_typed_selected_scores 1 16 children = Some binds ->
    exact_typed_child_score 2 1 children = Some body ->
    exact_pbint32_or_absent 5 children = Some bind_count ->
    exact_bool_or_absent 7 children = Some connective ->
    exact_typed_optional_score_or 8 1 empty_par_score children =
      Some condition ->
    exact_typed_optional_child_score 9 12 children = Some cost ->
    score_evidence_for 19 (Some true) children =
      ExactScore (score_constructor 301
        ([ScoreInteger (if persistent then 1 else 0);
          ScoreInteger (if peek then 1 else 0)] ++ binds ++
         [body; ScoreInteger (signed_score_word bind_count);
          ScoreInteger (if connective then 1 else 0); condition] ++
         match cost with None => [] | Some score => [score] end)).
Proof.
  intros children persistent peek binds body bind_count connective
    condition cost no_default persistent_exact peek_exact binds_exact
    body_exact count_exact connective_exact condition_exact cost_exact.
  cbn [score_evidence_for].
  unfold exact_receive_score.
  now rewrite no_default, persistent_exact, peek_exact, binds_exact,
    body_exact, count_exact, connective_exact, condition_exact, cost_exact.
Qed.

Theorem complete_match_score_retains_checked_source_values :
  forall children target cases connective,
    existsb match_case_has_present_default_guard children = false ->
    exact_typed_child_score 1 1 children = Some target ->
    exact_typed_selected_scores 2 21 children = Some cases ->
    exact_bool_or_absent 5 children = Some connective ->
    score_evidence_for 22 (Some true) children =
      ExactScore (score_constructor 304
        ([target] ++ cases ++
         [ScoreInteger (if connective then 1 else 0)])).
Proof.
  intros children target cases connective no_default target_exact
    cases_exact connective_exact.
  cbn [score_evidence_for].
  unfold exact_match_score.
  now rewrite no_default, target_exact, cases_exact, connective_exact.
Qed.

Theorem complete_receive_default_condition_has_no_claimed_score :
  forall children,
    receive_has_present_default_condition children = true ->
    score_evidence_for 19 (Some true) children = ScoreNotDerived.
Proof.
  intros children default.
  cbn [score_evidence_for].
  unfold exact_receive_score.
  now rewrite default.
Qed.

Theorem complete_match_default_guard_has_no_claimed_score :
  forall children,
    existsb match_case_has_present_default_guard children = true ->
    score_evidence_for 22 (Some true) children = ScoreNotDerived.
Proof.
  intros children default.
  cbn [score_evidence_for].
  unfold exact_match_score.
  now rewrite default.
Qed.

Theorem complete_receive_missing_persistent_evidence_remains_underived :
  forall children,
    exact_bool_or_absent 3 children = None ->
    score_evidence_for 19 (Some true) children = ScoreNotDerived.
Proof.
  intros children missing.
  cbn [score_evidence_for].
  unfold exact_receive_score.
  destruct (receive_has_present_default_condition children);
    [reflexivity |].
  now rewrite missing.
Qed.

Theorem complete_match_missing_target_evidence_remains_underived :
  forall children,
    exact_typed_child_score 1 1 children = None ->
    score_evidence_for 22 (Some true) children = ScoreNotDerived.
Proof.
  intros children missing.
  cbn [score_evidence_for].
  unfold exact_match_score.
  destruct (existsb match_case_has_present_default_guard children);
    [reflexivity |].
  now rewrite missing.
Qed.

Theorem incomplete_receive_and_match_keep_nonsemantic_scores :
  forall children,
    score_evidence_for 19 (Some false) children =
      ExactScore (score_constructor 2147483647 []) /\
    score_evidence_for 22 (Some false) children =
      ExactScore (score_constructor 2147483647 []).
Proof. intro children. split; reflexivity. Qed.

Example receive_true_boolean_uses_checked_source :
  exact_bool_or_absent 3
    [SemanticSummary 3 Varint Scalar 0 [1] false None
      ScoreNotDerived false false] = Some true.
Proof. vm_compute. reflexivity. Qed.

Example receive_default_boolean_remains_underived :
  exact_bool_or_absent 3
    [SemanticSummary 3 Varint Scalar 0 [0] false None
      ScoreNotDerived false false] = None.
Proof. vm_compute. reflexivity. Qed.

Example receive_wrong_wire_boolean_remains_underived :
  exact_bool_or_absent 3
    [SemanticSummary 3 LengthDelimited Scalar 0 [1] false None
      ScoreNotDerived false false] = None.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions generated_receive_parent_scalar_rules_are_exact.
Print Assumptions generated_match_parent_scalar_rule_is_exact.
Print Assumptions absent_parent_scalars_have_default_values.
Print Assumptions complete_receive_score_retains_checked_source_values.
Print Assumptions complete_match_score_retains_checked_source_values.
Print Assumptions complete_receive_default_condition_has_no_claimed_score.
Print Assumptions complete_match_default_guard_has_no_claimed_score.
Print Assumptions complete_receive_missing_persistent_evidence_remains_underived.
Print Assumptions complete_match_missing_target_evidence_remains_underived.
Print Assumptions incomplete_receive_and_match_keep_nonsemantic_scores.
Print Assumptions receive_true_boolean_uses_checked_source.
Print Assumptions receive_default_boolean_remains_underived.
Print Assumptions receive_wrong_wire_boolean_remains_underived.

Theorem cost_ground_score_uses_source_bytes :
  forall child complete,
    sem_tag child = 1 ->
    sem_payload child = Scalar ->
    score_evidence_for 10 complete [child] =
      ExactScore (score_constructor 502 [ScoreBytes (sem_body child)]).
Proof.
  intros child complete tag payload.
  unfold score_evidence_for, exact_or_unknown, exact_cost_atom_score.
  now rewrite tag, payload.
Qed.

Theorem cost_quote_score_uses_source_bytes :
  forall child complete,
    sem_tag child = 2 ->
    sem_payload child = Scalar ->
    score_evidence_for 10 complete [child] =
      ExactScore (score_constructor 503 [ScoreBytes (sem_body child)]).
Proof.
  intros child complete tag payload.
  unfold score_evidence_for, exact_or_unknown, exact_cost_atom_score.
  now rewrite tag, payload.
Qed.

Theorem cost_bound_atom_uses_child_score :
  forall child complete score,
    sem_tag child = 3 ->
    sem_payload child = Message ->
    sem_score_evidence child = ExactScore score ->
    score_evidence_for 10 complete [child] = ExactScore score.
Proof.
  intros child complete score tag payload exact.
  unfold score_evidence_for, exact_or_unknown, exact_cost_atom_score.
  now rewrite tag, payload, exact.
Qed.

Theorem cost_runtime_name_score_uses_source_bytes :
  forall child complete,
    sem_tag child = 4 ->
    sem_payload child = Scalar ->
    score_evidence_for 10 complete [child] =
      ExactScore (score_constructor 507 [ScoreBytes (sem_body child)]).
Proof.
  intros child complete tag payload.
  unfold score_evidence_for, exact_or_unknown, exact_cost_atom_score.
  now rewrite tag, payload.
Qed.

Theorem cost_absent_form_has_exact_score :
  forall complete,
    score_evidence_for 10 complete [] =
      ExactScore (score_constructor 505 []).
Proof. intros []; reflexivity. Qed.

Theorem cost_bound_signature_retains_signed_index :
  forall children complete index,
    exact_i32_or_absent 1 children = Some index ->
    score_evidence_for 11 complete children =
      ExactScore (score_constructor 504
        [ScoreInteger (signed_score_word index)]).
Proof.
  intros children complete index decoded.
  cbn [score_evidence_for].
  unfold exact_or_unknown, exact_cost_bound_score.
  now rewrite decoded.
Qed.

Theorem cost_signature_retains_all_atom_scores :
  forall children complete atoms,
    exact_selected_scores 1 children = Some atoms ->
    score_evidence_for 9 complete children =
      ExactScore (score_constructor 501 atoms).
Proof.
  intros children complete atoms collected.
  cbn [score_evidence_for].
  unfold exact_or_unknown, exact_cost_collection_score.
  now rewrite collected.
Qed.

Theorem cost_token_stack_retains_all_layer_scores :
  forall children layers,
    exact_selected_scores 1 children = Some layers ->
    score_evidence_for 13 (Some true) children =
      ExactScore (score_constructor 508 layers).
Proof.
  intros children layers collected.
  cbn [score_evidence_for].
  unfold exact_or_unknown, exact_cost_collection_score.
  now rewrite collected.
Qed.

Theorem cost_region_retains_scalar_coordinates_signer_and_ordinal :
  forall children complete version start_line start_column
      end_line end_column signer ordinal,
    exact_u32_or_absent 1 children = Some version ->
    exact_u64_or_absent 2 children = Some start_line ->
    exact_u64_or_absent 3 children = Some start_column ->
    exact_u64_or_absent 4 children = Some end_line ->
    exact_u64_or_absent 5 children = Some end_column ->
    exact_optional_child_score 6 children = Some signer ->
    exact_u32_or_absent 7 children = Some ordinal ->
    score_evidence_for 12 complete children =
      ExactScore (score_constructor 500
        [ScoreInteger version;
         ScoreBytes (score_u64_be_bytes start_line);
         ScoreBytes (score_u64_be_bytes start_column);
         ScoreBytes (score_u64_be_bytes end_line);
         ScoreBytes (score_u64_be_bytes end_column);
         match signer with
         | Some score => score
         | None => score_constructor 506 []
         end;
         ScoreInteger ordinal]).
Proof.
  intros children complete version start_line start_column
    end_line end_column signer ordinal
    version_seen start_line_seen start_column_seen
    end_line_seen end_column_seen signer_seen ordinal_seen.
  cbn [score_evidence_for].
  unfold exact_or_unknown, exact_cost_region_score.
  now rewrite version_seen, start_line_seen, start_column_seen,
    end_line_seen, end_column_seen, signer_seen, ordinal_seen.
Qed.

Theorem complete_send_score_retains_channel_data_flags_and_optional_cost :
  forall children persistent channel data connective cost,
    scalar_true_or_absent 3 children = Some persistent ->
    exact_child_score 1 children = Some channel ->
    exact_selected_scores 2 children = Some data ->
    scalar_true_or_absent 6 children = Some connective ->
    exact_optional_child_score 7 children = Some cost ->
    score_evidence_for 15 (Some true) children =
      ExactScore (score_constructor 300
        (ScoreInteger (if persistent then 1 else 0) :: channel ::
          (data ++ [ScoreInteger (if connective then 1 else 0)] ++
            match cost with None => [] | Some score => [score] end))).
Proof.
  intros children persistent channel data connective cost
    persistent_seen channel_seen data_seen connective_seen cost_seen.
  assert (send_score : exact_send_score children =
    Some (score_constructor 300
      (ScoreInteger (if persistent then 1 else 0) :: channel ::
        (data ++ [ScoreInteger (if connective then 1 else 0)] ++
          match cost with None => [] | Some score => [score] end)))).
  { unfold exact_send_score.
    now rewrite persistent_seen, channel_seen, data_seen,
      connective_seen, cost_seen. }
  change (exact_or_unknown (exact_send_score children) =
    ExactScore (score_constructor 300
      (ScoreInteger (if persistent then 1 else 0) :: channel ::
        (data ++ [ScoreInteger (if connective then 1 else 0)] ++
          match cost with None => [] | Some score => [score] end)))).
  now rewrite send_score.
Qed.

Theorem complete_if_score_retains_all_branches_and_connective_flag :
  forall children condition if_true if_false connective,
    exact_child_score 1 children = Some condition ->
    exact_child_score 2 children = Some if_true ->
    exact_child_score 3 children = Some if_false ->
    scalar_true_or_absent 5 children = Some connective ->
    score_evidence_for 23 (Some true) children =
      ExactScore (score_constructor 309
        [condition; if_true; if_false;
         ScoreInteger (if connective then 1 else 0)]).
Proof.
  intros children condition if_true if_false connective
    condition_seen true_seen false_seen connective_seen.
  assert (if_score : exact_if_score children =
    Some (score_constructor 309
      [condition; if_true; if_false;
       ScoreInteger (if connective then 1 else 0)])).
  { unfold exact_if_score.
    now rewrite condition_seen, true_seen, false_seen, connective_seen. }
  change (exact_or_unknown (exact_if_score children) =
    ExactScore (score_constructor 309
      [condition; if_true; if_false;
       ScoreInteger (if connective then 1 else 0)])).
  now rewrite if_score.
Qed.

Definition score_integer_bytes (bits : N) : list nat :=
  [1; 1;
   N.to_nat (N.lxor (N.land (N.shiftr bits 56) 255) 128);
   N.to_nat (N.land (N.shiftr bits 48) 255);
   N.to_nat (N.land (N.shiftr bits 40) 255);
   N.to_nat (N.land (N.shiftr bits 32) 255);
   N.to_nat (N.land (N.shiftr bits 24) 255);
   N.to_nat (N.land (N.shiftr bits 16) 255);
   N.to_nat (N.land (N.shiftr bits 8) 255);
   N.to_nat (N.land bits 255)].

Fixpoint score_terminated_bytes (bytes : list nat) : list nat :=
  match bytes with
  | [] => [0; 0]
  | byte :: rest =>
      if byte =? 0 then 0 :: 255 :: score_terminated_bytes rest
      else byte :: score_terminated_bytes rest
  end.

Fixpoint canonical_score_bytes (score : canonical_score) : list nat :=
  match score with
  | ScoreInteger value => score_integer_bytes value
  | ScoreString bytes => 1 :: 2 :: score_terminated_bytes bytes
  | ScoreBytes bytes => 1 :: 3 :: score_terminated_bytes bytes
  | ScoreNode children => 2 :: canonical_score_forest_bytes children
  end
with canonical_score_forest_bytes (scores : canonical_score_forest)
    : list nat :=
  match scores with
  | ScoreNil => [0]
  | ScoreCons score rest =>
      canonical_score_bytes score ++ canonical_score_forest_bytes rest
  end.

Fixpoint body_leq (left right : list nat) : bool :=
  match left, right with
  | [], _ => true
  | _ :: _, [] => false
  | a :: left', b :: right' =>
      if a <? b then true else if b <? a then false
      else body_leq left' right'
  end.

Lemma body_leq_refl : forall bytes, body_leq bytes bytes = true.
Proof.
  induction bytes as [|first rest IH]; simpl; [reflexivity|].
  now rewrite Nat.ltb_irrefl.
Qed.

Fixpoint body_sequence_ordered (bodies : list (list nat)) : bool :=
  match bodies with
  | [] | [_] => true
  | first :: ((second :: _) as rest) =>
      body_leq first second && body_sequence_ordered rest
  end.

Definition new_uri_ordered (children : list semantic_summary) : bool :=
  body_sequence_ordered
    (map sem_body (filter (fun child => sem_tag child =? 3) children)).

Theorem score_terminated_bytes_has_exact_escape_length : forall bytes,
  length (score_terminated_bytes bytes) =
    length bytes + length (filter (Nat.eqb 0) bytes) + 2.
Proof.
  induction bytes as [|byte rest IH]; [reflexivity|].
  cbn [score_terminated_bytes].
  destruct (byte =? 0) eqn:zero.
  - apply Nat.eqb_eq in zero. subst byte.
    cbn [length filter]. rewrite Nat.eqb_refl.
    cbn [length]. rewrite IH. lia.
  - assert ((0 =? byte) = false) as reverse_zero.
    { now rewrite Nat.eqb_sym. }
    cbn [length filter]. rewrite reverse_zero.
    cbn [length]. rewrite IH. lia.
Qed.

Theorem pathmap_snapshot_score_roundtrip : forall snapshot flag,
  pathmap_snapshot_from_score
    (score_constructor 13 [ScoreBytes snapshot; ScoreInteger flag]) =
      Some snapshot.
Proof. intros. reflexivity. Qed.

Theorem zipper_score_uses_its_own_connective_flag :
  forall children snapshot nested_flag own_flag,
    exact_child_score 1 children =
      Some (score_constructor 13
        [ScoreBytes snapshot; ScoreInteger nested_flag]) ->
    scalar_true_or_absent 5 children = Some own_flag ->
    exact_ezipper_score children =
      Some (score_constructor 14
        [ScoreBytes snapshot; ScoreInteger (if own_flag then 1 else 0)]).
Proof.
  intros children snapshot nested_flag own_flag pathmap connective.
  unfold exact_ezipper_score.
  rewrite pathmap, connective, pathmap_snapshot_score_roundtrip.
  reflexivity.
Qed.

Theorem new_score_retains_all_uris_injections_and_body :
  forall children bind_count body injections,
    new_bind_count_word children = Some bind_count ->
    exact_child_score 2 children = Some body ->
    exact_selected_scores 4 children = Some injections ->
    exact_new_score children =
      Some (score_constructor 303
        (ScoreInteger bind_count ::
          (scores_or_absent
            (map (fun child => ScoreString (sem_body child))
              (filter (fun child => sem_tag child =? 3) children)) ++
           scores_or_absent injections ++ [body]))).
Proof.
  intros children bind_count body injections count body_exact entries.
  unfold exact_new_score. now rewrite count, body_exact, entries.
Qed.

Example negative_integer_score_preserves_signed_order :
  score_integer_bytes (signed_score_word (-1)) =
    [1; 1; 127; 255; 255; 255; 255; 255; 255; 255] /\
  score_integer_bytes (signed_score_word (-9223372036854775808)) =
    [1; 1; 0; 0; 0; 0; 0; 0; 0; 0].
Proof. vm_compute. split; reflexivity. Qed.

Example string_and_bytes_scores_escape_zero_with_distinct_kinds :
  canonical_score_bytes (ScoreString [97; 0; 98]) =
    [1; 2; 97; 0; 255; 98; 0; 0] /\
  canonical_score_bytes (ScoreBytes [97; 0; 98]) =
    [1; 3; 97; 0; 255; 98; 0; 0].
Proof. vm_compute. split; reflexivity. Qed.

Theorem complete_bundle_has_exact_constructor_and_body_score :
  forall children body body_score read write,
    find (fun child => sem_tag child =? 1) children = Some body ->
    sem_score_evidence body = ExactScore body_score ->
    scalar_true_or_absent 2 children = Some write ->
    scalar_true_or_absent 3 children = Some read ->
    score_evidence_for 8 (Some true) children =
      ExactScore (score_constructor (bundle_constructor read write)
        [body_score]).
Proof.
  intros children body body_score read write found exact write_flag read_flag.
  unfold score_evidence_for, exact_bundle_score.
  rewrite found, write_flag, read_flag.
  now rewrite exact.
Qed.

Definition score_body_order (left right : semantic_summary) : option bool :=
  match sem_score_evidence left, sem_score_evidence right with
  | ExactScore left_score, ExactScore right_score =>
      let left_bytes := canonical_score_bytes left_score in
      let right_bytes := canonical_score_bytes right_score in
      if body_leq left_bytes right_bytes && body_leq right_bytes left_bytes
      then Some (body_leq (sem_body left) (sem_body right))
      else Some (body_leq left_bytes right_bytes)
  | _, _ => None
  end.

Theorem equal_exact_scores_use_raw_body_order :
  forall left right score,
    sem_score_evidence left = ExactScore score ->
    sem_score_evidence right = ExactScore score ->
    score_body_order left right =
      Some (body_leq (sem_body left) (sem_body right)).
Proof.
  intros left right score left_exact right_exact.
  unfold score_body_order.
  rewrite left_exact, right_exact.
  now rewrite body_leq_refl.
Qed.

Lemma score_integer_bytes_306 :
  score_integer_bytes 306 = [1; 1; 128; 0; 0; 0; 0; 0; 1; 50].
Proof. vm_compute. reflexivity. Qed.

Lemma score_integer_bytes_307 :
  score_integer_bytes 307 = [1; 1; 128; 0; 0; 0; 0; 0; 1; 51].
Proof. vm_compute. reflexivity. Qed.

Theorem read_bundle_score_precedes_write_bundle_score :
  forall left_body right_body,
    body_leq
      (canonical_score_bytes (score_constructor 306 [left_body]))
      (canonical_score_bytes (score_constructor 307 [right_body])) = true /\
    body_leq
      (canonical_score_bytes (score_constructor 307 [right_body]))
      (canonical_score_bytes (score_constructor 306 [left_body])) = false.
Proof.
  intros left_body right_body.
  unfold score_constructor.
  cbn [canonical_score_bytes canonical_score_forest_bytes score_forest_of_list].
  rewrite score_integer_bytes_306, score_integer_bytes_307.
  split; cbn [body_leq]; reflexivity.
Qed.

Theorem read_bundle_precedes_write_bundle_for_any_body_scores :
  forall left right left_body right_body,
    sem_score_evidence left =
      ExactScore (score_constructor 306 [left_body]) ->
    sem_score_evidence right =
      ExactScore (score_constructor 307 [right_body]) ->
    score_body_order left right = Some true.
Proof.
  intros left right left_body right_body left_exact right_exact.
  unfold score_body_order.
  rewrite left_exact, right_exact.
  destruct (read_bundle_score_precedes_write_bundle_score
    left_body right_body) as [forward backward].
  now rewrite forward, backward.
Qed.

Definition and_known (left right : option bool) : option bool :=
  match left, right with
  | Some false, _ | _, Some false => Some false
  | Some true, Some true => Some true
  | _, _ => None
  end.

Fixpoint raw_par_component_order (children : list semantic_summary)
    : option bool :=
  match children with
  | [] | [_] => Some true
  | first :: ((second :: _) as rest) =>
      and_known
        (if sem_tag first =? sem_tag second
         then score_body_order first second else Some true)
        (raw_par_component_order rest)
  end.

Definition order_definitely_bad (order : option bool) : bool :=
  match order with Some false => true | _ => false end.

Definition reduce_semantic_field (field : raw_field)
    (children : list semantic_summary) : semantic_summary :=
  let resolved := generated_wire_program field in
  let program := match resolved with Some program => program | None => empty_program end in
  let scanned := fold_left (scan_child program) children empty_scan in
  let scalar_has_no_children :=
    match raw_payload field, children with
    | Scalar, _ :: _ => false
    | _, _ => true
    end in
  let complete := component_completeness (program_family program) scanned children in
  let order :=
    if program_family program =? 1
    then raw_par_component_order children
    else if (program_family program =? 20) && seen_slot scanned 1
    then Some (new_uri_ordered children)
    else Some true in
  let refused :=
    negb (match resolved with Some _ => true | None => false end) ||
    negb (valid_raw_body field) ||
    negb scalar_has_no_children ||
    negb (scan_valid scanned) ||
    negb (mandatory_pathmaps_present program scanned) ||
    order_definitely_bad order ||
    complete_receive_has_default_condition (program_family program)
      complete children ||
    complete_match_has_default_guard (program_family program)
      complete children in
  SemanticSummary (raw_tag field) (raw_wire field) (raw_payload field)
    (program_family program) (raw_body field) refused complete
    (score_evidence_for (program_family program) complete children)
    (if program_family program =? 16 then seen_slot scanned 1 else false)
    (if program_family program =? 21
     then seen_slot scanned 0 && seen_slot scanned 1 else false).

Theorem complete_receive_default_condition_is_refused :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 19 ->
    component_completeness 19
      (fold_left (scan_child program) children empty_scan) children =
      Some true ->
    receive_has_present_default_condition children = true ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children program resolved family complete default.
  assert (guard :
    complete_receive_has_default_condition (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) children empty_scan) children)
      children = true).
  { rewrite family, complete.
    cbn [complete_receive_has_default_condition].
    exact default. }
  unfold reduce_semantic_field.
  rewrite resolved.
  cbn [sem_definitely_refused].
  rewrite guard.
  rewrite orb_true_r.
  reflexivity.
Qed.

Theorem complete_match_default_guard_is_refused :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 22 ->
    component_completeness 22
      (fold_left (scan_child program) children empty_scan) children =
      Some true ->
    existsb match_case_has_present_default_guard children = true ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children program resolved family complete default.
  assert (guard :
    complete_match_has_default_guard (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) children empty_scan) children)
      children = true).
  { rewrite family, complete.
    cbn [complete_match_has_default_guard].
    exact default. }
  unfold reduce_semantic_field.
  rewrite resolved.
  cbn [sem_definitely_refused].
  rewrite guard.
  rewrite orb_true_r.
  reflexivity.
Qed.

Print Assumptions complete_receive_default_condition_is_refused.
Print Assumptions complete_match_default_guard_is_refused.

Fixpoint semantic_reduce (subject : @tree raw_field) : semantic_summary :=
  match subject with
  | Node field children => reduce_semantic_field field (semantic_reduce_forest children)
  end
with semantic_reduce_forest (subjects : @forest raw_field)
    : list semantic_summary :=
  match subjects with
  | FNil => []
  | FCons subject rest => semantic_reduce subject :: semantic_reduce_forest rest
  end.

Definition connective_not_empty_par_source_witness : @tree raw_field :=
  Node (RawField 8 LengthDelimited Message [26; 0] 2 (Some 55) [] [])
    (FCons (Node (RawField 3 LengthDelimited Message [] 0
      (Some 1) [] []) FNil) FNil).

Definition connective_not_wrong_wire_source_mutant : @tree raw_field :=
  Node (RawField 8 LengthDelimited Message [24; 0] 2 (Some 55) [] [])
    (FCons (Node (RawField 3 Varint Scalar [0] 1 None [] []) FNil)
      FNil).

Definition connective_not_wrong_type_source_mutant : @tree raw_field :=
  Node (RawField 8 LengthDelimited Message [26; 0] 2 (Some 55) [] [])
    (FCons (Node (RawField 3 LengthDelimited Message [] 0
      (Some 57) [] []) FNil) FNil).

Example connective_not_empty_par_source_derives_exact_score :
  RPB3RawRegionTree.decode_root 55 [26; 0] <> None /\
  sem_definitely_refused
    (semantic_reduce connective_not_empty_par_source_witness) = false /\
  sem_score_evidence
    (semantic_reduce connective_not_empty_par_source_witness) =
      ExactScore (score_constructor 400 [empty_par_score]).
Proof. vm_compute. repeat split; try discriminate; reflexivity. Qed.

Example connective_not_source_mutants_are_refused_or_underived :
  RPB3RawRegionTree.decode_root 55 [24; 0] = None /\
  RPB3RawRegionTree.decode_root 55 [26; 1; 0] = None /\
  sem_definitely_refused
    (semantic_reduce connective_not_wrong_wire_source_mutant) = true /\
  sem_score_evidence
    (semantic_reduce connective_not_wrong_wire_source_mutant) =
      ScoreNotDerived /\
  sem_definitely_refused
    (semantic_reduce connective_not_wrong_type_source_mutant) = true /\
  sem_score_evidence
    (semantic_reduce connective_not_wrong_type_source_mutant) =
      ScoreNotDerived.
Proof. vm_compute. repeat split; reflexivity. Qed.

Print Assumptions connective_not_empty_par_source_derives_exact_score.
Print Assumptions connective_not_source_mutants_are_refused_or_underived.

Definition default_guard_match_case_summary : semantic_summary :=
  SemanticSummary 2 LengthDelimited Message 21
    [10; 0; 18; 0; 34; 0] false None ScoreNotDerived false true.

Definition nondefault_guard_match_case_summary : semantic_summary :=
  SemanticSummary 2 LengthDelimited Message 21
    [10; 0; 18; 0; 34; 2; 80; 1] false None
    ScoreNotDerived false true.

Definition locally_free_default_guard_match_case_summary :
    semantic_summary :=
  SemanticSummary 2 LengthDelimited Message 21
    [10; 0; 18; 0; 34; 3; 74; 1; 1] false None
    ScoreNotDerived false true.

Definition match_guard_witness_field : raw_field :=
  RawField 1 LengthDelimited Message [] 0 (Some 22) [] [].

Example source_default_match_case_guard_is_detected :
  match_case_has_present_default_guard default_guard_match_case_summary =
    true.
Proof. vm_compute. reflexivity. Qed.

Example locally_free_does_not_change_default_guard_refusal :
  match_case_has_present_default_guard
    locally_free_default_guard_match_case_summary = true.
Proof. vm_compute. reflexivity. Qed.

Example complete_match_refuses_present_default_case_guard :
  sem_definitely_refused (reduce_semantic_field
    match_guard_witness_field
    [auxiliary_empty_par_child 1; default_guard_match_case_summary]) =
    true.
Proof. vm_compute. reflexivity. Qed.

Example incomplete_match_does_not_refuse_case_guard :
  sem_definitely_refused (reduce_semantic_field
    match_guard_witness_field
    [default_guard_match_case_summary]) = false.
Proof. vm_compute. reflexivity. Qed.

Example complete_match_accepts_nondefault_case_guard :
  sem_definitely_refused (reduce_semantic_field
    match_guard_witness_field
    [auxiliary_empty_par_child 1; nondefault_guard_match_case_summary]) =
    false.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions source_default_match_case_guard_is_detected.
Print Assumptions locally_free_does_not_change_default_guard_refusal.
Print Assumptions complete_match_refuses_present_default_case_guard.
Print Assumptions incomplete_match_does_not_refuse_case_guard.
Print Assumptions complete_match_accepts_nondefault_case_guard.

Definition receive_parent_witness_field : raw_field :=
  RawField 1 LengthDelimited Message [] 0 (Some 19) [] [].

Definition default_receive_condition_summary : semantic_summary :=
  SemanticSummary 8 LengthDelimited Message 1 [] false None
    (ExactScore empty_par_score) false false.

Definition nondefault_receive_condition_summary : semantic_summary :=
  SemanticSummary 8 LengthDelimited Message 1 [80; 1] false None
    (ExactScore (score_constructor 999 [ScoreInteger 1])) false false.

Definition scored_match_case_without_guard_summary : semantic_summary :=
  SemanticSummary 2 LengthDelimited Message 21 [10; 0; 18; 0]
    false None
    (ExactScore (ScoreNode (score_forest_of_list
      [empty_par_score; empty_par_score; ScoreInteger 0;
       empty_par_score]))) false true.

Definition scored_match_case_with_default_guard_summary : semantic_summary :=
  SemanticSummary 2 LengthDelimited Message 21 [10; 0; 18; 0; 34; 0]
    false None
    (ExactScore (ScoreNode (score_forest_of_list
      [empty_par_score; empty_par_score; ScoreInteger 0;
       empty_par_score]))) false true.

Example complete_receive_without_optional_fields_has_exact_score :
  score_evidence_for 19 (Some true) [auxiliary_empty_par_child 2] =
    ExactScore (score_constructor 301
      [ScoreInteger 0; ScoreInteger 0; empty_par_score;
       ScoreInteger 0; ScoreInteger 0; empty_par_score]).
Proof. vm_compute. reflexivity. Qed.

Example complete_match_without_connective_has_exact_score :
  score_evidence_for 22 (Some true)
    [auxiliary_empty_par_child 1;
     scored_match_case_without_guard_summary] =
    ExactScore (score_constructor 304
      [empty_par_score;
       ScoreNode (score_forest_of_list
         [empty_par_score; empty_par_score; ScoreInteger 0;
          empty_par_score]); ScoreInteger 0]).
Proof. vm_compute. reflexivity. Qed.

Example complete_receive_refuses_present_default_condition :
  sem_definitely_refused (reduce_semantic_field
    receive_parent_witness_field
    [auxiliary_empty_par_child 2; default_receive_condition_summary]) = true.
Proof. vm_compute. reflexivity. Qed.

Example incomplete_receive_does_not_refuse_default_condition :
  sem_definitely_refused (reduce_semantic_field
    receive_parent_witness_field [default_receive_condition_summary]) = false.
Proof. vm_compute. reflexivity. Qed.

Example complete_receive_accepts_nondefault_condition :
  sem_definitely_refused (reduce_semantic_field
    receive_parent_witness_field
    [auxiliary_empty_par_child 2; nondefault_receive_condition_summary]) =
    false.
Proof. vm_compute. reflexivity. Qed.

Example complete_receive_default_condition_blocks_claimed_score :
  score_evidence_for 19 (Some true)
    [auxiliary_empty_par_child 2; default_receive_condition_summary] =
    ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example complete_match_default_guard_blocks_claimed_score :
  score_evidence_for 22 (Some true)
    [auxiliary_empty_par_child 1;
     scored_match_case_with_default_guard_summary] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example malformed_receive_bind_count_blocks_claimed_score :
  score_evidence_for 19 (Some true)
    [auxiliary_empty_par_child 2;
     SemanticSummary 5 Varint Scalar 0 [128; 0] false None
       ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Example malformed_match_connective_blocks_claimed_score :
  score_evidence_for 22 (Some true)
    [auxiliary_empty_par_child 1;
     SemanticSummary 5 Varint Scalar 0 [128; 0] false None
       ScoreNotDerived false false] = ScoreNotDerived.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions complete_receive_without_optional_fields_has_exact_score.
Print Assumptions complete_match_without_connective_has_exact_score.
Print Assumptions complete_receive_refuses_present_default_condition.
Print Assumptions incomplete_receive_does_not_refuse_default_condition.
Print Assumptions complete_receive_accepts_nondefault_condition.
Print Assumptions complete_receive_default_condition_blocks_claimed_score.
Print Assumptions complete_match_default_guard_blocks_claimed_score.
Print Assumptions malformed_receive_bind_count_blocks_claimed_score.
Print Assumptions malformed_match_connective_blocks_claimed_score.

(** Every witness carries deliberately false caller annotations [999]. Their
    irrelevance makes the removed requirement-list authority executable. *)
Definition witness_message (tag type_id : nat) (body : list nat) : raw_field :=
  RawField tag LengthDelimited Message body (length body)
    (Some type_id) [999] [999].

Definition cost_ground_atom_witness : @tree raw_field :=
  Node (witness_message 1 10 [10; 1; 65])
    (FCons (Node (RawField 1 LengthDelimited Scalar [65] 1
      None [] []) FNil) FNil).

Definition cost_quote_atom_witness : @tree raw_field :=
  Node (witness_message 1 10 [18; 1; 66])
    (FCons (Node (RawField 2 LengthDelimited Scalar [66] 1
      None [] []) FNil) FNil).

Definition cost_runtime_name_atom_witness : @tree raw_field :=
  Node (witness_message 1 10 [34; 1; 67])
    (FCons (Node (RawField 4 LengthDelimited Scalar [67] 1
      None [] []) FNil) FNil).

Definition cost_bound_signature_witness : @tree raw_field :=
  Node (witness_message 3 11 [8; 1])
    (FCons (Node (RawField 1 Varint Scalar [1] 1
      None [] []) FNil) FNil).

Definition cost_bound_atom_witness : @tree raw_field :=
  Node (witness_message 1 10 [26; 2; 8; 1])
    (FCons cost_bound_signature_witness FNil).

Definition cost_signature_witness : @tree raw_field :=
  Node (witness_message 1 9 [10; 3; 10; 1; 65])
    (FCons cost_ground_atom_witness FNil).

Definition cost_token_stack_witness : @tree raw_field :=
  Node (witness_message 0 13 [10; 5; 10; 3; 10; 1; 65])
    (FCons cost_signature_witness FNil).

Definition cost_region_witness : @tree raw_field :=
  Node (witness_message 0 12 [8; 1; 16; 2; 50; 0; 56; 3])
    (FCons (Node (RawField 1 Varint Scalar [1] 1 None [] []) FNil)
      (FCons (Node (RawField 2 Varint Scalar [2] 1 None [] []) FNil)
        (FCons (Node (witness_message 6 9 []) FNil)
          (FCons (Node (RawField 7 Varint Scalar [3] 1
            None [] []) FNil) FNil)))).

Example all_cost_atom_variants_have_distinct_exact_source_scores :
  sem_definitely_refused (semantic_reduce cost_ground_atom_witness) = false /\
  sem_score_evidence (semantic_reduce cost_ground_atom_witness) =
    ExactScore (score_constructor 502 [ScoreBytes [65]]) /\
  sem_definitely_refused (semantic_reduce cost_quote_atom_witness) = false /\
  sem_score_evidence (semantic_reduce cost_quote_atom_witness) =
    ExactScore (score_constructor 503 [ScoreBytes [66]]) /\
  sem_definitely_refused (semantic_reduce cost_runtime_name_atom_witness) = false /\
  sem_score_evidence (semantic_reduce cost_runtime_name_atom_witness) =
    ExactScore (score_constructor 507 [ScoreBytes [67]]) /\
  sem_definitely_refused (semantic_reduce cost_bound_atom_witness) = false /\
  sem_score_evidence (semantic_reduce cost_bound_atom_witness) =
    ExactScore (score_constructor 504
      [ScoreInteger (signed_score_word (-1))]).
Proof. vm_compute. repeat split; reflexivity. Qed.

Example nested_cost_stack_retains_signature_and_atom_scores :
  sem_definitely_refused (semantic_reduce cost_token_stack_witness) = false /\
  sem_score_evidence (semantic_reduce cost_token_stack_witness) =
    ExactScore (score_constructor 508
      [score_constructor 501
        [score_constructor 502 [ScoreBytes [65]]]]).
Proof. vm_compute. split; reflexivity. Qed.

Example source_cost_region_retains_signer_and_big_endian_coordinate :
  sem_definitely_refused (semantic_reduce cost_region_witness) = false /\
  sem_score_evidence (semantic_reduce cost_region_witness) =
    ExactScore (score_constructor 500
      [ScoreInteger 1;
       ScoreBytes (score_u64_be_bytes 2);
       ScoreBytes (score_u64_be_bytes 0);
       ScoreBytes (score_u64_be_bytes 0);
       ScoreBytes (score_u64_be_bytes 0);
       score_constructor 501 [];
       ScoreInteger 3]).
Proof. vm_compute. split; reflexivity. Qed.

Definition unknown_cost_atom_summary : semantic_summary :=
  SemanticSummary 1 LengthDelimited Message 10 [] false None
    ScoreNotDerived false false.

Definition unknown_cost_bound_summary : semantic_summary :=
  SemanticSummary 3 LengthDelimited Message 11 [8; 1] false None
    ScoreNotDerived false false.

Definition unknown_cost_signer_summary : semantic_summary :=
  SemanticSummary 6 LengthDelimited Message 9 [] false None
    ScoreNotDerived false false.

Example missing_cost_child_evidence_does_not_become_an_exact_score :
  score_evidence_for 9 None [unknown_cost_atom_summary] = ScoreNotDerived /\
  score_evidence_for 13 (Some true) [unknown_cost_atom_summary] =
    ScoreNotDerived /\
  score_evidence_for 10 None [unknown_cost_bound_summary] =
    ScoreNotDerived /\
  score_evidence_for 12 None [unknown_cost_signer_summary] =
    ScoreNotDerived.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example explicit_default_bound_index_cannot_supply_exact_score :
  exact_i32_or_absent 1
    [SemanticSummary 1 Varint Scalar 0 [0] false None
      ScoreNotDerived false false] = None.
Proof. vm_compute. reflexivity. Qed.

Example omitting_a_cost_layer_changes_the_stack_score :
  score_evidence_for 13 (Some true)
    [SemanticSummary 1 LengthDelimited Message 9 [] false None
      (ExactScore (score_constructor 501 [])) false false] <>
    ExactScore (score_constructor 508 []).
Proof. vm_compute. discriminate. Qed.

Definition witness_bool_true : @tree raw_field :=
  Node (RawField 3 Varint Scalar [1] 1 None [999] [999]) FNil.

Definition first_incomplete_send : @tree raw_field :=
  Node (witness_message 1 15 [24; 1])
    (FCons witness_bool_true FNil).

Definition empty_incomplete_send : @tree raw_field :=
  Node (witness_message 1 15 []) FNil.

(** Actual raw Par bytes: 0a 02 18 01 0a 00. Both Send scores are the same
    nonsemantic constructor, so the first body [18 01] must not precede []. *)
Definition reversed_incomplete_sends : @tree raw_field :=
  Node (witness_message 0 1 [10; 2; 24; 1; 10; 0])
    (FCons first_incomplete_send
      (FCons empty_incomplete_send FNil)).

(** Actual outer Send body: 0a 02 0a 00. Its channel Par contains an
    incomplete Send. The outer Send itself is complete because chan exists. *)
Definition outer_send_with_incomplete_channel : @tree raw_field :=
  Node (witness_message 0 15 [10; 2; 10; 0])
    (FCons (Node (witness_message 1 1 [10; 0])
      (FCons empty_incomplete_send FNil)) FNil).

Definition empty_epathmap : @tree raw_field :=
  Node (witness_message 0 31 []) FNil.

Definition empty_bundle_body : @tree raw_field :=
  Node (witness_message 1 1 []) FNil.

Definition bundle_write_flag : @tree raw_field :=
  Node (RawField 2 Varint Scalar [1] 1 None [999] [999]) FNil.

Definition bundle_read_flag : @tree raw_field :=
  Node (RawField 3 Varint Scalar [1] 1 None [999] [999]) FNil.

Definition read_only_bundle : @tree raw_field :=
  Node (witness_message 11 8 [10; 0; 24; 1])
    (FCons empty_bundle_body (FCons bundle_read_flag FNil)).

Definition write_only_bundle : @tree raw_field :=
  Node (witness_message 11 8 [10; 0; 16; 1])
    (FCons empty_bundle_body (FCons bundle_write_flag FNil)).

Definition reversed_bundle_scores : @tree raw_field :=
  Node (witness_message 0 1 [90; 4; 10; 0; 16; 1;
                             90; 4; 10; 0; 24; 1])
    (FCons write_only_bundle (FCons read_only_bundle FNil)).

Definition read_bundle_with_send_body : @tree raw_field :=
  Node (witness_message 11 8 [10; 2; 10; 0; 24; 1])
    (FCons (Node (witness_message 1 1 [10; 0])
      (FCons empty_incomplete_send FNil))
      (FCons bundle_read_flag FNil)).

Definition read_bundle_with_bundle_body : @tree raw_field :=
  Node (witness_message 11 8 [10; 4; 90; 2; 10; 0; 24; 1])
    (FCons (Node (witness_message 1 1 [90; 2; 10; 0])
      (FCons (Node (witness_message 11 8 [10; 0])
        (FCons empty_bundle_body FNil)) FNil))
      (FCons bundle_read_flag FNil)).

Example reversed_incomplete_sends_are_definitely_refused :
  sem_definitely_refused (semantic_reduce reversed_incomplete_sends) = true /\
  raw_par_component_order
    (semantic_reduce_forest
      (FCons first_incomplete_send (FCons empty_incomplete_send FNil))) =
    Some false.
Proof. vm_compute. split; reflexivity. Qed.

Example outer_send_completeness_ignores_nested_incomplete_component :
  sem_complete (semantic_reduce outer_send_with_incomplete_channel) = Some true /\
  sem_definitely_refused (semantic_reduce outer_send_with_incomplete_channel) = false.
Proof. vm_compute. split; reflexivity. Qed.

Example outer_send_has_exact_score_with_nested_incomplete_channel :
  sem_score_evidence (semantic_reduce outer_send_with_incomplete_channel) =
    ExactScore (score_constructor 300
      [ScoreInteger 0;
       score_constructor 999
         [score_constructor 2147483647 []; ScoreInteger 0];
       ScoreInteger 0]).
Proof. vm_compute. reflexivity. Qed.

Example empty_epathmap_is_definitely_refused :
  sem_definitely_refused (semantic_reduce empty_epathmap) = true.
Proof. vm_compute. reflexivity. Qed.

Example complete_bundles_have_distinct_exact_scores :
  sem_complete (semantic_reduce read_only_bundle) = Some true /\
  sem_complete (semantic_reduce write_only_bundle) = Some true /\
  sem_score_evidence (semantic_reduce read_only_bundle) =
    ExactScore (score_constructor 306
      [score_constructor 999 [ScoreInteger 0]]) /\
  sem_score_evidence (semantic_reduce write_only_bundle) =
    ExactScore (score_constructor 307
      [score_constructor 999 [ScoreInteger 0]]).
Proof. vm_compute. repeat split; reflexivity. Qed.

Example reversed_complete_bundles_are_definitely_refused :
  sem_definitely_refused (semantic_reduce reversed_bundle_scores) = true /\
  score_body_order (semantic_reduce write_only_bundle)
    (semantic_reduce read_only_bundle) = Some false.
Proof. vm_compute. split; reflexivity. Qed.

Example equal_constructor_bundles_order_by_nested_par_score :
  body_leq
    (sem_body (semantic_reduce read_bundle_with_send_body))
    (sem_body (semantic_reduce read_bundle_with_bundle_body)) = true /\
  score_body_order
    (semantic_reduce read_bundle_with_send_body)
    (semantic_reduce read_bundle_with_bundle_body) = Some false.
Proof. vm_compute. split; reflexivity. Qed.

Definition reversed_new_uris : @tree raw_field :=
  Node (witness_message 0 20 [18; 0; 26; 1; 98; 26; 1; 97])
    (FCons (Node (witness_message 2 1 []) FNil)
      (FCons (Node (RawField 3 LengthDelimited Scalar [98] 1
        None [] []) FNil)
        (FCons (Node (RawField 3 LengthDelimited Scalar [97] 1
          None [] []) FNil) FNil))).

Definition negative_new_bind_count : @tree raw_field :=
  Node (witness_message 0 20 [8; 1; 18; 0])
    (FCons (Node (RawField 1 Varint Scalar [1] 1 None [] []) FNil)
      (FCons (Node (witness_message 2 1 []) FNil) FNil)).

Definition incomplete_new_reversed_uris : @tree raw_field :=
  Node (witness_message 0 20 [26; 1; 98; 26; 1; 97])
    (FCons (Node (RawField 3 LengthDelimited Scalar [98] 1
      None [] []) FNil)
      (FCons (Node (RawField 3 LengthDelimited Scalar [97] 1
        None [] []) FNil) FNil)).

Example complete_new_refuses_reversed_uri_order :
  sem_complete (semantic_reduce reversed_new_uris) = Some true /\
  sem_definitely_refused (semantic_reduce reversed_new_uris) = true.
Proof. vm_compute. split; reflexivity. Qed.

Example incomplete_new_uri_order_is_not_a_refusal :
  sem_complete (semantic_reduce incomplete_new_reversed_uris) = Some false /\
  sem_definitely_refused (semantic_reduce incomplete_new_reversed_uris) = false.
Proof. vm_compute. split; reflexivity. Qed.

Example complete_new_has_exact_negative_bind_count_and_absent_scores :
  sem_definitely_refused (semantic_reduce negative_new_bind_count) = false /\
  sem_score_evidence (semantic_reduce negative_new_bind_count) =
    ExactScore (score_constructor 303
      [ScoreInteger (signed_score_word (-1)); ScoreInteger 0;
       ScoreInteger 0; score_constructor 999 [ScoreInteger 0]]).
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions reversed_incomplete_sends_are_definitely_refused.
Print Assumptions outer_send_completeness_ignores_nested_incomplete_component.
Print Assumptions outer_send_has_exact_score_with_nested_incomplete_channel.
Print Assumptions empty_epathmap_is_definitely_refused.
Print Assumptions complete_bundle_has_exact_constructor_and_body_score.
Print Assumptions complete_send_score_retains_channel_data_flags_and_optional_cost.
Print Assumptions complete_if_score_retains_all_branches_and_connective_flag.
Print Assumptions exact_par_score_preserves_all_component_scores.
Print Assumptions equal_exact_scores_use_raw_body_order.
Print Assumptions read_bundle_score_precedes_write_bundle_score.
Print Assumptions read_bundle_precedes_write_bundle_for_any_body_scores.
Print Assumptions complete_bundles_have_distinct_exact_scores.
Print Assumptions reversed_complete_bundles_are_definitely_refused.
Print Assumptions equal_constructor_bundles_order_by_nested_par_score.
Print Assumptions score_terminated_bytes_has_exact_escape_length.
Print Assumptions pathmap_snapshot_score_roundtrip.
Print Assumptions zipper_score_uses_its_own_connective_flag.
Print Assumptions new_score_retains_all_uris_injections_and_body.
Print Assumptions negative_integer_score_preserves_signed_order.
Print Assumptions string_and_bytes_scores_escape_zero_with_distinct_kinds.
Print Assumptions complete_new_refuses_reversed_uri_order.
Print Assumptions complete_new_has_exact_negative_bind_count_and_absent_scores.
Print Assumptions incomplete_new_uri_order_is_not_a_refusal.
Print Assumptions cost_ground_score_uses_source_bytes.
Print Assumptions cost_quote_score_uses_source_bytes.
Print Assumptions cost_bound_atom_uses_child_score.
Print Assumptions cost_runtime_name_score_uses_source_bytes.
Print Assumptions cost_absent_form_has_exact_score.
Print Assumptions cost_bound_signature_retains_signed_index.
Print Assumptions cost_signature_retains_all_atom_scores.
Print Assumptions cost_token_stack_retains_all_layer_scores.
Print Assumptions cost_region_retains_scalar_coordinates_signer_and_ordinal.
Print Assumptions all_cost_atom_variants_have_distinct_exact_source_scores.
Print Assumptions nested_cost_stack_retains_signature_and_atom_scores.
Print Assumptions source_cost_region_retains_signer_and_big_endian_coordinate.
Print Assumptions missing_cost_child_evidence_does_not_become_an_exact_score.
Print Assumptions explicit_default_bound_index_cannot_supply_exact_score.
Print Assumptions omitting_a_cost_layer_changes_the_stack_score.
