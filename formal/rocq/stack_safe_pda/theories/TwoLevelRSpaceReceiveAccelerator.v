From Stdlib Require Import Bool List Arith Lia ZArith Sorting.Permutation Sorting.Sorted.
From StackSafePDA Require Import PathMapCanonicalStreamPublicationBase
  TwoLevelRSpaceAuthority RhoPathV1Datum TwoLevelRSpaceContinuationAuthority.
Import ListNotations.
Import TwoLevelRSpaceAuthority.
Import TwoLevelRSpaceContinuationAuthority.
Import OrderedReceivePositions.

Open Scope nat_scope.

Module TwoLevelRSpaceReceiveAccelerator.

Definition product_eq_dec {A B : Type}
    (first_eq_dec : forall left right : A, {left = right} + {left <> right})
    (second_eq_dec : forall left right : B, {left = right} + {left <> right})
    (left right : A * B) : {left = right} + {left <> right}.
Proof.
  destruct left as [left_first left_second].
  destruct right as [right_first right_second].
  destruct (first_eq_dec left_first right_first) as [Hfirst|Hfirst];
    [subst right_first|right; congruence].
  destruct (second_eq_dec left_second right_second) as [Hsecond|Hsecond];
    [subst right_second; left; reflexivity|right; congruence].
Defined.

Inductive ReceiveKind : Type :=
| InstalledKind
| RestingKind.

Definition receive_kind_eq_dec (left right : ReceiveKind)
    : {left = right} + {left <> right}.
Proof. decide equality. Defined.

Record ReceiveTarget : Type := ReceiveTargetState {
  receive_target_kind : ReceiveKind;
  receive_target_group : GroupKey;
  receive_target_record : ContinuationRecord
}.

Definition receive_target_eq_dec (left right : ReceiveTarget)
    : {left = right} + {left <> right}.
Proof.
  destruct left as [left_kind left_group left_record].
  destruct right as [right_kind right_group right_record].
  destruct (receive_kind_eq_dec left_kind right_kind) as [Hkind|Hkind];
    [subst right_kind|right; congruence].
  destruct (group_eq_dec left_group right_group) as [Hgroup|Hgroup];
    [subst right_group|right; congruence].
  destruct (continuation_record_eq_dec left_record right_record)
    as [Hrecord|Hrecord];
    [subst right_record; left; reflexivity|right; congruence].
Defined.

Record Revision : Type := RevisionState {
  revision_generation : EqualityEpoch.t;
  revision_store : EqualityEpoch.t;
  revision_installed : EqualityEpoch.t
}.

Definition revision_eq_dec (left right : Revision)
    : {left = right} + {left <> right}.
Proof.
  destruct left as [left_generation left_store left_installed].
  destruct right as [right_generation right_store right_installed].
  destruct (EqualityEpoch.eq_dec left_generation right_generation)
    as [Hgeneration|Hgeneration];
    [subst right_generation|right; congruence].
  destruct (EqualityEpoch.eq_dec left_store right_store) as [Hstore|Hstore];
    [subst right_store|right; congruence].
  destruct (EqualityEpoch.eq_dec left_installed right_installed)
    as [Hinstalled|Hinstalled];
    [subst right_installed; left; reflexivity|right; congruence].
Defined.

Record ReceiveId : Type := ReceiveIdState {
  receive_id_revision : Revision;
  receive_id_target : ReceiveTarget
}.

Record Occurrence : Type := OccurrenceState {
  occurrence_channel : Path;
  occurrence_group : GroupKey;
  occurrence_positions : list nat;
  occurrence_target : ReceiveTarget
}.

Record Artifacts : Type := ArtifactsState {
  catalog_revision : Revision;
  occurrence_revision : Revision;
  automaton_revision : Revision
}.

Definition artifacts_at (revision : Revision) : Artifacts :=
  ArtifactsState revision revision revision.

Definition artifacts_eq_dec (left right : Artifacts)
    : {left = right} + {left <> right}.
Proof.
  destruct left as [left_catalog left_occurrence left_automaton].
  destruct right as [right_catalog right_occurrence right_automaton].
  destruct (revision_eq_dec left_catalog right_catalog) as [Hcatalog|Hcatalog];
    [subst right_catalog|right; congruence].
  destruct (revision_eq_dec left_occurrence right_occurrence)
    as [Hoccurrence|Hoccurrence];
    [subst right_occurrence|right; congruence].
  destruct (revision_eq_dec left_automaton right_automaton)
    as [Hautomaton|Hautomaton];
    [subst right_automaton; left; reflexivity|right; congruence].
Defined.

Section AssociationLists.

Context {Key Value : Type}.
Variable key_eq_dec : forall left right : Key, {left = right} + {left <> right}.

Fixpoint assoc_lookup (entries : list (Key * Value)) (key : Key)
    : option Value :=
  match entries with
  | [] => None
  | (entry_key, entry_value) :: tail =>
      if key_eq_dec entry_key key then Some entry_value
      else assoc_lookup tail key
  end.

Definition assoc_get (default : Value) (entries : list (Key * Value))
    (key : Key) : Value :=
  match assoc_lookup entries key with
  | Some value => value
  | None => default
  end.

Fixpoint assoc_set (entries : list (Key * Value)) (key : Key) (value : Value)
    : list (Key * Value) :=
  match entries with
  | [] => [(key, value)]
  | (entry_key, entry_value) :: tail =>
      if key_eq_dec entry_key key then (key, value) :: tail
      else (entry_key, entry_value) :: assoc_set tail key value
  end.

Fixpoint assoc_remove (entries : list (Key * Value)) (key : Key)
    : list (Key * Value) :=
  match entries with
  | [] => []
  | (entry_key, entry_value) :: tail =>
      if key_eq_dec entry_key key then assoc_remove tail key
      else (entry_key, entry_value) :: assoc_remove tail key
  end.

Lemma assoc_lookup_set_same : forall entries key value,
  assoc_lookup (assoc_set entries key value) key = Some value.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value; simpl.
  - destruct (key_eq_dec key key); [reflexivity|congruence].
  - destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent]; simpl.
    + destruct (key_eq_dec key key); [reflexivity|congruence].
    + destruct (key_eq_dec entry_key key); [congruence|apply IH].
Qed.

Lemma assoc_lookup_set_other : forall entries key value other,
  other <> key ->
  assoc_lookup (assoc_set entries key value) other = assoc_lookup entries other.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value other Hother; simpl.
  - destruct (key_eq_dec key other); [congruence|reflexivity].
  - destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent]; simpl.
    + subst entry_key.
      destruct (key_eq_dec key other); [congruence|reflexivity].
    + destruct (key_eq_dec entry_key other); [reflexivity|].
      apply IH. exact Hother.
Qed.

Lemma assoc_lookup_remove_same : forall entries key,
  assoc_lookup (assoc_remove entries key) key = None.
Proof.
  induction entries as [|[entry_key entry_value] tail IH]; intro key; simpl;
    [reflexivity|].
  destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent]; [apply IH|simpl].
  destruct (key_eq_dec entry_key key); [congruence|apply IH].
Qed.

Lemma assoc_lookup_remove_other : forall entries key other,
  other <> key ->
  assoc_lookup (assoc_remove entries key) other = assoc_lookup entries other.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key other Hother; simpl; [reflexivity|].
  destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent].
  - subst entry_key.
    destruct (key_eq_dec key other); [congruence|].
    apply IH. exact Hother.
  - simpl. destruct (key_eq_dec entry_key other); [reflexivity|].
    apply IH. exact Hother.
Qed.

Lemma assoc_lookup_some_key : forall entries key value,
  assoc_lookup entries key = Some value -> In key (map fst entries).
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value Hlookup; simpl in *; [discriminate|].
  destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent].
  - left. exact Hsame.
  - right. eapply IH. exact Hlookup.
Qed.

Lemma assoc_lookup_some_in : forall entries key value,
  assoc_lookup entries key = Some value -> In (key, value) entries.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value Hlookup; simpl in *; [discriminate|].
  destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent].
  - injection Hlookup as Hvalue. subst. left. reflexivity.
  - right. apply IH. exact Hlookup.
Qed.

Lemma assoc_lookup_in : forall entries key value,
  NoDup (map fst entries) -> In (key, value) entries ->
  assoc_lookup entries key = Some value.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value Hunique Hin; simpl in *; [contradiction|].
  apply NoDup_cons_iff in Hunique. destruct Hunique as [Hnotin Htail].
  destruct Hin as [Hhead|Htail_in].
  - injection Hhead as Hkey Hvalue.
    destruct (key_eq_dec entry_key key); congruence.
  - destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent].
    + subst entry_key. exfalso. apply Hnotin.
      apply (in_map fst) in Htail_in. exact Htail_in.
    + apply IH; assumption.
Qed.

Lemma assoc_lookup_none : forall entries key,
  ~ In key (map fst entries) -> assoc_lookup entries key = None.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key Hnotin; simpl in *; [reflexivity|].
  destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent].
  - exfalso. apply Hnotin. left. exact Hsame.
  - apply IH. intro Hin. apply Hnotin. right. exact Hin.
Qed.

Lemma assoc_lookup_key_present : forall entries key,
  In key (map fst entries) -> exists value, assoc_lookup entries key = Some value.
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key Hin; simpl in *; [contradiction|].
  destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent];
    [exists entry_value; reflexivity|].
  destruct Hin as [Hin|Hin]; [contradiction|apply IH; exact Hin].
Qed.

Lemma assoc_lookup_permutation : forall left right key,
  NoDup (map fst left) -> Permutation left right ->
  assoc_lookup left key = assoc_lookup right key.
Proof.
  intros left right key Hunique Hpermutation.
  assert (Hright_unique : NoDup (map fst right)).
  { eapply Permutation_NoDup; [apply Permutation_map; exact Hpermutation|].
    exact Hunique. }
  destruct (assoc_lookup left key) as [value|] eqn:Hleft.
  - symmetry. apply assoc_lookup_in; [exact Hright_unique|].
    eapply Permutation_in; [exact Hpermutation|].
    apply assoc_lookup_some_in. exact Hleft.
  - destruct (assoc_lookup right key) as [value|] eqn:Hright; [|reflexivity].
    exfalso.
    assert (Hkey : In key (map fst left)).
    { eapply Permutation_in.
      - apply Permutation_sym. apply Permutation_map. exact Hpermutation.
      - eapply assoc_lookup_some_key. exact Hright. }
    destruct (assoc_lookup_key_present left key Hkey) as [found Hfound].
    congruence.
Qed.

Lemma assoc_set_key_cases : forall entries key value other,
  In other (map fst (assoc_set entries key value)) ->
  other = key \/ In other (map fst entries).
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value other Hin; simpl in *.
  - destruct Hin as [Hin|[]]. left. symmetry. exact Hin.
  - destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent]; simpl in Hin.
    + destruct Hin as [Hin|Hin]; [left; symmetry; exact Hin|right; right; exact Hin].
    + destruct Hin as [Hin|Hin]; [right; left; exact Hin|].
      destruct (IH key value other Hin) as [Hkey|Htail];
        [left; exact Hkey|right; right; exact Htail].
Qed.

Lemma assoc_set_preserves_unique_keys : forall entries key value,
  NoDup (map fst entries) -> NoDup (map fst (assoc_set entries key value)).
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key value Hunique; simpl in *.
  - constructor; [intros []|constructor].
  - apply NoDup_cons_iff in Hunique. destruct Hunique as [Hnotin Htail].
    destruct (key_eq_dec entry_key key) as [Hsame|Hdifferent]; simpl.
    + subst entry_key. constructor; assumption.
    + constructor; [|apply IH; exact Htail].
      intro Hin.
      destruct (assoc_set_key_cases tail key value entry_key Hin)
        as [Hkey|Htail_in]; contradiction.
Qed.

Lemma assoc_remove_keys : forall entries key other,
  In other (map fst (assoc_remove entries key)) -> In other (map fst entries).
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key other Hin; simpl in *; [exact Hin|].
  destruct (key_eq_dec entry_key key); simpl in Hin.
  - right. eapply IH. exact Hin.
  - destruct Hin as [Hin|Hin]; [left; exact Hin|right; eapply IH; exact Hin].
Qed.

Lemma assoc_remove_preserves_unique_keys : forall entries key,
  NoDup (map fst entries) -> NoDup (map fst (assoc_remove entries key)).
Proof.
  induction entries as [|[entry_key entry_value] tail IH];
    intros key Hunique; simpl in *; [constructor|].
  apply NoDup_cons_iff in Hunique. destruct Hunique as [Hnotin Htail].
  destruct (key_eq_dec entry_key key); simpl.
  - apply IH. exact Htail.
  - constructor; [|apply IH; exact Htail].
    intro Hin. apply Hnotin. eapply assoc_remove_keys. exact Hin.
Qed.

End AssociationLists.

Lemma in_singleton_iff : forall (A : Type) (value item : A),
  In item [value] <-> item = value.
Proof.
  intros A value item. simpl. split.
  - intros [Hsame|[]]. symmetry. exact Hsame.
  - intro Hsame. left. symmetry. exact Hsame.
Qed.

Lemma nodup_map_injective : forall (A B : Type) (f : A -> B) (values : list A),
  (forall left right, In left values -> In right values ->
     f left = f right -> left = right) ->
  NoDup values -> NoDup (map f values).
Proof.
  intros A B f values Hinjective Hvalues. revert Hinjective.
  induction Hvalues as [|head tail Hnotin Htail IH]; intro Hinjective;
    simpl; constructor.
  - rewrite in_map_iff. intros [other [Hsame Hin]].
    assert (Hother : other = head).
    { apply Hinjective; [right; exact Hin|left; reflexivity|exact Hsame]. }
    subst other. contradiction.
  - apply IH. intros left right Hleft Hright Hsame.
    apply Hinjective; [right; exact Hleft|right; exact Hright|exact Hsame].
Qed.

Lemma nodup_flat_map : forall (A B : Type) (f : A -> list B) (values : list A),
  NoDup values ->
  (forall value, In value values -> NoDup (f value)) ->
  (forall left right item, In left values -> In right values ->
     In item (f left) -> In item (f right) -> left = right) ->
  NoDup (flat_map f values).
Proof.
  intros A B f values Hvalues.
  induction Hvalues as [|head tail Hnotin Htail IH];
    intros Hinner Hdisjoint; simpl; [constructor|].
  apply NoDup_app.
  - apply Hinner. left. reflexivity.
  - apply IH.
    + intros value Hin. apply Hinner. right. exact Hin.
    + intros left right item Hleft Hright Hitem_left Hitem_right.
      apply (Hdisjoint left right item).
      * right. exact Hleft.
      * right. exact Hright.
      * exact Hitem_left.
      * exact Hitem_right.
  - intros item Hitem_head Hitem_tail.
    apply in_flat_map in Hitem_tail.
    destruct Hitem_tail as [other [Hother Hitem_other]].
    assert (Hsame : head = other).
    { apply (Hdisjoint head other item).
      - left. reflexivity.
      - right. exact Hother.
      - exact Hitem_head.
      - exact Hitem_other. }
    subst other. contradiction.
Qed.

Record SemanticRoots : Type := SemanticRootsState {
  root_data : list (Path * list Path);
  root_continuations : list (GroupKey * list ContinuationRecord);
  root_joins : list (Path * list GroupKey)
}.

Definition semantic_roots_eq_dec (left right : SemanticRoots)
    : {left = right} + {left <> right}.
Proof.
  destruct left as [left_data left_continuations left_joins].
  destruct right as [right_data right_continuations right_joins].
  destruct (list_eq_dec (product_eq_dec path_eq_dec (list_eq_dec path_eq_dec))
    left_data right_data) as [Hdata|Hdata];
    [subst right_data|right; congruence].
  destruct (list_eq_dec
    (product_eq_dec group_eq_dec (list_eq_dec continuation_record_eq_dec))
    left_continuations right_continuations) as [Hcontinuations|Hcontinuations];
    [subst right_continuations|right; congruence].
  destruct (list_eq_dec (product_eq_dec path_eq_dec (list_eq_dec group_eq_dec))
    left_joins right_joins) as [Hjoins|Hjoins];
    [subst right_joins; left; reflexivity|right; congruence].
Defined.

Definition InstalledSlots : Type := list (GroupKey * ContinuationRecord).

Definition group_records (roots : SemanticRoots) (group : GroupKey)
    : list ContinuationRecord :=
  assoc_get group_eq_dec [] (root_continuations roots) group.

Definition group_installed (slots : InstalledSlots) (group : GroupKey)
    : option ContinuationRecord :=
  assoc_lookup group_eq_dec slots group.

Definition installed_target (group : GroupKey) (record : ContinuationRecord)
    : ReceiveTarget :=
  ReceiveTargetState InstalledKind group record.

Definition resting_target (group : GroupKey) (record : ContinuationRecord)
    : ReceiveTarget :=
  ReceiveTargetState RestingKind group record.

Definition installed_targets (slots : InstalledSlots) (group : GroupKey)
    : list ReceiveTarget :=
  match group_installed slots group with
  | Some record => [installed_target group record]
  | None => []
  end.

Definition group_targets (roots : SemanticRoots) (slots : InstalledSlots)
    (group : GroupKey) : list ReceiveTarget :=
  installed_targets slots group ++
  map (resting_target group) (group_records roots group).

Definition has_targets (roots : SemanticRoots) (slots : InstalledSlots)
    (group : GroupKey) : bool :=
  match group_targets roots slots group with
  | [] => false
  | _ :: _ => true
  end.

Definition receive_support (roots : SemanticRoots) (slots : InstalledSlots)
    : list GroupKey :=
  filter (has_targets roots slots)
    (nodup group_eq_dec (map fst (root_continuations roots) ++ map fst slots)).

Definition channel_groups (roots : SemanticRoots) (slots : InstalledSlots)
    (channel : Path) : list (GroupKey * list nat) :=
  flat_map (fun group =>
    match positions group channel with
    | [] => []
    | position :: rest => [(group, position :: rest)]
    end) (receive_support roots slots).

Definition derive_occurrences (roots : SemanticRoots) (slots : InstalledSlots)
    (channel : Path) : list Occurrence :=
  flat_map (fun entry =>
    map (OccurrenceState channel (fst entry) (snd entry))
      (group_targets roots slots (fst entry)))
    (channel_groups roots slots channel).

Definition occurrence_shape (occurrence : Occurrence) : Prop :=
  In (occurrence_channel occurrence) (occurrence_group occurrence) /\
  occurrence_positions occurrence =
    positions (occurrence_group occurrence) (occurrence_channel occurrence).

Definition target_occurrences (group : GroupKey) (target : ReceiveTarget)
    : list Occurrence :=
  map (fun channel =>
    OccurrenceState channel group (positions group channel) target)
    (nodup path_eq_dec group).

Definition group_entries (roots : SemanticRoots) (slots : InstalledSlots)
    (group : GroupKey) : list Occurrence :=
  flat_map (target_occurrences group) (group_targets roots slots group).

Definition scoped_entries_of (roots : SemanticRoots) (slots : InstalledSlots)
    (scope : list GroupKey) : list Occurrence :=
  flat_map (group_entries roots slots) scope.

Definition same_members {A : Type} (left right : list A) : Prop :=
  forall item, In item left <-> In item right.

Definition roots_valid (roots : SemanticRoots) : Prop :=
  NoDup (map fst (root_continuations roots)) /\
  forall group, NoDup (map continuation_identity (group_records roots group)).

Definition slots_valid (slots : InstalledSlots) : Prop :=
  NoDup (map fst slots).

Lemma positions_nonempty_iff : forall group channel,
  positions group channel <> [] <-> In channel group.
Proof.
  intros group channel. split.
  - intro Hnonempty.
    destruct (positions group channel) as [|position rest] eqn:Hpositions;
      [contradiction|].
    assert (Hin : In position (positions group channel)).
    { rewrite Hpositions. left. reflexivity. }
    apply positions_exact in Hin. eapply nth_error_In. exact Hin.
  - intros Hin Hempty.
    apply In_nth_error in Hin. destruct Hin as [position Hnth].
    apply positions_exact in Hnth. rewrite Hempty in Hnth. exact Hnth.
Qed.

Lemma group_targets_spec : forall roots slots group target,
  In target (group_targets roots slots group) <->
  (exists record, group_installed slots group = Some record /\
     target = installed_target group record) \/
  (exists record, In record (group_records roots group) /\
     target = resting_target group record).
Proof.
  intros roots slots group target. unfold group_targets, installed_targets.
  rewrite in_app_iff, in_map_iff.
  destruct (group_installed slots group) as [installed|]; simpl.
  - split.
    + intros [[Hsame|[]]|[record [Hsame Hin]]].
      * left. exists installed. split; [reflexivity|symmetry; exact Hsame].
      * right. exists record. split; [exact Hin|symmetry; exact Hsame].
    + intros [[record [Hrecord Hsame]]|[record [Hin Hsame]]].
      * left. left. injection Hrecord as Hrecord. subst. reflexivity.
      * right. exists record. split; [symmetry; exact Hsame|exact Hin].
  - split.
    + intros [[]|[record [Hsame Hin]]].
      right. exists record. split; [exact Hin|symmetry; exact Hsame].
    + intros [[record [Hrecord _]]|[record [Hin Hsame]]]; [discriminate|].
      right. exists record. split; [symmetry; exact Hsame|exact Hin].
Qed.

Lemma target_group_is_group : forall roots slots group target,
  In target (group_targets roots slots group) ->
  receive_target_group target = group.
Proof.
  intros roots slots group target Hin.
  apply group_targets_spec in Hin.
  destruct Hin as [[record [_ Hsame]]|[record [_ Hsame]]];
    subst target; reflexivity.
Qed.

Lemma receive_support_spec : forall roots slots group,
  In group (receive_support roots slots) <->
  group_targets roots slots group <> [].
Proof.
  intros roots slots group. unfold receive_support.
  rewrite filter_In, nodup_In, in_app_iff. unfold has_targets.
  split.
  - intros [_ Htargets] Hempty. rewrite Hempty in Htargets.
    simpl in Htargets. discriminate.
  - intro Hnonempty. split.
    + destruct (group_installed slots group) as [record|] eqn:Hinstalled.
      * right. eapply assoc_lookup_some_key. exact Hinstalled.
      * left.
        destruct (assoc_lookup group_eq_dec (root_continuations roots) group)
          as [records|] eqn:Hlookup.
        -- eapply assoc_lookup_some_key. exact Hlookup.
        -- exfalso. apply Hnonempty.
           unfold group_targets, installed_targets, group_records, assoc_get.
           rewrite Hinstalled, Hlookup. reflexivity.
    + destruct (group_targets roots slots group) as [|target rest]
        eqn:Htargets; [contradiction|reflexivity].
Qed.

Lemma channel_groups_spec : forall roots slots channel group found,
  In (group, found) (channel_groups roots slots channel) <->
  In group (receive_support roots slots) /\
  found = positions group channel /\ found <> [].
Proof.
  intros roots slots channel group found. unfold channel_groups.
  rewrite in_flat_map. split.
  - intros [candidate [Hsupport Hin]]. revert Hin.
    destruct (positions candidate channel) as [|position rest] eqn:Hpositions;
      simpl; [intros []|].
    intros [Hsame|[]]. injection Hsame as Hgroup Hfound. subst.
    split; [exact Hsupport|]. split; [symmetry; exact Hpositions|discriminate].
  - intros [Hsupport [Hfound Hnonempty]]. exists group.
    split; [exact Hsupport|].
    rewrite <- Hfound.
    destruct found as [|position rest];
      [exfalso; apply Hnonempty; reflexivity|].
    simpl. left. reflexivity.
Qed.

