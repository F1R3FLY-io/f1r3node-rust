From Stdlib Require Import Bool Lists.List.
Import ListNotations.

Record child := Child { live : bool; owned : bool; abort_requested : bool; observed : bool }.
Definition safe (c : child) : Prop :=
  (live c = true -> owned c = true \/ abort_requested c = true) /\
  (observed c = true -> live c = false).
Inductive operation := Poll | CompetingBranch | Finish | Join | Stop | DropOwner.
Definition step (op : operation) (c : child) : child :=
  match op with
  | Poll | CompetingBranch => c
  | Finish => Child false (owned c) (abort_requested c) (observed c)
  | Join => if live c then c else Child false false (abort_requested c) true
  | Stop => Child (live c) (owned c) (abort_requested c || owned c) (observed c)
  | DropOwner => Child (live c) false (abort_requested c || owned c) (observed c)
  end.
Fixpoint history (ops : list operation) (c : child) : child :=
  match ops with [] => c | op :: tail => history tail (step op c) end.

Theorem initial_child_is_owned : safe (Child true true false false).
Proof. unfold safe. simpl. intuition discriminate. Qed.

Theorem polling_does_not_move_ownership : forall c, step Poll c = c.
Proof. reflexivity. Qed.

Theorem competing_branch_preserves_result_owner : forall c, step CompetingBranch c = c.
Proof. reflexivity. Qed.

Theorem step_preserves_ownership : forall op c, safe c -> safe (step op c).
Proof.
  intros [] [l o a r]; destruct l, o, a, r; unfold safe; simpl;
    intuition discriminate.
Qed.

Theorem arbitrary_history_preserves_ownership : forall ops c, safe c -> safe (history ops c).
Proof.
  induction ops as [|op tail IH]; intros c H; simpl; [exact H|].
  apply IH. now apply step_preserves_ownership.
Qed.

Theorem dropping_owned_live_child_requests_abort : forall a r,
  abort_requested (step DropOwner (Child true true a r)) = true.
Proof. intros [] []; reflexivity. Qed.

Theorem drop_does_not_claim_immediate_retirement : forall c,
  live (step DropOwner c) = live c.
Proof. reflexivity. Qed.

Theorem live_child_cannot_be_joined : forall o a r,
  step Join (Child true o a r) = Child true o a r.
Proof. reflexivity. Qed.

Print Assumptions initial_child_is_owned.
Print Assumptions polling_does_not_move_ownership.
Print Assumptions competing_branch_preserves_result_owner.
Print Assumptions step_preserves_ownership.
Print Assumptions arbitrary_history_preserves_ownership.
Print Assumptions dropping_owned_live_child_requests_abort.
Print Assumptions drop_does_not_claim_immediate_retirement.
Print Assumptions live_child_cannot_be_joined.
