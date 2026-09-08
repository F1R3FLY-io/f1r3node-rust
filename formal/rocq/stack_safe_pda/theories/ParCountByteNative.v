From Stdlib Require Import List Arith Bool Lia ZArith.
From StackSafePDA Require Import SpatialMatcher.

Import ListNotations.
Import ParCountIntervals.
Set Implicit Arguments.

Module ParCountByteNative.

Inductive family := Sends | Receives | News | Exprs | Matches | Unforgeables | Bundles | Conditionals.

Definition family_eq_dec : forall left right : family, {left = right} + {left <> right}.
Proof. decide equality. Defined.

Definition all_families := [Sends; Receives; News; Exprs; Matches; Unforgeables; Bundles; Conditionals].

Inductive expression :=
| FreeExpression (level : Z)
| WildcardExpression
| BoundExpression (index : Z)
| AbsentExpression
| AbsentEVar
| AbsentVar
| OtherExpression (schema_form : nat).

Definition expression_is_hole expr :=
  match expr with FreeExpression _ | WildcardExpression => true | _ => false end.

Inductive component := Ground (kind : family) | Expression (expr : expression) | ConnectiveEndpoint.
Definition endpoint := (component * nat)%type.

Definition component_family value : option family :=
  match value with Ground kind => Some kind | Expression _ => Some Exprs | ConnectiveEndpoint => None end.

Definition component_is_hole value :=
  match value with Expression expr => expression_is_hole expr | _ => false end.

Definition component_belongs kind value :=
  match component_family value with
  | Some found => if family_eq_dec found kind then true else false
  | None => false
  end.

Definition resident_mass kind (entries : list endpoint) := family_mass (component_belongs kind) entries.
Definition hole_mass (entries : list endpoint) := family_mass component_is_hole entries.
Definition nonfree_mass kind (entries : list endpoint) :=
  family_mass (fun value => component_belongs kind value && negb (component_is_hole value)) entries.

Definition counts := family -> count_interval.
Definition constant_counts interval : counts := fun _ => interval.
Definition exact_zero := CountInterval (Finite 0) (Finite 0).
Definition zero_counts := constant_counts exact_zero.
Definition any_counts := constant_counts (CountInterval (Finite 0) Unbounded).
Definition scalar_counts : counts := fun kind =>
  if family_eq_dec kind Exprs then CountInterval (Finite 1) (Finite 1) else exact_zero.

Definition base_projection (entries : list endpoint) : counts := fun kind =>
  CountInterval (Finite (nonfree_mass kind entries))
    (if hole_mass entries =? 0 then Finite (nonfree_mass kind entries) else Unbounded).

Inductive source_field := CountedField (kind : family) | ConnectiveField.

Record encoded_endpoint := EncodedEndpoint {
  endpoint_field : source_field;
  endpoint_body : list nat;
  endpoint_multiplicity : nat
}.

Fixpoint project_encoded (view : source_field -> list nat -> option component) (wire : list encoded_endpoint)
    : option (list endpoint) :=
  match wire with
  | [] => Some []
  | entry :: rest =>
      match view (endpoint_field entry) (endpoint_body entry), project_encoded view rest with
      | Some value, Some tail => Some ((value, endpoint_multiplicity entry) :: tail)
      | _, _ => None
      end
  end.

Definition projection_admission (decoder view : source_field -> list nat -> option component)
    (wire : list encoded_endpoint) (resident : list endpoint) :=
  Forall2 (fun encoded decoded =>
    decoder (endpoint_field encoded) (endpoint_body encoded) = Some (fst decoded) /\
    endpoint_multiplicity encoded = snd decoded) wire resident /\
  (forall entry, In entry wire -> view (endpoint_field entry) (endpoint_body entry) =
    decoder (endpoint_field entry) (endpoint_body entry)).

Lemma admitted_projection_is_exact : forall decoder view wire resident,
  projection_admission decoder view wire resident -> project_encoded view wire = Some resident.
Proof.
  intros decoder view wire resident [decoded agrees].
  induction decoded as [|encoded [value multiplicity] wire resident [body amount] decoded IH]; simpl; auto.
  rewrite agrees by (left; reflexivity). simpl in *. rewrite body, amount.
  rewrite IH; [reflexivity|]. intros entry present. apply agrees. right; assumption.
Qed.

Theorem encoded_projection_preserves_all_eight_fields_and_hole_mass :
  forall decoder view wire resident,
  projection_admission decoder view wire resident ->
  exists projected, project_encoded view wire = Some projected /\
    hole_mass projected = hole_mass resident /\
    (forall kind, In kind all_families /\ resident_mass kind projected = resident_mass kind resident /\
      nonfree_mass kind projected = nonfree_mass kind resident /\
      base_projection projected kind = base_projection resident kind).
