From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import GenerationCheckpointPublication IncrementalLedgerTrial
  IncrementalDomainComposition CreditedClosureLedger RetentionV2Format RetentionV2Editor.

Import ListNotations.

(**
  The trusted successor capability of an incremental durable publication.

  An ordinary commit of the durable RSpace authority publishes its successor
  without re-reading the store.  It binds the sealed receipt to the exact
  predecessor core, runs one failure-atomic ledger trial per inventory domain
  on a shared three-count ledger, checks the hot policy's state ceilings,
  assembles the successor's object index by appending the plan to the
  predecessor's index, and adopts the successor after the catalog swap, the
  catalog-directory sync and the sidecar resolution, without physical recovery
  or cold admission.

  The theorems below show that this capability is exact.  The production order
  of the three domain trials on the shared ledger is the composed trial of
  [CheckedDomainComposition]; the appended index is the cold rebuild of the
  appointed records; the retained-state maxima are the componentwise maxima of
  the cold per-state inventories while retention only appends, and an eviction
  needs a recomputation; the ceilings accept exactly at the successor's values
  and refuse one below on each axis; the sealed policy and mode bind the
  publication; every trial and check precedes the first durable mutation; and
  along every commit-only history from a cold-admitted base, the trusted core
  equals cold admission of the final state in its ledgers, roots, inventories,
  retained-state count, maxima and index.  Cold demand, by contrast, is not a
  function of the trusted inventory summary, so a fresh checkpoint from a
  trusted core measures it.  The journal owner's empty checkpoint and its
  awaiting-acknowledgement retry never consult the cold limits.

  Delta soundness, that a receipt turns a semantically valid base into a valid
  target inside the object universe, is an explicit argument of the theorems
  that need it.  The variants other than [TrustedFaithful] and the cold
  preflight variants other than [MeasureColdDemand] are negative controls, each
  refuted by a closed counterexample theorem at the end.
*)

Module TrustedSuccessorCapability.

Import IncrementalReachability.
Import CheckedReachability.
Import CheckedLedgerTrial.
Import CheckedDomainComposition.

