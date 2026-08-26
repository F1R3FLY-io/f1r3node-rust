From Stdlib Require Import Arith Bool Lia Lists.List.

Import ListNotations.

Record durable_ledger := {
  ledger_head : nat;
  committed_round : nat -> bool
}.

Definition finalization_compare_append
  (expected candidate : nat)
  (ledger : durable_ledger) : option durable_ledger :=
  if Nat.eqb expected (ledger_head ledger) &&
     Nat.eqb candidate (S (ledger_head ledger))
  then
    Some {| ledger_head := candidate;
            committed_round := fun round =>
              Nat.eqb round candidate || committed_round ledger round |}
  else None.

Definition finalization_effect_set := nat -> bool.

Definition finalization_apply_effect
  (round : nat)
  (effects : finalization_effect_set) : finalization_effect_set :=
  fun candidate => Nat.eqb candidate round || effects candidate.

Definition finalization_publish_revision (candidate current : nat) : nat :=
  Nat.max candidate current.

Definition finalization_projection_step (head cursor : nat) : nat :=
  if cursor <? head then S cursor else cursor.

Definition finalization_effect_cursor_step
  (cursor : nat)
  (completed : nat -> bool) : nat :=
  if completed (S cursor) then S cursor else cursor.

Definition finalization_compaction_step
  (effects_cursor compaction_cursor : nat) : option nat :=
  if compaction_cursor <=? effects_cursor then Some effects_cursor else None.

Definition completed_prefix (cursor : nat) (completed : nat -> bool) : Prop :=
  forall round, round <= cursor -> completed round = true.

Record finalization_recovery_cursors := {
  projection_cursor : nat;
  effect_cursor : nat;
  effect_compaction_cursor : nat
}.

Definition restart_finalization_cursors
  (cursors : finalization_recovery_cursors) : finalization_recovery_cursors :=
  cursors.

Record finalization_node := {
  durable_state : durable_ledger;
  published_revision : nat
}.

Definition restart_finalization_node (node : finalization_node) : finalization_node :=
  {| durable_state := durable_state node;
     published_revision := ledger_head (durable_state node) |}.

Theorem successful_compare_append_is_linearization_point :
  forall ledger,
    finalization_compare_append
      (ledger_head ledger) (S (ledger_head ledger)) ledger =
    Some {| ledger_head := S (ledger_head ledger);
            committed_round := fun round =>
              Nat.eqb round (S (ledger_head ledger)) ||
              committed_round ledger round |}.
Proof.
  intros ledger. unfold finalization_compare_append.
  rewrite Nat.eqb_refl, Nat.eqb_refl. reflexivity.
Qed.

Theorem committed_head_has_its_round_record :
  forall ledger next,
    finalization_compare_append
      (ledger_head ledger) (S (ledger_head ledger)) ledger = Some next ->
    committed_round next (ledger_head next) = true.
Proof.
  intros ledger next Happ.
  rewrite successful_compare_append_is_linearization_point in Happ.
  inversion Happ. simpl. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem stale_append_is_observationally_inert :
  forall ledger expected candidate,
    expected <> ledger_head ledger ->
    finalization_compare_append expected candidate ledger = None.
Proof.
  intros ledger expected candidate Hstale.
  unfold finalization_compare_append.
  apply Nat.eqb_neq in Hstale. rewrite Hstale. reflexivity.
Qed.

Theorem one_successor_wins_same_head_race :
  forall ledger first,
    finalization_compare_append
      (ledger_head ledger) (S (ledger_head ledger)) ledger = Some first ->
    finalization_compare_append
      (ledger_head ledger) (S (ledger_head ledger)) first = None.
Proof.
  intros ledger first Hfirst.
  rewrite successful_compare_append_is_linearization_point in Hfirst.
  inversion Hfirst. subst first. clear Hfirst.
  apply stale_append_is_observationally_inert. simpl. lia.
Qed.

Theorem effect_retry_is_idempotent :
  forall round effects candidate,
    finalization_apply_effect round
      (finalization_apply_effect round effects) candidate =
    finalization_apply_effect round effects candidate.
Proof.
  intros round effects candidate.
  unfold finalization_apply_effect.
  destruct (Nat.eqb candidate round); reflexivity.
Qed.

Theorem independent_effects_commute :
  forall left right effects candidate,
    finalization_apply_effect left
      (finalization_apply_effect right effects) candidate =
    finalization_apply_effect right
      (finalization_apply_effect left effects) candidate.
