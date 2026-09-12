From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import EPathMap PathMapPhysicalOwnership PathMapReversePruneTeardown.

Import ListNotations.
Set Implicit Arguments.

Module PathMapLeafExtension.

Definition path := PathMapPhysicalOwnership.path.
Definition payload := PathMapReversePruneTeardown.payload.

Fixpoint prefixb (prefix target : path) : bool :=
  match prefix, target with
  | [], _ => true
  | _, [] => false
  | a :: rest, b :: tail => Nat.eqb a b && prefixb rest tail
  end.

Lemma prefixb_refl : forall p, prefixb p p = true.
Proof. induction p; simpl; auto. now rewrite Nat.eqb_refl, IHp. Qed.

Lemma prefixb_spec : forall p t,
  prefixb p t = true <-> exists suffix, t = p ++ suffix.
Proof.
  induction p as [|a p IH]; intros [|b t]; simpl.
  - split; intros; [exists []|]; reflexivity.
  - split; intros; [exists (b :: t)|]; reflexivity.
  - split; [discriminate|intros [suffix H]; discriminate].
  - rewrite andb_true_iff, Nat.eqb_eq, IH.
    split.
    + intros [-> [suffix ->]]. exists suffix. reflexivity.
    + intros [suffix H]. inversion H; subst. split; auto. exists suffix. reflexivity.
Qed.

Lemma prefixb_trans : forall a b c,
  prefixb a b = true -> prefixb b c = true -> prefixb a c = true.
Proof.
  intros a b c Hab Hbc. apply prefixb_spec in Hab, Hbc.
  destruct Hab as [x ->], Hbc as [y ->].
  apply prefixb_spec. exists (x ++ y). symmetry. apply app_assoc.
Qed.

Lemma prefixb_common : forall t a b,
  prefixb a t = true -> prefixb b t = true ->
  prefixb a b = true \/ prefixb b a = true.
Proof.
  induction t as [|x t IH]; intros [|a aa] [|b bb]; simpl; auto;
    try discriminate.
  intros Ha Hb. apply andb_true_iff in Ha, Hb.
  destruct Ha as [Ha Haa], Hb as [Hb Hbb].
  apply Nat.eqb_eq in Ha, Hb. subst.
  destruct (IH aa bb Haa Hbb) as [H|H];
    [left|right]; now rewrite Nat.eqb_refl, H.
Qed.

Definition path_eq_dec : forall a b : path, {a = b} + {a <> b}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Record state := State {
  topology : @set_trie path;
  values : path -> option payload
}.

Definition well_formed (s : state) : Prop :=
  topology s [] = true /\
  (forall p q, prefixb p q = true -> topology s q = true -> topology s p = true) /\
  (forall q, topology s q = false -> values s q = None).

Definition terminal (s : state) (boundary : path) : Prop :=
  topology s boundary = true /\
  forall q, prefixb boundary q = true -> topology s q = true -> q = boundary.

Definition same_observation (left right : state) : Prop :=
  forall q, topology left q = topology right q /\ values left q = values right q.

Definition reference_create (s : state) (target : path) : state :=
  State (set_join (topology s) (fun q => prefixb q target)) (values s).

Definition replace_value (s : state) (key : path) (value : option payload) : state :=
  State (topology s) (fun q => if path_eq_dec q key then value else values s q).

Definition reference_insert (s : state) (target : path) (value : payload) : state :=
  replace_value (reference_create s target) target (Some value).

Definition park (s : state) (boundary : path) : state * option payload :=
  (replace_value s boundary None, values s boundary).

Definition graft_valueless_line (s : state) (boundary target : path) : state :=
  State
    (fun q => if prefixb boundary q then prefixb q target else topology s q)
    (fun q => if prefixb boundary q then None else values s q).

Definition extend_leaf (s : state) (boundary suffix : path) : state :=
  let '(parked, carrier) := park s boundary in
  replace_value (graft_valueless_line parked boundary (boundary ++ suffix)) boundary carrier.

Definition extend_and_insert (s : state) (boundary suffix : path) (value : payload) : state :=
  replace_value (extend_leaf s boundary suffix) (boundary ++ suffix) (Some value).

Lemma terminal_descendant_absent : forall s boundary q,
  well_formed s -> terminal s boundary ->
  prefixb boundary q = true -> q <> boundary ->
  topology s q = false /\ values s q = None.
Proof.
  intros s boundary q [_ [_ support]] [_ leaf] Hp Hneq.
  destruct (topology s q) eqn:Htop.
  - exfalso. apply Hneq. now apply leaf.
  - split; auto.
Qed.

Theorem leaf_extension_matches_reference : forall s boundary suffix,
  well_formed s -> terminal s boundary ->
  same_observation (extend_leaf s boundary suffix)
    (reference_create s (boundary ++ suffix)).
Proof.
  intros s b suffix Hwf Hterminal q.
  destruct Hwf as [Hroot [Hclosed Hsupport]].
  destruct Hterminal as [Hpresent Hleaf].
  assert (Hbt : prefixb b (b ++ suffix) = true).
  { apply prefixb_spec. exists suffix. reflexivity. }
  unfold extend_leaf, park, replace_value, graft_valueless_line,
    reference_create, set_join; simpl.
  split.
  - destruct (prefixb b q) eqn:Hbq.
    + destruct (topology s q) eqn:Hq; simpl; auto.
      assert (q = b) by (apply Hleaf; assumption). subst.
      now rewrite Hbt.
    + destruct (prefixb q (b ++ suffix)) eqn:Hqt; simpl.
      * destruct (prefixb_common (b ++ suffix) q b Hqt Hbt) as [Hqb|Hbad].
        -- rewrite (Hclosed q b Hqb Hpresent). reflexivity.
        -- congruence.
      * now rewrite orb_false_r.
  - destruct (path_eq_dec q b) as [->|Hneq]; auto.
    destruct (prefixb b q) eqn:Hbq; auto.
    destruct (topology s q) eqn:Hq.
    + exfalso. apply Hneq. now apply Hleaf.
    + symmetry. now apply Hsupport.
