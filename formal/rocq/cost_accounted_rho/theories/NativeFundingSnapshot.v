From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
From CostAccountedRho Require Import FundingFamilyCapture FeeCursorTransition
  CanonicalCustodyAliasing SignedPhloCapture SignedPhloFunding SignedPhloControls.
Import ListNotations.

Inductive native_cursor_role := NativeResource | NativeFee.
Definition native_scope_identity := (native_cursor_role * list nat)%type.
Inductive native_read_key :=
| NativeWallet (custody : nat)
| NativeCursor (identity : native_scope_identity).
Inductive native_read_value :=
| NativeBalance (amount : nat)
| NativeCursorValue (cursor : option fee_cursor).

Record native_root_store := {
  native_wallet_at : nat -> nat -> nat;
  native_cursor_at : nat -> native_scope_identity -> option fee_cursor
}.

Definition native_read_at store root key :=
  match key with
  | NativeWallet custody => NativeBalance (native_wallet_at store root custody)
  | NativeCursor identity => NativeCursorValue (native_cursor_at store root identity)
  end.

Fixpoint native_pinned_reads store captured_root (schedule : list (nat * native_read_key)) :=
  match schedule with
  | [] => []
  | (external_root, key) :: rest =>
      native_read_at store captured_root key :: native_pinned_reads store captured_root rest
  end.

Fixpoint native_current_reads store schedule :=
  match schedule with
  | [] => []
  | (current_root, key) :: rest =>
      native_read_at store current_root key :: native_current_reads store rest
  end.

Theorem native_pinned_reads_share_one_root : forall store root schedule,
  native_pinned_reads store root schedule =
    map (native_read_at store root) (map snd schedule).
Proof.
  intros store root schedule. induction schedule as [|[external key] rest IH]; simpl; congruence.
Qed.

Theorem native_pinned_reads_ignore_external_root_changes : forall store root left right,
  map snd left = map snd right ->
  native_pinned_reads store root left = native_pinned_reads store root right.
Proof.
  intros store root left right same. rewrite !native_pinned_reads_share_one_root, same. reflexivity.
Qed.

Definition native_snapshot_keys cohort :=
  map NativeWallet cohort ++
    [NativeCursor (NativeResource, cohort); NativeCursor (NativeFee, cohort)].

Record native_funding_snapshot := {
  native_snapshot_root : nat;
  native_snapshot_cohort : list nat;
  native_snapshot_wallets : list (nat * nat);
  native_snapshot_resource : option fee_cursor;
  native_snapshot_fee : option fee_cursor
}.

Definition capture_native_snapshot store root cohort :=
  {| native_snapshot_root := root;
     native_snapshot_cohort := cohort;
     native_snapshot_wallets := map (fun custody => (custody, native_wallet_at store root custody)) cohort;
     native_snapshot_resource := native_cursor_at store root (NativeResource, cohort);
     native_snapshot_fee := native_cursor_at store root (NativeFee, cohort) |}.

Definition native_snapshot_values snapshot :=
  map (fun row => NativeBalance (snd row)) (native_snapshot_wallets snapshot) ++
    [NativeCursorValue (native_snapshot_resource snapshot);
     NativeCursorValue (native_snapshot_fee snapshot)].

Theorem native_complete_capture_matches_pinned_read_trace : forall store root cohort schedule,
  map snd schedule = native_snapshot_keys cohort ->
  native_pinned_reads store root schedule =
    native_snapshot_values (capture_native_snapshot store root cohort).
Proof.
  intros store root cohort schedule keys. rewrite native_pinned_reads_share_one_root, keys.
  unfold native_snapshot_keys, native_snapshot_values, capture_native_snapshot. simpl.
  rewrite map_app, !map_map. reflexivity.
Qed.

Theorem native_capture_wallet_membership_exact : forall store root cohort custody amount,
  In (custody, amount) (native_snapshot_wallets (capture_native_snapshot store root cohort)) <->
  In custody cohort /\ amount = native_wallet_at store root custody.
