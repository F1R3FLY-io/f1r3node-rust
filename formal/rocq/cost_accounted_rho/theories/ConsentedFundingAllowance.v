From Stdlib Require Import Arith.PeanoNat Arith.Compare_dec Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingPriceConsent FundingConsentHistory
  PersistentFundingAllowance.
Import ListNotations.

Record allowance_capture := {
  captured_consent : captured_reservation;
  captured_amount : nat
}.

Record consented_allowance := {
  grant_identity : nat;
  current_terms : funding_terms;
  quantity_state : allowance;
  captures : list allowance_capture
}.

Definition with_quantity (state : consented_allowance) (quantity : allowance)
  : consented_allowance :=
  {| grant_identity := grant_identity state;
     current_terms := current_terms state;
     quantity_state := quantity;
     captures := captures state |}.

Definition initial_consented_allowance (grant amount : nat) (terms : funding_terms)
  : consented_allowance :=
  {| grant_identity := grant; current_terms := terms;
     quantity_state := initial_allowance amount; captures := [] |}.

Definition reserve_consented (state : consented_allowance)
  (expected root price amount : nat) (authorized_and_funded : bool)
  : option consented_allowance :=
  match required_price_ceiling (required_ceilings (current_terms state)) with
  | None => None
  | Some ceiling =>
      if price <=? ceiling then
        match allowance_step (quantity_state state)
          (Reserve expected amount authorized_and_funded) with
        | None => None
        | Some quantity => Some {|
            grant_identity := grant_identity state;
            current_terms := current_terms state;
            quantity_state := quantity;
            captures := captures state ++ [{|
              captured_consent := {|
                reservation_right := grant_identity state;
                reservation_generation := authorization_version (quantity_state state);
                reservation_terms := current_terms state;
                reservation_root := root;
                reservation_price := price |};
              captured_amount := amount |}] |}
        end
      else None
  end.

Definition transfer_consented (state : consented_allowance) (expected : nat)
  (terms : funding_terms) (authorized : bool) : option consented_allowance :=
  match allowance_step (quantity_state state) (Transfer expected authorized) with
  | None => None
  | Some quantity => Some {|
      grant_identity := grant_identity state;
      current_terms := terms;
      quantity_state := quantity;
      captures := captures state |}
  end.

Definition settle_consented (state : consented_allowance) (index debit : nat)
  : option (allowance_capture * consented_allowance) :=
  match nth_error (captures state) index with
  | None => None
  | Some captured =>
      match settle_allowance (quantity_state state) index debit with
      | None => None
      | Some quantity => Some (captured, with_quantity state quantity)
      end
  end.

Definition expand_consented (state : consented_allowance) (expected amount : nat)
  (authorized : bool) : option consented_allowance :=
  match allowance_step (quantity_state state) (Expand expected amount authorized) with
  | None => None
  | Some quantity => Some (with_quantity state quantity)
  end.

Definition captures_match_slots (state : consented_allowance) : Prop :=
  length (captures state) = length (active_slots (quantity_state state)) /\
  forall index amount,
    nth_error (active_slots (quantity_state state)) index = Some (Some amount) ->
    exists captured, nth_error (captures state) index = Some captured /\
      captured_amount captured = amount /\
      reservation_right (captured_consent captured) = grant_identity state.

Definition consented_valid (state : consented_allowance) : Prop :=
  conserved (quantity_state state) /\ captures_match_slots state.

Lemma close_slot_preserves_length : forall slots index amount next,
  close_slot index slots = Some (amount, next) -> length next = length slots.
Proof.
  induction slots as [|slot rest IH]; intros index amount next H;
    destruct index; simpl in H; try discriminate.
  - destruct slot; inversion H; subst; reflexivity.
  - destruct (close_slot index rest) as [[found tail]|] eqn:E; try discriminate.
    inversion H; subst; simpl. f_equal. eapply IH; eauto.
