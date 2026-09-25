From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia Sorting.Permutation.
Import ListNotations.

Section Usage.
Variable charge : nat -> nat.

Fixpoint usage (slots : list nat) : nat :=
  match slots with [] => 0 | slot :: tail => charge slot + usage tail end.

Theorem usage_append : forall prefix suffix,
  usage (prefix ++ suffix) = usage prefix + usage suffix.
Proof. induction prefix; intros; simpl; [reflexivity|rewrite IHprefix; lia]. Qed.

Theorem independent_publications_commute : forall before a b after,
  usage (before ++ a :: b :: after) = usage (before ++ b :: a :: after).
Proof. intros. repeat rewrite usage_append. simpl. lia. Qed.

Theorem publication_permutation_preserves_usage : forall left right,
  Permutation left right -> usage left = usage right.
Proof.
  intros left right permutation. induction permutation; simpl; try lia.
Qed.

Theorem unique_subset_usage_bound : forall completed universe,
  NoDup completed -> incl completed universe -> usage completed <= usage universe.
Proof.
  induction completed as [|slot tail IH]; intros universe unique included; simpl; [lia|].
  inversion unique as [|a rest absent tail_unique]; subst.
  assert (present : In slot universe) by (apply included; simpl; auto).
  apply in_split in present. destruct present as [before [after ->]].
  assert (tail_included : incl tail (before ++ after)).
  { intros item member. specialize (included item (or_intror member)).
    apply in_app_iff in included. apply in_app_iff.
    destruct included as [left|[same|right]]; auto.
    subst. contradiction. }
  specialize (IH (before ++ after) tail_unique tail_included).
  repeat rewrite usage_append in *. simpl. lia.
Qed.

Theorem publication_machine_bound : forall prefix slot universe maximum,
  NoDup (prefix ++ [slot]) -> incl (prefix ++ [slot]) universe ->
  usage universe <= maximum -> usage prefix + charge slot <= maximum.
Proof.
  intros. pose proof (unique_subset_usage_bound _ _ H H0).
  rewrite usage_append in H2. simpl in H2. lia.
Qed.

Theorem suffix_restore_exact : forall prefix suffix,
  usage (prefix ++ suffix) - usage suffix = usage prefix.
Proof. intros. rewrite usage_append. lia. Qed.

Theorem suffix_restore_preserves_bound : forall prefix suffix maximum,
  usage (prefix ++ suffix) <= maximum -> usage prefix <= maximum.
Proof. intros. rewrite usage_append in H. lia. Qed.

Theorem zero_charge_publication : forall prefix slot,
  charge slot = 0 -> usage (prefix ++ [slot]) = usage prefix.
Proof. intros. rewrite usage_append. simpl. lia. Qed.

Theorem complete_ownership_exact : forall completed universe,
  NoDup completed -> NoDup universe -> incl completed universe -> incl universe completed ->
  usage completed = usage universe.
Proof.
  intros. apply publication_permutation_preserves_usage.
  apply NoDup_Permutation; auto. intros. split; auto.
Qed.
End Usage.

Section PrivateInitialization.
Context {Input Output : Type}.
Variable prepare : Input -> option Output.

Fixpoint prepare_all (requests : list Input) : option (list Output) :=
  match requests with
  | [] => Some []
  | request :: tail =>
      match prepare request, prepare_all tail with
      | Some installed, Some remaining => Some (installed :: remaining)
      | _, _ => None
      end
  end.

Theorem private_initialization_complete : forall requests installed,
  prepare_all requests = Some installed ->
  Forall2 (fun request result => prepare request = Some result) requests installed.
Proof.
  induction requests as [|request tail IH]; intros installed success; simpl in success.
  - inversion success. constructor.
  - destruct (prepare request) eqn:head; [|discriminate].
    destruct (prepare_all tail) eqn:rest; [|discriminate].
    inversion success; subst. constructor; [exact head|now apply IH].