Proof.
  intros store root cohort custody amount. simpl. rewrite in_map_iff. split.
  - intros [source [same included]]. inversion same; subst. auto.
  - intros [included same]. subst. exists custody. auto.
Qed.

Theorem native_capture_keeps_both_cursor_observations : forall store root cohort,
  native_snapshot_resource (capture_native_snapshot store root cohort) =
    native_cursor_at store root (NativeResource, cohort) /\
  native_snapshot_fee (capture_native_snapshot store root cohort) =
    native_cursor_at store root (NativeFee, cohort).
Proof. intros; split; reflexivity. Qed.

Definition native_selector_cursor observed :=
  match observed with
  | None => {| cursor_revision := 0; cursor_position := 0 |}
  | Some cursor => cursor
  end.

Theorem native_absence_distinct_until_selector_default :
  (None : option fee_cursor) <> Some {| cursor_revision := 0; cursor_position := 0 |} /\
  native_selector_cursor None =
    native_selector_cursor (Some {| cursor_revision := 0; cursor_position := 0 |}).
Proof. split; [discriminate|reflexivity]. Qed.

Theorem native_role_scope_preimages_are_distinct : forall (cohort : list nat),
  (NativeResource, cohort) <> (NativeFee, cohort).
Proof. discriminate. Qed.

Theorem native_role_scope_hashes_require_injectivity : forall
  (scope_hash : native_scope_identity -> nat) cohort,
  (forall left right, scope_hash left = scope_hash right -> left = right) ->
  scope_hash (NativeResource, cohort) <> scope_hash (NativeFee, cohort).
Proof.
  intros scope_hash cohort injective same. apply injective in same. discriminate.
Qed.

Definition native_snapshot_cursor_pair scope_hash snapshot :=
  let cohort := native_snapshot_cohort snapshot in
  {| family_snapshot_custodies := cohort;
     family_snapshot_resource :=
       {| family_snapshot_scope := scope_hash (NativeResource, cohort);
          family_snapshot_cursor := native_selector_cursor (native_snapshot_resource snapshot) |};
     family_snapshot_fee :=
       {| family_snapshot_scope := scope_hash (NativeFee, cohort);
          family_snapshot_cursor := native_selector_cursor (native_snapshot_fee snapshot) |} |}.

Record native_bound_family := {
  native_bound_snapshot : native_funding_snapshot;
  native_bound_policy : family_capture_policy
}.

Definition bind_native_family maximum scope_hash expected intent amounts snapshot transitions :=
  if Nat.eqb (scope_hash (NativeResource, native_snapshot_cohort snapshot))
      (scope_hash (NativeFee, native_snapshot_cohort snapshot)) then None else
    match prepare_family_capture_policy maximum expected intent amounts
      (native_snapshot_cursor_pair scope_hash snapshot) transitions with
    | None => None
    | Some policy => Some {| native_bound_snapshot := snapshot; native_bound_policy := policy |}
    end.

Theorem native_checked_binding_rejects_colliding_scope_keys :
  forall maximum scope_hash expected intent amounts snapshot transitions bound,
  bind_native_family maximum scope_hash expected intent amounts snapshot transitions = Some bound ->
  scope_hash (NativeResource, native_snapshot_cohort snapshot) <>
    scope_hash (NativeFee, native_snapshot_cohort snapshot).
Proof.
  intros maximum scope_hash expected intent amounts snapshot transitions bound checked.
  unfold bind_native_family in checked. destruct (Nat.eqb _ _) eqn:distinct; [discriminate|].
  apply Nat.eqb_neq. exact distinct.
Qed.

Theorem native_checked_binding_keeps_snapshot_and_roles : forall maximum scope_hash expected
  intent amounts store root cohort transitions bound,
  bind_native_family maximum scope_hash expected intent amounts
    (capture_native_snapshot store root cohort) transitions = Some bound ->
  native_bound_snapshot bound = capture_native_snapshot store root cohort /\
  native_snapshot_root (native_bound_snapshot bound) = root /\
  native_snapshot_cohort (native_bound_snapshot bound) = expected /\
  family_policy_cursors (native_bound_policy bound) =
    native_snapshot_cursor_pair scope_hash (capture_native_snapshot store root cohort) /\
  family_policy_snapshot (native_bound_policy bound) = amounts /\
  family_policy_transitions (native_bound_policy bound) = transitions.
