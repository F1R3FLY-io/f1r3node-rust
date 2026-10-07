From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingPriceConsent.
Import ListNotations.

Record funding_terms := {
  required_owner_consents : list (nat * nat);
  signed_limit_terms : list bool;
  asset_identity : nat;
  schedule_identity : nat
}.

Definition required_ceilings (terms : funding_terms) : list nat :=
  map snd (required_owner_consents terms).

Record live_funding_right := {
  right_generation : nat;
  right_terms : funding_terms
}.

Record captured_reservation := {
  reservation_right : nat;
  reservation_generation : nat;
  reservation_terms : funding_terms;
  reservation_root : nat;
  reservation_price : nat
}.

Record consent_state := {
  live_rights : nat -> option live_funding_right;
  reservations : nat -> option captured_reservation;
  settled : nat -> bool
}.

Definition replace_at {A : Type} (table : nat -> A) (key : nat) (value : A)
  : nat -> A := fun queried => if queried =? key then value else table queried.

Definition transfer_right (state : consent_state) (key expected : nat)
  (next_terms : funding_terms) (authorized : bool) : option consent_state :=
  match live_rights state key with
  | None => None
  | Some previous =>
      if authorized && (expected =? right_generation previous) then
        Some {|
          live_rights := replace_at (live_rights state) key
            (Some {| right_generation := S (right_generation previous);
                     right_terms := next_terms |});
          reservations := reservations state;
          settled := settled state
        |}
      else None
  end.

Definition reserve_right (state : consent_state) (key expected id root price : nat)
  (funding_proof_valid : bool) : option consent_state :=
  match reservations state id, live_rights state key with
  | None, Some current =>
      match required_price_ceiling (required_ceilings (right_terms current)) with
      | None => None
      | Some ceiling =>
          if funding_proof_valid && (expected =? right_generation current) &&
             (price <=? ceiling) then
            Some {|
              live_rights := live_rights state;
              reservations := replace_at (reservations state) id
                (Some {| reservation_right := key;
                         reservation_generation := right_generation current;
                         reservation_terms := right_terms current;
                         reservation_root := root;
                         reservation_price := price |});
              settled := settled state
            |}
          else None
      end
  | _, _ => None
  end.

Definition settlement_evidence (state : consent_state) (id : nat)
  : option captured_reservation :=
  if settled state id then None else reservations state id.

Definition settle_reservation (state : consent_state) (id : nat)
  : option (captured_reservation * consent_state) :=
  match settlement_evidence state id with
  | None => None
  | Some captured => Some (captured, {|
      live_rights := live_rights state;
      reservations := reservations state;
      settled := replace_at (settled state) id true
    |})
  end.

Record transfer_command := {
  transfer_key : nat;
  transfer_expected : nat;
  transfer_terms : funding_terms;
  transfer_authorized : bool
}.

Fixpoint run_transfers (state : consent_state) (commands : list transfer_command)
  : option consent_state :=
  match commands with
  | [] => Some state
  | command :: rest =>
      match transfer_right state (transfer_key command) (transfer_expected command)
        (transfer_terms command) (transfer_authorized command) with
      | None => None
      | Some next => run_transfers next rest
      end
  end.

Theorem transfer_preserves_reservations : forall state key expected terms auth next,
  transfer_right state key expected terms auth = Some next ->
  reservations next = reservations state /\ settled next = settled state.
Proof.
  intros state key expected terms auth next result.
  unfold transfer_right in result.
  destruct (live_rights state key); try discriminate.
  destruct (auth && (expected =? right_generation l)); try discriminate.
  inversion result; subst. split; reflexivity.
Qed.

Theorem transfer_preserves_other_rights : forall state key expected terms auth next other,
  transfer_right state key expected terms auth = Some next ->
  other <> key -> live_rights next other = live_rights state other.
Proof.
  intros state key expected terms auth next other result distinct.
  unfold transfer_right in result.
  destruct (live_rights state key); try discriminate.
  destruct (auth && (expected =? right_generation l)); try discriminate.
  inversion result; subst. simpl. unfold replace_at.
  apply Nat.eqb_neq in distinct. rewrite distinct. reflexivity.
Qed.

Theorem transfer_advances_authorization : forall state key expected terms auth next previous,
  live_rights state key = Some previous ->
  transfer_right state key expected terms auth = Some next ->
  expected = right_generation previous /\
  live_rights next key = Some {|
    right_generation := S (right_generation previous); right_terms := terms |}.
