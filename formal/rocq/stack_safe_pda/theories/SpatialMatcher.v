From Stdlib Require Import List Arith Bool FunctionalExtensionality Lia.
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

(** [record_linear_capture] in the Rholang query compiler inserts the
    big-endian key for a FreeVar level into one PathMap<()> shared by every
    atom of a BindPattern. PathMap::insert returns the old entry, so a second
    occurrence refuses the entire derived query. This model uses natural
    levels as keys: the concrete u32 big-endian encoding is injective. A
    capture site records where the generated ParCursor or admitted-region
    cursor found the hole; the same scan handles every site and atom. The
    source-to-observation correspondence remains a separate executable audit. *)
Section NonlinearRhoPathCaptureFallback.

Inductive capture_site : Type :=
| WholeAtomSite : nat -> capture_site
| MethodTargetSite : nat -> capture_site
| MethodArgumentSite : nat -> nat -> capture_site
| OrderedChildSite : nat -> nat -> capture_site
| GeneratedCursorSite : nat -> list nat -> capture_site
| AdmittedRegionSite : nat -> list nat -> capture_site.

Record capture_observation : Type := CaptureObservation {
  observed_site : capture_site;
  observed_level : option nat
}.

Definition observed_capture_ids (events : list capture_observation) : list nat :=
  flat_map (fun event =>
    match observed_level event with
    | Some level => [level]
    | None => []
    end) events.

Definition record_linear_capture (seen : list nat) (level : nat)
    : option (list nat) :=
  if in_dec Nat.eq_dec level seen then None else Some (level :: seen).

Lemma record_linear_capture_rejects_seen :
  forall seen level,
    In level seen -> record_linear_capture seen level = None.
Proof.
  intros seen level present.
  unfold record_linear_capture.
  destruct (in_dec Nat.eq_dec level seen); [reflexivity | contradiction].
Qed.

End NonlinearRhoPathCaptureFallback.

(** The generated and admitted cursors decide which syntactic FreeVar nodes
    reach [record_linear_capture]. "Emitted" here means passed to that recorder;
    an admitted dynamic scan may record a level before replacing its enclosing
    Par with a wildcard. The trace constructors below are the cursors'
    classified EnterNode, LeaveNode and conservative-fallback branches, not
    caller-supplied capture observations. An active capture records its exact
    site and level and suppresses that node's interior. A metadata-bearing
    wildcard suppresses its interior without recording any capture. Under
    suppression, even unsupported descendants are invisible to the query.
    An unsupported active node or unfinished suppression declines the query.
    The Rust-to-classified-event relation is the explicit source audit seam. *)
Section ClassifiedRhoPathCursorCapture.

Inductive classified_cursor_event : Type :=
| CursorEnterPlain : capture_site -> classified_cursor_event
| CursorEnterCapture : capture_site -> nat -> classified_cursor_event
| CursorEnterWildcard : capture_site -> classified_cursor_event
| CursorLeave : classified_cursor_event
| CursorUnsupported : capture_site -> classified_cursor_event.

Record cursor_scan_state : Type := CursorScanState {
  suppression_depth : nat;
  captured_levels : list nat;
  emitted_reverse : list capture_observation
}.

Definition cursor_scan_step
    (state : cursor_scan_state) (event : classified_cursor_event)
    : option cursor_scan_state :=
  match state with
  | CursorScanState (S hidden) seen emitted =>
      match event with
      | CursorEnterPlain _ | CursorEnterCapture _ _
      | CursorEnterWildcard _ | CursorUnsupported _ =>
          Some (CursorScanState (S (S hidden)) seen emitted)
      | CursorLeave => Some (CursorScanState hidden seen emitted)
      end
  | CursorScanState 0 seen emitted =>
      match event with
      | CursorEnterPlain _ => Some state
      | CursorEnterCapture site level =>
          match record_linear_capture seen level with
          | Some next =>
              Some (CursorScanState 1 next
                (CaptureObservation site (Some level) :: emitted))
          | None => None
          end
      | CursorEnterWildcard _ => Some (CursorScanState 1 seen emitted)
      | CursorLeave => Some state
      | CursorUnsupported _ => None
      end
  end.

Fixpoint scan_classified_cursor
    (state : cursor_scan_state) (events : list classified_cursor_event)
    : option cursor_scan_state :=
  match events with
  | [] => Some state
  | event :: rest =>
      match cursor_scan_step state event with
      | Some next => scan_classified_cursor next rest
      | None => None
      end
  end.

Definition initial_cursor_scan : cursor_scan_state := CursorScanState 0 [] [].

Definition complete_classified_cursor
    (events : list classified_cursor_event) : option cursor_scan_state :=
  match scan_classified_cursor initial_cursor_scan events with
  | Some state =>
      match suppression_depth state with
      | 0 => Some state
      | S _ => None
      end
  | None => None
  end.

Definition classified_cursor_prefilter_keeps
    (query_accepts : list capture_observation -> bool)
    (events : list classified_cursor_event) : bool :=
  match complete_classified_cursor events with
  | Some state => query_accepts (rev (emitted_reverse state))
  | None => true
  end.

Definition cursor_capture_invariant (state : cursor_scan_state) : Prop :=
  NoDup (captured_levels state) /\
  observed_capture_ids (emitted_reverse state) = captured_levels state.

