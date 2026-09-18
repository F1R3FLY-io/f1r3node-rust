From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import SubstitutionRegionRanges SubstitutionRegionTables
  SubstitutionRegionFraming SubstitutionRegionPublication SubstitutionRegionEmission.

Import ListNotations.
Set Implicit Arguments.
Module SubstitutionRegionProvenance.
Module R := SubstitutionRegionRanges.SubstitutionRegionRanges.
Module T := SubstitutionRegionTables.SubstitutionRegionTables.
Module F := SubstitutionRegionFraming.SubstitutionRegionFraming.
Module P := SubstitutionRegionPublication.SubstitutionRegionPublication.
Module E := SubstitutionRegionEmission.SubstitutionRegionEmission.

Inductive kind := MessageStep | MapValueStep | SnapshotStep | ValueStep.
Record step := Step {
  schema_owner : nat; field_tag : nat; occurrence : nat; step_kind : kind
}.
Record path := Path {
  parent : option nat; path_step : step; payload : R.region
}.
Record cut := Cut { cut_region : R.region; leaf : nat }.
Record group := Group {
  prefix : R.region; whole : R.region; group_leaf : nat;
  first_cut : nat; cut_count : nat
}.
Record state := State {
  paths : list path; cuts : list cut; groups : list group
}.

Definition contained (inner outer : R.region) : Prop :=
  R.owner inner = R.owner outer /\
  R.first outer <= R.first inner /\
  R.first inner <= R.last inner /\ R.last inner <= R.last outer.

Section Backing.
Variable backing : nat -> E.bytes.
Variable root : R.region.

Definition region_valid r := R.valid (length (backing (R.owner r))) r.
Definition region_bytes r := F.interval (backing (R.owner r)) (R.first r) (R.last r).

Theorem containment_is_transitive : forall a b c,
  contained a b -> contained b c -> contained a c.
Proof. unfold contained; intros; intuition congruence || lia. Qed.

Theorem containment_preserves_region_validity : forall a b,
  region_valid b -> contained a b -> region_valid a.
Proof.
  unfold region_valid, R.valid, contained; intros a b V [O B].
  rewrite O; intuition lia.
Qed.

Theorem checked_subregion_is_contained : forall limit r lo hi out,
  region_valid r -> R.subregion limit r lo hi = Some out -> contained out r.
Proof.
  intros limit r lo hi out V H; apply R.subregion_success_exact in H.
  destruct H as [-> [L _]].
  unfold region_valid, R.valid in V; unfold R.local_valid in L.
  unfold contained, R.child; cbn; intuition lia.
Qed.

Theorem subregion_bytes_are_parent_interval : forall r lo hi,
  region_valid r -> R.local_valid r lo hi ->
  region_bytes (R.child r lo hi) = F.interval (region_bytes r) lo hi.
Proof.
  intros r lo hi V L.
  unfold region_bytes, F.interval, R.child; cbn.
  rewrite skipn_firstn_comm, firstn_firstn, skipn_skipn.
  unfold R.local_valid in L.
  replace (R.first r + hi - (R.first r + lo)) with (hi - lo) by lia.
  replace (Nat.min (hi - lo) (R.last r - R.first r - lo)) with (hi - lo) by
    (symmetry; apply Nat.min_l; lia).
  now rewrite Nat.add_comm.
Qed.

Definition context_region (ps : list path) (p : option nat) : option R.region :=
  match p with
  | None => Some root
  | Some i => option_map payload (nth_error ps i)
  end.

Definition admitted ps p r : Prop :=
  exists outer, context_region ps p = Some outer /\ contained r outer.

Inductive path_chain : list path -> Prop :=
| PathsEmpty : path_chain []
| PathsAppend : forall ps p s r,
    path_chain ps -> admitted ps p r ->
    path_chain (ps ++ [Path p s r]).

Theorem path_append_preserves_old_lookup : forall ps p s r i,
  i < length ps ->
  nth_error (ps ++ [Path p s r]) i = nth_error ps i.
Proof. intros; now apply nth_error_app1. Qed.

Theorem appended_path_has_exact_index : forall ps p s r,
  nth_error (ps ++ [Path p s r]) (length ps) = Some (Path p s r).
Proof.
  intros; rewrite nth_error_app2 by lia; now rewrite Nat.sub_diag.
Qed.