Qed.

Lemma close_slot_preserves_remaining : forall slots closed amount next index remaining,
  close_slot closed slots = Some (amount, next) ->
  nth_error next index = Some (Some remaining) ->
  nth_error slots index = Some (Some remaining).
Proof.
  induction slots as [|slot rest IH]; intros closed amount next index remaining H N;
    destruct closed; simpl in H; try discriminate.
  - destruct slot; inversion H; subst. destruct index; simpl in *; congruence.
  - destruct (close_slot closed rest) as [[found tail]|] eqn:E; try discriminate.
    inversion H; subst. destruct index; simpl in *; [exact N|].
    eapply IH; eauto.
Qed.

Lemma close_slot_reads_exact_amount : forall slots index amount next,
  close_slot index slots = Some (amount, next) ->
  nth_error slots index = Some (Some amount).
Proof.
  induction slots as [|slot rest IH]; intros index amount next H;
    destruct index; simpl in H; try discriminate.
  - destruct slot; inversion H; subst; reflexivity.
  - destruct (close_slot index rest) as [[found tail]|] eqn:E; try discriminate.
    inversion H; subst; simpl. eapply IH; eauto.
Qed.

Lemma paired_append_preserves_slots : forall (left : list allowance_capture)
  (slots : list (option nat)) new_capture grant,
  length left = length slots ->
  (forall index amount, nth_error slots index = Some (Some amount) ->
    exists captured, nth_error left index = Some captured /\
      captured_amount captured = amount /\
      reservation_right (captured_consent captured) = grant) ->
  reservation_right (captured_consent new_capture) = grant ->
  forall index amount,
    nth_error (slots ++ [Some (captured_amount new_capture)]) index = Some (Some amount) ->
    exists captured, nth_error (left ++ [new_capture]) index = Some captured /\
      captured_amount captured = amount /\
      reservation_right (captured_consent captured) = grant.
Proof.
  intros left slots new_capture grant sizes aligned own index amount H.
  destruct (lt_dec index (length slots)) as [within|outside].
  - rewrite nth_error_app1 in H by assumption.
    destruct (aligned _ _ H) as [captured [found [quantity scope]]].
    exists captured. rewrite nth_error_app1 by lia. auto.
  - rewrite nth_error_app2 in H by lia.
    rewrite nth_error_app2 by lia. rewrite sizes.
    destruct (index - length slots); simpl in H; try discriminate.
    + inversion H; subst. exists new_capture. simpl. auto.
    + destruct n; discriminate.
Qed.

Theorem reserve_consented_preserves_validity : forall state expected root price amount valid next,
  consented_valid state ->
  reserve_consented state expected root price amount valid = Some next ->
  consented_valid next.
Proof.
  intros state expected root price amount valid next [conservation [sizes aligned]] H.
  unfold reserve_consented in H.
  destruct (required_price_ceiling (required_ceilings (current_terms state)));
    try discriminate.
  destruct (price <=? n); try discriminate.
  destruct (allowance_step (quantity_state state) (Reserve expected amount valid))
    as [quantity|] eqn:E; try discriminate.
  inversion H; subst. split.
  - simpl. eapply allowance_step_conserves; eauto.
  - unfold captures_match_slots; simpl.
    unfold allowance_step in E.
    destruct (valid && (expected =? authorization_version (quantity_state state)) &&
      (amount <=? available (quantity_state state))); inversion E; subst; simpl.
    split.
    + rewrite !length_app; simpl; lia.
    + exact (paired_append_preserves_slots _ _
        {| captured_consent := {|
             reservation_right := grant_identity state;
             reservation_generation := authorization_version (quantity_state state);
             reservation_terms := current_terms state;
             reservation_root := root; reservation_price := price |};
           captured_amount := amount |} _ sizes aligned eq_refl).
Qed.