Lemma record_linear_capture_fresh_result :
  forall seen level next,
    record_linear_capture seen level = Some next ->
    next = level :: seen /\ ~ In level seen.
Proof.
  intros seen level next recorded.
  unfold record_linear_capture in recorded.
  destruct (in_dec Nat.eq_dec level seen) as [present | fresh].
  - discriminate.
  - inversion recorded; subst. split; [reflexivity | exact fresh].
Qed.

Lemma active_capture_emits_its_exact_site :
  forall seen emitted site level next,
    cursor_scan_step (CursorScanState 0 seen emitted)
      (CursorEnterCapture site level) = Some next ->
    exists new_seen,
      next = CursorScanState 1 new_seen
        (CaptureObservation site (Some level) :: emitted) /\
      record_linear_capture seen level = Some new_seen.
Proof.
  intros seen emitted site level next stepped.
  cbn [cursor_scan_step] in stepped.
  destruct (record_linear_capture seen level) as [new_seen |];
    [| discriminate].
  inversion stepped; subst.
  exists new_seen. split; reflexivity.
Qed.

Lemma cursor_scan_step_preserves_capture_invariant :
  forall state event next,
    cursor_capture_invariant state ->
    cursor_scan_step state event = Some next ->
    cursor_capture_invariant next.
Proof.
  intros [depth seen emitted] event next [unique projected] stepped.
  destruct depth as [| hidden].
  - destruct event as [site | site level | site | | site];
      cbn [cursor_scan_step] in stepped.
    + inversion stepped; subst. split; assumption.
    + destruct (record_linear_capture seen level)
        as [new_seen |] eqn:recorded; [| discriminate].
      apply record_linear_capture_fresh_result in recorded
        as [same fresh]. subst new_seen.
      inversion stepped; subst. split.
      * constructor; assumption.
      * simpl. simpl in projected. rewrite projected. reflexivity.
    + inversion stepped; subst. split; assumption.
    + inversion stepped; subst. split; assumption.
    + discriminate.
  - destruct event as [site | site level | site | | site];
      cbn [cursor_scan_step] in stepped;
      inversion stepped; subst; split; assumption.
Qed.

Lemma scan_classified_cursor_preserves_capture_invariant :
  forall events state result,
    cursor_capture_invariant state ->
    scan_classified_cursor state events = Some result ->
    cursor_capture_invariant result.
Proof.
  induction events as [| event rest IH]; intros state result invariant scanned.
  - simpl in scanned. inversion scanned; subst. exact invariant.
  - cbn [scan_classified_cursor] in scanned.
    destruct (cursor_scan_step state event) as [next |] eqn:stepped;
      [| discriminate].
    eapply IH; [eapply cursor_scan_step_preserves_capture_invariant;
      eauto | exact scanned].
Qed.

Theorem successful_classified_cursor_has_unique_emitted_captures :
  forall events result,
    complete_classified_cursor events = Some result ->
    NoDup (captured_levels result) /\
    observed_capture_ids (emitted_reverse result) = captured_levels result.
Proof.
  intros events result completed.
  unfold complete_classified_cursor in completed.
  destruct (scan_classified_cursor initial_cursor_scan events)
    as [scanned_state |] eqn:scanned; [| discriminate].
  destruct (suppression_depth scanned_state); inversion completed; subst.
  eapply scan_classified_cursor_preserves_capture_invariant
    with (state := initial_cursor_scan);
    [unfold cursor_capture_invariant, initial_cursor_scan;
      simpl; split; [constructor | reflexivity] | exact scanned].
Qed.

Lemma duplicate_at_an_active_emitted_site_declines_query :
  forall state site level rest,
    suppression_depth state = 0 ->
    In level (captured_levels state) ->
    scan_classified_cursor state
      (CursorEnterCapture site level :: rest) = None.
Proof.
  intros [depth seen emitted] site level rest active repeated.
  simpl in active, repeated. subst depth.
  cbn [scan_classified_cursor cursor_scan_step].
  rewrite record_linear_capture_rejects_seen by exact repeated.
  reflexivity.
Qed.

Theorem two_visible_cursor_capture_sites_decline_query :
  forall first second level,
    complete_classified_cursor
      [CursorEnterCapture first level; CursorLeave;
       CursorEnterCapture second level; CursorLeave] = None.
Proof.
  intros first second level.
  unfold complete_classified_cursor, initial_cursor_scan,
    record_linear_capture.
  simpl.
  rewrite record_linear_capture_rejects_seen by (simpl; auto).
  reflexivity.
Qed.

Theorem visible_whole_method_ordered_and_cross_atom_sites_decline_query :
  forall level,
    complete_classified_cursor
      [CursorEnterCapture (WholeAtomSite 0) level; CursorLeave;
       CursorEnterCapture (WholeAtomSite 1) level; CursorLeave] = None /\
    complete_classified_cursor
      [CursorEnterCapture (MethodTargetSite 0) level; CursorLeave;
       CursorEnterCapture (MethodArgumentSite 0 0) level; CursorLeave] = None /\
    complete_classified_cursor
      [CursorEnterCapture (OrderedChildSite 0 0) level; CursorLeave;
       CursorEnterCapture (OrderedChildSite 0 1) level; CursorLeave] = None /\
    complete_classified_cursor
      [CursorEnterCapture (WholeAtomSite 0) level; CursorLeave;
       CursorEnterCapture (MethodTargetSite 1) level; CursorLeave] = None.
