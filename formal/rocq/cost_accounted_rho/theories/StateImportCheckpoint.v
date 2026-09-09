From Stdlib Require Import List Arith Bool.
From CostAccountedRho Require Import StateImportClosure StateImportStorage StateImportCold
  StateImportCodec StateImportCursor StateImportTraversal StateImportWire StateImportEncodedCold
  StateImportHistoryObservations StateImportScan.

Import ListNotations.

Section CheckpointClosure.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).
Variable history_hash : list ImportRadixEdge -> nat.
Variable payload_hash : list nat -> nat.

Definition import_checkpoint_base_reference
    (old : ImportStore) (bases : list ImportReference) (reference : ImportReference) : Prop :=
  exists root, In root bases /\ import_reachable history_hash payload_hash old root reference.

Definition import_checkpoint_covered (old : ImportStore) (bases delta : list ImportReference)
    (reference : ImportReference) : Prop :=
  import_checkpoint_base_reference old bases reference \/ In reference delta.

Definition import_checkpoint_delta_cut (old new : ImportStore)
    (bases delta : list ImportReference) : Prop :=
  (forall reference, In reference delta -> exists children,
    import_checked_read history_hash payload_hash new reference = Some children /\
    Forall (import_checkpoint_covered old bases delta) children) /\
  (forall key kind, In (ImportColdRef key kind) delta -> exists values,
    import_consume_stored_cold Value decode_item new key kind = Some values).

Lemma import_checkpoint_coverage_grows : forall old bases delta added reference,
  import_checkpoint_covered old bases delta reference ->
  import_checkpoint_covered old bases (added :: delta) reference.
Proof.
  intros old bases delta added reference [Base|Present]; [left|right]; simpl; auto.
Qed.

Lemma import_checkpoint_base_read : forall old bases reference,
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash old) bases ->
  import_checkpoint_base_reference old bases reference ->
  exists children,
    import_checked_read history_hash payload_hash old reference = Some children /\
    Forall (import_checkpoint_base_reference old bases) children.
Proof.
  intros old bases reference Closed [root [Root Reached]].
  rewrite Forall_forall in Closed.
  destruct (Closed root Root) as [ClosedRoot _].
  destruct (ClosedRoot reference Reached) as [children Read].
  exists children. split; [exact Read|]. apply Forall_forall.
  intros child Present. exists root. split; [exact Root|].
  eapply import_reachable_child; eauto.
Qed.

Theorem import_checkpoint_delta_covers_new_reachability :
  forall old new bases delta root,
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash old) bases ->
  import_binding_extension old new ->
  import_checkpoint_delta_cut old new bases delta ->
  import_checkpoint_covered old bases delta root ->
  forall reference, import_reachable history_hash payload_hash new root reference ->
    import_checkpoint_covered old bases delta reference.
Proof.
  intros old new bases delta root Closed Bindings [Delta _] Root reference Reached.
  pose proof (import_binding_extension_preserves_checked_reads
    history_hash payload_hash old new Bindings) as Extension.
  induction Reached as [|parent child children Reached Covered Read Present].
  - exact Root.
  - destruct Covered as [Base|Added].
    + destruct (import_checkpoint_base_read old bases parent Closed Base)
        as [old_children [OldRead Children]].
      pose proof (Extension parent old_children OldRead) as NewRead.
      rewrite Read in NewRead. inversion NewRead; subst old_children.
      left. rewrite Forall_forall in Children. now apply Children.
    + destruct (Delta parent Added) as [new_children [NewRead Children]].
      rewrite Read in NewRead. inversion NewRead; subst new_children.
      rewrite Forall_forall in Children. now apply Children.
Qed.

Theorem import_checkpoint_delta_establishes_consumable_new_root :
  forall old new bases delta root,
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash old) bases ->
  import_binding_extension old new ->
  import_checkpoint_delta_cut old new bases delta ->
  import_checkpoint_covered old bases delta root ->
  import_consumable_closed_root Value decode_item history_hash payload_hash new root.
Proof.
  intros old new bases delta root Closed Bindings Cut Root.
  pose proof (import_checkpoint_delta_covers_new_reachability
    old new bases delta root Closed Bindings Cut Root) as Covered.
  destruct Cut as [Delta Consume]. split.
  - intros reference Reached. destruct (Covered reference Reached) as [Base|Added].
    + destruct (import_checkpoint_base_read old bases reference Closed Base)
        as [children [Read _]]. exists children.
      eapply import_binding_extension_preserves_checked_reads; eauto.
    + destruct (Delta reference Added) as [children [Read _]]. eauto.
  - intros key kind Reached. destruct (Covered _ Reached) as [[base [Member OldReached]]|Added].
    + rewrite Forall_forall in Closed.
      destruct (Closed base Member) as [_ ConsumeOld].
      destruct (ConsumeOld key kind OldReached) as [values Read]. exists values.
      eapply import_storage_binding_extension_preserves_typed_consumption; eauto.
    + now apply Consume.
