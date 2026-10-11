(* G1-4b (DR-123): soundness of the bound kind of the check before execution.

   The check of design section 6 turns the estimate B of an offered deploy
   into a decision before any runtime entry.
   - An estimate above the signed limit rejects the offer.
   - A complete estimate binds the bound source Certificate with the bound B
     and holds B * phloPrice + 1.
   - An incomplete estimate binds SignedLimit with the signed limit and holds
     phloLimit * phloPrice + 1. Its remainder fills the sources in custody
     order completely, or the offer is rejected.
   An estimate is complete only when the analysis is complete and no signed
   source is an on-chain payer (review LOW 5). A run that passes a certified
   bound is a certificate failure: it rolls back with no effect and pays
   nothing.

   The module reuses the bound source, the guard of the execution contract
   and the unbilled certificate failure of SignedLimitExhaustion. Four
   negative controls refute plausible wrong rules.

   G1-6 (DR-124) adds the client lanes: the check holds only the demand of
   the lanes whose purses a source of the envelope is located at. A certified
   estimate has demand on client lanes only, a foreign lane with demand makes
   the estimate incomplete, and the limit test reads the client's demand. A
   fifth negative control keeps an estimate with a foreign lane complete, and
   its run passes the certified bound unpaid. *)

From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import SignedPhloControls SignedLimitExhaustion.
Import ListNotations.

(** * The estimate, the offer and the decision *)

Record estimate := {
  analysis_complete : bool;
  onchain_payer : bool;
  estimate_phlo : nat;
}.

(** Completeness of an estimate (design section 5.3, review LOW 5). *)
Definition complete (e : estimate) : bool :=
  analysis_complete e && negb (onchain_payer e).

Record offer := {
  offer_limit : nat;
  offer_price : nat;
}.

Inductive decision :=
| Reject
| Bind (source : bound_source) (bound hold : nat).

Definition hold_of (bound price : nat) : nat := bound * price + 1.

(** The check of design section 6. *)
Definition check (e : estimate) (o : offer) : decision :=
  if offer_limit o <? estimate_phlo e then Reject
  else if complete e
       then Bind CertificateSource (estimate_phlo e) (hold_of (estimate_phlo e) (offer_price o))
       else Bind SignedLimitSource (offer_limit o) (hold_of (offer_limit o) (offer_price o)).

(** * The classification *)

Theorem classification_conservative : forall e o bound hold,
  check e o = Bind CertificateSource bound hold -> complete e = true.
Proof.
  intros e o bound hold checked. unfold check in checked.
  destruct (offer_limit o <? estimate_phlo e); [discriminate|].
  destruct (complete e); [reflexivity|discriminate].
Qed.

Theorem onchain_payer_is_never_certified : forall e o bound hold,
  onchain_payer e = true -> check e o <> Bind CertificateSource bound hold.
Proof.
  intros e o bound hold onchain checked.
  pose proof (classification_conservative e o bound hold checked) as completed.
  unfold complete in completed. rewrite onchain, andb_false_r in completed.
  discriminate.
Qed.

Theorem estimate_above_limit_rejected : forall e o,
  offer_limit o < estimate_phlo e -> check e o = Reject.
Proof.
  intros e o above. unfold check.
  apply Nat.ltb_lt in above. now rewrite above.
Qed.

Theorem complete_hold_is_exact : forall e o,
  complete e = true -> estimate_phlo e <= offer_limit o ->
  check e o = Bind CertificateSource (estimate_phlo e) (estimate_phlo e * offer_price o + 1).
Proof.
  intros e o completed within. unfold check, hold_of.
  assert (not_above : (offer_limit o <? estimate_phlo e) = false)
    by (apply Nat.ltb_ge; exact within).
  now rewrite not_above, completed.
Qed.

Theorem incomplete_hold_is_full_limit : forall e o,
  complete e = false -> estimate_phlo e <= offer_limit o ->
  check e o = Bind SignedLimitSource (offer_limit o) (offer_limit o * offer_price o + 1).
