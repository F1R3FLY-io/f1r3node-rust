From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import GenerationCheckpointPublication IncrementalLedgerTrial.

Import ListNotations.

(**
  Current, retained and metadata inventory composition.

  A durable successor keeps three reachability ledgers.  The current domain
  roots the closure of the current retention entry's manifest.  The retained
  domain roots the union of the closures of every retention entry, historical
  and current, counted with multiplicity: two entries that share a manifest
  digest (root aliases) contribute two root occurrences.  The metadata domain
  roots the separately authenticated retention metadata over its own object
  graph.  A composed trial runs one failure-atomic private ledger trial of
  [CheckedLedgerTrial] per domain, in the order current, retained, metadata,
  and refuses at the first refusing domain with the composed predecessor
  unchanged.

  Retention steps derive signed root deltas from the entries.  A commit adds
  the new manifest to the current and retained domains and removes the old
  current manifest from the current domain only.  An eviction removes the
  evicted entry's manifest from the retained domain only.  Pins never reach a
  semantic trial; they protect objects physically.

  The variants other than [ComposeFaithful] are negative controls.  Each is
  refuted by a closed counterexample theorem below.
*)

Module CheckedDomainComposition.

Import IncrementalReachability.
Import CheckedReachability.
Import CheckedLedgerTrial.

Record retention_entry := RetentionEntry {
  entry_generation : nat;
  entry_manifest : nat
}.

Definition entry_eqb (left right : retention_entry) : bool :=
  Nat.eqb (entry_generation left) (entry_generation right) &&
  Nat.eqb (entry_manifest left) (entry_manifest right).

Record retention_entries := RetentionEntries {
  current_entry : retention_entry;
  historical_entries : list retention_entry
}.

Definition all_entries (state : retention_entries) : list retention_entry :=
  historical_entries state ++ [current_entry state].

Definition entry_manifests (entries : list retention_entry) : list nat :=
  map entry_manifest entries.

Definition current_roots (state : retention_entries) : counter :=
  occurrences [entry_manifest (current_entry state)].

Definition retained_roots (state : retention_entries) : counter :=
  occurrences (entry_manifests (all_entries state)).

Definition commit_entries (state : retention_entries) (next : retention_entry) :
  retention_entries :=
  RetentionEntries next (all_entries state).

Fixpoint remove_entry (victim : retention_entry) (entries : list retention_entry) :
  list retention_entry :=
  match entries with
  | [] => []
  | entry :: rest =>
      if entry_eqb entry victim then rest else entry :: remove_entry victim rest
  end.

Definition evict_entry (state : retention_entries) (victim : retention_entry) :
  retention_entries :=
  RetentionEntries (current_entry state) (remove_entry victim (historical_entries state)).

Inductive retention_step :=
| CommitStep (next : retention_entry)
| EvictStep (victim : retention_entry).

Definition step_entries (state : retention_entries) (step : retention_step) :
  retention_entries :=
  match step with
  | CommitStep next => commit_entries state next
  | EvictStep victim => evict_entry state victim
  end.

Definition step_admissible (state : retention_entries) (step : retention_step) : Prop :=
  match step with
  | CommitStep _ => True
  | EvictStep victim => In victim (historical_entries state)
  end.

Definition step_vertices (state : retention_entries) (step : retention_step) : list nat :=
  match step with
  | CommitStep next => [entry_manifest next; entry_manifest (current_entry state)]
  | EvictStep victim => [entry_manifest victim]
  end.

Record root_delta := RootDelta {
  added_roots : list nat;
  removed_roots : list nat
}.

Definition no_change : root_delta := RootDelta [] [].

Definition apply_delta (roots : counter) (change : root_delta) : counter :=
  remove_roots (add_roots roots (added_roots change)) (removed_roots change).

Definition delta_authorized (roots : counter) (change : root_delta) : bool :=
  authorized (add_roots roots (added_roots change)) (removed_roots change).

Definition canonical_current_delta (state : retention_entries) (step : retention_step) :
  root_delta :=
  match step with
  | CommitStep next =>
      RootDelta [entry_manifest next] [entry_manifest (current_entry state)]
  | EvictStep _ => no_change
  end.

Definition canonical_retained_delta (step : retention_step) : root_delta :=
  match step with
  | CommitStep next => RootDelta [entry_manifest next] []
  | EvictStep victim => RootDelta [] [entry_manifest victim]
  end.

Record composed_delta := ComposedDelta {
  current_delta : root_delta;
  retained_delta : root_delta;
  metadata_delta : root_delta;
  pin_delta : root_delta
}.

Definition canonical_delta (state : retention_entries) (step : retention_step)
  (metadata_change pin_change : root_delta) : composed_delta :=
  ComposedDelta (canonical_current_delta state step) (canonical_retained_delta step)
    metadata_change pin_change.

Definition with_pins (delta : composed_delta) (pin_change : root_delta) : composed_delta :=
  ComposedDelta (current_delta delta) (retained_delta delta) (metadata_delta delta)
    pin_change.

Inductive domain := CurrentDomain | RetainedDomain | MetadataDomain.

Inductive composition_variant :=
| ComposeFaithful
| CurrentDeltaOnRetained
| DeduplicatedRetainedRoots
| PinsAsRetainedRoots.

Record composed_environment := ComposedEnvironment {
  semantic_environment : trial_environment;
  metadata_environment : trial_environment
}.

Record composed_budget := ComposedBudget {
  current_budget : option trial_limits;
  retained_budget : option trial_limits;
  metadata_budget : option trial_limits
}.

Record composed_predecessor := ComposedPredecessor {
  current_base : predecessor;
  retained_base : predecessor;
  metadata_base : predecessor
}.

Record composed_evidence := ComposedEvidence {
  current_prepared : prepared_evidence;
  retained_prepared : prepared_evidence;
  metadata_prepared : prepared_evidence
}.

Inductive composition_outcome :=
| ComposedPrepared (evidence : composed_evidence)
| ComposedRefused (refusing : domain) (why : refusal).

Record composed_result := ComposedResult {
  kept_base : composed_predecessor;
  composed_outcome : composition_outcome
}.

Fixpoint set_insertions (roots : counter) (added : list nat) : list nat :=
  match added with
  | [] => []
  | vertex :: rest =>
      if roots vertex =? 0
      then vertex :: set_insertions (replace roots vertex 1) rest
      else set_insertions roots rest
  end.