Theorem admitted_parent_is_backward : forall ps i r,
  admitted ps (Some i) r -> i < length ps.
Proof.
  intros ps i r [outer [H _]]; cbn [context_region] in H.
  destruct (nth_error ps i) eqn:N; try discriminate.
  apply nth_error_Some; rewrite N; discriminate.
Qed.

Theorem path_append_preserves_admission : forall ps p s r old body,
  admitted ps old body -> admitted (ps ++ [Path p s r]) old body.
Proof.
  intros ps p s r old body [outer [H C]]; exists outer; split; auto.
  destruct old as [i|]; cbn [context_region] in *; auto.
  destruct (nth_error ps i) eqn:N; try discriminate.
  rewrite nth_error_app1.
  - now rewrite N.
  - apply nth_error_Some; rewrite N; discriminate.
Qed.

Theorem path_chain_has_valid_regions : forall ps,
  region_valid root -> path_chain ps ->
  forall i node, nth_error ps i = Some node -> region_valid (payload node).
Proof.
  intros ps Root Chain; induction Chain as [|ps p s r Chain IH A].
  - intros i node H; destruct i; discriminate.
  - intros i node H; destruct (lt_dec i (length ps)) as [B|B].
    + rewrite nth_error_app1 in H by exact B; eauto.
    + rewrite nth_error_app2 in H by lia.
      destruct (i - length ps) as [|n]; [|destruct n; discriminate].
      inversion H; subst; cbn.
      destruct A as [outer [Context C]].
      apply containment_preserves_region_validity with (b := outer); auto.
      destruct p as [j|]; cbn [context_region] in Context.
      * destruct (nth_error ps j) eqn:N; try discriminate.
        inversion Context; subst; eauto.
      * inversion Context; subst; exact Root.
Qed.

Definition erase_group g :=
  T.Group (region_bytes (prefix g)) (first_cut g) (cut_count g).
Definition erase t :=
  T.Tables (map (fun c => region_bytes (cut_region c)) (cuts t))
    (map erase_group (groups t)).

Definition add_path t p s r :=
  State (paths t ++ [Path p s r]) (cuts t) (groups t).
Definition add_cut t r i :=
  State (paths t) (cuts t ++ [Cut r i]) (groups t).
Definition add_par t p s r :=
  add_cut (add_path t p s r) r (length (paths t)).
Definition add_value anchor s ordinal t r :=
  add_par t (Some anchor)
    (Step (schema_owner s) (field_tag s) ordinal ValueStep) r.
Fixpoint add_values anchor s ordinal t rs :=
  match rs with
  | [] => t
  | r :: rest => add_values anchor s (S ordinal) (add_value anchor s ordinal t r) rest
  end.
Definition snapshot_step s :=
  Step (schema_owner s) (field_tag s) (occurrence s) SnapshotStep.
Definition add_epathmap t p s body pref values :=
  let anchor := length (paths t) in
  let staged := add_values anchor s 0 (add_path t p (snapshot_step s) body) values in
  State (paths staged) (cuts staged)
    (groups staged ++ [Group pref body anchor (length (cuts t)) (length values)]).

Theorem path_registration_erases_to_identity : forall t p s r,
  erase (add_path t p s r) = erase t.
Proof. reflexivity. Qed.

Theorem par_registration_erases_to_byte_registration : forall t p s r,
  erase (add_par t p s r) = T.register_cut (erase t) (region_bytes r).
Proof.
  intros; unfold erase, add_par, add_path, add_cut, T.register_cut; cbn.
  now rewrite map_app.
Qed.

Theorem value_registration_erases_to_byte_registration : forall anchor s n t r,
  erase (add_value anchor s n t r) = T.register_cut (erase t) (region_bytes r).
Proof. intros; apply par_registration_erases_to_byte_registration. Qed.

Theorem value_fold_erases_to_sequential_cuts : forall values anchor s n t,
  erase (add_values anchor s n t values) =
  fold_left T.register_cut (map region_bytes values) (erase t).
Proof.
  induction values as [|r rest IH]; intros; cbn [add_values map fold_left]; auto.
  rewrite IH, value_registration_erases_to_byte_registration; reflexivity.
Qed.

Lemma value_fold_keeps_groups : forall values anchor s n t,
  groups (add_values anchor s n t values) = groups t.
Proof.
  induction values; intros; cbn [add_values]; auto.
  rewrite IHvalues; reflexivity.