Proof.
  intros left right effects candidate.
  unfold finalization_apply_effect.
  destruct (Nat.eqb candidate left), (Nat.eqb candidate right); reflexivity.
Qed.

Theorem monotonic_publication_never_regresses :
  forall candidate current,
    current <= finalization_publish_revision candidate current.
Proof.
  intros candidate current. unfold finalization_publish_revision. lia.
Qed.

Theorem publication_order_is_irrelevant :
  forall left right current,
    finalization_publish_revision left
      (finalization_publish_revision right current) =
    finalization_publish_revision right
      (finalization_publish_revision left current).
Proof.
  intros left right current. unfold finalization_publish_revision. lia.
Qed.

Theorem restart_preserves_durable_head_and_republishes_it :
  forall node,
    durable_state (restart_finalization_node node) = durable_state node /\
    published_revision (restart_finalization_node node) =
      ledger_head (durable_state node).
Proof.
  intros node. split; reflexivity.
Qed.

Theorem compare_append_commits_recorded_successor :
  forall ledger,
    exists next,
      finalization_compare_append
        (ledger_head ledger) (S (ledger_head ledger)) ledger = Some next /\
      committed_round next (ledger_head next) = true.
Proof.
  intros ledger.
  exists {| ledger_head := S (ledger_head ledger);
            committed_round := fun round =>
              Nat.eqb round (S (ledger_head ledger)) ||
              committed_round ledger round |}.
  split.
  - apply successful_compare_append_is_linearization_point.
  - simpl. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem projection_step_is_bounded_by_head :
  forall head cursor,
    cursor <= head ->
    finalization_projection_step head cursor <= head.
Proof.
  intros head cursor Hbounded.
  unfold finalization_projection_step.
  destruct (cursor <? head) eqn:Hlt.
  - apply Nat.ltb_lt in Hlt. lia.
  - exact Hbounded.
Qed.

Theorem effect_cursor_step_never_skips_a_revision :
  forall cursor completed,
    finalization_effect_cursor_step cursor completed <= S cursor.
Proof.
  intros cursor completed.
  unfold finalization_effect_cursor_step.
  destruct (completed (S cursor)); lia.
Qed.

Theorem effect_cursor_advance_requires_completed_successor :
  forall cursor completed,
    finalization_effect_cursor_step cursor completed = S cursor ->
    completed (S cursor) = true.
Proof.
  intros cursor completed Hstep.
  unfold finalization_effect_cursor_step in Hstep.
  destruct (completed (S cursor)) eqn:Hcompleted.
  - reflexivity.
  - lia.
Qed.

Theorem completed_prefix_extends_one_at_a_time :
  forall cursor completed,
    completed_prefix cursor completed ->
    completed (S cursor) = true ->
    completed_prefix (S cursor) completed.
Proof.
  intros cursor completed Hprefix Hnext round Hround.
  destruct (Nat.eq_dec round (S cursor)) as [Heq | Hneq].
  - subst round. exact Hnext.
  - apply Hprefix. lia.
Qed.

Theorem restart_preserves_recovery_cursors :
  forall cursors,
    restart_finalization_cursors cursors = cursors.
Proof.
  reflexivity.
Qed.

Theorem successful_compaction_never_exceeds_completed_effects :
  forall effects_cursor compaction_cursor next,
    finalization_compaction_step effects_cursor compaction_cursor = Some next ->
    next <= effects_cursor.
Proof.
  intros effects_cursor compaction_cursor next Hstep.
  unfold finalization_compaction_step in Hstep.
  destruct (compaction_cursor <=? effects_cursor) eqn:Hbounded.
  - inversion Hstep. lia.
  - discriminate.
Qed.

Theorem finalization_recovery_contract :
  (forall head cursor,
    cursor <= head ->
    finalization_projection_step head cursor <= head)
  /\
  (forall cursor completed,
    finalization_effect_cursor_step cursor completed <= S cursor)
  /\
  (forall cursor completed,
    completed_prefix cursor completed ->
    completed (S cursor) = true ->
    completed_prefix (S cursor) completed)
  /\
  (forall effects_cursor compaction_cursor next,
    finalization_compaction_step effects_cursor compaction_cursor = Some next ->
    next <= effects_cursor)
  /\
  (forall cursors,
    restart_finalization_cursors cursors = cursors).
