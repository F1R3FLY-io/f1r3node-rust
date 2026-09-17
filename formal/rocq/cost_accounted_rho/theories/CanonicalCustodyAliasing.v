From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section CanonicalCustodyAliasing.

Context {lane purse : Type}.
Context (purse_eq_dec : forall left right : purse, {left = right} + {left <> right}).

Definition lane_debit := lane -> nat.
Definition purse_balance := purse -> nat.
Definition custody_plan := lane -> purse.

Definition physical_draw
  (lanes : list lane)
  (purse_of : custody_plan)
  (draw : lane_debit)
  (selected : purse)
  : nat :=
  fold_right
    (fun current total =>
       if purse_eq_dec (purse_of current) selected
       then draw current + total
       else total)
    0
    lanes.

Definition physically_admissible
  (lanes : list lane)
  (purse_of : custody_plan)
  (balance : purse_balance)
  (draw : lane_debit)
  : Prop :=
  forall selected, physical_draw lanes purse_of draw selected <= balance selected.

Definition settled_balance
  (lanes : list lane)
  (purse_of : custody_plan)
  (balance : purse_balance)
  (draw : lane_debit)
  : purse_balance :=
  fun selected => balance selected - physical_draw lanes purse_of draw selected.

Definition physical_conservation
  (lanes : list lane)
  (purse_of : custody_plan)
  (initial final : purse_balance)
  (draw : lane_debit)
  : Prop :=
  forall selected,
    final selected + physical_draw lanes purse_of draw selected = initial selected.

Definition per_lane_admissible
  (lanes : list lane)
  (purse_of : custody_plan)
  (balance : purse_balance)
  (draw : lane_debit)
  : Prop :=
  forall current,
    In current lanes -> draw current <= balance (purse_of current).

Theorem physically_admissible_prevents_double_capacity :
  forall lanes purse_of balance draw selected,
    physically_admissible lanes purse_of balance draw ->
    physical_draw lanes purse_of draw selected <= balance selected.
Proof.
  intros lanes purse_of balance draw selected admissible.
  apply admissible.
Qed.

Theorem physical_settlement_conserves :
  forall lanes purse_of balance draw,
    physically_admissible lanes purse_of balance draw ->
    physical_conservation
      lanes
      purse_of
      balance
      (settled_balance lanes purse_of balance draw)
      draw.
Proof.
  intros lanes purse_of balance draw admissible selected.
  unfold settled_balance.
  specialize (admissible selected).
  lia.
Qed.

Theorem physical_draw_is_permutation_invariant :
  forall left right purse_of draw selected,
    Permutation left right ->
    physical_draw left purse_of draw selected =
    physical_draw right purse_of draw selected.
Proof.
  intros left right purse_of draw selected permutation.
  induction permutation.
  - reflexivity.
  - simpl.
    destruct (purse_eq_dec (purse_of x) selected); now rewrite IHpermutation.
  - simpl.
    destruct (purse_eq_dec (purse_of x) selected);
      destruct (purse_eq_dec (purse_of y) selected); lia.
  - now rewrite IHpermutation1, IHpermutation2.
Qed.

Definition stack_admissible
  (available draw : lane_debit)
  : Prop :=
  forall current, draw current <= available current.

Definition settled_stack
  (available draw : lane_debit)
  : lane_debit :=
  fun current => available current - draw current.

Definition stack_conservation
  (initial final draw : lane_debit)
  : Prop :=
  forall current, final current + draw current = initial current.

Theorem stack_settlement_preserves_authority :
  forall available draw,
    stack_admissible available draw ->
    stack_conservation available (settled_stack available draw) draw.
Proof.
  intros available draw admissible current.
  unfold settled_stack.
  specialize (admissible current).
  lia.
Qed.

Theorem physical_alias_does_not_merge_stack_authority :
  forall available draw left right (purse_of : custody_plan),
    left <> right ->
    purse_of left = purse_of right ->
    draw right = 0 ->
    settled_stack available draw right = available right.
Proof.
  intros available draw left right purse_of distinct aliased absent.
  unfold settled_stack.
  now rewrite absent, Nat.sub_0_r.
Qed.

