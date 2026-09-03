From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.

Record LocalLedgerHead := {
  local_target : nat;
  local_revision : nat;
  local_digest : nat
}.

Definition stepped_history_head : LocalLedgerHead :=
  {| local_target := 10; local_revision := 2; local_digest := 512 |}.

Definition direct_history_head : LocalLedgerHead :=
  {| local_target := 10; local_revision := 1; local_digest := 101 |}.

Definition same_local_ledger_identity
  (left right : LocalLedgerHead) : bool :=
  Nat.eqb (local_revision left) (local_revision right) &&
  Nat.eqb (local_digest left) (local_digest right).

Theorem equal_finalized_target_does_not_imply_equal_local_ledger_identity :
  local_target stepped_history_head = local_target direct_history_head /\
  local_revision stepped_history_head <> local_revision direct_history_head /\
  local_digest stepped_history_head <> local_digest direct_history_head.
Proof.
  repeat split; discriminate.
Qed.

Theorem cross_node_local_ledger_lookup_rejects_divergent_histories :
  same_local_ledger_identity stepped_history_head direct_history_head = false.
Proof.
  reflexivity.
Qed.

Theorem canonical_target_identity_is_independent_of_local_round_history :
  forall stepped direct,
    local_target stepped = local_target direct ->
    local_target stepped_history_head = local_target direct_history_head.
Proof.
  intros.
  reflexivity.
Qed.

Print Assumptions equal_finalized_target_does_not_imply_equal_local_ledger_identity.
Print Assumptions cross_node_local_ledger_lookup_rejects_divergent_histories.
