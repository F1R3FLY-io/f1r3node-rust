From Stdlib Require Import Bool List Arith Lia NArith FunctionalExtensionality.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Set Implicit Arguments.

(** Exact outer-PathMap operations and transactional publication for the
    selected two-level RSpace authority.

    Lists remain ghost observations of canonical PathMap cursors.  The
    definitions below specify exact endpoint behavior; they do not authorize
    a production list, flattening pass, or sort. *)
Module TwoLevelRSpaceOperations.

Lemma path_ltb_false_not_less : forall left right,
  path_ltb left right = false -> ~ path_lex_lt left right.
Proof.
  intros left right Hfalse Hless.
  apply path_ltb_spec in Hless. congruence.
Qed.

Lemma strict_order_from_distinct_not_less : forall left right,
  left <> right ->
  path_ltb left right = false ->
  path_lex_lt right left.
Proof.
  intros left right Hdistinct Hnot_less.
  destruct (path_lex_order_total left right) as [Heq | [Hless | Hgreater]].
  - contradiction.
  - exfalso. now apply (path_ltb_false_not_less Hnot_less).
  - exact Hgreater.
Qed.

Fixpoint upsert_channel_exact
    (selected : Path)
    (replacement : ChannelBag)
    (authority : TwoLevelAuthority) : TwoLevelAuthority :=
  match authority with
  | [] => [(selected, replacement)]
  | (channel, contents) :: tail =>
      if path_eq_dec selected channel
      then (channel, replacement) :: tail
      else if path_ltb selected channel
           then (selected, replacement) :: (channel, contents) :: tail
           else (channel, contents) ::
             upsert_channel_exact selected replacement tail
  end.

Theorem upsert_target_lookup_is_exact : forall authority selected replacement,
  lookup_channel selected
    (upsert_channel_exact selected replacement authority) = Some replacement.
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected replacement; cbn [upsert_channel_exact lookup_channel].
  - destruct (path_eq_dec selected selected); [reflexivity | contradiction].
  - destruct (path_eq_dec selected current) as [Heq | Hneq].
    + subst. cbn [lookup_channel].
      destruct (path_eq_dec current current); [reflexivity | contradiction].
    + destruct (path_ltb selected current) eqn:Hlt;
        cbn [lookup_channel].
      * destruct (path_eq_dec selected selected); [reflexivity | contradiction].
      * destruct (path_eq_dec selected current);
          [contradiction | exact (IH selected replacement)].
Qed.

Theorem upsert_preserves_every_other_exact_channel : forall
    authority selected replacement other,
  other <> selected ->
  lookup_channel other (upsert_channel_exact selected replacement authority) =
  lookup_channel other authority.
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected replacement other Hdifferent;
    cbn [upsert_channel_exact lookup_channel].
  - destruct (path_eq_dec other selected); [contradiction | reflexivity].
  - destruct (path_eq_dec selected current) as [Hselected | Hselected].
    + subst. cbn [lookup_channel].
      destruct (path_eq_dec other current); [contradiction | reflexivity].
    + destruct (path_ltb selected current) eqn:Hlt;
        cbn [lookup_channel].
      * destruct (path_eq_dec other selected); [contradiction | reflexivity].
      * destruct (path_eq_dec other current); [reflexivity |].
        apply IH. exact Hdifferent.
Qed.

Theorem upsert_makes_channel_loaded : forall authority selected replacement,
  loaded (upsert_channel_exact selected replacement authority) selected.
Proof.
  intros. exists replacement. apply upsert_target_lookup_is_exact.
Qed.

Lemma upsert_preserves_key_forall : forall
    (predicate : Path -> Prop) authority selected replacement,
  predicate selected ->
  Forall (fun entry => predicate (channel_path entry)) authority ->
  Forall
    (fun entry => predicate (channel_path entry))
    (upsert_channel_exact selected replacement authority).