Lemma physical_draw_append : forall left right purse_of draw selected,
  physical_draw (left ++ right) purse_of draw selected =
  physical_draw left purse_of draw selected + physical_draw right purse_of draw selected.
Proof.
  induction left as [|current rest IH]; intros; simpl; [reflexivity|].
  destruct (purse_eq_dec (purse_of current) selected); rewrite IH; lia.
Qed.

Theorem physical_reservation_batches_use_remaining_capacity :
  forall first second purse_of balance draw,
  physically_admissible first purse_of balance draw ->
  (physically_admissible (first ++ second) purse_of balance draw <->
   physically_admissible second purse_of
     (settled_balance first purse_of balance draw) draw).
Proof.
  intros first second purse_of balance draw admitted. split; intros H selected;
    specialize (admitted selected); specialize (H selected);
    unfold settled_balance in *; rewrite physical_draw_append in *; lia.
Qed.

Theorem physical_refund_matches_original_custody :
  forall lanes purse_of maximum actual selected,
  (forall current, In current lanes -> actual current <= maximum current) ->
  physical_draw lanes purse_of (fun current => maximum current - actual current) selected +
  physical_draw lanes purse_of actual selected =
  physical_draw lanes purse_of maximum selected.
Proof.
  induction lanes as [|current rest IH]; intros purse_of maximum actual selected bounded;
    [reflexivity|].
  assert (tail_bound : forall item, In item rest -> actual item <= maximum item).
  { intros item member. apply bounded. right. exact member. }
  specialize (IH purse_of maximum actual selected tail_bound).
  assert (head_bound : actual current <= maximum current).
  { apply bounded. left. reflexivity. }
  simpl. destruct (purse_eq_dec (purse_of current) selected); lia.
Qed.

Theorem physical_actual_cannot_exceed_reservation :
  forall lanes purse_of maximum actual selected,
  (forall current, In current lanes -> actual current <= maximum current) ->
  physical_draw lanes purse_of actual selected <= physical_draw lanes purse_of maximum selected.
Proof.
  intros lanes purse_of maximum actual selected bounded.
  pose proof (physical_refund_matches_original_custody lanes purse_of maximum actual selected bounded). lia.
Qed.

Theorem physical_reserve_then_refund_conserves :
  forall lanes purse_of balance maximum actual selected,
  physically_admissible lanes purse_of balance maximum ->
  (forall current, In current lanes -> actual current <= maximum current) ->
  settled_balance lanes purse_of balance maximum selected +
  physical_draw lanes purse_of (fun current => maximum current - actual current) selected +
  physical_draw lanes purse_of actual selected = balance selected.
Proof.
  intros lanes purse_of balance maximum actual selected admitted bounded.
  specialize (admitted selected). unfold settled_balance.
  pose proof (physical_refund_matches_original_custody lanes purse_of maximum actual selected bounded). lia.
Qed.

Lemma physical_draw_additive : forall lanes purse_of left right selected,
  physical_draw lanes purse_of (fun current => left current + right current) selected =
  physical_draw lanes purse_of left selected + physical_draw lanes purse_of right selected.
Proof.
  unfold physical_draw.
  induction lanes as [|current lanes IH]; intros; cbn; [reflexivity|].
  destruct (purse_eq_dec (purse_of current) selected); rewrite IH; lia.
Qed.

Lemma physical_draw_zero : forall lanes purse_of selected,
  physical_draw lanes purse_of (fun _ => 0) selected = 0.
Proof.
  induction lanes as [|current lanes IH]; intros; cbn; [reflexivity|].
  destruct (purse_eq_dec (purse_of current) selected); exact (IH purse_of selected).
Qed.

Theorem two_settlements_use_remaining_capacity :
  forall first second purse_of balance first_draw second_draw,
  physically_admissible first purse_of balance first_draw ->
  ((forall selected,
      physical_draw first purse_of first_draw selected +
      physical_draw second purse_of second_draw selected <= balance selected) <->
   physically_admissible second purse_of
     (settled_balance first purse_of balance first_draw) second_draw).
Proof.
  intros first second purse_of balance first_draw second_draw admitted.
  split; intros bounded selected; specialize (bounded selected);
    specialize (admitted selected); unfold settled_balance in *; lia.
