(** Root locally-free quotient of the RPB3 Par reduction (design section 4, R5).

    Par field 9 ([locally_free]) is a free-variable cache at the root of a Par.
    Production quotients it out of the semantic score and the semantic default
    of a Par while keeping it observable in the raw body:

    - [root_locally_free_quotient]: for two Par-program fields of equal raw-body
      validity, adding one admitted root tag-9 scalar child leaves the score
      evidence, the completeness knowledge and the definite-refusal verdict of
      [reduce_semantic_field] unchanged;
    - [root_locally_free_quotient_default]: [source_par_semantic_default] is
      unchanged by a tag-9 node.

    The wire-level witnesses mirror the Rust tests
    [raw_par_score_preserves_multiplicity_and_quotients_only_root_locally_free]
    and [raw_par_default_equality_ignores_only_root_locally_free]; the two
    mutation controls make the quotient non-vacuous from both sides (decision
    HD-4 of the design). *)

From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement RPB3SemanticReducer.
From StackSafePDA Require RPB3WireCursor RPB3RawRegionTree
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.

(** The generated program the reducer resolves for a raw Par field. *)
Definition par_program : generated_program := raw_par_program.

(** A child is a root locally-free child when it is the tag-9 length-delimited
    scalar and is not itself refused. *)
Definition root_locally_free_child (c : semantic_summary) : bool :=
  (sem_tag c =? 9) && payload_eqb (sem_payload c) Scalar &&
  wire_eqb (sem_wire c) LengthDelimited && negb (sem_definitely_refused c).

(** ** Facts about the generated Par program. *)

Lemma lookup_projected_par_program :
  lookup_projected_program 1 [] [] = Some par_program.
Proof. vm_compute. reflexivity. Qed.

Definition root_locally_free_descriptor : field_descriptor :=
  FieldDescriptor 9 7 LengthDelimited false Scalar None None.

Lemma par_program_resolves_tag_nine :
  resolve_tag (program_fields par_program) 9 =
    Some root_locally_free_descriptor.
Proof. vm_compute. reflexivity. Qed.

Lemma generated_wire_program_par_is_message :
  forall field,
    generated_wire_program field = Some par_program ->
    raw_payload field = Message.
Proof.
  intros field resolved.
  unfold generated_wire_program in resolved.
  destruct (raw_payload field); try reflexivity;
    destruct (raw_program_type_id field) as [type_id|];
    apply (f_equal (option_map program_family)) in resolved;
    vm_compute in resolved;
    discriminate resolved.
Qed.

Lemma root_locally_free_child_facts :
  forall lf,
    root_locally_free_child lf = true ->
    sem_tag lf = 9 /\ sem_payload lf = Scalar /\
    sem_wire lf = LengthDelimited /\ sem_definitely_refused lf = false.
Proof.
  intros lf is_lf.
  unfold root_locally_free_child in is_lf.
  apply andb_true_iff in is_lf as [is_lf accepted].
  apply andb_true_iff in is_lf as [is_lf wire].
  apply andb_true_iff in is_lf as [tag payload].
  apply Nat.eqb_eq in tag.
  apply negb_true_iff in accepted.
  split; [exact tag|].
  split.
  - destruct (sem_payload lf); [reflexivity|discriminate payload..].
  - split; [|exact accepted].
    destruct (sem_wire lf); [discriminate wire|discriminate wire|reflexivity|discriminate wire].
Qed.

(** ** The reduction of a Par-program field, read off [reduce_semantic_field]. *)

Lemma reduce_semantic_field_par_evidence :
  forall field children,
    generated_wire_program field = Some par_program ->
    sem_score_evidence (reduce_semantic_field field children) =
      match exact_par_score children with
      | Some score => ExactScore score
      | None => ScoreNotDerived
      end.
Proof.
  intros field children resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma reduce_semantic_field_par_complete :
  forall field children,
    generated_wire_program field = Some par_program ->
    sem_complete (reduce_semantic_field field children) = None.
Proof.
  intros field children resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma par_refusal_boolean_shape :
  forall valid_body scan_ok bad : bool,
    negb true || negb valid_body || negb true || negb scan_ok ||
      negb true || bad || false || false =
    negb valid_body || negb scan_ok || bad.
Proof. intros [] [] []; reflexivity. Qed.

Lemma reduce_semantic_field_par_refused :
  forall field children,
    generated_wire_program field = Some par_program ->
    sem_definitely_refused (reduce_semantic_field field children) =
      negb (valid_raw_body field) ||
      negb (scan_valid (fold_left (scan_child par_program) children empty_scan)) ||
      order_definitely_bad (raw_par_component_order children).
Proof.
  intros field children resolved.
  pose proof (generated_wire_program_par_is_message field resolved) as message.
  unfold reduce_semantic_field.
  rewrite resolved, message.
  exact (par_refusal_boolean_shape (valid_raw_body field)
    (scan_valid (fold_left (scan_child par_program) children empty_scan))
    (order_definitely_bad (raw_par_component_order children))).
Qed.

(** ** Scan admissibility, factored out of [scan_child]. *)

Definition scan_admissible (program : generated_program) (state : scan_state)
    (child : semantic_summary) (descriptor : field_descriptor) : bool :=
  negb (sem_definitely_refused child) &&
  wire_eqb (sem_wire child) (descriptor_wire descriptor) &&
  payload_eqb (sem_payload child) (descriptor_payload descriptor) &&
  match descriptor_payload descriptor with
  | Message =>
      match descriptor_child_type_id descriptor with
      | Some type_id => sem_type_id child =? type_id
      | None => false
      end
  | _ => true
  end &&
  (descriptor_slot descriptor <? program_slot_count program) &&
  slot_follows (scan_last_slot state) (descriptor_slot descriptor)
    (descriptor_repeated descriptor) &&
  (descriptor_repeated descriptor ||
    negb (existsb (Nat.eqb (descriptor_slot descriptor)) (scan_seen state))).

