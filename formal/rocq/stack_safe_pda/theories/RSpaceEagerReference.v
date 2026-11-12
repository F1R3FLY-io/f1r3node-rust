From Stdlib Require Import Bool List Permutation.
Import ListNotations.

(**
  The eager tuple-space reference of RSpace and the obligations that relate a
  backend to it.

  The design document [docs/design/rspace/pathmap-bag-rspace-integration.md],
  section "Correctness criterion: the Rho calculus", judges a backend of RSpace
  against the Rho calculus as the Rholang interpreter realises it,
  nondeterminism included.  Its yardstick is the eager reference: a store whose
  contents are a multiset of data per channel and a multiset of continuations
  per ordered channel list, which fires any rendezvous that an arriving produce
  or consume enables, rests an operation only when none is enabled, and lets an
  install only rest.  The document proves on paper that a backend meeting the
  obligations O1 to O4 is branching bisimilar to the reference (Theorem CC-2),
  that a backend which fires the member a fixed selector picks is a refinement
  of the reference but not bisimilar to it (Theorem CC-3), and that the
  reference never holds an enabled rendezvous at rest (Lemma CC-1).  This
  theory mechanises those statements.  [RSpaceCommit.v] takes the selection as
  an input; here the choice is part of the model.

  Model (the store view).  A configuration is the current contents and a map
  from roots to recorded contents.  Contents are two lists read as multisets:
  [contents_equiv] is equality of multisets ([Permutation] on both lists), and
  every relation of the model is closed under it.  A datum has a channel, a
  value and a persistence flag; a continuation has an ordered channel list, one
  pattern per channel, an abstract guard, a body, a persistence flag and its
  peeked positions.  An operation (produce, consume or install) arrives in the
  label of a step, together with its result: [Rested] or [Fired r] for a
  produce or a consume, and [Rested] only for an install.  RSpace stores an
  install when nothing matches and refuses it with an error otherwise
  ([RSpace::locked_install_internal], [ReplayRSpace::locked_install_internal]),
  so the reference has no step for an install whose enabled set has a member.
  A checkpoint returns a root and records the current contents under it; a
  reset to a recorded root replaces the current contents by the recorded ones.
  The pending terms of the paper and the interpreter's steps are not part of
  the state: the environment offers any operation at any time, which is the
  freedom the paper's pending terms give.

  Abstraction.  Channels, values, patterns, bindings, guards, bodies and roots
  are types of the section context.  The match relation [matches], the guard
  [guard_accepts], the root function [root_of] and a decision procedure for
  equality of roots are section variables, so every result holds for every
  instance of them.  No equality on channels or values is decided anywhere.

  Theorems.
  - [ref_step_fires_any_member_and_rests_only_when_none_is_enabled]: the
    reference fires every member of the enabled set of a produce or a consume,
    fires only members and only for a produce or a consume, and rests an
    operation exactly when the set is empty.
  - [install_never_fires] and [install_rests_exactly_when_nothing_is_enabled]:
    no reference step fires an install; an install rests exactly when its
    enabled set is empty and has no step at all while the set has a member.
  - [obligations_give_branching_bisimulation] (CC-2): an implementation that
    meets O1 to O4 is related to the reference by a branching bisimulation
    (van Glabbeek and Weijland 1996), and every reachable state is branching
    bisimilar to its abstraction.
  - [selector_resolution_is_refinement] (CC-3, part 1): every choice function
    that picks a member of each non-empty enabled set gives an implementation
    whose steps are reference steps, which the reference simulates, whose runs
    the reference matches label for label, and which never rests while a COMM
    is enabled.
  - [selector_resolution_is_not_bisimilar] (CC-3, part 2): when an enabled set
    has two different members, no choice function gives an implementation that
    is branching bisimilar to the reference;
    [fixed_selector_is_not_bisimilar] states the same of every implementation
    that meets O1 and fires the member a fixed selector picks.
  - [replay_follows_any_enabled_choice] and
    [replay_refuses_a_non_enabled_choice]: replay of a logged COMM performs a
    member of the logged event when one is enabled and the operation is a
    produce or a consume, and every step it performs is the reference step
    with the logged label; it refuses when no member is enabled, and then the
    reference has no step with that label either.
  - [rest_states_have_no_enabled_rendezvous] (CC-1): every configuration that
    the reference reaches from empty contents holds no enabled rendezvous among
    its resting items, in its current and in its recorded contents, and every
    state of an implementation that meets O1 and O2 and starts at empty
    contents reads back as such contents.

  The companion [RSpaceEagerReferenceMutants.v] shows that the premises of
  these theorems are satisfiable on a concrete instance and refutes five
  mutants: an implementation that fires a non-member, one that rests while a
  COMM is enabled, a reset that returns other contents, a reader that sorts a
  channel list, and one that fires an install.
*)

Module RSpaceEagerReference.

(** * Branching bisimulation *)

(** An action of a labelled transition system is an internal step or a
    visible step that carries a label. *)
Inductive action (Label : Type) : Type :=
| Internal : action Label
| Visible : Label -> action Label.

Arguments Internal {Label}.
Arguments Visible {Label} _.

(** [internal_star step] is the reflexive and transitive closure of the
    internal steps of [step]. *)
Inductive internal_star (State Label : Type) (step : State -> action Label -> State -> Prop)
    : State -> State -> Prop :=
| internal_star_refl : forall state, internal_star State Label step state state
| internal_star_step : forall state middle final,
    step state Internal middle ->
    internal_star State Label step middle final ->
    internal_star State Label step state final.

Arguments internal_star {State Label} step _ _.
Arguments internal_star_refl {State Label step} state.
Arguments internal_star_step {State Label step} state middle final _ _.

(** The transfer property of a relation from the steps of [left_step] to the
    steps of [right_step] (van Glabbeek and Weijland 1996, Definition 2.5):
    each step of a related left state is either an internal step that stays
    related to the same right state, or is answered by internal right steps to
    a state still related to the left state, followed by a step with the same
    action to a state related to the left target. *)