Qed.

Theorem jointly_backed_settlements_commute :
  forall first second purse_of balance first_draw second_draw,
  (forall selected,
    physical_draw first purse_of first_draw selected +
    physical_draw second purse_of second_draw selected <= balance selected) ->
  physically_admissible first purse_of balance first_draw /\
  physically_admissible second purse_of balance second_draw /\
  physically_admissible second purse_of
    (settled_balance first purse_of balance first_draw) second_draw /\
  physically_admissible first purse_of
    (settled_balance second purse_of balance second_draw) first_draw /\
  (forall selected,
    settled_balance second purse_of
      (settled_balance first purse_of balance first_draw) second_draw selected =
    settled_balance first purse_of
      (settled_balance second purse_of balance second_draw) first_draw selected) /\
  (forall selected,
    settled_balance second purse_of
      (settled_balance first purse_of balance first_draw) second_draw selected +
    physical_draw first purse_of first_draw selected +
    physical_draw second purse_of second_draw selected = balance selected).
Proof.
  intros first second purse_of balance first_draw second_draw jointly_backed.
  unfold physically_admissible, settled_balance.
  repeat split; intros selected; specialize (jointly_backed selected); lia.
Qed.

Section OccurrenceProjection.

Context (lane_eq_dec : forall left right : lane, {left = right} + {left <> right}).
Context {occurrence : Type}.

Definition single_lane_draw (target : lane) (amount : nat) : lane_debit :=
  fun current => if lane_eq_dec current target then amount else 0.

Fixpoint aggregate_occurrences (events : list occurrence)
  (lane_of : occurrence -> lane) (amount : occurrence -> nat) : lane_debit :=
  match events with
  | [] => fun _ => 0
  | event :: rest => fun current =>
      single_lane_draw (lane_of event) (amount event) current +
      aggregate_occurrences rest lane_of amount current
  end.

Lemma absent_lane_has_zero_draw : forall lanes purse_of target amount selected,
  ~ In target lanes ->
  physical_draw lanes purse_of (single_lane_draw target amount) selected = 0.
Proof.
  induction lanes as [|current lanes IH]; intros purse_of target amount selected absent;
    cbn; [reflexivity|].
  assert (current <> target) as different by (intro same; subst; apply absent; now left).
  assert (~ In target lanes) as absent_tail by (intro present; apply absent; now right).
  destruct (purse_eq_dec (purse_of current) selected).
  - unfold single_lane_draw. destruct (lane_eq_dec current target); [contradiction|].
    cbn. now apply IH.
  - now apply IH.
Qed.

Lemma one_lane_is_projected_once : forall lanes purse_of target amount selected,
  NoDup lanes -> In target lanes ->
  physical_draw lanes purse_of (single_lane_draw target amount) selected =
    if purse_eq_dec (purse_of target) selected then amount else 0.
Proof.
  induction lanes as [|current lanes IH]; intros purse_of target amount selected unique present;
    [contradiction|].
  inversion unique as [|head tail absent unique_tail]; subst.
  destruct present as [same|present].
  - subst target. simpl.
    rewrite absent_lane_has_zero_draw by exact absent.
    unfold single_lane_draw. destruct (lane_eq_dec current current); [|contradiction].
    destruct (purse_eq_dec (purse_of current) selected); lia.
  - assert (current <> target) as different by (intro same; subst; contradiction).
    simpl. rewrite IH by assumption.
    unfold single_lane_draw. destruct (lane_eq_dec current target); [contradiction|].
    destruct (purse_eq_dec (purse_of current) selected); cbn; reflexivity.
Qed.

Theorem occurrence_aggregation_preserves_physical_draw :
  forall events lanes purse_of lane_of amount selected,
  NoDup lanes ->
  (forall event, In event events -> In (lane_of event) lanes) ->
  physical_draw lanes purse_of (aggregate_occurrences events lane_of amount) selected =
  fold_right
    (fun event total =>
      if purse_eq_dec (purse_of (lane_of event)) selected
      then amount event + total else total) 0 events.
