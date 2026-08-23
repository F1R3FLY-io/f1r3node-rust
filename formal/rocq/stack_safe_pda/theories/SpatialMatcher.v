From Stdlib Require Import List Arith Bool FunctionalExtensionality.
From StackSafePDA Require Import StackSafePDA.

Import ListNotations.
Set Implicit Arguments.

(** A consensus-relevant abstraction of SpatialMatcher's stateful control
    operators.  [free_map] is the Rust [FreeMap]; a matcher value is a state
    transformer that may refuse.  Children are already-compiled matcher
    values, so this algebra states the exact snapshot discipline independently
    of either recursive Rust source or PDA scheduling. *)

Definition free_map := nat -> option nat.
Definition matcher_value := free_map -> option free_map.

Definition empty_free_map : free_map := fun _ => None.

Definition insert_binding
    (state : free_map) (level value : nat) : free_map :=
  fun candidate => if Nat.eq_dec candidate level then Some value else state candidate.

Definition bind_value
    (state : free_map) (level value : nat) : option free_map :=
  match state level with
  | None => Some (insert_binding state level value)
  | Some prior => if Nat.eqb prior value then Some state else None
  end.

Fixpoint sequence_children
    (children : list matcher_value) (state : free_map) : option free_map :=
  match children with
  | [] => Some state
  | child :: rest =>
      match child state with
      | None => None
      | Some next => sequence_children rest next
      end
  end.

(** Every disjunct receives the same [state], and a disjunction publishes no
    branch-local bindings even when a branch succeeds.  This is the exact
    [attempt_opt] / [Frame::OrBranch] contract in Rust. *)
Fixpoint isolated_first_success
    (children : list matcher_value) (state : free_map) : option free_map :=
  match children with
  | [] => None
  | child :: rest =>
      match child state with
      | Some _ => Some state
      | None => isolated_first_success rest state
      end
  end.

Inductive matcher_label : Type :=
| Exact : nat -> nat -> matcher_label
| Bind : nat -> nat -> matcher_label
| Sequence : matcher_label
| Conjunction : matcher_label
| Disjunction : matcher_label
| Negation : matcher_label.

Definition matcher_algebra
    (label : matcher_label) (children : list matcher_value) : matcher_value :=
  match label with
  | Exact target pattern =>
      fun state => if Nat.eqb target pattern then Some state else None
  | Bind level value => fun state => bind_value state level value
  | Sequence | Conjunction => fun state => sequence_children children state
  | Disjunction => fun state => isolated_first_success children state
  | Negation =>
      fun state =>
        match children with
        | child :: _ =>
            match child state with
            | Some _ => None
            | None => Some state
            end
        | [] => Some state
        end
  end.

Definition recursive_spatial_match
    (subject : @tree matcher_label) (initial : free_map) : option free_map :=
  @fold_tree matcher_label matcher_value matcher_algebra subject initial.

Definition pda_spatial_match
    (subject : @tree matcher_label) (initial : free_map) : option free_map :=
  match @run matcher_label matcher_value matcher_algebra
          (@compile_tree matcher_label subject) [] with
  | Some [compiled] => compiled initial
  | _ => None
  end.

Theorem spatial_match_pda_equivalent_to_recursive_match :
  forall subject initial,
    pda_spatial_match subject initial = recursive_spatial_match subject initial.
Proof.
  intros subject initial.
  unfold pda_spatial_match, recursive_spatial_match.
  rewrite (@pda_fold_equivalent_to_recursive_fold
    matcher_label matcher_value matcher_algebra subject).
  reflexivity.
Qed.

(** State-isolation lemmas used by the connective and retry frames. *)

Theorem failed_disjunct_retries_from_the_original_snapshot :
  forall first second state,
    first state = None ->
    isolated_first_success [first; second] state =
      isolated_first_success [second] state.
Proof.
  intros first second state failed. simpl. now rewrite failed.
Qed.

Theorem successful_disjunct_restores_the_original_snapshot :
  forall first second state accepted,
    first state = Some accepted ->
    isolated_first_success [first; second] state = Some state.
Proof.
  intros first second state accepted success. simpl. now rewrite success.
Qed.

Theorem disjunction_never_publishes_branch_bindings :
  forall children state accepted,
    isolated_first_success children state = Some accepted -> accepted = state.
Proof.
  induction children as [|child rest IH]; intros state accepted success.
  - discriminate.
  - simpl in success. destruct (child state) as [child_state|] eqn:branch.
    + now injection success.
    + now apply IH in success.
Qed.

Theorem successful_negation_restores_the_original_snapshot :
  forall child state,
    child state = None ->
    matcher_algebra Negation [child] state = Some state.
Proof.
  intros child state refused. simpl. now rewrite refused.
Qed.

Theorem refused_negation_discards_the_child_state :
  forall child state child_state,
    child state = Some child_state ->
    matcher_algebra Negation [child] state = None.
Proof.
  intros child state child_state accepted. simpl. now rewrite accepted.
Qed.

Theorem binding_is_idempotent_for_the_same_value :
  forall state level value,
    state level = Some value -> bind_value state level value = Some state.
Proof.
  intros state level value present.
  unfold bind_value. rewrite present, Nat.eqb_refl. reflexivity.
Qed.

Theorem binding_rejects_a_conflicting_value_without_mutation :
  forall state level prior value,
    state level = Some prior -> prior <> value -> bind_value state level value = None.
Proof.
  intros state level prior value present different.
  unfold bind_value. rewrite present.
  destruct (Nat.eqb prior value) eqn:equal; [|reflexivity].
  apply Nat.eqb_eq in equal. contradiction.
Qed.

(** The PathMap singleton optimization changes ownership, not semantics.  It
    removes the only target and pattern entry from their owned zippers and
    feeds the same pair to the matcher algebra. *)
