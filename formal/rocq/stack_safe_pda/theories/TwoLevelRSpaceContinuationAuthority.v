From Stdlib Require Import
  Bool List Arith Lia ZArith Sorting.Sorted.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase TwoLevelRSpaceAuthority RhoPathV1Datum.

Import ListNotations.
Import TwoLevelRSpaceAuthority.
Set Implicit Arguments.
Open Scope Z_scope.

(** Exact continuation authority for the two-level PathMap RSpace.

    The functions in this model are ghost views of exact PathMap lookups.  A
    continuation group is the ordered channel vector itself; no hash, prefix,
    or reordered projection may authorize a commit.  Ordinary continuation
    paths are captured by an issued capability and are subsequently resolved
    by exact path, so insertions cannot retarget an older ordinal.

    Epoch tokens are sealed behind an equality-and-freshness interface.  In
    particular, clients cannot derive authorization from an ordering or from
    arithmetic on their representation. *)
Module TwoLevelRSpaceContinuationAuthority.

Definition GroupKey : Type := list Path.

Definition group_eq_dec : forall left right : GroupKey,
    {left = right} + {left <> right} :=
  list_eq_dec path_eq_dec.

Module EqualityEpoch.
  (** The nonce is a constructive freshness witness.  Every authorization
      definition below exposes and uses only [eq_dec], [fresh], and
      [fresh_is_distinct]; no ordering or arithmetic comparison is an
      authorization operation. *)
  Inductive t : Type := Token : nat -> t.

  Definition eq_dec : forall left right : t, {left = right} + {left <> right}.
  Proof. decide equality; apply Nat.eq_dec. Defined.

  Definition initial : t := Token 0.
  Definition fresh (current : t) : t :=
    match current with Token nonce => Token (S nonce) end.

  Definition nonce (epoch : t) : nat :=
    match epoch with Token value => value end.

  Fixpoint maximum_nonce (live : list t) : nat :=
    match live with
    | [] => 0
    | epoch :: tail => Nat.max (nonce epoch) (maximum_nonce tail)
    end.

  Definition fresh_from (live : list t) : t :=
    Token (S (maximum_nonce live)).

  Theorem fresh_is_distinct : forall current, fresh current <> current.
  Proof. intros [nonce] Heq. inversion Heq. lia. Qed.

  Lemma member_nonce_is_bounded : forall live prior,
    In prior live -> (nonce prior <= maximum_nonce live)%nat.
  Proof.
    induction live as [|head tail IH]; intros prior Hin; [contradiction|].
    simpl. destruct Hin as [Heq|Hin].
    - subst. apply Nat.le_max_l.
    - eapply Nat.le_trans; [now apply IH|apply Nat.le_max_r].
  Qed.

  Theorem fresh_from_rejects_every_prior : forall live prior,
    In prior live -> fresh_from live <> prior.
  Proof.
    intros live [prior_nonce] Hin Heq. inversion Heq.
    pose proof (@member_nonce_is_bounded live (Token prior_nonce) Hin) as Hbound.
    simpl in Hbound. lia.
  Qed.
End EqualityEpoch.

Definition i32_min : Z := -2147483648.
Definition i32_max : Z := 2147483647.

Definition peek_in_i32_range (peek : Z) : Prop :=
  i32_min <= peek <= i32_max.

Definition canonical_peeks (peeks : list Z) : Prop :=
  StronglySorted Z.lt peeks /\ Forall peek_in_i32_range peeks.

Record ContinuationRecord : Type := ContinuationRecordState {
  continuation_identity : Path;
  continuation_source : Path;
  continuation_persistent : bool;
  continuation_peeks : list Z
}.

Definition continuation_record_valid (record : ContinuationRecord) : Prop :=
  path_valid (continuation_identity record) = true /\
  path_valid (continuation_source record) = true /\
  canonical_peeks (continuation_peeks record).

Definition peek_in_i32_rangeb (peek : Z) : bool :=
  Z.leb i32_min peek && Z.leb peek i32_max.

Fixpoint canonical_peeksb (peeks : list Z) : bool :=
  match peeks with
  | [] => true
  | [peek] => peek_in_i32_rangeb peek
  | previous :: ((next :: tail) as remaining) =>
      peek_in_i32_rangeb previous && Z.ltb previous next &&
      canonical_peeksb remaining
  end.

Definition continuation_record_boundary_validb
    (record : ContinuationRecord) : bool :=
  path_valid (continuation_identity record) &&
  path_valid (continuation_source record) &&
  canonical_peeksb (continuation_peeks record).

(** The public commit ticket mirrors production [StoredContinuationCommit]:
    record identity is deliberately absent.  Installed-slot identity or the
    epoch-bound exact ordinary path is the identity authority; the caller may
    additionally attest only the fields that production carries. *)
Record ContinuationExpectation : Type := ContinuationExpectationState {
  expected_source : Path;
  expected_persistent : bool;
  expected_peeks : list Z
}.

Definition expectation_of_record (record : ContinuationRecord)
    : ContinuationExpectation :=
  ContinuationExpectationState
    (continuation_source record)
    (continuation_persistent record)
    (continuation_peeks record).

Definition expectation_matches_record
    (expected : ContinuationExpectation) (record : ContinuationRecord) : Prop :=
  continuation_source record = expected_source expected /\
  continuation_persistent record = expected_persistent expected /\
  continuation_peeks record = expected_peeks expected.

Definition expectation_matches_record_dec
    (expected : ContinuationExpectation) (record : ContinuationRecord)
    : {expectation_matches_record expected record} +
      {~ expectation_matches_record expected record}.
Proof.
  unfold expectation_matches_record.
  destruct (path_eq_dec (continuation_source record) (expected_source expected))
    as [Hsource|Hsource].
  - destruct (Bool.bool_dec
      (continuation_persistent record) (expected_persistent expected))
      as [Hpersistent|Hpersistent].
    + destruct (list_eq_dec Z.eq_dec
        (continuation_peeks record) (expected_peeks expected))
        as [Hpeeks|Hpeeks].
      * left. repeat split; assumption.
      * right. intros [_ [_ H]]. contradiction.
    + right. intros [_ [H _]]. contradiction.
  - right. intros [H _]. contradiction.
Defined.

Theorem expectation_match_is_exact : forall expected record,
  expectation_matches_record expected record ->
  continuation_source record = expected_source expected /\
  continuation_persistent record = expected_persistent expected /\
  continuation_peeks record = expected_peeks expected.
Proof. intros. exact H. Qed.

Definition continuation_record_eq_dec
    (left right : ContinuationRecord) : {left = right} + {left <> right}.
Proof.
  destruct left as [li ls lp lk].
  destruct right as [ri rs rp rk].
  destruct (path_eq_dec li ri) as [Hi|Hi]; [subst ri|right; congruence].
  destruct (path_eq_dec ls rs) as [Hs|Hs]; [subst rs|right; congruence].
  destruct (Bool.bool_dec lp rp) as [Hp|Hp]; [subst rp|right; congruence].
  destruct (list_eq_dec Z.eq_dec lk rk) as [Hk|Hk].
  - subst rk. left. reflexivity.
  - right. congruence.
Defined.

Theorem canonical_peeks_are_unique : forall peeks,
  canonical_peeks peeks -> NoDup peeks.
Proof.
  intros peeks [Hsorted _].
  induction Hsorted as [|head tail Hsorted IH Hall].
  - constructor.
  - constructor; [|exact IH].
    intro Hin. apply Forall_forall with (x := head) in Hall; [lia|exact Hin].
Qed.

Record OrdinaryContinuation : Type := OrdinaryContinuationState {
  ordinary_path : Path;
  ordinary_record : ContinuationRecord
}.

Definition ordinary_paths (ordinary : list OrdinaryContinuation) : list Path :=
  map ordinary_path ordinary.

(** Exact production record-address refinement.  [GroupKey] members denote
    already-canonical channel identity bytes, [continuation_identity] denotes
    the exact frozen source-free identity bytes (which commit persist/peeks),
    and [continuation_source] denotes the exact bincode source bytes. *)
Fixpoint escape_address_bytes (input : Path) : Path :=
  match input with
  | [] => []
  | byte :: tail =>
      if Nat.eqb byte 0 then
        0%nat :: 255%nat :: escape_address_bytes tail
      else byte :: escape_address_bytes tail
  end.

Definition frame_address_bytes (input : Path) : Path :=
  escape_address_bytes input ++ [0%nat; 0%nat].

Definition continuation_group_prefix (group : GroupKey) : Path :=
  [82%nat; 72%nat; 80%nat; 1%nat; 2%nat; 1%nat] ++
  concat (map frame_address_bytes group) ++ [0%nat; 1%nat].

Definition canonical_continuation_path
    (group : GroupKey) (record : ContinuationRecord) : Path :=
  continuation_group_prefix group ++
  continuation_identity record ++ continuation_source record.

Definition ordinary_path_is_canonical
    (group : GroupKey) (ordinary : OrdinaryContinuation) : Prop :=
  ordinary_path ordinary =
    canonical_continuation_path group (ordinary_record ordinary).

Fixpoint lookup_ordinary
    (selected : Path) (ordinary : list OrdinaryContinuation)
    : option ContinuationRecord :=
  match ordinary with
  | [] => None
  | entry :: tail =>
      if path_eq_dec selected (ordinary_path entry)
      then Some (ordinary_record entry)
      else lookup_ordinary selected tail
  end.

Fixpoint remove_ordinary_exact
    (selected : Path) (ordinary : list OrdinaryContinuation)
    : list OrdinaryContinuation :=
  match ordinary with
  | [] => []
  | entry :: tail =>
      if path_eq_dec selected (ordinary_path entry)
      then tail
      else entry :: remove_ordinary_exact selected tail
  end.

Definition ContinuationMap : Type := GroupKey -> list OrdinaryContinuation.
Definition InstalledContinuationMap : Type := GroupKey -> option ContinuationRecord.
Definition EpochMap : Type := GroupKey -> EqualityEpoch.t.
Definition JoinMap : Type := Path -> list GroupKey.

Record ContinuationAuthority : Type := ContinuationAuthorityState {
  installed_continuations : InstalledContinuationMap;
  ordinary_continuations : ContinuationMap;
  continuation_epochs : EpochMap;
  resting_joins : JoinMap;
  installed_joins : JoinMap
}.

(** Reachable production mutations bump the exact group's epoch.  Therefore
    equality of the Arc-backed epoch witnesses preserves that group's
    installed slot and ordinary catalog.  Snapshot reconstruction issues no
    outstanding capabilities, so it begins a new observation universe. *)
Definition reachable_same_epoch_group
    (observed commit : ContinuationAuthority) (group : GroupKey) : Prop :=
  continuation_epochs commit group = continuation_epochs observed group ->
  installed_continuations commit group = installed_continuations observed group /\
  ordinary_continuations commit group = ordinary_continuations observed group.

Definition continuation_authority_valid (state : ContinuationAuthority) : Prop :=
  (forall group, NoDup (ordinary_paths (ordinary_continuations state group))) /\
  (forall group ordinary,
      In ordinary (ordinary_continuations state group) ->
      path_valid (ordinary_path ordinary) = true /\
      continuation_record_valid (ordinary_record ordinary) /\
      ordinary_path_is_canonical group ordinary) /\
  (forall group record,
      installed_continuations state group = Some record ->
      continuation_persistent record = true /\ continuation_record_valid record) /\
  (forall channel, NoDup (resting_joins state channel)) /\
  (forall channel, NoDup (installed_joins state channel)).

Definition set_group {A : Type}
    (authority : GroupKey -> A) (selected : GroupKey) (value : A)
    : GroupKey -> A :=
  fun group => if group_eq_dec group selected then value else authority group.

Lemma set_group_exact : forall A (authority : GroupKey -> A) selected value,
  set_group authority selected value selected = value.
Proof.
  intros. unfold set_group.
  destruct (group_eq_dec selected selected); [reflexivity|contradiction].
Qed.

Lemma set_group_other : forall A (authority : GroupKey -> A) selected value other,
  other <> selected ->
  set_group authority selected value other = authority other.
Proof.
  intros. unfold set_group.
  destruct (group_eq_dec other selected); [contradiction|reflexivity].
Qed.

Record ContinuationCapability : Type := ContinuationCapabilityState {
  capability_group : GroupKey;
  capability_index : Z
}.

Definition issue_capability
    (state : ContinuationAuthority) (group : GroupKey) (index : Z)
    : ContinuationCapability :=
  ContinuationCapabilityState
    group index.

Definition installed_offset (state : ContinuationAuthority) (group : GroupKey) : Z :=
  match installed_continuations state group with
  | Some _ => 1
  | None => 0
  end.

Definition issue_installed_capability
    (state : ContinuationAuthority) (group : GroupKey) : ContinuationCapability :=
  issue_capability state group 0.

Definition issue_ordinary_capability
    (state : ContinuationAuthority) (group : GroupKey) (ordinal : nat)
    : ContinuationCapability :=
  issue_capability state group
    (Z.of_nat ordinal + installed_offset state group).

Inductive ResolvedTarget : Type :=
| InstalledTarget (record : ContinuationRecord)
| OrdinaryTarget (path : Path) (record : ContinuationRecord).

Definition target_record (target : ResolvedTarget) : ContinuationRecord :=
  match target with
  | InstalledTarget record | OrdinaryTarget _ record => record
  end.

Definition target_boundary_validb (target : ResolvedTarget) : bool :=
  match target with
  | InstalledTarget record => continuation_record_boundary_validb record
  | OrdinaryTarget path record =>
      path_valid path && continuation_record_boundary_validb record
  end.

Definition resolve_bound_ordinary
    (state : ContinuationAuthority) (group : GroupKey)
    (bound_paths : list Path) (index : Z) : option ResolvedTarget :=
  if Z_lt_ge_dec index 0 then None
  else
    match nth_error bound_paths (Z.to_nat index) with
    | None => None
    | Some path =>
        match lookup_ordinary path (ordinary_continuations state group) with
        | Some record => Some (OrdinaryTarget path record)
        | None => None
        end
    end.

Definition resolve_capability
    (observed commit : ContinuationAuthority) (requested_group : GroupKey)
    (capability : ContinuationCapability) : option ResolvedTarget :=
  if group_eq_dec requested_group (capability_group capability) then
    if EqualityEpoch.eq_dec
        (continuation_epochs commit requested_group)
        (continuation_epochs observed requested_group) then
      match installed_continuations observed requested_group with
      | Some observed_record =>
          if Z.eq_dec (capability_index capability) 0
          then
            match installed_continuations commit requested_group with
            | Some current_record => Some (InstalledTarget current_record)
            | None => None
            end
          else resolve_bound_ordinary commit requested_group
            (ordinary_paths (ordinary_continuations observed requested_group))
            (capability_index capability - 1)
      | None => resolve_bound_ordinary commit requested_group
          (ordinary_paths (ordinary_continuations observed requested_group))
          (capability_index capability)
      end
    else None
  else None.

Definition resolve_exact_continuation
    (observed commit : ContinuationAuthority) (requested_group : GroupKey)
    (capability : ContinuationCapability) (expected : ContinuationExpectation)
    : option ResolvedTarget :=
  match resolve_capability observed commit requested_group capability with
  | Some target =>
      if target_boundary_validb target then
        if expectation_matches_record_dec expected (target_record target)
        then Some target else None
      else None
  | None => None
  end.

(** Production constructs the expectation from the observed stored target;
    arbitrary caller-created expectations are useful rejection inputs but are
    not authorization witnesses. *)
Definition issue_continuation_expectation
    (observed : ContinuationAuthority) (group : GroupKey)
    (capability : ContinuationCapability) : option ContinuationExpectation :=
  match resolve_capability observed observed group capability with
  | Some target =>
      if target_boundary_validb target
      then Some (expectation_of_record (target_record target))
      else None
  | None => None
  end.

Theorem issued_installed_capability_resolves : forall state group record,
  installed_continuations state group = Some record ->
  resolve_capability state state group (issue_installed_capability state group) =
    Some (InstalledTarget record).
Proof.
  intros state group record Hinstalled.
  unfold resolve_capability, issue_installed_capability, issue_capability; simpl.
  destruct (group_eq_dec group group); [|contradiction].
  destruct (EqualityEpoch.eq_dec
    (continuation_epochs state group) (continuation_epochs state group));
    [|contradiction].
  rewrite Hinstalled. destruct (Z.eq_dec 0 0); [|contradiction].
  reflexivity.
Qed.

Lemma lookup_ordinary_nth_error_unique : forall ordinary ordinal record,
  NoDup (ordinary_paths ordinary) ->
  nth_error ordinary ordinal = Some record ->
  lookup_ordinary (ordinary_path record) ordinary = Some (ordinary_record record).
