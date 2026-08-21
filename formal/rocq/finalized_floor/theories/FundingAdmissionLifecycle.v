From Stdlib Require Import Arith.Arith.

Inductive AdmissionDecision :=
| Execute
| Reject.

Inductive TerminalStatus :=
| ExecutedFinalized
| RejectedFinalized.

Record AdmissionRecord := {
  recorded_supply : nat;
  recorded_demand : nat;
  recorded_decision : AdmissionDecision
}.

Definition classify (supply demand : nat) : AdmissionDecision :=
  if Nat.leb demand supply then Execute else Reject.

Definition propose (supply demand : nat) : AdmissionRecord :=
  {| recorded_supply := supply;
     recorded_demand := demand;
     recorded_decision := classify supply demand |}.

Definition validate_record (record : AdmissionRecord) : bool :=
  match classify (recorded_supply record) (recorded_demand record),
        recorded_decision record with
  | Execute, Execute => true
  | Reject, Reject => true
  | _, _ => false
  end.

Definition user_effects (record : AdmissionRecord) : nat :=
  match recorded_decision record with
  | Execute => 1
  | Reject => 0
  end.

Definition finalize_record (record : AdmissionRecord) : TerminalStatus :=
  match recorded_decision record with
  | Execute => ExecutedFinalized
  | Reject => RejectedFinalized
  end.

Theorem proposal_revalidates_from_recorded_supply :
  forall supply demand,
    validate_record (propose supply demand) = true.
Proof.
  intros supply demand.
  unfold validate_record, propose, classify.
  simpl.
  destruct (Nat.leb demand supply); reflexivity.
Qed.

Theorem underfunded_proposal_is_terminal_rejection :
  forall supply demand,
    supply < demand ->
    recorded_decision (propose supply demand) = Reject /\
    user_effects (propose supply demand) = 0 /\
    finalize_record (propose supply demand) = RejectedFinalized.
Proof.
  intros supply demand underfunded.
  assert (decision : Nat.leb demand supply = false).
  { apply Nat.leb_gt. exact underfunded. }
  unfold propose, classify, user_effects, finalize_record.
  simpl.
  rewrite decision.
  repeat split.
Qed.

Theorem later_supply_does_not_resurrect_recorded_rejection :
  forall record (later_supply : nat),
    recorded_decision record = Reject ->
    finalize_record record = RejectedFinalized /\
    user_effects record = 0.
Proof.
  intros [supply demand decision] later_supply rejected.
  destruct decision.
  - discriminate.
  - split; reflexivity.
Qed.

Theorem fundable_deploy_cannot_be_forged_as_rejected :
  forall supply demand,
    demand <= supply ->
    validate_record
      {| recorded_supply := supply;
         recorded_demand := demand;
         recorded_decision := Reject |} = false.
Proof.
  intros supply demand funded.
  assert (decision : Nat.leb demand supply = true).
  { apply Nat.leb_le. exact funded. }
  unfold validate_record, classify.
  simpl.
  rewrite decision.
  reflexivity.
Qed.
