From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import SubstitutionRegionEmission.

Import ListNotations.
Import SubstitutionRegionEmission.SubstitutionRegionEmission.
Set Implicit Arguments.

Module SubstitutionRegionPublication.

Definition update {A : Type} (table : nat -> A) (index : nat) (value : A) : nat -> A :=
  fun position => if position =? index then value else table position.

Lemma update_same : forall A (table : nat -> A) index value,
  update table index value index = value.
Proof. intros; unfold update; now rewrite Nat.eqb_refl. Qed.

Lemma update_other : forall A (table : nat -> A) index value position,
  position <> index -> update table index value position = table position.
Proof.
  intros; unfold update; destruct (position =? index) eqn:E; auto.
  apply Nat.eqb_eq in E; contradiction.
Qed.

Record arena := Arena {
  count : nat;
  programs : nat -> list piece;
  views : nat -> mode -> bytes;
  caches : nat -> mode -> nat;
  weights : nat -> nat;
  sources : nat -> bytes
}.

Definition empty : arena :=
  Arena 0 (fun _ => []) (fun _ _ => []) (fun _ _ => 0) (fun _ => 1) (fun _ => []).

Definition task_rank (a : arena) (t : task) : nat :=
  match t with
  | VisitNode node => weights a node
  | VisitPiece (Child _ node _) => S (weights a node)
  | VisitPiece _ => 1
  end.

Definition piece_rank_sum (a : arena) (ps : list piece) : nat :=
  fold_right (fun p total => task_rank a (VisitPiece p) + total) 0 ps.

Definition backward (owner : nat) (p : piece) : Prop :=
  match p with Child _ child _ => child < owner | _ => True end.

Section Tables.
Variable cuts : nat -> bytes.
Variable prefix : nat -> bytes.
Variable groups : nat -> list nat.

Definition denotation (a : arena) (m : mode) (ps : list piece) : bytes :=
  pieces_bytes (views a) cuts prefix groups m ps.

Definition valid (a : arena) : Prop :=
  (forall node m, views a node m = denotation a m (programs a node)) /\
  (forall node m, caches a node m = length (views a node m)) /\
  (forall node, weights a node = S (piece_rank_sum a (programs a node))) /\
  (forall node, Forall (backward node) (programs a node)) /\
  (forall node, count a <= node -> programs a node = []) /\
  (forall node, sources a node = views a node Original).

Definition append (a : arena) (ps : list piece) (sizes : mode -> nat) (source : bytes) : arena :=
  Arena (S (count a))
    (update (programs a) (count a) ps)
    (update (views a) (count a) (fun m => denotation a m ps))
    (update (caches a) (count a) sizes)
    (update (weights a) (count a) (S (piece_rank_sum a ps)))
    (update (sources a) (count a) source).

Lemma empty_is_valid : valid empty.
Proof.
  unfold valid, empty, denotation, pieces_bytes, piece_rank_sum; simpl.
  repeat split; intros; reflexivity || constructor.
Qed.

Theorem publication_preserves_old_indices : forall a ps sizes source node,
  node < count a ->
  programs (append a ps sizes source) node = programs a node /\
  views (append a ps sizes source) node = views a node /\
  caches (append a ps sizes source) node = caches a node /\
  weights (append a ps sizes source) node = weights a node /\
  sources (append a ps sizes source) node = sources a node.
Proof.
  intros; unfold append; simpl.
  repeat rewrite update_other by lia; repeat split; reflexivity.
Qed.

Theorem publication_gives_exact_new_index : forall a ps sizes source,
  count (append a ps sizes source) = S (count a) /\
  programs (append a ps sizes source) (count a) = ps /\
  views (append a ps sizes source) (count a) = (fun m => denotation a m ps) /\
  caches (append a ps sizes source) (count a) = sizes /\
  weights (append a ps sizes source) (count a) = S (piece_rank_sum a ps) /\
  sources (append a ps sizes source) (count a) = source.
Proof. intros; unfold append; simpl; rewrite !update_same; repeat split; reflexivity. Qed.

Lemma backward_weaken : forall ps before after,
  before <= after -> Forall (backward before) ps -> Forall (backward after) ps.