Section SingletonPathMap.

Context {Entry : Type}.
Variable match_entry : Entry -> Entry -> matcher_value.

Definition recursive_singleton_path_match
    (target pattern : Entry) : matcher_value := match_entry target pattern.

Definition owned_zipper_singleton_path_match
    (target pattern : Entry) : matcher_value := match_entry target pattern.

Theorem owned_singleton_path_move_preserves_match_semantics :
  forall target pattern state,
    owned_zipper_singleton_path_match target pattern state =
    recursive_singleton_path_match target pattern state.
Proof. reflexivity. Qed.

End SingletonPathMap.

(** The retained PathMap indexes are conservative prefilters.  Their exact
    prefix test and residual byte query may both admit false positives, since
    the spatial matcher remains authoritative.  They must never reject a
    spatial success.  This section proves that the local no-false-negative
    obligations are sufficient to preserve the first admissible candidate in
    the predecessor's canonical list order. *)
Section RetainedPathQuery.

Context {Pattern Datum : Type}.
Variable spatial_accepts : Pattern -> Datum -> bool.
Variable query_eligible : Pattern -> bool.
Variable retained_prefix_accepts : Pattern -> Datum -> bool.
Variable residual_query_accepts : Pattern -> Datum -> bool.

Hypothesis successful_match_has_retained_prefix :
  forall pattern datum,
    query_eligible pattern = true ->
    spatial_accepts pattern datum = true ->
    retained_prefix_accepts pattern datum = true.

Hypothesis successful_match_passes_residual_query :
  forall pattern datum,
    query_eligible pattern = true ->
    spatial_accepts pattern datum = true ->
    residual_query_accepts pattern datum = true.

Definition retained_candidate (pattern : Pattern) (datum : Datum) : bool :=
  if query_eligible pattern
  then retained_prefix_accepts pattern datum && residual_query_accepts pattern datum
  else true.

Theorem retained_candidate_has_no_false_negative :
  forall pattern datum,
    spatial_accepts pattern datum = true ->
    retained_candidate pattern datum = true.
Proof.
  intros pattern datum spatial.
  unfold retained_candidate.
  destruct (query_eligible pattern) eqn:eligible; [|reflexivity].
  rewrite (@successful_match_has_retained_prefix pattern datum eligible spatial).
  rewrite (@successful_match_passes_residual_query pattern datum eligible spatial).
  reflexivity.
Qed.

Fixpoint first_spatial_match
    (candidates : list Pattern) (datum : Datum) : option Pattern :=
  match candidates with
  | [] => None
  | pattern :: rest =>
      if spatial_accepts pattern datum
      then Some pattern
      else first_spatial_match rest datum
  end.

Definition retained_candidates
    (candidates : list Pattern) (datum : Datum) : list Pattern :=
  filter (fun pattern => retained_candidate pattern datum) candidates.

Theorem retained_filter_preserves_first_spatial_match :
  forall candidates datum,
    first_spatial_match (retained_candidates candidates datum) datum =
    first_spatial_match candidates datum.
Proof.
  induction candidates as [|pattern rest IH]; intros datum; [reflexivity|].
  unfold retained_candidates in *; simpl.
  destruct (retained_candidate pattern datum) eqn:retained.
  - simpl. destruct (spatial_accepts pattern datum) eqn:spatial.
    + reflexivity.
    + exact (IH datum).
  - destruct (spatial_accepts pattern datum) eqn:spatial.
    + pose proof (@retained_candidate_has_no_false_negative pattern datum spatial) as keep.
      rewrite keep in retained. discriminate.
    + exact (IH datum).
Qed.

End RetainedPathQuery.

(** The datum-first retained automaton now intersects its transition trie
    directly with the authoritative resting-data Bag.  The fresh datum can
    run both prefix and residual checks immediately.  A stored datum reached
    by the PathMap product runs only the prefix check here; the ordinary
    candidate PDA remains the authority for its residual query and spatial
    bindings.  Consequently the product may retain a false positive, but the
    theorem below rules out a false negative for either source. *)
Section RetainedBagProduct.

Context {Pattern Datum : Type}.
Variable spatial_accepts : Pattern -> Datum -> Prop.
Variable query_eligible : Pattern -> bool.
Variable retained_prefix_accepts : Pattern -> Datum -> bool.
Variable residual_query_accepts : Pattern -> Datum -> bool.
Variable stored_occurrence : Datum -> Prop.

Hypothesis successful_match_has_product_prefix :
  forall pattern datum,
    query_eligible pattern = true ->
    spatial_accepts pattern datum ->
    retained_prefix_accepts pattern datum = true.

Hypothesis successful_fresh_match_passes_residual :
  forall pattern datum,
    query_eligible pattern = true ->
    spatial_accepts pattern datum ->
    residual_query_accepts pattern datum = true.

Definition pool_spatial_accepts
    (pattern : Pattern) (fresh : Datum) : Prop :=
  spatial_accepts pattern fresh \/
  exists resting,
    stored_occurrence resting /\ spatial_accepts pattern resting.

Definition retained_bag_product_selects
    (pattern : Pattern) (fresh : Datum) : Prop :=
  query_eligible pattern = false \/
  (retained_prefix_accepts pattern fresh = true /\
   residual_query_accepts pattern fresh = true) \/
  exists resting,
    stored_occurrence resting /\
    retained_prefix_accepts pattern resting = true.

Theorem retained_bag_product_has_no_false_negative :
  forall pattern fresh,
    pool_spatial_accepts pattern fresh ->
    retained_bag_product_selects pattern fresh.
Proof.
  intros pattern fresh accepted.
  destruct (query_eligible pattern) eqn:eligible.
  - right. destruct accepted as [fresh_accepted | [resting [stored accepted]]].
    + left. split.
      * now apply successful_match_has_product_prefix.
      * now apply successful_fresh_match_passes_residual.
    + right. exists resting. split; [exact stored|].
      now apply successful_match_has_product_prefix.
  - now left.
Qed.

Corollary retained_bag_product_rejection_proves_no_pool_match :
  forall pattern fresh,
    ~ retained_bag_product_selects pattern fresh ->
    ~ pool_spatial_accepts pattern fresh.
Proof.
  intros pattern fresh rejected accepted.
  apply rejected.
  now apply retained_bag_product_has_no_false_negative.
Qed.

End RetainedBagProduct.

(** Concrete semantic bridge for the generated ordered RhoPath compiler.

    [rho_value] abstracts one descriptor node after canonical RMP2 encoding.
    Its [identity] is the complete canonical byte identity, [label] is the
    generated message/variant tag, and [children] are the ordered descriptor
    children.  Exact instructions compare identities, holes accept one complete
    node, and ordered instructions recursively compare the generated label and
    children.  Associative-commutative or otherwise unsupported patterns have
    no compile derivation, which models the production fallback to the spatial
    PDA.

    The earlier retained-filter theorem consumes a no-false-negative premise.
    The theorem below discharges that premise for every successfully compiled
    exact/hole/ordered program.  The generic PDA fold theorem supplies the
    separate recursive-to-iterative control equivalence. *)
Section GeneratedRhoPathQueryCompiler.

Inductive rho_value : Type :=
| RhoValue : nat -> nat -> list rho_value -> rho_value.

Inductive rho_pattern : Type :=
| RhoExact : nat -> rho_pattern
| RhoHole : rho_pattern
| RhoOrdered : nat -> list rho_pattern -> rho_pattern
| RhoUnsupported : rho_pattern.

Inductive rho_query : Type :=
| QueryExact : nat -> rho_query
| QueryHole : rho_query
| QueryOrdered : nat -> list rho_query -> rho_query.

Inductive rho_query_compiles : rho_pattern -> rho_query -> Prop :=
| CompileExact :
    forall identity,
      rho_query_compiles (RhoExact identity) (QueryExact identity)
| CompileHole :
    rho_query_compiles RhoHole QueryHole
| CompileOrdered :
    forall label patterns queries,
      rho_queries_compile patterns queries ->
      rho_query_compiles
        (RhoOrdered label patterns)
        (QueryOrdered label queries)
with rho_queries_compile : list rho_pattern -> list rho_query -> Prop :=
| CompileNil : rho_queries_compile [] []
| CompileCons :
    forall pattern query patterns queries,
      rho_query_compiles pattern query ->
      rho_queries_compile patterns queries ->
      rho_queries_compile (pattern :: patterns) (query :: queries).

Inductive spatial_rho_accepts : rho_pattern -> rho_value -> Prop :=
| SpatialExact :
    forall identity label children,
      spatial_rho_accepts
        (RhoExact identity)
        (RhoValue identity label children)
| SpatialHole :
    forall identity label children,
      spatial_rho_accepts RhoHole (RhoValue identity label children)
| SpatialOrdered :
    forall identity label patterns values,
      spatial_rho_accepts_list patterns values ->
      spatial_rho_accepts
        (RhoOrdered label patterns)
        (RhoValue identity label values)
with spatial_rho_accepts_list : list rho_pattern -> list rho_value -> Prop :=
| SpatialNil : spatial_rho_accepts_list [] []
| SpatialCons :
    forall pattern value patterns values,
      spatial_rho_accepts pattern value ->
      spatial_rho_accepts_list patterns values ->
      spatial_rho_accepts_list (pattern :: patterns) (value :: values).

Inductive rho_query_accepts : rho_query -> rho_value -> Prop :=
| QueryAcceptsExact :
    forall identity label children,
      rho_query_accepts
        (QueryExact identity)
        (RhoValue identity label children)
| QueryAcceptsHole :
    forall identity label children,
      rho_query_accepts QueryHole (RhoValue identity label children)
| QueryAcceptsOrdered :
    forall identity label queries values,
      rho_query_accepts_list queries values ->
      rho_query_accepts
        (QueryOrdered label queries)
        (RhoValue identity label values)
with rho_query_accepts_list : list rho_query -> list rho_value -> Prop :=
| QueryAcceptsNil : rho_query_accepts_list [] []
| QueryAcceptsCons :
    forall query value queries values,
      rho_query_accepts query value ->
      rho_query_accepts_list queries values ->
      rho_query_accepts_list (query :: queries) (value :: values).

Theorem compiled_rho_path_query_preserves_spatial_success :
  forall pattern query,
    rho_query_compiles pattern query ->
    forall value,
      spatial_rho_accepts pattern value ->
      rho_query_accepts query value
with compiled_rho_path_queries_preserve_spatial_success :
  forall patterns queries,
    rho_queries_compile patterns queries ->
    forall values,
      spatial_rho_accepts_list patterns values ->
      rho_query_accepts_list queries values.
Proof.
  - intros pattern query compiled.
    destruct compiled.
    + intros value accepted. inversion accepted; subst. constructor.
    + intros value accepted. inversion accepted; subst. constructor.
    + intros value accepted. inversion accepted; subst.
      constructor.
      eapply compiled_rho_path_queries_preserve_spatial_success; eauto.
  - intros patterns queries compiled.
    destruct compiled.
    + intros values accepted. inversion accepted; subst. constructor.
    + intros values accepted. inversion accepted; subst.
      constructor.
      * eapply compiled_rho_path_query_preserves_spatial_success; eauto.
      * eapply compiled_rho_path_queries_preserve_spatial_success; eauto.
Qed.

Theorem unsupported_rho_path_patterns_fail_closed :
  forall query, ~ rho_query_compiles RhoUnsupported query.
Proof. intros query compiled. inversion compiled. Qed.

End GeneratedRhoPathQueryCompiler.

(** The encoded Expr cursor performs one shallow outer-variant refinement
    before entering the existing matcher.  [Unproved] is deliberately evidence
    about the cursor, not a semantic result.  Therefore only two independently
    proven, unequal tags may reject without delegation.  This section proves
    that routing law parametrically; protobuf parsing and canonical ParBag
    admission remain executable obligations. *)
Section EncodedExprOuterEnvelopeRouting.

Context {Fault : Type}.

Inductive route_outcome : Type :=
| RouteAccepted : free_map -> route_outcome
| RouteRejected : route_outcome
| RouteInvalid : Fault -> route_outcome.

Inductive tag_evidence : Type :=
| Unproved : tag_evidence
| Proven : nat -> tag_evidence.

Inductive route_decision : Type :=
| RejectProvenDifferent : route_decision
| Delegate : route_decision.

Definition outer_route_decision
    (target_outer pattern_deep : tag_evidence) : route_decision :=
  match target_outer, pattern_deep with
  | Proven target, Proven pattern =>
      if Nat.eqb target pattern then Delegate else RejectProvenDifferent
  | _, _ => Delegate
  end.

Definition apply_outer_route
    (decision : route_decision) (downstream : route_outcome) : route_outcome :=
  match decision with
  | RejectProvenDifferent => RouteRejected
  | Delegate => downstream
  end.

Theorem outer_route_rejects_iff_tags_are_proven_different :
  forall target pattern,
    outer_route_decision target pattern = RejectProvenDifferent <->
    exists target_tag pattern_tag,
      target = Proven target_tag /\
      pattern = Proven pattern_tag /\
      target_tag <> pattern_tag.
Proof.
  intros target pattern.
  destruct target as [|target_tag], pattern as [|pattern_tag]; simpl.
  - split; [discriminate|]. intros [target [pattern [absurd _]]]. discriminate.
  - split; [discriminate|]. intros [target [pattern [absurd _]]]. discriminate.
  - split; [discriminate|]. intros [target [pattern [_ [absurd _]]]]. discriminate.
  - destruct (Nat.eqb target_tag pattern_tag) eqn:equal.
    + apply Nat.eqb_eq in equal. subst pattern_tag.
      split; [discriminate|].
      intros [witness_target [witness_pattern
        [target_eq [pattern_eq different]]]].
      injection target_eq as target_equal.
      injection pattern_eq as pattern_equal.
      subst witness_target witness_pattern.
      exfalso. exact (different eq_refl).
    + split.
      * intros _. exists target_tag, pattern_tag. repeat split; try reflexivity.
        now apply Nat.eqb_neq.
      * intros _. reflexivity.
Qed.

Theorem same_or_unproved_outer_route_delegates_exactly :
  forall target pattern,
    (target = Unproved \/ pattern = Unproved \/
     exists tag, target = Proven tag /\ pattern = Proven tag) ->
    outer_route_decision target pattern = Delegate.
Proof.
  intros target pattern evidence.
  destruct target as [|target_tag], pattern as [|pattern_tag]; simpl; try reflexivity.
  destruct evidence as [absurd | [absurd | [tag [target_eq pattern_eq]]]];
    try discriminate.
  inversion target_eq. inversion pattern_eq. subst.
  now rewrite Nat.eqb_refl.
Qed.

Theorem delegation_preserves_outcome_and_free_map :
  forall decision downstream,
    decision = Delegate ->
    apply_outer_route decision downstream = downstream.
Proof. intros [] downstream delegated; [discriminate|reflexivity]. Qed.

Theorem delegated_invalid_payload_is_not_a_semantic_negative :
  forall target pattern fault,
    outer_route_decision target pattern = Delegate ->
    apply_outer_route
      (outer_route_decision target pattern)
      (RouteInvalid fault) = RouteInvalid fault.
Proof. intros target pattern fault delegated. now rewrite delegated. Qed.

Theorem outer_route_different_tag_rejection_has_no_binding_output :
  forall target pattern,
    outer_route_decision target pattern = RejectProvenDifferent ->
    apply_outer_route
      (outer_route_decision target pattern)
      (RouteAccepted empty_free_map) = RouteRejected.
Proof. intros target pattern rejected. now rewrite rejected. Qed.

(** Conditional bridge to the owned matcher authority.  The assumptions state
    exactly the Rust/executable boundary: proven tags are sound, different
    decoded variants reject without reading children, and every delegated path
    returns the owned authority's complete outcome.  No assumption claims that
    arbitrary protobuf bytes are valid. *)
Context {Encoded Expr : Type}.
Variable decode_expr : Encoded -> Expr.
Variable expr_tag : Expr -> nat.
Variable target_outer_evidence : Encoded -> tag_evidence.
Variable pattern_deep_evidence : Encoded -> tag_evidence.
Variable delegate_match : Encoded -> Encoded -> free_map -> route_outcome.
Variable authoritative_match : Expr -> Expr -> free_map -> route_outcome.

Hypothesis target_outer_evidence_sound :
  forall encoded tag,
    target_outer_evidence encoded = Proven tag ->
    expr_tag (decode_expr encoded) = tag.

Hypothesis pattern_deep_evidence_sound :
  forall encoded tag,
    pattern_deep_evidence encoded = Proven tag ->
    expr_tag (decode_expr encoded) = tag.

Hypothesis different_decoded_expr_tags_reject :
  forall target pattern initial,
    expr_tag (decode_expr target) <> expr_tag (decode_expr pattern) ->
    authoritative_match (decode_expr target) (decode_expr pattern) initial =
      RouteRejected.

Hypothesis delegated_branch_is_authoritative :
  forall target pattern initial,
    outer_route_decision
      (target_outer_evidence target)
      (pattern_deep_evidence pattern) = Delegate ->
    delegate_match target pattern initial =
      authoritative_match (decode_expr target) (decode_expr pattern) initial.

Definition encoded_outer_envelope_match
    (target pattern : Encoded) (initial : free_map) : route_outcome :=
  apply_outer_route
    (outer_route_decision
      (target_outer_evidence target)
      (pattern_deep_evidence pattern))
    (delegate_match target pattern initial).

Theorem encoded_outer_envelope_route_preserves_authoritative_match :
  forall target pattern initial,
    encoded_outer_envelope_match target pattern initial =
    authoritative_match (decode_expr target) (decode_expr pattern) initial.
Proof.
  intros target pattern initial.
  unfold encoded_outer_envelope_match.
  destruct (outer_route_decision
    (target_outer_evidence target)
    (pattern_deep_evidence pattern)) eqn:decision.
  - apply outer_route_rejects_iff_tags_are_proven_different in decision.
    destruct decision as [target_tag [pattern_tag [target_proven [pattern_proven unequal]]]].
    simpl.
    rewrite (@different_decoded_expr_tags_reject target pattern initial).
    + reflexivity.
    + rewrite (@target_outer_evidence_sound target target_tag target_proven).
      rewrite (@pattern_deep_evidence_sound pattern pattern_tag pattern_proven).
      exact unequal.
  - simpl. now apply delegated_branch_is_authoritative.
Qed.

(** The prior defective rule collapsed target deep-parser refusal to semantic
    rejection whenever the pattern deep parser had succeeded.  This witness is
    executable anti-vacuity for the distinction above. *)
Definition collapsed_deep_failure_route
    (target_deep pattern_deep : tag_evidence)
    (downstream : route_outcome) : route_outcome :=
  match pattern_deep, target_deep with
  | Proven pattern, Proven target =>
      if Nat.eqb target pattern then downstream else RouteRejected
  | Proven _, Unproved => RouteRejected
  | _, _ => downstream
  end.

Theorem prior_deep_failure_collapse_is_detected :
  forall fault,
    apply_outer_route
      (outer_route_decision (Proven 10) (Proven 10))
      (RouteInvalid fault) = RouteInvalid fault /\
    collapsed_deep_failure_route
      Unproved (Proven 10) (RouteInvalid fault) = RouteRejected.
Proof. intros fault. split; reflexivity. Qed.

End EncodedExprOuterEnvelopeRouting.

(** Byte-native authoritative Connective matching.

    This section deliberately separates two obligations.  The definitions and
    lemmas below prove the Connective algebra and its [FreeMap] snapshot laws.
    The final bridge is conditional on sound evidence from the canonical
    protobuf cursor and on exact delegation to the generated decoder.  Thus a
    parser refusal is never reinterpreted as a semantic refusal, and malformed
    bytes remain an admission/decode outcome rather than a matcher result. *)
Section EncodedConnectiveAuthoritativeRouting.

Context {Fault Child DecodedChild Encoded : Type}.

Definition routed_matcher := free_map -> @route_outcome Fault.

Inductive scalar_family : Type :=
| ScalarBool
| ScalarInt
| ScalarString
| ScalarUri
| ScalarByteArray.

Definition scalar_family_eqb
    (left right : scalar_family) : bool :=
  match left, right with
  | ScalarBool, ScalarBool
  | ScalarInt, ScalarInt
  | ScalarString, ScalarString
  | ScalarUri, ScalarUri
  | ScalarByteArray, ScalarByteArray => true
  | _, _ => false
  end.

Inductive connective_view (Element : Type) : Type :=
| ViewAnd : list Element -> connective_view Element
| ViewOr : list Element -> connective_view Element
| ViewNot : Element -> connective_view Element
| ViewVarRef : connective_view Element
| ViewPredicate : scalar_family -> bool -> connective_view Element
| ViewAbsent : connective_view Element.

Arguments ViewAnd {Element} _.
Arguments ViewOr {Element} _.
Arguments ViewNot {Element} _.
Arguments ViewVarRef {Element}.
Arguments ViewPredicate {Element} _ _.
Arguments ViewAbsent {Element}.

Definition map_connective_view {A B : Type}
    (map_child : A -> B) (view : connective_view A) : connective_view B :=
  match view with
  | ViewAnd children => ViewAnd (map map_child children)
  | ViewOr children => ViewOr (map map_child children)
  | ViewNot child => ViewNot (map_child child)
  | ViewVarRef => ViewVarRef
  | ViewPredicate family payload => ViewPredicate family payload
  | ViewAbsent => ViewAbsent
  end.

Fixpoint route_sequence
    (children : list routed_matcher) (state : free_map) : @route_outcome Fault :=
  match children with
  | [] => RouteAccepted state
  | child :: rest =>
      match child state with
      | RouteAccepted next => route_sequence rest next
      | RouteRejected => RouteRejected
      | RouteInvalid fault => RouteInvalid fault
      end
  end.

(** Every OR arm observes the same snapshot.  Success carries only the
    verdict; no arm-local binding is published. *)
Fixpoint route_isolated_disjunction
    (children : list routed_matcher) (state : free_map) : @route_outcome Fault :=
  match children with
  | [] => RouteRejected
  | child :: rest =>
      match child state with
      | RouteAccepted _ => RouteAccepted state
      | RouteRejected => route_isolated_disjunction rest state
      | RouteInvalid fault => RouteInvalid fault
      end
  end.

Definition route_negation
    (child : routed_matcher) (state : free_map) : @route_outcome Fault :=
  match child state with
  | RouteAccepted _ => RouteRejected
  | RouteRejected => RouteAccepted state
  | RouteInvalid fault => RouteInvalid fault
  end.

(** A closed control theorem for the explicit Connective continuation
    machine.  Leaf jobs already denote child state transformers; AND, OR, and
    NOT are the exact frame reductions above.  The later parser bridge states
    when encoded byte ranges denote those leaves and labels. *)
Inductive connective_machine_label : Type :=
| ConnectiveLeaf : routed_matcher -> connective_machine_label
| ConnectiveAnd : connective_machine_label
| ConnectiveOr : connective_machine_label
| ConnectiveNot : connective_machine_label.

Definition connective_machine_algebra
    (label : connective_machine_label)
    (children : list routed_matcher) : routed_matcher :=
  match label with
  | ConnectiveLeaf leaf => leaf
  | ConnectiveAnd => route_sequence children
  | ConnectiveOr => route_isolated_disjunction children
  | ConnectiveNot =>
      fun state =>
        match children with
        | child :: _ => route_negation child state
        | [] => RouteAccepted state
        end
  end.

Definition recursive_parsed_connective
    (subject : @tree connective_machine_label) (initial : free_map)
    : @route_outcome Fault :=
  @fold_tree connective_machine_label routed_matcher
    connective_machine_algebra subject initial.

Definition pda_parsed_connective
    (subject : @tree connective_machine_label) (initial : free_map)
    : @route_outcome Fault :=
  match @run connective_machine_label routed_matcher connective_machine_algebra
          (@compile_tree connective_machine_label subject) [] with
  | Some [compiled] => compiled initial
  | _ => RouteRejected
  end.

Theorem encoded_connective_pda_equivalent_to_parsed_connective :
  forall subject initial,
    pda_parsed_connective subject initial =
    recursive_parsed_connective subject initial.
Proof.
  intros subject initial.
  unfold pda_parsed_connective, recursive_parsed_connective.
  rewrite (@pda_fold_equivalent_to_recursive_fold
    connective_machine_label routed_matcher connective_machine_algebra subject).
  reflexivity.
Qed.

Definition predicate_route
    (target_family : option scalar_family) (expected : scalar_family)
    : routed_matcher :=
  fun state =>
    match target_family with
    | Some actual =>
        if scalar_family_eqb actual expected
        then RouteAccepted state
        else RouteRejected
    | None => RouteRejected
    end.

Definition connective_view_semantics {Element : Type}
    (target_family : option scalar_family)
    (match_child : Element -> routed_matcher)
    (view : connective_view Element) : routed_matcher :=
  match view with
  | ViewAnd children => route_sequence (map match_child children)
  | ViewOr children => route_isolated_disjunction (map match_child children)
  | ViewNot child => route_negation (match_child child)
  | ViewVarRef => fun _ => RouteRejected
  | ViewPredicate family _ => predicate_route target_family family
  | ViewAbsent => fun _ => RouteRejected
  end.

Lemma child_route_map_refines_decode :
  forall (decode_child : Child -> DecodedChild)
         (encoded_child : Child -> routed_matcher)
         (decoded_child : DecodedChild -> routed_matcher),
    (forall child, encoded_child child = decoded_child (decode_child child)) ->
    forall children,
      map encoded_child children =
      map decoded_child (map decode_child children).
Proof.
  intros decode_child encoded_child decoded_child exact.
  induction children as [|child rest IH]; simpl; [reflexivity|].
  now rewrite exact, IH.
Qed.

Theorem parsed_connective_semantics_refines_decoded_connective :
  forall (decode_child : Child -> DecodedChild)
         (encoded_child : Child -> routed_matcher)
         (decoded_child : DecodedChild -> routed_matcher)
         target_family view,
    (forall child, encoded_child child = decoded_child (decode_child child)) ->
    connective_view_semantics target_family encoded_child view =
    connective_view_semantics target_family decoded_child
      (map_connective_view decode_child view).
Proof.
  intros decode_child encoded_child decoded_child target_family view exact.
  destruct view; simpl;
    try rewrite (@child_route_map_refines_decode
      decode_child encoded_child decoded_child exact);
    try rewrite exact; reflexivity.
Qed.

Theorem route_and_failure_publishes_no_partial_bindings :
  forall child rest state,
    child state = RouteRejected ->
    route_sequence (child :: rest) state = RouteRejected.
Proof. intros child rest state refused. simpl. now rewrite refused. Qed.

Theorem route_or_success_publishes_no_branch_bindings :
  forall child rest state accepted,
    child state = RouteAccepted accepted ->
    route_isolated_disjunction (child :: rest) state = RouteAccepted state.
Proof. intros child rest state accepted success. simpl. now rewrite success. Qed.

Theorem route_negation_restores_the_original_state :
  forall child state,
    (child state = RouteRejected ->
      route_negation child state = RouteAccepted state) /\
    (forall accepted,
      child state = RouteAccepted accepted ->
      route_negation child state = RouteRejected).
Proof.
  intros child state. split.
  - intro refused. unfold route_negation. now rewrite refused.
  - intros accepted success. unfold route_negation. now rewrite success.
Qed.

Theorem encoded_connective_predicate_ignores_payload :
  forall target_family family left_payload right_payload,
    connective_view_semantics target_family (fun _ : Child => predicate_route None family)
      (ViewPredicate family left_payload) =
    connective_view_semantics target_family (fun _ : Child => predicate_route None family)
      (ViewPredicate family right_payload).
Proof. reflexivity. Qed.

Theorem encoded_connective_var_ref_and_absent_reject :
  forall target_family (match_child : Child -> routed_matcher) state,
    connective_view_semantics target_family match_child ViewVarRef state = RouteRejected /\
    connective_view_semantics target_family match_child ViewAbsent state = RouteRejected.
Proof. intros. split; reflexivity. Qed.

Inductive decode_result (Value : Type) : Type :=
| DecodeOk : Value -> decode_result Value
| DecodeFault : Fault -> decode_result Value.

Arguments DecodeOk {Value} _.
Arguments DecodeFault {Value} _.

Variable decode_child : Child -> DecodedChild.
Variable decoded_child_match : DecodedChild -> routed_matcher.
Variable child_connective_used : Child -> option bool.
Variable child_singleton_connective : Child -> option Encoded.
Variable nested_encoded_match : Encoded -> routed_matcher.
Variable target_scalar_family : Encoded -> option scalar_family.
Variable admitted : Encoded -> bool.
Variable parse_connective : Encoded -> option (connective_view Child).
Variable decode_connective : Encoded -> decode_result (connective_view DecodedChild).
Variable generated_fallback : Encoded -> routed_matcher.
Variable rejected_at_admission : Encoded -> routed_matcher.

(** A singleton-Connective envelope is not sufficient evidence that the
    Connective is an active pattern.  The encoded recursive job is entered
    only when the enclosing child [Par] also proves [connective_used = true].
    A false or unproved summary follows ordinary concrete [Par] matching. *)
Definition encoded_child_dispatch (child : Child) : routed_matcher :=
  match child_singleton_connective child with
  | Some connective =>
      match child_connective_used child with
      | Some true => nested_encoded_match connective
      | Some false | None => decoded_child_match (decode_child child)
      end
  | None => decoded_child_match (decode_child child)
  end.

Hypothesis active_nested_connective_is_authoritative :
  forall child connective,
    child_singleton_connective child = Some connective ->
    child_connective_used child = Some true ->
    nested_encoded_match connective = decoded_child_match (decode_child child).

Theorem false_summary_singleton_delegates_to_concrete_match :
  forall child connective,
    child_singleton_connective child = Some connective ->
    child_connective_used child = Some false ->
    encoded_child_dispatch child = decoded_child_match (decode_child child).
Proof.
  intros child connective singleton inactive.
  unfold encoded_child_dispatch. now rewrite singleton, inactive.
Qed.

Theorem unproved_summary_singleton_delegates_to_concrete_match :
  forall child connective,
    child_singleton_connective child = Some connective ->
    child_connective_used child = None ->
    encoded_child_dispatch child = decoded_child_match (decode_child child).
Proof.
  intros child connective singleton unproved.
  unfold encoded_child_dispatch. now rewrite singleton, unproved.
Qed.

Theorem encoded_child_metadata_dispatch_is_authoritative :
  forall child,
    encoded_child_dispatch child = decoded_child_match (decode_child child).
Proof.
  intro child. unfold encoded_child_dispatch.
  destruct (child_singleton_connective child) as [connective|] eqn:singleton;
    [|reflexivity].
  destruct (child_connective_used child) as [[|]|] eqn:summary;
    try reflexivity.
  now apply active_nested_connective_is_authoritative.
Qed.

Definition decoded_connective_authority
    (encoded : Encoded) : routed_matcher :=
  match decode_connective encoded with
  | DecodeOk view =>
      connective_view_semantics
        (target_scalar_family encoded) decoded_child_match view
  | DecodeFault fault => fun _ => RouteInvalid fault
  end.

Definition encoded_connective_route
    (encoded : Encoded) : routed_matcher :=
  if admitted encoded
  then
    match parse_connective encoded with
    | Some view =>
        connective_view_semantics
          (target_scalar_family encoded) encoded_child_dispatch view
    | None => generated_fallback encoded
    end
  else rejected_at_admission encoded.

Hypothesis parsed_connective_view_is_sound :
  forall encoded view,
    admitted encoded = true ->
    parse_connective encoded = Some view ->
    decode_connective encoded = DecodeOk (map_connective_view decode_child view).

Hypothesis unproved_fallback_is_authoritative :
  forall encoded,
    admitted encoded = true ->
    parse_connective encoded = None ->
    generated_fallback encoded = decoded_connective_authority encoded.

Hypothesis rejected_admission_is_authoritative :
  forall encoded,
    admitted encoded = false ->
    rejected_at_admission encoded = decoded_connective_authority encoded.

Theorem encoded_connective_unproved_delegates_exactly :
  forall encoded,
    admitted encoded = true ->
    parse_connective encoded = None ->
    encoded_connective_route encoded = generated_fallback encoded.
Proof.
  intros encoded accepted unproved.
  unfold encoded_connective_route. now rewrite accepted, unproved.
Qed.

Theorem malformed_endpoint_never_enters_encoded_connective_job :
  forall encoded,
    admitted encoded = false ->
    encoded_connective_route encoded = rejected_at_admission encoded.
Proof.
  intros encoded rejected. unfold encoded_connective_route. now rewrite rejected.
Qed.

Theorem encoded_connective_delegated_invalid_is_not_rejection :
  forall encoded state fault,
    admitted encoded = true ->
    parse_connective encoded = None ->
    generated_fallback encoded state = RouteInvalid fault ->
    encoded_connective_route encoded state = RouteInvalid fault.
Proof.
  intros encoded state fault accepted unproved invalid.
  rewrite (@encoded_connective_unproved_delegates_exactly encoded accepted unproved).
  exact invalid.
Qed.

Theorem encoded_connective_route_preserves_authoritative_match :
  forall encoded,
    encoded_connective_route encoded = decoded_connective_authority encoded.
Proof.
  intro encoded.
  unfold encoded_connective_route.
  destruct (admitted encoded) eqn:accepted.
  - destruct (parse_connective encoded) as [view|] eqn:parsed.
    + unfold decoded_connective_authority.
      rewrite (@parsed_connective_view_is_sound encoded view accepted parsed).
      apply parsed_connective_semantics_refines_decoded_connective.
      exact encoded_child_metadata_dispatch_is_authoritative.
    + now apply unproved_fallback_is_authoritative.
  - now apply rejected_admission_is_authoritative.
Qed.

(** Root metadata is necessary but not sufficient for the direct encoded
    matcher.  The legacy [Par] authority first selects an AC candidate from the
    target using [ParCount]/[sub_pars].  A direct root route may bypass that
    selection only after a separate proof that the encoded connective has the
    same target domain.  In particular, an inactive Connective nested below an
    active root is concrete data but is not a counted [ParCount] family, so the
    summary-only route is unsound for that shape. *)
Definition root_connective_match_dispatch
    (connective_used : bool)
    (direct_domain_safe : bool)
    (encoded resident : routed_matcher) : routed_matcher :=
  if andb connective_used direct_domain_safe then encoded else resident.

Theorem false_summary_root_delegates_to_concrete_match :
  forall direct_domain_safe encoded resident,
    root_connective_match_dispatch false direct_domain_safe encoded resident = resident.
Proof. reflexivity. Qed.

Theorem unsafe_active_root_delegates_to_candidate_selecting_match :
  forall encoded resident,
    root_connective_match_dispatch true false encoded resident = resident.
Proof. reflexivity. Qed.

Theorem safe_active_root_enters_encoded_match :
  forall encoded resident,
    root_connective_match_dispatch true true encoded resident = encoded.
Proof. reflexivity. Qed.

Inductive root_candidate_decision : Type :=
| CompileConnectiveCandidate : root_candidate_decision
| DelegateConcreteCandidate : root_candidate_decision.

Definition root_candidate_dispatch
    (connective_used : bool) : root_candidate_decision :=
  if connective_used
  then CompileConnectiveCandidate
  else DelegateConcreteCandidate.

Theorem false_summary_root_does_not_compile_connective_candidate :
  root_candidate_dispatch false = DelegateConcreteCandidate.
Proof. reflexivity. Qed.

Theorem active_summary_root_compiles_connective_candidate :
  root_candidate_dispatch true = CompileConnectiveCandidate.
Proof. reflexivity. Qed.

Theorem encoded_connective_metadata_dispatch_preserves_owned_semantics :
  (forall child,
    encoded_child_dispatch child = decoded_child_match (decode_child child)) /\
  (forall direct_domain_safe encoded resident,
    root_connective_match_dispatch false direct_domain_safe encoded resident = resident) /\
  (forall encoded resident,
    root_connective_match_dispatch true false encoded resident = resident) /\
  (forall encoded resident,
    root_connective_match_dispatch true true encoded resident = encoded) /\
  root_candidate_dispatch false = DelegateConcreteCandidate /\
  root_candidate_dispatch true = CompileConnectiveCandidate.
Proof.
  refine (conj _ (conj _ (conj _ (conj _ (conj _ _))))).
  - exact encoded_child_metadata_dispatch_is_authoritative.
  - exact false_summary_root_delegates_to_concrete_match.
  - exact unsafe_active_root_delegates_to_candidate_selecting_match.
  - exact safe_active_root_enters_encoded_match.
  - exact false_summary_root_does_not_compile_connective_candidate.
  - exact active_summary_root_compiles_connective_candidate.
Qed.

End EncodedConnectiveAuthoritativeRouting.

(** The exact-scalar child route has a stronger proof domain than a shallow
    tag test: both operands have passed the canonical scalar recognizer.  In
    that admitted domain canonical byte equality is decoded equality, so both
    equality and inequality are conclusive.  Lack of evidence delegates. *)
Section CanonicalExactScalarRouting.

Context {Fault CanonicalScalar DecodedScalar : Type}.

Variable canonical_eqb : CanonicalScalar -> CanonicalScalar -> bool.
Variable decoded_eqb : DecodedScalar -> DecodedScalar -> bool.
Variable decode_scalar : CanonicalScalar -> DecodedScalar.

Hypothesis canonical_scalar_bytes_are_injective_for_equality :
  forall target pattern,
    canonical_eqb target pattern =
    decoded_eqb (decode_scalar target) (decode_scalar pattern).

Definition canonical_exact_scalar_route
    (target pattern : option CanonicalScalar)
    (state : free_map) (downstream : @route_outcome Fault) : @route_outcome Fault :=
  match target, pattern with
  | Some target_scalar, Some pattern_scalar =>
      if canonical_eqb target_scalar pattern_scalar
      then RouteAccepted state
      else RouteRejected
  | _, _ => downstream
  end.

Definition decoded_exact_scalar_authority
    (target pattern : option CanonicalScalar)
    (state : free_map) (downstream : @route_outcome Fault) : @route_outcome Fault :=
  match target, pattern with
  | Some target_scalar, Some pattern_scalar =>
      if decoded_eqb (decode_scalar target_scalar) (decode_scalar pattern_scalar)
      then RouteAccepted state
      else RouteRejected
  | _, _ => downstream
  end.

Theorem canonical_scalar_equal_or_mismatch_preserves_authority :
  forall target pattern state downstream,
    canonical_exact_scalar_route target pattern state downstream =
    decoded_exact_scalar_authority target pattern state downstream.
Proof.
  intros [target|] [pattern|] state downstream; simpl; try reflexivity.
  now rewrite canonical_scalar_bytes_are_injective_for_equality.
Qed.

Theorem unproved_scalar_bytes_delegate_exactly :
  forall target pattern state downstream,
    target = None \/ pattern = None ->
    canonical_exact_scalar_route target pattern state downstream = downstream.
Proof.
  intros [target|] [pattern|] state downstream unproved; simpl; try reflexivity.
  destruct unproved; discriminate.
Qed.

End CanonicalExactScalarRouting.