Proof.
  intro level. repeat split;
    apply two_visible_cursor_capture_sites_decline_query.
Qed.

Theorem wildcard_suppression_emits_no_hidden_capture :
  forall outer hidden_first hidden_second level,
    complete_classified_cursor
      [CursorEnterWildcard outer;
       CursorEnterCapture hidden_first level; CursorLeave;
       CursorEnterCapture hidden_second level; CursorLeave;
       CursorLeave] = Some initial_cursor_scan.
Proof. intros. reflexivity. Qed.

Theorem unsupported_or_unfinished_cursor_falls_back :
  forall site,
    complete_classified_cursor [CursorUnsupported site] = None /\
    complete_classified_cursor [CursorEnterWildcard site] = None.
Proof. intros. split; reflexivity. Qed.

Theorem classified_cursor_fallback_never_prunes :
  forall events query_accepts,
    complete_classified_cursor events = None ->
    classified_cursor_prefilter_keeps query_accepts events = true.
Proof.
  intros events query_accepts declined.
  unfold classified_cursor_prefilter_keeps.
  now rewrite declined.
Qed.

Theorem classified_cursor_prefilter_preserves_spatial_success :
  forall events query_accepts spatial_accepts,
    spatial_accepts = true ->
    (forall result,
      complete_classified_cursor events = Some result ->
      query_accepts (rev (emitted_reverse result)) = true) ->
    spatial_accepts = true /\
    classified_cursor_prefilter_keeps query_accepts events = true.
Proof.
  intros events query_accepts spatial_accepts accepted successful_soundness.
  split; [exact accepted |].
  unfold classified_cursor_prefilter_keeps.
  destruct (complete_classified_cursor events) as [result |] eqn:compiled;
    [apply successful_soundness with (result := result); reflexivity |
     reflexivity].
Qed.

End ClassifiedRhoPathCursorCapture.

(** The six [record_linear_capture] call sites in
    [rholang/src/rust/interpreter/matcher/match.rs] are: the admitted root
    [ExactFreeVar], an admitted ordered region's [ParBag], the nested admitted
    dynamic scan's [ParBag], the generated root hole, a generated cursor's
    [EnterNode] hole, and the generated cursor's shallow admitted Expr hole.
    Each site checks the recorder's Boolean result and propagates failure.
    The following effects describe only the *committed query branch* after
    those checks. A metadata-bearing whole-value wildcard is one
    [SourceWildcardEffect] and has no child effects. A nested dynamic scan may
    record captures before its enclosing Par becomes a wildcard; its recorder
    calls appear before that wildcard effect. Unsupported branches have no
    query. A failed speculative branch publishes no capture state, while an
    exact fallback requires both the compiler's [!connective_used &&
    locally_free empty] guard and a path projection that respects the
    authoritative equality. For a [BindPattern] Par the first guard implies
    the comparison lane in [SpatialMatcherContext::spatial_match_par_ref],
    whose branch condition is [!pattern.connective_used()]. It does not imply
    byte equality: [match_pars] ignores the outer Par [locally_free], so a
    generated complete-schema path is ineligible. An admitted sole-Expr path
    omits outer Par metadata but is eligible for exact fallback only when its
    region contains no nested ParBag or PathMap; either opaque token may carry
    a metadata-sensitive key. Ordered scans replace such nested Par-bearing
    regions with wildcards while continuing to inspect their capture sites.
    The retained exact prefix has the same eligibility restriction. A
    speculative branch may
    have recorded captures before failing, but it publishes none of them
    when a sound exact lane is selected.

    This is a source-indexed trace refinement, not a claim that Rocq parses
    Rust. The source audit must establish the six-site exhaustiveness, the
    shared BindPattern capture map, wildcard control-flow exclusion, and the
    authoritative exact-lane guard at the reviewed revision. *)
Section SourceIndexedRhoPathCaptureEffects.

Inductive recorder_call_site : Type :=
| AdmittedRootRecorder
| AdmittedParBagRecorder
| AdmittedDynamicScanRecorder
| GeneratedRootRecorder
| GeneratedEnterNodeRecorder
| GeneratedShallowExprRecorder.

Inductive source_query_effect : Type :=
| SourceCheckedCapture : recorder_call_site -> capture_site -> nat ->
    source_query_effect
| SourceWildcardEffect : capture_site -> source_query_effect
| SourceExactFallbackEffect : capture_site -> bool -> bool -> bool ->
    source_query_effect
| SourceUnsupportedEffect : capture_site -> source_query_effect.

Definition authoritative_exact_fallback_guard
    (connective_used has_locally_free projection_sound : bool) : bool :=
  negb connective_used && negb has_locally_free && projection_sound.

Lemma exact_fallback_guard_enters_comparison_lane :
  forall connective_used has_locally_free projection_sound,
    authoritative_exact_fallback_guard
      connective_used has_locally_free projection_sound = true ->
    connective_used = false.
Proof.
  intros connective_used has_locally_free projection_sound guarded.
  destruct connective_used, has_locally_free, projection_sound;
    simpl in guarded; try discriminate; reflexivity.
Qed.