Proof.
  intros predicate authority.
  induction authority as [|[current contents] tail IH];
    intros selected replacement Hselected Hforall;
    inversion Hforall; subst; cbn [upsert_channel_exact].
  - constructor; [exact Hselected | constructor].
  - destruct (path_eq_dec selected current).
    + constructor; assumption.
    + destruct (path_ltb selected current).
      * repeat constructor; assumption.
      * constructor; [assumption |].
        apply IH; assumption.
Qed.

Lemma selected_precedes_canonical_authority : forall selected authority,
  canonical_channels authority ->
  (forall entry, In entry authority ->
    path_lex_lt selected (channel_path entry)) ->
  Forall (fun entry => path_lex_lt selected (channel_path entry)) authority.
Proof.
  intros selected authority _ Hall.
  apply Forall_forall. intros entry Hin. now apply Hall.
Qed.

Theorem canonical_upsert_is_preserved : forall authority selected replacement,
  canonical_channels authority ->
  path_valid selected = true ->
  canonical_entries replacement ->
  canonical_channels (upsert_channel_exact selected replacement authority).
Proof.
  intros authority selected replacement Hcanonical.
  induction Hcanonical as
      [|current contents tail Hcurrent_valid Hcontents Hafter Htail IH];
    intros Hselected_valid Hreplacement; cbn [upsert_channel_exact].
  - constructor; [exact Hselected_valid | exact Hreplacement | constructor | constructor].
  - destruct (path_eq_dec selected current) as [Heq | Hneq].
    + subst. constructor; assumption.
    + destruct (path_ltb selected current) eqn:Hselected_before.
      * apply CanonicalChannelsCons.
        -- exact Hselected_valid.
        -- exact Hreplacement.
        -- constructor.
           ++ apply path_ltb_spec. exact Hselected_before.
           ++ eapply Forall_impl; [|exact Hafter].
              intros entry Hcurrent_before.
              eapply path_lex_lt_transitive.
              ** apply path_ltb_spec. exact Hselected_before.
              ** exact Hcurrent_before.
        -- apply CanonicalChannelsCons; assumption.
      * apply CanonicalChannelsCons.
        -- exact Hcurrent_valid.
        -- exact Hcontents.
        -- apply upsert_preserves_key_forall.
           ++ apply strict_order_from_distinct_not_less; assumption.
           ++ exact Hafter.
        -- apply IH; assumption.
Qed.

Lemma lookup_channel_some_implies_key_present : forall authority selected contents,
  lookup_channel selected authority = Some contents ->
  In selected (channel_paths authority).
Proof.
  induction authority as [|[current current_contents] tail IH];
    intros selected contents Hlookup; cbn in *; [discriminate |].
  destruct (path_eq_dec selected current).
  - subst. now left.
  - right. now apply IH with contents.
Qed.

Lemma upsert_loaded_length : forall authority selected replacement old,
  canonical_channels authority ->
  lookup_channel selected authority = Some old ->
  length (upsert_channel_exact selected replacement authority) = length authority.
Proof.
  intros authority selected replacement old Hcanonical.
  induction Hcanonical as
      [|current contents tail Hvalid Hcontents Hafter Htail IH];
    intro Hlookup; cbn in *; [discriminate |].
  destruct (path_eq_dec selected current) as [Heq | Hneq].
  - reflexivity.
  - destruct (path_ltb selected current) eqn:Hlt.
    + exfalso.
      pose proof
        (@lookup_channel_some_implies_key_present tail selected old Hlookup)
        as Hin.
      apply in_map_iff in Hin.
      destruct Hin as [entry [Heq_path Hin]].
      apply Forall_forall with (x := entry) in Hafter; [|exact Hin].
      rewrite Heq_path in Hafter.
      apply path_ltb_spec in Hlt.
      exact (path_lex_lt_irreflexive selected
        (@path_lex_lt_transitive selected current selected Hlt Hafter)).
    + simpl. f_equal. exact (IH Hlookup).
Qed.

