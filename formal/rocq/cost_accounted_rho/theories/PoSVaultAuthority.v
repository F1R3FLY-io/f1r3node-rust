From Stdlib Require Import Arith Bool Lia.

Inductive principal : Type :=
| PosGenerator
| Attacker
| LiteralPlaceholder.

Scheme Equality for principal.

Record vault_state : Type := {
  control_key : principal;
  pos_balance : nat;
  target_balance : nat
}.

Definition compile_template
    (has_unresolved_placeholder reject_unresolved_templates : bool) : bool :=
  negb (has_unresolved_placeholder && reject_unresolved_templates).

Definition install_pos_vault (bind_authenticated_key : bool)
    (balance target : nat) : vault_state :=
  {| control_key := if bind_authenticated_key then PosGenerator else LiteralPlaceholder;
     pos_balance := balance;
     target_balance := target |}.

Definition transfer (caller : principal) (state : vault_state)
    : vault_state * bool :=
  if principal_beq caller (control_key state) then
    match pos_balance state with
    | O => (state, false)
    | S remaining =>
        ({| control_key := control_key state;
            pos_balance := remaining;
            target_balance := S (target_balance state) |}, true)
    end
  else (state, false).

Theorem unresolved_templates_fail_closed :
  compile_template true true = false.
Proof.
  reflexivity.
Qed.

Theorem complete_templates_compile :
  forall reject,
    compile_template false reject = true.
Proof.
  intros reject.
  destruct reject; reflexivity.
Qed.

Theorem authenticated_install_binds_pos_generator :
  forall balance target,
    control_key (install_pos_vault true balance target) = PosGenerator.
Proof.
  reflexivity.
Qed.

Theorem unauthorized_transfer_is_effect_free :
  forall balance target,
    transfer Attacker (install_pos_vault true balance target) =
      (install_pos_vault true balance target, false).
Proof.
  reflexivity.
Qed.

Theorem authenticated_transfer_moves_exactly_one :
  forall balance target,
    transfer PosGenerator (install_pos_vault true (S balance) target) =
      ({| control_key := PosGenerator;
          pos_balance := balance;
          target_balance := S target |}, true).
Proof.
  reflexivity.
Qed.

Theorem transfer_conserves_custody :
  forall caller state next accepted,
    transfer caller state = (next, accepted) ->
    pos_balance next + target_balance next =
      pos_balance state + target_balance state.
Proof.
  intros caller [control balance target] next accepted Htransfer.
  destruct caller, control, balance; simpl in Htransfer;
    inversion Htransfer; subst; simpl; lia.
Qed.

Theorem literal_placeholder_denies_the_authenticated_generator :
  forall balance target,
    transfer PosGenerator (install_pos_vault false balance target) =
      (install_pos_vault false balance target, false).
Proof.
  reflexivity.
Qed.