Proof.
  intros ps before after B H; induction H; constructor; auto.
  destruct x; simpl in *; auto; lia.
Qed.

Lemma piece_denotation_stable : forall a ps sizes source m p,
  backward (count a) p ->
  piece_bytes (views (append a ps sizes source)) cuts prefix groups m p =
  piece_bytes (views a) cuts prefix groups m p.
Proof.
  intros a ps sizes source m p B; destruct p; simpl in B; try reflexivity.
  cbn [piece_bytes append views].
  rewrite update_other by lia; reflexivity.
Qed.

Theorem prior_piece_denotation_stable : forall a ps sizes source m earlier,
  Forall (backward (count a)) earlier ->
  denotation (append a ps sizes source) m earlier = denotation a m earlier.
Proof.
  intros a ps sizes source m earlier H.
  unfold denotation, pieces_bytes.
  induction H; cbn [map concat]; auto; rewrite piece_denotation_stable by assumption.
  now rewrite IHForall.
Qed.

Lemma piece_rank_stable : forall a ps sizes source p,
  backward (count a) p ->
  task_rank (append a ps sizes source) (VisitPiece p) = task_rank a (VisitPiece p).
Proof.
  intros a ps sizes source p B; destruct p; simpl in B; try reflexivity.
  cbn [task_rank append weights]; rewrite update_other by lia; reflexivity.
Qed.

Lemma prior_rank_sum_stable : forall a ps sizes source earlier,
  Forall (backward (count a)) earlier ->
  piece_rank_sum (append a ps sizes source) earlier = piece_rank_sum a earlier.
Proof.
  intros a ps sizes source earlier H; unfold piece_rank_sum.
  induction H; cbn [fold_right]; auto; rewrite piece_rank_stable by assumption.
  now rewrite IHForall.
Qed.

Definition source_piece (a : arena) (p : piece) : bytes :=
  match p with
  | Borrowed body => body
  | Child tag node _ => delimited tag (sources a node)
  | Par tag cut => delimited tag (cuts cut)
  | EPathMap tag group => delimited tag (group_body cuts prefix groups Original group)
  end.

Definition field_certificate (a : arena) (fields : list (piece * bytes)) : Prop :=
  Forall (fun field => snd field = source_piece a (fst field)) fields.

Definition frame_source (fields : list (piece * bytes)) : bytes :=
  concat (map snd fields).

Theorem certified_fields_reconstruct_original : forall a fields,
  valid a -> field_certificate a fields ->
  denotation a Original (map fst fields) = frame_source fields.
Proof.
  intros a fields V H; destruct V as (_ & _ & _ & _ & _ & S).
  unfold field_certificate in H; unfold denotation, pieces_bytes, frame_source.
  rewrite map_map.
  induction H as [|[p body] rest Field Rest IH]; simpl; auto.
  rewrite <- IH; f_equal.
  cbn [fst snd] in Field.
  rewrite Field; destruct p; cbn [source_piece piece_bytes keep_child]; auto.
  now rewrite S.
Qed.

Theorem append_establishes_all_arena_invariants : forall a ps sizes source,
  valid a ->
  Forall (backward (count a)) ps ->
  (forall m, sizes m = length (denotation a m ps)) ->
  source = denotation a Original ps ->
  valid (append a ps sizes source).