Qed.

Inductive import_checkpoint_construction (old new : ImportStore)
    (bases : list ImportReference) : list ImportReference -> Prop :=
| import_checkpoint_construction_empty : import_checkpoint_construction old new bases []
| import_checkpoint_construction_history : forall delta key context children,
    import_checkpoint_construction old new bases delta ->
    import_checked_read history_hash payload_hash new (ImportHistoryRef key context) = Some children ->
    Forall (import_checkpoint_covered old bases delta) children ->
    import_checkpoint_construction old new bases (ImportHistoryRef key context :: delta)
| import_checkpoint_construction_cold : forall delta key kind values,
    import_checkpoint_construction old new bases delta ->
    import_checked_read history_hash payload_hash new (ImportColdRef key kind) = Some [] ->
    import_consume_stored_cold Value decode_item new key kind = Some values ->
    import_checkpoint_construction old new bases (ImportColdRef key kind :: delta).

Theorem import_checkpoint_construction_derives_delta_cut : forall old new bases delta,
  import_checkpoint_construction old new bases delta ->
  import_checkpoint_delta_cut old new bases delta.
Proof.
  intros old new bases delta Construction. induction Construction as
    [|delta key context children Construction [Delta Consume] Read Children
     |delta key kind values Construction [Delta Consume] Read Consumed].
  - split; intros; contradiction.
  - split.
    + intros reference [Same|Present].
      * subst reference. exists children. split; [exact Read|].
        eapply Forall_impl; [|exact Children]. intros child Covered.
        now apply import_checkpoint_coverage_grows.
      * destruct (Delta reference Present) as [next [NextRead NextChildren]].
        exists next. split; [exact NextRead|].
        eapply Forall_impl; [|exact NextChildren]. intros child Covered.
        now apply import_checkpoint_coverage_grows.
    + intros other expected [Impossible|Present]; [discriminate|now apply Consume].
  - split.
    + intros reference [Same|Present].
      * subst reference. exists []. split; [exact Read|constructor].
      * destruct (Delta reference Present) as [next [NextRead NextChildren]].
        exists next. split; [exact NextRead|].
        eapply Forall_impl; [|exact NextChildren]. intros child Covered.
        now apply import_checkpoint_coverage_grows.
    + intros other expected [Same|Present].
      * inversion Same; subst. eauto.
      * now apply Consume.
Qed.

Theorem import_checkpoint_construction_is_ready_for_publication :
  forall old new bases delta root,
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash old) bases ->
  import_binding_extension old new ->
  import_checkpoint_construction old new bases delta ->
  import_checkpoint_covered old bases delta root ->
  import_consumable_closed_root Value decode_item history_hash payload_hash new root.
Proof.
  intros old new bases delta root Closed Bindings Construction Root.
  eapply import_checkpoint_delta_establishes_consumable_new_root; eauto.
  now apply import_checkpoint_construction_derives_delta_cut.
Qed.

Theorem import_checkpoint_publication_survives_independent_batches :
  forall old committed later bases delta root,
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash old) bases ->
  import_batch_run old committed ->
  import_checkpoint_construction old committed bases delta ->
  import_checkpoint_covered old bases delta root ->
  import_batch_run committed later ->
  Forall (import_consumable_closed_root Value decode_item history_hash payload_hash later) (root :: bases).
Proof.
  intros old committed later bases delta root Closed Writes Construction Root Later.
  apply (import_arbitrary_batches_preserve_all_consumable_roots
    Value decode_item history_hash payload_hash committed later (root :: bases)); [|exact Later].
  destruct (import_arbitrary_batches_preserve_exact_bindings_without_global_agreement
    old committed Writes) as [_ Bindings]. constructor.
  - eapply import_checkpoint_construction_is_ready_for_publication; eauto.
  - eapply import_arbitrary_batches_preserve_all_consumable_roots; eauto.
Qed.

Theorem import_checkpoint_first_root_requires_no_persisted_base :
  forall old new delta root,
  import_binding_extension old new ->
  import_checkpoint_construction old new [] delta ->
  In root delta ->
  import_consumable_closed_root Value decode_item history_hash payload_hash new root.