Definition retained_delta_of (variant : composition_variant) (base : composed_predecessor)
  (delta : composed_delta) : root_delta :=
  match variant with
  | ComposeFaithful => retained_delta delta
  | CurrentDeltaOnRetained => current_delta delta
  | DeduplicatedRetainedRoots =>
      RootDelta
        (set_insertions (base_roots (retained_base base)) (added_roots (retained_delta delta)))
        (removed_roots (retained_delta delta))
  | PinsAsRetainedRoots =>
      RootDelta (added_roots (retained_delta delta) ++ added_roots (pin_delta delta))
        (removed_roots (retained_delta delta) ++ removed_roots (pin_delta delta))
  end.

Definition domain_trial (env : trial_environment) (budget : option trial_limits)
  (schedule : nat -> bool) (base : predecessor) (change : root_delta) : trial_result :=
  run_trial env Faithful budget schedule base (added_roots change) (removed_roots change).

Definition current_trial (env : composed_environment) (budget : composed_budget)
  (schedule : nat -> bool) (base : composed_predecessor) (delta : composed_delta) :
  trial_result :=
  domain_trial (semantic_environment env) (current_budget budget) schedule
    (current_base base) (current_delta delta).

Definition retained_trial (variant : composition_variant) (env : composed_environment)
  (budget : composed_budget) (schedule : nat -> bool) (base : composed_predecessor)
  (delta : composed_delta) : trial_result :=
  domain_trial (semantic_environment env) (retained_budget budget) schedule
    (retained_base base) (retained_delta_of variant base delta).

Definition metadata_trial (env : composed_environment) (budget : composed_budget)
  (schedule : nat -> bool) (base : composed_predecessor) (delta : composed_delta) :
  trial_result :=
  domain_trial (metadata_environment env) (metadata_budget budget) schedule
    (metadata_base base) (metadata_delta delta).

Definition run_composed (variant : composition_variant) (env : composed_environment)
  (budget : composed_budget) (schedule : nat -> bool) (base : composed_predecessor)
  (delta : composed_delta) : composed_result :=
  match outcome (current_trial env budget schedule base delta) with
  | TrialRefused why => ComposedResult base (ComposedRefused CurrentDomain why)
  | TrialPrepared current_side =>
      match outcome (retained_trial variant env budget schedule base delta) with
      | TrialRefused why => ComposedResult base (ComposedRefused RetainedDomain why)
      | TrialPrepared retained_side =>
          match outcome (metadata_trial env budget schedule base delta) with
          | TrialRefused why => ComposedResult base (ComposedRefused MetadataDomain why)
          | TrialPrepared metadata_side =>
              ComposedResult base
                (ComposedPrepared (ComposedEvidence current_side retained_side metadata_side))
          end
      end
  end.

Definition composed_outcome_spec (first second third : trial_outcome) : composition_outcome :=
  match first, second, third with
  | TrialRefused why, _, _ => ComposedRefused CurrentDomain why
  | TrialPrepared _, TrialRefused why, _ => ComposedRefused RetainedDomain why
  | TrialPrepared _, TrialPrepared _, TrialRefused why => ComposedRefused MetadataDomain why
  | TrialPrepared current_side, TrialPrepared retained_side, TrialPrepared metadata_side =>
      ComposedPrepared (ComposedEvidence current_side retained_side metadata_side)
  end.

Definition as_predecessor (evidence : prepared_evidence) : predecessor :=
  Predecessor (prepared_ledger evidence) (prepared_roots evidence)
    (prepared_objects evidence) (prepared_bytes evidence).

Definition successor_base (evidence : composed_evidence) : composed_predecessor :=
  ComposedPredecessor (as_predecessor (current_prepared evidence))
    (as_predecessor (retained_prepared evidence))
    (as_predecessor (metadata_prepared evidence)).

Definition composed_input_valid (env : composed_environment) (base : composed_predecessor)
  (delta : composed_delta) : Prop :=
  trial_input_valid (semantic_environment env) (current_base base)
    (added_roots (current_delta delta)) (removed_roots (current_delta delta)) /\
  trial_input_valid (semantic_environment env) (retained_base base)
    (added_roots (retained_delta delta)) (removed_roots (retained_delta delta)) /\
  trial_input_valid (metadata_environment env) (metadata_base base)
    (added_roots (metadata_delta delta)) (removed_roots (metadata_delta delta)).

Definition domain_matches_cold (env : trial_environment) (roots : counter)
  (evidence : prepared_evidence) (cold : ledger) : Prop :=
  (forall vertex, counts (prepared_ledger evidence) vertex = counts cold vertex /\
     active (prepared_ledger evidence) vertex = active cold vertex) /\
  prepared_roots evidence = roots /\
  prepared_objects evidence = inventory (universe env) cold unit_weight /\
  prepared_bytes evidence = inventory (universe env) cold (object_bytes env).

Definition domain_base_valid (env : trial_environment) (roots : counter)
  (base : predecessor) : Prop :=
  coherent (base_ledger base) /\
  (forall vertex, base_roots base vertex = roots vertex) /\
  quiescent (graph env) (universe env) (base_roots base) (base_ledger base) /\
  exact_predecessor env base.

Definition composed_base_valid (env : composed_environment) (state : retention_entries)
  (metadata_roots : counter) (base : composed_predecessor) : Prop :=
  domain_base_valid (semantic_environment env) (current_roots state) (current_base base) /\
  domain_base_valid (semantic_environment env) (retained_roots state) (retained_base base) /\
  domain_base_valid (metadata_environment env) metadata_roots (metadata_base base).

Definition physically_protected (retained_ledger metadata_ledger : ledger) (pins : counter)
  (vertex : nat) : bool :=
  active retained_ledger vertex || active metadata_ledger vertex || (0 <? pins vertex).

Definition data_selection (retained_ledger metadata_ledger : ledger) (vertex : nat) : bool :=
  active retained_ledger vertex || active metadata_ledger vertex.

Lemma entry_eqb_true : forall left right, entry_eqb left right = true <-> left = right.
Proof.
  intros [left_generation left_manifest] [right_generation right_manifest].
  unfold entry_eqb. cbn [entry_generation entry_manifest].
  rewrite andb_true_iff, !Nat.eqb_eq. split.
  - intros [same_generation same_manifest]. subst. reflexivity.
  - intros same. split; congruence.
Qed.

Lemma occurrences_app : forall left right query,
  occurrences (left ++ right) query = occurrences left query + occurrences right query.
Proof.
  induction left as [|head tail IH]; intros right query; [reflexivity|].
  cbn [app occurrences]. rewrite IH. lia.
Qed.

Lemma entry_manifests_app : forall left right,
  entry_manifests (left ++ right) = entry_manifests left ++ entry_manifests right.
Proof. intros left right. apply map_app. Qed.