Theorem reservation_checks_price_and_allowance : forall state expected root price amount valid next,
  reserve_consented state expected root price amount valid = Some next ->
  Forall (fun maximum => price <= maximum) (required_ceilings (current_terms state)) /\
  amount <= available (quantity_state state) /\
  expected = authorization_version (quantity_state state) /\ valid = true.
Proof.
  intros state expected root price amount valid next H.
  unfold reserve_consented in H.
  destruct (required_price_ceiling (required_ceilings (current_terms state)))
    as [ceiling|] eqn:C; try discriminate.
  destruct (price <=? ceiling) eqn:P; try discriminate.
  unfold allowance_step in H.
  destruct (valid && (expected =? authorization_version (quantity_state state)) &&
    (amount <=? available (quantity_state state))) eqn:B; try discriminate.
  apply andb_true_iff in B as [B bounded]. apply andb_true_iff in B as [auth version].
  apply Nat.leb_le in bounded. apply Nat.eqb_eq in version. apply Nat.leb_le in P.
  split.
  - apply (proj1 (required_price_ceiling_checks_every_consent _ _ _ C)). exact P.
  - auto.
Qed.

Theorem transfer_consented_preserves_validity : forall state expected terms auth next,
  consented_valid state -> transfer_consented state expected terms auth = Some next ->
  consented_valid next.
Proof.
  intros state expected terms auth next [conservation aligned] H.
  unfold transfer_consented in H.
  destruct (allowance_step (quantity_state state) (Transfer expected auth))
    as [quantity|] eqn:E; try discriminate.
  inversion H; subst. split.
  - simpl. eapply allowance_step_conserves; eauto.
  - pose proof (transfer_preserves_allowance_stocks _ _ _ _ E)
      as [_ [_ [_ [slots _]]]].
    unfold captures_match_slots in *; simpl. rewrite slots. exact aligned.
Qed.

Theorem transfer_consented_preserves_captures : forall state expected terms auth next,
  transfer_consented state expected terms auth = Some next -> captures next = captures state.
Proof.
  intros state expected terms auth next H. unfold transfer_consented in H.
  destruct (allowance_step (quantity_state state) (Transfer expected auth));
    inversion H; subst; reflexivity.
Qed.

Theorem settlement_matches_captured_amount : forall state index debit captured next,
  captures_match_slots state -> settle_consented state index debit = Some (captured, next) ->
  debit <= captured_amount captured /\
  available (quantity_state next) =
    available (quantity_state state) + (captured_amount captured - debit) /\
  consumed (quantity_state next) = consumed (quantity_state state) + debit.
Proof.
  intros state index debit captured next [_ aligned] H. unfold settle_consented in H.
  destruct (nth_error (captures state) index) as [record|] eqn:N; try discriminate.
  unfold settle_allowance in H.
  destruct (close_slot index (active_slots (quantity_state state)))
    as [[reserved slots]|] eqn:E; try discriminate.
  destruct (debit <=? reserved) eqn:B; try discriminate.
  apply Nat.leb_le in B.
  destruct (aligned _ _ (close_slot_reads_exact_amount _ _ _ _ E))
    as [same [found [amount _]]].
  rewrite N in found. inversion found; subst.
  inversion H; subst. simpl. repeat split; lia.
Qed.

Theorem settlement_preserves_consented_validity : forall state index debit captured next,
  consented_valid state -> settle_consented state index debit = Some (captured, next) ->
  consented_valid next.
Proof.
  intros state index debit captured next [conservation [sizes aligned]] H.
  unfold settle_consented in H.
  destruct (nth_error (captures state) index); try discriminate.
  destruct (settle_allowance (quantity_state state) index debit) as [quantity|] eqn:E;
    try discriminate.
  inversion H; subst. split.
  - simpl. eapply settle_allowance_conserves; eauto.
  - unfold settle_allowance in E.
    destruct (close_slot index (active_slots (quantity_state state)))
      as [[reserved slots]|] eqn:C; try discriminate.
    destruct (debit <=? reserved); inversion E; subst.
    unfold captures_match_slots; simpl. split.
    + rewrite (close_slot_preserves_length _ _ _ _ C). exact sizes.
    + intros other amount found. apply aligned.
      eapply close_slot_preserves_remaining; eauto.
