From Stdlib Require Import Arith Bool Lia Lists.List.
From FinalizedFloor Require Import FinalizationAtomicity.

Import ListNotations.

Section RecordAudit.
  Context {Head Record : Type}.
  Variable validate_record : Head -> Record -> option Head.

  Fixpoint audit_records (head : Head) (records : list Record) : option Head :=
    match records with
    | [] => Some head
    | record :: rest =>
        match validate_record head record with
        | None => None
        | Some next => audit_records next rest
        end
    end.

  Inductive validated_chain : Head -> list Record -> Head -> Prop :=
  | validated_nil : forall head, validated_chain head [] head
  | validated_cons : forall head record next rest last,
      validate_record head record = Some next ->
      validated_chain next rest last ->
      validated_chain head (record :: rest) last.

  Theorem audit_success_checks_every_record :
    forall records head last,
      audit_records head records = Some last <->
      validated_chain head records last.
  Proof.
    induction records as [|record rest IH]; intros head last; simpl.
    - split; intro H.
      + inversion H; constructor.
      + inversion H; reflexivity.
    - split; intro H.
      + destruct (validate_record head record) as [next|] eqn:Hstep;
          try discriminate.
        econstructor; [exact Hstep | now apply IH].
      + inversion H; subst.
        match goal with
        | Hstep : validate_record _ _ = Some _ |- _ => rewrite Hstep
        end.
        now apply IH.
  Qed.

  Theorem audit_page_partition_equivalence :
    forall prefix suffix head,
      audit_records head (prefix ++ suffix) =
      match audit_records head prefix with
      | None => None
      | Some next => audit_records next suffix
      end.
  Proof.
    induction prefix as [|record rest IH]; intros suffix head; simpl.
    - reflexivity.
    - destruct (validate_record head record); [apply IH | reflexivity].
  Qed.

  Fixpoint audit_pages (head : Head) (pages : list (list Record)) : option Head :=
    match pages with
    | [] => Some head
    | page :: rest =>
        match audit_records head page with
        | None => None
        | Some next => audit_pages next rest
        end
    end.

  Theorem arbitrary_page_partitions_preserve_verdict :
    forall pages head,
      audit_pages head pages = audit_records head (concat pages).
  Proof.
    induction pages as [|page rest IH]; intro head; simpl.
    - reflexivity.
    - rewrite audit_page_partition_equivalence.
      destruct (audit_records head page); [apply IH | reflexivity].
  Qed.

  Theorem failed_prefix_cannot_be_repaired_by_a_suffix :
    forall prefix suffix head,
      audit_records head prefix = None ->
      audit_records head (prefix ++ suffix) = None.
  Proof.
    intros prefix suffix head Hfailed.
    rewrite audit_page_partition_equivalence, Hfailed. reflexivity.
  Qed.

  Theorem captured_prefix_ignores_later_appends :
    forall prefix suffix head,
      audit_records head (firstn (length prefix) (prefix ++ suffix)) =
      audit_records head prefix.
  Proof.
    intros prefix suffix head.
    rewrite firstn_app, firstn_all, Nat.sub_diag. simpl.
    now rewrite app_nil_r.
  Qed.

  Theorem completed_pages_validate_exact_captured_head :
    forall pages initial captured,
      audit_pages initial pages = Some captured ->
      validated_chain initial (concat pages) captured.
  Proof.
    intros pages initial captured H.
    rewrite arbitrary_page_partitions_preserve_verdict in H.
    now apply audit_success_checks_every_record.
  Qed.
End RecordAudit.

Record integrity_progress := {
  audit_target : nat;
  audit_cursor : nat
}.

Definition begin_integrity_audit (target : nat) : integrity_progress :=
  {| audit_target := target; audit_cursor := 0 |}.

Definition integrity_page_end (target cursor budget : nat) : nat :=
  cursor + Nat.min budget (target - cursor).

Theorem integrity_page_is_bounded :
  forall target cursor budget,
    cursor <= target ->
    cursor <= integrity_page_end target cursor budget /\
    integrity_page_end target cursor budget <= target /\
    integrity_page_end target cursor budget - cursor <= budget.