Definition transfer {Left Right Label : Type}
    (left_step : Left -> action Label -> Left -> Prop)
    (right_step : Right -> action Label -> Right -> Prop)
    (relation : Left -> Right -> Prop) : Prop :=
  forall left_state right_state performed left_state',
    relation left_state right_state ->
    left_step left_state performed left_state' ->
    (performed = Internal /\ relation left_state' right_state) \/
    (exists right_middle right_state',
        internal_star right_step right_state right_middle /\
        right_step right_middle performed right_state' /\
        relation left_state right_middle /\
        relation left_state' right_state').

Definition converse {Left Right : Type} (relation : Left -> Right -> Prop)
    : Right -> Left -> Prop :=
  fun right_state left_state => relation left_state right_state.

(** A branching bisimulation between two systems is a relation that, with its
    converse, has the transfer property. *)
Definition branching_bisimulation {Left Right Label : Type}
    (left_step : Left -> action Label -> Left -> Prop)
    (right_step : Right -> action Label -> Right -> Prop)
    (relation : Left -> Right -> Prop) : Prop :=
  transfer left_step right_step relation /\
  transfer right_step left_step (converse relation).

Definition branching_bisimilar {Left Right Label : Type}
    (left_step : Left -> action Label -> Left -> Prop)
    (right_step : Right -> action Label -> Right -> Prop)
    (left_state : Left) (right_state : Right) : Prop :=
  exists relation,
    branching_bisimulation left_step right_step relation /\ relation left_state right_state.

(** [simulated_by] keeps only the transfer property from left to right: the
    right system can answer every step of the left one. *)
Definition simulated_by {Left Right Label : Type}
    (left_step : Left -> action Label -> Left -> Prop)
    (right_step : Right -> action Label -> Right -> Prop)
    (left_state : Left) (right_state : Right) : Prop :=
  exists relation, transfer left_step right_step relation /\ relation left_state right_state.

Lemma internal_star_without_internal_steps :
  forall (State Label : Type) (step : State -> action Label -> State -> Prop)
      first_state last_state,
    (forall state state', ~ step state Internal state') ->
    internal_star step first_state last_state -> first_state = last_state.
Proof.
  intros State Label step first_state last_state no_internal path.
  destruct path as [state|state middle final internal_step _].
  - reflexivity.
  - destruct (no_internal state middle internal_step).
Qed.

(** * Lists read as multisets *)

Lemma permutation_filter :
  forall (Element : Type) (keep : Element -> bool) (one other : list Element),
    Permutation one other -> Permutation (filter keep one) (filter keep other).
Proof.
  intros Element keep one other same.
  induction same as
    [|element tail_one tail_two _ same_tail
     |element_one element_two tail
     |list_one list_two list_three _ step_one _ step_two]; simpl.
  - apply perm_nil.
  - destruct (keep element); [apply perm_skip|]; exact same_tail.
  - destruct (keep element_one); destruct (keep element_two);
      solve [apply perm_swap | apply Permutation_refl].
  - exact (Permutation_trans step_one step_two).
Qed.

(** Every member of a list on which [covered] holds either is [arriving] or
    lies in [pool].  The split is constructive: it needs no decision of
    equality on the elements. *)
Lemma covered_members_split :
  forall (Element : Type) (arriving : Element) (members pool : list Element),
    (forall member, In member members -> In member (arriving :: pool)) ->
    In arriving members \/ (forall member, In member members -> In member pool).
Proof.
  intros Element arriving members pool covered.
  induction members as [|head tail induction_hypothesis].
  - right. intros member inside. destruct inside.
  - assert (tail_covered : forall member, In member tail -> In member (arriving :: pool)).
    { intros member inside. apply covered. apply in_cons. exact inside. }
    destruct (covered head (in_eq head tail)) as [same|head_in_pool].
    + left. rewrite same. apply in_eq.
    + destruct (induction_hypothesis tail_covered) as [in_tail|tail_in_pool].
      * left. apply in_cons. exact in_tail.
      * right. intros member [same|inside].
        -- rewrite <- same. exact head_in_pool.
        -- exact (tail_in_pool member inside).
Qed.

Section EagerReference.

Context {Channel Value Pattern Binding Guard Body Root : Type}.

(** * Items, contents and configurations *)

Record datum : Type := Datum {
  datum_channel : Channel;
  datum_value : Value;
  datum_persistent : bool
}.

Record continuation : Type := Continuation {
  continuation_channels : list Channel;
  continuation_patterns : list Pattern;
  continuation_guard : Guard;
  continuation_body : Body;
  continuation_persistent : bool;
  continuation_peeks : list nat
}.

(** The resting items.  Data on a channel and continuations on an ordered
    channel list are sub-multisets of the two lists; the order of the lists
    carries no meaning, the order of a continuation's channel list does. *)
Record contents : Type := Contents {
  resting_data : list datum;
  resting_continuations : list continuation
}.

Record configuration : Type := Configuration {
  current : contents;
  recorded : Root -> option contents
}.

Inductive operation : Type :=
| Produce (arriving : datum)
| Consume (arriving : continuation)
| Install (arriving : continuation).

(** A rendezvous is a continuation with one datum per position, in the order of
    the continuation's channel list. *)
Record rendezvous : Type := Rendezvous {
  rendezvous_continuation : continuation;
  rendezvous_data : list datum
}.

Inductive result : Type :=
| Rested
| Fired (comm : rendezvous).

Inductive label : Type :=
| OpResult (op : operation) (outcome : result)
| CheckpointReturned (root : Root)
| ResetTo (root : Root).

(** * Equality of multisets *)

Definition contents_equiv (contents_one contents_two : contents) : Prop :=
  Permutation (resting_data contents_one) (resting_data contents_two) /\
  Permutation (resting_continuations contents_one) (resting_continuations contents_two).

Definition option_contents_equiv (entry_one entry_two : option contents) : Prop :=
  match entry_one, entry_two with
  | Some stored_one, Some stored_two => contents_equiv stored_one stored_two
  | None, None => True
  | _, _ => False
  end.

Definition recorded_equiv (history_one history_two : Root -> option contents) : Prop :=
  forall root, option_contents_equiv (history_one root) (history_two root).

Definition config_equiv (configuration_one configuration_two : configuration) : Prop :=
  contents_equiv (current configuration_one) (current configuration_two) /\
  recorded_equiv (recorded configuration_one) (recorded configuration_two).

Lemma contents_equiv_refl : forall contents_now, contents_equiv contents_now contents_now.
Proof. intros contents_now. split; apply Permutation_refl. Qed.

Lemma contents_equiv_sym :
  forall contents_one contents_two,
    contents_equiv contents_one contents_two -> contents_equiv contents_two contents_one.
Proof.
  intros contents_one contents_two [same_data same_continuations].
  split; apply Permutation_sym; assumption.
Qed.

Lemma contents_equiv_trans :
  forall contents_one contents_two contents_three,
    contents_equiv contents_one contents_two ->
    contents_equiv contents_two contents_three ->
    contents_equiv contents_one contents_three.
Proof.
  intros contents_one contents_two contents_three
    [data_one continuations_one] [data_two continuations_two].
  split; [exact (Permutation_trans data_one data_two)
         |exact (Permutation_trans continuations_one continuations_two)].
Qed.

Lemma option_contents_equiv_refl : forall entry, option_contents_equiv entry entry.
Proof.
  intros [stored|]; simpl; [apply contents_equiv_refl|exact I].
Qed.

Lemma option_contents_equiv_sym :
  forall entry_one entry_two,
    option_contents_equiv entry_one entry_two -> option_contents_equiv entry_two entry_one.
Proof.
  intros [stored_one|] [stored_two|]; simpl; intros equivalent.
  - apply contents_equiv_sym. exact equivalent.
  - exact equivalent.
  - exact equivalent.
  - exact I.
Qed.

Lemma option_contents_equiv_trans :
  forall entry_one entry_two entry_three,
    option_contents_equiv entry_one entry_two ->
    option_contents_equiv entry_two entry_three ->
    option_contents_equiv entry_one entry_three.
Proof.
  intros [stored_one|] [stored_two|] [stored_three|]; simpl;
    intros equivalent_one equivalent_two; try contradiction; try exact I.
  exact (contents_equiv_trans _ _ _ equivalent_one equivalent_two).
Qed.

Lemma recorded_equiv_refl : forall history, recorded_equiv history history.
Proof. intros history root. apply option_contents_equiv_refl. Qed.

Lemma recorded_equiv_sym :
  forall history_one history_two,
    recorded_equiv history_one history_two -> recorded_equiv history_two history_one.
Proof.
  intros history_one history_two equivalent root.
  apply option_contents_equiv_sym. apply equivalent.
Qed.

Lemma recorded_equiv_trans :
  forall history_one history_two history_three,
    recorded_equiv history_one history_two ->
    recorded_equiv history_two history_three ->
    recorded_equiv history_one history_three.
Proof.
  intros history_one history_two history_three equivalent_one equivalent_two root.
  exact (option_contents_equiv_trans _ _ _ (equivalent_one root) (equivalent_two root)).
Qed.

Lemma config_equiv_refl :
  forall configuration_now, config_equiv configuration_now configuration_now.
Proof.
  intros configuration_now.
  split; [apply contents_equiv_refl|apply recorded_equiv_refl].
Qed.

Lemma config_equiv_sym :
  forall configuration_one configuration_two,
    config_equiv configuration_one configuration_two ->
    config_equiv configuration_two configuration_one.
Proof.
  intros configuration_one configuration_two [same_current same_recorded].
  split; [apply contents_equiv_sym|apply recorded_equiv_sym]; assumption.
Qed.

Lemma config_equiv_trans :
  forall configuration_one configuration_two configuration_three,
    config_equiv configuration_one configuration_two ->
    config_equiv configuration_two configuration_three ->
    config_equiv configuration_one configuration_three.
Proof.
  intros configuration_one configuration_two configuration_three
    [current_one recorded_one] [current_two recorded_two].
  split; [exact (contents_equiv_trans _ _ _ current_one current_two)
         |exact (recorded_equiv_trans _ _ _ recorded_one recorded_two)].
Qed.

(** * Matching *)

(** [matches pattern value binding]: the pattern accepts the value and binds
    [binding].  [guard_accepts guard bindings]: the [where] clause of a join
    accepts the bindings of all its positions. *)
Variable matches : Pattern -> Value -> Binding -> Prop.
Variable guard_accepts : Guard -> list Binding -> Prop.

Inductive positions_match :
    list Channel -> list Pattern -> list datum -> list Binding -> Prop :=
| positions_match_nil : positions_match [] [] [] []
| positions_match_cons :
    forall channel pattern selected binding channels patterns selection bindings,
      datum_channel selected = channel ->
      matches pattern (datum_value selected) binding ->
      positions_match channels patterns selection bindings ->
      positions_match (channel :: channels) (pattern :: patterns)
        (selected :: selection) (binding :: bindings).

Definition admissible (waiting : continuation) (selection : list datum) : Prop :=
  exists bindings,
    positions_match (continuation_channels waiting) (continuation_patterns waiting)
      selection bindings /\
    guard_accepts (continuation_guard waiting) bindings.

(** * The enabled set *)

(** A non-persistent datum of a selection is consumed and fills one position;
    a persistent one is read and may fill several. *)
Definition consumed_data (selection : list datum) : list datum :=
  filter (fun selected => negb (datum_persistent selected)) selection.

Definition read_persistent_data (selection : list datum) : list datum :=
  filter datum_persistent selection.

Definition arriving_consumable (op : operation) : list datum :=
  match op with
  | Produce arriving => if datum_persistent arriving then [] else [arriving]
  | Consume _ => []
  | Install _ => []
  end.

Definition arriving_readable (op : operation) : list datum :=
  match op with
  | Produce arriving => [arriving]
  | Consume _ => []
  | Install _ => []
  end.

(** The consumed occurrences form a sub-multiset of [consumable]; every
    persistent datum read occurs in [readable]. *)
Definition data_available (consumable readable selection : list datum) : Prop :=
  (exists remainder, Permutation consumable (consumed_data selection ++ remainder)) /\
  (forall selected, In selected (read_persistent_data selection) -> In selected readable).

Definition continuation_available
    (contents_now : contents) (op : operation) (waiting : continuation) : Prop :=
  match op with
  | Produce _ => In waiting (resting_continuations contents_now)
  | Consume arriving => waiting = arriving
  | Install arriving => waiting = arriving
  end.

Definition participates (op : operation) (selection : list datum) : Prop :=
  match op with
  | Produce arriving => In arriving selection
  | Consume _ => True
  | Install _ => True
  end.

(** [enabled contents_now op candidate]: the rendezvous [candidate] is a
    member of the enabled set of [op] at [contents_now].  The arriving item
    takes part: an arriving continuation is the rendezvous' continuation, and
    an arriving datum is one of its data. *)
Definition enabled (contents_now : contents) (op : operation) (candidate : rendezvous) : Prop :=
  continuation_available contents_now op (rendezvous_continuation candidate) /\
  admissible (rendezvous_continuation candidate) (rendezvous_data candidate) /\
  participates op (rendezvous_data candidate) /\
  data_available (arriving_consumable op ++ resting_data contents_now)
    (arriving_readable op ++ resting_data contents_now) (rendezvous_data candidate).

(** A rendezvous among resting items alone. *)
Definition resting_rendezvous (contents_now : contents) (candidate : rendezvous) : Prop :=
  In (rendezvous_continuation candidate) (resting_continuations contents_now) /\
  admissible (rendezvous_continuation candidate) (rendezvous_data candidate) /\
  data_available (resting_data contents_now) (resting_data contents_now)
    (rendezvous_data candidate).

Definition at_rest (contents_now : contents) : Prop :=
  forall candidate, ~ resting_rendezvous contents_now candidate.

(** * Operations that fire *)

(** A produce or a consume fires a member of its enabled set; an install never
    fires.  Only the runtime issues installs, for its system processes at
    startup and again in [restore_installs]; [RSpace::locked_install_internal]
    and [ReplayRSpace::locked_install_internal] store the continuation when no
    selection matches and return an error otherwise.  The error is a refusal,
    not a transition.  The enabled set of an install stays defined: it is the
    precondition of its rest. *)
Definition may_fire (op : operation) : Prop :=
  match op with
  | Produce _ => True
  | Consume _ => True
  | Install _ => False
  end.

(** The operation whose fire steps show the members of an enabled set: the
    operation itself for a produce or a consume, and for an install the
    consume of the same continuation, whose enabled set is the same. *)
Definition firing_counterpart (op : operation) : operation :=
  match op with
  | Produce arriving => Produce arriving
  | Consume arriving => Consume arriving
  | Install arriving => Consume arriving
  end.

Lemma firing_counterpart_may_fire : forall op, may_fire (firing_counterpart op).
Proof.
  intros [arriving|arriving|arriving]; exact I.
Qed.

Lemma enabled_in_firing_counterpart :
  forall contents_now op candidate,
    enabled contents_now op candidate -> enabled contents_now (firing_counterpart op) candidate.
Proof.
  intros contents_now [arriving|arriving|arriving] candidate member; exact member.
Qed.

(** * Steps of the reference *)

Definition rest (contents_now : contents) (op : operation) : contents :=
  match op with
  | Produce arriving =>
      Contents (arriving :: resting_data contents_now) (resting_continuations contents_now)
  | Consume arriving =>
      Contents (resting_data contents_now) (arriving :: resting_continuations contents_now)
  | Install arriving =>
      Contents (resting_data contents_now) (arriving :: resting_continuations contents_now)
  end.

(** A resting continuation that fires stays when it is persistent and goes
    otherwise; an arriving continuation never comes to rest. *)
Definition continuation_after (op : operation) (waiting : continuation)
    (continuations_before continuations_after : list continuation) : Prop :=
  match op with
  | Produce _ =>
      if continuation_persistent waiting
      then Permutation continuations_after continuations_before
      else Permutation continuations_before (waiting :: continuations_after)
  | Consume _ => Permutation continuations_after continuations_before
  | Install _ => Permutation continuations_after continuations_before
  end.

(** Performing a rendezvous removes every consumed occurrence; the arriving
    datum never rests; persistent data read stay. *)
Definition performs (contents_before : contents) (op : operation) (candidate : rendezvous)
    (contents_after : contents) : Prop :=
  Permutation (arriving_consumable op ++ resting_data contents_before)
    (consumed_data (rendezvous_data candidate) ++ resting_data contents_after) /\
  continuation_after op (rendezvous_continuation candidate)
    (resting_continuations contents_before) (resting_continuations contents_after).

(** [root_of] is the root function that the reference shares with a backend;
    roots are compared by a decision procedure. *)
Variable root_of : contents -> Root.
Variable root_eq_dec : forall root_one root_two : Root, {root_one = root_two} + {root_one <> root_two}.

Definition record_root (history : Root -> option contents) (root : Root) (snapshot : contents)
    : Root -> option contents :=
  fun other => if root_eq_dec other root then Some snapshot else history other.

(** The eager reference.  Every operation rests exactly when its enabled set
    is empty; a produce or a consume fires any member of its set; an install
    has no step while its set has a member.  Successors are determined up to
    multiset equality.  A checkpoint returns the root of a representative of
    the current contents; when [root_of] respects multiset equality (the first
    half of O5), that is [root_of] of the current contents. *)
Inductive ref_step : configuration -> label -> configuration -> Prop :=
| ref_rest : forall origin op successor,
    (forall candidate, ~ enabled (current origin) op candidate) ->
    contents_equiv (current successor) (rest (current origin) op) ->
    recorded_equiv (recorded successor) (recorded origin) ->
    ref_step origin (OpResult op Rested) successor
| ref_fire : forall origin op candidate successor,
    may_fire op ->
    enabled (current origin) op candidate ->
    performs (current origin) op candidate (current successor) ->
    recorded_equiv (recorded successor) (recorded origin) ->
    ref_step origin (OpResult op (Fired candidate)) successor
| ref_checkpoint : forall origin representative successor,
    contents_equiv representative (current origin) ->
    contents_equiv (current successor) (current origin) ->
    recorded_equiv (recorded successor)
      (record_root (recorded origin) (root_of representative) (current origin)) ->
    ref_step origin (CheckpointReturned (root_of representative)) successor
| ref_reset : forall origin root snapshot successor,
    recorded origin root = Some snapshot ->
    contents_equiv (current successor) snapshot ->
    recorded_equiv (recorded successor) (recorded origin) ->
    ref_step origin (ResetTo root) successor.

(** * The reference respects multiset equality *)

Lemma data_available_transport :
  forall consumable_one consumable_two readable_one readable_two selection,
    Permutation consumable_one consumable_two ->
    (forall selected, In selected readable_one -> In selected readable_two) ->
    data_available consumable_one readable_one selection ->
    data_available consumable_two readable_two selection.
Proof.
  intros consumable_one consumable_two readable_one readable_two selection
    same_consumable readable_included [[remainder consumable_split] readable_cover].
  split.
  - exists remainder.
    exact (Permutation_trans (Permutation_sym same_consumable) consumable_split).
  - intros selected persistent_read.
    exact (readable_included selected (readable_cover selected persistent_read)).
Qed.

Lemma enabled_respects_contents_equiv :
  forall contents_one contents_two op candidate,
    contents_equiv contents_one contents_two ->
    enabled contents_one op candidate -> enabled contents_two op candidate.
Proof.
  intros contents_one contents_two op candidate [same_data same_continuations]
    [available [admissible_selection [participating data_ready]]].
  split; [|split; [exact admissible_selection|split; [exact participating|]]].
  - destruct op as [arriving|arriving|arriving]; simpl in available |- *.
    + exact (Permutation_in _ same_continuations available).
    + exact available.
    + exact available.
  - exact (data_available_transport _ _ _ _ _
      (Permutation_app_head (arriving_consumable op) same_data)
      (fun selected inside =>
         Permutation_in selected (Permutation_app_head (arriving_readable op) same_data) inside)
      data_ready).
Qed.

Lemma continuation_after_transport :
  forall op waiting before_one before_two after_one after_two,
    Permutation before_one before_two ->
    Permutation after_one after_two ->
    continuation_after op waiting before_one after_one ->
    continuation_after op waiting before_two after_two.
Proof.
  intros op waiting before_one before_two after_one after_two same_before same_after kept.
  destruct op as [arriving|arriving|arriving]; simpl in kept |- *.
  - revert kept. destruct (continuation_persistent waiting); intros kept.
    + exact (Permutation_trans (Permutation_trans (Permutation_sym same_after) kept) same_before).
    + exact (Permutation_trans (Permutation_trans (Permutation_sym same_before) kept)
        (perm_skip waiting same_after)).
  - exact (Permutation_trans (Permutation_trans (Permutation_sym same_after) kept) same_before).
  - exact (Permutation_trans (Permutation_trans (Permutation_sym same_after) kept) same_before).
Qed.

Lemma performs_transport :
  forall origin_one origin_two op candidate successor_one successor_two,
    contents_equiv origin_one origin_two ->
    contents_equiv successor_one successor_two ->
    performs origin_one op candidate successor_one ->
    performs origin_two op candidate successor_two.
Proof.
  intros origin_one origin_two op candidate successor_one successor_two
    [same_data_before same_continuations_before]
    [same_data_after same_continuations_after]
    [data_moved continuations_moved].
  split.
  - exact (Permutation_trans
      (Permutation_trans
        (Permutation_app_head (arriving_consumable op) (Permutation_sym same_data_before))
        data_moved)
      (Permutation_app_head (consumed_data (rendezvous_data candidate)) same_data_after)).
  - exact (continuation_after_transport _ _ _ _ _ _
      same_continuations_before same_continuations_after continuations_moved).
Qed.

Lemma rest_transport :
  forall contents_one contents_two op,
    contents_equiv contents_one contents_two ->
    contents_equiv (rest contents_one op) (rest contents_two op).
Proof.
  intros contents_one contents_two op [same_data same_continuations].
  destruct op as [arriving|arriving|arriving]; split; simpl;
    solve [apply perm_skip; assumption | assumption].
Qed.

Lemma record_root_transport :
  forall history_one history_two root snapshot_one snapshot_two,
    recorded_equiv history_one history_two ->
    contents_equiv snapshot_one snapshot_two ->
    recorded_equiv (record_root history_one root snapshot_one)
      (record_root history_two root snapshot_two).
Proof.
  intros history_one history_two root snapshot_one snapshot_two same_history same_snapshot
    other.
  unfold record_root.
  destruct (root_eq_dec other root) as [_|_].
  - exact same_snapshot.
  - exact (same_history other).
Qed.

Lemma ref_step_source_transport :
  forall origin_one origin_two observed successor,
    config_equiv origin_one origin_two ->
    ref_step origin_one observed successor ->
    ref_step origin_two observed successor.
Proof.
  intros origin_one origin_two observed successor same_origin step.
  revert origin_two same_origin.
  destruct step as
    [source op target nothing_enabled rested kept
    |source op candidate target fires member performed kept
    |source representative target represents unchanged recorded_now
    |source reset_root snapshot target stored restored kept];
    intros origin_two [same_current same_recorded].
  - apply ref_rest.
    + intros candidate member.
      exact (nothing_enabled candidate
        (enabled_respects_contents_equiv _ _ _ _ (contents_equiv_sym _ _ same_current) member)).
    + exact (contents_equiv_trans _ _ _ rested (rest_transport _ _ _ same_current)).
    + exact (recorded_equiv_trans _ _ _ kept same_recorded).
  - apply ref_fire.
    + exact fires.
    + exact (enabled_respects_contents_equiv _ _ _ _ same_current member).
    + exact (performs_transport _ _ _ _ _ _ same_current (contents_equiv_refl _) performed).
    + exact (recorded_equiv_trans _ _ _ kept same_recorded).
  - apply ref_checkpoint.
    + exact (contents_equiv_trans _ _ _ represents same_current).
    + exact (contents_equiv_trans _ _ _ unchanged same_current).
    + exact (recorded_equiv_trans _ _ _ recorded_now
        (record_root_transport _ _ _ _ _ same_recorded same_current)).
  - pose proof (same_recorded reset_root) as same_entry.
    rewrite stored in same_entry.
    destruct (recorded origin_two reset_root) as [snapshot_two|] eqn:stored_two.
    + apply (ref_reset _ _ snapshot_two).
      * exact stored_two.
      * simpl in same_entry.
        exact (contents_equiv_trans _ _ _ restored same_entry).
      * exact (recorded_equiv_trans _ _ _ kept same_recorded).
    + destruct same_entry.
Qed.

Lemma ref_step_target_transport :
  forall origin observed successor_one successor_two,
    ref_step origin observed successor_one ->
    config_equiv successor_one successor_two ->
    ref_step origin observed successor_two.
Proof.
  intros origin observed successor_one successor_two step.
  revert successor_two.
  destruct step as
    [source op target nothing_enabled rested kept
    |source op candidate target fires member performed kept
    |source representative target represents unchanged recorded_now
    |source reset_root snapshot target stored restored kept];
    intros successor_two [same_current same_recorded].
  - apply ref_rest; [exact nothing_enabled| |].
    + exact (contents_equiv_trans _ _ _ (contents_equiv_sym _ _ same_current) rested).
    + exact (recorded_equiv_trans _ _ _ (recorded_equiv_sym _ _ same_recorded) kept).
  - apply ref_fire; [exact fires|exact member| |].
    + exact (performs_transport _ _ _ _ _ _ (contents_equiv_refl _) same_current performed).
    + exact (recorded_equiv_trans _ _ _ (recorded_equiv_sym _ _ same_recorded) kept).
  - apply ref_checkpoint; [exact represents| |].
    + exact (contents_equiv_trans _ _ _ (contents_equiv_sym _ _ same_current) unchanged).
    + exact (recorded_equiv_trans _ _ _ (recorded_equiv_sym _ _ same_recorded) recorded_now).
  - apply (ref_reset _ _ snapshot); [exact stored| |].
    + exact (contents_equiv_trans _ _ _ (contents_equiv_sym _ _ same_current) restored).
    + exact (recorded_equiv_trans _ _ _ (recorded_equiv_sym _ _ same_recorded) kept).
Qed.

Theorem ref_step_respects_config_equiv :
  forall origin_one origin_two observed successor_one successor_two,
    config_equiv origin_one origin_two ->
    config_equiv successor_one successor_two ->
    ref_step origin_one observed successor_one ->
    ref_step origin_two observed successor_two.
Proof.
  intros origin_one origin_two observed successor_one successor_two
    same_origin same_successor step.
  exact (ref_step_target_transport _ _ _ _
    (ref_step_source_transport _ _ _ _ same_origin step) same_successor).
Qed.

(** * What the labels of the reference say *)

Lemma enabled_has_successor :
  forall contents_now op candidate,
    enabled contents_now op candidate ->
    exists contents_next, performs contents_now op candidate contents_next.
Proof.
  intros contents_now op candidate [available [_ [_ [[remainder consumable_split] _]]]].
  destruct op as [arriving|arriving|arriving].
  - simpl in available.
    destruct (continuation_persistent (rendezvous_continuation candidate)) eqn:persistent_waiting.
    + exists (Contents remainder (resting_continuations contents_now)).
      split; [exact consumable_split|].
      simpl. rewrite persistent_waiting. simpl. apply Permutation_refl.
    + destruct (in_split _ _ available) as [prefix [suffix split_continuations]].
      exists (Contents remainder (prefix ++ suffix)).
      split; [exact consumable_split|].
      simpl. rewrite persistent_waiting, split_continuations. simpl.
      apply Permutation_sym, Permutation_middle.
  - exists (Contents remainder (resting_continuations contents_now)).
    split; [exact consumable_split|simpl; apply Permutation_refl].
  - exists (Contents remainder (resting_continuations contents_now)).
    split; [exact consumable_split|simpl; apply Permutation_refl].
Qed.

Lemma ref_rest_label_means_nothing_enabled :
  forall origin op successor,
    ref_step origin (OpResult op Rested) successor ->
    forall candidate, ~ enabled (current origin) op candidate.
Proof.
  intros origin op successor step.
  remember (OpResult op Rested) as observed eqn:rest_label.
  revert rest_label.
  destruct step as
    [source op' target nothing_enabled _ _
    |source op' candidate target _ _ _ _
    |source representative target _ _ _
    |source reset_root snapshot target _ _ _];
    intros rest_label; try discriminate rest_label.
  injection rest_label as same_op.
  rewrite <- same_op.
  exact nothing_enabled.
Qed.

Lemma ref_fire_label_means_enabled :
  forall origin op candidate successor,
    ref_step origin (OpResult op (Fired candidate)) successor ->
    enabled (current origin) op candidate /\
    performs (current origin) op candidate (current successor).
Proof.
  intros origin op candidate successor step.
  remember (OpResult op (Fired candidate)) as observed eqn:fire_label.
  revert fire_label.
  destruct step as
    [source op' target _ _ _
    |source op' candidate' target _ member performed _
    |source representative target _ _ _
    |source reset_root snapshot target _ _ _];
    intros fire_label; try discriminate fire_label.
  injection fire_label as same_op same_candidate.
  rewrite <- same_op, <- same_candidate.
  split; [exact member|exact performed].
Qed.

Lemma ref_fire_label_means_may_fire :
  forall origin op candidate successor,
    ref_step origin (OpResult op (Fired candidate)) successor -> may_fire op.
Proof.
  intros origin op candidate successor step.
  remember (OpResult op (Fired candidate)) as observed eqn:fire_label.
  revert fire_label.
  destruct step as
    [source op' target _ _ _
    |source op' candidate' target fires _ _ _
    |source representative target _ _ _
    |source reset_root snapshot target _ _ _];
    intros fire_label; try discriminate fire_label.
  injection fire_label as same_op same_candidate.
  rewrite <- same_op.
  exact fires.
Qed.

(** For a produce or a consume, the reference fires every member of the
    enabled set; it fires only members, and only for a produce or a consume;
    every operation rests exactly when its enabled set is empty. *)
Theorem ref_step_fires_any_member_and_rests_only_when_none_is_enabled :
  forall origin op,
    (may_fire op -> forall candidate, enabled (current origin) op candidate ->
       exists successor, ref_step origin (OpResult op (Fired candidate)) successor) /\
    (forall candidate successor, ref_step origin (OpResult op (Fired candidate)) successor ->
       may_fire op /\ enabled (current origin) op candidate) /\
    ((forall candidate, ~ enabled (current origin) op candidate) ->
       exists successor, ref_step origin (OpResult op Rested) successor) /\
    (forall successor, ref_step origin (OpResult op Rested) successor ->
       forall candidate, ~ enabled (current origin) op candidate).
Proof.
  intros origin op.
  split; [|split; [|split]].
  - intros fires candidate member.
    destruct (enabled_has_successor _ _ _ member) as [contents_next performed].
    exists (Configuration contents_next (recorded origin)).
    apply ref_fire;
      [exact fires|exact member|exact performed|exact (recorded_equiv_refl (recorded origin))].
  - intros candidate successor step.
    split; [exact (ref_fire_label_means_may_fire _ _ _ _ step)
           |exact (proj1 (ref_fire_label_means_enabled _ _ _ _ step))].
  - intros nothing_enabled.
    exists (Configuration (rest (current origin) op) (recorded origin)).
    apply ref_rest;
      [exact nothing_enabled
      |exact (contents_equiv_refl (rest (current origin) op))
      |exact (recorded_equiv_refl (recorded origin))].
  - intros successor step.
    exact (ref_rest_label_means_nothing_enabled _ _ _ step).
Qed.

(** An install never fires: no reference step, from any configuration, carries
    the label of an install with the result [Fired]. *)
Theorem install_never_fires :
  forall origin arriving candidate successor,
    ~ ref_step origin (OpResult (Install arriving) (Fired candidate)) successor.
Proof.
  intros origin arriving candidate successor step.
  exact (ref_fire_label_means_may_fire origin (Install arriving) candidate successor step).
Qed.

(** An install rests exactly when its enabled set is empty, and while the set
    has a member the reference has no step for it, with any result. *)
Theorem install_rests_exactly_when_nothing_is_enabled :
  forall origin arriving,
    ((exists successor, ref_step origin (OpResult (Install arriving) Rested) successor) <->
     (forall candidate, ~ enabled (current origin) (Install arriving) candidate)) /\
    (forall candidate, enabled (current origin) (Install arriving) candidate ->
       forall outcome successor,
         ~ ref_step origin (OpResult (Install arriving) outcome) successor).
Proof.
  intros origin arriving.
  destruct (ref_step_fires_any_member_and_rests_only_when_none_is_enabled origin
      (Install arriving)) as [_ [_ [rest_exists rest_means_none_enabled]]].
  split.
  - split.
    + intros [successor step]. exact (rest_means_none_enabled successor step).
    + exact rest_exists.
  - intros candidate member [|fired] successor step.
    + exact (rest_means_none_enabled successor step candidate member).
    + exact (install_never_fires origin arriving fired successor step).
Qed.

(** * Implementations and the obligations O1 to O5 *)

(** An implementation: a state type, a contents function that reads a state
    back as a configuration (the abstraction [pi] of the paper), visible steps
    and internal steps. *)
Record implementation : Type := Implementation {
  impl_state : Type;
  impl_contents : impl_state -> configuration;
  impl_visible : impl_state -> label -> impl_state -> Prop;
  impl_internal : impl_state -> impl_state -> Prop
}.

Definition impl_step (impl : implementation)
    : impl_state impl -> action label -> impl_state impl -> Prop :=
  fun state performed state' =>
    match performed with
    | Internal => impl_internal impl state state'
    | Visible observed => impl_visible impl state observed state'
    end.

(** The reference as a transition system: it has no internal step. *)
Definition ref_action_step (origin : configuration) (performed : action label)
    (successor : configuration) : Prop :=
  match performed with
  | Internal => False
  | Visible observed => ref_step origin observed successor
  end.

Inductive reachable (impl : implementation) (start : impl_state impl)
    : impl_state impl -> Prop :=
| reachable_start : reachable impl start start
| reachable_internal : forall state state',
    reachable impl start state -> impl_internal impl state state' -> reachable impl start state'
| reachable_visible : forall state observed state',
    reachable impl start state -> impl_visible impl state observed state' ->
    reachable impl start state'.

(** O1: an internal step leaves the contents and the recorded roots alone. *)
Definition obligation_o1 (impl : implementation) (start : impl_state impl) : Prop :=
  forall state state',
    reachable impl start state -> impl_internal impl state state' ->
    config_equiv (impl_contents impl state') (impl_contents impl state).

(** O2: every visible step is a reference step with the same label. *)
Definition obligation_o2 (impl : implementation) (start : impl_state impl) : Prop :=
  forall state observed state',
    reachable impl start state -> impl_visible impl state observed state' ->
    ref_step (impl_contents impl state) observed (impl_contents impl state').

(** O3: every step of the reference is available after internal steps. *)
Definition obligation_o3 (impl : implementation) (start : impl_state impl) : Prop :=
  forall state observed configuration_next,
    reachable impl start state ->
    ref_step (impl_contents impl state) observed configuration_next ->
    exists state_middle state',
      internal_star (impl_step impl) state state_middle /\
      impl_visible impl state_middle observed state' /\
      config_equiv (impl_contents impl state') configuration_next.

(** O4: a reset to a recorded root yields the contents recorded there. *)
Definition obligation_o4 (impl : implementation) (start : impl_state impl) : Prop :=
  forall state root snapshot,
    reachable impl start state ->
    recorded (impl_contents impl state) root = Some snapshot ->
    exists state',
      impl_visible impl state (ResetTo root) state' /\
      contents_equiv (current (impl_contents impl state')) snapshot.

(** O5: a checkpoint returns [root_of] of the contents; equal contents have
    equal roots; different recorded contents have different roots. *)
Definition obligation_o5 (impl : implementation) (start : impl_state impl) : Prop :=
  (forall state root state',
     reachable impl start state ->
     impl_visible impl state (CheckpointReturned root) state' ->
     root = root_of (current (impl_contents impl state))) /\
  (forall contents_one contents_two,
     contents_equiv contents_one contents_two -> root_of contents_one = root_of contents_two) /\
  (forall state_one state_two root_one root_two snapshot_one snapshot_two,
     reachable impl start state_one -> reachable impl start state_two ->
     recorded (impl_contents impl state_one) root_one = Some snapshot_one ->
     recorded (impl_contents impl state_two) root_two = Some snapshot_two ->
     root_of snapshot_one = root_of snapshot_two -> contents_equiv snapshot_one snapshot_two).

Lemma internal_path_keeps_reachability_and_contents :
  forall impl start state state',
    obligation_o1 impl start ->
    reachable impl start state ->
    internal_star (impl_step impl) state state' ->
    reachable impl start state' /\
    config_equiv (impl_contents impl state') (impl_contents impl state).
Proof.
  intros impl start state state' o1 reached path.
  revert reached.
  induction path as [visited|visited middle final internal_step _ induction_hypothesis];
    intros reached.
  - split; [exact reached|apply config_equiv_refl].
  - assert (middle_reached : reachable impl start middle)
      by exact (reachable_internal impl start visited middle reached internal_step).
    destruct (induction_hypothesis middle_reached) as [final_reached final_same].
    split; [exact final_reached|].
    exact (config_equiv_trans _ _ _ final_same (o1 visited middle reached internal_step)).
Qed.

(** The reference itself meets O1 to O4. *)
Definition reference_implementation : implementation :=
  Implementation configuration (fun configuration_now => configuration_now) ref_step
    (fun _ _ => False).

Theorem reference_implementation_meets_o1_to_o4 :
  forall start,
    obligation_o1 reference_implementation start /\
    obligation_o2 reference_implementation start /\
    obligation_o3 reference_implementation start /\
    obligation_o4 reference_implementation start.
Proof.
  intros start.
  split; [|split; [|split]].
  - intros state state' _ impossible. destruct impossible.
  - intros state observed state' _ step. exact step.
  - intros state observed configuration_next _ step.
    exists state, configuration_next.
    split; [apply internal_star_refl|].
    split; [exact step|exact (config_equiv_refl configuration_next)].
  - intros state root snapshot _ stored.
    exists (Configuration snapshot (recorded state)).
    split.
    + exact (ref_reset state root snapshot (Configuration snapshot (recorded state)) stored
        (contents_equiv_refl snapshot) (recorded_equiv_refl (recorded state))).
    + exact (contents_equiv_refl snapshot).
Qed.

(** * Theorem CC-2: the obligations give branching bisimilarity *)

(** The relation of the paper: a reachable state and every configuration
    equal, as multisets, to its abstraction. *)
Definition abstraction_relation (impl : implementation) (start : impl_state impl)
    : impl_state impl -> configuration -> Prop :=
  fun state configuration_now =>
    reachable impl start state /\ config_equiv (impl_contents impl state) configuration_now.

Theorem o1_o2_o3_give_branching_bisimulation :
  forall impl start,
    obligation_o1 impl start -> obligation_o2 impl start -> obligation_o3 impl start ->
    branching_bisimulation (impl_step impl) ref_action_step (abstraction_relation impl start).
Proof.
  intros impl start o1 o2 o3.
  split.
  - intros state configuration_now performed state' [reached same] step.
    destruct performed as [|observed].
    + left. split; [reflexivity|].
      split; [exact (reachable_internal impl start state state' reached step)|].
      exact (config_equiv_trans _ _ _ (o1 _ _ reached step) same).
    + right. exists configuration_now, (impl_contents impl state').
      split; [apply internal_star_refl|].
      split; [exact (ref_step_source_transport _ _ _ _ same (o2 _ _ _ reached step))|].
      split; [split; [exact reached|exact same]|].
      split; [exact (reachable_visible impl start state observed state' reached step)
             |apply config_equiv_refl].
  - intros configuration_now state performed configuration_next [reached same] step.
    destruct performed as [|observed].
    + destruct step.
    + right.
      destruct (o3 state observed configuration_next reached
          (ref_step_source_transport _ _ _ _ (config_equiv_sym _ _ same) step))
        as [state_middle [state' [path [visible_step same_next]]]].
      destruct (internal_path_keeps_reachability_and_contents impl start state state_middle
          o1 reached path) as [middle_reached middle_same].
      exists state_middle, state'.
      split; [exact path|].
      split; [exact visible_step|].
      split; [split; [exact middle_reached|exact (config_equiv_trans _ _ _ middle_same same)]|].
      split; [exact (reachable_visible impl start state_middle observed state'
                       middle_reached visible_step)
             |exact same_next].
Qed.

(** O4 follows from O3 up to internal steps, as the paper remarks. *)
Lemma o3_gives_o4_up_to_internal_steps :
  forall impl start,
    obligation_o3 impl start ->
    forall state root snapshot,
      reachable impl start state ->
      recorded (impl_contents impl state) root = Some snapshot ->
      exists state_middle state',
        internal_star (impl_step impl) state state_middle /\
        impl_visible impl state_middle (ResetTo root) state' /\
        contents_equiv (current (impl_contents impl state')) snapshot.
Proof.
  intros impl start o3 state root snapshot reached stored.
  assert (reference_reset : ref_step (impl_contents impl state) (ResetTo root)
            (Configuration snapshot (recorded (impl_contents impl state)))).
  { exact (ref_reset (impl_contents impl state) root snapshot
      (Configuration snapshot (recorded (impl_contents impl state))) stored
      (contents_equiv_refl snapshot) (recorded_equiv_refl (recorded (impl_contents impl state)))). }
  destruct (o3 state (ResetTo root) _ reached reference_reset)
    as [state_middle [state' [path [visible_step [same_current _]]]]].
  exists state_middle, state'.
  split; [exact path|split; [exact visible_step|exact same_current]].
Qed.

Theorem obligations_give_branching_bisimulation :
  forall impl start,
    obligation_o1 impl start -> obligation_o2 impl start ->
    obligation_o3 impl start -> obligation_o4 impl start ->
    branching_bisimulation (impl_step impl) ref_action_step (abstraction_relation impl start) /\
    (forall state, reachable impl start state ->
       branching_bisimilar (impl_step impl) ref_action_step state (impl_contents impl state)).
Proof.
  intros impl start o1 o2 o3 _.
  pose proof (o1_o2_o3_give_branching_bisimulation impl start o1 o2 o3) as bisimulation.
  split; [exact bisimulation|].
  intros state reached.
  exists (abstraction_relation impl start).
  split; [exact bisimulation|].
  split; [exact reached|apply config_equiv_refl].
Qed.

(** The first half of O5 is what makes the reference's checkpoint label the
    backend's: with [root_of] respecting multiset equality, the reference
    returns only [root_of] of its current contents. *)
Theorem o5_fixes_the_reference_checkpoint_root :
  forall impl start,
    obligation_o5 impl start ->
    forall origin root successor,
      ref_step origin (CheckpointReturned root) successor ->
      root = root_of (current origin).
Proof.
  intros impl start [_ [roots_respect_equiv _]] origin root successor step.
  remember (CheckpointReturned root) as observed eqn:checkpoint_label.
  revert checkpoint_label.
  destruct step as
    [source op target _ _ _
    |source op candidate target _ _ _ _
    |source representative target represents _ _
    |source reset_root snapshot target _ _ _];
    intros checkpoint_label; try discriminate checkpoint_label.
  injection checkpoint_label as same_root.
  rewrite <- same_root.
  exact (roots_respect_equiv _ _ represents).
Qed.

(** * Theorem CC-3: a fixed selector gives a refinement, not a bisimulation *)

Inductive impl_run (impl : implementation)
    : impl_state impl -> list label -> impl_state impl -> Prop :=
| impl_run_nil : forall state, impl_run impl state [] state
| impl_run_internal : forall state middle trace final,
    impl_internal impl state middle -> impl_run impl middle trace final ->
    impl_run impl state trace final
| impl_run_visible : forall state observed middle trace final,
    impl_visible impl state observed middle -> impl_run impl middle trace final ->
    impl_run impl state (observed :: trace) final.

Inductive ref_run : configuration -> list label -> configuration -> Prop :=
| ref_run_nil : forall origin final, config_equiv origin final -> ref_run origin [] final
| ref_run_step : forall origin observed middle trace final,
    ref_step origin observed middle -> ref_run middle trace final ->
    ref_run origin (observed :: trace) final.

Theorem o1_o2_give_refinement :
  forall impl start,
    obligation_o1 impl start -> obligation_o2 impl start ->
    (forall state trace final,
       reachable impl start state -> impl_run impl state trace final ->
       ref_run (impl_contents impl state) trace (impl_contents impl final)) /\
    (forall state op state',
       reachable impl start state -> impl_visible impl state (OpResult op Rested) state' ->
       forall candidate, ~ enabled (current (impl_contents impl state)) op candidate).
Proof.
  intros impl start o1 o2.
  split.
  - intros state trace final reached run.
    assert (general : forall configuration_now,
              config_equiv configuration_now (impl_contents impl state) ->
              ref_run configuration_now trace (impl_contents impl final)).
    { revert reached.
      induction run as
        [visited
        |visited middle remaining_trace last internal_step _ induction_hypothesis
        |visited observed middle remaining_trace last visible_step _ induction_hypothesis];
        intros reached configuration_now same.
      - apply ref_run_nil. exact same.
      - apply (induction_hypothesis (reachable_internal impl start visited middle reached
                 internal_step)).
        exact (config_equiv_trans _ _ _ same
          (config_equiv_sym _ _ (o1 visited middle reached internal_step))).
      - apply (ref_run_step _ _ (impl_contents impl middle)).
        + exact (ref_step_source_transport _ _ _ _ (config_equiv_sym _ _ same)
            (o2 visited observed middle reached visible_step)).
        + apply (induction_hypothesis (reachable_visible impl start visited observed middle
                   reached visible_step)).
          apply config_equiv_refl. }
    exact (general _ (config_equiv_refl _)).
  - intros state op state' reached visible_step.
    exact (ref_rest_label_means_nothing_enabled _ _ _ (o2 _ _ _ reached visible_step)).
Qed.

(** A choice function names a rendezvous for every contents and operation;
    it chooses members when it picks a member of every non-empty enabled
    set. *)
Definition choice_function : Type := contents -> operation -> rendezvous.

Definition chooses_members (choose : choice_function) : Prop :=
  forall contents_now op candidate,
    enabled contents_now op candidate -> enabled contents_now op (choose contents_now op).

(** The implementation that a choice function gives: it fires the chosen
    rendezvous of a produce or a consume whenever the enabled set is
    non-empty, and rests an operation whose set is empty.  Like the reference,
    it has no step for an install whose set has a member. *)
Inductive selector_visible (choose : choice_function)
    : configuration -> label -> configuration -> Prop :=
| selector_fire : forall origin op successor,
    may_fire op ->
    (exists candidate, enabled (current origin) op candidate) ->
    performs (current origin) op (choose (current origin) op) (current successor) ->
    recorded_equiv (recorded successor) (recorded origin) ->
    selector_visible choose origin (OpResult op (Fired (choose (current origin) op))) successor
| selector_rest : forall origin op successor,
    (forall candidate, ~ enabled (current origin) op candidate) ->
    contents_equiv (current successor) (rest (current origin) op) ->
    recorded_equiv (recorded successor) (recorded origin) ->
    selector_visible choose origin (OpResult op Rested) successor
| selector_checkpoint : forall origin successor,
    contents_equiv (current successor) (current origin) ->
    recorded_equiv (recorded successor)
      (record_root (recorded origin) (root_of (current origin)) (current origin)) ->
    selector_visible choose origin (CheckpointReturned (root_of (current origin))) successor
| selector_reset : forall origin root snapshot successor,
    recorded origin root = Some snapshot ->
    contents_equiv (current successor) snapshot ->
    recorded_equiv (recorded successor) (recorded origin) ->
    selector_visible choose origin (ResetTo root) successor.

Definition selector_implementation (choose : choice_function) : implementation :=
  Implementation configuration (fun configuration_now => configuration_now)
    (selector_visible choose) (fun _ _ => False).

Lemma selector_step_is_reference_step :
  forall choose,
    chooses_members choose ->
    forall origin observed successor,
      selector_visible choose origin observed successor -> ref_step origin observed successor.
Proof.
  intros choose members origin observed successor step.
  destruct step as
    [source op target fires [candidate member] performed kept
    |source op target nothing_enabled rested kept
    |source target unchanged recorded_now
    |source reset_root snapshot target stored restored kept].
  - apply ref_fire; [exact fires|exact (members _ _ _ member)|exact performed|exact kept].
  - apply ref_rest; [exact nothing_enabled|exact rested|exact kept].
  - apply ref_checkpoint; [apply contents_equiv_refl|exact unchanged|exact recorded_now].
  - apply (ref_reset _ _ snapshot); [exact stored|exact restored|exact kept].
Qed.

Lemma selector_fire_label_is_the_choice :
  forall choose origin op candidate successor,
    selector_visible choose origin (OpResult op (Fired candidate)) successor ->
    candidate = choose (current origin) op.
Proof.
  intros choose origin op candidate successor step.
  remember (OpResult op (Fired candidate)) as observed eqn:fire_label.
  revert fire_label.
  destruct step as
    [source op' target _ _ _ _
    |source op' target _ _ _
    |source target _ _
    |source reset_root snapshot target _ _ _];
    intros fire_label; try discriminate fire_label.
  injection fire_label as same_op same_candidate.
  rewrite <- same_op, <- same_candidate.
  reflexivity.
Qed.

Theorem selector_resolution_is_refinement :
  forall choose,
    chooses_members choose ->
    (forall origin observed successor,
       selector_visible choose origin observed successor -> ref_step origin observed successor) /\
    (forall configuration_now,
       simulated_by (impl_step (selector_implementation choose)) ref_action_step
         configuration_now configuration_now) /\
    (forall origin trace final,
       impl_run (selector_implementation choose) origin trace final ->
       ref_run origin trace final) /\
    (forall origin op successor,
       selector_visible choose origin (OpResult op Rested) successor ->
       forall candidate, ~ enabled (current origin) op candidate).
Proof.
  intros choose members.
  assert (o1 : forall start, obligation_o1 (selector_implementation choose) start).
  { intros start state state' _ impossible. destruct impossible. }
  assert (o2 : forall start, obligation_o2 (selector_implementation choose) start).
  { intros start state observed state' _ step.
    exact (selector_step_is_reference_step choose members state observed state' step). }
  split; [|split; [|split]].
  - exact (selector_step_is_reference_step choose members).
  - intros configuration_now.
    exists (@eq configuration).
    split; [|reflexivity].
    intros selector_state reference_state performed selector_next same step.
    subst reference_state.
    destruct performed as [|observed].
    + destruct step.
    + right. exists selector_state, selector_next.
      split; [apply internal_star_refl|].
      split; [exact (selector_step_is_reference_step choose members
                       selector_state observed selector_next step)|].
      split; reflexivity.
  - intros origin trace final run.
    exact (proj1 (o1_o2_give_refinement (selector_implementation choose) origin
      (o1 origin) (o2 origin)) origin trace final
      (reachable_start (selector_implementation choose) origin) run).
  - intros origin op successor step.
    exact (ref_rest_label_means_nothing_enabled origin op successor
      (selector_step_is_reference_step choose members origin (OpResult op Rested) successor
         step)).
Qed.

(** Two different members of an enabled set separate every selector from the
    reference.  The reference fires both through [firing_counterpart]: the
    operation itself, or for an install the consume of the same continuation,
    whose enabled set is the same. *)
Theorem selector_resolution_is_not_bisimilar :
  forall choose configuration_now op one_member other_member,
    enabled (current configuration_now) op one_member ->
    enabled (current configuration_now) op other_member ->
    one_member <> other_member ->
    ~ branching_bisimilar (impl_step (selector_implementation choose)) ref_action_step
        configuration_now configuration_now.
Proof.
  intros choose configuration_now op one_member other_member one_enabled other_enabled
    different [relation [[_ backward] related]].
  assert (forced : forall member, enabled (current configuration_now) op member ->
            member = choose (current configuration_now) (firing_counterpart op)).
  { intros member member_enabled.
    pose proof (enabled_in_firing_counterpart _ _ _ member_enabled) as counterpart_enabled.
    destruct (enabled_has_successor _ _ _ counterpart_enabled) as [contents_next performed].
    assert (reference_fires : ref_action_step configuration_now
              (Visible (OpResult (firing_counterpart op) (Fired member)))
              (Configuration contents_next (recorded configuration_now))).
    { simpl. apply ref_fire;
        [exact (firing_counterpart_may_fire op)|exact counterpart_enabled|exact performed
        |exact (recorded_equiv_refl (recorded configuration_now))]. }
    destruct (backward configuration_now configuration_now _ _ related reference_fires)
      as [[visible_is_internal _]|[selector_middle [selector_next [path [selector_fires _]]]]].
    - discriminate visible_is_internal.
    - assert (same_state : configuration_now = selector_middle).
      { apply (internal_star_without_internal_steps _ _
          (impl_step (selector_implementation choose))).
        + intros state state' impossible. exact impossible.
        + exact path. }
      subst selector_middle.
      exact (selector_fire_label_is_the_choice choose configuration_now (firing_counterpart op)
        member selector_next selector_fires). }
  apply different.
  rewrite (forced one_member one_enabled), (forced other_member other_enabled).
  reflexivity.
Qed.

(** The general form of part 2 of CC-3: an implementation that meets O1 and
    fires, at every reachable state, the member that a selector depending only
    on the multiset of contents picks is not branching bisimilar to the
    reference at a state whose enabled set has two members.  For an install,
    the members are fired by the consume of the same continuation. *)
Theorem fixed_selector_is_not_bisimilar :
  forall impl start (choose : choice_function) state op one_member other_member,
    obligation_o1 impl start ->
    (forall contents_one contents_two op',
       contents_equiv contents_one contents_two -> choose contents_one op' = choose contents_two op') ->
    (forall state_now op' candidate state_next,
       reachable impl start state_now ->
       impl_visible impl state_now (OpResult op' (Fired candidate)) state_next ->
       candidate = choose (current (impl_contents impl state_now)) op') ->
    reachable impl start state ->
    enabled (current (impl_contents impl state)) op one_member ->
    enabled (current (impl_contents impl state)) op other_member ->
    one_member <> other_member ->
    ~ branching_bisimilar (impl_step impl) ref_action_step state (impl_contents impl state).
Proof.
  intros impl start choose state op one_member other_member o1 choose_respects_equiv
    fires_the_choice reached one_enabled other_enabled different
    [relation [[_ backward] related]].
  assert (forced : forall member, enabled (current (impl_contents impl state)) op member ->
            member = choose (current (impl_contents impl state)) (firing_counterpart op)).
  { intros member member_enabled.
    pose proof (enabled_in_firing_counterpart _ _ _ member_enabled) as counterpart_enabled.
    destruct (enabled_has_successor _ _ _ counterpart_enabled) as [contents_next performed].
    assert (reference_fires : ref_action_step (impl_contents impl state)
              (Visible (OpResult (firing_counterpart op) (Fired member)))
              (Configuration contents_next (recorded (impl_contents impl state)))).
    { simpl. apply ref_fire;
        [exact (firing_counterpart_may_fire op)|exact counterpart_enabled|exact performed
        |exact (recorded_equiv_refl (recorded (impl_contents impl state)))]. }
    destruct (backward (impl_contents impl state) state _ _ related reference_fires)
      as [[visible_is_internal _]|[state_middle [state_next [path [middle_fires _]]]]].
    - discriminate visible_is_internal.
    - destruct (internal_path_keeps_reachability_and_contents impl start state state_middle
          o1 reached path) as [middle_reached [middle_same _]].
      rewrite (fires_the_choice state_middle (firing_counterpart op) member state_next
                 middle_reached middle_fires).
      exact (choose_respects_equiv _ _ (firing_counterpart op) middle_same). }
  apply different.
  rewrite (forced one_member one_enabled), (forced other_member other_enabled).
  reflexivity.
Qed.

(** * Replay *)

(** A log entry names a COMM event: its continuation and the multiset of its
    data (the produce references of an RSpace COMM are sorted, so the log does
    not record the order of positions). *)
Definition same_event (one other : rendezvous) : Prop :=
  rendezvous_continuation one = rendezvous_continuation other /\
  Permutation (rendezvous_data one) (rendezvous_data other).

Inductive replay_outcome : Type :=
| Replayed (successor : configuration)
| ReplayRefused.

(** Replay of an operation whose log entry names the event of [logged]: it
    performs any enabled rendezvous of that event, and refuses when none is
    enabled.  Only a produce or a consume performs: an install never fires, so
    no log names a COMM for it. *)
Inductive replay_step (origin : configuration) (op : operation) (logged : rendezvous)
    : replay_outcome -> Prop :=
| replay_perform : forall chosen successor,
    may_fire op ->
    same_event chosen logged ->
    enabled (current origin) op chosen ->
    performs (current origin) op chosen (current successor) ->
    recorded_equiv (recorded successor) (recorded origin) ->
    replay_step origin op logged (Replayed successor)
| replay_refuse :
    (forall chosen, same_event chosen logged -> ~ enabled (current origin) op chosen) ->
    replay_step origin op logged ReplayRefused.

Lemma performs_same_event :
  forall contents_before op one other contents_after,
    same_event one other ->
    performs contents_before op one contents_after ->
    performs contents_before op other contents_after.
Proof.
  intros contents_before op one other contents_after [same_continuation same_data]
    [data_moved continuations_moved].
  split.
  - exact (Permutation_trans data_moved
      (Permutation_app_tail (resting_data contents_after)
        (permutation_filter _ (fun selected => negb (datum_persistent selected)) _ _
           same_data))).
  - rewrite <- same_continuation. exact continuations_moved.
Qed.

(** When the logged rendezvous is enabled, replay of a produce or a consume
    has an outcome; for every operation, every outcome performs the reference
    step with the logged label (an install has none). *)
Theorem replay_follows_any_enabled_choice :
  forall origin op logged,
    enabled (current origin) op logged ->
    (may_fire op -> exists outcome, replay_step origin op logged outcome) /\
    (forall outcome, replay_step origin op logged outcome ->
       exists successor,
         outcome = Replayed successor /\
         ref_step origin (OpResult op (Fired logged)) successor).
Proof.
  intros origin op logged logged_enabled.
  split.
  - intros fires.
    destruct (enabled_has_successor _ _ _ logged_enabled) as [contents_next performed].
    exists (Replayed (Configuration contents_next (recorded origin))).
    apply (replay_perform _ _ _ logged).
    + exact fires.
    + split; [reflexivity|apply Permutation_refl].
    + exact logged_enabled.
    + exact performed.
    + exact (recorded_equiv_refl (recorded origin)).
  - intros outcome replayed.
    destruct replayed as [chosen successor fires same _ performed kept|nothing_enabled].
    + exists successor. split; [reflexivity|].
      apply ref_fire; [exact fires|exact logged_enabled| |exact kept].
      exact (performs_same_event _ _ _ _ _ same performed).
    + exfalso.
      apply (nothing_enabled logged); [split; [reflexivity|apply Permutation_refl]|].
      exact logged_enabled.
Qed.

Theorem replay_refuses_a_non_enabled_choice :
  forall origin op logged,
    (forall chosen, same_event chosen logged -> ~ enabled (current origin) op chosen) ->
    replay_step origin op logged ReplayRefused /\
    (forall outcome, replay_step origin op logged outcome -> outcome = ReplayRefused) /\
    (forall successor, ~ ref_step origin (OpResult op (Fired logged)) successor).
Proof.
  intros origin op logged nothing_enabled.
  split; [apply replay_refuse; exact nothing_enabled|].
  split.
  - intros outcome replayed.
    destruct replayed as [chosen successor _ same chosen_enabled _ _|_].
    + destruct (nothing_enabled chosen same chosen_enabled).
    + reflexivity.
  - intros successor step.
    destruct (ref_fire_label_means_enabled _ _ _ _ step) as [logged_enabled _].
    apply (nothing_enabled logged); [split; [reflexivity|apply Permutation_refl]|].
    exact logged_enabled.
Qed.

(** ReplayRSpace stores an operation whose log entry names no COMM without a
    search.  Such a step is a reference step exactly when the enabled set is
    empty. *)
Definition replay_rested (origin : configuration) (op : operation) (successor : configuration)
    : Prop :=
  contents_equiv (current successor) (rest (current origin) op) /\
  recorded_equiv (recorded successor) (recorded origin).

Theorem logged_rest_is_a_reference_rest_exactly_when_nothing_is_enabled :
  forall origin op successor,
    replay_rested origin op successor ->
    (ref_step origin (OpResult op Rested) successor <->
     forall candidate, ~ enabled (current origin) op candidate).
Proof.
  intros origin op successor [rested kept].
  split.
  - intros step. exact (ref_rest_label_means_nothing_enabled _ _ _ step).
  - intros nothing_enabled. exact (ref_rest _ _ _ nothing_enabled rested kept).
Qed.

(** * Lemma CC-1: resting items never hold an enabled rendezvous *)

Lemma consumed_data_spec :
  forall selection selected,
    In selected (consumed_data selection) <->
    In selected selection /\ datum_persistent selected = false.
Proof.
  intros selection selected.
  unfold consumed_data.
  split.
  - intros consumed.
    destruct (proj1 (filter_In _ selected selection) consumed) as [inside flag].
    split; [exact inside|].
    simpl in flag. revert flag.
    destruct (datum_persistent selected); simpl; intros flag;
      [discriminate flag|reflexivity].
  - intros [inside flag].
    apply (proj2 (filter_In _ selected selection)).
    split; [exact inside|].
    simpl. rewrite flag. reflexivity.
Qed.

Lemma read_persistent_data_spec :
  forall selection selected,
    In selected (read_persistent_data selection) <->
    In selected selection /\ datum_persistent selected = true.
Proof.
  intros selection selected.
  exact (filter_In datum_persistent selected selection).
Qed.

Lemma at_rest_transport :
  forall contents_one contents_two,
    contents_equiv contents_one contents_two -> at_rest contents_one -> at_rest contents_two.
Proof.
  intros contents_one contents_two [same_data same_continuations] calm candidate
    [waiting_rests [admissible_selection data_ready]].
  apply (calm candidate).
  split; [exact (Permutation_in _ (Permutation_sym same_continuations) waiting_rests)|].
  split; [exact admissible_selection|].
  exact (data_available_transport _ _ _ _ _ (Permutation_sym same_data)
    (fun selected inside => Permutation_in selected (Permutation_sym same_data) inside)
    data_ready).
Qed.

Lemma rest_keeps_contents_at_rest :
  forall contents_now op,
    at_rest contents_now ->
    (forall candidate, ~ enabled contents_now op candidate) ->
    at_rest (rest contents_now op).
Proof.
  intros contents_now op calm nothing_enabled [waiting selection]
    [waiting_rests [admissible_selection [[remainder consumable_split] readable_cover]]].
  destruct op as [arriving|arriving|arriving]; simpl in waiting_rests, consumable_split,
    readable_cover.
  - assert (arriving_used : In arriving (consumed_data selection ++ remainder))
      by exact (Permutation_in arriving consumable_split (in_eq arriving _)).
    destruct (in_app_or _ _ _ arriving_used) as [arriving_consumed|arriving_left].
    + (* The arriving datum is consumed: the rendezvous is enabled for the produce. *)
      destruct (proj1 (consumed_data_spec selection arriving) arriving_consumed)
        as [arriving_selected arriving_transient].
      apply (nothing_enabled (Rendezvous waiting selection)).
      split; [exact waiting_rests|].
      split; [exact admissible_selection|].
      split; [exact arriving_selected|].
      simpl. rewrite arriving_transient. simpl.
      split; [exists remainder; exact consumable_split|exact readable_cover].
    + destruct (in_split _ _ arriving_left) as [remainder_front [remainder_back remainder_split]].
      rewrite remainder_split, app_assoc in consumable_split.
      pose proof (Permutation_cons_app_inv _ _ consumable_split) as pool_split.
      rewrite <- app_assoc in pool_split.
      destruct (covered_members_split _ arriving (read_persistent_data selection)
          (resting_data contents_now) readable_cover) as [arriving_read|reads_in_pool].
      * (* The arriving datum is read persistently: enabled for the produce. *)
        destruct (proj1 (read_persistent_data_spec selection arriving) arriving_read)
          as [arriving_selected arriving_persistent].
        apply (nothing_enabled (Rendezvous waiting selection)).
        split; [exact waiting_rests|].
        split; [exact admissible_selection|].
        split; [exact arriving_selected|].
        simpl. rewrite arriving_persistent. simpl.
        split; [exists (remainder_front ++ remainder_back); exact pool_split
               |exact readable_cover].
      * (* The arriving datum takes no part: the rendezvous rested already. *)
        apply (calm (Rendezvous waiting selection)).
        split; [exact waiting_rests|].
        split; [exact admissible_selection|].
        split; [exists (remainder_front ++ remainder_back); exact pool_split
               |exact reads_in_pool].
  - destruct waiting_rests as [same_waiting|waiting_was_resting].
    + apply (nothing_enabled (Rendezvous waiting selection)).
      split; [exact (eq_sym same_waiting)|].
      split; [exact admissible_selection|].
      split; [exact I|].
      split; [exists remainder; exact consumable_split|exact readable_cover].
    + apply (calm (Rendezvous waiting selection)).
      split; [exact waiting_was_resting|].
      split; [exact admissible_selection|].
      split; [exists remainder; exact consumable_split|exact readable_cover].
  - destruct waiting_rests as [same_waiting|waiting_was_resting].
    + apply (nothing_enabled (Rendezvous waiting selection)).
      split; [exact (eq_sym same_waiting)|].
      split; [exact admissible_selection|].
      split; [exact I|].
      split; [exists remainder; exact consumable_split|exact readable_cover].
    + apply (calm (Rendezvous waiting selection)).
      split; [exact waiting_was_resting|].
      split; [exact admissible_selection|].
      split; [exists remainder; exact consumable_split|exact readable_cover].
Qed.

Lemma performs_shrinks :
  forall contents_before op candidate contents_after,
    enabled contents_before op candidate ->
    performs contents_before op candidate contents_after ->
    (exists removed,
       Permutation (resting_data contents_before) (removed ++ resting_data contents_after)) /\
    (forall waiting, In waiting (resting_continuations contents_after) ->
       In waiting (resting_continuations contents_before)).
Proof.
  intros contents_before op candidate contents_after
    [_ [_ [participating _]]] [data_moved continuations_moved].
  split.
  - destruct op as [arriving|arriving|arriving].
    + simpl in participating. revert data_moved. simpl.
      destruct (datum_persistent arriving) eqn:arriving_flag; simpl; intros data_moved.
      * exists (consumed_data (rendezvous_data candidate)). exact data_moved.
      * assert (arriving_consumed : In arriving (consumed_data (rendezvous_data candidate))).
        { apply (proj2 (consumed_data_spec _ _)).
          split; [exact participating|exact arriving_flag]. }
        destruct (in_split _ _ arriving_consumed) as [consumed_front [consumed_back consumed_split]].
        exists (consumed_front ++ consumed_back).
        rewrite consumed_split, <- app_assoc in data_moved.
        simpl in data_moved.
        rewrite <- app_assoc.
        exact (Permutation_cons_app_inv _ _ data_moved).
    + exists (consumed_data (rendezvous_data candidate)). exact data_moved.
    + exists (consumed_data (rendezvous_data candidate)). exact data_moved.
  - intros waiting remaining.
    destruct op as [arriving|arriving|arriving]; simpl in continuations_moved.
    + revert continuations_moved.
      destruct (continuation_persistent (rendezvous_continuation candidate));
        intros continuations_moved.
      * exact (Permutation_in waiting continuations_moved remaining).
      * exact (Permutation_in waiting (Permutation_sym continuations_moved)
          (in_cons _ _ _ remaining)).
    + exact (Permutation_in waiting continuations_moved remaining).
    + exact (Permutation_in waiting continuations_moved remaining).
Qed.

Lemma at_rest_after_fire :
  forall contents_before op candidate contents_after,
    at_rest contents_before ->
    enabled contents_before op candidate ->
    performs contents_before op candidate contents_after ->
    at_rest contents_after.
Proof.
  intros contents_before op candidate contents_after calm member performed other
    [waiting_rests [admissible_selection [[remainder consumable_split] readable_cover]]].
  destruct (performs_shrinks _ _ _ _ member performed)
    as [[removed data_shrink] continuations_shrink].
  apply (calm other).
  split; [exact (continuations_shrink _ waiting_rests)|].
  split; [exact admissible_selection|].
  split.
  - exists (removed ++ remainder).
    exact (Permutation_trans data_shrink
      (Permutation_trans (Permutation_app_head removed consumable_split)
        (Permutation_app_swap_app removed (consumed_data (rendezvous_data other)) remainder))).
  - intros selected persistent_read.
    apply (Permutation_in selected (Permutation_sym data_shrink)).
    apply in_or_app. right. exact (readable_cover selected persistent_read).
Qed.

(** A configuration is settled when its current contents and every recorded
    contents are at rest. *)
Definition settled (configuration_now : configuration) : Prop :=
  at_rest (current configuration_now) /\
  (forall root snapshot, recorded configuration_now root = Some snapshot -> at_rest snapshot).

Lemma settled_transport :
  forall configuration_one configuration_two,
    config_equiv configuration_one configuration_two ->
    settled configuration_one -> settled configuration_two.
Proof.
  intros configuration_one configuration_two [same_current same_recorded]
    [calm_current calm_recorded].
  split.
  - exact (at_rest_transport _ _ same_current calm_current).
  - intros root snapshot stored.
    pose proof (same_recorded root) as same_entry.
    rewrite stored in same_entry.
    destruct (recorded configuration_one root) as [snapshot_one|] eqn:stored_one.
    + simpl in same_entry.
      exact (at_rest_transport _ _ same_entry (calm_recorded root snapshot_one stored_one)).
    + destruct same_entry.
Qed.

Lemma ref_step_keeps_settled :
  forall origin observed successor,
    settled origin -> ref_step origin observed successor -> settled successor.
Proof.
  intros origin observed successor settled_origin step.
  revert settled_origin.
  destruct step as
    [source op target nothing_enabled rested kept
    |source op candidate target _ member performed kept
    |source representative target represents unchanged recorded_now
    |source reset_root snapshot target stored restored kept];
    intros [calm_current calm_recorded].
  - apply (settled_transport (Configuration (rest (current source) op) (recorded source))).
    + split; [exact (contents_equiv_sym _ _ rested)|exact (recorded_equiv_sym _ _ kept)].
    + split; [exact (rest_keeps_contents_at_rest _ _ calm_current nothing_enabled)
             |exact calm_recorded].
  - apply (settled_transport (Configuration (current target) (recorded source))).
    + split; [exact (contents_equiv_refl (current target))
             |exact (recorded_equiv_sym _ _ kept)].
    + split; [exact (at_rest_after_fire _ _ _ _ calm_current member performed)
             |exact calm_recorded].
  - apply (settled_transport (Configuration (current source)
      (record_root (recorded source) (root_of representative) (current source)))).
    + split; [exact (contents_equiv_sym _ _ unchanged)
             |exact (recorded_equiv_sym _ _ recorded_now)].
    + split; [exact calm_current|].
      intros other snapshot stored.
      unfold record_root in stored. simpl in stored.
      destruct (root_eq_dec other (root_of representative)) as [_|_].
      * injection stored as same_snapshot. rewrite <- same_snapshot. exact calm_current.
      * exact (calm_recorded other snapshot stored).
  - apply (settled_transport (Configuration snapshot (recorded source))).
    + split; [exact (contents_equiv_sym _ _ restored)|exact (recorded_equiv_sym _ _ kept)].
    + split; [exact (calm_recorded reset_root snapshot stored)|exact calm_recorded].
Qed.

Definition empty_contents : contents := Contents [] [].

Definition initial_configuration : configuration :=
  Configuration empty_contents (fun _ => None).

Lemma initial_configuration_settled : settled initial_configuration.
Proof.
  split.
  - intros candidate [waiting_rests _]. exact waiting_rests.
  - intros root snapshot stored. discriminate stored.
Qed.

Inductive ref_reachable (origin : configuration) : configuration -> Prop :=
| ref_reachable_origin : ref_reachable origin origin
| ref_reachable_step : forall middle observed successor,
    ref_reachable origin middle -> ref_step middle observed successor ->
    ref_reachable origin successor.

Theorem rest_states_have_no_enabled_rendezvous :
  (forall configuration_now,
     ref_reachable initial_configuration configuration_now ->
     at_rest (current configuration_now) /\
     (forall root snapshot,
        recorded configuration_now root = Some snapshot -> at_rest snapshot)) /\
  (forall impl start,
     obligation_o1 impl start -> obligation_o2 impl start ->
     config_equiv (impl_contents impl start) initial_configuration ->
     forall state, reachable impl start state ->
       at_rest (current (impl_contents impl state))).
Proof.
  split.
  - intros configuration_now reached.
    induction reached as [|middle observed successor _ settled_middle step].
    + exact initial_configuration_settled.
    + exact (ref_step_keeps_settled _ _ _ settled_middle step).
  - intros impl start o1 o2 start_is_initial state reached.
    assert (settled_state : settled (impl_contents impl state)).
    { induction reached as
        [|source target source_reached settled_source internal_step
         |source observed target source_reached settled_source visible_step].
      - exact (settled_transport _ _ (config_equiv_sym _ _ start_is_initial)
          initial_configuration_settled).
      - exact (settled_transport _ _ (config_equiv_sym _ _ (o1 _ _ source_reached internal_step))
          settled_source).
      - exact (ref_step_keeps_settled _ _ _ settled_source (o2 _ _ _ source_reached visible_step)). }
    exact (proj1 settled_state).
Qed.

End EagerReference.

End RSpaceEagerReference.

Print Assumptions RSpaceEagerReference.ref_step_fires_any_member_and_rests_only_when_none_is_enabled.
Print Assumptions RSpaceEagerReference.install_never_fires.
Print Assumptions RSpaceEagerReference.install_rests_exactly_when_nothing_is_enabled.
Print Assumptions RSpaceEagerReference.ref_step_respects_config_equiv.
Print Assumptions RSpaceEagerReference.reference_implementation_meets_o1_to_o4.
Print Assumptions RSpaceEagerReference.o1_o2_o3_give_branching_bisimulation.
Print Assumptions RSpaceEagerReference.o3_gives_o4_up_to_internal_steps.
Print Assumptions RSpaceEagerReference.obligations_give_branching_bisimulation.
Print Assumptions RSpaceEagerReference.o5_fixes_the_reference_checkpoint_root.
Print Assumptions RSpaceEagerReference.o1_o2_give_refinement.
Print Assumptions RSpaceEagerReference.selector_resolution_is_refinement.
Print Assumptions RSpaceEagerReference.selector_resolution_is_not_bisimilar.
Print Assumptions RSpaceEagerReference.fixed_selector_is_not_bisimilar.
Print Assumptions RSpaceEagerReference.replay_follows_any_enabled_choice.
Print Assumptions RSpaceEagerReference.replay_refuses_a_non_enabled_choice.
Print Assumptions RSpaceEagerReference.logged_rest_is_a_reference_rest_exactly_when_nothing_is_enabled.
Print Assumptions RSpaceEagerReference.rest_states_have_no_enabled_rendezvous.