Qed.

Theorem insertion_matches_reference : forall s boundary suffix value,
  well_formed s -> terminal s boundary ->
  same_observation (extend_and_insert s boundary suffix value)
    (reference_insert s (boundary ++ suffix) value).
Proof.
  intros s b suffix value Hwf Hterm q.
  pose proof (@leaf_extension_matches_reference s b suffix Hwf Hterm q) as [Ht Hv].
  unfold extend_and_insert, reference_insert, replace_value; simpl.
  split; auto. destruct (path_eq_dec q (b ++ suffix)); auto.
Qed.

Theorem reference_preserves_well_formed : forall s target,
  well_formed s -> well_formed (reference_create s target).
Proof.
  intros s target [Hroot [Hclosed Hsupport]].
  unfold well_formed, reference_create, set_join; simpl.
  split; [now rewrite Hroot|]. split.
  - intros p q Hpq Hq. apply orb_true_iff in Hq.
    apply orb_true_iff. destruct Hq as [Hq|Hq].
    + left. eapply Hclosed; eauto.
    + right. eapply prefixb_trans; eauto.
  - intros q Hq. apply orb_false_iff in Hq. apply Hsupport. tauto.
Qed.

Theorem existing_path_creation_is_noop : forall s target,
  well_formed s -> topology s target = true ->
  same_observation (reference_create s target) s.
Proof.
  intros s target [_ [Hclosed _]] Hpresent q.
  split; [|reflexivity]. unfold reference_create, set_join; simpl.
  destruct (prefixb q target) eqn:Hprefix.
  - rewrite (Hclosed q target Hprefix Hpresent). reflexivity.
  - apply orb_false_r.
Qed.

Theorem empty_suffix_is_noop : forall s boundary,
  well_formed s -> terminal s boundary ->
  same_observation (extend_leaf s boundary []) s.
Proof.
  intros s b Hwf Hterm q.
  pose proof (@leaf_extension_matches_reference s b [] Hwf Hterm q) as [Ht Hv].
  rewrite app_nil_r in Ht, Hv.
  pose proof (@existing_path_creation_is_noop s b Hwf (proj1 Hterm) q) as [Ht' Hv'].
  split; congruence.
Qed.

Theorem repeated_creation_is_idempotent : forall s target,
  same_observation (reference_create (reference_create s target) target)
    (reference_create s target).
Proof.
  intros s target q. split; [|reflexivity].
  unfold reference_create, set_join; simpl.
  destruct (topology s q), (prefixb q target); reflexivity.
Qed.

Theorem segmentation_does_not_change_extension : forall s b chunks,
  extend_leaf s b (concat chunks) = extend_leaf s b (fold_right (@app nat) [] chunks).
Proof. reflexivity. Qed.

Inductive read_result := Existing (focus : path) | Missing (focus suffix : path).

Fixpoint read_boundary (s : state) (focus remaining : path) : read_result :=
  match remaining with
  | [] => Existing focus
  | byte :: rest =>
      if topology s (focus ++ [byte])
      then read_boundary s (focus ++ [byte]) rest
      else Missing focus (byte :: rest)
  end.

Theorem read_selection_is_exact : forall remaining s focus,
  topology s focus = true ->
  match read_boundary s focus remaining with
  | Existing endpoint => endpoint = focus ++ remaining /\ topology s endpoint = true
  | Missing boundary suffix =>
      boundary ++ suffix = focus ++ remaining /\
      topology s boundary = true /\ suffix <> [] /\
      topology s (boundary ++ firstn 1 suffix) = false
  end.
Proof.
  induction remaining as [|byte rest IH]; intros s focus Hfocus; simpl.
  - split; auto. now rewrite app_nil_r.
  - destruct (topology s (focus ++ [byte])) eqn:Hnext.
    + specialize (IH s (focus ++ [byte]) Hnext).
      destruct (read_boundary s (focus ++ [byte]) rest); simpl in *.
      * destruct IH as [Hpath Htop]. split; auto.
        rewrite Hpath, <- app_assoc. reflexivity.
      * destruct IH as [Hpath Hrest]. split; auto.
        rewrite Hpath, <- app_assoc. reflexivity.
    + repeat split; auto. discriminate.
Qed.

Theorem selected_leaf_matches_full_request : forall s request boundary suffix,
  well_formed s -> read_boundary s [] request = Missing boundary suffix ->
  terminal s boundary ->
  same_observation (extend_leaf s boundary suffix) (reference_create s request).
Proof.
  intros s request b suffix Hwf Hread Hterminal.
  pose proof (read_selection_is_exact request s [] (proj1 Hwf)) as Hselection.
  rewrite Hread in Hselection. simpl in Hselection.
  destruct Hselection as [Hpath _]. rewrite <- Hpath.
  now apply leaf_extension_matches_reference.
Qed.

Definition prepublication_mutable_focuses (boundary : path) : list path :=
  [boundary; boundary; boundary].

Theorem mutation_before_extension_stays_at_existing_boundary : forall s request b suffix p,
  well_formed s -> read_boundary s [] request = Missing b suffix ->
  In p (prepublication_mutable_focuses b) ->
  p = b /\ topology s p = true.