Proof.
  intros target cursor budget Hbound. unfold integrity_page_end.
  pose proof (Nat.le_min_l budget (target - cursor)).
  pose proof (Nat.le_min_r budget (target - cursor)). lia.
Qed.

Theorem nonempty_page_makes_strict_progress :
  forall target cursor budget,
    cursor < target -> budget > 0 ->
    cursor < integrity_page_end target cursor budget.
Proof.
  intros target cursor budget Hremaining Hbudget.
  unfold integrity_page_end.
  destruct budget; [lia |].
  destruct (target - cursor) eqn:Hdifference; simpl; lia.
Qed.

Theorem restart_never_inherits_an_integrity_cursor :
  forall old_progress target,
    audit_cursor old_progress <= audit_target old_progress ->
    audit_cursor (begin_integrity_audit target) = 0 /\
    audit_target (begin_integrity_audit target) = target.
Proof.
  intros. split; reflexivity.
Qed.

Definition receipt_key := (nat * nat)%type.
Definition receipt_store := receipt_key -> bool.

Definition receipt_key_eq_dec :
  forall left right : receipt_key, {left = right} + {left <> right}.
Proof.
  decide equality; apply Nat.eq_dec.
Defined.

Definition delete_receipt_page
  (page : list receipt_key) (store : receipt_store) : receipt_store :=
  fun key => if in_dec receipt_key_eq_dec key page
             then false else store key.

Definition receipted_effect (cursor : nat) (store : receipt_store)
  (key : receipt_key) : bool :=
  (fst key <=? cursor) || store key.

Definition page_is_completed (cursor : nat) (page : list receipt_key) : Prop :=
  forall key, In key page -> fst key <= cursor.

Theorem receipt_deletion_retry_is_idempotent :
  forall page store key,
    delete_receipt_page page (delete_receipt_page page store) key =
      delete_receipt_page page store key.
Proof.
  intros page store key. unfold delete_receipt_page.
  destruct (in_dec receipt_key_eq_dec key page); reflexivity.
Qed.

Theorem completed_receipt_deletion_preserves_effect_status :
  forall cursor page store key,
    page_is_completed cursor page ->
    receipted_effect cursor (delete_receipt_page page store) key =
      receipted_effect cursor store key.
Proof.
  intros cursor page store key Hcompleted.
  unfold receipted_effect, delete_receipt_page.
  destruct (in_dec receipt_key_eq_dec key page) as [Hin | Hnotin].
  - specialize (Hcompleted key Hin).
    apply Nat.leb_le in Hcompleted. now rewrite Hcompleted.
  - reflexivity.
Qed.

Theorem receipt_deletion_preserves_incomplete_rounds :
  forall cursor page store key,
    page_is_completed cursor page -> fst key > cursor ->
    delete_receipt_page page store key = store key.
Proof.
  intros cursor page store key Hcompleted Hincomplete.
  unfold delete_receipt_page.
  destruct (in_dec receipt_key_eq_dec key page) as [Hin | Hnotin].
  - specialize (Hcompleted key Hin). lia.
  - reflexivity.
Qed.

Theorem receipt_deletion_pages_commute :
  forall left right store key,
    delete_receipt_page left (delete_receipt_page right store) key =
      delete_receipt_page right (delete_receipt_page left store) key.
Proof.
  intros left right store key. unfold delete_receipt_page.
  destruct (in_dec receipt_key_eq_dec key left),
           (in_dec receipt_key_eq_dec key right); reflexivity.
Qed.

Theorem completed_prefix_survives_bounded_cursor_step :
  forall cursor completed,
    completed_prefix cursor completed ->
    completed_prefix (finalization_effect_cursor_step cursor completed) completed.
Proof.
  intros cursor completed Hprefix.
  unfold finalization_effect_cursor_step.
  destruct (completed (S cursor)) eqn:Hnext.
  - now apply completed_prefix_extends_one_at_a_time.
  - exact Hprefix.
Qed.

Fixpoint advance_completed_page
  (budget target cursor : nat) (completed : nat -> bool) : nat :=
  match budget with
  | 0 => cursor
  | S rest =>
      if (cursor <? target) && completed (S cursor)
      then advance_completed_page rest target (S cursor) completed
      else cursor
  end.