Qed.

Theorem consented_settlement_cannot_repeat : forall state index debit captured next retry,
  settle_consented state index debit = Some (captured, next) ->
  settle_consented next index retry = None.
Proof.
  intros state index debit captured next retry H. unfold settle_consented in H.
  destruct (nth_error (captures state) index) eqn:N; try discriminate.
  destruct (settle_allowance (quantity_state state) index debit) as [quantity|] eqn:E;
    try discriminate.
  inversion H; subst. unfold settle_consented; simpl. rewrite N.
  rewrite (settlement_cannot_repeat _ _ _ _ retry E). reflexivity.
Qed.

Theorem expansion_preserves_consented_validity : forall state expected amount auth next,
  consented_valid state -> expand_consented state expected amount auth = Some next ->
  consented_valid next.
Proof.
  intros state expected amount auth next [conservation aligned] H.
  unfold expand_consented in H.
  destruct (allowance_step (quantity_state state) (Expand expected amount auth))
    as [quantity|] eqn:E; try discriminate.
  inversion H; subst. split.
  - simpl. eapply allowance_step_conserves; eauto.
  - unfold allowance_step in E.
    destruct (auth && (expected =? authorization_version (quantity_state state)));
      inversion E; subst; exact aligned.
Qed.

Inductive consented_command :=
| ReserveConsented (expected root price amount : nat) (valid : bool)
| SettleConsented (index debit : nat)
| TransferConsented (expected : nat) (terms : funding_terms) (auth : bool)
| ExpandConsented (expected amount : nat) (auth : bool)
| TopUpConsented (amount : nat).

Definition consented_step (state : consented_allowance) (command : consented_command)
  : option consented_allowance :=
  match command with
  | ReserveConsented expected root price amount valid =>
      reserve_consented state expected root price amount valid
  | SettleConsented index debit =>
      match settle_consented state index debit with
      | Some (_, next) => Some next
      | None => None
      end
  | TransferConsented expected terms auth => transfer_consented state expected terms auth
  | ExpandConsented expected amount auth => expand_consented state expected amount auth
  | TopUpConsented _ => Some state
  end.

Fixpoint consented_history (state : consented_allowance) (commands : list consented_command)
  : option consented_allowance :=
  match commands with
  | [] => Some state
  | command :: rest =>
      match consented_step state command with
      | None => None
      | Some next => consented_history next rest
      end
  end.

Theorem consented_step_preserves_validity : forall state command next,
  consented_valid state -> consented_step state command = Some next -> consented_valid next.
Proof.
  intros state command next Hvalid H. destruct command; simpl in H.
  - eapply reserve_consented_preserves_validity; eauto.
  - destruct (settle_consented state index debit) as [[captured after]|] eqn:E;
      inversion H; subst. eapply settlement_preserves_consented_validity; eauto.
  - eapply transfer_consented_preserves_validity; eauto.
  - eapply expansion_preserves_consented_validity; eauto.
  - inversion H; subst; exact Hvalid.
Qed.

Theorem consented_arbitrary_history_preserves_validity : forall commands state next,
  consented_valid state -> consented_history state commands = Some next -> consented_valid next.
Proof.
  induction commands as [|command rest IH]; intros state next Hvalid H; simpl in H.
  - inversion H; subst; exact Hvalid.
  - destruct (consented_step state command) as [middle|] eqn:E; try discriminate.
    apply (IH middle next).
    + exact (consented_step_preserves_validity _ _ _ Hvalid E).
    + exact H.
Qed.

