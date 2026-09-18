From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import SubstitutionRegionEmission SubstitutionRegionPublication.

Import ListNotations.
Import SubstitutionRegionEmission.SubstitutionRegionEmission.
Import SubstitutionRegionPublication.SubstitutionRegionPublication.
Set Implicit Arguments.

Module SubstitutionRegionTables.

Record group := Group { retained_prefix : bytes; first_cut : nat; cut_count : nat }.
Record tables := Tables { cut_table : list bytes; group_table : list group }.

Definition cut_bytes (t : tables) (i : nat) := nth i (cut_table t) [].
Definition group_at (t : tables) (i : nat) :=
  nth i (group_table t) (Group [] 0 0).
Definition prefixes (t : tables) (i : nat) := retained_prefix (group_at t i).
Definition indices (t : tables) (i : nat) :=
  seq (first_cut (group_at t i)) (cut_count (group_at t i)).
Definition well_formed (t : tables) : Prop :=
  Forall (fun g => first_cut g + cut_count g <= length (cut_table t)) (group_table t).

Definition extends (before after : tables) : Prop :=
  (exists extra, cut_table after = cut_table before ++ extra) /\
  (exists extra, group_table after = group_table before ++ extra).

Definition supported (t : tables) (p : piece) : Prop :=
  match p with
  | Par _ cut => cut < length (cut_table t)
  | EPathMap _ g => g < length (group_table t)
  | _ => True
  end.

Definition register_cut (t : tables) (body : bytes) : tables :=
  Tables (cut_table t ++ [body]) (group_table t).

Definition register_group (t : tables) (prefix : bytes) (values : list bytes) : tables :=
  Tables (cut_table t ++ values)
    (group_table t ++ [Group prefix (length (cut_table t)) (length values)]).

Lemma append_lookup : forall A (before extra : list A) i fallback,
  i < length before -> nth i (before ++ extra) fallback = nth i before fallback.
Proof. intros; apply app_nth1; assumption. Qed.

Theorem extension_preserves_cut_lookup : forall before after i,
  extends before after -> i < length (cut_table before) ->
  cut_bytes after i = cut_bytes before i.
Proof.
  intros before after i [[extra Equal] _] Bound.
  unfold cut_bytes; rewrite Equal; apply append_lookup; exact Bound.
Qed.

Theorem extension_preserves_group_lookup : forall before after i,
  extends before after -> i < length (group_table before) ->
  group_at after i = group_at before i.
Proof.
  intros before after i [_ [extra Equal]] Bound.
  unfold group_at; rewrite Equal; apply append_lookup; exact Bound.
Qed.

Theorem extension_preserves_support : forall before after p,
  extends before after -> supported before p -> supported after p.
Proof.
  intros before after p [[cs C] [gs G]] S.
  destruct p; cbn [supported] in *; auto; rewrite ?C, ?G, length_app; lia.
Qed.

Lemma group_range_bounded : forall t g i,
  well_formed t -> g < length (group_table t) ->
  In i (indices t g) -> i < length (cut_table t).
Proof.
  intros t g i W G I.
  unfold indices in I; apply in_seq in I.
  unfold well_formed in W.
  apply Forall_forall with (x := group_at t g) in W.
  - lia.
  - unfold group_at; apply nth_In; exact G.
Qed.

Theorem extension_preserves_group_body : forall before after m g,
  well_formed before -> extends before after ->
  g < length (group_table before) ->
  group_body (cut_bytes after) (prefixes after) (indices after) m g =
  group_body (cut_bytes before) (prefixes before) (indices before) m g.
Proof.
  intros before after m g W E G.
  unfold group_body, prefixes, indices.
  rewrite (extension_preserves_group_lookup E G).
  f_equal; f_equal; apply map_ext_in; intros i I.
  rewrite (extension_preserves_cut_lookup E); auto.
  apply group_range_bounded with (g := g); assumption.
Qed.

Theorem extension_preserves_piece_bytes : forall before after reference m p,
  well_formed before -> extends before after -> supported before p ->
  piece_bytes reference (cut_bytes after) (prefixes after) (indices after) m p =
  piece_bytes reference (cut_bytes before) (prefixes before) (indices before) m p.
