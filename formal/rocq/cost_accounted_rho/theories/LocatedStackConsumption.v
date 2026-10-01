From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import AuthorityPresentation LocatedAuthoritySettlement.
Import ListNotations.

Section LocatedStackConsumption.

Context {atom location surface : Type}.
Context (near : surface -> location -> Prop).

Definition located_inventory := nat -> option (location * @authority_stack atom).

Definition forget_locations (inventory : located_inventory) : @stack_inventory atom :=
  fun id => option_map snd (inventory id).

Definition selected_head (inventory : located_inventory) (id : nat) : @authority_cell atom :=
  match inventory id with
  | Some (_, head :: _) => head
  | _ => []
  end.

Record located_request := {
  request_surface : surface;
  request_demand : list atom;
  request_stacks : list nat
}.

Definition request_admitted (request : located_request) (inventory : located_inventory) : Prop :=
  NoDup (request_stacks request) /\
  (forall id, In id (request_stacks request) ->
    exists place head tail,
      inventory id = Some (place, head :: tail) /\ near (request_surface request) place) /\
  exact_cover (request_demand request)
    (map (selected_head inventory) (request_stacks request)).

Definition consume_located (ids : list nat) (inventory : located_inventory) : located_inventory :=
  fun id =>
    if in_dec Nat.eq_dec id ids then
      match inventory id with
      | Some (place, _ :: tail) => Some (place, tail)
      | other => other
      end
    else inventory id.

Definition located_step request before after : Prop :=
  request_admitted request before /\
  after = consume_located (request_stacks request) before.

Inductive located_history : list located_request -> located_inventory -> located_inventory -> Prop :=
| located_history_nil : forall inventory, located_history [] inventory inventory
| located_history_cons : forall request rest before middle after,
    located_step request before middle ->
    located_history rest middle after ->
    located_history (request :: rest) before after.

Theorem located_step_refines_selected_consumption : forall request before after,
  located_step request before after ->
  selected_stack_step (request_stacks request) (forget_locations before) (forget_locations after).
Proof.
  intros request before after [[Hunique [Hnear Hcover]] ->].
  split; [exact Hunique |]. intros id.
  unfold forget_locations, consume_located.
  destruct (in_dec Nat.eq_dec id (request_stacks request)) as [Hin | Hout].
  - destruct (Hnear id Hin) as [place [head [tail [Hentry Hlocation]]]].
    rewrite Hentry. exists head, tail. split; reflexivity.
  - reflexivity.
Qed.

Theorem located_histories_refine_selected_consumption : forall requests before after,
  located_history requests before after ->
  selected_stack_history (map request_stacks requests) (forget_locations before) (forget_locations after).
Proof.
  intros requests before after Hhistory. induction Hhistory.
  - constructor.
  - simpl. econstructor.
    + exact (located_step_refines_selected_consumption _ _ _ H).
    + exact IHHhistory.
Qed.

Theorem consumption_preserves_location : forall ids inventory id place cells,
  inventory id = Some (place, cells) ->
  exists remaining, consume_located ids inventory id = Some (place, remaining).
Proof.
  intros ids inventory id place cells Hentry. unfold consume_located.
  destruct (in_dec Nat.eq_dec id ids); rewrite Hentry.
  - destruct cells as [| head tail]; eexists; reflexivity.
  - eexists; reflexivity.
Qed.

Theorem histories_preserve_location : forall requests before after,
  located_history requests before after ->
  forall id place cells, before id = Some (place, cells) ->
  exists remaining, after id = Some (place, remaining).
Proof.
  intros requests before after Hhistory. induction Hhistory; intros id place cells Hentry.
  - eexists. exact Hentry.
  - destruct H as [Hadmitted ->].
    destruct (consumption_preserves_location (request_stacks request) _ _ _ _ Hentry)
      as [remaining Hremaining].
    eapply IHHhistory. exact Hremaining.
Qed.

Theorem located_histories_preserve_exact_suffix : forall requests before after,
  located_history requests before after ->
  forall id place cells, before id = Some (place, cells) ->
  after id = Some (place, skipn (selected_uses id (map request_stacks requests)) cells) /\
  selected_uses id (map request_stacks requests) <= length cells.
Proof.
  intros requests before after Hhistory id place cells Hentry.
  pose proof (located_histories_refine_selected_consumption _ _ _ Hhistory) as Hselected.
  assert (Hforgot : forget_locations before id = Some cells).
  { unfold forget_locations. now rewrite Hentry. }
  pose proof (selected_history_preserves_ordered_suffix _ _ _ Hselected id cells Hforgot)
    as [Hsuffix Hbound].
  destruct (histories_preserve_location _ _ _ Hhistory id place cells Hentry)
    as [remaining Hremaining].
  unfold forget_locations in Hsuffix. rewrite Hremaining in Hsuffix. simpl in Hsuffix.
  inversion Hsuffix; subst. auto.