Lemma upsert_unloaded_length : forall authority selected replacement,
  lookup_channel selected authority = None ->
  length (upsert_channel_exact selected replacement authority) =
    S (length authority).
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected replacement Hlookup; cbn in *; [reflexivity |].
  destruct (path_eq_dec selected current) as [Heq | Hneq].
  - discriminate.
  - destruct (path_ltb selected current); simpl; [reflexivity |].
    f_equal. now apply IH.
Qed.

Fixpoint delete_channel_exact
    (selected : Path)
    (authority : TwoLevelAuthority) : TwoLevelAuthority :=
  match authority with
  | [] => []
  | (channel, contents) :: tail =>
      if path_eq_dec selected channel
      then tail
      else (channel, contents) :: delete_channel_exact selected tail
  end.

Theorem delete_preserves_every_other_exact_channel : forall
    authority selected other,
  other <> selected ->
  lookup_channel other (delete_channel_exact selected authority) =
  lookup_channel other authority.
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected other Hdifferent; cbn [delete_channel_exact lookup_channel];
    [reflexivity |].
  destruct (path_eq_dec selected current) as [Hselected | Hselected].
  - subst. cbn [lookup_channel].
    destruct (path_eq_dec other current); [contradiction | reflexivity].
  - cbn [lookup_channel].
    destruct (path_eq_dec other current); [reflexivity |].
    apply IH. exact Hdifferent.
Qed.

Lemma lookup_channel_none_when_key_absent : forall authority selected,
  ~ In selected (channel_paths authority) ->
  lookup_channel selected authority = None.
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected Habsent; cbn [channel_paths channel_path lookup_channel];
    [reflexivity |].
  destruct (path_eq_dec selected current) as [Heq | Hneq].
  - subst. exfalso. apply Habsent. now left.
  - apply IH. intro Hin. apply Habsent. now right.
Qed.

Theorem canonical_delete_removes_target_lookup : forall authority selected,
  canonical_channels authority ->
  lookup_channel selected (delete_channel_exact selected authority) = None.
Proof.
  intros authority selected Hcanonical.
  pose proof (canonical_channel_paths_are_unique Hcanonical) as Hunique.
  induction authority as [|[current contents] tail IH];
    cbn [delete_channel_exact lookup_channel]; [reflexivity |].
  inversion Hunique as [|head paths Hnotin Htail_unique]; subst.
  inversion Hcanonical; subst.
  destruct (path_eq_dec selected current) as [Heq | Hneq].
  - subst. apply lookup_channel_none_when_key_absent. exact Hnotin.
  - cbn [lookup_channel].
    destruct (path_eq_dec selected current); [contradiction |].
    apply IH; assumption.
Qed.

Lemma delete_preserves_key_forall : forall
    (predicate : Path -> Prop) authority selected,
  Forall (fun entry => predicate (channel_path entry)) authority ->
  Forall
    (fun entry => predicate (channel_path entry))
    (delete_channel_exact selected authority).
Proof.
  intros predicate authority.
  induction authority as [|[current contents] tail IH];
    intros selected Hforall; inversion Hforall; subst;
    cbn [delete_channel_exact]; [constructor |].
  destruct (path_eq_dec selected current).
  - assumption.
  - constructor; [assumption |]. now apply IH.
Qed.

Theorem canonical_delete_is_preserved : forall authority selected,
  canonical_channels authority ->
  canonical_channels (delete_channel_exact selected authority).
Proof.
  intros authority selected Hcanonical.
  induction Hcanonical as
      [|current contents tail Hvalid Hcontents Hafter Htail IH];
    cbn [delete_channel_exact]; [constructor |].
  destruct (path_eq_dec selected current).
  - exact Htail.
  - apply CanonicalChannelsCons; try assumption.
    now apply delete_preserves_key_forall.
Qed.

Theorem unloaded_delete_is_idempotent : forall authority selected,
  lookup_channel selected authority = None ->
  delete_channel_exact selected authority = authority.