Theorem completion_page_is_bounded :
  forall budget target cursor completed,
    cursor <= target ->
    cursor <= advance_completed_page budget target cursor completed /\
    advance_completed_page budget target cursor completed <= target /\
    advance_completed_page budget target cursor completed - cursor <= budget.
Proof.
  induction budget as [|budget IH]; intros target cursor completed Hbound; simpl.
  - repeat split; lia.
  - destruct ((cursor <? target) && completed (S cursor)) eqn:Hstep.
    + apply Bool.andb_true_iff in Hstep as [Hlt Hdone].
      apply Nat.ltb_lt in Hlt.
      specialize (IH target (S cursor) completed ltac:(lia)).
      destruct IH as [Hlower [Hupper Hwork]]. repeat split; lia.
    + repeat split; lia.
Qed.

Theorem completion_page_preserves_completed_prefix :
  forall budget target cursor completed,
    completed_prefix cursor completed ->
    completed_prefix (advance_completed_page budget target cursor completed) completed.
Proof.
  induction budget as [|budget IH]; intros target cursor completed Hprefix; simpl.
  - exact Hprefix.
  - destruct ((cursor <? target) && completed (S cursor)) eqn:Hstep.
    + apply Bool.andb_true_iff in Hstep as [Hlt Hdone].
      apply IH. now apply completed_prefix_extends_one_at_a_time.
    + exact Hprefix.
Qed.

Theorem completion_page_stops_at_a_gap_or_target :
  forall budget target cursor completed,
    (cursor <? target) && completed (S cursor) = false ->
    advance_completed_page budget target cursor completed = cursor.
Proof.
  intros [|budget] target cursor completed Hblocked; simpl.
  - reflexivity.
  - now rewrite Hblocked.
Qed.

Theorem completion_page_partition_equivalence :
  forall first second target cursor completed,
    advance_completed_page (first + second) target cursor completed =
    advance_completed_page second target
      (advance_completed_page first target cursor completed) completed.
Proof.
  induction first as [|first IH]; intros second target cursor completed; simpl.
  - reflexivity.
  - destruct ((cursor <? target) && completed (S cursor)) eqn:Hstep.
    + apply IH.
    + symmetry. now apply completion_page_stops_at_a_gap_or_target.
Qed.

Theorem completion_page_restarts_at_durable_progress :
  forall first second target cursor completed,
    let durable := advance_completed_page first target cursor completed in
    advance_completed_page second target durable completed =
    advance_completed_page (first + second) target cursor completed.
Proof.
  intros. symmetry. apply completion_page_partition_equivalence.
Qed.

Definition checked_span (remaining declared limit : nat) : option nat :=
  if (declared <=? remaining) && (declared <=? limit)
  then Some (remaining - declared)
  else None.

Theorem checked_span_sound :
  forall remaining declared limit rest,
    checked_span remaining declared limit = Some rest ->
    declared <= remaining /\ declared <= limit /\ rest + declared = remaining.
Proof.
  intros remaining declared limit rest H.
  unfold checked_span in H.
  destruct ((declared <=? remaining) && (declared <=? limit)) eqn:E;
    try discriminate.
  apply andb_true_iff in E as [Er El].
  apply Nat.leb_le in Er. apply Nat.leb_le in El.
  inversion H; subst. lia.
Qed.

Theorem checked_span_accepts_every_valid_span :
  forall remaining declared limit,
    declared <= remaining -> declared <= limit ->
    checked_span remaining declared limit = Some (remaining - declared).
Proof.
  intros remaining declared limit Hr Hl.
  unfold checked_span.
  apply Nat.leb_le in Hr. apply Nat.leb_le in Hl.
  now rewrite Hr, Hl.
Qed.

Theorem checked_span_rejects_before_consumption :
  forall remaining declared limit,
    remaining < declared \/ limit < declared ->
    checked_span remaining declared limit = None.
Proof.
  intros remaining declared limit H.
  unfold checked_span.
  destruct ((declared <=? remaining) && (declared <=? limit)) eqn:E;
    [|reflexivity].
  apply andb_true_iff in E as [Er El].
  apply Nat.leb_le in Er. apply Nat.leb_le in El. lia.
Qed.

