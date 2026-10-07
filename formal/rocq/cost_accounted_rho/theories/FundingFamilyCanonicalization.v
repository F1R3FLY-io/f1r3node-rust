From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import EligibleFundingAssignment FundingIdentityOrder FundingFamilyFeasibility FundingFamilyOptimization LexicographicMinimax.
Import ListNotations.

Theorem family_outcome_permutation_preserves_hold : forall draws reordered source,
  Permutation draws reordered -> family_source_hold draws source = family_source_hold reordered source.
Proof.
  intros draws reordered source same. unfold family_source_hold.
  apply Permutation_map with (f := fun draw : nat -> nat => draw source) in same.
  induction same; simpl; [reflexivity|now rewrite IHsame|lia|congruence].
Qed.

Theorem family_outcome_permutation_preserves_exposure : forall count draws reordered,
  Permutation draws reordered ->
  family_hold_sum count (family_source_hold draws) = family_hold_sum count (family_source_hold reordered).
Proof.
  intros count draws reordered same. unfold family_hold_sum.
  f_equal. apply map_ext. intro source. now apply family_outcome_permutation_preserves_hold.
Qed.

Definition family_canonical_flow (rows columns : nat -> nat) (flow : nat -> nat -> nat) :=
  fun source obligation => flow (rows source) (columns obligation).

Theorem family_canonical_case_preserves_source_debits : forall obligations rows columns flow source,
  funding_index_permutation obligations columns ->
  source_draw obligations (family_canonical_flow rows columns flow) source = source_draw obligations flow (rows source).
Proof.
  intros. unfold family_canonical_flow. now apply funding_reindex_preserves_source_draw.
Qed.

Theorem family_canonical_holds_restore_source_custody : forall draws rows source,
  family_source_hold (map (fun draw => fun i => draw (rows i)) draws) source = family_source_hold draws (rows source).
Proof.
  intros draws rows source. unfold family_source_hold. rewrite map_map. reflexivity.
Qed.

Theorem family_canonical_exposure_preserved : forall count draws rows,
  funding_index_permutation count rows ->
  family_hold_sum count (fun i => family_source_hold draws (rows i)) = family_hold_sum count (family_source_hold draws).
Proof.
  intros count draws rows same. unfold family_hold_sum. rewrite <- !funding_sum_is_list_sum.
  now apply funding_reindex_preserves_sum.
Qed.

Theorem family_canonical_source_bounds_preserved : forall count rows draw capacity,
  funding_index_permutation count rows ->
  (forall i, i < count -> draw i <= capacity i) ->
  forall i, i < count -> draw (rows i) <= capacity (rows i).
Proof.
  intros count rows draw capacity order bounded i inside.
  apply bounded. eapply funding_index_permutation_stays_in_bounds; eauto.
Qed.

Record family_normalized_outcome := {
  family_outcome_fee : nat;
  family_outcome_keys : list (list nat);
  family_outcome_amounts : list nat;
  family_outcome_fee_eligible : list bool;
  family_outcome_resource_eligible : list (list bool)
}.

Definition family_outcome_key outcome :=
  (family_outcome_fee outcome, family_outcome_keys outcome,
   family_outcome_amounts outcome, family_outcome_fee_eligible outcome,
   family_outcome_resource_eligible outcome).

Definition family_outcome_assignment sources capacity outcome flow :=
  length (family_outcome_keys outcome) = length (family_outcome_amounts outcome) /\
  length (family_outcome_fee_eligible outcome) = sources /\
  length (family_outcome_resource_eligible outcome) = sources /\
  Forall (fun row => length row = length (family_outcome_amounts outcome))
    (family_outcome_resource_eligible outcome) /\
  assignment_valid sources (S (length (family_outcome_amounts outcome)))
    (fun source obligation =>
      match obligation with
      | 0 => nth source (family_outcome_fee_eligible outcome) false
      | S resource => nth resource (nth source (family_outcome_resource_eligible outcome) []) false
      end)
    capacity
    (fun obligation =>
      match obligation with
      | 0 => family_outcome_fee outcome
      | S resource => nth resource (family_outcome_amounts outcome) 0
      end) flow.

Theorem family_normalized_key_injective : forall left right,
  family_outcome_key left = family_outcome_key right -> left = right.