(** * The shared three-count ledger

    The Rust ledger keeps one record per object digest with a count for each
    of the three domains, and one root counter and one inventory per domain.
    [shared_ledger] is its curried form: each component is indexed by the
    domain.  A trial of one domain reads that domain's projection and writes
    only that domain's components. *)

Definition domain_eqb (left right : domain) : bool :=
  match left, right with
  | CurrentDomain, CurrentDomain => true
  | RetainedDomain, RetainedDomain => true
  | MetadataDomain, MetadataDomain => true
  | _, _ => false
  end.

Record shared_ledger := SharedLedger {
  domain_ledger : domain -> ledger;
  domain_roots : domain -> counter;
  domain_objects : domain -> nat;
  domain_bytes : domain -> nat
}.

Definition overwrite {Value : Type} (old : domain -> Value) (target : domain) (value : Value)
    (query : domain) : Value :=
  if domain_eqb query target then value else old query.

Definition project (shared : shared_ledger) (target : domain) : predecessor :=
  Predecessor (domain_ledger shared target) (domain_roots shared target)
    (domain_objects shared target) (domain_bytes shared target).

Definition projections (shared : shared_ledger) : composed_predecessor :=
  ComposedPredecessor (project shared CurrentDomain) (project shared RetainedDomain)
    (project shared MetadataDomain).

Definition write_domain (shared : shared_ledger) (target : domain)
    (evidence : prepared_evidence) : shared_ledger :=
  SharedLedger (overwrite (domain_ledger shared) target (prepared_ledger evidence))
    (overwrite (domain_roots shared) target (prepared_roots evidence))
    (overwrite (domain_objects shared) target (prepared_objects evidence))
    (overwrite (domain_bytes shared) target (prepared_bytes evidence)).

Definition commit_shared (shared : shared_ledger) (evidence : composed_evidence) :
    shared_ledger :=
  write_domain (write_domain (write_domain shared CurrentDomain (current_prepared evidence))
    RetainedDomain (retained_prepared evidence)) MetadataDomain (metadata_prepared evidence).

Definition component (base : composed_predecessor) (target : domain) : predecessor :=
  match target with
  | CurrentDomain => current_base base
  | RetainedDomain => retained_base base
  | MetadataDomain => metadata_base base
  end.

Definition shared_of (base : composed_predecessor) : shared_ledger :=
  SharedLedger (fun target => base_ledger (component base target))
    (fun target => base_roots (component base target))
    (fun target => base_objects (component base target))
    (fun target => base_bytes (component base target)).

Lemma projections_shared_of : forall base, projections (shared_of base) = base.
Proof. intros [[] [] []]; reflexivity. Qed.

Definition domain_environment (env : composed_environment) (target : domain) :
    trial_environment :=
  match target with
  | MetadataDomain => metadata_environment env
  | _ => semantic_environment env
  end.

Definition domain_budget (budget : composed_budget) (target : domain) : option trial_limits :=
  match target with
  | CurrentDomain => current_budget budget
  | RetainedDomain => retained_budget budget
  | MetadataDomain => metadata_budget budget
  end.

Definition domain_change (delta : composed_delta) (target : domain) : root_delta :=
  match target with
  | CurrentDomain => current_delta delta
  | RetainedDomain => retained_delta delta
  | MetadataDomain => metadata_delta delta
  end.

Definition shared_domain_trial (env : composed_environment) (budget : composed_budget)
    (schedule : nat -> bool) (shared : shared_ledger) (delta : composed_delta)
    (target : domain) : trial_result :=
  domain_trial (domain_environment env target) (domain_budget budget target) schedule
    (project shared target) (domain_change delta target).

(** The publication variants.  [TrustedFaithful] is the production capability;
    the others are negative controls: a skipped retained or metadata trial, a
    publication preflight that ignores the sealed policy, and a publication that
    writes and swaps before the trials run. *)
Inductive trusted_variant :=
| TrustedFaithful
| SkipRetainedTrial
| SkipMetadataTrial
| IgnoreSealedPolicy
| PublishBeforeTrials.

Definition skipped_trial (base : predecessor) : trial_result :=
  TrialResult base (TrialPrepared (PreparedEvidence (base_ledger base) (base_roots base)
    (base_objects base) (base_bytes base) (TrialUsage 0 0 0 0 0 0 0 0))).

Definition retained_step (variant : trusted_variant) (env : composed_environment)
    (budget : composed_budget) (schedule : nat -> bool) (shared : shared_ledger)
    (delta : composed_delta) : trial_result :=
  match variant with
  | SkipRetainedTrial => skipped_trial (project shared RetainedDomain)
  | _ => shared_domain_trial env budget schedule shared delta RetainedDomain
  end.

Definition metadata_step (variant : trusted_variant) (env : composed_environment)
    (budget : composed_budget) (schedule : nat -> bool) (shared : shared_ledger)
    (delta : composed_delta) : trial_result :=
  match variant with
  | SkipMetadataTrial => skipped_trial (project shared MetadataDomain)
  | _ => shared_domain_trial env budget schedule shared delta MetadataDomain
  end.

Inductive production_result :=
| ProductionPrepared (successor : shared_ledger) (evidence : composed_evidence)
| ProductionRefused (refusing : domain) (why : refusal).

(** The production order: the current trial on the shared ledger, then the
    retained trial on the ledger the current trial wrote, then the metadata
    trial on the ledger both semantic trials wrote. *)
Definition production_order (variant : trusted_variant) (env : composed_environment)
    (budget : composed_budget) (schedule : nat -> bool) (shared : shared_ledger)
    (delta : composed_delta) : production_result :=
  match outcome (shared_domain_trial env budget schedule shared delta CurrentDomain) with
  | TrialRefused why => ProductionRefused CurrentDomain why
  | TrialPrepared current_side =>
      match outcome (retained_step variant env budget schedule
              (write_domain shared CurrentDomain current_side) delta) with
      | TrialRefused why => ProductionRefused RetainedDomain why
      | TrialPrepared retained_side =>
          match outcome (metadata_step variant env budget schedule
                  (write_domain (write_domain shared CurrentDomain current_side)
                     RetainedDomain retained_side) delta) with
          | TrialRefused why => ProductionRefused MetadataDomain why
          | TrialPrepared metadata_side =>
              ProductionPrepared
                (commit_shared shared
                   (ComposedEvidence current_side retained_side metadata_side))
                (ComposedEvidence current_side retained_side metadata_side)
          end
      end
  end.

Lemma shared_current_trial : forall env budget schedule shared delta,
  shared_domain_trial env budget schedule shared delta CurrentDomain =
  current_trial env budget schedule (projections shared) delta.
Proof. intros; reflexivity. Qed.

Lemma retained_step_faithful : forall env budget schedule shared current_side delta,
  retained_step TrustedFaithful env budget schedule
    (write_domain shared CurrentDomain current_side) delta =
  retained_trial ComposeFaithful env budget schedule (projections shared) delta.
Proof. intros; reflexivity. Qed.

Lemma metadata_step_faithful :
  forall env budget schedule shared current_side retained_side delta,
  metadata_step TrustedFaithful env budget schedule
    (write_domain (write_domain shared CurrentDomain current_side) RetainedDomain
       retained_side) delta =
  metadata_trial env budget schedule (projections shared) delta.
Proof. intros; reflexivity. Qed.

Lemma production_order_faithful_spec : forall env budget schedule shared delta,
  production_order TrustedFaithful env budget schedule shared delta =
  match composed_outcome (run_composed ComposeFaithful env budget schedule
          (projections shared) delta) with
  | ComposedPrepared evidence => ProductionPrepared (commit_shared shared evidence) evidence
  | ComposedRefused refusing why => ProductionRefused refusing why
  end.
Proof.
  intros env budget schedule shared delta.
  rewrite run_composed_spec. cbn [composed_outcome].
  unfold production_order. rewrite shared_current_trial.
  destruct (outcome (current_trial env budget schedule (projections shared) delta))
    as [current_side|why]; cbv beta iota; [|reflexivity].
  rewrite retained_step_faithful.
  destruct (outcome (retained_trial ComposeFaithful env budget schedule (projections shared)
    delta)) as [retained_side|why]; cbv beta iota; [|reflexivity].
  rewrite metadata_step_faithful.
  destruct (outcome (metadata_trial env budget schedule (projections shared) delta))
    as [metadata_side|why]; reflexivity.
Qed.

(** Capstone: the production order on the shared ledger is the composed trial
    of [CheckedDomainComposition] on the ledger's projections.  It prepares
    exactly when the composed trial prepares, its successor ledger projects to
    the composed successor base, it refuses at the domain and for the reason
    the composed trial refuses, and the metadata trial reads the same
    predecessor after both semantic trials wrote. *)
Theorem production_order_is_run_composed :
  forall env budget schedule shared delta,
  (forall successor evidence,
     production_order TrustedFaithful env budget schedule shared delta =
       ProductionPrepared successor evidence <->
     composed_outcome (run_composed ComposeFaithful env budget schedule (projections shared)
       delta) = ComposedPrepared evidence /\ successor = commit_shared shared evidence) /\
  (forall evidence, projections (commit_shared shared evidence) = successor_base evidence) /\
  (forall refusing why,
     production_order TrustedFaithful env budget schedule shared delta =
       ProductionRefused refusing why <->
     composed_outcome (run_composed ComposeFaithful env budget schedule (projections shared)
       delta) = ComposedRefused refusing why) /\
  (forall current_side retained_side,
     shared_domain_trial env budget schedule
       (write_domain (write_domain shared CurrentDomain current_side) RetainedDomain
          retained_side) delta MetadataDomain =
     metadata_trial env budget schedule (projections shared) delta).
Proof.
  intros env budget schedule shared delta.
  rewrite production_order_faithful_spec.
  split; [|split; [|split]].
  - intros successor evidence.
    destruct (composed_outcome (run_composed ComposeFaithful env budget schedule
      (projections shared) delta)) as [found|refusing why]; cbv beta iota.
    + split.
      * intros same. injection same as same_successor same_evidence. subst.
        split; reflexivity.
      * intros [same ->]. injection same as same_evidence. subst. reflexivity.
    + split; [intros impossible; discriminate impossible|].
      intros [impossible _]. discriminate impossible.
  - intros evidence. reflexivity.
  - intros refusing why.
    destruct (composed_outcome (run_composed ComposeFaithful env budget schedule
      (projections shared) delta)) as [found|found_refusing found_why]; cbv beta iota.
    + split; intros impossible; discriminate impossible.
    + split; intros same; injection same as same_refusing same_why; subst; reflexivity.
  - intros current_side retained_side. reflexivity.
Qed.

(** * The hot policy

    [TrustedSuccessorPolicyV1]: eleven state ceilings and, per domain, seven
    trial work budgets.  The trial of a domain refuses its final inventory
    against that domain's ceilings: the current domain against the per-state
    ceilings, the retained domain against the union ceilings and the metadata
    domain against the metadata ceilings. *)

Record state_ceilings := StateCeilings {
  max_state_objects : nat;
  max_state_bytes : nat;
  max_retained_states : nat;
  max_union_objects : nat;
  max_union_bytes : nat;
  max_metadata_objects : nat;
  max_metadata_bytes : nat;
  max_logical_nodes : nat;
  max_endpoints : nat;
  max_logical_path_bytes : nat;
  max_payload_bytes : nat
}.

Record trial_work := TrialWork {
  work_count : nat;
  work_union_objects : nat;
  work_union_bytes : nat;
  work_scratch : nat;
  work_references : nat;
  work_expansions : nat;
  work_quoted_bytes : nat
}.

Record trusted_policy := TrustedPolicy {
  policy_state : state_ceilings;
  policy_current_work : trial_work;
  policy_retained_work : trial_work;
  policy_metadata_work : trial_work
}.

Definition state_ceilings_eq_dec (left right : state_ceilings) :
  {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition trial_work_eq_dec (left right : trial_work) : {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition trusted_policy_eq_dec (left right : trusted_policy) :
  {left = right} + {left <> right}.
Proof. decide equality; first [apply state_ceilings_eq_dec | apply trial_work_eq_dec]. Defined.

Definition work_limits (work : trial_work) (final_objects final_bytes : nat) : trial_limits :=
  fun axis =>
    match axis with
    | CountAxis => work_count work
    | UnionObjectsAxis => work_union_objects work
    | UnionBytesAxis => work_union_bytes work
    | ScratchAxis => work_scratch work
    | ReferenceAxis => work_references work
    | ExpansionAxis => work_expansions work
    | QuotedBytesAxis => work_quoted_bytes work
    | FinalObjectsAxis => final_objects
    | FinalBytesAxis => final_bytes
    end.

Definition policy_budget (policy : trusted_policy) : composed_budget :=
  ComposedBudget
    (Some (work_limits (policy_current_work policy)
      (max_state_objects (policy_state policy)) (max_state_bytes (policy_state policy))))
    (Some (work_limits (policy_retained_work policy)
      (max_union_objects (policy_state policy)) (max_union_bytes (policy_state policy))))
    (Some (work_limits (policy_metadata_work policy)
      (max_metadata_objects (policy_state policy))
      (max_metadata_bytes (policy_state policy)))).

Definition generous_work : trial_work := TrialWork 64 64 64 64 64 64 64.

Definition generous_ceilings : state_ceilings :=
  StateCeilings 64 64 64 64 64 64 64 64 64 64 64.

Definition generous_policy : trusted_policy :=
  TrustedPolicy generous_ceilings generous_work generous_work generous_work.

(** * Ceilings checked outside the trials

    The trials check the union and metadata final ceilings and the target's
    per-state ceilings.  The binding checks the remaining ceilings against the
    successor's values: the retained-state maxima against the per-state
    ceilings, the retained-state count, and the four shape ceilings. *)

Inductive ceiling_axis :=
| StateObjectsCeiling
| StateBytesCeiling
| RetainedStatesCeiling
| LogicalNodesCeiling
| EndpointsCeiling
| LogicalPathBytesCeiling
| PayloadBytesCeiling.

Definition ceiling_axis_eq_dec (left right : ceiling_axis) : {left = right} + {left <> right}.
Proof. decide equality. Defined.

Definition ceiling_axes : list ceiling_axis :=
  [StateObjectsCeiling; StateBytesCeiling; RetainedStatesCeiling; LogicalNodesCeiling;
   EndpointsCeiling; LogicalPathBytesCeiling; PayloadBytesCeiling].

Lemma every_ceiling_axis_is_listed : forall axis, In axis ceiling_axes.
Proof. intros []; cbn [ceiling_axes In]; tauto. Qed.

Definition ceiling (limits : state_ceilings) (axis : ceiling_axis) : nat :=
  match axis with
  | StateObjectsCeiling => max_state_objects limits
  | StateBytesCeiling => max_state_bytes limits
  | RetainedStatesCeiling => max_retained_states limits
  | LogicalNodesCeiling => max_logical_nodes limits
  | EndpointsCeiling => max_endpoints limits
  | LogicalPathBytesCeiling => max_logical_path_bytes limits
  | PayloadBytesCeiling => max_payload_bytes limits
  end.

Definition with_ceiling (limits : state_ceilings) (axis : ceiling_axis) (value : nat) :
    state_ceilings :=
  let pick := fun (target : ceiling_axis) (old : nat) =>
    if ceiling_axis_eq_dec axis target then value else old in
  StateCeilings (pick StateObjectsCeiling (max_state_objects limits))
    (pick StateBytesCeiling (max_state_bytes limits))
    (pick RetainedStatesCeiling (max_retained_states limits))
    (max_union_objects limits) (max_union_bytes limits)
    (max_metadata_objects limits) (max_metadata_bytes limits)
    (pick LogicalNodesCeiling (max_logical_nodes limits))
    (pick EndpointsCeiling (max_endpoints limits))
    (pick LogicalPathBytesCeiling (max_logical_path_bytes limits))
    (pick PayloadBytesCeiling (max_payload_bytes limits)).

Lemma ceiling_with_ceiling : forall limits axis value query,
  ceiling (with_ceiling limits axis value) query =
  if ceiling_axis_eq_dec axis query then value else ceiling limits query.
Proof. intros limits axis value []; reflexivity. Qed.

Record shape_summary := ShapeSummary {
  logical_nodes : nat;
  endpoint_count : nat;
  logical_path_bytes : nat;
  payload_bytes : nat
}.

Definition zero_shape : shape_summary := ShapeSummary 0 0 0 0.

Definition successor_values (maxima : nat * nat) (retained_states : nat)
    (shape : shape_summary) (axis : ceiling_axis) : nat :=
  match axis with
  | StateObjectsCeiling => fst maxima
  | StateBytesCeiling => snd maxima
  | RetainedStatesCeiling => retained_states
  | LogicalNodesCeiling => logical_nodes shape
  | EndpointsCeiling => endpoint_count shape
  | LogicalPathBytesCeiling => logical_path_bytes shape
  | PayloadBytesCeiling => payload_bytes shape
  end.

Definition first_ceiling_exceeded (limits : state_ceilings) (values : ceiling_axis -> nat) :
    option ceiling_axis :=
  find (fun axis => ceiling limits axis <? values axis) ceiling_axes.

Definition exact_ceilings (values : ceiling_axis -> nat) (base : state_ceilings) :
    state_ceilings :=
  StateCeilings (values StateObjectsCeiling) (values StateBytesCeiling)
    (values RetainedStatesCeiling) (max_union_objects base) (max_union_bytes base)
    (max_metadata_objects base) (max_metadata_bytes base) (values LogicalNodesCeiling)
    (values EndpointsCeiling) (values LogicalPathBytesCeiling) (values PayloadBytesCeiling).

Lemma exact_ceilings_ceiling : forall values base axis,
  ceiling (exact_ceilings values base) axis = values axis.
Proof. intros values base []; reflexivity. Qed.

Lemma find_none_iff : forall (A : Type) (test : A -> bool) (items : list A),
  find test items = None <-> forall item, In item items -> test item = false.
Proof.
  intros A test items. induction items as [|head tail IH]; cbn [find In].
  - split; [intros _ item []|intros _; reflexivity].
  - destruct (test head) eqn:head_test; cbv beta iota.
    + split; [intros impossible; discriminate impossible|].
      intros all. rewrite (all head (or_introl eq_refl)) in head_test.
      discriminate head_test.
    + rewrite IH. split.
      * intros rest item [same|member]; [subst item; exact head_test|exact (rest item member)].
      * intros all item member. exact (all item (or_intror member)).
Qed.

Lemma find_unique_some : forall (A : Type)
  (eq_dec : forall left right : A, {left = right} + {left <> right})
  (test : A -> bool) (items : list A) (target : A),
  In target items -> test target = true ->
  (forall item, In item items -> item <> target -> test item = false) ->
  find test items = Some target.
Proof.
  intros A eq_dec test items target. induction items as [|head tail IH]; cbn [find In].
  - intros [].
  - intros member target_true others.
    destruct (eq_dec head target) as [same|different].
    + subst head. rewrite target_true. reflexivity.
    + rewrite (others head (or_introl eq_refl) different). cbv beta iota.
      apply IH.
      * destruct member as [same|member]; [contradiction|exact member].
      * exact target_true.
      * intros item item_member item_different.
        exact (others item (or_intror item_member) item_different).
Qed.

(** Capstone: the ceiling decision is exact.  It accepts exactly when every
    value is within its ceiling, a refusal names an exceeded axis, the ceilings
    equal to the successor's values accept, and lowering one ceiling to one
    below a positive value refuses on that axis. *)
Theorem ceiling_decisions_are_exact : forall limits values,
  (first_ceiling_exceeded limits values = None <->
     forall axis, values axis <= ceiling limits axis) /\
  (forall axis, first_ceiling_exceeded limits values = Some axis ->
     ceiling limits axis < values axis) /\
  (forall base, first_ceiling_exceeded (exact_ceilings values base) values = None) /\
  (forall base axis, 0 < values axis ->
     first_ceiling_exceeded (with_ceiling (exact_ceilings values base) axis
       (pred (values axis))) values = Some axis).
Proof.
  intros limits values. unfold first_ceiling_exceeded.
  assert (none_iff : forall bounds,
    find (fun axis => ceiling bounds axis <? values axis) ceiling_axes = None <->
    forall axis, values axis <= ceiling bounds axis).
  { intros bounds. rewrite find_none_iff. split.
    - intros all axis. specialize (all axis (every_ceiling_axis_is_listed axis)).
      cbv beta in all. apply Nat.ltb_ge in all. exact all.
    - intros all axis _. cbv beta. apply Nat.ltb_ge. exact (all axis). }
  split; [exact (none_iff limits)|].
  split.
  - intros axis found. apply find_some in found. destruct found as [_ exceeded].
    cbv beta in exceeded. apply Nat.ltb_lt in exceeded. exact exceeded.
  - split.
    + intros base. apply none_iff. intros axis. rewrite exact_ceilings_ceiling. lia.
    + intros base axis positive. apply (find_unique_some ceiling_axis ceiling_axis_eq_dec).
      * apply every_ceiling_axis_is_listed.
      * cbv beta. rewrite ceiling_with_ceiling.
        destruct (ceiling_axis_eq_dec axis axis) as [same_axis|different];
          [|contradiction (different eq_refl)].
        apply Nat.ltb_lt. lia.
      * intros item _ different. cbv beta. rewrite ceiling_with_ceiling.
        destruct (ceiling_axis_eq_dec axis item) as [same|different_axes];
          [subst item; contradiction (different eq_refl)|].
        rewrite exact_ceilings_ceiling. apply Nat.ltb_irrefl.
Qed.

(** * The committed object index

    [CommittedObjectIndexV1]: insertion refuses a second record for a digest,
    as duplicate data when the reference is the same and as a conflict
    otherwise, and refuses a count without a successor below the capacity.
    Recovery rebuilds the index by inserting the appointed records in order. *)

Record index_record := IndexRecord {
  record_digest : nat;
  record_reference : nat;
  record_location : nat
}.

Record committed_index := CommittedIndex {
  index_entries : list index_record;
  index_count : nat
}.

Definition empty_index : committed_index := CommittedIndex [] 0.

Definition index_lookup (index : committed_index) (digest : nat) : option index_record :=
  find (fun record => Nat.eqb (record_digest record) digest) (index_entries index).

Inductive index_refusal :=
| DuplicateData
| DigestReferenceConflict
| RecordCountOverflow.

Definition validate_insert (capacity : nat) (index : committed_index) (record : index_record) :
    option index_refusal :=
  match index_lookup index (record_digest record) with
  | Some existing =>
      if Nat.eqb (record_reference existing) (record_reference record)
      then Some DuplicateData else Some DigestReferenceConflict
  | None => if index_count index <? capacity then None else Some RecordCountOverflow
  end.

Definition insert_record (index : committed_index) (record : index_record) :
    committed_index :=
  CommittedIndex (index_entries index ++ [record]) (S (index_count index)).

Inductive index_result :=
| IndexBuilt (index : committed_index)
| IndexRefused (record : index_record) (why : index_refusal).

Fixpoint insert_all (capacity : nat) (index : committed_index) (records : list index_record) :
    index_result :=
  match records with
  | [] => IndexBuilt index
  | record :: rest =>
      match validate_insert capacity index record with
      | Some why => IndexRefused record why
      | None => insert_all capacity (insert_record index record) rest
      end
  end.

Definition cold_rebuild (capacity : nat) (records : list index_record) : index_result :=
  insert_all capacity empty_index records.

Definition index_tail (count : nat) : nat := index_header_len + index_record_len * count.

Lemma insert_all_app : forall capacity left right index,
  insert_all capacity index (left ++ right) =
  match insert_all capacity index left with
  | IndexBuilt middle => insert_all capacity middle right
  | IndexRefused record why => IndexRefused record why
  end.
Proof.
  intros capacity left. induction left as [|record rest IH]; intros right index;
    [reflexivity|].
  cbn [app insert_all].
  destruct (validate_insert capacity index record); cbv beta iota; [reflexivity|].
  apply IH.
Qed.

Lemma insert_all_built : forall capacity records index final,
  insert_all capacity index records = IndexBuilt final ->
  index_entries final = index_entries index ++ records /\
  index_count final = index_count index + length records.
Proof.
  intros capacity records. induction records as [|record rest IH]; intros index final built.
  - cbn [insert_all] in built. injection built as <-. rewrite app_nil_r. cbn [length].
    split; [reflexivity|lia].
  - cbn [insert_all] in built.
    destruct (validate_insert capacity index record); cbv beta iota in built;
      [discriminate built|].
    destruct (IH _ _ built) as [entries count]. rewrite entries, count.
    cbn [insert_record index_entries index_count length].
    split; [rewrite <- app_assoc; reflexivity|lia].
Qed.

(** Capstone: the trusted index is the cold rebuild.  Appending the plan to
    the predecessor's index, which is the cold rebuild of its records, gives
    the cold rebuild of the records followed by the plan; a built index holds
    exactly those records in order, and its tail address determines its
    count. *)
Theorem appended_index_is_cold_rebuild : forall capacity prefix plan index,
  cold_rebuild capacity prefix = IndexBuilt index ->
  insert_all capacity index plan = cold_rebuild capacity (prefix ++ plan) /\
  (forall final, insert_all capacity index plan = IndexBuilt final ->
     index_entries final = prefix ++ plan /\
     index_count final = length (prefix ++ plan) /\
     (index_tail (index_count final) - index_header_len) / index_record_len =
       index_count final).
Proof.
  intros capacity prefix plan index rebuilt.
  split.
  - unfold cold_rebuild in *. rewrite insert_all_app, rebuilt. reflexivity.
  - intros final built.
    destruct (insert_all_built capacity prefix empty_index index rebuilt)
      as [prefix_entries prefix_count].
    destruct (insert_all_built capacity plan index final built) as [entries count].
    cbn [index_entries index_count empty_index] in prefix_entries, prefix_count.
    split; [rewrite entries, prefix_entries; reflexivity|].
    split; [rewrite count, prefix_count, length_app; lia|].
    unfold index_tail, index_header_len, index_record_len.
    replace (64 + 72 * index_count final - 64) with (index_count final * 72) by lia.
    apply Nat.div_mul. discriminate.
Qed.

(** * The authority core, the sealed receipt and the binding *)

Record authority_core := AuthorityCore {
  core_state : retention_entries;
  core_metadata_roots : counter;
  core_ledger : shared_ledger;
  core_retained_states : nat;
  core_maxima : nat * nat;
  core_shape : shape_summary;
  core_validity : nat;
  core_profile : nat;
  core_index_records : list index_record;
  core_index : committed_index
}.

Record sealed_receipt := SealedReceipt {
  receipt_base : nat;
  receipt_target : nat;
  receipt_profile : nat;
  receipt_generation : nat;
  receipt_metadata : root_delta;
  receipt_shape : shape_summary;
  receipt_index_plan : list index_record
}.

Record prepared_trusted_successor := PreparedTrustedSuccessor {
  next_state : retention_entries;
  next_metadata_roots : counter;
  next_ledger : shared_ledger;
  next_retained_states : nat;
  next_maxima : nat * nat;
  next_shape : shape_summary;
  next_validity : nat;
  next_index_plan : list index_record;
  bound_policy : trusted_policy
}.

Definition commit_step (receipt : sealed_receipt) : retention_step :=
  CommitStep (RetentionEntry (receipt_generation receipt) (receipt_target receipt)).

Definition trusted_delta (core : authority_core) (receipt : sealed_receipt) : composed_delta :=
  canonical_delta (core_state core) (commit_step receipt) (receipt_metadata receipt) no_change.

Definition pair_max (left right : nat * nat) : nat * nat :=
  (Nat.max (fst left) (fst right), Nat.max (snd left) (snd right)).

Definition final_inventory (evidence : prepared_evidence) : nat * nat :=
  (prepared_objects evidence, prepared_bytes evidence).

Definition successor_maxima (core : authority_core) (evidence : composed_evidence) :
    nat * nat :=
  pair_max (core_maxima core) (final_inventory (current_prepared evidence)).

Inductive bind_refusal :=
| ForeignBase
| ProfileMismatch
| TrialRefusedAt (refusing : domain) (why : refusal)
| CeilingRefused (axis : ceiling_axis).

Inductive bind_result :=
| BindPrepared (prepared : prepared_trusted_successor)
| BindRefused (why : bind_refusal).

(** The binding: the receipt's base must be the core's validity token and its
    profile the core's profile; then the three domain trials run in production
    order under the policy's budgets, and the successor's maxima, retained-state
    count and shape are checked against the policy's ceilings.  Nothing is
    written. *)
Definition trusted_bind (variant : trusted_variant) (env : composed_environment)
    (schedule : nat -> bool) (core : authority_core) (receipt : sealed_receipt)
    (policy : trusted_policy) : bind_result :=
  if Nat.eqb (receipt_base receipt) (core_validity core) then
    if Nat.eqb (receipt_profile receipt) (core_profile core) then
      match production_order variant env (policy_budget policy) schedule (core_ledger core)
              (trusted_delta core receipt) with
      | ProductionRefused refusing why => BindRefused (TrialRefusedAt refusing why)
      | ProductionPrepared successor evidence =>
          match first_ceiling_exceeded (policy_state policy)
                  (successor_values (successor_maxima core evidence)
                     (S (core_retained_states core)) (receipt_shape receipt)) with
          | Some axis => BindRefused (CeilingRefused axis)
          | None =>
              BindPrepared (PreparedTrustedSuccessor
                (step_entries (core_state core) (commit_step receipt))
                (apply_delta (core_metadata_roots core) (receipt_metadata receipt))
                successor (S (core_retained_states core)) (successor_maxima core evidence)
                (receipt_shape receipt) (receipt_target receipt) (receipt_index_plan receipt)
                policy)
          end
      end
    else BindRefused ProfileMismatch
  else BindRefused ForeignBase.

Lemma trusted_bind_prepared_inversion :
  forall variant env schedule core receipt policy prepared,
  trusted_bind variant env schedule core receipt policy = BindPrepared prepared ->
  receipt_base receipt = core_validity core /\
  receipt_profile receipt = core_profile core /\
  exists successor evidence,
    production_order variant env (policy_budget policy) schedule (core_ledger core)
      (trusted_delta core receipt) = ProductionPrepared successor evidence /\
    first_ceiling_exceeded (policy_state policy)
      (successor_values (successor_maxima core evidence) (S (core_retained_states core))
         (receipt_shape receipt)) = None /\
    prepared = PreparedTrustedSuccessor (step_entries (core_state core) (commit_step receipt))
      (apply_delta (core_metadata_roots core) (receipt_metadata receipt)) successor
      (S (core_retained_states core)) (successor_maxima core evidence) (receipt_shape receipt)
      (receipt_target receipt) (receipt_index_plan receipt) policy.
Proof.
  intros variant env schedule core receipt policy prepared bound. unfold trusted_bind in bound.
  destruct (Nat.eqb (receipt_base receipt) (core_validity core)) eqn:base_check;
    [|discriminate bound].
  destruct (Nat.eqb (receipt_profile receipt) (core_profile core)) eqn:profile_check;
    [|discriminate bound].
  apply Nat.eqb_eq in base_check. apply Nat.eqb_eq in profile_check.
  split; [exact base_check|]. split; [exact profile_check|].
  destruct (production_order variant env (policy_budget policy) schedule (core_ledger core)
    (trusted_delta core receipt)) as [successor evidence|refusing why];
    [|discriminate bound].
  cbv beta iota in bound.
  destruct (first_ceiling_exceeded (policy_state policy)
    (successor_values (successor_maxima core evidence) (S (core_retained_states core))
       (receipt_shape receipt))) as [axis|] eqn:ceilings; [discriminate bound|].
  injection bound as <-. exists successor, evidence.
  split; [reflexivity|]. split; [exact ceilings|reflexivity].
Qed.

(** * Publication modes and their preflights *)

Record cold_demand := ColdDemand {
  demand_objects : nat;
  demand_bytes : nat;
  demand_references : nat
}.

Definition cold_demand_eq_dec (left right : cold_demand) : {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition cold_within (demand limits : cold_demand) : bool :=
  (demand_objects demand <=? demand_objects limits) &&
  (demand_bytes demand <=? demand_bytes limits) &&
  (demand_references demand <=? demand_references limits).

(** The binding sealed into a transaction: the cold-checked mode seals its
    admission limits, the trusted mode its prepared successor and policy. *)
Inductive sealed_binding :=
| ColdCheckedBinding (limits : cold_demand)
| TrustedBinding (prepared : prepared_trusted_successor).

Inductive preflight_refusal :=
| PublicationModeMismatch
| TrustedPolicyMismatch
| AdmissionPolicyMismatch.

Definition preflight_trusted (variant : trusted_variant) (binding : sealed_binding)
    (policy : trusted_policy) : option preflight_refusal :=
  match binding with
  | ColdCheckedBinding _ => Some PublicationModeMismatch
  | TrustedBinding prepared =>
      match variant with
      | IgnoreSealedPolicy => None
      | _ =>
          if trusted_policy_eq_dec (bound_policy prepared) policy then None
          else Some TrustedPolicyMismatch
      end
  end.

Definition preflight_cold (binding : sealed_binding) (limits : cold_demand) :
    option preflight_refusal :=
  match binding with
  | TrustedBinding _ => Some PublicationModeMismatch
  | ColdCheckedBinding sealed =>
      if cold_demand_eq_dec sealed limits then None else Some AdmissionPolicyMismatch
  end.

Lemma preflight_trusted_honest : forall variant prepared policy,
  variant <> IgnoreSealedPolicy ->
  preflight_trusted variant (TrustedBinding prepared) policy =
  if trusted_policy_eq_dec (bound_policy prepared) policy then None
  else Some TrustedPolicyMismatch.
Proof.
  intros [] prepared policy honest; try reflexivity.
  exfalso. apply honest. reflexivity.
Qed.

(** Capstone: the sealed mode and policy bind the publication.  An honest
    trusted preflight passes exactly a trusted binding sealed with the
    publication's policy, the cold preflight passes exactly a cold-checked
    binding sealed with the publication's limits, and no binding passes
    both. *)
Theorem sealed_policy_binding_is_exact :
  (forall variant binding policy, variant <> IgnoreSealedPolicy ->
     preflight_trusted variant binding policy = None <->
     exists prepared, binding = TrustedBinding prepared /\ bound_policy prepared = policy) /\
  (forall binding limits,
     preflight_cold binding limits = None <-> binding = ColdCheckedBinding limits) /\
  (forall binding policy limits,
     preflight_trusted TrustedFaithful binding policy = None ->
     preflight_cold binding limits <> None).
Proof.
  split; [|split].
  - intros variant binding policy honest. destruct binding as [limits|prepared].
    + cbn [preflight_trusted]. split; [intros impossible; discriminate impossible|].
      intros (prepared & impossible & _). discriminate impossible.
    + rewrite (preflight_trusted_honest variant prepared policy honest).
      destruct (trusted_policy_eq_dec (bound_policy prepared) policy) as [same|different].
      * split; [intros _; exists prepared; split; [reflexivity|exact same]|].
        intros _. reflexivity.
      * split; [intros impossible; discriminate impossible|].
        intros (found & same_binding & found_policy). injection same_binding as <-.
        contradiction (different found_policy).
  - intros binding limits. destruct binding as [sealed|prepared]; cbn [preflight_cold].
    + destruct (cold_demand_eq_dec sealed limits) as [same|different].
      * split; [intros _; rewrite same; reflexivity|intros _; reflexivity].
      * split; [intros impossible; discriminate impossible|].
        intros same. injection same as same_limits. contradiction (different same_limits).
    + split; intros impossible; discriminate impossible.
  - intros binding policy limits trusted. destruct binding as [sealed|prepared].
    + cbn [preflight_trusted] in trusted. discriminate trusted.
    + cbn [preflight_cold]. intros impossible. discriminate impossible.
Qed.

(** * Trusted publication *)

Inductive publication_fault :=
| NoFault
| PinFault
| CatalogMismatch
| CatalogInterrupted
| DirectorySyncFault
| SidecarFault.

Inductive not_published_stage :=
| BindStage (why : bind_refusal)
| PreflightStage (why : preflight_refusal)
| AssemblyStage (record : index_record) (why : index_refusal)
| PinStage
| CatalogStage.

Inductive published_failure :=
| DirectorySyncFailed
| SidecarResolutionFailed.

Inductive publication_outcome :=
| TrustedReady (successor : authority_core)
| DefinitelyNotPublished (stage : not_published_stage)
| PublishedFailure (failure : published_failure)
| CommitIndeterminate.

(** Publication events.  [PackAppended] stands for the truncation, append,
    commit and artifact syncs, [CatalogSwapped] for the catalog
    compare-and-swap, and [SidecarResolved] for the sidecar resolution; these
    three mutate durable state. *)
Inductive publication_event :=
| ReceiptChecked
| TrialRan (target : domain)
| CeilingsChecked
| PolicyPreflight
| IndexAssembled
| PackAppended (records : list index_record)
| StorePinned
| CatalogRefusedSwap
| CatalogSwapped
| CatalogDirectorySynced
| SidecarResolved.

Definition mutates (event : publication_event) : bool :=
  match event with
  | PackAppended _ | CatalogSwapped | SidecarResolved => true
  | _ => false
  end.

Definition bind_checks : list publication_event :=
  [ReceiptChecked; TrialRan CurrentDomain; TrialRan RetainedDomain; TrialRan MetadataDomain;
   CeilingsChecked].

Definition trials_through (refusing : domain) : list publication_event :=
  match refusing with
  | CurrentDomain => [TrialRan CurrentDomain]
  | RetainedDomain => [TrialRan CurrentDomain; TrialRan RetainedDomain]
  | MetadataDomain =>
      [TrialRan CurrentDomain; TrialRan RetainedDomain; TrialRan MetadataDomain]
  end.

Definition bind_trace (result : bind_result) : list publication_event :=
  match result with
  | BindRefused ForeignBase => [ReceiptChecked]
  | BindRefused ProfileMismatch => [ReceiptChecked]
  | BindRefused (TrialRefusedAt refusing _) => ReceiptChecked :: trials_through refusing
  | BindRefused (CeilingRefused _) => bind_checks
  | BindPrepared _ => bind_checks
  end.

Definition durable_suffix (fault : publication_fault) : list publication_event :=
  match fault with
  | NoFault => [StorePinned; CatalogSwapped; CatalogDirectorySynced; SidecarResolved]
  | PinFault => []
  | CatalogMismatch => [StorePinned; CatalogRefusedSwap]
  | CatalogInterrupted => [StorePinned; CatalogSwapped]
  | DirectorySyncFault => [StorePinned; CatalogSwapped]
  | SidecarFault => [StorePinned; CatalogSwapped; CatalogDirectorySynced]
  end.

Definition assembled_core (core : authority_core) (prepared : prepared_trusted_successor)
    (index : committed_index) : authority_core :=
  AuthorityCore (next_state prepared) (next_metadata_roots prepared) (next_ledger prepared)
    (next_retained_states prepared) (next_maxima prepared) (next_shape prepared)
    (next_validity prepared) (core_profile core)
    (core_index_records core ++ next_index_plan prepared) index.

Definition fault_outcome (fault : publication_fault) (successor : authority_core) :
    publication_outcome :=
  match fault with
  | NoFault => TrustedReady successor
  | PinFault => DefinitelyNotPublished PinStage
  | CatalogMismatch => DefinitelyNotPublished CatalogStage
  | CatalogInterrupted => CommitIndeterminate
  | DirectorySyncFault => PublishedFailure DirectorySyncFailed
  | SidecarFault => PublishedFailure SidecarResolutionFailed
  end.

Definition publish_sealed (variant : trusted_variant) (capacity : nat)
    (core : authority_core) (binding : sealed_binding) (policy : trusted_policy)
    (fault : publication_fault) : publication_outcome * list publication_event :=
  match preflight_trusted variant binding policy with
  | Some why => (DefinitelyNotPublished (PreflightStage why), [PolicyPreflight])
  | None =>
      match binding with
      | ColdCheckedBinding _ =>
          (DefinitelyNotPublished (PreflightStage PublicationModeMismatch), [PolicyPreflight])
      | TrustedBinding prepared =>
          match insert_all capacity (core_index core) (next_index_plan prepared) with
          | IndexRefused record why =>
              (DefinitelyNotPublished (AssemblyStage record why),
               [PolicyPreflight; IndexAssembled])
          | IndexBuilt index =>
              (fault_outcome fault (assembled_core core prepared index),
               [PolicyPreflight; IndexAssembled; PackAppended (next_index_plan prepared)] ++
                 durable_suffix fault)
          end
      end
  end.

Definition early_events (variant : trusted_variant) (plan : list index_record) :
    list publication_event :=
  match variant with
  | PublishBeforeTrials => [PackAppended plan; StorePinned; CatalogSwapped]
  | _ => []
  end.

(** One trusted incremental publication: bind under [bind_policy], then
    publish the sealed binding under [publication_policy]. *)
Definition trusted_publish (variant : trusted_variant) (env : composed_environment)
    (schedule : nat -> bool) (capacity : nat) (core : authority_core)
    (receipt : sealed_receipt) (bind_policy publication_policy : trusted_policy)
    (fault : publication_fault) : publication_outcome * list publication_event :=
  match trusted_bind variant env schedule core receipt bind_policy with
  | BindRefused why =>
      (DefinitelyNotPublished (BindStage why),
       early_events variant (receipt_index_plan receipt) ++ bind_trace (BindRefused why))
  | BindPrepared prepared =>
      (fst (publish_sealed variant capacity core (TrustedBinding prepared) publication_policy
              fault),
       early_events variant (receipt_index_plan receipt) ++ bind_checks ++
         snd (publish_sealed variant capacity core (TrustedBinding prepared)
                publication_policy fault))
  end.

Definition refused_before_mutation (result : publication_outcome) : bool :=
  match result with
  | DefinitelyNotPublished (BindStage _) => true
  | DefinitelyNotPublished (PreflightStage _) => true
  | DefinitelyNotPublished (AssemblyStage _ _) => true
  | _ => false
  end.

Definition trusted_safe_prefix : list publication_event :=
  bind_checks ++ [PolicyPreflight; IndexAssembled].

Lemma trusted_publish_ready_inversion :
  forall env schedule capacity core receipt policy fault successor trace,
  trusted_publish TrustedFaithful env schedule capacity core receipt policy policy fault =
    (TrustedReady successor, trace) ->
  fault = NoFault /\
  exists prepared index,
    trusted_bind TrustedFaithful env schedule core receipt policy = BindPrepared prepared /\
    insert_all capacity (core_index core) (next_index_plan prepared) = IndexBuilt index /\
    successor = assembled_core core prepared index.
Proof.
  intros env schedule capacity core receipt policy fault successor trace run.
  unfold trusted_publish in run.
  destruct (trusted_bind TrustedFaithful env schedule core receipt policy)
    as [prepared|why] eqn:bound; cbv beta iota in run; [|discriminate run].
  destruct (trusted_bind_prepared_inversion _ _ _ _ _ _ _ bound)
    as (_ & _ & shared & evidence & _ & _ & same_prepared).
  assert (policy_bound : bound_policy prepared = policy)
    by (rewrite same_prepared; reflexivity).
  injection run as ready _.
  unfold publish_sealed in ready. cbn [preflight_trusted] in ready.
  destruct (trusted_policy_eq_dec (bound_policy prepared) policy) as [policy_same|different];
    [|contradiction (different policy_bound)].
  destruct (insert_all capacity (core_index core) (next_index_plan prepared))
    as [index|record why] eqn:assembled; cbv beta iota in ready; [|discriminate ready].
  destruct fault; cbn [fault_outcome fst] in ready; try discriminate ready.
  injection ready as <-.
  split; [reflexivity|]. exists prepared, index.
  split; [reflexivity|]. split; [exact assembled|reflexivity].
Qed.

Lemma trusted_publish_trace_shape :
  forall env schedule capacity core receipt bind_policy publication_policy fault published
    trace,
  trusted_publish TrustedFaithful env schedule capacity core receipt bind_policy
    publication_policy fault = (published, trace) ->
  (refused_before_mutation published = true /\ existsb mutates trace = false) \/
  (refused_before_mutation published = false /\ bind_policy = publication_policy /\
   exists plan, trace = trusted_safe_prefix ++ PackAppended plan :: durable_suffix fault).
Proof.
  intros env schedule capacity core receipt bind_policy publication_policy fault published
    trace run.
  unfold trusted_publish in run.
  destruct (trusted_bind TrustedFaithful env schedule core receipt bind_policy)
    as [prepared|why] eqn:bound; cbv beta iota in run; injection run as <- <-.
  - destruct (trusted_bind_prepared_inversion _ _ _ _ _ _ _ bound)
      as (_ & _ & shared & evidence & _ & _ & same_prepared).
    assert (policy_bound : bound_policy prepared = bind_policy)
      by (rewrite same_prepared; reflexivity).
    unfold publish_sealed. cbn [preflight_trusted].
    destruct (trusted_policy_eq_dec (bound_policy prepared) publication_policy)
      as [same|different].
    + destruct (insert_all capacity (core_index core) (next_index_plan prepared))
        as [index|record why].
      * right. split; [destruct fault; reflexivity|].
        split; [congruence|]. exists (next_index_plan prepared). reflexivity.
      * left. split; reflexivity.
    + left. split; reflexivity.
  - left. split; [reflexivity|].
    destruct why as [| |refusing refused|axis]; try reflexivity.
    destruct refusing; reflexivity.
Qed.

Lemma mutation_free_has_no_mutating_event : forall trace before event after,
  existsb mutates trace = false -> trace = before ++ event :: after -> mutates event = false.
Proof.
  intros trace before event after free ->.
  rewrite existsb_app in free. cbn [existsb] in free.
  apply orb_false_iff in free. destruct free as [_ free].
  apply orb_false_iff in free. destruct free as [event_free _]. exact event_free.
Qed.

Lemma first_mutation_follows_safe_prefix : forall safe rest before event after,
  existsb mutates safe = false ->
  safe ++ rest = before ++ event :: after -> mutates event = true ->
  exists middle, before = safe ++ middle.
Proof.
  induction safe as [|head tail IH]; intros rest before event after free split_trace mutating.
  - exists before. reflexivity.
  - cbn [existsb] in free. apply orb_false_iff in free. destruct free as [head_free tail_free].
    destruct before as [|first before].
    + cbn [app] in split_trace. injection split_trace as same _. subst head.
      rewrite head_free in mutating. discriminate mutating.
    + cbn [app] in split_trace. injection split_trace as same rest_split. subst first.
      destruct (IH rest before event after tail_free rest_split mutating) as [middle ->].
      exists middle. reflexivity.
Qed.

(** Capstone: every trial and check precedes the first durable mutation.  A
    publication refused at the binding, the preflight or the index assembly
    mutates nothing; every mutating event of a trusted publication follows the
    receipt check, the three domain trials, the ceiling check, the policy
    preflight and the index assembly; and a publication that mutates was bound
    under its publication policy. *)
Theorem trials_precede_mutation :
  forall env schedule capacity core receipt bind_policy publication_policy fault published
    trace,
  trusted_publish TrustedFaithful env schedule capacity core receipt bind_policy
    publication_policy fault = (published, trace) ->
  (refused_before_mutation published = true -> existsb mutates trace = false) /\
  (forall before event after, trace = before ++ event :: after -> mutates event = true ->
     exists middle, before = trusted_safe_prefix ++ middle) /\
  (existsb mutates trace = true -> bind_policy = publication_policy).
Proof.
  intros env schedule capacity core receipt bind_policy publication_policy fault published
    trace run.
  destruct (trusted_publish_trace_shape _ _ _ _ _ _ _ _ _ _ run)
    as [[refused free]|[unrefused [same (plan & ->)]]].
  - split; [intros _; exact free|]. split.
    + intros before event after split_trace mutating.
      rewrite (mutation_free_has_no_mutating_event trace before event after free split_trace)
        in mutating.
      discriminate mutating.
    + intros mutating. rewrite free in mutating. discriminate mutating.
  - split; [intros refused; rewrite unrefused in refused; discriminate refused|]. split.
    + intros before event after split_trace mutating.
      exact (first_mutation_follows_safe_prefix trusted_safe_prefix
        (PackAppended plan :: durable_suffix fault) before event after eq_refl split_trace
        mutating).
    + intros _. exact same.
Qed.

(** * The journal owner

    [DurableRSpaceJournalOwnerV1]: a checkpoint first reconciles a pending
    publication; an empty journal publishes nothing; a captured journal
    publishes in trusted mode under the trusted policy alone.  Reconciliation
    acknowledges an awaiting successor before consulting any limit and runs the
    cold recovery only when recovery is required. *)

Inductive owner_phase :=
| OwnerReady
| OwnerAwaitingAck (successor : nat)
| OwnerRecoveryRequired.

Inductive owner_journal :=
| EmptyJournal
| CapturedJournal.

Record owner_limits := OwnerLimits {
  owner_cold : cold_demand;
  owner_trusted : trusted_policy
}.

Inductive owner_event :=
| OwnerNoChange
| OwnerTrustedPublished
| OwnerAcknowledgedOnce (successor : nat)
| OwnerColdRecovered.

Inductive owner_outcome :=
| OwnerAccepted (state : nat)
| OwnerRefused (next : owner_phase).

Definition owner_capture (current : nat) (journal : owner_journal) (limits : owner_limits)
    (publish : trusted_policy -> publication_outcome * list publication_event) :
    owner_outcome * list owner_event :=
  match journal with
  | EmptyJournal => (OwnerAccepted current, [OwnerNoChange])
  | CapturedJournal =>
      match fst (publish (owner_trusted limits)) with
      | TrustedReady successor =>
          (OwnerAccepted (core_validity successor),
           [OwnerTrustedPublished; OwnerAcknowledgedOnce (core_validity successor)])
      | DefinitelyNotPublished _ => (OwnerRefused OwnerReady, [])
      | PublishedFailure _ => (OwnerRefused OwnerRecoveryRequired, [OwnerTrustedPublished])
      | CommitIndeterminate => (OwnerRefused OwnerRecoveryRequired, [OwnerTrustedPublished])
      end
  end.

Definition owner_reconcile (current : nat) (pending : owner_phase) (limits : owner_limits)
    (recovery_demand : cold_demand) (recovered : nat) : owner_outcome * list owner_event :=
  match pending with
  | OwnerReady => (OwnerAccepted current, [])
  | OwnerAwaitingAck successor => (OwnerAccepted successor, [OwnerAcknowledgedOnce successor])
  | OwnerRecoveryRequired =>
      if cold_within recovery_demand (owner_cold limits)
      then (OwnerAccepted recovered,
            OwnerColdRecovered ::
              (if Nat.eqb recovered current then [] else [OwnerAcknowledgedOnce recovered]))
      else (OwnerRefused OwnerRecoveryRequired, [])
  end.

Definition owner_checkpoint (current : nat) (pending : owner_phase) (journal : owner_journal)
    (limits : owner_limits) (recovery_demand : cold_demand) (recovered : nat)
    (publish : trusted_policy -> publication_outcome * list publication_event) :
    owner_outcome * list owner_event :=
  match pending with
  | OwnerReady => owner_capture current journal limits publish
  | _ =>
      match owner_reconcile current pending limits recovery_demand recovered with
      | (OwnerRefused next, events) => (OwnerRefused next, events)
      | (OwnerAccepted state, events) =>
          if Nat.eqb state current
          then (fst (owner_capture current journal limits publish),
                events ++ snd (owner_capture current journal limits publish))
          else (OwnerAccepted state, events)
      end
  end.

(** Capstone: the owner's empty checkpoint and its awaiting-acknowledgement
    retry consult no limit, a ready or awaiting owner never consults the cold
    limits, and the recovery branch does. *)
Theorem empty_and_awaiting_ack_branches_do_not_consult_cold_limits :
  (forall current limits recovery_demand recovered publish,
     owner_checkpoint current OwnerReady EmptyJournal limits recovery_demand recovered
       publish = (OwnerAccepted current, [OwnerNoChange])) /\
  (forall current successor limits recovery_demand recovered,
     owner_reconcile current (OwnerAwaitingAck successor) limits recovery_demand recovered =
       (OwnerAccepted successor, [OwnerAcknowledgedOnce successor])) /\
  (forall current pending journal cold other_cold trusted recovery_demand recovered publish,
     pending <> OwnerRecoveryRequired ->
     owner_checkpoint current pending journal (OwnerLimits cold trusted) recovery_demand
       recovered publish =
     owner_checkpoint current pending journal (OwnerLimits other_cold trusted)
       recovery_demand recovered publish) /\
  (exists current recovery_demand recovered cold other_cold trusted,
     fst (owner_reconcile current OwnerRecoveryRequired (OwnerLimits cold trusted)
       recovery_demand recovered) <>
     fst (owner_reconcile current OwnerRecoveryRequired (OwnerLimits other_cold trusted)
       recovery_demand recovered)).
Proof.
  split; [intros; reflexivity|].
  split; [intros; reflexivity|].
  split.
  - intros current pending journal cold other_cold trusted recovery_demand recovered publish
      honest.
    destruct pending as [|successor|]; [reflexivity|reflexivity|].
    exfalso. apply honest. reflexivity.
  - exists 0, (ColdDemand 1 1 1), 1, (ColdDemand 1 1 1), (ColdDemand 0 0 0), generous_policy.
    cbn. intros same. discriminate same.
Qed.

(** * Retained-state maxima

    Cold admission measures every retained state's closure and folds the
    componentwise maximum of their inventories; the per-state ceilings bound
    every state.  The trusted core carries the maxima and updates them with the
    target's inventory on each commit. *)

Definition state_inventory_witness (env : trial_environment) (manifest : nat)
    (objects bytes : nat) : Prop :=
  exists state, coherent state /\
    quiescent (graph env) (universe env) (occurrences [manifest]) state /\
    inventory (universe env) state unit_weight = objects /\
    inventory (universe env) state (object_bytes env) = bytes.

Definition maxima_of (values : list (nat * nat)) : nat * nat :=
  fold_left pair_max values (0, 0).

Definition witnessed_inventories (env : trial_environment) (entries : list retention_entry)
    (values : list (nat * nat)) : Prop :=
  Forall2 (fun entry value =>
    state_inventory_witness env (entry_manifest entry) (fst value) (snd value))
    entries values.

Definition cold_retained_maxima (env : trial_environment) (state : retention_entries)
    (maxima : nat * nat) : Prop :=
  exists values, witnessed_inventories env (all_entries state) values /\
    maxima = maxima_of values.

Definition per_state_preflight (values : list (nat * nat)) (objects_limit bytes_limit : nat) :
    bool :=
  forallb (fun value => (fst value <=? objects_limit) && (snd value <=? bytes_limit)) values.

Lemma fst_pair_max : forall left right,
  fst (pair_max left right) = Nat.max (fst left) (fst right).
Proof. intros; reflexivity. Qed.

Lemma snd_pair_max : forall left right,
  snd (pair_max left right) = Nat.max (snd left) (snd right).
Proof. intros; reflexivity. Qed.

Lemma fold_pair_max_monotone : forall values start,
  fst start <= fst (fold_left pair_max values start) /\
  snd start <= snd (fold_left pair_max values start).
Proof.
  induction values as [|value rest IH]; intros start; cbn [fold_left]; [lia|].
  destruct (IH (pair_max start value)) as [first second].
  rewrite fst_pair_max in first. rewrite snd_pair_max in second. lia.
Qed.

Lemma fold_pair_max_bounds : forall values start value,
  In value values ->
  fst value <= fst (fold_left pair_max values start) /\
  snd value <= snd (fold_left pair_max values start).
Proof.
  induction values as [|head rest IH]; intros start value member; [destruct member|].
  cbn [fold_left]. destruct member as [same|member].
  - subst head. destruct (fold_pair_max_monotone rest (pair_max start value))
      as [first second].
    rewrite fst_pair_max in first. rewrite snd_pair_max in second. lia.
  - exact (IH (pair_max start head) value member).
Qed.

Lemma fold_pair_max_attained_fst : forall values start,
  fst (fold_left pair_max values start) = fst start \/
  exists value, In value values /\ fst value = fst (fold_left pair_max values start).
Proof.
  induction values as [|head rest IH]; intros start; cbn [fold_left]; [left; reflexivity|].
  destruct (IH (pair_max start head)) as [first|(value & member & first)].
  - rewrite fst_pair_max in first.
    destruct (Nat.le_ge_cases (fst head) (fst start)) as [head_small|head_large].
    + left. rewrite first. lia.
    + right. exists head. split; [left; reflexivity|]. rewrite first. lia.
  - right. exists value. split; [right; exact member|exact first].
Qed.

Lemma fold_pair_max_attained_snd : forall values start,
  snd (fold_left pair_max values start) = snd start \/
  exists value, In value values /\ snd value = snd (fold_left pair_max values start).
Proof.
  induction values as [|head rest IH]; intros start; cbn [fold_left]; [left; reflexivity|].
  destruct (IH (pair_max start head)) as [second|(value & member & second)].
  - rewrite snd_pair_max in second.
    destruct (Nat.le_ge_cases (snd head) (snd start)) as [head_small|head_large].
    + left. rewrite second. lia.
    + right. exists head. split; [left; reflexivity|]. rewrite second. lia.
  - right. exists value. split; [right; exact member|exact second].
Qed.

Lemma maxima_of_snoc : forall values value,
  maxima_of (values ++ [value]) = pair_max (maxima_of values) value.
Proof. intros values value. unfold maxima_of. rewrite fold_left_app. reflexivity. Qed.

Lemma per_state_preflight_iff : forall values objects_limit bytes_limit,
  per_state_preflight values objects_limit bytes_limit = true <->
  forall value, In value values -> fst value <= objects_limit /\ snd value <= bytes_limit.
Proof.
  intros values objects_limit bytes_limit. unfold per_state_preflight.
  rewrite forallb_forall. split.
  - intros all value member. specialize (all value member). cbv beta in all.
    apply andb_true_iff in all. destruct all as [first second].
    apply Nat.leb_le in first. apply Nat.leb_le in second. split; assumption.
  - intros all value member. specialize (all value member). cbv beta.
    destruct all as [first second]. apply andb_true_iff.
    split; apply Nat.leb_le; assumption.
Qed.

Lemma maxima_of_preflight_exact : forall values,
  per_state_preflight values (fst (maxima_of values)) (snd (maxima_of values)) = true /\
  (0 < fst (maxima_of values) ->
     per_state_preflight values (pred (fst (maxima_of values))) (snd (maxima_of values)) =
       false) /\
  (0 < snd (maxima_of values) ->
     per_state_preflight values (fst (maxima_of values)) (pred (snd (maxima_of values))) =
       false).
Proof.
  intros values. unfold maxima_of.
  split; [|split].
  - apply per_state_preflight_iff. intros value member.
    exact (fold_pair_max_bounds values (0, 0) value member).
  - intros positive.
    destruct (fold_pair_max_attained_fst values (0, 0)) as [zero|(value & member & attained)].
    + cbn [fst] in zero. lia.
    + apply not_true_is_false. intros accepted.
      pose proof (proj1 (per_state_preflight_iff _ _ _) accepted) as bounded.
      destruct (bounded value member) as [too_large _]. lia.
  - intros positive.
    destruct (fold_pair_max_attained_snd values (0, 0)) as [zero|(value & member & attained)].
    + cbn [snd] in zero. lia.
    + apply not_true_is_false. intros accepted.
      pose proof (proj1 (per_state_preflight_iff _ _ _) accepted) as bounded.
      destruct (bounded value member) as [_ too_large]. lia.
Qed.

Lemma state_inventory_witness_unique :
  forall env rank manifest objects bytes other_objects other_bytes,
  edges_descend env rank ->
  state_inventory_witness env manifest objects bytes ->
  state_inventory_witness env manifest other_objects other_bytes ->
  objects = other_objects /\ bytes = other_bytes.
Proof.
  intros env rank manifest objects bytes other_objects other_bytes descending
    (left & left_coherent & left_quiescent & left_objects & left_bytes)
    (right & right_coherent & right_quiescent & right_objects & right_bytes).
  assert (same : forall vertex, active left vertex = active right vertex).
  { intros vertex. exact (proj2 (@quiescent_ledger_is_unique (graph env) (universe env)
      (occurrences [manifest]) left right rank left_coherent left_quiescent right_coherent
      right_quiescent descending vertex)). }
  subst objects bytes other_objects other_bytes. unfold inventory.
  split; apply selected_sum_extensional; exact same.
Qed.

Lemma witnessed_inventories_unique : forall env rank entries values other_values,
  edges_descend env rank ->
  witnessed_inventories env entries values -> witnessed_inventories env entries other_values ->
  values = other_values.
Proof.
  intros env rank entries values other_values descending left. unfold witnessed_inventories in *.
  revert other_values.
  induction left as [|entry value rest_entries rest_values head tail IH];
    intros other_values right.
  - inversion right. reflexivity.
  - inversion right as [|? other_value ? other_rest other_head other_tail]; subst.
    destruct (state_inventory_witness_unique env rank _ (fst value) (snd value)
      (fst other_value) (snd other_value) descending head other_head)
      as [same_objects same_bytes].
    f_equal.
    + apply injective_projections; assumption.
    + exact (IH other_rest other_tail).
Qed.

Lemma cold_retained_maxima_unique : forall env rank state maxima other,
  edges_descend env rank ->
  cold_retained_maxima env state maxima -> cold_retained_maxima env state other ->
  maxima = other.
Proof.
  intros env rank state maxima other descending (values & witnessed & ->)
    (other_values & other_witnessed & ->).
  rewrite (witnessed_inventories_unique env rank (all_entries state) values other_values
    descending witnessed other_witnessed).
  reflexivity.
Qed.

Lemma cold_retained_maxima_snoc : forall env state next maxima objects bytes,
  cold_retained_maxima env state maxima ->
  state_inventory_witness env (entry_manifest next) objects bytes ->
  cold_retained_maxima env (commit_entries state next) (pair_max maxima (objects, bytes)).
Proof.
  intros env state next maxima objects bytes (values & witnessed & ->) witness.
  exists (values ++ [(objects, bytes)]). split.
  - unfold witnessed_inventories in *.
    change (all_entries (commit_entries state next)) with (all_entries state ++ [next]).
    apply Forall2_app; [exact witnessed|].
    apply Forall2_cons; [exact witness|apply Forall2_nil].
  - rewrite maxima_of_snoc. reflexivity.
Qed.

(** Capstone: while retention only appends, the trusted maxima are the cold
    componentwise maxima.  They are unique, a commit updates them with the
    target's inventory, and the cold per-state preflight accepts at the maxima
    and refuses one below on each positive component. *)
Theorem retained_state_maxima_is_cold_componentwise_max :
  forall env rank state maxima,
  edges_descend env rank ->
  cold_retained_maxima env state maxima ->
  (forall other, cold_retained_maxima env state other -> maxima = other) /\
  (forall next objects bytes,
     state_inventory_witness env (entry_manifest next) objects bytes ->
     cold_retained_maxima env (commit_entries state next) (pair_max maxima (objects, bytes))) /\
  (forall values, witnessed_inventories env (all_entries state) values ->
     maxima = maxima_of values /\
     per_state_preflight values (fst maxima) (snd maxima) = true /\
     (0 < fst maxima -> per_state_preflight values (pred (fst maxima)) (snd maxima) = false) /\
     (0 < snd maxima -> per_state_preflight values (fst maxima) (pred (snd maxima)) = false)).
Proof.
  intros env rank state maxima descending cold.
  split.
  { intros other other_cold.
    exact (cold_retained_maxima_unique env rank state maxima other descending cold other_cold). }
  split.
  { intros next objects bytes witness.
    exact (cold_retained_maxima_snoc env state next maxima objects bytes cold witness). }
  intros values witnessed.
  assert (maxima_exact : maxima = maxima_of values).
  { exact (cold_retained_maxima_unique env rank state maxima (maxima_of values) descending cold
      (ex_intro _ values (conj witnessed eq_refl))). }
  rewrite maxima_exact. split; [reflexivity|]. exact (maxima_of_preflight_exact values).
Qed.

Definition maxima_fixture : trial_environment :=
  TrialEnvironment no_edges [0; 1; 2; 3] (fun vertex => vertex) no_reads no_reads.

Definition isolated_ledger (manifest : nat) : ledger :=
  {| counts := occurrences [manifest];
     active := fun vertex => 0 <? occurrences [manifest] vertex |}.

Lemma incoming_no_edges : forall vertices selected query,
  incoming no_edges vertices selected query = 0.
Proof.
  induction vertices as [|head tail IH]; intros selected query; [reflexivity|].
  cbn [incoming]. rewrite IH. destruct (selected head); reflexivity.
Qed.

Lemma isolated_witness : forall manifest,
  In manifest [0; 1; 2; 3] -> state_inventory_witness maxima_fixture manifest 1 manifest.
Proof.
  intros manifest member. exists (isolated_ledger manifest).
  split; [intros vertex; reflexivity|].
  split.
  - intros vertex. cbn [isolated_ledger counts active graph universe maxima_fixture].
    rewrite incoming_no_edges. lia.
  - cbn [In] in member.
    destruct member as [<-|[<-|[<-|[<-|[]]]]]; split; reflexivity.
Qed.

Definition tied_state : retention_entries :=
  RetentionEntries (RetentionEntry 2 1) [RetentionEntry 0 3; RetentionEntry 1 3].

Definition strict_state : retention_entries :=
  RetentionEntries (RetentionEntry 2 1) [RetentionEntry 0 3; RetentionEntry 1 2].

Definition eviction_victim : retention_entry := RetentionEntry 0 3.

(** Counterexample: under eviction the maxima are not a function of the
    predecessor's maxima and the victim's inventory.  Two retention states with
    the same maxima evict a victim with the same inventory, and their cold
    maxima afterwards differ, so every update rule is wrong on one of them. *)
Theorem eviction_requires_recomputation_counterexample :
  cold_retained_maxima maxima_fixture tied_state (1, 3) /\
  cold_retained_maxima maxima_fixture strict_state (1, 3) /\
  step_admissible tied_state (EvictStep eviction_victim) /\
  step_admissible strict_state (EvictStep eviction_victim) /\
  state_inventory_witness maxima_fixture (entry_manifest eviction_victim) 1 3 /\
  cold_retained_maxima maxima_fixture (step_entries tied_state (EvictStep eviction_victim))
    (1, 3) /\
  cold_retained_maxima maxima_fixture (step_entries strict_state (EvictStep eviction_victim))
    (1, 2) /\
  (forall update : nat * nat -> nat * nat -> nat * nat,
     update (1, 3) (1, 3) <> (1, 3) \/ update (1, 3) (1, 3) <> (1, 2)).
Proof.
  pose proof (isolated_witness 1 (or_intror (or_introl eq_refl))) as one.
  pose proof (isolated_witness 2 (or_intror (or_intror (or_introl eq_refl)))) as two.
  pose proof (isolated_witness 3 (or_intror (or_intror (or_intror (or_introl eq_refl)))))
    as three.
  assert (tied_all : all_entries tied_state =
    [RetentionEntry 0 3; RetentionEntry 1 3; RetentionEntry 2 1]) by reflexivity.
  assert (strict_all : all_entries strict_state =
    [RetentionEntry 0 3; RetentionEntry 1 2; RetentionEntry 2 1]) by reflexivity.
  assert (tied_after : all_entries (step_entries tied_state (EvictStep eviction_victim)) =
    [RetentionEntry 1 3; RetentionEntry 2 1]) by reflexivity.
  assert (strict_after : all_entries (step_entries strict_state (EvictStep eviction_victim)) =
    [RetentionEntry 1 2; RetentionEntry 2 1]) by reflexivity.
  split.
  { exists [(1, 3); (1, 3); (1, 1)]. split; [|reflexivity].
    unfold witnessed_inventories. rewrite tied_all.
    apply Forall2_cons; [exact three|]. apply Forall2_cons; [exact three|].
    apply Forall2_cons; [exact one|apply Forall2_nil]. }
  split.
  { exists [(1, 3); (1, 2); (1, 1)]. split; [|reflexivity].
    unfold witnessed_inventories. rewrite strict_all.
    apply Forall2_cons; [exact three|]. apply Forall2_cons; [exact two|].
    apply Forall2_cons; [exact one|apply Forall2_nil]. }
  split; [left; reflexivity|].
  split; [left; reflexivity|].
  split; [exact three|].
  split.
  { exists [(1, 3); (1, 1)]. split; [|reflexivity].
    unfold witnessed_inventories. rewrite tied_after.
    apply Forall2_cons; [exact three|]. apply Forall2_cons; [exact one|apply Forall2_nil]. }
  split.
  { exists [(1, 2); (1, 1)]. split; [|reflexivity].
    unfold witnessed_inventories. rewrite strict_after.
    apply Forall2_cons; [exact two|]. apply Forall2_cons; [exact one|apply Forall2_nil]. }
  intros update. destruct (update (1, 3) (1, 3)) as [first second].
  destruct (Nat.eq_dec second 3) as [three_bytes|other_bytes].
  - right. intros same. injection same as first_one second_two. lia.
  - left. intros same. injection same as first_one second_three.
    contradiction (other_bytes second_three).
Qed.

(** * The exact authority and the successor rule *)

Definition exact_authority (env : composed_environment) (capacity : nat)
    (semantically_valid : nat -> Prop) (core : authority_core) : Prop :=
  composed_base_valid env (core_state core) (core_metadata_roots core)
    (projections (core_ledger core)) /\
  In (entry_manifest (current_entry (core_state core))) (universe (semantic_environment env)) /\
  core_retained_states core = length (all_entries (core_state core)) /\
  cold_retained_maxima (semantic_environment env) (core_state core) (core_maxima core) /\
  core_validity core = entry_manifest (current_entry (core_state core)) /\
  semantically_valid (core_validity core) /\
  cold_rebuild capacity (core_index_records core) = IndexBuilt (core_index core).

(** Delta soundness of a receipt, the explicit premise owned by the delta
    tasks: it turns a semantically valid base into a valid target, the target
    manifest is an object of the semantic universe, and its metadata root
    changes are objects of the metadata universe. *)
Definition delta_sound (env : composed_environment) (semantically_valid : nat -> Prop)
    (receipt : sealed_receipt) : Prop :=
  (semantically_valid (receipt_base receipt) -> semantically_valid (receipt_target receipt)) /\
  In (receipt_target receipt) (universe (semantic_environment env)) /\
  Forall (fun vertex => In vertex (universe (metadata_environment env)))
    (added_roots (receipt_metadata receipt)) /\
  Forall (fun vertex => In vertex (universe (metadata_environment env)))
    (removed_roots (receipt_metadata receipt)).

Definition environment_well_formed (env : composed_environment)
    (semantic_rank metadata_rank : nat -> nat) : Prop :=
  NoDup (universe (semantic_environment env)) /\ edges_closed (semantic_environment env) /\
  edges_descend (semantic_environment env) semantic_rank /\
  NoDup (universe (metadata_environment env)) /\ edges_closed (metadata_environment env) /\
  edges_descend (metadata_environment env) metadata_rank.

Lemma current_domain_witness : forall env state base,
  domain_base_valid env (current_roots state) base ->
  state_inventory_witness env (entry_manifest (current_entry state)) (base_objects base)
    (base_bytes base).
Proof.
  intros env state base (base_coherent & roots_exact & base_quiescent & objects_exact &
    bytes_exact).
  exists (base_ledger base). split; [exact base_coherent|].
  split.
  - exact (quiescent_roots_extensional (graph env) (universe env) (base_roots base)
      (current_roots state) (base_ledger base) roots_exact base_quiescent).
  - split; symmetry; assumption.
Qed.

(** Capstone: the successor rule.  From an exact authority, a delta-sound
    receipt and a trusted publication that adopted its successor, the successor
    is an exact authority: its three domain ledgers are the coherent quiescent
    ledgers of the committed retention state and metadata roots, its maxima are
    the cold maxima, its validity token is the receipt's target and its index
    is the cold rebuild of its records. *)
Theorem trusted_successor_rule :
  forall env semantic_rank metadata_rank schedule capacity semantically_valid policy core
    receipt successor trace,
  environment_well_formed env semantic_rank metadata_rank ->
  exact_authority env capacity semantically_valid core ->
  delta_sound env semantically_valid receipt ->
  trusted_publish TrustedFaithful env schedule capacity core receipt policy policy NoFault =
    (TrustedReady successor, trace) ->
  exact_authority env capacity semantically_valid successor /\
  core_state successor = step_entries (core_state core) (commit_step receipt) /\
  core_metadata_roots successor =
    apply_delta (core_metadata_roots core) (receipt_metadata receipt) /\
  core_validity successor = receipt_target receipt /\
  core_profile successor = core_profile core /\
  core_retained_states successor = S (core_retained_states core) /\
  core_index_records successor = core_index_records core ++ receipt_index_plan receipt.
Proof.
  intros env semantic_rank metadata_rank schedule capacity semantically_valid policy core
    receipt successor trace
    (semantic_unique & semantic_closed & semantic_descending & metadata_unique &
     metadata_closed & metadata_descending)
    (base_valid & current_member & retained_count & maxima_cold & validity_current & valid &
     index_cold)
    (target_valid & target_member & metadata_added & metadata_removed) run.
  destruct (trusted_publish_ready_inversion _ _ _ _ _ _ _ _ _ run)
    as (_ & prepared & index & bound & assembled & ->).
  destruct (trusted_bind_prepared_inversion _ _ _ _ _ _ _ bound)
    as (base_check & profile_check & shared & evidence & production & ceilings & ->).
  destruct (production_order_is_run_composed env (policy_budget policy) schedule
    (core_ledger core) (trusted_delta core receipt)) as [prepared_iff _].
  destruct (proj1 (prepared_iff shared evidence) production) as [composed ->].
  assert (step_domain : Forall (fun vertex => In vertex (universe (semantic_environment env)))
    (step_vertices (core_state core) (commit_step receipt))).
  { cbn [step_vertices commit_step entry_manifest].
    apply Forall_cons; [exact target_member|].
    apply Forall_cons; [exact current_member|apply Forall_nil]. }
  destruct (canonical_composed_step_preserves_entry_domains env (policy_budget policy) schedule
    (projections (core_ledger core)) (core_state core) (commit_step receipt)
    (core_metadata_roots core) (receipt_metadata receipt) no_change semantic_rank metadata_rank
    evidence semantic_unique semantic_closed semantic_descending metadata_unique
    metadata_closed metadata_descending base_valid I step_domain metadata_added
    metadata_removed composed) as [successor_valid _].
  split.
  - split; [exact successor_valid|].
    split; [exact target_member|].
    split.
    + change (S (core_retained_states core) =
        length (all_entries (core_state core) ++
          [RetentionEntry (receipt_generation receipt) (receipt_target receipt)])).
      rewrite length_app, retained_count. cbn [length]. lia.
    + split.
      * destruct successor_valid as [current_valid _].
        exact (cold_retained_maxima_snoc (semantic_environment env) (core_state core)
          (RetentionEntry (receipt_generation receipt) (receipt_target receipt))
          (core_maxima core) (prepared_objects (current_prepared evidence))
          (prepared_bytes (current_prepared evidence)) maxima_cold
          (current_domain_witness _ _ _ current_valid)).
      * split; [reflexivity|].
        split.
        -- change (semantically_valid (receipt_target receipt)).
           apply target_valid. rewrite base_check. exact valid.
        -- change (cold_rebuild capacity (core_index_records core ++ receipt_index_plan receipt) =
             IndexBuilt index).
           destruct (appended_index_is_cold_rebuild capacity (core_index_records core)
             (receipt_index_plan receipt) (core_index core) index_cold) as [same _].
           rewrite <- same. exact assembled.
  - repeat split; reflexivity.
Qed.

(** * Cold admission and commit-only histories *)

Inductive trusted_history (env : composed_environment) (schedule : nat -> bool)
    (capacity : nat) (policy : trusted_policy) (start : authority_core) :
    authority_core -> Prop :=
| history_start : trusted_history env schedule capacity policy start start
| history_step : forall middle receipt successor trace,
    trusted_history env schedule capacity policy start middle ->
    trusted_publish TrustedFaithful env schedule capacity middle receipt policy policy
      NoFault = (TrustedReady successor, trace) ->
    trusted_history env schedule capacity policy start successor.

(** The cold metadata base: a source without retention metadata starts from
    the empty ledger and zero roots; a V2 source starts from the reader's
    occurrence-count seed of its anchor ([RetentionV2Editor]). *)
Inductive cold_metadata_base (env : trial_environment) : counter -> predecessor -> Prop :=
| cold_metadata_empty :
    cold_metadata_base env CreditedClosureLedger.zero_roots
      (Predecessor CreditedClosureLedger.empty_ledger CreditedClosureLedger.zero_roots 0 0)
| cold_metadata_seeded : forall (name : RetentionV2Editor.radix_tree -> nat) anchor root,
    RetentionV2Editor.realizes_metadata name env anchor root ->
    cold_metadata_base env (occurrences [anchor])
      (RetentionV2Editor.seed_predecessor env (occurrences [anchor])
        (RetentionV2Editor.metadata_seed name env anchor root)).

Definition cold_admitted_core (env : composed_environment) (state : retention_entries)
    (current retained : ledger) (metadata_roots : counter) (metadata : predecessor)
    (maxima : nat * nat) (shape : shape_summary) (profile : nat)
    (records : list index_record) (index : committed_index) : authority_core :=
  AuthorityCore state metadata_roots
    (shared_of (ComposedPredecessor
       (RetentionV2Editor.seed_predecessor (semantic_environment env) (current_roots state)
          current)
       (RetentionV2Editor.seed_predecessor (semantic_environment env) (retained_roots state)
          retained)
       metadata))
    (length (all_entries state)) maxima shape (entry_manifest (current_entry state)) profile
    records index.

Definition predecessor_matches_cold (env : trial_environment) (roots : counter)
    (base : predecessor) (cold : ledger) : Prop :=
  (forall vertex, counts (base_ledger base) vertex = counts cold vertex /\
     active (base_ledger base) vertex = active cold vertex) /\
  (forall vertex, base_roots base vertex = roots vertex) /\
  base_objects base = inventory (universe env) cold unit_weight /\
  base_bytes base = inventory (universe env) cold (object_bytes env).

Lemma selected_sum_nowhere : forall vertices weight,
  selected_sum vertices (fun _ => false) weight = 0.
Proof.
  induction vertices as [|head tail IH]; intros weight; [reflexivity|].
  cbn [selected_sum]. exact (IH weight).
Qed.

Lemma cold_metadata_base_valid : forall env rank roots base,
  edges_descend env rank -> cold_metadata_base env roots base -> domain_base_valid env roots base.
Proof.
  intros env rank roots base descending cold. destruct cold as [|name anchor root realized].
  - split; [intros vertex; reflexivity|].
    split; [intros vertex; reflexivity|].
    split.
    + intros vertex.
      cbn [counts active base_ledger base_roots CreditedClosureLedger.empty_ledger].
      rewrite CreditedClosureLedger.incoming_nowhere. reflexivity.
    + split; cbn [base_objects base_bytes base_ledger inventory active
        CreditedClosureLedger.empty_ledger]; symmetry; apply selected_sum_nowhere.
  - exact (proj2 (proj2 (proj2 (RetentionV2Editor.metadata_ledger_seed_is_the_cold_ledger
      name env anchor root rank descending realized)))).
Qed.

Lemma cold_admission_is_an_exact_authority :
  forall env semantic_rank metadata_rank capacity semantically_valid state current retained
    metadata_roots metadata maxima shape profile records index,
  environment_well_formed env semantic_rank metadata_rank ->
  CreditedClosureLedger.credited_build (graph (semantic_environment env))
    (universe (semantic_environment env)) (CreditedClosureLedger.current_epochs state) current ->
  CreditedClosureLedger.credited_build (graph (semantic_environment env))
    (universe (semantic_environment env)) (CreditedClosureLedger.retained_epochs state)
    retained ->
  cold_metadata_base (metadata_environment env) metadata_roots metadata ->
  In (entry_manifest (current_entry state)) (universe (semantic_environment env)) ->
  cold_retained_maxima (semantic_environment env) state maxima ->
  semantically_valid (entry_manifest (current_entry state)) ->
  cold_rebuild capacity records = IndexBuilt index ->
  exact_authority env capacity semantically_valid
    (cold_admitted_core env state current retained metadata_roots metadata maxima shape profile
       records index).
Proof.
  intros env semantic_rank metadata_rank capacity semantically_valid state current retained
    metadata_roots metadata maxima shape profile records index
    (_ & _ & _ & _ & _ & metadata_descending) current_build retained_build metadata_cold member
    maxima_cold valid index_cold.
  destruct (CreditedClosureLedger.credited_cold_build_realizes_entry_domains
    (graph (semantic_environment env)) (universe (semantic_environment env)) state current
    retained current_build retained_build)
    as [[current_coherent current_quiescent] [retained_coherent retained_quiescent]].
  split; [split; [|split]|].
  - split; [exact current_coherent|]. split; [intros vertex; reflexivity|].
    split; [exact current_quiescent|]. split; reflexivity.
  - split; [exact retained_coherent|]. split; [intros vertex; reflexivity|].
    split; [exact retained_quiescent|]. split; reflexivity.
  - exact (cold_metadata_base_valid _ _ _ _ metadata_descending metadata_cold).
  - split; [exact member|]. split; [reflexivity|]. split; [exact maxima_cold|].
    split; [reflexivity|]. split; [exact valid|exact index_cold].
Qed.

Lemma trusted_history_preserves_exact_authority :
  forall env semantic_rank metadata_rank schedule capacity semantically_valid policy start
    final,
  environment_well_formed env semantic_rank metadata_rank ->
  exact_authority env capacity semantically_valid start ->
  trusted_history env schedule capacity policy start final ->
  (forall middle receipt successor trace,
     trusted_history env schedule capacity policy start middle ->
     trusted_publish TrustedFaithful env schedule capacity middle receipt policy policy
       NoFault = (TrustedReady successor, trace) ->
     delta_sound env semantically_valid receipt) ->
  exact_authority env capacity semantically_valid final.
Proof.
  intros env semantic_rank metadata_rank schedule capacity semantically_valid policy start
    final well_formed start_exact history sound.
  induction history as [|middle receipt successor trace prefix_history IH run].
  - exact start_exact.
  - exact (proj1 (trusted_successor_rule env semantic_rank metadata_rank schedule capacity
      semantically_valid policy middle receipt successor trace well_formed IH
      (sound middle receipt successor trace prefix_history run) run)).
Qed.

Lemma valid_domain_matches_cold : forall env rank roots base cold,
  edges_descend env rank -> domain_base_valid env roots base ->
  coherent cold -> quiescent (graph env) (universe env) roots cold ->
  predecessor_matches_cold env roots base cold.
Proof.
  intros env rank roots base cold descending
    (base_coherent & roots_exact & base_quiescent & objects_exact & bytes_exact)
    cold_coherent cold_quiescent.
  assert (base_rooted : quiescent (graph env) (universe env) roots (base_ledger base)).
  { exact (quiescent_roots_extensional (graph env) (universe env) (base_roots base) roots
      (base_ledger base) roots_exact base_quiescent). }
  pose proof (@quiescent_ledger_is_unique (graph env) (universe env) roots (base_ledger base)
    cold rank base_coherent base_rooted cold_coherent cold_quiescent descending) as same.
  split; [exact same|]. split; [exact roots_exact|].
  split.
  - rewrite objects_exact. unfold inventory. apply selected_sum_extensional.
    intros vertex. exact (proj2 (same vertex)).
  - rewrite bytes_exact. unfold inventory. apply selected_sum_extensional.
    intros vertex. exact (proj2 (same vertex)).
Qed.

(** Capstone: along every commit-only history from a cold-admitted base, the
    trusted core is an exact authority and equals cold admission of the final
    state: its current and retained ledgers are the credited cold builds of the
    final retention state, its metadata ledger is the cold metadata ledger of
    its roots, its retained-state count, maxima and index are the cold
    values. *)
Theorem trusted_inventory_equals_cold_admission :
  forall env semantic_rank metadata_rank schedule capacity semantically_valid policy
    state current retained metadata_roots metadata maxima shape profile records index start
    final,
  environment_well_formed env semantic_rank metadata_rank ->
  CreditedClosureLedger.credited_build (graph (semantic_environment env))
    (universe (semantic_environment env)) (CreditedClosureLedger.current_epochs state) current ->
  CreditedClosureLedger.credited_build (graph (semantic_environment env))
    (universe (semantic_environment env)) (CreditedClosureLedger.retained_epochs state)
    retained ->
  cold_metadata_base (metadata_environment env) metadata_roots metadata ->
  In (entry_manifest (current_entry state)) (universe (semantic_environment env)) ->
  cold_retained_maxima (semantic_environment env) state maxima ->
  semantically_valid (entry_manifest (current_entry state)) ->
  cold_rebuild capacity records = IndexBuilt index ->
  start = cold_admitted_core env state current retained metadata_roots metadata maxima shape
    profile records index ->
  trusted_history env schedule capacity policy start final ->
  (forall middle receipt successor trace,
     trusted_history env schedule capacity policy start middle ->
     trusted_publish TrustedFaithful env schedule capacity middle receipt policy policy
       NoFault = (TrustedReady successor, trace) ->
     delta_sound env semantically_valid receipt) ->
  exact_authority env capacity semantically_valid final /\
  (forall current_cold retained_cold,
     CreditedClosureLedger.credited_build (graph (semantic_environment env))
       (universe (semantic_environment env))
       (CreditedClosureLedger.current_epochs (core_state final)) current_cold ->
     CreditedClosureLedger.credited_build (graph (semantic_environment env))
       (universe (semantic_environment env))
       (CreditedClosureLedger.retained_epochs (core_state final)) retained_cold ->
     predecessor_matches_cold (semantic_environment env) (current_roots (core_state final))
       (project (core_ledger final) CurrentDomain) current_cold /\
     predecessor_matches_cold (semantic_environment env) (retained_roots (core_state final))
       (project (core_ledger final) RetainedDomain) retained_cold) /\
  (forall metadata_ledger, coherent metadata_ledger ->
     quiescent (graph (metadata_environment env)) (universe (metadata_environment env))
       (core_metadata_roots final) metadata_ledger ->
     predecessor_matches_cold (metadata_environment env) (core_metadata_roots final)
       (project (core_ledger final) MetadataDomain) metadata_ledger) /\
  core_retained_states final = length (all_entries (core_state final)) /\
  (forall other, cold_retained_maxima (semantic_environment env) (core_state final) other ->
     core_maxima final = other) /\
  cold_rebuild capacity (core_index_records final) = IndexBuilt (core_index final).
Proof.
  intros env semantic_rank metadata_rank schedule capacity semantically_valid policy state
    current retained metadata_roots metadata maxima shape profile records index start final
    well_formed current_build retained_build metadata_cold member maxima_cold valid index_cold
    start_is_cold history sound.
  assert (start_exact : exact_authority env capacity semantically_valid start).
  { rewrite start_is_cold.
    exact (cold_admission_is_an_exact_authority env semantic_rank metadata_rank capacity
      semantically_valid state current retained metadata_roots metadata maxima shape profile
      records index well_formed current_build retained_build metadata_cold member maxima_cold
      valid index_cold). }
  pose proof (trusted_history_preserves_exact_authority env semantic_rank metadata_rank
    schedule capacity semantically_valid policy start final well_formed start_exact history
    sound) as final_exact.
  destruct well_formed as (_ & _ & semantic_descending & _ & _ & metadata_descending).
  pose proof final_exact as (base_valid & final_member & retained_count & final_maxima &
    final_validity & final_valid & final_index).
  destruct base_valid as (current_valid & retained_valid & metadata_valid).
  split; [exact final_exact|].
  split.
  - intros current_cold retained_cold current_cold_build retained_cold_build.
    destruct (CreditedClosureLedger.credited_cold_build_realizes_entry_domains
      (graph (semantic_environment env)) (universe (semantic_environment env))
      (core_state final) current_cold retained_cold current_cold_build retained_cold_build)
      as [[current_coherent current_quiescent] [retained_coherent retained_quiescent]].
    split.
    + exact (valid_domain_matches_cold _ _ _ _ _ semantic_descending current_valid
        current_coherent current_quiescent).
    + exact (valid_domain_matches_cold _ _ _ _ _ semantic_descending retained_valid
        retained_coherent retained_quiescent).
  - split.
    + intros metadata_ledger metadata_coherent metadata_quiescent.
      exact (valid_domain_matches_cold _ _ _ _ _ metadata_descending metadata_valid
        metadata_coherent metadata_quiescent).
    + split; [exact retained_count|]. split; [|exact final_index].
      intros other other_cold.
      exact (cold_retained_maxima_unique _ _ _ _ _ semantic_descending final_maxima
        other_cold).
Qed.

(** * Cold demand

    Cold admission bounds more than the inventory: the cold verifier's
    per-state references and work are measured from the store.  Two states
    with the same object count and bytes can have different reference counts,
    so the trusted inventory summary does not determine cold demand. *)

Definition demand_children (vertex : nat) : list nat :=
  match vertex with
  | 1 => [0; 0]
  | 3 => [2]
  | _ => []
  end.

Definition demand_weight (vertex : nat) : nat :=
  match vertex with
  | 0 => 4
  | 1 => 11
  | 2 => 5
  | 3 => 10
  | _ => 0
  end.

Definition demand_fixture : trial_environment :=
  TrialEnvironment demand_children [0; 1; 2; 3] demand_weight no_reads no_reads.

Definition state_references (env : trial_environment) (state : ledger) : nat :=
  selected_sum (universe env) (active state) (fun vertex => length (graph env vertex)).

Definition measure_cold_demand (env : trial_environment) (state : ledger) : cold_demand :=
  ColdDemand (inventory (universe env) state unit_weight)
    (inventory (universe env) state (object_bytes env)) (state_references env state).

Definition shared_child_state : ledger :=
  RetentionV2Editor.seed_ledger demand_fixture (occurrences [1])
    (fun vertex => (vertex =? 0) || (vertex =? 1)).

Definition single_child_state : ledger :=
  RetentionV2Editor.seed_ledger demand_fixture (occurrences [3])
    (fun vertex => (vertex =? 2) || (vertex =? 3)).

(** The fresh-checkpoint preflight variants: [MeasureColdDemand] measures the
    new state; the negative controls reuse the predecessor's measured demand or
    derive a demand from the trusted inventory summary. *)
Inductive cold_preflight_variant :=
| MeasureColdDemand
| ReusePredecessorDemand
| TrustedSummaryDemand.

Definition summary_demand (objects bytes : nat) : cold_demand :=
  ColdDemand objects bytes (pred objects).

Definition fresh_checkpoint_preflight (variant : cold_preflight_variant)
    (env : trial_environment) (predecessor_demand : cold_demand) (state : ledger)
    (limits : cold_demand) : bool :=
  match variant with
  | MeasureColdDemand => cold_within (measure_cold_demand env state) limits
  | ReusePredecessorDemand => cold_within predecessor_demand limits
  | TrustedSummaryDemand =>
      cold_within (summary_demand (inventory (universe env) state unit_weight)
        (inventory (universe env) state (object_bytes env))) limits
  end.

(** Capstone: cold demand is not a function of the inventory summary.  Two
    coherent quiescent states share the summary of two objects and fifteen
    bytes and differ in their reference counts, so every function from the
    summary to a cold demand is wrong on one of them. *)
Theorem cold_demand_is_not_a_function_of_the_inventory_summary :
  coherent shared_child_state /\
  quiescent (graph demand_fixture) (universe demand_fixture) (occurrences [1])
    shared_child_state /\
  coherent single_child_state /\
  quiescent (graph demand_fixture) (universe demand_fixture) (occurrences [3])
    single_child_state /\
  inventory (universe demand_fixture) shared_child_state unit_weight = 2 /\
  inventory (universe demand_fixture) single_child_state unit_weight = 2 /\
  inventory (universe demand_fixture) shared_child_state (object_bytes demand_fixture) = 15 /\
  inventory (universe demand_fixture) single_child_state (object_bytes demand_fixture) = 15 /\
  measure_cold_demand demand_fixture shared_child_state = ColdDemand 2 15 2 /\
  measure_cold_demand demand_fixture single_child_state = ColdDemand 2 15 1 /\
  (forall summarize : nat -> nat -> cold_demand,
     summarize 2 15 <> measure_cold_demand demand_fixture shared_child_state \/
     summarize 2 15 <> measure_cold_demand demand_fixture single_child_state).
Proof.
  assert (shared_demand :
    measure_cold_demand demand_fixture shared_child_state = ColdDemand 2 15 2)
    by reflexivity.
  assert (single_demand :
    measure_cold_demand demand_fixture single_child_state = ColdDemand 2 15 1)
    by reflexivity.
  split; [intros vertex; destruct vertex as [|[|[|[|vertex]]]]; reflexivity|].
  split; [intros vertex; reflexivity|].
  split; [intros vertex; destruct vertex as [|[|[|[|vertex]]]]; reflexivity|].
  split; [intros vertex; reflexivity|].
  do 4 (split; [reflexivity|]).
  split; [exact shared_demand|]. split; [exact single_demand|].
  intros summarize. rewrite shared_demand, single_demand.
  destruct (cold_demand_eq_dec (summarize 2 15) (ColdDemand 2 15 2)) as [same|different].
  - right. rewrite same. intros impossible. discriminate impossible.
  - left. exact different.
Qed.

(** * Negative controls *)

(** A fresh checkpoint that reuses the predecessor's cold demand fabricates
    cold evidence: the predecessor was genuinely within the limits, the new
    state is not, and the reused demand accepts it. *)
Theorem reused_predecessor_demand_mutant_fabricates_cold_evidence :
  fresh_checkpoint_preflight MeasureColdDemand demand_fixture
    (measure_cold_demand demand_fixture single_child_state) single_child_state
    (ColdDemand 2 15 1) = true /\
  fresh_checkpoint_preflight ReusePredecessorDemand demand_fixture
    (measure_cold_demand demand_fixture single_child_state) shared_child_state
    (ColdDemand 2 15 1) = true /\
  fresh_checkpoint_preflight MeasureColdDemand demand_fixture
    (measure_cold_demand demand_fixture single_child_state) shared_child_state
    (ColdDemand 2 15 1) = false.
Proof. vm_compute. repeat split. Qed.

(** A preflight that derives cold demand from the trusted summary accepts a
    limit one below the measured reference demand, which the measuring
    preflight refuses; the measuring preflight accepts the exact limit. *)
Theorem trusted_source_preflight_mutant_accepts_minus_one :
  fresh_checkpoint_preflight TrustedSummaryDemand demand_fixture
    (measure_cold_demand demand_fixture shared_child_state) shared_child_state
    (ColdDemand 2 15 1) = true /\
  fresh_checkpoint_preflight MeasureColdDemand demand_fixture
    (measure_cold_demand demand_fixture shared_child_state) shared_child_state
    (ColdDemand 2 15 1) = false /\
  fresh_checkpoint_preflight MeasureColdDemand demand_fixture
    (measure_cold_demand demand_fixture shared_child_state) shared_child_state
    (ColdDemand 2 15 2) = true.
Proof. vm_compute. repeat split. Qed.

Definition metadata_edges (vertex : nat) : list nat := if vertex =? 10 then [11] else [].

Definition skip_fixture : composed_environment :=
  ComposedEnvironment (fixture single_edge [0; 1; 2; 3] no_reads)
    (fixture metadata_edges [9; 10; 11] no_reads).

Definition skip_base : composed_predecessor :=
  ComposedPredecessor (established (semantic_environment skip_fixture) [1])
    (established (semantic_environment skip_fixture) [1])
    (established (metadata_environment skip_fixture) [9]).

Definition skip_core : authority_core :=
  AuthorityCore (first_entries 1) (occurrences [9]) (shared_of skip_base) 1 (2, 2) zero_shape
    1 0 [] empty_index.

Definition skip_receipt : sealed_receipt :=
  SealedReceipt 1 2 0 1 (RootDelta [10] [9]) zero_shape [].

Definition tight_policy : trusted_policy :=
  TrustedPolicy (StateCeilings 64 64 64 2 64 64 64 64 64 64 64) generous_work generous_work
    generous_work.

(** A skipped retained or metadata trial leaves that domain at the
    predecessor's ledger, so the successor's domain no longer agrees with cold
    admission: its roots miss the committed manifest or the new metadata
    anchor. *)
Theorem skipped_domain_trial_mutant_breaks_cold_agreement :
  match trusted_bind TrustedFaithful skip_fixture prefer_cursors skip_core skip_receipt
          generous_policy with
  | BindPrepared prepared =>
      domain_objects (next_ledger prepared) RetainedDomain = 3 /\
      domain_objects (next_ledger prepared) MetadataDomain = 2
  | BindRefused _ => False
  end /\
  match trusted_bind SkipRetainedTrial skip_fixture prefer_cursors skip_core skip_receipt
          generous_policy with
  | BindPrepared prepared =>
      domain_objects (next_ledger prepared) RetainedDomain = 2 /\
      ~ domain_base_valid (semantic_environment skip_fixture)
          (retained_roots (next_state prepared)) (project (next_ledger prepared) RetainedDomain)
  | BindRefused _ => False
  end /\
  match trusted_bind SkipMetadataTrial skip_fixture prefer_cursors skip_core skip_receipt
          generous_policy with
  | BindPrepared prepared =>
      domain_objects (next_ledger prepared) MetadataDomain = 1 /\
      ~ domain_base_valid (metadata_environment skip_fixture) (next_metadata_roots prepared)
          (project (next_ledger prepared) MetadataDomain)
  | BindRefused _ => False
  end.
Proof.
  split; [vm_compute; split; reflexivity|].
  split.
  - vm_compute. split; [reflexivity|].
    intros (_ & roots & _). specialize (roots 2). vm_compute in roots. discriminate roots.
  - vm_compute. split; [reflexivity|].
    intros (_ & roots & _). specialize (roots 10). vm_compute in roots. discriminate roots.
Qed.

(** A publication preflight that ignores the sealed policy publishes a
    successor bound under a generous policy although the publication's policy
    refuses its retained union; the faithful preflight refuses before any
    mutation. *)
Theorem changed_policy_mutant_accepts_unbound_limits :
  fst (trusted_publish TrustedFaithful skip_fixture prefer_cursors 8 skip_core skip_receipt
         generous_policy tight_policy NoFault) =
    DefinitelyNotPublished (PreflightStage TrustedPolicyMismatch) /\
  existsb mutates (snd (trusted_publish TrustedFaithful skip_fixture prefer_cursors 8 skip_core
    skip_receipt generous_policy tight_policy NoFault)) = false /\
  match fst (trusted_publish IgnoreSealedPolicy skip_fixture prefer_cursors 8 skip_core
               skip_receipt generous_policy tight_policy NoFault) with
  | TrustedReady successor =>
      max_union_objects (policy_state tight_policy) <
        domain_objects (core_ledger successor) RetainedDomain
  | _ => False
  end /\
  trusted_bind TrustedFaithful skip_fixture prefer_cursors skip_core skip_receipt
    tight_policy = BindRefused (TrialRefusedAt RetainedDomain (BudgetRefusal FinalObjectsAxis)).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; lia|].
  vm_compute. reflexivity.
Qed.

(** A publication that appends and swaps before the trials reports the same
    refusal as the faithful publication, yet its catalog already appointed the
    refused successor. *)
Theorem publication_before_trials_mutant_publishes_refused_successor :
  fst (trusted_publish TrustedFaithful skip_fixture prefer_cursors 8 skip_core skip_receipt
         tight_policy tight_policy NoFault) =
    DefinitelyNotPublished
      (BindStage (TrialRefusedAt RetainedDomain (BudgetRefusal FinalObjectsAxis))) /\
  existsb mutates (snd (trusted_publish TrustedFaithful skip_fixture prefer_cursors 8 skip_core
    skip_receipt tight_policy tight_policy NoFault)) = false /\
  fst (trusted_publish PublishBeforeTrials skip_fixture prefer_cursors 8 skip_core skip_receipt
         tight_policy tight_policy NoFault) =
    DefinitelyNotPublished
      (BindStage (TrialRefusedAt RetainedDomain (BudgetRefusal FinalObjectsAxis))) /\
  In CatalogSwapped (snd (trusted_publish PublishBeforeTrials skip_fixture prefer_cursors 8
    skip_core skip_receipt tight_policy tight_policy NoFault)).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  vm_compute. right. right. left. reflexivity.
Qed.

End TrustedSuccessorCapability.

Print Assumptions TrustedSuccessorCapability.production_order_is_run_composed.
Print Assumptions TrustedSuccessorCapability.ceiling_decisions_are_exact.
Print Assumptions TrustedSuccessorCapability.appended_index_is_cold_rebuild.
Print Assumptions TrustedSuccessorCapability.sealed_policy_binding_is_exact.
Print Assumptions TrustedSuccessorCapability.trials_precede_mutation.
Print Assumptions TrustedSuccessorCapability.empty_and_awaiting_ack_branches_do_not_consult_cold_limits.
Print Assumptions TrustedSuccessorCapability.retained_state_maxima_is_cold_componentwise_max.
Print Assumptions TrustedSuccessorCapability.eviction_requires_recomputation_counterexample.
Print Assumptions TrustedSuccessorCapability.trusted_successor_rule.
Print Assumptions TrustedSuccessorCapability.trusted_inventory_equals_cold_admission.
Print Assumptions TrustedSuccessorCapability.cold_demand_is_not_a_function_of_the_inventory_summary.
Print Assumptions TrustedSuccessorCapability.reused_predecessor_demand_mutant_fabricates_cold_evidence.
Print Assumptions TrustedSuccessorCapability.trusted_source_preflight_mutant_accepts_minus_one.
Print Assumptions TrustedSuccessorCapability.skipped_domain_trial_mutant_breaks_cold_agreement.
Print Assumptions TrustedSuccessorCapability.changed_policy_mutant_accepts_unbound_limits.
Print Assumptions TrustedSuccessorCapability.publication_before_trials_mutant_publishes_refused_successor.