Fixpoint checked_layout (remaining : nat) (fields : list (nat * nat)) : option nat :=
  match fields with
  | [] => Some remaining
  | (declared, limit) :: tail =>
      match checked_span remaining declared limit with
      | Some rest => checked_layout rest tail
      | None => None
      end
  end.

Definition layout_bytes (fields : list (nat * nat)) : nat :=
  fold_right (fun field total => fst field + total) 0 fields.

Theorem checked_layout_preserves_total_and_all_limits :
  forall fields remaining rest,
    checked_layout remaining fields = Some rest ->
    rest + layout_bytes fields = remaining /\
    Forall (fun field => fst field <= snd field) fields.
Proof.
  induction fields as [|[declared limit] tail IH]; intros remaining rest H.
  - inversion H; subst. split; [unfold layout_bytes; simpl; lia | constructor].
  - simpl in H.
    destruct (checked_span remaining declared limit) as [next|] eqn:E;
      try discriminate.
    apply checked_span_sound in E as [Hr [Hl Heq]].
    apply IH in H as [Htotal Hlimits].
    split.
    + unfold layout_bytes in *. simpl. lia.
    + constructor; [exact Hl | exact Hlimits].
Qed.

Theorem layout_checked_before_decode_bounds_consumed_bytes :
  forall fields remaining machine_max,
    remaining <= machine_max ->
    checked_layout remaining fields = Some 0 ->
    layout_bytes fields <= machine_max.
Proof.
  intros fields remaining machine_max Hmax H.
  apply checked_layout_preserves_total_and_all_limits in H as [Htotal _]. lia.
Qed.

Definition checked_collection (count limit width remaining : nat) : bool :=
  (count <=? limit) && (count * width <=? remaining).

Theorem checked_collection_bounds_iteration_and_size :
  forall count limit width remaining machine_max,
    remaining <= machine_max ->
    checked_collection count limit width remaining = true ->
    count <= limit /\ count * width <= remaining /\ count * width <= machine_max.
Proof.
  intros count limit width remaining machine_max Hmax H.
  unfold checked_collection in H.
  apply andb_true_iff in H as [Hcount Hsize].
  apply Nat.leb_le in Hcount. apply Nat.leb_le in Hsize. lia.
Qed.

Theorem checked_collection_accepts_all_encodable_counts :
  forall count limit width remaining,
    count <= limit -> count * width <= remaining ->
    checked_collection count limit width remaining = true.
Proof.
  intros count limit width remaining Hcount Hsize.
  unfold checked_collection.
  apply Nat.leb_le in Hcount. apply Nat.leb_le in Hsize.
  now rewrite Hcount, Hsize.
Qed.

Section RecoverySelection.
  Context {Row Payload : Type}.
  Variable valid_row : Row -> bool.
  Variable select_row : Row -> option Payload.

  Fixpoint selected_rows (rows : list Row) : list Payload :=
    match rows with
    | [] => []
    | row :: rest =>
        match select_row row with
        | None => selected_rows rest
        | Some payload => payload :: selected_rows rest
        end
    end.

  Fixpoint scan_rows (rows : list Row) (acc : list Payload) : option (list Payload) :=
    match rows with
    | [] => Some acc
    | row :: rest =>
        if valid_row row then
          scan_rows rest
            (match select_row row with None => acc | Some payload => payload :: acc end)
        else None
    end.

  Theorem streaming_selection_matches_full_validation :
    forall rows acc,
      scan_rows rows acc =
      if forallb valid_row rows
      then Some (rev (selected_rows rows) ++ acc)
      else None.
  Proof.
    induction rows as [|row rest IH]; intro acc; simpl.
    - reflexivity.
    - destruct (valid_row row); simpl; [|reflexivity].
      rewrite IH.
      destruct (select_row row); destruct (forallb valid_row rest); simpl;
        try reflexivity.
      now rewrite <- app_assoc.
  Qed.

  Theorem successful_selection_checks_unselected_rows :
    forall rows acc result,
      scan_rows rows acc = Some result ->
      Forall (fun row => valid_row row = true) rows.
  Proof.
    intros rows acc result H.
    rewrite streaming_selection_matches_full_validation in H.
    destruct (forallb valid_row rows) eqn:E; [|discriminate].
    apply Forall_forall. now apply forallb_forall.
  Qed.

  Theorem streaming_selection_partition_equivalence :
    forall prefix suffix acc,
      scan_rows (prefix ++ suffix) acc =
      match scan_rows prefix acc with
      | None => None
      | Some next => scan_rows suffix next
      end.
  Proof.
    induction prefix as [|row rest IH]; intros suffix acc; simpl.
    - reflexivity.
    - destruct (valid_row row); [apply IH | reflexivity].
  Qed.

  Theorem validated_unselected_row_does_not_change_output :
    forall prefix row suffix acc,
      valid_row row = true -> select_row row = None ->
      scan_rows (prefix ++ row :: suffix) acc = scan_rows (prefix ++ suffix) acc.
  Proof.
    intros prefix row suffix acc Hv Hs.
    rewrite !streaming_selection_partition_equivalence.
    destruct (scan_rows prefix acc); [|reflexivity].
    simpl. now rewrite Hv, Hs.
  Qed.

  Theorem selected_state_does_not_exceed_snapshot_rows :
    forall rows, length (selected_rows rows) <= length rows.
  Proof.
    induction rows as [|row rest IH]; simpl; [lia |].
    destruct (select_row row); simpl; lia.
  Qed.