Proof.
  intros state key expected terms auth next previous found result.
  unfold transfer_right in result. rewrite found in result.
  destruct (auth && (expected =? right_generation previous)) eqn:guard; try discriminate.
  apply andb_true_iff in guard. destruct guard as [_ same]. apply Nat.eqb_eq in same.
  inversion result; subst. split; [reflexivity|].
  simpl. unfold replace_at. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem transfer_rejects_previous_authorization : forall state key expected terms auth next again,
  transfer_right state key expected terms auth = Some next ->
  transfer_right next key expected again true = None.
Proof.
  intros state key expected terms auth next again result.
  destruct (live_rights state key) as [previous|] eqn:found.
  - pose proof (transfer_advances_authorization state key expected terms auth next
      previous found result) as [same updated].
    unfold transfer_right. rewrite updated. simpl. rewrite same.
    assert (right_generation previous =? S (right_generation previous) = false) as stale.
    { apply Nat.eqb_neq. lia. }
    rewrite stale. reflexivity.
  - unfold transfer_right in result. rewrite found in result. discriminate.
Qed.

Theorem arbitrary_transfer_history_preserves_reservations : forall commands state next,
  run_transfers state commands = Some next ->
  reservations next = reservations state /\ settled next = settled state.
Proof.
  intros commands. induction commands as [|command rest IH]; intros state next result.
  - simpl in result. inversion result; subst. split; reflexivity.
  - simpl in result.
    destruct (transfer_right state (transfer_key command) (transfer_expected command)
      (transfer_terms command) (transfer_authorized command)) as [middle|] eqn:step;
      try discriminate.
    pose proof (transfer_preserves_reservations _ _ _ _ _ _ step) as [before flag_before].
    pose proof (IH middle next result) as [after flag_after].
    split; congruence.
Qed.

Theorem arbitrary_transfer_history_preserves_settlement_evidence :
  forall commands state next id,
  run_transfers state commands = Some next ->
  settlement_evidence next id = settlement_evidence state id.
Proof.
  intros commands state next id result.
  pose proof (arbitrary_transfer_history_preserves_reservations commands state next result)
    as [same flags].
  unfold settlement_evidence. rewrite same, flags. reflexivity.
Qed.

Theorem reservation_captures_current_consent : forall state key expected id root price valid next,
  reserve_right state key expected id root price valid = Some next ->
  exists current,
    live_rights state key = Some current /\
    reservations next id = Some {|
      reservation_right := key;
      reservation_generation := right_generation current;
      reservation_terms := right_terms current;
      reservation_root := root;
      reservation_price := price |} /\
    Forall (fun maximum => price <= maximum) (required_ceilings (right_terms current)).
Proof.
  intros state key expected id root price valid next result.
  unfold reserve_right in result.
  destruct (reservations state id); try discriminate.
  destruct (live_rights state key) as [current|] eqn:found; try discriminate.
  destruct (required_price_ceiling (required_ceilings (right_terms current)))
    as [ceiling|] eqn:computed; try discriminate.
  destruct (valid && (expected =? right_generation current) && (price <=? ceiling))
    eqn:guard; try discriminate.
  apply andb_true_iff in guard. destruct guard as [_ bounded].
  apply Nat.leb_le in bounded.
  inversion result; subst. exists current. split; [reflexivity|]. split.
  - simpl. unfold replace_at. rewrite Nat.eqb_refl. reflexivity.
  - apply (proj1 (required_price_ceiling_checks_every_consent _ _ _ computed)).
    assumption.
Qed.

Theorem settled_reservation_cannot_settle_twice : forall state id captured next,
  settle_reservation state id = Some (captured, next) ->
  settle_reservation next id = None.
Proof.
  intros state id captured next result.
  unfold settle_reservation in result.
  destruct (settlement_evidence state id); try discriminate.
  inversion result; subst.
  unfold settle_reservation, settlement_evidence. simpl.
  unfold replace_at. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem arbitrary_authorized_term_sequence : forall replacements state key current,
  live_rights state key = Some current ->
  exists commands next final_right,
    map transfer_terms commands = replacements /\
    length commands = length replacements /\
    run_transfers state commands = Some next /\
    live_rights next key = Some final_right /\
    right_generation final_right = right_generation current + length replacements.