Proof.
  exact (conj projection_step_is_bounded_by_head
    (conj effect_cursor_step_never_skips_a_revision
      (conj completed_prefix_extends_one_at_a_time
        (conj successful_compaction_never_exceeds_completed_effects
          restart_preserves_recovery_cursors)))).
Qed.

Theorem finalization_atomicity_contract :
  (forall ledger,
    exists next,
      finalization_compare_append
        (ledger_head ledger) (S (ledger_head ledger)) ledger = Some next /\
      committed_round next (ledger_head next) = true)
  /\
  (forall ledger expected candidate,
    expected <> ledger_head ledger ->
    finalization_compare_append expected candidate ledger = None)
  /\
  (forall round effects candidate,
    finalization_apply_effect round
      (finalization_apply_effect round effects) candidate =
    finalization_apply_effect round effects candidate)
  /\
  (forall left right effects candidate,
    finalization_apply_effect left
      (finalization_apply_effect right effects) candidate =
    finalization_apply_effect right
      (finalization_apply_effect left effects) candidate)
  /\
  (forall candidate current,
    current <= finalization_publish_revision candidate current)
  /\
  (forall node,
    durable_state (restart_finalization_node node) = durable_state node /\
    published_revision (restart_finalization_node node) =
      ledger_head (durable_state node)).
Proof.
  exact (conj compare_append_commits_recorded_successor
    (conj stale_append_is_observationally_inert
      (conj effect_retry_is_idempotent
        (conj independent_effects_commute
          (conj monotonic_publication_never_regresses
            restart_preserves_durable_head_and_republishes_it))))).
Qed.

Inductive finalization_worker_exit : Type :=
| FinalizationWorkerSucceeded
| FinalizationWorkerFailed.

Definition worker_completed_after
  (exit : finalization_worker_exit)
  (completed coverage : nat) : nat :=
  match exit with
  | FinalizationWorkerSucceeded => Nat.max completed coverage
  | FinalizationWorkerFailed => completed
  end.

Definition worker_succeeded_after
  (exit : finalization_worker_exit)
  (succeeded coverage : nat) : nat :=
  match exit with
  | FinalizationWorkerSucceeded => Nat.max succeeded coverage
  | FinalizationWorkerFailed => succeeded
  end.

Definition worker_retry_required
  (exit : finalization_worker_exit)
  (completed coverage : nat) : bool :=
  match exit with
  | FinalizationWorkerSucceeded => false
  | FinalizationWorkerFailed => completed <? coverage
  end.

Theorem failed_worker_does_not_complete_coverage :
  forall completed coverage,
    worker_completed_after FinalizationWorkerFailed completed coverage = completed.
Proof. reflexivity. Qed.

Theorem failed_worker_does_not_certify_success :
  forall succeeded coverage,
    worker_succeeded_after FinalizationWorkerFailed succeeded coverage = succeeded.
Proof. reflexivity. Qed.

Theorem uncovered_failed_worker_requires_retry :
  forall completed coverage,
    completed < coverage ->
    worker_retry_required FinalizationWorkerFailed completed coverage = true.
Proof.
  intros completed coverage Huncovered.
  unfold worker_retry_required.
  apply Nat.ltb_lt. exact Huncovered.
Qed.

Theorem successful_worker_completion_is_certified :
  forall completed coverage,
    worker_completed_after FinalizationWorkerSucceeded completed coverage =
    worker_succeeded_after FinalizationWorkerSucceeded completed coverage.
Proof. reflexivity. Qed.

Theorem newer_success_subsumes_older_retry :
  forall completed older newer,
    older <= newer ->
    older <= worker_completed_after FinalizationWorkerSucceeded completed newer.
Proof.
  intros completed older newer Hordered.
  unfold worker_completed_after.
  eapply Nat.le_trans; [exact Hordered | apply Nat.le_max_r].
Qed.

Theorem finalization_worker_retry_contract :
  (forall completed coverage,
    worker_completed_after FinalizationWorkerFailed completed coverage = completed)
  /\
  (forall succeeded coverage,
    worker_succeeded_after FinalizationWorkerFailed succeeded coverage = succeeded)
  /\
  (forall completed coverage,
    completed < coverage ->
    worker_retry_required FinalizationWorkerFailed completed coverage = true)
  /\
  (forall completed coverage,
    worker_completed_after FinalizationWorkerSucceeded completed coverage =
    worker_succeeded_after FinalizationWorkerSucceeded completed coverage)
  /\
  (forall completed older newer,
    older <= newer ->
    older <= worker_completed_after FinalizationWorkerSucceeded completed newer).
