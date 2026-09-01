From StackSafePDA Require Export PathMapCanonicalStreamPublicationBase.
From Stdlib Require Import List Arith Bool Lia PeanoNat NArith.

Import ListNotations.
Open Scope nat_scope.

(** Opaque branch lemmas keep the recursive extensional proof linear in proof
    elaboration space.  Each lemma consumes only the induction hypothesis for
    its one recursive call; no branch retains the proof terms of its siblings. *)
Inductive upsert_result_is_extensionally_exact
    (source additions output : list Entry) : Prop :=
| UpsertResultIsExtensionallyExact :
    (forall queried,
      lookup_count queried output =
        (lookup_count queried source + lookup_count queried additions)%N) ->
    upsert_result_is_extensionally_exact source additions output.

Lemma checked_upsert_lt_branch_is_extensionally_exact :
  forall source_path source_count source_tail
         addition_path addition_count addition_tail recursive_output output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    path_compare source_path addition_path = Lt ->
    upsert_result_is_extensionally_exact
      source_tail
      ((addition_path, addition_count) :: addition_tail)
      recursive_output ->
    output = (source_path, source_count) :: recursive_output ->
    forall queried,
      lookup_count queried output =
        (lookup_count queried ((source_path, source_count) :: source_tail) +
         lookup_count queried
           ((addition_path, addition_count) :: addition_tail))%N.