Theorem consented_step_preserves_existing_capture : forall state command next index record,
  consented_step state command = Some next ->
  nth_error (captures state) index = Some record ->
  nth_error (captures next) index = Some record.
Proof.
  intros state command next index record H N. destruct command; simpl in H.
  - unfold reserve_consented in H.
    destruct (required_price_ceiling (required_ceilings (current_terms state)));
      try discriminate.
    destruct (price <=? n); try discriminate.
    destruct (allowance_step (quantity_state state) (Reserve expected amount valid));
      inversion H; subst; simpl.
    rewrite nth_error_app1; [exact N|].
    apply nth_error_Some. rewrite N. discriminate.
  - destruct (settle_consented state index0 debit) as [[captured after]|] eqn:E;
      inversion H; subst.
    unfold settle_consented in E.
    destruct (nth_error (captures state) index0); try discriminate.
    destruct (settle_allowance (quantity_state state) index0 debit); inversion E; subst.
    exact N.
  - rewrite (transfer_consented_preserves_captures _ _ _ _ _ H). exact N.
  - unfold expand_consented in H.
    destruct (allowance_step (quantity_state state) (Expand expected amount auth));
      inversion H; subst; exact N.
  - inversion H; subst; exact N.
Qed.

Theorem consented_arbitrary_history_preserves_captured_consent :
  forall commands state next index record,
  consented_history state commands = Some next ->
  nth_error (captures state) index = Some record ->
  nth_error (captures next) index = Some record.
Proof.
  induction commands as [|command rest IH]; intros state next index record H N; simpl in H.
  - inversion H; subst; exact N.
  - destruct (consented_step state command) as [middle|] eqn:E; try discriminate.
    apply (IH middle next index record H).
    exact (consented_step_preserves_existing_capture _ _ _ _ _ E N).
Qed.

Definition quantity_command (command : consented_command) : allowance_command :=
  match command with
  | ReserveConsented expected _ _ amount valid => Reserve expected amount valid
  | SettleConsented index debit => Settle index debit
  | TransferConsented expected _ auth => Transfer expected auth
  | ExpandConsented expected amount auth => Expand expected amount auth
  | TopUpConsented amount => WalletTopUp amount
  end.

Theorem consented_step_refines_quantity_step : forall state command next,
  consented_step state command = Some next ->
  allowance_step (quantity_state state) (quantity_command command) = Some (quantity_state next).
Proof.
  intros state command next H. destruct command; simpl in *.
  - unfold reserve_consented in H.
    destruct (required_price_ceiling (required_ceilings (current_terms state)));
      try discriminate.
    destruct (price <=? n); try discriminate.
    destruct (allowance_step (quantity_state state) (Reserve expected amount valid)) eqn:E;
      inversion H; subst. exact E.
  - destruct (settle_consented state index debit) as [[captured after]|] eqn:E;
      inversion H; subst.
    unfold settle_consented in E.
    destruct (nth_error (captures state) index); try discriminate.
    destruct (settle_allowance (quantity_state state) index debit) eqn:S;
      inversion E; subst. reflexivity.
  - unfold transfer_consented in H.
    destruct (allowance_step (quantity_state state) (Transfer expected auth)) eqn:E;
      inversion H; subst. exact E.
  - unfold expand_consented in H.
    destruct (allowance_step (quantity_state state) (Expand expected amount auth)) eqn:E;
      inversion H; subst. exact E.
  - inversion H; reflexivity.
Qed.

Theorem consented_history_refines_quantity_history : forall commands state next,
  consented_history state commands = Some next ->
  allowance_history (quantity_state state) (map quantity_command commands) =
    Some (quantity_state next).
Proof.
  induction commands as [|command rest IH]; intros state next H; simpl in *.
  - inversion H; reflexivity.
  - destruct (consented_step state command) as [middle|] eqn:E; try discriminate.
    rewrite (consented_step_refines_quantity_step _ _ _ E).
    exact (IH _ _ H).
