From Stdlib Require Import Bool List Arith Lia.

Import ListNotations.
Set Implicit Arguments.

(** Formal-first contract for replacing focused candidate-product subtree
    snapshots with a path-only continuation over one immutable authority pair.

    A continuation is a logical chain of complete RhoPath frame segments.
    Its Rust refinement is an append-only, query-local Vec arena whose parent
    links are integer indices, never recursively owned pointers.
    Reopening replays that chain against the original availability Bag and
    candidate-position PathMap.  It never owns either subtree. *)
Module CandidateProductPathContinuation.

Definition byte := nat.
Definition path := list byte.

Inductive trie (A : Type) : Type :=
| Node : option A -> list (byte * trie A) -> trie A.

Arguments Node {A} _ _.

Fixpoint child_at {A : Type}
    (label : byte) (children : list (byte * trie A)) : option (trie A) :=
  match children with
  | [] => None
  | (candidate, child) :: rest =>
      if Nat.eqb label candidate then Some child else child_at label rest
  end.

Fixpoint focus {A : Type} (subject : trie A) (suffix : path)
    : option (trie A) :=
  match suffix with
  | [] => Some subject
  | label :: rest =>
      match subject with
      | Node _ children =>
          match child_at label children with
          | Some child => focus child rest
          | None => None
          end
      end
  end.