Proof.
  intros source_path source_count source_tail
    addition_path addition_count addition_tail recursive_output output
    Hsource Hadditions Hcompare Hrecursive Houtput queried.
  inversion Hsource as
    [|? ? ? Hsource_path Hsource_count Hsource_order Hsource_tail].
  inversion Hadditions as
    [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
  destruct Hrecursive as [Hrecursive].
  pose proof (proj1
    (path_compare_lt_iff source_path addition_path) Hcompare)
    as Hsource_addition.
  subst output.
  specialize (Hrecursive queried).
  destruct (path_eqb queried source_path) eqn:Hqueried.
  - apply path_eqb_spec in Hqueried.
    subst queried.
    assert (Hadditions_after_source :
      Forall
        (fun entry => path_lex_lt source_path (entry_path entry))
        ((addition_path, addition_count) :: addition_tail)).
    { constructor.
      - exact Hsource_addition.
      - eapply path_order_transports_over_ordered_tail.
        + exact Hsource_addition.
        + exact Haddition_order. }
    pose proof (ordered_head_is_absent_from_tail
      source_path
      ((addition_path, addition_count) :: addition_tail)
      Hadditions_after_source) as Hadditions_absent.
    simpl in Hadditions_absent |- *.
    rewrite path_eqb_reflexive.
    rewrite Hadditions_absent.
    now rewrite N.add_0_r.
  - simpl.
    rewrite Hqueried.
    exact Hrecursive.
Qed.

Lemma checked_upsert_gt_branch_is_extensionally_exact :
  forall source_path source_count source_tail
         addition_path addition_count addition_tail count recursive_output output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    path_compare source_path addition_path = Gt ->
    count = addition_count ->
    upsert_result_is_extensionally_exact
      ((source_path, source_count) :: source_tail)
      addition_tail recursive_output ->
    output = (addition_path, count) :: recursive_output ->
    forall queried,
      lookup_count queried output =
        (lookup_count queried ((source_path, source_count) :: source_tail) +
         lookup_count queried
           ((addition_path, addition_count) :: addition_tail))%N.
Proof.
  intros source_path source_count source_tail
    addition_path addition_count addition_tail count recursive_output output
    Hsource Hadditions Hcompare Hcount Hrecursive Houtput queried.
  inversion Hsource as
    [|? ? ? Hsource_path Hsource_count Hsource_order Hsource_tail].
  inversion Hadditions as
    [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
  destruct Hrecursive as [Hrecursive].
  pose proof (proj1
    (path_compare_gt_iff source_path addition_path) Hcompare)
    as Haddition_source.
  subst count.
  subst output.
  specialize (Hrecursive queried).
  destruct (path_eqb queried addition_path) eqn:Hqueried.
  - apply path_eqb_spec in Hqueried.
    subst queried.
    assert (Hsource_after_addition :
      Forall
        (fun entry => path_lex_lt addition_path (entry_path entry))
        ((source_path, source_count) :: source_tail)).
    { constructor.
      - exact Haddition_source.
      - eapply path_order_transports_over_ordered_tail.
        + exact Haddition_source.
        + exact Hsource_order. }
    pose proof (ordered_head_is_absent_from_tail
      addition_path
      ((source_path, source_count) :: source_tail)
      Hsource_after_addition) as Hsource_absent.
    simpl in Hsource_absent |- *.
    rewrite path_eqb_reflexive.
    rewrite Hsource_absent.
    now rewrite N.add_0_l.
  - simpl.
    rewrite Hqueried.
    exact Hrecursive.
Qed.

Lemma option_cons_some_result_is_exact :
  forall (head : Entry) result tail output,
    result = Some tail ->
    (match result with
     | Some result_tail => Some (head :: result_tail)
     | None => None
     end) = Some output ->
    output = head :: tail.
Proof.
  intros head result tail output Hresult Houtput.
  rewrite Hresult in Houtput.
  inversion Houtput.
  reflexivity.
Qed.

Lemma option_cons_none_cannot_publish :
  forall (head : Entry) result output,
    result = None ->
    (match result with
     | Some result_tail => Some (head :: result_tail)
     | None => None
     end) = Some output ->
    False.
Proof.
  intros head result output Hresult Houtput.
  rewrite Hresult in Houtput.
  discriminate.
Qed.

Lemma option_cons_publication_has_source :
  forall (head : Entry) result output,
    (match result with
     | Some result_tail => Some (head :: result_tail)
     | None => None
     end) = Some output ->
    exists result_tail,
      result = Some result_tail /\ output = head :: result_tail.
Proof.
  intros head result output Houtput.
  destruct result as [result_tail|].
  - exists result_tail.
    split.
    + reflexivity.
    + inversion Houtput.
      reflexivity.
  - discriminate.
Qed.

Lemma checked_option_cons_publication_has_source :
  forall (path : Path)
         (checked_result : option Multiplicity)
         (recursive_result : option (list Entry))
         (output : list Entry),
    (match checked_result with
     | Some count =>
         match recursive_result with
         | Some result_tail => Some ((path, count) :: result_tail)
         | None => None
         end
     | None => None
     end) = Some output ->
    exists count result_tail,
      checked_result = Some count /\
      recursive_result = Some result_tail /\
      output = (path, count) :: result_tail.
Proof.
  intros path checked_result recursive_result output Houtput.
  destruct checked_result as [count|].
  - destruct recursive_result as [result_tail|].
    + exists count, result_tail.
      split.
      * reflexivity.
      * split.
        -- reflexivity.
        -- inversion Houtput.
           reflexivity.
    + discriminate.
  - discriminate.
Qed.

Lemma checked_upsert_nonempty_step_preserves_canonicality :
  forall fuel source_path source_count source_tail
         addition_path addition_count addition_tail output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    (forall recursive_source recursive_additions recursive_output,
      canonical_entries recursive_source ->
      canonical_entries recursive_additions ->
      checked_upsert_merge_fuel fuel recursive_source recursive_additions =
        Some recursive_output ->
      canonical_entries recursive_output) ->
    checked_upsert_merge_fuel (S fuel)
      ((source_path, source_count) :: source_tail)
      ((addition_path, addition_count) :: addition_tail) = Some output ->
    canonical_entries output.
Proof.
  intros fuel source_path source_count source_tail
    addition_path addition_count addition_tail output
    Hsource Hadditions Hrecursive Hmerge.
  simpl in Hmerge.
  destruct (path_compare source_path addition_path) eqn:Hcompare.
  - apply path_compare_eq_iff in Hcompare. subst addition_path.
    destruct (checked_option_cons_publication_has_source
      source_path
      (checked_add_multiplicity source_count addition_count)
      (checked_upsert_merge_fuel fuel source_tail addition_tail)
      output Hmerge)
      as [count [canonical_tail [Hcount [Htail Houtput]]]].
    eapply (checked_upsert_equal_case_from_recursive_canonicality
      fuel source_path source_count source_tail addition_count addition_tail
      count canonical_tail output); eauto.
  - pose proof (proj1
      (path_compare_lt_iff source_path addition_path) Hcompare)
      as Hsource_addition.
    destruct (option_cons_publication_has_source
      (source_path, source_count)
      (checked_upsert_merge_fuel fuel source_tail
        ((addition_path, addition_count) :: addition_tail))
      output Hmerge) as [canonical_tail [Htail Houtput]].
    eapply (checked_upsert_left_case_from_recursive_canonicality
      fuel source_path source_count source_tail
      addition_path addition_count addition_tail canonical_tail output); eauto.
  - pose proof (proj1
      (path_compare_gt_iff source_path addition_path) Hcompare)
      as Haddition_source.
    destruct (checked_option_cons_publication_has_source
      addition_path
      (checked_add_multiplicity 0%N addition_count)
      (checked_upsert_merge_fuel fuel
        ((source_path, source_count) :: source_tail) addition_tail)
      output Hmerge)
      as [count [canonical_tail [Hcount [Htail Houtput]]]].
    eapply (checked_upsert_right_case_from_recursive_canonicality
      fuel source_path source_count source_tail
      addition_path addition_count addition_tail count canonical_tail output); eauto.
Qed.

Theorem checked_upsert_merge_preserves_canonicality :
  forall fuel source additions output,
    canonical_entries source ->
    canonical_entries additions ->
    checked_upsert_merge_fuel fuel source additions = Some output ->
    canonical_entries output.
Proof.
  induction fuel as [|fuel IH]; intros source additions output
    Hsource Hadditions Hmerge.
  - destruct source, additions; simpl in Hmerge; try discriminate.
    inversion Hmerge. constructor.
  - destruct source as [|[source_path source_count] source_tail],
      additions as [|[addition_path addition_count] addition_tail];
      simpl in Hmerge.
    + inversion Hmerge. constructor.
    + destruct (checked_option_cons_publication_has_source
        addition_path
        (checked_add_multiplicity 0%N addition_count)
        (checked_upsert_merge_fuel fuel [] addition_tail)
        output Hmerge)
        as [count [canonical_tail [Hcount [Htail Houtput]]]].
      eapply (checked_upsert_empty_left_branch_preserves_canonicality
        fuel addition_path addition_count addition_tail count canonical_tail output).
      * exact Hadditions.
      * exact Hcount.
      * exact Htail.
      * abstract (eapply (IH [] addition_tail canonical_tail);
          [ constructor
          | exact (canonical_entries_tail _ _ Hadditions)
          | exact Htail ]).
      * exact Houtput.
    + inversion Hmerge; subst output. exact Hsource.
    + eapply (checked_upsert_nonempty_step_preserves_canonicality
        fuel source_path source_count source_tail
        addition_path addition_count addition_tail output).
      * exact Hsource.
      * exact Hadditions.
      * exact IH.
      * exact Hmerge.
Qed.

Theorem checked_exact_upsert_preserves_canonicality :
  forall source additions output,
    canonical_entries source ->
    canonical_entries additions ->
    checked_exact_upsert source additions = Some output ->
    canonical_entries output.
Proof.
  intros source additions output Hsource Hadditions Hupsert.
  unfold checked_exact_upsert in Hupsert.
  eapply (checked_upsert_merge_preserves_canonicality
    (S (length source + length additions)) source additions output).
  - exact Hsource.
  - exact Hadditions.
  - exact Hupsert.
Qed.

Theorem checked_upsert_merge_is_extensionally_exact :
  forall fuel source additions output queried,
    canonical_entries source ->
    canonical_entries additions ->
    checked_upsert_merge_fuel fuel source additions = Some output ->
    lookup_count queried output =
      (lookup_count queried source + lookup_count queried additions)%N.
Proof.
  induction fuel as [|fuel IH]; intros source additions output queried
    Hsource Hadditions Hmerge.
  - destruct source, additions; simpl in Hmerge; try discriminate.
    inversion Hmerge. reflexivity.
  - destruct source as [|[source_path source_count] source_tail],
      additions as [|[addition_path addition_count] addition_tail];
      simpl in Hmerge.
    + inversion Hmerge. reflexivity.
    + inversion Hadditions as
        [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
      destruct (checked_add_multiplicity 0%N addition_count)
        as [count|] eqn:Hcount; [|discriminate].
      destruct (checked_upsert_merge_fuel fuel [] addition_tail)
        as [ext_tail|] eqn:Htail; [|discriminate].
      inversion Hmerge; subst output. simpl.
      pose proof (checked_add_zero_is_identity addition_count count Hcount).
      subst count.
      specialize (IH [] addition_tail ext_tail queried
        CanonicalEntriesNil Haddition_tail Htail). simpl in IH.
      destruct (path_eqb queried addition_path).
      * simpl. reflexivity.
      * simpl. exact IH.
    + inversion Hmerge; subst output. simpl. now rewrite N.add_0_r.
    + inversion Hsource as
        [|? ? ? Hsource_path Hsource_count Hsource_order Hsource_tail].
      inversion Hadditions as
        [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
      destruct (path_compare source_path addition_path) eqn:Hcompare.
      * apply path_compare_eq_iff in Hcompare. subst addition_path.
        destruct (checked_add_multiplicity source_count addition_count)
          as [count|] eqn:Hcount; [|discriminate].
        destruct (checked_upsert_merge_fuel fuel source_tail addition_tail)
          as [ext_tail|] eqn:Htail; [|discriminate].
        inversion Hmerge; subst. simpl.
        pose proof (successful_checked_add_is_exact
          source_count addition_count count Hcount). subst count.
        specialize (IH source_tail addition_tail ext_tail queried
          Hsource_tail Haddition_tail Htail).
        destruct (path_eqb queried source_path).
        -- simpl. reflexivity.
        -- simpl. exact IH.
      * destruct (option_cons_publication_has_source
          (source_path, source_count)
          (checked_upsert_merge_fuel fuel source_tail
            ((addition_path, addition_count) :: addition_tail))
          output Hmerge) as [recursive_output [Hrecursive Houtput]].
        eapply (checked_upsert_lt_branch_is_extensionally_exact
          source_path source_count source_tail
          addition_path addition_count addition_tail recursive_output output).
        -- exact Hsource.
        -- exact Hadditions.
        -- exact Hcompare.
        -- constructor.
           intros recursive_queried.
           eapply (IH source_tail
             ((addition_path, addition_count) :: addition_tail)
             recursive_output recursive_queried).
           ++ exact Hsource_tail.
           ++ exact Hadditions.
           ++ exact Hrecursive.
        -- exact Houtput.
      * destruct (checked_option_cons_publication_has_source
          addition_path
          (checked_add_multiplicity 0%N addition_count)
          (checked_upsert_merge_fuel fuel
            ((source_path, source_count) :: source_tail) addition_tail)
          output Hmerge)
          as [count [recursive_output [Hcount [Hrecursive Houtput]]]].
        eapply (checked_upsert_gt_branch_is_extensionally_exact
          source_path source_count source_tail
          addition_path addition_count addition_tail count recursive_output output).
        -- exact Hsource.
        -- exact Hadditions.
        -- exact Hcompare.
        -- exact (checked_add_zero_is_identity addition_count count Hcount).
        -- constructor.
           intros recursive_queried.
           eapply (IH ((source_path, source_count) :: source_tail)
             addition_tail recursive_output recursive_queried).
           ++ exact Hsource.
           ++ exact Haddition_tail.
           ++ exact Hrecursive.
        -- exact Houtput.
Qed.

Theorem checked_exact_upsert_is_extensionally_exact :
  forall source additions output queried,
    canonical_entries source ->
    canonical_entries additions ->
    checked_exact_upsert source additions = Some output ->
    lookup_count queried output =
      (lookup_count queried source + lookup_count queried additions)%N.
Proof.
  intros source additions output queried Hsource Hadditions Hupsert.
  unfold checked_exact_upsert in Hupsert.
  eapply checked_upsert_merge_is_extensionally_exact; eauto.
Qed.

Lemma checked_upsert_merge_output_length :
  forall fuel source additions output,
    checked_upsert_merge_fuel fuel source additions = Some output ->
    length output <= length source + length additions.
Proof.
  induction fuel as [|fuel IH]; intros source additions output Hmerge.
  - destruct source, additions; simpl in Hmerge; try discriminate.
    inversion Hmerge. simpl. lia.
  - destruct source as [|source_head source_tail],
      additions as [|addition addition_tail]; simpl in Hmerge.
    + inversion Hmerge. simpl. lia.
    + destruct (checked_add_multiplicity 0%N
        (entry_multiplicity addition)); try discriminate.
      destruct (checked_upsert_merge_fuel fuel [] addition_tail)
        as [tail|] eqn:Htail; try discriminate.
      inversion Hmerge; subst output. simpl.
      specialize (IH [] addition_tail tail Htail). simpl in IH. lia.
    + inversion Hmerge; subst output. simpl. lia.
    + destruct (path_compare (entry_path source_head)
        (entry_path addition)) eqn:Hcompare.
      * destruct (checked_add_multiplicity
          (entry_multiplicity source_head)
          (entry_multiplicity addition)) as [count|] eqn:Hcount;
          try discriminate.
        destruct (checked_upsert_merge_fuel fuel source_tail addition_tail)
          as [tail|] eqn:Htail; try discriminate.
        inversion Hmerge; subst. simpl.
        specialize (IH source_tail addition_tail tail Htail).
        simpl in IH. lia.
      * destruct (checked_upsert_merge_fuel fuel source_tail
          (addition :: addition_tail)) as [tail|] eqn:Htail;
          try discriminate.
        inversion Hmerge; subst. simpl.
        specialize (IH source_tail (addition :: addition_tail) tail Htail).
        simpl in IH. lia.
      * destruct (checked_add_multiplicity 0%N
          (entry_multiplicity addition)) as [count|] eqn:Hcount;
          try discriminate.
        destruct (checked_upsert_merge_fuel fuel
          (source_head :: source_tail) addition_tail)
          as [tail|] eqn:Htail; try discriminate.
        inversion Hmerge; subst. simpl.
        specialize (IH (source_head :: source_tail) addition_tail tail Htail).
        simpl in IH. lia.
Qed.

Theorem checked_exact_upsert_output_length :
  forall source additions output,
    checked_exact_upsert source additions = Some output ->
    length output <= length source + length additions.
Proof.
  intros source additions output Hupsert.
  unfold checked_exact_upsert in Hupsert.
  eapply checked_upsert_merge_output_length; eauto.
Qed.

Lemma exact_stream_transform_output_length :
  forall source removals output,
    exact_stream_transform source removals = Some output ->
    length output <= length source.
Proof.
  induction source as [|entry tail IH]; intros removals output Htransform;
    simpl in Htransform.
  - inversion Htransform. simpl. lia.
  - destruct (checked_subtract_multiplicity
      (entry_multiplicity entry)
      (lookup_count (entry_path entry) removals)) as [remaining|] eqn:Hremaining;
      try discriminate.
    destruct (exact_stream_transform tail removals)
      as [tail_output|] eqn:Htail; try discriminate.
    destruct (N.eqb remaining 0%N); inversion Htransform; subst output; simpl;
      specialize (IH removals tail_output Htail); lia.
Qed.

Lemma idempotent_exact_delete_output_length :
  forall source deletions,
    length (idempotent_exact_delete source deletions) <= length source.
Proof.
  induction source as [|entry tail IH]; intros deletions; simpl; [lia|].
  destruct (path_memberb (entry_path entry) deletions); simpl;
    specialize (IH deletions); lia.
Qed.

Inductive BagDeltaRemovalMode : Type :=
| StrictSubtract
| IdempotentDelete.

Fixpoint strict_subtraction_reaches_zero_b
  (source subtractions : list Entry) : bool :=
  match subtractions with
  | [] => false
  | subtraction :: tail =>
      N.eqb (lookup_count (entry_path subtraction) source)
            (entry_multiplicity subtraction)
      || strict_subtraction_reaches_zero_b source tail
  end.

Fixpoint deletion_removes_existing_b
  (source : list Entry) (deletions : list Path) : bool :=
  match deletions with
  | [] => false
  | path :: tail =>
      negb (N.eqb (lookup_count path source) 0%N)
      || deletion_removes_existing_b source tail
  end.

Fixpoint additions_are_value_only_b
  (source additions : list Entry) : bool :=
  match additions with
  | [] => true
  | addition :: tail =>
      negb (N.eqb (lookup_count (entry_path addition) source) 0%N)
      && additions_are_value_only_b source tail
  end.

Inductive CandidateBuildStrategy : Type :=
| GuardedValueOnlySetVal
| PrivateShallowCowInsertion
| IndependentCanonicalStreamRebuild.

Record BagDelta : Type := {
  bag_delta_removal_mode : BagDeltaRemovalMode;
  bag_delta_subtractions : list Entry;
  bag_delta_deletions : list Path;
  bag_delta_additions : list Entry
}.

Definition bag_delta_requires_prune_b
  (source : list Entry) (delta : BagDelta) : bool :=
  match bag_delta_removal_mode delta with
  | StrictSubtract =>
      strict_subtraction_reaches_zero_b source
        (bag_delta_subtractions delta)
  | IdempotentDelete =>
      deletion_removes_existing_b source (bag_delta_deletions delta)
  end.

Definition choose_candidate_build_strategy
  (source : list Entry) (delta : BagDelta) : CandidateBuildStrategy :=
  if bag_delta_requires_prune_b source delta
  then IndependentCanonicalStreamRebuild
  else if additions_are_value_only_b source (bag_delta_additions delta)
       then GuardedValueOnlySetVal
       else PrivateShallowCowInsertion.

Theorem pruning_delta_uses_independent_stream :
  forall source delta,
    bag_delta_requires_prune_b source delta = true ->
    choose_candidate_build_strategy source delta =
      IndependentCanonicalStreamRebuild.
Proof.
  intros source delta Hprune.
  unfold choose_candidate_build_strategy. rewrite Hprune. reflexivity.
Qed.

Theorem nonpruning_value_only_delta_uses_guarded_set_val :
  forall source delta,
    bag_delta_requires_prune_b source delta = false ->
    additions_are_value_only_b source (bag_delta_additions delta) = true ->
    choose_candidate_build_strategy source delta = GuardedValueOnlySetVal.
Proof.
  intros source delta Hprune Hvalues.
  unfold choose_candidate_build_strategy.
  rewrite Hprune, Hvalues. reflexivity.
Qed.

Theorem nonpruning_insertion_delta_uses_private_cow :
  forall source delta,
    bag_delta_requires_prune_b source delta = false ->
    additions_are_value_only_b source (bag_delta_additions delta) = false ->
    choose_candidate_build_strategy source delta = PrivateShallowCowInsertion.
Proof.
  intros source delta Hprune Hinsertion.
  unfold choose_candidate_build_strategy.
  rewrite Hprune, Hinsertion. reflexivity.
Qed.

Definition apply_bag_delta
  (source : list Entry) (delta : BagDelta) : option (list Entry) :=
  let after_removal :=
    match bag_delta_removal_mode delta with
    | StrictSubtract =>
        strict_exact_remove source (bag_delta_subtractions delta)
    | IdempotentDelete =>
        Some (idempotent_exact_delete source (bag_delta_deletions delta))
    end in
  match after_removal with
  | None => None
  | Some retained =>
      checked_exact_upsert retained (bag_delta_additions delta)
  end.

Definition atomic_bag_delta
  (source : list Entry) (delta : BagDelta) : list Entry :=
  match apply_bag_delta source delta with
  | Some candidate => candidate
  | None => source
  end.

Theorem failed_bag_delta_preserves_source :
  forall source delta,
    apply_bag_delta source delta = None ->
    atomic_bag_delta source delta = source.
Proof.
  intros source delta Hfailure.
  unfold atomic_bag_delta. rewrite Hfailure. reflexivity.
Qed.

(** Closed traces are generated from the operation inputs and the successful
    logical result.  The projections below are therefore consequences of the
    trace constructor, rather than scenario assumptions. *)

Definition closed_operation_trace
  (source removals : list Entry)
  (deletions : list Path)
  (additions output : list Entry) : list StreamEvent :=
  map RemovalAdvance removals
  ++ map ExactDeleteAdvance deletions
  ++ map ReplacementAdvance additions
  ++ map SourceAdvance source
  ++ map DestinationEmit output.

Lemma source_projection_app :
  forall left right,
    source_projection (left ++ right) =
      source_projection left ++ source_projection right.
Proof.
  induction left as [|event tail IH]; intros right; simpl; [reflexivity|].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma removal_projection_app :
  forall left right,
    removal_projection (left ++ right) =
      removal_projection left ++ removal_projection right.
Proof.
  induction left as [|event tail IH]; intros right; simpl; [reflexivity|].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma replacement_projection_app :
  forall left right,
    replacement_projection (left ++ right) =
      replacement_projection left ++ replacement_projection right.
Proof.
  induction left as [|event tail IH]; intros right; simpl; [reflexivity|].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma emitted_trace_app :
  forall left right,
    emitted_trace (left ++ right) =
      emitted_trace left ++ emitted_trace right.
Proof.
  induction left as [|event tail IH]; intros right; simpl; [reflexivity|].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma deletion_projection_app :
  forall left right,
    deletion_projection (left ++ right) =
      deletion_projection left ++ deletion_projection right.
Proof.
  induction left as [|event tail IH]; intros right; simpl; [reflexivity|].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma source_projection_source_events :
  forall entries, source_projection (map SourceAdvance entries) = entries.
Proof. induction entries; simpl; congruence. Qed.

Lemma removal_projection_removal_events :
  forall entries, removal_projection (map RemovalAdvance entries) = entries.
Proof. induction entries; simpl; congruence. Qed.

Lemma replacement_projection_replacement_events :
  forall entries,
    replacement_projection (map ReplacementAdvance entries) = entries.
Proof. induction entries; simpl; congruence. Qed.

Lemma emitted_trace_destination_events :
  forall entries, emitted_trace (map DestinationEmit entries) = entries.
Proof. induction entries; simpl; congruence. Qed.

Lemma deletion_projection_deletion_events :
  forall paths, deletion_projection (map ExactDeleteAdvance paths) = paths.
Proof. induction paths; simpl; congruence. Qed.

Lemma source_projection_removal_events :
  forall entries, source_projection (map RemovalAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma source_projection_deletion_events :
  forall paths, source_projection (map ExactDeleteAdvance paths) = [].
Proof. induction paths; simpl; congruence. Qed.

Lemma source_projection_replacement_events :
  forall entries, source_projection (map ReplacementAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma source_projection_destination_events :
  forall entries, source_projection (map DestinationEmit entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma removal_projection_deletion_events :
  forall paths, removal_projection (map ExactDeleteAdvance paths) = [].
Proof. induction paths; simpl; congruence. Qed.

Lemma removal_projection_replacement_events :
  forall entries, removal_projection (map ReplacementAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma removal_projection_source_events :
  forall entries, removal_projection (map SourceAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma removal_projection_destination_events :
  forall entries, removal_projection (map DestinationEmit entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma replacement_projection_removal_events :
  forall entries, replacement_projection (map RemovalAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma replacement_projection_deletion_events :
  forall paths, replacement_projection (map ExactDeleteAdvance paths) = [].
Proof. induction paths; simpl; congruence. Qed.

Lemma replacement_projection_source_events :
  forall entries, replacement_projection (map SourceAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma replacement_projection_destination_events :
  forall entries, replacement_projection (map DestinationEmit entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma deletion_projection_removal_events :
  forall entries, deletion_projection (map RemovalAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma deletion_projection_replacement_events :
  forall entries, deletion_projection (map ReplacementAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma deletion_projection_source_events :
  forall entries, deletion_projection (map SourceAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma deletion_projection_destination_events :
  forall entries, deletion_projection (map DestinationEmit entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma emitted_trace_removal_events :
  forall entries, emitted_trace (map RemovalAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma emitted_trace_deletion_events :
  forall paths, emitted_trace (map ExactDeleteAdvance paths) = [].
Proof. induction paths; simpl; congruence. Qed.

Lemma emitted_trace_replacement_events :
  forall entries, emitted_trace (map ReplacementAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma emitted_trace_source_events :
  forall entries, emitted_trace (map SourceAdvance entries) = [].
Proof. induction entries; simpl; congruence. Qed.

Lemma source_projection_non_source_events :
  forall removals deletions additions output,
    source_projection
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map DestinationEmit output) = [].
Proof.
  intros removals deletions additions output.
  repeat rewrite source_projection_app.
  rewrite source_projection_removal_events,
    source_projection_deletion_events,
    source_projection_replacement_events,
    source_projection_destination_events.
  reflexivity.
Qed.

Lemma removal_projection_non_removal_events :
  forall source deletions additions output,
    removal_projection
      (map ExactDeleteAdvance deletions ++ map ReplacementAdvance additions ++
       map SourceAdvance source ++ map DestinationEmit output) = [].
Proof.
  intros source deletions additions output.
  repeat rewrite removal_projection_app.
  rewrite removal_projection_deletion_events,
    removal_projection_replacement_events,
    removal_projection_source_events,
    removal_projection_destination_events.
  reflexivity.
Qed.

Lemma replacement_projection_non_replacement_events :
  forall source removals deletions output,
    replacement_projection
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map SourceAdvance source ++ map DestinationEmit output) = [].
Proof.
  intros source removals deletions output.
  repeat rewrite replacement_projection_app.
  rewrite replacement_projection_removal_events,
    replacement_projection_deletion_events,
    replacement_projection_source_events,
    replacement_projection_destination_events.
  reflexivity.
Qed.

Lemma deletion_projection_non_deletion_events :
  forall source removals additions output,
    deletion_projection
      (map RemovalAdvance removals ++ map ReplacementAdvance additions ++
       map SourceAdvance source ++ map DestinationEmit output) = [].
Proof.
  intros source removals additions output.
  repeat rewrite deletion_projection_app.
  rewrite deletion_projection_removal_events,
    deletion_projection_replacement_events,
    deletion_projection_source_events,
    deletion_projection_destination_events.
  reflexivity.
Qed.

Lemma emitted_trace_non_destination_events :
  forall source removals deletions additions,
    emitted_trace
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map SourceAdvance source) = [].
Proof.
  intros source removals deletions additions.
  repeat rewrite emitted_trace_app.
  rewrite emitted_trace_removal_events,
    emitted_trace_deletion_events,
    emitted_trace_replacement_events,
    emitted_trace_source_events.
  reflexivity.
Qed.

Theorem closed_operation_trace_refines_all_projections :
  forall source removals deletions additions output,
    let trace := closed_operation_trace
      source removals deletions additions output in
    source_projection trace = source /\
    removal_projection trace = removals /\
    deletion_projection trace = deletions /\
    replacement_projection trace = additions /\
    emitted_trace trace = output /\
    source_event_count trace = length source /\
    removal_event_count trace = length removals /\
    deletion_event_count trace = length deletions /\
    replacement_event_count trace = length additions /\
    destination_event_count trace = length output.
Proof.
  intros source removals deletions additions output; simpl.
  unfold closed_operation_trace.
  assert (Hsource_count :
    source_event_count
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map SourceAdvance source ++
       map DestinationEmit output) = length source).
  { rewrite source_event_count_is_projection_length.
    repeat rewrite source_projection_app.
    rewrite source_projection_removal_events,
      source_projection_deletion_events,
      source_projection_replacement_events,
      source_projection_source_events,
      source_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity. }
  assert (Hremoval_count :
    removal_event_count
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map SourceAdvance source ++
       map DestinationEmit output) = length removals).
  { rewrite removal_event_count_is_projection_length.
    repeat rewrite removal_projection_app.
    rewrite removal_projection_removal_events,
      removal_projection_deletion_events,
      removal_projection_replacement_events,
      removal_projection_source_events,
      removal_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity. }
  assert (Hdeletion_count :
    deletion_event_count
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map SourceAdvance source ++
       map DestinationEmit output) = length deletions).
  { rewrite deletion_event_count_is_projection_length.
    repeat rewrite deletion_projection_app.
    rewrite deletion_projection_removal_events,
      deletion_projection_deletion_events,
      deletion_projection_replacement_events,
      deletion_projection_source_events,
      deletion_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity. }
  assert (Hreplacement_count :
    replacement_event_count
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map SourceAdvance source ++
       map DestinationEmit output) = length additions).
  { rewrite replacement_event_count_is_projection_length.
    repeat rewrite replacement_projection_app.
    rewrite replacement_projection_removal_events,
      replacement_projection_deletion_events,
      replacement_projection_replacement_events,
      replacement_projection_source_events,
      replacement_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity. }
  assert (Hdestination_count :
    destination_event_count
      (map RemovalAdvance removals ++ map ExactDeleteAdvance deletions ++
       map ReplacementAdvance additions ++ map SourceAdvance source ++
       map DestinationEmit output) = length output).
  { rewrite destination_event_count_is_projection_length.
    repeat rewrite emitted_trace_app.
    rewrite emitted_trace_removal_events,
      emitted_trace_deletion_events,
      emitted_trace_replacement_events,
      emitted_trace_source_events,
      emitted_trace_destination_events.
    simpl. try rewrite app_nil_r. reflexivity. }
  repeat split.
  - repeat rewrite source_projection_app.
    rewrite source_projection_removal_events,
      source_projection_deletion_events,
      source_projection_replacement_events,
      source_projection_source_events,
      source_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity.
  - repeat rewrite removal_projection_app.
    rewrite removal_projection_removal_events,
      removal_projection_deletion_events,
      removal_projection_replacement_events,
      removal_projection_source_events,
      removal_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity.
  - repeat rewrite deletion_projection_app.
    rewrite deletion_projection_removal_events,
      deletion_projection_deletion_events,
      deletion_projection_replacement_events,
      deletion_projection_source_events,
      deletion_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity.
  - repeat rewrite replacement_projection_app.
    rewrite replacement_projection_removal_events,
      replacement_projection_deletion_events,
      replacement_projection_replacement_events,
      replacement_projection_source_events,
      replacement_projection_destination_events.
    simpl. try rewrite app_nil_r. reflexivity.
  - repeat rewrite emitted_trace_app.
    rewrite emitted_trace_removal_events,
      emitted_trace_deletion_events,
      emitted_trace_replacement_events,
      emitted_trace_source_events,
      emitted_trace_destination_events.
    simpl. try rewrite app_nil_r. reflexivity.
  - exact Hsource_count.
  - exact Hremoval_count.
  - exact Hdeletion_count.
  - exact Hreplacement_count.
  - exact Hdestination_count.
Qed.

Theorem closed_operation_trace_linear_in_inputs_and_output :
  forall source removals deletions additions output,
    length (closed_operation_trace source removals deletions additions output) =
      length source + length removals + length deletions +
      length additions + length output.
Proof.
  intros source removals deletions additions output.
  unfold closed_operation_trace.
  repeat rewrite length_app. repeat rewrite map_length. lia.
Qed.

Theorem strict_exact_cursor_to_logical_transform_refinement :
  forall source removals output,
    strict_exact_remove source removals = Some output ->
    exact_stream_transform source removals = Some output /\
    let trace := closed_operation_trace source removals [] [] output in
    source_projection trace = source /\
    removal_projection trace = removals /\
    deletion_projection trace = [] /\
    replacement_projection trace = [] /\
    emitted_trace trace = output /\
    length trace = length source + length removals + length output.
Proof.
  intros source removals output Hsuccess.
  unfold strict_exact_remove in Hsuccess.
  destruct (strict_removal_preflightb source removals) eqn:Hpreflight;
    try discriminate.
  split.
  - exact Hsuccess.
  - pose proof (closed_operation_trace_refines_all_projections
      source removals [] [] output) as Hprojections.
    simpl in Hprojections. destruct Hprojections as
      [Hsource [Hremoval [Hdelete [Hreplace [Hemit Hcounts]]]]].
    repeat split; try assumption.
    rewrite closed_operation_trace_linear_in_inputs_and_output. simpl. lia.
Qed.

Theorem idempotent_delete_cursor_to_logical_transform_refinement :
  forall source deletions,
    let output := idempotent_exact_delete source deletions in
    let trace := closed_operation_trace source [] deletions [] output in
    source_projection trace = source /\
    removal_projection trace = [] /\
    deletion_projection trace = deletions /\
    replacement_projection trace = [] /\
    emitted_trace trace = output /\
    length trace = length source + length deletions + length output.
Proof.
  intros source deletions; simpl.
  pose proof (closed_operation_trace_refines_all_projections
    source [] deletions [] (idempotent_exact_delete source deletions))
    as Hprojections.
  simpl in Hprojections. destruct Hprojections as
    [Hsource [Hremoval [Hdelete [Hreplace [Hemit _]]]]].
  repeat split; try assumption.
  rewrite closed_operation_trace_linear_in_inputs_and_output. simpl. lia.
Qed.

Theorem checked_upsert_cursor_to_logical_transform_refinement :
  forall source additions output,
    checked_exact_upsert source additions = Some output ->
    let trace := closed_operation_trace source [] [] additions output in
    source_projection trace = source /\
    removal_projection trace = [] /\
    deletion_projection trace = [] /\
    replacement_projection trace = additions /\
    emitted_trace trace = output /\
    length trace = length source + length additions + length output.
Proof.
  intros source additions output Hsuccess; simpl.
  pose proof (closed_operation_trace_refines_all_projections
    source [] [] additions output) as Hprojections.
  simpl in Hprojections. destruct Hprojections as
    [Hsource [Hremoval [Hdelete [Hreplace [Hemit _]]]]].
  repeat split; try assumption.
  rewrite closed_operation_trace_linear_in_inputs_and_output. simpl. lia.
Qed.

(** Prefix replacements are expressed relative to the selected prefix.  No
    synthetic successor is used: below/inside/above are direct comparisons
    with the prefix and the prefix relation. *)
Definition absolutize_entry (prefix : Path) (entry : Entry) : Entry :=
  (prefix ++ entry_path entry, entry_multiplicity entry).

Definition absolute_replacements
  (prefix : Path) (relative : list Entry) : list Entry :=
  map (absolutize_entry prefix) relative.

Definition entry_is_in_region_b
  (region : PrefixPosition) (prefix : Path) (entry : Entry) : bool :=
  match region, classify_prefix prefix (entry_path entry) with
  | PrefixBelow, PrefixBelow => true
  | PrefixInside, PrefixInside => true
  | PrefixAbove, PrefixAbove => true
  | _, _ => false
  end.

Definition entries_in_region
  (region : PrefixPosition) (prefix : Path) (entries : list Entry) : list Entry :=
  filter (entry_is_in_region_b region prefix) entries.

Lemma entries_in_region_is_native_prefix_selection :
  forall region prefix entries,
    entries_in_region region prefix entries =
      select_prefix_position region prefix entries.
Proof.
  intros region prefix entries.
  induction entries as [|entry tail IH]; simpl; [reflexivity|].
  unfold entry_is_in_region_b, prefix_position_eqb.
  destruct region, (classify_prefix prefix (entry_path entry));
    simpl; rewrite IH; reflexivity.
Qed.

Inductive PrefixReplacementMode : Type :=
| ClearPrefixRegion
| PreservePrefixRegion.

Definition clear_prefix_replace
  (source : list Entry) (prefix : Path) (relative : list Entry) : list Entry :=
  entries_in_region PrefixBelow prefix source
  ++ absolute_replacements prefix relative
  ++ entries_in_region PrefixAbove prefix source.

Theorem clear_prefix_replace_is_native_filter_splice :
  forall source prefix relative,
    clear_prefix_replace source prefix relative =
      prefix_splice_stream prefix source
        (absolute_replacements prefix relative).
Proof.
  intros source prefix relative.
  unfold clear_prefix_replace, prefix_splice_stream,
    prefix_below_stream, prefix_above_stream.
  repeat rewrite entries_in_region_is_native_prefix_selection.
  reflexivity.
Qed.

Definition preserve_prefix_replace
  (source : list Entry) (prefix : Path) (relative : list Entry)
  : option (list Entry) :=
  match checked_exact_upsert
          (entries_in_region PrefixInside prefix source)
          (absolute_replacements prefix relative) with
  | None => None
  | Some inside =>
      Some (entries_in_region PrefixBelow prefix source
            ++ inside
            ++ entries_in_region PrefixAbove prefix source)
  end.

Definition apply_relative_prefix_replace
  (mode : PrefixReplacementMode)
  (source : list Entry) (prefix : Path) (relative : list Entry)
  : option (list Entry) :=
  match mode with
  | ClearPrefixRegion => Some (clear_prefix_replace source prefix relative)
  | PreservePrefixRegion => preserve_prefix_replace source prefix relative
  end.

Definition native_relative_prefix_subtract
  (source : list Entry) (prefix : Path) (relative_removals : list Entry)
  : option (list Entry) :=
  strict_exact_remove source
    (absolute_replacements prefix relative_removals).

Theorem native_relative_prefix_subtract_refines_complete_byte_paths :
  forall source prefix relative_removals output,
    native_relative_prefix_subtract source prefix relative_removals =
      Some output ->
    exact_stream_transform source
      (absolute_replacements prefix relative_removals) = Some output.
Proof.
  intros source prefix relative_removals output Hsuccess.
  unfold native_relative_prefix_subtract in Hsuccess.
  eapply strict_exact_cursor_to_logical_transform_refinement in Hsuccess.
  tauto.
Qed.

Theorem native_relative_prefix_subtract_never_targets_outside_prefix :
  forall prefix relative_removals removal,
    In removal (absolute_replacements prefix relative_removals) ->
    classify_prefix prefix (entry_path removal) = PrefixInside.
Proof.
  intros prefix relative_removals removal Hin.
  unfold absolute_replacements in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [relative [Hequal _]]. subst removal.
  destruct relative as [relative_path relative_count].
  unfold absolutize_entry; simpl.
  apply classify_prefix_inside_iff.
  exists relative_path. reflexivity.
Qed.

Lemma absolutized_path_has_prefix :
  forall prefix entry,
    path_prefix prefix (entry_path (absolutize_entry prefix entry)).
Proof.
  intros prefix [path count].
  exists path. reflexivity.
Qed.

Theorem absolutized_entry_classifies_inside :
  forall prefix entry,
    classify_prefix prefix (entry_path (absolutize_entry prefix entry)) =
      PrefixInside.
Proof.
  intros prefix entry.
  apply classify_prefix_inside_iff.
  apply absolutized_path_has_prefix.
Qed.

Lemma every_absolute_replacement_is_inside :
  forall prefix relative,
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      (absolute_replacements prefix relative).
Proof.
  intros prefix relative. unfold absolute_replacements.
  induction relative as [|entry tail IH]; simpl.
  - constructor.
  - constructor.
    + apply absolutized_entry_classifies_inside.
    + exact IH.
Qed.

Lemma path_valid_append :
  forall left right,
    path_valid left = true ->
    path_valid right = true ->
    path_valid (left ++ right) = true.
Proof.
  intros left right Hleft Hright.
  unfold path_valid in *. rewrite forallb_app, Hleft, Hright. reflexivity.
Qed.

Lemma path_lex_lt_under_common_prefix :
  forall prefix left right,
    path_lex_lt left right ->
    path_lex_lt (prefix ++ left) (prefix ++ right).
Proof.
  induction prefix as [|byte tail IH]; intros left right Horder; simpl.
  - exact Horder.
  - apply PathLexTail. apply IH. exact Horder.
Qed.

Lemma absolute_replacements_preserve_tail_order :
  forall prefix path tail,
    Forall (fun entry => path_lex_lt path (entry_path entry)) tail ->
    Forall
      (fun entry => path_lex_lt (prefix ++ path) (entry_path entry))
      (absolute_replacements prefix tail).
Proof.
  intros prefix path tail Horder.
  induction Horder as [|[tail_path tail_count] tail Hhead Htail IH]; simpl.
  - constructor.
  - constructor.
    + simpl. apply path_lex_lt_under_common_prefix. exact Hhead.
    + exact IH.
Qed.

Theorem absolute_replacements_preserve_canonicality :
  forall prefix relative,
    path_valid prefix = true ->
    canonical_entries relative ->
    canonical_entries (absolute_replacements prefix relative).
Proof.
  intros prefix relative Hprefix Hrelative.
  induction Hrelative as
    [|path count tail Hpath Hcount Horder Htail IH]; simpl.
  - constructor.
  - apply CanonicalEntriesCons.
    + apply path_valid_append; assumption.
    + exact Hcount.
    + apply absolute_replacements_preserve_tail_order. exact Horder.
    + exact IH.
Qed.

Theorem clear_prefix_replace_is_extensionally_exact :
  forall source prefix relative queried,
    lookup_count queried (clear_prefix_replace source prefix relative) =
      match classify_prefix prefix queried with
      | PrefixInside =>
          lookup_count queried (absolute_replacements prefix relative)
      | PrefixBelow | PrefixAbove => lookup_count queried source
      end.
Proof.
  intros source prefix relative queried.
  rewrite clear_prefix_replace_is_native_filter_splice.
  apply prefix_splice_stream_is_extensionally_correct.
  apply every_absolute_replacement_is_inside.
Qed.

Definition paths_ordered_before
  (left right : list Entry) : Prop :=
  forall left_entry right_entry,
    In left_entry left ->
    In right_entry right ->
    path_lex_lt (entry_path left_entry) (entry_path right_entry).

Lemma paths_ordered_before_implies_ordered_before :
  forall left right,
    paths_ordered_before left right -> ordered_before left right.
Proof.
  intros left right Hordered.
  unfold paths_ordered_before, ordered_before in *.
  apply Forall_forall. intros left_entry Hleft.
  apply Forall_forall. intros right_entry Hright.
  eapply Hordered; eauto.
Qed.

Lemma selected_region_members_classify :
  forall region prefix entries entry,
    In entry (entries_in_region region prefix entries) ->
    classify_prefix prefix (entry_path entry) = region.
Proof.
  intros region prefix entries entry Hin.
  unfold entries_in_region in Hin.
  apply filter_In in Hin. destruct Hin as [_ Hregion].
  unfold entry_is_in_region_b in Hregion.
  destruct region, (classify_prefix prefix (entry_path entry));
    simpl in Hregion; try discriminate; reflexivity.
Qed.

Theorem splice_below_precedes_relative_replacements :
  forall source prefix relative,
    paths_ordered_before
      (entries_in_region PrefixBelow prefix source)
      (absolute_replacements prefix relative).
Proof.
  intros source prefix relative left right Hleft Hright.
  apply (selected_region_members_classify
    PrefixBelow prefix source left) in Hleft.
  unfold absolute_replacements in Hright.
  apply in_map_iff in Hright.
  destruct Hright as [relative_entry [Hright _]]. subst right.
  unfold absolutize_entry; simpl.
  apply prefix_below_is_before_every_prefix_extension
    with (prefix := prefix).
  - exact Hleft.
  - exists (entry_path relative_entry). reflexivity.
Qed.

Theorem splice_relative_replacements_precede_above :
  forall source prefix relative,
    paths_ordered_before
      (absolute_replacements prefix relative)
      (entries_in_region PrefixAbove prefix source).
Proof.
  intros source prefix relative left right Hleft Hright.
  unfold absolute_replacements in Hleft.
  apply in_map_iff in Hleft.
  destruct Hleft as [relative_entry [Hleft _]]. subst left.
  apply (selected_region_members_classify
    PrefixAbove prefix source right) in Hright.
  unfold absolutize_entry; simpl.
  apply every_prefix_extension_is_before_prefix_above
    with (prefix := prefix).
  - exists (entry_path relative_entry). reflexivity.
  - exact Hright.
Qed.

Theorem splice_below_precedes_above :
  forall source prefix,
    paths_ordered_before
      (entries_in_region PrefixBelow prefix source)
      (entries_in_region PrefixAbove prefix source).
Proof.
  intros source prefix left right Hleft Hright.
  apply (selected_region_members_classify
    PrefixBelow prefix source left) in Hleft.
  apply (selected_region_members_classify
    PrefixAbove prefix source right) in Hright.
  eapply path_lex_lt_transitive.
  - apply prefix_below_is_before_every_prefix_extension
      with (prefix := prefix).
    + exact Hleft.
    + exists ([] : Path).
      rewrite app_nil_r.
      reflexivity.
  - apply every_prefix_extension_is_before_prefix_above
      with (prefix := prefix).
    + exists ([] : Path).
      rewrite app_nil_r.
      reflexivity.
    + exact Hright.
Qed.

Theorem clear_prefix_replace_preserves_canonicality :
  forall source prefix relative,
    canonical_entries source ->
    path_valid prefix = true ->
    canonical_entries relative ->
    canonical_entries (clear_prefix_replace source prefix relative).
Proof.
  intros source prefix relative Hsource Hprefix Hrelative.
  rewrite clear_prefix_replace_is_native_filter_splice.
  apply prefix_splice_stream_preserves_canonicality.
  constructor.
  - exact Hsource.
  - apply absolute_replacements_preserve_canonicality; assumption.
  - apply every_absolute_replacement_is_inside.
  - apply paths_ordered_before_implies_ordered_before.
    unfold prefix_below_stream.
    rewrite <- entries_in_region_is_native_prefix_selection.
    apply splice_below_precedes_relative_replacements.
  - apply paths_ordered_before_implies_ordered_before.
    unfold prefix_below_stream, prefix_above_stream.
    repeat rewrite <- entries_in_region_is_native_prefix_selection.
    apply splice_below_precedes_above.
  - apply paths_ordered_before_implies_ordered_before.
    unfold prefix_above_stream.
    rewrite <- entries_in_region_is_native_prefix_selection.
    apply splice_relative_replacements_precede_above.
Qed.

Lemma selected_below_precedes_inside_entries :
  forall source prefix inside,
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      inside ->
    paths_ordered_before
      (entries_in_region PrefixBelow prefix source) inside.
Proof.
  intros source prefix inside Hinside left right Hleft Hright.
  apply selected_region_members_classify in Hleft.
  rewrite Forall_forall in Hinside.
  specialize (Hinside right Hright).
  apply prefix_below_is_before_every_prefix_extension with (prefix := prefix).
  - exact Hleft.
  - apply classify_prefix_inside_iff. exact Hinside.
Qed.

Lemma inside_entries_precede_selected_above :
  forall source prefix inside,
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      inside ->
    paths_ordered_before inside
      (entries_in_region PrefixAbove prefix source).
Proof.
  intros source prefix inside Hinside left right Hleft Hright.
  rewrite Forall_forall in Hinside.
  specialize (Hinside left Hleft).
  apply selected_region_members_classify in Hright.
  apply every_prefix_extension_is_before_prefix_above with (prefix := prefix).
  - apply classify_prefix_inside_iff. exact Hinside.
  - exact Hright.
Qed.

Theorem preserve_prefix_replace_preserves_canonicality :
  forall source prefix relative output,
    canonical_entries source ->
    path_valid prefix = true ->
    canonical_entries relative ->
    preserve_prefix_replace source prefix relative = Some output ->
    canonical_entries output.
Proof.
  intros source prefix relative output Hsource Hprefix Hrelative Hsuccess.
  unfold preserve_prefix_replace in Hsuccess.
  destruct (checked_exact_upsert
    (entries_in_region PrefixInside prefix source)
    (absolute_replacements prefix relative)) as [inside|] eqn:Hinside;
    try discriminate.
  inversion Hsuccess; subst output.
  assert (Hselected_canonical :
    canonical_entries (entries_in_region PrefixInside prefix source)).
  { rewrite entries_in_region_is_native_prefix_selection.
    apply select_prefix_position_preserves_canonicality. exact Hsource. }
  assert (Habsolutized_canonical :
    canonical_entries (absolute_replacements prefix relative)).
  { apply absolute_replacements_preserve_canonicality; assumption. }
  assert (Hinside_canonical : canonical_entries inside).
  { exact (checked_exact_upsert_preserves_canonicality
      (entries_in_region PrefixInside prefix source)
      (absolute_replacements prefix relative)
      inside Hselected_canonical Habsolutized_canonical Hinside). }
  assert (Hinside_region :
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      inside).
  { eapply (checked_exact_upsert_preserves_path_forall
      (fun path => classify_prefix prefix path = PrefixInside)
      (entries_in_region PrefixInside prefix source)
      (absolute_replacements prefix relative) inside).
    - apply Forall_forall. intros entry Hin.
      eapply selected_region_members_classify. exact Hin.
    - apply every_absolute_replacement_is_inside.
    - exact Hinside. }
  rewrite entries_in_region_is_native_prefix_selection
    in Hselected_canonical, Hinside.
  repeat rewrite entries_in_region_is_native_prefix_selection.
  apply prefix_splice_stream_preserves_canonicality.
  constructor.
  - exact Hsource.
  - exact Hinside_canonical.
  - exact Hinside_region.
  - apply paths_ordered_before_implies_ordered_before.
    unfold prefix_below_stream.
    rewrite <- entries_in_region_is_native_prefix_selection.
    apply selected_below_precedes_inside_entries. exact Hinside_region.
  - apply paths_ordered_before_implies_ordered_before.
    unfold prefix_below_stream, prefix_above_stream.
    repeat rewrite <- entries_in_region_is_native_prefix_selection.
    apply splice_below_precedes_above.
  - apply paths_ordered_before_implies_ordered_before.
    unfold prefix_above_stream.
    rewrite <- entries_in_region_is_native_prefix_selection.
    apply inside_entries_precede_selected_above. exact Hinside_region.
Qed.

Theorem preserve_prefix_replace_is_extensionally_exact :
  forall source prefix relative output queried,
    canonical_entries source ->
    path_valid prefix = true ->
    canonical_entries relative ->
    preserve_prefix_replace source prefix relative = Some output ->
    lookup_count queried output =
      match classify_prefix prefix queried with
      | PrefixInside =>
          (lookup_count queried source +
           lookup_count queried (absolute_replacements prefix relative))%N
      | PrefixBelow | PrefixAbove => lookup_count queried source
      end.
Proof.
  intros source prefix relative output queried Hsource Hprefix Hrelative Hsuccess.
  unfold preserve_prefix_replace in Hsuccess.
  destruct (checked_exact_upsert
    (entries_in_region PrefixInside prefix source)
    (absolute_replacements prefix relative)) as [inside|] eqn:Hinside;
    try discriminate.
  inversion Hsuccess; subst output.
  assert (Hselected_canonical :
    canonical_entries (entries_in_region PrefixInside prefix source)).
  { rewrite entries_in_region_is_native_prefix_selection.
    apply select_prefix_position_preserves_canonicality. exact Hsource. }
  assert (Habsolutized_canonical :
    canonical_entries (absolute_replacements prefix relative)).
  { apply absolute_replacements_preserve_canonicality; assumption. }
  assert (Hinside_region :
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      inside).
  { eapply (checked_exact_upsert_preserves_path_forall
      (fun path => classify_prefix prefix path = PrefixInside)
      (entries_in_region PrefixInside prefix source)
      (absolute_replacements prefix relative) inside).
    - apply Forall_forall. intros entry Hin.
      eapply selected_region_members_classify. exact Hin.
    - apply every_absolute_replacement_is_inside.
    - exact Hinside. }
  rewrite entries_in_region_is_native_prefix_selection
    in Hselected_canonical, Hinside.
  repeat rewrite entries_in_region_is_native_prefix_selection.
  change
    (lookup_count queried (prefix_splice_stream prefix source inside) =
      match classify_prefix prefix queried with
      | PrefixInside =>
          (lookup_count queried source +
           lookup_count queried (absolute_replacements prefix relative))%N
      | PrefixBelow | PrefixAbove => lookup_count queried source
      end).
  rewrite prefix_splice_stream_is_extensionally_correct
    by exact Hinside_region.
  destruct (classify_prefix prefix queried) eqn:Hposition; simpl.
  - reflexivity.
  - rewrite (checked_exact_upsert_is_extensionally_exact
      (select_prefix_position PrefixInside prefix source)
      (absolute_replacements prefix relative) inside queried);
      try assumption.
    rewrite lookup_selected_position, Hposition. reflexivity.
  - reflexivity.
Qed.

Theorem relative_prefix_cursor_to_logical_transform_refinement :
  forall mode source prefix relative output,
    apply_relative_prefix_replace mode source prefix relative = Some output ->
    let additions := absolute_replacements prefix relative in
    let trace := closed_operation_trace source [] [] additions output in
    source_projection trace = source /\
    removal_projection trace = [] /\
    deletion_projection trace = [] /\
    replacement_projection trace = additions /\
    emitted_trace trace = output /\
    length trace = length source + length additions + length output.
Proof.
  intros mode source prefix relative output Hsuccess; simpl.
  pose proof (closed_operation_trace_refines_all_projections
    source [] [] (absolute_replacements prefix relative) output)
    as Hprojections.
  simpl in Hprojections. destruct Hprojections as
    [Hsource [Hremoval [Hdelete [Hreplace [Hemit _]]]]].
  repeat split; try assumption.
  rewrite closed_operation_trace_linear_in_inputs_and_output. simpl. lia.
Qed.

Theorem clear_prefix_mode_has_exact_logical_output :
  forall source prefix relative output,
    apply_relative_prefix_replace
      ClearPrefixRegion source prefix relative = Some output ->
    output = clear_prefix_replace source prefix relative.
Proof.
  intros source prefix relative output Hsuccess.
  unfold apply_relative_prefix_replace in Hsuccess.
  inversion Hsuccess. reflexivity.
Qed.

Theorem preserve_prefix_mode_has_checked_logical_output :
  forall source prefix relative output,
    apply_relative_prefix_replace
      PreservePrefixRegion source prefix relative = Some output ->
    exists inside,
      checked_exact_upsert
        (entries_in_region PrefixInside prefix source)
        (absolute_replacements prefix relative) = Some inside /\
      output = entries_in_region PrefixBelow prefix source
               ++ inside
               ++ entries_in_region PrefixAbove prefix source.
Proof.
  intros source prefix relative output Hsuccess.
  unfold apply_relative_prefix_replace, preserve_prefix_replace in Hsuccess.
  destruct (checked_exact_upsert
    (entries_in_region PrefixInside prefix source)
    (absolute_replacements prefix relative)) eqn:Hinside;
    try discriminate.
  inversion Hsuccess; subst output.
  exists l.
  split.
  - reflexivity.
  - reflexivity.
Qed.

Theorem empty_prefix_selects_every_path_inside :
  forall entries,
    entries_in_region PrefixInside [] entries = entries.
Proof.
  induction entries as [|[path count] tail IH]; simpl; [reflexivity|].
  assert (Hinside : classify_prefix [] path = PrefixInside).
  { apply classify_prefix_inside_iff. exists path. reflexivity. }
  unfold entry_is_in_region_b. simpl in Hinside |- *.
  rewrite Hinside. simpl. f_equal. exact IH.
Qed.

Theorem maximal_byte_prefix_needs_no_successor :
  forall suffix,
    classify_prefix [255] ([255] ++ suffix) = PrefixInside.
Proof.
  intros suffix. apply classify_prefix_inside_iff.
  exists suffix. reflexivity.
Qed.

(** The publication protocol is indexed by phase.  In particular, no value of
    [PreparedCandidateV2] can be built from an open cursor or failed logical
    operation: its sole constructor requires successful execution, a closed
    trace, canonical validation, checked metadata, and the structural image. *)
Inductive LogicalOperation : Type :=
| OpStrictSubtract (removals : list Entry)
| OpIdempotentDelete (deletions : list Path)
| OpCheckedUpsert (additions : list Entry)
| OpRelativePrefixReplace
    (mode : PrefixReplacementMode) (prefix : Path) (relative : list Entry)
| OpBagDelta (delta : BagDelta).

Definition execute_logical_operation
  (source : list Entry) (operation : LogicalOperation)
  : option (list Entry) :=
  match operation with
  | OpStrictSubtract removals => strict_exact_remove source removals
  | OpIdempotentDelete deletions =>
      Some (idempotent_exact_delete source deletions)
  | OpCheckedUpsert additions => checked_exact_upsert source additions
  | OpRelativePrefixReplace mode prefix relative =>
      apply_relative_prefix_replace mode source prefix relative
  | OpBagDelta delta => apply_bag_delta source delta
  end.

Inductive valid_logical_operation : list Entry -> LogicalOperation -> Prop :=
| ValidStrictSubtract :
    forall source removals,
      canonical_entries source ->
      canonical_entries removals ->
      valid_logical_operation source (OpStrictSubtract removals)
| ValidIdempotentDelete :
    forall source deletions,
      canonical_entries source ->
      valid_logical_operation source (OpIdempotentDelete deletions)
| ValidCheckedUpsert :
    forall source additions,
      canonical_entries source ->
      canonical_entries additions ->
      valid_logical_operation source (OpCheckedUpsert additions)
| ValidRelativePrefixReplace :
    forall source mode prefix relative,
      canonical_entries source ->
      path_valid prefix = true ->
      canonical_entries relative ->
      valid_logical_operation source
        (OpRelativePrefixReplace mode prefix relative)
| ValidStrictBagDelta :
    forall source removals additions,
      canonical_entries source ->
      canonical_entries removals ->
      canonical_entries additions ->
      valid_logical_operation source
        (OpBagDelta
          {| bag_delta_removal_mode := StrictSubtract;
             bag_delta_subtractions := removals;
             bag_delta_deletions := [];
             bag_delta_additions := additions |})
| ValidIdempotentBagDelta :
    forall source deletions additions,
      canonical_entries source ->
      canonical_entries additions ->
      valid_logical_operation source
        (OpBagDelta
          {| bag_delta_removal_mode := IdempotentDelete;
             bag_delta_subtractions := [];
             bag_delta_deletions := deletions;
             bag_delta_additions := additions |}).

Lemma valid_logical_operation_source_is_canonical :
  forall source operation,
    valid_logical_operation source operation ->
    canonical_entries source.
Proof.
  intros source operation Hvalid.
  inversion Hvalid; assumption.
Qed.

Theorem successful_valid_logical_operation_is_canonical :
  forall source operation output,
    valid_logical_operation source operation ->
    execute_logical_operation source operation = Some output ->
    canonical_entries output.
Proof.
  intros source operation output Hvalid Hsuccess.
  pose proof (valid_logical_operation_source_is_canonical
    source operation Hvalid) as Hsource_canonical.
  inversion Hvalid; subst; simpl in Hsuccess.
  - unfold strict_exact_remove in Hsuccess.
    destruct (strict_removal_preflightb source removals) eqn:Hpreflight;
      try discriminate.
    eapply (exact_stream_transform_preserves_canonicality
      source removals output).
    + assumption.
    + exact Hsuccess.
  - inversion Hsuccess; subst output.
    apply idempotent_exact_delete_preserves_canonicality. assumption.
  - eapply (checked_exact_upsert_preserves_canonicality
      source additions output).
    + assumption.
    + assumption.
    + exact Hsuccess.
  - destruct mode.
    + inversion Hsuccess; subst output.
      apply clear_prefix_replace_preserves_canonicality; assumption.
    + eapply (preserve_prefix_replace_preserves_canonicality
        source prefix relative output).
      * assumption.
      * assumption.
      * assumption.
      * exact Hsuccess.
  - unfold apply_bag_delta in Hsuccess; simpl in Hsuccess.
    destruct (strict_exact_remove source removals) as [retained|] eqn:Hretained;
      try discriminate.
    eapply checked_exact_upsert_preserves_canonicality.
    + unfold strict_exact_remove in Hretained.
      destruct (strict_removal_preflightb source removals) eqn:Hpreflight;
        try discriminate.
      eapply (exact_stream_transform_preserves_canonicality
        source removals retained).
      * assumption.
      * exact Hretained.
    + eassumption.
    + eassumption.
  - unfold apply_bag_delta in Hsuccess; simpl in Hsuccess.
    eapply checked_exact_upsert_preserves_canonicality.
    + apply idempotent_exact_delete_preserves_canonicality.
      exact Hsource_canonical.
    + eassumption.
    + eassumption.
Qed.

Definition logical_operation_removals
  (operation : LogicalOperation) : list Entry :=
  match operation with
  | OpStrictSubtract removals => removals
  | OpBagDelta delta => bag_delta_subtractions delta
  | _ => []
  end.

Definition logical_operation_deletions
  (operation : LogicalOperation) : list Path :=
  match operation with
  | OpIdempotentDelete deletions => deletions
  | OpBagDelta delta => bag_delta_deletions delta
  | _ => []
  end.

Definition logical_operation_additions
  (operation : LogicalOperation) : list Entry :=
  match operation with
  | OpCheckedUpsert additions => additions
  | OpRelativePrefixReplace _ prefix relative =>
      absolute_replacements prefix relative
  | OpBagDelta delta => bag_delta_additions delta
  | _ => []
  end.

Definition logical_operation_trace
  (source : list Entry) (operation : LogicalOperation) (output : list Entry)
  : list StreamEvent :=
  closed_operation_trace source
    (logical_operation_removals operation)
    (logical_operation_deletions operation)
    (logical_operation_additions operation)
    output.

Theorem logical_operation_output_has_linear_size_bound :
  forall source operation output,
    execute_logical_operation source operation = Some output ->
    length output <=
      3 * (S (length source +
              length (logical_operation_additions operation))).
Proof.
  intros source operation output Hsuccess.
  destruct operation as
    [removals|deletions|additions|mode prefix relative|delta]; simpl in *.
  - unfold strict_exact_remove in Hsuccess.
    destruct (strict_removal_preflightb source removals); try discriminate.
    pose proof (exact_stream_transform_output_length
      source removals output Hsuccess). lia.
  - inversion Hsuccess; subst output.
    pose proof (idempotent_exact_delete_output_length source deletions). lia.
  - pose proof (checked_exact_upsert_output_length
      source additions output Hsuccess). lia.
  - destruct mode.
    + inversion Hsuccess; subst output.
      unfold clear_prefix_replace.
      repeat rewrite length_app.
      pose proof (filter_length
        (entry_is_in_region_b PrefixBelow prefix) source) as Hbelow.
      pose proof (filter_length
        (entry_is_in_region_b PrefixAbove prefix) source) as Habove.
      unfold entries_in_region. lia.
    + unfold apply_relative_prefix_replace, preserve_prefix_replace in Hsuccess.
      destruct (checked_exact_upsert
        (entries_in_region PrefixInside prefix source)
        (absolute_replacements prefix relative))
        as [inside|] eqn:Hinside; try discriminate.
      inversion Hsuccess; subst. repeat rewrite length_app.
      pose proof (checked_exact_upsert_output_length
        (entries_in_region PrefixInside prefix source)
        (absolute_replacements prefix relative) inside Hinside) as Hmerged.
      pose proof (filter_length
        (entry_is_in_region_b PrefixBelow prefix) source) as Hbelow.
      pose proof (filter_length
        (entry_is_in_region_b PrefixInside prefix) source) as Hinsource.
      pose proof (filter_length
        (entry_is_in_region_b PrefixAbove prefix) source) as Habove.
      unfold entries_in_region in *. unfold absolute_replacements in *.
      repeat rewrite length_map in *.
      lia.
  - unfold apply_bag_delta in Hsuccess.
    destruct (bag_delta_removal_mode delta).
    + destruct (strict_exact_remove source (bag_delta_subtractions delta))
        as [retained|] eqn:Hretained; try discriminate.
      pose proof (checked_exact_upsert_output_length retained
        (bag_delta_additions delta) output Hsuccess) as Houtput.
      unfold strict_exact_remove in Hretained.
      destruct (strict_removal_preflightb source
        (bag_delta_subtractions delta)); try discriminate.
      pose proof (exact_stream_transform_output_length source
        (bag_delta_subtractions delta) retained Hretained) as Hretained_length.
      lia.
    + pose proof (checked_exact_upsert_output_length
        (idempotent_exact_delete source (bag_delta_deletions delta))
        (bag_delta_additions delta) output Hsuccess) as Houtput.
      pose proof (idempotent_exact_delete_output_length source
        (bag_delta_deletions delta)) as Hretained_length.
      lia.
Qed.

Record AtomicRootAuthority : Type := {
  atomic_root_identity : nat;
  atomic_root_entries : list Entry;
  atomic_root_image : list CompressedImageToken;
  atomic_root_metadata : Metadata;
  atomic_root_entries_canonical : canonical_entries atomic_root_entries;
  atomic_root_image_exact :
    atomic_root_image = canonical_compressed_image atomic_root_entries;
  atomic_root_metadata_exact :
    checked_metadata atomic_root_entries = Some atomic_root_metadata
}.

Record ExternalAliasFootprint : Type := {
  external_alias_root_identities : list nat;
  external_alias_node_identities : list nat;
  external_alias_value_identities : list nat
}.

Definition fresh_candidate_identity
  (source : AtomicRootAuthority)
  (aliases : ExternalAliasFootprint)
  (candidate_identity : nat) : Prop :=
  candidate_identity <> atomic_root_identity source /\
  ~ In candidate_identity (external_alias_root_identities aliases) /\
  ~ In candidate_identity (external_alias_node_identities aliases) /\
  ~ In candidate_identity (external_alias_value_identities aliases).

Definition complete_candidate_identity_footprint
  (source : AtomicRootAuthority) (aliases : ExternalAliasFootprint)
  : list nat :=
  atomic_root_identity source
  :: external_alias_root_identities aliases
     ++ external_alias_node_identities aliases
     ++ external_alias_value_identities aliases.

Definition allocate_fresh_candidate_identity
  (source : AtomicRootAuthority) (aliases : ExternalAliasFootprint) : nat :=
  fresh_identity (complete_candidate_identity_footprint source aliases).

Theorem allocated_candidate_identity_is_genuinely_outside_complete_footprint :
  forall source aliases,
    fresh_candidate_identity source aliases
      (allocate_fresh_candidate_identity source aliases).
Proof.
  intros source aliases.
  unfold fresh_candidate_identity, allocate_fresh_candidate_identity,
    complete_candidate_identity_footprint.
  pose proof (fresh_identity_is_outside_the_complete_footprint
    (atomic_root_identity source
     :: external_alias_root_identities aliases
        ++ external_alias_node_identities aliases
        ++ external_alias_value_identities aliases)) as Hfresh.
  repeat split; intro Hin.
  - apply Hfresh. left. symmetry. exact Hin.
  - apply Hfresh. right. apply in_or_app. left. exact Hin.
  - apply Hfresh. right. apply in_or_app. right.
    apply in_or_app. left. exact Hin.
  - apply Hfresh. right. apply in_or_app. right.
    apply in_or_app. right. exact Hin.
Qed.

Inductive PreparedCandidateV2 : Type :=
| PreparedFromClosedValidatedCursor :
    forall source operation output trace metadata image,
      execute_logical_operation source operation = Some output ->
      trace = logical_operation_trace source operation output ->
      canonical_entries output ->
      checked_metadata output = Some metadata ->
      image = canonical_compressed_image output ->
      PreparedCandidateV2.

Definition prepared_candidate_entries
  (candidate : PreparedCandidateV2) : list Entry :=
  match candidate with
  | PreparedFromClosedValidatedCursor _ _ output _ _ _ _ _ _ _ _ => output
  end.

Definition prepared_candidate_metadata
  (candidate : PreparedCandidateV2) : Metadata :=
  match candidate with
  | PreparedFromClosedValidatedCursor _ _ _ _ metadata _ _ _ _ _ _ => metadata
  end.

Definition prepared_candidate_image
  (candidate : PreparedCandidateV2) : list CompressedImageToken :=
  match candidate with
  | PreparedFromClosedValidatedCursor _ _ _ _ _ image _ _ _ _ _ => image
  end.

Inductive PublicationFaultPoint : Type :=
| FaultDuringPreflight
| FaultDuringBuild
| FaultDuringClose
| FaultDuringValidation
| FaultBeforePublication
| FaultAfterPublicationBeforeOldDrop
| FaultDuringCandidateUnwind.

Inductive PublicationPhase : Type :=
| PreflightPhase
| BuildPhase
| ClosedPhase
| ValidatedPhase
| PublishablePhase
| PublishedPhase
| AbortedPhase.

Inductive PublicationArtifact : PublicationPhase -> Type :=
| PreflightArtifact :
    AtomicRootAuthority -> LogicalOperation ->
    PublicationArtifact PreflightPhase
| BuildArtifact :
    forall source operation output,
      execute_logical_operation (atomic_root_entries source) operation =
        Some output ->
      PublicationArtifact BuildPhase
| ClosedArtifact :
    forall source operation output trace,
      execute_logical_operation (atomic_root_entries source) operation =
        Some output ->
      trace = logical_operation_trace
        (atomic_root_entries source) operation output ->
      PublicationArtifact ClosedPhase
| ValidatedArtifact :
    forall source operation output trace metadata,
      execute_logical_operation (atomic_root_entries source) operation =
        Some output ->
      trace = logical_operation_trace
        (atomic_root_entries source) operation output ->
      canonical_entries output ->
      checked_metadata output = Some metadata ->
      PublicationArtifact ValidatedPhase
| PublishableArtifact :
    forall source aliases (candidate : PreparedCandidateV2) candidate_identity,
      fresh_candidate_identity source aliases candidate_identity ->
      PublicationArtifact PublishablePhase
| PublishedArtifact :
    AtomicRootAuthority -> PublicationArtifact PublishedPhase
| AbortedArtifact :
    forall source operation,
      execute_logical_operation (atomic_root_entries source) operation = None ->
      PublicationArtifact AbortedPhase
| FaultAbortedArtifact :
    AtomicRootAuthority -> PublicationFaultPoint ->
    PublicationArtifact AbortedPhase.

Inductive PublicationTransition :
  forall {from to},
    PublicationArtifact from -> PublicationArtifact to -> Prop :=
| TransitionPreflightBuild :
    forall source operation output Hsuccess,
      PublicationTransition
        (PreflightArtifact source operation)
        (BuildArtifact source operation output Hsuccess)
| TransitionBuildClosed :
    forall source operation output Hsuccess trace Htrace,
      PublicationTransition
        (BuildArtifact source operation output Hsuccess)
        (ClosedArtifact source operation output trace Hsuccess Htrace)
| TransitionClosedValidated :
    forall source operation output trace metadata Hsuccess Htrace Hcanonical Hmetadata,
      PublicationTransition
        (ClosedArtifact source operation output trace Hsuccess Htrace)
        (ValidatedArtifact source operation output trace metadata
          Hsuccess Htrace Hcanonical Hmetadata)
| TransitionValidatedPublishable :
    forall source operation output trace metadata aliases candidate_identity
      Hsuccess Htrace Hcanonical Hmetadata Hfresh,
      PublicationTransition
        (ValidatedArtifact source operation output trace metadata
          Hsuccess Htrace Hcanonical Hmetadata)
        (PublishableArtifact source aliases
          (PreparedFromClosedValidatedCursor
            (atomic_root_entries source) operation output trace metadata
            (canonical_compressed_image output)
            Hsuccess Htrace Hcanonical Hmetadata eq_refl)
          candidate_identity Hfresh)
| TransitionPublishablePublished :
    forall source aliases candidate candidate_identity Hfresh,
      PublicationTransition
        (PublishableArtifact source aliases candidate candidate_identity Hfresh)
        (PublishedArtifact
          {| atomic_root_identity := candidate_identity;
             atomic_root_entries := prepared_candidate_entries candidate;
             atomic_root_image := prepared_candidate_image candidate;
             atomic_root_metadata := prepared_candidate_metadata candidate;
             atomic_root_entries_canonical :=
               match candidate with
               | PreparedFromClosedValidatedCursor
                   _ _ _ _ _ _ _ _ Hcanonical _ _ => Hcanonical
               end;
             atomic_root_image_exact :=
               match candidate with
               | PreparedFromClosedValidatedCursor
                   _ _ _ _ _ _ _ _ _ _ Himage => Himage
               end;
             atomic_root_metadata_exact :=
               match candidate with
               | PreparedFromClosedValidatedCursor
                   _ _ _ _ _ _ _ _ _ Hmetadata _ => Hmetadata
               end |})
| TransitionBuildFault :
    forall source operation output Hsuccess fault,
      PublicationTransition
        (BuildArtifact source operation output Hsuccess)
        (FaultAbortedArtifact source fault)
| TransitionClosedFault :
    forall source operation output trace Hsuccess Htrace fault,
      PublicationTransition
        (ClosedArtifact source operation output trace Hsuccess Htrace)
        (FaultAbortedArtifact source fault)
| TransitionValidatedFault :
    forall source operation output trace metadata
      Hsuccess Htrace Hcanonical Hmetadata fault,
      PublicationTransition
        (ValidatedArtifact source operation output trace metadata
          Hsuccess Htrace Hcanonical Hmetadata)
        (FaultAbortedArtifact source fault)
| TransitionPublishableFault :
    forall source aliases candidate candidate_identity Hfresh fault,
      PublicationTransition
        (PublishableArtifact source aliases candidate candidate_identity Hfresh)
        (FaultAbortedArtifact source fault)
| TransitionPreflightAbort :
    forall source operation Hfailure,
      PublicationTransition
        (PreflightArtifact source operation)
        (AbortedArtifact source operation Hfailure).

Theorem aborted_artifact_retains_source_authority :
  forall (source : AtomicRootAuthority) (operation : LogicalOperation),
    execute_logical_operation (atomic_root_entries source) operation = None ->
    atomic_root_entries source =
      atomic_root_entries source.
Proof. intros. reflexivity. Qed.

(** Byte-level PDAs.  Their explicit remaining-byte fields are the only depth
    state; the machine call stack is constant and fuel is derived from input
    byte counts, never from an artificial maximum path depth. *)
Record ByteCompareState : Type := {
  compare_left_remaining : Path;
  compare_right_remaining : Path;
  compare_answer : option comparison
}.

Definition byte_compare_step (state : ByteCompareState) : ByteCompareState :=
  match compare_answer state with
  | Some _ => state
  | None =>
      match compare_left_remaining state, compare_right_remaining state with
      | [], [] =>
          {| compare_left_remaining := [];
             compare_right_remaining := [];
             compare_answer := Some Eq |}
      | [], remaining_right =>
          {| compare_left_remaining := [];
             compare_right_remaining := remaining_right;
             compare_answer := Some Lt |}
      | remaining_left, [] =>
          {| compare_left_remaining := remaining_left;
             compare_right_remaining := [];
             compare_answer := Some Gt |}
      | left_byte :: left_tail, right_byte :: right_tail =>
          match Nat.compare left_byte right_byte with
          | Eq =>
              {| compare_left_remaining := left_tail;
                 compare_right_remaining := right_tail;
                 compare_answer := None |}
          | Lt =>
              {| compare_left_remaining := left_tail;
                 compare_right_remaining := right_tail;
                 compare_answer := Some Lt |}
          | Gt =>
              {| compare_left_remaining := left_tail;
                 compare_right_remaining := right_tail;
                 compare_answer := Some Gt |}
          end
      end
  end.

Fixpoint drive_byte_compare
  (fuel : nat) (state : ByteCompareState) : ByteCompareState :=
  match fuel with
  | O => state
  | S fuel' =>
      match compare_answer state with
      | Some _ => state
      | None => drive_byte_compare fuel' (byte_compare_step state)
      end
  end.

Definition byte_compare_pda (left right : Path) : option comparison :=
  compare_answer
    (drive_byte_compare (S (Nat.min (length left) (length right)))
      {| compare_left_remaining := left;
         compare_right_remaining := right;
         compare_answer := None |}).

Lemma byte_compare_step_consumes_or_finishes :
  forall state,
    compare_answer state = None ->
    compare_answer (byte_compare_step state) <> None \/
    length (compare_left_remaining (byte_compare_step state)) +
      length (compare_right_remaining (byte_compare_step state)) <
    length (compare_left_remaining state) +
      length (compare_right_remaining state).
Proof.
  intros [left right answer] Hopen; simpl in Hopen |- *.
  subst answer. destruct left as [|left_byte left_tail],
    right as [|right_byte right_tail]; simpl; try (left; discriminate).
  destruct (Nat.compare left_byte right_byte) eqn:Hcompare.
  - right. unfold byte_compare_step. simpl. rewrite Hcompare. simpl. lia.
  - left. unfold byte_compare_step. simpl. rewrite Hcompare. discriminate.
  - left. unfold byte_compare_step. simpl. rewrite Hcompare. discriminate.
Qed.

Theorem byte_compare_has_no_path_depth_cap :
  forall left right,
    exists answer,
      byte_compare_pda left right = Some answer.
Proof.
  induction left as [|left_byte left_tail IH]; intros right.
  - destruct right as [|right_byte right_tail].
    + exists Eq. reflexivity.
    + exists Lt. reflexivity.
  - destruct right as [|right_byte right_tail].
    + exists Gt. reflexivity.
    + destruct (Nat.compare left_byte right_byte) eqn:Hcompare.
      * specialize (IH right_tail).
        destruct IH as [answer Hanswer]. exists answer.
        unfold byte_compare_pda in Hanswer |- *; simpl in Hanswer |- *.
        assert (Hstep :
          byte_compare_step
            {| compare_left_remaining := left_byte :: left_tail;
               compare_right_remaining := right_byte :: right_tail;
               compare_answer := None |} =
            {| compare_left_remaining := left_tail;
               compare_right_remaining := right_tail;
               compare_answer := None |}).
        { unfold byte_compare_step. simpl. rewrite Hcompare. reflexivity. }
        rewrite Hstep. simpl. exact Hanswer.
      * exists Lt. unfold byte_compare_pda. simpl.
        assert (Hstep :
          byte_compare_step
            {| compare_left_remaining := left_byte :: left_tail;
               compare_right_remaining := right_byte :: right_tail;
               compare_answer := None |} =
            {| compare_left_remaining := left_tail;
               compare_right_remaining := right_tail;
               compare_answer := Some Lt |}).
        { unfold byte_compare_step. simpl. rewrite Hcompare. reflexivity. }
        rewrite Hstep. reflexivity.
      * exists Gt. unfold byte_compare_pda. simpl.
        assert (Hstep :
          byte_compare_step
            {| compare_left_remaining := left_byte :: left_tail;
               compare_right_remaining := right_byte :: right_tail;
               compare_answer := None |} =
            {| compare_left_remaining := left_tail;
               compare_right_remaining := right_tail;
               compare_answer := Some Gt |}).
        { unfold byte_compare_step. simpl. rewrite Hcompare. reflexivity. }
        rewrite Hstep. reflexivity.
Qed.

Record BytePrefixState : Type := {
  prefix_remaining : Path;
  candidate_remaining : Path;
  prefix_answer : option bool
}.

Definition byte_prefix_step (state : BytePrefixState) : BytePrefixState :=
  match prefix_answer state with
  | Some _ => state
  | None =>
      match prefix_remaining state, candidate_remaining state with
      | [], candidate =>
          {| prefix_remaining := [];
             candidate_remaining := candidate;
             prefix_answer := Some true |}
      | _ :: _, [] =>
          {| prefix_remaining := prefix_remaining state;
             candidate_remaining := [];
             prefix_answer := Some false |}
      | prefix_byte :: prefix_tail, candidate_byte :: candidate_tail =>
          if Nat.eqb prefix_byte candidate_byte
          then {| prefix_remaining := prefix_tail;
                  candidate_remaining := candidate_tail;
                  prefix_answer := None |}
          else {| prefix_remaining := prefix_tail;
                  candidate_remaining := candidate_tail;
                  prefix_answer := Some false |}
      end
  end.

Fixpoint drive_byte_prefix
  (fuel : nat) (state : BytePrefixState) : BytePrefixState :=
  match fuel with
  | O => state
  | S fuel' =>
      match prefix_answer state with
      | Some _ => state
      | None => drive_byte_prefix fuel' (byte_prefix_step state)
      end
  end.

Definition byte_prefix_pda (prefix candidate : Path) : option bool :=
  prefix_answer
    (drive_byte_prefix (S (Nat.min (length prefix) (length candidate)))
      {| prefix_remaining := prefix;
         candidate_remaining := candidate;
         prefix_answer := None |}).

Lemma byte_prefix_step_consumes_or_finishes :
  forall state,
    prefix_answer state = None ->
    prefix_answer (byte_prefix_step state) <> None \/
    length (prefix_remaining (byte_prefix_step state)) +
      length (candidate_remaining (byte_prefix_step state)) <
    length (prefix_remaining state) + length (candidate_remaining state).
Proof.
  intros [prefix candidate answer] Hopen; simpl in Hopen |- *.
  subst answer. destruct prefix as [|prefix_byte prefix_tail],
    candidate as [|candidate_byte candidate_tail]; simpl;
    try (left; discriminate).
  destruct (Nat.eqb prefix_byte candidate_byte) eqn:Hequal.
  - right. unfold byte_prefix_step. simpl. rewrite Hequal. simpl. lia.
  - left. unfold byte_prefix_step. simpl. rewrite Hequal. discriminate.
Qed.

Theorem byte_prefix_has_no_path_depth_cap :
  forall prefix candidate,
    exists answer,
      byte_prefix_pda prefix candidate = Some answer.
Proof.
  induction prefix as [|prefix_byte prefix_tail IH]; intros candidate.
  - exists true. reflexivity.
  - destruct candidate as [|candidate_byte candidate_tail].
    + exists false. reflexivity.
    + destruct (Nat.eqb prefix_byte candidate_byte) eqn:Hequal.
      * specialize (IH candidate_tail).
        destruct IH as [answer Hanswer]. exists answer.
        unfold byte_prefix_pda in Hanswer |- *; simpl in Hanswer |- *.
        assert (Hstep :
          byte_prefix_step
            {| prefix_remaining := prefix_byte :: prefix_tail;
               candidate_remaining := candidate_byte :: candidate_tail;
               prefix_answer := None |} =
            {| prefix_remaining := prefix_tail;
               candidate_remaining := candidate_tail;
               prefix_answer := None |}).
        { unfold byte_prefix_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. simpl. exact Hanswer.
      * exists false. unfold byte_prefix_pda. simpl.
        assert (Hstep :
          byte_prefix_step
            {| prefix_remaining := prefix_byte :: prefix_tail;
               candidate_remaining := candidate_byte :: candidate_tail;
               prefix_answer := None |} =
            {| prefix_remaining := prefix_tail;
               candidate_remaining := candidate_tail;
               prefix_answer := Some false |}).
        { unfold byte_prefix_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. reflexivity.
Qed.

Theorem byte_compare_pda_refines_recursive_specification :
  forall left right,
    byte_compare_pda left right = Some (path_compare left right).
Proof.
  induction left as [|left_byte left_tail IH]; intros right.
  - destruct right; reflexivity.
  - destruct right as [|right_byte right_tail].
    + reflexivity.
    + unfold byte_compare_pda in IH |- *; simpl in IH |- *.
      destruct (Nat.compare left_byte right_byte) eqn:Hcompare.
      * assert (Hstep :
          byte_compare_step
            {| compare_left_remaining := left_byte :: left_tail;
               compare_right_remaining := right_byte :: right_tail;
               compare_answer := None |} =
            {| compare_left_remaining := left_tail;
               compare_right_remaining := right_tail;
               compare_answer := None |}).
        { unfold byte_compare_step. simpl. rewrite Hcompare. reflexivity. }
        rewrite Hstep. simpl. exact (IH right_tail).
      * assert (Hstep :
          byte_compare_step
            {| compare_left_remaining := left_byte :: left_tail;
               compare_right_remaining := right_byte :: right_tail;
               compare_answer := None |} =
            {| compare_left_remaining := left_tail;
               compare_right_remaining := right_tail;
               compare_answer := Some Lt |}).
        { unfold byte_compare_step. simpl. rewrite Hcompare. reflexivity. }
        rewrite Hstep. reflexivity.
      * assert (Hstep :
          byte_compare_step
            {| compare_left_remaining := left_byte :: left_tail;
               compare_right_remaining := right_byte :: right_tail;
               compare_answer := None |} =
            {| compare_left_remaining := left_tail;
               compare_right_remaining := right_tail;
               compare_answer := Some Gt |}).
        { unfold byte_compare_step. simpl. rewrite Hcompare. reflexivity. }
        rewrite Hstep. reflexivity.
Qed.

Theorem byte_prefix_pda_refines_recursive_specification :
  forall prefix candidate,
    byte_prefix_pda prefix candidate = Some (path_prefixb prefix candidate).
Proof.
  induction prefix as [|prefix_byte prefix_tail IH]; intros candidate.
  - reflexivity.
  - destruct candidate as [|candidate_byte candidate_tail].
    + reflexivity.
    + unfold byte_prefix_pda in IH |- *; simpl in IH |- *.
      destruct (Nat.eqb prefix_byte candidate_byte) eqn:Hequal.
      * assert (Hstep :
          byte_prefix_step
            {| prefix_remaining := prefix_byte :: prefix_tail;
               candidate_remaining := candidate_byte :: candidate_tail;
               prefix_answer := None |} =
            {| prefix_remaining := prefix_tail;
               candidate_remaining := candidate_tail;
               prefix_answer := None |}).
        { unfold byte_prefix_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. simpl. exact (IH candidate_tail).
      * assert (Hstep :
          byte_prefix_step
            {| prefix_remaining := prefix_byte :: prefix_tail;
               candidate_remaining := candidate_byte :: candidate_tail;
               prefix_answer := None |} =
            {| prefix_remaining := prefix_tail;
               candidate_remaining := candidate_tail;
               prefix_answer := Some false |}).
        { unfold byte_prefix_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. reflexivity.
Qed.

Theorem byte_comparator_and_prefix_pd_as_are_constant_call_stack :
  forall left right prefix candidate,
    exists compare_answer prefix_answer,
      byte_compare_pda left right = Some compare_answer /\
      byte_prefix_pda prefix candidate = Some prefix_answer.
Proof.
  intros left right prefix candidate.
  destruct (byte_compare_has_no_path_depth_cap left right) as [comparison Hcomparison].
  destruct (byte_prefix_has_no_path_depth_cap prefix candidate) as [answer Hanswer].
  exists comparison, answer. split; assumption.
Qed.

Record ByteLcpState : Type := {
  lcp_reversed : Path;
  lcp_left_remaining : Path;
  lcp_right_remaining : Path;
  lcp_answer : option Path
}.

Definition byte_lcp_step (state : ByteLcpState) : ByteLcpState :=
  match lcp_answer state with
  | Some _ => state
  | None =>
      match lcp_left_remaining state, lcp_right_remaining state with
      | left_byte :: left_tail, right_byte :: right_tail =>
          if Nat.eqb left_byte right_byte
          then {| lcp_reversed := left_byte :: lcp_reversed state;
                  lcp_left_remaining := left_tail;
                  lcp_right_remaining := right_tail;
                  lcp_answer := None |}
          else {| lcp_reversed := lcp_reversed state;
                  lcp_left_remaining := left_tail;
                  lcp_right_remaining := right_tail;
                  lcp_answer := Some (rev (lcp_reversed state)) |}
      | _, _ =>
          {| lcp_reversed := lcp_reversed state;
             lcp_left_remaining := lcp_left_remaining state;
             lcp_right_remaining := lcp_right_remaining state;
             lcp_answer := Some (rev (lcp_reversed state)) |}
      end
  end.

Fixpoint drive_byte_lcp
  (fuel : nat) (state : ByteLcpState) : ByteLcpState :=
  match fuel with
  | O => state
  | S fuel' =>
      match lcp_answer state with
      | Some _ => state
      | None => drive_byte_lcp fuel' (byte_lcp_step state)
      end
  end.

Definition byte_lcp_pda (left right : Path) : option Path :=
  lcp_answer
    (drive_byte_lcp (S (Nat.min (length left) (length right)))
      {| lcp_reversed := [];
         lcp_left_remaining := left;
         lcp_right_remaining := right;
         lcp_answer := None |}).

Lemma byte_lcp_step_consumes_or_finishes :
  forall state,
    lcp_answer state = None ->
    lcp_answer (byte_lcp_step state) <> None \/
    length (lcp_left_remaining (byte_lcp_step state)) +
      length (lcp_right_remaining (byte_lcp_step state)) <
    length (lcp_left_remaining state) + length (lcp_right_remaining state).
Proof.
  intros [reversed left right answer] Hopen; simpl in Hopen |- *.
  subst answer. destruct left as [|left_byte left_tail],
    right as [|right_byte right_tail]; simpl;
    try (left; discriminate).
  destruct (Nat.eqb left_byte right_byte) eqn:Hequal.
  - right. unfold byte_lcp_step. simpl. rewrite Hequal. simpl. lia.
  - left. unfold byte_lcp_step. simpl. rewrite Hequal. discriminate.
Qed.

Lemma drive_byte_lcp_accumulator_total :
  forall left right reversed,
    exists answer,
      lcp_answer
        (drive_byte_lcp (S (Nat.min (length left) (length right)))
          {| lcp_reversed := reversed;
             lcp_left_remaining := left;
             lcp_right_remaining := right;
             lcp_answer := None |}) = Some answer.
Proof.
  induction left as [|left_byte left_tail IH]; intros right reversed.
  - destruct right; exists (rev reversed); reflexivity.
  - destruct right as [|right_byte right_tail].
    + exists (rev reversed). reflexivity.
    + destruct (Nat.eqb left_byte right_byte) eqn:Hequal.
      * destruct (IH right_tail (left_byte :: reversed))
          as [answer Hanswer].
        exists answer. simpl in Hanswer |- *.
        assert (Hstep :
          byte_lcp_step
            {| lcp_reversed := reversed;
               lcp_left_remaining := left_byte :: left_tail;
               lcp_right_remaining := right_byte :: right_tail;
               lcp_answer := None |} =
            {| lcp_reversed := left_byte :: reversed;
               lcp_left_remaining := left_tail;
               lcp_right_remaining := right_tail;
               lcp_answer := None |}).
        { unfold byte_lcp_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. simpl. exact Hanswer.
      * exists (rev reversed). simpl.
        assert (Hstep :
          byte_lcp_step
            {| lcp_reversed := reversed;
               lcp_left_remaining := left_byte :: left_tail;
               lcp_right_remaining := right_byte :: right_tail;
               lcp_answer := None |} =
            {| lcp_reversed := reversed;
               lcp_left_remaining := left_tail;
               lcp_right_remaining := right_tail;
               lcp_answer := Some (rev reversed) |}).
        { unfold byte_lcp_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. reflexivity.
Qed.

Theorem byte_lcp_has_no_path_depth_cap :
  forall left right,
    exists answer, byte_lcp_pda left right = Some answer.
Proof.
  intros left right. apply drive_byte_lcp_accumulator_total.
Qed.

Lemma drive_byte_lcp_refines_path_lcp_with_accumulator :
  forall left right reversed,
    lcp_answer
      (drive_byte_lcp (S (Nat.min (length left) (length right)))
        {| lcp_reversed := reversed;
           lcp_left_remaining := left;
           lcp_right_remaining := right;
           lcp_answer := None |}) =
    Some (rev reversed ++ path_lcp left right).
Proof.
  induction left as [|left_byte left_tail IH]; intros right reversed.
  - destruct right; simpl; rewrite app_nil_r; reflexivity.
  - destruct right as [|right_byte right_tail].
    + simpl. rewrite app_nil_r. reflexivity.
    + simpl. destruct (left_byte =? right_byte) eqn:Hequal; simpl.
      * specialize (IH right_tail (left_byte :: reversed)).
        assert (Hstep :
          byte_lcp_step
            {| lcp_reversed := reversed;
               lcp_left_remaining := left_byte :: left_tail;
               lcp_right_remaining := right_byte :: right_tail;
               lcp_answer := None |} =
            {| lcp_reversed := left_byte :: reversed;
               lcp_left_remaining := left_tail;
               lcp_right_remaining := right_tail;
               lcp_answer := None |}).
        { unfold byte_lcp_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. simpl. simpl in IH. rewrite IH. simpl.
        rewrite <- app_assoc. reflexivity.
      * assert (Hstep :
          byte_lcp_step
            {| lcp_reversed := reversed;
               lcp_left_remaining := left_byte :: left_tail;
               lcp_right_remaining := right_byte :: right_tail;
               lcp_answer := None |} =
            {| lcp_reversed := reversed;
               lcp_left_remaining := left_tail;
               lcp_right_remaining := right_tail;
               lcp_answer := Some (rev reversed) |}).
        { unfold byte_lcp_step. simpl. rewrite Hequal. reflexivity. }
        rewrite Hstep. simpl. rewrite app_nil_r. reflexivity.
Qed.

Theorem byte_lcp_pda_refines_recursive_specification :
  forall left right,
    byte_lcp_pda left right = Some (path_lcp left right).
Proof.
  intros left right.
  unfold byte_lcp_pda.
  rewrite drive_byte_lcp_refines_path_lcp_with_accumulator. reflexivity.
Qed.

Inductive DestinationZipperEvent : Type :=
| DestinationAscend
| DestinationDescend (byte : Byte)
| DestinationSetValue (count : Multiplicity).

Record MovingDestinationZipper : Type := {
  moving_zipper_identity : nat;
  moving_zipper_ascents : nat;
  moving_zipper_descents : Path;
  moving_zipper_events_reversed : list DestinationZipperEvent
}.

Definition destination_zipper_for_neighbor
  (zipper_identity : nat) (previous next : Path)
  : MovingDestinationZipper :=
  let common := path_lcp previous next in
  {| moving_zipper_identity := zipper_identity;
     moving_zipper_ascents := length previous - length common;
     moving_zipper_descents := path_drop (length common) next;
     moving_zipper_events_reversed := [] |}.

Definition destination_zipper_step
  (state : MovingDestinationZipper) : MovingDestinationZipper :=
  match moving_zipper_ascents state with
  | S ascents =>
      {| moving_zipper_identity := moving_zipper_identity state;
         moving_zipper_ascents := ascents;
         moving_zipper_descents := moving_zipper_descents state;
         moving_zipper_events_reversed :=
           DestinationAscend :: moving_zipper_events_reversed state |}
  | O =>
      match moving_zipper_descents state with
      | [] => state
      | byte :: tail =>
          {| moving_zipper_identity := moving_zipper_identity state;
             moving_zipper_ascents := O;
             moving_zipper_descents := tail;
             moving_zipper_events_reversed :=
               DestinationDescend byte ::
                 moving_zipper_events_reversed state |}
      end
  end.

Fixpoint drive_destination_zipper
  (fuel : nat) (state : MovingDestinationZipper)
  : MovingDestinationZipper :=
  match fuel with
  | O => state
  | S fuel' =>
      match moving_zipper_ascents state, moving_zipper_descents state with
      | O, [] => state
      | _, _ => drive_destination_zipper fuel' (destination_zipper_step state)
      end
  end.

Theorem destination_zipper_step_preserves_single_identity :
  forall state,
    moving_zipper_identity (destination_zipper_step state) =
      moving_zipper_identity state.
Proof.
  intros [identity ascents descents events].
  destruct ascents, descents; reflexivity.
Qed.

Theorem destination_zipper_work_is_exact_lcp_distance :
  forall zipper_identity previous next,
    moving_zipper_ascents
      (destination_zipper_for_neighbor zipper_identity previous next) +
    length (moving_zipper_descents
      (destination_zipper_for_neighbor zipper_identity previous next)) =
    (length previous - length (path_lcp previous next)) +
    length (path_drop (length (path_lcp previous next)) next).
Proof. reflexivity. Qed.

Lemma destination_zipper_step_decreases_open_work :
  forall state,
    moving_zipper_ascents state <> O \/
    moving_zipper_descents state <> [] ->
    moving_zipper_ascents (destination_zipper_step state) +
      length (moving_zipper_descents (destination_zipper_step state)) <
    moving_zipper_ascents state + length (moving_zipper_descents state).
Proof.
  intros [identity ascents descents events] Hopen; simpl in Hopen |- *.
  destruct ascents as [|ascents], descents as [|byte tail].
  - destruct Hopen as [Hascents | Hdescents].
    + exfalso. apply Hascents. reflexivity.
    + exfalso. apply Hdescents. reflexivity.
  - change (length tail < S (length tail)). lia.
  - unfold destination_zipper_step. simpl. lia.
  - unfold destination_zipper_step. simpl. lia.
Qed.

Theorem destination_zipper_closes_without_depth_cap :
  forall state,
    let fuel := S (moving_zipper_ascents state +
                   length (moving_zipper_descents state)) in
    moving_zipper_ascents (drive_destination_zipper fuel state) = O /\
    moving_zipper_descents (drive_destination_zipper fuel state) = [].
Proof.
  intros state.
  remember (moving_zipper_ascents state +
            length (moving_zipper_descents state)) as work eqn:Hwork.
  revert state Hwork.
  induction work as [|work IH]; intros state Hwork; simpl.
  - destruct state as [identity ascents descents events]; simpl in Hwork |- *.
    destruct ascents, descents; simpl in Hwork |- *; try lia; split; reflexivity.
  - destruct (moving_zipper_ascents state) eqn:Hascents,
    (moving_zipper_descents state) eqn:Hdescents.
    + rewrite Hascents, Hdescents. split; reflexivity.
    + apply IH.
      unfold destination_zipper_step. rewrite Hascents, Hdescents. simpl in *.
      lia.
    + apply IH.
      unfold destination_zipper_step. rewrite Hascents, Hdescents. simpl in *.
      lia.
    + apply IH.
      unfold destination_zipper_step. rewrite Hascents, Hdescents. simpl in *.
      lia.
Qed.

(** Topology-preserving value updates and structural changes intentionally use
    different protocols.  A native insertion is private shallow CoW after
    complete checked preflight; destructive pruning never uses that protocol. *)
Inductive RootOwnership : Type :=
| UniqueRoot
| SharedRoot.

Inductive TopologyChange : Type :=
| ExistingEndpointValueOnly
| StructuralInsertionOnly
| StructuralPruneOrPrefixClear.

Definition topology_strategy
  (ownership : RootOwnership) (change : TopologyChange)
  : CandidateBuildStrategy :=
  match change with
  | ExistingEndpointValueOnly => GuardedValueOnlySetVal
  | StructuralInsertionOnly => PrivateShallowCowInsertion
  | StructuralPruneOrPrefixClear => IndependentCanonicalStreamRebuild
  end.

Definition candidate_anchor_identity
  (ownership : RootOwnership) (change : TopologyChange)
  (live_identity detached_identity : nat) : nat :=
  match ownership, change with
  | UniqueRoot, ExistingEndpointValueOnly => live_identity
  | _, _ => detached_identity
  end.

Theorem unique_value_only_update_retains_anchor :
  forall live_identity detached_identity,
    candidate_anchor_identity UniqueRoot ExistingEndpointValueOnly
      live_identity detached_identity = live_identity.
Proof. reflexivity. Qed.

Theorem shared_value_update_may_detach_without_mutating_aliases :
  forall live_identity detached_identity,
    candidate_anchor_identity SharedRoot ExistingEndpointValueOnly
      live_identity detached_identity = detached_identity.
Proof. reflexivity. Qed.

Definition native_insertion_preflight
  (source additions : list Entry) : option (list Entry) :=
  checked_exact_upsert source additions.

Inductive NativeCowInsertionRefines :
  list Entry -> list Entry -> list Entry -> Prop :=
| NativeCowInsertionAfterCompletePreflight :
    forall source additions candidate,
      native_insertion_preflight source additions = Some candidate ->
      NativeCowInsertionRefines source additions candidate.

Theorem native_insertion_has_no_semantic_failure_after_preflight :
  forall source additions candidate,
    native_insertion_preflight source additions = Some candidate ->
    exists result,
      NativeCowInsertionRefines source additions result /\
      result = candidate.
Proof.
  intros source additions candidate Hpreflight.
  exists candidate. split.
  - constructor. exact Hpreflight.
  - reflexivity.
Qed.

Theorem native_insertion_refines_checked_upsert :
  forall source additions candidate,
    NativeCowInsertionRefines source additions candidate ->
    checked_exact_upsert source additions = Some candidate.
Proof.
  intros source additions candidate Hnative.
  inversion Hnative; subst. exact H.
Qed.

Record NodeValueFootprint : Type := {
  footprint_node_identities : list nat;
  footprint_value_identities : list nat
}.

Definition footprints_disjoint
  (left right : NodeValueFootprint) : Prop :=
  (forall identity,
      In identity (footprint_node_identities left) ->
      ~ In identity (footprint_node_identities right)) /\
  (forall identity,
      In identity (footprint_value_identities left) ->
      ~ In identity (footprint_value_identities right)).

Record CandidateFootprint : Type := {
  candidate_private_footprint : NodeValueFootprint;
  candidate_borrowed_footprint : NodeValueFootprint;
  candidate_private_borrowed_disjoint :
    footprints_disjoint candidate_private_footprint candidate_borrowed_footprint;
  candidate_private_nodes_unique :
    NoDup (footprint_node_identities candidate_private_footprint);
  candidate_private_values_unique :
    NoDup (footprint_value_identities candidate_private_footprint)
}.

Definition aliases_as_footprint
  (aliases : ExternalAliasFootprint) : NodeValueFootprint :=
  {| footprint_node_identities := external_alias_node_identities aliases;
     footprint_value_identities := external_alias_value_identities aliases |}.

Definition candidate_private_is_fresh_for_external_aliases
  (candidate : CandidateFootprint) (aliases : ExternalAliasFootprint) : Prop :=
  footprints_disjoint (candidate_private_footprint candidate)
    (aliases_as_footprint aliases).

Record TransactionWorldV2 : Type := {
  world_live_authority : AtomicRootAuthority;
  world_external_aliases : ExternalAliasFootprint;
  world_staged_candidate : option (nat * CandidateFootprint);
  world_disposed_root_handles : list nat;
  world_disposed_nodes : list nat;
  world_disposed_values : list nat
}.

Definition abort_world_before_publication
  (world : TransactionWorldV2) : TransactionWorldV2 :=
  {| world_live_authority := world_live_authority world;
     world_external_aliases := world_external_aliases world;
     world_staged_candidate := None;
     world_disposed_root_handles := world_disposed_root_handles world;
     world_disposed_nodes :=
       match world_staged_candidate world with
       | None => world_disposed_nodes world
       | Some (_, footprint) =>
           footprint_node_identities (candidate_private_footprint footprint)
           ++ world_disposed_nodes world
       end;
     world_disposed_values :=
       match world_staged_candidate world with
       | None => world_disposed_values world
       | Some (_, footprint) =>
           footprint_value_identities (candidate_private_footprint footprint)
           ++ world_disposed_values world
       end |}.

Theorem abort_preserves_live_authority_and_external_aliases :
  forall world,
    world_live_authority (abort_world_before_publication world) =
      world_live_authority world /\
    world_external_aliases (abort_world_before_publication world) =
      world_external_aliases world /\
    world_staged_candidate (abort_world_before_publication world) = None.
Proof. intros world. repeat split; reflexivity. Qed.

Definition publish_world
  (world : TransactionWorldV2) (new_root : AtomicRootAuthority)
  : TransactionWorldV2 :=
  {| world_live_authority := new_root;
     world_external_aliases := world_external_aliases world;
     world_staged_candidate := None;
     world_disposed_root_handles :=
       atomic_root_identity (world_live_authority world)
       :: world_disposed_root_handles world;
     world_disposed_nodes := world_disposed_nodes world;
     world_disposed_values := world_disposed_values world |}.

Theorem publish_is_atomic_and_preserves_external_aliases :
  forall world new_root,
    world_live_authority (publish_world world new_root) = new_root /\
    world_external_aliases (publish_world world new_root) =
      world_external_aliases world /\
    world_staged_candidate (publish_world world new_root) = None.
Proof. intros world new_root. repeat split; reflexivity. Qed.

Theorem publish_disposes_old_live_handle_exactly_once_at_head :
  forall world new_root,
    world_disposed_root_handles (publish_world world new_root) =
      atomic_root_identity (world_live_authority world)
      :: world_disposed_root_handles world.
Proof. reflexivity. Qed.

Theorem abort_unwinds_only_private_candidate_footprint :
  forall world candidate_identity footprint,
    world_staged_candidate world = Some (candidate_identity, footprint) ->
    world_disposed_nodes (abort_world_before_publication world) =
      footprint_node_identities (candidate_private_footprint footprint)
      ++ world_disposed_nodes world /\
    world_disposed_values (abort_world_before_publication world) =
      footprint_value_identities (candidate_private_footprint footprint)
      ++ world_disposed_values world.
Proof.
  intros world candidate_identity footprint Hstaged.
  unfold abort_world_before_publication. rewrite Hstaged. split; reflexivity.
Qed.

Theorem abort_cannot_drop_an_external_alias_node :
  forall world candidate_identity footprint identity,
    world_staged_candidate world = Some (candidate_identity, footprint) ->
    candidate_private_is_fresh_for_external_aliases footprint
      (world_external_aliases world) ->
    In identity
      (external_alias_node_identities (world_external_aliases world)) ->
    ~ In identity
      (footprint_node_identities (candidate_private_footprint footprint)).
Proof.
  intros world candidate_identity footprint identity _ Hfresh Halias.
  destruct Hfresh as [Hnodes _]. intro Hprivate.
  eapply (Hnodes identity Hprivate). exact Halias.
Qed.

Theorem abort_cannot_drop_an_external_alias_value :
  forall world candidate_identity footprint identity,
    world_staged_candidate world = Some (candidate_identity, footprint) ->
    candidate_private_is_fresh_for_external_aliases footprint
      (world_external_aliases world) ->
    In identity
      (external_alias_value_identities (world_external_aliases world)) ->
    ~ In identity
      (footprint_value_identities (candidate_private_footprint footprint)).
Proof.
  intros world candidate_identity footprint identity _ Hfresh Halias.
  destruct Hfresh as [_ Hvalues]. intro Hprivate.
  eapply (Hvalues identity Hprivate). exact Halias.
Qed.

Definition disposed_exactly_once (identity : nat) (disposed : list nat) : Prop :=
  count_occ Nat.eq_dec disposed identity = 1.

Lemma count_occ_absent_is_zero :
  forall identity disposed,
    ~ In identity disposed ->
    count_occ Nat.eq_dec disposed identity = 0.
Proof.
  intros identity disposed Habsent.
  induction disposed as [|head tail IH].
  - reflexivity.
  - simpl.
  assert (Hneq : head <> identity).
  { intro Hequal. apply Habsent. left. exact Hequal. }
  destruct (Nat.eq_dec head identity) as [Hequal | Hdifferent].
  + exfalso. apply Hneq. exact Hequal.
  + apply IH.
    intro Hin.
    apply Habsent.
    right.
    exact Hin.
Qed.

Theorem publish_disposes_previously_live_handle_exactly_once :
  forall world new_root,
    ~ In (atomic_root_identity (world_live_authority world))
         (world_disposed_root_handles world) ->
    disposed_exactly_once
      (atomic_root_identity (world_live_authority world))
      (world_disposed_root_handles (publish_world world new_root)).
Proof.
  intros world new_root Hfresh.
  unfold disposed_exactly_once, publish_world; simpl.
  destruct (Nat.eq_dec
    (atomic_root_identity (world_live_authority world))
    (atomic_root_identity (world_live_authority world)));
    [|contradiction].
  rewrite count_occ_absent_is_zero by exact Hfresh. reflexivity.
Qed.

Definition fault_before_atomic_publication
  (fault : PublicationFaultPoint) : bool :=
  match fault with
  | FaultAfterPublicationBeforeOldDrop => false
  | _ => true
  end.

Definition recover_from_publication_fault
  (world : TransactionWorldV2) (new_root : AtomicRootAuthority)
  (fault : PublicationFaultPoint) : TransactionWorldV2 :=
  if fault_before_atomic_publication fault
  then abort_world_before_publication world
  else publish_world world new_root.

Theorem every_fault_observes_either_old_or_fully_published_authority :
  forall world new_root fault,
    world_live_authority
      (recover_from_publication_fault world new_root fault) =
        world_live_authority world \/
    world_live_authority
      (recover_from_publication_fault world new_root fault) = new_root.
Proof.
  intros world new_root fault.
  unfold recover_from_publication_fault.
  destruct (fault_before_atomic_publication fault); simpl; auto.
Qed.

Theorem fault_recovery_never_changes_external_alias_footprint :
  forall world new_root fault,
    world_external_aliases
      (recover_from_publication_fault world new_root fault) =
    world_external_aliases world.
Proof.
  intros world new_root fault.
  unfold recover_from_publication_fault.
  destruct (fault_before_atomic_publication fault); reflexivity.
Qed.

Theorem private_candidate_unwind_cannot_drop_borrowed_alias_node :
  forall world candidate_identity footprint identity,
    world_staged_candidate world = Some (candidate_identity, footprint) ->
    In identity
      (footprint_node_identities (candidate_borrowed_footprint footprint)) ->
    ~ In identity
      (footprint_node_identities (candidate_private_footprint footprint)).
Proof.
  intros world candidate_identity footprint identity _ Hborrowed.
  destruct (candidate_private_borrowed_disjoint footprint) as [Hnodes _].
  intro Hprivate. eapply (Hnodes identity Hprivate). exact Hborrowed.
Qed.

Theorem candidate_unwind_has_no_internal_double_node_drop :
  forall footprint,
    NoDup
      (footprint_node_identities (candidate_private_footprint footprint)).
Proof. intros footprint. apply candidate_private_nodes_unique. Qed.

Theorem candidate_unwind_has_no_internal_double_value_drop :
  forall footprint,
    NoDup
      (footprint_value_identities (candidate_private_footprint footprint)).
Proof. intros footprint. apply candidate_private_values_unique. Qed.

(** A canonical trie is built structurally.  ACT observation traverses nodes
    and labelled edges directly; it is not a [list Entry] byte flattening and
    does not sort or project through [Par]. *)
Inductive CanonicalByteTrie : Type :=
| CanonicalByteNode
    (endpoint : option Multiplicity)
    (children : list (Byte * CanonicalByteTrie)).

Fixpoint canonical_trie_size (trie : CanonicalByteTrie) : nat :=
  match trie with
  | CanonicalByteNode _ children =>
      S (fold_right
           (fun child_suffix total =>
              let '(_, child) := child_suffix in
              canonical_trie_size child + total)
           0 children)
  end.

Fixpoint insert_canonical_child_fuel
  (fuel : nat) (byte : Byte) (tail : Path) (count : Multiplicity)
  (children : list (Byte * CanonicalByteTrie))
  : list (Byte * CanonicalByteTrie) :=
  match fuel with
  | O => children
  | S fuel' =>
      match children with
      | [] =>
          [(byte, insert_canonical_path_fuel fuel' tail count
                    (CanonicalByteNode None []))]
      | (child_byte, child) :: siblings =>
          match Nat.compare byte child_byte with
          | Lt =>
              (byte, insert_canonical_path_fuel fuel' tail count
                       (CanonicalByteNode None []))
              :: (child_byte, child) :: siblings
          | Eq =>
              (child_byte,
                insert_canonical_path_fuel fuel' tail count child) :: siblings
          | Gt =>
              (child_byte, child) ::
                insert_canonical_child_fuel fuel' byte tail count siblings
          end
      end
  end
with insert_canonical_path_fuel
  (fuel : nat) (path : Path) (count : Multiplicity)
  (trie : CanonicalByteTrie) : CanonicalByteTrie :=
  match fuel with
  | O => trie
  | S fuel' =>
      match trie with
      | CanonicalByteNode endpoint children =>
          match path with
          | [] => CanonicalByteNode (Some count) children
          | byte :: tail =>
              CanonicalByteNode endpoint
                (insert_canonical_child_fuel fuel' byte tail count children)
          end
      end
  end.

Definition insert_canonical_path
  (path : Path) (count : Multiplicity) (trie : CanonicalByteTrie)
  : CanonicalByteTrie :=
  insert_canonical_path_fuel
    (S (length path + canonical_trie_size trie)) path count trie.

Definition build_canonical_byte_trie (entries : list Entry) : CanonicalByteTrie :=
  fold_left
    (fun trie entry =>
       insert_canonical_path (entry_path entry)
         (entry_multiplicity entry) trie)
    entries (CanonicalByteNode None []).

Inductive StructuralActToken : Type :=
| ActOpenNode
| ActEndpointAbsent
| ActEndpointValue (count : Multiplicity)
| ActOpenEdge (byte : Byte)
| ActCloseEdge
| ActCloseNode.

Fixpoint canonical_trie_structural_act_image (trie : CanonicalByteTrie)
  : list StructuralActToken :=
  match trie with
  | CanonicalByteNode endpoint children =>
      ActOpenNode
      :: (match endpoint with
          | None => [ActEndpointAbsent]
          | Some count => [ActEndpointValue count]
          end)
      ++ fold_right
           (fun child_suffix image =>
              let '(byte, child) := child_suffix in
              ActOpenEdge byte :: canonical_trie_structural_act_image child
              ++ ActCloseEdge :: image)
           [] children
      ++ [ActCloseNode]
  end.

Definition canonical_structural_act_image (entries : list Entry)
  : list StructuralActToken :=
  canonical_trie_structural_act_image (build_canonical_byte_trie entries).

Definition CanonicalCompactObservationV2 : Type :=
  (list StructuralActToken * option Metadata)%type.

Definition canonical_compact_observation_v2 (entries : list Entry)
  : CanonicalCompactObservationV2 :=
  (canonical_structural_act_image entries, checked_metadata entries).

Theorem equal_logical_entries_build_equal_canonical_tries :
  forall left right,
    left = right ->
    build_canonical_byte_trie left = build_canonical_byte_trie right.
Proof. intros left right Hequal. subst. reflexivity. Qed.

Theorem equal_logical_entries_have_equal_structural_act_images :
  forall left right,
    left = right ->
    canonical_structural_act_image left = canonical_structural_act_image right.
Proof. intros left right Hequal. subst. reflexivity. Qed.

Theorem structural_act_build_is_deterministic :
  forall entries first second,
    first = build_canonical_byte_trie entries ->
    second = build_canonical_byte_trie entries ->
    canonical_trie_structural_act_image first =
      canonical_trie_structural_act_image second.
Proof.
  intros entries first second Hfirst Hsecond.
  subst. reflexivity.
Qed.

Theorem compact_observation_is_deterministic_across_equal_logical_entries :
  forall left right,
    canonical_entries left ->
    canonical_entries right ->
    left = right ->
    canonical_compact_observation_v2 left =
      canonical_compact_observation_v2 right.
Proof. intros left right _ _ Hequal. subst. reflexivity. Qed.

(** Interpreted invariant catalog.  A valid scenario contains only operation
    inputs plus the facts produced by successful execution/validation.  Trace
    accounting, cursor order, image determinism, and publication properties
    below are derived theorems rather than freely asserted fields. *)
Record WellFormedPublicationScenario : Type := {
  scenario_source_root : AtomicRootAuthority;
  scenario_operation : LogicalOperation;
  scenario_operation_valid :
    valid_logical_operation
      (atomic_root_entries scenario_source_root) scenario_operation;
  scenario_output : list Entry;
  scenario_execution_success :
    execute_logical_operation
      (atomic_root_entries scenario_source_root) scenario_operation =
      Some scenario_output;
  scenario_output_metadata : Metadata;
  scenario_output_metadata_checked :
    checked_metadata scenario_output = Some scenario_output_metadata;
  scenario_aliases : ExternalAliasFootprint
}.

Definition scenario_trace (scenario : WellFormedPublicationScenario)
  : list StreamEvent :=
  logical_operation_trace
    (atomic_root_entries (scenario_source_root scenario))
    (scenario_operation scenario)
    (scenario_output scenario).

Inductive PublicationInvariantV2 : Type :=
| InvExactCursorRefinementV2
| InvPrefixCursorRefinementV2
| InvDerivedInputProjectionsV2
| InvDerivedOutputProjectionV2
| InvLinearVisitsV2
| InvStrictAbsentFaultV2
| InvStrictUnderflowFaultV2
| InvIdempotentAbsentNoOpV2
| InvCheckedAddOverflowV2
| InvBagDeltaAtomicV2
| InvPrefixClassificationTotalV2
| InvPrefixRegionsMonotoneV2
| InvEmptyPrefixV2
| InvMaxBytePrefixV2
| InvSpliceCrossOrderV2
| InvRelativeClearModeV2
| InvRelativePreserveModeV2
| InvClosedBeforePreparedV2
| InvIndexedPublicationPhasesV2
| InvCanonicalTrieDeterminismV2
| InvStructuralActDeterminismV2
| InvFreshOutsideAliasFootprintV2
| InvAtomicMetadataPublicationV2
| InvPrivateUnwindV2
| InvExternalAliasesStableV2
| InvOldRootExactOnceV2
| InvUniqueValueAnchorV2
| InvSharedValueDetachV2
| InvPruneFreshStreamV2
| InvInsertionPrivateCowV2
| InvOneMovingDestinationZipperV2
| InvLcpMovementV2
| InvByteComparatorStackSafeV2
| InvBytePrefixStackSafeV2
| InvByteLcpStackSafeV2
| InvNoDepthCapV2
| InvCheckedUpsertExtensionalV2
| InvIdempotentDeleteExtensionalV2
| InvClearPrefixExtensionalV2
| InvPreservePrefixExtensionalV2
| InvValidOperationCanonicalV2
| InvNativePrefixSubtractV2
| InvAllFaultPhasesAtomicV2
| InvNoPrivateDoubleDropV2
| InvCompactObservationDeterminismV2.

Definition publication_invariant_catalog_v2 : list PublicationInvariantV2 :=
  [InvExactCursorRefinementV2;
   InvPrefixCursorRefinementV2;
   InvDerivedInputProjectionsV2;
   InvDerivedOutputProjectionV2;
   InvLinearVisitsV2;
   InvStrictAbsentFaultV2;
   InvStrictUnderflowFaultV2;
   InvIdempotentAbsentNoOpV2;
   InvCheckedAddOverflowV2;
   InvBagDeltaAtomicV2;
   InvPrefixClassificationTotalV2;
   InvPrefixRegionsMonotoneV2;
   InvEmptyPrefixV2;
   InvMaxBytePrefixV2;
   InvSpliceCrossOrderV2;
   InvRelativeClearModeV2;
   InvRelativePreserveModeV2;
   InvClosedBeforePreparedV2;
   InvIndexedPublicationPhasesV2;
   InvCanonicalTrieDeterminismV2;
   InvStructuralActDeterminismV2;
   InvFreshOutsideAliasFootprintV2;
   InvAtomicMetadataPublicationV2;
   InvPrivateUnwindV2;
   InvExternalAliasesStableV2;
   InvOldRootExactOnceV2;
   InvUniqueValueAnchorV2;
   InvSharedValueDetachV2;
   InvPruneFreshStreamV2;
   InvInsertionPrivateCowV2;
   InvOneMovingDestinationZipperV2;
   InvLcpMovementV2;
   InvByteComparatorStackSafeV2;
   InvBytePrefixStackSafeV2;
   InvByteLcpStackSafeV2;
   InvNoDepthCapV2;
   InvCheckedUpsertExtensionalV2;
   InvIdempotentDeleteExtensionalV2;
   InvClearPrefixExtensionalV2;
   InvPreservePrefixExtensionalV2;
   InvValidOperationCanonicalV2;
   InvNativePrefixSubtractV2;
   InvAllFaultPhasesAtomicV2;
   InvNoPrivateDoubleDropV2;
   InvCompactObservationDeterminismV2].

Definition interpret_publication_invariant_v2
  (invariant : PublicationInvariantV2)
  (scenario : WellFormedPublicationScenario) : Prop :=
  let source := atomic_root_entries (scenario_source_root scenario) in
  let operation := scenario_operation scenario in
  let output := scenario_output scenario in
  let trace := scenario_trace scenario in
  match invariant with
  | InvExactCursorRefinementV2 =>
      match operation with
      | OpStrictSubtract removals =>
          exact_stream_transform source removals = Some output
      | _ => True
      end
  | InvPrefixCursorRefinementV2 =>
      match operation with
      | OpRelativePrefixReplace mode prefix relative =>
          apply_relative_prefix_replace mode source prefix relative = Some output
      | _ => True
      end
  | InvDerivedInputProjectionsV2 =>
      source_projection trace = source /\
      removal_projection trace = logical_operation_removals operation /\
      deletion_projection trace = logical_operation_deletions operation /\
      replacement_projection trace = logical_operation_additions operation /\
      source_event_count trace = length source /\
      removal_event_count trace =
        length (logical_operation_removals operation) /\
      deletion_event_count trace =
        length (logical_operation_deletions operation) /\
      replacement_event_count trace =
        length (logical_operation_additions operation)
  | InvDerivedOutputProjectionV2 => emitted_trace trace = output
  | InvLinearVisitsV2 =>
      length trace <=
        5 * (S (length source +
                length (logical_operation_removals operation) +
                length (logical_operation_deletions operation) +
                length (logical_operation_additions operation)))
  | InvStrictAbsentFaultV2 =>
      forall path amount,
        lookup_count path source = 0%N ->
        (0 < amount)%N ->
        strict_exact_remove source [(path, amount)] = None
  | InvStrictUnderflowFaultV2 =>
      forall path amount,
        (lookup_count path source < amount)%N ->
        strict_exact_remove source [(path, amount)] = None
  | InvIdempotentAbsentNoOpV2 =>
      forall deletions,
        (forall entry,
            In entry source ->
            path_memberb (entry_path entry) deletions = false) ->
        idempotent_exact_delete source deletions = source
  | InvCheckedAddOverflowV2 =>
      forall left right,
        (multiplicity_max < left + right)%N ->
        checked_add_multiplicity left right = None
  | InvBagDeltaAtomicV2 =>
      forall delta,
        apply_bag_delta source delta = None ->
        atomic_bag_delta source delta = source
  | InvPrefixClassificationTotalV2 =>
      forall prefix path,
        classify_prefix prefix path = PrefixBelow \/
        classify_prefix prefix path = PrefixInside \/
        classify_prefix prefix path = PrefixAbove
  | InvPrefixRegionsMonotoneV2 =>
      forall prefix below suffix above,
        classify_prefix prefix below = PrefixBelow ->
        classify_prefix prefix above = PrefixAbove ->
        path_lex_lt below (prefix ++ suffix) /\
        path_lex_lt (prefix ++ suffix) above
  | InvEmptyPrefixV2 =>
      forall entries, entries_in_region PrefixInside [] entries = entries
  | InvMaxBytePrefixV2 =>
      forall suffix, classify_prefix [255] ([255] ++ suffix) = PrefixInside
  | InvSpliceCrossOrderV2 =>
      forall prefix relative,
        paths_ordered_before
          (entries_in_region PrefixBelow prefix source)
          (absolute_replacements prefix relative) /\
        paths_ordered_before
          (absolute_replacements prefix relative)
          (entries_in_region PrefixAbove prefix source)
  | InvRelativeClearModeV2 =>
      forall prefix relative,
        apply_relative_prefix_replace ClearPrefixRegion source prefix relative =
          Some (clear_prefix_replace source prefix relative)
  | InvRelativePreserveModeV2 =>
      forall prefix relative output',
        apply_relative_prefix_replace PreservePrefixRegion source prefix relative =
          Some output' ->
        exists inside,
          checked_exact_upsert (entries_in_region PrefixInside prefix source)
            (absolute_replacements prefix relative) = Some inside
  | InvClosedBeforePreparedV2 =>
      exists candidate : PreparedCandidateV2,
        prepared_candidate_entries candidate = output /\
        prepared_candidate_metadata candidate =
          scenario_output_metadata scenario /\
        prepared_candidate_image candidate =
          canonical_compressed_image output
  | InvIndexedPublicationPhasesV2 =>
      forall (closed : PublicationArtifact ClosedPhase)
             (published : PublicationArtifact PublishedPhase),
        ~ PublicationTransition closed published
  | InvCanonicalTrieDeterminismV2 =>
      forall entries first second,
        first = build_canonical_byte_trie entries ->
        second = build_canonical_byte_trie entries ->
        first = second
  | InvStructuralActDeterminismV2 =>
      forall left right,
        left = right ->
        canonical_structural_act_image left = canonical_structural_act_image right
  | InvFreshOutsideAliasFootprintV2 =>
      fresh_candidate_identity (scenario_source_root scenario)
        (scenario_aliases scenario)
        (allocate_fresh_candidate_identity
          (scenario_source_root scenario) (scenario_aliases scenario))
  | InvAtomicMetadataPublicationV2 =>
      checked_metadata output = Some (scenario_output_metadata scenario)
  | InvPrivateUnwindV2 =>
      forall world,
        world_live_authority (abort_world_before_publication world) =
          world_live_authority world
  | InvExternalAliasesStableV2 =>
      forall world new_root,
        world_external_aliases (publish_world world new_root) =
          world_external_aliases world
  | InvOldRootExactOnceV2 =>
      forall world new_root,
        ~ In (atomic_root_identity (world_live_authority world))
             (world_disposed_root_handles world) ->
        disposed_exactly_once
          (atomic_root_identity (world_live_authority world))
          (world_disposed_root_handles (publish_world world new_root))
  | InvUniqueValueAnchorV2 =>
      forall live detached,
        candidate_anchor_identity UniqueRoot ExistingEndpointValueOnly
          live detached = live
  | InvSharedValueDetachV2 =>
      forall live detached,
        candidate_anchor_identity SharedRoot ExistingEndpointValueOnly
          live detached = detached
  | InvPruneFreshStreamV2 =>
      forall delta,
        bag_delta_requires_prune_b source delta = true ->
        choose_candidate_build_strategy source delta =
          IndependentCanonicalStreamRebuild
  | InvInsertionPrivateCowV2 =>
      forall delta,
        bag_delta_requires_prune_b source delta = false ->
        additions_are_value_only_b source (bag_delta_additions delta) = false ->
        choose_candidate_build_strategy source delta = PrivateShallowCowInsertion
  | InvOneMovingDestinationZipperV2 =>
      destination_event_count trace = length output /\
      forall identity previous next,
        moving_zipper_identity
          (destination_zipper_for_neighbor identity previous next) = identity
  | InvLcpMovementV2 =>
      forall identity previous next,
        moving_zipper_ascents
          (destination_zipper_for_neighbor identity previous next) =
          length previous - length (path_lcp previous next)
  | InvByteComparatorStackSafeV2 =>
      forall left right, exists answer,
        byte_compare_pda left right = Some answer
  | InvBytePrefixStackSafeV2 =>
      forall prefix candidate, exists answer,
        byte_prefix_pda prefix candidate = Some answer
  | InvByteLcpStackSafeV2 =>
      forall left right, exists answer,
        byte_lcp_pda left right = Some answer
  | InvNoDepthCapV2 =>
      forall left right prefix candidate,
        exists compare_answer prefix_answer,
          byte_compare_pda left right = Some compare_answer /\
          byte_prefix_pda prefix candidate = Some prefix_answer
  | InvCheckedUpsertExtensionalV2 =>
      forall source' additions output' queried,
        canonical_entries source' ->
        canonical_entries additions ->
        checked_exact_upsert source' additions = Some output' ->
        lookup_count queried output' =
          (lookup_count queried source' + lookup_count queried additions)%N
  | InvIdempotentDeleteExtensionalV2 =>
      forall source' deletions queried,
        lookup_count queried (idempotent_exact_delete source' deletions) =
          if path_memberb queried deletions
          then 0%N else lookup_count queried source'
  | InvClearPrefixExtensionalV2 =>
      forall source' prefix relative queried,
        lookup_count queried (clear_prefix_replace source' prefix relative) =
          match classify_prefix prefix queried with
          | PrefixInside =>
              lookup_count queried (absolute_replacements prefix relative)
          | PrefixBelow | PrefixAbove => lookup_count queried source'
          end
  | InvPreservePrefixExtensionalV2 =>
      forall source' prefix relative output' queried,
        canonical_entries source' ->
        path_valid prefix = true ->
        canonical_entries relative ->
        preserve_prefix_replace source' prefix relative = Some output' ->
        lookup_count queried output' =
          match classify_prefix prefix queried with
          | PrefixInside =>
              (lookup_count queried source' +
               lookup_count queried
                 (absolute_replacements prefix relative))%N
          | PrefixBelow | PrefixAbove => lookup_count queried source'
          end
  | InvValidOperationCanonicalV2 =>
      canonical_entries output
  | InvNativePrefixSubtractV2 =>
      forall prefix relative_removals removal,
        In removal (absolute_replacements prefix relative_removals) ->
        classify_prefix prefix (entry_path removal) = PrefixInside
  | InvAllFaultPhasesAtomicV2 =>
      forall world new_root fault,
        world_live_authority
          (recover_from_publication_fault world new_root fault) =
            world_live_authority world \/
        world_live_authority
          (recover_from_publication_fault world new_root fault) = new_root
  | InvNoPrivateDoubleDropV2 =>
      forall footprint,
        NoDup
          (footprint_node_identities
            (candidate_private_footprint footprint)) /\
        NoDup
          (footprint_value_identities
            (candidate_private_footprint footprint))
  | InvCompactObservationDeterminismV2 =>
      forall left right,
        canonical_entries left ->
        canonical_entries right ->
        left = right ->
        canonical_compact_observation_v2 left =
          canonical_compact_observation_v2 right
  end.

Theorem every_publication_invariant_v2_is_derived :
  forall invariant scenario,
    interpret_publication_invariant_v2 invariant scenario.
Proof.
  intros invariant scenario.
  unfold interpret_publication_invariant_v2.
  destruct invariant; simpl.
  - destruct (scenario_operation scenario) eqn:Hoperation; simpl; try exact I.
    pose proof (scenario_execution_success scenario) as Hsuccess.
    rewrite Hoperation in Hsuccess. simpl in Hsuccess.
    eapply strict_exact_cursor_to_logical_transform_refinement in Hsuccess.
    tauto.
  - destruct (scenario_operation scenario) eqn:Hoperation; simpl; try exact I.
    pose proof (scenario_execution_success scenario) as Hsuccess.
    rewrite Hoperation in Hsuccess. simpl in Hsuccess. exact Hsuccess.
  - unfold scenario_trace, logical_operation_trace.
    pose proof (closed_operation_trace_refines_all_projections
      (atomic_root_entries (scenario_source_root scenario))
      (logical_operation_removals (scenario_operation scenario))
      (logical_operation_deletions (scenario_operation scenario))
      (logical_operation_additions (scenario_operation scenario))
      (scenario_output scenario)) as Hprojections.
    destruct Hprojections as
      [Hsource [Hremoval [Hdeletion [Hreplacement
       [Hemitted [Hsource_count [Hremoval_count [Hdeletion_count
       [Hreplacement_count Hdestination_count]]]]]]]]].
    repeat split; assumption.
  - unfold scenario_trace, logical_operation_trace.
    pose proof (closed_operation_trace_refines_all_projections
      (atomic_root_entries (scenario_source_root scenario))
      (logical_operation_removals (scenario_operation scenario))
      (logical_operation_deletions (scenario_operation scenario))
      (logical_operation_additions (scenario_operation scenario))
      (scenario_output scenario)) as Hprojections.
    destruct Hprojections as
      [Hsource [Hremoval [Hdeletion [Hreplacement
       [Hemitted [Hsource_count [Hremoval_count [Hdeletion_count
       [Hreplacement_count Hdestination_count]]]]]]]]].
    exact Hemitted.
  - unfold scenario_trace, logical_operation_trace.
    rewrite closed_operation_trace_linear_in_inputs_and_output.
    pose proof (logical_operation_output_has_linear_size_bound
      (atomic_root_entries (scenario_source_root scenario))
      (scenario_operation scenario) (scenario_output scenario)
      (scenario_execution_success scenario)) as Houtput.
    lia.
  - apply strict_exact_remove_rejects_absent_positive.
  - apply strict_exact_remove_rejects_underflow.
  - apply idempotent_exact_delete_no_matching_endpoint.
  - intros left right Hoverflow.
    unfold checked_add_multiplicity.
    assert (Hpositive : N.ltb 0 (left + right) = true).
    { apply N.ltb_lt. eapply N.le_lt_trans.
      - apply N.le_0_l.
      - exact Hoverflow. }
    rewrite Hpositive.
    assert (Hbound : N.leb (left + right) multiplicity_max = false).
    { apply N.leb_gt. exact Hoverflow. }
    rewrite Hbound. reflexivity.
  - apply failed_bag_delta_preserves_source.
  - intros prefix path.
    destruct (classify_prefix prefix path); auto.
  - intros prefix below suffix above Hbelow Habove. split.
    + apply prefix_below_is_before_every_prefix_extension
        with (prefix := prefix).
      * exact Hbelow.
      * exists suffix. reflexivity.
    + apply every_prefix_extension_is_before_prefix_above
        with (prefix := prefix).
      * exists suffix. reflexivity.
      * exact Habove.
  - apply empty_prefix_selects_every_path_inside.
  - apply maximal_byte_prefix_needs_no_successor.
  - intros prefix relative. split.
    + apply splice_below_precedes_relative_replacements.
    + apply splice_relative_replacements_precede_above.
  - intros prefix relative. reflexivity.
  - intros prefix relative output Hsuccess.
    destruct (preserve_prefix_mode_has_checked_logical_output
      (atomic_root_entries (scenario_source_root scenario))
      prefix relative output Hsuccess) as [inside [Hinside _]].
    exists inside. exact Hinside.
  - assert (Hcanonical : canonical_entries (scenario_output scenario)).
    { eapply successful_valid_logical_operation_is_canonical.
      - apply scenario_operation_valid.
      - apply scenario_execution_success. }
    exists (PreparedFromClosedValidatedCursor
      (atomic_root_entries (scenario_source_root scenario))
      (scenario_operation scenario) (scenario_output scenario)
      (scenario_trace scenario) (scenario_output_metadata scenario)
      (canonical_compressed_image (scenario_output scenario))
      (scenario_execution_success scenario) eq_refl Hcanonical
      (scenario_output_metadata_checked scenario) eq_refl).
    repeat split; reflexivity.
  - intros closed published Htransition. inversion Htransition.
  - intros entries first second Hfirst Hsecond. congruence.
  - apply equal_logical_entries_have_equal_structural_act_images.
  - apply allocated_candidate_identity_is_genuinely_outside_complete_footprint.
  - apply scenario_output_metadata_checked.
  - intros world. reflexivity.
  - intros world new_root. reflexivity.
  - apply publish_disposes_previously_live_handle_exactly_once.
  - apply unique_value_only_update_retains_anchor.
  - apply shared_value_update_may_detach_without_mutating_aliases.
  - apply pruning_delta_uses_independent_stream.
  - apply nonpruning_insertion_delta_uses_private_cow.
  - split.
    + unfold scenario_trace, logical_operation_trace.
      pose proof (closed_operation_trace_refines_all_projections
        (atomic_root_entries (scenario_source_root scenario))
        (logical_operation_removals (scenario_operation scenario))
        (logical_operation_deletions (scenario_operation scenario))
        (logical_operation_additions (scenario_operation scenario))
        (scenario_output scenario)) as Hprojections.
      destruct Hprojections as
        [Hsource [Hremoval [Hdeletion [Hreplacement
         [Hemitted [Hsource_count [Hremoval_count [Hdeletion_count
         [Hreplacement_count Hdestination_count]]]]]]]]].
      exact Hdestination_count.
    + intros identity previous next. reflexivity.
  - intros identity previous next. reflexivity.
  - apply byte_compare_has_no_path_depth_cap.
  - apply byte_prefix_has_no_path_depth_cap.
  - apply byte_lcp_has_no_path_depth_cap.
  - apply byte_comparator_and_prefix_pd_as_are_constant_call_stack.
  - apply checked_exact_upsert_is_extensionally_exact.
  - apply idempotent_exact_delete_is_extensionally_exact.
  - apply clear_prefix_replace_is_extensionally_exact.
  - apply preserve_prefix_replace_is_extensionally_exact.
  - eapply successful_valid_logical_operation_is_canonical.
    + apply scenario_operation_valid.
    + apply scenario_execution_success.
  - apply native_relative_prefix_subtract_never_targets_outside_prefix.
  - apply every_fault_observes_either_old_or_fully_published_authority.
  - intro footprint. split.
    + apply candidate_unwind_has_no_internal_double_node_drop.
    + apply candidate_unwind_has_no_internal_double_value_drop.
  - apply compact_observation_is_deterministic_across_equal_logical_entries.
Qed.

Theorem every_cataloged_publication_invariant_v2_holds :
  forall scenario invariant,
    In invariant publication_invariant_catalog_v2 ->
    interpret_publication_invariant_v2 invariant scenario.
Proof.
  intros scenario invariant _. apply every_publication_invariant_v2_is_derived.
Qed.

Theorem publication_invariant_catalog_v2_is_unique_complete_and_exact :
  NoDup publication_invariant_catalog_v2 /\
  (forall scenario,
    Forall (fun invariant =>
      interpret_publication_invariant_v2 invariant scenario)
      publication_invariant_catalog_v2) /\
  length publication_invariant_catalog_v2 = 45.
Proof.
  repeat split.
  - repeat constructor; simpl; intuition discriminate.
  - intro scenario. apply Forall_forall. intros invariant Hinvariant.
    apply every_cataloged_publication_invariant_v2_holds. exact Hinvariant.
Qed.

(** Complete-path exactness: endpoint operations never alias a root, ancestor,
    descendant, or lexical neighbor merely because bytes share a prefix. *)
Lemma path_not_equal_to_strict_extension :
  forall (path : Path) (byte : Byte) (suffix : Path),
    path <> path ++ byte :: suffix.
Proof.
  intros path byte suffix Hequal.
  apply (f_equal (@length Byte)) in Hequal.
  rewrite length_app in Hequal. simpl in Hequal. lia.
Qed.

Theorem path_eqb_rejects_strict_descendant :
  forall path byte suffix,
    path_eqb path (path ++ byte :: suffix) = false.
Proof.
  intros path byte suffix.
  destruct (path_eqb path (path ++ byte :: suffix)) eqn:Hequal;
    [|reflexivity].
  apply path_eqb_spec in Hequal.
  exfalso. eapply path_not_equal_to_strict_extension. exact Hequal.
Qed.

Theorem exact_lookup_separates_ancestor_and_descendant :
  forall path byte suffix count,
    lookup_count path [((path ++ byte :: suffix), count)] = 0%N.
Proof.
  intros path byte suffix count. simpl.
  rewrite path_eqb_rejects_strict_descendant. reflexivity.
Qed.

Theorem exact_lookup_separates_root_from_nonroot :
  forall byte suffix count,
    lookup_count [] [((byte :: suffix), count)] = 0%N.
Proof.
  intros byte suffix count.
  replace (byte :: suffix) with ([] ++ byte :: suffix) by reflexivity.
  apply exact_lookup_separates_ancestor_and_descendant.
Qed.

Theorem exact_lookup_separates_lexical_neighbors :
  forall prefix left_byte right_byte suffix count,
    left_byte <> right_byte ->
    lookup_count (prefix ++ left_byte :: suffix)
      [((prefix ++ right_byte :: suffix), count)] = 0%N.
Proof.
  intros prefix left_byte right_byte suffix count Hneq. simpl.
  destruct (path_eqb (prefix ++ left_byte :: suffix)
                     (prefix ++ right_byte :: suffix)) eqn:Hequal;
    [|reflexivity].
  apply path_eqb_spec in Hequal.
  apply app_inv_head in Hequal. inversion Hequal. contradiction.
Qed.

Print Assumptions every_cataloged_publication_invariant_v2_holds.
Print Assumptions publication_invariant_catalog_v2_is_unique_complete_and_exact.
Print Assumptions exact_lookup_separates_ancestor_and_descendant.
Print Assumptions exact_lookup_separates_root_from_nonroot.
Print Assumptions exact_lookup_separates_lexical_neighbors.