Qed.

Theorem consented_history_never_resets_consumption : forall commands state next,
  consented_history state commands = Some next ->
  consumed (quantity_state state) <= consumed (quantity_state next).
Proof.
  intros commands state next H.
  eapply arbitrary_mixed_history_never_resets_consumption.
  exact (consented_history_refines_quantity_history _ _ _ H).
Qed.

Definition explicit_allowance_increase (command : consented_command) : nat :=
  match command with ExpandConsented _ amount _ => amount | _ => 0 end.

Theorem consented_step_changes_issuance_only_by_expansion : forall state command next,
  consented_step state command = Some next ->
  issued (quantity_state next) = issued (quantity_state state) + explicit_allowance_increase command.
Proof.
  intros state command next H.
  pose proof (consented_step_refines_quantity_step _ _ _ H) as Q.
  destruct command; simpl in Q |- *.
  - destruct (valid && (expected =? authorization_version (quantity_state state)) &&
      (amount <=? available (quantity_state state))); inversion Q; simpl; lia.
  - unfold settle_allowance in Q.
    destruct (close_slot index (active_slots (quantity_state state))) as [[reserved slots]|];
      try discriminate.
    destruct (debit <=? reserved); inversion Q; simpl; lia.
  - destruct (auth && (expected =? authorization_version (quantity_state state)));
      inversion Q; simpl; lia.
  - destruct (auth && (expected =? authorization_version (quantity_state state)));
      inversion Q; simpl; lia.
  - inversion Q; lia.
Qed.

Theorem consented_history_changes_issuance_only_by_expansion : forall commands state next,
  consented_history state commands = Some next ->
  issued (quantity_state next) = issued (quantity_state state) +
    fold_right (fun command total => explicit_allowance_increase command + total) 0 commands.
Proof.
  induction commands as [|command rest IH]; intros state next H; simpl in H |- *.
  - inversion H; lia.
  - destruct (consented_step state command) as [middle|] eqn:E; try discriminate.
    rewrite (IH _ _ H), (consented_step_changes_issuance_only_by_expansion _ _ _ E).
    lia.
Qed.

Theorem every_finite_history_has_explicit_consumption_bound : forall commands state next,
  consented_valid state -> consented_history state commands = Some next ->
  consumed (quantity_state next) <= issued (quantity_state state) +
    fold_right (fun command total => explicit_allowance_increase command + total) 0 commands.
Proof.
  intros commands state next Hvalid Hhistory.
  pose proof (consented_arbitrary_history_preserves_validity _ _ _ Hvalid Hhistory) as [C _].
  pose proof (consented_history_changes_issuance_only_by_expansion _ _ _ Hhistory) as I.
  unfold conserved in C. lia.
Qed.

Theorem consented_step_preserves_grant_identity : forall state command next,
  consented_step state command = Some next -> grant_identity next = grant_identity state.
Proof.
  intros state command next H. destruct command; simpl in H.
  - unfold reserve_consented in H.
    destruct (required_price_ceiling (required_ceilings (current_terms state))); try discriminate.
    destruct (price <=? n); try discriminate.
    destruct (allowance_step (quantity_state state) (Reserve expected amount valid));
      inversion H; reflexivity.
  - destruct (settle_consented state index debit) as [[record after]|] eqn:E;
      inversion H; subst.
    unfold settle_consented in E.
    destruct (nth_error (captures state) index); try discriminate.
    destruct (settle_allowance (quantity_state state) index debit); inversion E; reflexivity.
  - unfold transfer_consented in H.
    destruct (allowance_step (quantity_state state) (Transfer expected auth));
      inversion H; reflexivity.
  - unfold expand_consented in H.
    destruct (allowance_step (quantity_state state) (Expand expected amount auth));
      inversion H; reflexivity.
  - inversion H; reflexivity.
Qed.