Proof.
  intros e o incomplete within. unfold check, hold_of.
  assert (not_above : (offer_limit o <? estimate_phlo e) = false)
    by (apply Nat.ltb_ge; exact within).
  now rewrite not_above, incomplete.
Qed.

(** Every bound that the check binds passes the guard of the execution
    contract. *)
Theorem bound_is_admissible : forall e o source bound hold,
  check e o = Bind source bound hold -> admissible source bound (offer_limit o) = true.
Proof.
  intros e o source bound hold checked. unfold check in checked.
  destruct (offer_limit o <? estimate_phlo e) eqn:above; [discriminate|].
  apply Nat.ltb_ge in above.
  destruct (complete e); inversion checked; subst; simpl.
  - now apply Nat.leb_le.
  - apply Nat.eqb_refl.
Qed.

(** * The remainder fill in custody order *)

(** [fill rooms left] places [left] atomic units over the rooms in custody
    order. It returns the part of each room and the units that stay
    unplaced. *)
Fixpoint fill (rooms : list nat) (left : nat) : list nat * nat :=
  match rooms with
  | [] => ([], left)
  | room :: rest =>
      let take := Nat.min room left in
      let (parts, unplaced) := fill rest (left - take) in
      (take :: parts, unplaced)
  end.

Fixpoint total (values : list nat) : nat :=
  match values with
  | [] => 0
  | value :: rest => value + total rest
  end.

(** The check places the remainder completely or rejects the offer. *)
Definition place_remainder (rooms : list nat) (remainder : nat) : option (list nat) :=
  let (parts, unplaced) := fill rooms remainder in
  if unplaced =? 0 then Some parts else None.

Lemma fill_parts_within_rooms : forall rooms left,
  Forall2 le (fst (fill rooms left)) rooms.
Proof.
  induction rooms as [|room rest IH]; intros left; simpl.
  - constructor.
  - specialize (IH (left - Nat.min room left)).
    destruct (fill rest (left - Nat.min room left)) as [parts unplaced].
    simpl in *. constructor; [apply Nat.le_min_l | exact IH].
Qed.

Lemma fill_conserves : forall rooms left,
  total (fst (fill rooms left)) + snd (fill rooms left) = left.
Proof.
  induction rooms as [|room rest IH]; intros left; simpl.
  - reflexivity.
  - specialize (IH (left - Nat.min room left)).
    destruct (fill rest (left - Nat.min room left)) as [parts unplaced].
    simpl in *. pose proof (Nat.le_min_r room left). lia.
Qed.

Lemma fill_unplaced_iff : forall rooms left,
  snd (fill rooms left) = 0 <-> left <= total rooms.
Proof.
  induction rooms as [|room rest IH]; intros left; simpl.
  - split; intro; lia.
  - specialize (IH (left - Nat.min room left)).
    destruct (fill rest (left - Nat.min room left)) as [parts unplaced].
    simpl in *. rewrite IH.
    destruct (Nat.min_spec room left) as [[smaller same] | [larger same]];
      rewrite same; split; intro; lia.
Qed.

Theorem custody_fill_complete_or_reject : forall rooms remainder,
  (exists parts, place_remainder rooms remainder = Some parts /\
     Forall2 le parts rooms /\ total parts = remainder) \/
  (place_remainder rooms remainder = None /\ total rooms < remainder).
Proof.
  intros rooms remainder. unfold place_remainder.
  pose proof (fill_parts_within_rooms rooms remainder) as within.
  pose proof (fill_conserves rooms remainder) as conserves.
  pose proof (fill_unplaced_iff rooms remainder) as unplaced_iff.
  destruct (fill rooms remainder) as [parts unplaced]. simpl in *.
  destruct (unplaced =? 0) eqn:placed.
  - left. apply Nat.eqb_eq in placed. exists parts.
    split; [reflexivity|]. split; [exact within | lia].
  - right. apply Nat.eqb_neq in placed. split; [reflexivity|].
    destruct (Nat.le_gt_cases remainder (total rooms)) as [fits | short]; [|exact short].
    exfalso. apply placed. apply unplaced_iff. exact fits.
Qed.

(** * A run that passes a certified bound *)