Proof.
  exact (conj failed_worker_does_not_complete_coverage
    (conj failed_worker_does_not_certify_success
      (conj uncovered_failed_worker_requires_retry
        (conj successful_worker_completion_is_certified
          newer_success_subsumes_older_retry)))).
Qed.

Record rooted_finalization_store := {
  stored_genesis_anchor : option nat;
  stored_finalization_head : option nat;
  stored_finalization_records : list nat;
  stored_recovery_cursor_count : nat
}.

Definition pristine_finalization_store : rooted_finalization_store :=
  {| stored_genesis_anchor := None;
     stored_finalization_head := None;
     stored_finalization_records := [];
     stored_recovery_cursor_count := 0 |}.

Definition atomic_genesis_store (genesis : nat) : rooted_finalization_store :=
  {| stored_genesis_anchor := Some genesis;
     stored_finalization_head := Some 0;
     stored_finalization_records := [];
     stored_recovery_cursor_count := 3 |}.

Definition ensure_genesis_identity
  (requested : nat)
  (store : rooted_finalization_store) : option rooted_finalization_store :=
  match stored_genesis_anchor store,
        stored_finalization_head store,
        stored_finalization_records store,
        stored_recovery_cursor_count store with
  | None, None, [], 0 => Some (atomic_genesis_store requested)
  | Some canonical, Some _, _, 3 =>
      if Nat.eq_dec requested canonical then Some store else None
  | _, _, _, _ => None
  end.

Definition append_rooted_finalization
  (candidate : nat)
  (store : rooted_finalization_store) : option rooted_finalization_store :=
  match stored_genesis_anchor store,
        stored_finalization_head store,
        stored_recovery_cursor_count store with
  | Some genesis, Some head, 3 =>
      if Nat.eqb candidate (S head)
      then
        Some
          {| stored_genesis_anchor := Some genesis;
             stored_finalization_head := Some candidate;
             stored_finalization_records :=
               stored_finalization_records store ++ [candidate];
             stored_recovery_cursor_count := 3 |}
      else None
  | _, _, _ => None
  end.

Definition restart_rooted_finalization
  (store : rooted_finalization_store) : rooted_finalization_store := store.

Theorem pristine_bootstrap_is_atomic :
  forall genesis,
    ensure_genesis_identity genesis pristine_finalization_store =
    Some (atomic_genesis_store genesis).
Proof.
  reflexivity.
Qed.

Theorem exact_genesis_assertion_is_write_free :
  forall genesis head records,
    let store :=
      {| stored_genesis_anchor := Some genesis;
         stored_finalization_head := Some head;
         stored_finalization_records := records;
         stored_recovery_cursor_count := 3 |} in
    ensure_genesis_identity genesis store = Some store.
Proof.
  intros genesis head records store.
  unfold ensure_genesis_identity, store. simpl.
  destruct (Nat.eq_dec genesis genesis); congruence.
Qed.

Theorem conflicting_genesis_assertion_fails_closed :
  forall canonical requested head records,
    requested <> canonical ->
    let store :=
      {| stored_genesis_anchor := Some canonical;
         stored_finalization_head := Some head;
         stored_finalization_records := records;
         stored_recovery_cursor_count := 3 |} in
    ensure_genesis_identity requested store = None.
Proof.
  intros canonical requested head records Hconflict store.
  unfold ensure_genesis_identity, store. simpl.
  destruct (Nat.eq_dec requested canonical); congruence.
Qed.

Theorem successful_genesis_assertion_is_atomic_bootstrap_or_identity :
  forall requested store next,
    ensure_genesis_identity requested store = Some next ->
    (store = pristine_finalization_store /\
     next = atomic_genesis_store requested) \/
    (exists head records,
      store =
        {| stored_genesis_anchor := Some requested;
           stored_finalization_head := Some head;
           stored_finalization_records := records;
           stored_recovery_cursor_count := 3 |} /\
      next = store).