Proof.
  induction authority as [|[current contents] tail IH];
    intros selected Hlookup; cbn in *; [reflexivity |].
  destruct (path_eq_dec selected current) as [Heq | Hneq].
  - discriminate.
  - f_equal. now apply IH.
Qed.

Theorem loaded_delete_decrements_once : forall authority selected old,
  canonical_channels authority ->
  lookup_channel selected authority = Some old ->
  S (length (delete_channel_exact selected authority)) = length authority.
Proof.
  intros authority selected old Hcanonical.
  induction Hcanonical as
      [|current contents tail Hvalid Hcontents Hafter Htail IH];
    intro Hlookup; cbn in *; [discriminate |].
  destruct (path_eq_dec selected current) as [Heq | Hneq].
  - reflexivity.
  - simpl. f_equal. exact (IH Hlookup).
Qed.

Inductive prepared_channel_operation : Type :=
| PreparedUpsert : forall channel replacement,
    path_valid channel = true ->
    canonical_entries replacement ->
    prepared_channel_operation
| PreparedUnload : Path -> prepared_channel_operation.

Definition operation_channel (operation : prepared_channel_operation) : Path :=
  match operation with
  | @PreparedUpsert channel _ _ _ => channel
  | PreparedUnload channel => channel
  end.

Definition apply_prepared_channel_operation
    (authority : TwoLevelAuthority)
    (operation : prepared_channel_operation) : TwoLevelAuthority :=
  match operation with
  | @PreparedUpsert channel replacement _ _ =>
      upsert_channel_exact channel replacement authority
  | PreparedUnload channel => delete_channel_exact channel authority
  end.

Fixpoint apply_prepared_channel_operations
    (authority : TwoLevelAuthority)
    (operations : list prepared_channel_operation) : TwoLevelAuthority :=
  match operations with
  | [] => authority
  | operation :: tail =>
      apply_prepared_channel_operations
        (apply_prepared_channel_operation authority operation) tail
  end.

Definition touched_channels
    (operations : list prepared_channel_operation) : list Path :=
  map operation_channel operations.

Record PreparedChannelBatch : Type := PrepareChannelBatch {
  prepared_operations : list prepared_channel_operation;
  prepared_touched_channels_are_unique :
    NoDup (touched_channels prepared_operations)
}.

Theorem prepared_operations_preserve_canonicality : forall authority operations,
  canonical_channels authority ->
  canonical_channels (apply_prepared_channel_operations authority operations).
Proof.
  intros authority operations. revert authority.
  induction operations as [|operation tail IH]; intros authority Hcanonical;
    cbn [apply_prepared_channel_operations]; [exact Hcanonical |].
  apply IH. destruct operation as [channel replacement Hvalid Hreplacement | channel];
    cbn [apply_prepared_channel_operation].
  - now apply canonical_upsert_is_preserved.
  - now apply canonical_delete_is_preserved.
Qed.

Theorem untouched_channel_is_preserved_by_prepared_operations : forall
    authority operations channel,
  ~ In channel (touched_channels operations) ->
  lookup_channel channel
    (apply_prepared_channel_operations authority operations) =
  lookup_channel channel authority.
Proof.
  intros authority operations. revert authority.
  induction operations as [|operation tail IH]; intros authority channel Hnot_touched;
    cbn [apply_prepared_channel_operations touched_channels] in *;
    [reflexivity |].
  assert (Hhead : channel <> operation_channel operation).
  { intro Heq. apply Hnot_touched. now left. }
  assert (Htail : ~ In channel (map operation_channel tail)).
  { intro Hin. apply Hnot_touched. now right. }
  rewrite IH by exact Htail.
  destruct operation as [selected replacement Hvalid Hcanonical | selected];
    cbn [operation_channel apply_prepared_channel_operation] in *.
  - apply upsert_preserves_every_other_exact_channel. exact Hhead.
  - apply delete_preserves_every_other_exact_channel. exact Hhead.
Qed.

Section TransactionalPublication.

Context {Catalog ExternalSnapshots : Type}.

Definition CatalogAuthority : Type := Path -> option Catalog.