Proof.
  intros a ps sizes source V B Size Src.
  destruct V as (Ref & Cache & Rank & Back & Empty & Source).
  assert (Old : forall node, node <> count a ->
    Forall (backward (count a)) (programs a node)).
  { intros node N.
    destruct (lt_dec node (count a)) as [L|L].
    - eapply backward_weaken with (before := node); [lia|apply Back].
    - rewrite Empty by lia; constructor. }
  unfold valid; repeat split.
  - intros node m.
    destruct (Nat.eq_dec node (count a)) as [->|N].
    + cbn [append views programs]; rewrite !update_same.
      symmetry; apply prior_piece_denotation_stable; exact B.
    + cbn [append views programs]; rewrite !update_other by exact N.
      rewrite Ref; symmetry; apply prior_piece_denotation_stable; apply Old; exact N.
  - intros node m; cbn [append caches views].
    destruct (Nat.eq_dec node (count a)) as [->|N].
    + rewrite !update_same; apply Size.
    + rewrite !update_other by exact N; apply Cache.
  - intros node.
    destruct (Nat.eq_dec node (count a)) as [->|N].
    + cbn [append weights programs]; rewrite !update_same.
      rewrite prior_rank_sum_stable by exact B; reflexivity.
    + cbn [append weights programs]; rewrite !update_other by exact N.
      rewrite prior_rank_sum_stable by (apply Old; exact N); apply Rank.
  - intro node; cbn [append programs].
    destruct (Nat.eq_dec node (count a)) as [->|N].
    + rewrite update_same; exact B.
    + rewrite update_other by exact N; apply Back.
  - intros node H; cbn [append count programs] in *.
    rewrite update_other by lia; apply Empty; lia.
  - intro node; cbn [append sources views].
    destruct (Nat.eq_dec node (count a)) as [->|N].
    + rewrite !update_same; exact Src.
    + rewrite !update_other by exact N; apply Source.
Qed.

Variable limit word64 : nat.

Definition publish (a : arena) (ps : list piece) (source : bytes) : option arena :=
  let plan := planned_node_size cuts prefix groups (caches a) limit word64 in
  match plan Compact 0 ps, plan Shallow 0 ps, plan Original 0 ps with
  | Some compact, Some shallow, Some original =>
      if original =? length source then
        Some (append a ps (fun m => match m with
          | Compact => compact | Shallow => shallow | Original => original end) source)
      else None
  | _, _, _ => None
  end.

Theorem successful_checked_publication_establishes_invariants :
  forall a fields next,
  valid a ->
  Forall (backward (count a)) (map fst fields) ->
  field_certificate a fields ->
  publish a (map fst fields) (frame_source fields) = Some next ->
  valid next.
Proof.
  intros a fields next V B C H.
  assert (Cache : forall node m, caches a node m = length (views a node m)).
  { destruct V as (_ & Cache & _); exact Cache. }
  assert (Exact : forall m result,
    planned_node_size cuts prefix groups (caches a) limit word64 m 0 (map fst fields) = Some result ->
    result = length (denotation a m (map fst fields))).
  { intros m result E.
    exact (@finished_node_success_has_exact_mode_length (views a) cuts prefix groups
      (caches a) Cache limit word64 (map fst fields) m 0 result E). }
  unfold publish in H.
  destruct (planned_node_size cuts prefix groups (caches a) limit word64 Compact 0 (map fst fields)) eqn:PC; [|discriminate].
  destruct (planned_node_size cuts prefix groups (caches a) limit word64 Shallow 0 (map fst fields)) eqn:PS; [|discriminate].
  destruct (planned_node_size cuts prefix groups (caches a) limit word64 Original 0 (map fst fields)) eqn:PO; [|discriminate].
  destruct (_ =? _) eqn:L; inversion H; subst; clear H.
  apply append_establishes_all_arena_invariants; auto.
  - intro m; destruct m; eapply Exact; eauto.
  - symmetry; eapply certified_fields_reconstruct_original; eauto.
Qed.

Theorem publication_checks_original_length : forall a ps source next,
  publish a ps source = Some next ->
  caches next (count a) Original = length source.
Proof.
  intros a ps source next H; unfold publish in H.
  repeat match type of H with
  | context[planned_node_size ?c ?p ?g ?v ?l ?w ?m ?t ?ps] =>
    destruct (planned_node_size c p g v l w m t ps)
  end; try discriminate.
  destruct (_ =? _) eqn:L; inversion H; subst.
  apply Nat.eqb_eq in L; cbn [append caches]; rewrite update_same; exact L.
Qed.

Theorem length_mismatch_refuses_publication : forall a ps source original,
  planned_node_size cuts prefix groups (caches a) limit word64 Original 0 ps = Some original ->
  original <> length source -> publish a ps source = None.