Proof.
  intros maximum scope_hash expected intent amounts store root cohort transitions bound checked.
  unfold bind_native_family in checked.
  destruct (Nat.eqb _ _) eqn:distinct; [discriminate|].
  destruct (prepare_family_capture_policy _ _ _ _ _ _) as [policy|] eqn:prepared;
    [|discriminate]. inversion checked; subst bound. simpl.
  pose proof (family_prepared_capture_retains_checked_inputs _ _ _ _ _ _ _ prepared)
    as [same_amounts [same_pair [same_transitions [_ [_ [same_cohort _]]]]]].
  cbn in same_cohort. repeat split; assumption || reflexivity.
Qed.

Theorem native_checked_binding_cannot_rebind_root_or_cohort :
  forall bound store root cohort other_root other_cohort,
  native_bound_snapshot bound = capture_native_snapshot store root cohort ->
  native_bound_snapshot bound = capture_native_snapshot store other_root other_cohort ->
  root = other_root /\ cohort = other_cohort.
Proof.
  intros bound store root cohort other_root other_cohort first second.
  rewrite first in second. inversion second. auto.
Qed.

Definition native_aba_store :=
  {| native_wallet_at := fun root _ => root;
     native_cursor_at := fun _ _ => None |}.

Theorem native_before_after_labels_do_not_prove_snapshot_consistency :
  let before := 0 in
  let after := 0 in
  let schedule := [(0, NativeWallet 10); (1, NativeWallet 20);
                   (0, NativeCursor (NativeResource, [10;20]));
                   (0, NativeCursor (NativeFee, [10;20]))] in
  before = after /\
  native_current_reads native_aba_store schedule <>
    native_snapshot_values (capture_native_snapshot native_aba_store before [10;20]) /\
  native_pinned_reads native_aba_store before schedule =
    native_snapshot_values (capture_native_snapshot native_aba_store before [10;20]) /\
  (forall root, native_current_reads native_aba_store schedule <>
    native_snapshot_values (capture_native_snapshot native_aba_store root [10;20])).
Proof.
  simpl. repeat split; try discriminate; try reflexivity.
  intros root same. inversion same. lia.
Qed.

Print Assumptions native_pinned_reads_share_one_root.
Print Assumptions native_pinned_reads_ignore_external_root_changes.
Print Assumptions native_complete_capture_matches_pinned_read_trace.
Print Assumptions native_capture_wallet_membership_exact.
Print Assumptions native_capture_keeps_both_cursor_observations.
Print Assumptions native_absence_distinct_until_selector_default.
Print Assumptions native_role_scope_preimages_are_distinct.
Print Assumptions native_role_scope_hashes_require_injectivity.
Print Assumptions native_checked_binding_keeps_snapshot_and_roles.
Print Assumptions native_checked_binding_rejects_colliding_scope_keys.
Print Assumptions native_checked_binding_cannot_rebind_root_or_cohort.
Print Assumptions native_before_after_labels_do_not_prove_snapshot_consistency.

Section NativeExecutionBinding.
Context {Root Envelope Controls : Type}.
Variable root_eq : forall (left right : Root), {left = right} + {left <> right}.
Variable envelope_eq : forall (left right : Envelope), {left = right} + {left <> right}.
Variable controls_eq : forall (left right : Controls), {left = right} + {left <> right}.

Record native_execution_authorization := {
  execution_authorized_root : Root;
  execution_authorized_envelope : Envelope;
  execution_authorized_controls : Controls
}.

Definition bind_native_execution authorized root envelope controls :=
  if root_eq root (execution_authorized_root authorized) then
    if envelope_eq envelope (execution_authorized_envelope authorized) then
      if controls_eq controls (execution_authorized_controls authorized) then Some authorized else None
    else None
  else None.