Qed.

Theorem epathmap_registration_erases_to_byte_registration : forall t p s body pref values,
  erase (add_epathmap t p s body pref values) =
  T.register_group (erase t) (region_bytes pref) (map region_bytes values).
Proof.
  intros; unfold add_epathmap; cbn zeta.
  pose proof (@value_fold_erases_to_sequential_cuts values (length (paths t)) s 0
    (add_path t p (snapshot_step s) body)) as H.
  rewrite path_registration_erases_to_identity, T.sequential_cuts_equal_batch in H.
  pose proof (f_equal T.cut_table H) as C.
  unfold erase in *; cbn in C |- *.
  rewrite C, value_fold_keeps_groups, map_app; cbn.
  unfold T.register_group; cbn; rewrite !length_map; reflexivity.
Qed.

Definition cuts_linked t : Prop :=
  Forall (fun c => exists node,
    nth_error (paths t) (leaf c) = Some node /\ payload node = cut_region c) (cuts t).

Theorem path_registration_preserves_cut_links : forall t p s r,
  cuts_linked t -> cuts_linked (add_path t p s r).
Proof.
  intros t p s r H; unfold cuts_linked in *; cbn [add_path paths cuts].
  eapply Forall_impl; [|exact H]; intros c [node [N Equal]].
  exists node; split; auto.
  rewrite nth_error_app1; auto.
  apply nth_error_Some; rewrite N; discriminate.
Qed.

Theorem par_registration_preserves_cut_links : forall t p s r,
  cuts_linked t -> cuts_linked (add_par t p s r).
Proof.
  intros t p s r H; unfold add_par.
  pose proof (@path_registration_preserves_cut_links t p s r H) as Old.
  unfold cuts_linked in *; cbn [add_cut paths cuts]; apply Forall_app; split; auto.
  constructor; [|constructor]; cbn.
  exists (Path p s r); split; [apply appended_path_has_exact_index|reflexivity].
Qed.

Theorem par_registration_preserves_path_chain : forall t p s r,
  path_chain (paths t) -> admitted (paths t) p r ->
  path_chain (paths (add_par t p s r)).
Proof. intros; cbn [add_par add_cut add_path paths]; now constructor. Qed.

Theorem group_anchor_is_admitted : forall t p s body r,
  contained r body ->
  admitted (paths (add_path t p (snapshot_step s) body)) (Some (length (paths t))) r.
Proof.
  intros; exists body; split; auto.
  cbn [add_path paths context_region].
  rewrite appended_path_has_exact_index; reflexivity.
Qed.

Theorem value_fold_preserves_provenance : forall values anchor s n t,
  path_chain (paths t) -> cuts_linked t ->
  Forall (admitted (paths t) (Some anchor)) values ->
  path_chain (paths (add_values anchor s n t values)) /\
  cuts_linked (add_values anchor s n t values).
Proof.
  induction values as [|r rest IH]; intros anchor s n t Chain Links A.
  - cbn; auto.
  - inversion A as [|? ? Head Tail]; subst.
    cbn [add_values]; apply IH.
    + apply par_registration_preserves_path_chain; assumption.
    + apply par_registration_preserves_cut_links; assumption.
    + eapply Forall_impl; [|exact Tail].
      intros body Bound; apply path_append_preserves_admission; exact Bound.
Qed.

Theorem epathmap_registration_preserves_provenance : forall t p s body pref values,
  path_chain (paths t) -> cuts_linked t ->
  admitted (paths t) p body -> Forall (fun r => contained r body) values ->
  path_chain (paths (add_epathmap t p s body pref values)) /\
  cuts_linked (add_epathmap t p s body pref values).
Proof.
  intros t p s body pref values Chain Links A V.
  pose proof (@value_fold_preserves_provenance values (length (paths t)) s 0
    (add_path t p (snapshot_step s) body)) as H.
  assert (NewChain : path_chain (paths (add_path t p (snapshot_step s) body))) by
    (cbn [add_path paths]; now constructor).
  assert (NewLinks : cuts_linked (add_path t p (snapshot_step s) body)) by
    (now apply path_registration_preserves_cut_links).
  assert (ValueAdmission : Forall (admitted (paths (add_path t p (snapshot_step s) body))
    (Some (length (paths t)))) values).
  { eapply Forall_impl; [|exact V]; intros; now apply group_anchor_is_admitted. }
  specialize (H NewChain NewLinks ValueAdmission).
  exact H.