Lemma scan_child_resolved :
  forall program state child descriptor,
    scan_valid state = true ->
    resolve_tag (program_fields program) (sem_tag child) = Some descriptor ->
    scan_child program state child =
      if scan_admissible program state child descriptor
      then ScanState true (Some (descriptor_slot descriptor))
             (descriptor_slot descriptor :: scan_seen state)
             (scan_ordered_slots state ++ [descriptor_slot descriptor]) []
      else ScanState false (scan_last_slot state) (scan_seen state)
             (scan_ordered_slots state) [].
Proof.
  intros program state child descriptor valid resolved.
  unfold scan_child.
  rewrite valid, resolved.
  reflexivity.
Qed.

Lemma scan_child_on_invalid :
  forall program state child,
    scan_valid state = false -> scan_child program state child = state.
Proof.
  intros program state child invalid.
  unfold scan_child.
  rewrite invalid.
  reflexivity.
Qed.

Lemma scan_child_admitted :
  forall program state child,
    scan_valid (scan_child program state child) = true ->
    scan_valid state = true /\
    exists descriptor,
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      scan_admissible program state child descriptor = true /\
      scan_child program state child =
        ScanState true (Some (descriptor_slot descriptor))
          (descriptor_slot descriptor :: scan_seen state)
          (scan_ordered_slots state ++ [descriptor_slot descriptor]) [].
Proof.
  intros program state child valid_step.
  destruct (scan_valid state) eqn:state_valid.
  - split; [reflexivity|].
    destruct (resolve_tag (program_fields program) (sem_tag child))
      as [descriptor|] eqn:resolved.
    + pose proof (scan_child_resolved program state child descriptor
        state_valid resolved) as unfolded.
      destruct (scan_admissible program state child descriptor) eqn:admissible.
      * exists descriptor.
        split; [reflexivity|].
        split; [exact admissible|].
        exact unfolded.
      * exfalso.
        rewrite unfolded in valid_step.
        discriminate valid_step.
    + exfalso.
      unfold scan_child in valid_step.
      rewrite state_valid, resolved in valid_step.
      discriminate valid_step.
  - exfalso.
    rewrite (scan_child_on_invalid program state child state_valid) in valid_step.
    rewrite state_valid in valid_step.
    discriminate valid_step.
Qed.

Lemma scan_fold_on_invalid :
  forall program children state,
    scan_valid state = false ->
    fold_left (scan_child program) children state = state.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state invalid; cbn [fold_left].
  - reflexivity.
  - rewrite (scan_child_on_invalid program state child invalid).
    apply IH. exact invalid.
Qed.

Lemma scan_fold_valid_head :
  forall program head tail state,
    scan_valid (fold_left (scan_child program) (head :: tail) state) = true ->
    scan_valid (scan_child program state head) = true /\
    scan_valid (fold_left (scan_child program) tail
      (scan_child program state head)) = true.
Proof.
  intros program head tail state valid.
  cbn [fold_left] in valid.
  split; [|exact valid].
  destruct (scan_valid (scan_child program state head)) eqn:after; [reflexivity|].
  exfalso.
  rewrite (scan_fold_on_invalid program tail _ after) in valid.
  rewrite after in valid.
  discriminate valid.
Qed.

(** ** Scan validity is preserved by removing an admitted child. *)

Definition last_slot_below (smaller larger : option nat) : Prop :=
  match smaller, larger with
  | None, _ => True
  | Some lower, Some upper => lower <= upper
  | Some _, None => False
  end.

Definition scan_related (with_state without_state : scan_state) : Prop :=
  scan_valid with_state = true ->
  scan_valid without_state = true /\
  last_slot_below (scan_last_slot without_state) (scan_last_slot with_state) /\
  (forall slot,
    existsb (Nat.eqb slot) (scan_seen without_state) = true ->
    existsb (Nat.eqb slot) (scan_seen with_state) = true).

Lemma slot_follows_monotone :
  forall smaller larger slot repeated,
    last_slot_below smaller larger ->
    slot_follows larger slot repeated = true ->
    slot_follows smaller slot repeated = true.
Proof.
  intros smaller larger slot repeated below follows.
  destruct smaller as [lower|], larger as [upper|];
    cbn [last_slot_below slot_follows] in below, follows |- *.
  - apply orb_true_iff in follows.
    apply orb_true_iff.
    destruct follows as [less|equal].
    + left.
      apply Nat.ltb_lt in less.
      apply Nat.ltb_lt.
      lia.
    + apply andb_true_iff in equal as [equal repeated_true].
      apply Nat.eqb_eq in equal.
      destruct (Nat.lt_ge_cases lower slot) as [less|at_least].
      * left. apply Nat.ltb_lt. exact less.
      * right. apply andb_true_iff. split.
        -- apply Nat.eqb_eq. lia.
        -- exact repeated_true.
  - contradiction.
  - reflexivity.
  - reflexivity.
Qed.

Lemma scan_admissible_transfers :
  forall program with_state without_state child descriptor,
    scan_admissible program with_state child descriptor = true ->
    last_slot_below (scan_last_slot without_state) (scan_last_slot with_state) ->
    (forall slot,
      existsb (Nat.eqb slot) (scan_seen without_state) = true ->
      existsb (Nat.eqb slot) (scan_seen with_state) = true) ->
    scan_admissible program without_state child descriptor = true.
