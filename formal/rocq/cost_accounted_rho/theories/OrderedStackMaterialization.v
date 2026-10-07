From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import AuthorityPresentation.
Import ListNotations.

Section OrderedStackMaterialization.

Context {atom : Type}.
Context (atom_eq_dec : forall left right : atom, {left = right} + {left <> right}).

Record funded_birth := {
  birth_identity : nat;
  birth_cells : @authority_stack atom;
  birth_funding : list atom
}.

Definition birth_charge (birth : funded_birth) (payer : atom) : nat :=
  length (birth_cells birth) * count_occ atom_eq_dec (birth_funding birth) payer.

Definition book_charge (book : list funded_birth) (payer : atom) : nat :=
  fold_right (fun birth total => birth_charge birth payer + total) 0 book.

Record materialization_ledger := {
  unreserved : atom -> nat;
  pending_births : list funded_birth;
  committed_births : list funded_birth
}.

Definition accounted_funding (state : materialization_ledger) (payer : atom) : nat :=
  unreserved state payer + book_charge (pending_births state) payer +
  book_charge (committed_births state) payer.

Definition identity_fresh (id : nat) (state : materialization_ledger) : Prop :=
  ~ In id (map birth_identity (pending_births state ++ committed_births state)).

Definition reserve_birth (state : materialization_ledger) (birth : funded_birth)
    : materialization_ledger :=
  {| unreserved := fun payer => unreserved state payer - birth_charge birth payer;
     pending_births := birth :: pending_births state;
     committed_births := committed_births state |}.

Definition publish_birth (state : materialization_ledger) (birth : funded_birth)
    (left right : list funded_birth) : materialization_ledger :=
  {| unreserved := unreserved state;
     pending_births := left ++ right;
     committed_births := birth :: committed_births state |}.

Definition release_birth (state : materialization_ledger) (birth : funded_birth)
    (left right : list funded_birth) : materialization_ledger :=
  {| unreserved := fun payer => unreserved state payer + birth_charge birth payer;
     pending_births := left ++ right;
     committed_births := committed_births state |}.

Inductive materialization_action :=
| PrepareBirth (birth : funded_birth)
| CommitBirth (id : nat)
| AbortBirth (id : nat).

Inductive materialization_step :
    materialization_action -> materialization_ledger -> materialization_ledger -> Prop :=
| MaterializationPrepare : forall state birth,
    birth_cells birth <> [] ->
    identity_fresh (birth_identity birth) state ->
    (forall payer, birth_charge birth payer <= unreserved state payer) ->
    materialization_step (PrepareBirth birth) state (reserve_birth state birth)
| MaterializationCommit : forall state birth left right,
    pending_births state = left ++ birth :: right ->
    materialization_step (CommitBirth (birth_identity birth)) state
      (publish_birth state birth left right)
| MaterializationAbort : forall state birth left right,
    pending_births state = left ++ birth :: right ->
    materialization_step (AbortBirth (birth_identity birth)) state
      (release_birth state birth left right).

Inductive materialization_history :
    list materialization_action -> materialization_ledger -> materialization_ledger -> Prop :=
| MaterializationHistoryNil : forall state, materialization_history [] state state
| MaterializationHistoryCons : forall action actions before middle after,
    materialization_step action before middle ->
    materialization_history actions middle after ->
    materialization_history (action :: actions) before after.

Lemma book_charge_app : forall left right payer,
  book_charge (left ++ right) payer = book_charge left payer + book_charge right payer.
Proof.
  intros left right payer. induction left; simpl; lia.
Qed.

Definition unique_birth_identities (state : materialization_ledger) : Prop :=
  NoDup (map birth_identity (pending_births state ++ committed_births state)).

Lemma commit_reorders_the_same_births : forall (left right : list funded_birth) birth committed,
  Permutation ((left ++ birth :: right) ++ committed)
    ((left ++ right) ++ birth :: committed).
Proof.
  intros. rewrite <- app_assoc. simpl.
  transitivity (birth :: left ++ right ++ committed).
  - symmetry. apply Permutation_middle.
  - rewrite app_assoc. apply Permutation_middle.
Qed.

Theorem materialization_preserves_unique_identities : forall action before after,
  materialization_step action before after ->
  unique_birth_identities before -> unique_birth_identities after.