Proof.
  intros [lf lk la le lr] [rf rk ra re rr] same.
  unfold family_outcome_key in same. simpl in same. inversion same. reflexivity.
Qed.

Theorem family_normalized_key_binds_assignment_domain : forall sources capacity left right flow,
  family_outcome_key left = family_outcome_key right ->
  (family_outcome_assignment sources capacity left flow <->
   family_outcome_assignment sources capacity right flow).
Proof.
  intros sources capacity left right flow same.
  apply family_normalized_key_injective in same. now subst right.
Qed.

Theorem family_normalized_key_retains_occurrence_multiplicity : forall outcome key amount,
  family_outcome_keys outcome = [key; key] ->
  family_outcome_amounts outcome = [amount; amount] ->
  forall sources capacity flow,
  family_outcome_assignment sources capacity outcome flow ->
  funding_sum sources (source_draw 3 flow) = family_outcome_fee outcome + amount + amount.
Proof.
  intros outcome key amount keys amounts sources capacity flow [_ [_ [_ [_ valid]]]].
  rewrite amounts in valid.
  pose proof (accepted_assignment_conserves_obligation _ _ _ _ _ _ valid) as conserved.
  simpl in conserved. lia.
Qed.

Section DuplicateOutcomeNormalization.
  Context {Key Plan : Type}.
  Variable key_eq : forall left right : Key, {left = right} + {left <> right}.
  Variable draw : Plan -> nat -> nat.
  Variable valid : Key -> Plan -> Prop.
  Variable rank : Plan -> list nat.

  Definition family_copy_duplicate key representative (entry : Key * Plan) :=
    if key_eq (fst entry) key then (fst entry, representative) else entry.

  Definition family_copy_duplicates key representative entries :=
    map (family_copy_duplicate key representative) entries.

  Lemma family_finite_plan_minimum : forall plans,
    plans <> [] -> exists representative,
      In representative plans /\
      forall plan, In plan plans -> funding_lex_le (rank representative) (rank plan).
  Proof.
    induction plans as [|first rest IH]; intro nonempty; [contradiction|].
    destruct rest as [|next tail].
    - exists first. split; [now left|]. intros plan [same|absent];
        [subst; apply funding_lex_reflexive|contradiction].
    - destruct (IH ltac:(discriminate)) as [best [member minimum]].
      destruct (funding_lex_total (rank first) (rank best)) as [first_best|best_first].
      + exists first. split; [now left|]. intros plan [same|inside].
        * subst. apply funding_lex_reflexive.
        * eapply funding_lex_transitive; [exact first_best|now apply minimum].
      + exists best. split; [now right|]. intros plan [same|inside];
          [now subst|now apply minimum].
  Qed.

  Theorem family_duplicate_minimum_exists : forall (key : Key) (entries : list (Key * Plan)),
    (exists plan, In (key, plan) entries) ->
    exists representative,
      In (key, representative) entries /\
      forall plan, In (key, plan) entries -> funding_lex_le (rank representative) (rank plan).
  Proof.
    intros key entries [first present].
    set (plans := map snd (filter (fun entry => if key_eq (fst entry) key then true else false) entries)).
    assert (membership : forall plan, In plan plans <-> In (key, plan) entries).
    { intro plan. unfold plans. rewrite in_map_iff. split.
      - intros [[entry_key entry_plan] [same included]]. simpl in same. subst entry_plan.
        apply filter_In in included. destruct included as [included matches]. simpl in matches.
        destruct (key_eq entry_key key); [now subst|discriminate].
      - intro included. exists (key, plan). split; [reflexivity|]. apply filter_In.
        split; [exact included|]. simpl. destruct (key_eq key key); [reflexivity|contradiction]. }
    assert (nonempty : plans <> []).
    { intro empty. apply membership in present. rewrite empty in present. contradiction. }
    destruct (family_finite_plan_minimum plans nonempty) as [best [member minimum]].
    exists best. split; [now apply membership|]. intros plan included.
    apply minimum. now apply membership.
  Qed.

  Theorem family_duplicate_copy_preserves_outcome_count : forall key representative entries,
    length (family_copy_duplicates key representative entries) = length entries.
  Proof. intros. apply length_map. Qed.

  Theorem family_duplicate_copy_preserves_outcome_keys : forall key representative entries,
    map fst (family_copy_duplicates key representative entries) = map fst entries.
  Proof.
    intros. unfold family_copy_duplicates. rewrite map_map. apply map_ext.
    intros [entry_key plan]. unfold family_copy_duplicate. simpl.
    destruct (key_eq entry_key key); reflexivity.
  Qed.

  Theorem family_duplicate_copy_preserves_domains : forall key representative entries,
    valid key representative -> Forall (fun entry => valid (fst entry) (snd entry)) entries ->
    Forall (fun entry => valid (fst entry) (snd entry))
      (family_copy_duplicates key representative entries).
  Proof.
    intros key representative entries authorized all_valid.
    unfold family_copy_duplicates. apply Forall_map. eapply Forall_impl; [|exact all_valid].
    intros [entry_key plan] member_valid. unfold family_copy_duplicate. simpl in *.
    destruct (key_eq entry_key key); subst; assumption.
  Qed.

  Theorem family_duplicate_copy_does_not_increase_holds : forall key representative entries source,
    In (key, representative) entries ->
    family_source_hold (map (fun entry => draw (snd entry))
      (family_copy_duplicates key representative entries)) source <=
    family_source_hold (map (fun entry => draw (snd entry)) entries) source.
  Proof.
    intros key representative entries source present. apply family_source_hold_bound.
    intros copied copied_in. apply in_map_iff in copied_in.
    destruct copied_in as [entry [same entry_in]]. subst copied.
    unfold family_copy_duplicates in entry_in. apply in_map_iff in entry_in.
    destruct entry_in as [[entry_key plan] [same entry_in]]. subst entry.
    unfold family_copy_duplicate. simpl. destruct (key_eq entry_key key).
    - apply (proj1 (family_source_hold_bound _ _ _) (Nat.le_refl _)).
      apply in_map_iff. exists (key, representative). auto.
    - apply (proj1 (family_source_hold_bound _ _ _) (Nat.le_refl _)).
      apply in_map_iff. exists (entry_key, plan). auto.
  Qed.

  Theorem family_duplicate_copy_preserves_exposure : forall count capacity exposure key representative entries,
    In (key, representative) entries ->
    (forall source, source < count ->
      family_source_hold (map (fun entry => draw (snd entry)) entries) source <= capacity source) ->
    family_hold_sum count (family_source_hold (map (fun entry => draw (snd entry)) entries)) <= exposure ->
    (forall source, source < count ->
      family_source_hold (map (fun entry => draw (snd entry))
        (family_copy_duplicates key representative entries)) source <= capacity source) /\
    family_hold_sum count (family_source_hold (map (fun entry => draw (snd entry))
      (family_copy_duplicates key representative entries))) <= exposure.
  Proof.
    intros count capacity exposure key representative entries present bounded fits. split.
    - intros source inside. eapply Nat.le_trans;
        [apply family_duplicate_copy_does_not_increase_holds; exact present|now apply bounded].
    - eapply Nat.le_trans; [apply family_hold_sum_monotone|exact fits].
      intros source _. now apply family_duplicate_copy_does_not_increase_holds.
  Qed.

  Theorem family_duplicate_copy_preserves_schedule_preference : forall schedule key representative entries default,
    (forall plan, In (key, plan) entries -> length (rank representative) = length (rank plan)) ->
    (forall plan, In (key, plan) entries -> funding_lex_le (rank representative) (rank plan)) ->
    funding_lex_le
      (funding_schedule_emit schedule
        (fun row => nth row (map (fun entry => rank (snd entry))
          (family_copy_duplicates key representative entries)) default))
      (funding_schedule_emit schedule
        (fun row => nth row (map (fun entry => rank (snd entry)) entries) default)).
  Proof.
    intros schedule key representative entries default sizes preferred.
    assert (rows : Forall2
      (fun left right => length left = length right /\ funding_lex_le left right)
      (map (fun entry => rank (snd entry)) (family_copy_duplicates key representative entries))
      (map (fun entry => rank (snd entry)) entries)).
    { unfold family_copy_duplicates. rewrite map_map.
      induction entries as [|[entry_key plan] rest IH]; simpl; constructor.
      - unfold family_copy_duplicate. simpl. destruct (key_eq entry_key key) as [same|different].
        + subst entry_key. split; [apply sizes|apply preferred]; now left.
        + split; [reflexivity|apply funding_lex_reflexive].
      - apply IH; intros item inside; [apply sizes|apply preferred]; now right. }
    assert (nth_rows : forall row,
      length (nth row (map (fun entry => rank (snd entry))
        (family_copy_duplicates key representative entries)) default) =
      length (nth row (map (fun entry => rank (snd entry)) entries) default) /\
      funding_lex_le
        (nth row (map (fun entry => rank (snd entry))
          (family_copy_duplicates key representative entries)) default)
        (nth row (map (fun entry => rank (snd entry)) entries) default)).
    { induction rows; intros [|row]; simpl; auto; split; auto using funding_lex_reflexive. }
    apply funding_schedule_preserves_local_minima; intros row; apply (nth_rows row).
  Qed.

  Theorem family_duplicate_copy_selects_identical_plans : forall key representative entries plan,
    In (key, plan) (family_copy_duplicates key representative entries) -> plan = representative.
  Proof.
    intros key representative entries plan included. unfold family_copy_duplicates in included.
    apply in_map_iff in included. destruct included as [[entry_key entry_plan] [same _]].
    unfold family_copy_duplicate in same. simpl in same.
    destruct (key_eq entry_key key); inversion same; subst; congruence.
  Qed.

  Theorem family_duplicate_normalization_has_feasible_no_worse_witness :
    forall count capacity exposure schedule key entries default,
    (exists plan, In (key, plan) entries) ->
    Forall (fun entry => valid (fst entry) (snd entry)) entries ->
    (forall left right, In (key, left) entries -> In (key, right) entries ->
      length (rank left) = length (rank right)) ->
    (forall source, source < count ->
      family_source_hold (map (fun entry => draw (snd entry)) entries) source <= capacity source) ->
    family_hold_sum count (family_source_hold (map (fun entry => draw (snd entry)) entries)) <= exposure ->
    exists representative,
      In (key, representative) entries /\
      Forall (fun entry => valid (fst entry) (snd entry))
        (family_copy_duplicates key representative entries) /\
      (forall source, source < count ->
        family_source_hold (map (fun entry => draw (snd entry))
          (family_copy_duplicates key representative entries)) source <= capacity source) /\
      family_hold_sum count (family_source_hold (map (fun entry => draw (snd entry))
        (family_copy_duplicates key representative entries))) <= exposure /\
      funding_lex_le
        (funding_schedule_emit schedule
          (fun row => nth row (map (fun entry => rank (snd entry))
            (family_copy_duplicates key representative entries)) default))
        (funding_schedule_emit schedule
          (fun row => nth row (map (fun entry => rank (snd entry)) entries) default)) /\
      forall plan, In (key, plan) (family_copy_duplicates key representative entries) -> plan = representative.
  Proof.
    intros count capacity exposure schedule key entries default present authorized sizes bounded fits.
    destruct (family_duplicate_minimum_exists key entries present) as [best [member minimum]].
    destruct (family_duplicate_copy_preserves_exposure count capacity exposure key best entries member bounded fits)
      as [new_bounded new_fits].
    exists best. split; [exact member|]. split.
    - apply family_duplicate_copy_preserves_domains; [|exact authorized].
      rewrite Forall_forall in authorized. exact (authorized (key, best) member).
    - split; [exact new_bounded|]. split; [exact new_fits|]. split.
      + apply family_duplicate_copy_preserves_schedule_preference; [|exact minimum].
        intros plan included. now apply sizes.
      + intros plan included. now apply family_duplicate_copy_selects_identical_plans in included.
  Qed.
End DuplicateOutcomeNormalization.

Print Assumptions family_outcome_permutation_preserves_exposure.
Print Assumptions family_canonical_case_preserves_source_debits.
Print Assumptions family_canonical_holds_restore_source_custody.
Print Assumptions family_canonical_exposure_preserved.
Print Assumptions family_canonical_source_bounds_preserved.
Print Assumptions family_normalized_key_binds_assignment_domain.
Print Assumptions family_normalized_key_retains_occurrence_multiplicity.
Print Assumptions family_duplicate_copy_preserves_domains.
Print Assumptions family_duplicate_copy_preserves_exposure.
Print Assumptions family_duplicate_copy_preserves_schedule_preference.
Print Assumptions family_duplicate_minimum_exists.
Print Assumptions family_duplicate_normalization_has_feasible_no_worse_witness.