Record run := {
  pre_balance : nat;
  post_balance : nat;
  run_usage : nat;
}.

(** The outcome of a run under a bound, as the final balance and the charge.
    Within the bound, settlement charges the usage. A run that passes a
    certified bound is a certificate failure: the state rolls back to the
    pre-state and nothing is charged. A run that passes the signed limit is
    a user failure, which pays the bound and the fee. *)
Definition outcome (source : bound_source) (bound price : nat) (r : run) : nat * nat :=
  if bound <? run_usage r then
    match source with
    | CertificateSource => (pre_balance r, 0)
    | SignedLimitSource => (pre_balance r - hold_of bound price, hold_of bound price)
    end
  else (post_balance r - hold_of (run_usage r) price, hold_of (run_usage r) price).

Theorem overrun_rollback_effect_free : forall e o bound hold r schedule fresh,
  check e o = Bind CertificateSource bound hold ->
  bound < run_usage r ->
  outcome CertificateSource bound (offer_price o) r = (pre_balance r, 0) /\
  retained_phlo_charge schedule fresh (PhloAccepted [exhaustion_failure CertificateSource]) = 0.
Proof.
  intros e o bound hold r schedule fresh checked over. split.
  - unfold outcome. apply Nat.ltb_lt in over. now rewrite over.
  - apply (certificate_exhaustion_is_unbilled_for_every_admissible_bound bound (offer_limit o)).
    exact (bound_is_admissible e o CertificateSource bound hold checked).
Qed.

(** * Negative controls *)

(** N1: an incomplete estimate treated as complete. A recursive deploy whose
    cost depends on its input has an incomplete estimate that sees one unit.
    The wrong check binds Certificate with that bound, the run passes it, and
    the work that ran pays nothing. The check of design section 6 binds the
    signed limit, the run fits, and settlement charges it. *)
Definition check_treating_every_estimate_as_complete (e : estimate) (o : offer) : decision :=
  if offer_limit o <? estimate_phlo e then Reject
  else Bind CertificateSource (estimate_phlo e) (hold_of (estimate_phlo e) (offer_price o)).

Definition recursive_estimate : estimate :=
  {| analysis_complete := false; onchain_payer := false; estimate_phlo := 1 |}.

Definition recursive_offer : offer := {| offer_limit := 10; offer_price := 1 |}.

Definition recursive_run : run :=
  {| pre_balance := 100; post_balance := 100; run_usage := 5 |}.

Theorem nc_incomplete_as_complete_leaves_recursive_work_unpaid :
  check_treating_every_estimate_as_complete recursive_estimate recursive_offer =
    Bind CertificateSource 1 2 /\
  snd (outcome CertificateSource 1 (offer_price recursive_offer) recursive_run) = 0 /\
  check recursive_estimate recursive_offer = Bind SignedLimitSource 10 11 /\
  snd (outcome SignedLimitSource 10 (offer_price recursive_offer) recursive_run) = 6.
Proof.
  split; [reflexivity|]. split; [reflexivity|]. split; reflexivity.
Qed.

(** N2: a partial remainder hold. The wrong fill accepts whatever fits, so an
    incomplete estimate holds less than its signed limit, against
    P1:1990-1992: supply covers all of a hold or none of it. *)
Definition place_remainder_partially (rooms : list nat) (remainder : nat) : option (list nat) :=
  Some (fst (fill rooms remainder)).

Theorem nc_partial_remainder_holds_less_than_the_limit :
  exists rooms remainder parts,
    place_remainder_partially rooms remainder = Some parts /\
    total parts < remainder /\
    place_remainder rooms remainder = None.
Proof.
  exists [2; 1], 5, [2; 1].
  split; [reflexivity|]. split; [simpl; lia | reflexivity].
Qed.

(** N3: a check without the limit test binds a certified bound above the
    signed limit, which the guard of the execution contract refuses. *)
Definition check_without_the_limit (e : estimate) (o : offer) : decision :=
  if complete e
  then Bind CertificateSource (estimate_phlo e) (hold_of (estimate_phlo e) (offer_price o))
  else Bind SignedLimitSource (offer_limit o) (hold_of (offer_limit o) (offer_price o)).