Proof.
  intros s request b suffix p Hwf Hread Hin.
  pose proof (read_selection_is_exact request s [] (proj1 Hwf)) as Hselection.
  rewrite Hread in Hselection. simpl in Hselection.
  destruct Hselection as [_ [Hpresent _]].
  unfold prepublication_mutable_focuses in Hin. simpl in Hin.
  assert (p = b) by intuition congruence. subst. auto.
Qed.

Theorem publication_makes_target_present_before_final_descent : forall s b suffix,
  topology (extend_leaf s b suffix) (b ++ suffix) = true.
Proof.
  intros. unfold extend_leaf, park, replace_value, graft_valueless_line; simpl.
  assert (H : prefixb b (b ++ suffix) = true).
  { apply prefixb_spec. exists suffix. reflexivity. }
  rewrite H. apply prefixb_refl.
Qed.

Fixpoint value_count (domain : list path) (s : state) : nat :=
  match domain with
  | [] => 0
  | q :: rest => (match values s q with None => 0 | Some _ => 1 end) + value_count rest s
  end.

Lemma same_values_same_count : forall domain left right,
  (forall q, values left q = values right q) -> value_count domain left = value_count domain right.
Proof.
  induction domain; intros left right H; simpl; auto.
  rewrite H, (IHdomain left right H). reflexivity.
Qed.

Theorem extension_preserves_all_values_and_count : forall s b suffix domain,
  well_formed s -> terminal s b ->
  (forall q, values (extend_leaf s b suffix) q = values s q) /\
  value_count domain (extend_leaf s b suffix) = value_count domain s.
Proof.
  intros s b suffix domain Hwf Hterm.
  assert (Hvalues : forall q, values (extend_leaf s b suffix) q = values s q).
  { intro q. exact (proj2 (@leaf_extension_matches_reference s b suffix Hwf Hterm q)). }
  split; auto. now apply same_values_same_count.
Qed.

Definition carrier_count (cell : option payload) := match cell with None => 0 | Some _ => 1 end.

Lemma replace_outside_domain_count : forall domain s b cell,
  ~ In b domain -> value_count domain (replace_value s b cell) = value_count domain s.
Proof.
  induction domain; intros s b cell Hnot; [reflexivity|].
  change ((match (if path_eq_dec a b then cell else values s a) with
    None => 0 | Some _ => 1 end) + value_count domain (replace_value s b cell) =
    carrier_count (values s a) + value_count domain s).
  destruct (path_eq_dec a b); [subst; exfalso; apply Hnot; now left|].
  rewrite IHdomain; [reflexivity|]. intro H. apply Hnot. now right.
Qed.

Theorem parking_count_is_exact : forall domain s b,
  NoDup domain -> In b domain ->
  value_count domain s = value_count domain (fst (park s b)) + carrier_count (snd (park s b)).
