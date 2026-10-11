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
   negative controls refute plausible wrong rules. *)

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