Qed.

Theorem admitted_use_requires_near_location : forall request inventory id place cells,
  request_admitted request inventory ->
  In id (request_stacks request) -> inventory id = Some (place, cells) ->
  near (request_surface request) place.
Proof.
  intros request inventory id place cells [_ [Hnear _]] Hin Hentry.
  destruct (Hnear id Hin) as [actual [head [tail [Hactual Hlocation]]]].
  rewrite Hentry in Hactual. inversion Hactual; subst. exact Hlocation.
Qed.

Theorem wrong_location_cannot_fund : forall request inventory id place cells,
  In id (request_stacks request) -> inventory id = Some (place, cells) ->
  ~ near (request_surface request) place -> ~ request_admitted request inventory.
Proof.
  intros request inventory id place cells Hin Hentry Hfar Hadmitted.
  apply Hfar. eapply admitted_use_requires_near_location; eauto.
Qed.

Theorem admitted_heads_preserve_each_authority :
  forall eq_dec request inventory candidate,
  request_admitted request inventory ->
  count_occ eq_dec (concat (map (selected_head inventory) (request_stacks request))) candidate =
  count_occ eq_dec (request_demand request) candidate.
Proof.
  intros eq_dec request inventory candidate [_ [_ Hcover]].
  exact (exact_cover_preserves_each_atom eq_dec _ _ candidate Hcover).
Qed.

Theorem unselected_locations_are_unchanged : forall ids inventory id,
  ~ In id ids -> consume_located ids inventory id = inventory id.
Proof.
  intros ids inventory id Habsent. unfold consume_located.
  destruct (in_dec Nat.eq_dec id ids); tauto.
Qed.

Theorem non_near_location_is_unchanged : forall request before after id place cells,
  located_step request before after -> before id = Some (place, cells) ->
  ~ near (request_surface request) place -> after id = before id.
Proof.
  intros request before after id place cells [Hadmitted ->] Hentry Hfar.
  apply unselected_locations_are_unchanged. intro Hin.
  exact (wrong_location_cannot_fund _ _ _ _ _ Hin Hentry Hfar Hadmitted).
Qed.

Theorem located_histories_compose : forall first second before middle after,
  located_history first before middle -> located_history second middle after ->
  located_history (first ++ second) before after.
Proof.
  intros first second before middle after Hfirst Hsecond. induction Hfirst.
  - exact Hsecond.
  - simpl. econstructor; eauto.
Qed.

Theorem consumption_commutes_pointwise : forall first second inventory id,
  consume_located first (consume_located second inventory) id =
  consume_located second (consume_located first inventory) id.
Proof.
  intros first second inventory id. unfold consume_located.
  destruct (in_dec Nat.eq_dec id first), (in_dec Nat.eq_dec id second);
    try reflexivity.
Qed.

Definition selection_location_plan request (inventory : located_inventory) : @located_plan nat surface nat :=
  {| located_regions := map (fun id =>
       {| region_signature := id; region_location := request_surface request |})
       (request_stacks request);
     located_purses := fun current => region_signature current;
     located_near := fun current id =>
       exists place cells, inventory id = Some (place, cells) /\
         near (region_location current) place |}.

Theorem admitted_selection_has_located_plan : forall request inventory,
  request_admitted request inventory ->
  plan_is_located (selection_location_plan request inventory).
Proof.
  intros request inventory [_ [Hnear _]] current Hin.
  unfold selection_location_plan in *. simpl in *.
  apply in_map_iff in Hin. destruct Hin as [id [<- Hin]]. simpl.
  destruct (Hnear id Hin) as [place [head [tail [Hentry Hlocation]]]].
  exists place, (head :: tail). auto.
Qed.

Definition selections_disjoint (first second : located_request) : Prop :=
  forall id, In id (request_stacks first) -> ~ In id (request_stacks second).

Theorem disjoint_consumption_preserves_admission : forall first second inventory,
  selections_disjoint first second -> request_admitted second inventory ->
  request_admitted second (consume_located (request_stacks first) inventory).
Proof.
  intros first second inventory Hdisjoint [Hunique [Hnear Hcover]].
  assert (Hunchanged : forall id, In id (request_stacks second) ->
    consume_located (request_stacks first) inventory id = inventory id).
  { intros id Hin. apply unselected_locations_are_unchanged.
    intro Hfirst. exact (Hdisjoint id Hfirst Hin). }
  split; [exact Hunique |]. split.
  - intros id Hin. rewrite (Hunchanged id Hin). exact (Hnear id Hin).
  - replace (map (selected_head (consume_located (request_stacks first) inventory))
      (request_stacks second)) with (map (selected_head inventory) (request_stacks second)).
    + exact Hcover.
    + apply map_ext_in. intros id Hin. unfold selected_head. now rewrite Hunchanged.