Proof.
  intros action before after Hstep Hunique.
  unfold unique_birth_identities in *. destruct Hstep.
  - simpl. constructor; [exact H0 | exact Hunique].
  - simpl. rewrite H in Hunique.
    eapply Permutation_NoDup; [| exact Hunique].
    apply Permutation_map. apply commit_reorders_the_same_births.
  - simpl. rewrite H in Hunique.
    rewrite !map_app in *. simpl in Hunique.
    rewrite <- app_assoc in Hunique. simpl in Hunique.
    rewrite <- app_assoc.
    eapply NoDup_remove_1. exact Hunique.
Qed.

Theorem materialization_histories_preserve_unique_identities : forall actions before after,
  materialization_history actions before after ->
  unique_birth_identities before -> unique_birth_identities after.
Proof.
  intros actions before after Hhistory. induction Hhistory; intro Hunique.
  - exact Hunique.
  - apply IHHhistory. eapply materialization_preserves_unique_identities; eauto.
Qed.

Theorem each_materialized_cell_requires_enclosing_funding : forall birth payer,
  birth_charge birth payer =
    count_occ atom_eq_dec (concat (repeat (birth_funding birth) (length (birth_cells birth)))) payer.
Proof.
  intros [id cells funding] payer. unfold birth_charge. simpl.
  induction cells as [| cell rest IH]; simpl; [reflexivity |].
  rewrite count_occ_app. now rewrite <- IH.
Qed.

Theorem materialization_step_conserves_each_payer : forall action before after,
  materialization_step action before after ->
  forall payer, accounted_funding after payer = accounted_funding before payer.
Proof.
  intros action before after Hstep payer. destruct Hstep.
  - unfold accounted_funding, reserve_birth. simpl. specialize (H1 payer). lia.
  - unfold accounted_funding, publish_birth. simpl.
    rewrite H, !book_charge_app. simpl. lia.
  - unfold accounted_funding, release_birth. simpl.
    rewrite H, !book_charge_app. simpl. lia.
Qed.

Theorem arbitrary_materialization_histories_conserve_each_payer :
  forall actions before after,
    materialization_history actions before after ->
    forall payer, accounted_funding after payer = accounted_funding before payer.
Proof.
  intros actions before after Hhistory. induction Hhistory; intro payer.
  - reflexivity.
  - rewrite IHHhistory. now apply (materialization_step_conserves_each_payer _ _ _ H).
Qed.

Theorem duplicate_materialization_cannot_prepare : forall state birth after,
  In (birth_identity birth)
    (map birth_identity (pending_births state ++ committed_births state)) ->
  ~ materialization_step (PrepareBirth birth) state after.
Proof.
  intros state birth after Hseen Hstep. inversion Hstep; subst.
  unfold identity_fresh in *. contradiction.
Qed.

Theorem underfunded_materialization_cannot_prepare : forall state birth after payer,
  unreserved state payer < birth_charge birth payer ->
  ~ materialization_step (PrepareBirth birth) state after.
Proof.
  intros state birth after payer Hshort Hstep. inversion Hstep; subst.
  match goal with Hfits : forall payer, _ |- _ => specialize (Hfits payer) end.
  lia.
Qed.

Theorem empty_payload_cannot_prepare : forall state birth after,
  birth_cells birth = [] -> ~ materialization_step (PrepareBirth birth) state after.
Proof.
  intros state birth after Hempty Hstep. inversion Hstep; subst. contradiction.
Qed.

Theorem preparation_keeps_exact_payload_and_no_committed_birth : forall state birth,
  pending_births (reserve_birth state birth) = birth :: pending_births state /\
  committed_births (reserve_birth state birth) = committed_births state.
Proof. intros. split; reflexivity. Qed.

Theorem commit_publishes_exact_reserved_payload : forall state id after,
  materialization_step (CommitBirth id) state after ->
  exists birth left right,
    birth_identity birth = id /\
    pending_births state = left ++ birth :: right /\
    pending_births after = left ++ right /\
    committed_births after = birth :: committed_births state.
Proof.
  intros state id after Hstep. inversion Hstep; subst.
  eexists. eexists. eexists. repeat split; eauto.
Qed.