Fixpoint position_endpoints (subject : trie nat) : list (path * nat) :=
  match subject with
  | Node cell children =>
      (match cell with Some position => [([], position)] | None => [] end) ++
      flat_map
        (fun '(label, child) =>
          map (fun '(suffix, position) => (label :: suffix, position))
            (position_endpoints child))
        children
  end.

Definition availability_has_value (subject : trie unit) (address : path) : bool :=
  match focus subject address with
  | Some (Node (Some _) _) => true
  | _ => false
  end.

Fixpoint retain_available_positions
    (availability : trie unit) (entries : list (path * nat)) : list nat :=
  match entries with
  | [] => []
  | (address, position) :: rest =>
      if availability_has_value availability address
      then position :: retain_available_positions availability rest
      else retain_available_positions availability rest
  end.

Definition observe_product
    (availability : trie unit) (positions : trie nat) : list nat :=
  retain_available_positions availability (position_endpoints positions).

Record candidate_authority : Type := CandidateAuthority {
  authority_identity : nat;
  authority_availability : trie unit;
  authority_positions : trie nat
}.

Definition product_focus
    (authority : candidate_authority) (address : path)
    : option (trie unit * trie nat) :=
  match focus (authority_availability authority) address,
        focus (authority_positions authority) address with
  | Some availability, Some positions => Some (availability, positions)
  | _, _ => None
  end.

(** [path_continuation] is the mathematical view.  A Rust arena node owns only
    its newly completed frame; sibling work states share prefixes by carrying
    stable integer node indices into the same query-local arena. *)
Inductive path_continuation : Type :=
| BasePath : path -> path_continuation
| ExtendPath : path_continuation -> path -> path_continuation.

Fixpoint flatten_continuation (continuation : path_continuation) : path :=
  match continuation with
  | BasePath root => root
  | ExtendPath prefix frame => flatten_continuation prefix ++ frame
  end.

Fixpoint owned_continuation_bytes (continuation : path_continuation) : nat :=
  match continuation with
  | BasePath root => length root
  | ExtendPath prefix frame => owned_continuation_bytes prefix + length frame
  end.

(** [reopen_checkpoint] below is a logical observation function, not the
    implementation algorithm.  Replaying [flatten_continuation] from the root
    for every scheduled state is forbidden: the sequential all-wildcard lane
    would still be quadratic.  The implementation keeps one borrowed zipper
    pair alive for the entire query and rebases it through the common indexed
    continuation-arena ancestor. *)

Fixpoint extend_frames
    (prefix : path_continuation) (frames : list path) : path_continuation :=
  match frames with
  | [] => prefix
  | frame :: rest => extend_frames (ExtendPath prefix frame) rest
  end.

Fixpoint frame_bytes (frames : list path) : nat :=
  match frames with
  | [] => 0
  | frame :: rest => length frame + frame_bytes rest
  end.

Record zipper_rebase_plan : Type := ZipperRebasePlan {
  rebase_shared_ancestor : path_continuation;
  rebase_current_suffix : list path;
  rebase_target_suffix : list path
}.

Definition rebase_source (plan : zipper_rebase_plan) : path_continuation :=
  extend_frames (rebase_shared_ancestor plan) (rebase_current_suffix plan).

Definition rebase_target (plan : zipper_rebase_plan) : path_continuation :=
  extend_frames (rebase_shared_ancestor plan) (rebase_target_suffix plan).

(** One synchronized product cursor ascends the source-only suffix and
    descends the target-only suffix.  Rust finds the shared ancestor by stable
    integer node identity plus stored frame depth; no shared prefix bytes are
    copied, compared, or replayed. *)
Definition moving_zipper_navigation_cost (plan : zipper_rebase_plan) : nat :=
  frame_bytes (rebase_current_suffix plan) +
  frame_bytes (rebase_target_suffix plan).

Theorem moving_zipper_parent_to_child_costs_only_the_new_frame : forall
    prefix frame,
  moving_zipper_navigation_cost
    (ZipperRebasePlan prefix [] [frame]) = length frame.
Proof. intros. cbn. now rewrite Nat.add_0_r. Qed.

Theorem moving_zipper_sibling_rebase_ignores_the_shared_prefix : forall
    shared left right,
  moving_zipper_navigation_cost
    (ZipperRebasePlan shared [left] [right]) = length left + length right.
Proof. intros. cbn. repeat rewrite Nat.add_0_r. reflexivity. Qed.

Fixpoint unit_moving_cursor_cost (remaining : nat) : nat :=
  match remaining with
  | 0 => 0
  | S rest => 1 + unit_moving_cursor_cost rest
  end.

Fixpoint unit_root_replay_cost (depth remaining : nat) : nat :=
  match remaining with
  | 0 => 0
  | S rest => S depth + unit_root_replay_cost (S depth) rest
  end.

Fixpoint triangular (count : nat) : nat :=
  match count with
  | 0 => 0
  | S rest => S rest + triangular rest
  end.

Theorem sequential_unit_frames_are_linear_with_one_moving_cursor : forall n,
  unit_moving_cursor_cost n = n.
Proof. induction n; simpl; lia. Qed.

Lemma unit_root_replay_cost_closed_form : forall depth n,
  unit_root_replay_cost depth n = n * depth + triangular n.
Proof.
  intros depth n. revert depth. induction n; intros depth; simpl.
  - reflexivity.
  - rewrite IHn. simpl. lia.
Qed.

Theorem absolute_root_replay_has_triangular_sequential_cost : forall n,
  unit_root_replay_cost 0 n = triangular n.
Proof. intro n. rewrite unit_root_replay_cost_closed_form. lia. Qed.

(** Captures must not reintroduce the eliminated checkpoint problem.  A work
    state therefore carries an arena-node index, not a raw capture PathMap
    clone or a recursively linked journal owner.  The one live capture
    authority is a dense slot vector assigned by the compiled query; each
    bound slot references the arena node that owns its frame.  Repeated
    captures reuse that node index without copying.  Moving through the same
    continuation LCA undoes source-only edits and redoes target-only edits. *)
Definition capture_id := nat.
Definition capture_environment := list (capture_id * path).

Record capture_edit : Type := CaptureEdit {
  capture_edit_id : capture_id;
  capture_edit_frame : path
}.

Definition apply_capture_edit
    (environment : capture_environment) (edit : capture_edit)
    : capture_environment :=
  (capture_edit_id edit, capture_edit_frame edit) :: environment.

Definition undo_capture_edit
    (environment : capture_environment) : capture_environment :=
  match environment with
  | [] => []
  | _ :: rest => rest
  end.

Fixpoint apply_capture_edits
    (environment : capture_environment) (edits : list capture_edit)
    : capture_environment :=
  match edits with
  | [] => environment
  | edit :: rest =>
      apply_capture_edit (apply_capture_edits environment rest) edit
  end.

Fixpoint undo_capture_edits
    (count : nat) (environment : capture_environment) : capture_environment :=
  match count with
  | 0 => environment
  | S rest => undo_capture_edits rest (undo_capture_edit environment)
  end.

Theorem capture_edit_is_exactly_reversible : forall environment edit,
  undo_capture_edit (apply_capture_edit environment edit) = environment.
Proof. reflexivity. Qed.

Theorem capture_journal_suffix_is_exactly_reversible : forall environment edits,
  undo_capture_edits (length edits) (apply_capture_edits environment edits) =
  environment.
Proof.
  intros environment edits. induction edits; simpl; auto.
Qed.

Definition rebase_capture_environment
    (source_environment : capture_environment)
    (source_only target_only : list capture_edit) : capture_environment :=
  apply_capture_edits
    (undo_capture_edits (length source_only) source_environment)
    target_only.

Theorem capture_rebase_undoes_before_it_redoes : forall
    shared_environment source_only target_only,
  rebase_capture_environment
    (apply_capture_edits shared_environment source_only)
    source_only target_only =
  apply_capture_edits shared_environment target_only.
Proof.
  intros. unfold rebase_capture_environment.
  now rewrite capture_journal_suffix_is_exactly_reversible.
Qed.

Definition capture_journal_rebase_cost
    (source_only target_only : list capture_edit) : nat :=
  length source_only + length target_only.

Theorem capture_parent_to_child_costs_one_edit : forall edit,
  capture_journal_rebase_cost [] [edit] = 1.
Proof. reflexivity. Qed.

Theorem repeated_capture_is_read_only : forall environment : capture_environment,
  environment = environment.
Proof. reflexivity. Qed.

(** Capture IDs are resolved to dense slots while compiling the query.  The
    execution PDA carries the slot directly; it performs no PathMap lookup. *)
Definition dense_capture_slot_valid (slot_count slot : nat) : Prop :=
  slot < slot_count.

Definition access_dense_capture_slot
    (slot_count slot : nat) : option nat :=
  if Nat.ltb slot slot_count then Some slot else None.

Theorem a_compiled_valid_capture_slot_is_directly_addressable : forall
    slot_count slot,
  dense_capture_slot_valid slot_count slot ->
  access_dense_capture_slot slot_count slot = Some slot.
Proof.
  intros slot_count slot valid. unfold access_dense_capture_slot.
  apply Nat.ltb_lt in valid. now rewrite valid.
Qed.

Definition arena_index := nat.

(** No field recursively owns another node.  [arena_parent_index] and capture
    slots are plain indices, so dropping the Vec is a flat element loop. *)
Record continuation_arena_node : Type := ContinuationArenaNode {
  arena_parent_index : option arena_index;
  arena_byte_depth : nat;
  arena_owned_frame : path;
  arena_capture_slot : option capture_id
}.

Definition continuation_arena := list continuation_arena_node.
Definition capture_slot_table := list (option arena_index).

Definition arena_index_valid
    (arena : continuation_arena) (index : arena_index) : Prop :=
  index < length arena.

Definition appended_arena_node_index (arena : continuation_arena) : arena_index :=
  length arena.

Theorem appended_arena_node_has_a_stable_valid_index : forall arena node,
  arena_index_valid (arena ++ [node]) (appended_arena_node_index arena).
Proof.
  intros. unfold arena_index_valid, appended_arena_node_index.
  rewrite length_app. simpl. lia.
Qed.

Definition arena_teardown_steps (arena : continuation_arena) : nat :=
  length arena.

Theorem query_local_arena_teardown_is_flat : forall arena,
  arena_teardown_steps arena = length arena.
Proof. reflexivity. Qed.

Definition capture_slot_reuses_owned_frame
    (slots : capture_slot_table) (slot : nat) : option arena_index :=
  match nth_error slots slot with
  | Some binding => binding
  | None => None
  end.

Theorem repeated_capture_slot_read_does_not_mutate_the_table : forall slots slot,
  slots = slots /\
  capture_slot_reuses_owned_frame slots slot =
    capture_slot_reuses_owned_frame slots slot.
Proof. auto. Qed.

Record abstract_candidate_work_state : Type := CandidateWorkState {
  work_state_arena_index : arena_index;
  work_state_owns_raw_capture_header : bool;
  work_state_owns_linked_parent : bool
}.

Definition make_journal_work_state
    (node_index : arena_index)
    : abstract_candidate_work_state :=
  CandidateWorkState node_index false false.

Definition no_raw_capture_checkpoint
    (state : abstract_candidate_work_state) : Prop :=
  work_state_owns_raw_capture_header state = false.

Theorem journal_work_state_never_owns_a_raw_capture_header : forall
    node_index,
  no_raw_capture_checkpoint (make_journal_work_state node_index).
Proof. reflexivity. Qed.

Definition no_recursively_linked_continuation_owner
    (state : abstract_candidate_work_state) : Prop :=
  work_state_owns_linked_parent state = false.

Theorem arena_work_state_never_owns_a_recursive_parent_link : forall node_index,
  no_recursively_linked_continuation_owner
    (make_journal_work_state node_index).
Proof. reflexivity. Qed.

Record path_checkpoint : Type := PathCheckpoint {
  checkpoint_authority_identity : nat;
  checkpoint_continuation : path_continuation
}.

Definition make_checkpoint
    (authority : candidate_authority) (continuation : path_continuation)
    : option path_checkpoint :=
  match product_focus authority (flatten_continuation continuation) with
  | Some _ => Some (PathCheckpoint (authority_identity authority) continuation)
  | None => None
  end.

Definition reopen_checkpoint
    (authority : candidate_authority) (checkpoint : path_checkpoint)
    : option (trie unit * trie nat) :=
  if Nat.eqb (checkpoint_authority_identity checkpoint)
      (authority_identity authority)
  then product_focus authority
      (flatten_continuation (checkpoint_continuation checkpoint))
  else None.

Record focused_snapshot : Type := FocusedSnapshot {
  snapshot_availability : trie unit;
  snapshot_positions : trie nat
}.

Definition make_focused_snapshot
    (authority : candidate_authority) (continuation : path_continuation)
    : option focused_snapshot :=
  match product_focus authority (flatten_continuation continuation) with
  | Some (availability, positions) =>
      Some (FocusedSnapshot availability positions)
  | None => None
  end.

Definition observe_snapshot (snapshot : focused_snapshot) : list nat :=
  observe_product
    (snapshot_availability snapshot) (snapshot_positions snapshot).

Definition observe_reopened
    (authority : candidate_authority) (checkpoint : path_checkpoint)
    : option (list nat) :=
  match reopen_checkpoint authority checkpoint with
  | Some (availability, positions) =>
      Some (observe_product availability positions)
  | None => None
  end.

Theorem checkpoint_reopens_the_exact_focused_product : forall
    authority continuation availability positions,
  product_focus authority (flatten_continuation continuation) =
    Some (availability, positions) ->
  exists checkpoint,
    make_checkpoint authority continuation = Some checkpoint /\
    reopen_checkpoint authority checkpoint = Some (availability, positions).
Proof.
  intros authority continuation availability positions focused.
  unfold make_checkpoint. rewrite focused.
  exists (PathCheckpoint (authority_identity authority) continuation).
  split; [reflexivity|].
  unfold reopen_checkpoint. simpl. rewrite Nat.eqb_refl. exact focused.
Qed.

Theorem path_checkpoint_and_snapshot_are_observationally_equivalent : forall
    authority continuation snapshot checkpoint,
  make_focused_snapshot authority continuation = Some snapshot ->
  make_checkpoint authority continuation = Some checkpoint ->
  observe_reopened authority checkpoint = Some (observe_snapshot snapshot).
Proof.
  intros authority continuation snapshot checkpoint snapshot_made checkpoint_made.
  unfold make_focused_snapshot in snapshot_made.
  unfold make_checkpoint in checkpoint_made.
  destruct (product_focus authority (flatten_continuation continuation))
    as [[availability positions]|] eqn:focused; try discriminate.
  inversion snapshot_made; inversion checkpoint_made; subst.
  unfold observe_reopened, reopen_checkpoint. simpl.
  rewrite Nat.eqb_refl, focused. reflexivity.
Qed.

Theorem persistent_extension_owns_only_the_new_frame : forall prefix frame,
  owned_continuation_bytes (ExtendPath prefix frame) =
    owned_continuation_bytes prefix + length frame.
Proof. reflexivity. Qed.

Theorem persistent_extension_flattens_in_lockstep : forall prefix frame,
  flatten_continuation (ExtendPath prefix frame) =
    flatten_continuation prefix ++ frame.
Proof. reflexivity. Qed.

Record product_cursor : Type := ProductCursor {
  availability_cursor_path : path;
  positions_cursor_path : path
}.

Definition aligned_cursor (address : path) : product_cursor :=
  ProductCursor address address.

Record synchronized_query_state : Type := SynchronizedQueryState {
  query_state_continuation : path_continuation;
  query_state_cursor : product_cursor
}.

Definition query_state_path_lockstep
    (state : synchronized_query_state) : Prop :=
  availability_cursor_path (query_state_cursor state) =
    flatten_continuation (query_state_continuation state) /\
  positions_cursor_path (query_state_cursor state) =
    flatten_continuation (query_state_continuation state).

Definition consume_complete_atom_frame
    (state : synchronized_query_state) (frame : path)
    : synchronized_query_state :=
  SynchronizedQueryState
    (ExtendPath (query_state_continuation state) frame)
    (ProductCursor
      (availability_cursor_path (query_state_cursor state) ++ frame)
      (positions_cursor_path (query_state_cursor state) ++ frame)).

Theorem every_consumed_atom_extends_logical_and_physical_paths : forall
    state frame,
  query_state_path_lockstep state ->
  query_state_path_lockstep (consume_complete_atom_frame state frame).
Proof.
  intros state frame [availability_aligned positions_aligned].
  unfold query_state_path_lockstep, consume_complete_atom_frame. simpl.
  now rewrite availability_aligned, positions_aligned.
Qed.

(** A frame scanner owns the cursor only temporarily and must return it to
    its structural base before another work state is resumed. *)
Definition scanner_return_path (_traversed base : path) : path := base.

Theorem scanner_returns_to_its_structural_base : forall traversed base,
  scanner_return_path traversed base = base.
Proof. reflexivity. Qed.

Theorem remainder_extension_makes_strict_progress : forall continuation frame,
  frame <> [] ->
  length (flatten_continuation (ExtendPath continuation frame)) >
  length (flatten_continuation continuation).
Proof.
  intros continuation frame nonempty. simpl. rewrite length_app.
  destruct frame; [contradiction|simpl; lia].
Qed.

(** Exact refinement of [StoredSequenceFrameScanner].  RhoPath uses zero as
    the escape byte, 255 as escaped zero, and decoded [0,0] as an inner frame
    terminator.  The scanner sits below an outer datum frame, hence it tracks
    the two escape layers independently. *)
Definition scanner_escape : byte := 0.
Definition scanner_escaped_zero : byte := 255.
Definition scanner_frame_end : byte := 0.

Inductive stored_sequence_frame_step : Type :=
| ScanNeedMore
| ScanComplete
| ScanInvalid.

Record stored_sequence_frame_scanner : Type := StoredSequenceFrameScanner {
  scanner_outer_escape : bool;
  scanner_inner_escape : bool
}.

Definition scanner_default : stored_sequence_frame_scanner :=
  StoredSequenceFrameScanner false false.

Definition scanner_feed_inner
    (scanner : stored_sequence_frame_scanner) (input : byte)
    : stored_sequence_frame_scanner * stored_sequence_frame_step :=
  if scanner_inner_escape scanner then
    let next := StoredSequenceFrameScanner
      (scanner_outer_escape scanner) false in
    if Nat.eqb input scanner_escaped_zero then (next, ScanNeedMore)
    else if Nat.eqb input scanner_frame_end then (next, ScanComplete)
    else (next, ScanInvalid)
  else
    if Nat.eqb input scanner_escape then
      (StoredSequenceFrameScanner
        (scanner_outer_escape scanner) true, ScanNeedMore)
    else (scanner, ScanNeedMore).

Definition scanner_feed
    (scanner : stored_sequence_frame_scanner) (input : byte)
    : stored_sequence_frame_scanner * stored_sequence_frame_step :=
  if scanner_outer_escape scanner then
    let next := StoredSequenceFrameScanner false
      (scanner_inner_escape scanner) in
    if Nat.eqb input scanner_escaped_zero then
      scanner_feed_inner next scanner_escape
    else if Nat.eqb input scanner_frame_end then (next, ScanInvalid)
    else (next, ScanInvalid)
  else if Nat.eqb input scanner_escape then
    (StoredSequenceFrameScanner true (scanner_inner_escape scanner), ScanNeedMore)
  else scanner_feed_inner scanner input.

Fixpoint scanner_initialize_prefix
    (scanner : stored_sequence_frame_scanner) (prefix : path)
    : option stored_sequence_frame_scanner :=
  match prefix with
  | [] => Some scanner
  | input :: rest =>
      match scanner_feed scanner input with
      | (next, ScanNeedMore) => scanner_initialize_prefix next rest
      | (_, ScanComplete) | (_, ScanInvalid) => None
      end
  end.

Inductive scanner_prefix_initializes :
    stored_sequence_frame_scanner -> path ->
    stored_sequence_frame_scanner -> Prop :=
| ScannerPrefixNil : forall scanner,
    scanner_prefix_initializes scanner [] scanner
| ScannerPrefixCons : forall scanner input next rest final,
    scanner_feed scanner input = (next, ScanNeedMore) ->
    scanner_prefix_initializes next rest final ->
    scanner_prefix_initializes scanner (input :: rest) final.

Theorem scanner_prefix_initialization_is_sound : forall scanner prefix final,
  scanner_initialize_prefix scanner prefix = Some final ->
  scanner_prefix_initializes scanner prefix final.
Proof.
  intros scanner prefix. revert scanner.
  induction prefix as [|input rest IH]; intros scanner final initialized; simpl in *.
  - inversion initialized. constructor.
  - destruct (scanner_feed scanner input) as [next step] eqn:fed.
    destruct step; try discriminate.
    econstructor; [exact fed|]. now apply IH.
Qed.

Theorem scanner_prefix_initialization_is_complete : forall scanner prefix final,
  scanner_prefix_initializes scanner prefix final ->
  scanner_initialize_prefix scanner prefix = Some final.
Proof.
  intros scanner prefix final initialized. induction initialized; simpl.
  - reflexivity.
  - rewrite H. exact IHinitialized.
Qed.

Fixpoint scanner_drive_until_classified
    (scanner : stored_sequence_frame_scanner) (inputs : path)
    : stored_sequence_frame_scanner * stored_sequence_frame_step :=
  match inputs with
  | [] => (scanner, ScanNeedMore)
  | input :: rest =>
      match scanner_feed scanner input with
      | (next, ScanNeedMore) => scanner_drive_until_classified next rest
      | classified => classified
      end
  end.

Theorem outer_datum_frame_end_is_not_a_sequence_frame :
  scanner_drive_until_classified scanner_default [0; 0] =
    (scanner_default, ScanInvalid).
Proof. reflexivity. Qed.

Theorem double_escaped_inner_zero_completes_a_sequence_frame :
  scanner_drive_until_classified scanner_default [0; 255; 0; 255] =
    (scanner_default, ScanComplete).
Proof. reflexivity. Qed.

Theorem malformed_outer_escape_is_invalid :
  scanner_drive_until_classified scanner_default [0; 7] =
    (scanner_default, ScanInvalid).
Proof. reflexivity. Qed.

(** Refinement of the iterative level stack used by
    [visit_sequence_frames_with_prefix].  Suffix bytes are stored reversed in
    the model because only push/pop balance matters.  Complete probes descend
    and ascend in one transition; invalid probes never descend. *)
Record scanner_walk_state : Type := ScannerWalkState {
  scanner_walk_levels : list stored_sequence_frame_scanner;
  scanner_walk_cursor_suffix_rev : path;
  scanner_walk_frame_suffix_rev : path
}.

Definition scanner_walk_balanced (state : scanner_walk_state) : Prop :=
  scanner_walk_cursor_suffix_rev state = scanner_walk_frame_suffix_rev state /\
  match scanner_walk_levels state with
  | [] => scanner_walk_cursor_suffix_rev state = []
  | _ => length (scanner_walk_levels state) =
      S (length (scanner_walk_cursor_suffix_rev state))
  end.

Inductive scanner_walk_transition : scanner_walk_state -> scanner_walk_state -> Prop :=
| ScannerWalkNeedMore : forall current older cursor frame input next,
    scanner_feed current input = (next, ScanNeedMore) ->
    scanner_walk_transition
      (ScannerWalkState (current :: older) cursor frame)
      (ScannerWalkState (next :: current :: older)
        (input :: cursor) (input :: frame))
| ScannerWalkComplete : forall current older cursor frame input next,
    scanner_feed current input = (next, ScanComplete) ->
    scanner_walk_transition
      (ScannerWalkState (current :: older) cursor frame)
      (ScannerWalkState (current :: older) cursor frame)
| ScannerWalkInvalid : forall current older cursor frame input next,
    scanner_feed current input = (next, ScanInvalid) ->
    scanner_walk_transition
      (ScannerWalkState (current :: older) cursor frame)
      (ScannerWalkState (current :: older) cursor frame)
| ScannerWalkPopChild : forall current parent older input cursor,
    scanner_walk_transition
      (ScannerWalkState (current :: parent :: older)
        (input :: cursor) (input :: cursor))
      (ScannerWalkState (parent :: older) cursor cursor)
| ScannerWalkFinishBase : forall current,
    scanner_walk_transition
      (ScannerWalkState [current] [] [])
      (ScannerWalkState [] [] []).

Theorem scanner_walk_transition_preserves_balance : forall before after,
  scanner_walk_balanced before ->
  scanner_walk_transition before after ->
  scanner_walk_balanced after.
Proof.
  intros before after balanced transition.
  inversion transition; subst; unfold scanner_walk_balanced in *; simpl in *.
  - destruct balanced as [paths levels]. split; [now f_equal|lia].
  - exact balanced.
  - exact balanced.
  - destruct balanced as [paths levels]. inversion paths. split; [reflexivity|lia].
  - split; reflexivity.
Qed.

Inductive scanner_walk_transitions : scanner_walk_state -> scanner_walk_state -> Prop :=
| ScannerWalkRefl : forall state, scanner_walk_transitions state state
| ScannerWalkStep : forall first middle last,
    scanner_walk_transition first middle ->
    scanner_walk_transitions middle last ->
    scanner_walk_transitions first last.

Theorem scanner_walk_transitions_preserve_balance : forall first last,
  scanner_walk_balanced first ->
  scanner_walk_transitions first last ->
  scanner_walk_balanced last.
Proof.
  intros first last balanced transitions. revert balanced.
  induction transitions; intro balanced.
  - exact balanced.
  - apply IHtransitions.
    eapply scanner_walk_transition_preserves_balance; eauto.
Qed.

Theorem completed_scanner_walk_restores_cursor_and_frame_to_prefix_base : forall
    initialized terminal,
  scanner_walk_transitions
    (ScannerWalkState [initialized] [] []) terminal ->
  scanner_walk_levels terminal = [] ->
  scanner_walk_cursor_suffix_rev terminal = [] /\
  scanner_walk_frame_suffix_rev terminal = [].
Proof.
  intros initialized terminal transitions done.
  assert (balanced : scanner_walk_balanced terminal).
  { eapply scanner_walk_transitions_preserve_balance; [|exact transitions].
    unfold scanner_walk_balanced. simpl. auto. }
  unfold scanner_walk_balanced in balanced. rewrite done in balanced.
  simpl in balanced. destruct balanced as [same cursor_empty].
  split; [exact cursor_empty|]. now rewrite <- same.
Qed.

(** A first-child/next-sibling tree represents the append-only arena's logical
    branch topology.  Every non-root node weight is its owned frame length.
    A depth-first enter/exit scheduler descends and ascends every arena edge
    exactly once; therefore whole-tree navigation is linear in encoded bytes. *)
Inductive arena_navigation_tree : Type :=
| ArenaNavEmpty
| ArenaNavNode : nat -> arena_navigation_tree -> arena_navigation_tree ->
    arena_navigation_tree.

Fixpoint arena_navigation_edge_bytes (tree : arena_navigation_tree) : nat :=
  match tree with
  | ArenaNavEmpty => 0
  | ArenaNavNode frame_length first_child next_sibling =>
      frame_length + arena_navigation_edge_bytes first_child +
      arena_navigation_edge_bytes next_sibling
  end.

Fixpoint depth_first_enter_exit_navigation_cost
    (tree : arena_navigation_tree) : nat :=
  match tree with
  | ArenaNavEmpty => 0
  | ArenaNavNode frame_length first_child next_sibling =>
      frame_length + depth_first_enter_exit_navigation_cost first_child +
      frame_length + depth_first_enter_exit_navigation_cost next_sibling
  end.

Theorem whole_branching_tree_navigation_is_amortized_linear : forall tree,
  depth_first_enter_exit_navigation_cost tree =
    2 * arena_navigation_edge_bytes tree.
Proof. induction tree; simpl; lia. Qed.

Theorem whole_branching_tree_navigation_has_constant_edge_charge : forall tree,
  depth_first_enter_exit_navigation_cost tree <=
    2 * arena_navigation_edge_bytes tree.
Proof. intro tree. rewrite whole_branching_tree_navigation_is_amortized_linear. lia. Qed.

Theorem candidate_product_scanner_scheduler_capstone : forall
    prefix initialized terminal navigation_tree,
  scanner_initialize_prefix scanner_default prefix = Some initialized ->
  scanner_walk_transitions
    (ScannerWalkState [initialized] [] []) terminal ->
  scanner_walk_levels terminal = [] ->
  scanner_prefix_initializes scanner_default prefix initialized /\
  scanner_walk_cursor_suffix_rev terminal = [] /\
  scanner_walk_frame_suffix_rev terminal = [] /\
  scanner_drive_until_classified scanner_default [0; 255; 0; 255] =
    (scanner_default, ScanComplete) /\
  scanner_drive_until_classified scanner_default [0; 0] =
    (scanner_default, ScanInvalid) /\
  depth_first_enter_exit_navigation_cost navigation_tree =
    2 * arena_navigation_edge_bytes navigation_tree.
Proof.
  intros prefix initialized terminal navigation_tree prefix_ok walked done.
  pose proof (completed_scanner_walk_restores_cursor_and_frame_to_prefix_base
    (initialized := initialized) (terminal := terminal) walked done)
    as [cursor_restored frame_restored].
  repeat split.
  - now eapply scanner_prefix_initialization_is_sound.
  - exact cursor_restored.
  - exact frame_restored.
  - apply whole_branching_tree_navigation_is_amortized_linear.
Qed.

Definition attempt_product_descent
    (authority : candidate_authority) (cursor : product_cursor) (segment : path)
    : product_cursor * bool :=
  let availability_target := availability_cursor_path cursor ++ segment in
  let positions_target := positions_cursor_path cursor ++ segment in
  match focus (authority_availability authority) availability_target,
        focus (authority_positions authority) positions_target with
  | Some _, Some _ => (ProductCursor availability_target positions_target, true)
  | _, _ => (cursor, false)
  end.

Theorem asymmetric_failed_descent_restores_both_cursors : forall
    authority cursor segment,
  snd (attempt_product_descent authority cursor segment) = false ->
  fst (attempt_product_descent authority cursor segment) = cursor.
Proof.
  intros authority cursor segment failed.
  unfold attempt_product_descent in *.
  destruct (focus (authority_availability authority)
    (availability_cursor_path cursor ++ segment));
  destruct (focus (authority_positions authority)
    (positions_cursor_path cursor ++ segment)); simpl in *;
    try discriminate; reflexivity.
Qed.

Theorem successful_descent_preserves_product_alignment : forall
    authority address segment,
  snd (attempt_product_descent authority (aligned_cursor address) segment) = true ->
  availability_cursor_path
    (fst (attempt_product_descent authority (aligned_cursor address) segment)) =
  positions_cursor_path
    (fst (attempt_product_descent authority (aligned_cursor address) segment)).
Proof.
  intros authority address segment succeeded.
  unfold attempt_product_descent in *; simpl in *.
  destruct (focus (authority_availability authority) (address ++ segment));
  destruct (focus (authority_positions authority) (address ++ segment)); simpl in *;
    try discriminate; reflexivity.
Qed.

Theorem root_endpoint_is_observed_before_descendants : forall
    availability_children positions_children position,
  hd_error
    (observe_product
      (Node (Some tt) availability_children)
      (Node (Some position) positions_children)) = Some position.
Proof. intros. reflexivity. Qed.

Theorem exact_or_repeated_capture_extends_once : forall continuation frame,
  flatten_continuation (ExtendPath continuation frame) =
    flatten_continuation continuation ++ frame.
Proof. reflexivity. Qed.

Theorem nested_match_prefix_and_tail_are_not_duplicated : forall
    continuation product_prefix completed_tail,
  flatten_continuation
    (ExtendPath continuation (product_prefix ++ completed_tail)) =
  (flatten_continuation continuation ++ product_prefix) ++ completed_tail.
Proof. intros. simpl. now rewrite app_assoc. Qed.

Definition result_suffix : path := [0; 0].

Definition visit_result_suffix_then_restore
    (_ : candidate_authority) (checkpoint : path_checkpoint) : path_checkpoint :=
  checkpoint.

Theorem result_and_remainder_traversal_restore_the_structural_focus : forall
    authority checkpoint,
  visit_result_suffix_then_restore authority checkpoint = checkpoint.
Proof. reflexivity. Qed.

Theorem changed_authority_identity_cannot_reopen : forall
    authority checkpoint,
  checkpoint_authority_identity checkpoint <>
    authority_identity authority ->
  reopen_checkpoint authority checkpoint = None.
Proof.
  intros authority checkpoint different.
  unfold reopen_checkpoint.
  apply Nat.eqb_neq in different. now rewrite different.
Qed.

(** Runtime refinement boundary: [Bag::with_read_cursor] must enclose the
    entire work loop, and the positions zipper must borrow one immutable,
    guarded position authority for that closure.  Neither zipper may escape.
    [CandidateQueryState] carries only stable indices into one query-local Vec
    arena; it owns no raw trie header and no recursively linked parent node.
    The theorem collects the obligations for one consumed atom / resumed
    branch; repeated application covers exact, repeated-capture, wildcard,
    nested-match, and accepts-remainder work states. *)
Theorem candidate_product_single_moving_pda_capstone : forall
    state frame environment edits slot_count slot traversed base,
  query_state_path_lockstep state ->
  frame <> [] ->
  dense_capture_slot_valid slot_count slot ->
  query_state_path_lockstep (consume_complete_atom_frame state frame) /\
  moving_zipper_navigation_cost
    (ZipperRebasePlan (query_state_continuation state) [] [frame]) =
      length frame /\
  undo_capture_edits (length edits)
    (apply_capture_edits environment edits) = environment /\
  no_raw_capture_checkpoint
    (make_journal_work_state 0) /\
  no_recursively_linked_continuation_owner
    (make_journal_work_state 0) /\
  access_dense_capture_slot slot_count slot = Some slot /\
  scanner_return_path traversed base = base /\
  length (flatten_continuation
    (ExtendPath (query_state_continuation state) frame)) >
  length (flatten_continuation (query_state_continuation state)).
Proof.
  intros state frame environment edits slot_count slot traversed base
    aligned nonempty valid.
  split.
  - now apply every_consumed_atom_extends_logical_and_physical_paths.
  - split.
    + apply moving_zipper_parent_to_child_costs_only_the_new_frame.
    + split.
      * apply capture_journal_suffix_is_exactly_reversible.
      * split.
        -- apply journal_work_state_never_owns_a_raw_capture_header.
        -- split.
           ++ apply arena_work_state_never_owns_a_recursive_parent_link.
           ++ split.
              ** now apply a_compiled_valid_capture_slot_is_directly_addressable.
              ** split.
                 --- apply scanner_returns_to_its_structural_base.
                 --- now apply remainder_extension_makes_strict_progress.
Qed.

Inductive continuation_invariant_id : Type :=
| ImmutableAuthorityInvariant
| DualCursorAlignmentInvariant
| PersistentPrefixSharingInvariant
| FocusReopenInvariant
| SnapshotObservationEquivalenceInvariant
| RootEndpointInvariant
| DescendantObservationInvariant
| AsymmetricFailureRestorationInvariant
| ExactAndCaptureExtensionInvariant
| NestedMatchExtensionInvariant
| RemainderRestorationInvariant
| StaleAuthorityRejectionInvariant
| SingleMovingProductCursorInvariant
| SharedAncestorRebaseInvariant
| LinearSequentialNavigationInvariant
| CaptureJournalExactnessInvariant
| CaptureJournalRebaseInvariant
| NoRawCaptureCheckpointInvariant
| ConsumedAtomPathLockstepInvariant
| DenseCaptureSlotInvariant
| CaptureUndoBeforeRedoInvariant
| ScannerBaseRestorationInvariant
| RemainderStrictProgressInvariant
| QueryLocalArenaIndexInvariant
| FlatArenaTeardownInvariant
| NoRecursiveLinkedOwnerInvariant
| ExactScannerTransitionInvariant
| PrefixInitializationRefinementInvariant
| ScannerWalkBalanceInvariant
| DepthFirstSchedulerOrderInvariant
| WholeTreeAmortizedNavigationInvariant.

Definition continuation_invariant_catalog : list continuation_invariant_id :=
  [ImmutableAuthorityInvariant; DualCursorAlignmentInvariant;
   PersistentPrefixSharingInvariant; FocusReopenInvariant;
   SnapshotObservationEquivalenceInvariant; RootEndpointInvariant;
   DescendantObservationInvariant; AsymmetricFailureRestorationInvariant;
   ExactAndCaptureExtensionInvariant; NestedMatchExtensionInvariant;
   RemainderRestorationInvariant; StaleAuthorityRejectionInvariant;
   SingleMovingProductCursorInvariant; SharedAncestorRebaseInvariant;
   LinearSequentialNavigationInvariant; CaptureJournalExactnessInvariant;
   CaptureJournalRebaseInvariant; NoRawCaptureCheckpointInvariant;
   ConsumedAtomPathLockstepInvariant; DenseCaptureSlotInvariant;
   CaptureUndoBeforeRedoInvariant; ScannerBaseRestorationInvariant;
   RemainderStrictProgressInvariant; QueryLocalArenaIndexInvariant;
   FlatArenaTeardownInvariant; NoRecursiveLinkedOwnerInvariant;
   ExactScannerTransitionInvariant; PrefixInitializationRefinementInvariant;
   ScannerWalkBalanceInvariant; DepthFirstSchedulerOrderInvariant;
   WholeTreeAmortizedNavigationInvariant].

Theorem continuation_invariant_catalog_is_duplicate_free_and_exact :
  NoDup continuation_invariant_catalog /\
  length continuation_invariant_catalog = 31.
Proof. split; [repeat constructor; simpl; intuition discriminate|reflexivity]. Qed.

Theorem candidate_product_path_continuation_capstone : forall
    authority continuation snapshot checkpoint,
  make_focused_snapshot authority continuation = Some snapshot ->
  make_checkpoint authority continuation = Some checkpoint ->
  observe_reopened authority checkpoint = Some (observe_snapshot snapshot) /\
  checkpoint_authority_identity checkpoint = authority_identity authority /\
  flatten_continuation (checkpoint_continuation checkpoint) =
    flatten_continuation continuation /\
  NoDup continuation_invariant_catalog.
Proof.
  intros authority continuation snapshot checkpoint snapshot_made checkpoint_made.
  split.
  - eapply path_checkpoint_and_snapshot_are_observationally_equivalent; eauto.
  - unfold make_checkpoint in checkpoint_made.
    destruct (product_focus authority (flatten_continuation continuation));
      try discriminate.
    inversion checkpoint_made; subst. repeat split; try reflexivity.
    exact (proj1 continuation_invariant_catalog_is_duplicate_free_and_exact).
Qed.

End CandidateProductPathContinuation.
