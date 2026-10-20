(* Changed by the merge of feature/cost-accounted-rho (DR-131, plan step 1.8).
   DR-131 removes host work. The merge removes the host-work budget of the
   reducer from root_with_observation
   (rholang/src/rust/interpreter/deterministic_reduction.rs), so a re-entry
   into the same domain no longer compares host-work policies. The domain key
   is the space, the phlo budget and the coordinator (matches_domain).

   The model keeps budget_id, which models the phlo RuntimeBudget in the
   domain key, and every space, coordinator, scope, roster and failure-ledger
   theorem. The host-work parts stay below as comments: host_compatible, the
   host arguments of may_reuse and decide_admission, the Reject decision,
   same_domain_host_mismatch_rejects, distinct_explicit_host_work_cannot_reuse,
   HostUsage, HostLimit, reserve_host_for_domain,
   host_charge_cannot_touch_foreign_domain and
   owner_over_limit_rejects_without_charging_foreign_host. Their Print
   Assumptions lines are comments too. *)

From Stdlib Require Import List Bool Arith.
Import ListNotations.

Record DomainKey := {
  space_id : nat;
  budget_id : nat;
  coordinator_id : nat;
}.

(* Disabled by the merge of feature/cost-accounted-rho (DR-131): no host-work
   policy takes part in domain admission.

Definition host_compatible (requested current : option nat) : bool :=
  match requested with
  | None => true
  | Some requested_id =>
      match current with
      | None => false
      | Some current_id => Nat.eqb requested_id current_id
      end
  end.
*)

Definition same_domain (requested current : DomainKey) : bool :=
  Nat.eqb (space_id requested) (space_id current) &&
  Nat.eqb (budget_id requested) (budget_id current) &&
  Nat.eqb (coordinator_id requested) (coordinator_id current).

(* Changed by the merge of feature/cost-accounted-rho (DR-131): a domain is
   reused exactly when the domain keys agree. The old definition follows.

Definition may_reuse
  (requested current : DomainKey) (requested_host current_host : option nat) : bool :=
  same_domain requested current && host_compatible requested_host current_host.
*)
Definition may_reuse (requested current : DomainKey) : bool :=
  same_domain requested current.

(* Changed by the merge of feature/cost-accounted-rho (DR-131): with no
   host-work policy, admission never rejects. The old definitions follow.

Inductive AdmissionDecision := Reuse | Fresh | Reject.

Definition decide_admission
  (requested current : DomainKey) (requested_host current_host : option nat)
  : AdmissionDecision :=
  if same_domain requested current
  then if host_compatible requested_host current_host then Reuse else Reject
  else Fresh.
*)
Inductive AdmissionDecision := Reuse | Fresh.

Definition decide_admission (requested current : DomainKey) : AdmissionDecision :=
  if same_domain requested current then Reuse else Fresh.