Theorem immediate_abort_restores_each_payer : forall state birth,
  (forall payer, birth_charge birth payer <= unreserved state payer) ->
  forall payer,
    unreserved (release_birth (reserve_birth state birth) birth [] (pending_births state)) payer =
      unreserved state payer /\
    pending_births (release_birth (reserve_birth state birth) birth [] (pending_births state)) =
      pending_births state /\
    committed_births (release_birth (reserve_birth state birth) birth [] (pending_births state)) =
      committed_births state.
Proof.
  intros state birth Hfits payer. specialize (Hfits payer).
  unfold release_birth, reserve_birth. simpl. repeat split; try reflexivity; lia.
Qed.

Theorem fresh_funded_payload_can_commit : forall state birth,
  birth_cells birth <> [] -> identity_fresh (birth_identity birth) state ->
  (forall payer, birth_charge birth payer <= unreserved state payer) ->
  materialization_history [PrepareBirth birth; CommitBirth (birth_identity birth)] state
    (publish_birth (reserve_birth state birth) birth [] (pending_births state)).
Proof.
  intros state birth Hnonempty Hfresh Hfits.
  econstructor; [now apply MaterializationPrepare |].
  econstructor.
  - apply (MaterializationCommit (reserve_birth state birth) birth [] (pending_births state)).
    reflexivity.
  - constructor.
Qed.

Definition install_birth (inventory : @stack_inventory atom) (birth : funded_birth)
    : @stack_inventory atom :=
  fun id => if Nat.eq_dec id (birth_identity birth)
            then Some (birth_cells birth) else inventory id.

Definition installed_committed_birth (state : materialization_ledger) (birth : funded_birth)
    (before after : @stack_inventory atom) : Prop :=
  In birth (committed_births state) /\
  before (birth_identity birth) = None /\ after = install_birth before birth.

Theorem installed_birth_preserves_exact_order : forall inventory birth,
  install_birth inventory birth (birth_identity birth) = Some (birth_cells birth).
Proof.
  intros. unfold install_birth. destruct (Nat.eq_dec _ _); congruence.
Qed.

Theorem installation_preserves_other_stacks : forall inventory birth id,
  id <> birth_identity birth -> install_birth inventory birth id = inventory id.
Proof.
  intros inventory birth id Hdifferent. unfold install_birth.
  destruct (Nat.eq_dec _ _); congruence.
Qed.

Theorem consumption_after_birth_preserves_ordered_suffix :
  forall state inventory installed birth events after,
    installed_committed_birth state birth inventory installed ->
    selected_stack_history events installed after ->
    after (birth_identity birth) =
      Some (skipn (selected_uses (birth_identity birth) events) (birth_cells birth)) /\
    selected_uses (birth_identity birth) events <= length (birth_cells birth).
Proof.
  intros state inventory installed birth events after [_ [_ Hinstalled]] Hhistory.
  subst installed.
  eapply selected_history_preserves_ordered_suffix; [exact Hhistory |].
  apply installed_birth_preserves_exact_order.
Qed.

Theorem committed_birth_installation_cannot_overwrite : forall state birth before after,
  before (birth_identity birth) <> None ->
  ~ installed_committed_birth state birth before after.
Proof.
  intros state birth before after Hoccupied [_ [Hfresh _]]. contradiction.
Qed.

Theorem committed_birth_cannot_be_installed_twice : forall state birth before middle after,
  installed_committed_birth state birth before middle ->
  ~ installed_committed_birth state birth middle after.
Proof.
  intros state birth before middle after [_ [_ Hinstalled]]. subst middle.
  apply committed_birth_installation_cannot_overwrite.
  rewrite installed_birth_preserves_exact_order. discriminate.
Qed.

Theorem not_yet_born_stack_cannot_fund_an_event :
  forall (inventory after : @stack_inventory atom) ids id,
  inventory id = None -> In id ids -> ~ selected_stack_step ids inventory after.
Proof.
  intros inventory after ids id Habsent Hin [_ Hstep]. specialize (Hstep id).
  destruct (in_dec Nat.eq_dec id ids); [| contradiction].
  destruct Hstep as [head [tail [Hhead Htail]]]. congruence.
Qed.

End OrderedStackMaterialization.

Fixpoint resolve_birth_positions (lookup : nat -> option nat) (required : list nat)
    : option (list nat) :=
  match required with
  | [] => Some []
  | id :: rest =>
      match lookup id, resolve_birth_positions lookup rest with
      | Some position, Some positions => Some (position :: positions)
      | _, _ => None
      end
  end.