Proof.
  intros before after reference m p W E S.
  destruct p; cbn [supported] in S; try reflexivity.
  - destruct m; cbn [piece_bytes]; try reflexivity.
    now rewrite (extension_preserves_cut_lookup E S).
  - cbn [piece_bytes]; now rewrite (extension_preserves_group_body m W E S).
Qed.

Theorem extension_preserves_source_piece : forall before after a p,
  well_formed before -> extends before after -> supported before p ->
  source_piece (cut_bytes after) (prefixes after) (indices after) a p =
  source_piece (cut_bytes before) (prefixes before) (indices before) a p.
Proof.
  intros before after a p W E S.
  destruct p; cbn [supported] in S; try reflexivity.
  - cbn [source_piece]; now rewrite (extension_preserves_cut_lookup E S).
  - cbn [source_piece]; now rewrite (extension_preserves_group_body Original W E S).
Qed.

Theorem extension_preserves_field_certificates : forall before after a fields,
  well_formed before -> extends before after ->
  Forall (fun field => supported before (fst field)) fields ->
  field_certificate (cut_bytes before) (prefixes before) (indices before) a fields ->
  field_certificate (cut_bytes after) (prefixes after) (indices after) a fields.
Proof.
  intros before after a fields W E S C.
  unfold field_certificate in *.
  apply Forall_forall; intros field I.
  apply Forall_forall with (x := field) in S; auto.
  apply Forall_forall with (x := field) in C; auto.
  rewrite (@extension_preserves_source_piece before after a (fst field) W E S); exact C.
Qed.

Theorem extension_preserves_arena_validity : forall before after a,
  well_formed before -> extends before after ->
  (forall node, Forall (supported before) (programs a node)) ->
  valid (cut_bytes before) (prefixes before) (indices before) a ->
  valid (cut_bytes after) (prefixes after) (indices after) a.
Proof.
  intros before after a W E S [Ref Rest].
  split; [|exact Rest].
  intros node m; rewrite Ref; unfold denotation, pieces_bytes.
  f_equal; apply map_ext_in; intros p I.
  symmetry; apply extension_preserves_piece_bytes; auto.
  pose proof (S node) as Supported.
  apply Forall_forall with (x := p) in Supported; auto.
Qed.

Theorem cut_registration_extends : forall t body,
  extends t (register_cut t body).
Proof.
  intros; split; [exists [body]; reflexivity|exists []; cbn [register_cut group_table]; now rewrite app_nil_r].
Qed.

Theorem cut_registration_preserves_well_formed : forall t body,
  well_formed t -> well_formed (register_cut t body).
Proof.
  intros t body W; unfold well_formed in *; cbn [register_cut cut_table group_table].
  apply Forall_forall; intros g I.
  apply Forall_forall with (x := g) in W; auto.
  rewrite length_app; cbn; lia.
Qed.

Theorem cut_registration_has_exact_new_ordinal : forall t body,
  cut_bytes (register_cut t body) (length (cut_table t)) = body.
Proof.
  intros; unfold cut_bytes; cbn [register_cut cut_table].
  rewrite app_nth2 by lia; now rewrite Nat.sub_diag.
Qed.

Theorem group_registration_extends : forall t prefix values,
  extends t (register_group t prefix values).
Proof. intros; split; eexists; reflexivity. Qed.

Theorem group_registration_preserves_well_formed : forall t prefix values,
  well_formed t -> well_formed (register_group t prefix values).
Proof.
  intros t prefix values W.
  unfold well_formed in *; cbn [register_group group_table cut_table].
  apply Forall_app; split.
  - apply Forall_forall; intros g I.
    apply Forall_forall with (x := g) in W; auto.
    rewrite length_app; lia.
  - constructor; [cbn; rewrite length_app; lia|constructor].
Qed.

Theorem group_registration_has_exact_descriptor : forall t prefix values,
  group_at (register_group t prefix values) (length (group_table t)) =
  Group prefix (length (cut_table t)) (length values).
Proof.
  intros; unfold group_at; cbn [register_group group_table].
  rewrite app_nth2 by lia; now rewrite Nat.sub_diag.
Qed.

Lemma suffix_indices_select_values : forall (prefix values : list bytes),
  map (fun i => nth i (prefix ++ values) []) (seq (length prefix) (length values)) = values.