Proof.
  intros program with_state without_state child descriptor admissible
    last_below seen_below.
  unfold scan_admissible in *.
  apply andb_true_iff in admissible as [admissible unseen].
  apply andb_true_iff in admissible as [admissible follows].
  apply andb_true_iff.
  split.
  - apply andb_true_iff.
    split.
    + exact admissible.
    + exact (slot_follows_monotone _ _ _ _ last_below follows).
  - apply orb_true_iff in unseen.
    apply orb_true_iff.
    destruct unseen as [repeated|fresh].
    + left. exact repeated.
    + right.
      apply negb_true_iff in fresh.
      apply negb_true_iff.
      destruct (existsb (Nat.eqb (descriptor_slot descriptor))
        (scan_seen without_state)) eqn:present; [|reflexivity].
      exfalso.
      pose proof (seen_below (descriptor_slot descriptor) present) as seen_with.
      rewrite seen_with in fresh.
      discriminate fresh.
Qed.

Lemma scan_related_step :
  forall program child with_state without_state,
    scan_related with_state without_state ->
    scan_related (scan_child program with_state child)
      (scan_child program without_state child).
Proof.
  intros program child with_state without_state related valid_after.
  destruct (scan_child_admitted program with_state child valid_after)
    as [with_valid [descriptor [resolved [admissible with_next]]]].
  destruct (related with_valid) as [without_valid [last_below seen_below]].
  pose proof (scan_admissible_transfers program with_state without_state child
    descriptor admissible last_below seen_below) as admissible_without.
  pose proof (scan_child_resolved program without_state child descriptor
    without_valid resolved) as without_unfolded.
  rewrite admissible_without in without_unfolded.
  rewrite with_next, without_unfolded.
  split; [reflexivity|].
  split.
  - apply Nat.le_refl.
  - intros slot seen.
    cbn [scan_seen existsb] in seen |- *.
    apply orb_true_iff in seen.
    apply orb_true_iff.
    destruct seen as [head|tail].
    + left. exact head.
    + right. exact (seen_below slot tail).
Qed.

Lemma scan_related_after_removed_child :
  forall program state child,
    scan_related (scan_child program state child) state.
Proof.
  intros program state child valid_after.
  destruct (scan_child_admitted program state child valid_after)
    as [state_valid [descriptor [_ [admissible next]]]].
  split; [exact state_valid|].
  rewrite next.
  cbn [scan_last_slot scan_seen].
  split.
  - unfold scan_admissible in admissible.
    apply andb_true_iff in admissible as [admissible _].
    apply andb_true_iff in admissible as [_ follows].
    destruct (scan_last_slot state) as [previous|]; cbn [last_slot_below].
    + cbn [slot_follows] in follows.
      apply orb_true_iff in follows as [less|equal_repeated].
      * apply Nat.ltb_lt in less. lia.
      * apply andb_true_iff in equal_repeated as [equal _].
        apply Nat.eqb_eq in equal. lia.
    + exact I.
  - intros slot seen.
    cbn [existsb].
    apply orb_true_iff.
    right. exact seen.
Qed.

Lemma scan_related_fold :
  forall program children with_state without_state,
    scan_related with_state without_state ->
    scan_valid (fold_left (scan_child program) children with_state) = true ->
    scan_valid (fold_left (scan_child program) children without_state) = true.
Proof.
  intros program children.
  induction children as [|child rest IH];
    intros with_state without_state related valid.
  - cbn [fold_left] in valid |- *.
    exact (proj1 (related valid)).
  - cbn [fold_left] in valid |- *.
    exact (IH _ _ (scan_related_step program child with_state without_state
      related) valid).
Qed.

Lemma scan_valid_after_removal :
  forall program prefix removed suffix,
    scan_valid (fold_left (scan_child program) (prefix ++ removed :: suffix)
      empty_scan) = true ->
    scan_valid (fold_left (scan_child program) (prefix ++ suffix) empty_scan)
      = true.
Proof.
  intros program prefix removed suffix valid.
  rewrite fold_left_app in valid.
  rewrite fold_left_app.
  cbn [fold_left] in valid.
  exact (scan_related_fold program suffix _ _
    (scan_related_after_removed_child program _ removed) valid).
Qed.

(** ** Consecutive Par children around the tag-9 child have distinct tags. *)

Lemma par_scan_admitted_locally_free :
  forall state lf,
    sem_tag lf = 9 ->
    scan_valid (scan_child par_program state lf) = true ->
    slot_follows (scan_last_slot state) 7 false = true /\
    scan_last_slot (scan_child par_program state lf) = Some 7.
Proof.
  intros state lf lf_tag admitted.
  destruct (scan_child_admitted par_program state lf admitted)
    as [_ [descriptor [resolved [admissible next]]]].
  rewrite lf_tag, par_program_resolves_tag_nine in resolved.
  injection resolved as same.
  subst descriptor.
  split.
  - unfold scan_admissible in admissible.
    apply andb_true_iff in admissible as [admissible _].
    apply andb_true_iff in admissible as [_ follows].
    exact follows.
  - rewrite next. reflexivity.
Qed.

Lemma par_scan_successor_of_locally_free :
  forall state child,
    scan_last_slot state = Some 7 ->
    scan_valid (scan_child par_program state child) = true ->
    exists descriptor,
      resolve_tag (program_fields par_program) (sem_tag child) = Some descriptor /\
      7 <= descriptor_slot descriptor /\
      sem_tag child <> 9.