Qed.

Theorem par_registration_transports_published_arena : forall t a p s r,
  T.table_arena_valid (erase t) a ->
  T.table_arena_valid (erase (add_par t p s r)) a.
Proof.
  intros; rewrite par_registration_erases_to_byte_registration.
  now apply T.cut_registration_transports_table_arena.
Qed.

Theorem epathmap_registration_transports_published_arena : forall t a p s body pref values,
  T.table_arena_valid (erase t) a ->
  T.table_arena_valid (erase (add_epathmap t p s body pref values)) a.
Proof.
  intros; rewrite epathmap_registration_erases_to_byte_registration.
  now apply T.group_registration_transports_table_arena.
Qed.

Theorem epm_region_framing_establishes_new_field_certificate :
  forall t a p s body pref values tag,
  region_bytes body = region_bytes pref ++
    concat (map (E.value_frame E.Original) (map region_bytes values)) ->
  let after := erase (add_epathmap t p s body pref values) in
  P.field_certificate (T.cut_bytes after) (T.prefixes after) (T.indices after) a
    [(E.EPathMap tag (length (groups t)), E.delimited tag (region_bytes body))].
Proof.
  intros t a p s body pref values tag Frame; cbn zeta.
  rewrite epathmap_registration_erases_to_byte_registration, Frame.
  replace (length (groups t)) with (length (T.group_table (erase t))) by
    (unfold erase; cbn; apply length_map).
  apply T.group_registration_establishes_field_certificate.
Qed.

Theorem inherited_frame_composes_admission : forall ps p frame child,
  admitted ps p frame -> contained child frame -> admitted ps p child.
Proof.
  intros ps p frame child [outer [H C]] Inner.
  exists outer; split; auto; eapply containment_is_transitive; eauto.
Qed.

Fixpoint value_paths anchor s ordinal values : list path :=
  match values with
  | [] => []
  | r :: rest =>
      Path (Some anchor) (Step (schema_owner s) (field_tag s) ordinal ValueStep) r ::
        value_paths anchor s (S ordinal) rest
  end.

Fixpoint value_cuts first values : list cut :=
  match values with
  | [] => []
  | r :: rest => Cut r first :: value_cuts (S first) rest
  end.

Theorem value_fold_has_exact_paths_and_cuts : forall values anchor s ordinal t,
  paths (add_values anchor s ordinal t values) =
    paths t ++ value_paths anchor s ordinal values /\
  cuts (add_values anchor s ordinal t values) =
    cuts t ++ value_cuts (length (paths t)) values.
Proof.
  induction values as [|r rest IH]; intros anchor s ordinal t.
  - cbn; now rewrite !app_nil_r.
  - cbn [add_values value_paths value_cuts].
    destruct (IH anchor s (S ordinal) (add_value anchor s ordinal t r)) as [Paths Cuts].
    rewrite Paths, Cuts; cbn [add_value add_par add_path add_cut paths cuts].
    rewrite length_app; cbn.
    replace (length (paths t) + 1) with (S (length (paths t))) by lia.
    now rewrite <- !app_assoc.
Qed.

Theorem value_path_count_is_exact : forall values anchor s ordinal,
  length (value_paths anchor s ordinal values) = length values.
Proof. induction values; intros; cbn; auto. Qed.

Theorem value_cut_count_is_exact : forall values first,
  length (value_cuts first values) = length values.
Proof. induction values; intros; cbn; auto. Qed.

Theorem value_path_lookup_is_exact : forall values anchor s ordinal i r,
  nth_error values i = Some r ->
  nth_error (value_paths anchor s ordinal values) i =
    Some (Path (Some anchor)
      (Step (schema_owner s) (field_tag s) (ordinal + i) ValueStep) r).
Proof.
  induction values as [|head rest IH]; intros anchor s ordinal i r N.
  - destruct i; discriminate.
  - destruct i as [|i]; cbn in N |- *.
    + inversion N; subst; now rewrite Nat.add_0_r.
    + rewrite (IH anchor s (S ordinal) i r N).
      replace (S ordinal + i) with (ordinal + S i) by lia; reflexivity.
Qed.

Theorem value_cut_lookup_is_exact : forall values first i r,
  nth_error values i = Some r ->
  nth_error (value_cuts first values) i = Some (Cut r (first + i)).