Lemma exact_fallback_guard_requires_sound_projection :
  forall connective_used has_locally_free projection_sound,
    authoritative_exact_fallback_guard
      connective_used has_locally_free projection_sound = true ->
    projection_sound = true.
Proof.
  intros connective_used has_locally_free projection_sound guarded.
  destruct connective_used, has_locally_free, projection_sound;
    simpl in guarded; try discriminate; reflexivity.
Qed.

Definition source_query_step
    (state : cursor_scan_state) (effect : source_query_effect)
    : option cursor_scan_state :=
  match effect with
  | SourceCheckedCapture _ site level =>
      match record_linear_capture (captured_levels state) level with
      | Some seen =>
          Some (CursorScanState 0 seen
            (CaptureObservation site (Some level) :: emitted_reverse state))
      | None => None
      end
  | SourceWildcardEffect _ => Some state
  | SourceExactFallbackEffect _ connective_used has_locally_free projection_sound =>
      if authoritative_exact_fallback_guard
        connective_used has_locally_free projection_sound
      then Some state else None
  | SourceUnsupportedEffect _ => None
  end.

Definition source_effect_cursor_events
    (effect : source_query_effect) : list classified_cursor_event :=
  match effect with
  | SourceCheckedCapture _ site level =>
      [CursorEnterCapture site level; CursorLeave]
  | SourceWildcardEffect site => [CursorEnterWildcard site; CursorLeave]
  | SourceExactFallbackEffect site connective_used has_locally_free projection_sound =>
      if authoritative_exact_fallback_guard
        connective_used has_locally_free projection_sound
      then [] else [CursorUnsupported site]
  | SourceUnsupportedEffect site => [CursorUnsupported site]
  end.

Fixpoint scan_source_query_effects
    (state : cursor_scan_state) (effects : list source_query_effect)
    : option cursor_scan_state :=
  match effects with
  | [] => Some state
  | effect :: rest =>
      match source_query_step state effect with
      | Some next => scan_source_query_effects next rest
      | None => None
      end
  end.

Fixpoint scan_projected_source_effects
    (state : cursor_scan_state) (effects : list source_query_effect)
    : option cursor_scan_state :=
  match effects with
  | [] => Some state
  | effect :: rest =>
      match scan_classified_cursor state (source_effect_cursor_events effect) with
      | Some next => scan_projected_source_effects next rest
      | None => None
      end
  end.

Lemma source_checked_capture_refines_cursor_step :
  forall state effect,
    suppression_depth state = 0 ->
    source_query_step state effect =
      scan_classified_cursor state (source_effect_cursor_events effect).
Proof.
  intros [depth seen emitted] effect active.
  simpl in active. subst depth.
  destruct effect as [call site level | site |
    site connective_used has_locally_free projection_sound | site].
  - cbn [source_query_step source_effect_cursor_events
      scan_classified_cursor cursor_scan_step captured_levels emitted_reverse].
    destruct (record_linear_capture seen level);
      cbn [scan_classified_cursor cursor_scan_step]; reflexivity.
  - reflexivity.
  - destruct connective_used, has_locally_free, projection_sound; reflexivity.
  - reflexivity.
Qed.

Lemma source_query_step_keeps_cursor_active :
  forall state effect next,
    suppression_depth state = 0 ->
    source_query_step state effect = Some next ->
    suppression_depth next = 0.
Proof.
  intros [depth seen emitted] effect next active stepped.
  simpl in active. subst depth.
  destruct effect as [call site level | site |
    site connective_used has_locally_free projection_sound | site];
    cbn [source_query_step captured_levels emitted_reverse] in stepped.
  - destruct (record_linear_capture seen level);
      inversion stepped; reflexivity.
  - inversion stepped; reflexivity.
  - destruct connective_used, has_locally_free, projection_sound;
      simpl in stepped; inversion stepped; reflexivity.
  - discriminate.
Qed.

Theorem source_effect_model_refines_classified_cursor :
  forall effects state,
    suppression_depth state = 0 ->
    scan_source_query_effects state effects =
      scan_projected_source_effects state effects.
Proof.
  induction effects as [| effect rest IH]; intros state active;
    cbn [scan_source_query_effects scan_projected_source_effects];
    [reflexivity |].
  rewrite <- source_checked_capture_refines_cursor_step by exact active.
  destruct (source_query_step state effect) as [next |] eqn:stepped;
    [apply IH; eapply source_query_step_keeps_cursor_active; eauto |
     reflexivity].
Qed.

Lemma checked_source_capture_rejects_duplicate :
  forall state call site level,
    In level (captured_levels state) ->
    source_query_step state (SourceCheckedCapture call site level) = None.
Proof.
  intros state call site level repeated.
  cbn [source_query_step].
  rewrite record_linear_capture_rejects_seen by exact repeated.
  reflexivity.
Qed.

Theorem any_two_checked_source_calls_decline_query :
  forall first_call second_call first_site second_site level,
    scan_source_query_effects initial_cursor_scan
      [SourceCheckedCapture first_call first_site level;
       SourceCheckedCapture second_call second_site level] = None.
Proof.
  intros first_call second_call first_site second_site level.
  unfold initial_cursor_scan.
  simpl.
  rewrite record_linear_capture_rejects_seen by (simpl; auto).
  reflexivity.
Qed.