Theorem native_execution_binding_exact : forall authorized root envelope controls bound,
  bind_native_execution authorized root envelope controls = Some bound <->
  bound = authorized /\ root = execution_authorized_root authorized /\
  envelope = execution_authorized_envelope authorized /\
  controls = execution_authorized_controls authorized.
Proof.
  intros. unfold bind_native_execution.
  destruct (root_eq root (execution_authorized_root authorized));
    destruct (envelope_eq envelope (execution_authorized_envelope authorized));
    destruct (controls_eq controls (execution_authorized_controls authorized));
    split; intros H; try discriminate; try (inversion H; subst; tauto); tauto.
Qed.

Theorem native_execution_binding_accepts_authorized_inputs : forall authorized,
  bind_native_execution authorized (execution_authorized_root authorized)
    (execution_authorized_envelope authorized) (execution_authorized_controls authorized) = Some authorized.
Proof.
  intros. apply native_execution_binding_exact. repeat split; reflexivity.
Qed.

Theorem native_execution_binding_rejects_substitution : forall authorized root envelope controls,
  (root <> execution_authorized_root authorized \/
   envelope <> execution_authorized_envelope authorized \/
   controls <> execution_authorized_controls authorized) ->
  bind_native_execution authorized root envelope controls = None.
Proof.
  intros authorized root envelope controls changed.
  destruct (bind_native_execution authorized root envelope controls) as [bound|] eqn:checked; auto.
  apply native_execution_binding_exact in checked. tauto.
Qed.

Theorem native_execution_binding_same_authorization_agrees : forall authorized
    root_left envelope_left controls_left left root_right envelope_right controls_right right,
  bind_native_execution authorized root_left envelope_left controls_left = Some left ->
  bind_native_execution authorized root_right envelope_right controls_right = Some right ->
  left = right /\ root_left = root_right /\ envelope_left = envelope_right /\ controls_left = controls_right.
Proof.
  intros authorized root_left envelope_left controls_left left root_right envelope_right controls_right right one two.
  apply native_execution_binding_exact in one, two.
  destruct one as [-> [-> [-> ->]]]. destruct two as [-> [-> [-> ->]]].
  repeat split; reflexivity.
Qed.
End NativeExecutionBinding.

Section NativeReplaySettlementBoundary.
Context {Root Evidence : Type}.

Definition replay_settlement_state (before after : Root) (failed : bool) : Root :=
  if failed then before else after.

Definition complete_funded_replay (authorized complete failed : bool)
    (before after : Root) (evidence : Evidence) : option (Root * Evidence) :=
  if authorized && complete then
    Some (replay_settlement_state before after failed, evidence)
  else None.

Theorem funded_replay_completion_exact : forall authorized complete failed before after evidence result,
  complete_funded_replay authorized complete failed before after evidence = Some result <->
  authorized = true /\ complete = true /\
  result = (replay_settlement_state before after failed, evidence).
Proof.
  intros [] [] [] before after evidence result;
    unfold complete_funded_replay, replay_settlement_state; simpl; intuition congruence.
Qed.

Theorem funded_replay_incomplete_has_no_settlement : forall authorized failed before after evidence,
  complete_funded_replay authorized false failed before after evidence = None.
Proof. intros []; reflexivity. Qed.

Theorem funded_replay_unauthorized_has_no_settlement : forall complete failed before after evidence,
  complete_funded_replay false complete failed before after evidence = None.
Proof. reflexivity. Qed.

Theorem funded_replay_failure_keeps_evidence : forall before after evidence,
  complete_funded_replay true true true before after evidence = Some (before, evidence).
Proof. reflexivity. Qed.

Theorem funded_replay_success_keeps_effects : forall before after evidence,
  complete_funded_replay true true false before after evidence = Some (after, evidence).
Proof. reflexivity. Qed.

Theorem funded_replay_failure_ignores_partial_state : forall before left right evidence,
  complete_funded_replay true true true before left evidence =
  complete_funded_replay true true true before right evidence.
Proof. reflexivity. Qed.

Theorem funded_replay_does_not_fabricate_evidence : forall authorized complete failed before after evidence root captured,
  complete_funded_replay authorized complete failed before after evidence = Some (root, captured) ->
  captured = evidence.