Proof.
  induction values as [|head rest IH]; intros first i r N.
  - destruct i; discriminate.
  - destruct i as [|i]; cbn in N |- *.
    + inversion N; subst; now rewrite Nat.add_0_r.
    + rewrite (IH (S first) i r N).
      replace (S first + i) with (first + S i) by lia; reflexivity.
Qed.

Theorem epathmap_has_exact_group_and_value_records : forall t p s body pref values,
  let after := add_epathmap t p s body pref values in
  paths after = paths t ++ Path p (snapshot_step s) body :: value_paths (length (paths t)) s 0 values /\
  cuts after = cuts t ++ value_cuts (S (length (paths t))) values /\
  groups after = groups t ++ [Group pref body (length (paths t))
    (length (cuts t)) (length values)].
Proof.
  intros t p s body pref values; cbn zeta; unfold add_epathmap; cbn zeta.
  destruct (@value_fold_has_exact_paths_and_cuts values (length (paths t)) s 0
    (add_path t p (snapshot_step s) body)) as [Paths Cuts].
  cbn [paths cuts groups]; rewrite Paths, Cuts, value_fold_keeps_groups.
  cbn [add_path paths cuts groups]; rewrite length_app; cbn.
  replace (length (paths t) + 1) with (S (length (paths t))) by lia.
  rewrite <- app_assoc; auto.
Qed.

Theorem admitted_region_is_valid : forall ps p r,
  region_valid root -> path_chain ps -> admitted ps p r -> region_valid r.
Proof.
  intros ps p r Root Chain [outer [Context C]].
  apply containment_preserves_region_validity with (b := outer); auto.
  destruct p as [i|]; cbn [context_region] in Context.
  - destruct (nth_error ps i) eqn:N; try discriminate.
    inversion Context; subst; eapply path_chain_has_valid_regions; eauto.
  - inversion Context; subst; exact Root.
Qed.

Theorem epm_prefix_retains_backing_and_bounds : forall t p body pref,
  region_valid root -> path_chain (paths t) ->
  admitted (paths t) p body -> contained pref body ->
  region_valid pref /\ R.owner pref = R.owner body.
Proof.
  intros t p body pref Root Chain A C; split.
  - eapply containment_preserves_region_validity; [|exact C].
    eapply admitted_region_is_valid; eauto.
  - exact (proj1 C).
Qed.

Theorem local_value_ranges_interpret_as_parent_slices : forall body ranges,
  region_valid body ->
  Forall (fun bounds => R.local_valid body (fst bounds) (snd bounds)) ranges ->
  map region_bytes (map (fun bounds => R.child body (fst bounds) (snd bounds)) ranges) =
  map (fun bounds => F.interval (region_bytes body) (fst bounds) (snd bounds)) ranges.
Proof.
  intros body ranges V Bounds; rewrite map_map.
  apply map_ext_in; intros bounds I.
  apply subregion_bytes_are_parent_interval; auto.
  apply Forall_forall with (x := bounds) in Bounds; assumption.
Qed.

Theorem source_framing_derives_region_field_certificate :
  forall t a p s body prefix_bounds ranges tag,
  region_valid body ->
  R.local_valid body (fst prefix_bounds) (snd prefix_bounds) ->
  Forall (fun bounds => R.local_valid body (fst bounds) (snd bounds)) ranges ->
  region_bytes body = F.interval (region_bytes body) (fst prefix_bounds) (snd prefix_bounds) ++
    concat (map (E.value_frame E.Original)
      (map (fun bounds => F.interval (region_bytes body) (fst bounds) (snd bounds)) ranges)) ->
  let after := erase (add_epathmap t p s body
    (R.child body (fst prefix_bounds) (snd prefix_bounds))
    (map (fun bounds => R.child body (fst bounds) (snd bounds)) ranges)) in
  P.field_certificate (T.cut_bytes after) (T.prefixes after) (T.indices after) a
    [(E.EPathMap tag (length (groups t)), E.delimited tag (region_bytes body))].
Proof.
  intros t a p s body prefix_bounds ranges tag V Prefix Ranges Frame; cbn zeta.
  apply epm_region_framing_establishes_new_field_certificate.
  rewrite subregion_bytes_are_parent_interval by assumption.
  rewrite local_value_ranges_interpret_as_parent_slices by assumption.
  exact Frame.