Proof.
  intros old new delta root Bindings Construction Root.
  eapply import_checkpoint_construction_is_ready_for_publication with
    (old := old) (bases := []) (delta := delta).
  - constructor.
  - exact Bindings.
  - exact Construction.
  - right. exact Root.
Qed.

Theorem import_checkpoint_reuse_requires_the_original_context :
  forall old bases key context delta,
  ~ import_checkpoint_base_reference old bases (ImportHistoryRef key context) ->
  ~ In (ImportHistoryRef key context) delta ->
  ~ import_checkpoint_covered old bases delta (ImportHistoryRef key context).
Proof. intros old bases key context delta NotBase NotDelta [Base|Added]; contradiction. Qed.

End CheckpointClosure.

Theorem import_checkpoint_physical_commits_preserve_new_and_captured_roots :
  forall Value decode_item Hash old_history old_cold committed_history committed_cold
    later_history later_cold bases delta root,
  import_cursor_word_validb root = true ->
  Forall (import_consumable_closed_root Value decode_item
    (import_project_wire_hash Hash) (import_scan_payload_hash Hash)
    (import_physical_store_view old_history old_cold)) bases ->
  import_history_store_extension old_history committed_history ->
  import_encoded_run old_cold committed_cold ->
  import_checkpoint_construction Value decode_item
    (import_project_wire_hash Hash) (import_scan_payload_hash Hash)
    (import_physical_store_view old_history old_cold)
    (import_physical_store_view committed_history committed_cold) bases delta ->
  import_checkpoint_covered (import_project_wire_hash Hash) (import_scan_payload_hash Hash)
    (import_physical_store_view old_history old_cold) bases delta
    (ImportHistoryRef (import_key_to_nat root) []) ->
  import_history_store_extension committed_history later_history ->
  import_encoded_run committed_cold later_cold ->
  import_nat_to_key 32 (import_key_to_nat root) = root /\
  Forall (import_consumable_closed_root Value decode_item
    (import_project_wire_hash Hash) (import_scan_payload_hash Hash)
    (import_physical_store_view later_history later_cold))
    (ImportHistoryRef (import_key_to_nat root) [] :: bases).
Proof.
  intros Value decode_item Hash old_history old_cold committed_history committed_cold
    later_history later_cold bases delta root ValidRoot Closed History Cold Construction Root LaterHistory LaterCold.
  apply import_cursor_word_validity_characterization in ValidRoot.
  destruct ValidRoot as [RootWidth RootBytes]. split.
  { rewrite <- RootWidth. now apply import_key_conversion_is_reversible. }
  assert (First : import_binding_extension (import_physical_store_view old_history old_cold)
    (import_physical_store_view committed_history committed_cold)).
  {
    apply import_encoded_view_preserves_joint_history_and_cold_extensions; [|exact Cold].
    now apply import_physical_history_extension_preserves_the_decoded_projection.
  }
  assert (Later : import_binding_extension (import_physical_store_view committed_history committed_cold)
    (import_physical_store_view later_history later_cold)).
  {
    apply import_encoded_view_preserves_joint_history_and_cold_extensions; [|exact LaterCold].
    now apply import_physical_history_extension_preserves_the_decoded_projection.
  }
  constructor.
  - eapply import_binding_extension_preserves_consumable_closed_root; [exact Later|].
    eapply import_checkpoint_construction_is_ready_for_publication; eauto.
  - eapply Forall_impl; [|exact Closed]. intros base Usable.
    eapply import_binding_extension_preserves_consumable_closed_root; [|exact Usable].
    eapply import_binding_extension_transitive; eauto.
Qed.

Theorem import_checkpoint_compaction_preserves_absolute_reference :
  forall context parent_index parent_prefix child_index child_prefix target,
  import_edge_reference (context ++ parent_index :: parent_prefix)
    {| import_edge_index := child_index; import_edge_prefix := child_prefix;
       import_edge_target := target |} =
  import_edge_reference context
    {| import_edge_index := parent_index;
       import_edge_prefix := parent_prefix ++ child_index :: child_prefix;
       import_edge_target := target |}.
Proof.
  intros. unfold import_edge_reference. simpl. now rewrite <- app_assoc.
Qed.

