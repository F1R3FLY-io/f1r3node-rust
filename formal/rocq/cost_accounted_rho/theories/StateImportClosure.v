From Stdlib Require Import List Arith Bool.

Import ListNotations.

Inductive ImportLeafKind : Type :=
| ImportJoins
| ImportData
| ImportContinuations.

Definition import_leaf_kind_eq_dec :
  forall left right : ImportLeafKind, {left = right} + {left <> right}.
Proof. decide equality. Defined.

Inductive ImportReference : Type :=
| ImportHistoryRef (key : nat) (context : list nat)
| ImportColdRef (key : nat) (kind : ImportLeafKind).

Definition import_reference_eq_dec :
  forall left right : ImportReference, {left = right} + {left <> right}.
Proof.
  decide equality.
  - apply list_eq_dec. apply Nat.eq_dec.
  - apply Nat.eq_dec.
  - apply import_leaf_kind_eq_dec.
  - apply Nat.eq_dec.
Defined.

Definition import_unchecked_frontier
  (checked frontier : list ImportReference) : list ImportReference :=
  filter (fun reference => if in_dec import_reference_eq_dec reference checked
    then false else true) frontier.

Lemma import_frontier_member_is_checked_or_retained :
  forall checked frontier reference,
    In reference frontier ->
    In reference checked \/ In reference (import_unchecked_frontier checked frontier).
Proof.
  intros checked frontier reference member.
  destruct (in_dec import_reference_eq_dec reference checked) as [done | waiting].
  - left. exact done.
  - right. unfold import_unchecked_frontier. apply filter_In. split.
    + exact member.
    + destruct (in_dec import_reference_eq_dec reference checked); congruence.
Qed.

Inductive ImportRadixTarget : Type :=
| ImportRadixNode (key : nat)
| ImportRadixLeaf (key : nat).

Record ImportRadixEdge : Type := {
  import_edge_index : nat;
  import_edge_prefix : list nat;
  import_edge_target : ImportRadixTarget
}.

Definition import_kind_at_path (path : list nat) : option ImportLeafKind :=
  match path with
  | 0 :: _ => Some ImportData
  | 1 :: _ => Some ImportContinuations
  | 2 :: _ => Some ImportJoins
  | _ => None
  end.

Definition import_edge_reference
  (context : list nat) (edge : ImportRadixEdge) : option ImportReference :=
  let path := context ++ import_edge_index edge :: import_edge_prefix edge in
  match import_edge_target edge with
  | ImportRadixNode key => Some (ImportHistoryRef key path)
  | ImportRadixLeaf key =>
      match import_kind_at_path path with
      | Some kind => Some (ImportColdRef key kind)
      | None => None
      end
  end.

Fixpoint import_contextual_children
  (context : list nat) (edges : list ImportRadixEdge) : option (list ImportReference) :=
  match edges with
  | [] => Some []
  | edge :: rest =>
      match import_edge_reference context edge, import_contextual_children context rest with
      | Some reference, Some children => Some (reference :: children)
      | _, _ => None
      end
  end.

Record ImportLeaf : Type := {
  import_leaf_kind : ImportLeafKind;
  import_leaf_payload : list nat
}.

Definition import_leaf_eq_dec :
  forall left right : ImportLeaf, {left = right} + {left <> right}.
Proof.
  decide equality.
  - apply list_eq_dec. apply Nat.eq_dec.
  - apply import_leaf_kind_eq_dec.
Defined.

Record ImportStore : Type := {
  import_history : nat -> option (list ImportRadixEdge);
  import_cold_raw : nat -> option ImportLeaf;
  import_cold_legacy : nat -> option ImportLeaf
}.

Definition import_resolve_cold (store : ImportStore) (key : nat) : option ImportLeaf :=
  match import_cold_raw store key, import_cold_legacy store key with
  | None, None => None
  | Some value, None | None, Some value => Some value
  | Some raw, Some legacy =>
      if import_leaf_eq_dec raw legacy then Some raw else None
  end.

Section ImportClosure.

Variable history_hash : list ImportRadixEdge -> nat.
Variable payload_hash : list nat -> nat.