Definition invalidate_one_catalog
    (selected : Path) (catalogs : CatalogAuthority) : CatalogAuthority :=
  fun channel => if path_eq_dec channel selected then None else catalogs channel.

Fixpoint invalidate_touched_catalogs
    (operations : list prepared_channel_operation)
    (catalogs : CatalogAuthority) : CatalogAuthority :=
  match operations with
  | [] => catalogs
  | operation :: tail =>
      invalidate_touched_catalogs tail
        (invalidate_one_catalog (operation_channel operation) catalogs)
  end.

Lemma catalog_none_remains_none_after_invalidations : forall
    operations catalogs channel,
  catalogs channel = None ->
  invalidate_touched_catalogs operations catalogs channel = None.
Proof.
  induction operations as [|operation tail IH];
    intros catalogs channel Hnone; cbn [invalidate_touched_catalogs];
    [exact Hnone |].
  apply IH. unfold invalidate_one_catalog.
  destruct (path_eq_dec channel (operation_channel operation));
    [reflexivity | exact Hnone].
Qed.

Theorem touched_catalog_is_invalidated : forall operations catalogs channel,
  In channel (touched_channels operations) ->
  invalidate_touched_catalogs operations catalogs channel = None.
Proof.
  induction operations as [|operation tail IH];
    intros catalogs channel Hin; cbn [touched_channels] in Hin;
    [contradiction |].
  destruct Hin as [Heq | Hin].
  - subst. cbn [invalidate_touched_catalogs].
    apply catalog_none_remains_none_after_invalidations.
    unfold invalidate_one_catalog.
    destruct (path_eq_dec (operation_channel operation)
      (operation_channel operation)); [reflexivity | contradiction].
  - cbn [invalidate_touched_catalogs].
    apply IH. exact Hin.
Qed.

Theorem untouched_catalog_identity_is_preserved : forall
    operations catalogs channel,
  ~ In channel (touched_channels operations) ->
  invalidate_touched_catalogs operations catalogs channel = catalogs channel.
Proof.
  induction operations as [|operation tail IH];
    intros catalogs channel Hnot_touched;
    cbn [touched_channels invalidate_touched_catalogs] in *;
    [reflexivity |].
  assert (Hhead : channel <> operation_channel operation).
  { intro Heq. apply Hnot_touched. now left. }
  assert (Htail : ~ In channel (map operation_channel tail)).
  { intro Hin. apply Hnot_touched. now right. }
  rewrite IH by exact Htail.
  unfold invalidate_one_catalog.
  destruct (path_eq_dec channel (operation_channel operation)); congruence.
Qed.

Record transactional_store_state : Type := TransactionalStoreState {
  store_authority : TwoLevelAuthority;
  store_loaded_channel_count : nat;
  store_catalogs : CatalogAuthority;
  store_external_snapshots : ExternalSnapshots;
  store_publication_generation : nat
}.

Definition store_state_well_formed (state : transactional_store_state) : Prop :=
  canonical_channels (store_authority state) /\
  store_loaded_channel_count state = length (store_authority state).

Definition publish_prepared_channel_batch
    (before : transactional_store_state)
    (batch : PreparedChannelBatch) : transactional_store_state :=
  let operations := prepared_operations batch in
  let authority := apply_prepared_channel_operations
    (store_authority before) operations in
  TransactionalStoreState
    authority
    (length authority)
    (invalidate_touched_catalogs operations (store_catalogs before))
    (store_external_snapshots before)
    (S (store_publication_generation before)).

Inductive channel_batch_decision : Type :=
| ChannelBatchRefused
| ChannelBatchPublished.

Record channel_batch_outcome : Type := ChannelBatchOutcome {
  batch_decision : channel_batch_decision;
  batch_state : transactional_store_state;
  batch_publication_count : nat
}.

Inductive channel_batch_transition
    (before : transactional_store_state)
    : option PreparedChannelBatch -> channel_batch_outcome -> Prop :=