Theorem nc_bound_above_the_limit_fails_the_guard :
  exists e o bound hold,
    check_without_the_limit e o = Bind CertificateSource bound hold /\
    admissible CertificateSource bound (offer_limit o) = false /\
    check e o = Reject.
Proof.
  exists {| analysis_complete := true; onchain_payer := false; estimate_phlo := 11 |},
    {| offer_limit := 10; offer_price := 1 |}, 11, 12.
  split; [reflexivity|]. split; reflexivity.
Qed.

(** N4: a check that ignores the on-chain payer certifies an offer whose
    purse authority no run has shown yet (review LOW 5). *)
Definition check_ignoring_the_onchain_payer (e : estimate) (o : offer) : decision :=
  if offer_limit o <? estimate_phlo e then Reject
  else if analysis_complete e
       then Bind CertificateSource (estimate_phlo e) (hold_of (estimate_phlo e) (offer_price o))
       else Bind SignedLimitSource (offer_limit o) (hold_of (offer_limit o) (offer_price o)).

Theorem nc_ignoring_the_onchain_payer_certifies_it :
  exists e o bound hold,
    onchain_payer e = true /\
    check_ignoring_the_onchain_payer e o = Bind CertificateSource bound hold /\
    check e o = Bind SignedLimitSource (offer_limit o) (hold_of (offer_limit o) (offer_price o)).
Proof.
  exists {| analysis_complete := true; onchain_payer := true; estimate_phlo := 1 |},
    {| offer_limit := 10; offer_price := 1 |}, 1, 2.
  split; [reflexivity|]. split; reflexivity.
Qed.

(** * Client lanes (G1-6, DR-124)

    The check holds only the client's demand. P1's acceptance protocol checks
    the client's demand and supply only (P1:2191-2200), and a layer signed by
    another signature adds nothing to the client's demand (P1:2002-2027). A
    lane is a client lane when a source of the envelope is located at the
    purse of the lane. The model gives each lane of an estimate that flag and
    its known demand in phlo. The restriction keeps the client lanes, and a
    foreign lane with known demand makes the estimate incomplete. The model
    does not cover how the code decides the flag (the lane key of each
    payer), the classes of a lane, or the placement of the holds. *)

Record lane_demand := {
  located : bool;
  lane_phlo : nat;
}.

Record lane_estimate := {
  lanes_analysis_complete : bool;
  lanes_onchain_payer : bool;
  lane_demands : list lane_demand;
}.

(** The known demand of the client lanes. *)
Fixpoint client_phlo (lanes : list lane_demand) : nat :=
  match lanes with
  | [] => 0
  | lane :: rest => (if located lane then lane_phlo lane else 0) + client_phlo rest
  end.

(** The known demand of every lane. *)
Fixpoint total_phlo (lanes : list lane_demand) : nat :=
  match lanes with
  | [] => 0
  | lane :: rest => lane_phlo lane + total_phlo rest
  end.

(** Whether a foreign lane has known demand. *)
Fixpoint foreign_demand (lanes : list lane_demand) : bool :=
  match lanes with
  | [] => false
  | lane :: rest => (negb (located lane) && (0 <? lane_phlo lane)) || foreign_demand rest
  end.

(** The restriction of DR-124: the estimate of the client lanes. *)
Definition restrict (e : lane_estimate) : estimate :=
  {| analysis_complete := lanes_analysis_complete e && negb (foreign_demand (lane_demands e));
     onchain_payer := lanes_onchain_payer e;
     estimate_phlo := client_phlo (lane_demands e) |}.

(** The check of DR-124: the check of design section 6 on the restriction. *)
Definition check_client (e : lane_estimate) (o : offer) : decision :=
  check (restrict e) o.

Lemma foreign_demand_false_iff : forall lanes,
  foreign_demand lanes = false <->
  Forall (fun lane => located lane = true \/ lane_phlo lane = 0) lanes.