Proof.
  induction ordinary as [|head tail IH]; intros ordinal record Hunique Hnth;
    [destruct ordinal; discriminate|].
  inversion Hunique as [|head_path tail_paths Hnotin Htail]; subst.
  destruct ordinal as [|ordinal].
  - simpl in Hnth. inversion Hnth; subst record. simpl.
    destruct (path_eq_dec (ordinary_path head) (ordinary_path head));
      [reflexivity|contradiction].
  - simpl in Hnth. simpl.
    destruct (path_eq_dec (ordinary_path record) (ordinary_path head))
      as [Heq|Hneq].
    + exfalso. apply Hnotin.
      pose proof (@map_nth_error OrdinaryContinuation Path ordinary_path
        ordinal tail record Hnth) as Hpath.
      apply nth_error_In in Hpath. now rewrite Heq in Hpath.
    + eapply IH; eassumption.
Qed.

Theorem issued_ordinary_capability_resolves : forall state group ordinal record,
  continuation_authority_valid state ->
  nth_error (ordinary_continuations state group) ordinal = Some record ->
  resolve_capability state state group
    (issue_ordinary_capability state group ordinal) =
  Some (OrdinaryTarget (ordinary_path record) (ordinary_record record)).
Proof.
  intros state group ordinal record
    [Hunique [Hordinary_valid
      [Hinstalled_valid [Hresting_unique Hinstalled_join_unique]]]] Hnth.
  unfold resolve_capability, issue_ordinary_capability, issue_capability; simpl.
  destruct (group_eq_dec group group); [|contradiction].
  destruct (EqualityEpoch.eq_dec
    (continuation_epochs state group) (continuation_epochs state group));
    [|contradiction].
  unfold installed_offset.
  destruct (installed_continuations state group) as [installed|] eqn:Hinstalled.
  - destruct (Z.eq_dec (Z.of_nat ordinal + 1) 0) as [Heq|Hneq]; [lia|].
    unfold resolve_bound_ordinary.
    destruct (Z_lt_ge_dec (Z.of_nat ordinal + 1 - 1) 0); [lia|].
    replace (Z.to_nat (Z.of_nat ordinal + 1 - 1)) with ordinal by
      (replace (Z.of_nat ordinal + 1 - 1) with (Z.of_nat ordinal) by lia;
       symmetry; apply Nat2Z.id).
    pose proof (@map_nth_error OrdinaryContinuation Path ordinary_path
      ordinal (ordinary_continuations state group) record Hnth) as Hpath.
    unfold ordinary_paths.
    rewrite Hpath.
    rewrite (@lookup_ordinary_nth_error_unique
      (ordinary_continuations state group) ordinal record (Hunique group) Hnth).
    reflexivity.
  - unfold resolve_bound_ordinary.
    destruct (Z_lt_ge_dec (Z.of_nat ordinal + 0) 0); [lia|].
    replace (Z.to_nat (Z.of_nat ordinal + 0)) with ordinal by
      (replace (Z.of_nat ordinal + 0) with (Z.of_nat ordinal) by lia;
       symmetry; apply Nat2Z.id).
    pose proof (@map_nth_error OrdinaryContinuation Path ordinary_path
      ordinal (ordinary_continuations state group) record Hnth) as Hpath.
    unfold ordinary_paths.
    rewrite Hpath.
    rewrite (@lookup_ordinary_nth_error_unique
      (ordinary_continuations state group) ordinal record (Hunique group) Hnth).
    reflexivity.
Qed.

Theorem group_mismatch_cannot_resolve : forall observed commit requested capability,
  requested <> capability_group capability ->
  resolve_capability observed commit requested capability = None.
Proof.
  intros. unfold resolve_capability.
  destruct (group_eq_dec requested (capability_group capability));
    [contradiction|reflexivity].
Qed.

Theorem stale_epoch_cannot_resolve : forall observed commit group capability,
  capability_group capability = group ->
  continuation_epochs commit group <> continuation_epochs observed group ->
  resolve_capability observed commit group capability = None.
Proof.
  intros observed commit group capability Hgroup Hstale. subst.
  unfold resolve_capability.
  destruct (group_eq_dec
    (capability_group capability) (capability_group capability));
    [|contradiction].
  destruct (EqualityEpoch.eq_dec
    (continuation_epochs commit (capability_group capability))
    (continuation_epochs observed (capability_group capability)));
    [contradiction|reflexivity].
Qed.

Theorem exact_resolution_refines_capability_and_expectation : forall
    observed commit group capability expected target,
  resolve_exact_continuation observed commit group capability expected = Some target ->
  resolve_capability observed commit group capability = Some target /\
  target_boundary_validb target = true /\
  expectation_matches_record expected (target_record target).
Proof.
  intros observed commit group capability expected target Hresolve.
  unfold resolve_exact_continuation in Hresolve.
  destruct (resolve_capability observed commit group capability) as [actual|]
    eqn:Hcap; [|discriminate].
  destruct (target_boundary_validb actual) eqn:Hvalid; [|discriminate].
  destruct (expectation_matches_record_dec expected (target_record actual))
    as [Hmatches|Hreject]; [|discriminate].
  injection Hresolve as Heq. subst actual.
  split; [reflexivity|]. split; [exact Hvalid|exact Hmatches].
Qed.

Lemma reachable_same_epoch_resolves_observed_layout : forall
    observed commit group capability target,
  reachable_same_epoch_group observed commit group ->
  resolve_capability observed commit group capability = Some target ->
  resolve_capability observed observed group capability = Some target.
Proof.
  intros observed commit group capability target Hreachable Hresolve.
  assert (Hepoch : continuation_epochs commit group =
      continuation_epochs observed group).
  { pose proof Hresolve as Hcheck. unfold resolve_capability in Hcheck.
    destruct (group_eq_dec group (capability_group capability)); [|discriminate].
    destruct (EqualityEpoch.eq_dec
      (continuation_epochs commit group) (continuation_epochs observed group));
      [assumption|discriminate]. }
  destruct (Hreachable Hepoch) as [Hinstalled Hordinary].
  unfold resolve_capability, resolve_bound_ordinary in Hresolve |- *.
  rewrite Hepoch, Hinstalled, Hordinary in Hresolve.
  exact Hresolve.
Qed.

(** Public success capstone: the expectation was issued from the observed
    target, and the reachable same-epoch witness prevents arbitrary formal
    states from treating epoch equality as layout equality. *)
Theorem issued_expectation_exact_resolution_is_observed_target : forall
    observed commit group capability expected target,
  reachable_same_epoch_group observed commit group ->
  issue_continuation_expectation observed group capability = Some expected ->
  resolve_exact_continuation observed commit group capability expected =
    Some target ->
  resolve_capability observed observed group capability = Some target /\
  expected = expectation_of_record (target_record target).
Proof.
  intros observed commit group capability expected target Hreachable Hissued Hresolve.
  destruct (@exact_resolution_refines_capability_and_expectation
    observed commit group capability expected target Hresolve)
    as [Hcap [Hboundary Hmatches]].
  pose proof (@reachable_same_epoch_resolves_observed_layout
    observed commit group capability target Hreachable Hcap) as Hobserved.
  unfold issue_continuation_expectation in Hissued. rewrite Hobserved in Hissued.
  rewrite Hboundary in Hissued. injection Hissued as Heq. subst expected.
  split; [exact Hobserved|reflexivity].
Qed.

Theorem exact_resolution_preserves_expected_source_persistence_and_peeks : forall
    observed commit group capability expected target,
  resolve_exact_continuation observed commit group capability expected = Some target ->
  continuation_source (target_record target) = expected_source expected /\
  continuation_persistent (target_record target) =
      expected_persistent expected /\
  continuation_peeks (target_record target) = expected_peeks expected.
Proof.
  intros observed commit group capability expected target H.
  destruct (@exact_resolution_refines_capability_and_expectation
    observed commit group capability expected target H) as [_ [_ Hmatches]].
  exact Hmatches.
Qed.

Lemma resolve_bound_ordinary_is_live : forall state group bound index path record,
  resolve_bound_ordinary state group bound index =
      Some (OrdinaryTarget path record) ->
  lookup_ordinary path (ordinary_continuations state group) = Some record.
Proof.
  intros. unfold resolve_bound_ordinary in H.
  destruct (Z_lt_ge_dec index 0); [discriminate|].
  destruct (nth_error bound (Z.to_nat index)) as [selected|] eqn:Hnth;
    [|discriminate].
  destruct (lookup_ordinary selected (ordinary_continuations state group))
    as [current|] eqn:Hlookup; [|discriminate].
  inversion H; subst. exact Hlookup.
Qed.

Lemma resolve_bound_ordinary_uses_observed_path : forall
    state group bound index path record,
  resolve_bound_ordinary state group bound index =
      Some (OrdinaryTarget path record) ->
  In path bound.
Proof.
  intros state group bound index path record Hresolve.
  unfold resolve_bound_ordinary in Hresolve.
  destruct (Z_lt_ge_dec index 0); [discriminate|].
  destruct (nth_error bound (Z.to_nat index)) as [selected|] eqn:Hnth;
    [|discriminate].
  destruct (lookup_ordinary selected (ordinary_continuations state group));
    [|discriminate].
  inversion Hresolve; subst. now apply nth_error_In in Hnth.
Qed.

Lemma resolved_ordinary_capability_is_live : forall
    observed commit group capability path record,
  resolve_capability observed commit group capability =
      Some (OrdinaryTarget path record) ->
  lookup_ordinary path (ordinary_continuations commit group) = Some record /\
  In path (ordinary_paths (ordinary_continuations observed group)).
Proof.
  intros observed commit group capability path record Hresolve.
  unfold resolve_capability in Hresolve.
  destruct (group_eq_dec group (capability_group capability)); [|discriminate].
  destruct (EqualityEpoch.eq_dec
    (continuation_epochs commit group) (continuation_epochs observed group));
    [|discriminate].
  destruct (installed_continuations observed group) as [installed|].
  - destruct (Z.eq_dec (capability_index capability) 0).
    + destruct (installed_continuations commit group) as [current|];
        [|discriminate].
      discriminate.
    + split.
      * eapply resolve_bound_ordinary_is_live. exact Hresolve.
      * eapply resolve_bound_ordinary_uses_observed_path. exact Hresolve.
  - split.
    + eapply resolve_bound_ordinary_is_live. exact Hresolve.
    + eapply resolve_bound_ordinary_uses_observed_path. exact Hresolve.
Qed.

Lemma exact_resolved_ordinary_is_live : forall
    observed commit group capability expected path record,
  resolve_exact_continuation observed commit group capability expected =
      Some (OrdinaryTarget path record) ->
  lookup_ordinary path (ordinary_continuations commit group) = Some record /\
  In path (ordinary_paths (ordinary_continuations observed group)).
Proof.
  intros observed commit group capability expected path record Hresolve.
  unfold resolve_exact_continuation in Hresolve.
  destruct (resolve_capability observed commit group capability) as [target|]
    eqn:Hcap; [|discriminate].
  destruct (target_boundary_validb target) eqn:Hvalid; [|discriminate].
  destruct (expectation_matches_record_dec expected (target_record target));
    [|discriminate].
  inversion Hresolve; subst target.
  now apply resolved_ordinary_capability_is_live with (capability := capability).
Qed.

Lemma resolve_bound_ordinary_never_installed : forall
    state group bound index record,
  resolve_bound_ordinary state group bound index <>
    Some (InstalledTarget record).
Proof.
  intros state group bound index record Hresolve.
  unfold resolve_bound_ordinary in Hresolve.
  destruct (Z_lt_ge_dec index 0); [discriminate|].
  destruct (nth_error bound (Z.to_nat index)) as [path|]; [|discriminate].
  destruct (lookup_ordinary path (ordinary_continuations state group)); discriminate.
Qed.

Lemma resolved_installed_capability_is_live : forall
    observed commit group capability record,
  resolve_capability observed commit group capability = Some (InstalledTarget record) ->
  (exists observed_record,
      installed_continuations observed group = Some observed_record) /\
  installed_continuations commit group = Some record.
Proof.
  intros observed commit group capability record Hresolve.
  unfold resolve_capability in Hresolve.
  destruct (group_eq_dec group (capability_group capability)); [|discriminate].
  destruct (EqualityEpoch.eq_dec
    (continuation_epochs commit group) (continuation_epochs observed group));
    [|discriminate].
  destruct (installed_continuations observed group) as [installed|]
    eqn:Hobserved; [|].
  - destruct (Z.eq_dec (capability_index capability) 0).
    + destruct (installed_continuations commit group) as [current|]
        eqn:Hcommit; [|discriminate].
      inversion Hresolve; subst current. split.
      * exists installed. reflexivity.
      * reflexivity.
    + exfalso. eapply resolve_bound_ordinary_never_installed. exact Hresolve.
  - exfalso. eapply resolve_bound_ordinary_never_installed. exact Hresolve.
Qed.

Definition target_identity_is_authoritative
    (observed commit : ContinuationAuthority) (group : GroupKey)
    (target : ResolvedTarget) : Prop :=
  match target with
  | InstalledTarget record =>
      continuation_epochs commit group = continuation_epochs observed group /\
      (exists observed_record,
          installed_continuations observed group = Some observed_record) /\
      installed_continuations commit group = Some record
  | OrdinaryTarget path record =>
      continuation_epochs commit group = continuation_epochs observed group /\
      In path (ordinary_paths (ordinary_continuations observed group)) /\
      lookup_ordinary path (ordinary_continuations commit group) = Some record /\
      path_valid path = true /\
      path = canonical_continuation_path group record
  end.

Lemma resolved_capability_uses_observed_epoch : forall
    observed commit group capability target,
  resolve_capability observed commit group capability = Some target ->
  continuation_epochs commit group = continuation_epochs observed group.
Proof.
  intros observed commit group capability target Hresolve.
  unfold resolve_capability in Hresolve.
  destruct (group_eq_dec group (capability_group capability)); [|discriminate].
  destruct (EqualityEpoch.eq_dec
    (continuation_epochs commit group) (continuation_epochs observed group));
    [assumption|discriminate].
Qed.

Lemma lookup_ordinary_has_member : forall ordinary path record,
  lookup_ordinary path ordinary = Some record ->
  exists entry,
    In entry ordinary /\
    ordinary_path entry = path /\
    ordinary_record entry = record.
Proof.
  induction ordinary as [|head tail IH]; intros path record Hlookup;
    [discriminate|].
  simpl in Hlookup.
  destruct (path_eq_dec path (ordinary_path head)) as [Hpath|Hdifferent].
  - inversion Hlookup; subst record. exists head. split; [now left|].
    split; [symmetry; exact Hpath|reflexivity].
  - destruct (IH path record Hlookup) as [entry [Hin [Hpath Heq]]].
    exists entry. split; [now right|]. split; assumption.
Qed.

Theorem exact_resolution_identity_comes_from_bound_path_or_installed_slot : forall
    observed commit group capability expected target,
  continuation_authority_valid commit ->
  resolve_exact_continuation observed commit group capability expected = Some target ->
  target_identity_is_authoritative observed commit group target.
Proof.
  intros observed commit group capability expected target
    [Hpaths [Hordinary [Hinstalled [Hresting Hinstalled_joins]]]] Hresolve.
  destruct (@exact_resolution_refines_capability_and_expectation
    observed commit group capability expected target Hresolve) as [Hcap [_ _]].
  destruct target as [record|path record]; simpl.
  - split.
    + now apply resolved_capability_uses_observed_epoch with
        (capability := capability) (target := InstalledTarget record).
    + now apply resolved_installed_capability_is_live with
        (capability := capability).
  - destruct (@resolved_ordinary_capability_is_live
      observed commit group capability path record Hcap) as [Hlive Hobserved].
    split.
    + now apply resolved_capability_uses_observed_epoch with
        (capability := capability) (target := OrdinaryTarget path record).
    + split; [exact Hobserved|]. split; [exact Hlive|].
    destruct (@lookup_ordinary_has_member
      (ordinary_continuations commit group) path record Hlive)
      as [entry [Hin [Hentry_path Heq]]].
    destruct (Hordinary group entry Hin) as [Hpath [Hvalid Hcanonical]].
    split.
    * rewrite Hentry_path in Hpath. exact Hpath.
    * unfold ordinary_path_is_canonical in Hcanonical.
      now rewrite Hentry_path, Heq in Hcanonical.
Qed.

Theorem exact_resolution_preserves_canonical_signed_peeks : forall
    observed commit group capability expected target,
  continuation_authority_valid commit ->
  resolve_exact_continuation observed commit group capability expected = Some target ->
  path_valid (expected_source expected) = true /\
  canonical_peeks (expected_peeks expected) /\
  NoDup (expected_peeks expected).