Qed.

Theorem private_initialization_preserves_request_count : forall requests installed,
  prepare_all requests = Some installed -> length requests = length installed.
Proof.
  intros. apply private_initialization_complete in H.
  induction H; simpl; congruence.
Qed.

Theorem private_initialization_failure_prevents_publication : forall before failed after,
  prepare failed = None -> prepare_all (before ++ failed :: after) = None.
Proof.
  induction before; intros failed after failure; simpl.
  - now rewrite failure.
  - rewrite IHbefore by exact failure. destruct (prepare a); reflexivity.
Qed.

Theorem private_initialization_append : forall left right installed_left installed_right,
  prepare_all left = Some installed_left -> prepare_all right = Some installed_right ->
  prepare_all (left ++ right) = Some (installed_left ++ installed_right).
Proof.
  induction left as [|request tail IH]; intros right installed_left installed_right first second; simpl in *.
  - inversion first. exact second.
  - destruct (prepare request) eqn:head; [|discriminate].
    destruct (prepare_all tail) eqn:rest; [|discriminate].
    inversion first; subst. rewrite (IH right l installed_right eq_refl second). reflexivity.
Qed.
End PrivateInitialization.

Section CompletedEvidence.
Context {Row : Type}.
Variable granted : Row -> bool.

Definition accepted_rows (rows : list Row) : list Row := filter granted rows.

Definition completion_evidence (complete : bool) (rows : list Row) : option (list Row) :=
  if complete then Some (accepted_rows rows) else None.

Theorem completion_evidence_requires_complete : forall complete rows accepted,
  completion_evidence complete rows = Some accepted -> complete = true.
Proof. intros [] rows accepted evidence; [reflexivity|discriminate]. Qed.

Theorem completion_evidence_exact : forall rows accepted,
  completion_evidence true rows = Some accepted -> accepted = filter granted rows.
Proof. intros rows accepted evidence. inversion evidence. reflexivity. Qed.

Theorem completion_evidence_membership : forall rows accepted row,
  completion_evidence true rows = Some accepted ->
  (In row accepted <-> In row rows /\ granted row = true).
Proof.
  intros rows accepted row evidence. apply completion_evidence_exact in evidence.
  rewrite evidence. apply filter_In.
Qed.

Theorem accepted_rows_append : forall left right,
  accepted_rows (left ++ right) = accepted_rows left ++ accepted_rows right.
Proof. intros. apply filter_app. Qed.

Theorem accepted_rows_capacity : forall rows,
  length (accepted_rows rows) <= length rows.
Proof.
  induction rows as [|row tail IH]; simpl; [lia|].
  unfold accepted_rows in *. simpl. destruct (granted row); simpl; lia.
Qed.

Theorem accepted_rows_preserve_permutation : forall left right,
  Permutation left right -> Permutation (accepted_rows left) (accepted_rows right).
Proof.
  intros left right permutation. unfold accepted_rows. induction permutation; simpl.
  - constructor.
  - destruct (granted x); [constructor|]; assumption.
  - destruct (granted x), (granted y); simpl; try apply Permutation_refl; apply perm_swap.
  - eapply Permutation_trans; eauto.
Qed.
End CompletedEvidence.

Section AuthorityProjection.
Context {Purse : Type}.
Variable debit : nat -> Purse -> nat.

Definition authority_total (events : list nat) (purse : Purse) : nat :=
  usage (fun event => debit event purse) events.

Definition authority_held (published pending : list nat) (purse : Purse) : nat :=
  authority_total published purse + authority_total pending purse.

Theorem authority_projection_append : forall left right purse,
  authority_total (left ++ right) purse =
  authority_total left purse + authority_total right purse.
Proof. intros. apply usage_append. Qed.

Theorem authority_publication_moves_reservation : forall published pending event purse,
  authority_held (published ++ [event]) pending purse =
  authority_held published (event :: pending) purse.
