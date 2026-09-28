From Coq Require Import List.
From NodeAuthority Require Import AuthorityObserver.
From NodeAuthority Require Import AuthorityWork.

Theorem authority_instance_attaches_at_most_once : forall bs c rs,
  attach_all None bs = (c, rs) -> successes rs <= 1.
Proof. exact instance_attaches_at_most_once. Qed.

Theorem authority_first_attachment_wins : forall b rest c rs,
  attach_all None (b :: rest) = (c, rs) -> c = Some b.
Proof. exact first_attachment_wins. Qed.

Theorem authority_replaced_binding_refused : forall bound c b,
  accepted c b = true -> accepted (install bound c) b = false.
Proof. exact replaced_binding_refused. Qed.

Theorem authority_shutdown_refuses_all : forall c b,
  accepted (shutdown c) b = false.
Proof. exact shutdown_refuses_all. Qed.

Theorem authority_installation_never_wraps : forall bound c,
  current_installation (install bound c) <= bound \/
  current_installation (install bound c) = current_installation c.
Proof. exact installation_never_wraps. Qed.

Theorem authority_queue_never_exceeds_capacity : forall cap bound active steps,
  queued (ledger_run cap bound active steps) <= cap.
Proof. exact queue_never_exceeds_capacity. Qed.

Theorem authority_ledger_accounts_for_every_attempt : forall cap bound active steps,
  attempted (ledger_run cap bound active steps) =
  delivered (ledger_run cap bound active steps) + lost (ledger_run cap bound active steps).
Proof. exact ledger_accounts_for_every_attempt. Qed.

Theorem authority_complete_coverage_delivers_every_attempt : forall cap bound active steps,
  complete active (ledger_run cap bound active steps) ->
  delivered (ledger_run cap bound active steps) = attempted (ledger_run cap bound active steps).
Proof. exact complete_coverage_delivers_every_attempt. Qed.

Theorem authority_sequence_exhaustion_refused : forall cap bound l a,
  bound <= attempted l ->
  attempted (emit cap bound true l a) = attempted l /\ overflow (emit cap bound true l a) = true.
Proof. exact sequence_exhaustion_refused. Qed.

Theorem authority_shared_budget_bounded : forall limit amounts b,
  used b <= limit -> used (charge_all limit b amounts) <= limit.
Proof. exact shared_budget_bounded. Qed.

Theorem authority_budget_failure_sticky : forall limit amounts b,
  failed b = true -> charge_all limit b amounts = b.
Proof. exact budget_failure_sticky. Qed.

Theorem authority_budget_failure_keeps_usage : forall limit b amount,
  failed b = false -> failed (charge limit b amount) = true ->
  used (charge limit b amount) = used b /\ limit < used b + amount.
Proof. exact budget_failure_keeps_usage. Qed.

Theorem authority_paths_share_one_budget : forall limit b xs ys,
  charge_all limit b (xs ++ ys) = charge_all limit (charge_all limit b xs) ys.
Proof. exact paths_share_one_budget. Qed.

Theorem authority_checked_overflow_is_limit_failure : forall bound limit b amount,
  limit <= bound -> failed b = false ->
  checked_add bound (used b) amount = None -> failed (charge limit b amount) = true.
Proof. exact checked_overflow_is_limit_failure. Qed.

Theorem authority_comparison_requires_equal_digest : forall (A R : Type) (digest : A -> nat)
  (decide : A -> A -> R) (x y : A) (r : R),
  compare digest decide x y = Some r -> digest x = digest y.
Proof. exact comparison_requires_equal_digest. Qed.
