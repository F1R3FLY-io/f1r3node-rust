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