Proof.
  intros state child last_slot admitted.
  destruct (scan_child_admitted par_program state child admitted)
    as [_ [descriptor [resolved [admissible _]]]].
  unfold scan_admissible in admissible.
  apply andb_true_iff in admissible as [admissible _].
  apply andb_true_iff in admissible as [_ follows].
  rewrite last_slot in follows.
  cbn [slot_follows] in follows.
  exists descriptor.
  split; [exact resolved|].
  split.
  - apply orb_true_iff in follows as [less|equal_part].
    + apply Nat.ltb_lt in less. lia.
    + apply andb_true_iff in equal_part as [equal _].
      apply Nat.eqb_eq in equal. lia.
  - intro tag_nine.
    rewrite tag_nine, par_program_resolves_tag_nine in resolved.
    injection resolved as same.
    subst descriptor.
    vm_compute in follows.
    discriminate follows.
Qed.

Lemma par_scan_predecessor_of_locally_free :
  forall state child,
    scan_valid (scan_child par_program state child) = true ->
    slot_follows (scan_last_slot (scan_child par_program state child)) 7 false
      = true ->
    exists descriptor,
      resolve_tag (program_fields par_program) (sem_tag child) = Some descriptor /\
      descriptor_slot descriptor < 7 /\
      sem_tag child <> 9.
Proof.
  intros state child admitted follows.
  destruct (scan_child_admitted par_program state child admitted)
    as [_ [descriptor [resolved [_ next]]]].
  rewrite next in follows.
  cbn [scan_last_slot slot_follows] in follows.
  rewrite andb_false_r, orb_false_r in follows.
  apply Nat.ltb_lt in follows.
  exists descriptor.
  split; [exact resolved|].
  split; [exact follows|].
  intro tag_nine.
  rewrite tag_nine, par_program_resolves_tag_nine in resolved.
  injection resolved as same.
  subst descriptor.
  change (descriptor_slot root_locally_free_descriptor) with 7 in follows.
  lia.
Qed.

Lemma and_known_bad :
  forall left right,
    order_definitely_bad (and_known left right) =
      order_definitely_bad left || order_definitely_bad right.
Proof. intros [[|]|] [[|]|]; reflexivity. Qed.

Definition adjacent_order_bad (first second : semantic_summary) : bool :=
  order_definitely_bad
    (if sem_tag first =? sem_tag second
     then score_body_order first second else Some true).

Lemma adjacent_order_bad_distinct_tags :
  forall first second,
    sem_tag first <> sem_tag second ->
    adjacent_order_bad first second = false.
Proof.
  intros first second distinct.
  unfold adjacent_order_bad.
  apply Nat.eqb_neq in distinct.
  rewrite distinct.
  reflexivity.
Qed.

Lemma par_order_bad_cons_cons :
  forall first second rest,
    order_definitely_bad (raw_par_component_order (first :: second :: rest)) =
      adjacent_order_bad first second ||
      order_definitely_bad (raw_par_component_order (second :: rest)).
Proof.
  intros first second rest.
  change (raw_par_component_order (first :: second :: rest)) with
    (and_known
       (if sem_tag first =? sem_tag second
        then score_body_order first second else Some true)
       (raw_par_component_order (second :: rest))).
  unfold adjacent_order_bad.
  apply and_known_bad.
Qed.

Lemma par_order_bad_skip_locally_free :
  forall lf suffix,
    sem_tag lf = 9 ->
    forall prefix state,
      scan_valid (fold_left (scan_child par_program)
        (prefix ++ lf :: suffix) state) = true ->
      order_definitely_bad (raw_par_component_order (prefix ++ lf :: suffix)) =
      order_definitely_bad (raw_par_component_order (prefix ++ suffix)).