Proof.
  intros decoder view wire resident admitted. exists resident.
  split; [eapply admitted_projection_is_exact; eauto|]. split; [reflexivity|].
  intro kind. split; [destruct kind; simpl; tauto|]. repeat split; reflexivity.
Qed.

Fixpoint expand_occurrences (entries : list endpoint) : list endpoint :=
  match entries with
  | [] => []
  | (value, amount) :: rest => repeat (value, 1) amount ++ expand_occurrences rest
  end.

Lemma repeated_family_mass : forall (belongs : component -> bool) value amount,
  family_mass belongs (repeat (value, 1) amount) = if belongs value then amount else 0.
Proof.
  intros belongs value amount. induction amount; unfold family_mass, endpoint_mass, count_mass in *;
    simpl in *; destruct (belongs value); simpl in *; lia.
Qed.

Lemma expanded_family_mass : forall (belongs : component -> bool) entries,
  family_mass belongs (expand_occurrences entries) = family_mass belongs entries.
Proof.
  intros belongs entries. induction entries as [|[value amount] rest IH]; simpl; [reflexivity|].
  rewrite (proj1 (exact_family_total_is_endpoint_mass belongs _ _)), repeated_family_mass, IH.
  unfold family_mass, endpoint_mass, count_mass. simpl. destruct (belongs value); simpl; lia.
Qed.

Theorem duplicate_occurrences_and_compressed_endpoints_have_equal_projection : forall entries kind,
  resident_mass kind (expand_occurrences entries) = resident_mass kind entries /\
  hole_mass (expand_occurrences entries) = hole_mass entries /\
  base_projection (expand_occurrences entries) kind = base_projection entries kind.
Proof.
  intros. unfold base_projection, resident_mass, hole_mass, nonfree_mass.
  rewrite !expanded_family_mass. repeat split; reflexivity.
Qed.

Inductive proof_class := ProvenHole | ProvenConcrete | Unproved.
Inductive decoded_expression := DecodedExpression (value : expression) | ExpressionFailure (error : nat).
Inductive hole_result := HoleResult (hole : bool) | HoleFailure (error : nat).

Definition decoder_result decoded :=
  match decoded with
  | DecodedExpression value => HoleResult (expression_is_hole value)
  | ExpressionFailure error => HoleFailure error
  end.

Definition resolve_classification classified decoded :=
  match classified with
  | ProvenHole => HoleResult true
  | ProvenConcrete => HoleResult false
  | Unproved => decoder_result decoded
  end.

Definition class_sound classified decoded :=
  match classified with
  | ProvenHole => decoder_result decoded = HoleResult true
  | ProvenConcrete => decoder_result decoded = HoleResult false
  | Unproved => True
  end.

Definition semantic_classification value :=
  if expression_is_hole value then ProvenHole else ProvenConcrete.

Definition decoder_calls classified := match classified with Unproved => 1 | _ => 0 end.

Theorem tri_state_refinement_preserves_decoder_results_and_failure_authority :
  (forall classified decoded, class_sound classified decoded ->
    resolve_classification classified decoded = decoder_result decoded) /\
  (forall decoded, resolve_classification Unproved decoded = decoder_result decoded /\
    decoder_calls Unproved = 1) /\
  (forall error, resolve_classification Unproved (ExpressionFailure error) = HoleFailure error) /\
  (forall classified error, class_sound classified (ExpressionFailure error) -> classified = Unproved).
Proof.
  repeat split; intros; try reflexivity; destruct classified; simpl in *; try discriminate; auto.
Qed.

Theorem admitted_expression_forms_have_complete_zero_fallback_classification :
  (forall value, class_sound (semantic_classification value) (DecodedExpression value) /\
    semantic_classification value <> Unproved /\ decoder_calls (semantic_classification value) = 0) /\
  (forall level, semantic_classification (FreeExpression level) = ProvenHole /\
    semantic_classification (BoundExpression level) = ProvenConcrete) /\
  semantic_classification WildcardExpression = ProvenHole /\
  semantic_classification AbsentExpression = ProvenConcrete /\
  semantic_classification AbsentEVar = ProvenConcrete /\
  semantic_classification AbsentVar = ProvenConcrete.
Proof.
  repeat split; intros; try reflexivity; destruct value; simpl; try reflexivity; discriminate.
Qed.

Definition combine_counts operator (left right : counts) : counts :=
  fun kind => interval_combine operator (left kind) (right kind).

Definition scale_interval factor interval :=
  CountInterval (bound_scale factor (lower_bound interval)) (bound_scale factor (upper_bound interval)).

Definition scale_counts factor (value : counts) : counts := fun kind => scale_interval factor (value kind).

Inductive count_tree :=
| CountLeaf (value : counts)
| CountBranch (operator : interval_operator) (seed : counts) (children : count_forest)
| CountScale (factor : nat) (child : count_tree)
with count_forest :=
| CountNil
| CountCons (child : count_tree) (rest : count_forest).