Theorem consented_history_preserves_grant_identity : forall commands state next,
  consented_history state commands = Some next -> grant_identity next = grant_identity state.
Proof.
  induction commands as [|command rest IH]; intros state next H; simpl in H.
  - inversion H; reflexivity.
  - destruct (consented_step state command) as [middle|] eqn:E; try discriminate.
    rewrite (IH _ _ H). exact (consented_step_preserves_grant_identity _ _ _ E).
Qed.

Theorem initial_consented_valid : forall grant amount terms,
  consented_valid (initial_consented_allowance grant amount terms).
Proof.
  intros. split.
  - unfold conserved; simpl; lia.
  - split; [reflexivity|]. intros index quantity H. destruct index; discriminate.
Qed.

Definition captured_price_authorized (record : allowance_capture) : Prop :=
  exists ceiling,
    required_price_ceiling
      (required_ceilings (reservation_terms (captured_consent record))) = Some ceiling /\
    reservation_price (captured_consent record) <= ceiling.

Theorem consented_step_preserves_price_authorization : forall state command next,
  Forall captured_price_authorized (captures state) ->
  consented_step state command = Some next ->
  Forall captured_price_authorized (captures next).
Proof.
  intros state command next authorized H. destruct command; simpl in H.
  - unfold reserve_consented in H.
    destruct (required_price_ceiling (required_ceilings (current_terms state)))
      as [ceiling|] eqn:C; try discriminate.
    destruct (price <=? ceiling) eqn:P; try discriminate.
    destruct (allowance_step (quantity_state state) (Reserve expected amount valid));
      inversion H; subst; simpl.
    apply Forall_app. split; [exact authorized|]. constructor; [|constructor].
    exists ceiling. simpl. split; [exact C|]. apply Nat.leb_le. exact P.
  - destruct (settle_consented state index debit) as [[record after]|] eqn:E;
      inversion H; subst.
    unfold settle_consented in E.
    destruct (nth_error (captures state) index); try discriminate.
    destruct (settle_allowance (quantity_state state) index debit);
      inversion E; subst; exact authorized.
  - rewrite (transfer_consented_preserves_captures _ _ _ _ _ H). exact authorized.
  - unfold expand_consented in H.
    destruct (allowance_step (quantity_state state) (Expand expected amount auth));
      inversion H; subst; exact authorized.
  - inversion H; subst; exact authorized.
Qed.

Theorem arbitrary_history_retains_price_authorization : forall commands state next,
  Forall captured_price_authorized (captures state) ->
  consented_history state commands = Some next ->
  Forall captured_price_authorized (captures next).
Proof.
  induction commands as [|command rest IH]; intros state next authorized H; simpl in H.
  - inversion H; subst; exact authorized.
  - destruct (consented_step state command) as [middle|] eqn:E; try discriminate.
    apply (IH middle next).
    + exact (consented_step_preserves_price_authorization _ _ _ authorized E).
    + exact H.
Qed.

Theorem captured_price_satisfies_every_required_owner : forall record,
  captured_price_authorized record ->
  Forall (fun ceiling => reservation_price (captured_consent record) <= ceiling)
    (required_ceilings (reservation_terms (captured_consent record))).
Proof.
  intros record [ceiling [computed bounded]].
  apply (proj1 (required_price_ceiling_checks_every_consent _ _ _ computed)).
  exact bounded.
Qed.

Definition example_original_terms : funding_terms :=
  {| required_owner_consents := [(1, 3); (2, 5); (3, 8)];
     signed_limit_terms := []; asset_identity := 1; schedule_identity := 1 |}.

Definition example_replacement_terms : funding_terms :=
  {| required_owner_consents := [(4, 1); (5, 2); (6, 3); (7, 4)];
     signed_limit_terms := []; asset_identity := 1; schedule_identity := 2 |}.

Example lower_owner_ceiling_rejects_even_with_available_allowance :
  reserve_consented (initial_consented_allowance 7 100 example_original_terms)
    0 8 4 10 true = None.