Definition import_checked_read
  (store : ImportStore) (reference : ImportReference) : option (list ImportReference) :=
  match reference with
  | ImportHistoryRef key context =>
      match import_history store key with
      | None => None
      | Some children =>
          if Nat.eqb (history_hash children) key
          then import_contextual_children context children else None
      end
  | ImportColdRef key expected_kind =>
      match import_resolve_cold store key with
      | None => None
      | Some value =>
          if Nat.eqb (payload_hash (import_leaf_payload value)) key then
            if import_leaf_kind_eq_dec (import_leaf_kind value) expected_kind
            then Some [] else None
          else None
      end
  end.

Inductive import_reachable (store : ImportStore) (root : ImportReference) :
  ImportReference -> Prop :=
| import_reachable_root : import_reachable store root root
| import_reachable_child : forall parent child children,
    import_reachable store root parent ->
    import_checked_read store parent = Some children ->
    In child children ->
    import_reachable store root child.

Definition import_closed (store : ImportStore) (root : ImportReference) : Prop :=
  forall reference,
    import_reachable store root reference ->
    exists children, import_checked_read store reference = Some children.

Definition import_checked_extension (old new : ImportStore) : Prop :=
  forall reference children,
    import_checked_read old reference = Some children ->
    import_checked_read new reference = Some children.

Definition import_binding_extension (old new : ImportStore) : Prop :=
  (forall key children,
      import_history old key = Some children ->
      import_history new key = Some children) /\
  (forall key value,
      import_resolve_cold old key = Some value ->
      import_resolve_cold new key = Some value).

Lemma import_binding_extension_preserves_checked_reads :
  forall old new,
    import_binding_extension old new -> import_checked_extension old new.
Proof.
  intros old new [history_ext cold_ext] reference children read.
  destruct reference as [key context | key kind]; simpl in *.
  - destruct (import_history old key) as [stored |] eqn:found; try discriminate.
    rewrite (history_ext key stored found). exact read.
  - destruct (import_resolve_cold old key) as [stored |] eqn:found; try discriminate.
    rewrite (cold_ext key stored found). exact read.
Qed.

Lemma import_checked_extension_reflexive :
  forall store, import_checked_extension store store.
Proof. intros store reference children read. exact read. Qed.

Lemma import_checked_extension_transitive :
  forall first second third,
    import_checked_extension first second ->
    import_checked_extension second third ->
    import_checked_extension first third.
Proof.
  intros first second third first_ext second_ext reference children read.
  apply second_ext. apply first_ext. exact read.
Qed.

Lemma import_closed_extension_reachability :
  forall old new root,
    import_closed old root ->
    import_checked_extension old new ->
    forall reference,
      import_reachable new root reference ->
      import_reachable old root reference.
Proof.
  intros old new root closed extension reference reached.
  induction reached as [| parent child children reached previous read contained].
  - constructor.
  - destruct (closed parent previous) as [old_children old_read].
    pose proof (extension parent old_children old_read) as new_read.
    rewrite read in new_read. inversion new_read. subst old_children.
    eapply import_reachable_child; eauto.
Qed.

Theorem import_compatible_insertion_preserves_closed_root :
  forall old new root,
    import_closed old root ->
    import_checked_extension old new ->
    import_closed new root.
Proof.
  intros old new root closed extension reference reached.
  pose proof (import_closed_extension_reachability old new root closed extension
    reference reached) as previous.
  destruct (closed reference previous) as [children read].
  exists children. apply extension. exact read.
Qed.

Definition import_frontier_cut
  (store : ImportStore) (root : ImportReference)
  (checked frontier : list ImportReference) : Prop :=
  (In root checked \/ In root frontier) /\
  (forall reference, In reference checked ->
    exists children,
      import_checked_read store reference = Some children /\
      forall child, In child children -> In child checked \/ In child frontier).

Theorem import_initial_frontier_covers_root :
  forall store root, import_frontier_cut store root [] [root].
Proof.
  intros store root. split.
  - right. simpl. auto.
  - intros reference impossible. inversion impossible.
Qed.

Lemma import_advance_cut_member :
  forall (checked frontier : list ImportReference) (current : ImportReference)
    (children : list ImportReference) (reference : ImportReference),
    In reference checked \/ In reference (current :: frontier) ->
    In reference (current :: checked) \/ In reference (children ++ frontier).
Proof.
  intros checked frontier current children reference [done | [same | waiting]].
  - left. right. exact done.
  - subst reference. left. left. reflexivity.
  - right. apply in_or_app. right. exact waiting.
Qed.