Lemma occurrences_remove_entry : forall victim entries query,
  In victim entries ->
  occurrences (entry_manifests entries) query =
    occurrences (entry_manifests (remove_entry victim entries)) query +
    occurrences [entry_manifest victim] query.
Proof.
  intros victim entries. induction entries as [|entry rest IH]; intros query member;
    [contradiction|].
  cbn [remove_entry]. destruct (entry_eqb entry victim) eqn:same.
  - apply entry_eqb_true in same. subst entry.
    unfold entry_manifests. cbn [map occurrences]. lia.
  - assert (rest_member : In victim rest).
    { destruct member as [equal|rest_member]; [|exact rest_member].
      subst entry. rewrite (proj2 (entry_eqb_true victim victim) eq_refl) in same.
      discriminate same. }
    pose proof (IH query rest_member) as removed.
    unfold entry_manifests in *. cbn [map occurrences] in *. lia.
Qed.

Lemma retained_roots_split : forall state query,
  retained_roots state query =
    occurrences (entry_manifests (historical_entries state)) query +
    current_roots state query.
Proof.
  intros state query. unfold retained_roots, all_entries, current_roots.
  rewrite entry_manifests_app, occurrences_app. reflexivity.
Qed.

Lemma current_roots_below_retained_roots : forall state vertex,
  current_roots state vertex <= retained_roots state vertex.
Proof. intros state vertex. rewrite retained_roots_split. lia. Qed.

Lemma apply_delta_extensional : forall left right change,
  (forall vertex, left vertex = right vertex) ->
  forall vertex, apply_delta left change vertex = apply_delta right change vertex.
Proof.
  intros left right change agreement vertex. unfold apply_delta, remove_roots, add_roots.
  rewrite agreement. reflexivity.
Qed.

Lemma quiescent_roots_extensional : forall children vertices left right state,
  (forall vertex, left vertex = right vertex) ->
  quiescent children vertices left state -> quiescent children vertices right state.
Proof.
  intros children vertices left right state agreement balance vertex.
  rewrite <- (agreement vertex). exact (balance vertex).
Qed.

Lemma reachable_monotone_roots : forall children vertices smaller larger vertex,
  (forall vertex, smaller vertex <= larger vertex) ->
  reachable children vertices smaller vertex -> reachable children vertices larger vertex.
Proof.
  intros children vertices smaller larger vertex below path.
  induction path as [vertex rooted|parent child path IH member edge].
  - apply reachable_root. specialize (below vertex). lia.
  - eapply reachable_child; eauto.
Qed.

Lemma reachable_same_positive_roots : forall children vertices left right vertex,
  (forall vertex, 0 < left vertex <-> 0 < right vertex) ->
  reachable children vertices left vertex -> reachable children vertices right vertex.
Proof.
  intros children vertices left right vertex same path.
  induction path as [vertex rooted|parent child path IH member edge].
  - apply reachable_root. apply same. exact rooted.
  - eapply reachable_child; eauto.
Qed.

Lemma reachable_within_universe : forall env roots vertex,
  edges_closed env ->
  (forall vertex, 0 < roots vertex -> In vertex (universe env)) ->
  reachable (graph env) (universe env) roots vertex -> In vertex (universe env).
Proof.
  intros env roots vertex closed supported path.
  induction path as [vertex rooted|parent child path IH member edge].
  - exact (supported vertex rooted).
  - exact (closed parent child member edge).
Qed.

Lemma cold_support_is_within_universe : forall env rank roots state vertex,
  edges_closed env -> edges_descend env rank ->
  (forall vertex, 0 < roots vertex -> In vertex (universe env)) ->
  coherent state -> quiescent (graph env) (universe env) roots state ->
  active state vertex = true -> In vertex (universe env).
Proof.
  intros env rank roots state vertex closed descending supported agreement balance enabled.
  apply (reachable_within_universe env roots vertex closed supported).
  exact (proj1 (@quiescent_support_is_exact_root_reachability (graph env) (universe env)
    roots state rank agreement balance descending vertex) enabled).
Qed.

Lemma selected_sum_app : forall left right selected weight,
  selected_sum (left ++ right) selected weight =
    selected_sum left selected weight + selected_sum right selected weight.
Proof.
  induction left as [|head tail IH]; intros right selected weight; [reflexivity|].
  cbn [app selected_sum]. rewrite IH. lia.
Qed.

Lemma selected_sum_extensional_on : forall (vertices : list nat) (left right : membership)
  (left_weight right_weight : nat -> nat),
  (forall vertex, In vertex vertices ->
    (if left vertex then left_weight vertex else 0) =
    (if right vertex then right_weight vertex else 0)) ->
  selected_sum vertices left left_weight = selected_sum vertices right right_weight.
Proof.
  induction vertices as [|head tail IH]; intros left right left_weight right_weight agreement;
    [reflexivity|].
  cbn [selected_sum]. rewrite (agreement head (or_introl eq_refl)).
  rewrite (IH left right left_weight right_weight); [reflexivity|].
  intros vertex member. apply agreement. now right.
Qed.

Lemma nodup_app_disjoint : forall (left right : list nat) vertex,
  NoDup (left ++ right) -> In vertex left -> In vertex right -> False.
Proof.
  induction left as [|head tail IH]; intros right vertex unique in_left in_right;
    [contradiction|].
  inversion unique as [|? ? absent tail_unique]; subst.
  destruct in_left as [same|in_tail].
  - subst vertex. apply absent. apply in_or_app. now right.
  - exact (IH right vertex tail_unique in_tail in_right).
Qed.

Lemma faithful_trial_prepared_iff : forall env budget schedule base added removed evidence,
  run_trial env Faithful budget schedule base added removed =
    TrialResult base (TrialPrepared evidence) <->
  outcome (run_trial env Faithful budget schedule base added removed) =
    TrialPrepared evidence.
Proof.
  intros env budget schedule base added removed evidence.
  pose proof (every_trial_retains_the_bit_identical_predecessor env Faithful budget schedule
    base added removed ltac:(discriminate)) as same.
  destruct (run_trial env Faithful budget schedule base added removed) as [kept result].
  cbn [retained outcome] in *. subst kept.
  split; [intros prepared; congruence|intros ->; reflexivity].
Qed.

Lemma run_composed_spec : forall variant env budget schedule base delta,
  run_composed variant env budget schedule base delta =
    ComposedResult base (composed_outcome_spec
      (outcome (current_trial env budget schedule base delta))
      (outcome (retained_trial variant env budget schedule base delta))
      (outcome (metadata_trial env budget schedule base delta))).