Proof. reflexivity. Qed.

Example valid_price_does_not_replace_allowance_backing :
  reserve_consented (initial_consented_allowance 7 1 example_original_terms)
    0 8 2 10 true = None.
Proof. reflexivity. Qed.

Example new_owner_price_does_not_reprice_existing_reservation :
  match reserve_consented (initial_consented_allowance 7 100 example_original_terms)
      0 8 2 10 true with
  | None => None
  | Some reserved => match transfer_consented reserved 0 example_replacement_terms true with
    | None => None
    | Some transferred => match settle_consented transferred 0 6 with
      | None => None
      | Some (record, next) => Some
          (reservation_price (captured_consent record),
           required_ceilings (reservation_terms (captured_consent record)),
           available (quantity_state next), consumed (quantity_state next))
      end
    end
  end = Some (2, [3; 5; 8], 94, 6).
Proof. reflexivity. Qed.

Fixpoint paired_captures_valid (grant : nat) (records : list allowance_capture)
  (slots : list (option nat)) : bool :=
  match records, slots with
  | [], [] => true
  | record :: rest, slot :: tail =>
      (reservation_right (captured_consent record) =? grant) &&
      (match slot with
       | None => true
       | Some amount => captured_amount record =? amount
       end) && paired_captures_valid grant rest tail
  | _, _ => false
  end.

Definition consented_observation_valid (state : consented_allowance) : bool :=
  allowance_observation_valid (quantity_state state) &&
  paired_captures_valid (grant_identity state) (captures state)
    (active_slots (quantity_state state)) &&
  forallb (fun record =>
    let consent := captured_consent record in
    match required_price_ceiling (required_ceilings (reservation_terms consent)) with
    | None => false
    | Some ceiling => reservation_price consent <=? ceiling
    end) (captures state).

Definition generated_consented_commands : list consented_command :=
  [ReserveConsented 0 8 2 1 true;
   ReserveConsented 0 8 4 1 true;
   ReserveConsented 0 8 2 3 true;
   ReserveConsented 0 8 2 1 false;
   ReserveConsented 1 9 1 1 true;
   SettleConsented 0 0; SettleConsented 0 1; SettleConsented 0 3;
   SettleConsented 1 1;
   TransferConsented 0 example_replacement_terms true;
   TransferConsented 1 example_original_terms true;
   TransferConsented 0 example_replacement_terms false;
   ExpandConsented 0 1 true; ExpandConsented 1 1 true;
   TopUpConsented 100].

Fixpoint check_consented_histories (depth : nat) (state : consented_allowance) : bool :=
  consented_observation_valid state &&
  match depth with
  | 0 => true
  | S rest => forallb (fun command =>
      match consented_step state command with
      | None => check_consented_histories rest state
      | Some next => check_consented_histories rest next
      end) generated_consented_commands
  end.

Example generated_consented_funded_histories :
  check_consented_histories 4 (initial_consented_allowance 7 2 example_original_terms) = true.
Proof. vm_compute. reflexivity. Qed.

Example generated_consented_empty_histories :
  check_consented_histories 4 (initial_consented_allowance 7 0 example_original_terms) = true.
Proof. vm_compute. reflexivity. Qed.

Example returned_owner_cannot_reuse_original_authorization :
  consented_history (initial_consented_allowance 7 100 example_original_terms)
    [TransferConsented 0 example_replacement_terms true;
     TransferConsented 1 example_original_terms true;
     ReserveConsented 0 8 2 10 true] = None.
Proof. reflexivity. Qed.

Example new_draw_requires_new_owner_price_consent :
  consented_history (initial_consented_allowance 7 100 example_original_terms)
    [TransferConsented 0 example_replacement_terms true;
     ReserveConsented 1 8 2 10 true] = None.
Proof. reflexivity. Qed.