Theorem import_check_step_preserves_frontier_cut :
  forall store root checked frontier current children,
    import_frontier_cut store root checked (current :: frontier) ->
    import_checked_read store current = Some children ->
    import_frontier_cut store root (current :: checked) (children ++ frontier).
Proof.
  intros store root checked frontier current children [root_member cut] read.
  split.
  - apply import_advance_cut_member. exact root_member.
  - intros reference [same | already_checked].
    + subst reference. exists children. split.
      * exact read.
      * intros child member. right. apply in_or_app. left. exact member.
    + destruct (cut reference already_checked) as [successors [old_read covered]].
      exists successors. split.
      * exact old_read.
      * intros child member. apply import_advance_cut_member. apply covered. exact member.
Qed.

Theorem import_removing_checked_work_preserves_frontier_cut :
  forall store root checked frontier,
    import_frontier_cut store root checked frontier ->
    import_frontier_cut store root checked (import_unchecked_frontier checked frontier).
Proof.
  intros store root checked frontier [root_member cut]. split.
  - destruct root_member as [done | waiting].
    + left. exact done.
    + apply import_frontier_member_is_checked_or_retained. exact waiting.
  - intros reference member.
    destruct (cut reference member) as [children [read covered]].
    exists children. split.
    + exact read.
    + intros child contained. destruct (covered child contained) as [done | waiting].
      * left. exact done.
      * apply import_frontier_member_is_checked_or_retained. exact waiting.
Qed.

Theorem import_concurrent_extension_preserves_frontier_cut :
  forall old new root checked frontier,
    import_checked_extension old new ->
    import_frontier_cut old root checked frontier ->
    import_frontier_cut new root checked frontier.
Proof.
  intros old new root checked frontier extension [root_member cut].
  split.
  - exact root_member.
  - intros reference member.
    destruct (cut reference member) as [children [read covered]].
    exists children. split.
    + apply extension. exact read.
    + exact covered.
Qed.

Theorem import_empty_frontier_establishes_typed_closure :
  forall store root checked,
    import_frontier_cut store root checked [] -> import_closed store root.
Proof.
  intros store root checked [root_member cut].
  assert (all_reached_checked : forall reference,
    import_reachable store root reference -> In reference checked).
  {
    intros reference reached.
    induction reached as [| parent child children reached previous read contained].
    - destruct root_member as [member | impossible].
      + exact member.
      + inversion impossible.
    - destruct (cut parent previous) as [successors [parent_read covered]].
      rewrite read in parent_read. inversion parent_read. subst successors.
      destruct (covered child contained) as [member | impossible].
      + exact member.
      + inversion impossible.
  }
  intros reference reached.
  destruct (cut reference (all_reached_checked reference reached))
    as [children [read _]].
  exists children. exact read.
Qed.

Inductive import_compatible_history : ImportStore -> ImportStore -> Prop :=
| import_compatible_history_refl : forall store,
    import_compatible_history store store
| import_compatible_history_step : forall first middle last,
    import_compatible_history first middle ->
    import_checked_extension middle last ->
    import_compatible_history first last.

Theorem import_arbitrary_compatible_history_preserves_checked_reads :
  forall first last,
    import_compatible_history first last -> import_checked_extension first last.
Proof.
  intros first last history.
  induction history as [store | first middle last history previous step].
  - apply import_checked_extension_reflexive.
  - eapply import_checked_extension_transitive; eauto.
Qed.

Theorem import_arbitrary_compatible_history_preserves_closed_root :
  forall first last root,
    import_closed first root ->
    import_compatible_history first last -> import_closed last root.
Proof.
  intros first last root closed history.
  eapply import_compatible_insertion_preserves_closed_root.
  - exact closed.
  - apply import_arbitrary_compatible_history_preserves_checked_reads. exact history.
Qed.

Theorem import_any_number_of_shared_roots_preserved :
  forall first last roots,
    Forall (import_closed first) roots ->
    import_compatible_history first last ->
    Forall (import_closed last) roots.
Proof.
  intros first last roots closed history.
  induction closed as [| root roots root_closed others preserved].
  - constructor.
  - constructor.
    + eapply import_arbitrary_compatible_history_preserves_closed_root; eauto.
    + exact preserved.
Qed.

Record ImportScanState : Type := {
  import_scan_store : ImportStore;
  import_scan_checked : list ImportReference;
  import_scan_frontier : list ImportReference
}.