Fixpoint structural_tree subject : counts :=
  match subject with
  | CountLeaf value => value
  | CountBranch operator seed children => structural_forest operator seed children
  | CountScale factor child => scale_counts factor (structural_tree child)
  end
with structural_forest operator accumulator children : counts :=
  match children with
  | CountNil => accumulator
  | CountCons child rest =>
      structural_forest operator (combine_counts operator accumulator (structural_tree child)) rest
  end.

Definition par_tree (metadata : bool) entries children := CountBranch ParallelSum (base_projection entries) children.
Definition and_tree children := CountBranch Conjunction any_counts children.
Definition or_tree children := CountBranch Disjunction (constant_counts (CountInterval Unbounded (Finite 0))) children.
Definition not_tree (ignored_child : count_tree) := CountLeaf any_counts.
Definition predicate_tree (tag : nat) (payload : bool) := CountLeaf scalar_counts.
Definition var_ref_tree (index depth : Z) := CountLeaf zero_counts.
Definition absent_connective_tree := CountLeaf zero_counts.

Theorem structural_connective_equations_ignore_metadata_but_not_children :
  (forall entries children kind, structural_tree (par_tree true entries children) kind =
    structural_tree (par_tree false entries children) kind) /\
  (forall kind, structural_tree (and_tree CountNil) kind = CountInterval (Finite 0) Unbounded /\
    structural_tree (or_tree CountNil) kind = CountInterval Unbounded (Finite 0)) /\
  (forall child kind, structural_tree (not_tree child) kind = CountInterval (Finite 0) Unbounded) /\
  (forall tag payload kind, structural_tree (predicate_tree tag payload) kind = scalar_counts kind) /\
  (forall index depth kind, structural_tree (var_ref_tree index depth) kind = exact_zero /\
    structural_tree absent_connective_tree kind = exact_zero).
Proof. repeat split; reflexivity. Qed.

Inductive continuation :=
| ResumeCursor (operator : interval_operator) (accumulator : counts) (remaining : count_forest)
| ScaleResult (factor : nat).

Inductive machine_state :=
| Visit (subject : count_tree) (stack : list continuation)
| Scan (operator : interval_operator) (accumulator : counts) (remaining : count_forest)
    (stack : list continuation)
| Return (value : counts) (stack : list continuation).

Definition cursor_step state : option machine_state :=
  match state with
  | Visit (CountLeaf value) stack => Some (Return value stack)
  | Visit (CountBranch operator seed children) stack => Some (Scan operator seed children stack)
  | Visit (CountScale factor child) stack => Some (Visit child (ScaleResult factor :: stack))
  | Scan operator accumulator CountNil stack => Some (Return accumulator stack)
  | Scan operator accumulator (CountCons child rest) stack =>
      Some (Visit child (ResumeCursor operator accumulator rest :: stack))
  | Return value (ResumeCursor operator accumulator remaining :: stack) =>
      Some (Scan operator (combine_counts operator accumulator value) remaining stack)
  | Return value (ScaleResult factor :: stack) => Some (Return (scale_counts factor value) stack)
  | Return _ [] => None
  end.

Fixpoint continuation_value stack (value : counts) : counts :=
  match stack with
  | [] => value
  | ResumeCursor operator accumulator remaining :: tail =>
      continuation_value tail (structural_forest operator (combine_counts operator accumulator value) remaining)
  | ScaleResult factor :: tail => continuation_value tail (scale_counts factor value)
  end.

Definition state_value state : counts :=
  match state with
  | Visit subject stack => continuation_value stack (structural_tree subject)
  | Scan operator accumulator remaining stack => continuation_value stack (structural_forest operator accumulator remaining)
  | Return value stack => continuation_value stack value
  end.

Theorem suspended_cursor_step_preserves_the_structural_fold : forall state next,
  cursor_step state = Some next -> state_value next = state_value state.
Proof.
  intros state next step. destruct state as [subject stack|operator accumulator remaining stack|value stack];
    [destruct subject|destruct remaining|destruct stack as [|frame stack]; [|destruct frame]];
    simpl in step; inversion step; reflexivity.
Qed.

Fixpoint tree_potential subject : nat :=
  match subject with
  | CountLeaf _ => 1
  | CountBranch _ _ children => S (forest_potential children)
  | CountScale _ child => S (S (tree_potential child))
  end
with forest_potential children : nat :=
  match children with
  | CountNil => 1
  | CountCons child rest => S (S (tree_potential child + forest_potential rest))
  end.

Fixpoint stack_potential stack : nat :=
  match stack with
  | [] => 0
  | ResumeCursor _ _ remaining :: tail => S (forest_potential remaining + stack_potential tail)
  | ScaleResult _ :: tail => S (stack_potential tail)
  end.