Lemma scan_source_query_effects_preserves_capture_invariant :
  forall effects state result,
    suppression_depth state = 0 ->
    cursor_capture_invariant state ->
    scan_source_query_effects state effects = Some result ->
    cursor_capture_invariant result.
Proof.
  induction effects as [| effect rest IH];
    intros state result active invariant scanned.
  - simpl in scanned. inversion scanned; subst. exact invariant.
  - cbn [scan_source_query_effects] in scanned.
    destruct (source_query_step state effect)
      as [next |] eqn:stepped; [| discriminate].
    eapply IH.
    + eapply source_query_step_keeps_cursor_active; eauto.
    + eapply scan_classified_cursor_preserves_capture_invariant;
        [exact invariant |].
      rewrite <- source_checked_capture_refines_cursor_step by exact active.
      exact stepped.
    + exact scanned.
Qed.

Theorem source_query_success_has_unique_emitted_captures :
  forall effects result,
    scan_source_query_effects initial_cursor_scan effects = Some result ->
    NoDup (captured_levels result) /\
    observed_capture_ids (emitted_reverse result) = captured_levels result.
Proof.
  intros effects result scanned.
  eapply scan_source_query_effects_preserves_capture_invariant
    with (state := initial_cursor_scan);
    [reflexivity | unfold cursor_capture_invariant, initial_cursor_scan;
       simpl; split; [constructor | reflexivity] | exact scanned].
Qed.

Theorem source_wildcard_effect_preserves_capture_state :
  forall state site,
    source_query_step state (SourceWildcardEffect site) = Some state.
Proof. reflexivity. Qed.

Theorem metadata_sensitive_complete_par_exact_path_declines :
  forall site,
    scan_source_query_effects initial_cursor_scan
      [SourceExactFallbackEffect site false false false] = None.
Proof. reflexivity. Qed.

Theorem metadata_sensitive_exact_projection_always_declines :
  forall state site connective_used has_locally_free,
    source_query_step state
      (SourceExactFallbackEffect site connective_used has_locally_free false)
      = None.
Proof.
  intros state site connective_used has_locally_free.
  destruct connective_used, has_locally_free; reflexivity.
Qed.

Definition source_query_prefilter_keeps
    (query_accepts : list capture_observation -> bool)
    (effects : list source_query_effect) : bool :=
  match scan_source_query_effects initial_cursor_scan effects with
  | Some state => query_accepts (rev (emitted_reverse state))
  | None => true
  end.

Theorem source_duplicate_or_unsupported_fallback_never_prunes :
  forall effects query_accepts,
    scan_source_query_effects initial_cursor_scan effects = None ->
    source_query_prefilter_keeps query_accepts effects = true.
Proof.
  intros effects query_accepts declined.
  unfold source_query_prefilter_keeps.
  now rewrite declined.
Qed.

Theorem source_effect_prefilter_preserves_spatial_success :
  forall effects query_accepts spatial_accepts,
    spatial_accepts = true ->
    (forall result,
      scan_source_query_effects initial_cursor_scan effects = Some result ->
      query_accepts (rev (emitted_reverse result)) = true) ->
    source_query_prefilter_keeps query_accepts effects = true.
Proof.
  intros effects query_accepts spatial_accepts accepted successful_soundness.
  unfold source_query_prefilter_keeps.
  destruct (scan_source_query_effects initial_cursor_scan effects)
    as [result |] eqn:compiled;
    [eapply successful_soundness; reflexivity | reflexivity].
Qed.

End SourceIndexedRhoPathCaptureEffects.

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

Module ParCountIntervals.

Definition count_mass (counts : list nat) := fold_right Nat.add 0 counts.

Section FamilyMass.

Context {Key : Type}.

Definition endpoint_mass (entries : list (Key * nat)) := count_mass (map snd entries).
Definition family_mass (belongs : Key -> bool) (entries : list (Key * nat)) :=
  endpoint_mass (filter (fun entry => belongs (fst entry)) entries).

Lemma endpoint_mass_append : forall left right,
  endpoint_mass (left ++ right) = endpoint_mass left + endpoint_mass right.
Proof.
  induction left as [|[key count] rest IH]; intro right;
    unfold endpoint_mass, count_mass in *; simpl in *; [reflexivity|].
  rewrite IH; lia.
Qed.

Lemma family_mass_partition : forall belongs entries,
  endpoint_mass entries = family_mass belongs entries + family_mass (fun key => negb (belongs key)) entries.
Proof.
  intros belongs entries. induction entries as [|[key count] rest IH]; [reflexivity|].
  unfold family_mass, endpoint_mass, count_mass in *. simpl in *.
  destruct (belongs key); simpl in *; lia.
Qed.

Theorem exact_family_total_is_endpoint_mass : forall belongs left right,
  family_mass belongs (left ++ right) = family_mass belongs left + family_mass belongs right /\
  family_mass belongs left <= endpoint_mass left.
Proof.
  intros belongs left right. split.
  - unfold family_mass. rewrite filter_app. apply endpoint_mass_append.
  - pose proof (family_mass_partition belongs left). lia.
Qed.

Theorem concrete_expression_minimum_is_exact_nonfree_mass : forall is_free entries,
  endpoint_mass entries = family_mass is_free entries + family_mass (fun key => negb (is_free key)) entries /\
  endpoint_mass entries - family_mass is_free entries = family_mass (fun key => negb (is_free key)) entries.