Proof.
  induction events as [|event events IH]; intros lanes purse_of lane_of amount selected unique covered.
  - apply physical_draw_zero.
  - cbn [aggregate_occurrences fold_right]. rewrite physical_draw_additive.
    rewrite one_lane_is_projected_once; [|exact unique|apply covered; now left].
    rewrite IH; [|exact unique|intros other present; apply covered; now right].
    destruct (purse_eq_dec (purse_of (lane_of event)) selected); lia.
Qed.

Theorem occurrence_aggregation_preserves_capacity :
  forall events lanes purse_of lane_of amount balance,
  NoDup lanes ->
  (forall event, In event events -> In (lane_of event) lanes) ->
  (physically_admissible lanes purse_of balance (aggregate_occurrences events lane_of amount) <->
   forall selected,
   fold_right
    (fun event total =>
      if purse_eq_dec (purse_of (lane_of event)) selected
      then amount event + total else total) 0 events <= balance selected).
Proof.
  intros events lanes purse_of lane_of amount balance unique covered.
  unfold physically_admissible.
  split; intros bound selected; specialize (bound selected);
    rewrite occurrence_aggregation_preserves_physical_draw in * by assumption; exact bound.
Qed.

End OccurrenceProjection.

End CanonicalCustodyAliasing.

Definition alias_purse (_ : bool) : unit := tt.
Definition alias_balance (_ : unit) : nat := 1.
Definition alias_draw (_ : bool) : nat := 1.

Definition unit_eq_dec (left right : unit) : {left = right} + {left <> right}.
Proof.
  decide equality.
Defined.

Example per_lane_capacity_can_double_count_one_physical_purse :
  per_lane_admissible [true; false] alias_purse alias_balance alias_draw /\
  ~ physically_admissible unit_eq_dec [true; false] alias_purse alias_balance alias_draw.
Proof.
  split.
  - intros current present.
    destruct current; reflexivity.
  - intro admissible.
    specialize (admissible tt).
    cbv [physical_draw alias_purse alias_balance alias_draw unit_eq_dec] in admissible.
    exact (Nat.nle_succ_diag_l 1 admissible).
Qed.

Example repeated_occurrences_are_not_deduplicated :
  aggregate_occurrences Nat.eq_dec [tt; tt] (fun _ => 0) (fun _ => 1) 0 = 2 /\
  physical_draw unit_eq_dec [0]
    (fun _ => tt)
    (aggregate_occurrences Nat.eq_dec [tt; tt] (fun _ => 0) (fun _ => 1)) tt = 2.
Proof.
  split; reflexivity.
Qed.

Example repeating_aggregated_lane_keys_double_counts :
  physical_draw unit_eq_dec [0; 0]
    (fun _ => tt)
    (aggregate_occurrences Nat.eq_dec [tt; tt] (fun _ => 0) (fun _ => 1)) tt = 4.
Proof.
  reflexivity.
Qed.

Example independently_admitted_snapshots_can_overdraw :
  physically_admissible unit_eq_dec [true] alias_purse alias_balance alias_draw /\
  physically_admissible unit_eq_dec [false] alias_purse alias_balance alias_draw /\
  ~ physically_admissible unit_eq_dec [false] alias_purse
      (settled_balance unit_eq_dec [true] alias_purse alias_balance alias_draw) alias_draw.
Proof.
  split; [intros []; cbv; lia|].
  split; [intros []; cbv; lia|].
  intro admitted. specialize (admitted tt).
  change (1 <= 0) in admitted.
  lia.
Qed.

Print Assumptions physically_admissible_prevents_double_capacity.
Print Assumptions physical_settlement_conserves.
Print Assumptions physical_draw_is_permutation_invariant.
Print Assumptions stack_settlement_preserves_authority.
Print Assumptions physical_alias_does_not_merge_stack_authority.
Print Assumptions per_lane_capacity_can_double_count_one_physical_purse.

Section DirectWalletAuthorization.
Context {principal custody : Type}.
Context (custody_eq : forall left right : custody, {left = right} + {left <> right}).
Context (wallet_of : principal -> custody).

Definition direct_wallets_authorized (selected : list principal) (requested : list custody) : bool :=
  forallb (fun target => existsb
    (fun owner => if custody_eq (wallet_of owner) target then true else false) selected) requested.