Definition state_potential state :=
  match state with
  | Visit subject stack => tree_potential subject + stack_potential stack
  | Scan _ _ remaining stack => forest_potential remaining + stack_potential stack
  | Return _ stack => stack_potential stack
  end.

Lemma cursor_step_decreases_potential : forall state next,
  cursor_step state = Some next -> state_potential state = S (state_potential next).
Proof.
  intros state next step. destruct state as [subject stack|operator accumulator remaining stack|value stack];
    [destruct subject|destruct remaining|destruct stack as [|frame stack]; [|destruct frame]];
    simpl in step; inversion step; simpl; lia.
Qed.

Inductive cursor_run : nat -> machine_state -> machine_state -> Prop :=
| CursorDone : forall state, cursor_run 0 state state
| CursorNext : forall amount state next final,
    cursor_step state = Some next -> cursor_run amount next final -> cursor_run (S amount) state final.

Lemma cursor_run_preserves_value : forall amount state final,
  cursor_run amount state final -> state_value final = state_value state.
Proof.
  intros amount state final run. induction run; [reflexivity|].
  rewrite IHrun. eapply suspended_cursor_step_preserves_the_structural_fold; eauto.
Qed.

Fixpoint tree_visits subject : list count_tree :=
  subject :: match subject with
  | CountLeaf _ => []
  | CountBranch _ _ children => forest_visits children
  | CountScale _ child => tree_visits child
  end
with forest_visits children : list count_tree :=
  match children with
  | CountNil => []
  | CountCons child rest => tree_visits child ++ forest_visits rest
  end.

Fixpoint stack_visits stack : list count_tree :=
  match stack with
  | [] => []
  | ResumeCursor _ _ remaining :: tail => forest_visits remaining ++ stack_visits tail
  | ScaleResult _ :: tail => stack_visits tail
  end.

Definition state_visits state :=
  match state with
  | Visit subject stack => tree_visits subject ++ stack_visits stack
  | Scan _ _ remaining stack => forest_visits remaining ++ stack_visits stack
  | Return _ stack => stack_visits stack
  end.

Inductive machine_effect := InspectTree (subject : count_tree) | SerializeWholePar (subject : count_tree).

Definition cursor_effect state :=
  match state with Visit subject _ => [InspectTree subject] | _ => [] end.

Inductive effect_run : machine_state -> list machine_effect -> machine_state -> Prop :=
| EffectDone : forall state, effect_run state [] state
| EffectNext : forall state next final effects,
    cursor_step state = Some next -> effect_run next effects final ->
    effect_run state (cursor_effect state ++ effects) final.

Lemma cursor_step_preserves_ordered_visits : forall state next,
  cursor_step state = Some next ->
  map InspectTree (state_visits state) = cursor_effect state ++ map InspectTree (state_visits next).
Proof.
  intros state next step. destruct state as [subject stack|operator accumulator remaining stack|value stack];
    [destruct subject|destruct remaining|destruct stack as [|frame stack]; [|destruct frame]];
    simpl in step; inversion step; subst; simpl;
    rewrite ?map_app, ?app_assoc; reflexivity.
Qed.

Lemma effect_run_preserves_ordered_visits : forall state effects final,
  effect_run state effects final ->
  map InspectTree (state_visits state) = effects ++ map InspectTree (state_visits final).
Proof.
  intros state effects final run. induction run; simpl; [reflexivity|].
  rewrite (@cursor_step_preserves_ordered_visits state next H), IHrun, app_assoc. reflexivity.
Qed.

Lemma cursor_run_has_effect_trace : forall amount state final,
  cursor_run amount state final -> exists effects, effect_run state effects final.
Proof.
  intros amount state final run. induction run.
  - exists []. constructor.
  - destruct IHrun as [effects traced]. exists (cursor_effect state ++ effects). econstructor; eauto.
Qed.

Theorem suspended_cursor_execution_preserves_exact_ordered_visits : forall subject amount result,
  cursor_run amount (Visit subject []) (Return result []) ->
  exists effects, effect_run (Visit subject []) effects (Return result []) /\
    effects = map InspectTree (tree_visits subject).
Proof.
  intros subject amount result run. destruct (cursor_run_has_effect_trace run) as [effects traced].
  exists effects. split; [assumption|].
  pose proof (effect_run_preserves_ordered_visits traced) as exact. simpl in exact.
  rewrite !app_nil_r in exact. symmetry. assumption.
Qed.

Definition state_stack state :=
  match state with Visit _ stack | Scan _ _ _ stack | Return _ stack => stack end.

Lemma stack_length_bounded_by_potential : forall stack, length stack <= stack_potential stack.
Proof. induction stack as [|frame rest IH]; simpl; [lia|]. destruct frame; simpl; lia. Qed.