Proof. intros. pose proof (family_mass_partition is_free entries). split; lia. Qed.

End FamilyMass.

Inductive count_bound := Finite (amount : nat) | Unbounded.

Definition bound_le left right : Prop :=
  match left, right with
  | Finite x, Finite y => x <= y
  | _, Unbounded => True
  | Unbounded, Finite _ => False
  end.

Definition normalize_bound cap bound :=
  match bound with
  | Finite value => if value <=? cap then Finite value else Unbounded
  | Unbounded => Unbounded
  end.

Definition bound_min left right :=
  match left, right with
  | Finite x, Finite y => Finite (Nat.min x y)
  | Unbounded, other | other, Unbounded => other
  end.

Definition bound_max left right :=
  match left, right with
  | Finite x, Finite y => Finite (Nat.max x y)
  | _, _ => Unbounded
  end.

Definition bound_add left right :=
  match left, right with
  | Finite x, Finite y => Finite (x + y)
  | _, _ => Unbounded
  end.

Definition bound_scale factor bound :=
  match factor, bound with
  | 0, _ => Finite 0
  | S _, Finite value => Finite (factor * value)
  | S _, Unbounded => Unbounded
  end.

Theorem finite_cap_quotient_preserves_target_constraints : forall cap bound available,
  available <= cap ->
  (bound_le (normalize_bound cap bound) (Finite available) <-> bound_le bound (Finite available)) /\
  (bound_le (Finite available) (normalize_bound cap bound) <-> bound_le (Finite available) bound) /\
  bound_scale 0 (normalize_bound cap bound) = Finite 0.
Proof.
  intros cap [value|] available valid; cbn [normalize_bound bound_le bound_scale]; [|tauto].
  destruct (value <=? cap) eqn:within; cbn [bound_le];
    [apply Nat.leb_le in within|apply Nat.leb_gt in within]; intuition lia.
Qed.

Lemma normalize_bound_idempotent : forall cap bound,
  normalize_bound cap (normalize_bound cap bound) = normalize_bound cap bound.
Proof. intros cap [value|]; simpl; [|reflexivity]. destruct (value <=? cap) eqn:within; simpl; now rewrite ?within. Qed.

Lemma normalized_min_commutes : forall cap left right,
  normalize_bound cap (bound_min (normalize_bound cap left) (normalize_bound cap right)) =
  normalize_bound cap (bound_min left right).
Proof.
  intros cap [left|] [right|]; cbn [normalize_bound bound_min];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    cbn [normalize_bound bound_min];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    repeat match goal with
    | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
    | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
    end; try reflexivity; try (f_equal; lia); lia.
Qed.

Lemma normalized_max_commutes : forall cap left right,
  normalize_bound cap (bound_max (normalize_bound cap left) (normalize_bound cap right)) =
  normalize_bound cap (bound_max left right).
Proof.
  intros cap [left|] [right|]; cbn [normalize_bound bound_max];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    cbn [normalize_bound bound_max];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    repeat match goal with
    | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
    | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
    end; try reflexivity; try (f_equal; lia); lia.
Qed.

Lemma normalized_add_commutes : forall cap left right,
  normalize_bound cap (bound_add (normalize_bound cap left) (normalize_bound cap right)) =
  normalize_bound cap (bound_add left right).
Proof.
  intros cap [left|] [right|]; cbn [normalize_bound bound_add];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    cbn [normalize_bound bound_add];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    repeat match goal with
    | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
    | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
    end; try reflexivity; try (f_equal; lia); lia.
Qed.

Lemma normalized_scale_commutes : forall cap factor bound,
  normalize_bound cap (bound_scale factor (normalize_bound cap bound)) =
  normalize_bound cap (bound_scale factor bound).
Proof.
  intros cap [|factor] [value|]; cbn [bound_scale normalize_bound]; try reflexivity;
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    cbn [bound_scale normalize_bound];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    repeat match goal with
    | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
    | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
    end; try reflexivity; try (f_equal; nia); nia.
Qed.

Record count_interval := CountInterval { lower_bound : count_bound; upper_bound : count_bound }.
Inductive interval_operator := ParallelSum | Conjunction | Disjunction.

Definition interval_combine operator left right :=
  match operator with
  | ParallelSum => CountInterval (bound_add (lower_bound left) (lower_bound right))
      (bound_add (upper_bound left) (upper_bound right))
  | Conjunction => CountInterval (bound_max (lower_bound left) (lower_bound right))
      (bound_min (upper_bound left) (upper_bound right))
  | Disjunction => CountInterval (bound_min (lower_bound left) (lower_bound right))
      (bound_max (upper_bound left) (upper_bound right))
  end.

Definition normalize_interval cap interval := CountInterval
  (normalize_bound cap (lower_bound interval)) (normalize_bound cap (upper_bound interval)).

Lemma normalized_interval_combine_commutes : forall cap operator left right,
  normalize_interval cap (interval_combine operator (normalize_interval cap left) (normalize_interval cap right)) =
  normalize_interval cap (interval_combine operator left right).
Proof.
  intros cap operator [ll lu] [rl ru]; destruct operator;
    unfold normalize_interval, interval_combine; simpl;
    now rewrite ?normalized_add_commutes, ?normalized_min_commutes, ?normalized_max_commutes.