Proof.
  intros. unfold authority_held. rewrite authority_projection_append.
  unfold authority_total. simpl. lia.
Qed.

Theorem authority_cancellation_preserves_other_reservations :
  forall published before after event purse,
  authority_held published (before ++ event :: after) purse - debit event purse =
  authority_held published (before ++ after) purse.
Proof.
  intros. unfold authority_held. repeat rewrite authority_projection_append.
  unfold authority_total. simpl. lia.
Qed.

Theorem authority_independent_publications_commute : forall published a b purse,
  authority_total (published ++ [a; b]) purse =
  authority_total (published ++ [b; a]) purse.
Proof. intros. apply independent_publications_commute. Qed.

Theorem authority_publication_permutation : forall left right,
  Permutation left right -> forall purse,
  authority_total left purse = authority_total right purse.
Proof. intros. now apply publication_permutation_preserves_usage. Qed.

Theorem authority_unique_prefix_is_bounded : forall published pending universe capacity,
  NoDup (published ++ pending) -> incl (published ++ pending) universe ->
  (forall purse, authority_total universe purse <= capacity purse) ->
  forall purse, authority_held published pending purse <= capacity purse.
Proof.
  intros published pending universe capacity unique included bound purse.
  unfold authority_held. rewrite <- authority_projection_append.
  eapply Nat.le_trans; [apply unique_subset_usage_bound; eauto|apply bound].
Qed.

Theorem authority_restore_removes_exact_suffix : forall prefix suffix purse,
  authority_total (prefix ++ suffix) purse - authority_total suffix purse =
  authority_total prefix purse.
Proof. intros. apply suffix_restore_exact. Qed.

Theorem authority_complete_permutation_is_exact : forall completed universe,
  NoDup completed -> NoDup universe -> incl completed universe -> incl universe completed ->
  forall purse, authority_total completed purse = authority_total universe purse.
Proof. intros. now apply complete_ownership_exact. Qed.

Theorem authority_missing_nonzero_publication_is_detectable : forall before after event purse,
  0 < debit event purse ->
  authority_total (before ++ after) purse < authority_total (before ++ event :: after) purse.
Proof.
  intros. repeat rewrite authority_projection_append. unfold authority_total. simpl. lia.
Qed.

Theorem authority_pending_omission_can_overdraw : forall capacity pending event,
  0 < pending -> pending <= capacity -> event <= capacity -> capacity < pending + event ->
  event <= capacity /\ ~ pending + event <= capacity.
Proof. intros. lia. Qed.
End AuthorityProjection.

Inductive stage := Footprint | Introduction | Comm.
Definition stage_eq_dec : forall a b : stage, {a = b} + {a <> b}.
Proof. decide equality. Defined.

Definition observe (pending : list stage) (actual : stage) (authenticated : bool)
  : option (list stage) :=
  match pending with
  | [] => None
  | expected :: rest =>
      if authenticated then if stage_eq_dec actual expected then Some rest else None else None
  end.

Theorem observation_requires_authentication : forall pending actual rest,
  observe pending actual false <> Some rest.
Proof. intros [] actual rest; discriminate. Qed.

Theorem observation_preserves_stage_prefix : forall seen pending actual rest required,
  seen ++ pending = required -> observe pending actual true = Some rest ->
  (seen ++ [actual]) ++ rest = required.
Proof.
  intros seen [|expected tail] actual rest required prefix result; [discriminate|].
  unfold observe in result. destruct (stage_eq_dec actual expected); [|discriminate].
  subst actual. inversion result; subst rest. rewrite <- app_assoc. exact prefix.
Qed.

Theorem complete_stage_coverage : forall (seen required : list stage),
  seen ++ [] = required -> seen = required.
Proof. intros. rewrite app_nil_r in H. exact H. Qed.

Theorem terminal_stage_cannot_repeat : forall actual,
  observe [] actual true = None.