Proof.
  intros prefix values.
  apply nth_ext with (d := []) (d' := []).
  - rewrite length_map, length_seq; reflexivity.
  - intros n Bound.
    rewrite length_map, length_seq in Bound.
    rewrite <- nth_nth_nth_map with (dn := 0) by
      (left; rewrite length_seq; exact Bound).
    rewrite seq_nth by exact Bound.
    rewrite app_nth2 by lia.
    replace (length prefix + n - length prefix) with n by lia; reflexivity.
Qed.

Theorem group_registration_preserves_exact_value_order : forall t prefix values,
  map (cut_bytes (register_group t prefix values))
    (indices (register_group t prefix values) (length (group_table t))) = values.
Proof.
  intros; unfold indices; rewrite group_registration_has_exact_descriptor.
  cbn [first_cut cut_count]; unfold cut_bytes; cbn [register_group cut_table].
  apply suffix_indices_select_values.
Qed.

Theorem group_registration_reconstructs_original_payload : forall t prefix values,
  group_body (cut_bytes (register_group t prefix values))
    (prefixes (register_group t prefix values))
    (indices (register_group t prefix values)) Original (length (group_table t)) =
  prefix ++ concat (map (value_frame Original) values).
Proof.
  intros; unfold group_body.
  rewrite <- map_map.
  rewrite group_registration_preserves_exact_value_order.
  unfold prefixes; rewrite group_registration_has_exact_descriptor; reflexivity.
Qed.

Theorem registration_establishes_piece_support : forall t tag prefix values body,
  supported (register_cut t body) (Par tag (length (cut_table t))) /\
  supported (register_group t prefix values) (EPathMap tag (length (group_table t))).
Proof.
  intros; cbn [supported register_cut register_group cut_table group_table].
  rewrite !length_app; cbn; lia.
Qed.

Theorem cut_registration_establishes_field_certificate : forall t a tag body,
  field_certificate
    (cut_bytes (register_cut t body)) (prefixes (register_cut t body))
    (indices (register_cut t body)) a
    [(Par tag (length (cut_table t)), delimited tag body)].
Proof.
  intros; constructor; [|constructor].
  cbn [fst snd source_piece]; now rewrite cut_registration_has_exact_new_ordinal.
Qed.

Theorem group_registration_establishes_field_certificate : forall t a tag prefix values,
  field_certificate
    (cut_bytes (register_group t prefix values)) (prefixes (register_group t prefix values))
    (indices (register_group t prefix values)) a
    [(EPathMap tag (length (group_table t)),
      delimited tag (prefix ++ concat (map (value_frame Original) values)))].
Proof.
  intros; constructor; [|constructor].
  cbn [fst snd source_piece]; now rewrite group_registration_reconstructs_original_payload.
Qed.

Print Assumptions extension_preserves_cut_lookup.
Print Assumptions extension_preserves_group_lookup.
Print Assumptions extension_preserves_support.
Print Assumptions group_range_bounded.
Print Assumptions extension_preserves_group_body.
Print Assumptions extension_preserves_piece_bytes.
Print Assumptions extension_preserves_source_piece.
Print Assumptions extension_preserves_field_certificates.
Print Assumptions extension_preserves_arena_validity.
Print Assumptions cut_registration_extends.
Print Assumptions cut_registration_preserves_well_formed.
Print Assumptions cut_registration_has_exact_new_ordinal.
Print Assumptions group_registration_extends.
Print Assumptions group_registration_preserves_well_formed.
Print Assumptions group_registration_has_exact_descriptor.
Print Assumptions suffix_indices_select_values.
Print Assumptions group_registration_preserves_exact_value_order.
Print Assumptions group_registration_reconstructs_original_payload.
Print Assumptions registration_establishes_piece_support.
Print Assumptions cut_registration_establishes_field_certificate.
Print Assumptions group_registration_establishes_field_certificate.

Theorem empty_tables_are_well_formed : well_formed (Tables [] []).
Proof. constructor. Qed.

Theorem sequential_cuts_equal_batch : forall values t,
  fold_left register_cut values t =
  Tables (cut_table t ++ values) (group_table t).
Proof.
  induction values as [|body rest IH]; intro t.
  - cbn; destruct t; cbn; now rewrite app_nil_r.
  - cbn [fold_left]; rewrite IH; cbn [register_cut cut_table group_table].
    rewrite <- app_assoc; reflexivity.
Qed.

Theorem publication_preserves_piece_support : forall t a ps sizes source,
  (forall node, Forall (supported t) (programs a node)) ->
  Forall (supported t) ps ->
  forall node, Forall (supported t)
    (programs (append (cut_bytes t) (prefixes t) (indices t) a ps sizes source) node).
Proof.
  intros t a ps sizes source Old New node.
  cbn [append programs].
  unfold update; destruct (node =? count a); assumption || apply Old.
Qed.

Print Assumptions empty_tables_are_well_formed.
Print Assumptions sequential_cuts_equal_batch.
Print Assumptions publication_preserves_piece_support.

Definition table_arena_valid (t : tables) (a : arena) : Prop :=
  well_formed t /\
  (forall node, Forall (supported t) (programs a node)) /\
  valid (cut_bytes t) (prefixes t) (indices t) a.

Theorem sequential_group_publication_equals_registration : forall t prefix values,
  let after_values := fold_left register_cut values t in
  Tables (cut_table after_values)
    (group_table after_values ++ [Group prefix (length (cut_table t)) (length values)]) =
  register_group t prefix values.
Proof. intros t prefix values; cbn zeta; rewrite sequential_cuts_equal_batch; reflexivity. Qed.

Theorem empty_table_arena_is_valid : table_arena_valid (Tables [] []) empty.
Proof.
  split; [apply empty_tables_are_well_formed|split].
  - intro node; constructor.
  - apply empty_is_valid.
Qed.

Theorem extension_transports_table_arena : forall before after a,
  table_arena_valid before a -> extends before after -> well_formed after ->
  table_arena_valid after a.
Proof.
  intros before after a [W [S V]] E New.
  split; [exact New|split].
  - intro node; eapply Forall_impl; [|exact (S node)].
    intros p Supported; exact (@extension_preserves_support before after p E Supported).
  - exact (@extension_preserves_arena_validity before after a W E S V).
Qed.

Theorem cut_registration_transports_table_arena : forall t a body,
  table_arena_valid t a -> table_arena_valid (register_cut t body) a.
Proof.
  intros t a body V.
  apply extension_transports_table_arena with (before := t); auto.
  - apply cut_registration_extends.
  - apply cut_registration_preserves_well_formed; exact (proj1 V).
Qed.

Theorem group_registration_transports_table_arena : forall t a prefix values,
  table_arena_valid t a -> table_arena_valid (register_group t prefix values) a.
Proof.
  intros t a prefix values V.
  apply extension_transports_table_arena with (before := t); auto.
  - apply group_registration_extends.
  - apply group_registration_preserves_well_formed; exact (proj1 V).
Qed.

Theorem checked_publication_preserves_table_arena : forall t a fields limit word64 next,
  table_arena_valid t a ->
  Forall (supported t) (map fst fields) ->
  Forall (backward (count a)) (map fst fields) ->
  field_certificate (cut_bytes t) (prefixes t) (indices t) a fields ->
  publish (cut_bytes t) (prefixes t) (indices t) limit word64
    a (map fst fields) (frame_source fields) = Some next ->
  table_arena_valid t next.
Proof.
  intros t a fields limit word64 next [W [S V]] New B C H.
  split; [exact W|split].
  - unfold publish in H.
    repeat match type of H with
    | context[planned_node_size ?c ?p ?g ?v ?l ?w ?m ?z ?ps] =>
      destruct (planned_node_size c p g v l w m z ps)
    end; try discriminate.
    destruct (_ =? _) eqn:L; inversion H; subst.
    apply publication_preserves_piece_support; assumption.
  - eapply successful_checked_publication_establishes_invariants; eauto.
Qed.

Print Assumptions sequential_group_publication_equals_registration.
Print Assumptions empty_table_arena_is_valid.
Print Assumptions extension_transports_table_arena.
Print Assumptions cut_registration_transports_table_arena.
Print Assumptions group_registration_transports_table_arena.
Print Assumptions checked_publication_preserves_table_arena.

End SubstitutionRegionTables.