Proof.
  intros requested [anchor head records cursors] next Hensure.
  unfold ensure_genesis_identity in Hensure.
  destruct anchor as [canonical |].
  - destruct head as [current |].
    + destruct cursors as [|[|[|[|remaining]]]]; simpl in Hensure;
        try discriminate.
      destruct (Nat.eq_dec requested canonical) as [Heq | Hneq];
        try discriminate.
      inversion Hensure. subst canonical.
      right. exists current, records. split; reflexivity.
    + simpl in Hensure. discriminate.
  - destruct head as [current |].
    + simpl in Hensure. discriminate.
    + destruct records as [|record records].
      * destruct cursors as [|cursors]; simpl in Hensure; try discriminate.
        inversion Hensure.
        left. split; reflexivity.
      * simpl in Hensure. discriminate.
Qed.

Theorem successful_rooted_append_preserves_genesis_and_advances_one :
  forall candidate store next,
    append_rooted_finalization candidate store = Some next ->
    stored_genesis_anchor next = stored_genesis_anchor store /\
    stored_genesis_anchor next <> None /\
    stored_finalization_head next = Some candidate /\
    stored_recovery_cursor_count next = 3.
Proof.
  intros candidate [anchor head records cursors] next Happend.
  unfold append_rooted_finalization in Happend.
  destruct anchor as [genesis |]; try discriminate.
  destruct head as [current |]; try discriminate.
  destruct cursors as [|[|[|[|remaining]]]]; simpl in Happend;
    try discriminate.
  destruct (Nat.eqb candidate (S current)) eqn:Hsuccess; try discriminate.
  inversion Happend. simpl.
  repeat split; congruence.
Qed.

Theorem restart_preserves_rooted_finalization_identity :
  forall store,
    restart_rooted_finalization store = store.
Proof.
  reflexivity.
Qed.

Theorem successful_rooted_append_preserves_genesis :
  forall candidate store next,
    append_rooted_finalization candidate store = Some next ->
    stored_genesis_anchor next = stored_genesis_anchor store /\
    stored_genesis_anchor next <> None.
Proof.
  intros candidate store next Happend.
  pose proof
    (successful_rooted_append_preserves_genesis_and_advances_one
      candidate store next Happend) as [Hanchor [Hrooted _]].
  exact (conj Hanchor Hrooted).
Qed.

Theorem successful_genesis_assertion_cannot_rewrite_initialized_state :
  forall requested store next,
    ensure_genesis_identity requested store = Some next ->
    (store = pristine_finalization_store /\
     next = atomic_genesis_store requested) \/ next = store.
Proof.
  intros requested store next Hensure.
  pose proof
    (successful_genesis_assertion_is_atomic_bootstrap_or_identity
      requested store next Hensure) as [Hbootstrap | Hidentity].
  - left. exact Hbootstrap.
  - right. destruct Hidentity as [head [records [_ Hnext]]]. exact Hnext.
Qed.

Theorem rooted_genesis_identity_contract :
  (forall genesis,
    ensure_genesis_identity genesis pristine_finalization_store =
    Some (atomic_genesis_store genesis))
  /\
  (forall genesis head records,
    let store :=
      {| stored_genesis_anchor := Some genesis;
         stored_finalization_head := Some head;
         stored_finalization_records := records;
         stored_recovery_cursor_count := 3 |} in
    ensure_genesis_identity genesis store = Some store)
  /\
  (forall canonical requested head records,
    requested <> canonical ->
    let store :=
      {| stored_genesis_anchor := Some canonical;
         stored_finalization_head := Some head;
         stored_finalization_records := records;
         stored_recovery_cursor_count := 3 |} in
    ensure_genesis_identity requested store = None)
  /\
  (forall requested store next,
    ensure_genesis_identity requested store = Some next ->
    (store = pristine_finalization_store /\
     next = atomic_genesis_store requested) \/ next = store)
  /\
  (forall candidate store next,
    append_rooted_finalization candidate store = Some next ->
    stored_genesis_anchor next = stored_genesis_anchor store /\
    stored_genesis_anchor next <> None /\
    stored_finalization_head next = Some candidate /\
    stored_recovery_cursor_count next = 3)
  /\
  (forall store, restart_rooted_finalization store = store).
Proof.
  exact
    (conj pristine_bootstrap_is_atomic
      (conj exact_genesis_assertion_is_write_free
        (conj conflicting_genesis_assertion_fails_closed
          (conj successful_genesis_assertion_cannot_rewrite_initialized_state
            (conj successful_rooted_append_preserves_genesis_and_advances_one
                  restart_preserves_rooted_finalization_identity))))).
Qed.

Section BoundFinalizationHead.