Lemma derive_occurrences_spec : forall roots slots channel occurrence,
  In occurrence (derive_occurrences roots slots channel) <->
  occurrence_channel occurrence = channel /\
  In (occurrence_target occurrence)
    (group_targets roots slots (occurrence_group occurrence)) /\
  occurrence_shape occurrence.
Proof.
  intros roots slots channel occurrence.
  unfold derive_occurrences, occurrence_shape.
  rewrite in_flat_map. split.
  - intros [[group found] [Hentry Hin]]. simpl in Hin.
    apply channel_groups_spec in Hentry.
    destruct Hentry as [_ [Hfound Hnonempty]].
    apply in_map_iff in Hin. destruct Hin as [target [Hsame Htarget]].
    subst occurrence. simpl.
    split; [reflexivity|]. split; [exact Htarget|].
    split; [|exact Hfound].
    apply positions_nonempty_iff. rewrite <- Hfound. exact Hnonempty.
  - destruct occurrence as [actual group found target]. simpl.
    intros [Hchannel [Htarget [Hin Hfound]]]. subst actual.
    exists (group, found). simpl. split.
    + apply channel_groups_spec. split.
      * apply receive_support_spec. intro Hempty.
        rewrite Hempty in Htarget. exact Htarget.
      * split; [exact Hfound|].
        rewrite Hfound. apply positions_nonempty_iff. exact Hin.
    + apply in_map_iff. exists target. split; [reflexivity|exact Htarget].
Qed.

Lemma target_occurrences_spec : forall group target occurrence,
  In occurrence (target_occurrences group target) <->
  occurrence_target occurrence = target /\
  occurrence_group occurrence = group /\
  occurrence_shape occurrence.
Proof.
  intros group target occurrence. unfold target_occurrences, occurrence_shape.
  rewrite in_map_iff. split.
  - intros [channel [Hsame Hin]]. subst occurrence. simpl.
    rewrite nodup_In in Hin.
    split; [reflexivity|]. split; [reflexivity|]. split; [exact Hin|reflexivity].
  - destruct occurrence as [channel actual found actual_target]. simpl.
    intros [Htarget [Hgroup [Hin Hfound]]].
    subst actual_target actual found.
    exists channel. split; [reflexivity|]. apply nodup_In. exact Hin.
Qed.

Lemma group_entries_spec : forall roots slots group occurrence,
  In occurrence (group_entries roots slots group) <->
  occurrence_group occurrence = group /\
  In (occurrence_target occurrence) (group_targets roots slots group) /\
  occurrence_shape occurrence.
Proof.
  intros roots slots group occurrence. unfold group_entries.
  rewrite in_flat_map. split.
  - intros [target [Htarget Hin]]. apply target_occurrences_spec in Hin.
    destruct Hin as [Hsame [Hgroup Hshape]]. rewrite Hsame. tauto.
  - intros [Hgroup [Htarget Hshape]]. exists (occurrence_target occurrence).
    split; [exact Htarget|]. apply target_occurrences_spec.
    split; [reflexivity|tauto].
Qed.

Lemma scoped_entries_spec : forall roots slots scope occurrence,
  In occurrence (scoped_entries_of roots slots scope) <->
  In (occurrence_group occurrence) scope /\
  In (occurrence_target occurrence)
    (group_targets roots slots (occurrence_group occurrence)) /\
  occurrence_shape occurrence.
Proof.
  intros roots slots scope occurrence. unfold scoped_entries_of.
  rewrite in_flat_map. split.
  - intros [group [Hscope Hin]]. apply group_entries_spec in Hin.
    destruct Hin as [Hgroup [Htarget Hshape]]. subst group. tauto.
  - intros [Hscope [Htarget Hshape]]. exists (occurrence_group occurrence).
    split; [exact Hscope|]. apply group_entries_spec.
    split; [reflexivity|tauto].
Qed.

Lemma group_targets_unique : forall roots slots group,
  NoDup (group_records roots group) -> NoDup (group_targets roots slots group).
Proof.
  intros roots slots group Hrecords. unfold group_targets, installed_targets.
  apply NoDup_app.
  - destruct (group_installed slots group) as [record|].
    + constructor; [intros []|constructor].
    + constructor.
  - apply nodup_map_injective; [|exact Hrecords].
    intros left right _ _ Hsame. unfold resting_target in Hsame.
    injection Hsame; intros; subst; reflexivity.
  - intros target Hinstalled Hresting. revert Hinstalled.
    destruct (group_installed slots group) as [record|]; simpl; [|intros []].
    intros [Hsame|[]]. subst target.
    apply in_map_iff in Hresting. destruct Hresting as [other [Hother _]].
    unfold resting_target, installed_target in Hother. discriminate.
Qed.

Lemma channel_groups_unique : forall roots slots channel,
  NoDup (channel_groups roots slots channel).
Proof.
  intros roots slots channel. unfold channel_groups.
  apply nodup_flat_map.
  - apply NoDup_filter. apply NoDup_nodup.
  - intros group _. destruct (positions group channel) as [|position rest].
    + constructor.
    + constructor; [intros []|constructor].
  - intros left right item _ _.
    destruct (positions left channel) as [|left_position left_rest];
      [intros []|].
    destruct (positions right channel) as [|right_position right_rest];
      [intros _ []|].
    intros Hleft Hright.
    apply in_singleton_iff in Hleft. apply in_singleton_iff in Hright.
    subst item. injection Hright; intros; subst; reflexivity.
Qed.

Theorem derived_occurrence_is_sound : forall roots slots channel occurrence,
  In occurrence (derive_occurrences roots slots channel) ->
  occurrence_channel occurrence = channel /\
  In (occurrence_group occurrence) (receive_support roots slots) /\
  occurrence_positions occurrence =
    positions (occurrence_group occurrence) channel /\
  positions (occurrence_group occurrence) channel <> [] /\
  In (occurrence_target occurrence)
    (group_targets roots slots (occurrence_group occurrence)).
Proof.
  intros roots slots channel occurrence Hin.
  apply derive_occurrences_spec in Hin.
  destruct Hin as [Hchannel [Htarget [Hmember Hfound]]].
  rewrite Hchannel in Hmember, Hfound.
  split; [exact Hchannel|]. split.
  - apply receive_support_spec. intro Hempty.
    rewrite Hempty in Htarget. exact Htarget.
  - split; [exact Hfound|]. split; [|exact Htarget].
    apply positions_nonempty_iff. exact Hmember.
Qed.

Theorem derived_occurrence_is_complete :
    forall roots slots group position channel target,
  In group (receive_support roots slots) ->
  nth_error group position = Some channel ->
  In target (group_targets roots slots group) ->
  In (OccurrenceState channel group (positions group channel) target)
    (derive_occurrences roots slots channel) /\
  In position (positions group channel).
Proof.
  intros roots slots group position channel target Hsupport Hnth Htarget.
  split.
  - apply derive_occurrences_spec. unfold occurrence_shape. simpl.
    split; [reflexivity|]. split; [exact Htarget|].
    split; [|reflexivity]. eapply nth_error_In. exact Hnth.
  - apply positions_exact. exact Hnth.
Qed.

Theorem derived_positions_factor_through_group : forall roots slots channel,
  derive_occurrences roots slots channel =
    flat_map (fun entry =>
      map (OccurrenceState channel (fst entry) (snd entry))
        (group_targets roots slots (fst entry)))
      (channel_groups roots slots channel) /\
  (forall group found,
     In (group, found) (channel_groups roots slots channel) ->
     found = positions group channel /\ found <> []) /\
  (forall occurrence,
     In occurrence (derive_occurrences roots slots channel) ->
     occurrence_positions occurrence =
       positions (occurrence_group occurrence) channel).
Proof.
  intros roots slots channel. split; [reflexivity|]. split.
  - intros group found Hin. apply channel_groups_spec in Hin. tauto.
  - intros occurrence Hin. apply derived_occurrence_is_sound in Hin. tauto.
Qed.

Theorem repeated_channel_is_one_occurrence_with_all_positions :
    forall roots slots channel,
  roots_valid roots ->
  NoDup (derive_occurrences roots slots channel) /\
  (forall left right,
     In left (derive_occurrences roots slots channel) ->
     In right (derive_occurrences roots slots channel) ->
     occurrence_group left = occurrence_group right ->
     occurrence_target left = occurrence_target right -> left = right) /\
  (forall occurrence position,
     In occurrence (derive_occurrences roots slots channel) ->
     nth_error (occurrence_group occurrence) position = Some channel ->
     In position (occurrence_positions occurrence)).
Proof.
  intros roots slots channel [_ Hidentities]. split; [|split].
  - unfold derive_occurrences. apply nodup_flat_map.
    + apply channel_groups_unique.
    + intros [group found] _. simpl.
      apply nodup_map_injective.
      * intros left right _ _ Hsame.
        injection Hsame; intros; subst; reflexivity.
      * apply group_targets_unique. eapply NoDup_map_inv. apply Hidentities.
    + intros [left_group left_found] [right_group right_found] item _ _
        Hleft Hright.
      simpl in Hleft, Hright.
      apply in_map_iff in Hleft. destruct Hleft as [left_target [Hleft _]].
      apply in_map_iff in Hright. destruct Hright as [right_target [Hright _]].
      subst item. injection Hright; intros; subst; reflexivity.
  - intros left right Hleft Hright Hgroup Htarget.
    apply derive_occurrences_spec in Hleft.
    apply derive_occurrences_spec in Hright.
    destruct Hleft as [Hleft_channel [_ [_ Hleft_found]]].
    destruct Hright as [Hright_channel [_ [_ Hright_found]]].
    destruct left as [left_channel left_group left_found left_target].
    destruct right as [right_channel right_group right_found right_target].
    simpl in *. subst. reflexivity.
  - intros occurrence position Hin Hnth.
    apply derive_occurrences_spec in Hin.
    destruct Hin as [Hchannel [_ [_ Hfound]]].
    rewrite Hfound, Hchannel. apply positions_exact. exact Hnth.
Qed.

Lemma channel_at_is_head : forall head tail channel,
  channel_at_is (head :: tail) channel 0 =
  if path_eq_dec head channel then true else false.
Proof. reflexivity. Qed.

Lemma positions_cons : forall head tail channel,
  positions (head :: tail) channel =
  (if path_eq_dec head channel then [0] else []) ++
  map S (positions tail channel).
Proof.
  intros head tail channel. unfold positions. simpl.
  rewrite <- seq_shift, filter_map_swap, channel_at_is_head.
  destruct (path_eq_dec head channel); reflexivity.
Qed.

Lemma positions_length_is_count : forall group channel,
  length (positions group channel) = count_occ path_eq_dec group channel.
Proof.
  induction group as [|head tail IH]; intro channel; [reflexivity|].
  rewrite positions_cons, length_app, length_map, IH. cbn [count_occ].
  destruct (path_eq_dec head channel); reflexivity.
Qed.

Theorem occurrence_capacity_is_position_multiplicity :
    forall roots slots channel occurrence,
  In occurrence (derive_occurrences roots slots channel) ->
  length (occurrence_positions occurrence) =
    count_occ path_eq_dec (occurrence_group occurrence) channel.
Proof.
  intros roots slots channel occurrence Hin.
  apply derived_occurrence_is_sound in Hin.
  destruct Hin as [_ [_ [Hfound _]]].
  rewrite Hfound. apply positions_length_is_count.
Qed.

Theorem prefix_aliased_channels_have_disjoint_occurrences :
  (forall group left right position,
     left <> right -> In position (positions group left) ->
     ~ In position (positions group right)) /\
  (forall roots slots left right left_occurrence right_occurrence,
     left <> right ->
     In left_occurrence (derive_occurrences roots slots left) ->
     In right_occurrence (derive_occurrences roots slots right) ->
     occurrence_group left_occurrence = occurrence_group right_occurrence ->
     forall position, In position (occurrence_positions left_occurrence) ->
     ~ In position (occurrence_positions right_occurrence)) /\
  (forall channels channel position,
     Forall (fun bytes => RhoPathV1Datum.bytes_valid bytes = true) channels ->
     RhoPathV1Datum.bytes_valid channel = true ->
     (In position (positions (map RhoPathV1Datum.encode_frame channels)
        (RhoPathV1Datum.encode_frame channel)) <->
      In position (positions channels channel))).
Proof.
  assert (Hdisjoint : forall group left right position,
     left <> right -> In position (positions group left) ->
     ~ In position (positions group right)).
  { intros group left right position Hdifferent Hleft Hright.
    apply positions_exact in Hleft. apply positions_exact in Hright.
    rewrite Hleft in Hright. injection Hright as Hsame. contradiction. }
  split; [exact Hdisjoint|]. split.
  - intros roots slots left right left_occurrence right_occurrence Hdifferent
      Hleft Hright Hgroup position Hposition.
    apply derived_occurrence_is_sound in Hleft.
    apply derived_occurrence_is_sound in Hright.
    destruct Hleft as [_ [_ [Hleft_found _]]].
    destruct Hright as [_ [_ [Hright_found _]]].
    rewrite Hleft_found in Hposition. rewrite Hright_found, <- Hgroup.
    exact (Hdisjoint _ left right position Hdifferent Hposition).
  - intros channels channel position Hchannels Hchannel.
    exact (@escaped_channel_occurrences_are_exact
      channels channel position Hchannels Hchannel).
Qed.

Theorem derivation_ignores_data_and_runtime_order :
  (forall roots data slots channel,
     derive_occurrences
       (SemanticRootsState data (root_continuations roots) (root_joins roots))
       slots channel =
     derive_occurrences roots slots channel) /\
  (forall left right slots channel,
     root_continuations left = root_continuations right ->
     derive_occurrences left slots channel =
       derive_occurrences right slots channel).
Proof.
  split.
  - intros [data_before continuations joins] data slots channel. reflexivity.
  - intros [left_data continuations left_joins]
      [right_data right_continuations right_joins] slots channel
      Hcontinuations.
    simpl in Hcontinuations. subst right_continuations. reflexivity.
Qed.

Definition resting_joins_of (roots : SemanticRoots) (channel : Path)
    : list GroupKey :=
  assoc_get path_eq_dec [] (root_joins roots) channel.

Definition installed_joins_of (slots : InstalledSlots) (channel : Path)
    : list GroupKey :=
  filter (fun group => if in_dec path_eq_dec channel group then true else false)
    (map fst slots).

Definition joins_of (roots : SemanticRoots) (slots : InstalledSlots)
    (channel : Path) : list GroupKey :=
  resting_joins_of roots channel ++ installed_joins_of slots channel.

Definition joins_cover (roots : SemanticRoots) : Prop :=
  forall channel group,
    In group (resting_joins_of roots channel) <->
    In channel group /\ group_records roots group <> [].

Theorem join_root_projection_is_exact : forall roots slots channel group,
  joins_cover roots ->
  (In group (map fst (channel_groups roots slots channel)) <->
   In group (joins_of roots slots channel) /\
   group_targets roots slots group <> []).
Proof.
  intros roots slots channel group Hcover.
  rewrite in_map_iff. unfold joins_of. rewrite in_app_iff.
  unfold installed_joins_of. rewrite filter_In.
  split.
  - intros [[actual found] [Hactual Hin]]. simpl in Hactual. subst actual.
    apply channel_groups_spec in Hin.
    destruct Hin as [Hsupport [Hfound Hnonempty]].
    apply receive_support_spec in Hsupport.
    assert (Hmember : In channel group).
    { apply positions_nonempty_iff. rewrite <- Hfound. exact Hnonempty. }
    split; [|exact Hsupport].
    destruct (group_records roots group) as [|record rest] eqn:Hrecords.
    + right. split.
      * unfold group_targets, installed_targets in Hsupport.
        rewrite Hrecords in Hsupport. revert Hsupport.
        destruct (group_installed slots group) as [installed|] eqn:Hinstalled;
          intro Hsupport.
        -- eapply assoc_lookup_some_key. exact Hinstalled.
        -- exfalso. apply Hsupport. reflexivity.
      * destruct (in_dec path_eq_dec channel group); [reflexivity|contradiction].
    + left. apply Hcover. split; [exact Hmember|].
      rewrite Hrecords. discriminate.
  - intros [Hjoin Htargets].
    assert (Hmember : In channel group).
    { destruct Hjoin as [Hresting|[_ Hdecision]].
      - apply Hcover in Hresting. tauto.
      - revert Hdecision.
        destruct (in_dec path_eq_dec channel group) as [Hin|Hnotin];
          [intros _; exact Hin|intro Hdecision; discriminate]. }
    exists (group, positions group channel). split; [reflexivity|].
    apply channel_groups_spec.
    split; [apply receive_support_spec; exact Htargets|].
    split; [reflexivity|]. apply positions_nonempty_iff. exact Hmember.
Qed.

Lemma in_remove_group_iff : forall groups group other,
  In other (remove group_eq_dec group groups) <-> In other groups /\ other <> group.
Proof.
  intros groups group other. split.
  - intro Hin. apply in_remove in Hin. exact Hin.
  - intros [Hin Hother]. apply in_in_remove; [exact Hother|exact Hin].
Qed.

Fixpoint join_add_channels (joins : list (Path * list GroupKey))
    (group : GroupKey) (channels : list Path) : list (Path * list GroupKey) :=
  match channels with
  | [] => joins
  | channel :: rest =>
      join_add_channels
        (if in_dec group_eq_dec group (assoc_get path_eq_dec [] joins channel)
         then joins
         else assoc_set path_eq_dec joins channel
           (assoc_get path_eq_dec [] joins channel ++ [group]))
        group rest
  end.

Fixpoint join_remove_channels (joins : list (Path * list GroupKey))
    (group : GroupKey) (channels : list Path) : list (Path * list GroupKey) :=
  match channels with
  | [] => joins
  | channel :: rest =>
      join_remove_channels
        (assoc_set path_eq_dec joins channel
          (remove group_eq_dec group (assoc_get path_eq_dec [] joins channel)))
        group rest
  end.

Lemma join_add_channels_spec : forall channels joins group channel other,
  In other (assoc_get path_eq_dec [] (join_add_channels joins group channels)
    channel) <->
  In other (assoc_get path_eq_dec [] joins channel) \/
  (other = group /\ In channel channels).
Proof.
  induction channels as [|head rest IH]; intros joins group channel other;
    cbn [join_add_channels In].
  - tauto.
  - rewrite IH.
    destruct (in_dec group_eq_dec group (assoc_get path_eq_dec [] joins head))
      as [Hpresent|Habsent]; cbv beta iota.
    + split.
      * intros [Hin|[Hother Hrest]];
          [left; exact Hin|right; split; [exact Hother|right; exact Hrest]].
      * intros [Hin|[Hother [Hhead|Hrest]]].
        -- left. exact Hin.
        -- left. subst other head. exact Hpresent.
        -- right. split; [exact Hother|exact Hrest].
    + destruct (path_eq_dec head channel) as [Hsame|Hdifferent].
      * subst head. unfold assoc_get at 1. rewrite assoc_lookup_set_same.
        rewrite in_app_iff, in_singleton_iff. tauto.
      * unfold assoc_get at 1.
        rewrite assoc_lookup_set_other
          by (intro Hchannel; apply Hdifferent; symmetry; exact Hchannel).
        fold (assoc_get path_eq_dec [] joins channel). split.
        -- intros [Hin|[Hother Hrest]];
             [left; exact Hin|right; split; [exact Hother|right; exact Hrest]].
        -- intros [Hin|[Hother [Hhead|Hrest]]];
             [left; exact Hin|contradiction|right; split; assumption].
Qed.

Lemma join_remove_channels_spec : forall channels joins group channel other,
  In other (assoc_get path_eq_dec [] (join_remove_channels joins group channels)
    channel) <->
  In other (assoc_get path_eq_dec [] joins channel) /\
  ~ (other = group /\ In channel channels).
Proof.
  induction channels as [|head rest IH]; intros joins group channel other;
    cbn [join_remove_channels In].
  - tauto.
  - rewrite IH. destruct (path_eq_dec head channel) as [Hsame|Hdifferent].
    + subst head. unfold assoc_get at 1. rewrite assoc_lookup_set_same.
      rewrite in_remove_group_iff. tauto.
    + unfold assoc_get at 1.
      rewrite assoc_lookup_set_other
        by (intro Hchannel; apply Hdifferent; symmetry; exact Hchannel).
      fold (assoc_get path_eq_dec [] joins channel). tauto.
Qed.

Fixpoint remove_first_record (record : ContinuationRecord)
    (records : list ContinuationRecord) : list ContinuationRecord :=
  match records with
  | [] => []
  | head :: tail =>
      if continuation_record_eq_dec head record then tail
      else head :: remove_first_record record tail
  end.

Lemma remove_first_record_subset : forall record records other,
  In other (remove_first_record record records) -> In other records.
Proof.
  intros record records other. induction records as [|head tail IH];
    cbn [remove_first_record]; [intros []|].
  destruct (continuation_record_eq_dec head record); cbv beta iota.
  - intro Hin. right. exact Hin.
  - intros [Hhead|Htail]; [left; exact Hhead|right; apply IH; exact Htail].
Qed.

Lemma remove_first_record_spec : forall record records other,
  NoDup records ->
  (In other (remove_first_record record records) <->
   In other records /\ other <> record).
Proof.
  intros record records other Hunique.
  induction Hunique as [|head tail Hnotin Htail IH];
    cbn [remove_first_record]; [simpl; tauto|].
  destruct (continuation_record_eq_dec head record) as [Hsame|Hdifferent];
    cbv beta iota.
  - subst head. split.
    + intro Hin. split; [right; exact Hin|].
      intro Hother. subst other. contradiction.
    + intros [[Hhead|Hin] Hother]; [subst; contradiction|exact Hin].
  - simpl. rewrite IH. split.
    + intros [Hhead|[Hin Hother]].
      * subst other. split; [left; reflexivity|exact Hdifferent].
      * split; [right; exact Hin|exact Hother].
    + intros [[Hhead|Hin] Hother]; [left; exact Hhead|right; split; assumption].
Qed.

Lemma remove_first_record_preserves_identities : forall record records,
  NoDup (map continuation_identity records) ->
  NoDup (map continuation_identity (remove_first_record record records)).
Proof.
  intros record records. induction records as [|head tail IH];
    cbn [remove_first_record]; [intro; constructor|].
  intro Hunique. simpl in Hunique.
  apply NoDup_cons_iff in Hunique. destruct Hunique as [Hnotin Htail].
  destruct (continuation_record_eq_dec head record); cbv beta iota.
  - exact Htail.
  - simpl. constructor; [|apply IH; exact Htail].
    intro Hin. apply Hnotin. apply in_map_iff in Hin.
    destruct Hin as [other [Hidentity Hother]].
    rewrite <- Hidentity. apply in_map.
    eapply remove_first_record_subset. exact Hother.
Qed.

Lemma first_identity_none_is_absent : forall identity records,
  first_identity identity records = None ->
  ~ In identity (map continuation_identity records).
Proof.
  intros identity records. induction records as [|head tail IH];
    cbn [first_identity map In]; [intros _ []|].
  destruct (path_eq_dec (continuation_identity head) identity)
    as [Hsame|Hdifferent]; cbv beta iota; [discriminate|].
  intros Hnone [Hhead|Htail]; [contradiction|exact (IH Hnone Htail)].
Qed.

Record PersistedAccelerator : Type := PersistedAcceleratorState {
  persisted_lineage : SemanticRoots;
  persisted_scope : list GroupKey;
  persisted_entries : list Occurrence
}.

Record World : Type := WorldState {
  world_roots : SemanticRoots;
  world_installed : InstalledSlots;
  world_revision : Revision;
  world_issued : list EqualityEpoch.t;
  world_manifest : SemanticRoots;
  world_base_epoch : EqualityEpoch.t;
  world_group_epochs : list (GroupKey * EqualityEpoch.t)
}.