Lemma cursor_run_potential : forall amount state final,
  cursor_run amount state final -> state_potential state = amount + state_potential final.
Proof.
  intros amount state final run. induction run; [reflexivity|].
  pose proof (@cursor_step_decreases_potential state next H). lia.
Qed.

Theorem live_continuations_are_bounded_without_occurrence_expansion : forall subject amount reached,
  cursor_run amount (Visit subject []) reached ->
  length (state_stack reached) <= tree_potential subject /\
  (forall factor, tree_potential (CountScale factor subject) = 2 + tree_potential subject).
Proof.
  intros subject amount reached run. pose proof (cursor_run_potential run) as remaining.
  pose proof (stack_length_bounded_by_potential (state_stack reached)) as frames.
  split; [destruct reached; simpl in *; lia|reflexivity].
Qed.

Lemma cursor_stopped_is_return : forall state,
  cursor_step state = None -> exists value, state = Return value [].
Proof.
  intros state stopped. destruct state as [subject stack|operator accumulator remaining stack|value stack];
    [destruct subject|destruct remaining|destruct stack as [|frame stack]; [|destruct frame]];
    simpl in stopped; try discriminate. eauto.
Qed.

Lemma cursor_terminates : forall fuel state,
  state_potential state <= fuel ->
  exists amount value, amount <= fuel /\ cursor_run amount state (Return value []).
Proof.
  induction fuel as [|fuel IH]; intros state bounded;
    destruct (cursor_step state) as [next|] eqn:step.
  - pose proof (@cursor_step_decreases_potential state next step). lia.
  - destruct (@cursor_stopped_is_return state step) as [value ->]. exists 0, value. split; [lia|constructor].
  - pose proof (@cursor_step_decreases_potential state next step) as decrease.
    destruct (IH next ltac:(lia)) as [amount [value [bound run]]].
    exists (S amount), value. split; [lia|econstructor; eauto].
  - destruct (@cursor_stopped_is_return state step) as [value ->]. exists 0, value. split; [lia|constructor].
Qed.

Theorem suspended_cursor_terminates_with_exact_all_field_results : forall subject,
  exists amount result, amount <= tree_potential subject /\
    cursor_run amount (Visit subject []) (Return result []) /\
    (forall kind, result kind = structural_tree subject kind).
Proof.
  intro subject. destruct (@cursor_terminates (tree_potential subject) (Visit subject []))
    as [amount [result [bounded run]]]; [simpl; lia|].
  exists amount, result. split; [assumption|]. split; [assumption|].
  pose proof (cursor_run_preserves_value run) as same. simpl in same. now rewrite same.
Qed.

Fixpoint repeated_bound_sum factor value :=
  match factor with 0 => Finite 0 | S rest => bound_add value (repeated_bound_sum rest value) end.

Lemma bound_scaling_is_repeated_addition : forall factor value,
  bound_scale factor value = repeated_bound_sum factor value.
Proof.
  induction factor as [|factor IH]; intros [value|]; simpl; try reflexivity.
  - rewrite <- IH. destruct factor; simpl; f_equal; lia.
Qed.

Theorem compressed_endpoint_scaling_refines_occurrence_fold_and_cap : forall cap factor value,
  normalize_bound cap (bound_scale factor (normalize_bound cap value)) =
    normalize_bound cap (repeated_bound_sum factor value) /\
  bound_scale 0 value = Finite 0 /\
  tree_potential (CountScale factor (CountLeaf (constant_counts (CountInterval value value)))) = 3.
Proof.
  intros. rewrite normalized_scale_commutes, bound_scaling_is_repeated_addition.
  repeat split; reflexivity.
Qed.

Record byte_region := ByteRegion {
  region_owner : nat;
  region_start : nat;
  region_end : nat
}.

Definition region_valid (owner_length : nat -> nat) retained region :=
  In (region_owner region) retained /\ region_start region <= region_end region /\
    region_end region <= owner_length (region_owner region).

Definition child_region parent start stop :=
  if (start <=? stop) && (stop <=? region_end parent - region_start parent) then
    Some (ByteRegion (region_owner parent) (region_start parent + start) (region_start parent + stop))
  else None.

Theorem checked_child_ranges_preserve_retained_owner_and_bounds : forall lengths retained parent start stop child,
  region_valid lengths retained parent -> child_region parent start stop = Some child ->
  region_valid lengths retained child /\ region_owner child = region_owner parent /\
  region_start parent <= region_start child /\ region_end child <= region_end parent.
Proof.
  intros lengths retained parent start stop child valid subregion.
  unfold child_region in subregion. destruct ((start <=? stop) && (stop <=? region_end parent - region_start parent)) eqn:within;
    [|discriminate]. apply andb_true_iff in within. destruct within as [ordered bounded].
  apply Nat.leb_le in ordered. apply Nat.leb_le in bounded. inversion subregion; subst.
  unfold region_valid in *. simpl in *. destruct valid as [owned [range length]].
  repeat split; auto; lia.