Context {Block : Type}.
Variable block_eq_dec : forall left right : Block, {left = right} + {left <> right}.
Variable state_preserves : Block -> Block -> bool.
Variable dag_descends : Block -> Block -> bool.

Record bound_finalization_ledger := {
  bound_revision : nat;
  bound_head : Block;
  bound_history : list Block
}.

Record bound_finalization_certificate := {
  certificate_revision : nat;
  certificate_base : Block;
  certificate_candidate : Block
}.

Definition bound_finalization_compare_append
  (certificate : bound_finalization_certificate)
  (ledger : bound_finalization_ledger) : option bound_finalization_ledger :=
  if Nat.eqb (certificate_revision certificate) (bound_revision ledger)
  then
    if block_eq_dec (certificate_base certificate) (bound_head ledger)
    then
      if state_preserves
           (certificate_base certificate)
           (certificate_candidate certificate)
      then
        Some {| bound_revision := S (bound_revision ledger);
                bound_head := certificate_candidate certificate;
                bound_history :=
                  bound_history ledger ++ [certificate_candidate certificate] |}
      else None
    else None
  else None.

Definition late_bound_dag_accepts
  (certificate : bound_finalization_certificate)
  (ledger : bound_finalization_ledger) : bool :=
  dag_descends (bound_head ledger) (certificate_candidate certificate).

Theorem successful_bound_append_increments_revision :
  forall certificate ledger next,
    bound_finalization_compare_append certificate ledger = Some next ->
    bound_revision next = S (bound_revision ledger).
Proof.
  intros certificate ledger next Happend.
  unfold bound_finalization_compare_append in Happend.
  destruct (Nat.eqb (certificate_revision certificate) (bound_revision ledger));
    try discriminate.
  destruct (block_eq_dec (certificate_base certificate) (bound_head ledger));
    try discriminate.
  destruct (state_preserves
    (certificate_base certificate)
    (certificate_candidate certificate)); try discriminate.
  inversion Happend. reflexivity.
Qed.

Theorem successful_bound_append_preserves_exact_predecessor :
  forall certificate ledger next,
    bound_finalization_compare_append certificate ledger = Some next ->
    state_preserves (bound_head ledger) (bound_head next) = true.
Proof.
  intros certificate ledger next Happend.
  unfold bound_finalization_compare_append in Happend.
  destruct (Nat.eqb (certificate_revision certificate) (bound_revision ledger));
    try discriminate.
  destruct (block_eq_dec (certificate_base certificate) (bound_head ledger))
    as [Hbase | Hbase]; try discriminate.
  destruct (state_preserves
    (certificate_base certificate)
    (certificate_candidate certificate)) eqn:Hpreserves; try discriminate.
  inversion Happend. simpl. rewrite <- Hbase. exact Hpreserves.
Qed.

Theorem stale_bound_revision_is_inert :
  forall certificate ledger,
    certificate_revision certificate <> bound_revision ledger ->
    bound_finalization_compare_append certificate ledger = None.
Proof.
  intros certificate ledger Hstale.
  unfold bound_finalization_compare_append.
  apply Nat.eqb_neq in Hstale. rewrite Hstale. reflexivity.
Qed.

Theorem stale_bound_head_is_inert :
  forall certificate ledger,
    certificate_revision certificate = bound_revision ledger ->
    certificate_base certificate <> bound_head ledger ->
    bound_finalization_compare_append certificate ledger = None.
Proof.
  intros certificate ledger Hrevision Hstale.
  unfold bound_finalization_compare_append.
  rewrite Hrevision, Nat.eqb_refl.
  destruct (block_eq_dec (certificate_base certificate) (bound_head ledger));
    congruence.
Qed.

Theorem changed_bound_base_requires_fresh_evaluation :
  forall certificate ledger,
    certificate_revision certificate <> bound_revision ledger \/
    certificate_base certificate <> bound_head ledger ->
    bound_finalization_compare_append certificate ledger = None.
Proof.
  intros certificate ledger [Hrevision | Hhead].
  - apply stale_bound_revision_is_inert. exact Hrevision.
  - destruct (Nat.eq_dec
      (certificate_revision certificate)
      (bound_revision ledger)) as [Hequal | Hrevision].
    + apply stale_bound_head_is_inert; assumption.
    + apply stale_bound_revision_is_inert. exact Hrevision.
Qed.