Proof.
  intros. apply funded_replay_completion_exact in H.
  destruct H as [_ [_ H]]. now inversion H.
Qed.
End NativeReplaySettlementBoundary.

Section BoundNativeReplaySettlement.
Context {Root Envelope Controls Evidence : Type}.
Variable root_eq : forall (a b : Root), {a = b} + {a <> b}.
Variable envelope_eq : forall (a b : Envelope), {a = b} + {a <> b}.
Variable controls_eq : forall (a b : Controls), {a = b} + {a <> b}.

Definition complete_bound_funded_replay authorized root envelope controls complete failed after (evidence : Evidence) :=
  match bind_native_execution root_eq envelope_eq controls_eq authorized root envelope controls with
  | Some _ => complete_funded_replay true complete failed root after evidence
  | None => None
  end.

Theorem bound_funded_replay_checks_every_input : forall authorized root envelope controls complete failed after evidence result,
  complete_bound_funded_replay authorized root envelope controls complete failed after evidence = Some result <->
  root = execution_authorized_root authorized /\
  envelope = execution_authorized_envelope authorized /\
  controls = execution_authorized_controls authorized /\ complete = true /\
  result = (replay_settlement_state root after failed, evidence).
Proof.
  intros. unfold complete_bound_funded_replay.
  destruct (bind_native_execution root_eq envelope_eq controls_eq authorized root envelope controls)
    as [bound|] eqn:binding.
  - apply native_execution_binding_exact in binding.
    rewrite funded_replay_completion_exact. tauto.
  - split; [discriminate|]. intros [Hroot [Henvelope [Hcontrols _]]].
    subst root envelope controls.
    rewrite native_execution_binding_accepts_authorized_inputs in binding. discriminate.
Qed.

Theorem bound_funded_replay_rejects_any_substitution : forall authorized root envelope controls complete failed after evidence,
  (root <> execution_authorized_root authorized \/
   envelope <> execution_authorized_envelope authorized \/
   controls <> execution_authorized_controls authorized) ->
  complete_bound_funded_replay authorized root envelope controls complete failed after evidence = None.
Proof.
  intros. unfold complete_bound_funded_replay.
  rewrite native_execution_binding_rejects_substitution by assumption. reflexivity.
Qed.

Theorem bound_funded_replay_preserves_failed_charge_evidence : forall authorized after evidence,
  complete_bound_funded_replay authorized (execution_authorized_root authorized)
    (execution_authorized_envelope authorized) (execution_authorized_controls authorized)
    true true after evidence = Some (execution_authorized_root authorized, evidence).
Proof.
  intros. unfold complete_bound_funded_replay.
  rewrite native_execution_binding_accepts_authorized_inputs. reflexivity.
Qed.
End BoundNativeReplaySettlement.

Section NativeSettlementRoots.
Context {Root Evidence : Type}.
Variable root_eq : forall (a b : Root), {a = b} + {a <> b}.

Definition settlement_roots_match (expected actual : Root * Root) :=
  if root_eq (fst expected) (fst actual) then
    if root_eq (snd expected) (snd actual) then true else false
  else false.

Definition funded_settlement_roots authorized complete failed (before after : Root) (evidence : Evidence) :=
  match complete_funded_replay authorized complete failed before after evidence with
  | Some (working, checked) => Some ((before, working), checked)
  | None => None
  end.

Theorem settlement_roots_match_exact : forall expected actual,
  settlement_roots_match expected actual = true <-> expected = actual.
Proof.
  intros [captured working] [other_captured other_working].
  unfold settlement_roots_match; simpl.
  destruct (root_eq captured other_captured); destruct (root_eq working other_working);
    split; intros; try discriminate; congruence.
Qed.

Theorem settlement_roots_preserve_both_bindings : forall captured working observed runtime,
  settlement_roots_match (captured, working) (observed, runtime) = true ->
  captured = observed /\ working = runtime.
Proof. intros. apply settlement_roots_match_exact in H. inversion H. auto. Qed.