End RecoverySelection.

Example unselected_corrupt_row_is_not_ignored :
  @scan_rows bool nat (fun row => row) (fun _ => None) [false] [] = None.
Proof. reflexivity. Qed.

Section SparseTransaction.
  Context {Key Value Operation : Type}.
  Variable key_eq_dec : forall x y : Key, {x = y} + {x <> y}.
  Variable evaluate : Operation -> option Value -> option (option Value).

  Definition transaction_store := Key -> option Value.
  Definition transaction_overlay := list (Key * option Value).

  Fixpoint overlay_read (base : transaction_store) (overlay : transaction_overlay)
      (key : Key) : option Value :=
    match overlay with
    | [] => base key
    | (changed, value) :: rest =>
        if key_eq_dec key changed then value else overlay_read base rest key
    end.

  Definition store_write (store : transaction_store) (key : Key) (value : option Value)
      : transaction_store :=
    fun query => if key_eq_dec query key then value else store query.

  Fixpoint dense_transaction (changes : list (Key * Operation))
      (store : transaction_store) : option transaction_store :=
    match changes with
    | [] => Some store
    | (key, operation) :: rest =>
        match evaluate operation (store key) with
        | None => None
        | Some value => dense_transaction rest (store_write store key value)
        end
    end.

  Fixpoint sparse_transaction (changes : list (Key * Operation))
      (base : transaction_store) (overlay : transaction_overlay)
      : option transaction_overlay :=
    match changes with
    | [] => Some overlay
    | (key, operation) :: rest =>
        match evaluate operation (overlay_read base overlay key) with
        | None => None
        | Some value => sparse_transaction rest base ((key, value) :: overlay)
        end
    end.

  Definition transaction_results_agree (base : transaction_store)
      (dense : option transaction_store) (sparse : option transaction_overlay) : Prop :=
    match dense, sparse with
    | None, None => True
    | Some store, Some overlay => forall key, store key = overlay_read base overlay key
    | _, _ => False
    end.

  Theorem sparse_staging_preserves_sequential_transaction_results :
    forall changes base store overlay,
      (forall key, store key = overlay_read base overlay key) ->
      transaction_results_agree base
        (dense_transaction changes store) (sparse_transaction changes base overlay).
  Proof.
    induction changes as [|[key operation] rest IH]; intros base store overlay Heq; simpl.
    - exact Heq.
    - rewrite (Heq key).
      destruct (evaluate operation (overlay_read base overlay key)); simpl; [|exact I].
      apply IH. intro query. unfold store_write. simpl.
      destruct (key_eq_dec query key); [reflexivity | apply Heq].
  Qed.

  Theorem sparse_staging_retains_only_touched_keys :
    forall changes base overlay result,
      sparse_transaction changes base overlay = Some result ->
      forall key value, In (key, value) result ->
      In key (map fst changes) \/ In key (map fst overlay).
  Proof.
    induction changes as [|[key operation] rest IH]; intros base overlay result H query value Hin.
    - simpl in H. inversion H; subst. right.
      change query with (fst (query, value)). now apply in_map.
    - simpl in H.
      destruct (evaluate operation (overlay_read base overlay key)) as [next|] eqn:E;
        try discriminate.
      specialize (IH base ((key, next) :: overlay) result H query value Hin).
      simpl in *. tauto.
  Qed.

  Theorem sparse_staging_size_depends_on_mutations_not_store_size :
    forall changes base overlay result,
      sparse_transaction changes base overlay = Some result ->
      length result = length changes + length overlay.
  Proof.
    induction changes as [|[key operation] rest IH]; intros base overlay result H.
    - simpl in H. inversion H; subst. reflexivity.
    - simpl in H.
      destruct (evaluate operation (overlay_read base overlay key)) as [next|];
        try discriminate.
      apply IH in H. simpl in *. lia.
  Qed.

  Definition publish_sparse_transaction (changes : list (Key * Operation))
      (base : transaction_store) : transaction_store :=
    match sparse_transaction changes base [] with
    | None => base
    | Some overlay => overlay_read base overlay
    end.

  Theorem failed_sparse_transaction_publishes_no_changes :
    forall changes base,
      sparse_transaction changes base [] = None ->
      forall key, publish_sparse_transaction changes base key = base key.
  Proof.
    intros changes base H key. unfold publish_sparse_transaction. now rewrite H.
  Qed.