(* Changed by the merge of feature/cost-accounted-rho (DR-131): the theorems
   below lose the host arguments of may_reuse and decide_admission. The old
   statement and proof of each one follow.

Theorem reuse_iff_may_reuse : forall requested current requested_host current_host,
  decide_admission requested current requested_host current_host = Reuse <->
  may_reuse requested current requested_host current_host = true.
Proof.
  intros requested current requested_host current_host.
  unfold decide_admission, may_reuse.
  destruct (same_domain requested current);
    destruct (host_compatible requested_host current_host); simpl; split; intros H;
    try discriminate; reflexivity.
Qed.

Theorem foreign_domain_starts_fresh : forall requested current requested_host current_host,
  same_domain requested current = false ->
  decide_admission requested current requested_host current_host = Fresh.
Proof.
  intros requested current requested_host current_host Hforeign.
  unfold decide_admission. now rewrite Hforeign.
Qed.

Theorem same_domain_host_mismatch_rejects :
  forall requested current requested_host current_host,
    same_domain requested current = true ->
    host_compatible requested_host current_host = false ->
    decide_admission requested current requested_host current_host = Reject.
Proof.
  intros requested current requested_host current_host Hsame Hhost.
  unfold decide_admission. now rewrite Hsame, Hhost.
Qed.

Theorem reuse_requires_same_space : forall requested current requested_host current_host,
  may_reuse requested current requested_host current_host = true ->
  space_id requested = space_id current.
Proof.
  intros requested current requested_host current_host H.
  unfold may_reuse, same_domain in H.
  repeat rewrite andb_true_iff in H.
  destruct H as [[[Hspace _] _] _].
  now apply Nat.eqb_eq in Hspace.
Qed.

Theorem reuse_requires_same_budget : forall requested current requested_host current_host,
  may_reuse requested current requested_host current_host = true ->
  budget_id requested = budget_id current.
Proof.
  intros requested current requested_host current_host H.
  unfold may_reuse, same_domain in H.
  repeat rewrite andb_true_iff in H.
  destruct H as [[[_ Hbudget] _] _].
  now apply Nat.eqb_eq in Hbudget.
Qed.

Theorem reuse_requires_same_coordinator : forall requested current requested_host current_host,
  may_reuse requested current requested_host current_host = true ->
  coordinator_id requested = coordinator_id current.
Proof.
  intros requested current requested_host current_host H.
  unfold may_reuse, same_domain in H.
  repeat rewrite andb_true_iff in H.
  destruct H as [[[_ _] Hcoordinator] _].
  now apply Nat.eqb_eq in Hcoordinator.
Qed.

Theorem distinct_spaces_cannot_reuse : forall requested current requested_host current_host,
  space_id requested <> space_id current ->
  may_reuse requested current requested_host current_host = false.
Proof.
  intros requested current requested_host current_host Hdistinct.
  destruct (may_reuse requested current requested_host current_host) eqn:Hreuse; auto.
  exfalso.
  apply Hdistinct.
  now apply (reuse_requires_same_space _ _ requested_host current_host).
Qed.

Theorem distinct_explicit_host_work_cannot_reuse :
  forall requested current requested_host current_host,
    requested_host <> current_host ->
    requested_host <> None ->
    may_reuse requested current requested_host current_host = false.
Proof.
  intros requested current [requested_id|] [current_id|] Hdistinct Hexplicit;
    simpl in *; try contradiction.
  - destruct (Nat.eqb requested_id current_id) eqn:Heq.
    + apply Nat.eqb_eq in Heq. subst. contradiction.
    + unfold may_reuse, host_compatible. rewrite Heq.
      now rewrite andb_false_r.
  - unfold may_reuse, host_compatible.
    now rewrite andb_false_r.
Qed.
*)

Theorem reuse_iff_may_reuse : forall requested current,
  decide_admission requested current = Reuse <->
  may_reuse requested current = true.
Proof.
  intros requested current.
  unfold decide_admission, may_reuse.
  destruct (same_domain requested current); split; intros H;
    try discriminate; reflexivity.
Qed.

Theorem foreign_domain_starts_fresh : forall requested current,
  same_domain requested current = false ->
  decide_admission requested current = Fresh.
Proof.
  intros requested current Hforeign.
  unfold decide_admission. now rewrite Hforeign.
Qed.

Theorem reuse_requires_same_space : forall requested current,
  may_reuse requested current = true ->
  space_id requested = space_id current.
Proof.
  intros requested current H.
  unfold may_reuse, same_domain in H.
  repeat rewrite andb_true_iff in H.
  destruct H as [[Hspace _] _].
  now apply Nat.eqb_eq in Hspace.
Qed.

Theorem reuse_requires_same_budget : forall requested current,
  may_reuse requested current = true ->
  budget_id requested = budget_id current.
Proof.
  intros requested current H.
  unfold may_reuse, same_domain in H.
  repeat rewrite andb_true_iff in H.
  destruct H as [[_ Hbudget] _].
  now apply Nat.eqb_eq in Hbudget.
Qed.