Theorem settlement_roots_reject_either_substitution : forall captured working observed runtime,
  (captured <> observed \/ working <> runtime) ->
  settlement_roots_match (captured, working) (observed, runtime) = false.
Proof.
  intros captured working observed runtime different.
  destruct (settlement_roots_match (captured, working) (observed, runtime)) eqn:checked; auto.
  apply settlement_roots_preserve_both_bindings in checked. tauto.
Qed.

Theorem funded_settlement_roots_keep_original_capture : forall authorized complete failed
    before after evidence roots checked,
  funded_settlement_roots authorized complete failed before after evidence = Some (roots, checked) ->
  authorized = true /\ complete = true /\ fst roots = before /\
  snd roots = replay_settlement_state before after failed /\ checked = evidence.
Proof.
  intros authorized complete failed before after evidence roots checked accepted.
  unfold funded_settlement_roots in accepted.
  destruct (complete_funded_replay authorized complete failed before after evidence)
    as [[working value]|] eqn:replayed; [|discriminate].
  apply funded_replay_completion_exact in replayed.
  destruct replayed as [auth [done same]]. inversion same; subst.
  inversion accepted; subst. simpl. repeat split; reflexivity.
Qed.

Theorem successful_replay_accepts_its_distinct_working_root : forall before after evidence,
  funded_settlement_roots true true false before after evidence = Some ((before, after), evidence) /\
  settlement_roots_match (before, after) (before, after) = true.
Proof. intros. split; [reflexivity|apply settlement_roots_match_exact; reflexivity]. Qed.

Theorem failed_replay_keeps_original_root_and_evidence : forall before after evidence,
  funded_settlement_roots true true true before after evidence = Some ((before, before), evidence).
Proof. reflexivity. Qed.

Theorem unauthorized_replay_cannot_supply_settlement_roots : forall complete failed before after evidence,
  funded_settlement_roots false complete failed before after evidence = None.
Proof. reflexivity. Qed.

Theorem incomplete_replay_cannot_supply_settlement_roots : forall authorized failed before after evidence,
  funded_settlement_roots authorized false failed before after evidence = None.
Proof. intros []; reflexivity. Qed.
End NativeSettlementRoots.

Example original_root_only_guard_rejects_a_valid_replay :
  settlement_roots_match Nat.eq_dec (1, 2) (1, 2) = true /\ (1 =? 2) = false.
Proof. split; reflexivity. Qed.

Print Assumptions native_execution_binding_exact.
Print Assumptions native_execution_binding_accepts_authorized_inputs.
Print Assumptions native_execution_binding_rejects_substitution.
Print Assumptions native_execution_binding_same_authorization_agrees.

Record direct_wallet_row := {
  wallet_row_custody : nat;
  wallet_row_capacity : nat;
  wallet_row_exposure : nat;
  wallet_row_debit : nat
}.

Definition wallet_row_eq_dec : forall (a b : direct_wallet_row), {a = b} + {a <> b}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition direct_family_rows snapshot :=
  let domain := snapshot_domain snapshot in
  map (fun source => {| wallet_row_custody := funding_custody domain source;
    wallet_row_capacity := funding_capacity domain source;
    wallet_row_exposure := signed_source_exposure domain source;
    wallet_row_debit := signed_source_debit domain source |}) (seq 0 (funding_sources domain)).

Definition check_direct_offered_binding (wallet_of : nat -> nat) selected wallets terms intent snapshot minimum offer :=
  direct_wallets_authorized Nat.eq_dec wallet_of selected (map wallet_row_custody wallets) &&
  (length wallets =? length (direct_family_rows snapshot)) &&
  snapshot_rows_match wallet_row_eq_dec wallets (direct_family_rows snapshot) &&
  check_offered_phlo_family_intent terms intent snapshot minimum offer.

Theorem direct_offered_binding_retains_all_obligations :
  forall wallet_of selected wallets terms intent snapshot minimum offer,
  check_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer = true <->
  (forall custody, In custody (map wallet_row_custody wallets) ->
    exists owner, In owner selected /\ wallet_of owner = custody) /\
  length wallets = length (direct_family_rows snapshot) /\
  incl (direct_family_rows snapshot) wallets /\
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true.
Proof.
  intros. unfold check_direct_offered_binding.
  rewrite !andb_true_iff, Nat.eqb_eq, direct_wallet_authorization_exact, snapshot_rows_match_exact. tauto.