Inductive import_scan_step : ImportScanState -> ImportScanState -> Prop :=
| import_scan_reference : forall store checked frontier current children,
    import_checked_read store current = Some children ->
    import_scan_step
      {| import_scan_store := store;
         import_scan_checked := checked;
         import_scan_frontier := current :: frontier |}
      {| import_scan_store := store;
         import_scan_checked := current :: checked;
         import_scan_frontier :=
           import_unchecked_frontier (current :: checked) (children ++ frontier) |}
| import_scan_concurrent_commit : forall old new checked frontier,
    import_checked_extension old new ->
    import_scan_step
      {| import_scan_store := old;
         import_scan_checked := checked;
         import_scan_frontier := frontier |}
      {| import_scan_store := new;
         import_scan_checked := checked;
         import_scan_frontier := frontier |}.

Inductive import_scan_run : ImportScanState -> ImportScanState -> Prop :=
| import_scan_run_refl : forall state, import_scan_run state state
| import_scan_run_cons : forall first middle last,
    import_scan_step first middle ->
    import_scan_run middle last -> import_scan_run first last.

Definition import_scan_cut (root : ImportReference) (state : ImportScanState) : Prop :=
  import_frontier_cut (import_scan_store state) root
    (import_scan_checked state) (import_scan_frontier state).

Theorem import_scan_step_preserves_cut :
  forall root first last,
    import_scan_step first last ->
    import_scan_cut root first -> import_scan_cut root last.
Proof.
  intros root first last step cut.
  destruct step; unfold import_scan_cut in *; simpl in *.
  - apply import_removing_checked_work_preserves_frontier_cut.
    eapply import_check_step_preserves_frontier_cut; eauto.
  - eapply import_concurrent_extension_preserves_frontier_cut; eauto.
Qed.

Theorem import_interleaved_scan_preserves_cut :
  forall root first last,
    import_scan_run first last ->
    import_scan_cut root first -> import_scan_cut root last.
Proof.
  intros root first last run.
  induction run as [state | first middle last step run preserved]; intros cut.
  - exact cut.
  - apply preserved. eapply import_scan_step_preserves_cut; eauto.
Qed.

Theorem import_completed_interleaved_scan_establishes_closure :
  forall store root last,
    import_scan_run
      {| import_scan_store := store;
         import_scan_checked := [];
         import_scan_frontier := [root] |}
      last ->
    import_scan_frontier last = [] ->
    import_closed (import_scan_store last) root.
Proof.
  intros store root last run empty.
  assert (initial : import_scan_cut root
    {| import_scan_store := store;
       import_scan_checked := [];
       import_scan_frontier := [root] |}).
  { apply import_initial_frontier_covers_root. }
  pose proof (import_interleaved_scan_preserves_cut root _ last run initial) as cut.
  unfold import_scan_cut in cut. rewrite empty in cut.
  eapply import_empty_frontier_establishes_typed_closure. exact cut.
Qed.

Theorem import_history_read_binds_its_children :
  forall store key context children,
    import_checked_read store (ImportHistoryRef key context) = Some children ->
    exists edges,
      import_history store key = Some edges /\
      history_hash edges = key /\
      import_contextual_children context edges = Some children.
Proof.
  intros store key context children read. simpl in read.
  destruct (import_history store key) as [stored |] eqn:found; try discriminate.
  destruct (Nat.eqb (history_hash stored) key) eqn:hashed; try discriminate.
  exists stored. split.
  - reflexivity.
  - split.
    + apply Nat.eqb_eq. exact hashed.
    + exact read.
Qed.

Theorem import_cold_reference_requires_matching_kind :
  forall store key expected children,
    import_checked_read store (ImportColdRef key expected) = Some children ->
    exists value,
      import_resolve_cold store key = Some value /\
      import_leaf_kind value = expected /\
      payload_hash (import_leaf_payload value) = key /\
      children = [].
Proof.
  intros store key expected children read.
  simpl in read.
  destruct (import_resolve_cold store key) as [value |] eqn:found; try discriminate.
  destruct (Nat.eqb (payload_hash (import_leaf_payload value)) key) eqn:hashed;
    try discriminate.
  destruct (import_leaf_kind_eq_dec (import_leaf_kind value) expected) as [kind | wrong];
    try discriminate.
  inversion read. subst children.
  exists value. split.
  - reflexivity.
  - split.
    + exact kind.
    + split.
      * apply Nat.eqb_eq. exact hashed.
      * reflexivity.