End SparseTransaction.

Section CompletionObservation.
  Definition logical_completion (revision cursor : nat) (receipt : bool) : bool :=
    (revision <=? cursor) || receipt.

  Definition completion_query (revision first_cursor last_cursor : nat)
      (receipt : bool) : bool :=
    if revision <=? first_cursor then true
    else if receipt then true else revision <=? last_cursor.

  Theorem completion_query_is_linearizable :
    forall revision first_cursor receipt_cursor last_cursor first_receipt receipt last_receipt,
      first_cursor <= receipt_cursor -> receipt_cursor <= last_cursor ->
      let answer := completion_query revision first_cursor last_cursor receipt in
      answer = logical_completion revision first_cursor first_receipt \/
      answer = logical_completion revision receipt_cursor receipt \/
      answer = logical_completion revision last_cursor last_receipt.
  Proof.
    intros revision first_cursor receipt_cursor last_cursor first_receipt receipt last_receipt H12 H23.
    unfold completion_query, logical_completion.
    destruct (revision <=? first_cursor) eqn:Hfirst.
    - left. reflexivity.
    - destruct receipt.
      + right. left. destruct (revision <=? receipt_cursor); reflexivity.
      + destruct (revision <=? last_cursor) eqn:Hlast.
        * right. right. reflexivity.
        * right. left. apply Nat.leb_gt in Hlast.
          assert (revision <=? receipt_cursor = false) as Hreceipt by (apply Nat.leb_gt; lia).
          now rewrite Hreceipt.
  Qed.

  Theorem completion_query_cannot_lose_a_completed_receipt :
    forall revision first_cursor receipt_cursor last_cursor receipt,
      receipt_cursor <= last_cursor ->
      logical_completion revision receipt_cursor receipt = true ->
      completion_query revision first_cursor last_cursor receipt = true.
  Proof.
    intros revision first_cursor receipt_cursor last_cursor receipt Hcursor Hcomplete.
    unfold logical_completion in Hcomplete. unfold completion_query.
    destruct (revision <=? first_cursor); [reflexivity |].
    destruct receipt; [reflexivity |].
    rewrite Bool.orb_false_r in Hcomplete. apply Nat.leb_le in Hcomplete.
    apply Nat.leb_le. lia.
  Qed.

  Example missing_cursor_recheck_loses_completion :
    (if 1 <=? 0 then true else false) = false /\
    logical_completion 1 1 false = true /\
    completion_query 1 0 1 false = true.
  Proof. repeat split; reflexivity. Qed.
End CompletionObservation.