Proof.
  induction domain as [|q domain IH]; intros s b Hnodup Hin; [contradiction|].
  inversion Hnodup as [|? ? Hnot Htail]; subst.
  change (carrier_count (values s q) + value_count domain s =
    (match (if path_eq_dec q b then None else values s q) with None => 0 | Some _ => 1 end) +
    value_count domain (replace_value s b None) + carrier_count (values s b)).
  destruct (path_eq_dec q b) as [->|Hneq].
  - simpl.
    rewrite replace_outside_domain_count by assumption.
    unfold carrier_count. destruct (values s b); simpl; lia.
  - assert (Hin' : In b domain) by (simpl in Hin; intuition congruence).
    specialize (IH s b Htail Hin').
    change (value_count domain s = value_count domain (replace_value s b None) +
      carrier_count (values s b)) in IH.
    change (carrier_count (values s q) + value_count domain s =
      carrier_count (values s q) + value_count domain (replace_value s b None) +
      carrier_count (values s b)). lia.
Qed.

Inductive ownership_phase := Parked | Grafted | Restored | TransferredToCallee | Interrupted.

Record owned_stage := OwnedStage {
  stage_map : state;
  stage_carrier : option payload;
  stage_phase : ownership_phase
}.

Definition begin_park (s : state) (b : path) : owned_stage :=
  OwnedStage (fst (park s b)) (snd (park s b)) Parked.

Definition graft_stage (stage : owned_stage) (b suffix : path) : owned_stage :=
  OwnedStage (graft_valueless_line (stage_map stage) b (b ++ suffix))
    (stage_carrier stage) Grafted.

Definition restore_stage (stage : owned_stage) (b : path) : owned_stage :=
  OwnedStage (replace_value (stage_map stage) b (stage_carrier stage)) None Restored.

Definition transfer_to_callee (stage : owned_stage) : owned_stage * option payload :=
  (OwnedStage (stage_map stage) None TransferredToCallee, stage_carrier stage).

Definition interrupt_stage (stage : owned_stage) :=
  (OwnedStage (stage_map stage) None Interrupted,
    PathMapReversePruneTeardown.dispose_cell (stage_carrier stage)).

Fixpoint disposed_originals (trace : list PathMapReversePruneTeardown.teardown_event) : list payload :=
  match trace with
  | [] => []
  | PathMapReversePruneTeardown.DetachOriginal value :: rest => value :: disposed_originals rest
  | _ :: rest => disposed_originals rest
  end.

Lemma dispose_cell_exact : forall cell,
  disposed_originals (PathMapReversePruneTeardown.dispose_cell cell) =
    PathMapReversePruneTeardown.option_list cell.
Proof.
  intros [value|]; simpl; auto. destruct value; simpl; auto.
  f_equal. induction l; simpl; auto.
Qed.

Theorem interruption_releases_only_still_parked_value : forall stage,
  stage_map (fst (interrupt_stage stage)) = stage_map stage /\
  stage_carrier (fst (interrupt_stage stage)) = None /\
  disposed_originals (snd (interrupt_stage stage)) =
    PathMapReversePruneTeardown.option_list (stage_carrier stage) /\
  snd (interrupt_stage (fst (interrupt_stage stage))) = [].
Proof. intros. repeat split; try reflexivity. apply dispose_cell_exact. Qed.

Theorem successful_restore_transfers_carrier_once : forall s b suffix,
  stage_map (restore_stage (graft_stage (begin_park s b) b suffix) b) =
    extend_leaf s b suffix /\
  stage_carrier (restore_stage (graft_stage (begin_park s b) b suffix) b) = None /\
  snd (interrupt_stage (restore_stage (graft_stage (begin_park s b) b suffix) b)) = [].
Proof. intros. repeat split; reflexivity. Qed.

Theorem transferred_value_is_not_owned_by_carrier : forall stage,
  stage_carrier (fst (transfer_to_callee stage)) = None /\
  snd (transfer_to_callee stage) = stage_carrier stage /\
  snd (interrupt_stage (fst (transfer_to_callee stage))) = [].
Proof. intros. repeat split; reflexivity. Qed.

Definition singleton_line (target : path) (cell : option payload) : state :=
  State (fun q => prefixb q target)
    (fun q => if path_eq_dec q target then cell else None).

Theorem singleton_line_is_well_formed : forall target cell,
  well_formed (singleton_line target cell) /\ terminal (singleton_line target cell) target.
Proof.
  intros target cell. unfold well_formed, terminal, singleton_line; simpl.
  split.
  - split; [reflexivity|]. split.
    + intros p q Hpq Hqt. eapply prefixb_trans; eauto.
    + intros q Hq. destruct (path_eq_dec q target); auto.
      subst. rewrite prefixb_refl in Hq. discriminate.
  - split; [apply prefixb_refl|]. intros q Htq Hqt.
    apply prefixb_spec in Htq, Hqt.
    destruct Htq as [a Ha], Hqt as [b Hb].
    assert (length a = 0).
    { pose proof (f_equal (@length nat) Ha) as Hla.
      pose proof (f_equal (@length nat) Hb) as Hlb.
      rewrite length_app in Hla, Hlb.
      cbv [PathMapPhysicalOwnership.byte] in Hla, Hlb |- *. lia. }
    apply length_zero_iff_nil in H. subst a. now rewrite app_nil_r in Ha.
Qed.

Theorem nonvacuous_root_and_dangling_extension :
  same_observation
    (extend_leaf (singleton_line [] (Some (PathMapReversePruneTeardown.SetMember 7))) [] [1; 0])
    (reference_create (singleton_line [] (Some (PathMapReversePruneTeardown.SetMember 7))) [1; 0]) /\
  topology (extend_leaf (singleton_line [1] None) [1] [0; 2]) [1; 0; 2] = true /\
  values (extend_leaf (singleton_line [1] None) [1] [0; 2]) [1; 0; 2] = None.
Proof.
  split.
  - apply leaf_extension_matches_reference; apply singleton_line_is_well_formed.
  - split; reflexivity.
Qed.

Theorem naive_graft_loses_root_value :
  values (graft_valueless_line
    (singleton_line [] (Some (PathMapReversePruneTeardown.SetMember 7))) [] [1]) [] = None /\
  values (reference_create
    (singleton_line [] (Some (PathMapReversePruneTeardown.SetMember 7))) [1]) [] =
    Some (PathMapReversePruneTeardown.SetMember 7).
Proof. split; reflexivity. Qed.

Definition branching_fixture : state :=
  State (set_join (fun q => prefixb q [1; 2]) (fun q => prefixb q [1; 3]))
    (fun q => if path_eq_dec q [1; 2] then Some (PathMapReversePruneTeardown.SetMember 2) else None).

Theorem wrong_boundary_graft_destroys_sibling :
  topology (extend_leaf branching_fixture [1] [4]) [1; 2] = false /\
  topology (reference_create branching_fixture [1; 4]) [1; 2] = true /\
  values (extend_leaf branching_fixture [1] [4]) [1; 2] = None /\
  values (reference_create branching_fixture [1; 4]) [1; 2] =
    Some (PathMapReversePruneTeardown.SetMember 2) /\
  ~ terminal branching_fixture [1].
Proof.
  repeat split; try reflexivity.
  intros [_ H]. specialize (H [1; 2] eq_refl eq_refl). discriminate.
Qed.

Theorem interrupted_park_does_not_roll_back :
  values (stage_map (fst (interrupt_stage
    (begin_park (singleton_line [] (Some (PathMapReversePruneTeardown.SetMember 7))) [])))) [] = None /\
  values (singleton_line [] (Some (PathMapReversePruneTeardown.SetMember 7))) [] =
    Some (PathMapReversePruneTeardown.SetMember 7).
Proof. split; reflexivity. Qed.

Record virtual_cursor := VirtualCursor {
  authority_prefix : path;
  relative_focus : path
}.

Definition logical_focus (cursor : virtual_cursor) : path :=
  authority_prefix cursor ++ relative_focus cursor.

Definition virtual_move (s : state) (cursor : virtual_cursor) (relative : path) :=
  (s, VirtualCursor (authority_prefix cursor) relative).

Definition virtual_ascend (s : state) (cursor : virtual_cursor) (distance : nat) :=
  (s, VirtualCursor (authority_prefix cursor)
    (firstn (length (relative_focus cursor) - distance) (relative_focus cursor))).

Theorem virtual_navigation_preserves_source_and_authority : forall s cursor relative distance,
  fst (virtual_move s cursor relative) = s /\
  fst (virtual_ascend s cursor distance) = s /\
  authority_prefix (snd (virtual_move s cursor relative)) = authority_prefix cursor /\
  authority_prefix (snd (virtual_ascend s cursor distance)) = authority_prefix cursor /\
  prefixb (authority_prefix cursor) (logical_focus (snd (virtual_move s cursor relative))) = true /\
  prefixb (authority_prefix cursor) (logical_focus (snd (virtual_ascend s cursor distance))) = true.
Proof.
  intros. repeat split; try reflexivity;
    apply prefixb_spec; eexists; reflexivity.
Qed.

Theorem virtual_ascent_cannot_cross_authority_prefix : forall s cursor distance,
  length (relative_focus cursor) <= distance ->
  logical_focus (snd (virtual_ascend s cursor distance)) = authority_prefix cursor.
Proof.
  intros s cursor distance Hdistance.
  unfold virtual_ascend, logical_focus; simpl.
  assert (Hzero : length (relative_focus cursor) - distance = 0) by lia.
  rewrite Hzero. simpl. apply app_nil_r.
Qed.

Definition selected_writer_focus (result : read_result) : path :=
  match result with Existing focus | Missing focus _ => focus end.

Theorem root_read_selection_is_existing_prefix : forall s request,
  well_formed s ->
  topology s (selected_writer_focus (read_boundary s [] request)) = true /\
  prefixb (selected_writer_focus (read_boundary s [] request)) request = true.
Proof.
  intros s request Hwf.
  pose proof (read_selection_is_exact request s [] (proj1 Hwf)) as Hselection.
  destruct (read_boundary s [] request) as [focus|focus suffix]; simpl in *.
  - destruct Hselection as [-> Hpresent]. split; auto. apply prefixb_refl.
  - destruct Hselection as [Hpath [Hpresent _]]. split; auto.
    apply prefixb_spec. exists suffix. symmetry. exact Hpath.
Qed.

Theorem missing_requested_focus_has_no_value : forall s request b suffix,
  well_formed s -> read_boundary s [] request = Missing b suffix ->
  topology s request = false /\ values s request = None.
Proof.
  intros s request b suffix Hwf Hread.
  pose proof (read_selection_is_exact request s [] (proj1 Hwf)) as Hselection.
  rewrite Hread in Hselection. simpl in Hselection.
  destruct Hselection as [Hpath [_ [_ Hmissing]]].
  destruct Hwf as [_ [Hclosed Hsupport]].
  assert (Hprefix : prefixb (b ++ firstn 1 suffix) request = true).
  { apply prefixb_spec. exists (skipn 1 suffix). rewrite <- Hpath, <- app_assoc.
    now rewrite firstn_skipn. }
  destruct (topology s request) eqn:Hrequest.
  - pose proof (Hclosed _ _ Hprefix Hrequest) as Hpresent. simpl in Hpresent.
    rewrite Hmissing in Hpresent. discriminate.
  - split; auto.
Qed.

Theorem branching_missing_edge_is_not_leaf_extension :
  read_boundary branching_fixture [] [1; 4] = Missing [1] [4] /\
  ~ terminal branching_fixture [1] /\
  topology (reference_create branching_fixture [1; 4]) [1; 2] = true /\
  topology (reference_create branching_fixture [1; 4]) [1; 3] = true.
Proof.
  split; [reflexivity|]. split.
  - exact (proj2 (proj2 (proj2 (proj2 wrong_boundary_graft_destroys_sibling)))).
  - split; reflexivity.
Qed.

Module DescendantRemoval.

Inductive slot :=
| ValueSlot (key : path) (value : payload)
| ChildSlot (key : path) (relative_endpoints : list path).

Definition endpoints (entry : slot) : list path :=
  match entry with
  | ValueSlot key _ => [key]
  | ChildSlot key descendants => key :: map (app key) descendants
  end.

Definition slot_topology (q : path) (entry : slot) : bool :=
  existsb (prefixb q) (endpoints entry).

Definition parent_topology (entries : list slot) (q : path) : bool :=
  if path_eq_dec q [] then true else existsb (slot_topology q) entries.

Definition strict_descendant (focus endpoint : path) : bool :=
  prefixb focus endpoint && if path_eq_dec endpoint focus then false else true.

Definition has_descendants (focus : path) (entries : list slot) : bool :=
  existsb (fun entry => existsb (strict_descendant focus) (endpoints entry)) entries.

Definition value_at (q : path) (entry : slot) : option payload :=
  match entry with
  | ValueSlot key value => if path_eq_dec q key then Some value else None
  | ChildSlot _ _ => None
  end.

Fixpoint lookup_value (q : path) (entries : list slot) : option payload :=
  match entries with
  | [] => None
  | entry :: rest =>
      match value_at q entry with Some value => Some value | None => lookup_value q rest end
  end.

Definition original_entries (focus : path) (cell : option payload) (rest : list slot) :=
  match cell with None => rest | Some value => ValueSlot focus value :: rest end.

Definition image (entries : list slot) : state :=
  State (parent_topology entries) (fun q => lookup_value q entries).

Definition reference_removal (focus : path) (cell : option payload) (rest : list slot) :=
  replace_value (image (original_entries focus cell rest)) focus None.

Definition native_prune_flag (focus : path) (rest : list slot) : bool :=
  if path_eq_dec focus [] then false else has_descendants focus rest.

Definition removal_entries (focus : path) (cell : option payload) (rest : list slot) :=
  match cell with
  | None => rest
  | Some _ =>
      if path_eq_dec focus [] then rest
      else if native_prune_flag focus rest then rest
      else ChildSlot focus [] :: rest
  end.

Definition conditional_removal (focus : path) (cell : option payload) (rest : list slot) :=
  image (removal_entries focus cell rest).

Definition residual_value_unique (focus : path) (rest : list slot) :=
  Forall (fun entry => value_at focus entry = None) rest.

Lemma residual_lookup_absent : forall focus rest,
  residual_value_unique focus rest -> lookup_value focus rest = None.
Proof.
  intros focus rest H. induction H; simpl; auto. now rewrite H, IHForall.
Qed.

Lemma descendant_witness : forall focus rest,
  has_descendants focus rest = true ->
  exists entry endpoint,
    In entry rest /\ In endpoint (endpoints entry) /\
    prefixb focus endpoint = true /\ endpoint <> focus.
Proof.
  intros focus rest H. apply existsb_exists in H.
  destruct H as [entry [Hin H]]. apply existsb_exists in H.
  destruct H as [endpoint [Hendpoint H]]. unfold strict_descendant in H.
  apply andb_true_iff in H. destruct H as [Hprefix Hneq].
  exists entry, endpoint. repeat split; auto.
  destruct (path_eq_dec endpoint focus); congruence.
Qed.

Theorem descendant_payload_is_distinct_and_retained : forall focus rest value,
  has_descendants focus rest = true ->
  exists entry endpoint,
    In entry rest /\ In endpoint (endpoints entry) /\
    entry <> ValueSlot focus value /\ rest <> [].
Proof.
  intros focus rest value H.
  destruct (descendant_witness focus rest H) as [entry [endpoint [Hin [He [Hp Hneq]]]]].
  exists entry, endpoint. repeat split; auto.
  - intros ->. simpl in He. destruct He as [He|[]]. congruence.
  - intros ->. contradiction.
Qed.

Lemma descendant_supports_all_focus_prefixes : forall focus rest q,
  has_descendants focus rest = true -> prefixb q focus = true ->
  parent_topology rest q = true.
Proof.
  intros focus rest q Hdesc Hprefix.
  unfold parent_topology. destruct (path_eq_dec q []); auto.
  destruct (descendant_witness focus rest Hdesc) as [entry [endpoint [Hin [He [Hp _]]]]].
  apply existsb_exists. exists entry. split; auto.
  apply existsb_exists. exists endpoint. split; auto.
  eapply prefixb_trans; eauto.
Qed.

Lemma removal_lookup_exact : forall focus cell rest q,
  lookup_value q (removal_entries focus cell rest) = lookup_value q rest.
Proof.
  intros focus [value|] rest q; [|reflexivity].
  unfold removal_entries. destruct (path_eq_dec focus []); auto.
  destruct (native_prune_flag focus rest); reflexivity.
Qed.

Theorem conditional_removal_preserves_exact_topology : forall focus cell rest q,
  parent_topology (removal_entries focus cell rest) q =
    parent_topology (original_entries focus cell rest) q.
Proof.
  intros focus [value|] rest q; [|reflexivity].
  unfold removal_entries, original_entries.
  destruct (path_eq_dec focus []) as [->|Hnonroot].
  - unfold parent_topology. destruct (path_eq_dec q []) as [->|Hq]; auto.
    destruct q; [contradiction|reflexivity].
  - unfold native_prune_flag. destruct (path_eq_dec focus []); [contradiction|].
    destruct (has_descendants focus rest) eqn:Hdesc.
    + destruct (prefixb q focus) eqn:Hprefix.
      * pose proof (descendant_supports_all_focus_prefixes focus rest q Hdesc Hprefix) as Hcovered.
        unfold parent_topology in *. destruct (path_eq_dec q []); auto.
        unfold slot_topology in *. simpl. rewrite Hprefix, Hcovered. reflexivity.
      * unfold parent_topology. destruct (path_eq_dec q []); auto.
        unfold slot_topology. simpl. now rewrite Hprefix.
    + reflexivity.
Qed.

Theorem conditional_removal_matches_independent_reference : forall focus cell rest,
  residual_value_unique focus rest ->
  same_observation (conditional_removal focus cell rest)
    (reference_removal focus cell rest).
Proof.
  intros focus cell rest Hunique q. split.
  - apply conditional_removal_preserves_exact_topology.
  - unfold conditional_removal, reference_removal, image, replace_value; simpl.
    rewrite removal_lookup_exact.
    destruct (path_eq_dec q focus) as [->|Hneq].
    + now apply residual_lookup_absent.
    + destruct cell; simpl; auto.
      unfold value_at. destruct (path_eq_dec q focus); congruence.
Qed.

Theorem removal_changes_only_selected_value : forall focus cell rest,
  residual_value_unique focus rest ->
  values (conditional_removal focus cell rest) focus = None /\
  (forall q, q <> focus ->
    values (conditional_removal focus cell rest) q =
      values (image (original_entries focus cell rest)) q).
Proof.
  intros focus cell rest Hunique.
  pose proof (@conditional_removal_matches_independent_reference focus cell rest Hunique) as Hsame.
  split.
  - rewrite (proj2 (Hsame focus)). unfold reference_removal, replace_value; simpl.
    destruct (path_eq_dec focus focus); congruence.
  - intros q Hneq. rewrite (proj2 (Hsame q)). unfold reference_removal, replace_value; simpl.
    destruct (path_eq_dec q focus); congruence.
Qed.

Theorem removal_count_is_exact : forall domain focus cell rest,
  NoDup domain -> In focus domain -> residual_value_unique focus rest ->
  value_count domain (image (original_entries focus cell rest)) =
    value_count domain (conditional_removal focus cell rest) + carrier_count cell.
Proof.
  intros domain focus cell rest Hnodup Hin Hunique.
  pose proof (@parking_count_is_exact domain (image (original_entries focus cell rest)) focus Hnodup Hin) as Hcount.
  assert (Hcell : values (image (original_entries focus cell rest)) focus = cell).
  { destruct cell; simpl.
    - unfold value_at. destruct (path_eq_dec focus focus); congruence.
    - now apply residual_lookup_absent. }
  change (value_count domain (image (original_entries focus cell rest)) =
    value_count domain (reference_removal focus cell rest) +
    carrier_count (values (image (original_entries focus cell rest)) focus)) in Hcount.
  rewrite Hcell in Hcount.
  assert (Hcounts : value_count domain (conditional_removal focus cell rest) =
    value_count domain (reference_removal focus cell rest)).
  { apply same_values_same_count. intro q.
    exact (proj2 (@conditional_removal_matches_independent_reference focus cell rest Hunique q)). }
  now rewrite Hcounts.
Qed.

Inductive prune_entry_result := EarlyReturn | NeedsAncestorLoop.

Definition prune_entry (rest : list slot) :=
  match rest with [] => NeedsAncestorLoop | _ :: _ => EarlyReturn end.

Definition invokes_prune (focus : path) (cell : option payload) (rest : list slot) :=
  match cell with None => false | Some _ => native_prune_flag focus rest end.

Definition attempted_prune_entry (focus : path) (cell : option payload) (rest : list slot) :=
  if invokes_prune focus cell rest then Some (prune_entry rest) else None.

Theorem descendant_removal_stops_at_first_prune_guard : forall focus value rest,
  focus <> [] -> has_descendants focus rest = true ->
  invokes_prune focus (Some value) rest = true /\
  attempted_prune_entry focus (Some value) rest = Some EarlyReturn.
Proof.
  intros focus value rest Hroot Hdesc.
  destruct (descendant_payload_is_distinct_and_retained focus rest value Hdesc)
    as [entry [endpoint [_ [_ [_ Hnonempty]]]]].
  unfold attempted_prune_entry, invokes_prune, native_prune_flag.
  destruct (path_eq_dec focus []); [contradiction|]. rewrite Hdesc.
  split; auto. destruct rest; [contradiction|reflexivity].
Qed.

Theorem conditional_removal_never_enters_ancestor_pruning : forall focus cell rest,
  attempted_prune_entry focus cell rest <> Some NeedsAncestorLoop.
Proof.
  intros focus [value|] rest; [|discriminate].
  unfold attempted_prune_entry, invokes_prune, native_prune_flag.
  destruct (path_eq_dec focus []); [discriminate|].
  destruct (has_descendants focus rest) eqn:Hdesc; [|discriminate].
  destruct rest as [|entry rest]; [discriminate Hdesc|discriminate].
Qed.

Definition prune_work (focus : path) (cell : option payload) (rest : list slot) :=
  match attempted_prune_entry focus cell rest with
  | None => Some (0, 0, 0, 0)
  | Some EarlyReturn => Some (1, 0, 0, 0)
  | Some NeedsAncestorLoop => None
  end.

Theorem conditional_removal_has_bounded_prune_work : forall focus cell rest,
  exists guards, guards <= 1 /\ prune_work focus cell rest = Some (guards, 0, 0, 0).
Proof.
  intros focus cell rest.
  pose proof (conditional_removal_never_enters_ancestor_pruning focus cell rest) as Hsafe.
  unfold prune_work. destruct (attempted_prune_entry focus cell rest) as [[|]|];
    [exists 1|contradiction|exists 0]; split; auto.
Qed.

Theorem absent_and_root_removal_bypass_pruning : forall focus cell rest,
  removal_entries focus None rest = rest /\
  attempted_prune_entry focus None rest = None /\
  removal_entries [] cell rest = rest /\
  attempted_prune_entry [] cell rest = None.
Proof. intros focus [value|] rest; repeat split; reflexivity. Qed.

Theorem terminal_removal_retains_dangling_path : forall focus value rest,
  focus <> [] -> has_descendants focus rest = false ->
  removal_entries focus (Some value) rest = ChildSlot focus [] :: rest /\
  attempted_prune_entry focus (Some value) rest = None /\
  parent_topology (removal_entries focus (Some value) rest) focus = true.
Proof.
  intros focus value rest Hroot Hterminal.
  assert (Hflag : native_prune_flag focus rest = false).
  { unfold native_prune_flag. destruct (path_eq_dec focus []); congruence. }
  unfold removal_entries, attempted_prune_entry, invokes_prune.
  destruct (path_eq_dec focus []); [contradiction|]. rewrite Hflag.
  repeat split; auto. unfold parent_topology.
  destruct (path_eq_dec focus []); auto.
  unfold slot_topology; simpl. rewrite prefixb_refl. reflexivity.
Qed.

Definition child_key (entry : slot) :=
  match entry with ChildSlot key _ => [key] | ValueSlot _ _ => [] end.

Definition slot_one_false_mutant (focus : path) (rest : list slot) :=
  rest ++ [ChildSlot focus []].

Theorem unconditional_false_duplicates_same_key_child :
  flat_map child_key (slot_one_false_mutant [1] [ChildSlot [1] [[0]]]) = [[1]; [1]] /\
  ~ NoDup (flat_map child_key (slot_one_false_mutant [1] [ChildSlot [1] [[0]]])) /\
  removal_entries [1] (Some (PathMapReversePruneTeardown.SetMember 9))
    [ChildSlot [1] [[0]]] = [ChildSlot [1] [[0]]].
Proof.
  split; [reflexivity|]. split; [|reflexivity].
  intro H. inversion H; subst. simpl in *. tauto.
Qed.

Theorem unguarded_true_can_discard_terminal_path :
  parent_topology [] [1] = false /\
  parent_topology (removal_entries [1] (Some (PathMapReversePruneTeardown.SetMember 9)) []) [1] = true /\
  prune_entry [] = NeedsAncestorLoop.
Proof. repeat split; reflexivity. Qed.

Theorem compressed_deeper_value_is_sufficient_support :
  has_descendants [1] [ValueSlot [1; 0] (PathMapReversePruneTeardown.SetMember 10)] = true /\
  attempted_prune_entry [1] (Some (PathMapReversePruneTeardown.SetMember 9))
    [ValueSlot [1; 0] (PathMapReversePruneTeardown.SetMember 10)] = Some EarlyReturn /\
  lookup_value [1; 0] (removal_entries [1] (Some (PathMapReversePruneTeardown.SetMember 9))
    [ValueSlot [1; 0] (PathMapReversePruneTeardown.SetMember 10)]) =
      Some (PathMapReversePruneTeardown.SetMember 10).
Proof. repeat split; reflexivity. Qed.

End DescendantRemoval.

Inductive native_obligation :=
| ReadSelectionExact | TerminalChildMask | MutableFocusBeforeMissingEdge
| CompressedEndpointParentFocus | FreshDonorRootAbsent | DonorSuffixExact
| ParkNoPrune | ParkCarrierCount | BorrowedGraftRetainsDonor
| RestoreReplacementAbsent | SuccessfulEndpointCount | SiblingValuesAndTopology
| CowSnapshotPreservation | ScopedDisposerNonpanicking | DonorFinalRootTeardown
| CalleeTransferUnwindBoundary | EmptyRootNormalizationAfterCursor | NoOpPreservesTopology
| VirtualNavigationTopologyNeutral | VirtualAuthorityPrefix | BranchInsertionPreservesChildren.

Definition native_obligations : list native_obligation :=
  [ReadSelectionExact; TerminalChildMask; MutableFocusBeforeMissingEdge;
   CompressedEndpointParentFocus; FreshDonorRootAbsent; DonorSuffixExact;
   ParkNoPrune; ParkCarrierCount; BorrowedGraftRetainsDonor;
   RestoreReplacementAbsent; SuccessfulEndpointCount; SiblingValuesAndTopology;
   CowSnapshotPreservation; ScopedDisposerNonpanicking; DonorFinalRootTeardown;
   CalleeTransferUnwindBoundary; EmptyRootNormalizationAfterCursor; NoOpPreservesTopology;
   VirtualNavigationTopologyNeutral; VirtualAuthorityPrefix; BranchInsertionPreservesChildren].

Theorem native_catalog_is_exhaustive_and_unique :
  (forall obligation, In obligation native_obligations) /\ NoDup native_obligations.
Proof.
  split.
  - destruct obligation; simpl; tauto.
  - repeat constructor; simpl; intuition discriminate.
Qed.

Print Assumptions leaf_extension_matches_reference.
Print Assumptions insertion_matches_reference.
Print Assumptions reference_preserves_well_formed.
Print Assumptions existing_path_creation_is_noop.
Print Assumptions empty_suffix_is_noop.
Print Assumptions repeated_creation_is_idempotent.
Print Assumptions segmentation_does_not_change_extension.
Print Assumptions read_selection_is_exact.
Print Assumptions selected_leaf_matches_full_request.
Print Assumptions mutation_before_extension_stays_at_existing_boundary.
Print Assumptions publication_makes_target_present_before_final_descent.
Print Assumptions extension_preserves_all_values_and_count.
Print Assumptions parking_count_is_exact.
Print Assumptions interruption_releases_only_still_parked_value.
Print Assumptions successful_restore_transfers_carrier_once.
Print Assumptions transferred_value_is_not_owned_by_carrier.
Print Assumptions nonvacuous_root_and_dangling_extension.
Print Assumptions naive_graft_loses_root_value.
Print Assumptions wrong_boundary_graft_destroys_sibling.
Print Assumptions interrupted_park_does_not_roll_back.
Print Assumptions virtual_navigation_preserves_source_and_authority.
Print Assumptions virtual_ascent_cannot_cross_authority_prefix.
Print Assumptions root_read_selection_is_existing_prefix.
Print Assumptions missing_requested_focus_has_no_value.
Print Assumptions branching_missing_edge_is_not_leaf_extension.
Print Assumptions native_catalog_is_exhaustive_and_unique.
Print Assumptions DescendantRemoval.descendant_payload_is_distinct_and_retained.
Print Assumptions DescendantRemoval.conditional_removal_preserves_exact_topology.
Print Assumptions DescendantRemoval.conditional_removal_matches_independent_reference.
Print Assumptions DescendantRemoval.removal_changes_only_selected_value.
Print Assumptions DescendantRemoval.removal_count_is_exact.
Print Assumptions DescendantRemoval.descendant_removal_stops_at_first_prune_guard.
Print Assumptions DescendantRemoval.conditional_removal_never_enters_ancestor_pruning.
Print Assumptions DescendantRemoval.conditional_removal_has_bounded_prune_work.
Print Assumptions DescendantRemoval.absent_and_root_removal_bypass_pruning.
Print Assumptions DescendantRemoval.terminal_removal_retains_dangling_path.
Print Assumptions DescendantRemoval.unconditional_false_duplicates_same_key_child.
Print Assumptions DescendantRemoval.unguarded_true_can_discard_terminal_path.
Print Assumptions DescendantRemoval.compressed_deeper_value_is_sufficient_support.

End PathMapLeafExtension.
