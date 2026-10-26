(* DR-113: exhaustion of the signed phlo limit is a classified user failure.

   A native execution contract names the source of its scalar resource bound.
   - SignedLimitSource: the bound is the signed phloLimit of the offer.
   - CertificateSource: the bound is a certified bound on the demand.

   The guard of the contract requires a signed-limit bound to equal the signed
   limit, and a certified bound to be at most the limit. An exhausted bound
   takes the class of its source: a user failure for the signed limit, a
   certificate failure for a certified bound. Replay classifies a denial with
   the source of the contract that checked its evidence, so play and replay
   agree. The meter computes a charge exactly: a zero factor gives a zero
   charge, and a checked addition that overflows the machine word exceeds every
   bound that fits the word.

   The module reuses the failure classes, the billability rule and the
   retained charge of SignedPhloControls. *)

From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import SignedPhloControls.
Import ListNotations.

(** * The bound source and its guard *)

Inductive bound_source :=
| SignedLimitSource
| CertificateSource.

(** The guard of [NativePhloExecutionContract::new]. *)
Definition admissible (source : bound_source) (bound limit : nat) : bool :=
  match source with
  | SignedLimitSource => Nat.eqb bound limit
  | CertificateSource => bound <=? limit
  end.

(** The class of an exhausted bound ([exhaustion_error] in the native runtime). *)
Definition exhaustion_failure (source : bound_source) : phlo_failure :=
  match source with
  | SignedLimitSource => PhloUserFailure
  | CertificateSource => PhloCertificateFailure
  end.

(** Play classifies a denial with the source of its contract. Replay
    classifies a denial with the source stored in the checked evidence, which
    the contract copies when it checks the journal. *)
Record checked_evidence := { evidence_source : bound_source }.

Definition check_journal (contract_source : bound_source) : checked_evidence :=
  {| evidence_source := contract_source |}.

Definition play_denial (contract_source : bound_source) : phlo_failure :=
  exhaustion_failure contract_source.

Definition replay_denial (evidence : checked_evidence) : phlo_failure :=
  exhaustion_failure (evidence_source evidence).

(** * Theorems *)

Theorem admissible_signed_limit_source_fixes_the_bound : forall bound limit,
  admissible SignedLimitSource bound limit = true -> bound = limit.
Proof. intros bound limit admitted. now apply Nat.eqb_eq. Qed.

Theorem admissible_bound_fits_the_limit : forall source bound limit,
  admissible source bound limit = true -> bound <= limit.
Proof.
  intros [|] bound limit admitted; simpl in admitted.
  - apply Nat.eqb_eq in admitted. lia.
  - now apply Nat.leb_le.
Qed.

Theorem signed_limit_exhaustion_is_billable :
  billable_failure (exhaustion_failure SignedLimitSource) = true.
Proof. reflexivity. Qed.

Theorem signed_limit_exhaustion_retains_fresh_work_and_fee : forall schedule fresh,
  retained_phlo_charge schedule fresh (PhloAccepted [exhaustion_failure SignedLimitSource]) =
  execution_phlo schedule fresh * phlo_actual_price schedule + 1.
Proof. intros. reflexivity. Qed.

Theorem signed_limit_exhaustion_within_signed_ceiling :
  forall version machine_max terms schedule bound available required used unused fresh,
  check_phlo_execution version machine_max terms schedule bound available required used unused fresh = true ->
  retained_phlo_charge schedule fresh (PhloAccepted [exhaustion_failure SignedLimitSource]) <=
  signed_charge_ceiling terms.
Proof.
  intros version machine_max terms schedule bound available required used unused fresh checked.
  destruct (checked_retained_charge_bounded version machine_max terms schedule bound
    available required used unused fresh
    (PhloAccepted [exhaustion_failure SignedLimitSource]) checked) as [ceiling _].
  exact ceiling.
Qed.

Theorem certificate_exhaustion_is_unbilled_for_every_admissible_bound :
  forall bound limit schedule fresh,
  admissible CertificateSource bound limit = true ->
  retained_phlo_charge schedule fresh (PhloAccepted [exhaustion_failure CertificateSource]) = 0.