Definition birth_completion_position (lookup : nat -> option nat) (required : list nat)
    : option nat :=
  match resolve_birth_positions lookup required with
  | Some (position :: positions) => Some (fold_right Nat.max position positions)
  | _ => None
  end.

Definition birth_ready_at (index : nat) (lookup : nat -> option nat) (required : list nat)
    : bool :=
  match birth_completion_position lookup required with
  | Some position => position <? index
  | None => false
  end.

Lemma resolved_positions_match_each_required_event : forall lookup required positions,
  resolve_birth_positions lookup required = Some positions ->
  Forall2 (fun id position => lookup id = Some position) required positions.
Proof.
  intros lookup required. induction required as [| id rest IH]; intros positions Hresolved.
  - simpl in Hresolved. inversion Hresolved; constructor.
  - simpl in Hresolved. destruct (lookup id) eqn:Hlookup; [| discriminate].
    destruct (resolve_birth_positions lookup rest) eqn:Hrest; [| discriminate].
    inversion Hresolved; subst. constructor; [exact Hlookup | now apply IH].
Qed.

Lemma fold_max_precedes_iff : forall positions initial index,
  fold_right Nat.max initial positions < index <->
  initial < index /\ Forall (fun position => position < index) positions.
Proof.
  induction positions as [| position rest IH]; intros initial index; simpl.
  - split; [auto | tauto].
  - rewrite Nat.max_lub_lt_iff, IH. split.
    + intros [Hhead [Hinitial Hrest]]. split; [exact Hinitial | now constructor].
    + intros [Hinitial Hall]. inversion Hall; subst. auto.
Qed.

Theorem readiness_iff_all_required_positions_precede : forall index lookup required,
  birth_ready_at index lookup required = true <->
  exists positions,
    resolve_birth_positions lookup required = Some positions /\
    positions <> [] /\ Forall (fun position => position < index) positions.
Proof.
  intros index lookup required. unfold birth_ready_at, birth_completion_position.
  destruct (resolve_birth_positions lookup required) as [positions |] eqn:Hresolved.
  - destruct positions as [| position rest].
    + split; [discriminate | intros [positions [Hsame [Hnonempty _]]]; congruence].
    + rewrite Nat.ltb_lt, fold_max_precedes_iff. split.
      * intros [Hhead Hrest]. exists (position :: rest).
        repeat split; try discriminate. now constructor.
      * intros [positions [Hsame [_ Hall]]]. inversion Hsame; subst.
        inversion Hall; subst. auto.
  - split; [discriminate | intros [positions [Hsame _]]; discriminate].
Qed.

Theorem every_birth_event_precedes_ready_use : forall index lookup required id,
  birth_ready_at index lookup required = true -> In id required ->
  exists position, lookup id = Some position /\ position < index.
Proof.
  intros index lookup required id Hready Hin.
  apply readiness_iff_all_required_positions_precede in Hready.
  destruct Hready as [positions [Hresolved [_ Hall]]].
  pose proof (resolved_positions_match_each_required_event _ _ _ Hresolved) as Hmatching.
  clear Hresolved. revert Hin Hall.
  induction Hmatching; intros Hin Hall; [contradiction |].
  inversion Hall; subst. destruct Hin as [Heq | Hin].
  - subst. eauto.
  - now apply IHHmatching.
Qed.

Lemma preceding_events_resolve : forall index lookup required,
  (forall id, In id required -> exists position, lookup id = Some position /\ position < index) ->
  exists positions, resolve_birth_positions lookup required = Some positions /\
    Forall (fun position => position < index) positions.
Proof.
  intros index lookup required. induction required as [| id rest IH]; intro Hprior.
  - exists []. split; [reflexivity | constructor].
  - destruct (Hprior id (or_introl eq_refl)) as [position [Hlookup Hbefore]].
    destruct IH as [positions [Hresolved Hall]].
    { intros nested Hin. apply Hprior. now right. }
    exists (position :: positions). split.
    + simpl. now rewrite Hlookup, Hresolved.
    + now constructor.
Qed.

Theorem readiness_iff_all_required_events_precede : forall index lookup required,
  birth_ready_at index lookup required = true <->
  required <> [] /\
  forall id, In id required -> exists position, lookup id = Some position /\ position < index.