Proof.
  induction lanes as [|lane rest IH]; simpl.
  - split; intros; [constructor | reflexivity].
  - rewrite orb_false_iff, Forall_cons_iff, <- IH.
    split; intros [head tail]; split; try exact tail.
    + apply andb_false_iff in head. destruct head as [located_lane | idle].
      * left. apply negb_false_iff. exact located_lane.
      * right. apply Nat.ltb_ge in idle. lia.
    + apply andb_false_iff. destruct head as [located_lane | idle].
      * left. rewrite located_lane. reflexivity.
      * right. apply Nat.ltb_ge. lia.
Qed.

Lemma client_phlo_of_client_lanes : forall lanes,
  Forall (fun lane => located lane = true) lanes -> client_phlo lanes = total_phlo lanes.
Proof.
  induction lanes as [|lane rest IH]; intros client; simpl; [reflexivity|].
  apply Forall_cons_iff in client. destruct client as [head tail].
  rewrite head, (IH tail). reflexivity.
Qed.

(** A certified estimate has known demand on client lanes only. *)
Theorem certified_estimate_has_only_client_lanes : forall e o bound hold,
  check_client e o = Bind CertificateSource bound hold ->
  Forall (fun lane => located lane = true \/ lane_phlo lane = 0) (lane_demands e).
Proof.
  intros e o bound hold checked.
  pose proof (classification_conservative (restrict e) o bound hold checked) as completed.
  unfold complete, restrict in completed. simpl in completed.
  apply andb_true_iff in completed. destruct completed as [analysed _].
  apply andb_true_iff in analysed. destruct analysed as [_ no_foreign].
  apply negb_true_iff in no_foreign.
  apply foreign_demand_false_iff. exact no_foreign.
Qed.

(** A foreign lane with known demand forces the incomplete classification. *)
Theorem foreign_lane_forces_incomplete : forall e,
  (exists lane, In lane (lane_demands e) /\ located lane = false /\ 0 < lane_phlo lane) ->
  complete (restrict e) = false.
Proof.
  intros e [lane [member [foreign positive]]].
  assert (counted : foreign_demand (lane_demands e) = true).
  { destruct (foreign_demand (lane_demands e)) eqn:demand; [reflexivity|].
    apply foreign_demand_false_iff in demand. rewrite Forall_forall in demand.
    destruct (demand lane member) as [located_lane | idle]; [congruence | lia]. }
  unfold complete, restrict. simpl. rewrite counted. simpl.
  destruct (lanes_analysis_complete e); reflexivity.
Qed.

Theorem foreign_lane_is_never_certified : forall e o bound hold,
  (exists lane, In lane (lane_demands e) /\ located lane = false /\ 0 < lane_phlo lane) ->
  check_client e o <> Bind CertificateSource bound hold.
Proof.
  intros e o bound hold foreign checked.
  pose proof (classification_conservative (restrict e) o bound hold checked) as completed.
  rewrite (foreign_lane_forces_incomplete e foreign) in completed. discriminate.
Qed.

(** With a foreign lane, an offer whose client demand fits its limit holds
    the signed limit under SignedLimit. *)
Theorem foreign_lane_holds_the_signed_limit : forall e o,
  (exists lane, In lane (lane_demands e) /\ located lane = false /\ 0 < lane_phlo lane) ->
  client_phlo (lane_demands e) <= offer_limit o ->
  check_client e o = Bind SignedLimitSource (offer_limit o) (offer_limit o * offer_price o + 1).
Proof.
  intros e o foreign within. unfold check_client.
  apply incomplete_hold_is_full_limit.
  - exact (foreign_lane_forces_incomplete e foreign).
  - exact within.
Qed.