Proof.
  intros lf suffix lf_tag prefix.
  induction prefix as [|head rest IH]; intros state valid.
  - cbn [app] in valid |- *.
    destruct suffix as [|next tail]; [reflexivity|].
    rewrite (par_order_bad_cons_cons lf next tail).
    destruct (scan_fold_valid_head _ _ _ _ valid) as [lf_admitted after_lf].
    destruct (scan_fold_valid_head _ _ _ _ after_lf) as [next_admitted _].
    destruct (par_scan_admitted_locally_free _ lf lf_tag lf_admitted)
      as [_ last_seven].
    destruct (par_scan_successor_of_locally_free _ next last_seven next_admitted)
      as [_ [_ [_ next_tag]]].
    rewrite (adjacent_order_bad_distinct_tags lf next).
    + reflexivity.
    + rewrite lf_tag.
      intro same.
      apply next_tag.
      symmetry.
      exact same.
  - destruct rest as [|second rest'].
    + cbn [app] in valid |- *.
      destruct (scan_fold_valid_head _ _ _ _ valid)
        as [head_admitted after_head].
      destruct (scan_fold_valid_head _ _ _ _ after_head)
        as [lf_admitted after_lf].
      destruct (par_scan_admitted_locally_free _ lf lf_tag lf_admitted)
        as [lf_follows last_seven].
      destruct (par_scan_predecessor_of_locally_free state head head_admitted
        lf_follows) as [head_descriptor [head_resolved [head_below head_tag]]].
      assert (head_not_lf : sem_tag head <> sem_tag lf).
      { rewrite lf_tag. exact head_tag. }
      rewrite (par_order_bad_cons_cons head lf suffix).
      rewrite (adjacent_order_bad_distinct_tags head lf head_not_lf).
      destruct suffix as [|next tail].
      * reflexivity.
      * destruct (scan_fold_valid_head _ _ _ _ after_lf)
          as [next_admitted _].
        destruct (par_scan_successor_of_locally_free _ next last_seven
          next_admitted)
          as [next_descriptor [next_resolved [next_above next_tag]]].
        assert (lf_not_next : sem_tag lf <> sem_tag next).
        { rewrite lf_tag. intro same. apply next_tag. symmetry. exact same. }
        assert (head_not_next : sem_tag head <> sem_tag next).
        { intro same.
          rewrite same in head_resolved.
          rewrite next_resolved in head_resolved.
          injection head_resolved as equal.
          subst next_descriptor.
          lia. }
        rewrite (par_order_bad_cons_cons lf next tail).
        rewrite (adjacent_order_bad_distinct_tags lf next lf_not_next).
        rewrite (par_order_bad_cons_cons head next tail).
        rewrite (adjacent_order_bad_distinct_tags head next head_not_next).
        reflexivity.
    + cbn [app] in valid |- *.
      destruct (scan_fold_valid_head _ _ _ _ valid) as [_ tail_valid].
      pose proof (IH _ tail_valid) as IH_instance.
      cbn [app] in IH_instance.
      rewrite (par_order_bad_cons_cons head second (rest' ++ lf :: suffix)).
      rewrite (par_order_bad_cons_cons head second (rest' ++ suffix)).
      rewrite IH_instance.
      reflexivity.
Qed.

(** ** Score evidence ignores the tag-9 child. *)

Lemma find_skip_excluded :
  forall (A : Type) (test : A -> bool) (removed : A),
    test removed = false ->
    forall prefix suffix,
      find test (prefix ++ removed :: suffix) = find test (prefix ++ suffix).
Proof.
  intros A test removed excluded prefix suffix.
  induction prefix as [|head tail IH]; simpl.
  - rewrite excluded. reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma filter_skip_excluded :
  forall (A : Type) (test : A -> bool) (removed : A),
    test removed = false ->
    forall prefix suffix,
      filter test (prefix ++ removed :: suffix) = filter test (prefix ++ suffix).
Proof.
  intros A test removed excluded prefix suffix.
  induction prefix as [|head tail IH]; simpl.
  - rewrite excluded. reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma exact_par_component_scores_skip_scalar :
  forall lf,
    sem_payload lf = Scalar ->
    forall prefix suffix,
      exact_par_component_scores (prefix ++ lf :: suffix) =
      exact_par_component_scores (prefix ++ suffix).
Proof.
  intros lf payload prefix suffix.
  induction prefix as [|child rest IH]; simpl.
  - rewrite payload.
    destruct (exact_par_component_scores suffix) as [scores|]; reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma scalar_true_or_absent_skip :
  forall tag lf,
    sem_tag lf <> tag ->
    forall prefix suffix,
      scalar_true_or_absent tag (prefix ++ lf :: suffix) =
      scalar_true_or_absent tag (prefix ++ suffix).
Proof.
  intros tag lf different prefix suffix.
  unfold scalar_true_or_absent.
  rewrite (find_skip_excluded _ (fun child => sem_tag child =? tag) lf
    (proj2 (Nat.eqb_neq (sem_tag lf) tag) different) prefix suffix).
  reflexivity.
Qed.

Lemma exact_par_score_skip_locally_free :
  forall lf prefix suffix,
    root_locally_free_child lf = true ->
    exact_par_score (prefix ++ lf :: suffix) =
    exact_par_score (prefix ++ suffix).
Proof.
  intros lf prefix suffix is_lf.
  destruct (root_locally_free_child_facts lf is_lf) as [tag [payload _]].
  unfold exact_par_score.
  rewrite (exact_par_component_scores_skip_scalar lf payload prefix suffix).
  rewrite (scalar_true_or_absent_skip 10 lf ltac:(rewrite tag; discriminate)
    prefix suffix).
  reflexivity.
Qed.

(** ** The quotient of the reducer. *)

Theorem root_locally_free_quotient :
  forall field_with field_without prefix suffix lf,
    generated_wire_program field_with = Some par_program ->
    generated_wire_program field_without = Some par_program ->
    valid_raw_body field_with = valid_raw_body field_without ->
    root_locally_free_child lf = true ->
    scan_valid (fold_left (scan_child par_program)
      (prefix ++ lf :: suffix) empty_scan) = true ->
    let w := reduce_semantic_field field_with (prefix ++ lf :: suffix) in
    let o := reduce_semantic_field field_without (prefix ++ suffix) in
    sem_score_evidence w = sem_score_evidence o /\
    sem_complete w = sem_complete o /\
    sem_definitely_refused w = sem_definitely_refused o.
Proof.
  intros field_with field_without prefix suffix lf
    resolved_with resolved_without bodies_agree is_lf valid_scan w o.
  destruct (root_locally_free_child_facts lf is_lf) as [lf_tag _].
  pose proof (scan_valid_after_removal par_program prefix lf suffix valid_scan)
    as valid_without.
  pose proof (par_order_bad_skip_locally_free lf suffix lf_tag prefix empty_scan
    valid_scan) as order_same.
  unfold w, o.
  split; [|split].
  - rewrite (reduce_semantic_field_par_evidence field_with _ resolved_with).
    rewrite (reduce_semantic_field_par_evidence field_without _ resolved_without).
    rewrite (exact_par_score_skip_locally_free lf prefix suffix is_lf).
    reflexivity.
  - rewrite (reduce_semantic_field_par_complete field_with _ resolved_with).
    rewrite (reduce_semantic_field_par_complete field_without _ resolved_without).
    reflexivity.
  - rewrite (reduce_semantic_field_par_refused field_with _ resolved_with).
    rewrite (reduce_semantic_field_par_refused field_without _ resolved_without).
    rewrite valid_scan, valid_without, order_same, bodies_agree.
    reflexivity.
Qed.

(** ** The semantic default of a decoded Par ignores a tag-9 node. *)

Lemma source_par_semantic_default_skips_irrelevant_node :
  forall bytes prefix suffix node,
    RPB3RawRegionTree.source_par_component node = false ->
    RPB3RawRegionTree.source_par_connective_node node = false ->
    RPB3RawRegionTree.source_par_semantic_default bytes (prefix ++ node :: suffix) =
    RPB3RawRegionTree.source_par_semantic_default bytes (prefix ++ suffix).
Proof.
  intros bytes prefix suffix node not_component not_connective.
  unfold RPB3RawRegionTree.source_par_semantic_default,
    RPB3RawRegionTree.source_par_connective_used,
    RPB3RawRegionTree.source_par_component_count.
  rewrite (filter_skip_excluded _ _ node not_component prefix suffix).
  rewrite (find_skip_excluded _ _ node not_connective prefix suffix).
  reflexivity.
Qed.

Lemma source_par_tag_nine_node_is_irrelevant :
  forall node,
    resolve_tag (program_fields par_program) 9 =
      Some (RPB3RawRegionTree.source_field node) ->
    RPB3RawRegionTree.source_par_component node = false /\
    RPB3RawRegionTree.source_par_connective_node node = false.
Proof.
  intros node resolved.
  rewrite par_program_resolves_tag_nine in resolved.
  injection resolved as same.
  unfold RPB3RawRegionTree.source_par_component,
    RPB3RawRegionTree.source_par_connective_node.
  rewrite <- same.
  split; reflexivity.
Qed.

Theorem root_locally_free_quotient_default :
  forall bytes prefix suffix node,
    resolve_tag (program_fields par_program) 9 =
      Some (RPB3RawRegionTree.source_field node) ->
    RPB3RawRegionTree.source_par_semantic_default bytes (prefix ++ node :: suffix) =
    RPB3RawRegionTree.source_par_semantic_default bytes (prefix ++ suffix).
Proof.
  intros bytes prefix suffix node resolved.
  destruct (source_par_tag_nine_node_is_irrelevant node resolved)
    as [not_component not_connective].
  exact (source_par_semantic_default_skips_irrelevant_node bytes prefix suffix
    node not_component not_connective).
Qed.

(** The theorems apply to the actual root field of the reference. *)
Lemma generated_wire_program_par_raw :
  forall body,
    generated_wire_program (RPB3RecursiveAuxiliarySourceReference.par_raw body) =
      Some par_program.
Proof. intro body. exact lookup_projected_par_program. Qed.

(** ** Wire-level witnesses, by [vm_compute] on the recursive reference. *)

Definition constant_hash : list nat -> nat := fun _ => 0.

Definition wire_reference (body : list nat)
    : option (bool * semantic_summary) :=
  RPB3RecursiveAuxiliarySourceReference.recursive_par_reference constant_hash body.

(** The summary the reference returns for an admitted root Par body. *)
Definition par_witness_summary (body : list nat) (score : canonical_score)
    : semantic_summary :=
  SemanticSummary 0 LengthDelimited Message 1 body false None
    (ExactScore score) false false.

Definition gint_seven_score : canonical_score :=
  score_constructor 2 [ScoreInteger 7].

Definition single_gint_score : canonical_score :=
  score_constructor 999 [gint_seven_score; ScoreInteger 0].

Definition double_gint_score : canonical_score :=
  score_constructor 999 [gint_seven_score; gint_seven_score; ScoreInteger 0].

Definition connective_gint_score : canonical_score :=
  score_constructor 999 [gint_seven_score; ScoreInteger 1].

Example witness_empty_root :
  wire_reference [] = Some (true, par_witness_summary [] empty_par_score).
Proof. vm_compute. reflexivity. Qed.

Example witness_locally_free_only_root :
  wire_reference [74; 3; 1; 3; 5] =
    Some (true, par_witness_summary [74; 3; 1; 3; 5] empty_par_score).
Proof. vm_compute. reflexivity. Qed.

Example witness_single_expr :
  wire_reference [42; 2; 16; 14] =
    Some (false, par_witness_summary [42; 2; 16; 14] single_gint_score).
Proof. vm_compute. reflexivity. Qed.

Example witness_single_expr_locally_free :
  wire_reference [42; 2; 16; 14; 74; 3; 2; 4; 6] =
    Some (false,
      par_witness_summary [42; 2; 16; 14; 74; 3; 2; 4; 6] single_gint_score).
Proof. vm_compute. reflexivity. Qed.

Example witness_double_expr :
  wire_reference [42; 2; 16; 14; 42; 2; 16; 14] =
    Some (false,
      par_witness_summary [42; 2; 16; 14; 42; 2; 16; 14] double_gint_score).
Proof. vm_compute. reflexivity. Qed.

Example witness_single_expr_connective :
  wire_reference [42; 2; 16; 14; 80; 1] =
    Some (false,
      par_witness_summary [42; 2; 16; 14; 80; 1] connective_gint_score).
Proof. vm_compute. reflexivity. Qed.

(** Two admitted root Par bodies that agree on score evidence, completeness,
    refusal and semantic default, yet differ in the raw body. *)
Definition same_quotient_distinct_body (first_body second_body : list nat)
    : Prop :=
  exists default_bit first_summary second_summary,
    wire_reference first_body = Some (default_bit, first_summary) /\
    wire_reference second_body = Some (default_bit, second_summary) /\
    sem_score_evidence first_summary = sem_score_evidence second_summary /\
    sem_complete first_summary = sem_complete second_summary /\
    sem_definitely_refused first_summary =
      sem_definitely_refused second_summary /\
    sem_body first_summary <> sem_body second_summary.

Example root_locally_free_empty_pair :
  same_quotient_distinct_body [] [74; 3; 1; 3; 5].
Proof.
  exists true, (par_witness_summary [] empty_par_score),
    (par_witness_summary [74; 3; 1; 3; 5] empty_par_score).
  split; [exact witness_empty_root|].
  split; [exact witness_locally_free_only_root|].
  split; [reflexivity|].
  split; [reflexivity|].
  split; [reflexivity|].
  discriminate.
Qed.

Example root_locally_free_single_expr_pair :
  same_quotient_distinct_body [42; 2; 16; 14] [42; 2; 16; 14; 74; 3; 2; 4; 6].
Proof.
  exists false, (par_witness_summary [42; 2; 16; 14] single_gint_score),
    (par_witness_summary [42; 2; 16; 14; 74; 3; 2; 4; 6] single_gint_score).
  split; [exact witness_single_expr|].
  split; [exact witness_single_expr_locally_free|].
  split; [reflexivity|].
  split; [reflexivity|].
  split; [reflexivity|].
  discriminate.
Qed.

(** Two admitted root Par bodies whose score evidence differs. *)
Definition wire_scores_differ (first_body second_body : list nat) : Prop :=
  exists first_bit second_bit first_summary second_summary,
    wire_reference first_body = Some (first_bit, first_summary) /\
    wire_reference second_body = Some (second_bit, second_summary) /\
    sem_score_evidence first_summary <> sem_score_evidence second_summary.

Example multiplicity_separates_scores :
  wire_scores_differ [42; 2; 16; 14] [42; 2; 16; 14; 42; 2; 16; 14].
Proof.
  exists false, false, (par_witness_summary [42; 2; 16; 14] single_gint_score),
    (par_witness_summary [42; 2; 16; 14; 42; 2; 16; 14] double_gint_score).
  split; [exact witness_single_expr|].
  split; [exact witness_double_expr|].
  intro same.
  vm_compute in same.
  discriminate same.
Qed.

Example connective_separates_scores :
  wire_scores_differ [42; 2; 16; 14] [42; 2; 16; 14; 80; 1].
Proof.
  exists false, false, (par_witness_summary [42; 2; 16; 14] single_gint_score),
    (par_witness_summary [42; 2; 16; 14; 80; 1] connective_gint_score).
  split; [exact witness_single_expr|].
  split; [exact witness_single_expr_connective|].
  intro same.
  vm_compute in same.
  discriminate same.
Qed.

(** ** Mutation controls of the quotient (decision HD-4). *)

(** The children of a witness body, computed by the actual source reference.
    No witness below has an auxiliary map value, so the [par_child] oracle is
    never consulted; [witness_root_children_agree_with_reference] confirms that
    the actual reduction of these children is the reference's summary. *)
Definition witness_root_children (body : list nat)
    : option (list semantic_summary) :=
  match lookup_projected_program 1 [] [],
      RPB3RawRegionTree.decode_root 1 body with
  | Some program, Some nodes =>
      option_map
        (map RPB3RecursiveAuxiliarySourceReference.recursive_result_semantic)
        (RPB3RecursiveAuxiliarySourceReference.recursive_source_children
          (fun _ => None) constant_hash body program nodes)
  | _, _ => None
  end.

Definition wire_reduction
    (reduction : raw_field -> list semantic_summary -> semantic_summary)
    (body : list nat) : option semantic_summary :=
  option_map
    (reduction (RPB3RecursiveAuxiliarySourceReference.par_raw body))
    (witness_root_children body).

Example witness_root_children_agree_with_reference :
  option_map snd (wire_reference []) =
    wire_reduction reduce_semantic_field [] /\
  option_map snd (wire_reference [74; 3; 1; 3; 5]) =
    wire_reduction reduce_semantic_field [74; 3; 1; 3; 5] /\
  option_map snd (wire_reference [42; 2; 16; 14]) =
    wire_reduction reduce_semantic_field [42; 2; 16; 14] /\
  option_map snd (wire_reference [42; 2; 16; 14; 74; 3; 2; 4; 6]) =
    wire_reduction reduce_semantic_field [42; 2; 16; 14; 74; 3; 2; 4; 6].
Proof. vm_compute. repeat split; reflexivity. Qed.

(** Control 1 (under-quotient): a Par score that also absorbs the root tag-9
    bytes.  It differs from [exact_par_score] only in that the appended
    [ScoreBytes] carry the root [locally_free] payload, so on the first two
    witness pairs the actual score agrees and the mutant separates them. *)
Definition under_quotient_par_score (children : list semantic_summary)
    : option canonical_score :=
  match exact_par_component_scores children,
      scalar_true_or_absent 10 children with
  | Some scores, Some connective_used =>
      Some (score_constructor 999
        (scores ++ [ScoreInteger (if connective_used then 1 else 0)] ++
          map (fun child => ScoreBytes (sem_body child))
            (filter (fun child => sem_tag child =? 9) children)))
  | _, _ => None
  end.

Definition under_quotient_wire_score (body : list nat)
    : option canonical_score :=
  match witness_root_children body with
  | Some children => under_quotient_par_score children
  | None => None
  end.

Example under_quotient_separates_empty_pair :
  option_map exact_par_score (witness_root_children []) =
    option_map exact_par_score (witness_root_children [74; 3; 1; 3; 5]) /\
  under_quotient_wire_score [] <> under_quotient_wire_score [74; 3; 1; 3; 5].
Proof.
  vm_compute. split; [reflexivity|intro same; discriminate same].
Qed.

Example under_quotient_separates_single_expr_pair :
  option_map exact_par_score (witness_root_children [42; 2; 16; 14]) =
    option_map exact_par_score
      (witness_root_children [42; 2; 16; 14; 74; 3; 2; 4; 6]) /\
  under_quotient_wire_score [42; 2; 16; 14] <>
    under_quotient_wire_score [42; 2; 16; 14; 74; 3; 2; 4; 6].
Proof.
  vm_compute. split; [reflexivity|intro same; discriminate same].
Qed.

(** Control 2 (over-quotient, HD-4): a reduction that erases the whole encoded
    root tag-9 field from [sem_body].  It identifies summaries that RPO-13
    requires to differ: dropping root [locally_free] preserves score equality
    but must change the raw body.  [erased_root_body] removes the byte range of
    the root tag-9 field reported by the decoder. *)
Definition erased_root_body (body : list nat) : list nat :=
  match RPB3RawRegionTree.decode_root 1 body with
  | Some nodes =>
      match find (fun node =>
          descriptor_tag (RPB3RawRegionTree.source_field node) =? 9) nodes with
      | Some node =>
          firstn (RPB3WireCursor.span_start
            (RPB3RawRegionTree.source_span node)) body ++
          skipn (RPB3WireCursor.span_end
            (RPB3RawRegionTree.source_span node)) body
      | None => body
      end
  | None => body
  end.

Definition over_quotient_reduction (field : raw_field)
    (children : list semantic_summary) : semantic_summary :=
  let reduced := reduce_semantic_field field children in
  SemanticSummary (sem_tag reduced) (sem_wire reduced) (sem_payload reduced)
    (sem_type_id reduced) (erased_root_body (sem_body reduced))
    (sem_definitely_refused reduced) (sem_complete reduced)
    (sem_score_evidence reduced) (sem_bind_source_present reduced)
    (sem_case_required_present reduced).

Example over_quotient_identifies_empty_pair :
  wire_reduction over_quotient_reduction [] =
    wire_reduction over_quotient_reduction [74; 3; 1; 3; 5] /\
  wire_reduction reduce_semantic_field [] <>
    wire_reduction reduce_semantic_field [74; 3; 1; 3; 5].
Proof.
  vm_compute. split; [reflexivity|intro same; discriminate same].
Qed.

Example over_quotient_identifies_single_expr_pair :
  wire_reduction over_quotient_reduction [42; 2; 16; 14] =
    wire_reduction over_quotient_reduction [42; 2; 16; 14; 74; 3; 2; 4; 6] /\
  wire_reduction reduce_semantic_field [42; 2; 16; 14] <>
    wire_reduction reduce_semantic_field [42; 2; 16; 14; 74; 3; 2; 4; 6].
Proof.
  vm_compute. split; [reflexivity|intro same; discriminate same].
Qed.

Print Assumptions lookup_projected_par_program.
Print Assumptions par_program_resolves_tag_nine.
Print Assumptions generated_wire_program_par_is_message.
Print Assumptions root_locally_free_child_facts.
Print Assumptions reduce_semantic_field_par_evidence.
Print Assumptions reduce_semantic_field_par_complete.
Print Assumptions par_refusal_boolean_shape.
Print Assumptions reduce_semantic_field_par_refused.
Print Assumptions scan_child_resolved.
Print Assumptions scan_child_on_invalid.
Print Assumptions scan_child_admitted.
Print Assumptions scan_fold_on_invalid.
Print Assumptions scan_fold_valid_head.
Print Assumptions slot_follows_monotone.
Print Assumptions scan_admissible_transfers.
Print Assumptions scan_related_step.
Print Assumptions scan_related_after_removed_child.
Print Assumptions scan_related_fold.
Print Assumptions scan_valid_after_removal.
Print Assumptions par_scan_admitted_locally_free.
Print Assumptions par_scan_successor_of_locally_free.
Print Assumptions par_scan_predecessor_of_locally_free.
Print Assumptions and_known_bad.
Print Assumptions adjacent_order_bad_distinct_tags.
Print Assumptions par_order_bad_cons_cons.
Print Assumptions par_order_bad_skip_locally_free.
Print Assumptions find_skip_excluded.
Print Assumptions filter_skip_excluded.
Print Assumptions exact_par_component_scores_skip_scalar.
Print Assumptions scalar_true_or_absent_skip.
Print Assumptions exact_par_score_skip_locally_free.
Print Assumptions root_locally_free_quotient.
Print Assumptions source_par_semantic_default_skips_irrelevant_node.
Print Assumptions source_par_tag_nine_node_is_irrelevant.
Print Assumptions root_locally_free_quotient_default.
Print Assumptions generated_wire_program_par_raw.
Print Assumptions witness_empty_root.
Print Assumptions witness_locally_free_only_root.
Print Assumptions witness_single_expr.
Print Assumptions witness_single_expr_locally_free.
Print Assumptions witness_double_expr.
Print Assumptions witness_single_expr_connective.
Print Assumptions root_locally_free_empty_pair.
Print Assumptions root_locally_free_single_expr_pair.
Print Assumptions multiplicity_separates_scores.
Print Assumptions connective_separates_scores.
Print Assumptions witness_root_children_agree_with_reference.
Print Assumptions under_quotient_separates_empty_pair.
Print Assumptions under_quotient_separates_single_expr_pair.
Print Assumptions over_quotient_identifies_empty_pair.
Print Assumptions over_quotient_identifies_single_expr_pair.