Theorem import_checkpoint_split_preserves_absolute_reference :
  forall context index common branch suffix target,
  import_edge_reference context
    {| import_edge_index := index; import_edge_prefix := common ++ branch :: suffix;
       import_edge_target := target |} =
  import_edge_reference (context ++ index :: common)
    {| import_edge_index := branch; import_edge_prefix := suffix;
       import_edge_target := target |}.
Proof. intros. symmetry. apply import_checkpoint_compaction_preserves_absolute_reference. Qed.

Definition import_checkpoint_demo_old : ImportStore :=
  {| import_history := fun key => if Nat.eqb key 0 then Some [] else None;
     import_cold_raw := fun _ => None; import_cold_legacy := fun _ => None |}.

Definition import_checkpoint_demo_leaf : ImportLeaf :=
  {| import_leaf_kind := ImportJoins; import_leaf_payload := repeat 0 8 |}.

Definition import_checkpoint_demo_edges : list ImportRadixEdge :=
  [{| import_edge_index := 2; import_edge_prefix := []; import_edge_target := ImportRadixLeaf 8 |}].

Definition import_checkpoint_demo_new : ImportStore :=
  {| import_history := fun key => if Nat.eqb key 1 then Some import_checkpoint_demo_edges
       else import_history import_checkpoint_demo_old key;
     import_cold_raw := fun key => if Nat.eqb key 8 then Some import_checkpoint_demo_leaf else None;
     import_cold_legacy := fun _ => None |}.

Definition import_checkpoint_demo_decoder (_ : ImportLeafKind) (_ : list nat) : option unit := Some tt.

Definition import_checkpoint_demo_empty : ImportStore :=
  {| import_history := fun _ => None;
     import_cold_raw := fun _ => None; import_cold_legacy := fun _ => None |}.

Example import_checkpoint_constructed_first_root_is_consumable_without_an_old_row :
  import_consumable_closed_root (fun _ => unit) import_checkpoint_demo_decoder
    (@length ImportRadixEdge) (@length nat) import_checkpoint_demo_new (ImportHistoryRef 1 []).
Proof.
  eapply import_checkpoint_first_root_requires_no_persisted_base with
    (old := import_checkpoint_demo_empty)
    (delta := [ImportHistoryRef 1 []; ImportColdRef 8 ImportJoins]).
  - split; intros key value Read; discriminate.
  - eapply import_checkpoint_construction_history.
    + eapply import_checkpoint_construction_cold with (values := []).
      * constructor.
      * reflexivity.
      * vm_compute. reflexivity.
    + reflexivity.
    + constructor; [right; simpl; auto|constructor].
  - now left.
Qed.

Example import_checkpoint_new_cold_and_root_have_a_construction :
  import_checkpoint_construction (fun _ => unit) import_checkpoint_demo_decoder
    (@length ImportRadixEdge) (@length nat) import_checkpoint_demo_old import_checkpoint_demo_new
    [ImportHistoryRef 0 []] [ImportHistoryRef 1 []; ImportColdRef 8 ImportJoins].
Proof.
  eapply import_checkpoint_construction_history.
  - eapply import_checkpoint_construction_cold with (values := []).
    + constructor.
    + reflexivity.
    + vm_compute. reflexivity.
  - reflexivity.
  - constructor; [right; simpl; auto|constructor].
Qed.

Example import_checkpoint_an_uncommitted_cold_row_cannot_supply_a_construction :
  let missing := {| import_history := import_history import_checkpoint_demo_new;
    import_cold_raw := fun _ => None; import_cold_legacy := fun _ => None |} in
  import_checked_read (@length ImportRadixEdge) (@length nat) missing
    (ImportColdRef 8 ImportJoins) = None.
Proof. reflexivity. Qed.

Example import_checkpoint_wrong_kind_cannot_supply_a_construction :
  import_checked_read (@length ImportRadixEdge) (@length nat) import_checkpoint_demo_new
    (ImportColdRef 8 ImportData) = None.
Proof. reflexivity. Qed.

Example import_checkpoint_uncommitted_history_cannot_supply_a_construction :
  import_checked_read (@length ImportRadixEdge) (@length nat) import_checkpoint_demo_old
    (ImportHistoryRef 1 []) = None.
Proof. reflexivity. Qed.

Example import_checkpoint_compaction_keeps_kind_and_key :
  import_edge_reference [2; 7]
    {| import_edge_index := 9; import_edge_prefix := [4]; import_edge_target := ImportRadixLeaf 8 |} =
  import_edge_reference []
    {| import_edge_index := 2; import_edge_prefix := [7; 9; 4]; import_edge_target := ImportRadixLeaf 8 |}.
Proof. reflexivity. Qed.