(** The limit test compares the signed limit with the client's demand. *)
Theorem limit_test_uses_client_demand : forall e o,
  check_client e o = Reject <-> offer_limit o < client_phlo (lane_demands e).
Proof.
  intros e o. unfold check_client, check.
  change (estimate_phlo (restrict e)) with (client_phlo (lane_demands e)).
  destruct (offer_limit o <? client_phlo (lane_demands e)) eqn:above.
  - apply Nat.ltb_lt in above. split; [intros _; exact above | reflexivity].
  - apply Nat.ltb_ge in above. split; [|intro; lia].
    destruct (complete (restrict e)); discriminate.
Qed.

(** For an estimate whose lanes are all client lanes, the restriction
    changes nothing. *)
Theorem client_lanes_keep_the_check : forall e o,
  Forall (fun lane => located lane = true) (lane_demands e) ->
  check_client e o =
    check {| analysis_complete := lanes_analysis_complete e;
             onchain_payer := lanes_onchain_payer e;
             estimate_phlo := total_phlo (lane_demands e) |} o.
Proof.
  intros e o client.
  assert (no_foreign : foreign_demand (lane_demands e) = false).
  { apply foreign_demand_false_iff. eapply Forall_impl; [|exact client].
    intros lane located_lane. left. exact located_lane. }
  unfold check_client, restrict. rewrite no_foreign, andb_true_r.
  rewrite (client_phlo_of_client_lanes (lane_demands e) client). reflexivity.
Qed.

(** N5 (G1-6, DR-124): a restriction that removes a foreign lane with known
    demand but keeps the estimate complete. The check then certifies the
    client's demand alone. The foreign work runs in the deployment, so the
    run passes that certified bound and pays nothing. The restriction of
    DR-124 makes the estimate incomplete, so the run fits the signed limit
    and settlement charges it. *)
Definition restrict_ignoring_foreign_lanes (e : lane_estimate) : estimate :=
  {| analysis_complete := lanes_analysis_complete e;
     onchain_payer := lanes_onchain_payer e;
     estimate_phlo := client_phlo (lane_demands e) |}.

Definition foreign_lane_estimate : lane_estimate :=
  {| lanes_analysis_complete := true;
     lanes_onchain_payer := false;
     lane_demands := [{| located := true; lane_phlo := 1 |};
                      {| located := false; lane_phlo := 5 |}] |}.

Definition foreign_lane_offer : offer := {| offer_limit := 10; offer_price := 1 |}.

Definition foreign_lane_run : run :=
  {| pre_balance := 100; post_balance := 100; run_usage := 6 |}.

Theorem nc_foreign_lane_as_complete_breaks_the_bound :
  check (restrict_ignoring_foreign_lanes foreign_lane_estimate) foreign_lane_offer =
    Bind CertificateSource 1 2 /\
  snd (outcome CertificateSource 1 (offer_price foreign_lane_offer) foreign_lane_run) = 0 /\
  check_client foreign_lane_estimate foreign_lane_offer = Bind SignedLimitSource 10 11 /\
  snd (outcome SignedLimitSource 10 (offer_price foreign_lane_offer) foreign_lane_run) = 7.
Proof.
  split; [reflexivity|]. split; [reflexivity|]. split; reflexivity.
Qed.

Print Assumptions classification_conservative.
Print Assumptions onchain_payer_is_never_certified.
Print Assumptions estimate_above_limit_rejected.
Print Assumptions complete_hold_is_exact.
Print Assumptions incomplete_hold_is_full_limit.
Print Assumptions bound_is_admissible.
Print Assumptions fill_parts_within_rooms.
Print Assumptions fill_conserves.
Print Assumptions fill_unplaced_iff.
Print Assumptions custody_fill_complete_or_reject.
Print Assumptions overrun_rollback_effect_free.
Print Assumptions nc_incomplete_as_complete_leaves_recursive_work_unpaid.
Print Assumptions nc_partial_remainder_holds_less_than_the_limit.
Print Assumptions nc_bound_above_the_limit_fails_the_guard.
Print Assumptions nc_ignoring_the_onchain_payer_certifies_it.
Print Assumptions foreign_demand_false_iff.
Print Assumptions client_phlo_of_client_lanes.
Print Assumptions certified_estimate_has_only_client_lanes.
Print Assumptions foreign_lane_forces_incomplete.
Print Assumptions foreign_lane_is_never_certified.
Print Assumptions foreign_lane_holds_the_signed_limit.
Print Assumptions limit_test_uses_client_demand.
Print Assumptions client_lanes_keep_the_check.
Print Assumptions nc_foreign_lane_as_complete_breaks_the_bound.
