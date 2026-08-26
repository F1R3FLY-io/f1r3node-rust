From Stdlib Require Import Arith.Arith Lia.

Definition approval_authorized
    (local_minimum candidate_threshold bonded_count valid_distinct_count : nat) : Prop :=
  local_minimum <= candidate_threshold /\
  candidate_threshold <= bonded_count /\
  candidate_threshold <= valid_distinct_count.

Definition CeremonyState := (bool * nat)%type.

Definition apply_approval
  (local_minimum candidate_threshold bonded_count valid_distinct_count : nat)
  (state : CeremonyState) : CeremonyState :=
  if andb (Nat.leb local_minimum candidate_threshold)
       (andb (Nat.leb candidate_threshold bonded_count)
             (Nat.leb candidate_threshold valid_distinct_count))
  then (true, S (snd state))
  else state.

Lemma approval_authorized_sound :
  forall local_minimum candidate_threshold bonded_count valid_distinct_count,
    approval_authorized local_minimum candidate_threshold bonded_count valid_distinct_count ->
    local_minimum <= candidate_threshold /\
    candidate_threshold <= bonded_count /\
    candidate_threshold <= valid_distinct_count.
Proof. intros; exact H. Qed.

Lemma zero_signature_approval_requires_zero_minimum :
  forall local_minimum bonded_count,
    approval_authorized local_minimum 0 bonded_count 0 ->
    local_minimum = 0.
Proof. intros local_minimum bonded_count [Hminimum _]. lia. Qed.

Lemma rejected_approval_preserves_state :
  forall local_minimum candidate_threshold bonded_count valid_distinct_count state,
    ~ approval_authorized local_minimum candidate_threshold bonded_count valid_distinct_count ->
    apply_approval local_minimum candidate_threshold bonded_count valid_distinct_count state = state.
Proof.
  intros local_minimum candidate_threshold bonded_count valid_distinct_count state Hreject.
  unfold apply_approval, approval_authorized in *.
  destruct (Nat.leb local_minimum candidate_threshold) eqn:Hlocal;
    destruct (Nat.leb candidate_threshold bonded_count) eqn:Hbonded;
    destruct (Nat.leb candidate_threshold valid_distinct_count) eqn:Hvalid;
    simpl; try reflexivity.
  apply Nat.leb_le in Hlocal.
  apply Nat.leb_le in Hbonded.
  apply Nat.leb_le in Hvalid.
  exfalso. apply Hreject. auto.
Qed.

Theorem genesis_approval_trust_correct :
  (forall local_minimum candidate_threshold bonded_count valid_distinct_count,
    approval_authorized local_minimum candidate_threshold bonded_count valid_distinct_count ->
    local_minimum <= candidate_threshold /\
    candidate_threshold <= bonded_count /\
    candidate_threshold <= valid_distinct_count)
  /\
  (forall local_minimum bonded_count,
    approval_authorized local_minimum 0 bonded_count 0 -> local_minimum = 0)
  /\
  (forall local_minimum candidate_threshold bonded_count valid_distinct_count state,
    ~ approval_authorized local_minimum candidate_threshold bonded_count valid_distinct_count ->
    apply_approval local_minimum candidate_threshold bonded_count valid_distinct_count state = state).
Proof.
  exact (conj approval_authorized_sound
    (conj zero_signature_approval_requires_zero_minimum rejected_approval_preserves_state)).
Qed.

Print Assumptions genesis_approval_trust_correct.