Theorem reuse_requires_same_coordinator : forall requested current,
  may_reuse requested current = true ->
  coordinator_id requested = coordinator_id current.
Proof.
  intros requested current H.
  unfold may_reuse, same_domain in H.
  repeat rewrite andb_true_iff in H.
  destruct H as [[_ _] Hcoordinator].
  now apply Nat.eqb_eq in Hcoordinator.
Qed.

Theorem distinct_spaces_cannot_reuse : forall requested current,
  space_id requested <> space_id current ->
  may_reuse requested current = false.
Proof.
  intros requested current Hdistinct.
  destruct (may_reuse requested current) eqn:Hreuse; auto.
  exfalso.
  apply Hdistinct.
  now apply (reuse_requires_same_space requested current).
Qed.

Definition enter_domain (stack : list nat) (requested : nat) : list nat * bool :=
  (requested :: stack, negb (existsb (Nat.eqb requested) stack)).

Definition leave_domain (entry : list nat * bool) : list nat :=
  tl (fst entry).

Theorem scoped_entry_restores_caller : forall stack requested,
  leave_domain (enter_domain stack requested) = stack.
Proof.
  intros stack requested.
  reflexivity.
Qed.

Theorem ancestor_reentry_reuses_session : forall stack requested,
  In requested stack -> snd (enter_domain stack requested) = false.
Proof.
  intros stack requested Hin.
  unfold enter_domain.
  simpl.
  assert (Hex : existsb (Nat.eqb requested) stack = true).
  { apply existsb_exists. exists requested. split; auto.
    apply Nat.eqb_refl. }
  rewrite Hex.
  reflexivity.
Qed.

Definition Rosters := nat -> list nat.

Definition split_in_domain
  (rosters : Rosters) (domain parent child : nat) : Rosters :=
  fun selected =>
    if Nat.eqb selected domain
    then child :: remove Nat.eq_dec parent (rosters domain)
    else rosters selected.

Theorem foreign_split_preserves_outer_roster :
  forall rosters outer inner parent child,
    outer <> inner ->
    split_in_domain rosters inner parent child outer = rosters outer.
Proof.
  intros rosters outer inner parent child Hdistinct.
  unfold split_in_domain.
  apply Nat.eqb_neq in Hdistinct.
  rewrite Hdistinct.
  reflexivity.
Qed.

Theorem nested_split_preserves_outer_participant :
  forall rosters outer inner parent child,
    outer <> inner ->
    In parent (rosters outer) ->
    In parent (split_in_domain rosters inner parent child outer).
Proof.
  intros rosters outer inner parent child Hdistinct Hregistered.
  rewrite foreign_split_preserves_outer_roster by exact Hdistinct.
  exact Hregistered.
Qed.

Definition FailureLedger := DomainKey -> list nat.

Definition record_failure_for_domain
  (active_domains : list DomainKey) (owner : DomainKey)
  (error : nat) (ledger : FailureLedger)
  : FailureLedger :=
  if existsb (same_domain owner) active_domains
  then fun domain =>
    if same_domain domain owner then error :: ledger domain else ledger domain
  else ledger.

Theorem absent_domain_cannot_record_failure :
  forall active_domains owner error ledger domain,
    existsb (same_domain owner) active_domains = false ->
    record_failure_for_domain active_domains owner error ledger domain = ledger domain.
Proof.
  intros active_domains owner error ledger domain Habsent.
  unfold record_failure_for_domain.
  now rewrite Habsent.
Qed.

Theorem active_domain_records_only_its_failure :
  forall active_domains owner error ledger domain,
    In owner active_domains ->
    record_failure_for_domain active_domains owner error ledger domain =
      if same_domain domain owner then error :: ledger domain else ledger domain.
Proof.
  intros active_domains owner error ledger domain Hin.
  unfold record_failure_for_domain.
  assert (Hfound : existsb (same_domain owner) active_domains = true).
  { apply existsb_exists. exists owner. split; [exact Hin |].
    unfold same_domain. repeat rewrite Nat.eqb_refl. reflexivity. }
  now rewrite Hfound.