Theorem compare_append_closes_validation_commit_race :
  forall first second ledger next,
    certificate_revision first = bound_revision ledger ->
    certificate_revision second = bound_revision ledger ->
    bound_finalization_compare_append first ledger = Some next ->
    bound_finalization_compare_append second next = None.
Proof.
  intros first second ledger next Hfirst_revision Hsecond_revision Hfirst.
  apply stale_bound_revision_is_inert.
  pose proof (successful_bound_append_increments_revision first ledger next Hfirst)
    as Hnext.
  rewrite Hsecond_revision, Hnext. lia.
Qed.

Theorem dag_ancestry_is_insufficient_for_late_bound_commit :
  forall old_base current_base candidate revision history,
    dag_descends current_base candidate = true ->
    state_preserves old_base candidate = true ->
    state_preserves current_base candidate = false ->
    let certificate :=
      {| certificate_revision := revision;
         certificate_base := old_base;
         certificate_candidate := candidate |} in
    let ledger :=
      {| bound_revision := S revision;
         bound_head := current_base;
         bound_history := history |} in
    late_bound_dag_accepts certificate ledger = true /\
    state_preserves (bound_head ledger) (certificate_candidate certificate) = false.
Proof.
  intros old_base current_base candidate revision history
    Hdag _ Hregression.
  simpl. split; assumption.
Qed.

Theorem bound_finalization_head_contract :
  (forall certificate ledger next,
    bound_finalization_compare_append certificate ledger = Some next ->
    state_preserves (bound_head ledger) (bound_head next) = true)
  /\
  (forall certificate ledger,
    certificate_revision certificate <> bound_revision ledger \/
    certificate_base certificate <> bound_head ledger ->
    bound_finalization_compare_append certificate ledger = None)
  /\
  (forall first second ledger next,
    certificate_revision first = bound_revision ledger ->
    certificate_revision second = bound_revision ledger ->
    bound_finalization_compare_append first ledger = Some next ->
    bound_finalization_compare_append second next = None).
Proof.
  exact (conj successful_bound_append_preserves_exact_predecessor
    (conj changed_bound_base_requires_fresh_evaluation
      compare_append_closes_validation_commit_race)).
Qed.

End BoundFinalizationHead.

Print Assumptions successful_compare_append_is_linearization_point.
Print Assumptions committed_head_has_its_round_record.
Print Assumptions stale_append_is_observationally_inert.
Print Assumptions one_successor_wins_same_head_race.
Print Assumptions effect_retry_is_idempotent.
Print Assumptions independent_effects_commute.
Print Assumptions monotonic_publication_never_regresses.
Print Assumptions publication_order_is_irrelevant.
Print Assumptions restart_preserves_durable_head_and_republishes_it.
Print Assumptions compare_append_commits_recorded_successor.
Print Assumptions finalization_atomicity_contract.
Print Assumptions finalization_worker_retry_contract.
Print Assumptions projection_step_is_bounded_by_head.
Print Assumptions effect_cursor_step_never_skips_a_revision.
Print Assumptions effect_cursor_advance_requires_completed_successor.
Print Assumptions completed_prefix_extends_one_at_a_time.
Print Assumptions restart_preserves_recovery_cursors.
Print Assumptions successful_compaction_never_exceeds_completed_effects.
Print Assumptions finalization_recovery_contract.
Print Assumptions pristine_bootstrap_is_atomic.
Print Assumptions exact_genesis_assertion_is_write_free.
Print Assumptions conflicting_genesis_assertion_fails_closed.
Print Assumptions successful_genesis_assertion_is_atomic_bootstrap_or_identity.
Print Assumptions successful_rooted_append_preserves_genesis_and_advances_one.
Print Assumptions successful_rooted_append_preserves_genesis.
Print Assumptions restart_preserves_rooted_finalization_identity.
Print Assumptions successful_genesis_assertion_cannot_rewrite_initialized_state.
Print Assumptions rooted_genesis_identity_contract.
Print Assumptions successful_bound_append_increments_revision.
Print Assumptions successful_bound_append_preserves_exact_predecessor.
Print Assumptions stale_bound_revision_is_inert.
Print Assumptions stale_bound_head_is_inert.
Print Assumptions changed_bound_base_requires_fresh_evaluation.
Print Assumptions compare_append_closes_validation_commit_race.
Print Assumptions dag_ancestry_is_insufficient_for_late_bound_commit.
Print Assumptions bound_finalization_head_contract.