Proof.
  intros variant env budget schedule base delta. unfold run_composed, composed_outcome_spec.
  destruct (outcome (current_trial env budget schedule base delta));
    [destruct (outcome (retained_trial variant env budget schedule base delta));
     [destruct (outcome (metadata_trial env budget schedule base delta))|]|];
    reflexivity.
Qed.

Lemma composed_outcome_spec_prepared_iff : forall first second third evidence,
  composed_outcome_spec first second third = ComposedPrepared evidence <->
  first = TrialPrepared (current_prepared evidence) /\
  second = TrialPrepared (retained_prepared evidence) /\
  third = TrialPrepared (metadata_prepared evidence).
Proof.
  intros [current_side|current_why] [retained_side|retained_why]
    [metadata_side|metadata_why] [current_evidence retained_evidence metadata_evidence];
    cbn [composed_outcome_spec current_prepared retained_prepared metadata_prepared];
    (split; [intros hypothesis|intros [first [second third]]]); try discriminate.
  - injection hypothesis as same_current same_retained same_metadata. subst.
    repeat split.
  - injection first as ->. injection second as ->. injection third as ->. reflexivity.
Qed.

Lemma prepared_evidence_is_cold :
  forall env budget schedule base added removed evidence rank,
  trial_input_valid env base added removed -> edges_descend env rank ->
  run_trial env Faithful budget schedule base added removed =
    TrialResult base (TrialPrepared evidence) ->
  coherent (prepared_ledger evidence) /\
  quiescent (graph env) (universe env) (prepared_roots evidence) (prepared_ledger evidence) /\
  exact_predecessor env (as_predecessor evidence) /\
  prepared_roots evidence = remove_roots (add_roots (base_roots base) added) removed.
Proof.
  intros env budget schedule base added removed evidence rank valid descending prepared.
  destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ prepared) as [_ [final [core ->]]].
  destruct (prepared_core_facts _ _ _ _ _ _ _ valid core)
    as [middle [first_execution [second_execution [bounded [_
      [[final_objects final_bytes] _]]]]]].
  pose proof valid as [unique [closed [agreement [balance [_ [added_domain removed_domain]]]]]].
  pose proof (@activation_initial_configuration_is_valid (graph env) (universe env)
    (base_roots base) (base_ledger base) added agreement balance added_domain) as start_valid.
  destruct (@drained_execution_has_exact_root_reachability (graph env) (universe env)
    (add_roots (base_roots base) added) Activation
    (Configuration (base_ledger base) added []) (resident (workspace middle)) rank
    unique closed descending start_valid first_execution)
    as [middle_coherent [middle_quiescent _]].
  pose proof (@deactivation_initial_configuration_is_valid (graph env) (universe env)
    (add_roots (base_roots base) added) (resident (workspace middle)) removed
    middle_coherent middle_quiescent removed_domain bounded) as restart_valid.
  destruct (@drained_execution_has_exact_root_reachability (graph env) (universe env)
    (remove_roots (add_roots (base_roots base) added) removed) Deactivation
    (Configuration (resident (workspace middle)) removed []) (resident (workspace final)) rank
    unique closed descending restart_valid second_execution)
    as [final_coherent [final_quiescent _]].
  cbn [evidence_of as_predecessor prepared_ledger prepared_roots prepared_objects
    prepared_bytes].
  split; [exact final_coherent|]. split; [exact final_quiescent|].
  split; [|reflexivity].
  unfold exact_predecessor. cbn [base_objects base_bytes base_ledger].
  split; [exact final_objects|exact final_bytes].
Qed.

Lemma canonical_deltas_in_universe : forall (member : nat -> Prop) state step,
  Forall member (step_vertices state step) ->
  Forall member (added_roots (canonical_current_delta state step)) /\
  Forall member (removed_roots (canonical_current_delta state step)) /\
  Forall member (added_roots (canonical_retained_delta step)) /\
  Forall member (removed_roots (canonical_retained_delta step)).
Proof.
  intros member state [next|victim] listed; cbn [step_vertices] in listed;
    cbn [canonical_current_delta canonical_retained_delta no_change added_roots
      removed_roots].
  - inversion listed as [|? ? next_member rest]; subst.
    inversion rest as [|? ? current_member _]; subst.
    repeat split; repeat constructor; assumption.
  - inversion listed as [|? ? victim_member _]; subst.
    repeat split; repeat constructor; assumption.
Qed.

Theorem composed_trial_is_the_conjunction_of_domain_trials :
  forall variant env budget schedule base delta,
  kept_base (run_composed variant env budget schedule base delta) = base /\
  composed_outcome (run_composed variant env budget schedule base delta) =
    composed_outcome_spec (outcome (current_trial env budget schedule base delta))
      (outcome (retained_trial variant env budget schedule base delta))
      (outcome (metadata_trial env budget schedule base delta)) /\
  (forall evidence,
    composed_outcome (run_composed variant env budget schedule base delta) =
      ComposedPrepared evidence <->
    current_trial env budget schedule base delta =
      TrialResult (current_base base) (TrialPrepared (current_prepared evidence)) /\
    retained_trial variant env budget schedule base delta =
      TrialResult (retained_base base) (TrialPrepared (retained_prepared evidence)) /\
    metadata_trial env budget schedule base delta =
      TrialResult (metadata_base base) (TrialPrepared (metadata_prepared evidence))).
Proof.
  intros variant env budget schedule base delta.
  rewrite run_composed_spec. cbn [kept_base composed_outcome].
  split; [reflexivity|]. split; [reflexivity|].
  intros evidence. rewrite composed_outcome_spec_prepared_iff.
  unfold current_trial, retained_trial, metadata_trial, domain_trial.
  rewrite !faithful_trial_prepared_iff. reflexivity.
Qed.

Theorem composed_trial_agrees_with_cold_domains :
  forall env budget schedule base delta evidence semantic_rank metadata_rank
    current_cold retained_cold metadata_cold,
  composed_input_valid env base delta ->
  edges_descend (semantic_environment env) semantic_rank ->
  edges_descend (metadata_environment env) metadata_rank ->
  composed_outcome (run_composed ComposeFaithful env budget schedule base delta) =
    ComposedPrepared evidence ->
  coherent current_cold ->
  quiescent (graph (semantic_environment env)) (universe (semantic_environment env))
    (apply_delta (base_roots (current_base base)) (current_delta delta)) current_cold ->
  coherent retained_cold ->
  quiescent (graph (semantic_environment env)) (universe (semantic_environment env))
    (apply_delta (base_roots (retained_base base)) (retained_delta delta)) retained_cold ->
  coherent metadata_cold ->
  quiescent (graph (metadata_environment env)) (universe (metadata_environment env))
    (apply_delta (base_roots (metadata_base base)) (metadata_delta delta)) metadata_cold ->
  domain_matches_cold (semantic_environment env)
    (apply_delta (base_roots (current_base base)) (current_delta delta))
    (current_prepared evidence) current_cold /\
  domain_matches_cold (semantic_environment env)
    (apply_delta (base_roots (retained_base base)) (retained_delta delta))
    (retained_prepared evidence) retained_cold /\
  domain_matches_cold (metadata_environment env)
    (apply_delta (base_roots (metadata_base base)) (metadata_delta delta))
    (metadata_prepared evidence) metadata_cold.
