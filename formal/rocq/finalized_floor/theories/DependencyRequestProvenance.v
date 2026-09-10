From Stdlib Require Import Bool Lists.List.
Import ListNotations.

Definition promote (previous dependency : bool) := orb previous dependency.
Fixpoint apply_requests (previous : bool) (requests : list bool) : bool :=
  match requests with
  | [] => previous
  | dependency :: rest => apply_requests (promote previous dependency) rest
  end.

Theorem promotion_is_exact : forall previous dependency,
  promote previous dependency = true <-> previous = true \/ dependency = true.
Proof. intros [] []; simpl; intuition congruence. Qed.

Theorem announcements_preserve_provenance : forall previous,
  promote previous false = previous.
Proof. intros []; reflexivity. Qed.

Theorem dependency_request_establishes_provenance : forall previous,
  promote previous true = true.
Proof. intros []; reflexivity. Qed.

Theorem request_order_is_irrelevant : forall previous first second,
  promote (promote previous first) second = promote (promote previous second) first.
Proof. intros [] [] []; reflexivity. Qed.

Theorem arbitrary_history_is_exact : forall requests previous,
  apply_requests previous requests = true <-> previous = true \/ In true requests.
Proof.
  induction requests as [|head rest IH]; intros previous; simpl.
  - tauto.
  - rewrite IH, promotion_is_exact. simpl. intuition congruence.
Qed.

Theorem update_preserves_other_fields : forall (Policy : Type) (policy : Policy)
  previous dependency,
  snd (promote previous dependency, policy) = policy.
Proof. reflexivity. Qed.

Print Assumptions promotion_is_exact.
Print Assumptions announcements_preserve_provenance.
Print Assumptions dependency_request_establishes_provenance.
Print Assumptions request_order_is_irrelevant.
Print Assumptions arbitrary_history_is_exact.
Print Assumptions update_preserves_other_fields.