Proof.
  intros index lookup required. split.
  - intro Hready. split.
    + intro Hempty. subst required. discriminate.
    + intros id Hin. eapply every_birth_event_precedes_ready_use; eauto.
  - intros [Hnonempty Hprior].
    destruct (preceding_events_resolve _ _ _ Hprior) as [positions [Hresolved Hall]].
    apply readiness_iff_all_required_positions_precede. exists positions.
    split; [exact Hresolved |]. split; [| exact Hall].
    intro Hempty. subst positions.
    pose proof (resolved_positions_match_each_required_event _ _ _ Hresolved) as Hmatching.
    inversion Hmatching; subst. contradiction.
Qed.

Theorem birth_readiness_ignores_required_event_order : forall index lookup first second,
  Permutation first second ->
  birth_ready_at index lookup first = birth_ready_at index lookup second.
Proof.
  intros index lookup first second Hperm.
  assert (Hequiv : birth_ready_at index lookup first = true <->
                   birth_ready_at index lookup second = true).
  { rewrite !readiness_iff_all_required_events_precede.
    split; intros [Hnonempty Hprior]; split.
    - intro Hempty. subst second. symmetry in Hperm. apply Permutation_nil in Hperm. contradiction.
    - intros id Hin. apply Hprior. eapply Permutation_in; [symmetry; exact Hperm | exact Hin].
    - intro Hempty. subst first. apply Permutation_nil in Hperm. contradiction.
    - intros id Hin. apply Hprior. eapply Permutation_in; eauto. }
  destruct (birth_ready_at index lookup first), (birth_ready_at index lookup second); intuition discriminate.
Qed.

Theorem missing_birth_event_prevents_use : forall index lookup required id,
  In id required -> lookup id = None -> birth_ready_at index lookup required = false.
Proof.
  intros index lookup required id Hin Hmissing.
  destruct (birth_ready_at index lookup required) eqn:Hready; [| reflexivity].
  destruct (every_birth_event_precedes_ready_use _ _ _ _ Hready Hin) as [position [Hfound _]].
  congruence.
Qed.

Theorem empty_birth_event_list_prevents_use : forall index lookup,
  birth_ready_at index lookup [] = false.
Proof. reflexivity. Qed.

Theorem creation_event_cannot_use_its_new_stack : forall lookup required id index,
  In id required -> lookup id = Some index -> birth_ready_at index lookup required = false.
Proof.
  intros lookup required id index Hin Hposition.
  destruct (birth_ready_at index lookup required) eqn:Hready; [| reflexivity].
  destruct (every_birth_event_precedes_ready_use _ _ _ _ Hready Hin)
    as [position [Hfound Hprior]].
  rewrite Hposition in Hfound. inversion Hfound. lia.
Qed.

Theorem first_event_after_completion_is_ready : forall lookup required position,
  birth_completion_position lookup required = Some position ->
  birth_ready_at (S position) lookup required = true.
Proof.
  intros lookup required position Hposition. unfold birth_ready_at.
  rewrite Hposition. apply Nat.ltb_lt. lia.
Qed.

Theorem readiness_is_monotone_after_completion : forall first later lookup required,
  first <= later -> birth_ready_at first lookup required = true ->
  birth_ready_at later lookup required = true.
Proof.
  intros first later lookup required Hlater.
  unfold birth_ready_at. destruct (birth_completion_position lookup required); [| discriminate].
  rewrite !Nat.ltb_lt. lia.
Qed.

Section CausalStackConsumption.

Context {atom : Type}.

Definition causal_stack_step (index : nat) (lookup : nat -> option nat)
    (birth_events : nat -> option (list nat)) (ids : list nat)
    (before after : @stack_inventory atom) : Prop :=
  selected_stack_step ids before after /\
  forall id required, In id ids -> birth_events id = Some required ->
    birth_ready_at index lookup required = true.

Inductive causal_stack_history (lookup : nat -> option nat)
    (birth_events : nat -> option (list nat)) :
    nat -> list (list nat) -> @stack_inventory atom -> @stack_inventory atom -> Prop :=
| CausalHistoryNil : forall index inventory,
    causal_stack_history lookup birth_events index [] inventory inventory