Proof.
  intros env budget schedule base delta evidence semantic_rank metadata_rank current_cold
    retained_cold metadata_cold [current_valid [retained_valid metadata_valid]]
    semantic_descending metadata_descending prepared current_coherent current_quiescent
    retained_coherent retained_quiescent metadata_coherent metadata_quiescent.
  destruct (composed_trial_is_the_conjunction_of_domain_trials ComposeFaithful env budget
    schedule base delta) as [_ [_ prepared_iff]].
  apply prepared_iff in prepared. destruct prepared as [current_run [retained_run metadata_run]].
  unfold current_trial, retained_trial, metadata_trial, domain_trial in *.
  cbn [retained_delta_of] in retained_run.
  split; [|split].
  - exact (prepared_trial_agrees_with_cold_ledger _ _ _ _ _ _ _ _ semantic_rank current_cold
      current_valid semantic_descending current_run current_coherent current_quiescent).
  - exact (prepared_trial_agrees_with_cold_ledger _ _ _ _ _ _ _ _ semantic_rank retained_cold
      retained_valid semantic_descending retained_run retained_coherent retained_quiescent).
  - exact (prepared_trial_agrees_with_cold_ledger _ _ _ _ _ _ _ _ metadata_rank metadata_cold
      metadata_valid metadata_descending metadata_run metadata_coherent metadata_quiescent).
Qed.

Theorem canonical_retention_deltas_realize_entry_roots : forall state step,
  step_admissible state step ->
  (delta_authorized (current_roots state) (canonical_current_delta state step) = true /\
   forall vertex,
     apply_delta (current_roots state) (canonical_current_delta state step) vertex =
     current_roots (step_entries state step) vertex) /\
  (delta_authorized (retained_roots state) (canonical_retained_delta step) = true /\
   forall vertex,
     apply_delta (retained_roots state) (canonical_retained_delta step) vertex =
     retained_roots (step_entries state step) vertex).
Proof.
  intros state [next|victim] admissible; cbn [step_admissible] in admissible.
  - split; split.
    + unfold delta_authorized. apply authorized_iff. intros vertex.
      unfold add_roots, current_roots.
      cbn [canonical_current_delta added_roots removed_roots occurrences]. lia.
    + intros vertex. unfold apply_delta, remove_roots, add_roots, current_roots.
      cbn [canonical_current_delta step_entries commit_entries current_entry added_roots
        removed_roots occurrences]. lia.
    + unfold delta_authorized. apply authorized_iff. intros vertex.
      unfold add_roots. cbn [canonical_retained_delta added_roots removed_roots occurrences].
      lia.
    + intros vertex. unfold apply_delta, remove_roots, add_roots, retained_roots.
      cbn [canonical_retained_delta step_entries commit_entries added_roots removed_roots].
      replace (all_entries (commit_entries state next)) with
        (all_entries state ++ [next]) by reflexivity.
      rewrite entry_manifests_app, occurrences_app.
      change (entry_manifests [next]) with [entry_manifest next].
      cbn [occurrences]. lia.
  - pose proof (occurrences_remove_entry victim (historical_entries state) ) as removal.
    split; split.
    + reflexivity.
    + intros vertex. unfold apply_delta, remove_roots, add_roots, current_roots.
      cbn [canonical_current_delta step_entries evict_entry current_entry no_change
        added_roots removed_roots occurrences]. lia.
    + unfold delta_authorized. apply authorized_iff. intros vertex.
      specialize (removal vertex admissible).
      unfold add_roots. rewrite (retained_roots_split state vertex).
      cbn [canonical_retained_delta added_roots removed_roots occurrences] in *. lia.
    + intros vertex. specialize (removal vertex admissible).
      cbn [step_entries]. unfold apply_delta, remove_roots, add_roots.
      rewrite (retained_roots_split state vertex),
        (retained_roots_split (evict_entry state victim) vertex).
      unfold current_roots.
      cbn [canonical_retained_delta evict_entry historical_entries current_entry added_roots
        removed_roots occurrences] in *.
      lia.
Qed.

Theorem current_closure_is_within_retained_union :
  forall env rank state current_ledger retained_ledger,
  edges_descend env rank ->
  coherent current_ledger ->
  quiescent (graph env) (universe env) (current_roots state) current_ledger ->
  coherent retained_ledger ->
  quiescent (graph env) (universe env) (retained_roots state) retained_ledger ->
  (forall vertex, active current_ledger vertex = true -> active retained_ledger vertex = true) /\
  (forall weight, inventory (universe env) retained_ledger weight =
    inventory (universe env) current_ledger weight +
    selected_sum (universe env)
      (fun vertex => active retained_ledger vertex && negb (active current_ledger vertex))
      weight).
Proof.
  intros env rank state current_ledger retained_ledger descending current_coherent
    current_quiescent retained_coherent retained_quiescent.
  assert (within : forall vertex, active current_ledger vertex = true ->
    active retained_ledger vertex = true).
  { intros vertex current_active.
    apply (proj2 (@quiescent_support_is_exact_root_reachability (graph env) (universe env)
      (retained_roots state) retained_ledger rank retained_coherent retained_quiescent
      descending vertex)).
    apply (reachable_monotone_roots (graph env) (universe env) (current_roots state)).
    - apply current_roots_below_retained_roots.
    - exact (proj1 (@quiescent_support_is_exact_root_reachability (graph env) (universe env)
        (current_roots state) current_ledger rank current_coherent current_quiescent
        descending vertex) current_active). }
  split; [exact within|].
  intros weight. unfold inventory.
  exact (selected_sum_partition (universe env) (active retained_ledger) (active current_ledger)
    weight within).
Qed.

Theorem evicting_one_root_alias_keeps_retained_support :
  forall env rank state victim before after,
  In victim (historical_entries state) ->
  In (entry_manifest victim) (entry_manifests (all_entries (evict_entry state victim))) ->
  edges_descend env rank ->
  coherent before -> quiescent (graph env) (universe env) (retained_roots state) before ->
  coherent after ->
  quiescent (graph env) (universe env) (retained_roots (evict_entry state victim)) after ->
  (forall vertex, active before vertex = active after vertex) /\
  (forall weight,
    inventory (universe env) before weight = inventory (universe env) after weight).