Qed.

Theorem import_missing_history_root_is_not_closed :
  forall store key context,
    import_history store key = None ->
    ~ import_closed store (ImportHistoryRef key context).
Proof.
  intros store key context missing closed.
  destruct (closed _ (import_reachable_root store _)) as [children read].
  simpl in read. rewrite missing in read. discriminate.
Qed.

Theorem import_missing_reachable_reference_is_not_closed :
  forall store root reference,
    import_reachable store root reference ->
    import_checked_read store reference = None -> ~ import_closed store root.
Proof.
  intros store root reference reachable missing closed.
  destruct (closed reference reachable) as [children read].
  rewrite missing in read. discriminate.
Qed.

End ImportClosure.

Theorem import_contextual_children_account_for_every_edge :
  forall context edges children,
    import_contextual_children context edges = Some children ->
    Forall2 (fun edge reference => import_edge_reference context edge = Some reference)
      edges children.
Proof.
  intros context edges. induction edges as [| edge rest previous]; intros children read.
  - simpl in read. inversion read. constructor.
  - simpl in read.
    destruct (import_edge_reference context edge) as [reference |] eqn:found;
      try discriminate.
    destruct (import_contextual_children context rest) as [tail |] eqn:remaining;
      try discriminate.
    inversion read. subst children. constructor.
    + exact found.
    + apply previous. reflexivity.
Qed.

Theorem import_history_child_retains_occurrence_context :
  forall context edge key,
    import_edge_target edge = ImportRadixNode key ->
    import_edge_reference context edge =
      Some (ImportHistoryRef key
        (context ++ import_edge_index edge :: import_edge_prefix edge)).
Proof.
  intros context edge key target. unfold import_edge_reference. rewrite target. reflexivity.
Qed.

Theorem import_cold_child_kind_comes_from_occurrence_path :
  forall context edge key kind,
    import_edge_target edge = ImportRadixLeaf key ->
    import_edge_reference context edge = Some (ImportColdRef key kind) ->
    import_kind_at_path
      (context ++ import_edge_index edge :: import_edge_prefix edge) = Some kind.
Proof.
  intros context edge key kind target read.
  unfold import_edge_reference in read. rewrite target in read.
  destruct (import_kind_at_path
    (context ++ import_edge_index edge :: import_edge_prefix edge)) as [found |];
    inversion read. reflexivity.
Qed.

Example import_shared_node_has_context_sensitive_leaf_kinds :
  let edge := {| import_edge_index := 9; import_edge_prefix := [];
    import_edge_target := ImportRadixLeaf 7 |} in
  import_contextual_children [0] [edge] = Some [ImportColdRef 7 ImportData] /\
  import_contextual_children [2] [edge] = Some [ImportColdRef 7 ImportJoins].
Proof. split; reflexivity. Qed.

Theorem import_conflicting_aliases_do_not_resolve :
  forall store key raw legacy,
    import_cold_raw store key = Some raw ->
    import_cold_legacy store key = Some legacy ->
    raw <> legacy -> import_resolve_cold store key = None.
Proof.
  intros store key raw legacy raw_found legacy_found different.
  unfold import_resolve_cold. rewrite raw_found, legacy_found.
  destruct (import_leaf_eq_dec raw legacy) as [same | not_same].
  - contradiction.
  - reflexivity.
Qed.

Theorem import_single_alias_resolves :
  forall store key value,
    (import_cold_raw store key = Some value /\ import_cold_legacy store key = None) \/
    (import_cold_raw store key = None /\ import_cold_legacy store key = Some value) ->
    import_resolve_cold store key = Some value.
Proof.
  intros store key value [[raw legacy] | [raw legacy]];
    unfold import_resolve_cold; rewrite raw, legacy; reflexivity.
Qed.

Theorem import_matching_aliases_resolve :
  forall store key value,
    import_cold_raw store key = Some value ->
    import_cold_legacy store key = Some value ->
    import_resolve_cold store key = Some value.
Proof.
  intros store key value raw legacy.
  unfold import_resolve_cold. rewrite raw, legacy.
  destruct (import_leaf_eq_dec value value) as [same | impossible].
  - reflexivity.
  - contradiction.
Qed.