Inductive Operation : Type :=
| InsertResting (group : GroupKey) (record : ContinuationRecord)
| RemoveResting (group : GroupKey) (record : ContinuationRecord)
| Install (group : GroupKey) (record : ContinuationRecord)
| Uninstall (group : GroupKey)
| HistoryLoad (group : GroupKey)
| ResetState (target : SemanticRoots) (slots : InstalledSlots)
    (persisted : option PersistedAccelerator).

Inductive Effect : Type :=
| NoEffect
| Inserted (group : GroupKey) (record : ContinuationRecord)
| Removed (group : GroupKey) (record : ContinuationRecord)
    (remaining : list ContinuationRecord)
| Installed (group : GroupKey) (record : ContinuationRecord)
| Uninstalled (group : GroupKey)
| Loaded (group : GroupKey)
| Replaced (target : SemanticRoots) (slots : InstalledSlots)
    (persisted : option PersistedAccelerator).

Definition insert_roots (roots : SemanticRoots) (group : GroupKey)
    (record : ContinuationRecord) : SemanticRoots :=
  SemanticRootsState (root_data roots)
    (assoc_set group_eq_dec (root_continuations roots) group
      (group_records roots group ++ [record]))
    (join_add_channels (root_joins roots) group group).

Definition put_group_records
    (continuations : list (GroupKey * list ContinuationRecord))
    (group : GroupKey) (records : list ContinuationRecord)
    : list (GroupKey * list ContinuationRecord) :=
  match records with
  | [] => assoc_remove group_eq_dec continuations group
  | _ :: _ => assoc_set group_eq_dec continuations group records
  end.

Definition remove_roots (roots : SemanticRoots) (group : GroupKey)
    (remaining : list ContinuationRecord) : SemanticRoots :=
  SemanticRootsState (root_data roots)
    (put_group_records (root_continuations roots) group remaining)
    (match remaining with
     | [] => join_remove_channels (root_joins roots) group group
     | _ :: _ => root_joins roots
     end).

Definition fresh_token (w : World) : EqualityEpoch.t :=
  EqualityEpoch.fresh_from (world_issued w).

Definition bump_group (w : World) (roots : SemanticRoots)
    (slots : InstalledSlots) (group : GroupKey) (installed_changed : bool)
    : World :=
  WorldState roots slots
    (RevisionState (revision_generation (world_revision w)) (fresh_token w)
      (if installed_changed then fresh_token w
       else revision_installed (world_revision w)))
    (fresh_token w :: world_issued w)
    (world_manifest w)
    (world_base_epoch w)
    (assoc_set group_eq_dec (world_group_epochs w) group (fresh_token w)).

Definition reset_world (w : World) (target : SemanticRoots)
    (slots : InstalledSlots) : World :=
  WorldState target slots
    (RevisionState (fresh_token w) (fresh_token w) (fresh_token w))
    (fresh_token w :: world_issued w)
    target
    (fresh_token w)
    [].

Definition operation_effect (operation : Operation) (w : World) : Effect :=
  match operation with
  | InsertResting group record =>
      match first_identity (continuation_identity record)
          (group_records (world_roots w) group) with
      | Some _ => NoEffect
      | None => Inserted group record
      end
  | RemoveResting group record =>
      if in_dec continuation_record_eq_dec record
          (group_records (world_roots w) group)
      then Removed group record
        (remove_first_record record (group_records (world_roots w) group))
      else NoEffect
  | Install group record => Installed group record
  | Uninstall group =>
      match group_installed (world_installed w) group with
      | Some _ => Uninstalled group
      | None => NoEffect
      end
  | HistoryLoad group => Loaded group
  | ResetState target slots persisted => Replaced target slots persisted
  end.

Definition step_effect (effect : Effect) (w : World) : World :=
  match effect with
  | NoEffect => w
  | Loaded _ => w
  | Inserted group record =>
      bump_group w (insert_roots (world_roots w) group record)
        (world_installed w) group false
  | Removed group _ remaining =>
      bump_group w (remove_roots (world_roots w) group remaining)
        (world_installed w) group false
  | Installed group record =>
      bump_group w (world_roots w)
        (assoc_set group_eq_dec (world_installed w) group record) group true
  | Uninstalled group =>
      bump_group w (world_roots w)
        (assoc_remove group_eq_dec (world_installed w) group) group true
  | Replaced target slots _ => reset_world w target slots
  end.

Definition step_world (operation : Operation) (w : World) : World :=
  step_effect (operation_effect operation w) w.

Definition bumps (effect : Effect) : Prop :=
  match effect with
  | NoEffect | Loaded _ => False
  | _ => True
  end.

Definition revision_issued (issued : list EqualityEpoch.t)
    (revision : Revision) : Prop :=
  In (revision_generation revision) issued /\
  In (revision_store revision) issued /\
  In (revision_installed revision) issued.

Definition group_epoch (w : World) (group : GroupKey) : EqualityEpoch.t :=
  match assoc_lookup group_eq_dec (world_group_epochs w) group with
  | Some epoch => epoch
  | None => world_base_epoch w
  end.

Definition world_tokens_issued (w : World) : Prop :=
  revision_issued (world_issued w) (world_revision w) /\
  In (world_base_epoch w) (world_issued w) /\
  (forall group epoch,
     assoc_lookup group_eq_dec (world_group_epochs w) group = Some epoch ->
     In epoch (world_issued w)).

Definition world_valid (w : World) : Prop :=
  roots_valid (world_roots w) /\ slots_valid (world_installed w) /\
  world_tokens_issued w.

Definition targets_at (w : World) (group : GroupKey) : list ReceiveTarget :=
  group_targets (world_roots w) (world_installed w) group.

Definition derive_world (w : World) (channel : Path) : list Occurrence :=
  derive_occurrences (world_roots w) (world_installed w) channel.

Definition support_of (w : World) : list GroupKey :=
  receive_support (world_roots w) (world_installed w).

Definition scoped_entries (w : World) (scope : list GroupKey)
    : list Occurrence :=
  scoped_entries_of (world_roots w) (world_installed w) scope.

Lemma fresh_token_is_fresh : forall w, ~ In (fresh_token w) (world_issued w).
Proof.
  intros w Hin. unfold fresh_token in Hin.
  exact (@EqualityEpoch.fresh_from_rejects_every_prior (world_issued w)
    (EqualityEpoch.fresh_from (world_issued w)) Hin eq_refl).
Qed.

Lemma issued_token_is_not_fresh : forall w token,
  In token (world_issued w) -> token <> fresh_token w.
Proof.
  intros w token Hin Hsame. apply (fresh_token_is_fresh w).
  rewrite <- Hsame. exact Hin.
Qed.

Lemma step_effect_quiet : forall effect w, ~ bumps effect -> step_effect effect w = w.
Proof.
  intros effect w Hquiet. destruct effect; cbn [step_effect]; try reflexivity;
    exfalso; apply Hquiet; exact I.
Qed.

Lemma step_effect_fresh : forall effect w,
  bumps effect ->
  revision_store (world_revision (step_effect effect w)) = fresh_token w /\
  world_issued (step_effect effect w) = fresh_token w :: world_issued w.
Proof.
  intros effect w Hbumps.
  destruct effect; cbn [bumps] in Hbumps; try contradiction; split; reflexivity.
Qed.

Lemma step_effect_issued_grows : forall effect w token,
  In token (world_issued w) -> In token (world_issued (step_effect effect w)).
Proof.
  intros effect w token Hin. destruct effect; cbn [step_effect];
    try exact Hin; right; exact Hin.
Qed.

Lemma bump_group_tokens : forall w roots slots group installed_changed,
  world_tokens_issued w ->
  world_tokens_issued (bump_group w roots slots group installed_changed).
Proof.
  intros w roots slots group installed_changed
    [[Hgeneration [Hstore Hinstalled]] [Hbase Hepochs]].
  unfold world_tokens_issued, revision_issued, bump_group.
  cbn [world_issued world_revision world_base_epoch world_group_epochs
    revision_generation revision_store revision_installed].
  split; [split; [right; exact Hgeneration|split; [left; reflexivity|]]|
    split; [right; exact Hbase|]].
  - destruct installed_changed; [left; reflexivity|right; exact Hinstalled].
  - intros other epoch Hlookup.
    destruct (group_eq_dec other group) as [Hsame|Hdifferent].
    + subst other. rewrite assoc_lookup_set_same in Hlookup.
      injection Hlookup as Hepoch. subst epoch. left. reflexivity.
    + rewrite assoc_lookup_set_other in Hlookup by exact Hdifferent.
      right. exact (Hepochs other epoch Hlookup).
Qed.

Lemma reset_world_tokens : forall w target slots,
  world_tokens_issued (reset_world w target slots).
Proof.
  intros w target slots. unfold world_tokens_issued, revision_issued, reset_world.
  cbn [world_issued world_revision world_base_epoch world_group_epochs
    revision_generation revision_store revision_installed].
  split; [split; [left; reflexivity|split; left; reflexivity]|
    split; [left; reflexivity|]].
  intros group epoch Hlookup. discriminate.
Qed.

Lemma step_effect_tokens : forall effect w,
  world_tokens_issued w -> world_tokens_issued (step_effect effect w).
Proof.
  intros effect w Htokens.
  destruct effect; cbn [step_effect];
    first [exact Htokens|apply reset_world_tokens|
      apply bump_group_tokens; exact Htokens].
Qed.

Lemma group_epoch_issued : forall w group,
  world_tokens_issued w -> In (group_epoch w group) (world_issued w).
Proof.
  intros w group [_ [Hbase Hepochs]]. unfold group_epoch.
  destruct (assoc_lookup group_eq_dec (world_group_epochs w) group)
    as [epoch|] eqn:Hlookup; [exact (Hepochs group epoch Hlookup)|exact Hbase].
Qed.

Lemma insert_roots_records_same : forall roots group record,
  group_records (insert_roots roots group record) group =
  group_records roots group ++ [record].
Proof.
  intros roots group record. unfold group_records at 1.
  unfold insert_roots, assoc_get. cbn [root_continuations].
  rewrite assoc_lookup_set_same. reflexivity.
Qed.

Lemma insert_roots_records_other : forall roots group record other,
  other <> group ->
  group_records (insert_roots roots group record) other =
  group_records roots other.
Proof.
  intros roots group record other Hother.
  unfold group_records at 1. unfold insert_roots. cbn [root_continuations].
  unfold assoc_get at 1. rewrite assoc_lookup_set_other by exact Hother.
  reflexivity.
Qed.

Lemma remove_roots_records_same : forall roots group remaining,
  group_records (remove_roots roots group remaining) group = remaining.
Proof.
  intros roots group remaining.
  unfold group_records, remove_roots, put_group_records, assoc_get.
  cbn [root_continuations].
  destruct remaining as [|record rest]; cbv beta iota.
  - rewrite assoc_lookup_remove_same. reflexivity.
  - rewrite assoc_lookup_set_same. reflexivity.
Qed.

Lemma remove_roots_records_other : forall roots group remaining other,
  other <> group ->
  group_records (remove_roots roots group remaining) other =
  group_records roots other.
Proof.
  intros roots group remaining other Hother.
  unfold group_records, remove_roots, put_group_records, assoc_get.
  cbn [root_continuations].
  destruct remaining as [|record rest]; cbv beta iota.
  - rewrite assoc_lookup_remove_other by exact Hother. reflexivity.
  - rewrite assoc_lookup_set_other by exact Hother. reflexivity.
Qed.

Lemma targets_after_insert_same : forall roots slots group record,
  group_targets (insert_roots roots group record) slots group =
  group_targets roots slots group ++ [resting_target group record].
Proof.
  intros roots slots group record. unfold group_targets.
  rewrite insert_roots_records_same, map_app, app_assoc. reflexivity.
Qed.

Lemma targets_after_insert_other : forall roots slots group record other,
  other <> group ->
  group_targets (insert_roots roots group record) slots other =
  group_targets roots slots other.
Proof.
  intros roots slots group record other Hother. unfold group_targets.
  rewrite insert_roots_records_other by exact Hother. reflexivity.
Qed.

Lemma targets_after_remove_same : forall roots slots group remaining,
  group_targets (remove_roots roots group remaining) slots group =
  installed_targets slots group ++ map (resting_target group) remaining.
Proof.
  intros roots slots group remaining. unfold group_targets.
  rewrite remove_roots_records_same. reflexivity.
Qed.

Lemma targets_after_remove_other : forall roots slots group remaining other,
  other <> group ->
  group_targets (remove_roots roots group remaining) slots other =
  group_targets roots slots other.
Proof.
  intros roots slots group remaining other Hother. unfold group_targets.
  rewrite remove_roots_records_other by exact Hother. reflexivity.
Qed.

Lemma targets_after_install_same : forall roots slots group record,
  group_targets roots (assoc_set group_eq_dec slots group record) group =
  installed_target group record ::
    map (resting_target group) (group_records roots group).
Proof.
  intros roots slots group record.
  unfold group_targets, installed_targets, group_installed.
  rewrite assoc_lookup_set_same. reflexivity.
Qed.

Lemma targets_after_install_other : forall roots slots group record other,
  other <> group ->
  group_targets roots (assoc_set group_eq_dec slots group record) other =
  group_targets roots slots other.
Proof.
  intros roots slots group record other Hother.
  unfold group_targets, installed_targets, group_installed.
  rewrite assoc_lookup_set_other by exact Hother. reflexivity.
Qed.

Lemma targets_after_uninstall_same : forall roots slots group,
  group_targets roots (assoc_remove group_eq_dec slots group) group =
  map (resting_target group) (group_records roots group).
Proof.
  intros roots slots group.
  unfold group_targets, installed_targets, group_installed.
  rewrite assoc_lookup_remove_same. reflexivity.
Qed.

Lemma targets_after_uninstall_other : forall roots slots group other,
  other <> group ->
  group_targets roots (assoc_remove group_eq_dec slots group) other =
  group_targets roots slots other.
Proof.
  intros roots slots group other Hother.
  unfold group_targets, installed_targets, group_installed.
  rewrite assoc_lookup_remove_other by exact Hother. reflexivity.
Qed.

Definition live_records (w : World) (target : ReceiveTarget)
    : list ContinuationRecord :=
  match receive_target_kind target with
  | InstalledKind =>
      match group_installed (world_installed w) (receive_target_group target) with
      | Some record => [record]
      | None => []
      end
  | RestingKind => group_records (world_roots w) (receive_target_group target)
  end.

Definition record_is (expected record : ContinuationRecord) : bool :=
  if continuation_record_eq_dec record expected then true else false.

Definition resolve_receive (w : World) (id : ReceiveId)
    : option ContinuationRecord :=
  if revision_eq_dec (receive_id_revision id) (world_revision w)
  then find (record_is (receive_target_record (receive_id_target id)))
    (live_records w (receive_id_target id))
  else None.

Definition bucketed_resolve (bucket : Path -> nat) (w : World) (id : ReceiveId)
    : option ContinuationRecord :=
  if revision_eq_dec (receive_id_revision id) (world_revision w)
  then find (record_is (receive_target_record (receive_id_target id)))
    (filter (fun record =>
       Nat.eqb (bucket (continuation_identity record))
         (bucket (continuation_identity
            (receive_target_record (receive_id_target id)))))
      (live_records w (receive_id_target id)))
  else None.

Lemma record_is_true : forall expected record,
  record_is expected record = true <-> record = expected.
Proof.
  intros expected record. unfold record_is.
  destruct (continuation_record_eq_dec record expected) as [Hsame|Hdifferent].
  - split; [intros _; exact Hsame|intros _; reflexivity].
  - split; [intro Hfalse; discriminate|intro Hsame; contradiction].
Qed.

Lemma find_record_is_some : forall expected records found,
  find (record_is expected) records = Some found ->
  found = expected /\ In expected records.
Proof.
  intros expected records. induction records as [|head tail IH];
    intros found Hfind; cbn [find] in Hfind; [discriminate|].
  revert Hfind. destruct (record_is expected head) eqn:Hhead; intro Hfind.
  - apply record_is_true in Hhead. injection Hfind as Hfound. subst.
    split; [reflexivity|left; reflexivity].
  - destruct (IH found Hfind) as [Hfound Hin].
    split; [exact Hfound|right; exact Hin].
Qed.

Lemma find_record_is_present : forall expected records,
  In expected records -> find (record_is expected) records = Some expected.
Proof.
  intros expected records. induction records as [|head tail IH];
    intro Hin; [destruct Hin|].
  cbn [find]. destruct (record_is expected head) eqn:Hhead.
  - apply record_is_true in Hhead. subst. reflexivity.
  - destruct Hin as [Hsame|Htail].
    + subst head. exfalso.
      assert (Htrue : record_is expected expected = true)
        by (apply record_is_true; reflexivity).
      congruence.
    + apply IH. exact Htail.
Qed.

Lemma find_record_is_decides : forall expected records,
  find (record_is expected) records =
  if in_dec continuation_record_eq_dec expected records
  then Some expected else None.
Proof.
  intros expected records.
  destruct (in_dec continuation_record_eq_dec expected records)
    as [Hin|Hnotin].
  - apply find_record_is_present. exact Hin.
  - destruct (find (record_is expected) records) as [found|] eqn:Hfind;
      [|reflexivity].
    apply find_record_is_some in Hfind. destruct Hfind as [_ Hin].
    contradiction.
Qed.

Lemma find_record_is_filter : forall expected keep records,
  keep expected = true ->
  find (record_is expected) (filter keep records) =
  find (record_is expected) records.
Proof.
  intros expected keep records Hkeep.
  induction records as [|head tail IH]; [reflexivity|].
  cbn [filter find].
  destruct (keep head) eqn:Hkeep_head.
  - cbn [find]. destruct (record_is expected head); [reflexivity|exact IH].
  - destruct (record_is expected head) eqn:Hhead; [|exact IH].
    apply record_is_true in Hhead. subst head. congruence.
Qed.

Lemma live_record_target : forall w target,
  In (receive_target_record target) (live_records w target) <->
  In target (group_targets (world_roots w) (world_installed w)
    (receive_target_group target)).
Proof.
  intros w [kind group record]. unfold live_records.
  cbn [receive_target_kind receive_target_group receive_target_record].
  rewrite group_targets_spec. destruct kind.
  - destruct (group_installed (world_installed w) group) as [installed|]
      eqn:Hinstalled; cbv beta iota.
    + split.
      * intros [Hsame|[]]. subst installed. left. exists record.
        split; reflexivity.
      * intros [[other [Hother Hsame]]|[other [_ Hsame]]].
        -- unfold installed_target in Hsame.
           injection Hother; injection Hsame; intros; subst.
           left. reflexivity.
        -- unfold resting_target in Hsame. discriminate.
    + split; [intros []|].
      intros [[other [Hother _]]|[other [_ Hsame]]]; [discriminate|].
      unfold resting_target in Hsame. discriminate.
  - split.
    + intro Hin. right. exists record. split; [exact Hin|reflexivity].
    + intros [[other [_ Hsame]]|[other [Hin Hsame]]].
      * unfold installed_target in Hsame. discriminate.
      * unfold resting_target in Hsame. injection Hsame; intros; subst.
        exact Hin.
Qed.

Theorem receive_id_resolution_is_exact :
  (forall w id record,
     resolve_receive w id = Some record ->
     receive_id_revision id = world_revision w /\
     record = receive_target_record (receive_id_target id) /\
     continuation_identity record =
       continuation_identity (receive_target_record (receive_id_target id)) /\
     continuation_source record =
       continuation_source (receive_target_record (receive_id_target id)) /\
     In (receive_id_target id) (group_targets (world_roots w)
       (world_installed w) (receive_target_group (receive_id_target id)))) /\
  (forall w target,
     In target (group_targets (world_roots w) (world_installed w)
       (receive_target_group target)) ->
     resolve_receive w (ReceiveIdState (world_revision w) target) =
       Some (receive_target_record target)).
Proof.
  split.
  - intros w id record Hresolve. unfold resolve_receive in Hresolve.
    revert Hresolve.
    destruct (revision_eq_dec (receive_id_revision id) (world_revision w))
      as [Hrevision|Hrevision]; intro Hresolve; [|discriminate].
    apply find_record_is_some in Hresolve. destruct Hresolve as [Hrecord Hin].
    subst record. split; [exact Hrevision|].
    split; [reflexivity|]. split; [reflexivity|]. split; [reflexivity|].
    apply live_record_target. exact Hin.
  - intros w target Htarget. unfold resolve_receive.
    cbn [receive_id_revision receive_id_target].
    destruct (revision_eq_dec (world_revision w) (world_revision w))
      as [_|Hdifferent]; [|exfalso; apply Hdifferent; reflexivity].
    apply find_record_is_present. apply live_record_target. exact Htarget.
Qed.

Theorem distinct_records_have_distinct_receive_ids :
  (forall group left right,
     continuation_identity left <> continuation_identity right \/
     continuation_source left <> continuation_source right ->
     resting_target group left <> resting_target group right /\
     installed_target group left <> installed_target group right) /\
  (forall group installed resting,
     installed_target group installed <> resting_target group resting) /\
  (forall group left right, left <> right ->
     resting_target group left <> resting_target group right /\
     installed_target group left <> installed_target group right) /\
  (forall revision left right, left <> right ->
     ReceiveIdState revision left <> ReceiveIdState revision right) /\
  (forall record path (position : nat),
     (InstalledTarget record, position) <> (OrdinaryTarget path record, position)).
Proof.
  split; [|split; [|split; [|split]]].
  - intros group left right Hdifferent.
    split; intro Hsame; unfold resting_target, installed_target in Hsame;
      injection Hsame; intros; subst;
      destruct Hdifferent as [Hfield|Hfield]; apply Hfield; reflexivity.
  - intros group installed resting Hsame.
    unfold installed_target, resting_target in Hsame. discriminate.
  - intros group left right Hdifferent.
    split; intro Hsame; unfold resting_target, installed_target in Hsame;
      injection Hsame; intros; subst; apply Hdifferent; reflexivity.
  - intros revision left right Hdifferent Hsame.
    injection Hsame; intros; subst. apply Hdifferent. reflexivity.
  - intros record path position.
    apply installed_and_resting_positions_are_distinct.
Qed.

Theorem hash_bucket_resolution_is_collision_checked :
    forall (bucket : Path -> nat) w id,
  bucketed_resolve bucket w id = resolve_receive w id.
Proof.
  intros bucket w id. unfold bucketed_resolve, resolve_receive.
  destruct (revision_eq_dec (receive_id_revision id) (world_revision w));
    [|reflexivity].
  apply find_record_is_filter. cbv beta. apply Nat.eqb_refl.
Qed.

Theorem stale_revision_never_resolves :
  (forall w id,
     receive_id_revision id <> world_revision w -> resolve_receive w id = None) /\
  (forall operation w id,
     bumps (operation_effect operation w) ->
     In (revision_store (receive_id_revision id)) (world_issued w) ->
     resolve_receive (step_world operation w) id = None).
Proof.
  split.
  - intros w id Hstale. unfold resolve_receive.
    destruct (revision_eq_dec (receive_id_revision id) (world_revision w));
      [contradiction|reflexivity].
  - intros operation w id Hbumps Hissued. unfold resolve_receive.
    destruct (revision_eq_dec (receive_id_revision id)
      (world_revision (step_world operation w))) as [Hsame|Hdifferent];
      [|reflexivity].
    exfalso. apply (fresh_token_is_fresh w).
    unfold step_world in Hsame.
    destruct (step_effect_fresh (operation_effect operation w) w Hbumps)
      as [Hstore _].
    rewrite <- Hstore, <- Hsame. exact Hissued.
Qed.

Theorem first_identity_representative_is_preserved : forall w group record prior,
  first_identity (continuation_identity record)
    (group_records (world_roots w) group) = Some prior ->
  operation_effect (InsertResting group record) w = NoEffect /\
  step_world (InsertResting group record) w = w /\
  first_identity (continuation_identity record)
    (group_records (world_roots (step_world (InsertResting group record) w))
      group) = Some prior /\
  first_identity (continuation_identity record)
    (group_records (world_roots w) group ++ [record]) = Some prior.