Proof.
  intros observed commit group capability expected target
    [Hpaths_unique [Hordinary_valid
      [Hinstalled_valid [Hresting_unique Hinstalled_join_unique]]]] Hresolve.
  destruct (@exact_resolution_refines_capability_and_expectation
    observed commit group capability expected target Hresolve)
    as [Hcap_authority [Hboundary_valid Hmatches]].
  assert (Hvalid : continuation_record_valid (target_record target)).
  { destruct target as [record|path record].
    - unfold resolve_exact_continuation in Hresolve.
      destruct (resolve_capability observed commit group capability) as [actual|]
        eqn:Hcap; [|discriminate].
      destruct (target_boundary_validb actual) eqn:Hboundary; [|discriminate].
      destruct (expectation_matches_record_dec expected (target_record actual));
        [|discriminate].
      inversion Hresolve; subst actual.
      destruct (@resolved_installed_capability_is_live
        observed commit group capability record Hcap) as [_ Hcommit_live].
      destruct (Hinstalled_valid group record
        Hcommit_live) as [_ Hvalid].
      exact Hvalid.
    - pose proof (@exact_resolved_ordinary_is_live
        observed commit group capability expected path record Hresolve) as Hlive.
      destruct Hlive as [Hcurrent_live Hobserved_live].
      destruct (@lookup_ordinary_has_member
        (ordinary_continuations commit group) path record Hcurrent_live)
        as [entry [Hin [Hentry_path Heq]]].
      rewrite <- Heq.
      destruct (Hordinary_valid group entry Hin) as [_ [Hrecord_valid _]].
      exact Hrecord_valid. }
  destruct Hvalid as [Hidentity [Hsource Hcanonical]].
  destruct Hmatches as [Hsource_exact [_ Hpeeks]].
  rewrite <- Hsource_exact. rewrite <- Hpeeks.
  split; [exact Hsource|]. split; [exact Hcanonical|].
  now apply canonical_peeks_are_unique.
Qed.

Definition bump_group_epoch
    (state : ContinuationAuthority) (group : GroupKey) : ContinuationAuthority :=
  ContinuationAuthorityState
    (installed_continuations state)
    (ordinary_continuations state)
    (set_group (continuation_epochs state) group
      (EqualityEpoch.fresh (continuation_epochs state group)))
    (resting_joins state)
    (installed_joins state).

Definition set_channel {A : Type}
    (authority : Path -> A) (selected : Path) (value : A) : Path -> A :=
  fun channel => if path_eq_dec channel selected then value else authority channel.

Lemma set_channel_exact : forall A (authority : Path -> A) selected value,
  set_channel authority selected value selected = value.
Proof.
  intros. unfold set_channel.
  destruct (path_eq_dec selected selected); [reflexivity|contradiction].
Qed.

Lemma set_channel_other : forall A (authority : Path -> A) selected value other,
  other <> selected ->
  set_channel authority selected value other = authority other.
Proof.
  intros. unfold set_channel.
  destruct (path_eq_dec other selected); [contradiction|reflexivity].
Qed.

Fixpoint remove_join_group_exact
    (selected : GroupKey) (groups : list GroupKey) : list GroupKey :=
  match groups with
  | [] => []
  | group :: tail =>
      if group_eq_dec selected group
      then tail
      else group :: remove_join_group_exact selected tail
  end.

Fixpoint remove_group_from_join_channels
    (joins : JoinMap) (channels : list Path) (group : GroupKey) : JoinMap :=
  match channels with
  | [] => joins
  | channel :: tail =>
      remove_group_from_join_channels
        (set_channel joins channel
          (remove_join_group_exact group (joins channel)))
        tail group
  end.

Lemma remove_absent_join_group_is_identity : forall groups selected,
  ~ In selected groups -> remove_join_group_exact selected groups = groups.
Proof.
  induction groups as [|head tail IH]; intros selected Hnotin; [reflexivity|].
  simpl. destruct (group_eq_dec selected head) as [Heq|Hneq].
  - subst. exfalso. apply Hnotin. now left.
  - f_equal. apply IH. intro Hin. apply Hnotin. now right.
Qed.

Lemma in_remove_join_group_exact : forall groups selected other,
  In other (remove_join_group_exact selected groups) -> In other groups.
Proof.
  induction groups as [|head tail IH]; intros selected other Hin; [contradiction|].
  simpl in Hin. destruct (group_eq_dec selected head).
  - now right.
  - destruct Hin as [Heq|Hin]; [now left|right; now apply IH with selected].
Qed.

Lemma removed_join_group_is_absent : forall groups selected,
  NoDup groups -> ~ In selected (remove_join_group_exact selected groups).