Proof.
  intros replacements. induction replacements as [|replacement rest IH];
    intros state key current found.
  - exists [], state, current. repeat split; simpl; auto.
  - set (next := {|
      live_rights := replace_at (live_rights state) key
        (Some {| right_generation := S (right_generation current);
                 right_terms := replacement |});
      reservations := reservations state;
      settled := settled state |}).
    assert (live_rights next key = Some {|
      right_generation := S (right_generation current); right_terms := replacement |}) as updated.
    { unfold next. simpl. unfold replace_at. rewrite Nat.eqb_refl. reflexivity. }
    destruct (IH next key _ updated) as
      [commands [last [final_right [terms [size [ran [last_found generation]]]]]]].
    exists ({| transfer_key := key; transfer_expected := right_generation current;
               transfer_terms := replacement; transfer_authorized := true |} :: commands),
      last, final_right.
    split; [simpl; f_equal; assumption|].
    split; [simpl; lia|]. split.
    + simpl. unfold transfer_right at 1. rewrite found. simpl.
      rewrite Nat.eqb_refl. exact ran.
    + split; [assumption|]. simpl in generation. simpl. lia.
Qed.

Theorem no_fixed_transfer_count : forall count state key current (replacement : funding_terms),
  live_rights state key = Some current ->
  exists commands next,
    length commands = count /\ run_transfers state commands = Some next.
Proof.
  intros count state key current replacement found.
  destruct (arbitrary_authorized_term_sequence (repeat replacement count) state key current found)
    as [commands [next [final_right [_ [size [ran _]]]]]].
  exists commands, next. split; [rewrite repeat_length in size; assumption|assumption].
Qed.

Definition original_terms : funding_terms := {|
  required_owner_consents := [(10, 3); (11, 5)];
  signed_limit_terms := [true]; asset_identity := 1; schedule_identity := 7
|}.

Definition acquired_terms : funding_terms := {|
  required_owner_consents := [(20, 8); (21, 9); (22, 10)];
  signed_limit_terms := [false]; asset_identity := 1; schedule_identity := 8
|}.

Definition consent_example_start : consent_state := {|
  live_rights := fun key => if key =? 0 then
    Some {| right_generation := 0; right_terms := original_terms |} else None;
  reservations := fun _ => None;
  settled := fun _ => false
|}.

Definition owner_return_commands : list transfer_command := [
  {| transfer_key := 0; transfer_expected := 0;
     transfer_terms := acquired_terms; transfer_authorized := true |};
  {| transfer_key := 0; transfer_expected := 1;
     transfer_terms := original_terms; transfer_authorized := true |}
].

Example owner_return_does_not_restore_old_authorization :
  match run_transfers consent_example_start owner_return_commands with
  | None => False
  | Some next =>
      live_rights next 0 = Some {| right_generation := 2; right_terms := original_terms |} /\
      reserve_right next 0 0 12 99 2 true = None
  end.
Proof. cbv. split; reflexivity. Qed.

Definition unsafe_current_owner_terms (state : consent_state) (id : nat)
  : option funding_terms :=
  match reservations state id with
  | None => None
  | Some captured =>
      match live_rights state (reservation_right captured) with
      | None => None
      | Some current => Some (right_terms current)
      end
  end.

Example current_owner_substitution_changes_reserved_consent :
  match reserve_right consent_example_start 0 0 12 99 2 true with
  | None => False
  | Some reserved =>
      match transfer_right reserved 0 0 acquired_terms true with
      | None => False
      | Some transferred =>
          option_map reservation_terms (settlement_evidence transferred 12) = Some original_terms /\
          unsafe_current_owner_terms transferred 12 = Some acquired_terms /\
          unsafe_current_owner_terms transferred 12 <>
            option_map reservation_terms (settlement_evidence transferred 12)
      end
  end.
Proof. cbv. repeat split; discriminate || reflexivity. Qed.

Print Assumptions transfer_preserves_reservations.
Print Assumptions transfer_preserves_other_rights.
Print Assumptions transfer_advances_authorization.
Print Assumptions transfer_rejects_previous_authorization.
Print Assumptions arbitrary_transfer_history_preserves_reservations.
Print Assumptions arbitrary_transfer_history_preserves_settlement_evidence.
Print Assumptions reservation_captures_current_consent.
Print Assumptions settled_reservation_cannot_settle_twice.
Print Assumptions no_fixed_transfer_count.
Print Assumptions arbitrary_authorized_term_sequence.