Section ReadOnlyObserverErasure.
  Context {State Event : Type}.
  Variable mutation : State -> Event -> State -> Prop.
  Variable observe : State -> bool.

  Inductive mutation_trace : State -> list Event -> State -> Prop :=
  | MutationsDone : forall state, mutation_trace state [] state
  | MutationThen : forall before event middle rest after,
      mutation before event middle -> mutation_trace middle rest after ->
      mutation_trace before (event :: rest) after.

  Inductive observed_trace : State -> list (Event + bool) -> State -> Prop :=
  | ObservationsDone : forall state, observed_trace state [] state
  | MutationObserved : forall before event middle rest after,
      mutation before event middle -> observed_trace middle rest after ->
      observed_trace before (inl event :: rest) after
  | ReadObserved : forall before rest after,
      observed_trace before rest after ->
      observed_trace before (inr (observe before) :: rest) after.

  Fixpoint erase_reads (trace : list (Event + bool)) : list Event :=
    match trace with
    | [] => []
    | inl event :: rest => event :: erase_reads rest
    | inr _ :: rest => erase_reads rest
    end.

  Theorem read_erasure_preserves_the_complete_mutation_trace :
    forall before trace after,
      observed_trace before trace after ->
      mutation_trace before (erase_reads trace) after.
  Proof.
    intros before trace after Htrace. induction Htrace; simpl.
    - constructor.
    - econstructor; eauto.
    - exact IHHtrace.
  Qed.

  Theorem mutation_traces_embed_without_observers :
    forall before trace after,
      mutation_trace before trace after ->
      observed_trace before (map (@inl Event bool) trace) after.
  Proof.
    intros before trace after Htrace. induction Htrace; simpl.
    - constructor.
    - econstructor; eauto.
  Qed.

  Lemma observed_traces_compose :
    forall before prefix middle suffix after,
      observed_trace before prefix middle ->
      observed_trace middle suffix after ->
      observed_trace before (prefix ++ suffix) after.
  Proof.
    intros before prefix middle suffix after Hprefix Hsuffix.
    induction Hprefix; simpl.
    - exact Hsuffix.
    - econstructor; eauto.
    - constructor. now apply IHHprefix.
  Qed.

  Theorem observers_can_be_reinserted_at_a_quiescent_boundary :
    forall before prefix middle suffix after,
      mutation_trace before prefix middle ->
      mutation_trace middle suffix after ->
      observed_trace before
        (map (@inl Event bool) prefix ++ inr (observe middle) :: map (@inl Event bool) suffix)
        after.
  Proof.
    intros before prefix middle suffix after Hprefix Hsuffix.
    eapply observed_traces_compose with (middle := middle).
    - apply mutation_traces_embed_without_observers. exact Hprefix.
    - constructor. apply mutation_traces_embed_without_observers. exact Hsuffix.
  Qed.

  Theorem invariant_observers_return_only_the_proven_answer :
    forall (invariant : State -> Prop) answer,
      (forall before event after,
          invariant before -> mutation before event after -> invariant after) ->
      (forall state, invariant state -> observe state = answer) ->
      forall before trace after,
        observed_trace before trace after -> invariant before ->
        Forall (fun action => match action with inl _ => True | inr value => value = answer end) trace.
  Proof.
    intros invariant answer Hstep Hanswer before trace after Htrace.
    induction Htrace; intro Hbefore.
    - constructor.
    - constructor; [exact I |]. apply IHHtrace. eapply Hstep; eauto.
    - constructor; [now apply Hanswer | now apply IHHtrace].
  Qed.

  Theorem mutation_invariants_hold_after_observer_insertion :
    forall (invariant : State -> Prop),
      (forall before event after,
          invariant before -> mutation before event after -> invariant after) ->
      forall before trace after,
        observed_trace before trace after -> invariant before -> invariant after.
  Proof.
    intros invariant Hstep before trace after Htrace.
    induction Htrace; intro Hbefore.
    - exact Hbefore.
    - apply IHHtrace. eapply Hstep; eauto.
    - now apply IHHtrace.
  Qed.
End ReadOnlyObserverErasure.

Theorem covered_cursor_short_circuits_completion_observation :
  forall revision first_cursor last_cursor receipt,
    revision <= first_cursor ->
    completion_query revision first_cursor last_cursor receipt = true.
Proof.
  intros revision first_cursor last_cursor receipt Hcovered.
  unfold completion_query.
  assert (revision <=? first_cursor = true) as H by (now apply Nat.leb_le).
  now rewrite H.
Qed.