Proof.
  intros w group record prior Hfirst.
  assert (Heffect : operation_effect (InsertResting group record) w = NoEffect).
  { cbn [operation_effect]. rewrite Hfirst. reflexivity. }
  assert (Hstep : step_world (InsertResting group record) w = w).
  { unfold step_world. rewrite Heffect. reflexivity. }
  split; [exact Heffect|]. split; [exact Hstep|]. split.
  - rewrite Hstep. exact Hfirst.
  - apply appending_aliases_preserves_first_identity. exact Hfirst.
Qed.

Lemma group_targets_permutation :
    forall left right left_slots right_slots group target,
  Permutation (group_records left group) (group_records right group) ->
  group_installed left_slots group = group_installed right_slots group ->
  (In target (group_targets left left_slots group) <->
   In target (group_targets right right_slots group)).
Proof.
  intros left right left_slots right_slots group target Hrecords Hinstalled.
  rewrite !group_targets_spec, Hinstalled.
  split; intros [Hin|[record [Hrecord Hsame]]];
    [left; exact Hin| |left; exact Hin|].
  - right. exists record. split; [|exact Hsame].
    eapply Permutation_in; [exact Hrecords|exact Hrecord].
  - right. exists record. split; [|exact Hsame].
    eapply Permutation_in; [apply Permutation_sym; exact Hrecords|exact Hrecord].
Qed.

Theorem derivation_is_order_neutral :
  (forall left right left_slots right_slots channel,
     roots_valid left -> slots_valid left_slots ->
     Permutation (root_continuations left) (root_continuations right) ->
     Permutation left_slots right_slots ->
     same_members (derive_occurrences left left_slots channel)
       (derive_occurrences right right_slots channel)) /\
  (forall left right left_slots right_slots channel,
     (forall group,
        Permutation (group_records left group) (group_records right group)) ->
     (forall group,
        group_installed left_slots group = group_installed right_slots group) ->
     same_members (derive_occurrences left left_slots channel)
       (derive_occurrences right right_slots channel)) /\
  (forall left right id,
     world_revision left = world_revision right ->
     (forall group, Permutation (group_records (world_roots left) group)
        (group_records (world_roots right) group)) ->
     (forall group, group_installed (world_installed left) group =
        group_installed (world_installed right) group) ->
     resolve_receive left id = resolve_receive right id).
Proof.
  assert (Hrecords_neutral : forall left right left_slots right_slots channel,
     (forall group,
        Permutation (group_records left group) (group_records right group)) ->
     (forall group,
        group_installed left_slots group = group_installed right_slots group) ->
     same_members (derive_occurrences left left_slots channel)
       (derive_occurrences right right_slots channel)).
  { intros left right left_slots right_slots channel Hrecords Hinstalled
      occurrence.
    rewrite !derive_occurrences_spec.
    rewrite (group_targets_permutation left right left_slots right_slots
      (occurrence_group occurrence) (occurrence_target occurrence)
      (Hrecords _) (Hinstalled _)).
    reflexivity. }
  split; [|split; [exact Hrecords_neutral|]].
  - intros left right left_slots right_slots channel [Hunique _] Hslots
      Hcontinuations Hpermuted_slots.
    apply Hrecords_neutral.
    + intro group. unfold group_records, assoc_get.
      rewrite (assoc_lookup_permutation group_eq_dec (root_continuations left)
        (root_continuations right) group Hunique Hcontinuations).
      apply Permutation_refl.
    + intro group. unfold group_installed.
      apply assoc_lookup_permutation; [exact Hslots|exact Hpermuted_slots].
  - intros left right id Hrevision Hrecords Hinstalled.
    assert (Hlive : Permutation (live_records left (receive_id_target id))
      (live_records right (receive_id_target id))).
    { destruct (receive_id_target id) as [kind group expected].
      unfold live_records. cbn [receive_target_kind receive_target_group].
      destruct kind.
      - rewrite Hinstalled. apply Permutation_refl.
      - apply Hrecords. }
    unfold resolve_receive. rewrite Hrevision.
    destruct (revision_eq_dec (receive_id_revision id) (world_revision right));
      [|reflexivity].
    rewrite !find_record_is_decides.
    destruct (in_dec continuation_record_eq_dec
      (receive_target_record (receive_id_target id))
      (live_records left (receive_id_target id))) as [Hleft|Hleft];
    destruct (in_dec continuation_record_eq_dec
      (receive_target_record (receive_id_target id))
      (live_records right (receive_id_target id))) as [Hright|Hright];
      try reflexivity; exfalso.
    + apply Hright. eapply Permutation_in; [exact Hlive|exact Hleft].
    + apply Hleft. eapply Permutation_in;
        [apply Permutation_sym; exact Hlive|exact Hright].
Qed.

Definition persisted_valid (persisted : PersistedAccelerator) : Prop :=
  (forall group, group_records (persisted_lineage persisted) group <> [] ->
     In group (persisted_scope persisted)) /\
  same_members (persisted_entries persisted)
    (scoped_entries_of (persisted_lineage persisted) [] (persisted_scope persisted)).

Definition op_admissible (operation : Operation) : Prop :=
  match operation with
  | ResetState target slots persisted =>
      roots_valid target /\ slots_valid slots /\
      match persisted with
      | Some stored => persisted_valid stored
      | None => True
      end
  | _ => True
  end.

Inductive AcceleratorState : Type :=
| AcceleratorAbsent
| AcceleratorBuilding (base : Revision) (scope : list GroupKey)
    (entries : list Occurrence)
| AcceleratorPublished (bound : Revision) (lineage : SemanticRoots)
    (scope : list GroupKey) (entries : list Occurrence) (artifacts : Artifacts).

Inductive AcceleratorClass : Type :=
| ClassAbsent
| ClassBuilding
| ClassForeign
| ClassStale
| ClassIncomplete
| ClassCoherent.

Definition covers (scope : list GroupKey) (w : World) : Prop :=
  forall group, targets_at w group <> [] -> In group scope.

Definition coversb (scope : list GroupKey) (w : World) : bool :=
  forallb (fun group => if in_dec group_eq_dec group scope then true else false)
    (support_of w).

Definition classify_accelerator (w : World) (accelerator : AcceleratorState)
    : AcceleratorClass :=
  match accelerator with
  | AcceleratorAbsent => ClassAbsent
  | AcceleratorBuilding _ _ _ => ClassBuilding
  | AcceleratorPublished bound lineage scope _ artifacts =>
      if semantic_roots_eq_dec lineage (world_manifest w) then
        if EqualityEpoch.eq_dec (revision_generation bound)
            (revision_generation (world_revision w)) then
          if revision_eq_dec bound (world_revision w) then
            if artifacts_eq_dec artifacts (artifacts_at bound) then
              if coversb scope w then ClassCoherent else ClassIncomplete
            else ClassStale
          else ClassStale
        else ClassForeign
      else ClassForeign
  end.

Inductive Answer : Type :=
| ExactAnswer (occurrences : list Occurrence)
| FallbackAnswer (occurrences : list Occurrence).

Definition answer_occurrences (answer : Answer) : list Occurrence :=
  match answer with
  | ExactAnswer occurrences => occurrences
  | FallbackAnswer occurrences => occurrences
  end.

Definition accelerator_lookup (entries : list Occurrence) (channel : Path)
    : list Occurrence :=
  filter (fun occurrence =>
    if path_eq_dec (occurrence_channel occurrence) channel then true else false)
    entries.

Definition accelerator_answer (w : World) (accelerator : AcceleratorState)
    (channel : Path) : Answer :=
  match accelerator with
  | AcceleratorPublished _ _ _ entries _ =>
      match classify_accelerator w accelerator with
      | ClassCoherent => ExactAnswer (accelerator_lookup entries channel)
      | _ => FallbackAnswer (derive_world w channel)
      end
  | _ => FallbackAnswer (derive_world w channel)
  end.

Definition accelerator_read (w : World) (accelerator : AcceleratorState)
    (channel : Path) : list Occurrence :=
  answer_occurrences (accelerator_answer w accelerator channel).

Definition publish_at (w : World) (scope : list GroupKey)
    (entries : list Occurrence) : AcceleratorState :=
  AcceleratorPublished (world_revision w) (world_manifest w) scope entries
    (artifacts_at (world_revision w)).

Definition build_accelerator (w : World) : AcceleratorState :=
  publish_at w (support_of w) (scoped_entries w (support_of w)).

Definition begin_build (w : World) : AcceleratorState :=
  AcceleratorBuilding (world_revision w) [] [].

Definition build_group (w : World) (group : GroupKey)
    (accelerator : AcceleratorState) : AcceleratorState :=
  match accelerator with
  | AcceleratorBuilding base scope entries =>
      if revision_eq_dec base (world_revision w) then
        if in_dec group_eq_dec group scope then accelerator
        else AcceleratorBuilding base (scope ++ [group])
          (entries ++ group_entries (world_roots w) (world_installed w) group)
      else accelerator
  | _ => accelerator
  end.

Definition publish_build (w : World) (accelerator : AcceleratorState)
    : AcceleratorState :=
  match accelerator with
  | AcceleratorBuilding base scope entries =>
      if revision_eq_dec base (world_revision w) then
        if coversb scope w then publish_at w scope entries else accelerator
      else accelerator
  | _ => accelerator
  end.

Definition abort_build (accelerator : AcceleratorState) : AcceleratorState :=
  match accelerator with
  | AcceleratorBuilding _ _ _ => AcceleratorAbsent
  | _ => accelerator
  end.

Definition bound_is_current (w : World) (bound : Revision)
    (lineage : SemanticRoots) (artifacts : Artifacts) : bool :=
  if revision_eq_dec bound (world_revision w) then
    if semantic_roots_eq_dec lineage (world_manifest w) then
      if artifacts_eq_dec artifacts (artifacts_at bound) then true else false
    else false
  else false.

Definition current_bound (w : World) (accelerator : AcceleratorState) : bool :=
  match accelerator with
  | AcceleratorPublished bound lineage _ _ artifacts =>
      bound_is_current w bound lineage artifacts
  | _ => false
  end.

Definition without_target (target : ReceiveTarget) (entries : list Occurrence)
    : list Occurrence :=
  filter (fun occurrence =>
    if receive_target_eq_dec (occurrence_target occurrence) target
    then false else true) entries.

Definition installed_in_group (group : GroupKey) (target : ReceiveTarget) : bool :=
  match receive_target_kind target with
  | InstalledKind =>
      if group_eq_dec (receive_target_group target) group then true else false
  | RestingKind => false
  end.

Definition without_installed (group : GroupKey) (entries : list Occurrence)
    : list Occurrence :=
  filter (fun occurrence =>
    negb (installed_in_group group (occurrence_target occurrence))) entries.

Definition installed_entries (slots : InstalledSlots) : list Occurrence :=
  flat_map (fun group =>
    flat_map (target_occurrences group) (installed_targets slots group))
    (map fst slots).

Definition maintained_view (effect : Effect) (w : World)
    (scope : list GroupKey) (entries : list Occurrence)
    : list GroupKey * list Occurrence :=
  match effect with
  | Inserted group record =>
      if in_dec group_eq_dec group scope
      then (scope,
            entries ++ target_occurrences group (resting_target group record))
      else match targets_at w group with
           | [] => (scope ++ [group],
                    entries ++ target_occurrences group (resting_target group record))
           | _ :: _ => (scope, entries)
           end
  | Removed group record _ =>
      (scope, without_target (resting_target group record) entries)
  | Installed group record =>
      if in_dec group_eq_dec group scope
      then (scope, without_installed group entries ++
              target_occurrences group (installed_target group record))
      else match targets_at w group with
           | [] => (scope ++ [group],
                    entries ++ target_occurrences group (installed_target group record))
           | _ :: _ => (scope, entries)
           end
  | Uninstalled group => (scope, without_installed group entries)
  | Loaded group =>
      if in_dec group_eq_dec group scope then (scope, entries)
      else (scope ++ [group],
            entries ++ group_entries (world_roots w) (world_installed w) group)
  | NoEffect => (scope, entries)
  | Replaced _ _ _ => (scope, entries)
  end.

Definition update_effect (effect : Effect) (w : World)
    (accelerator : AcceleratorState) : AcceleratorState :=
  match effect with
  | NoEffect => accelerator
  | Replaced target slots persisted =>
      match persisted with
      | Some stored =>
          if semantic_roots_eq_dec (persisted_lineage stored) target
          then publish_at (step_effect effect w)
            (persisted_scope stored ++ map fst slots)
            (persisted_entries stored ++ installed_entries slots)
          else accelerator
      | None => accelerator
      end
  | _ =>
      match accelerator with
      | AcceleratorPublished bound lineage scope entries artifacts =>
          if bound_is_current w bound lineage artifacts then
            publish_at (step_effect effect w)
              (fst (maintained_view effect w scope entries))
              (snd (maintained_view effect w scope entries))
          else accelerator
      | _ => accelerator
      end
  end.

Definition update_accelerator (operation : Operation) (w : World)
    (accelerator : AcceleratorState) : AcceleratorState :=
  update_effect (operation_effect operation w) w accelerator.

Definition commit_operation (operation : Operation) (w : World)
    (accelerator : AcceleratorState) (fault : bool) : World * AcceleratorState :=
  if fault then (w, accelerator)
  else (step_world operation w, update_accelerator operation w accelerator).

Definition accelerator_invariant (w : World) (accelerator : AcceleratorState)
    : Prop :=
  match accelerator with
  | AcceleratorAbsent => True
  | AcceleratorBuilding base scope entries =>
      revision_issued (world_issued w) base /\
      (base = world_revision w -> same_members entries (scoped_entries w scope))
  | AcceleratorPublished bound lineage scope entries _ =>
      revision_issued (world_issued w) bound /\
      (bound = world_revision w -> lineage = world_manifest w ->
       same_members entries (scoped_entries w scope))
  end.

Definition exact_accelerator (w : World) (accelerator : AcceleratorState)
    : Prop :=
  exists scope entries,
    accelerator = publish_at w scope entries /\ covers scope w /\
    same_members entries (scoped_entries w scope).

Definition not_reset (operation : Operation) : Prop :=
  match operation with
  | ResetState _ _ _ => False
  | _ => True
  end.

Definition maintains (effect : Effect) : Prop :=
  match effect with
  | Replaced _ _ _ => False
  | _ => True
  end.

Lemma same_members_refl {A : Type} (items : list A) : same_members items items.
Proof. intro item. reflexivity. Qed.

Lemma same_members_sym {A : Type} {left right : list A} :
  same_members left right -> same_members right left.
Proof. intros Hsame item. symmetry. apply Hsame. Qed.

Lemma same_members_trans {A : Type} {left middle right : list A} :
  same_members left middle -> same_members middle right ->
  same_members left right.
Proof.
  intros Hleft Hright item. rewrite (Hleft item). apply Hright.
Qed.

Lemma same_members_app {A : Type} {left left' right right' : list A} :
  same_members left left' -> same_members right right' ->
  same_members (left ++ right) (left' ++ right').
Proof.
  intros Hleft Hright item.
  rewrite !in_app_iff, (Hleft item), (Hright item). reflexivity.
Qed.

Lemma same_members_filter {A : Type} (keep : A -> bool) {left right : list A} :
  same_members left right -> same_members (filter keep left) (filter keep right).
Proof.
  intros Hsame item. rewrite !filter_In, (Hsame item). reflexivity.
Qed.

Lemma nonempty_subset : forall (A : Type) (left right : list A),
  (forall item, In item left -> In item right) -> left <> [] -> right <> [].
Proof.
  intros A left right Hsubset Hleft Hright.
  destruct left as [|item rest]; [apply Hleft; reflexivity|].
  assert (Hin : In item right) by (apply Hsubset; left; reflexivity).
  rewrite Hright in Hin. destruct Hin.
Qed.

Lemma coversb_spec : forall scope w, coversb scope w = true <-> covers scope w.
Proof.
  intros scope w. unfold coversb, covers. rewrite forallb_forall. split.
  - intros Hall group Htargets.
    assert (Hsupport : In group (support_of w)).
    { unfold support_of. apply receive_support_spec. exact Htargets. }
    specialize (Hall group Hsupport). cbv beta in Hall. revert Hall.
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin];
      intro Hall; [exact Hin|discriminate].
  - intros Hcover group Hsupport. cbv beta.
    unfold support_of in Hsupport. apply receive_support_spec in Hsupport.
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin];
      [reflexivity|exfalso; apply Hnotin; apply Hcover; exact Hsupport].
Qed.

Lemma accelerator_lookup_spec : forall entries channel occurrence,
  In occurrence (accelerator_lookup entries channel) <->
  In occurrence entries /\ occurrence_channel occurrence = channel.
Proof.
  intros entries channel occurrence. unfold accelerator_lookup.
  rewrite filter_In. cbv beta.
  destruct (path_eq_dec (occurrence_channel occurrence) channel)
    as [Hsame|Hdifferent]; cbv iota.
  - split; intros [Hin _]; split;
      [exact Hin|exact Hsame|exact Hin|reflexivity].
  - split; intros [Hin Hfalse]; [discriminate|contradiction].
Qed.

Lemma scoped_entries_world_spec : forall w scope occurrence,
  In occurrence (scoped_entries w scope) <->
  In (occurrence_group occurrence) scope /\
  In (occurrence_target occurrence) (targets_at w (occurrence_group occurrence)) /\
  occurrence_shape occurrence.
Proof.
  intros w scope occurrence. unfold scoped_entries, targets_at.
  apply scoped_entries_spec.
Qed.

Lemma lookup_scoped_entries_is_derivation : forall w scope entries channel,
  covers scope w ->
  same_members entries (scoped_entries w scope) ->
  same_members (accelerator_lookup entries channel) (derive_world w channel).
Proof.
  intros w scope entries channel Hcover Hentries occurrence.
  rewrite accelerator_lookup_spec, (Hentries occurrence),
    scoped_entries_world_spec.
  unfold derive_world. rewrite derive_occurrences_spec. split.
  - intros [[_ [Htarget Hshape]] Hchannel].
    split; [exact Hchannel|split; [exact Htarget|exact Hshape]].
  - intros [Hchannel [Htarget Hshape]].
    split; [|exact Hchannel].
    split; [|split; [exact Htarget|exact Hshape]].
    apply Hcover. intro Hempty. unfold targets_at in Hempty.
    rewrite Hempty in Htarget. destruct Htarget.
Qed.

Lemma bound_is_current_at : forall w,
  bound_is_current w (world_revision w) (world_manifest w)
    (artifacts_at (world_revision w)) = true.
Proof.
  intro w. unfold bound_is_current.
  destruct (revision_eq_dec (world_revision w) (world_revision w)) as [_|Hne];
    [|exfalso; apply Hne; reflexivity].
  destruct (semantic_roots_eq_dec (world_manifest w) (world_manifest w))
    as [_|Hne]; [|exfalso; apply Hne; reflexivity].
  destruct (artifacts_eq_dec (artifacts_at (world_revision w))
    (artifacts_at (world_revision w))) as [_|Hne];
    [reflexivity|exfalso; apply Hne; reflexivity].
Qed.

Lemma current_bound_publish_at : forall w scope entries,
  current_bound w (publish_at w scope entries) = true.
Proof.
  intros w scope entries. unfold publish_at. cbn [current_bound].
  apply bound_is_current_at.
Qed.

Lemma current_bound_spec : forall w accelerator,
  current_bound w accelerator = true ->
  exists scope entries, accelerator = publish_at w scope entries.
Proof.
  intros w accelerator Hcurrent.
  destruct accelerator as [|base scope entries|bound lineage scope entries artifacts];
    cbn [current_bound] in Hcurrent; try discriminate.
  unfold bound_is_current in Hcurrent. revert Hcurrent.
  destruct (revision_eq_dec bound (world_revision w)) as [Hbound|];
    [|intro Hfalse; discriminate].
  destruct (semantic_roots_eq_dec lineage (world_manifest w)) as [Hlineage|];
    [|intro Hfalse; discriminate].
  destruct (artifacts_eq_dec artifacts (artifacts_at bound)) as [Hartifacts|];
    [|intro Hfalse; discriminate].
  intros _. subst bound lineage artifacts. exists scope, entries. reflexivity.
Qed.

Lemma classify_publish_at : forall w scope entries,
  classify_accelerator w (publish_at w scope entries) =
  if coversb scope w then ClassCoherent else ClassIncomplete.
Proof.
  intros w scope entries. unfold publish_at. cbn [classify_accelerator].
  destruct (semantic_roots_eq_dec (world_manifest w) (world_manifest w))
    as [_|Hne]; [|exfalso; apply Hne; reflexivity].
  destruct (EqualityEpoch.eq_dec (revision_generation (world_revision w))
    (revision_generation (world_revision w))) as [_|Hne];
    [|exfalso; apply Hne; reflexivity].
  destruct (revision_eq_dec (world_revision w) (world_revision w)) as [_|Hne];
    [|exfalso; apply Hne; reflexivity].
  destruct (artifacts_eq_dec (artifacts_at (world_revision w))
    (artifacts_at (world_revision w))) as [_|Hne];
    [reflexivity|exfalso; apply Hne; reflexivity].
Qed.

Lemma read_publish_at : forall w scope entries channel,
  accelerator_read w (publish_at w scope entries) channel =
  if coversb scope w then accelerator_lookup entries channel
  else derive_world w channel.
Proof.
  intros w scope entries channel. unfold accelerator_read, accelerator_answer.
  rewrite classify_publish_at. unfold publish_at.
  destruct (coversb scope w); reflexivity.
Qed.

Lemma exact_accelerator_reads : forall w accelerator,
  exact_accelerator w accelerator ->
  classify_accelerator w accelerator = ClassCoherent /\
  current_bound w accelerator = true /\
  (forall channel, same_members (accelerator_read w accelerator channel)
     (derive_world w channel)).
Proof.
  intros w accelerator [scope [entries [Haccelerator [Hcover Hentries]]]].
  subst accelerator. assert (Hcoversb := proj2 (coversb_spec scope w) Hcover).
  split; [rewrite classify_publish_at, Hcoversb; reflexivity|].
  split; [apply current_bound_publish_at|].
  intro channel. rewrite read_publish_at, Hcoversb.
  apply lookup_scoped_entries_is_derivation with (scope := scope);
    [exact Hcover|exact Hentries].
Qed.

Lemma build_accelerator_exact : forall w, exact_accelerator w (build_accelerator w).
Proof.
  intro w. exists (support_of w), (scoped_entries w (support_of w)).
  split; [reflexivity|]. split; [|apply same_members_refl].
  intros group Htargets. unfold support_of. apply receive_support_spec.
  exact Htargets.
Qed.

Lemma scoped_entries_extend : forall w scope group,
  scoped_entries w (scope ++ [group]) =
  scoped_entries w scope ++ group_entries (world_roots w) (world_installed w) group.
Proof.
  intros w scope group. unfold scoped_entries, scoped_entries_of.
  rewrite flat_map_app. cbn [flat_map]. rewrite app_nil_r. reflexivity.
Qed.