Qed.

Theorem direct_offered_binding_authorizes_every_family_row :
  forall wallet_of selected wallets terms intent snapshot minimum offer row,
  check_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer = true ->
  In row (direct_family_rows snapshot) ->
  In row wallets /\ exists owner, In owner selected /\ wallet_of owner = wallet_row_custody row.
Proof.
  intros wallet_of selected wallets terms intent snapshot minimum offer row checked present.
  apply direct_offered_binding_retains_all_obligations in checked.
  destruct checked as [authorized [_ [included _]]].
  specialize (included row present). split; [exact included|].
  apply authorized. apply in_map. exact included.
Qed.

Record direct_offered_binding := {
  direct_binding_root : nat;
  direct_binding_wallets : list direct_wallet_row;
  direct_binding_payload : list nat;
  direct_binding_offer : signed_phlo_offer;
  direct_binding_snapshot : phlo_snapshot
}.

Definition prepare_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer root payload :=
  if check_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer then
    Some {| direct_binding_root := root; direct_binding_wallets := wallets;
      direct_binding_payload := payload; direct_binding_offer := offer; direct_binding_snapshot := snapshot |}
  else None.

Theorem prepared_direct_offered_binding_keeps_original_inputs :
  forall wallet_of selected wallets terms intent snapshot minimum offer root payload bound,
  prepare_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer root payload = Some bound ->
  direct_binding_root bound = root /\ direct_binding_wallets bound = wallets /\
  direct_binding_payload bound = payload /\ direct_binding_offer bound = offer /\
  direct_binding_snapshot bound = snapshot /\
  check_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer = true.
Proof.
  intros wallet_of selected wallets terms intent snapshot minimum offer root payload bound prepared.
  unfold prepare_direct_offered_binding in prepared.
  destruct (check_direct_offered_binding wallet_of selected wallets terms intent snapshot minimum offer) eqn:checked;
    [|discriminate].
  inversion prepared; subst bound. simpl. repeat split; assumption || reflexivity.
Qed.

Print Assumptions direct_offered_binding_retains_all_obligations.
Print Assumptions direct_offered_binding_authorizes_every_family_row.
Print Assumptions prepared_direct_offered_binding_keeps_original_inputs.

Record native_payment_row := {
  payment_custody : nat;
  payment_wallet : nat;
  payment_amounts : phlo_native_amounts
}.

Definition native_amounts_conserve amounts :=
  native_acquisition amounts + native_fee amounts + native_refund amounts = native_hold amounts.

Definition project_native_payment (wallet_of : nat -> option nat) custody amounts :=
  match wallet_of custody with
  | Some wallet =>
      if Nat.eqb (native_acquisition amounts + native_fee amounts + native_refund amounts)
          (native_hold amounts) then
        Some {| payment_custody := custody; payment_wallet := wallet; payment_amounts := amounts |}
      else None
  | None => None
  end.

Fixpoint project_native_payments wallet_of (rows : list (nat * phlo_native_amounts)) :=
  match rows with
  | [] => Some []
  | (custody, amounts) :: rest =>
      match project_native_payment wallet_of custody amounts, project_native_payments wallet_of rest with
      | Some row, Some remaining => Some (row :: remaining)
      | _, _ => None
      end
  end.

Definition positive_native_payments :=
  filter (fun row => negb (native_hold (payment_amounts row) =? 0)).

Theorem native_payment_projection_exact : forall wallet_of custody amounts row,
  project_native_payment wallet_of custody amounts = Some row <->
  wallet_of custody = Some (payment_wallet row) /\
  payment_custody row = custody /\ payment_amounts row = amounts /\ native_amounts_conserve amounts.