Qed.

Theorem region_backed_registration_preserves_provenance_and_publication :
  forall t a p s body pref values,
  path_chain (paths t) -> cuts_linked t ->
  admitted (paths t) p body -> Forall (fun r => contained r body) values ->
  T.table_arena_valid (erase t) a ->
  let after := add_epathmap t p s body pref values in
  path_chain (paths after) /\ cuts_linked after /\
  T.table_arena_valid (erase after) a.
Proof.
  intros t a p s body pref values Chain Links A V Arena; cbn zeta.
  destruct (@epathmap_registration_preserves_provenance t p s body pref values
    Chain Links A V) as [NewChain NewLinks].
  split; [exact NewChain|split]; auto.
  now apply epathmap_registration_transports_published_arena.
Qed.

Definition groups_linked t : Prop :=
  Forall (fun g => exists node,
    nth_error (paths t) (group_leaf g) = Some node /\
    payload node = whole g /\ contained (prefix g) (whole g)) (groups t).

Theorem path_registration_preserves_group_links : forall t p s r,
  groups_linked t -> groups_linked (add_path t p s r).
Proof.
  intros t p s r H; unfold groups_linked in *; cbn [add_path paths groups].
  eapply Forall_impl; [|exact H]; intros g [node [N Rest]].
  exists node; split; auto.
  rewrite nth_error_app1; auto.
  apply nth_error_Some; rewrite N; discriminate.
Qed.

Theorem value_fold_preserves_group_links : forall values anchor s n t,
  groups_linked t -> groups_linked (add_values anchor s n t values).
Proof.
  induction values; intros; cbn [add_values]; auto.
  apply IHvalues; unfold add_value, add_par.
  apply path_registration_preserves_group_links; assumption.
Qed.

Theorem epathmap_registration_preserves_group_links : forall t p s body pref values,
  groups_linked t -> contained pref body ->
  groups_linked (add_epathmap t p s body pref values).
Proof.
  intros t p s body pref values Old Prefix.
  pose proof (@value_fold_preserves_group_links values (length (paths t)) s 0
    (add_path t p (snapshot_step s) body) (@path_registration_preserves_group_links t p (snapshot_step s) body Old)) as Preserved.
  unfold groups_linked, add_epathmap; cbn zeta; cbn [paths groups].
  apply Forall_app; split; [exact Preserved|].
  constructor; [|constructor]; cbn.
  exists (Path p (snapshot_step s) body); split; [|auto].
  destruct (@value_fold_has_exact_paths_and_cuts values (length (paths t)) s 0
    (add_path t p (snapshot_step s) body)) as [Paths Cuts].
  rewrite Paths; cbn [add_path paths].
  rewrite nth_error_app1.
  - apply appended_path_has_exact_index.
  - rewrite length_app; cbn; lia.
Qed.

Theorem linked_group_prefixes_are_valid : forall t,
  region_valid root -> path_chain (paths t) -> groups_linked t ->
  Forall (fun g => region_valid (whole g) /\ region_valid (prefix g)) (groups t).
Proof.
  intros t Root Chain Links; unfold groups_linked in Links.
  eapply Forall_impl; [|exact Links]; intros g [node [N [Equal C]]].
  assert (V : region_valid (whole g)).
  { rewrite <- Equal; eapply path_chain_has_valid_regions; eauto. }
  split; auto; eapply containment_preserves_region_validity; eauto.
Qed.

Theorem region_registration_preserves_all_retained_provenance :
  forall t a p s body pref values,
  region_valid root -> path_chain (paths t) -> cuts_linked t -> groups_linked t ->
  admitted (paths t) p body -> contained pref body ->
  Forall (fun r => contained r body) values ->
  T.table_arena_valid (erase t) a ->
  let after := add_epathmap t p s body pref values in
  path_chain (paths after) /\ cuts_linked after /\ groups_linked after /\
  Forall (fun g => region_valid (whole g) /\ region_valid (prefix g)) (groups after) /\
  T.table_arena_valid (erase after) a.