Qed.

Definition cursor_regions_valid lengths retained (active : byte_region) (suspended : list byte_region) :=
  region_valid lengths retained active /\ Forall (region_valid lengths retained) suspended.

Theorem suspending_and_resuming_preserve_every_retained_range : forall lengths retained parent start stop child stack,
  cursor_regions_valid lengths retained parent stack -> child_region parent start stop = Some child ->
  cursor_regions_valid lengths retained child (parent :: stack) /\
  cursor_regions_valid lengths retained parent stack.
Proof.
  intros lengths retained parent start stop child stack [parent_valid stack_valid] child_valid.
  pose proof (@checked_child_ranges_preserve_retained_owner_and_bounds lengths retained parent start stop child
    parent_valid child_valid) as [valid _].
  split.
  - split; [exact valid|]. constructor; assumption.
  - split; assumption.
Qed.

Inductive header_cursor_run {A : Type} : list A -> list A -> list A -> Prop :=
| HeaderStop : forall remaining, header_cursor_run remaining [] remaining
| HeaderRead : forall header rest visited remaining,
    header_cursor_run rest visited remaining -> header_cursor_run (header :: rest) (header :: visited) remaining.

Lemma header_cursor_partitions_input : forall A (input visited remaining : list A),
  header_cursor_run input visited remaining -> input = visited ++ remaining.
Proof. intros A input visited remaining run. induction run; simpl; congruence. Qed.

Lemma header_cursor_complete : forall A (input : list A), header_cursor_run input input [].
Proof. intros A input. induction input; constructor; auto. Qed.

Definition repeated_header_passes {A : Type} passes (headers : list A) := concat (repeat headers passes).

Lemma repeated_header_pass_length : forall A passes (headers : list A),
  length (repeated_header_passes passes headers) = passes * length headers.
Proof.
  intros A passes headers. induction passes; unfold repeated_header_passes in *; simpl; [reflexivity|].
  rewrite length_app, IHpasses. reflexivity.
Qed.

Definition header_work_mass {A : Type} (work : list (list A)) :=
  fold_right (fun remaining total => length remaining + total) 0 work.

Lemma header_work_mass_app : forall A (left right : list (list A)),
  header_work_mass (left ++ right) = header_work_mass left + header_work_mass right.
Proof.
  intros A left right. induction left; unfold header_work_mass in *; simpl in *; [reflexivity|].
  rewrite IHleft. lia.
Qed.

Lemma repeated_cursor_mass : forall A passes (headers : list A),
  header_work_mass (repeat headers passes) = passes * length headers.
Proof.
  intros A passes headers. induction passes; unfold header_work_mass in *; simpl in *; [reflexivity|].
  now rewrite IHpasses.
Qed.

Definition catalog_cursor_pool {A : Type} passes (catalogs : list (list A)) :=
  flat_map (fun headers => repeat headers passes) catalogs.

Lemma catalog_cursor_pool_mass : forall A passes (catalogs : list (list A)),
  header_work_mass (catalog_cursor_pool passes catalogs) = passes * length (concat catalogs).
Proof.
  intros A passes catalogs. induction catalogs; unfold catalog_cursor_pool in *; simpl; [lia|].
  rewrite header_work_mass_app, repeated_cursor_mass, IHcatalogs, length_app. nia.
Qed.

Inductive interleaved_header_run {A : Type} : list (list A) -> list A -> list (list A) -> Prop :=
| InterleavedStop : forall work, interleaved_header_run work [] work
| InterleavedRead : forall before header rest after visited remaining,
    interleaved_header_run (before ++ rest :: after) visited remaining ->
    interleaved_header_run (before ++ (header :: rest) :: after) (header :: visited) remaining.

Lemma interleaved_headers_conserve_remaining_work : forall A (work remaining : list (list A)) visited,
  interleaved_header_run work visited remaining ->
  header_work_mass work = length visited + header_work_mass remaining.
Proof.
  intros A work remaining visited run. induction run; simpl; [reflexivity|].
  rewrite header_work_mass_app in *. unfold header_work_mass at 2 in IHrun.
  unfold header_work_mass at 2. simpl in *. lia.
Qed.

Theorem inspected_headers_have_a_derived_constant_pass_byte_budget :
  forall (catalogs : list (list (nat * nat))) owner_byte_positions passes visited remaining,
  NoDup (concat catalogs) -> incl (concat catalogs) owner_byte_positions ->
  interleaved_header_run (catalog_cursor_pool passes catalogs) visited remaining ->
  length visited <= passes * length owner_byte_positions /\
  (forall headers, In headers catalogs -> header_cursor_run headers headers []).