Proof.
  intros env rank state victim before after member alias descending before_coherent
    before_quiescent after_coherent after_quiescent.
  assert (split_roots : forall vertex,
    retained_roots state vertex =
      retained_roots (evict_entry state victim) vertex +
      occurrences [entry_manifest victim] vertex).
  { intros vertex.
    pose proof (occurrences_remove_entry victim (historical_entries state) vertex member)
      as removal.
    rewrite (retained_roots_split state vertex),
      (retained_roots_split (evict_entry state victim) vertex).
    unfold current_roots. cbn [evict_entry historical_entries current_entry]. lia. }
  assert (alias_rooted : 0 < retained_roots (evict_entry state victim) (entry_manifest victim)).
  { unfold retained_roots. apply (occurrences_positive (graph env)). exact alias. }
  assert (same_positive : forall vertex,
    0 < retained_roots state vertex <-> 0 < retained_roots (evict_entry state victim) vertex).
  { intros vertex. rewrite (split_roots vertex). cbn [occurrences].
    destruct (Nat.eqb vertex (entry_manifest victim)) eqn:same.
    - apply Nat.eqb_eq in same. subst vertex. split; intros; lia.
    - split; intros; lia. }
  assert (same_support : forall vertex, active before vertex = active after vertex).
  { intros vertex.
    pose proof (@quiescent_support_is_exact_root_reachability (graph env) (universe env)
      (retained_roots state) before rank before_coherent before_quiescent descending vertex)
      as before_support.
    pose proof (@quiescent_support_is_exact_root_reachability (graph env) (universe env)
      (retained_roots (evict_entry state victim)) after rank after_coherent after_quiescent
      descending vertex) as after_support.
    assert (forward : reachable (graph env) (universe env) (retained_roots state) vertex ->
      reachable (graph env) (universe env) (retained_roots (evict_entry state victim)) vertex).
    { apply reachable_same_positive_roots. exact same_positive. }
    assert (backward :
      reachable (graph env) (universe env) (retained_roots (evict_entry state victim)) vertex ->
      reachable (graph env) (universe env) (retained_roots state) vertex).
    { apply reachable_same_positive_roots. intros query. symmetry. apply same_positive. }
    destruct (active before vertex), (active after vertex); intuition congruence. }
  split; [exact same_support|].
  intros weight. unfold inventory. apply selected_sum_extensional. exact same_support.
Qed.

Theorem data_inventory_is_disjoint_retained_plus_metadata :
  forall semantic metadata semantic_rank metadata_rank semantic_roots metadata_roots
    retained_ledger metadata_ledger data_weight semantic_weight metadata_weight,
  NoDup (universe semantic ++ universe metadata) ->
  edges_closed semantic -> edges_descend semantic semantic_rank ->
  edges_closed metadata -> edges_descend metadata metadata_rank ->
  (forall vertex, 0 < semantic_roots vertex -> In vertex (universe semantic)) ->
  (forall vertex, 0 < metadata_roots vertex -> In vertex (universe metadata)) ->
  coherent retained_ledger ->
  quiescent (graph semantic) (universe semantic) semantic_roots retained_ledger ->
  coherent metadata_ledger ->
  quiescent (graph metadata) (universe metadata) metadata_roots metadata_ledger ->
  (forall vertex, In vertex (universe semantic) -> data_weight vertex = semantic_weight vertex) ->
  (forall vertex, In vertex (universe metadata) -> data_weight vertex = metadata_weight vertex) ->
  (forall vertex, active retained_ledger vertex = true ->
    active metadata_ledger vertex = true -> False) /\
  selected_sum (universe semantic ++ universe metadata)
    (data_selection retained_ledger metadata_ledger) data_weight =
  inventory (universe semantic) retained_ledger semantic_weight +
  inventory (universe metadata) metadata_ledger metadata_weight.
Proof.
  intros semantic metadata semantic_rank metadata_rank semantic_roots metadata_roots
    retained_ledger metadata_ledger data_weight semantic_weight metadata_weight unique
    semantic_closed semantic_descending metadata_closed metadata_descending
    semantic_supported metadata_supported retained_coherent retained_quiescent
    metadata_coherent metadata_quiescent semantic_weights metadata_weights.
  assert (retained_member : forall vertex, active retained_ledger vertex = true ->
    In vertex (universe semantic)).
  { intros vertex enabled.
    exact (cold_support_is_within_universe semantic semantic_rank semantic_roots
      retained_ledger vertex semantic_closed semantic_descending semantic_supported
      retained_coherent retained_quiescent enabled). }
  assert (metadata_member : forall vertex, active metadata_ledger vertex = true ->
    In vertex (universe metadata)).
  { intros vertex enabled.
    exact (cold_support_is_within_universe metadata metadata_rank metadata_roots
      metadata_ledger vertex metadata_closed metadata_descending metadata_supported
      metadata_coherent metadata_quiescent enabled). }
  split.
  - intros vertex in_retained in_metadata.
    exact (nodup_app_disjoint (universe semantic) (universe metadata) vertex unique
      (retained_member vertex in_retained) (metadata_member vertex in_metadata)).
  - rewrite selected_sum_app. unfold inventory. f_equal.
    + apply selected_sum_extensional_on. intros vertex member.
      unfold data_selection. rewrite (semantic_weights vertex member).
      destruct (active metadata_ledger vertex) eqn:in_metadata.
      * exfalso. exact (nodup_app_disjoint (universe semantic) (universe metadata) vertex
          unique member (metadata_member vertex in_metadata)).
      * rewrite orb_false_r. reflexivity.
    + apply selected_sum_extensional_on. intros vertex member.
      unfold data_selection. rewrite (metadata_weights vertex member).
      destruct (active retained_ledger vertex) eqn:in_retained.
      * exfalso. exact (nodup_app_disjoint (universe semantic) (universe metadata) vertex
          unique (retained_member vertex in_retained) member).
      * reflexivity.
Qed.

Theorem pins_protect_physically_without_changing_semantic_evidence :
  forall env budget schedule base delta pin_change,
  run_composed ComposeFaithful env budget schedule base (with_pins delta pin_change) =
    run_composed ComposeFaithful env budget schedule base delta /\
  (forall retained_ledger metadata_ledger pins vertex,
    physically_protected retained_ledger metadata_ledger pins vertex = true <->
    active retained_ledger vertex = true \/ active metadata_ledger vertex = true \/
      0 < pins vertex).
