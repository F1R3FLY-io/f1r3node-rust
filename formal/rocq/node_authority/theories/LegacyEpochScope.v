From Stdlib Require Import Bool.Bool.

Inductive event_epoch := Legacy | CostV1 | CostV2.

Definition carries_cost (epoch : event_epoch) : bool :=
  match epoch with
  | Legacy => false
  | CostV1 | CostV2 => true
  end.

Definition opens_accounting_scope (epoch : event_epoch) (requested : bool) : bool :=
  requested && carries_cost epoch.

Theorem legacy_never_opens_accounting_scope :
  forall requested, opens_accounting_scope Legacy requested = false.
Proof. destruct requested; reflexivity. Qed.

Theorem accounting_scope_requires_cost_epoch :
  forall epoch requested,
    opens_accounting_scope epoch requested = true -> carries_cost epoch = true.
Proof. destruct epoch, requested; simpl; congruence. Qed.

Theorem cost_epochs_preserve_requested_scope :
  forall epoch requested,
    carries_cost epoch = true ->
    opens_accounting_scope epoch requested = requested.
Proof. destruct epoch, requested; simpl; congruence. Qed.