| RefuseChannelBatch :
    channel_batch_transition before None
      (ChannelBatchOutcome ChannelBatchRefused before 0)
| PublishChannelBatch : forall batch,
    channel_batch_transition before (Some batch)
      (ChannelBatchOutcome ChannelBatchPublished
        (publish_prepared_channel_batch before batch) 1).

Theorem refused_batch_is_strongly_atomic : forall before outcome,
  channel_batch_transition before None outcome ->
  batch_state outcome = before /\ batch_publication_count outcome = 0%nat.
Proof.
  intros before outcome Htransition.
  inversion Htransition; subst.
  split; reflexivity.
Qed.

Theorem accepted_batch_publishes_exactly_one_derived_authority : forall
    before batch outcome,
  channel_batch_transition before (Some batch) outcome ->
  batch_state outcome = publish_prepared_channel_batch before batch /\
  batch_publication_count outcome = 1%nat /\
  batch_decision outcome = ChannelBatchPublished.
Proof.
  intros before batch outcome Htransition.
  inversion Htransition; subst. repeat split; reflexivity.
Qed.

Theorem channel_batch_transition_is_deterministic : forall
    before proposal left right,
  channel_batch_transition before proposal left ->
  channel_batch_transition before proposal right ->
  left = right.
Proof.
  intros before proposal left right Hleft Hright.
  inversion Hleft; inversion Hright; subst; congruence.
Qed.

Theorem successful_publication_preserves_store_well_formedness : forall
    before batch,
  store_state_well_formed before ->
  store_state_well_formed (publish_prepared_channel_batch before batch).
Proof.
  intros before batch [Hcanonical Hcount].
  unfold store_state_well_formed, publish_prepared_channel_batch; cbn.
  split.
  - apply prepared_operations_preserve_canonicality. exact Hcanonical.
  - reflexivity.
Qed.

Theorem successful_publication_preserves_every_untouched_channel : forall
    before batch channel,
  ~ In channel (touched_channels (prepared_operations batch)) ->
  lookup_channel channel
      (store_authority (publish_prepared_channel_batch before batch)) =
  lookup_channel channel (store_authority before).
Proof.
  intros. unfold publish_prepared_channel_batch; cbn.
  now apply untouched_channel_is_preserved_by_prepared_operations.
Qed.

Theorem successful_publication_invalidates_every_touched_catalog : forall
    before batch channel,
  In channel (touched_channels (prepared_operations batch)) ->
  store_catalogs (publish_prepared_channel_batch before batch) channel = None.
Proof.
  intros. unfold publish_prepared_channel_batch; cbn.
  now apply touched_catalog_is_invalidated.
Qed.

Theorem successful_publication_preserves_untouched_catalog_identity : forall
    before batch channel,
  ~ In channel (touched_channels (prepared_operations batch)) ->
  store_catalogs (publish_prepared_channel_batch before batch) channel =
  store_catalogs before channel.
Proof.
  intros. unfold publish_prepared_channel_batch; cbn.
  now apply untouched_catalog_identity_is_preserved.
Qed.

Theorem successful_publication_preserves_issued_catalog_snapshots : forall
    before batch,
  store_external_snapshots (publish_prepared_channel_batch before batch) =
  store_external_snapshots before.
Proof. reflexivity. Qed.

Theorem successful_publication_advances_generation_once : forall before batch,
  store_publication_generation
      (publish_prepared_channel_batch before batch) =
  S (store_publication_generation before).
Proof. reflexivity. Qed.

End TransactionalPublication.

Print Assumptions canonical_upsert_is_preserved.
Print Assumptions canonical_delete_removes_target_lookup.
Print Assumptions prepared_operations_preserve_canonicality.
Print Assumptions untouched_channel_is_preserved_by_prepared_operations.
Print Assumptions refused_batch_is_strongly_atomic.
Print Assumptions successful_publication_preserves_store_well_formedness.
Print Assumptions successful_publication_invalidates_every_touched_catalog.

End TwoLevelRSpaceOperations.