Proof.
  induction groups as [|head tail IH]; intros selected Hunique; simpl; [tauto|].
  inversion Hunique as [|head' tail' Hnotin Htail]; subst.
  destruct (group_eq_dec selected head) as [Heq|Hneq].
  - subst. exact Hnotin.
  - simpl. intros [Heq'|Hin]; [congruence|].
    eapply IH; eassumption.
Qed.

Lemma remove_join_group_preserves_nodup : forall groups selected,
  NoDup groups -> NoDup (remove_join_group_exact selected groups).
Proof.
  induction groups as [|head tail IH]; intros selected Hunique; [constructor|].
  inversion Hunique as [|head' tail' Hnotin Htail]; subst.
  simpl. destruct (group_eq_dec selected head) as [Heq|Hneq].
  - exact Htail.
  - constructor.
    + intro Hin. apply Hnotin.
      apply in_remove_join_group_exact in Hin. exact Hin.
    + now apply IH.
Qed.

Lemma remove_join_group_exact_is_idempotent : forall groups selected,
  NoDup groups ->
  remove_join_group_exact selected (remove_join_group_exact selected groups) =
  remove_join_group_exact selected groups.
Proof.
  intros. apply remove_absent_join_group_is_identity.
  now apply removed_join_group_is_absent.
Qed.

Lemma other_join_group_membership_is_preserved : forall groups selected other,
  other <> selected ->
  (In other (remove_join_group_exact selected groups) <-> In other groups).
Proof.
  induction groups as [|head tail IH]; intros selected other Hneq; [tauto|].
  simpl. destruct (group_eq_dec selected head) as [Heq|Hdiffer].
  - subst. split; [now right|].
    intros [Heq'|Hin]; [exfalso; apply Hneq; symmetry; exact Heq'|exact Hin].
  - simpl. rewrite IH by exact Hneq. tauto.
Qed.

Lemma join_channel_other_group_membership_is_preserved : forall
    joins channels selected other channel,
  other <> selected ->
  (In other (remove_group_from_join_channels joins channels selected channel) <->
   In other (joins channel)).
Proof.
  intros joins channels. revert joins.
  induction channels as [|head tail IH];
    intros joins selected other channel Hneq; [tauto|].
  simpl. rewrite IH by exact Hneq.
  unfold set_channel. destruct (path_eq_dec channel head) as [Heq|Hdifferent].
  - subst head. apply other_join_group_membership_is_preserved. exact Hneq.
  - tauto.
Qed.

Lemma join_channel_not_selected_is_preserved : forall joins channels group channel,
  ~ In channel channels ->
  remove_group_from_join_channels joins channels group channel = joins channel.
Proof.
  intros joins channels. revert joins.
  induction channels as [|head tail IH]; intros joins group channel Hnotin;
    [reflexivity|].
  simpl. rewrite IH.
  - apply set_channel_other. intro Heq. subst. apply Hnotin. now left.
  - intro Hin. apply Hnotin. now right.
Qed.

Lemma selected_join_channel_removes_exact_group : forall joins channels group channel,
  (forall selected, NoDup (joins selected)) ->
  In channel channels ->
  remove_group_from_join_channels joins channels group channel =
  remove_join_group_exact group (joins channel).
Proof.
  intros joins channels. revert joins.
  induction channels as [|head tail IH]; intros joins group channel Hunique Hin;
    [contradiction|].
  simpl. destruct Hin as [Heq|Hin].
  - subst head.
    destruct (in_dec path_eq_dec channel tail) as [Hin_tail|Hnotin_tail].
    + rewrite IH with
        (joins := set_channel joins channel
          (remove_join_group_exact group (joins channel))).
      * rewrite set_channel_exact.
        apply remove_join_group_exact_is_idempotent. apply Hunique.
      * intro selected. unfold set_channel.
        destruct (path_eq_dec selected channel) as [Hsame|Hdifferent].
        -- subst. apply remove_join_group_preserves_nodup. apply Hunique.
        -- apply Hunique.
      * exact Hin_tail.
    + rewrite join_channel_not_selected_is_preserved by exact Hnotin_tail.
      apply set_channel_exact.
  - rewrite IH with
      (joins := set_channel joins head
        (remove_join_group_exact group (joins head))).
    + unfold set_channel.
      destruct (path_eq_dec channel head) as [Heq|Hneq].
      * subst. apply remove_join_group_exact_is_idempotent. apply Hunique.
      * reflexivity.
    + intro selected. unfold set_channel.
      destruct (path_eq_dec selected head) as [Heq|Hneq].
      * subst. apply remove_join_group_preserves_nodup. apply Hunique.
      * apply Hunique.
    + exact Hin.
Qed.

Definition remove_nonpersistent_ordinary
    (state : ContinuationAuthority) (group : GroupKey) (path : Path)
    (join_channels : list Path)
    : ContinuationAuthority :=
  let remaining := remove_ordinary_exact path (ordinary_continuations state group) in
  let ordinary' := set_group (ordinary_continuations state) group remaining in
  let epoch' := set_group (continuation_epochs state) group
    (EqualityEpoch.fresh (continuation_epochs state group)) in
  let group_remains :=
    match installed_continuations state group with
    | Some _ => true
    | None => negb (Nat.eqb (length remaining) 0)
    end in
  let resting_joins' :=
    if group_remains then resting_joins state
    else remove_group_from_join_channels
      (resting_joins state) join_channels group in
  ContinuationAuthorityState
    (installed_continuations state) ordinary' epoch' resting_joins'
    (installed_joins state).

Definition commit_exact_continuation
    (observed state : ContinuationAuthority) (group : GroupKey)
    (capability : ContinuationCapability) (expected : ContinuationExpectation)
    (join_channels : list Path)
    : option ContinuationAuthority :=
  match resolve_exact_continuation observed state group capability expected with
  | None => None
  | Some (InstalledTarget record) =>
      if continuation_persistent record then Some state else None
  | Some (OrdinaryTarget path record) =>
      if continuation_persistent record
      then Some state
      else Some (remove_nonpersistent_ordinary state group path join_channels)
  end.

Theorem accepted_installed_continuation_is_preserved : forall
    state group capability expected join_channels record,
  resolve_exact_continuation state state group capability expected =
      Some (InstalledTarget record) ->
  continuation_persistent record = true ->
  commit_exact_continuation state state group capability expected join_channels =
    Some state.
Proof.
  intros. unfold commit_exact_continuation. rewrite H. now rewrite H0.
Qed.

Theorem accepted_persistent_ordinary_continuation_is_preserved : forall
    state group capability expected join_channels path record,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = true ->
  commit_exact_continuation state state group capability expected join_channels =
    Some state.
Proof.
  intros. unfold commit_exact_continuation. rewrite H. now rewrite H0.
Qed.

Lemma lookup_absent_ordinary_path_is_none : forall ordinary selected,
  ~ In selected (ordinary_paths ordinary) -> lookup_ordinary selected ordinary = None.
Proof.
  induction ordinary as [|entry tail IH]; intros selected Hnotin; [reflexivity|].
  simpl in Hnotin. simpl.
  destruct (path_eq_dec selected (ordinary_path entry)) as [Heq|Hneq].
  - exfalso. apply Hnotin. now left.
  - apply IH. intro Hin. apply Hnotin. now right.
Qed.

Lemma lookup_removed_path_is_none : forall ordinary selected,
  NoDup (ordinary_paths ordinary) ->
  lookup_ordinary selected (remove_ordinary_exact selected ordinary) = None.
Proof.
  induction ordinary as [|entry tail IH]; intros selected Hunique; [reflexivity|].
  inversion Hunique as [|head paths Hnotin Htail]; subst.
  simpl. destruct (path_eq_dec selected (ordinary_path entry)) as [Heq|Hneq].
  - subst. apply lookup_absent_ordinary_path_is_none. exact Hnotin.
  - simpl. destruct (path_eq_dec selected (ordinary_path entry));
      [contradiction|].
    apply IH. exact Htail.
Qed.

Lemma lookup_other_path_is_preserved_by_removal : forall ordinary selected other,
  other <> selected ->
  lookup_ordinary other (remove_ordinary_exact selected ordinary) =
  lookup_ordinary other ordinary.
Proof.
  induction ordinary as [|entry tail IH]; intros selected other Hneq;
    [reflexivity|].
  simpl. destruct (path_eq_dec selected (ordinary_path entry)) as [Heq|Hdifferent].
  - subst. destruct (path_eq_dec other (ordinary_path entry));
      [contradiction|reflexivity].
  - simpl. destruct (path_eq_dec other (ordinary_path entry));
      [reflexivity|].
    apply IH. exact Hneq.
Qed.

Lemma remove_present_unique_path_decrements_length : forall ordinary selected record,
  NoDup (ordinary_paths ordinary) ->
  lookup_ordinary selected ordinary = Some record ->
  S (length (remove_ordinary_exact selected ordinary)) = length ordinary.
Proof.
  induction ordinary as [|entry tail IH]; intros selected record Hunique Hlookup;
    [discriminate|].
  inversion Hunique as [|head paths Hnotin Htail]; subst.
  simpl in Hlookup. simpl.
  destruct (path_eq_dec selected (ordinary_path entry)) as [Heq|Hneq].
  - reflexivity.
  - simpl. destruct (path_eq_dec selected (ordinary_path entry));
      [contradiction|].
    f_equal. eapply IH; eassumption.
Qed.

Lemma in_remove_ordinary_exact : forall ordinary selected entry,
  In entry (remove_ordinary_exact selected ordinary) -> In entry ordinary.
Proof.
  induction ordinary as [|head tail IH]; intros selected entry Hin;
    [contradiction|].
  simpl in Hin. destruct (path_eq_dec selected (ordinary_path head)).
  - now right.
  - destruct Hin as [Heq|Hin]; [now left|right; now apply IH with selected].
Qed.

Lemma remove_ordinary_exact_preserves_unique_paths : forall ordinary selected,
  NoDup (ordinary_paths ordinary) ->
  NoDup (ordinary_paths (remove_ordinary_exact selected ordinary)).
Proof.
  induction ordinary as [|head tail IH]; intros selected Hunique; [constructor|].
  inversion Hunique as [|head_path tail_paths Hnotin Htail]; subst.
  simpl. destruct (path_eq_dec selected (ordinary_path head)).
  - exact Htail.
  - constructor.
    + intro Hin. apply Hnotin.
      apply in_map_iff in Hin. destruct Hin as [entry [Heq Hin]].
      apply in_map_iff. exists entry. split; [exact Heq|].
      now apply in_remove_ordinary_exact in Hin.
    + now apply IH.
Qed.

Lemma join_channel_removal_preserves_unique_groups : forall joins channels group,
  (forall channel, NoDup (joins channel)) ->
  forall channel,
    NoDup (remove_group_from_join_channels joins channels group channel).
Proof.
  intros joins channels. revert joins.
  induction channels as [|head tail IH]; intros joins group Hunique channel;
    [apply Hunique|].
  simpl. apply IH. intro selected. unfold set_channel.
  destruct (path_eq_dec selected head).
  - apply remove_join_group_preserves_nodup. apply Hunique.
  - apply Hunique.
Qed.

Theorem nonpersistent_removal_preserves_authority_validity : forall
    state group path join_channels,
  continuation_authority_valid state ->
  continuation_authority_valid
    (remove_nonpersistent_ordinary state group path join_channels).
Proof.
  intros state group path join_channels
    [Hpaths [Hordinary [Hinstalled [Hresting Hinstalled_joins]]]].
  unfold remove_nonpersistent_ordinary; simpl.
  split.
  - intro selected.
    change (NoDup (ordinary_paths
      (set_group (ordinary_continuations state) group
        (remove_ordinary_exact path (ordinary_continuations state group))
        selected))).
    unfold set_group.
    destruct (group_eq_dec selected group).
    + apply remove_ordinary_exact_preserves_unique_paths. apply Hpaths.
    + apply Hpaths.
  - split.
    + intros selected candidate Hin.
      change (In candidate
        (set_group (ordinary_continuations state) group
          (remove_ordinary_exact path (ordinary_continuations state group))
          selected)) in Hin.
      unfold set_group in Hin.
      destruct (group_eq_dec selected group) as [Heq|Hneq].
      * subst selected. apply Hordinary with (group := group).
        now apply in_remove_ordinary_exact in Hin.
      * now apply Hordinary with (group := selected).
    + split.
      * exact Hinstalled.
      * split.
        -- destruct (installed_continuations state group); simpl.
           ++ exact Hresting.
           ++ destruct (Nat.eqb
               (length (remove_ordinary_exact path
                 (ordinary_continuations state group))) 0); simpl.
              ** now apply join_channel_removal_preserves_unique_groups.
              ** exact Hresting.
        -- exact Hinstalled_joins.
Qed.

Theorem accepted_nonpersistent_removes_exactly_one_ordinary : forall
    state group capability expected join_channels path record after,
  continuation_authority_valid state ->
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels = Some after ->
  lookup_ordinary path (ordinary_continuations after group) = None /\
  S (length (ordinary_continuations after group)) =
      length (ordinary_continuations state group).
Proof.
  intros state group capability expected join_channels path record after
    [Hunique [Hordinary_valid
      [Hinstalled [Hresting_unique Hinstalled_join_unique]]]]
    Hresolve Hpersistent Hcommit.
  unfold commit_exact_continuation in Hcommit. rewrite Hresolve in Hcommit.
  rewrite Hpersistent in Hcommit. inversion Hcommit; subst after; clear Hcommit.
  unfold remove_nonpersistent_ordinary; simpl.
  rewrite set_group_exact. split.
  - apply lookup_removed_path_is_none. apply Hunique.
  - apply remove_present_unique_path_decrements_length with (record := record).
    + apply Hunique.
    + destruct (@exact_resolved_ordinary_is_live
        state state group capability expected path record Hresolve)
        as [Hlive _]. exact Hlive.
Qed.

Theorem accepted_nonpersistent_preserves_every_other_ordinary_path : forall
    state group capability expected join_channels path record after other,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels = Some after ->
  other <> path ->
  lookup_ordinary other (ordinary_continuations after group) =
  lookup_ordinary other (ordinary_continuations state group).
Proof.
  intros. unfold commit_exact_continuation in H1. rewrite H in H1.
  rewrite H0 in H1. inversion H1; subst after.
  unfold remove_nonpersistent_ordinary; simpl. rewrite set_group_exact.
  now apply lookup_other_path_is_preserved_by_removal.
Qed.

Theorem accepted_nonpersistent_bumps_exact_group_epoch : forall
    state group capability expected join_channels path record after,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels = Some after ->
  continuation_epochs after group =
    EqualityEpoch.fresh (continuation_epochs state group) /\
  continuation_epochs after group <> continuation_epochs state group.
Proof.
  intros. unfold commit_exact_continuation in H1. rewrite H in H1.
  rewrite H0 in H1. inversion H1; subst after.
  unfold remove_nonpersistent_ordinary; simpl. rewrite set_group_exact.
  split; [reflexivity|apply EqualityEpoch.fresh_is_distinct].
Qed.

Theorem accepted_nonpersistent_preserves_other_group_epoch : forall
    state group capability expected join_channels path record after other,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels = Some after ->
  other <> group ->
  continuation_epochs after other = continuation_epochs state other.
Proof.
  intros. unfold commit_exact_continuation in H1. rewrite H in H1.
  rewrite H0 in H1. inversion H1; subst after.
  unfold remove_nonpersistent_ordinary; simpl. now apply set_group_other.
Qed.

Theorem accepted_nonpersistent_preserves_all_installed_continuations : forall
    state group capability expected join_channels path record after,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels = Some after ->
  installed_continuations after = installed_continuations state.
Proof.
  intros. unfold commit_exact_continuation in H1. rewrite H in H1.
  rewrite H0 in H1. inversion H1; reflexivity.
Qed.

Theorem accepted_nonpersistent_preserves_all_installed_joins : forall
    state group capability expected join_channels path record after,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels = Some after ->
  installed_joins after = installed_joins state.
Proof.
  intros. unfold commit_exact_continuation in H1. rewrite H in H1.
  rewrite H0 in H1. inversion H1; reflexivity.
Qed.

Theorem final_nonpersistent_removal_cleans_exact_resting_join_group : forall
    state group capability expected join_channels path record after channel,
  continuation_authority_valid state ->
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  installed_continuations state group = None ->
  remove_ordinary_exact path (ordinary_continuations state group) = [] ->
  commit_exact_continuation state state group capability expected join_channels =
      Some after ->
  In channel join_channels ->
  resting_joins after channel =
    remove_join_group_exact group (resting_joins state channel) /\
  ~ In group (resting_joins after channel).
Proof.
  intros state group capability expected join_channels path record after channel
    [Hordinary [Hordinary_valid
      [Hinstalled_valid [Hresting_unique Hinstalled_unique]]]]
    Hresolve Hpersistent Hinstalled Hempty Hcommit Hchannel.
  unfold commit_exact_continuation in Hcommit. rewrite Hresolve in Hcommit.
  rewrite Hpersistent in Hcommit. inversion Hcommit; subst after.
  unfold remove_nonpersistent_ordinary. rewrite Hempty. simpl. rewrite Hinstalled.
  simpl. split.
  - apply selected_join_channel_removes_exact_group; assumption.
  - rewrite selected_join_channel_removes_exact_group; [|assumption|assumption].
    apply removed_join_group_is_absent. apply Hresting_unique.
Qed.

Theorem final_nonpersistent_removal_preserves_unlisted_join_channels : forall
    state group capability expected join_channels path record after channel,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  installed_continuations state group = None ->
  remove_ordinary_exact path (ordinary_continuations state group) = [] ->
  commit_exact_continuation state state group capability expected join_channels =
      Some after ->
  ~ In channel join_channels ->
  resting_joins after channel = resting_joins state channel.
Proof.
  intros. unfold commit_exact_continuation in H3. rewrite H in H3.
  rewrite H0 in H3. inversion H3; subst after.
  unfold remove_nonpersistent_ordinary. rewrite H2. simpl. rewrite H1. simpl.
  now apply join_channel_not_selected_is_preserved.
Qed.

Theorem nonfinal_nonpersistent_removal_preserves_resting_joins : forall
    state group capability expected join_channels path record after,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  (installed_continuations state group <> None \/
   remove_ordinary_exact path (ordinary_continuations state group) <> []) ->
  commit_exact_continuation state state group capability expected join_channels =
      Some after ->
  resting_joins after = resting_joins state.
Proof.
  intros state group capability expected join_channels path record after
    Hresolve Hpersistent Hremains Hcommit.
  unfold commit_exact_continuation in Hcommit. rewrite Hresolve in Hcommit.
  rewrite Hpersistent in Hcommit. inversion Hcommit; subst after.
  unfold remove_nonpersistent_ordinary.
  destruct (installed_continuations state group) as [installed|] eqn:Hinstalled;
    simpl; [reflexivity|].
  destruct (Nat.eqb
    (length (remove_ordinary_exact path (ordinary_continuations state group))) 0)
    eqn:Hlength; [|reflexivity].
  apply Nat.eqb_eq in Hlength. apply length_zero_iff_nil in Hlength.
  destruct Hremains as [Hsome|Hnonempty]; contradiction.
Qed.

Theorem accepted_nonpersistent_preserves_every_unrelated_group : forall
    state group capability expected join_channels path record after other,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels =
      Some after ->
  other <> group ->
  ordinary_continuations after other = ordinary_continuations state other /\
  installed_continuations after other = installed_continuations state other /\
  continuation_epochs after other = continuation_epochs state other.
Proof.
  intros. unfold commit_exact_continuation in H1. rewrite H in H1.
  rewrite H0 in H1. inversion H1; subst after.
  unfold remove_nonpersistent_ordinary; simpl.
  rewrite set_group_other by assumption.
  rewrite set_group_other by assumption. repeat split; reflexivity.
Qed.

Definition proper_group_prefix (prefix whole : GroupKey) : Prop :=
  exists next suffix, whole = prefix ++ next :: suffix.

Lemma proper_group_prefix_is_distinct : forall prefix whole,
  proper_group_prefix prefix whole -> prefix <> whole.
Proof.
  intros prefix whole [next [suffix Heq]] Hequal. subst whole.
  apply (f_equal (@length Path)) in Hequal.
  rewrite length_app in Hequal. simpl in Hequal. lia.
Qed.

Theorem prefix_related_groups_do_not_alias_authority : forall
    state group capability expected join_channels path record after other,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels =
      Some after ->
  (proper_group_prefix other group \/ proper_group_prefix group other) ->
  ordinary_continuations after other = ordinary_continuations state other /\
  continuation_epochs after other = continuation_epochs state other /\
  installed_continuations after other = installed_continuations state other.
Proof.
  intros state group capability expected join_channels path record after other
    Hresolve Hpersistent Hcommit Hprefix.
  assert (Hdifferent : other <> group).
  { destruct Hprefix as [Hprefix|Hprefix].
    - exact (proper_group_prefix_is_distinct Hprefix).
    - intro Heq. subst. exact (proper_group_prefix_is_distinct Hprefix eq_refl). }
  unfold commit_exact_continuation in Hcommit. rewrite Hresolve in Hcommit.
  rewrite Hpersistent in Hcommit. inversion Hcommit; subst after.
  unfold remove_nonpersistent_ordinary; simpl.
  rewrite set_group_other by exact Hdifferent.
  rewrite set_group_other by exact Hdifferent.
  repeat split; reflexivity.
Qed.

Theorem prefix_related_join_group_membership_is_preserved : forall
    state group capability expected join_channels path record after other channel,
  resolve_exact_continuation state state group capability expected =
      Some (OrdinaryTarget path record) ->
  continuation_persistent record = false ->
  commit_exact_continuation state state group capability expected join_channels =
      Some after ->
  (proper_group_prefix other group \/ proper_group_prefix group other) ->
  (In other (resting_joins after channel) <->
   In other (resting_joins state channel)).
Proof.
  intros state group capability expected join_channels path record after other channel
    Hresolve Hpersistent Hcommit Hprefix.
  assert (Hdifferent : other <> group).
  { destruct Hprefix as [Hprefix|Hprefix].
    - exact (proper_group_prefix_is_distinct Hprefix).
    - intro Heq. subst. exact (proper_group_prefix_is_distinct Hprefix eq_refl). }
  unfold commit_exact_continuation in Hcommit. rewrite Hresolve in Hcommit.
  rewrite Hpersistent in Hcommit. inversion Hcommit; subst after.
  unfold remove_nonpersistent_ordinary; simpl.
  destruct (installed_continuations state group); simpl; [tauto|].
  destruct (Nat.eqb
    (length (remove_ordinary_exact path (ordinary_continuations state group))) 0);
    simpl; [|tauto].
  apply join_channel_other_group_membership_is_preserved. exact Hdifferent.
Qed.

Module OrderedReceivePositions.
Open Scope nat_scope.

Record PatternFields : Type := PatternFieldsState {
  pattern_terms : list Path;
  pattern_remainder : option Path;
  pattern_free_count : Z
}.

Record ReceiveFields : Type := ReceiveFieldsState {
  receive_channels : GroupKey;
  receive_patterns : list PatternFields;
  receive_body : Path;
  receive_persistent : bool;
  receive_peeks : list Z;
  receive_source : Path
}.

Record BindFields : Type := BindFieldsState {
  bind_channel : Path;
  bind_pattern : PatternFields;
  bind_peek : bool
}.

Definition aligned (receive : ReceiveFields) : Prop :=
  length (receive_channels receive) = length (receive_patterns receive).

Definition effective_peek (peeks : list Z) (position : nat) : bool :=
  existsb (Z.eqb (Z.of_nat position)) peeks.

Definition coarse_peek (peeks : list Z) : bool :=
  match peeks with [] => false | _ :: _ => true end.

Definition bind_at (receive : ReceiveFields) (position : nat)
    : option BindFields :=
  if Nat.eqb (length (receive_channels receive)) (length (receive_patterns receive))
  then
    match nth_error (receive_channels receive) position,
          nth_error (receive_patterns receive) position with
    | Some channel, Some pattern =>
        Some (BindFieldsState channel pattern
          (effective_peek (receive_peeks receive) position))
    | _, _ => None
    end
  else None.

Definition channel_at_is (channels : GroupKey) (channel : Path)
    (position : nat) : bool :=
  match nth_error channels position with
  | Some actual => if path_eq_dec actual channel then true else false
  | None => false
  end.

Definition positions (channels : GroupKey) (channel : Path) : list nat :=
  filter (channel_at_is channels channel) (seq 0 (length channels)).

Lemma channel_at_is_exact : forall channels channel position,
  channel_at_is channels channel position = true <->
  nth_error channels position = Some channel.
Proof.
  intros. unfold channel_at_is.
  destruct (nth_error channels position) as [actual|]; [|split; discriminate].
  destruct (path_eq_dec actual channel) as [->|Hdifferent].
  - tauto.
  - split; [discriminate|intro H; inversion H; contradiction].
Qed.

Theorem positions_exact : forall channels channel position,
  In position (positions channels channel) <->
  nth_error channels position = Some channel.
Proof.
  intros. unfold positions. rewrite filter_In, channel_at_is_exact.
  split; [tauto|]. intro Hnth. split; [|exact Hnth].
  apply in_seq. split; [lia|].
  apply nth_error_Some. rewrite Hnth. discriminate.
Qed.

Theorem positions_are_bounded : forall channels channel position,
  In position (positions channels channel) -> position < length channels.
Proof.
  intros channels channel position Hin. apply positions_exact in Hin.
  apply nth_error_Some. rewrite Hin. discriminate.
Qed.

Theorem positions_are_unique : forall channels channel,
  NoDup (positions channels channel).
Proof. intros. apply NoDup_filter, seq_NoDup. Qed.

Lemma seq_strictly_sorted : forall count start,
  StronglySorted Nat.lt (seq start count).
Proof.
  induction count as [|count IH]; intro start; simpl; constructor.
  - apply IH.
  - apply Forall_forall. intros position Hin. apply in_seq in Hin. lia.
Qed.

Lemma filter_strictly_sorted : forall values keep,
  StronglySorted Nat.lt values -> StronglySorted Nat.lt (filter keep values).
Proof.
  intros values keep Hsorted. induction Hsorted; simpl; [constructor|].
  destruct (keep a); [constructor|exact IHHsorted].
  - exact IHHsorted.
  - apply Forall_forall. intros value Hin. apply filter_In in Hin.
    rewrite Forall_forall in H. apply H. exact (proj1 Hin).
Qed.

Theorem positions_are_increasing : forall channels channel,
  StronglySorted Nat.lt (positions channels channel).
Proof. intros. apply filter_strictly_sorted, seq_strictly_sorted. Qed.

Theorem mismatched_arity_refuses_every_position : forall receive position,
  ~ aligned receive -> bind_at receive position = None.
Proof.
  intros receive position Hmismatch. unfold bind_at.
  destruct (Nat.eqb (length (receive_channels receive))
    (length (receive_patterns receive))) eqn:Harity; [|reflexivity].
  apply Nat.eqb_eq in Harity. contradiction.
Qed.

Theorem bind_at_preserves_all_fields : forall receive position bind,
  aligned receive ->
  (bind_at receive position = Some bind <->
   exists channel pattern,
     nth_error (receive_channels receive) position = Some channel /\
     nth_error (receive_patterns receive) position = Some pattern /\
     bind = BindFieldsState channel pattern
       (effective_peek (receive_peeks receive) position)).
Proof.
  intros receive position bind Haligned. unfold bind_at.
  unfold aligned in Haligned. rewrite Haligned, Nat.eqb_refl.
  destruct (nth_error (receive_channels receive) position) as [channel|] eqn:Hchannel;
  destruct (nth_error (receive_patterns receive) position) as [pattern|] eqn:Hpattern.
  - split.
    + intro H. inversion H; subst. exists channel, pattern. auto.
    + intros [c [p [Hc [Hp Hb]]]]. inversion Hc; inversion Hp; subst. reflexivity.
  - split; [discriminate|intros [c [p [_ [H _]]]]; discriminate].
  - split; [discriminate|intros [c [p [H _]]]; discriminate].
  - split; [discriminate|intros [c [p [H _]]]; discriminate].
Qed.

Theorem bind_at_exists_exactly_in_range : forall receive position,
  aligned receive ->
  ((exists bind, bind_at receive position = Some bind) <->
    position < length (receive_channels receive)).
Proof.
  intros receive position Haligned. split.
  - intros [bind Hbind]. apply (bind_at_preserves_all_fields position bind Haligned) in Hbind.
    destruct Hbind as [channel [pattern [Hchannel _]]].
    apply nth_error_Some. rewrite Hchannel. discriminate.
  - intro Hrange.
    assert (Hchannel : nth_error (receive_channels receive) position <> None).
    { apply nth_error_Some. exact Hrange. }
    assert (Hpattern : nth_error (receive_patterns receive) position <> None).
    { apply nth_error_Some. unfold aligned in Haligned. lia. }
    destruct (nth_error (receive_channels receive) position) as [channel|] eqn:Hc;
      [|contradiction].
    destruct (nth_error (receive_patterns receive) position) as [pattern|] eqn:Hp;
      [|contradiction].
    exists (BindFieldsState channel pattern (effective_peek (receive_peeks receive) position)).
    apply (bind_at_preserves_all_fields position _ Haligned). exists channel, pattern. auto.
Qed.

Theorem indexed_bind_correspondence : forall receive channel position,
  aligned receive ->
  (In position (positions (receive_channels receive) channel) <->
   exists bind, bind_at receive position = Some bind /\ bind_channel bind = channel).
Proof.
  intros receive channel position Haligned. rewrite positions_exact. split.
  - intro Hchannel.
    assert (Hrange : position < length (receive_channels receive)).
    { apply nth_error_Some. rewrite Hchannel. discriminate. }
    destruct ((proj2 (bind_at_exists_exactly_in_range position Haligned)) Hrange)
      as [bind Hbind].
    exists bind. split; [exact Hbind|].
    apply (bind_at_preserves_all_fields position bind Haligned) in Hbind.
    destruct Hbind as [c [p [Hc [_ Hb]]]]. rewrite Hchannel in Hc.
    inversion Hc; subst. reflexivity.
  - intros [bind [Hbind Hchannel]].
    apply (bind_at_preserves_all_fields position bind Haligned) in Hbind.
    destruct Hbind as [c [p [Hc [_ Hb]]]]. subst bind. simpl in Hchannel.
    now subst c.
Qed.

Theorem effective_peek_is_signed_membership : forall peeks position,
  effective_peek peeks position = true <-> In (Z.of_nat position) peeks.
Proof.
  intros. unfold effective_peek. rewrite existsb_exists. split.
  - intros [peek [Hin Heq]]. apply Z.eqb_eq in Heq. now subst peek.
  - intro Hin. exists (Z.of_nat position). split; [exact Hin|apply Z.eqb_refl].
Qed.

Theorem wide_position_does_not_wrap_to_signed_peek : forall peeks position,
  canonical_peeks peeks -> (i32_max < Z.of_nat position)%Z ->
  effective_peek peeks position = false.
Proof.
  intros peeks position [_ Hrange] Hwide.
  destruct (effective_peek peeks position) eqn:Hpeek; [|reflexivity].
  apply effective_peek_is_signed_membership in Hpeek.
  rewrite Forall_forall in Hrange. specialize (Hrange _ Hpeek).
  unfold peek_in_i32_range in Hrange. lia.
Qed.

Theorem coarse_peek_preserves_raw_nonemptiness : forall peeks,
  coarse_peek peeks = true <-> peeks <> [].
Proof. intros [|peek peeks]; simpl; split; congruence. Qed.

Definition source_free_fields (receive : ReceiveFields) :=
  (receive_patterns receive, receive_body receive,
   receive_persistent receive, receive_peeks receive).

Definition replace_source (receive : ReceiveFields) (source : Path) : ReceiveFields :=
  ReceiveFieldsState (receive_channels receive) (receive_patterns receive)
    (receive_body receive) (receive_persistent receive) (receive_peeks receive) source.

Theorem source_replacement_preserves_identity_and_positions : forall receive source,
  source_free_fields (replace_source receive source) = source_free_fields receive /\
  receive_channels (replace_source receive source) = receive_channels receive /\
  forall position, bind_at (replace_source receive source) position = bind_at receive position.
Proof. intros. repeat split; reflexivity. Qed.

Theorem distinct_source_remains_observable : forall receive source,
  source <> receive_source receive -> replace_source receive source <> receive.
Proof.
  intros receive source Hdifferent Heq.
  apply (f_equal receive_source) in Heq. simpl in Heq. contradiction.
Qed.

Theorem same_channel_does_not_erase_distinct_pattern_fields : forall
    receive left right left_bind right_bind,
  bind_at receive left = Some left_bind ->
  bind_at receive right = Some right_bind ->
  bind_pattern left_bind <> bind_pattern right_bind -> left_bind <> right_bind.
Proof. intros receive left right left_bind right_bind Hl Hr Hpattern Heq.
  subst. contradiction. Qed.

Theorem empty_aligned_receive_has_no_bind : forall body persistent peeks source position,
  bind_at (ReceiveFieldsState [] [] body persistent peeks source) position = None.
Proof. intros. unfold bind_at. destruct position; reflexivity. Qed.

Lemma frame_equality_preserves_channel : forall left right,
  RhoPathV1Datum.bytes_valid left = true ->
  RhoPathV1Datum.bytes_valid right = true ->
  RhoPathV1Datum.encode_frame left = RhoPathV1Datum.encode_frame right -> left = right.
Proof.
  intros left right Hleft Hright Heq.
  pose proof (RhoPathV1Datum.frame_encode_parse_round_trip left [] Hleft) as Hl.
  pose proof (RhoPathV1Datum.frame_encode_parse_round_trip right [] Hright) as Hr.
  repeat rewrite app_nil_r in Hl, Hr. rewrite Heq, Hr in Hl.
  now inversion Hl.
Qed.

Lemma framed_nth_error : forall channels position,
  nth_error (map RhoPathV1Datum.encode_frame channels) position =
  option_map RhoPathV1Datum.encode_frame (nth_error channels position).
Proof.
  induction channels as [|channel tail IH]; intros [|position]; simpl; auto.
Qed.

Theorem escaped_channel_occurrences_are_exact : forall channels channel position,
  Forall (fun bytes => RhoPathV1Datum.bytes_valid bytes = true) channels ->
  RhoPathV1Datum.bytes_valid channel = true ->
  (In position (positions (map RhoPathV1Datum.encode_frame channels)
    (RhoPathV1Datum.encode_frame channel)) <-> In position (positions channels channel)).
Proof.
  intros channels channel position Hchannels Hchannel.
  repeat rewrite positions_exact. split.
  - intro Hencoded. rewrite framed_nth_error in Hencoded.
    destruct (nth_error channels position) as [actual|] eqn:Hactual;
      simpl in Hencoded; [|discriminate].
    injection Hencoded as Hframes.
    assert (Hvalid : RhoPathV1Datum.bytes_valid actual = true).
    { rewrite Forall_forall in Hchannels. apply Hchannels.
      eapply nth_error_In; eassumption. }
    apply (frame_equality_preserves_channel actual channel Hvalid Hchannel) in Hframes.
    now subst actual.
  - intro Hnth. rewrite framed_nth_error.
    exact (f_equal (option_map RhoPathV1Datum.encode_frame) Hnth).
Qed.

Fixpoint first_identity (identity : Path) (records : list ContinuationRecord)
    : option ContinuationRecord :=
  match records with
  | [] => None
  | record :: tail =>
      if path_eq_dec (continuation_identity record) identity
      then Some record else first_identity identity tail
  end.

Theorem first_identity_retains_head_source : forall record tail,
  first_identity (continuation_identity record) (record :: tail) = Some record.
Proof.
  intros. simpl. destruct (path_eq_dec (continuation_identity record)
    (continuation_identity record)); congruence.
Qed.

Theorem appending_aliases_preserves_first_identity : forall records identity prior suffix,
  first_identity identity records = Some prior ->
  first_identity identity (records ++ suffix) = Some prior.
Proof.
  induction records as [|record tail IH]; intros identity prior suffix Hfirst;
    simpl in *; [discriminate|].
  destruct (path_eq_dec (continuation_identity record) identity).
  - exact Hfirst.
  - now apply IH.
Qed.

Theorem installed_and_resting_positions_are_distinct : forall record path (position : nat),
  (InstalledTarget record, position) <> (OrdinaryTarget path record, position).
Proof. intros. discriminate. Qed.

Definition position_max : Z := 18446744073709551615%Z.

Definition checked_position (position : Z) : option Z :=
  if (Z.leb 0 position && Z.leb position position_max)%bool
  then Some position else None.

Theorem checked_position_is_exact : forall position,
  checked_position position = Some position <->
  (0 <= position <= position_max)%Z.
Proof.
  intros position. unfold checked_position.
  destruct (Z.leb 0 position && Z.leb position position_max)%bool eqn:Hrange.
  - apply andb_true_iff in Hrange. rewrite !Z.leb_le in Hrange. tauto.
  - split; [discriminate|]. intros [Hlower Hupper].
    assert (Htrue : (Z.leb 0 position && Z.leb position position_max)%bool = true).
    { apply andb_true_iff. rewrite !Z.leb_le. auto. }
    congruence.
Qed.

Theorem successful_position_codes_cannot_alias : forall left right encoded,
  checked_position left = Some encoded ->
  checked_position right = Some encoded -> left = right.
Proof.
  intros left right encoded Hl Hr. unfold checked_position in *.
  destruct (Z.leb 0 left && Z.leb left position_max)%bool; [|discriminate].
  destruct (Z.leb 0 right && Z.leb right position_max)%bool; congruence.
Qed.

Theorem representable_arity_has_representable_occurrences : forall channels channel position,
  (Z.of_nat (length channels) <= position_max)%Z ->
  In position (positions channels channel) ->
  checked_position (Z.of_nat position) = Some (Z.of_nat position).
Proof.
  intros channels channel position Harity Hin.
  apply positions_are_bounded in Hin.
  apply checked_position_is_exact. lia.
Qed.

Print Assumptions checked_position_is_exact.
Print Assumptions successful_position_codes_cannot_alias.
Print Assumptions representable_arity_has_representable_occurrences.

Print Assumptions first_identity_retains_head_source.
Print Assumptions appending_aliases_preserves_first_identity.
Print Assumptions installed_and_resting_positions_are_distinct.
Print Assumptions positions_exact.
Print Assumptions positions_are_bounded.
Print Assumptions positions_are_unique.
Print Assumptions positions_are_increasing.
Print Assumptions mismatched_arity_refuses_every_position.
Print Assumptions bind_at_preserves_all_fields.
Print Assumptions bind_at_exists_exactly_in_range.
Print Assumptions indexed_bind_correspondence.
Print Assumptions effective_peek_is_signed_membership.
Print Assumptions wide_position_does_not_wrap_to_signed_peek.
Print Assumptions coarse_peek_preserves_raw_nonemptiness.
Print Assumptions source_replacement_preserves_identity_and_positions.
Print Assumptions distinct_source_remains_observable.
Print Assumptions same_channel_does_not_erase_distinct_pattern_fields.
Print Assumptions empty_aligned_receive_has_no_bind.
Print Assumptions escaped_channel_occurrences_are_exact.

End OrderedReceivePositions.

Module OrderedReceiveByteView.
Import OrderedReceivePositions.
Open Scope nat_scope.

Definition word_max : Z := position_max.
Definition word_modulus : Z := 4294967296%Z.

Fixpoint le_bytes (width : nat) (value : Z) : Path :=
  match width with
  | O => []
  | S rest => Z.to_nat (value mod 256)%Z :: le_bytes rest (value / 256)%Z
  end.

Fixpoint le_value (bytes : Path) : Z :=
  match bytes with
  | [] => 0%Z
  | byte :: rest => (Z.of_nat byte + 256 * le_value rest)%Z
  end.

Definition signed_word (raw : Z) : Z :=
  if Z.leb raw i32_max then raw else (raw - word_modulus)%Z.

Definition i32_bytes (value : Z) : Path := le_bytes 4 (value mod word_modulus)%Z.

Definition i32_value (bytes : Path) : Z := signed_word (le_value bytes mod word_modulus)%Z.

Definition persist_byte (persistent : bool) : Byte := if persistent then 1 else 0.

Definition pattern_image (prefix : PatternFields -> Path) (pattern : PatternFields) : Path :=
  prefix pattern ++ i32_bytes (pattern_free_count pattern).

Definition pattern_images (prefix : PatternFields -> Path) (receive : ReceiveFields)
    : list Path :=
  map (pattern_image prefix) (receive_patterns receive).

Definition peek_image (peeks : list Z) : Path := concat (map i32_bytes peeks).

Definition identity_image (prefix : PatternFields -> Path) (receive : ReceiveFields) : Path :=
  le_bytes 8 (Z.of_nat (length (receive_patterns receive))) ++
  concat (pattern_images prefix receive) ++
  receive_body receive ++
  persist_byte (receive_persistent receive) ::
  le_bytes 8 (Z.of_nat (length (receive_peeks receive))) ++
  peek_image (receive_peeks receive).

Definition record_image (prefix : PatternFields -> Path) (receive : ReceiveFields) : Path :=
  identity_image prefix receive ++ receive_source receive.

Fixpoint pattern_ends (start : nat) (images : list Path) : list nat :=
  match images with
  | [] => []
  | image :: rest => start + length image :: pattern_ends (start + length image) rest
  end.

Definition body_finish_at (prefix : PatternFields -> Path) (receive : ReceiveFields) : nat :=
  8 + length (concat (pattern_images prefix receive)) + length (receive_body receive).

Definition field_bounds (prefix : PatternFields -> Path) (receive : ReceiveFields) : list nat :=
  pattern_ends 8 (pattern_images prefix receive) ++
  [body_finish_at prefix receive;
   body_finish_at prefix receive + 9 + 4 * length (receive_peeks receive)].

Record ByteView : Type := ByteViewState {
  view_record : Path;
  view_ends : list nat;
  view_arity : nat;
  view_peeks : list Z
}.

Definition slice (start finish : nat) (bytes : Path) : Path :=
  firstn (finish - start) (skipn start bytes).

Fixpoint ends_ordered (start : nat) (ends : list nat) : bool :=
  match ends with
  | [] => true
  | finish :: rest => Nat.leb start finish && ends_ordered finish rest
  end.

Fixpoint decode_words (bytes : Path) : list Z :=
  match bytes with
  | first :: second :: third :: fourth :: rest =>
      i32_value [first; second; third; fourth] :: decode_words rest
  | _ => []
  end.

Fixpoint strictly_ascending (values : list Z) : bool :=
  match values with
  | [] => true
  | first :: rest =>
      match rest with
      | [] => true
      | second :: _ => Z.ltb first second && strictly_ascending rest
      end
  end.

Definition header_count (record : Path) : Z := le_value (firstn 8 record).

Definition arity_of (ends : list nat) : nat := length ends - 2.

Definition body_finish_of (ends : list nat) : nat := nth (arity_of ends) ends 0.

Definition identity_finish_of (ends : list nat) : nat := nth (S (arity_of ends)) ends 0.

Definition tail_count (record : Path) (body_finish : nat) : Z :=
  le_value (slice (S body_finish) (body_finish + 9) record).

Definition tail_is_exact (body_finish : nat) (peek_count : Z) (identity_finish : nat) : bool :=
  Z.eqb (Z.of_nat body_finish + 9 + 4 * peek_count)%Z (Z.of_nat identity_finish) &&
  Z.leb (Z.of_nat identity_finish) word_max.

Definition peek_slice (record : Path) (ends : list nat) : Path :=
  slice (body_finish_of ends + 9) (identity_finish_of ends) record.

Definition admit_record (record : Path) (ends : list nat) : option ByteView :=
  if Z.eqb (Z.of_nat (length ends)) (header_count record + 2)%Z then
    if ends_ordered 8 ends then
      match nth_error record (body_finish_of ends) with
      | Some persist =>
          if Nat.leb persist 1 then
            if tail_is_exact (body_finish_of ends) (tail_count record (body_finish_of ends))
                (identity_finish_of ends) then
              if strictly_ascending (decode_words (peek_slice record ends)) then
                if Nat.leb (identity_finish_of ends) (length record) then
                  Some (ByteViewState record ends (arity_of ends)
                    (decode_words (peek_slice record ends)))
                else None
              else None
            else None
          else None
      | None => None
      end
    else None
  else None.

Definition field_start (view : ByteView) (index : nat) : nat :=
  nth index (8 :: view_ends view) 0.

Definition body_end (view : ByteView) : nat := nth (view_arity view) (view_ends view) 0.

Definition identity_end (view : ByteView) : nat := nth (S (view_arity view)) (view_ends view) 0.

Fixpoint field_slices (start : nat) (ends : list nat) (record : Path) : list Path :=
  match ends with
  | [] => []
  | finish :: rest => slice start finish record :: field_slices finish rest record
  end.

Definition view_patterns (view : ByteView) : list Path :=
  field_slices 8 (firstn (view_arity view) (view_ends view)) (view_record view).

Definition view_body (view : ByteView) : Path :=
  slice (field_start view (view_arity view)) (body_end view) (view_record view).

Definition view_persistent (view : ByteView) : bool :=
  match nth_error (view_record view) (body_end view) with
  | Some 1 => true
  | _ => false
  end.

Definition view_identity (view : ByteView) : Path :=
  firstn (identity_end view) (view_record view).

Definition view_source (view : ByteView) : Path :=
  skipn (identity_end view) (view_record view).

Inductive ChannelProfile : Type := LegacyV1 | Rci2V2.

Definition profile_eqb (left right : ChannelProfile) : bool :=
  match left, right with
  | LegacyV1, LegacyV1 => true
  | Rci2V2, Rci2V2 => true
  | _, _ => false
  end.

Record ChannelGroup : Type := ChannelGroupState {
  group_profile : ChannelProfile;
  group_images : GroupKey
}.

Definition group_frames (group : ChannelGroup) : list Path :=
  @map Path Path RhoPathV1Datum.encode_frame (group_images group).

Record ReceiveView : Type := ReceiveViewState {
  receive_view_group : ChannelGroup;
  receive_view_bytes : ByteView
}.

Definition admit_receive (group : ChannelGroup) (record : Path) (ends : list nat)
    : option ReceiveView :=
  match admit_record record ends with
  | Some view =>
      if Nat.eqb (length (group_images group)) (view_arity view)
      then Some (ReceiveViewState group view) else None
  | None => None
  end.

Definition view_positions (group : ChannelGroup) (profile : ChannelProfile) (query : Path)
    : option (list nat) :=
  if profile_eqb (group_profile group) profile
  then Some (positions (group_frames group) (RhoPathV1Datum.encode_frame query))
  else None.

Definition view_effective_peek (peeks : list Z) (position : Z) : bool :=
  Z.leb position i32_max && existsb (Z.eqb position) peeks.

Definition view_bind (view : ReceiveView) (index : nat) : option (Path * Path * bool) :=
  match nth_error (group_frames (receive_view_group view)) index,
        nth_error (view_patterns (receive_view_bytes view)) index with
  | Some frame, Some pattern =>
      Some (frame, pattern,
        view_effective_peek (view_peeks (receive_view_bytes view)) (Z.of_nat index))
  | _, _ => None
  end.

Definition bind_image (prefix : PatternFields -> Path) (bind : BindFields) : Path * Path * bool :=
  (RhoPathV1Datum.encode_frame (bind_channel bind),
   pattern_image prefix (bind_pattern bind), bind_peek bind).

Definition encodable (prefix : PatternFields -> Path) (receive : ReceiveFields) : Prop :=
  canonical_peeks (receive_peeks receive) /\
  Forall (fun pattern => peek_in_i32_range (pattern_free_count pattern))
    (receive_patterns receive) /\
  (Z.of_nat (length (record_image prefix receive)) <= word_max)%Z.

Definition encoded_view (prefix : PatternFields -> Path) (receive : ReceiveFields) : ByteView :=
  ByteViewState (record_image prefix receive) (field_bounds prefix receive)
    (length (receive_patterns receive)) (receive_peeks receive).

Definition view_continuation (view : ByteView) : ContinuationRecord :=
  ContinuationRecordState (view_identity view) (view_source view)
    (view_persistent view) (view_peeks view).

Definition trailing_word (bytes : Path) : Path := skipn (length bytes - 4) bytes.

Lemma le_bytes_length : forall width value, length (le_bytes width value) = width.
Proof. induction width as [|width IH]; intro value; simpl; [reflexivity|now rewrite IH]. Qed.

Lemma le_value_nonnegative : forall bytes, (0 <= le_value bytes)%Z.
Proof. induction bytes as [|byte bytes IH]; cbn [le_value]; lia. Qed.

Lemma le_value_le_bytes : forall width value,
  (0 <= value < 256 ^ Z.of_nat width)%Z -> le_value (le_bytes width value) = value.
Proof.
  induction width as [|width IH]; intros value Hrange.
  - simpl in Hrange. simpl. lia.
  - rewrite Nat2Z.inj_succ, Z.pow_succ_r in Hrange by lia.
    pose proof (Z.mod_pos_bound value 256 ltac:(lia)) as Hmod.
    pose proof (Z.div_mod value 256 ltac:(lia)) as Hdiv.
    cbn [le_bytes le_value].
    rewrite Z2Nat.id by lia.
    rewrite IH.
    + lia.
    + split.
      * apply Z.div_pos; lia.
      * apply Z.div_lt_upper_bound; lia.
Qed.

Lemma firstn_le_bytes : forall width value rest,
  firstn width (le_bytes width value ++ rest) = le_bytes width value.
Proof. induction width as [|width IH]; intros; simpl; [reflexivity|now rewrite IH]. Qed.

Lemma i32_bytes_length : forall value, length (i32_bytes value) = 4.
Proof. intro value. apply le_bytes_length. Qed.

Lemma i32_value_i32_bytes : forall value,
  peek_in_i32_range value -> i32_value (i32_bytes value) = value.
Proof.
  intros value Hrange. unfold peek_in_i32_range, i32_min, i32_max in Hrange.
  unfold i32_value, i32_bytes, signed_word, i32_max, word_modulus.
  pose proof (Z.mod_pos_bound value 4294967296 ltac:(lia)) as Hmod.
  rewrite le_value_le_bytes.
  2: { change (256 ^ Z.of_nat 4)%Z with 4294967296%Z. lia. }
  rewrite Z.mod_mod by lia.
  destruct (Z.leb_spec 0 value) as [Hnonnegative|Hnegative].
  - rewrite Z.mod_small by lia.
    destruct (Z.leb_spec value 2147483647); lia.
  - assert (Hshift : (value mod 4294967296 = value + 4294967296)%Z).
    { rewrite <- (Z.mod_add value 1 4294967296) by lia.
      rewrite Z.mod_small by lia. lia. }
    rewrite Hshift. destruct (Z.leb_spec (value + 4294967296) 2147483647); lia.
Qed.

Lemma i32_value_in_range : forall bytes, peek_in_i32_range (i32_value bytes).
Proof.
  intro bytes. unfold i32_value, signed_word, peek_in_i32_range, i32_min, i32_max,
    word_modulus.
  pose proof (Z.mod_pos_bound (le_value bytes) 4294967296 ltac:(lia)) as Hbound.
  destruct (Z.leb_spec (le_value bytes mod 4294967296) 2147483647); lia.
Qed.

Lemma decode_words_app_word : forall word rest,
  length word = 4 -> decode_words (word ++ rest) = i32_value word :: decode_words rest.
Proof.
  intros [|first [|second [|third [|fourth [|fifth word]]]]] rest Hlength;
    simpl in Hlength; try lia. reflexivity.
Qed.

Lemma decode_words_in_range : forall bytes, Forall peek_in_i32_range (decode_words bytes).
Proof.
  fix IH 1. intros [|first [|second [|third [|fourth rest]]]]; try constructor.
  - apply i32_value_in_range.
  - apply IH.
Qed.

Lemma decode_words_length : forall bytes words,
  length bytes = 4 * words -> length (decode_words bytes) = words.
Proof.
  fix IH 1. intros [|first [|second [|third [|fourth rest]]]] words Hlength;
    simpl in Hlength.
  - simpl. lia.
  - lia.
  - lia.
  - lia.
  - destruct words as [|words]; [lia|].
    simpl. f_equal. apply IH. lia.
Qed.

Lemma peek_image_cons : forall peek rest,
  peek_image (peek :: rest) = i32_bytes peek ++ peek_image rest.
Proof. reflexivity. Qed.

Lemma decode_peek_image : forall peeks,
  Forall peek_in_i32_range peeks -> decode_words (peek_image peeks) = peeks.
Proof.
  induction peeks as [|peek rest IH]; intro Hrange; [reflexivity|].
  inversion Hrange as [|head tail Hpeek Hrest]; subst.
  rewrite peek_image_cons, decode_words_app_word by apply i32_bytes_length.
  rewrite i32_value_i32_bytes by exact Hpeek.
  f_equal. now apply IH.
Qed.

Lemma peek_image_length : forall peeks, length (peek_image peeks) = 4 * length peeks.
Proof.
  induction peeks as [|peek rest IH]; [reflexivity|].
  rewrite peek_image_cons, length_app, i32_bytes_length, IH. simpl. lia.
Qed.

Lemma pattern_image_length : forall prefix pattern,
  length (pattern_image prefix pattern) = length (prefix pattern) + 4.
Proof. intros. unfold pattern_image. rewrite length_app, i32_bytes_length. reflexivity. Qed.

Lemma pattern_images_cover_words : forall prefix patterns,
  4 * length patterns <= length (concat (map (pattern_image prefix) patterns)).
Proof.
  induction patterns as [|pattern rest IH]; [simpl; lia|].
  cbn [map concat length]. rewrite length_app, pattern_image_length. lia.
Qed.

Lemma strictly_ascending_sorted : forall values,
  strictly_ascending values = true -> StronglySorted Z.lt values.
Proof.
  intros values Hascending. apply Sorted_StronglySorted; [exact Z.lt_trans|].
  induction values as [|first rest IH]; [constructor|].
  destruct rest as [|second rest]; [repeat constructor|].
  change (Z.ltb first second && strictly_ascending (second :: rest) = true) in Hascending.
  apply andb_true_iff in Hascending as [Hfirst Hrest].
  apply Z.ltb_lt in Hfirst. constructor.
  - now apply IH.
  - now constructor.
Qed.

Lemma sorted_strictly_ascending : forall values,
  StronglySorted Z.lt values -> strictly_ascending values = true.
Proof.
  intros values Hsorted. induction Hsorted as [|first rest Hrest IH Hfirst]; [reflexivity|].
  destruct rest as [|second rest]; [reflexivity|].
  change (Z.ltb first second && strictly_ascending (second :: rest) = true).
  apply andb_true_iff. split; [|exact IH].
  apply Z.ltb_lt. now inversion Hfirst.
Qed.

Lemma skipn_exact_prefix : forall (A : Type) (front back : list A),
  skipn (length front) (front ++ back) = back.
Proof. intros A front. induction front as [|value front IH]; intro back; simpl; auto. Qed.

Lemma firstn_exact_prefix : forall (A : Type) (front back : list A),
  firstn (length front) (front ++ back) = front.
Proof.
  intros A front. induction front as [|value front IH]; intro back; simpl;
    [reflexivity|now rewrite IH].
Qed.

Lemma nth_after_prefix : forall (A : Type) (front : list A) value rest default,
  nth (length front) (front ++ value :: rest) default = value.
Proof. intros A front. induction front as [|head front IH]; intros; simpl; auto. Qed.

Lemma nth_after_prefix_next : forall (A : Type) (front : list A) first second rest default,
  nth (S (length front)) (front ++ first :: second :: rest) default = second.
Proof. intros A front. induction front as [|head front IH]; intros; simpl; auto. Qed.

Lemma nth_error_middle : forall (A : Type) (front : list A) value rest position,
  position = length front -> nth_error (front ++ value :: rest) position = Some value.
Proof.
  intros A front value rest position Hposition. subst position.
  induction front as [|head front IH]; simpl; auto.
Qed.

Lemma slice_middle : forall (front middle back : Path) start finish,
  start = length front -> finish = start + length middle ->
  slice start finish (front ++ middle ++ back) = middle.
Proof.
  intros front middle back start finish Hstart Hfinish. subst start finish.
  unfold slice. rewrite skipn_exact_prefix.
  replace (length front + length middle - length front) with (length middle) by lia.
  apply firstn_exact_prefix.
Qed.

Lemma pattern_ends_length : forall images start,
  length (pattern_ends start images) = length images.
Proof. induction images as [|image rest IH]; intro start; simpl; [reflexivity|now rewrite IH]. Qed.

Lemma pattern_ends_last : forall images start,
  nth (length images) (start :: pattern_ends start images) 0 = start + length (concat images).
Proof.
  induction images as [|image images IH]; intro start.
  - simpl. lia.
  - change (nth (length images)
      (start + length image :: pattern_ends (start + length image) images) 0 =
      start + length (image ++ concat images)).
    rewrite IH, length_app. lia.
Qed.

Lemma pattern_ends_ordered : forall images start rest,
  ends_ordered start (pattern_ends start images ++ rest) =
  ends_ordered (start + length (concat images)) rest.
Proof.
  induction images as [|image images IH]; intros start rest.
  - simpl. now rewrite Nat.add_0_r.
  - cbn [pattern_ends concat app ends_ordered].
    rewrite IH, length_app, Nat.add_assoc.
    assert (Hstep : Nat.leb start (start + length image) = true)
      by (apply Nat.leb_le; lia).
    rewrite Hstep. reflexivity.
Qed.

Lemma field_slices_concat : forall images front back start,
  start = length front ->
  field_slices start (pattern_ends start images) (front ++ concat images ++ back) = images.
Proof.
  induction images as [|image images IH]; intros front back start Hstart; [reflexivity|].
  cbn [pattern_ends field_slices concat]. f_equal.
  - rewrite <- app_assoc. apply slice_middle; [exact Hstart|reflexivity].
  - rewrite <- app_assoc, app_assoc. apply IH. rewrite length_app. lia.
Qed.

Lemma last_nonempty_default : forall rest (head : nat) first second,
  last (head :: rest) first = last (head :: rest) second.
Proof.
  induction rest as [|next rest IH]; intros head first second; [reflexivity|].
  change (last (next :: rest) first = last (next :: rest) second). apply IH.
Qed.

Lemma last_cons_default : forall (head : nat) rest default,
  last (head :: rest) default = last rest head.
Proof.
  intros head [|next rest] default; [reflexivity|].
  change (last (next :: rest) default = last (next :: rest) head).
  apply last_nonempty_default.
Qed.

Lemma ends_ordered_last : forall ends start,
  ends_ordered start ends = true -> start <= last ends start.
Proof.
  induction ends as [|finish rest IH]; intros start Hordered; [simpl; lia|].
  cbn [ends_ordered] in Hordered.
  apply andb_true_iff in Hordered as [Hstart Hrest]. apply Nat.leb_le in Hstart.
  rewrite last_cons_default. specialize (IH finish Hrest). lia.
Qed.

Lemma ends_ordered_bounds : forall ends start,
  ends_ordered start ends = true ->
  Forall (fun finish => start <= finish <= last ends start) ends.
Proof.
  induction ends as [|finish rest IH]; intros start Hordered; [constructor|].
  cbn [ends_ordered] in Hordered.
  apply andb_true_iff in Hordered as [Hstart Hrest]. apply Nat.leb_le in Hstart.
  rewrite last_cons_default.
  pose proof (@ends_ordered_last rest finish Hrest) as Hlast.
  constructor; [cbv beta; lia|].
  eapply Forall_impl; [|exact (IH finish Hrest)].
  intros value Hvalue. cbv beta in *. lia.
Qed.

Lemma last_is_nth : forall (ends : list nat) index first second,
  length ends = S index -> last ends first = nth index ends second.
Proof.
  induction ends as [|finish rest IH]; intros index first second Hlength; [discriminate|].
  rewrite last_cons_default. destruct index as [|index].
  - destruct rest as [|next rest]; [reflexivity|discriminate].
  - simpl. apply IH. simpl in Hlength. lia.
Qed.

Lemma slice_length : forall start finish bytes,
  start <= finish -> finish <= length bytes -> length (slice start finish bytes) = finish - start.
Proof.
  intros start finish bytes Hordered Hinside. unfold slice.
  rewrite length_firstn, length_skipn. lia.
Qed.

Lemma admitted_record_facts : forall record ends view,
  admit_record record ends = Some view ->
  view = ByteViewState record ends (arity_of ends) (decode_words (peek_slice record ends)) /\
  Z.of_nat (length ends) = (header_count record + 2)%Z /\
  ends_ordered 8 ends = true /\
  (exists persist, nth_error record (body_finish_of ends) = Some persist /\ persist <= 1) /\
  tail_is_exact (body_finish_of ends) (tail_count record (body_finish_of ends))
    (identity_finish_of ends) = true /\
  strictly_ascending (decode_words (peek_slice record ends)) = true /\
  identity_finish_of ends <= length record.
Proof.
  intros record ends view Hadmit. unfold admit_record in Hadmit.
  destruct (Z.eqb (Z.of_nat (length ends)) (header_count record + 2)%Z) eqn:Hcount;
    [|discriminate].
  destruct (ends_ordered 8 ends) eqn:Hordered; [|discriminate].
  destruct (nth_error record (body_finish_of ends)) as [persist|] eqn:Hpersist;
    [|discriminate].
  destruct (Nat.leb persist 1) eqn:Hboolean; [|discriminate].
  destruct (tail_is_exact (body_finish_of ends) (tail_count record (body_finish_of ends))
    (identity_finish_of ends)) eqn:Htail; [|discriminate].
  destruct (strictly_ascending (decode_words (peek_slice record ends))) eqn:Hascending;
    [|discriminate].
  destruct (Nat.leb (identity_finish_of ends) (length record)) eqn:Hinside; [|discriminate].
  injection Hadmit as Hview. subst view.
  apply Z.eqb_eq in Hcount. apply Nat.leb_le in Hboolean. apply Nat.leb_le in Hinside.
  split; [reflexivity|]. split; [exact Hcount|]. split; [reflexivity|].
  split; [exists persist; split; [reflexivity|exact Hboolean]|].
  split; [reflexivity|]. split; [reflexivity|exact Hinside].
Qed.

Lemma record_image_layout : forall prefix receive,
  record_image prefix receive =
  le_bytes 8 (Z.of_nat (length (receive_patterns receive))) ++
  concat (pattern_images prefix receive) ++
  receive_body receive ++
  persist_byte (receive_persistent receive) ::
  le_bytes 8 (Z.of_nat (length (receive_peeks receive))) ++
  peek_image (receive_peeks receive) ++ receive_source receive.
Proof.
  intros. unfold record_image, identity_image.
  now repeat (rewrite <- app_assoc || rewrite <- app_comm_cons).
Qed.

Lemma record_header : forall prefix receive,
  firstn 8 (record_image prefix receive) =
  le_bytes 8 (Z.of_nat (length (receive_patterns receive))).
Proof. intros. rewrite record_image_layout. apply firstn_le_bytes. Qed.

Lemma record_image_persist_split : forall prefix receive,
  record_image prefix receive =
  (le_bytes 8 (Z.of_nat (length (receive_patterns receive))) ++
   concat (pattern_images prefix receive) ++ receive_body receive) ++
  persist_byte (receive_persistent receive) ::
  le_bytes 8 (Z.of_nat (length (receive_peeks receive))) ++
  peek_image (receive_peeks receive) ++ receive_source receive.
Proof. intros. rewrite record_image_layout. now repeat rewrite <- app_assoc. Qed.

Lemma record_image_tail_split : forall prefix receive,
  record_image prefix receive =
  (le_bytes 8 (Z.of_nat (length (receive_patterns receive))) ++
   concat (pattern_images prefix receive) ++ receive_body receive ++
   [persist_byte (receive_persistent receive)]) ++
  le_bytes 8 (Z.of_nat (length (receive_peeks receive))) ++
  peek_image (receive_peeks receive) ++ receive_source receive.
Proof. intros. rewrite record_image_layout. now repeat rewrite <- app_assoc. Qed.

Lemma record_image_peek_split : forall prefix receive,
  record_image prefix receive =
  (le_bytes 8 (Z.of_nat (length (receive_patterns receive))) ++
   concat (pattern_images prefix receive) ++ receive_body receive ++
   persist_byte (receive_persistent receive) ::
   le_bytes 8 (Z.of_nat (length (receive_peeks receive)))) ++
  peek_image (receive_peeks receive) ++ receive_source receive.
Proof.
  intros. rewrite record_image_layout.
  now repeat (rewrite <- app_assoc || rewrite <- app_comm_cons).
Qed.

Lemma record_image_body_split : forall prefix receive,
  record_image prefix receive =
  (le_bytes 8 (Z.of_nat (length (receive_patterns receive))) ++
   concat (pattern_images prefix receive)) ++
  receive_body receive ++
  persist_byte (receive_persistent receive) ::
  le_bytes 8 (Z.of_nat (length (receive_peeks receive))) ++
  peek_image (receive_peeks receive) ++ receive_source receive.
Proof. intros. rewrite record_image_layout. now repeat rewrite <- app_assoc. Qed.

Lemma identity_image_length : forall prefix receive,
  length (identity_image prefix receive) =
  8 + length (concat (pattern_images prefix receive)) +
    length (receive_body receive) + 9 + 4 * length (receive_peeks receive).
Proof.
  intros. unfold identity_image.
  rewrite !length_app. cbn [length]. rewrite !length_app, !le_bytes_length, peek_image_length.
  lia.
Qed.

Lemma field_bounds_shape : forall prefix receive,
  arity_of (field_bounds prefix receive) = length (receive_patterns receive) /\
  length (field_bounds prefix receive) = S (S (length (receive_patterns receive))) /\
  body_finish_of (field_bounds prefix receive) = body_finish_at prefix receive /\
  identity_finish_of (field_bounds prefix receive) =
    body_finish_at prefix receive + 9 + 4 * length (receive_peeks receive) /\
  ends_ordered 8 (field_bounds prefix receive) = true /\
  firstn (length (receive_patterns receive)) (field_bounds prefix receive) =
    pattern_ends 8 (pattern_images prefix receive) /\
  nth (length (receive_patterns receive)) (8 :: field_bounds prefix receive) 0 =
    8 + length (concat (pattern_images prefix receive)).
Proof.
  intros prefix receive.
  assert (Hcount : length (pattern_ends 8 (pattern_images prefix receive)) =
    length (receive_patterns receive)).
  { unfold pattern_images. now rewrite pattern_ends_length, length_map. }
  assert (Hlength : length (field_bounds prefix receive) =
    S (S (length (receive_patterns receive)))).
  { unfold field_bounds. rewrite length_app, Hcount. cbn [length]. lia. }
  assert (Harity : arity_of (field_bounds prefix receive) = length (receive_patterns receive)).
  { unfold arity_of. rewrite Hlength. lia. }
  split; [exact Harity|]. split; [exact Hlength|].
  unfold body_finish_of, identity_finish_of. rewrite Harity.
  unfold field_bounds.
  split; [rewrite <- Hcount; apply nth_after_prefix|].
  split; [rewrite <- Hcount; apply nth_after_prefix_next|].
  split.
  { rewrite pattern_ends_ordered. cbn [ends_ordered].
    apply andb_true_iff. split; [apply Nat.leb_le; unfold body_finish_at; lia|].
    apply andb_true_iff. split; [apply Nat.leb_le; lia|reflexivity]. }
  split; [rewrite <- Hcount; apply firstn_exact_prefix|].
  rewrite app_comm_cons, app_nth1.
  - replace (length (receive_patterns receive)) with (length (pattern_images prefix receive))
      by (unfold pattern_images; rewrite length_map; reflexivity).
    apply pattern_ends_last.
  - cbn [length]. rewrite Hcount. lia.
Qed.

Theorem encoded_record_admits_exact_fields : forall prefix receive,
  encodable prefix receive ->
  admit_record (record_image prefix receive) (field_bounds prefix receive) =
    Some (encoded_view prefix receive) /\
  view_patterns (encoded_view prefix receive) =
    map (pattern_image prefix) (receive_patterns receive) /\
  view_body (encoded_view prefix receive) = receive_body receive /\
  view_persistent (encoded_view prefix receive) = receive_persistent receive /\
  view_peeks (encoded_view prefix receive) = receive_peeks receive /\
  view_identity (encoded_view prefix receive) = identity_image prefix receive /\
  view_source (encoded_view prefix receive) = receive_source receive.
Proof.
  intros prefix receive [Hpeeks [Hcounts Hsize]].
  destruct (field_bounds_shape prefix receive)
    as [Harity [Hlength [Hbody [Hidentity [Hordered [Hpatterns Hstart]]]]]].
  assert (Hbf : body_finish_at prefix receive =
    8 + length (concat (pattern_images prefix receive)) + length (receive_body receive))
    by reflexivity.
  assert (Hrecord_length : length (record_image prefix receive) =
    body_finish_at prefix receive + 9 + 4 * length (receive_peeks receive) +
    length (receive_source receive)).
  { unfold record_image. rewrite length_app, identity_image_length, Hbf. lia. }
  assert (Hcover : 4 * length (receive_patterns receive) <=
    length (concat (pattern_images prefix receive))).
  { unfold pattern_images. apply pattern_images_cover_words. }
  unfold word_max, position_max in Hsize.
  assert (Hheader : header_count (record_image prefix receive) =
    Z.of_nat (length (receive_patterns receive))).
  { unfold header_count. rewrite record_header. apply le_value_le_bytes.
    change (256 ^ Z.of_nat 8)%Z with 18446744073709551616%Z. lia. }
  assert (Hpersist : nth_error (record_image prefix receive) (body_finish_at prefix receive) =
    Some (persist_byte (receive_persistent receive))).
  { rewrite record_image_persist_split. apply nth_error_middle.
    rewrite !length_app, le_bytes_length, Hbf. lia. }
  assert (Htail_count : tail_count (record_image prefix receive) (body_finish_at prefix receive) =
    Z.of_nat (length (receive_peeks receive))).
  { unfold tail_count. rewrite record_image_tail_split, slice_middle.
    - apply le_value_le_bytes.
      change (256 ^ Z.of_nat 8)%Z with 18446744073709551616%Z. lia.
    - rewrite !length_app, le_bytes_length. cbn [length]. rewrite Hbf. lia.
    - rewrite le_bytes_length. lia. }
  assert (Hpeek_slice : peek_slice (record_image prefix receive) (field_bounds prefix receive) =
    peek_image (receive_peeks receive)).
  { unfold peek_slice. rewrite Hbody, Hidentity, record_image_peek_split. apply slice_middle.
    - rewrite !length_app. cbn [length]. rewrite !le_bytes_length, Hbf. lia.
    - rewrite peek_image_length. lia. }
  assert (Hdecoded : decode_words (peek_image (receive_peeks receive)) = receive_peeks receive)
    by (apply decode_peek_image; exact (proj2 Hpeeks)).
  assert (Hadmit : admit_record (record_image prefix receive) (field_bounds prefix receive) =
    Some (encoded_view prefix receive)).
  { unfold admit_record.
    rewrite Hlength, Hheader, Hordered, Hbody, Hpersist.
    cbv beta iota.
    assert (Hcount_check : Z.eqb (Z.of_nat (S (S (length (receive_patterns receive)))))
      (Z.of_nat (length (receive_patterns receive)) + 2)%Z = true) by (apply Z.eqb_eq; lia).
    assert (Hboolean : Nat.leb (persist_byte (receive_persistent receive)) 1 = true)
      by (destruct (receive_persistent receive); reflexivity).
    rewrite Hcount_check, Hboolean, Htail_count, Hidentity.
    assert (Htail_check : tail_is_exact (body_finish_at prefix receive)
      (Z.of_nat (length (receive_peeks receive)))
      (body_finish_at prefix receive + 9 + 4 * length (receive_peeks receive)) = true).
    { unfold tail_is_exact, word_max, position_max.
      apply andb_true_iff. split; [apply Z.eqb_eq; lia|apply Z.leb_le; lia]. }
    rewrite Htail_check, Hpeek_slice, Hdecoded, (sorted_strictly_ascending (proj1 Hpeeks)).
    assert (Hinside_check : Nat.leb
      (body_finish_at prefix receive + 9 + 4 * length (receive_peeks receive))
      (length (record_image prefix receive)) = true) by (apply Nat.leb_le; lia).
    rewrite Hinside_check, Harity. reflexivity. }
  split; [exact Hadmit|].
  assert (Hend : nth (length (receive_patterns receive)) (field_bounds prefix receive) 0 =
    body_finish_at prefix receive).
  { unfold body_finish_of in Hbody. rewrite Harity in Hbody. exact Hbody. }
  assert (Hidentity_end :
    nth (S (length (receive_patterns receive))) (field_bounds prefix receive) 0 =
    length (identity_image prefix receive)).
  { unfold identity_finish_of in Hidentity. rewrite Harity in Hidentity.
    rewrite Hidentity, identity_image_length, Hbf. lia. }
  unfold view_patterns, view_body, view_persistent, view_identity, view_source,
    field_start, body_end, identity_end, encoded_view.
  cbn [view_record view_ends view_arity view_peeks].
  rewrite Hend, Hidentity_end, Hpersist, Hstart, Hpatterns.
  split.
  { change (map (pattern_image prefix) (receive_patterns receive))
      with (pattern_images prefix receive).
    rewrite record_image_layout. apply field_slices_concat. now rewrite le_bytes_length. }
  split.
  { rewrite record_image_body_split. apply slice_middle.
    - rewrite length_app, le_bytes_length. reflexivity.
    - rewrite Hbf. reflexivity. }
  split; [destruct (receive_persistent receive); reflexivity|].
  split; [reflexivity|].
  unfold record_image. split; [apply firstn_exact_prefix|apply skipn_exact_prefix].
Qed.

Theorem admitted_receive_refuses_mismatched_arity : forall group record ends view,
  admit_record record ends = Some view ->
  length (group_images group) <> view_arity view ->
  admit_receive group record ends = None.
Proof.
  intros group record ends view Hadmit Hmismatch. unfold admit_receive. rewrite Hadmit.
  cbv beta iota.
  destruct (Nat.eqb (length (group_images group)) (view_arity view)) eqn:Harity;
    [apply Nat.eqb_eq in Harity; contradiction|reflexivity].
Qed.

Theorem admitted_bounds_are_ordered_and_inside_record : forall record ends view,
  admit_record record ends = Some view ->
  length ends = S (S (view_arity view)) /\
  ends_ordered 8 ends = true /\
  Forall (fun finish => 8 <= finish <= length record) ends.
Proof.
  intros record ends view Hadmit.
  destruct (@admitted_record_facts record ends view Hadmit)
    as [Hview [Hcount [Hordered [_ [_ [_ Hinside]]]]]].
  subst view. cbn [view_arity].
  pose proof (le_value_nonnegative (firstn 8 record)) as Hnonnegative.
  unfold header_count in Hcount.
  assert (Hlength : length ends = S (S (arity_of ends))) by (unfold arity_of; lia).
  split; [exact Hlength|]. split; [exact Hordered|].
  apply Forall_forall. intros finish Hin.
  pose proof (proj1 (Forall_forall _ ends) (@ends_ordered_bounds ends 8 Hordered) finish Hin)
    as Hbounds.
  cbv beta in Hbounds |- *.
  rewrite (@last_is_nth ends (S (arity_of ends)) 8 0 Hlength) in Hbounds.
  unfold identity_finish_of in Hinside. lia.
Qed.

Theorem admitted_peek_tail_is_checked_exactly : forall record ends view,
  admit_record record ends = Some view ->
  (Z.of_nat (body_end view) + 9 + 4 * tail_count record (body_end view) =
     Z.of_nat (identity_end view))%Z /\
  (Z.of_nat (identity_end view) <= word_max)%Z /\
  Z.of_nat (length (view_peeks view)) = tail_count record (body_end view).
Proof.
  intros record ends view Hadmit.
  destruct (@admitted_record_facts record ends view Hadmit)
    as [Hview [_ [_ [_ [Htail [_ Hinside]]]]]].
  subst view. unfold body_end, identity_end. cbn [view_arity view_ends view_peeks].
  unfold tail_is_exact, body_finish_of, identity_finish_of in Htail.
  unfold identity_finish_of in Hinside.
  apply andb_true_iff in Htail as [Hequation Hbound].
  apply Z.eqb_eq in Hequation. apply Z.leb_le in Hbound.
  split; [exact Hequation|]. split; [exact Hbound|].
  assert (Hnonnegative : (0 <= tail_count record (nth (arity_of ends) ends 0%nat))%Z)
    by apply le_value_nonnegative.
  rewrite (@decode_words_length (peek_slice record ends)
    (Z.to_nat (tail_count record (nth (arity_of ends) ends 0)))).
  - lia.
  - unfold peek_slice, body_finish_of, identity_finish_of.
    rewrite slice_length by lia. lia.
Qed.

Theorem admitted_peeks_are_canonical_signed : forall record ends view,
  admit_record record ends = Some view -> canonical_peeks (view_peeks view).
Proof.
  intros record ends view Hadmit.
  destruct (@admitted_record_facts record ends view Hadmit)
    as [Hview [_ [_ [_ [_ [Hascending _]]]]]].
  subst view. cbn [view_peeks]. split.
  - now apply strictly_ascending_sorted.
  - apply decode_words_in_range.
Qed.

Theorem admitted_persist_is_boolean : forall record ends view,
  admit_record record ends = Some view ->
  nth_error record (body_end view) = Some (persist_byte (view_persistent view)).
Proof.
  intros record ends view Hadmit.
  destruct (@admitted_record_facts record ends view Hadmit)
    as [Hview [_ [_ [[persist [Hpersist Hboolean]] _]]]].
  subst view. unfold view_persistent, body_end. cbn [view_record view_arity view_ends].
  unfold body_finish_of in Hpersist. rewrite Hpersist.
  destruct persist as [|[|persist]]; [reflexivity|reflexivity|lia].
Qed.

Lemma view_effective_peek_refines_effective_peek : forall peeks position,
  canonical_peeks peeks ->
  view_effective_peek peeks (Z.of_nat position) = effective_peek peeks position.
Proof.
  intros peeks position Hcanonical. unfold view_effective_peek.
  destruct (Z.leb_spec (Z.of_nat position) i32_max) as [Hnarrow|Hwide].
  - reflexivity.
  - symmetry. now apply wide_position_does_not_wrap_to_signed_peek.
Qed.

Theorem admitted_view_binds_refine_bind_at : forall prefix receive profile,
  encodable prefix receive -> aligned receive ->
  admit_receive (ChannelGroupState profile (receive_channels receive))
    (record_image prefix receive) (field_bounds prefix receive) =
    Some (ReceiveViewState (ChannelGroupState profile (receive_channels receive))
      (encoded_view prefix receive)) /\
  forall index,
    view_bind (ReceiveViewState (ChannelGroupState profile (receive_channels receive))
      (encoded_view prefix receive)) index =
    option_map (bind_image prefix) (bind_at receive index).
Proof.
  intros prefix receive profile Hencodable Haligned. unfold aligned in Haligned.
  destruct (@encoded_record_admits_exact_fields prefix receive Hencodable)
    as [Hadmit [Hpatterns [_ [_ [Hpeeks _]]]]].
  split.
  - unfold admit_receive. rewrite Hadmit. cbn [group_images].
    change (view_arity (encoded_view prefix receive)) with (length (receive_patterns receive)).
    rewrite Haligned, Nat.eqb_refl. reflexivity.
  - intro index. unfold view_bind. cbn [receive_view_group receive_view_bytes].
    unfold group_frames. cbn [group_images].
    rewrite Hpatterns, Hpeeks, !nth_error_map.
    unfold bind_at. rewrite Haligned, Nat.eqb_refl.
    destruct Hencodable as [Hcanonical _].
    destruct (nth_error (receive_channels receive) index) as [channel|];
      destruct (nth_error (receive_patterns receive) index) as [pattern|];
      cbn [option_map]; try reflexivity.
    unfold bind_image. cbn [bind_channel bind_pattern bind_peek].
    now rewrite view_effective_peek_refines_effective_peek.
Qed.

Lemma framed_channel_at_is : forall images query position,
  Forall (fun bytes => RhoPathV1Datum.bytes_valid bytes = true) images ->
  RhoPathV1Datum.bytes_valid query = true ->
  channel_at_is (map RhoPathV1Datum.encode_frame images)
    (RhoPathV1Datum.encode_frame query) position =
  channel_at_is images query position.
Proof.
  intros images query position Himages Hquery.
  apply Bool.eq_true_iff_eq. rewrite !channel_at_is_exact, framed_nth_error.
  change (@nth_error RhoPathV1Datum.bytes images position)
    with (@nth_error Path images position).
  split.
  - intro Hframed.
    destruct (@nth_error Path images position) as [image|] eqn:Himage; [|discriminate].
    cbn [option_map] in Hframed. injection Hframed as Hframes.
    assert (Hvalid : RhoPathV1Datum.bytes_valid image = true).
    { rewrite Forall_forall in Himages. apply Himages. eapply nth_error_In. exact Himage. }
    now rewrite (frame_equality_preserves_channel image query Hvalid Hquery Hframes).
  - intro Himage. now rewrite Himage.
Qed.

Lemma profile_eqb_refl : forall profile, profile_eqb profile profile = true.
Proof. intros [|]; reflexivity. Qed.

Lemma profile_eqb_sound : forall left right, profile_eqb left right = true -> left = right.
Proof. intros [|] [|] Hequal; solve [reflexivity|discriminate]. Qed.

Theorem view_positions_are_exact_image_positions : forall group query,
  Forall (fun bytes => RhoPathV1Datum.bytes_valid bytes = true) (group_images group) ->
  RhoPathV1Datum.bytes_valid query = true ->
  view_positions group (group_profile group) query =
    Some (positions (group_images group) query).
Proof.
  intros group query Himages Hquery. unfold view_positions.
  rewrite profile_eqb_refl. cbv beta iota. f_equal.
  unfold positions, group_frames. rewrite length_map.
  apply filter_ext. intro position. now apply framed_channel_at_is.
Qed.

Theorem prefix_related_images_are_not_aliases : forall group query position image,
  Forall (fun bytes => RhoPathV1Datum.bytes_valid bytes = true) (group_images group) ->
  RhoPathV1Datum.bytes_valid query = true ->
  nth_error (group_images group) position = Some image ->
  image <> query ->
  exists found, view_positions group (group_profile group) query = Some found /\
    ~ In position found.
Proof.
  intros group query position image Himages Hquery Himage Hdifferent.
  exists (positions (group_images group) query).
  split; [now apply view_positions_are_exact_image_positions|].
  intro Hin. apply positions_exact in Hin. rewrite Himage in Hin.
  injection Hin as Hsame. contradiction.
Qed.

Theorem cross_profile_query_is_refused : forall group profile query,
  profile <> group_profile group -> view_positions group profile query = None.
Proof.
  intros group profile query Hdifferent. unfold view_positions.
  destruct (profile_eqb (group_profile group) profile) eqn:Hprofile; [|reflexivity].
  apply profile_eqb_sound in Hprofile. symmetry in Hprofile. contradiction.
Qed.

Theorem empty_aligned_view_has_no_bind_or_position :
  forall prefix body persistent peeks source profile query,
  encodable prefix (ReceiveFieldsState [] [] body persistent peeks source) ->
  exists view,
    admit_receive (ChannelGroupState profile [])
      (record_image prefix (ReceiveFieldsState [] [] body persistent peeks source))
      (field_bounds prefix (ReceiveFieldsState [] [] body persistent peeks source)) =
      Some view /\
    (forall index, view_bind view index = None) /\
    view_positions (ChannelGroupState profile []) profile query = Some [].
Proof.
  intros prefix body persistent peeks source profile query Hencodable.
  destruct (@admitted_view_binds_refine_bind_at prefix
    (ReceiveFieldsState [] [] body persistent peeks source) profile Hencodable eq_refl)
    as [Hadmit Hbinds].
  eexists. split; [exact Hadmit|]. split.
  - intro index. rewrite Hbinds, empty_aligned_receive_has_no_bind. reflexivity.
  - unfold view_positions. cbn [group_profile group_frames group_images map].
    rewrite profile_eqb_refl. reflexivity.
Qed.

Theorem source_suffix_is_exact_and_identity_is_source_free : forall prefix receive source,
  encodable prefix (replace_source receive source) ->
  admit_record (record_image prefix (replace_source receive source))
    (field_bounds prefix receive) = Some (encoded_view prefix (replace_source receive source)) /\
  view_identity (encoded_view prefix (replace_source receive source)) =
    identity_image prefix receive /\
  view_source (encoded_view prefix (replace_source receive source)) = source.
Proof.
  intros prefix receive source Hencodable.
  destruct (@encoded_record_admits_exact_fields prefix (replace_source receive source)
    Hencodable) as [Hadmit [_ [_ [_ [_ [Hidentity Hsource]]]]]].
  split; [exact Hadmit|]. split; [exact Hidentity|exact Hsource].
Qed.

Theorem first_view_identity_retains_head_source_suffix : forall prefix receive tail aliases,
  encodable prefix receive ->
  first_identity (identity_image prefix receive)
    ((view_continuation (encoded_view prefix receive) :: tail) ++ aliases) =
    Some (view_continuation (encoded_view prefix receive)) /\
  continuation_source (view_continuation (encoded_view prefix receive)) =
    receive_source receive.
Proof.
  intros prefix receive tail aliases Hencodable.
  destruct (@encoded_record_admits_exact_fields prefix receive Hencodable)
    as [_ [_ [_ [_ [_ [Hidentity Hsource]]]]]].
  split.
  - apply appending_aliases_preserves_first_identity.
    assert (Hhead : continuation_identity (view_continuation (encoded_view prefix receive)) =
      identity_image prefix receive) by exact Hidentity.
    rewrite <- Hhead. apply first_identity_retains_head_source.
  - exact Hsource.
Qed.

Theorem free_count_is_trailing_signed_word : forall prefix receive position pattern,
  encodable prefix receive ->
  nth_error (receive_patterns receive) position = Some pattern ->
  exists bytes,
    nth_error (view_patterns (encoded_view prefix receive)) position = Some bytes /\
    i32_value (trailing_word bytes) = pattern_free_count pattern.
Proof.
  intros prefix receive position pattern Hencodable Hpattern.
  destruct (@encoded_record_admits_exact_fields prefix receive Hencodable)
    as [_ [Hpatterns _]].
  exists (pattern_image prefix pattern). split.
  - rewrite Hpatterns, nth_error_map, Hpattern. reflexivity.
  - unfold trailing_word, pattern_image.
    rewrite length_app, i32_bytes_length.
    replace (length (prefix pattern) + 4 - 4) with (length (prefix pattern)) by lia.
    rewrite skipn_exact_prefix. apply i32_value_i32_bytes.
    destruct Hencodable as [_ [Hcounts _]]. rewrite Forall_forall in Hcounts.
    apply (Hcounts pattern). eapply nth_error_In. exact Hpattern.
Qed.

Theorem wide_view_position_does_not_wrap : forall peeks position,
  (i32_max < position)%Z -> view_effective_peek peeks position = false.
Proof.
  intros peeks position Hwide. unfold view_effective_peek.
  destruct (Z.leb_spec position i32_max); [lia|reflexivity].
Qed.

Print Assumptions encoded_record_admits_exact_fields.
Print Assumptions admitted_receive_refuses_mismatched_arity.
Print Assumptions admitted_bounds_are_ordered_and_inside_record.
Print Assumptions admitted_peek_tail_is_checked_exactly.
Print Assumptions admitted_peeks_are_canonical_signed.
Print Assumptions admitted_persist_is_boolean.
Print Assumptions admitted_view_binds_refine_bind_at.
Print Assumptions view_positions_are_exact_image_positions.
Print Assumptions prefix_related_images_are_not_aliases.
Print Assumptions cross_profile_query_is_refused.
Print Assumptions empty_aligned_view_has_no_bind_or_position.
Print Assumptions source_suffix_is_exact_and_identity_is_source_free.
Print Assumptions first_view_identity_retains_head_source_suffix.
Print Assumptions free_count_is_trailing_signed_word.
Print Assumptions wide_view_position_does_not_wrap.

End OrderedReceiveByteView.

(** Installed-join restoration.

    The installed joins of a channel live in an ordered group store: each
    stored join carries an order number, and a read lists the joins by
    descending order, newest first.  Installation gives an absent join the
    order after the newest one, so it is read first.  Replacing a whole group
    of [n] joins gives the join at input index [i] the order [n - i], so a
    read returns the input sequence.

    A store is represented by its read sequence of occurrences.  Both
    constructions keep the orders strictly descending, so the production
    read's descending sort is the identity on this representation and its
    tie-break never applies.

    Restoring a snapshot must therefore replace the group: replaying the
    snapshot through installation returns distinct joins reversed. *)
Module InstalledJoinRestoration.
Open Scope nat_scope.

Definition JoinOccurrence : Type := (nat * GroupKey)%type.

Definition read_store (store : list JoinOccurrence) : list GroupKey :=
  map snd store.

Definition orders_descending (store : list JoinOccurrence) : Prop :=
  StronglySorted
    (fun newer older : JoinOccurrence => fst older < fst newer) store.

Definition next_order (store : list JoinOccurrence) : nat :=
  match store with
  | [] => 1
  | (order, _) :: _ => S order
  end.

Definition insert_join (store : list JoinOccurrence) (join : GroupKey)
    : list JoinOccurrence :=
  if in_dec group_eq_dec join (read_store store)
  then store
  else (next_order store, join) :: store.

Fixpoint replace_group (joins : list GroupKey) : list JoinOccurrence :=
  match joins with
  | [] => []
  | join :: tail => (length joins, join) :: replace_group tail
  end.

Definition restore_by_install (joins : list GroupKey) : list JoinOccurrence :=
  fold_left insert_join joins [].

Lemma replace_group_orders_bounded : forall joins occurrence,
  In occurrence (replace_group joins) -> fst occurrence <= length joins.
Proof.
  induction joins as [|join tail IH]; simpl; intros occurrence Hin.
  - contradiction.
  - destruct Hin as [Heq|Hin].
    + subst occurrence. simpl. lia.
    + specialize (IH occurrence Hin). lia.
Qed.

Theorem replace_group_orders_descending : forall joins,
  orders_descending (replace_group joins).
Proof.
  unfold orders_descending.
  induction joins as [|join tail IH]; simpl.
  - constructor.
  - constructor; [exact IH|].
    apply Forall_forall. intros occurrence Hin.
    apply replace_group_orders_bounded in Hin. simpl. lia.
Qed.

Lemma next_order_exceeds_every_order : forall store,
  orders_descending store ->
  Forall (fun occurrence : JoinOccurrence => fst occurrence < next_order store)
    store.
Proof.
  intros store Hsorted. destruct store as [|[order join] tail].
  - constructor.
  - inversion Hsorted as [|head rest Htail Hhead]; subst.
    constructor.
    + simpl. lia.
    + apply Forall_forall. intros occurrence Hin.
      rewrite Forall_forall in Hhead. specialize (Hhead occurrence Hin).
      simpl in *. lia.
Qed.

Theorem insert_join_orders_descending : forall store join,
  orders_descending store -> orders_descending (insert_join store join).
Proof.
  intros store join Hsorted. unfold insert_join.
  destruct (in_dec group_eq_dec join (read_store store)) as [_|_].
  - exact Hsorted.
  - pose proof (@next_order_exceeds_every_order store Hsorted) as Hexceeds.
    unfold orders_descending in *. constructor; [exact Hsorted|].
    apply Forall_forall. intros occurrence Hin.
    rewrite Forall_forall in Hexceeds. exact (Hexceeds occurrence Hin).
Qed.

Lemma read_insert_absent : forall store join,
  ~ In join (read_store store) ->
  read_store (insert_join store join) = join :: read_store store.
Proof.
  intros store join Habsent. unfold insert_join.
  destruct (in_dec group_eq_dec join (read_store store)) as [Hpresent|_].
  - contradiction.
  - reflexivity.
Qed.

Lemma restore_by_install_from : forall joins store,
  NoDup joins ->
  (forall join, In join joins -> ~ In join (read_store store)) ->
  read_store (fold_left insert_join joins store) =
    rev joins ++ read_store store.
Proof.
  induction joins as [|join tail IH]; intros store Hnodup Hfresh.
  - reflexivity.
  - cbn [fold_left rev].
    inversion Hnodup as [|head rest Hnotin Hnodup_tail]; subst.
    assert (Habsent : ~ In join (read_store store))
      by (apply Hfresh; left; reflexivity).
    rewrite IH.
    + rewrite (@read_insert_absent store join Habsent), <- app_assoc.
      reflexivity.
    + exact Hnodup_tail.
    + intros other Hother.
      rewrite (@read_insert_absent store join Habsent).
      intros [Hsame|Hin].
      * subst other. contradiction.
      * exact (Hfresh other (or_intror Hother) Hin).
Qed.

Theorem restore_by_install_reverses : forall joins,
  NoDup joins -> read_store (restore_by_install joins) = rev joins.
Proof.
  intros joins Hnodup. unfold restore_by_install.
  rewrite restore_by_install_from.
  - exact (app_nil_r (rev joins)).
  - exact Hnodup.
  - intros join _ Hin. exact Hin.
Qed.

Theorem replace_group_round_trip : forall joins,
  read_store (replace_group joins) = joins.
Proof.
  induction joins as [|join tail IH].
  - reflexivity.
  - change (join :: read_store (replace_group tail) = join :: tail).
    rewrite IH. reflexivity.
Qed.

Definition restore_installed_channel
    (state : ContinuationAuthority) (channel : Path) (joins : list GroupKey)
    : ContinuationAuthority :=
  ContinuationAuthorityState
    (installed_continuations state)
    (ordinary_continuations state)
    (continuation_epochs state)
    (resting_joins state)
    (set_channel (installed_joins state) channel
      (read_store (replace_group joins))).

Theorem restore_installed_channel_exact : forall state channel joins,
  installed_joins (restore_installed_channel state channel joins) channel =
    joins.
Proof.
  intros state channel joins.
  change (set_channel (installed_joins state) channel
    (read_store (replace_group joins)) channel = joins).
  rewrite set_channel_exact. apply replace_group_round_trip.
Qed.

Theorem restore_installed_channel_preserves_validity :
  forall state channel joins,
    continuation_authority_valid state ->
    NoDup joins ->
    continuation_authority_valid (restore_installed_channel state channel joins).
Proof.
  intros state channel joins
    [Hordinary_unique [Hordinary [Hinstalled [Hresting Hinstalled_joins]]]]
    Hunique.
  unfold continuation_authority_valid.
  split; [exact Hordinary_unique|].
  split; [exact Hordinary|].
  split; [exact Hinstalled|].
  split; [exact Hresting|].
  intros other.
  change (NoDup (set_channel (installed_joins state) channel
    (read_store (replace_group joins)) other)).
  destruct (path_eq_dec other channel) as [Hsame|Hdifferent].
  - subst other. rewrite set_channel_exact, replace_group_round_trip.
    exact Hunique.
  - rewrite set_channel_other by exact Hdifferent. apply Hinstalled_joins.
Qed.

Print Assumptions replace_group_orders_descending.
Print Assumptions insert_join_orders_descending.
Print Assumptions replace_group_round_trip.
Print Assumptions restore_by_install_reverses.
Print Assumptions restore_installed_channel_exact.
Print Assumptions restore_installed_channel_preserves_validity.

End InstalledJoinRestoration.

Print Assumptions EqualityEpoch.fresh_from_rejects_every_prior.
Print Assumptions issued_ordinary_capability_resolves.
Print Assumptions stale_epoch_cannot_resolve.
Print Assumptions exact_resolution_preserves_expected_source_persistence_and_peeks.
Print Assumptions exact_resolution_identity_comes_from_bound_path_or_installed_slot.
Print Assumptions exact_resolution_preserves_canonical_signed_peeks.
Print Assumptions nonpersistent_removal_preserves_authority_validity.
Print Assumptions accepted_nonpersistent_removes_exactly_one_ordinary.
Print Assumptions final_nonpersistent_removal_cleans_exact_resting_join_group.
Print Assumptions prefix_related_groups_do_not_alias_authority.
Print Assumptions prefix_related_join_group_membership_is_preserved.

End TwoLevelRSpaceContinuationAuthority.