Lemma scoped_entries_frame : forall w w' scope,
  (forall group, In group scope -> targets_at w' group = targets_at w group) ->
  same_members (scoped_entries w' scope) (scoped_entries w scope).
Proof.
  intros w w' scope Hsame occurrence.
  rewrite !scoped_entries_world_spec. split.
  - intros [Hscope [Htarget Hshape]]. rewrite (Hsame _ Hscope) in Htarget.
    split; [exact Hscope|split; [exact Htarget|exact Hshape]].
  - intros [Hscope [Htarget Hshape]]. rewrite <- (Hsame _ Hscope) in Htarget.
    split; [exact Hscope|split; [exact Htarget|exact Hshape]].
Qed.

Lemma foreign_target_is_not_installed_in_group : forall w owner group target,
  owner <> group -> In target (targets_at w owner) ->
  installed_in_group group target = false.
Proof.
  intros w owner group target Hdifferent Hin. unfold targets_at in Hin.
  apply target_group_is_group in Hin. unfold installed_in_group. rewrite Hin.
  destruct (receive_target_kind target); [|reflexivity].
  destruct (group_eq_dec owner group); [contradiction|reflexivity].
Qed.

Lemma resting_targets_are_not_installed : forall group records target,
  In target (map (resting_target group) records) ->
  installed_in_group group target = false.
Proof.
  intros group records target Hin. apply in_map_iff in Hin.
  destruct Hin as [record [Htarget _]]. subst target. reflexivity.
Qed.

Lemma installed_targets_are_installed : forall slots group target,
  In target (installed_targets slots group) ->
  installed_in_group group target = true.
Proof.
  intros slots group target Hin. unfold installed_targets in Hin. revert Hin.
  destruct (group_installed slots group) as [record|]; intro Hin;
    [|destruct Hin].
  destruct Hin as [Htarget|[]]. subst target.
  unfold installed_in_group, installed_target.
  cbn [receive_target_kind receive_target_group].
  destruct (group_eq_dec group group) as [_|Hne];
    [reflexivity|exfalso; apply Hne; reflexivity].
Qed.

Lemma resting_part_spec : forall w group target,
  In target (targets_at w group) /\ installed_in_group group target = false <->
  In target (map (resting_target group) (group_records (world_roots w) group)).
Proof.
  intros w group target. unfold targets_at, group_targets.
  rewrite in_app_iff. split.
  - intros [[Hinstalled|Hresting] Hfalse]; [|exact Hresting].
    rewrite (installed_targets_are_installed _ _ _ Hinstalled) in Hfalse.
    discriminate.
  - intro Hresting. split; [right; exact Hresting|].
    exact (resting_targets_are_not_installed _ _ _ Hresting).
Qed.

Lemma insert_targets_spec : forall w group record target,
  In target (targets_at (step_effect (Inserted group record) w) group) <->
  In target (targets_at w group) \/ target = resting_target group record.
Proof.
  intros w group record target. unfold targets_at.
  cbn [step_effect bump_group world_roots world_installed].
  rewrite targets_after_insert_same, in_app_iff, in_singleton_iff.
  reflexivity.
Qed.

Lemma insert_frame : forall w group record other, other <> group ->
  targets_at (step_effect (Inserted group record) w) other = targets_at w other.
Proof.
  intros w group record other Hother. unfold targets_at.
  cbn [step_effect bump_group world_roots world_installed].
  apply targets_after_insert_other. exact Hother.
Qed.

Lemma removal_targets_spec : forall w group record target,
  NoDup (group_records (world_roots w) group) ->
  (In target (installed_targets (world_installed w) group ++
     map (resting_target group)
       (remove_first_record record (group_records (world_roots w) group))) <->
   In target (targets_at w group) /\ target <> resting_target group record).
Proof.
  intros w group record target Hunique.
  pose proof (remove_first_record_spec record
    (group_records (world_roots w) group)) as Hremove.
  unfold targets_at, group_targets. rewrite !in_app_iff, !in_map_iff. split.
  - intros [Hinstalled|[other [Htarget Hother]]].
    + split; [left; exact Hinstalled|].
      intro Hsame. rewrite Hsame in Hinstalled.
      unfold installed_targets in Hinstalled. revert Hinstalled.
      destruct (group_installed (world_installed w) group) as [installed|];
        intro Hinstalled; [|destruct Hinstalled].
      destruct Hinstalled as [Hinstalled|[]].
      unfold installed_target, resting_target in Hinstalled. discriminate.
    + destruct (proj1 (Hremove other Hunique) Hother) as [Hin Hne]. split.
      * right. exists other. split; [exact Htarget|exact Hin].
      * intro Hsame. rewrite <- Htarget in Hsame.
        unfold resting_target in Hsame. congruence.
  - intros [[Hinstalled|[other [Htarget Hother]]] Hne].
    + left. exact Hinstalled.
    + right. exists other. split; [exact Htarget|].
      apply (proj2 (Hremove other Hunique)). split; [exact Hother|].
      intro Hsame. subst other. apply Hne. symmetry. exact Htarget.
Qed.

Lemma remove_targets_spec : forall w group record target,
  NoDup (group_records (world_roots w) group) ->
  (In target (targets_at (step_effect (Removed group record
     (remove_first_record record (group_records (world_roots w) group))) w)
     group) <->
   In target (targets_at w group) /\ target <> resting_target group record).
Proof.
  intros w group record target Hunique. unfold targets_at at 1.
  cbn [step_effect bump_group world_roots world_installed].
  rewrite targets_after_remove_same. apply removal_targets_spec. exact Hunique.
Qed.

Lemma remove_frame : forall w group record remaining other, other <> group ->
  targets_at (step_effect (Removed group record remaining) w) other =
  targets_at w other.
Proof.
  intros w group record remaining other Hother. unfold targets_at.
  cbn [step_effect bump_group world_roots world_installed].
  apply targets_after_remove_other. exact Hother.
Qed.

Lemma install_targets_spec : forall w group record target,
  In target (targets_at (step_effect (Installed group record) w) group) <->
  target = installed_target group record \/
  (In target (targets_at w group) /\ installed_in_group group target = false).
Proof.
  intros w group record target. rewrite resting_part_spec.
  unfold targets_at. cbn [step_effect bump_group world_roots world_installed].
  rewrite targets_after_install_same. cbn [In]. split;
    intros [Hsame|Hin];
    [left; symmetry; exact Hsame|right; exact Hin|
     left; symmetry; exact Hsame|right; exact Hin].
Qed.

Lemma install_frame : forall w group record other, other <> group ->
  targets_at (step_effect (Installed group record) w) other = targets_at w other.
Proof.
  intros w group record other Hother. unfold targets_at.
  cbn [step_effect bump_group world_roots world_installed].
  apply targets_after_install_other. exact Hother.
Qed.

Lemma uninstall_targets_spec : forall w group target,
  In target (targets_at (step_effect (Uninstalled group) w) group) <->
  In target (targets_at w group) /\ installed_in_group group target = false.
Proof.
  intros w group target. rewrite resting_part_spec. unfold targets_at.
  cbn [step_effect bump_group world_roots world_installed].
  rewrite targets_after_uninstall_same. reflexivity.
Qed.

Lemma uninstall_frame : forall w group other, other <> group ->
  targets_at (step_effect (Uninstalled group) w) other = targets_at w other.
Proof.
  intros w group other Hother. unfold targets_at.
  cbn [step_effect bump_group world_roots world_installed].
  apply targets_after_uninstall_other. exact Hother.
Qed.

Lemma scoped_entries_add_target : forall w w' scope group target,
  In group scope ->
  (forall other, other <> group -> targets_at w' other = targets_at w other) ->
  (forall t, In t (targets_at w' group) <-> In t (targets_at w group) \/ t = target) ->
  same_members (scoped_entries w' scope)
    (scoped_entries w scope ++ target_occurrences group target).
Proof.
  intros w w' scope group target Hin Hother Htargets occurrence.
  rewrite in_app_iff, !scoped_entries_world_spec, target_occurrences_spec.
  destruct (group_eq_dec (occurrence_group occurrence) group)
    as [Hsame|Hdifferent].
  - rewrite Hsame, Htargets. split.
    + intros [Hscope [[Hold|Hnew] Hshape]].
      * left. split; [exact Hscope|split; [exact Hold|exact Hshape]].
      * right. split; [exact Hnew|split; [reflexivity|exact Hshape]].
    + intros [[Hscope [Hold Hshape]]|[Htarget [_ Hshape]]].
      * split; [exact Hscope|split; [left; exact Hold|exact Hshape]].
      * split; [exact Hin|split; [right; exact Htarget|exact Hshape]].
  - rewrite (Hother _ Hdifferent). split.
    + intro Hentry. left. exact Hentry.
    + intros [Hentry|[_ [Hgroup _]]]; [exact Hentry|contradiction].
Qed.

Lemma scoped_entries_new_group : forall w w' scope group target,
  ~ In group scope ->
  (forall other, other <> group -> targets_at w' other = targets_at w other) ->
  (forall t, In t (targets_at w' group) <-> t = target) ->
  same_members (scoped_entries w' (scope ++ [group]))
    (scoped_entries w scope ++ target_occurrences group target).
Proof.
  intros w w' scope group target Hnotin Hother Htargets occurrence.
  rewrite in_app_iff, !scoped_entries_world_spec, target_occurrences_spec,
    in_app_iff, in_singleton_iff.
  destruct (group_eq_dec (occurrence_group occurrence) group)
    as [Hsame|Hdifferent].
  - rewrite Hsame, Htargets. split.
    + intros [_ [Htarget Hshape]]. right.
      split; [exact Htarget|split; [reflexivity|exact Hshape]].
    + intros [[Hscope _]|[Htarget [_ Hshape]]]; [contradiction|].
      split; [right; reflexivity|split; [exact Htarget|exact Hshape]].
  - rewrite (Hother _ Hdifferent). split.
    + intros [[Hscope|Hgroup] [Htarget Hshape]]; [|contradiction].
      left. split; [exact Hscope|split; [exact Htarget|exact Hshape]].
    + intros [[Hscope [Htarget Hshape]]|[_ [Hgroup _]]]; [|contradiction].
      split; [left; exact Hscope|split; [exact Htarget|exact Hshape]].
Qed.

Lemma scoped_entries_drop_target : forall w w' scope group target,
  receive_target_group target = group ->
  (forall other, other <> group -> targets_at w' other = targets_at w other) ->
  (forall t, In t (targets_at w' group) <->
     In t (targets_at w group) /\ t <> target) ->
  same_members (scoped_entries w' scope)
    (without_target target (scoped_entries w scope)).
Proof.
  intros w w' scope group target Hgroup Hother Htargets occurrence.
  unfold without_target. rewrite filter_In, !scoped_entries_world_spec.
  cbv beta.
  destruct (receive_target_eq_dec (occurrence_target occurrence) target)
    as [Htarget|Htarget]; cbv iota.
  - split; [|intros [_ Hfalse]; discriminate].
    intros [Hscope [Hin Hshape]]. exfalso.
    destruct (group_eq_dec (occurrence_group occurrence) group)
      as [Hsame|Hdifferent].
    + rewrite Hsame, Htargets in Hin. destruct Hin as [_ Hne].
      contradiction.
    + rewrite (Hother _ Hdifferent) in Hin. unfold targets_at in Hin.
      apply target_group_is_group in Hin. rewrite Htarget, Hgroup in Hin.
      apply Hdifferent. symmetry. exact Hin.
  - split.
    + intros [Hscope [Hin Hshape]]. split; [|reflexivity].
      split; [exact Hscope|split; [|exact Hshape]].
      destruct (group_eq_dec (occurrence_group occurrence) group)
        as [Hsame|Hdifferent].
      * rewrite Hsame in Hin |- *. apply Htargets in Hin.
        destruct Hin as [Hin _]. exact Hin.
      * rewrite (Hother _ Hdifferent) in Hin. exact Hin.
    + intros [[Hscope [Hin Hshape]] _].
      split; [exact Hscope|split; [|exact Hshape]].
      destruct (group_eq_dec (occurrence_group occurrence) group)
        as [Hsame|Hdifferent].
      * rewrite Hsame in Hin |- *. apply Htargets.
        split; [exact Hin|exact Htarget].
      * rewrite (Hother _ Hdifferent). exact Hin.
Qed.

Lemma scoped_entries_drop_installed : forall w w' scope group,
  (forall other, other <> group -> targets_at w' other = targets_at w other) ->
  (forall t, In t (targets_at w' group) <->
     In t (targets_at w group) /\ installed_in_group group t = false) ->
  same_members (scoped_entries w' scope)
    (without_installed group (scoped_entries w scope)).
Proof.
  intros w w' scope group Hother Htargets occurrence.
  unfold without_installed. rewrite filter_In, !scoped_entries_world_spec.
  cbv beta.
  destruct (group_eq_dec (occurrence_group occurrence) group)
    as [Hsame|Hdifferent].
  - rewrite Hsame, Htargets.
    destruct (installed_in_group group (occurrence_target occurrence));
      cbn [negb].
    + split; [intros [_ [[_ Hfalse] _]]; discriminate|].
      intros [_ Hfalse]. discriminate.
    + split.
      * intros [Hscope [[Hin _] Hshape]].
        split; [split; [exact Hscope|split; [exact Hin|exact Hshape]]|].
        reflexivity.
      * intros [[Hscope [Hin Hshape]] _].
        split; [exact Hscope|split; [split; [exact Hin|reflexivity]|]].
        exact Hshape.
  - rewrite (Hother _ Hdifferent). split.
    + intros [Hscope [Hin Hshape]].
      rewrite (foreign_target_is_not_installed_in_group w _ group _
        Hdifferent Hin).
      split; [split; [exact Hscope|split; [exact Hin|exact Hshape]]|].
      reflexivity.
    + intros [Hentry _]. exact Hentry.
Qed.

Lemma scoped_entries_replace_installed : forall w w' scope group target,
  In group scope ->
  (forall other, other <> group -> targets_at w' other = targets_at w other) ->
  (forall t, In t (targets_at w' group) <->
     t = target \/ (In t (targets_at w group) /\ installed_in_group group t = false)) ->
  same_members (scoped_entries w' scope)
    (without_installed group (scoped_entries w scope) ++
       target_occurrences group target).
Proof.
  intros w w' scope group target Hgroup_scope Hother Htargets occurrence.
  unfold without_installed.
  rewrite in_app_iff, filter_In, !scoped_entries_world_spec,
    target_occurrences_spec.
  cbv beta.
  destruct (group_eq_dec (occurrence_group occurrence) group)
    as [Hsame|Hdifferent].
  - rewrite Hsame, Htargets.
    destruct (installed_in_group group (occurrence_target occurrence));
      cbn [negb].
    + split.
      * intros [_ [[Hnew|[_ Hfalse]] Hshape]]; [|discriminate].
        right. split; [exact Hnew|split; [reflexivity|exact Hshape]].
      * intros [[_ Hfalse]|[Hnew [_ Hshape]]]; [discriminate|].
        split; [exact Hgroup_scope|split; [left; exact Hnew|exact Hshape]].
    + split.
      * intros [_ [[Hnew|[Hin _]] Hshape]].
        -- right. split; [exact Hnew|split; [reflexivity|exact Hshape]].
        -- left. split; [|reflexivity].
           split; [exact Hgroup_scope|split; [exact Hin|exact Hshape]].
      * intros [[[_ [Hin Hshape]] _]|[Hnew [_ Hshape]]].
        -- split; [exact Hgroup_scope|split; [|exact Hshape]].
           right. split; [exact Hin|reflexivity].
        -- split; [exact Hgroup_scope|split; [left; exact Hnew|exact Hshape]].
  - rewrite (Hother _ Hdifferent). split.
    + intros [Hscope [Hin Hshape]]. left.
      rewrite (foreign_target_is_not_installed_in_group w _ group _
        Hdifferent Hin).
      split; [split; [exact Hscope|split; [exact Hin|exact Hshape]]|].
      reflexivity.
    + intros [[Hentry _]|[_ [Hgroup _]]]; [exact Hentry|contradiction].
Qed.

Lemma covers_frame : forall w w' scope scope' group,
  covers scope w ->
  (forall other, other <> group -> targets_at w' other = targets_at w other) ->
  (forall item, In item scope -> In item scope') ->
  (targets_at w' group <> [] -> In group scope') ->
  covers scope' w'.
Proof.
  intros w w' scope scope' group Hcover Hframe Hsubset Hgroup other Hnonempty.
  destruct (group_eq_dec other group) as [Hsame|Hdifferent].
  - subst other. apply Hgroup. exact Hnonempty.
  - apply Hsubset. apply Hcover. rewrite <- (Hframe _ Hdifferent).
    exact Hnonempty.
Qed.

Lemma maintained_view_scoped : forall operation w scope entries,
  not_reset operation ->
  roots_valid (world_roots w) ->
  same_members entries (scoped_entries w scope) ->
  same_members
    (snd (maintained_view (operation_effect operation w) w scope entries))
    (scoped_entries (step_world operation w)
      (fst (maintained_view (operation_effect operation w) w scope entries))).
Proof.
  intros operation w scope entries Hreset [_ Hidentities] Hentries.
  unfold step_world.
  destruct operation as [group record|group record|group record|group|group|
    target slots persisted]; cbn [not_reset] in Hreset;
    [| | | | |contradiction]; cbn [operation_effect].
  - destruct (first_identity (continuation_identity record)
      (group_records (world_roots w) group)); [exact Hentries|].
    cbn [maintained_view].
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin].
    + cbn [fst snd]. apply same_members_sym. eapply same_members_trans.
      * apply (scoped_entries_add_target w
          (step_effect (Inserted group record) w) scope group
          (resting_target group record) Hin).
        -- intros other Hother. apply insert_frame. exact Hother.
        -- intro t. apply insert_targets_spec.
      * apply same_members_app;
          [apply same_members_sym; exact Hentries|apply same_members_refl].
    + destruct (targets_at w group) as [|head tail] eqn:Htargets;
        cbn [fst snd].
      * apply same_members_sym. eapply same_members_trans.
        -- apply (scoped_entries_new_group w
             (step_effect (Inserted group record) w) scope group
             (resting_target group record) Hnotin).
           ++ intros other Hother. apply insert_frame. exact Hother.
           ++ intro t. rewrite insert_targets_spec, Htargets. cbn [In]. tauto.
        -- apply same_members_app;
             [apply same_members_sym; exact Hentries|apply same_members_refl].
      * eapply same_members_trans; [exact Hentries|].
        apply same_members_sym. apply scoped_entries_frame.
        intros other Hother. apply insert_frame.
        intro Hsame. subst other. contradiction.
  - destruct (in_dec continuation_record_eq_dec record
      (group_records (world_roots w) group)) as [Hin|Hnotin];
      [|exact Hentries].
    cbn [maintained_view fst snd].
    assert (Hunique : NoDup (group_records (world_roots w) group))
      by (eapply NoDup_map_inv; exact (Hidentities group)).
    apply same_members_sym. eapply same_members_trans.
    + apply (scoped_entries_drop_target w
        (step_effect (Removed group record
          (remove_first_record record (group_records (world_roots w) group))) w)
        scope group (resting_target group record) eq_refl).
      * intros other Hother. apply remove_frame. exact Hother.
      * intro t. apply remove_targets_spec. exact Hunique.
    + unfold without_target. apply same_members_filter.
      apply same_members_sym. exact Hentries.
  - cbn [maintained_view].
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin].
    + cbn [fst snd]. apply same_members_sym. eapply same_members_trans.
      * apply (scoped_entries_replace_installed w
          (step_effect (Installed group record) w) scope group
          (installed_target group record) Hin).
        -- intros other Hother. apply install_frame. exact Hother.
        -- intro t. apply install_targets_spec.
      * apply same_members_app; [|apply same_members_refl].
        unfold without_installed. apply same_members_filter.
        apply same_members_sym. exact Hentries.
    + destruct (targets_at w group) as [|head tail] eqn:Htargets;
        cbn [fst snd].
      * apply same_members_sym. eapply same_members_trans.
        -- apply (scoped_entries_new_group w
             (step_effect (Installed group record) w) scope group
             (installed_target group record) Hnotin).
           ++ intros other Hother. apply install_frame. exact Hother.
           ++ intro t. rewrite install_targets_spec, Htargets. cbn [In]. tauto.
        -- apply same_members_app;
             [apply same_members_sym; exact Hentries|apply same_members_refl].
      * eapply same_members_trans; [exact Hentries|].
        apply same_members_sym. apply scoped_entries_frame.
        intros other Hother. apply install_frame.
        intro Hsame. subst other. contradiction.
  - destruct (group_installed (world_installed w) group) as [installed|];
      [|exact Hentries].
    cbn [maintained_view fst snd]. apply same_members_sym.
    eapply same_members_trans.
    + apply (scoped_entries_drop_installed w
        (step_effect (Uninstalled group) w) scope group).
      * intros other Hother. apply uninstall_frame. exact Hother.
      * intro t. apply uninstall_targets_spec.
    + unfold without_installed. apply same_members_filter.
      apply same_members_sym. exact Hentries.
  - cbn [maintained_view step_effect].
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin];
      cbn [fst snd]; [exact Hentries|].
    rewrite scoped_entries_extend.
    apply same_members_app; [exact Hentries|apply same_members_refl].
Qed.

Lemma maintained_view_covers : forall operation w scope entries,
  not_reset operation ->
  roots_valid (world_roots w) ->
  covers scope w ->
  covers (fst (maintained_view (operation_effect operation w) w scope entries))
    (step_world operation w).
Proof.
  intros operation w scope entries Hreset [_ Hidentities] Hcover.
  unfold step_world.
  destruct operation as [group record|group record|group record|group|group|
    target slots persisted]; cbn [not_reset] in Hreset;
    [| | | | |contradiction]; cbn [operation_effect].
  - destruct (first_identity (continuation_identity record)
      (group_records (world_roots w) group)); [exact Hcover|].
    cbn [maintained_view].
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin].
    + cbn [fst]. apply (covers_frame w _ scope scope group Hcover).
      * intros other Hother. apply insert_frame. exact Hother.
      * intros item Hitem. exact Hitem.
      * intros _. exact Hin.
    + destruct (targets_at w group) as [|head tail] eqn:Htargets; cbn [fst].
      * apply (covers_frame w _ scope (scope ++ [group]) group Hcover).
        -- intros other Hother. apply insert_frame. exact Hother.
        -- intros item Hitem. apply in_or_app. left. exact Hitem.
        -- intros _. apply in_or_app. right. left. reflexivity.
      * exfalso. apply Hnotin. apply Hcover. rewrite Htargets. discriminate.
  - destruct (in_dec continuation_record_eq_dec record
      (group_records (world_roots w) group)) as [Hin|Hnotin];
      [|exact Hcover].
    cbn [maintained_view fst].
    assert (Hunique : NoDup (group_records (world_roots w) group))
      by (eapply NoDup_map_inv; exact (Hidentities group)).
    apply (covers_frame w _ scope scope group Hcover).
    + intros other Hother. apply remove_frame. exact Hother.
    + intros item Hitem. exact Hitem.
    + intro Hnonempty. apply Hcover.
      apply (nonempty_subset _ (targets_at (step_effect (Removed group record
        (remove_first_record record (group_records (world_roots w) group))) w)
        group)); [|exact Hnonempty].
      intros t Ht.
      destruct (proj1 (remove_targets_spec w group record t Hunique) Ht)
        as [Hold _].
      exact Hold.
  - cbn [maintained_view].
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin].
    + cbn [fst]. apply (covers_frame w _ scope scope group Hcover).
      * intros other Hother. apply install_frame. exact Hother.
      * intros item Hitem. exact Hitem.
      * intros _. exact Hin.
    + destruct (targets_at w group) as [|head tail] eqn:Htargets; cbn [fst].
      * apply (covers_frame w _ scope (scope ++ [group]) group Hcover).
        -- intros other Hother. apply install_frame. exact Hother.
        -- intros item Hitem. apply in_or_app. left. exact Hitem.
        -- intros _. apply in_or_app. right. left. reflexivity.
      * exfalso. apply Hnotin. apply Hcover. rewrite Htargets. discriminate.
  - destruct (group_installed (world_installed w) group) as [installed|];
      [|exact Hcover].
    cbn [maintained_view fst].
    apply (covers_frame w _ scope scope group Hcover).
    + intros other Hother. apply uninstall_frame. exact Hother.
    + intros item Hitem. exact Hitem.
    + intro Hnonempty. apply Hcover.
      apply (nonempty_subset _
        (targets_at (step_effect (Uninstalled group) w) group));
        [|exact Hnonempty].
      intros t Ht.
      destruct (proj1 (uninstall_targets_spec w group t) Ht) as [Hold _].
      exact Hold.
  - cbn [maintained_view step_effect].
    destruct (in_dec group_eq_dec group scope) as [Hin|Hnotin];
      cbn [fst]; [exact Hcover|].
    intros other Hnonempty. apply in_or_app. left. apply Hcover.
    exact Hnonempty.
Qed.

Lemma operation_effect_maintains : forall operation w,
  not_reset operation -> maintains (operation_effect operation w).
Proof.
  intros operation w Hreset.
  destruct operation as [group record|group record|group record|group|group|
    target slots persisted]; cbn [not_reset] in Hreset; cbn [operation_effect].
  - destruct (first_identity (continuation_identity record)
      (group_records (world_roots w) group)); exact I.
  - destruct (in_dec continuation_record_eq_dec record
      (group_records (world_roots w) group)); exact I.
  - exact I.
  - destruct (group_installed (world_installed w) group); exact I.
  - exact I.
  - contradiction.
Qed.

Lemma update_effect_maintains : forall effect w scope entries,
  maintains effect ->
  update_effect effect w (publish_at w scope entries) =
  publish_at (step_effect effect w)
    (fst (maintained_view effect w scope entries))
    (snd (maintained_view effect w scope entries)).
Proof.
  intros effect w scope entries Hmaintains.
  destruct effect; cbn [maintains] in Hmaintains; try contradiction;
    [reflexivity| | | | | ];
    cbn [update_effect publish_at]; rewrite bound_is_current_at; reflexivity.
Qed.

Lemma update_effect_unmaintained : forall effect w accelerator,
  maintains effect -> current_bound w accelerator = false ->
  update_effect effect w accelerator = accelerator.
Proof.
  intros effect w accelerator Hmaintains Hcurrent.
  destruct effect; cbn [maintains] in Hmaintains; try contradiction;
    [reflexivity| | | | | ];
    destruct accelerator as [|base scope entries|bound lineage scope entries
      artifacts]; try reflexivity;
    cbn [current_bound] in Hcurrent; cbn [update_effect]; rewrite Hcurrent;
    reflexivity.
Qed.

Lemma update_preserves_exactness : forall operation w accelerator,
  not_reset operation -> roots_valid (world_roots w) ->
  exact_accelerator w accelerator ->
  exact_accelerator (step_world operation w)
    (update_accelerator operation w accelerator).
Proof.
  intros operation w accelerator Hreset Hroots
    [scope [entries [Haccelerator [Hcover Hentries]]]].
  subst accelerator. unfold update_accelerator.
  rewrite (update_effect_maintains (operation_effect operation w) w scope entries
    (operation_effect_maintains operation w Hreset)).
  exists (fst (maintained_view (operation_effect operation w) w scope entries)),
    (snd (maintained_view (operation_effect operation w) w scope entries)).
  split; [reflexivity|]. split.
  - apply maintained_view_covers; assumption.
  - apply maintained_view_scoped; assumption.
Qed.

Lemma bumps_decidable : forall effect, bumps effect \/ ~ bumps effect.
Proof.
  intro effect. destruct effect; cbn [bumps];
    first [left; exact I|right; intros []].
Qed.

Lemma insert_roots_valid : forall roots group record,
  roots_valid roots ->
  first_identity (continuation_identity record) (group_records roots group) = None ->
  roots_valid (insert_roots roots group record).
Proof.
  intros roots group record [Hkeys Hidentities] Hfirst. split.
  - unfold insert_roots. cbn [root_continuations].
    apply assoc_set_preserves_unique_keys. exact Hkeys.
  - intro other. destruct (group_eq_dec other group) as [Hsame|Hdifferent].
    + subst other. rewrite insert_roots_records_same, map_app. cbn [map].
      apply NoDup_app.
      * exact (Hidentities group).
      * constructor; [intros []|constructor].
      * intros identity Hin Hsingleton. apply in_singleton_iff in Hsingleton.
        subst identity. exact (first_identity_none_is_absent _ _ Hfirst Hin).
    + rewrite insert_roots_records_other by exact Hdifferent.
      exact (Hidentities other).
Qed.

Lemma remove_roots_valid : forall roots group record,
  roots_valid roots ->
  roots_valid (remove_roots roots group
    (remove_first_record record (group_records roots group))).
Proof.
  intros roots group record [Hkeys Hidentities]. split.
  - unfold remove_roots, put_group_records. cbn [root_continuations].
    destruct (remove_first_record record (group_records roots group));
      [apply assoc_remove_preserves_unique_keys|
       apply assoc_set_preserves_unique_keys]; exact Hkeys.
  - intro other. destruct (group_eq_dec other group) as [Hsame|Hdifferent].
    + subst other. rewrite remove_roots_records_same.
      apply remove_first_record_preserves_identities. exact (Hidentities group).
    + rewrite remove_roots_records_other by exact Hdifferent.
      exact (Hidentities other).
Qed.

Lemma step_world_valid : forall operation w,
  op_admissible operation -> world_valid w -> world_valid (step_world operation w).
Proof.
  intros operation w Hadmissible [Hroots [Hslots Htokens]].
  unfold step_world.
  split; [|split; [|apply step_effect_tokens; exact Htokens]].
  - destruct operation as [group record|group record|group record|group|group|
      target slots persisted]; cbn [operation_effect].
    + destruct (first_identity (continuation_identity record)
        (group_records (world_roots w) group)) eqn:Hfirst; [exact Hroots|].
      cbn [step_effect bump_group world_roots].
      apply insert_roots_valid; assumption.
    + destruct (in_dec continuation_record_eq_dec record
        (group_records (world_roots w) group)); [|exact Hroots].
      cbn [step_effect bump_group world_roots].
      apply remove_roots_valid. exact Hroots.
    + exact Hroots.
    + destruct (group_installed (world_installed w) group); exact Hroots.
    + exact Hroots.
    + destruct Hadmissible as [Htarget _]. exact Htarget.
  - destruct operation as [group record|group record|group record|group|group|
      target slots persisted]; cbn [operation_effect].
    + destruct (first_identity (continuation_identity record)
        (group_records (world_roots w) group)); exact Hslots.
    + destruct (in_dec continuation_record_eq_dec record
        (group_records (world_roots w) group)); exact Hslots.
    + cbn [step_effect bump_group world_installed]. unfold slots_valid.
      apply assoc_set_preserves_unique_keys. exact Hslots.
    + destruct (group_installed (world_installed w) group); [|exact Hslots].
      cbn [step_effect bump_group world_installed]. unfold slots_valid.
      apply assoc_remove_preserves_unique_keys. exact Hslots.
    + exact Hslots.
    + destruct Hadmissible as [_ [Hvalid_slots _]]. exact Hvalid_slots.
Qed.

Lemma held_revision_goes_stale : forall effect w revision,
  bumps effect -> revision_issued (world_issued w) revision ->
  revision <> world_revision (step_effect effect w) /\
  revision_issued (world_issued (step_effect effect w)) revision.
Proof.
  intros effect w revision Hbumps [Hgeneration [Hstore Hinstalled]].
  destruct (step_effect_fresh effect w Hbumps) as [Hfresh Hissued].
  split.
  - intro Hsame. apply (fresh_token_is_fresh w).
    rewrite <- Hfresh, <- Hsame. exact Hstore.
  - rewrite Hissued.
    split; [right; exact Hgeneration|split; right; assumption].
Qed.

Lemma invariant_after_bump : forall effect w accelerator,
  bumps effect -> accelerator_invariant w accelerator ->
  accelerator_invariant (step_effect effect w) accelerator.
Proof.
  intros effect w accelerator Hbumps Hinvariant.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; cbn [accelerator_invariant] in *; [exact I| |].
  - destruct Hinvariant as [Hissued _].
    destruct (held_revision_goes_stale effect w base Hbumps Hissued)
      as [Hstale Hissued'].
    split; [exact Hissued'|intro Hsame; contradiction].
  - destruct Hinvariant as [Hissued _].
    destruct (held_revision_goes_stale effect w bound Hbumps Hissued)
      as [Hstale Hissued'].
    split; [exact Hissued'|intros Hsame _; contradiction].
Qed.

Lemma publish_at_invariant : forall w scope entries,
  world_tokens_issued w ->
  same_members entries (scoped_entries w scope) ->
  accelerator_invariant w (publish_at w scope entries).
Proof.
  intros w scope entries [Hrevision _] Hentries. unfold publish_at.
  cbn [accelerator_invariant].
  split; [exact Hrevision|intros _ _; exact Hentries].
Qed.

Lemma installed_targets_nil : forall group, installed_targets [] group = [].
Proof. reflexivity. Qed.

Lemma installed_entries_spec : forall slots occurrence,
  In occurrence (installed_entries slots) <->
  In (occurrence_group occurrence) (map fst slots) /\
  In (occurrence_target occurrence)
    (installed_targets slots (occurrence_group occurrence)) /\
  occurrence_shape occurrence.
Proof.
  intros slots occurrence. unfold installed_entries. rewrite in_flat_map. split.
  - intros [group [Hgroup Hin]]. rewrite in_flat_map in Hin.
    destruct Hin as [target [Htarget Hin]].
    rewrite target_occurrences_spec in Hin.
    destruct Hin as [Htarget_same [Hgroup_same Hshape]].
    rewrite Hgroup_same, Htarget_same.
    split; [exact Hgroup|split; [exact Htarget|exact Hshape]].
  - intros [Hgroup [Htarget Hshape]].
    exists (occurrence_group occurrence). split; [exact Hgroup|].
    rewrite in_flat_map. exists (occurrence_target occurrence).
    split; [exact Htarget|]. rewrite target_occurrences_spec.
    split; [reflexivity|split; [reflexivity|exact Hshape]].
Qed.

Lemma adopted_entries_exact : forall w target slots stored,
  persisted_valid stored -> persisted_lineage stored = target ->
  covers (persisted_scope stored ++ map fst slots) (reset_world w target slots) /\
  same_members (persisted_entries stored ++ installed_entries slots)
    (scoped_entries (reset_world w target slots)
      (persisted_scope stored ++ map fst slots)).
Proof.
  intros w target slots stored [Hscope Hentries] Hlineage.
  rewrite Hlineage in Hscope, Hentries. split.
  - intros group Hnonempty. apply in_or_app.
    unfold targets_at, group_targets in Hnonempty.
    cbn [reset_world world_roots world_installed] in Hnonempty.
    destruct (group_installed slots group) as [record|] eqn:Hslot.
    + right. eapply assoc_lookup_some_key. exact Hslot.
    + left. apply Hscope. intro Hempty. apply Hnonempty.
      unfold installed_targets. rewrite Hslot, Hempty. reflexivity.
  - intro occurrence.
    rewrite in_app_iff, (Hentries occurrence), installed_entries_spec.
    unfold scoped_entries. rewrite !scoped_entries_spec.
    cbn [reset_world world_roots world_installed]. rewrite in_app_iff. split.
    + intros [[Hgroup [Htarget Hshape]]|[Hgroup [Htarget Hshape]]].
      * split; [left; exact Hgroup|split; [|exact Hshape]].
        unfold group_targets in Htarget |- *.
        rewrite installed_targets_nil in Htarget.
        apply in_or_app. right. exact Htarget.
      * split; [right; exact Hgroup|split; [|exact Hshape]].
        unfold group_targets. apply in_or_app. left. exact Htarget.
    + intros [Hgroup [Htarget Hshape]].
      unfold group_targets in Htarget. apply in_app_or in Htarget.
      destruct Htarget as [Hinstalled|Hresting].
      * right. split; [|split; [exact Hinstalled|exact Hshape]].
        unfold installed_targets in Hinstalled. revert Hinstalled.
        destruct (group_installed slots (occurrence_group occurrence))
          as [record|] eqn:Hslot; intro Hinstalled; [|destruct Hinstalled].
        eapply assoc_lookup_some_key. exact Hslot.
      * left. split; [|split; [|exact Hshape]].
        -- apply Hscope. intro Hempty. rewrite Hempty in Hresting.
           destruct Hresting.
        -- unfold group_targets. rewrite installed_targets_nil. exact Hresting.
Qed.

Lemma commit_preserves_invariant_maintained : forall operation w accelerator,
  not_reset operation -> world_valid w -> accelerator_invariant w accelerator ->
  accelerator_invariant (step_world operation w)
    (update_accelerator operation w accelerator).
Proof.
  intros operation w accelerator Hreset Hvalid Hinvariant.
  assert (Hadmissible : op_admissible operation)
    by (destruct operation; cbn [not_reset] in Hreset;
          [exact I|exact I|exact I|exact I|exact I|contradiction]).
  destruct (step_world_valid operation w Hadmissible Hvalid)
    as [_ [_ Htokens']].
  destruct Hvalid as [Hroots [Hslots Htokens]].
  pose proof (operation_effect_maintains operation w Hreset) as Hmaintains.
  unfold update_accelerator.
  destruct (current_bound w accelerator) eqn:Hcurrent.
  - destruct (current_bound_spec w accelerator Hcurrent)
      as [scope [entries Haccelerator]].
    subst accelerator.
    rewrite (update_effect_maintains _ w scope entries Hmaintains).
    apply publish_at_invariant; [exact Htokens'|].
    unfold publish_at in Hinvariant. cbn [accelerator_invariant] in Hinvariant.
    destruct Hinvariant as [_ Hentries].
    apply maintained_view_scoped; [exact Hreset|exact Hroots|].
    apply Hentries; reflexivity.
  - rewrite (update_effect_unmaintained _ w accelerator Hmaintains Hcurrent).
    unfold step_world.
    destruct (bumps_decidable (operation_effect operation w))
      as [Hbumps|Hquiet].
    + apply invariant_after_bump; assumption.
    + rewrite step_effect_quiet by exact Hquiet. exact Hinvariant.
Qed.

Lemma commit_preserves_invariant_reset :
    forall target slots persisted w accelerator,
  op_admissible (ResetState target slots persisted) -> world_valid w ->
  accelerator_invariant w accelerator ->
  accelerator_invariant (step_world (ResetState target slots persisted) w)
    (update_accelerator (ResetState target slots persisted) w accelerator).
Proof.
  intros target slots persisted w accelerator Hadmissible Hvalid Hinvariant.
  destruct (step_world_valid _ w Hadmissible Hvalid) as [_ [_ Htokens']].
  destruct Hadmissible as [_ [_ Hpersisted]].
  unfold update_accelerator, step_world.
  cbn [operation_effect update_effect].
  destruct persisted as [stored|].
  - destruct (semantic_roots_eq_dec (persisted_lineage stored) target)
      as [Hlineage|Hlineage].
    + apply publish_at_invariant; [exact Htokens'|].
      exact (proj2 (adopted_entries_exact w target slots stored Hpersisted
        Hlineage)).
    + apply invariant_after_bump; [exact I|exact Hinvariant].
  - apply invariant_after_bump; [exact I|exact Hinvariant].
Qed.

Lemma commit_preserves_invariant : forall operation w accelerator,
  op_admissible operation -> world_valid w ->
  accelerator_invariant w accelerator ->
  accelerator_invariant (step_world operation w)
    (update_accelerator operation w accelerator).
Proof.
  intros operation w accelerator Hadmissible Hvalid Hinvariant.
  destruct operation as [group record|group record|group record|group|group|
    target slots persisted];
    try (apply commit_preserves_invariant_maintained;
         [exact I|exact Hvalid|exact Hinvariant]).
  apply commit_preserves_invariant_reset; assumption.
Qed.

Lemma unmaintained_step_invariant : forall operation w accelerator,
  accelerator_invariant w accelerator ->
  accelerator_invariant (step_world operation w) accelerator.
Proof.
  intros operation w accelerator Hinvariant. unfold step_world.
  destruct (bumps_decidable (operation_effect operation w))
    as [Hbumps|Hquiet].
  - apply invariant_after_bump; assumption.
  - rewrite step_effect_quiet by exact Hquiet. exact Hinvariant.
Qed.

Lemma begin_build_invariant : forall w,
  world_tokens_issued w -> accelerator_invariant w (begin_build w).
Proof.
  intros w [Hrevision _]. unfold begin_build. cbn [accelerator_invariant].
  split; [exact Hrevision|]. intros _ occurrence. reflexivity.
Qed.

Lemma build_group_invariant : forall w group accelerator,
  accelerator_invariant w accelerator ->
  accelerator_invariant w (build_group w group accelerator).
Proof.
  intros w group accelerator Hinvariant.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; cbn [build_group]; [exact Hinvariant| |exact Hinvariant].
  destruct (revision_eq_dec base (world_revision w)) as [Hbase|Hbase];
    [|exact Hinvariant].
  destruct (in_dec group_eq_dec group scope); [exact Hinvariant|].
  cbn [accelerator_invariant] in *. destruct Hinvariant as [Hissued Hentries].
  split; [exact Hissued|]. intros _.
  rewrite scoped_entries_extend.
  apply same_members_app; [apply Hentries; exact Hbase|apply same_members_refl].
Qed.

Lemma publish_build_invariant : forall w accelerator,
  accelerator_invariant w accelerator ->
  accelerator_invariant w (publish_build w accelerator).
Proof.
  intros w accelerator Hinvariant.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; cbn [publish_build]; [exact Hinvariant| |exact Hinvariant].
  destruct (revision_eq_dec base (world_revision w)) as [Hbase|Hbase];
    [|exact Hinvariant].
  destruct (coversb scope w); [|exact Hinvariant].
  cbn [accelerator_invariant] in Hinvariant.
  destruct Hinvariant as [Hissued Hentries].
  unfold publish_at. cbn [accelerator_invariant].
  split; [rewrite <- Hbase; exact Hissued|].
  intros _ _. apply Hentries. exact Hbase.
Qed.

Lemma abort_build_invariant : forall w accelerator,
  accelerator_invariant w accelerator ->
  accelerator_invariant w (abort_build accelerator).
Proof.
  intros w accelerator Hinvariant.
  destruct accelerator; cbn [abort_build];
    [exact Hinvariant|exact I|exact Hinvariant].
Qed.

Lemma final_removal_preserves_joins_cover : forall roots group,
  joins_cover roots -> joins_cover (remove_roots roots group []).
Proof.
  intros roots group Hcover channel other.
  assert (Hjoins : resting_joins_of (remove_roots roots group []) channel =
    assoc_get path_eq_dec [] (join_remove_channels (root_joins roots) group group)
      channel) by reflexivity.
  rewrite Hjoins, join_remove_channels_spec.
  change (assoc_get path_eq_dec [] (root_joins roots) channel)
    with (resting_joins_of roots channel).
  rewrite (Hcover channel other).
  destruct (group_eq_dec other group) as [Hsame|Hdifferent].
  - subst other. rewrite remove_roots_records_same. split.
    + intros [[Hin _] Hnot]. exfalso. apply Hnot. split; [reflexivity|exact Hin].
    + intros [_ Hnonempty]. exfalso. apply Hnonempty. reflexivity.
  - rewrite remove_roots_records_other by exact Hdifferent. split.
    + intros [Hjoin _]. exact Hjoin.
    + intro Hjoin. split; [exact Hjoin|]. intros [Hsame _]. contradiction.
Qed.

Lemma insertion_preserves_joins_cover : forall roots group record,
  joins_cover roots -> joins_cover (insert_roots roots group record).
Proof.
  intros roots group record Hcover channel other.
  assert (Hjoins : resting_joins_of (insert_roots roots group record) channel =
    assoc_get path_eq_dec [] (join_add_channels (root_joins roots) group group)
      channel) by reflexivity.
  rewrite Hjoins, join_add_channels_spec.
  change (assoc_get path_eq_dec [] (root_joins roots) channel)
    with (resting_joins_of roots channel).
  rewrite (Hcover channel other).
  destruct (group_eq_dec other group) as [Hsame|Hdifferent].
  - subst other. rewrite insert_roots_records_same. split.
    + intros [[Hin _]|[_ Hin]]; (split; [exact Hin|]);
        intro Hempty; apply app_eq_nil in Hempty;
        destruct Hempty as [_ Hsingleton]; discriminate.
    + intros [Hin _]. right. split; [reflexivity|exact Hin].
  - rewrite insert_roots_records_other by exact Hdifferent. split.
    + intros [Hjoin|[Hsame _]]; [exact Hjoin|contradiction].
    + intro Hjoin. left. exact Hjoin.
Qed.

Theorem insertion_update_equals_rebuild : forall w accelerator group record,
  world_valid w -> exact_accelerator w accelerator ->
  first_identity (continuation_identity record)
    (group_records (world_roots w) group) = None ->
  exact_accelerator (step_world (InsertResting group record) w)
    (update_accelerator (InsertResting group record) w accelerator) /\
  classify_accelerator (step_world (InsertResting group record) w)
    (update_accelerator (InsertResting group record) w accelerator) =
    ClassCoherent /\
  (forall channel, same_members
     (accelerator_read (step_world (InsertResting group record) w)
        (update_accelerator (InsertResting group record) w accelerator) channel)
     (accelerator_read (step_world (InsertResting group record) w)
        (build_accelerator (step_world (InsertResting group record) w))
        channel)) /\
  In (resting_target group record)
    (targets_at (step_world (InsertResting group record) w) group) /\
  (joins_cover (world_roots w) ->
   joins_cover (world_roots (step_world (InsertResting group record) w))).
Proof.
  intros w accelerator group record [Hroots _] Hexact Hfirst.
  assert (Hupdated := update_preserves_exactness (InsertResting group record)
    w accelerator I Hroots Hexact).
  destruct (exact_accelerator_reads _ _ Hupdated) as [Hclass [_ Hreads]].
  destruct (exact_accelerator_reads _ _ (build_accelerator_exact
    (step_world (InsertResting group record) w))) as [_ [_ Hbuild]].
  assert (Hstep : step_world (InsertResting group record) w =
    step_effect (Inserted group record) w).
  { unfold step_world. cbn [operation_effect]. rewrite Hfirst. reflexivity. }
  split; [exact Hupdated|]. split; [exact Hclass|]. split.
  { intro channel. eapply same_members_trans; [apply Hreads|].
    apply same_members_sym. apply Hbuild. }
  split.
  - rewrite Hstep. apply insert_targets_spec. right. reflexivity.
  - intro Hcover. rewrite Hstep. cbn [step_effect bump_group world_roots].
    apply insertion_preserves_joins_cover. exact Hcover.
Qed.

Theorem nonfinal_removal_update_equals_rebuild :
    forall w accelerator group record,
  world_valid w -> exact_accelerator w accelerator ->
  In record (group_records (world_roots w) group) ->
  remove_first_record record (group_records (world_roots w) group) <> [] ->
  exact_accelerator (step_world (RemoveResting group record) w)
    (update_accelerator (RemoveResting group record) w accelerator) /\
  classify_accelerator (step_world (RemoveResting group record) w)
    (update_accelerator (RemoveResting group record) w accelerator) =
    ClassCoherent /\
  (forall channel, same_members
     (accelerator_read (step_world (RemoveResting group record) w)
        (update_accelerator (RemoveResting group record) w accelerator) channel)
     (accelerator_read (step_world (RemoveResting group record) w)
        (build_accelerator (step_world (RemoveResting group record) w))
        channel)) /\
  root_joins (world_roots (step_world (RemoveResting group record) w)) =
    root_joins (world_roots w) /\
  ~ In (resting_target group record)
      (targets_at (step_world (RemoveResting group record) w) group).
Proof.
  intros w accelerator group record [Hroots _] Hexact Hin Hremaining.
  assert (Hupdated := update_preserves_exactness (RemoveResting group record)
    w accelerator I Hroots Hexact).
  destruct (exact_accelerator_reads _ _ Hupdated) as [Hclass [_ Hreads]].
  destruct (exact_accelerator_reads _ _ (build_accelerator_exact
    (step_world (RemoveResting group record) w))) as [_ [_ Hbuild]].
  assert (Hunique : NoDup (group_records (world_roots w) group))
    by (eapply NoDup_map_inv; exact (proj2 Hroots group)).
  assert (Hstep : step_world (RemoveResting group record) w =
    step_effect (Removed group record
      (remove_first_record record (group_records (world_roots w) group))) w).
  { unfold step_world. cbn [operation_effect].
    destruct (in_dec continuation_record_eq_dec record
      (group_records (world_roots w) group)) as [_|Hnotin];
      [reflexivity|contradiction]. }
  split; [exact Hupdated|]. split; [exact Hclass|]. split.
  { intro channel. eapply same_members_trans; [apply Hreads|].
    apply same_members_sym. apply Hbuild. }
  split.
  - rewrite Hstep. cbn [step_effect bump_group world_roots].
    unfold remove_roots. cbn [root_joins].
    destruct (remove_first_record record (group_records (world_roots w) group))
      as [|head tail] eqn:Hrest;
      [exfalso; apply Hremaining; reflexivity|reflexivity].
  - rewrite Hstep. intro Htarget.
    destruct (proj1 (remove_targets_spec w group record _ Hunique) Htarget)
      as [_ Hne].
    apply Hne. reflexivity.
Qed.

Theorem final_removal_update_equals_rebuild :
    forall w accelerator group record,
  world_valid w -> exact_accelerator w accelerator ->
  joins_cover (world_roots w) ->
  group_records (world_roots w) group = [record] ->
  exact_accelerator (step_world (RemoveResting group record) w)
    (update_accelerator (RemoveResting group record) w accelerator) /\
  classify_accelerator (step_world (RemoveResting group record) w)
    (update_accelerator (RemoveResting group record) w accelerator) =
    ClassCoherent /\
  (forall channel, same_members
     (accelerator_read (step_world (RemoveResting group record) w)
        (update_accelerator (RemoveResting group record) w accelerator) channel)
     (accelerator_read (step_world (RemoveResting group record) w)
        (build_accelerator (step_world (RemoveResting group record) w))
        channel)) /\
  joins_cover (world_roots (step_world (RemoveResting group record) w)) /\
  group_records (world_roots (step_world (RemoveResting group record) w))
    group = [] /\
  (forall channel occurrence,
     In occurrence (derive_world (step_world (RemoveResting group record) w)
       channel) ->
     occurrence_group occurrence = group ->
     receive_target_kind (occurrence_target occurrence) = InstalledKind) /\
  (group_installed (world_installed w) group = None ->
   forall channel occurrence,
     In occurrence (accelerator_read (step_world (RemoveResting group record) w)
       (update_accelerator (RemoveResting group record) w accelerator) channel) ->
     occurrence_group occurrence <> group).
Proof.
  intros w accelerator group record [Hroots _] Hexact Hcover Hrecords.
  assert (Hupdated := update_preserves_exactness (RemoveResting group record)
    w accelerator I Hroots Hexact).
  destruct (exact_accelerator_reads _ _ Hupdated) as [Hclass [_ Hreads]].
  destruct (exact_accelerator_reads _ _ (build_accelerator_exact
    (step_world (RemoveResting group record) w))) as [_ [_ Hbuild]].
  assert (Hremaining : remove_first_record record
    (group_records (world_roots w) group) = []).
  { rewrite Hrecords. cbn [remove_first_record].
    destruct (continuation_record_eq_dec record record) as [_|Hne];
      [reflexivity|exfalso; apply Hne; reflexivity]. }
  assert (Hstep : step_world (RemoveResting group record) w =
    step_effect (Removed group record []) w).
  { unfold step_world. cbn [operation_effect].
    destruct (in_dec continuation_record_eq_dec record
      (group_records (world_roots w) group)) as [_|Hnotin].
    - rewrite Hremaining. reflexivity.
    - exfalso. apply Hnotin. rewrite Hrecords. left. reflexivity. }
  assert (Htargets : forall target,
    In target (targets_at (step_world (RemoveResting group record) w) group) ->
    In target (installed_targets (world_installed w) group)).
  { intros target Htarget. rewrite Hstep in Htarget. unfold targets_at in Htarget.
    cbn [step_effect bump_group world_roots world_installed] in Htarget.
    rewrite targets_after_remove_same in Htarget. cbn [map] in Htarget.
    rewrite app_nil_r in Htarget. exact Htarget. }
  split; [exact Hupdated|]. split; [exact Hclass|]. split.
  { intro channel. eapply same_members_trans; [apply Hreads|].
    apply same_members_sym. apply Hbuild. }
  split.
  { rewrite Hstep. cbn [step_effect bump_group world_roots].
    apply final_removal_preserves_joins_cover. exact Hcover. }
  split.
  { rewrite Hstep. cbn [step_effect bump_group world_roots].
    apply remove_roots_records_same. }
  split.
  - intros channel occurrence Hin Hgroup. unfold derive_world in Hin.
    rewrite derive_occurrences_spec in Hin. destruct Hin as [_ [Htarget _]].
    rewrite Hgroup in Htarget.
    pose proof (Htargets _ Htarget) as Hinstalled.
    unfold installed_targets in Hinstalled. revert Hinstalled.
    destruct (group_installed (world_installed w) group) as [installed|];
      intro Hinstalled; [|destruct Hinstalled].
    destruct Hinstalled as [Hsame|[]]. rewrite <- Hsame. reflexivity.
  - intros Hnone channel occurrence Hin Hgroup.
    pose proof (proj1 (Hreads channel occurrence) Hin) as Hderived.
    unfold derive_world in Hderived.
    rewrite derive_occurrences_spec in Hderived.
    destruct Hderived as [_ [Htarget _]]. rewrite Hgroup in Htarget.
    pose proof (Htargets _ Htarget) as Hinstalled.
    unfold installed_targets in Hinstalled. rewrite Hnone in Hinstalled.
    destruct Hinstalled.
Qed.

Lemma first_identity_present : forall record records,
  In record records ->
  exists prior, first_identity (continuation_identity record) records = Some prior.
Proof.
  intros record records. induction records as [|head tail IH];
    intro Hin; [destruct Hin|].
  cbn [first_identity].
  destruct (path_eq_dec (continuation_identity head)
    (continuation_identity record)) as [Hsame|Hdifferent].
  - exists head. reflexivity.
  - destruct Hin as [Hhead|Htail].
    + subst head. exfalso. apply Hdifferent. reflexivity.
    + apply IH. exact Htail.
Qed.

Theorem install_and_uninstall_update_equals_rebuild :
  (forall w accelerator group record,
     world_valid w -> exact_accelerator w accelerator ->
     exact_accelerator (step_world (Install group record) w)
       (update_accelerator (Install group record) w accelerator) /\
     (forall channel, same_members
        (accelerator_read (step_world (Install group record) w)
           (update_accelerator (Install group record) w accelerator) channel)
        (accelerator_read (step_world (Install group record) w)
           (build_accelerator (step_world (Install group record) w)) channel)) /\
     revision_store (world_revision (step_world (Install group record) w)) =
       fresh_token w /\
     revision_installed (world_revision (step_world (Install group record) w)) =
       fresh_token w /\
     ~ In (fresh_token w) (world_issued w) /\
     In (installed_target group record)
       (targets_at (step_world (Install group record) w) group) /\
     (forall resting, In resting (group_records (world_roots w) group) ->
        In (resting_target group resting)
          (targets_at (step_world (Install group record) w) group)) /\
     installed_target group record <> resting_target group record) /\
  (forall w accelerator group installed,
     world_valid w -> exact_accelerator w accelerator ->
     group_installed (world_installed w) group = Some installed ->
     exact_accelerator (step_world (Uninstall group) w)
       (update_accelerator (Uninstall group) w accelerator) /\
     (forall channel, same_members
        (accelerator_read (step_world (Uninstall group) w)
           (update_accelerator (Uninstall group) w accelerator) channel)
        (accelerator_read (step_world (Uninstall group) w)
           (build_accelerator (step_world (Uninstall group) w)) channel)) /\
     revision_store (world_revision (step_world (Uninstall group) w)) =
       fresh_token w /\
     revision_installed (world_revision (step_world (Uninstall group) w)) =
       fresh_token w /\
     ~ In (installed_target group installed)
         (targets_at (step_world (Uninstall group) w) group) /\
     (forall resting, In resting (group_records (world_roots w) group) ->
        In (resting_target group resting)
          (targets_at (step_world (Uninstall group) w) group))).
Proof.
  split.
  - intros w accelerator group record [Hroots _] Hexact.
    assert (Hupdated := update_preserves_exactness (Install group record)
      w accelerator I Hroots Hexact).
    destruct (exact_accelerator_reads _ _ Hupdated) as [_ [_ Hreads]].
    destruct (exact_accelerator_reads _ _ (build_accelerator_exact
      (step_world (Install group record) w))) as [_ [_ Hbuild]].
    change (step_world (Install group record) w)
      with (step_effect (Installed group record) w) in *.
    split; [exact Hupdated|]. split.
    { intro channel. eapply same_members_trans; [apply Hreads|].
      apply same_members_sym. apply Hbuild. }
    split; [reflexivity|]. split; [reflexivity|].
    split; [apply fresh_token_is_fresh|]. split.
    { apply install_targets_spec. left. reflexivity. }
    split.
    { intros resting Hresting. apply install_targets_spec. right.
      apply resting_part_spec. apply in_map. exact Hresting. }
    unfold installed_target, resting_target. discriminate.
  - intros w accelerator group installed [Hroots _] Hexact Hslot.
    assert (Hupdated := update_preserves_exactness (Uninstall group)
      w accelerator I Hroots Hexact).
    destruct (exact_accelerator_reads _ _ Hupdated) as [_ [_ Hreads]].
    destruct (exact_accelerator_reads _ _ (build_accelerator_exact
      (step_world (Uninstall group) w))) as [_ [_ Hbuild]].
    assert (Hstep : step_world (Uninstall group) w =
      step_effect (Uninstalled group) w).
    { unfold step_world. cbn [operation_effect]. rewrite Hslot. reflexivity. }
    split; [exact Hupdated|]. split.
    { intro channel. eapply same_members_trans; [apply Hreads|].
      apply same_members_sym. apply Hbuild. }
    rewrite Hstep. split; [reflexivity|]. split; [reflexivity|]. split.
    + intro Htarget. apply uninstall_targets_spec in Htarget.
      destruct Htarget as [_ Hfalse].
      unfold installed_in_group, installed_target in Hfalse.
      cbn [receive_target_kind receive_target_group] in Hfalse.
      destruct (group_eq_dec group group) as [_|Hne];
        [discriminate|apply Hne; reflexivity].
    + intros resting Hresting. apply uninstall_targets_spec.
      apply resting_part_spec. apply in_map. exact Hresting.
Qed.

Theorem history_load_and_byte_equal_replacement_preserve_derivation :
  (forall w group, step_world (HistoryLoad group) w = w) /\
  (forall w accelerator group,
     world_valid w -> exact_accelerator w accelerator ->
     exact_accelerator w (update_accelerator (HistoryLoad group) w accelerator)) /\
  (forall w scope entries group,
     same_members entries (scoped_entries w scope) ->
     same_members
       (entries ++ group_entries (world_roots w) (world_installed w) group)
       (scoped_entries w (scope ++ [group]))) /\
  (forall w accelerator group record,
     In record (group_records (world_roots w) group) ->
     step_world (InsertResting group record) w = w /\
     update_accelerator (InsertResting group record) w accelerator =
       accelerator).
Proof.
  split; [intros w group; reflexivity|]. split.
  - intros w accelerator group [Hroots _] Hexact.
    exact (update_preserves_exactness (HistoryLoad group) w accelerator I
      Hroots Hexact).
  - split.
    + intros w scope entries group Hentries. rewrite scoped_entries_extend.
      apply same_members_app; [exact Hentries|apply same_members_refl].
    + intros w accelerator group record Hin.
      destruct (first_identity_present record _ Hin) as [prior Hprior].
      unfold step_world, update_accelerator. cbn [operation_effect].
      rewrite Hprior. split; reflexivity.
Qed.

Theorem reset_issues_fresh_generation_and_rebuilds :
  (forall w target slots persisted,
     ~ In (revision_generation (world_revision
         (step_world (ResetState target slots persisted) w))) (world_issued w) /\
     ~ In (revision_store (world_revision
         (step_world (ResetState target slots persisted) w))) (world_issued w) /\
     ~ In (revision_installed (world_revision
         (step_world (ResetState target slots persisted) w))) (world_issued w) /\
     world_manifest (step_world (ResetState target slots persisted) w) = target) /\
  (forall w target slots persisted bound lineage scope entries artifacts,
     In (revision_generation bound) (world_issued w) ->
     classify_accelerator (step_world (ResetState target slots persisted) w)
       (AcceleratorPublished bound lineage scope entries artifacts) =
       ClassForeign) /\
  (forall w target slots persisted,
     exact_accelerator (step_world (ResetState target slots persisted) w)
       (build_accelerator (step_world (ResetState target slots persisted) w))) /\
  (forall w accelerator target slots stored,
     persisted_valid stored -> persisted_lineage stored = target ->
     exact_accelerator (step_world (ResetState target slots (Some stored)) w)
       (update_accelerator (ResetState target slots (Some stored)) w
          accelerator)) /\
  (forall w accelerator target slots stored,
     persisted_lineage stored <> target ->
     update_accelerator (ResetState target slots (Some stored)) w accelerator =
       accelerator) /\
  (forall w accelerator target slots,
     update_accelerator (ResetState target slots None) w accelerator =
       accelerator).
Proof.
  split.
  { intros w target slots persisted.
    split; [apply fresh_token_is_fresh|].
    split; [apply fresh_token_is_fresh|].
    split; [apply fresh_token_is_fresh|reflexivity]. }
  split.
  { intros w target slots persisted bound lineage scope entries artifacts
      Hissued.
    unfold step_world.
    cbn [operation_effect step_effect reset_world classify_accelerator
      world_manifest world_revision revision_generation].
    destruct (semantic_roots_eq_dec lineage target) as [_|_]; [|reflexivity].
    destruct (EqualityEpoch.eq_dec (revision_generation bound) (fresh_token w))
      as [Hsame|_]; [|reflexivity].
    exfalso. apply (fresh_token_is_fresh w). rewrite <- Hsame. exact Hissued. }
  split.
  { intros w target slots persisted. apply build_accelerator_exact. }
  split.
  { intros w accelerator target slots stored Hvalid Hlineage.
    unfold update_accelerator, step_world.
    cbn [operation_effect update_effect].
    destruct (semantic_roots_eq_dec (persisted_lineage stored) target)
      as [_|Hne]; [|contradiction].
    destruct (adopted_entries_exact w target slots stored Hvalid Hlineage)
      as [Hcover Hentries].
    exists (persisted_scope stored ++ map fst slots),
      (persisted_entries stored ++ installed_entries slots).
    split; [reflexivity|]. split; [exact Hcover|exact Hentries]. }
  split.
  { intros w accelerator target slots stored Hne.
    unfold update_accelerator. cbn [operation_effect update_effect].
    destruct (semantic_roots_eq_dec (persisted_lineage stored) target)
      as [Hsame|_]; [contradiction|reflexivity]. }
  intros w accelerator target slots. reflexivity.
Qed.

Inductive WorldRun (initial : World) : World -> Prop :=
| WorldRunInitial : WorldRun initial initial
| WorldRunStep : forall current operation,
    WorldRun initial current -> op_admissible operation ->
    WorldRun initial (step_world operation current).

Inductive AcceleratorTransition
    : World * AcceleratorState -> World * AcceleratorState -> Prop :=
| TransitionCommit : forall w accelerator operation fault,
    op_admissible operation ->
    AcceleratorTransition (w, accelerator)
      (commit_operation operation w accelerator fault)
| TransitionUnmaintained : forall w accelerator operation,
    op_admissible operation ->
    AcceleratorTransition (w, accelerator) (step_world operation w, accelerator)
| TransitionBegin : forall w accelerator,
    AcceleratorTransition (w, accelerator) (w, begin_build w)
| TransitionBuildGroup : forall w accelerator group,
    AcceleratorTransition (w, accelerator) (w, build_group w group accelerator)
| TransitionPublish : forall w accelerator,
    AcceleratorTransition (w, accelerator) (w, publish_build w accelerator)
| TransitionAbort : forall w accelerator,
    AcceleratorTransition (w, accelerator) (w, abort_build accelerator)
| TransitionDrop : forall w accelerator,
    AcceleratorTransition (w, accelerator) (w, AcceleratorAbsent).

Inductive Reachable (initial : World) : World * AcceleratorState -> Prop :=
| ReachableInitial : Reachable initial (initial, AcceleratorAbsent)
| ReachableStep : forall source target,
    Reachable initial source -> AcceleratorTransition source target ->
    Reachable initial target.

Lemma reachable_invariant : forall initial state,
  world_valid initial -> Reachable initial state ->
  world_valid (fst state) /\ accelerator_invariant (fst state) (snd state).
Proof.
  intros initial state Hvalid Hreachable.
  induction Hreachable as [|source target Hsource IH Htransition].
  - split; [exact Hvalid|exact I].
  - destruct IH as [Hworld Hinvariant]. revert Hworld Hinvariant.
    destruct Htransition as [w accelerator operation fault Hadmissible|
      w accelerator operation Hadmissible|w accelerator|w accelerator group|
      w accelerator|w accelerator|w accelerator];
      cbn [fst snd]; intros Hworld Hinvariant.
    + unfold commit_operation. destruct fault; cbn [fst snd].
      * split; [exact Hworld|exact Hinvariant].
      * split; [apply step_world_valid; assumption|].
        apply commit_preserves_invariant; assumption.
    + split; [apply step_world_valid; assumption|].
      apply unmaintained_step_invariant. exact Hinvariant.
    + split; [exact Hworld|]. apply begin_build_invariant.
      destruct Hworld as [_ [_ Htokens]]. exact Htokens.
    + split; [exact Hworld|]. apply build_group_invariant. exact Hinvariant.
    + split; [exact Hworld|]. apply publish_build_invariant. exact Hinvariant.
    + split; [exact Hworld|]. apply abort_build_invariant. exact Hinvariant.
    + split; [exact Hworld|exact I].
Qed.

Lemma classify_coherent_spec : forall w bound lineage scope entries artifacts,
  classify_accelerator w
    (AcceleratorPublished bound lineage scope entries artifacts) = ClassCoherent ->
  lineage = world_manifest w /\ bound = world_revision w /\
  artifacts = artifacts_at bound /\ covers scope w.
Proof.
  intros w bound lineage scope entries artifacts Hclass.
  cbn [classify_accelerator] in Hclass. revert Hclass.
  destruct (semantic_roots_eq_dec lineage (world_manifest w)) as [Hlineage|];
    [|intro Hfalse; discriminate].
  destruct (EqualityEpoch.eq_dec (revision_generation bound)
    (revision_generation (world_revision w))) as [_|];
    [|intro Hfalse; discriminate].
  destruct (revision_eq_dec bound (world_revision w)) as [Hbound|];
    [|intro Hfalse; discriminate].
  destruct (artifacts_eq_dec artifacts (artifacts_at bound)) as [Hartifacts|];
    [|intro Hfalse; discriminate].
  destruct (coversb scope w) eqn:Hcover; [|intro Hfalse; discriminate].
  intros _. split; [exact Hlineage|]. split; [exact Hbound|].
  split; [exact Hartifacts|]. apply coversb_spec. exact Hcover.
Qed.

Lemma read_not_coherent : forall w accelerator channel,
  classify_accelerator w accelerator <> ClassCoherent ->
  accelerator_read w accelerator channel = derive_world w channel.
Proof.
  intros w accelerator channel Hclass.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; [reflexivity|reflexivity|].
  unfold accelerator_read, accelerator_answer. revert Hclass.
  destruct (classify_accelerator w
    (AcceleratorPublished bound lineage scope entries artifacts));
    intro Hclass; first [reflexivity|exfalso; apply Hclass; reflexivity].
Qed.

Lemma exact_answer_is_coherent : forall w accelerator channel occurrences,
  accelerator_answer w accelerator channel = ExactAnswer occurrences ->
  classify_accelerator w accelerator = ClassCoherent.
Proof.
  intros w accelerator channel occurrences Hanswer.
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; [discriminate|discriminate|].
  unfold accelerator_answer in Hanswer. revert Hanswer.
  destruct (classify_accelerator w
    (AcceleratorPublished bound lineage scope entries artifacts));
    intro Hanswer; first [reflexivity|discriminate].
Qed.

Lemma invariant_read_is_derivation : forall w accelerator channel,
  accelerator_invariant w accelerator ->
  same_members (accelerator_read w accelerator channel) (derive_world w channel).
Proof.
  intros w accelerator channel Hinvariant.
  destruct (classify_accelerator w accelerator) eqn:Hclass;
    try (rewrite read_not_coherent by (rewrite Hclass; discriminate);
         apply same_members_refl).
  destruct accelerator as [|base scope entries|bound lineage scope entries
    artifacts]; [discriminate|discriminate|].
  destruct (classify_coherent_spec _ _ _ _ _ _ Hclass)
    as [Hlineage [Hbound [_ Hcover]]].
  destruct Hinvariant as [_ Hentries].
  unfold accelerator_read, accelerator_answer. rewrite Hclass.
  cbn [answer_occurrences].
  apply lookup_scoped_entries_is_derivation with (scope := scope);
    [exact Hcover|apply Hentries; assumption].
Qed.

Lemma bump_group_epoch_same : forall w roots slots group changed,
  group_epoch (bump_group w roots slots group changed) group = fresh_token w.
Proof.
  intros w roots slots group changed. unfold group_epoch, bump_group.
  cbn [world_group_epochs world_base_epoch].
  rewrite assoc_lookup_set_same. reflexivity.
Qed.

Lemma bump_group_epoch_other : forall w roots slots group changed other,
  other <> group ->
  group_epoch (bump_group w roots slots group changed) other =
  group_epoch w other.
Proof.
  intros w roots slots group changed other Hother. unfold group_epoch, bump_group.
  cbn [world_group_epochs world_base_epoch].
  rewrite assoc_lookup_set_other by exact Hother. reflexivity.
Qed.

Lemma reset_world_epoch : forall w target slots group,
  group_epoch (reset_world w target slots) group = fresh_token w.
Proof. reflexivity. Qed.

Lemma group_installed_set_other : forall slots group record other,
  other <> group ->
  group_installed (assoc_set group_eq_dec slots group record) other =
  group_installed slots other.
Proof.
  intros slots group record other Hother. unfold group_installed.
  apply assoc_lookup_set_other. exact Hother.
Qed.

Lemma group_installed_remove_other : forall slots group other,
  other <> group ->
  group_installed (assoc_remove group_eq_dec slots group) other =
  group_installed slots other.
Proof.
  intros slots group other Hother. unfold group_installed.
  apply assoc_lookup_remove_other. exact Hother.
Qed.

Lemma world_run_issued : forall initial w token,
  WorldRun initial w -> In token (world_issued initial) ->
  In token (world_issued w).
Proof.
  intros initial w token Hrun Hin.
  induction Hrun as [|current operation Hrun IH Hadmissible]; [exact Hin|].
  unfold step_world. apply step_effect_issued_grows. exact IH.
Qed.

Lemma world_run_same_store : forall initial w,
  WorldRun initial w ->
  In (revision_store (world_revision initial)) (world_issued initial) ->
  revision_store (world_revision w) = revision_store (world_revision initial) ->
  world_roots w = world_roots initial /\ world_installed w = world_installed initial.
Proof.
  intros initial w Hrun Hissued.
  induction Hrun as [|current operation Hrun IH Hadmissible].
  - intros _. split; reflexivity.
  - intro Hsame. unfold step_world in Hsame |- *.
    destruct (bumps_decidable (operation_effect operation current))
      as [Hbumps|Hquiet].
    + exfalso.
      destruct (step_effect_fresh (operation_effect operation current) current
        Hbumps) as [Hfresh _].
      rewrite Hfresh in Hsame. apply (fresh_token_is_fresh current).
      rewrite Hsame. apply (world_run_issued initial current); assumption.
    + rewrite step_effect_quiet in Hsame |- * by exact Hquiet.
      apply IH. exact Hsame.
Qed.

Lemma world_run_same_group_epoch : forall initial w group,
  WorldRun initial w -> world_tokens_issued initial ->
  group_epoch w group = group_epoch initial group ->
  group_records (world_roots w) group = group_records (world_roots initial) group /\
  group_installed (world_installed w) group =
    group_installed (world_installed initial) group.
Proof.
  intros initial w group Hrun Htokens.
  assert (Hissued : In (group_epoch initial group) (world_issued initial))
    by (apply group_epoch_issued; exact Htokens).
  induction Hrun as [|current operation Hrun IH Hadmissible].
  - intros _. split; reflexivity.
  - intro Hsame.
    assert (Hcurrent : In (group_epoch initial group) (world_issued current))
      by (apply (world_run_issued initial current); assumption).
    assert (Hstale : forall token, token = fresh_token current ->
      token <> group_epoch initial group).
    { intros token Htoken Hequal. apply (fresh_token_is_fresh current).
      rewrite <- Htoken, Hequal. exact Hcurrent. }
    unfold step_world in Hsame |- *.
    destruct operation as [owner record|owner record|owner record|owner|owner|
      target slots persisted]; cbn [operation_effect] in Hsame |- *.
    + destruct (first_identity (continuation_identity record)
        (group_records (world_roots current) owner));
        [apply IH; exact Hsame|].
      cbn [step_effect] in Hsame |- *. cbn [bump_group world_roots world_installed].
      destruct (group_eq_dec group owner) as [Hgroup|Hgroup].
      * subst owner. rewrite bump_group_epoch_same in Hsame.
        exfalso. exact (Hstale _ eq_refl Hsame).
      * rewrite bump_group_epoch_other in Hsame by exact Hgroup.
        rewrite insert_roots_records_other by exact Hgroup.
        apply IH. exact Hsame.
    + destruct (in_dec continuation_record_eq_dec record
        (group_records (world_roots current) owner));
        [|apply IH; exact Hsame].
      cbn [step_effect] in Hsame |- *. cbn [bump_group world_roots world_installed].
      destruct (group_eq_dec group owner) as [Hgroup|Hgroup].
      * subst owner. rewrite bump_group_epoch_same in Hsame.
        exfalso. exact (Hstale _ eq_refl Hsame).
      * rewrite bump_group_epoch_other in Hsame by exact Hgroup.
        rewrite remove_roots_records_other by exact Hgroup.
        apply IH. exact Hsame.
    + cbn [step_effect] in Hsame |- *. cbn [bump_group world_roots world_installed].
      destruct (group_eq_dec group owner) as [Hgroup|Hgroup].
      * subst owner. rewrite bump_group_epoch_same in Hsame.
        exfalso. exact (Hstale _ eq_refl Hsame).
      * rewrite bump_group_epoch_other in Hsame by exact Hgroup.
        rewrite group_installed_set_other by exact Hgroup.
        apply IH. exact Hsame.
    + destruct (group_installed (world_installed current) owner);
        [|apply IH; exact Hsame].
      cbn [step_effect] in Hsame |- *. cbn [bump_group world_roots world_installed].
      destruct (group_eq_dec group owner) as [Hgroup|Hgroup].
      * subst owner. rewrite bump_group_epoch_same in Hsame.
        exfalso. exact (Hstale _ eq_refl Hsame).
      * rewrite bump_group_epoch_other in Hsame by exact Hgroup.
        rewrite group_installed_remove_other by exact Hgroup.
        apply IH. exact Hsame.
    + apply IH. exact Hsame.
    + cbn [step_effect] in Hsame. rewrite reset_world_epoch in Hsame.
      exfalso. exact (Hstale _ eq_refl Hsame).
Qed.

Lemma peek_in_i32_rangeb_complete : forall peek,
  peek_in_i32_range peek -> peek_in_i32_rangeb peek = true.
Proof.
  intros peek [Hlow Hhigh]. unfold peek_in_i32_rangeb.
  apply andb_true_intro. split; apply Z.leb_le; assumption.
Qed.

Lemma canonical_peeksb_complete : forall peeks,
  canonical_peeks peeks -> canonical_peeksb peeks = true.
Proof.
  induction peeks as [|peek tail IH]; intros [Hsorted Hrange]; [reflexivity|].
  destruct tail as [|next rest].
  - inversion Hrange as [|head_peek head_tail Hpeek Hrest]; subst.
    apply peek_in_i32_rangeb_complete. exact Hpeek.
  - assert (Hstep : canonical_peeksb (peek :: next :: rest) =
      peek_in_i32_rangeb peek && Z.ltb peek next &&
      canonical_peeksb (next :: rest)) by reflexivity.
    rewrite Hstep.
    inversion Hsorted as [|sorted_head sorted_tail Htail_sorted Hhead_forall];
      subst.
    inversion Hrange as [|range_head range_tail Hpeek Htail_range]; subst.
    inversion Hhead_forall as [|lt_head lt_tail Hlt Hlt_rest]; subst.
    rewrite (peek_in_i32_rangeb_complete _ Hpeek).
    rewrite (proj2 (Z.ltb_lt peek next) Hlt).
    rewrite IH; [reflexivity|split; assumption].
Qed.

Lemma record_boundary_valid_complete : forall record,
  continuation_record_valid record ->
  continuation_record_boundary_validb record = true.
Proof.
  intros record [Hidentity [Hsource Hpeeks]].
  unfold continuation_record_boundary_validb.
  rewrite Hidentity, Hsource, (canonical_peeksb_complete _ Hpeeks).
  reflexivity.
Qed.

Lemma expectation_of_record_matches : forall record,
  expectation_matches_record (expectation_of_record record) record.
Proof. intro record. split; [reflexivity|split; reflexivity]. Qed.

Definition ordinary_entries (roots : SemanticRoots) (group : GroupKey)
    : list OrdinaryContinuation :=
  map (fun entry =>
    OrdinaryContinuationState (canonical_continuation_path group entry) entry)
    (group_records roots group).

Definition authority_of (w : World) : ContinuationAuthority :=
  ContinuationAuthorityState
    (group_installed (world_installed w))
    (ordinary_entries (world_roots w))
    (group_epoch w)
    (resting_joins_of (world_roots w))
    (installed_joins_of (world_installed w)).

Fixpoint record_ordinal (record : ContinuationRecord)
    (records : list ContinuationRecord) : nat :=
  match records with
  | [] => 0
  | head :: tail =>
      if continuation_record_eq_dec head record then 0
      else S (record_ordinal record tail)
  end.

Definition candidate_capability (observed : World) (target : ReceiveTarget)
    : ContinuationCapability :=
  match receive_target_kind target with
  | InstalledKind =>
      issue_installed_capability (authority_of observed)
        (receive_target_group target)
  | RestingKind =>
      issue_ordinary_capability (authority_of observed)
        (receive_target_group target)
        (record_ordinal (receive_target_record target)
          (group_records (world_roots observed) (receive_target_group target)))
  end.

Definition candidate_commit (observed current : World) (target : ReceiveTarget)
    : option ResolvedTarget :=
  resolve_exact_continuation (authority_of observed) (authority_of current)
    (receive_target_group target) (candidate_capability observed target)
    (expectation_of_record (receive_target_record target)).

Lemma record_ordinal_nth_error : forall record records,
  In record records -> nth_error records (record_ordinal record records) = Some record.
Proof.
  intros record records. induction records as [|head tail IH];
    intro Hin; [destruct Hin|].
  cbn [record_ordinal].
  destruct (continuation_record_eq_dec head record) as [Hsame|Hdifferent].
  - subst head. reflexivity.
  - cbn [nth_error]. apply IH.
    destruct Hin as [Hhead|Htail]; [contradiction|exact Htail].
Qed.

Lemma ordinary_entry_at_ordinal : forall w group record,
  In record (group_records (world_roots w) group) ->
  nth_error (ordinary_continuations (authority_of w) group)
    (record_ordinal record (group_records (world_roots w) group)) =
  Some (OrdinaryContinuationState (canonical_continuation_path group record)
    record).
Proof.
  intros w group record Hin. cbn [authority_of ordinary_continuations].
  unfold ordinary_entries.
  exact (@map_nth_error ContinuationRecord OrdinaryContinuation
    (fun entry =>
       OrdinaryContinuationState (canonical_continuation_path group entry) entry)
    (record_ordinal record (group_records (world_roots w) group))
    (group_records (world_roots w) group) record
    (record_ordinal_nth_error _ _ Hin)).
Qed.

Lemma same_group_layout_resolves_alike :
    forall observed commit group capability,
  continuation_epochs commit group = continuation_epochs observed group ->
  installed_continuations commit group = installed_continuations observed group ->
  ordinary_continuations commit group = ordinary_continuations observed group ->
  resolve_capability observed commit group capability =
  resolve_capability observed observed group capability.
Proof.
  intros observed commit group capability Hepoch Hinstalled Hordinary.
  unfold resolve_capability.
  destruct (group_eq_dec group (capability_group capability)); [|reflexivity].
  rewrite Hepoch.
  destruct (EqualityEpoch.eq_dec (continuation_epochs observed group)
    (continuation_epochs observed group)); [|reflexivity].
  unfold resolve_bound_ordinary. rewrite Hinstalled, Hordinary. reflexivity.
Qed.

Lemma exact_continuation_of_resolved :
    forall observed commit group capability target expected,
  resolve_capability observed commit group capability = Some target ->
  target_boundary_validb target = true ->
  expectation_matches_record expected (target_record target) ->
  resolve_exact_continuation observed commit group capability expected =
    Some target.
Proof.
  intros observed commit group capability target expected Hresolve Hboundary
    Hmatch.
  unfold resolve_exact_continuation. rewrite Hresolve, Hboundary.
  destruct (expectation_matches_record_dec expected (target_record target))
    as [_|Hmismatch]; [reflexivity|contradiction].
Qed.

Theorem same_revision_same_derivation : forall initial w,
  WorldRun initial w -> world_tokens_issued initial ->
  (revision_store (world_revision w) = revision_store (world_revision initial) ->
   world_roots w = world_roots initial /\
   world_installed w = world_installed initial /\
   (forall channel, derive_world w channel = derive_world initial channel)) /\
  (forall group,
     reachable_same_epoch_group (authority_of initial) (authority_of w) group).
Proof.
  intros initial w Hrun Htokens. split.
  - intro Hsame.
    destruct Htokens as [[_ [Hstore _]] _].
    destruct (world_run_same_store initial w Hrun Hstore Hsame)
      as [Hroots Hinstalled].
    split; [exact Hroots|]. split; [exact Hinstalled|].
    intro channel. unfold derive_world. rewrite Hroots, Hinstalled.
    reflexivity.
  - intros group Hepoch.
    cbn [authority_of continuation_epochs installed_continuations
      ordinary_continuations] in Hepoch |- *.
    destruct (world_run_same_group_epoch initial w group Hrun Htokens Hepoch)
      as [Hrecords Hinstalled].
    split; [exact Hinstalled|]. unfold ordinary_entries. rewrite Hrecords.
    reflexivity.
Qed.

Theorem read_never_hides_an_enabled_receive : forall initial w accelerator,
  world_valid initial -> Reachable initial (w, accelerator) ->
  forall channel,
    same_members (accelerator_read w accelerator channel)
      (derive_world w channel) /\
    (forall occurrences,
       accelerator_answer w accelerator channel = ExactAnswer occurrences ->
       classify_accelerator w accelerator = ClassCoherent).
Proof.
  intros initial w accelerator Hvalid Hreachable channel.
  destruct (reachable_invariant initial (w, accelerator) Hvalid Hreachable)
    as [_ Hinvariant].
  cbn [fst snd] in Hinvariant.
  split; [apply invariant_read_is_derivation; exact Hinvariant|].
  intros occurrences Hanswer. eapply exact_answer_is_coherent. exact Hanswer.
Qed.

Theorem failed_or_stale_update_publishes_nothing :
  (forall operation w accelerator,
     commit_operation operation w accelerator true = (w, accelerator)) /\
  (forall w base scope entries,
     base <> world_revision w ->
     publish_build w (AcceleratorBuilding base scope entries) =
       AcceleratorBuilding base scope entries) /\
  (forall w base scope entries,
     ~ covers scope w ->
     publish_build w (AcceleratorBuilding base scope entries) =
       AcceleratorBuilding base scope entries) /\
  (forall effect w bound lineage scope entries artifacts,
     bumps effect -> revision_issued (world_issued w) bound ->
     classify_accelerator (step_effect effect w)
       (AcceleratorPublished bound lineage scope entries artifacts) <>
       ClassCoherent).
Proof.
  split; [intros operation w accelerator; reflexivity|].
  split.
  - intros w base scope entries Hstale. cbn [publish_build].
    destruct (revision_eq_dec base (world_revision w));
      [contradiction|reflexivity].
  - split.
    + intros w base scope entries Hincomplete. cbn [publish_build].
      destruct (revision_eq_dec base (world_revision w)); [|reflexivity].
      destruct (coversb scope w) eqn:Hcover; [|reflexivity].
      exfalso. apply Hincomplete. apply coversb_spec. exact Hcover.
    + intros effect w bound lineage scope entries artifacts Hbumps Hissued
        Hclass.
      destruct (classify_coherent_spec _ _ _ _ _ _ Hclass)
        as [_ [Hbound _]].
      destruct (held_revision_goes_stale effect w bound Hbumps Hissued)
        as [Hstale _].
      contradiction.
Qed.

Theorem published_artifacts_share_one_revision :
  (forall w scope entries,
     publish_at w scope entries =
       AcceleratorPublished (world_revision w) (world_manifest w) scope entries
         (artifacts_at (world_revision w))) /\
  (forall revision,
     catalog_revision (artifacts_at revision) = revision /\
     occurrence_revision (artifacts_at revision) = revision /\
     automaton_revision (artifacts_at revision) = revision) /\
  (forall w bound lineage scope entries artifacts,
     classify_accelerator w
       (AcceleratorPublished bound lineage scope entries artifacts) =
       ClassCoherent ->
     catalog_revision artifacts = world_revision w /\
     occurrence_revision artifacts = world_revision w /\
     automaton_revision artifacts = world_revision w) /\
  (forall w bound lineage scope entries artifacts,
     catalog_revision artifacts <> bound \/
     occurrence_revision artifacts <> bound \/
     automaton_revision artifacts <> bound ->
     classify_accelerator w
       (AcceleratorPublished bound lineage scope entries artifacts) <>
       ClassCoherent).
Proof.
  split; [intros w scope entries; reflexivity|].
  split; [intro revision; split; [reflexivity|split; reflexivity]|].
  split.
  - intros w bound lineage scope entries artifacts Hclass.
    destruct (classify_coherent_spec _ _ _ _ _ _ Hclass)
      as [_ [Hbound [Hartifacts _]]].
    subst artifacts bound. split; [reflexivity|split; reflexivity].
  - intros w bound lineage scope entries artifacts Hsplit Hclass.
    destruct (classify_coherent_spec _ _ _ _ _ _ Hclass)
      as [_ [_ [Hartifacts _]]].
    subst artifacts.
    destruct Hsplit as [Hcatalog|[Hoccurrence|Hautomaton]];
      [apply Hcatalog|apply Hoccurrence|apply Hautomaton]; reflexivity.
Qed.

Lemma candidate_capability_group : forall observed target,
  capability_group (candidate_capability observed target) =
  receive_target_group target.
Proof. intros observed [kind group record]. destruct kind; reflexivity. Qed.

Lemma candidate_commit_stale : forall observed current target,
  group_epoch current (receive_target_group target) <>
    group_epoch observed (receive_target_group target) ->
  candidate_commit observed current target = None.
Proof.
  intros observed current target Hepoch.
  unfold candidate_commit, resolve_exact_continuation.
  rewrite (@stale_epoch_cannot_resolve (authority_of observed)
    (authority_of current) (receive_target_group target)
    (candidate_capability observed target)
    (candidate_capability_group observed target) Hepoch).
  reflexivity.
Qed.

Lemma candidate_commit_resolves : forall observed current kind group record,
  WorldRun observed current -> world_tokens_issued observed ->
  continuation_authority_valid (authority_of observed) ->
  In (ReceiveTargetState kind group record) (targets_at observed group) ->
  group_epoch current group = group_epoch observed group ->
  exists resolved,
    candidate_commit observed current (ReceiveTargetState kind group record) =
      Some resolved /\
    target_record resolved = record.
Proof.
  intros observed current kind group record Hrun Htokens Hauthority Htarget
    Hepoch.
  destruct (world_run_same_group_epoch observed current group Hrun Htokens
    Hepoch) as [Hrecords Hinstalled].
  assert (Hlayout : forall capability,
    resolve_capability (authority_of observed) (authority_of current) group
      capability =
    resolve_capability (authority_of observed) (authority_of observed) group
      capability).
  { intro capability. apply same_group_layout_resolves_alike;
      cbn [authority_of continuation_epochs installed_continuations
        ordinary_continuations].
    - exact Hepoch.
    - exact Hinstalled.
    - unfold ordinary_entries. rewrite Hrecords. reflexivity. }
  pose proof Hauthority as Hparts.
  destruct Hparts as [_ [Hordinary [Hinstalled_valid _]]].
  unfold targets_at in Htarget. apply group_targets_spec in Htarget.
  destruct Htarget as [[installed [Hslot Hsame]]|[resting [Hin Hsame]]].
  - unfold installed_target in Hsame.
    assert (Hkind : kind = InstalledKind) by congruence.
    assert (Hrecord : record = installed) by congruence.
    subst kind record.
    exists (InstalledTarget installed). split; [|reflexivity].
    unfold candidate_commit.
    cbn [candidate_capability receive_target_kind receive_target_group
      receive_target_record].
    apply exact_continuation_of_resolved.
    + rewrite Hlayout. apply issued_installed_capability_resolves.
      exact Hslot.
    + cbn [target_boundary_validb]. apply record_boundary_valid_complete.
      exact (proj2 (Hinstalled_valid group installed Hslot)).
    + cbn [target_record]. apply expectation_of_record_matches.
  - unfold resting_target in Hsame.
    assert (Hkind : kind = RestingKind) by congruence.
    assert (Hrecord : record = resting) by congruence.
    subst kind record.
    pose proof (ordinary_entry_at_ordinal observed group resting Hin) as Hnth.
    assert (Hentry : In
      (OrdinaryContinuationState (canonical_continuation_path group resting)
        resting)
      (ordinary_continuations (authority_of observed) group))
      by (eapply nth_error_In; exact Hnth).
    destruct (Hordinary group _ Hentry) as [Hpath [Hvalid _]].
    cbn [ordinary_path ordinary_record] in Hpath, Hvalid.
    exists (OrdinaryTarget (canonical_continuation_path group resting) resting).
    split; [|reflexivity].
    unfold candidate_commit.
    cbn [candidate_capability receive_target_kind receive_target_group
      receive_target_record].
    apply exact_continuation_of_resolved.
    + rewrite Hlayout.
      exact (@issued_ordinary_capability_resolves (authority_of observed) group
        (record_ordinal resting (group_records (world_roots observed) group))
        (OrdinaryContinuationState (canonical_continuation_path group resting)
          resting) Hauthority Hnth).
    + cbn [target_boundary_validb]. apply andb_true_intro.
      split; [exact Hpath|apply record_boundary_valid_complete; exact Hvalid].
    + cbn [target_record]. apply expectation_of_record_matches.
Qed.

Theorem accelerator_candidate_commit_refines_accepted_match :
  (forall observed current target,
     candidate_commit observed current target =
     resolve_exact_continuation (authority_of observed) (authority_of current)
       (receive_target_group target) (candidate_capability observed target)
       (expectation_of_record (receive_target_record target))) /\
  (forall observed current target,
     group_epoch current (receive_target_group target) <>
       group_epoch observed (receive_target_group target) ->
     candidate_commit observed current target = None) /\
  (forall observed current target,
     WorldRun observed current -> world_tokens_issued observed ->
     continuation_authority_valid (authority_of observed) ->
     In target (targets_at observed (receive_target_group target)) ->
     group_epoch current (receive_target_group target) =
       group_epoch observed (receive_target_group target) ->
     exists resolved,
       candidate_commit observed current target = Some resolved /\
       target_record resolved = receive_target_record target).
Proof.
  split; [intros observed current target; reflexivity|].
  split; [exact candidate_commit_stale|].
  intros observed current [kind group record] Hrun Htokens Hauthority Htarget
    Hepoch.
  exact (candidate_commit_resolves observed current kind group record Hrun
    Htokens Hauthority Htarget Hepoch).
Qed.

Theorem semantic_successor_is_accelerator_free :
  (forall operation w left right fault,
     fst (commit_operation operation w left fault) =
     fst (commit_operation operation w right fault)) /\
  (forall operation w accelerator,
     fst (commit_operation operation w accelerator false) =
     step_world operation w) /\
  (forall initial w left right channel,
     world_valid initial -> Reachable initial (w, left) ->
     Reachable initial (w, right) ->
     same_members (accelerator_read w left channel)
       (accelerator_read w right channel)).
Proof.
  split; [intros operation w left right fault; destruct fault; reflexivity|].
  split; [intros operation w accelerator; reflexivity|].
  intros initial w left right channel Hvalid Hleft Hright.
  destruct (read_never_hides_an_enabled_receive initial w left Hvalid Hleft
    channel) as [Hleft_read _].
  destruct (read_never_hides_an_enabled_receive initial w right Hvalid Hright
    channel) as [Hright_read _].
  exact (same_members_trans Hleft_read (same_members_sym Hright_read)).
Qed.

Print Assumptions derived_occurrence_is_sound.
Print Assumptions derived_occurrence_is_complete.
Print Assumptions derived_positions_factor_through_group.
Print Assumptions repeated_channel_is_one_occurrence_with_all_positions.
Print Assumptions occurrence_capacity_is_position_multiplicity.
Print Assumptions prefix_aliased_channels_have_disjoint_occurrences.
Print Assumptions derivation_ignores_data_and_runtime_order.
Print Assumptions join_root_projection_is_exact.
Print Assumptions derivation_is_order_neutral.
Print Assumptions receive_id_resolution_is_exact.
Print Assumptions distinct_records_have_distinct_receive_ids.
Print Assumptions hash_bucket_resolution_is_collision_checked.
Print Assumptions stale_revision_never_resolves.
Print Assumptions first_identity_representative_is_preserved.
Print Assumptions insertion_update_equals_rebuild.
Print Assumptions nonfinal_removal_update_equals_rebuild.
Print Assumptions final_removal_update_equals_rebuild.
Print Assumptions install_and_uninstall_update_equals_rebuild.
Print Assumptions history_load_and_byte_equal_replacement_preserve_derivation.
Print Assumptions reset_issues_fresh_generation_and_rebuilds.
Print Assumptions same_revision_same_derivation.
Print Assumptions read_never_hides_an_enabled_receive.
Print Assumptions failed_or_stale_update_publishes_nothing.
Print Assumptions published_artifacts_share_one_revision.
Print Assumptions accelerator_candidate_commit_refines_accepted_match.
Print Assumptions semantic_successor_is_accelerator_free.

End TwoLevelRSpaceReceiveAccelerator.