Qed.

Definition mathematical_interval_fold operator seed children := fold_right (interval_combine operator) seed children.
Definition bounded_interval_fold cap operator seed children :=
  fold_right (fun child acc => normalize_interval cap (interval_combine operator (normalize_interval cap child) acc))
    (normalize_interval cap seed) children.

Theorem bounded_interval_fold_preserves_existing_equations :
  (forall cap operator seed children,
    bounded_interval_fold cap operator seed children =
      normalize_interval cap (mathematical_interval_fold operator seed children)) /\
  (forall cap factor bound,
    normalize_bound cap (bound_scale factor (normalize_bound cap bound)) =
      normalize_bound cap (bound_scale factor bound)).
Proof.
  split; [|apply normalized_scale_commutes].
  intros cap operator seed children. induction children as [|child rest IH]; [reflexivity|].
  unfold bounded_interval_fold, mathematical_interval_fold in *. simpl. rewrite IH.
  apply normalized_interval_combine_commutes.
Qed.

Definition clip_upper available bound :=
  match bound with Finite value => Nat.min available value | Unbounded => available end.

Definition feasible_range available selection remainder : option (nat * nat) :=
  match lower_bound selection, lower_bound remainder with
  | Finite minimum, Finite minimum_prune =>
      if minimum_prune <=? available then
        let lower := Nat.max minimum (available - clip_upper available (upper_bound remainder)) in
        let upper := Nat.min (clip_upper available (upper_bound selection)) (available - minimum_prune) in
        if lower <=? upper then Some (lower, upper) else None
      else None
  | _, _ => None
  end.

Definition range_contains range selected :=
  match range with Some (lower, upper) => lower <= selected /\ selected <= upper | None => False end.

Definition selection_and_complement_constraints available selected selection remainder :=
  selected <= available /\
  bound_le (lower_bound selection) (Finite selected) /\
  bound_le (Finite selected) (upper_bound selection) /\
  bound_le (lower_bound remainder) (Finite (available - selected)) /\
  bound_le (Finite (available - selected)) (upper_bound remainder).

Theorem exact_feasible_range_preserves_both_constraints : forall available selected selection remainder,
  range_contains (feasible_range available selection remainder) selected <->
  selection_and_complement_constraints available selected selection remainder.
Proof.
  intros available selected [[minimum|] [maximum|]] [[minimum_prune|] [maximum_prune|]];
    unfold selection_and_complement_constraints;
    cbn [feasible_range lower_bound upper_bound clip_upper range_contains
      selection_and_complement_constraints bound_le];
    repeat match goal with |- context [if ?condition then _ else _] => destruct condition eqn:? end;
    cbn [range_contains];
    repeat match goal with
    | H : (_ <=? _) = true |- _ => apply Nat.leb_le in H
    | H : (_ <=? _) = false |- _ => apply Nat.leb_gt in H
    end; intuition lia.
Qed.

Theorem impossible_lower_bounds_refuse_finite_targets : forall available upper selection remainder,
  feasible_range available (CountInterval Unbounded upper) remainder = None /\
  feasible_range available selection (CountInterval Unbounded upper) = None /\
  (forall minimum, available < minimum ->
    feasible_range available selection (CountInterval (Finite minimum) upper) = None) /\
  (forall cap amount, cap < amount -> available <= cap ->
    feasible_range available (CountInterval (normalize_bound cap (Finite amount)) upper) remainder = None).
Proof.
  intros available upper [sl su] [rl ru]. repeat split; try reflexivity.
  - destruct sl; reflexivity.
  - intros minimum too_large. destruct sl; [|reflexivity].
    unfold feasible_range; simpl. assert ((minimum <=? available) = false) by (apply Nat.leb_gt; lia).
    now rewrite H.
  - intros cap amount overflow bounded. unfold normalize_bound.
    assert ((amount <=? cap) = false) by (apply Nat.leb_gt; lia).
    rewrite H. reflexivity.
Qed.

Theorem zero_complement_requires_the_entire_exact_target :
  (forall available, feasible_range available (CountInterval (Finite 0) Unbounded)
      (CountInterval (Finite 0) (Finite 0)) = Some (available, available)) /\
  (forall available selected selection,
    range_contains (feasible_range available selection (CountInterval (Finite 0) (Finite 0))) selected <->
    selected = available /\ bound_le (lower_bound selection) (Finite available) /\
      bound_le (Finite available) (upper_bound selection)).
Proof.
  split.
  - intro available. unfold feasible_range; simpl.
    rewrite Nat.min_0_r, Nat.sub_0_r, Nat.min_id, Nat.leb_refl. reflexivity.
  - intros available selected [[minimum|] [maximum|]];
      rewrite exact_feasible_range_preserves_both_constraints;
      unfold selection_and_complement_constraints; simpl; intuition lia.
Qed.

Fixpoint complement_counts available selected :=
  match available, selected with
  | count :: rest, chosen :: tail => count - chosen :: complement_counts rest tail
  | _, _ => []
  end.

Definition valid_selection available selected := Forall2 (fun count chosen => chosen <= count) available selected.

Theorem endpointwise_partitions_conserve_exact_mass : forall available selected,
  valid_selection available selected ->
  length selected = length available /\
  Forall2 (fun count chosen => chosen + (count - chosen) = count) available selected /\
  count_mass selected + count_mass (complement_counts available selected) = count_mass available.