Proof.
  intros wallet_of custody amounts row. unfold project_native_payment, native_amounts_conserve.
  destruct (wallet_of custody) as [wallet|] eqn:lookup.
  - destruct (Nat.eqb (native_acquisition amounts + native_fee amounts + native_refund amounts)
        (native_hold amounts)) eqn:conserves.
    + apply Nat.eqb_eq in conserves. split.
      * intros same. inversion same; subst. simpl. auto.
      * intros [same [custody_same [amounts_same _]]].
        destruct row; simpl in *. inversion same. subst. reflexivity.
    + apply Nat.eqb_neq in conserves. split; [discriminate|tauto].
  - split; [discriminate|intros [impossible _]; discriminate].
Qed.

Theorem native_payment_projection_covers_complete_cohort : forall wallet_of rows projected,
  project_native_payments wallet_of rows = Some projected ->
  Forall2 (fun input output =>
    wallet_of (fst input) = Some (payment_wallet output) /\
    payment_custody output = fst input /\ payment_amounts output = snd input /\
    native_amounts_conserve (snd input)) rows projected.
Proof.
  intros wallet_of rows. induction rows as [|[custody amounts] rest IH]; intros projected complete; simpl in complete.
  - inversion complete. constructor.
  - destruct (project_native_payment wallet_of custody amounts) as [row|] eqn:checked; [|discriminate].
    destruct (project_native_payments wallet_of rest) as [remaining|] eqn:tail_checked; [|discriminate].
    inversion complete; subst. constructor.
    + apply native_payment_projection_exact in checked. exact checked.
    + apply IH. reflexivity.
Qed.

Theorem native_payment_projection_preserves_cohort_size : forall wallet_of rows projected,
  project_native_payments wallet_of rows = Some projected -> length rows = length projected.
Proof.
  intros wallet_of rows projected checked.
  apply native_payment_projection_covers_complete_cohort in checked.
  induction checked; simpl; congruence.
Qed.

Theorem native_payment_zero_hold_has_no_amounts : forall amounts,
  native_amounts_conserve amounts -> native_hold amounts = 0 ->
  native_acquisition amounts = 0 /\ native_fee amounts = 0 /\ native_refund amounts = 0.
Proof. intros amounts conserved zero. unfold native_amounts_conserve in conserved. lia. Qed.

Theorem native_payment_missing_wallet_rejects_even_zero_hold : forall wallet_of custody amounts,
  wallet_of custody = None -> project_native_payment wallet_of custody amounts = None.
Proof. intros wallet_of custody amounts missing. unfold project_native_payment. now rewrite missing. Qed.

Theorem native_payment_filter_keeps_full_refunds : forall rows row,
  In row rows -> 0 < native_hold (payment_amounts row) -> In row (positive_native_payments rows).
Proof.
  intros rows row present positive. unfold positive_native_payments.
  apply filter_In. split; [assumption|]. apply negb_true_iff, Nat.eqb_neq. lia.
Qed.

Theorem native_payment_filter_omits_only_zero_amounts : forall rows row,
  In row rows -> native_amounts_conserve (payment_amounts row) ->
  ~ In row (positive_native_payments rows) ->
  native_hold (payment_amounts row) = 0 /\ native_acquisition (payment_amounts row) = 0 /\
  native_fee (payment_amounts row) = 0 /\ native_refund (payment_amounts row) = 0.
Proof.
  intros rows row present conserved absent.
  assert (native_hold (payment_amounts row) = 0) as zero.
  { destruct (Nat.eq_dec (native_hold (payment_amounts row)) 0); [assumption|].
    exfalso. apply absent, native_payment_filter_keeps_full_refunds; [assumption|lia]. }
  split; [assumption|]. now apply native_payment_zero_hold_has_no_amounts.
Qed.

Print Assumptions native_payment_projection_exact.
Print Assumptions native_payment_projection_covers_complete_cohort.
Print Assumptions native_payment_projection_preserves_cohort_size.
Print Assumptions native_payment_zero_hold_has_no_amounts.
Print Assumptions native_payment_missing_wallet_rejects_even_zero_hold.
Print Assumptions native_payment_filter_keeps_full_refunds.
Print Assumptions native_payment_filter_omits_only_zero_amounts.