| CausalHistoryCons : forall index ids events before middle after,
    causal_stack_step index lookup birth_events ids before middle ->
    causal_stack_history lookup birth_events (S index) events middle after ->
    causal_stack_history lookup birth_events index (ids :: events) before after.

Theorem causal_histories_refine_ordered_consumption : forall lookup births index events before after,
  causal_stack_history lookup births index events before after ->
  selected_stack_history events before after.
Proof.
  intros lookup births index events before after Hhistory.
  induction Hhistory; [constructor |]. econstructor; [exact (proj1 H) | exact IHHhistory].
Qed.

Theorem causal_histories_preserve_exact_suffix : forall lookup births index events before after,
  causal_stack_history lookup births index events before after ->
  forall id cells, before id = Some cells ->
    after id = Some (skipn (selected_uses id events) cells) /\
    selected_uses id events <= length cells.
Proof.
  intros lookup births index events before after Hhistory id cells Hbefore.
  apply causal_histories_refine_ordered_consumption in Hhistory.
  eapply selected_history_preserves_ordered_suffix; eauto.
Qed.

Theorem causal_step_rejects_premature_birth_use :
  forall index lookup births ids before after stack required event position,
    In stack ids -> births stack = Some required -> In event required ->
    lookup event = Some position -> index <= position ->
    ~ causal_stack_step index lookup births ids before after.
Proof.
  intros index lookup births ids before after stack required event position
    Hselected Hbirth Hin Hposition Hpremature [_ Hready].
  specialize (Hready stack required Hselected Hbirth).
  destruct (every_birth_event_precedes_ready_use _ _ _ _ Hready Hin)
    as [actual [Hactual Hprior]].
  rewrite Hposition in Hactual. inversion Hactual. lia.
Qed.

Definition birth_metadata_well_formed (lookup : nat -> option nat)
    (births : nat -> option (list nat)) (inventory : @stack_inventory atom) : Prop :=
  forall id required, births id = Some required ->
    exists cells completion,
      inventory id = Some cells /\ length required = length cells /\
      birth_completion_position lookup required = Some completion.

Definition validated_causal_history lookup births index events before after : Prop :=
  birth_metadata_well_formed lookup births before /\
  causal_stack_history lookup births index events before after.

Theorem global_precheck_rejects_unknown_unused_stack : forall lookup births inventory id required,
  births id = Some required -> inventory id = None ->
  ~ birth_metadata_well_formed lookup births inventory.
Proof.
  intros lookup births inventory id required Hbirth Hmissing Hvalid.
  destruct (Hvalid id required Hbirth) as [cells [completion [Hcells _]]]. congruence.
Qed.

Theorem global_precheck_rejects_missing_unused_birth_event :
  forall lookup births inventory id required event,
    births id = Some required -> In event required -> lookup event = None ->
    ~ birth_metadata_well_formed lookup births inventory.
Proof.
  intros lookup births inventory id required event Hbirth Hin Hmissing Hvalid.
  destruct (Hvalid id required Hbirth) as [cells [completion [_ [_ Hcompletion]]]].
  pose proof (first_event_after_completion_is_ready _ _ _ Hcompletion) as Hready.
  rewrite (missing_birth_event_prevents_use (S completion) lookup required event Hin Hmissing)
    in Hready. discriminate.
Qed.

Theorem global_precheck_rejects_empty_unused_stack : forall lookup births inventory id required,
  births id = Some required -> inventory id = Some [] ->
  ~ birth_metadata_well_formed lookup births inventory.
Proof.
  intros lookup births inventory id required Hbirth Hempty Hvalid.
  destruct (Hvalid id required Hbirth) as [cells [completion [Hcells [Hlength Hcompletion]]]].
  rewrite Hempty in Hcells. inversion Hcells; subst cells. simpl in Hlength.
  apply length_zero_iff_nil in Hlength. subst required. discriminate.
Qed.

Theorem validated_histories_preserve_exact_suffix : forall lookup births index events before after,
  validated_causal_history lookup births index events before after ->
  forall id cells, before id = Some cells ->
    after id = Some (skipn (selected_uses id events) cells) /\
    selected_uses id events <= length cells.
Proof.
  intros lookup births index events before after [_ Hhistory] id cells Hbefore.
  eapply causal_histories_preserve_exact_suffix; eauto.
Qed.

End CausalStackConsumption.
