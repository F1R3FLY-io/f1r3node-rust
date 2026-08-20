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

(** Every disjunct receives the same [state].  A failed branch can therefore
    never leak a binding into the next attempt. *)
Fixpoint first_success
    (children : list matcher_value) (state : free_map) : option free_map :=
  match children with
  | [] => None
  | child :: rest =>
      match child state with
      | Some accepted => Some accepted
      | None => first_success rest state
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
  | Disjunction => fun state => first_success children state
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
    first_success [first; second] state = second state.
Proof.
  intros first second state failed.
  simpl. rewrite failed. destruct (second state); reflexivity.
Qed.

Theorem successful_disjunct_commits_only_its_own_state :
  forall first second state accepted,
    first state = Some accepted ->
    first_success [first; second] state = Some accepted.
Proof.
  intros first second state accepted success.
  simpl. now rewrite success.
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

    [rho_value] abstracts one descriptor node after canonical RMP1 encoding.
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