Proof.
  intros catalogs positions passes visited remaining distinct included run.
  pose proof (NoDup_incl_length distinct included) as header_bound.
  pose proof (interleaved_headers_conserve_remaining_work run) as conservation.
  rewrite catalog_cursor_pool_mass in conservation.
  split; [nia|]. intros. apply header_cursor_complete.
Qed.

Definition copied_bytes (endpoints : list byte_region) :=
  count_mass (map (fun region => region_end region - region_start region) endpoints).

Inductive endpoint_copy_run : list byte_region -> nat -> Prop :=
| CopyDone : endpoint_copy_run [] 0
| CopyNext : forall region rest copied,
    endpoint_copy_run rest copied ->
    endpoint_copy_run (region :: rest) (region_end region - region_start region + copied).

Inductive retention_action := CopyOuterEndpoint (region : byte_region) |
  ShareChildRange (parent : byte_region) (start stop : nat).

Definition retention_step action copied :=
  match action with
  | CopyOuterEndpoint region => Some (copied + (region_end region - region_start region), region)
  | ShareChildRange parent start stop =>
      match child_region parent start stop with Some child => Some (copied, child) | None => None end
  end.

Theorem outer_endpoint_copying_is_exact_and_descents_add_no_payload_copy : forall endpoints,
  endpoint_copy_run endpoints (copied_bytes endpoints) /\
  (forall copied, endpoint_copy_run endpoints copied -> copied = copied_bytes endpoints) /\
  (forall parent start stop child copied, child_region parent start stop = Some child ->
    retention_step (ShareChildRange parent start stop) copied = Some (copied, child) /\
    region_owner child = region_owner parent).
Proof.
  intro endpoints. split.
  - induction endpoints; simpl; constructor; assumption.
  - split.
    + intros copied run. induction run; unfold copied_bytes, count_mass in *; simpl in *; lia.
    + intros parent start stop child copied checked. split.
      * simpl. now rewrite checked.
      * unfold child_region in checked.
        destruct ((start <=? stop) && (stop <=? region_end parent - region_start parent)); inversion checked; reflexivity.
Qed.

Definition public_connective_entry subject := Visit subject [].

Theorem public_connective_entry_remains_structural_for_arbitrary_mass : forall subject factor,
  public_connective_entry (CountScale factor subject) = Visit (CountScale factor subject) [] /\
  tree_potential (CountScale factor subject) = 2 + tree_potential subject /\
  (forall effects result serialized,
    effect_run (public_connective_entry (CountScale factor subject)) effects (Return result []) ->
    ~ In (SerializeWholePar serialized) effects).
Proof.
  intros subject factor. split; [reflexivity|]. split; [reflexivity|].
  intros effects result serialized run present.
  pose proof (effect_run_preserves_ordered_visits run) as exact.
  unfold public_connective_entry in exact. simpl in exact. rewrite !app_nil_r in exact.
  rewrite <- exact in present. destruct present as [impossible|present]; [discriminate|].
  apply in_map_iff in present.
  destruct present as [node [impossible _]]. discriminate.
Qed.

Example forgetting_unproved_delegation_changes_a_valid_hole :
  resolve_classification Unproved (DecodedExpression (FreeExpression (-1))) = HoleResult true /\
  resolve_classification ProvenConcrete (DecodedExpression (FreeExpression (-1))) = HoleResult false /\
  ~ class_sound ProvenConcrete (DecodedExpression (FreeExpression (-1))).
Proof. repeat split; discriminate. Qed.

Example absent_options_are_not_holes :
  map semantic_classification [AbsentExpression; AbsentEVar; AbsentVar; BoundExpression (-1)] =
    [ProvenConcrete; ProvenConcrete; ProvenConcrete; ProvenConcrete].
Proof. reflexivity. Qed.

Example metadata_gating_would_omit_a_scalar_connective :
  structural_tree (par_tree false [] (CountCons (predicate_tree 5 false) CountNil)) Exprs =
    CountInterval (Finite 1) (Finite 1) /\ base_projection [] Exprs = exact_zero.
Proof. split; reflexivity. Qed.

Example zero_scale_does_not_repeat_or_preserve_top :
  structural_tree (CountScale 0 (CountLeaf any_counts)) Exprs = exact_zero /\
  tree_potential (CountScale 0 (CountLeaf any_counts)) = 3.
Proof. split; reflexivity. Qed.

Definition empty_fixture_decoder field (body : list nat) :=
  match body with
  | [] => Some (match field with
      | CountedField Exprs => Expression AbsentExpression
      | CountedField kind => Ground kind
      | ConnectiveField => ConnectiveEndpoint
      end)
  | _ => None
  end.

Definition mixed_empty_wire :=
  [EncodedEndpoint (CountedField Sends) [] 1;
   EncodedEndpoint (CountedField Receives) [] 1;
   EncodedEndpoint (CountedField Exprs) [] 1;
   EncodedEndpoint ConnectiveField [] 1].