Proof.
  intros a ps source original H N; unfold publish; rewrite H.
  destruct (planned_node_size _ _ _ _ _ _ Compact _ _); auto.
  destruct (planned_node_size _ _ _ _ _ _ Shallow _ _); auto.
  assert (E : (original =? length source) = false) by (apply Nat.eqb_neq; exact N).
  now rewrite E.
Qed.

Theorem arithmetic_failure_refuses_publication : forall a ps source m,
  planned_node_size cuts prefix groups (caches a) limit word64 m 0 ps = None ->
  publish a ps source = None.
Proof.
  intros a ps source m H; unfold publish; destruct m; rewrite H;
  repeat match goal with
  | |- context[planned_node_size ?c ?p ?g ?v ?l ?w ?m ?t ?ps] =>
    destruct (planned_node_size c p g v l w m t ps)
  end; reflexivity.
Qed.

Theorem valid_arena_supplies_node_rank : forall a,
  valid a -> forall node,
  task_rank a (VisitNode node) =
  S (fold_right (fun t total => task_rank a t + total) 0
     (map VisitPiece (programs a node))).
Proof.
  intros a (_ & _ & Rank & _) node.
  cbn [task_rank]; rewrite Rank; unfold piece_rank_sum.
  f_equal; induction (programs a node); cbn [map fold_right]; congruence.
Qed.

Theorem valid_arena_supplies_piece_rank : forall a m p,
  task_rank a (VisitPiece p) >
  fold_right (fun t total => task_rank a t + total) 0
    (snd (expand_piece (views a) cuts prefix groups m p)).
Proof.
  intros a m p; destruct p; cbn [task_rank expand_piece snd fold_right]; try lia.
  destruct (keep_child _ _ _ _); cbn [snd fold_right task_rank]; lia.
Qed.

Theorem valid_published_root_emits_its_original_input : forall a root,
  valid a ->
  root < count a ->
  exists result,
    execution (programs a) (views a) cuts prefix groups Original
      [VisitNode root] [] [] result /\ result = sources a root.
Proof.
  intros a root V PublishedRoot.
  assert (N := valid_arena_supplies_node_rank V).
  assert (P := valid_arena_supplies_piece_rank a).
  destruct (@ranked_emission_terminates_without_fuel (programs a) (views a)
    cuts prefix groups (task_rank a) N P Original [VisitNode root] [])
    as [result Run].
  exists result; split; [exact Run|].
  destruct V as (Ref & _ & _ & _ & _ & Source).
  rewrite Source; eapply worklist_emission_equals_mode_reference; eauto.
Qed.

Inductive constructed : arena -> Prop :=
| Initial : constructed empty
| Published : forall a fields actual_frame next,
    constructed a ->
    Forall (backward (count a)) (map fst fields) ->
    field_certificate a fields ->
    actual_frame = frame_source fields ->
    publish a (map fst fields) actual_frame = Some next ->
    constructed next.

Theorem constructed_arena_is_valid : forall a,
  constructed a -> valid a.
Proof.
  intros a H; induction H.
  - apply empty_is_valid.
  - subst actual_frame; eapply successful_checked_publication_establishes_invariants; eauto.
Qed.

Theorem publication_increases_count_once : forall a ps source next,
  publish a ps source = Some next -> count next = S (count a).
Proof.
  intros a ps source next H; unfold publish in H.
  repeat match type of H with
  | context[planned_node_size ?c ?p ?g ?v ?l ?w ?m ?t ?ps] =>
    destruct (planned_node_size c p g v l w m t ps)
  end; try discriminate.
  destruct (_ =? _) eqn:L; inversion H; reflexivity.
Qed.

Theorem constructed_root_emits_all_modes_with_exact_cached_length : forall a root m,
  constructed a -> root < count a ->
  exists result,
    execution (programs a) (views a) cuts prefix groups m [VisitNode root] [] [] result /\
    result = views a root m /\ length result = caches a root m.