Proof.
  intros env budget schedule base delta pin_change. split.
  - reflexivity.
  - intros retained_ledger metadata_ledger pins vertex. unfold physically_protected.
    rewrite !orb_true_iff, Nat.ltb_lt. tauto.
Qed.

Theorem canonical_composed_step_preserves_entry_domains :
  forall env budget schedule base state step metadata_roots metadata_change pin_change
    semantic_rank metadata_rank evidence,
  NoDup (universe (semantic_environment env)) -> edges_closed (semantic_environment env) ->
  edges_descend (semantic_environment env) semantic_rank ->
  NoDup (universe (metadata_environment env)) -> edges_closed (metadata_environment env) ->
  edges_descend (metadata_environment env) metadata_rank ->
  composed_base_valid env state metadata_roots base ->
  step_admissible state step ->
  Forall (fun vertex => In vertex (universe (semantic_environment env)))
    (step_vertices state step) ->
  Forall (fun vertex => In vertex (universe (metadata_environment env)))
    (added_roots metadata_change) ->
  Forall (fun vertex => In vertex (universe (metadata_environment env)))
    (removed_roots metadata_change) ->
  composed_outcome (run_composed ComposeFaithful env budget schedule base
    (canonical_delta state step metadata_change pin_change)) = ComposedPrepared evidence ->
  composed_base_valid env (step_entries state step)
    (apply_delta metadata_roots metadata_change) (successor_base evidence) /\
  (forall vertex, active (prepared_ledger (current_prepared evidence)) vertex = true ->
    active (prepared_ledger (retained_prepared evidence)) vertex = true) /\
  (forall weight,
    inventory (universe (semantic_environment env))
      (prepared_ledger (retained_prepared evidence)) weight =
    inventory (universe (semantic_environment env))
      (prepared_ledger (current_prepared evidence)) weight +
    selected_sum (universe (semantic_environment env))
      (fun vertex => active (prepared_ledger (retained_prepared evidence)) vertex &&
        negb (active (prepared_ledger (current_prepared evidence)) vertex)) weight).
Proof.
  intros env budget schedule base state step metadata_roots metadata_change pin_change
    semantic_rank metadata_rank evidence semantic_unique semantic_closed semantic_descending
    metadata_unique metadata_closed metadata_descending
    [[current_coherent [current_roots_exact [current_quiescent current_counted]]]
     [[retained_coherent [retained_roots_exact [retained_quiescent retained_counted]]]
      [metadata_coherent [metadata_roots_exact [metadata_quiescent metadata_counted]]]]]
    admissible step_domain metadata_added_domain metadata_removed_domain prepared.
  destruct (canonical_deltas_in_universe _ state step step_domain)
    as [current_added_domain [current_removed_domain
      [retained_added_domain retained_removed_domain]]].
  destruct (canonical_retention_deltas_realize_entry_roots state step admissible)
    as [[_ current_realized] [_ retained_realized]].
  destruct (composed_trial_is_the_conjunction_of_domain_trials ComposeFaithful env budget
    schedule base (canonical_delta state step metadata_change pin_change))
    as [_ [_ prepared_iff]].
  apply prepared_iff in prepared. destruct prepared as [current_run [retained_run metadata_run]].
  unfold current_trial, retained_trial, metadata_trial, domain_trial in *.
  cbn [canonical_delta current_delta retained_delta metadata_delta retained_delta_of] in *.
  assert (current_valid : trial_input_valid (semantic_environment env) (current_base base)
    (added_roots (canonical_current_delta state step))
    (removed_roots (canonical_current_delta state step))).
  { exact (conj semantic_unique (conj semantic_closed (conj current_coherent
      (conj current_quiescent (conj current_counted
        (conj current_added_domain current_removed_domain)))))). }
  assert (retained_valid : trial_input_valid (semantic_environment env) (retained_base base)
    (added_roots (canonical_retained_delta step))
    (removed_roots (canonical_retained_delta step))).
  { exact (conj semantic_unique (conj semantic_closed (conj retained_coherent
      (conj retained_quiescent (conj retained_counted
        (conj retained_added_domain retained_removed_domain)))))). }
  assert (metadata_valid : trial_input_valid (metadata_environment env) (metadata_base base)
    (added_roots metadata_change) (removed_roots metadata_change)).
  { exact (conj metadata_unique (conj metadata_closed (conj metadata_coherent
      (conj metadata_quiescent (conj metadata_counted
        (conj metadata_added_domain metadata_removed_domain)))))). }
  destruct (prepared_evidence_is_cold _ _ _ _ _ _ _ semantic_rank current_valid
    semantic_descending current_run)
    as [current_after_coherent [current_after_quiescent [current_after_counted
      current_after_roots]]].
  destruct (prepared_evidence_is_cold _ _ _ _ _ _ _ semantic_rank retained_valid
    semantic_descending retained_run)
    as [retained_after_coherent [retained_after_quiescent [retained_after_counted
      retained_after_roots]]].
  destruct (prepared_evidence_is_cold _ _ _ _ _ _ _ metadata_rank metadata_valid
    metadata_descending metadata_run)
    as [metadata_after_coherent [metadata_after_quiescent [metadata_after_counted
      metadata_after_roots]]].
  assert (current_after_exact : forall vertex,
    prepared_roots (current_prepared evidence) vertex =
      current_roots (step_entries state step) vertex).
  { intros vertex. rewrite current_after_roots.
    rewrite <- (current_realized vertex).
    exact (apply_delta_extensional _ _ (canonical_current_delta state step)
      current_roots_exact vertex). }
  assert (retained_after_exact : forall vertex,
    prepared_roots (retained_prepared evidence) vertex =
      retained_roots (step_entries state step) vertex).
  { intros vertex. rewrite retained_after_roots.
    rewrite <- (retained_realized vertex).
    exact (apply_delta_extensional _ _ (canonical_retained_delta step)
      retained_roots_exact vertex). }
  assert (metadata_after_exact : forall vertex,
    prepared_roots (metadata_prepared evidence) vertex =
      apply_delta metadata_roots metadata_change vertex).
  { intros vertex. rewrite metadata_after_roots.
    exact (apply_delta_extensional _ _ metadata_change metadata_roots_exact vertex). }
  destruct (current_closure_is_within_retained_union (semantic_environment env) semantic_rank
    (step_entries state step) (prepared_ledger (current_prepared evidence))
    (prepared_ledger (retained_prepared evidence)) semantic_descending
    current_after_coherent
    (quiescent_roots_extensional _ _ _ _ _ current_after_exact current_after_quiescent)
    retained_after_coherent
    (quiescent_roots_extensional _ _ _ _ _ retained_after_exact retained_after_quiescent))
    as [within partition].
  split; [|split; [exact within|exact partition]].
  unfold composed_base_valid, domain_base_valid, successor_base.
  cbn [current_base retained_base metadata_base as_predecessor base_ledger base_roots].
  split; [|split].
  - split; [exact current_after_coherent|].
    split; [exact current_after_exact|].
    split; [exact current_after_quiescent|exact current_after_counted].
  - split; [exact retained_after_coherent|].
    split; [exact retained_after_exact|].
    split; [exact retained_after_quiescent|exact retained_after_counted].
  - split; [exact metadata_after_coherent|].
    split; [exact metadata_after_exact|].
    split; [exact metadata_after_quiescent|exact metadata_after_counted].