Definition mixed_empty_resident :=
  [(Ground Sends, 1); (Ground Receives, 1); (Expression AbsentExpression, 1); (ConnectiveEndpoint, 1)].

Example identical_empty_bodies_retain_distinct_source_family_authority :
  projection_admission empty_fixture_decoder empty_fixture_decoder mixed_empty_wire mixed_empty_resident /\
  project_encoded empty_fixture_decoder mixed_empty_wire = Some mixed_empty_resident.
Proof.
  split; [split|reflexivity].
  - repeat constructor.
  - intros; reflexivity.
Qed.

Definition connective_counts (entries : list endpoint) :=
  map snd (filter (fun entry => match fst entry with ConnectiveEndpoint => true | _ => false end) entries).

Fixpoint assemble_connectives (children : list (count_tree * nat)) :=
  match children with
  | [] => CountNil
  | (child, factor) :: rest => CountCons (CountScale factor child) (assemble_connectives rest)
  end.

Fixpoint forest_children forest : list count_tree :=
  match forest with CountNil => [] | CountCons child rest => child :: forest_children rest end.

Definition connective_assembly_admission entries (children : list (count_tree * nat)) :=
  map snd children = connective_counts entries.

Lemma assembled_children_are_exactly_scaled_endpoints : forall children,
  forest_children (assemble_connectives children) = map (fun entry => CountScale (snd entry) (fst entry)) children.
Proof. induction children as [|[child factor] rest IH]; simpl; [reflexivity|]. now rewrite IH. Qed.

Theorem ordered_endpoint_assembly_preserves_exact_multiplicity_and_visits : forall entries children,
  connective_assembly_admission entries children ->
  length (forest_children (assemble_connectives children)) = length (connective_counts entries) /\
  forest_children (assemble_connectives children) = map (fun entry => CountScale (snd entry) (fst entry)) children /\
  forest_visits (assemble_connectives children) =
    concat (map (fun entry => CountScale (snd entry) (fst entry) :: tree_visits (fst entry)) children).
Proof.
  intros entries children admitted. split.
  - rewrite assembled_children_are_exactly_scaled_endpoints, length_map.
    unfold connective_assembly_admission in admitted.
    rewrite <- admitted, length_map. reflexivity.
  - split; [apply assembled_children_are_exactly_scaled_endpoints|].
    clear admitted entries. induction children as [|[child factor] rest IH]; simpl; [reflexivity|].
    now rewrite IH.
Qed.

Example connective_inventory_requires_the_declared_scale :
  connective_assembly_admission [(ConnectiveEndpoint, 2)] [(predicate_tree 5 false, 2)] /\
  ~ connective_assembly_admission [(ConnectiveEndpoint, 2)] [(predicate_tree 5 false, 1)] /\
  structural_tree (par_tree false [(ConnectiveEndpoint, 2)]
    (assemble_connectives [(predicate_tree 5 false, 2)])) Exprs = CountInterval (Finite 2) (Finite 2).
Proof. repeat split; try reflexivity; discriminate. Qed.

Print Assumptions encoded_projection_preserves_all_eight_fields_and_hole_mass.
Print Assumptions duplicate_occurrences_and_compressed_endpoints_have_equal_projection.
Print Assumptions tri_state_refinement_preserves_decoder_results_and_failure_authority.
Print Assumptions admitted_expression_forms_have_complete_zero_fallback_classification.
Print Assumptions structural_connective_equations_ignore_metadata_but_not_children.
Print Assumptions suspended_cursor_step_preserves_the_structural_fold.
Print Assumptions suspended_cursor_terminates_with_exact_all_field_results.
Print Assumptions suspended_cursor_execution_preserves_exact_ordered_visits.
Print Assumptions live_continuations_are_bounded_without_occurrence_expansion.
Print Assumptions compressed_endpoint_scaling_refines_occurrence_fold_and_cap.
Print Assumptions checked_child_ranges_preserve_retained_owner_and_bounds.
Print Assumptions suspending_and_resuming_preserve_every_retained_range.
Print Assumptions inspected_headers_have_a_derived_constant_pass_byte_budget.
Print Assumptions outer_endpoint_copying_is_exact_and_descents_add_no_payload_copy.
Print Assumptions public_connective_entry_remains_structural_for_arbitrary_mass.
Print Assumptions forgetting_unproved_delegation_changes_a_valid_hole.
Print Assumptions absent_options_are_not_holes.
Print Assumptions metadata_gating_would_omit_a_scalar_connective.
Print Assumptions zero_scale_does_not_repeat_or_preserve_top.
Print Assumptions identical_empty_bodies_retain_distinct_source_family_authority.
Print Assumptions ordered_endpoint_assembly_preserves_exact_multiplicity_and_visits.

End ParCountByteNative.