Proof.
  intros a root m Built PublishedRoot.
  pose proof (constructed_arena_is_valid Built) as V.
  assert (N := valid_arena_supplies_node_rank V).
  assert (P := valid_arena_supplies_piece_rank a).
  destruct (@ranked_emission_terminates_without_fuel (programs a) (views a)
    cuts prefix groups (task_rank a) N P m [VisitNode root] [])
    as [result Run].
  destruct V as (Ref & Cache & _).
  assert (E : result = views a root m).
  { eapply worklist_emission_equals_mode_reference; eauto. }
  exists result; split; [exact Run|]; split; [exact E|].
  rewrite E; symmetry; apply Cache.
Qed.

Theorem constructed_published_root_reconstructs_certified_source : forall a root,
  constructed a -> root < count a ->
  exists result,
    execution (programs a) (views a) cuts prefix groups Original
      [VisitNode root] [] [] result /\ result = sources a root.
Proof.
  intros a root Built Bound.
  apply valid_published_root_emits_its_original_input; auto.
  now apply constructed_arena_is_valid.
Qed.


End Tables.

Print Assumptions empty_is_valid.
Print Assumptions publication_preserves_old_indices.
Print Assumptions publication_gives_exact_new_index.
Print Assumptions prior_piece_denotation_stable.
Print Assumptions certified_fields_reconstruct_original.
Print Assumptions append_establishes_all_arena_invariants.
Print Assumptions successful_checked_publication_establishes_invariants.
Print Assumptions publication_checks_original_length.
Print Assumptions length_mismatch_refuses_publication.
Print Assumptions arithmetic_failure_refuses_publication.
Print Assumptions valid_arena_supplies_node_rank.
Print Assumptions valid_arena_supplies_piece_rank.
Print Assumptions valid_published_root_emits_its_original_input.

Print Assumptions constructed_arena_is_valid.
Print Assumptions publication_increases_count_once.
Print Assumptions constructed_root_emits_all_modes_with_exact_cached_length.
Print Assumptions constructed_published_root_reconstructs_certified_source.

Definition no_cuts (_ : nat) : bytes := [].
Definition no_prefix (_ : nat) : bytes := [].
Definition no_groups (_ : nat) : list nat := [].
Definition first_fixture := append no_cuts no_prefix no_groups empty
  [Borrowed [8; 1]] (fun m => match m with Compact => 2 | Shallow => 2 | Original => 2 end) [8; 1].
Definition second_fixture := append no_cuts no_prefix no_groups first_fixture
  [Child 1 0 true] (fun m => match m with Compact => 4 | Shallow => 4 | Original => 4 end) [10; 2; 8; 1].

Example two_node_publication_is_constructed :
  constructed no_cuts no_prefix no_groups 255 255 second_fixture.
Proof.
  eapply Published with (a := first_fixture)
    (fields := [(Child 1 0 true, [10; 2; 8; 1])])
    (actual_frame := [10; 2; 8; 1]).
  - eapply Published with (a := empty)
      (fields := [(Borrowed [8; 1], [8; 1])]) (actual_frame := [8; 1]).
    + constructor.
    + repeat constructor.
    + repeat constructor.
    + reflexivity.
    + reflexivity.
  - constructor; [change (0 < 1); lia|constructor].
  - repeat constructor.
  - reflexivity.
  - reflexivity.
Qed.

Example same_length_wrong_source_passes_only_the_length_check :
  exists next,
    publish no_cuts no_prefix no_groups 255 255 empty [Borrowed [1]] [2] = Some next /\
    sources next 0 <> views next 0 Original.
Proof.
  eexists; split; [reflexivity|]; discriminate.
Qed.

Example self_reference_is_not_an_admissible_publication :
  ~ Forall (backward (count empty)) [Child 1 0 true].
Proof.
  intro H; inversion H; subst; cbn [backward empty count] in *; lia.
Qed.

Example unrepresentable_publication_is_refused :
  publish no_cuts no_prefix no_groups 1 255 empty [Borrowed [1; 2]] [1; 2] = None.
Proof. reflexivity. Qed.

Print Assumptions two_node_publication_is_constructed.
Print Assumptions same_length_wrong_source_passes_only_the_length_check.
Print Assumptions self_reference_is_not_an_admissible_publication.
Print Assumptions unrepresentable_publication_is_refused.


End SubstitutionRegionPublication.