Qed.

Definition unbounded_budget : composed_budget := ComposedBudget None None None.

Definition established (env : trial_environment) (root_events : list nat) : predecessor :=
  match outcome (run_trial env Faithful None prefer_cursors empty_predecessor root_events [])
  with
  | TrialPrepared evidence => as_predecessor evidence
  | TrialRefused _ => empty_predecessor
  end.

Definition composition_fixture : composed_environment :=
  ComposedEnvironment (fixture single_edge [0; 1; 2; 3] no_reads)
    (fixture no_edges [9] no_reads).

Definition fixture_base (current_events retained_events : list nat) : composed_predecessor :=
  ComposedPredecessor (established (semantic_environment composition_fixture) current_events)
    (established (semantic_environment composition_fixture) retained_events)
    (established (metadata_environment composition_fixture) [9]).

Definition retained_union_objects (result : composed_result) : option nat :=
  match composed_outcome result with
  | ComposedPrepared evidence => Some (prepared_objects (retained_prepared evidence))
  | ComposedRefused _ _ => None
  end.

Definition continue_from (variant : composition_variant) (env : composed_environment)
  (budget : composed_budget) (schedule : nat -> bool) (result : composed_result)
  (delta : composed_delta) : composed_result :=
  match composed_outcome result with
  | ComposedPrepared evidence =>
      run_composed variant env budget schedule (successor_base evidence) delta
  | ComposedRefused _ _ => result
  end.

Definition first_entries (manifest : nat) : retention_entries :=
  RetentionEntries (RetentionEntry 0 manifest) [].

Definition sign_step : retention_step := CommitStep (RetentionEntry 1 2).

Definition sign_delta : composed_delta :=
  canonical_delta (first_entries 1) sign_step no_change no_change.

Definition alias_commit_step : retention_step := CommitStep (RetentionEntry 1 1).

Definition alias_evict_step : retention_step := EvictStep (RetentionEntry 0 1).

Definition alias_commit : composed_delta :=
  canonical_delta (first_entries 1) alias_commit_step no_change no_change.

Definition alias_evict : composed_delta :=
  canonical_delta (step_entries (first_entries 1) alias_commit_step) alias_evict_step
    no_change no_change.

Definition pin_only_delta : composed_delta :=
  ComposedDelta no_change no_change no_change (RootDelta [3] []).

Theorem retained_sign_mutant_drops_the_predecessor_closure :
  retained_union_objects (run_composed ComposeFaithful composition_fixture unbounded_budget
    prefer_cursors (fixture_base [1] [1]) sign_delta) = Some 3 /\
  retained_union_objects (run_composed CurrentDeltaOnRetained composition_fixture
    unbounded_budget prefer_cursors (fixture_base [1] [1]) sign_delta) = Some 1 /\
  retained_roots (step_entries (first_entries 1) sign_step) 1 = 1.
Proof. vm_compute. repeat split. Qed.

Theorem deduplicated_alias_mutant_drops_a_shared_closure :
  retained_union_objects (continue_from ComposeFaithful composition_fixture unbounded_budget
    prefer_cursors (run_composed ComposeFaithful composition_fixture unbounded_budget
      prefer_cursors (fixture_base [1] [1]) alias_commit) alias_evict) = Some 2 /\
  retained_union_objects (continue_from DeduplicatedRetainedRoots composition_fixture
    unbounded_budget prefer_cursors (run_composed DeduplicatedRetainedRoots
      composition_fixture unbounded_budget prefer_cursors (fixture_base [1] [1]) alias_commit)
    alias_evict) = Some 0 /\
  retained_roots (step_entries (step_entries (first_entries 1) alias_commit_step)
    alias_evict_step) 1 = 1.
Proof. vm_compute. repeat split. Qed.

Theorem pinned_root_mutant_inflates_the_retained_union :
  retained_union_objects (run_composed ComposeFaithful composition_fixture unbounded_budget
    prefer_cursors (fixture_base [2] [2]) pin_only_delta) = Some 1 /\
  retained_union_objects (run_composed PinsAsRetainedRoots composition_fixture
    unbounded_budget prefer_cursors (fixture_base [2] [2]) pin_only_delta) = Some 2 /\
  match composed_outcome (run_composed ComposeFaithful composition_fixture unbounded_budget
    prefer_cursors (fixture_base [2] [2]) pin_only_delta) with
  | ComposedPrepared evidence =>
      active (prepared_ledger (retained_prepared evidence)) 3 = false /\
      physically_protected (prepared_ledger (retained_prepared evidence))
        (prepared_ledger (metadata_prepared evidence)) (occurrences [3]) 3 = true
  | ComposedRefused _ _ => False
  end.
Proof. vm_compute. repeat split. Qed.

End CheckedDomainComposition.

Print Assumptions CheckedDomainComposition.composed_trial_is_the_conjunction_of_domain_trials.
Print Assumptions CheckedDomainComposition.composed_trial_agrees_with_cold_domains.
Print Assumptions CheckedDomainComposition.canonical_retention_deltas_realize_entry_roots.
Print Assumptions CheckedDomainComposition.current_closure_is_within_retained_union.
Print Assumptions CheckedDomainComposition.evicting_one_root_alias_keeps_retained_support.
Print Assumptions CheckedDomainComposition.data_inventory_is_disjoint_retained_plus_metadata.
Print Assumptions CheckedDomainComposition.pins_protect_physically_without_changing_semantic_evidence.
Print Assumptions CheckedDomainComposition.canonical_composed_step_preserves_entry_domains.
Print Assumptions CheckedDomainComposition.retained_sign_mutant_drops_the_predecessor_closure.
Print Assumptions CheckedDomainComposition.deduplicated_alias_mutant_drops_a_shared_closure.
Print Assumptions CheckedDomainComposition.pinned_root_mutant_inflates_the_retained_union.