Proof.
  intros t a p s body pref values Root Chain Cuts Groups A Prefix Values Arena; cbn zeta.
  destruct (@region_backed_registration_preserves_provenance_and_publication
    t a p s body pref values Chain Cuts A Values Arena) as [NewChain [NewCuts NewArena]].
  assert (NewGroups : groups_linked (add_epathmap t p s body pref values)).
  { now apply epathmap_registration_preserves_group_links. }
  split; [exact NewChain|split]; [exact NewCuts|split]; [exact NewGroups|split].
  - now apply linked_group_prefixes_are_valid.
  - exact NewArena.
Qed.

Theorem decoded_epm_suffix_establishes_region_field_certificate :
  forall t a p s body prefix_end values tag count,
  region_valid body -> R.local_valid body 0 prefix_end ->
  EPM1.decode_frames count (skipn prefix_end (region_bytes body)) =
    Some (map region_bytes values, []) ->
  let after := erase (add_epathmap t p s body
    (R.child body 0 prefix_end) values) in
  P.field_certificate (T.cut_bytes after) (T.prefixes after) (T.indices after) a
    [(E.EPathMap tag (length (groups t)), E.delimited tag (region_bytes body))].
Proof.
  intros t a p s body prefix_end values tag count V Prefix Decode; cbn zeta.
  apply epm_region_framing_establishes_new_field_certificate.
  rewrite subregion_bytes_are_parent_interval by assumption.
  unfold F.interval; cbn; rewrite Nat.sub_0_r.
  pose proof (@F.successful_frames_reencode_consumed_prefix count
    (skipn prefix_end (region_bytes body)) (map region_bytes values) [] Decode) as Frames.
  rewrite app_nil_r in Frames.
  assert (Equal : EPM1.encode_frames (map region_bytes values) =
    concat (map (E.value_frame E.Original) (map region_bytes values))).
  { clear Decode Frames.
    induction (map region_bytes values) as [|b rest IH]; cbn; auto.
    unfold E.value_frame, E.value_body, EPM1.encode_frame; now rewrite IH. }
  rewrite <- Equal, <- Frames; symmetry; apply firstn_skipn.
Qed.

End Backing.
Print Assumptions decoded_epm_suffix_establishes_region_field_certificate.
Print Assumptions path_registration_preserves_group_links.
Print Assumptions value_fold_preserves_group_links.
Print Assumptions epathmap_registration_preserves_group_links.
Print Assumptions linked_group_prefixes_are_valid.
Print Assumptions region_registration_preserves_all_retained_provenance.
Print Assumptions containment_is_transitive.
Print Assumptions containment_preserves_region_validity.
Print Assumptions checked_subregion_is_contained.
Print Assumptions subregion_bytes_are_parent_interval.
Print Assumptions path_append_preserves_old_lookup.
Print Assumptions appended_path_has_exact_index.
Print Assumptions admitted_parent_is_backward.
Print Assumptions path_append_preserves_admission.
Print Assumptions path_chain_has_valid_regions.
Print Assumptions path_registration_erases_to_identity.
Print Assumptions par_registration_erases_to_byte_registration.
Print Assumptions value_registration_erases_to_byte_registration.
Print Assumptions value_fold_erases_to_sequential_cuts.
Print Assumptions value_fold_keeps_groups.
Print Assumptions epathmap_registration_erases_to_byte_registration.
Print Assumptions path_registration_preserves_cut_links.
Print Assumptions par_registration_preserves_cut_links.
Print Assumptions par_registration_preserves_path_chain.
Print Assumptions group_anchor_is_admitted.
Print Assumptions value_fold_preserves_provenance.
Print Assumptions epathmap_registration_preserves_provenance.
Print Assumptions par_registration_transports_published_arena.
Print Assumptions epathmap_registration_transports_published_arena.
Print Assumptions epm_region_framing_establishes_new_field_certificate.
Print Assumptions inherited_frame_composes_admission.
Print Assumptions value_fold_has_exact_paths_and_cuts.
Print Assumptions value_path_count_is_exact.
Print Assumptions value_cut_count_is_exact.
Print Assumptions value_path_lookup_is_exact.
Print Assumptions value_cut_lookup_is_exact.
Print Assumptions epathmap_has_exact_group_and_value_records.
Print Assumptions admitted_region_is_valid.
Print Assumptions epm_prefix_retains_backing_and_bounds.
Print Assumptions local_value_ranges_interpret_as_parent_slices.
Print Assumptions source_framing_derives_region_field_certificate.
Print Assumptions region_backed_registration_preserves_provenance_and_publication.
End SubstitutionRegionProvenance.