Qed.

Theorem independent_admitted_events_have_both_orders : forall first second inventory,
  selections_disjoint first second ->
  request_admitted first inventory -> request_admitted second inventory ->
  exists first_final second_final,
    located_history [first; second] inventory first_final /\
    located_history [second; first] inventory second_final /\
    forall id, first_final id = second_final id.
Proof.
  intros first second inventory Hdisjoint Hfirst Hsecond.
  assert (Hreverse : selections_disjoint second first).
  { intros id Hin Hother. exact (Hdisjoint id Hother Hin). }
  exists (consume_located (request_stacks second) (consume_located (request_stacks first) inventory)),
    (consume_located (request_stacks first) (consume_located (request_stacks second) inventory)).
  split.
  - econstructor.
    + split; [exact Hfirst | reflexivity].
    + econstructor.
      * split; [eapply disjoint_consumption_preserves_admission; eauto | reflexivity].
      * constructor.
  - split.
    + econstructor.
      * split; [exact Hsecond | reflexivity].
      * econstructor.
        -- split; [eapply disjoint_consumption_preserves_admission; eauto | reflexivity].
        -- constructor.
    + intro id. apply consumption_commutes_pointwise.
Qed.

Theorem one_cell_cannot_fund_two_snapshot_requests : forall first second inventory id place cell,
  request_admitted first inventory -> request_admitted second inventory ->
  In id (request_stacks first) -> In id (request_stacks second) ->
  inventory id = Some (place, [cell]) ->
  ~ request_admitted second (consume_located (request_stacks first) inventory).
Proof.
  intros first second inventory id place cell Hfirst Hsecond Hinfirst Hinsecond Hentry.
  intros [_ [Hnear _]]. destruct (Hnear id Hinsecond) as [actual [head [tail [Hafter _]]]].
  unfold consume_located in Hafter.
  destruct (in_dec Nat.eq_dec id (request_stacks first)); [| contradiction].
  rewrite Hentry in Hafter. discriminate.
Qed.

Theorem one_compound_head_is_admitted : forall cut place cell,
  near cut place ->
  request_admitted
    {| request_surface := cut; request_demand := cell; request_stacks := [0] |}
    (fun id => if Nat.eq_dec id 0 then Some (place, [cell]) else None).
Proof.
  intros cut place cell Hnear. split.
  - repeat constructor. simpl. tauto.
  - split.
    + intros id [<- | Hin]; [| contradiction].
      exists place, cell, []. auto.
    + unfold exact_cover, presentation_atoms, selected_head. simpl.
      rewrite app_nil_r. apply Permutation.Permutation_refl.
Qed.

Theorem two_heads_with_repeated_authority_are_admitted : forall cut place candidate,
  near cut place ->
  request_admitted
    {| request_surface := cut; request_demand := [candidate; candidate]; request_stacks := [0; 1] |}
    (fun id => if Nat.ltb id 2 then Some (place, [[candidate]]) else None).
Proof.
  intros cut place candidate Hnear. split.
  - repeat constructor; simpl; intuition discriminate.
  - split.
    + intros id [<- | [<- | Hin]]; try contradiction;
        exists place, [candidate], []; auto.
    + unfold exact_cover, presentation_atoms, selected_head. simpl.
      apply Permutation.Permutation_refl.
Qed.

Theorem functional_nearness_has_one_result :
  forall (matching_location : surface -> option location),
  (forall cut place, near cut place <-> matching_location cut = Some place) ->
  forall request inventory first second first_place second_place first_cells second_cells,
  request_admitted request inventory ->
  In first (request_stacks request) -> In second (request_stacks request) ->
  inventory first = Some (first_place, first_cells) ->
  inventory second = Some (second_place, second_cells) -> first_place = second_place.
Proof.
  intros matching_location Hmatch request inventory first second first_place second_place
    first_cells second_cells Hadmitted Hfirst Hsecond Efirst Esecond.
  pose proof (admitted_use_requires_near_location _ _ _ _ _ Hadmitted Hfirst Efirst) as Nfirst.
  pose proof (admitted_use_requires_near_location _ _ _ _ _ Hadmitted Hsecond Esecond) as Nsecond.
  apply Hmatch in Nfirst. apply Hmatch in Nsecond. congruence.
Qed.

End LocatedStackConsumption.
