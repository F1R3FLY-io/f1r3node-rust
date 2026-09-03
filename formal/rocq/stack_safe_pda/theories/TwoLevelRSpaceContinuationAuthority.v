From Stdlib Require Import
  Bool List Arith Lia ZArith Sorting.Sorted.
From StackSafePDA Require Import
  PathMapCanonicalStreamPublicationBase TwoLevelRSpaceAuthority.

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