Qed.

Theorem nested_failure_does_not_contaminate_other_domain :
  forall active_domains owner other error ledger,
    same_domain other owner = false ->
    record_failure_for_domain active_domains owner error ledger other = ledger other.
Proof.
  intros active_domains owner other error ledger Hdistinct.
  unfold record_failure_for_domain.
  destruct (existsb (same_domain owner) active_domains); [|reflexivity].
  now rewrite Hdistinct.
Qed.

(* Disabled by the merge of feature/cost-accounted-rho (DR-131): no domain
   reserves host work, so no host-work charge or limit exists. The model of
   that reservation follows.

Definition HostUsage := DomainKey -> nat.
Definition HostLimit := DomainKey -> nat.

Definition reserve_host_for_domain
  (active_domains : list DomainKey) (owner : DomainKey)
  (amount : nat) (host_enabled : DomainKey -> bool)
  (limit : HostLimit) (usage : HostUsage)
  : option HostUsage :=
  if existsb (same_domain owner) active_domains && host_enabled owner
  then
    if Nat.leb (usage owner + amount) (limit owner)
    then Some (fun domain =>
      if same_domain domain owner then usage domain + amount else usage domain)
    else None
  else Some usage.

Theorem host_charge_cannot_touch_foreign_domain :
  forall active_domains owner other amount host_enabled limit usage next,
    same_domain other owner = false ->
    reserve_host_for_domain active_domains owner amount host_enabled limit usage = Some next ->
    next other = usage other.
Proof.
  intros active_domains owner other amount host_enabled limit usage next Hforeign Hcharge.
  unfold reserve_host_for_domain in Hcharge.
  destruct (existsb (same_domain owner) active_domains && host_enabled owner) eqn:Hactive.
  - destruct (Nat.leb (usage owner + amount) (limit owner)) eqn:Hwithin;
      inversion Hcharge; subst; clear Hcharge.
    now rewrite Hforeign.
  - now inversion Hcharge.
Qed.

Theorem owner_over_limit_rejects_without_charging_foreign_host :
  forall active_domains owner amount host_enabled limit usage,
    In owner active_domains ->
    host_enabled owner = true ->
    limit owner < usage owner + amount ->
    reserve_host_for_domain active_domains owner amount host_enabled limit usage = None.
Proof.
  intros active_domains owner amount host_enabled limit usage Hin Henabled Hover.
  unfold reserve_host_for_domain.
  assert (Hfound : existsb (same_domain owner) active_domains = true).
  { apply existsb_exists. exists owner. split; [exact Hin |].
    unfold same_domain. repeat rewrite Nat.eqb_refl. reflexivity. }
  rewrite Hfound, Henabled.
  simpl.
  apply Nat.leb_gt in Hover.
  now rewrite Hover.
Qed.
*)

Print Assumptions reuse_requires_same_space.
Print Assumptions reuse_iff_may_reuse.
Print Assumptions foreign_domain_starts_fresh.
(* Disabled by the merge of feature/cost-accounted-rho (DR-131):
Print Assumptions same_domain_host_mismatch_rejects.
*)
Print Assumptions reuse_requires_same_budget.
Print Assumptions reuse_requires_same_coordinator.
Print Assumptions distinct_spaces_cannot_reuse.
(* Disabled by the merge of feature/cost-accounted-rho (DR-131):
Print Assumptions distinct_explicit_host_work_cannot_reuse.
*)
Print Assumptions scoped_entry_restores_caller.
Print Assumptions ancestor_reentry_reuses_session.
Print Assumptions foreign_split_preserves_outer_roster.
Print Assumptions nested_split_preserves_outer_participant.
Print Assumptions absent_domain_cannot_record_failure.
Print Assumptions active_domain_records_only_its_failure.
Print Assumptions nested_failure_does_not_contaminate_other_domain.