Theorem direct_wallet_authorization_exact : forall selected requested,
  direct_wallets_authorized selected requested = true <->
  forall target, In target requested ->
    exists owner, In owner selected /\ wallet_of owner = target.
Proof.
  intros selected requested. unfold direct_wallets_authorized.
  rewrite forallb_forall. split; intros accepted target present.
  - specialize (accepted target present). apply existsb_exists in accepted.
    destruct accepted as [owner [member equal]].
    destruct (custody_eq (wallet_of owner) target); try discriminate.
    exists owner. auto.
  - destruct (accepted target present) as [owner [member equal]].
    apply existsb_exists. exists owner. split; auto.
    destruct (custody_eq (wallet_of owner) target); congruence.
Qed.

Theorem direct_wallet_authorization_rejects_unsigned_custody : forall selected requested target,
  In target requested ->
  (forall owner, In owner selected -> wallet_of owner <> target) ->
  direct_wallets_authorized selected requested = false.
Proof.
  intros selected requested target present absent.
  destruct (direct_wallets_authorized selected requested) eqn:accepted; auto.
  destruct (proj1 (direct_wallet_authorization_exact selected requested) accepted target present)
    as [owner [member equal]].
  exfalso. exact (absent owner member equal).
Qed.

Theorem direct_wallet_authorization_permutation : forall selected other requested reordered,
  Permutation selected other -> Permutation requested reordered ->
  direct_wallets_authorized selected requested = direct_wallets_authorized other reordered.
Proof.
  intros selected other requested reordered owners targets.
  assert (equivalent : direct_wallets_authorized selected requested = true <->
    direct_wallets_authorized other reordered = true).
  { rewrite !direct_wallet_authorization_exact. split; intros accepted target present.
    - apply (Permutation_in target (Permutation_sym targets)) in present.
      destruct (accepted target present) as [owner [member equal]].
      exists owner. split; [eapply Permutation_in; eauto|exact equal].
    - apply (Permutation_in target targets) in present.
      destruct (accepted target present) as [owner [member equal]].
      exists owner. split; [eapply Permutation_in; [apply Permutation_sym; exact owners|exact member]|exact equal]. }
  destruct (direct_wallets_authorized selected requested),
    (direct_wallets_authorized other reordered); intuition discriminate.
Qed.
End DirectWalletAuthorization.

Print Assumptions direct_wallet_authorization_exact.
Print Assumptions direct_wallet_authorization_rejects_unsigned_custody.
Print Assumptions direct_wallet_authorization_permutation.

Section FundingSnapshotRows.
Context {row : Type}.
Context (row_eq : forall left right : row, {left = right} + {left <> right}).

Definition snapshot_rows_match (snapshot family : list row) : bool :=
  direct_wallets_authorized row_eq (fun source => source) snapshot family.

Theorem snapshot_rows_match_exact : forall snapshot family,
  snapshot_rows_match snapshot family = true <-> incl family snapshot.
Proof.
  intros snapshot family. unfold snapshot_rows_match.
  rewrite direct_wallet_authorization_exact. unfold incl.
  split; intros accepted source present.
  - destruct (accepted source present) as [original [member equal]].
    subst. exact member.
  - exists source. auto.
Qed.

Theorem snapshot_rows_reject_substitution : forall snapshot family source,
  In source family -> ~ In source snapshot -> snapshot_rows_match snapshot family = false.
Proof.
  intros snapshot family source present absent.
  destruct (snapshot_rows_match snapshot family) eqn:accepted; auto.
  exfalso. apply absent.
  apply (proj1 (snapshot_rows_match_exact snapshot family) accepted source present).
Qed.

Theorem snapshot_rows_permutation : forall snapshot other family reordered,
  Permutation snapshot other -> Permutation family reordered ->
  snapshot_rows_match snapshot family = snapshot_rows_match other reordered.
Proof.
  intros. unfold snapshot_rows_match.
  apply direct_wallet_authorization_permutation; assumption.
Qed.
End FundingSnapshotRows.

Print Assumptions snapshot_rows_match_exact.
Print Assumptions snapshot_rows_reject_substitution.
Print Assumptions snapshot_rows_permutation.