Proof. intros. reflexivity. Qed.

(** The equality case: a certified bound equal to the limit stays unbilled. *)
Theorem certified_bound_equal_to_the_limit_is_unbilled : forall limit schedule fresh,
  admissible CertificateSource limit limit = true /\
  retained_phlo_charge schedule fresh (PhloAccepted [exhaustion_failure CertificateSource]) = 0.
Proof. intros. split; [simpl; apply Nat.leb_refl|reflexivity]. Qed.

Theorem platform_failure_vetoes_signed_limit_exhaustion : forall schedule fresh failures,
  In PhloPlatformFailure failures ->
  retained_phlo_charge schedule fresh
    (PhloAccepted (exhaustion_failure SignedLimitSource :: failures)) = 0.
Proof.
  intros schedule fresh failures present.
  apply (unsafe_failure_prevents_all_candidate_charge schedule fresh _ PhloPlatformFailure);
    [now right|reflexivity].
Qed.

Theorem play_and_replay_classify_identically : forall source,
  replay_denial (check_journal source) = play_denial source.
Proof. intros. reflexivity. Qed.

Theorem replay_of_a_signed_limit_denial_is_billable :
  billable_failure (replay_denial (check_journal SignedLimitSource)) = true.
Proof. reflexivity. Qed.

(** A checked addition fails when the exact sum exceeds the machine word. A
    bound that fits the word is then exceeded too. *)
Theorem sum_overflow_implies_bound_exceeded : forall used charge bound word,
  bound <= word -> word < used + charge -> bound < used + charge.
Proof. lia. Qed.

(** The product of a charge with a zero factor is exactly zero, so the zero
    short-circuit never hides a charge. *)
Theorem zero_factor_charge_is_exact : forall weight units quantity,
  weight = 0 \/ units = 0 \/ quantity = 0 -> weight * units * quantity = 0.
Proof. intros weight units quantity [-> | [-> | ->]]; lia. Qed.

(** * Negative controls

    Each control replaces one rule of the design with a plausible wrong rule
    and proves that the wrong rule breaks a required property. *)

(** N1: charging every exhaustion bills a certificate failure. *)
Definition charge_every_exhaustion (_ : bound_source) : phlo_failure := PhloUserFailure.

Theorem charge_every_exhaustion_bills_a_certified_bound : forall limit,
  admissible CertificateSource limit limit = true /\
  billable_failure (charge_every_exhaustion CertificateSource) = true.
Proof. intros. split; [simpl; apply Nat.leb_refl|reflexivity]. Qed.

(** N2: never charging an exhaustion leaves the fee of granted work unpaid. *)
Definition never_charge_exhaustion (_ : bound_source) : phlo_failure := PhloCertificateFailure.

Theorem never_charge_exhaustion_leaves_the_fee_unpaid : forall schedule,
  retained_phlo_charge schedule [] (PhloAccepted [never_charge_exhaustion SignedLimitSource]) = 0 /\
  retained_phlo_charge schedule [] (PhloAccepted [exhaustion_failure SignedLimitSource]) = 1.
Proof. intros. split; reflexivity. Qed.

(** N3: one shared class for both sources cannot meet both rules. *)
Theorem one_shared_class_cannot_meet_both_rules : forall shared : phlo_failure,
  billable_failure shared <> billable_failure (exhaustion_failure SignedLimitSource) \/
  billable_failure shared <> billable_failure (exhaustion_failure CertificateSource).
Proof. intros [| | |]; simpl; [right|left|left|left]; discriminate. Qed.

(** N4: a class derived by comparing the bound with the limit bills a certified
    bound equal to the limit. *)
Definition value_derived_class (bound limit : nat) : phlo_failure :=
  if Nat.eqb bound limit then PhloUserFailure else PhloCertificateFailure.

Theorem value_derived_class_bills_a_certified_bound : forall limit,
  admissible CertificateSource limit limit = true /\
  billable_failure (value_derived_class limit limit) = true.
Proof.
  intros. unfold value_derived_class. rewrite Nat.eqb_refl.
  split; [simpl; apply Nat.leb_refl|reflexivity].
Qed.