Proof.
  intros available selected valid.
  induction valid as [|count chosen rest tail bounded valid IH]; simpl; [repeat split; constructor|].
  destruct IH as [lengths [cells total]]. repeat split; try lia.
  - constructor; [lia|assumption].
Qed.

Inductive subsequence {A : Type} : list A -> list A -> Prop :=
| SubsequenceEmpty : forall source, subsequence [] source
| SubsequenceKeep : forall head left right, subsequence left right -> subsequence (head :: left) (head :: right)
| SubsequenceSkip : forall head left right, subsequence left right -> subsequence left (head :: right).

Lemma filter_preserves_subsequence : forall A (predicate : A -> bool) source,
  subsequence (filter predicate source) source.
Proof.
  intros A predicate source. induction source; simpl; [constructor|].
  destruct (predicate a); constructor; assumption.
Qed.

Definition range_accepts range selected :=
  match range with Some (lower, upper) => (lower <=? selected) && (selected <=? upper) | None => false end.

Lemma range_accepts_exactly : forall range selected,
  range_accepts range selected = true <-> range_contains range selected.
Proof. intros [[lower upper]|] selected; simpl; [rewrite andb_true_iff, !Nat.leb_le; tauto|intuition discriminate]. Qed.

Definition filter_partitions available selection remainder reference :=
  filter (fun counts => range_accepts (feasible_range available selection remainder) (count_mass counts)) reference.

Theorem feasible_filter_preserves_reference_order_and_uniqueness : forall available selection remainder reference,
  subsequence (filter_partitions available selection remainder reference) reference /\
  (NoDup reference -> NoDup (filter_partitions available selection remainder reference)) /\
  (forall counts, In counts (filter_partitions available selection remainder reference) <->
    In counts reference /\ selection_and_complement_constraints available (count_mass counts) selection remainder).
Proof.
  intros. unfold filter_partitions. split; [apply filter_preserves_subsequence|].
  split; [apply NoDup_filter|]. intro counts.
  rewrite filter_In, range_accepts_exactly, exact_feasible_range_preserves_both_constraints.
  reflexivity.
Qed.

Definition initial_partition_scratch (available : list nat) := map (fun _ => 0) available.

Theorem partition_scratch_size_depends_only_on_distinct_endpoints : forall left right,
  length (initial_partition_scratch left) = length left /\
  (length left = length right -> length (initial_partition_scratch left) = length (initial_partition_scratch right)) /\
  (forall first second, length (initial_partition_scratch [first; second]) = 2).
Proof. intros. unfold initial_partition_scratch. rewrite !length_map. repeat split; auto. Qed.

Example finite_maximum_is_not_top : forall cap, normalize_bound cap (Finite cap) = Finite cap /\ Finite cap <> Unbounded.
Proof. intro cap. unfold normalize_bound. rewrite Nat.leb_refl. split; [reflexivity|discriminate]. Qed.

Example saturated_free_subtraction_loses_the_literal_minimum : forall cap,
  0 < cap ->
  Nat.min cap (cap + 1) - Nat.min cap cap = 0 /\
  family_mass (fun free => negb free) [(true, cap); (false, 1)] = 1.
Proof. intros cap positive. rewrite Nat.min_l by lia. rewrite Nat.min_id. split; [lia|reflexivity]. Qed.

Example projected_available_admits_a_nonzero_complement : forall cap,
  feasible_range cap (CountInterval (Finite 0) Unbounded) (CountInterval (Finite 0) (Finite 0)) = Some (cap, cap) /\
  (cap + 1) - cap = 1.
Proof. intro cap. split; [apply (proj1 zero_complement_requires_the_entire_exact_target)|lia]. Qed.

Example finite_old_top_omits_the_complete_larger_target : forall cap,
  ~ range_contains (feasible_range (cap + 1) (CountInterval (Finite 0) (Finite cap))
    (CountInterval (Finite 0) Unbounded)) (cap + 1).
Proof. intros cap accepted. apply exact_feasible_range_preserves_both_constraints in accepted.
  unfold selection_and_complement_constraints in accepted. simpl in accepted. lia.
Qed.

Example clamping_a_top_lower_to_available_invents_a_candidate :
  feasible_range 3 (CountInterval Unbounded Unbounded) (CountInterval (Finite 0) (Finite 0)) = None /\
  feasible_range 3 (CountInterval (Finite 3) Unbounded) (CountInterval (Finite 0) (Finite 0)) = Some (3, 3).
Proof. split; reflexivity. Qed.

Print Assumptions exact_family_total_is_endpoint_mass.
Print Assumptions finite_cap_quotient_preserves_target_constraints.
Print Assumptions bounded_interval_fold_preserves_existing_equations.
Print Assumptions concrete_expression_minimum_is_exact_nonfree_mass.
Print Assumptions exact_feasible_range_preserves_both_constraints.
Print Assumptions impossible_lower_bounds_refuse_finite_targets.
Print Assumptions zero_complement_requires_the_entire_exact_target.
Print Assumptions endpointwise_partitions_conserve_exact_mass.
Print Assumptions feasible_filter_preserves_reference_order_and_uniqueness.
Print Assumptions partition_scratch_size_depends_only_on_distinct_endpoints.

End ParCountIntervals.