Proof. reflexivity. Qed.

Theorem comm_cannot_precede_introduction : forall remaining,
  observe (Introduction :: remaining) Comm true = None.
Proof. reflexivity. Qed.

Theorem introduction_requires_footprint : forall remaining,
  observe (Footprint :: remaining) Introduction true = None.
Proof. reflexivity. Qed.

Theorem accepted_introduction_denied_comm_usage : forall introduction,
  introduction + 0 = introduction.
Proof. intros. lia. Qed.

Definition publication_closed (closed aborted : bool) := closed || aborted.

Fixpoint publication_history (closed : bool) (aborts : list bool) : bool :=
  match aborts with
  | [] => closed
  | aborted :: rest => publication_history (publication_closed closed aborted) rest
  end.

Theorem successful_publication_preserves_closure : forall closed,
  publication_closed closed false = closed.
Proof. intros []; reflexivity. Qed.

Theorem failed_publication_closes_session : forall closed,
  publication_closed closed true = true.
Proof. intros []; reflexivity. Qed.

Theorem publication_history_cannot_reopen : forall aborts,
  publication_history true aborts = true.
Proof. induction aborts; simpl; assumption || reflexivity. Qed.

Theorem publication_history_exact : forall aborts closed,
  publication_history closed aborts = closed || existsb (fun aborted => aborted) aborts.
Proof.
  induction aborts; intro closed; simpl.
  - destruct closed; reflexivity.
  - rewrite IHaborts. unfold publication_closed.
    destruct closed, a; reflexivity.
Qed.

Theorem publication_history_any_failure_closes : forall aborts closed,
  In true aborts -> publication_history closed aborts = true.
Proof.
  induction aborts; intros closed member; [contradiction|].
  destruct member as [same | member].
  - subst a. simpl. rewrite failed_publication_closes_session.
    apply publication_history_cannot_reopen.
  - simpl. apply IHaborts. exact member.
Qed.

Theorem independent_publication_completion_commutes : forall closed left right,
  publication_closed (publication_closed closed left) right =
  publication_closed (publication_closed closed right) left.
Proof. intros [] [] []; reflexivity. Qed.

Inductive rejection_class := EconomicRejection | HostRejection | StructuralRejection.
Inductive space_rejection := PhloExhausted | WorkExhausted | OpaqueFailure.

Definition encode_rejection (reason : rejection_class) : space_rejection :=
  match reason with
  | EconomicRejection => PhloExhausted
  | HostRejection => WorkExhausted
  | StructuralRejection => OpaqueFailure
  end.

Definition decode_rejection (reason : space_rejection) : rejection_class :=
  match reason with
  | PhloExhausted => EconomicRejection
  | WorkExhausted => HostRejection
  | OpaqueFailure => StructuralRejection
  end.

Fixpoint rejection_transport (hops : nat) (reason : rejection_class) : rejection_class :=
  match hops with
  | 0 => reason
  | S remaining => rejection_transport remaining (decode_rejection (encode_rejection reason))
  end.

Theorem rejection_bridge_round_trip : forall reason,
  decode_rejection (encode_rejection reason) = reason.
Proof. intros []; reflexivity. Qed.

Theorem rejection_transport_preserves_class : forall hops reason,
  rejection_transport hops reason = reason.
Proof.
  induction hops; intros; simpl; [reflexivity|].
  rewrite rejection_bridge_round_trip. apply IHhops.
Qed.

Theorem host_rejection_never_becomes_economic : forall hops,
  rejection_transport hops HostRejection <> EconomicRejection.
Proof. intros. rewrite rejection_transport_preserves_class. discriminate. Qed.

Theorem opaque_rejection_has_no_resource_authority :
  decode_rejection OpaqueFailure <> HostRejection /\
  decode_rejection OpaqueFailure <> EconomicRejection.
Proof. split; discriminate. Qed.
