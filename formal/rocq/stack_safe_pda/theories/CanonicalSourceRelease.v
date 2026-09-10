From Stdlib Require Import Bool List Arith Lia Wf_nat.
Import ListNotations.

Module CanonicalSourceRelease.

Inductive source_lease :=
| NoLease
| Borrowed (source : nat)
| Owned (source : nat).

Record node := Node {
  rank : nat;
  children : list nat;
  source : source_lease
}.

Definition slot_children (entry : option node) :=
  match entry with Some value => children value | None => [] end.

Definition lease_tokens lease :=
  match lease with Owned id => [id] | _ => [] end.

Definition slot_leases (entry : option node) :=
  match entry with Some value => lease_tokens (source value) | None => [] end.

Definition heap_children heap := flat_map slot_children heap.
Definition heap_leases heap := flat_map slot_leases heap.
Definition slot (heap : list (option node)) id := nth id heap None.
Definition occurrences id values := count_occ Nat.eq_dec values id.

Fixpoint clear_slot id (heap : list (option node)) :=
  match id, heap with
  | _, [] => []
  | 0, _ :: rest => None :: rest
  | S next, entry :: rest => entry :: clear_slot next rest
  end.

Lemma occurrences_app id left right :
  occurrences id (left ++ right) = occurrences id left + occurrences id right.
Proof. apply count_occ_app. Qed.

Lemma occurrences_cons_same id rest :
  occurrences id (id :: rest) = S (occurrences id rest).
Proof. unfold occurrences. cbn. destruct (Nat.eq_dec id id); congruence. Qed.

Lemma occurrences_cons_other id other rest :
  id <> other -> occurrences id (other :: rest) = occurrences id rest.
Proof.
  intro different. unfold occurrences. cbn.
  destruct (Nat.eq_dec other id); congruence.
Qed.

Theorem clearing_a_node_empties_exactly_its_original_slot heap id :
  slot (clear_slot id heap) id = None.
Proof.
  revert id. induction heap as [|entry rest IH]; intros [|id]; cbn [slot clear_slot nth]; auto.
  exact (IH id).
Qed.

Theorem clearing_a_node_preserves_every_other_complete_node heap id other :
  id <> other -> slot (clear_slot id heap) other = slot heap other.
Proof.
  revert id other.
  induction heap as [|entry rest IH]; intros [|id] [|other] different;
    cbn [slot clear_slot nth]; try reflexivity; try congruence.
  apply IH. congruence.
Qed.

Lemma clearing_preserves_heap_length heap id :
  length (clear_slot id heap) = length heap.
Proof.
  revert id. induction heap as [|entry rest IH]; intros [|id]; cbn [clear_slot length]; auto.
Qed.

Lemma clear_slot_tokens (tokens : option node -> list nat) heap id target :
  tokens None = [] ->
  occurrences target (flat_map tokens heap) =
    occurrences target (tokens (slot heap id)) +
    occurrences target (flat_map tokens (clear_slot id heap)).
Proof.
  intro absent. revert id.
  induction heap as [|entry rest IH]; intros [|id]; cbn [slot clear_slot nth flat_map].
  - rewrite absent. reflexivity.
  - rewrite absent. reflexivity.
  - rewrite absent. cbn. apply occurrences_app.
  - rewrite !occurrences_app. specialize (IH id). unfold slot in IH. lia.
Qed.

Inductive phase := Ready | Extracted | Evacuated.

Record state := State {
  heap : list (option node);
  external_nodes : list nat;
  external_sources : list nat;
  pending : list nat;
  local_shell : option node;
  staged_children : list nat;
  staged_source : source_lease;
  control : phase;
  claimed_nodes : list nat;
  released_leases : list nat
}.

Definition node_refs current id :=
  occurrences id (external_nodes current) +
  occurrences id (pending current) +
  occurrences id (slot_children (local_shell current)) +
  occurrences id (staged_children current) +
  occurrences id (heap_children (heap current)).

Definition source_refs current id :=
  occurrences id (external_sources current) +
  occurrences id (slot_leases (local_shell current)) +
  occurrences id (lease_tokens (staged_source current)) +
  occurrences id (heap_leases (heap current)).

Definition idle current :=
  control current = Ready /\ local_shell current = None /\
  staged_children current = [] /\ staged_source current = NoLease.

Inductive event :=
| NonfinalExtraction
| FinalExtraction (id : nat)
| DetachPayload
| EnqueueChild (id : nat)
| ReleaseOwnedLease (id : nat)
| ReleaseBorrowedLease
| DropEmptyShell.

Definition extract current :=
  match control current, pending current with
  | Ready, id :: rest =>
      match slot (heap current) id with
      | None => None
      | Some value =>
          if node_refs current id =? 1 then
            Some (FinalExtraction id,
              State (clear_slot id (heap current))
                (external_nodes current) (external_sources current) rest
                (Some value) [] NoLease Extracted
                (id :: claimed_nodes current) (released_leases current))
          else
            Some (NonfinalExtraction,
              State (heap current) (external_nodes current) (external_sources current)
                rest None [] NoLease Ready (claimed_nodes current) (released_leases current))
      end
  | _, _ => None
  end.

Definition detach current :=
  match control current, local_shell current with
  | Extracted, Some value =>
      Some (DetachPayload,
        State (heap current) (external_nodes current) (external_sources current)
          (pending current) None (children value) (source value) Evacuated
          (claimed_nodes current) (released_leases current))
  | _, _ => None
  end.

Definition enqueue current :=
  match control current, staged_children current with
  | Evacuated, id :: rest =>
      Some (EnqueueChild id,
        State (heap current) (external_nodes current) (external_sources current)
          (id :: pending current) (local_shell current) rest (staged_source current)
          Evacuated (claimed_nodes current) (released_leases current))
  | _, _ => None
  end.

Definition release_source current :=
  match control current, staged_source current with
  | Evacuated, Owned id =>
      Some (ReleaseOwnedLease id,
        State (heap current) (external_nodes current) (external_sources current)
          (pending current) (local_shell current) (staged_children current) NoLease
          Evacuated (claimed_nodes current) (id :: released_leases current))
  | Evacuated, Borrowed _ =>
      Some (ReleaseBorrowedLease,
        State (heap current) (external_nodes current) (external_sources current)
          (pending current) (local_shell current) (staged_children current) NoLease
          Evacuated (claimed_nodes current) (released_leases current))
  | _, _ => None
  end.

Definition drop_empty current :=
  match control current, local_shell current, staged_children current, staged_source current with
  | Evacuated, None, [], NoLease =>
      Some (DropEmptyShell,
        State (heap current) (external_nodes current) (external_sources current)
          (pending current) None [] NoLease Ready
          (claimed_nodes current) (released_leases current))
  | _, _, _, _ => None
  end.

Definition step current :=
  match control current with
  | Ready => extract current
  | Extracted => detach current
  | Evacuated =>
      match staged_children current with
      | _ :: _ => enqueue current
      | [] =>
          match staged_source current with
          | NoLease => drop_empty current
          | _ => release_source current
          end
      end
  end.

Theorem extraction_consumes_exactly_one_reference_and_preserves_all_other_counts
    current observed next id rest target :
  idle current -> pending current = id :: rest ->
  extract current = Some (observed, next) ->
  node_refs current target =
    node_refs next target + if Nat.eq_dec id target then 1 else 0.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  cbn [idle pending control local_shell staged_children staged_source].
  intros (ready & empty & no_staged & no_lease) work_is stepped.
  cbn in ready, empty, no_staged, no_lease.
  subst pc held staged lease work.
  cbn [extract control pending heap local_shell staged_children staged_source node_refs] in stepped |- *.
  destruct (slot storage id) as [value|] eqn:found; [|discriminate].
  destruct (_ =? 1) eqn:final; inversion stepped; subst next observed; clear stepped.
  - cbn [node_refs heap external_nodes pending local_shell staged_children slot_children].
    pose proof (clear_slot_tokens slot_children storage id target eq_refl) as kept.
    unfold heap_children in *. rewrite found in kept. cbn [slot_children] in kept.
    unfold occurrences in *. cbn in *.
    destruct (Nat.eq_dec id target); lia.
  - cbn [node_refs heap external_nodes pending local_shell staged_children slot_children].
    unfold occurrences. cbn. destruct (Nat.eq_dec id target); lia.
Qed.

Theorem final_extraction_keeps_child_occurrences_in_the_full_local_shell
    current id next :
  idle current -> extract current = Some (FinalExtraction id, next) ->
  exists value,
    slot (heap current) id = Some value /\
    local_shell next = Some value /\
    slot (heap next) id = None /\
    staged_children next = [] /\ control next = Extracted.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  cbn [idle control local_shell staged_children staged_source].
  intros (ready & empty & no_staged & no_lease) stepped.
  cbn in ready, empty, no_staged, no_lease.
  subst pc held staged lease. destruct work as [|selected rest]; [discriminate|].
  cbn [extract control pending heap node_refs] in stepped.
  destruct (slot storage selected) as [value|] eqn:found; [|discriminate].
  destruct (_ =? 1); inversion stepped; subst; clear stepped.
  exists value. cbn [heap local_shell staged_children control].
  repeat split; auto using clearing_a_node_empties_exactly_its_original_slot.
Qed.

Theorem nonfinal_extraction_keeps_the_heap_external_owners_and_release_ledgers current next :
  extract current = Some (NonfinalExtraction, next) ->
  heap next = heap current /\
  external_nodes next = external_nodes current /\
  external_sources next = external_sources current /\
  claimed_nodes next = claimed_nodes current /\
  released_leases next = released_leases current.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; destruct work as [|id rest]; cbn [extract control pending heap]; try discriminate.
  destruct (slot storage id); [|discriminate].
  destruct (_ =? 1); intro same; inversion same; repeat split; reflexivity.
Qed.

Theorem detaching_moves_every_owned_child_without_changing_reference_counts
    current observed next target :
  staged_children current = [] -> staged_source current = NoLease ->
  detach current = Some (observed, next) ->
  node_refs next target = node_refs current target /\
  source_refs next target = source_refs current target /\ local_shell next = None.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  cbn [staged_children staged_source]. intros no_staged no_lease; subst staged lease.
  destruct pc; destruct held as [value|]; cbn [detach control local_shell]; try discriminate.
  intro same; inversion same; subst; clear same.
  cbn [node_refs source_refs external_nodes external_sources pending local_shell staged_children staged_source heap slot_children slot_leases lease_tokens].
  cbn.
  unfold occurrences. repeat split; lia.
Qed.

Theorem enqueueing_transfers_one_occurrence_without_changing_ownership
    current observed next target :
  enqueue current = Some (observed, next) ->
  node_refs next target = node_refs current target /\
  source_refs next target = source_refs current target.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; destruct staged as [|id rest]; cbn [enqueue control staged_children]; try discriminate.
  intro same; inversion same; subst; clear same.
  cbn [node_refs source_refs external_nodes external_sources pending local_shell staged_children staged_source heap].
  unfold occurrences. cbn. destruct (Nat.eq_dec id target); split; lia.
Qed.

Theorem releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease
    current observed next target :
  release_source current = Some (observed, next) ->
  node_refs next target = node_refs current target /\
  source_refs next target + occurrences target (released_leases next) =
    source_refs current target + occurrences target (released_leases current).
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; destruct lease; cbn [release_source control staged_source]; try discriminate;
    intro same; inversion same; subst; clear same;
    cbn [node_refs source_refs external_nodes external_sources pending local_shell staged_children staged_source heap released_leases lease_tokens];
    unfold occurrences; cbn; try (split; lia).
  destruct (Nat.eq_dec source0 target); split; lia.
Qed.

Theorem empty_shell_drop_is_work_and_ownership_neutral current observed next target :
  drop_empty current = Some (observed, next) ->
  node_refs next target = node_refs current target /\
  source_refs next target = source_refs current target /\
  pending next = pending current /\
  heap next = heap current /\
  claimed_nodes next = claimed_nodes current /\
  released_leases next = released_leases current /\ idle next.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; destruct held; destruct staged; destruct lease;
    cbn [drop_empty control local_shell staged_children staged_source]; try discriminate;
    intro same; inversion same; subst; repeat split; reflexivity.
Qed.

Theorem a_surviving_external_reader_keeps_its_source_reference_count_positive current owner :
  In owner (external_sources current) -> source_refs current owner > 0.
Proof.
  intro present. unfold source_refs, occurrences.
  apply (proj1 (count_occ_In Nat.eq_dec _ _)) in present. lia.
Qed.

Theorem a_cleared_node_cannot_be_extracted_again current id rest :
  pending current = id :: rest -> slot (heap current) id = None ->
  extract current = None.
Proof.
  intros queued empty. unfold extract. rewrite queued, empty.
  destruct (control current); reflexivity.
Qed.

Definition phase_valid current :=
  match control current with
  | Ready =>
      local_shell current = None /\ staged_children current = [] /\
      staged_source current = NoLease
  | Extracted =>
      (exists value, local_shell current = Some value) /\
      staged_children current = [] /\ staged_source current = NoLease
  | Evacuated => local_shell current = None
  end.

Theorem every_successful_step_preserves_the_local_ownership_phase current observed next :
  phase_valid current -> step current = Some (observed, next) -> phase_valid next.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; unfold phase_valid; cbn.
  - intros (empty & no_staged & no_lease) stepped. subst held staged lease.
    destruct work as [|id rest]; [cbn [step control extract pending] in stepped; discriminate|].
    cbn [step control extract pending heap node_refs] in stepped.
    destruct (nth id storage None) as [value|]; [|discriminate].
    destruct (_ =? 1); inversion stepped; subst; cbn; eauto.
  - intros ((value & full) & no_staged & no_lease) stepped.
    subst held staged lease. inversion stepped; subst; reflexivity.
  - intros empty stepped. subst held.
    destruct staged; destruct lease;
      cbn [step control staged_children staged_source enqueue release_source drop_empty local_shell] in stepped;
      inversion stepped; subst; cbn; repeat split; reflexivity.
Qed.

Theorem extraction_preserves_every_original_source_lease current observed next target :
  idle current -> extract current = Some (observed, next) ->
  source_refs next target = source_refs current target.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  unfold idle; cbn. intros (ready & empty & no_staged & no_lease) stepped.
  subst pc held staged lease. destruct work as [|id rest]; [discriminate|].
  cbn [extract control pending heap node_refs] in stepped.
  destruct (slot storage id) as [value|] eqn:found; [|discriminate].
  destruct (_ =? 1); inversion stepped; subst; clear stepped.
  - unfold source_refs. cbn.
    pose proof (clear_slot_tokens slot_leases storage id target eq_refl) as kept.
    unfold heap_leases in *. rewrite found in kept. cbn [slot_leases] in kept.
    unfold occurrences in *. cbn in *. lia.
  - reflexivity.
Qed.

Definition backing_disposal_admissible current owner := source_refs current owner = 0.

Theorem no_backing_disposal_is_admissible_while_an_external_reader_survives current owner :
  In owner (external_sources current) -> ~ backing_disposal_admissible current owner.
Proof.
  intro reader.
  pose proof (a_surviving_external_reader_keeps_its_source_reference_count_positive current owner reader).
  unfold backing_disposal_admissible. lia.
Qed.

Definition staging_size current :=
  length (slot_children (local_shell current)) +
  length (staged_children current) + length (pending current).

Theorem detaching_preserves_the_combined_local_and_queued_frontier current observed next :
  staged_children current = [] -> staged_source current = NoLease ->
  detach current = Some (observed, next) ->
  staging_size next = staging_size current.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  cbn. intros no_staged no_lease; subst staged lease.
  destruct pc; destruct held; cbn [detach control local_shell]; try discriminate;
    intro same; inversion same; subst; unfold staging_size; cbn; lia.
Qed.

Theorem enqueueing_preserves_the_combined_local_and_queued_frontier current observed next :
  enqueue current = Some (observed, next) -> staging_size next = staging_size current.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; destruct staged; cbn [enqueue control staged_children]; try discriminate;
    intro same; inversion same; subst; unfold staging_size; cbn; lia.
Qed.

Fixpoint staged_push (held work : list nat) :=
  match held with [] => work | id :: rest => staged_push rest (id :: work) end.

Theorem completed_child_staging_is_exactly_reverse_children_followed_by_old_work held work :
  staged_push held work = rev held ++ work.
Proof.
  revert work. induction held as [|id rest IH]; intro work; cbn [staged_push rev].
  - reflexivity.
  - rewrite IH, <- app_assoc. reflexivity.
Qed.

Section RankedFrontier.

Variable original_rank : nat -> nat.

Inductive frontier (upper : nat) : list nat -> Prop :=
| FrontierEmpty : frontier upper []
| FrontierNext id rest :
    original_rank id <= upper ->
    length rest <= 2 * (upper - original_rank id) ->
    frontier upper rest ->
    frontier upper (id :: rest).

Theorem each_ranked_lifo_frontier_has_at_most_one_plus_twice_root_height upper work :
  frontier upper work -> length work <= 1 + 2 * upper.
Proof.
  intro valid. inversion valid; subst; cbn [length]; lia.
Qed.

Lemma ranked_children_fit_before_the_saved_siblings upper parent kids rest :
  original_rank parent <= upper ->
  length rest <= 2 * (upper - original_rank parent) ->
  length kids <= 3 ->
  Forall (fun child => original_rank child < original_rank parent) kids ->
  frontier upper rest ->
  frontier upper (kids ++ rest).
Proof.
  intros parent_bound rest_bound child_count ranked saved.
  induction ranked as [|child tail smaller ranked IH]; cbn [app].
  - exact saved.
  - constructor.
    + lia.
    + rewrite length_app. cbn [length] in child_count. lia.
    + apply IH. cbn [length] in child_count. lia.
Qed.

Theorem final_lifo_extraction_preserves_the_ranked_frontier upper parent rest kids :
  frontier upper (parent :: rest) ->
  length kids <= 3 ->
  Forall (fun child => original_rank child < original_rank parent) kids ->
  frontier upper (rev kids ++ rest).
Proof.
  intros valid count ranked. inversion valid; subst.
  apply ranked_children_fit_before_the_saved_siblings with (parent := parent); try assumption.
  - now rewrite length_rev.
  - now apply Forall_rev.
Qed.

Theorem nonfinal_lifo_extraction_preserves_the_ranked_frontier upper parent rest :
  frontier upper (parent :: rest) -> frontier upper rest.
Proof. intro valid. inversion valid; assumption. Qed.

Fixpoint rank_weight depth :=
  match depth with 0 => 1 | S previous => 4 * rank_weight previous end.

Lemma rank_weight_positive depth : 0 < rank_weight depth.
Proof. induction depth; cbn [rank_weight]; lia. Qed.

Lemma rank_weight_monotone left right :
  left <= right -> rank_weight left <= rank_weight right.
Proof.
  revert left. induction right as [|right IH]; intros [|left] ordered;
    cbn [rank_weight]; try lia.
  - pose proof (rank_weight_positive right). lia.
  - specialize (IH left ltac:(lia)). lia.
Qed.

Definition debt work :=
  fold_right (fun id total => rank_weight (original_rank id) + total) 0 work.

Lemma debt_app first last : debt (first ++ last) = debt first + debt last.
Proof.
  unfold debt. induction first as [|id rest IH]; cbn; [reflexivity|].
  rewrite IH. lia.
Qed.

Lemma debt_rev work : debt (rev work) = debt work.
Proof.
  induction work as [|id rest IH]; cbn [rev]; [reflexivity|].
  rewrite debt_app, IH. unfold debt. cbn. lia.
Qed.

Lemma debt_bounded_by_common_rank kids bound :
  Forall (fun id => original_rank id <= bound) kids ->
  debt kids <= length kids * rank_weight bound.
Proof.
  intro valid. induction valid as [|id rest bounded valid IH].
  - cbn [debt length fold_right]. lia.
  - pose proof (rank_weight_monotone _ _ bounded).
    unfold debt in *. cbn in *. nia.
Qed.

Lemma fewer_lower_rank_children_have_strictly_less_debt parent kids :
  length kids <= 3 ->
  Forall (fun child => original_rank child < original_rank parent) kids ->
  debt kids < rank_weight (original_rank parent).
Proof.
  intros count ranked. destruct (original_rank parent) as [|previous] eqn:parent_rank.
  - destruct kids as [|child rest]; cbn [debt fold_right rank_weight]; [lia|].
    inversion ranked; lia.
  - assert (lower : Forall (fun child => original_rank child <= previous) kids).
    { eapply Forall_impl; [|exact ranked]. cbn. intros child smaller. lia. }
    pose proof (debt_bounded_by_common_rank kids previous lower).
    pose proof (rank_weight_positive previous).
    cbn [rank_weight]. nia.
Qed.

Theorem every_final_extraction_strictly_decreases_rank_debt parent rest kids :
  length kids <= 3 ->
  Forall (fun child => original_rank child < original_rank parent) kids ->
  debt (rev kids ++ rest) < debt (parent :: rest).
Proof.
  intros count ranked. rewrite debt_app, debt_rev.
  pose proof (fewer_lower_rank_children_have_strictly_less_debt parent kids count ranked).
  unfold debt in *. cbn in *. lia.
Qed.

Theorem every_nonfinal_extraction_strictly_decreases_rank_debt parent rest :
  debt rest < debt (parent :: rest).
Proof.
  pose proof (rank_weight_positive (original_rank parent)).
  unfold debt. cbn. lia.
Qed.

End RankedFrontier.

Theorem a_successful_step_only_preserves_storage_or_claims_one_live_slot current observed next :
  step current = Some (observed, next) ->
  (heap next = heap current /\ claimed_nodes next = claimed_nodes current) \/
  exists id value,
    slot (heap current) id = Some value /\
    heap next = clear_slot id (heap current) /\
    claimed_nodes next = id :: claimed_nodes current.
Proof.
  destruct current as [storage outside readers work held staged lease pc disposed leases].
  destruct pc; cbn [step control] in *.
  - destruct work as [|id rest]; cbn [extract control pending heap] in *; try discriminate.
    destruct (slot storage id) as [value|] eqn:found; try discriminate.
    destruct (_ =? 1); intro same; inversion same; subst; cbn.
    + right. exists id, value. auto.
    + left. auto.
  - destruct held; cbn [detach control local_shell] in *; try discriminate.
    intro same; inversion same; subst. left. auto.
  - destruct staged; destruct lease;
      cbn [staged_children staged_source enqueue release_source drop_empty control local_shell] in *.
    + destruct held; try discriminate; intro same; inversion same; subst; left; auto.
    + intro same; inversion same; subst; left; auto.
    + intro same; inversion same; subst; left; auto.
    + intro same; inversion same; subst; left; auto.
    + intro same; inversion same; subst; left; auto.
    + intro same; inversion same; subst; left; auto.
Qed.

Theorem every_successful_step_preserves_all_previously_cleared_slots current observed next id :
  step current = Some (observed, next) ->
  slot (heap current) id = None -> slot (heap next) id = None.
Proof.
  intros stepped empty.
  destruct (a_successful_step_only_preserves_storage_or_claims_one_live_slot
    current observed next stepped) as [(same & ledger)|(claimed & value & live & changed & ledger)].
  - now rewrite same.
  - rewrite changed. destruct (Nat.eq_dec claimed id) as [same|different].
    + subst. apply clearing_a_node_empties_exactly_its_original_slot.
    + now rewrite clearing_a_node_preserves_every_other_complete_node.
Qed.

Definition claims_valid current :=
  NoDup (claimed_nodes current) /\
  Forall (fun id => slot (heap current) id = None) (claimed_nodes current).

Theorem every_successful_step_preserves_unique_claims_and_their_empty_slots current observed next :
  claims_valid current -> step current = Some (observed, next) -> claims_valid next.
Proof.
  intros (unique & empty) stepped.
  pose proof (every_successful_step_preserves_all_previously_cleared_slots
    current observed next) as retained.
  destruct (a_successful_step_only_preserves_storage_or_claims_one_live_slot
    current observed next stepped) as [(same & ledger)|(claimed & value & live & changed & ledger)].
  - unfold claims_valid. rewrite ledger, same. auto.
  - unfold claims_valid. rewrite ledger. split.
    + constructor; [|exact unique]. intro present.
      apply Forall_forall with (x := claimed) in empty; [congruence|exact present].
    + constructor.
      * rewrite changed. apply clearing_a_node_empties_exactly_its_original_slot.
      * eapply Forall_impl; [|exact empty]. cbn. intros id cleared.
        exact (retained id stepped cleared).
Qed.

Inductive execution : state -> list event -> state -> Prop :=
| ExecutionEmpty current : execution current [] current
| ExecutionStep current observed middle events final :
    step current = Some (observed, middle) ->
    execution middle events final ->
    execution current (observed :: events) final.

Theorem every_finite_release_trace_claims_each_node_at_most_once initial events final :
  claims_valid initial -> execution initial events final -> claims_valid final.
Proof.
  intros valid run. induction run; [exact valid|].
  apply IHrun. eapply every_successful_step_preserves_unique_claims_and_their_empty_slots; eauto.
Qed.

Definition references_valid current :=
  forall id, node_refs current id > 0 -> exists value, slot (heap current) id = Some value.

Theorem extraction_preserves_all_live_node_references current observed next :
  idle current -> references_valid current ->
  extract current = Some (observed, next) -> references_valid next.
Proof.
  intros ready valid stepped.
  destruct (pending current) as [|id rest] eqn:work.
  { unfold extract in stepped. rewrite work in stepped.
    destruct (control current); discriminate. }
  pose proof (fun target =>
    extraction_consumes_exactly_one_reference_and_preserves_all_other_counts
      current observed next id rest target ready work stepped) as consumed.
  unfold extract in stepped.
  destruct (control current) eqn:pc; try discriminate.
  rewrite work in stepped.
  destruct (slot (heap current) id) as [value|] eqn:live; try discriminate.
  destruct (node_refs current id =? 1) eqn:last; inversion stepped; subst; clear stepped.
  - apply Nat.eqb_eq in last. intros target positive.
    destruct (Nat.eq_dec id target) as [same|different].
    + subst target. specialize (consumed id).
      destruct (Nat.eq_dec id id); lia.
    + specialize (consumed target).
      destruct (Nat.eq_dec id target); [contradiction|].
      destruct (valid target ltac:(lia)) as [other present].
      exists other. cbn [heap].
      now rewrite clearing_a_node_preserves_every_other_complete_node.
  - intros target positive. specialize (consumed target).
    destruct (valid target ltac:(lia)) as [other present]. exists other. exact present.
Qed.

Theorem every_successful_step_preserves_all_live_node_references current observed next :
  phase_valid current -> references_valid current ->
  step current = Some (observed, next) -> references_valid next.
Proof.
  intros phase_ok valid stepped.
  unfold step in stepped.
  destruct (control current) eqn:pc.
  - eapply extraction_preserves_all_live_node_references; [|exact valid|exact stepped].
    unfold phase_valid in phase_ok. rewrite pc in phase_ok.
    unfold idle. tauto.
  - unfold phase_valid in phase_ok. rewrite pc in phase_ok.
    destruct phase_ok as (held & staged & lease).
    intros target positive.
    pose proof (detaching_moves_every_owned_child_without_changing_reference_counts
      current observed next target staged lease stepped) as (same & _).
    destruct (valid target ltac:(lia)) as [value live].
    exists value.
    unfold detach in stepped. rewrite pc in stepped.
    destruct (local_shell current); inversion stepped; subst; exact live.
  - destruct (staged_children current) eqn:staged.
    + destruct (staged_source current) eqn:lease.
      * intros target positive.
        pose proof (empty_shell_drop_is_work_and_ownership_neutral
          current observed next target stepped) as (same & sources & work & storage & _).
        rewrite storage. apply valid. lia.
      * intros target positive.
        pose proof (releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease
          current observed next target stepped) as (same & _).
        destruct (valid target ltac:(lia)) as [value live]. exists value.
        unfold release_source in stepped. rewrite pc, lease in stepped.
        inversion stepped; subst; exact live.
      * intros target positive.
        pose proof (releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease
          current observed next target stepped) as (same & _).
        destruct (valid target ltac:(lia)) as [value live]. exists value.
        unfold release_source in stepped. rewrite pc, lease in stepped.
        inversion stepped; subst; exact live.
    + intros target positive.
      pose proof (enqueueing_transfers_one_occurrence_without_changing_ownership
        current observed next target stepped) as (same & _).
      destruct (valid target ltac:(lia)) as [value live]. exists value.
      unfold enqueue in stepped. rewrite pc, staged in stepped.
      inversion stepped; subst; exact live.
Qed.

Theorem every_finite_release_trace_preserves_live_reference_validity initial events final :
  phase_valid initial -> references_valid initial ->
  execution initial events final -> phase_valid final /\ references_valid final.
Proof.
  intros phase_ok valid run. induction run.
  - auto.
  - apply IHrun.
    + eapply every_successful_step_preserves_the_local_ownership_phase; eauto.
    + eapply every_successful_step_preserves_all_live_node_references; eauto.
Qed.

Theorem every_successful_step_conserves_owned_source_leases_with_release_occurrences
    current observed next owner :
  phase_valid current -> step current = Some (observed, next) ->
  source_refs next owner + occurrences owner (released_leases next) =
    source_refs current owner + occurrences owner (released_leases current).
Proof.
  intros phase_ok stepped. unfold step in stepped.
  destruct (control current) eqn:pc.
  - assert (ready : idle current).
    { unfold phase_valid in phase_ok. rewrite pc in phase_ok. unfold idle. tauto. }
    rewrite (extraction_preserves_every_original_source_lease current observed next owner ready stepped).
    assert (ledger : released_leases next = released_leases current).
    { unfold extract in stepped. rewrite pc in stepped.
      destruct (pending current) as [|id rest]; [discriminate|].
      destruct (slot (heap current) id); [|discriminate].
      destruct (_ =? 1); inversion stepped; reflexivity. }
    now rewrite ledger.
  - unfold phase_valid in phase_ok. rewrite pc in phase_ok.
    destruct phase_ok as (held & staged & lease).
    pose proof (detaching_moves_every_owned_child_without_changing_reference_counts
      current observed next owner staged lease stepped) as (_ & same & _).
    rewrite same.
    unfold detach in stepped. rewrite pc in stepped.
    destruct (local_shell current); inversion stepped; reflexivity.
  - destruct (staged_children current) eqn:staged.
    + destruct (staged_source current) eqn:lease.
      * pose proof (empty_shell_drop_is_work_and_ownership_neutral
          current observed next owner stepped) as (_ & same & work & storage & claims & ledger & _).
        now rewrite same, ledger.
      * exact (proj2 (releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease
          current observed next owner stepped)).
      * exact (proj2 (releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease
          current observed next owner stepped)).
    + pose proof (enqueueing_transfers_one_occurrence_without_changing_ownership
        current observed next owner stepped) as (_ & same).
      rewrite same. unfold enqueue in stepped. rewrite pc, staged in stepped.
      inversion stepped; reflexivity.
Qed.

Theorem every_finite_release_trace_accounts_for_every_owned_source_lease
    initial events final owner :
  phase_valid initial -> execution initial events final ->
  source_refs final owner + occurrences owner (released_leases final) =
    source_refs initial owner + occurrences owner (released_leases initial).
Proof.
  intros phase_ok run. induction run; [reflexivity|].
  rewrite IHrun.
  - eapply every_successful_step_conserves_owned_source_leases_with_release_occurrences; eauto.
  - eapply every_successful_step_preserves_the_local_ownership_phase; eauto.
Qed.

Definition realized_frontier current :=
  rev (slot_children (local_shell current)) ++
  rev (staged_children current) ++ pending current.

Theorem realized_frontier_counts_every_local_staged_and_pending_occurrence current :
  length (realized_frontier current) = staging_size current.
Proof.
  unfold realized_frontier, staging_size.
  rewrite !length_app, !length_rev. lia.
Qed.

Definition retained_payloads original current :=
  (forall id value, slot (heap current) id = Some value ->
    slot original id = Some value) /\
  (forall value, local_shell current = Some value ->
    exists id, slot original id = Some value).

Definition original_graph_ranked original original_rank :=
  forall id value, slot original id = Some value ->
    rank value = original_rank id /\ length (children value) <= 3 /\
    Forall (fun child => original_rank child < original_rank id) (children value).

Lemma successful_step_preserves_original_heap_payloads original current observed next :
  (forall id value, slot (heap current) id = Some value -> slot original id = Some value) ->
  step current = Some (observed, next) ->
  forall id value, slot (heap next) id = Some value -> slot original id = Some value.
Proof.
  intros retained stepped id value live.
  destruct (a_successful_step_only_preserves_storage_or_claims_one_live_slot
    current observed next stepped) as [(same & _)|(claimed & old & was_live & changed & _) ].
  - apply retained. now rewrite same in live.
  - rewrite changed in live. destruct (Nat.eq_dec claimed id) as [same|different].
    + subst id. rewrite clearing_a_node_empties_exactly_its_original_slot in live. discriminate.
    + apply retained. now rewrite clearing_a_node_preserves_every_other_complete_node in live.
Qed.

Theorem every_successful_step_retains_exact_original_heap_and_shell_payloads
    original current observed next :
  phase_valid current -> retained_payloads original current ->
  step current = Some (observed, next) -> retained_payloads original next.
Proof.
  intros phase_ok (stored & held) stepped. split.
  - eapply successful_step_preserves_original_heap_payloads; eauto.
  - destruct current as [storage outside readers work shell staged lease pc claims leases].
    destruct pc; cbn [phase_valid control local_shell staged_children staged_source] in phase_ok.
    + destruct phase_ok as (empty & no_staged & no_lease). subst shell staged lease.
      destruct work as [|id rest]; [discriminate|].
      cbn [step control extract pending heap] in stepped.
      destruct (slot storage id) as [value|] eqn:found; [|discriminate].
      destruct (_ =? 1); inversion stepped; subst; cbn [local_shell]; intros other same;
        try discriminate.
      inversion same; subst other. exists id. apply stored. exact found.
    + destruct phase_ok as ((value & full) & no_staged & no_lease).
      subst shell staged lease. inversion stepped; subst; cbn [local_shell]. discriminate.
    + subst shell. destruct staged; destruct lease;
        cbn [step control staged_children staged_source enqueue release_source drop_empty local_shell] in stepped;
        inversion stepped; subst; cbn [local_shell]; discriminate.
Qed.

Theorem original_heap_ranks_and_fanout_apply_to_every_retained_live_node
    original original_rank current id value :
  original_graph_ranked original original_rank -> retained_payloads original current ->
  slot (heap current) id = Some value ->
  rank value = original_rank id /\ length (children value) <= 3 /\
  Forall (fun child => original_rank child < original_rank id) (children value).
Proof. intros ranked (stored & held) live. apply ranked. now apply stored. Qed.

Definition lease_work lease := match lease with NoLease => 0 | _ => 1 end.

Definition phase_work current :=
  match control current with
  | Ready => 0
  | Extracted =>
      length (slot_children (local_shell current)) + 2 +
      match local_shell current with Some value => lease_work (source value) | None => 0 end
  | Evacuated => length (staged_children current) + 1 + lease_work (staged_source current)
  end.

Theorem actual_final_extraction_replaces_the_realized_head_with_exact_reversed_children
    current id next :
  phase_valid current -> step current = Some (FinalExtraction id, next) ->
  exists value rest,
    slot (heap current) id = Some value /\
    realized_frontier current = id :: rest /\
    realized_frontier next = rev (children value) ++ rest /\
    phase_work current = 0 /\
    phase_work next = length (children value) + 2 + lease_work (source value).
Proof.
  destruct current as [storage outside readers work shell staged lease pc claims leases].
  destruct pc; cbn [phase_valid control local_shell staged_children staged_source].
  - intros (empty & no_staged & no_lease) stepped. subst shell staged lease.
    destruct work as [|selected rest]; [discriminate|].
    cbn [step control extract pending heap] in stepped.
    destruct (slot storage selected) as [value|] eqn:found; [|discriminate].
    destruct (_ =? 1); inversion stepped; subst; clear stepped.
    exists value, rest. cbn [heap realized_frontier phase_work local_shell staged_children pending control slot_children rev app].
    repeat split; auto.
  - intros ((value & full) & no_staged & no_lease) stepped.
    subst shell staged lease. discriminate.
  - intros empty stepped. subst shell. destruct staged; destruct lease; discriminate.
Qed.

Theorem actual_nonfinal_extraction_removes_only_the_realized_head current next :
  phase_valid current -> step current = Some (NonfinalExtraction, next) ->
  exists id rest, realized_frontier current = id :: rest /\
    realized_frontier next = rest /\ phase_work current = 0 /\ phase_work next = 0.
Proof.
  destruct current as [storage outside readers work shell staged lease pc claims leases].
  destruct pc; cbn [phase_valid control local_shell staged_children staged_source].
  - intros (empty & no_staged & no_lease) stepped. subst shell staged lease.
    destruct work as [|id rest]; [discriminate|].
    cbn [step control extract pending heap] in stepped.
    destruct (slot storage id) as [value|]; [|discriminate].
    destruct (_ =? 1); inversion stepped; subst; clear stepped.
    exists id, rest. repeat split; reflexivity.
  - intros ((value & full) & no_staged & no_lease) stepped.
    subst shell staged lease. discriminate.
  - intros empty stepped. subst shell. destruct staged; destruct lease; discriminate.
Qed.

Definition staging_event observed :=
  match observed with NonfinalExtraction | FinalExtraction _ => False | _ => True end.

Theorem every_actual_staging_step_preserves_frontier_order_and_decreases_phase_work
    current observed next :
  phase_valid current -> staging_event observed -> step current = Some (observed, next) ->
  realized_frontier next = realized_frontier current /\ phase_work next < phase_work current.
Proof.
  destruct current as [storage outside readers work shell staged lease pc claims leases].
  destruct pc; cbn [phase_valid control local_shell staged_children staged_source].
  - intros (empty & no_staged & no_lease) stage stepped. subst shell staged lease.
    destruct work as [|id rest]; [discriminate|].
    cbn [step control extract pending heap] in stepped.
    destruct (slot storage id); [|discriminate].
    destruct (_ =? 1); inversion stepped; subst; contradiction.
  - intros ((value & full) & no_staged & no_lease) stage stepped.
    subst shell staged lease. inversion stepped; subst; clear stepped.
    cbn [realized_frontier phase_work local_shell staged_children staged_source pending control slot_children].
    split; [reflexivity|lia].
  - intros empty stage stepped. subst shell. destruct staged as [|id rest].
    + destruct lease;
        cbn [step control staged_children staged_source release_source drop_empty local_shell] in stepped;
        inversion stepped; subst; clear stepped;
        cbn [realized_frontier phase_work local_shell staged_children staged_source pending control slot_children lease_work];
        (split; [reflexivity|lia]).
    + cbn [step control staged_children enqueue] in stepped.
      inversion stepped; subst; clear stepped.
      cbn [realized_frontier phase_work local_shell staged_children staged_source pending control slot_children rev].
      split.
      * change (rev rest ++ id :: work = (rev rest ++ [id]) ++ work).
        now rewrite <- app_assoc.
      * cbn [length]. lia.
Qed.

Theorem every_actual_step_preserves_the_ranked_realized_frontier
    original original_rank upper current observed next :
  original_graph_ranked original original_rank ->
  retained_payloads original current -> phase_valid current ->
  frontier original_rank upper (realized_frontier current) ->
  step current = Some (observed, next) ->
  frontier original_rank upper (realized_frontier next).
Proof.
  intros ranked retained phase_ok bounded stepped.
  destruct observed as [|id| |child|owner| | ].
  { destruct (actual_nonfinal_extraction_removes_only_the_realized_head
      current next phase_ok stepped) as (id & rest & before & after & _).
    rewrite before in bounded. rewrite after.
    now apply nonfinal_lifo_extraction_preserves_the_ranked_frontier with (parent := id). }
  { destruct (actual_final_extraction_replaces_the_realized_head_with_exact_reversed_children
      current id next phase_ok stepped) as (value & rest & live & before & after & _).
    pose proof (original_heap_ranks_and_fanout_apply_to_every_retained_live_node
      original original_rank current id value ranked retained live) as (_ & count & smaller).
    rewrite before in bounded. rewrite after.
    eapply final_lifo_extraction_preserves_the_ranked_frontier; eauto. }
  all: match type of stepped with
    | step _ = Some (?observed, _) =>
      pose proof (every_actual_staging_step_preserves_frontier_order_and_decreases_phase_work
        current observed next phase_ok I stepped) as (same & _)
    end; now rewrite same.
Qed.

Definition drain_measure original_rank current :=
  8 * debt original_rank (realized_frontier current) + phase_work current.

Lemma lease_work_at_most_one lease : lease_work lease <= 1.
Proof. destruct lease; cbn [lease_work]; lia. Qed.

Theorem every_actual_ranked_step_strictly_decreases_the_whole_drain_measure
    original original_rank current observed next :
  original_graph_ranked original original_rank ->
  retained_payloads original current -> phase_valid current ->
  step current = Some (observed, next) ->
  drain_measure original_rank next < drain_measure original_rank current.
Proof.
  intros ranked retained phase_ok stepped. unfold drain_measure.
  destruct observed as [|id| |child|owner| | ].
  { destruct (actual_nonfinal_extraction_removes_only_the_realized_head
      current next phase_ok stepped) as (id & rest & before & after & ready & done).
    rewrite before, after, ready, done.
    pose proof (every_nonfinal_extraction_strictly_decreases_rank_debt original_rank id rest). lia. }
  { destruct (actual_final_extraction_replaces_the_realized_head_with_exact_reversed_children
      current id next phase_ok stepped) as (value & rest & live & before & after & ready & held).
    pose proof (original_heap_ranks_and_fanout_apply_to_every_retained_live_node
      original original_rank current id value ranked retained live) as (_ & count & smaller).
    pose proof (every_final_extraction_strictly_decreases_rank_debt
      original_rank id rest (children value) count smaller).
    pose proof (lease_work_at_most_one (source value)).
    rewrite before, after, ready, held. lia. }
  all: match type of stepped with
    | step _ = Some (?observed, _) =>
      pose proof (every_actual_staging_step_preserves_frontier_order_and_decreases_phase_work
        current observed next phase_ok I stepped) as (same & decreased)
    end; rewrite same; lia.
Qed.

Theorem a_valid_state_stops_exactly_when_idle_and_work_is_empty current :
  phase_valid current -> references_valid current ->
  (step current = None <-> idle current /\ pending current = []).
Proof.
  intros phase_ok references. split.
  - intros stopped. unfold step in stopped.
    destruct (control current) eqn:pc.
    + unfold phase_valid in phase_ok. rewrite pc in phase_ok.
      assert (ready : idle current) by (unfold idle; tauto).
      split; [exact ready|].
      destruct (pending current) as [|id rest] eqn:work; [reflexivity|].
      assert (positive : node_refs current id > 0).
      { unfold node_refs. rewrite work, occurrences_cons_same. lia. }
      destruct (references id positive) as [value live].
      unfold extract in stopped. rewrite pc, work, live in stopped.
      destruct (_ =? 1); discriminate.
    + unfold phase_valid in phase_ok. rewrite pc in phase_ok.
      destruct phase_ok as ((value & full) & _).
      unfold detach in stopped. rewrite pc, full in stopped. discriminate.
    + unfold phase_valid in phase_ok. rewrite pc in phase_ok.
      destruct (staged_children current) as [|id rest] eqn:staged.
      * destruct (staged_source current) eqn:lease.
        -- unfold drop_empty in stopped. rewrite pc, phase_ok, staged, lease in stopped. discriminate.
        -- unfold release_source in stopped. rewrite pc, lease in stopped. discriminate.
        -- unfold release_source in stopped. rewrite pc, lease in stopped. discriminate.
      * unfold enqueue in stopped. rewrite pc, staged in stopped. discriminate.
  - intros ((ready & empty & no_staged & no_lease) & no_work).
    unfold step, extract. now rewrite ready, no_work.
Qed.

Definition drain_valid original current :=
  phase_valid current /\ references_valid current /\ retained_payloads original current.

Lemma successful_step_preserves_drain_valid original current observed next :
  drain_valid original current -> step current = Some (observed, next) ->
  drain_valid original next.
Proof.
  intros (phase_ok & references & retained) stepped. repeat split.
  - eapply every_successful_step_preserves_the_local_ownership_phase; eauto.
  - eapply every_successful_step_preserves_all_live_node_references; eauto.
  - exact (proj1 (every_successful_step_retains_exact_original_heap_and_shell_payloads
      original current observed next phase_ok retained stepped)).
  - exact (proj2 (every_successful_step_retains_exact_original_heap_and_shell_payloads
      original current observed next phase_ok retained stepped)).
Qed.

Theorem every_valid_ranked_drain_reaches_idle_empty_work_without_a_fuel_parameter
    original original_rank :
  original_graph_ranked original original_rank ->
  forall current, drain_valid original current ->
    exists events final, execution current events final /\ idle final /\ pending final = [].
Proof.
  intros ranked.
  refine (well_founded_induction
    (well_founded_ltof state (drain_measure original_rank))
    (fun current => drain_valid original current ->
      exists events final, execution current events final /\ idle final /\ pending final = []) _).
  intros current IH valid.
  destruct (step current) as [[observed next]|] eqn:stepped.
  - assert (smaller : drain_measure original_rank next < drain_measure original_rank current).
    { destruct valid as (phase_ok & references & retained).
      eapply every_actual_ranked_step_strictly_decreases_the_whole_drain_measure; eauto. }
    destruct (IH next smaller (successful_step_preserves_drain_valid
      original current observed next valid stepped)) as (events & final & run & done).
    exists (observed :: events), final. split; [econstructor; eauto|exact done].
  - destruct valid as (phase_ok & references & retained).
    exists [], current. split; [constructor|].
    now apply (proj1 (a_valid_state_stops_exactly_when_idle_and_work_is_empty
      current phase_ok references)).
Qed.

Theorem every_finite_actual_trace_preserves_original_payloads_and_ranked_frontier
    original original_rank upper initial events final :
  original_graph_ranked original original_rank -> drain_valid original initial ->
  frontier original_rank upper (realized_frontier initial) ->
  execution initial events final ->
  drain_valid original final /\ frontier original_rank upper (realized_frontier final).
Proof.
  intros ranked valid bounded run. induction run.
  - auto.
  - apply IHrun.
    + eapply successful_step_preserves_drain_valid; eauto.
    + destruct valid as (phase_ok & references & retained).
      eapply every_actual_step_preserves_the_ranked_realized_frontier; eauto.
Qed.

Theorem every_reachable_single_root_state_bounds_all_local_staged_and_pending_frames
    original original_rank root initial events reached :
  original_graph_ranked original original_rank -> drain_valid original initial ->
  idle initial -> pending initial = [root] -> execution initial events reached ->
  staging_size reached <= 1 + 2 * original_rank root.
Proof.
  intros ranked valid (ready & empty & no_staged & no_lease) root_work run.
  assert (start : realized_frontier initial = [root]).
  { unfold realized_frontier. now rewrite empty, no_staged, root_work. }
  assert (bounded : frontier original_rank (original_rank root) (realized_frontier initial)).
  { rewrite start. constructor; cbn [length]; try lia. constructor. }
  pose proof (every_finite_actual_trace_preserves_original_payloads_and_ranked_frontier
    original original_rank (original_rank root) initial events reached ranked valid bounded run)
    as (_ & final_bound).
  rewrite <- realized_frontier_counts_every_local_staged_and_pending_occurrence.
  exact (each_ranked_lifo_frontier_has_at_most_one_plus_twice_root_height
    original_rank (original_rank root) (realized_frontier reached) final_bound).
Qed.

Theorem every_actual_trace_length_is_bounded_by_the_initial_whole_drain_measure
    original original_rank initial events final :
  original_graph_ranked original original_rank -> drain_valid original initial ->
  execution initial events final ->
  length events + drain_measure original_rank final <= drain_measure original_rank initial.
Proof.
  intros ranked valid run. induction run.
  - cbn [length]. lia.
  - specialize (IHrun (successful_step_preserves_drain_valid
      original current observed middle valid H)).
    destruct valid as (phase_ok & references & retained).
    pose proof (every_actual_ranked_step_strictly_decreases_the_whole_drain_measure
      original original_rank current observed middle ranked retained phase_ok H).
    cbn [length]. lia.
Qed.

Definition repeated_child_original :=
  [Some (Node 0 [] (Owned 7)); Some (Node 1 [0; 0; 0] NoLease)].

Definition repeated_child_ranks id := if id =? 0 then 0 else 1.

Definition repeated_child_initial :=
  State repeated_child_original [] [7] [1] None [] NoLease Ready [] [].

Example repeated_child_control_has_valid_original_ranks_and_initial_ownership :
  original_graph_ranked repeated_child_original repeated_child_ranks /\
  drain_valid repeated_child_original repeated_child_initial.
Proof.
  split.
  - intros [|[|id]] value live.
    + change (Some (Node 0 [] (Owned 7)) = Some value) in live.
      inversion live; subst value. cbn [rank children repeated_child_ranks].
      repeat constructor; lia.
    + change (Some (Node 1 [0; 0; 0] NoLease) = Some value) in live.
      inversion live; subst value. cbn [rank children repeated_child_ranks].
      repeat constructor; lia.
    + change (nth id [] None = Some value) in live.
      rewrite nth_overflow in live by (cbn; lia). discriminate.
  - split; [repeat split; reflexivity|]. split.
    + intros [|[|id]] positive.
      * exists (Node 0 [] (Owned 7)). reflexivity.
      * exists (Node 1 [0; 0; 0] NoLease). reflexivity.
      * change (0 > 0) in positive. lia.
    + split.
      * intros id value live. exact live.
      * intros value impossible. discriminate.
Qed.

Example repeated_child_control_executes_every_token_once_and_keeps_the_external_source_reader :
  execution repeated_child_initial
    [FinalExtraction 1; DetachPayload; EnqueueChild 0; EnqueueChild 0; EnqueueChild 0;
     DropEmptyShell; NonfinalExtraction; NonfinalExtraction; FinalExtraction 0;
     DetachPayload; ReleaseOwnedLease 7; DropEmptyShell]
    (State [None; None] [] [7] [] None [] NoLease Ready [0; 1] [7]).
Proof.
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [] (Some (Node 1 [0; 0; 0] NoLease)) [] NoLease Extracted [1] []);
    [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [] None [0; 0; 0] NoLease Evacuated [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [0] None [0; 0] NoLease Evacuated [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [0; 0] None [0] NoLease Evacuated [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [0; 0; 0] None [] NoLease Evacuated [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [0; 0; 0] None [] NoLease Ready [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [0; 0] None [] NoLease Ready [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [Some (Node 0 [] (Owned 7)); None]
    [] [7] [0] None [] NoLease Ready [1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [None; None]
    [] [7] [] (Some (Node 0 [] (Owned 7))) [] NoLease Extracted [0; 1] []);
    [reflexivity|].
  eapply ExecutionStep with (middle := State [None; None]
    [] [7] [] None [] (Owned 7) Evacuated [0; 1] []); [reflexivity|].
  eapply ExecutionStep with (middle := State [None; None]
    [] [7] [] None [] NoLease Evacuated [0; 1] [7]); [reflexivity|].
  eapply ExecutionStep; [reflexivity|]. constructor.
Qed.

Example final_extraction_control_counts_children_before_the_first_enqueue :
  let extracted := State [Some (Node 0 [] (Owned 7)); None] [] [7] []
    (Some (Node 1 [0; 0; 0] NoLease)) [] NoLease Extracted [1] [] in
  step repeated_child_initial = Some (FinalExtraction 1, extracted) /\
  realized_frontier extracted = [0; 0; 0] /\
  staging_size extracted = 3 /\ length (pending extracted) = 0.
Proof. repeat split; reflexivity. Qed.

Example cyclic_child_control_cannot_satisfy_the_immutable_rank_precondition :
  forall original_rank,
    ~ original_graph_ranked [Some (Node 1 [0] NoLease)] original_rank.
Proof.
  intros original_rank ranked.
  destruct (ranked 0 (Node 1 [0] NoLease) eq_refl) as (_ & _ & smaller).
  inversion smaller. lia.
Qed.

Print Assumptions every_actual_trace_length_is_bounded_by_the_initial_whole_drain_measure.
Print Assumptions repeated_child_control_has_valid_original_ranks_and_initial_ownership.
Print Assumptions repeated_child_control_executes_every_token_once_and_keeps_the_external_source_reader.
Print Assumptions final_extraction_control_counts_children_before_the_first_enqueue.
Print Assumptions cyclic_child_control_cannot_satisfy_the_immutable_rank_precondition.

Print Assumptions every_actual_step_preserves_the_ranked_realized_frontier.
Print Assumptions every_actual_ranked_step_strictly_decreases_the_whole_drain_measure.
Print Assumptions a_valid_state_stops_exactly_when_idle_and_work_is_empty.
Print Assumptions every_valid_ranked_drain_reaches_idle_empty_work_without_a_fuel_parameter.
Print Assumptions every_finite_actual_trace_preserves_original_payloads_and_ranked_frontier.
Print Assumptions every_reachable_single_root_state_bounds_all_local_staged_and_pending_frames.

Print Assumptions realized_frontier_counts_every_local_staged_and_pending_occurrence.
Print Assumptions every_successful_step_retains_exact_original_heap_and_shell_payloads.
Print Assumptions original_heap_ranks_and_fanout_apply_to_every_retained_live_node.
Print Assumptions actual_final_extraction_replaces_the_realized_head_with_exact_reversed_children.
Print Assumptions actual_nonfinal_extraction_removes_only_the_realized_head.
Print Assumptions every_actual_staging_step_preserves_frontier_order_and_decreases_phase_work.

Print Assumptions clearing_a_node_empties_exactly_its_original_slot.
Print Assumptions clearing_a_node_preserves_every_other_complete_node.
Print Assumptions extraction_consumes_exactly_one_reference_and_preserves_all_other_counts.
Print Assumptions final_extraction_keeps_child_occurrences_in_the_full_local_shell.
Print Assumptions nonfinal_extraction_keeps_the_heap_external_owners_and_release_ledgers.
Print Assumptions detaching_moves_every_owned_child_without_changing_reference_counts.
Print Assumptions enqueueing_transfers_one_occurrence_without_changing_ownership.
Print Assumptions releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease.
Print Assumptions empty_shell_drop_is_work_and_ownership_neutral.
Print Assumptions a_surviving_external_reader_keeps_its_source_reference_count_positive.
Print Assumptions a_cleared_node_cannot_be_extracted_again.

Print Assumptions every_successful_step_preserves_the_local_ownership_phase.
Print Assumptions extraction_preserves_every_original_source_lease.
Print Assumptions no_backing_disposal_is_admissible_while_an_external_reader_survives.
Print Assumptions detaching_preserves_the_combined_local_and_queued_frontier.
Print Assumptions enqueueing_preserves_the_combined_local_and_queued_frontier.
Print Assumptions completed_child_staging_is_exactly_reverse_children_followed_by_old_work.
Print Assumptions each_ranked_lifo_frontier_has_at_most_one_plus_twice_root_height.
Print Assumptions final_lifo_extraction_preserves_the_ranked_frontier.
Print Assumptions nonfinal_lifo_extraction_preserves_the_ranked_frontier.
Print Assumptions every_final_extraction_strictly_decreases_rank_debt.
Print Assumptions every_nonfinal_extraction_strictly_decreases_rank_debt.
Print Assumptions a_successful_step_only_preserves_storage_or_claims_one_live_slot.
Print Assumptions every_successful_step_preserves_all_previously_cleared_slots.
Print Assumptions every_successful_step_preserves_unique_claims_and_their_empty_slots.
Print Assumptions every_finite_release_trace_claims_each_node_at_most_once.
Print Assumptions extraction_preserves_all_live_node_references.
Print Assumptions every_successful_step_preserves_all_live_node_references.
Print Assumptions every_finite_release_trace_preserves_live_reference_validity.
Print Assumptions every_successful_step_conserves_owned_source_leases_with_release_occurrences.
Print Assumptions every_finite_release_trace_accounts_for_every_owned_source_lease.

End CanonicalSourceRelease.

Module ConcurrentCanonicalSourceRelease.

Import CanonicalSourceRelease.

Record local_drain := LocalDrain {
  local_pending : list nat;
  local_payload : option node;
  local_staged : list nat;
  local_lease : source_lease;
  local_phase : phase
}.

Definition capture current :=
  LocalDrain (pending current) (local_shell current) (staged_children current)
    (staged_source current) (control current).

Definition bare local :=
  State [] [] [] (local_pending local) (local_payload local) (local_staged local)
    (local_lease local) (local_phase local) [] [].

Definition local_nodes local :=
  local_pending local ++ slot_children (local_payload local) ++ local_staged local.

Definition local_sources local :=
  slot_leases (local_payload local) ++ lease_tokens (local_lease local).

Record world := World {
  shared_heap : list (option node);
  outside_nodes : list nat;
  outside_sources : list nat;
  drains : list local_drain;
  global_claims : list nat;
  global_releases : list nat
}.

Definition project current local others :=
  State (shared_heap current)
    (outside_nodes current ++ flat_map local_nodes others)
    (outside_sources current ++ flat_map local_sources others)
    (local_pending local) (local_payload local) (local_staged local)
    (local_lease local) (local_phase local)
    (global_claims current) (global_releases current).

Definition global_node_refs current id :=
  occurrences id (outside_nodes current) +
  occurrences id (flat_map local_nodes (drains current)) +
  occurrences id (heap_children (shared_heap current)).

Definition global_source_refs current id :=
  occurrences id (outside_sources current) +
  occurrences id (flat_map local_sources (drains current)) +
  occurrences id (heap_leases (shared_heap current)).

Theorem selected_projection_counts_every_other_drain_token_as_external_ownership
    current before local after id :
  drains current = before ++ local :: after ->
  node_refs (project current local (before ++ after)) id = global_node_refs current id /\
  source_refs (project current local (before ++ after)) id = global_source_refs current id.
Proof.
  intro selected. unfold node_refs, source_refs, project, global_node_refs, global_source_refs.
  cbn [external_nodes external_sources pending local_shell staged_children staged_source heap].
  rewrite selected, !flat_map_app. cbn [flat_map].
  unfold local_nodes, local_sources. repeat rewrite occurrences_app. split; lia.
Qed.

Lemma successful_local_step_preserves_projected_external_owners current observed next :
  step current = Some (observed, next) ->
  external_nodes next = external_nodes current /\ external_sources next = external_sources current.
Proof.
  destruct current as [storage outside readers work held staged lease pc claims releases].
  destruct pc; destruct work; destruct held; destruct staged; destruct lease;
    cbn [step control extract pending heap detach local_shell enqueue staged_children
      release_source staged_source drop_empty] ; try discriminate;
    repeat match goal with
    | |- context [match slot ?storage ?id with _ => _ end] => destruct (slot storage id)
    | |- context [if ?guard then _ else _] => destruct guard
    end; intro stepped; inversion stepped; subst; split; reflexivity.
Qed.

Definition commit current before next after :=
  World (heap next) (outside_nodes current) (outside_sources current)
    (before ++ capture next :: after) (claimed_nodes next) (released_leases next).

Lemma committing_and_reprojecting_the_selected_drain_is_exact current before local after observed next :
  step (project current local (before ++ after)) = Some (observed, next) ->
  project (commit current before next after) (capture next) (before ++ after) = next.
Proof.
  intro stepped.
  destruct (successful_local_step_preserves_projected_external_owners _ _ _ stepped) as [nodes sources].
  unfold project in nodes, sources. cbn [external_nodes external_sources] in nodes, sources.
  destruct next. unfold project, commit, capture; cbn in *. now rewrite <- nodes, <- sources.
Qed.

Inductive global_step : world -> event -> world -> Prop :=
| GlobalStep current before local after observed next :
    drains current = before ++ local :: after ->
    step (project current local (before ++ after)) = Some (observed, next) ->
    global_step current observed (commit current before next after).

Definition local_valid original local :=
  phase_valid (bare local) /\
  (forall value, local_payload local = Some value -> exists id, slot original id = Some value).

Definition global_references_valid current :=
  forall id, global_node_refs current id > 0 ->
    exists value, slot (shared_heap current) id = Some value.

Definition global_claims_valid current :=
  NoDup (global_claims current) /\
  Forall (fun id => slot (shared_heap current) id = None) (global_claims current).

Definition world_valid original current :=
  Forall (local_valid original) (drains current) /\
  (forall id value, slot (shared_heap current) id = Some value -> slot original id = Some value) /\
  global_references_valid current /\ global_claims_valid current.

Lemma selected_local_valid original current before local after :
  world_valid original current -> drains current = before ++ local :: after ->
  drain_valid original (project current local (before ++ after)).
Proof.
  intros (locals & stored & references & claims) selected.
  rewrite selected, Forall_app in locals. destruct locals as [prefix locals].
  inversion locals as [|entry rest [phase_ok held] suffix]; subst entry rest.
  split; [exact phase_ok|]. split.
  - intros id positive. apply references.
    now rewrite <- (proj1 (selected_projection_counts_every_other_drain_token_as_external_ownership
      current before local after id selected)).
  - split; [exact stored|exact held].
Qed.

Lemma selected_claims_valid current local others :
  global_claims_valid current -> claims_valid (project current local others).
Proof. exact (fun valid => valid). Qed.

Theorem every_global_step_preserves_all_live_node_references original current observed next :
  world_valid original current -> global_step current observed next -> global_references_valid next.
Proof.
  intros valid ran. revert valid.
  destruct ran as [before_world before local after event_seen result selected stepped]. intro valid.
  destruct (selected_local_valid original before_world before local after valid selected)
    as (phase_ok & references & retained).
  pose proof (every_successful_step_preserves_all_live_node_references
    _ _ _ phase_ok references stepped) as preserved.
  intros id positive.
  pose proof (proj1 (selected_projection_counts_every_other_drain_token_as_external_ownership
    (commit before_world before result after) before (capture result) after id eq_refl)) as counted.
  rewrite (committing_and_reprojecting_the_selected_drain_is_exact
    before_world before local after event_seen result stepped) in counted.
  apply preserved. now rewrite counted.
Qed.

Theorem every_global_step_conserves_owned_source_leases_across_all_drains
    original current observed next owner :
  world_valid original current -> global_step current observed next ->
  global_source_refs next owner + occurrences owner (global_releases next) =
    global_source_refs current owner + occurrences owner (global_releases current).
Proof.
  intros valid ran. revert valid.
  destruct ran as [before_world before local after event_seen result selected stepped]. intro valid.
  destruct (selected_local_valid original before_world before local after valid selected)
    as (phase_ok & references & retained).
  pose proof (every_successful_step_conserves_owned_source_leases_with_release_occurrences
    _ _ _ owner phase_ok stepped) as conserved.
  pose proof (proj2 (selected_projection_counts_every_other_drain_token_as_external_ownership
    before_world before local after owner selected)) as before_count.
  pose proof (proj2 (selected_projection_counts_every_other_drain_token_as_external_ownership
    (commit before_world before result after) before (capture result) after owner eq_refl)) as after_count.
  rewrite (committing_and_reprojecting_the_selected_drain_is_exact
    before_world before local after event_seen result stepped) in after_count.
  rewrite <- before_count, <- after_count. exact conserved.
Qed.

Theorem every_global_step_preserves_unique_claims_and_all_cleared_slots current observed next :
  global_claims_valid current -> global_step current observed next -> global_claims_valid next.
Proof.
  intros valid ran. revert valid.
  destruct ran as [before_world before local after event_seen result selected stepped]. intro valid.
  exact (every_successful_step_preserves_unique_claims_and_their_empty_slots
    _ _ _ (selected_claims_valid before_world local (before ++ after) valid) stepped).
Qed.

Lemma capture_keeps_the_exact_local_phase_and_shell current :
  phase_valid (bare (capture current)) = phase_valid current /\
  local_payload (capture current) = local_shell current.
Proof. split; reflexivity. Qed.

Theorem every_global_step_preserves_the_complete_aggregate_invariant original current observed next :
  world_valid original current -> global_step current observed next -> world_valid original next.
Proof.
  intros valid ran.
  pose proof (every_global_step_preserves_all_live_node_references original _ _ _ valid ran) as references.
  pose proof (every_global_step_preserves_unique_claims_and_all_cleared_slots _ _ _
    (proj2 (proj2 (proj2 valid))) ran) as claims.
  revert valid references claims.
  destruct ran as [before_world before local after event_seen result selected stepped].
  intros valid references claims.
  pose proof (successful_step_preserves_drain_valid original _ _ _
    (selected_local_valid original before_world before local after valid selected) stepped)
    as (phase_ok & selected_refs & stored & held).
  destruct valid as (locals & old_stored & old_refs & old_claims).
  unfold world_valid, commit; cbn [drains shared_heap].
  split.
  - rewrite selected, Forall_app in locals. destruct locals as [prefix locals].
    inversion locals; subst. apply Forall_app. split; [exact prefix|].
    constructor; [split; assumption|assumption].
  - split; [exact stored|]. split; assumption.
Qed.

Definition slot_work entry :=
  match entry with
  | None => 0
  | Some value => 2 * length (children value) + 2 + lease_work (source value)
  end.

Definition total {A : Type} (weight : A -> nat) values :=
  fold_right (fun value accumulated => weight value + accumulated) 0 values.

Lemma total_app {A : Type} (weight : A -> nat) before after :
  total weight (before ++ after) = total weight before + total weight after.
Proof.
  induction before as [|head rest IH]; [reflexivity|].
  change (weight head + total weight (rest ++ after) =
    (weight head + total weight rest) + total weight after).
  rewrite IH. lia.
Qed.

Definition heap_work storage := total slot_work storage.
Definition local_work local := staging_size (bare local) + phase_work (bare local).
Definition single_work current := heap_work (heap current) + staging_size current + phase_work current.
Definition global_work current := heap_work (shared_heap current) + total local_work (drains current).

Lemma clearing_a_live_slot_removes_exactly_its_physical_work storage id :
  heap_work storage = slot_work (slot storage id) + heap_work (clear_slot id storage).
Proof.
  revert id. induction storage as [|entry rest IH]; intros [|id];
    try reflexivity.
  change (slot_work entry + heap_work rest =
    slot_work (slot rest id) + (slot_work entry + heap_work (clear_slot id rest))).
  rewrite (IH id). lia.
Qed.

Theorem each_local_release_step_consumes_exactly_one_unit_of_physical_work current observed next :
  phase_valid current -> step current = Some (observed, next) ->
  S (single_work next) = single_work current.
Proof.
  destruct current as [storage outside readers work held staged lease pc claims releases].
  destruct pc; cbn [phase_valid control local_shell staged_children staged_source].
  - intros (empty & no_staged & no_lease) stepped. subst held staged lease.
    destruct work as [|id rest]; [discriminate|].
    cbn [step control extract pending heap] in stepped.
    destruct (slot storage id) as [value|] eqn:live; [|discriminate].
    pose proof (clearing_a_live_slot_removes_exactly_its_physical_work storage id) as removed.
    rewrite live in removed. cbn [slot_work] in removed.
    destruct (_ =? 1); inversion stepped; subst; clear stepped;
      unfold single_work, staging_size, phase_work;
      cbn [heap pending local_shell staged_children control
        slot_children staged_source length]; lia.
  - intros ((value & full) & no_staged & no_lease) stepped. subst held staged lease.
    inversion stepped; subst; clear stepped.
    unfold single_work, staging_size, phase_work.
    cbn [heap pending local_shell staged_children control
      slot_children staged_source]. lia.
  - intros empty stepped. subst held.
    destruct staged as [|id rest]; destruct lease;
      cbn [step control staged_children staged_source enqueue release_source drop_empty local_shell] in stepped;
      inversion stepped; subst; clear stepped;
      unfold single_work, staging_size, phase_work;
      cbn [heap pending local_shell staged_children control
        slot_children staged_source lease_work length]; lia.
Qed.

Lemma focused_work_plus_other_drains_is_exact current before local after :
  drains current = before ++ local :: after ->
  global_work current = single_work (project current local (before ++ after)) +
    total local_work before + total local_work after.
Proof.
  intro selected. unfold global_work. rewrite selected, (total_app local_work before (local :: after)).
  assert (focused : single_work (project current local (before ++ after)) =
    heap_work (shared_heap current) + local_work local).
  { change (heap_work (shared_heap current) + staging_size (bare local) + phase_work (bare local) =
      heap_work (shared_heap current) + (staging_size (bare local) + phase_work (bare local))). lia. }
  rewrite focused.
  change (heap_work (shared_heap current) +
    (total local_work before + (local_work local + total local_work after)) =
    heap_work (shared_heap current) + local_work local + total local_work before + total local_work after).
  lia.
Qed.

Lemma committed_work_plus_other_drains_is_exact current before next after :
  global_work (commit current before next after) = single_work next +
    total local_work before + total local_work after.
Proof.
  unfold global_work, commit. cbn [shared_heap drains].
  rewrite (total_app local_work before (capture next :: after)).
  assert (focused : single_work next = heap_work (heap next) + local_work (capture next)).
  { change (heap_work (heap next) + staging_size next + phase_work next =
      heap_work (heap next) + (staging_size next + phase_work next)). lia. }
  rewrite focused.
  change (heap_work (heap next) +
    (total local_work before + (local_work (capture next) + total local_work after)) =
    heap_work (heap next) + local_work (capture next) + total local_work before + total local_work after).
  lia.
Qed.

Theorem every_global_step_consumes_exactly_one_unit_of_total_physical_work
    original current observed next :
  world_valid original current -> global_step current observed next ->
  S (global_work next) = global_work current.
Proof.
  intros valid ran. revert valid.
  destruct ran as [before_world before local after event_seen result selected stepped]. intro valid.
  pose proof (proj1 (selected_local_valid original before_world before local after valid selected)) as phase_ok.
  pose proof (each_local_release_step_consumes_exactly_one_unit_of_physical_work _ _ _ phase_ok stepped).
  rewrite (focused_work_plus_other_drains_is_exact before_world before local after selected),
    committed_work_plus_other_drains_is_exact. lia.
Qed.

Inductive global_execution : world -> list event -> world -> Prop :=
| GlobalEmpty current : global_execution current [] current
| GlobalAdvance current observed middle events final :
    global_step current observed middle -> global_execution middle events final ->
    global_execution current (observed :: events) final.

Theorem every_finite_interleaving_preserves_aggregate_ownership_and_lease_accounting
    original initial events final :
  world_valid original initial -> global_execution initial events final ->
  world_valid original final /\
  forall owner, global_source_refs final owner + occurrences owner (global_releases final) =
    global_source_refs initial owner + occurrences owner (global_releases initial).
Proof.
  intros valid ran. induction ran.
  - split; [exact valid|intro owner; reflexivity].
  - pose proof (every_global_step_preserves_the_complete_aggregate_invariant original _ _ _ valid H) as middle_valid.
    destruct (IHran middle_valid) as [final_valid conserved]. split; [exact final_valid|].
    intro owner. rewrite conserved.
    exact (every_global_step_conserves_owned_source_leases_across_all_drains original _ _ _ owner valid H).
Qed.

Theorem finite_interleaving_length_plus_remaining_physical_work_is_exact
    original initial events final :
  world_valid original initial -> global_execution initial events final ->
  length events + global_work final = global_work initial.
Proof.
  intros valid ran. induction ran; [reflexivity|].
  specialize (IHran (every_global_step_preserves_the_complete_aggregate_invariant original _ _ _ valid H)).
  pose proof (every_global_step_consumes_exactly_one_unit_of_total_physical_work original _ _ _ valid H).
  cbn [length]. lia.
Qed.

Definition local_done local := idle (bare local) /\ local_pending local = [].

Fixpoint schedule current before remaining :=
  match remaining with
  | [] => None
  | local :: after =>
      match step (project current local (before ++ after)) with
      | Some (observed, next) => Some (observed, commit current before next after)
      | None => schedule current (before ++ [local]) after
      end
  end.

Lemma schedule_selects_only_an_existing_global_step current remaining :
  forall before observed next,
  drains current = before ++ remaining -> schedule current before remaining = Some (observed, next) ->
  global_step current observed next.
Proof.
  induction remaining as [|local after IH]; intros before observed next selected scheduled;
    cbn [schedule] in scheduled; [discriminate|].
  destruct (step (project current local (before ++ after))) as [[event_seen result]|] eqn:stepped.
  - inversion scheduled; subst observed next. econstructor; eassumption.
  - eapply IH; [|exact scheduled]. rewrite <- app_assoc. exact selected.
Qed.

Lemma an_empty_schedule_identifies_all_completed_local_drains original current remaining :
  world_valid original current -> forall before,
  drains current = before ++ remaining -> schedule current before remaining = None ->
  Forall local_done remaining.
Proof.
  intro valid. induction remaining as [|local after IH]; intros before selected scheduled; [constructor|].
  cbn [schedule] in scheduled.
  destruct (step (project current local (before ++ after))) as [[event_seen result]|] eqn:stepped;
    [discriminate|].
  destruct (selected_local_valid original current before local after valid selected)
    as (phase_ok & references & retained).
  pose proof (proj1 (a_valid_state_stops_exactly_when_idle_and_work_is_empty
    _ phase_ok references) stepped) as done.
  constructor; [exact done|].
  apply (IH (before ++ [local])); [|exact scheduled]. rewrite <- app_assoc. exact selected.
Qed.

Theorem every_finite_closed_family_of_drains_reaches_quiescence_without_fuel original :
  forall current, world_valid original current ->
  exists events final, global_execution current events final /\
    world_valid original final /\ Forall local_done (drains final).
Proof.
  refine (well_founded_induction (well_founded_ltof world global_work)
    (fun current => world_valid original current ->
      exists events final, global_execution current events final /\
        world_valid original final /\ Forall local_done (drains final)) _).
  intros current IH valid.
  destruct (schedule current [] (drains current)) as [[observed next]|] eqn:scheduled.
  - pose proof (schedule_selects_only_an_existing_global_step current (drains current)
      [] observed next eq_refl scheduled) as stepped.
    pose proof (every_global_step_consumes_exactly_one_unit_of_total_physical_work
      original current observed next valid stepped) as decreased.
    pose proof (every_global_step_preserves_the_complete_aggregate_invariant
      original current observed next valid stepped) as next_valid.
    destruct (IH next ltac:(unfold ltof; lia) next_valid) as (events & final & ran & done).
    exists (observed :: events), final. split; [econstructor; eauto|exact done].
  - exists [], current. split; [constructor|]. split; [exact valid|].
    exact (an_empty_schedule_identifies_all_completed_local_drains
      original current (drains current) valid [] eq_refl scheduled).
Qed.

Definition live_nodes (storage : list (option node)) := total (fun entry => match entry with Some _ => 1 | None => 0 end) storage.
Definition source_actions storage := total (fun entry => match entry with Some value => lease_work (source value) | None => 0 end) storage.

Theorem heap_work_counts_physical_nodes_child_occurrences_and_source_actions storage :
  heap_work storage = 2 * length (heap_children storage) + 2 * live_nodes storage + source_actions storage.
Proof.
  induction storage as [|[value|] rest IH]; [reflexivity| |exact IH].
  change ((2 * length (children value) + 2 + lease_work (source value)) + heap_work rest =
    2 * length (children value ++ heap_children rest) + 2 * (1 + live_nodes rest) +
      (lease_work (source value) + source_actions rest)).
  rewrite length_app, IH. lia.
Qed.

Theorem physical_work_bound_for_idle_initial_drains_counts_no_logical_tree_expansion
    original initial events final :
  world_valid original initial ->
  Forall (fun local => idle (bare local)) (drains initial) ->
  global_execution initial events final ->
  length events <= length (flat_map local_pending (drains initial)) +
    2 * length (heap_children (shared_heap initial)) + 2 * live_nodes (shared_heap initial) +
    source_actions (shared_heap initial).
Proof.
  intros valid ready ran.
  pose proof (finite_interleaving_length_plus_remaining_physical_work_is_exact original _ _ _ valid ran) as work.
  assert (initial_local : total local_work (drains initial) = length (flat_map local_pending (drains initial))).
  { induction ready as [|local rest is_ready all_ready IH]; [reflexivity|].
    assert (head_work : local_work local = length (local_pending local)).
    { destruct local as [pending_work held staged lease pc].
      unfold idle, bare in is_ready. cbn in is_ready.
      destruct is_ready as (ready_phase & empty & no_staged & no_lease). subst pc held staged lease.
      unfold local_work, bare, staging_size, phase_work. cbn. lia. }
    change (local_work local + total local_work rest =
      length (local_pending local ++ flat_map local_pending rest)).
    rewrite length_app, head_work. lia. }
  unfold global_work in work. rewrite initial_local,
    (heap_work_counts_physical_nodes_child_occurrences_and_source_actions (shared_heap initial)) in work.
  lia.
Qed.

Definition family_frontiers original_rank upper current :=
  Forall (fun local => frontier original_rank upper (realized_frontier (bare local))) (drains current).

Theorem a_global_step_preserves_each_drain_frontier_under_arbitrary_interleaving
    original original_rank upper current observed next :
  original_graph_ranked original original_rank -> world_valid original current ->
  family_frontiers original_rank upper current -> global_step current observed next ->
  family_frontiers original_rank upper next.
Proof.
  intros ranked valid bounded ran. revert valid bounded.
  destruct ran as [before_world before local after event_seen result selected stepped]. intros valid bounded.
  destruct (selected_local_valid original before_world before local after valid selected)
    as (phase_ok & references & retained).
  unfold family_frontiers in *. rewrite selected, Forall_app in bounded.
  destruct bounded as [prefix bounded]. inversion bounded as [|entry rest local_bound suffix]; subst entry rest.
  pose proof (every_actual_step_preserves_the_ranked_realized_frontier
    original original_rank upper _ event_seen result ranked retained phase_ok local_bound stepped) as preserved.
  cbn [commit drains]. apply Forall_app. split; [exact prefix|]. constructor; assumption.
Qed.

Lemma global_steps_keep_the_finite_drain_family_fixed current observed next :
  global_step current observed next -> length (drains next) = length (drains current).
Proof.
  intro ran. destruct ran as [before_world before local after event_seen result selected stepped].
  cbn [commit drains]. rewrite selected, !length_app. reflexivity.
Qed.

Lemma combined_frontier_sum_bound original_rank upper locals :
  Forall (fun local => frontier original_rank upper (realized_frontier (bare local))) locals ->
  total (fun local => staging_size (bare local)) locals <= length locals * (1 + 2 * upper).
Proof.
  intro bounded. induction bounded as [|local rest local_bound tail_bound IH]; [apply Nat.le_refl|].
  pose proof (each_ranked_lifo_frontier_has_at_most_one_plus_twice_root_height
    original_rank upper (realized_frontier (bare local)) local_bound) as local_size.
  rewrite realized_frontier_counts_every_local_staged_and_pending_occurrence in local_size.
  change (staging_size (bare local) + total (fun item => staging_size (bare item)) rest <=
    S (length rest) * (1 + 2 * upper)). nia.
Qed.

Theorem every_reachable_family_bounds_combined_local_staged_and_pending_storage
    original original_rank upper initial events final :
  original_graph_ranked original original_rank -> world_valid original initial ->
  family_frontiers original_rank upper initial -> global_execution initial events final ->
  total (fun local => staging_size (bare local)) (drains final) <=
    length (drains initial) * (1 + 2 * upper).
Proof.
  intros ranked valid bounded ran.
  assert (preserved : family_frontiers original_rank upper final /\ length (drains final) = length (drains initial)).
  { induction ran; [split; assumption || reflexivity|].
    destruct (IHran
      (every_global_step_preserves_the_complete_aggregate_invariant original _ _ _ valid H)
      (a_global_step_preserves_each_drain_frontier_under_arbitrary_interleaving
        original original_rank upper _ _ _ ranked valid bounded H)) as [kept count].
    split; [exact kept|]. rewrite count. exact (global_steps_keep_the_finite_drain_family_fixed _ _ _ H). }
  destruct preserved as [kept count]. rewrite <- count. unfold family_frontiers in kept.
  exact (combined_frontier_sum_bound original_rank upper (drains final) kept).
Qed.

Inductive consumption_kind := OrdinaryDrop | IntoInner.
Inductive native_outcome := NoPayload | PopulatedCallback (id : nat) | MovedPayload (id : nat).

Inductive arc_consumption_contract : consumption_kind -> nat -> nat -> nat -> native_outcome -> Prop :=
| OrdinaryNonfinal remaining id :
    arc_consumption_contract OrdinaryDrop (S (S remaining)) (S remaining) id NoPayload
| OrdinaryFinal id : arc_consumption_contract OrdinaryDrop 1 0 id (PopulatedCallback id)
| IntoInnerNonfinal remaining id :
    arc_consumption_contract IntoInner (S (S remaining)) (S remaining) id NoPayload
| IntoInnerFinal id : arc_consumption_contract IntoInner 1 0 id (MovedPayload id).

Definition native_route kind observed :=
  match observed with
  | NonfinalExtraction => Some NoPayload
  | FinalExtraction id =>
      Some (match kind with OrdinaryDrop => PopulatedCallback id | IntoInner => MovedPayload id end)
  | _ => None
  end.

Lemma extraction_identifies_the_exact_predecrement_owner_count current observed next id rest :
  idle current -> pending current = id :: rest -> step current = Some (observed, next) ->
  (node_refs current id = 1 /\ observed = FinalExtraction id) \/
  (1 < node_refs current id /\ observed = NonfinalExtraction).
Proof.
  intros ready work stepped.
  assert (positive : 0 < node_refs current id).
  { unfold node_refs. rewrite work, occurrences_cons_same. lia. }
  destruct ready as (pc & shell & staged & lease).
  unfold step in stepped. rewrite pc in stepped.
  unfold extract in stepped. rewrite pc, work in stepped.
  destruct (slot (heap current) id); [|discriminate].
  destruct (node_refs current id =? 1) eqn:final_owner; inversion stepped; subst.
  - left. apply Nat.eqb_eq in final_owner. auto.
  - right. apply Nat.eqb_neq in final_owner. split; [lia|reflexivity].
Qed.

Theorem global_extraction_consumes_exactly_one_token_across_all_drains
    current before local after observed next id rest target :
  drains current = before ++ local :: after ->
  idle (bare local) -> local_pending local = id :: rest ->
  step (project current local (before ++ after)) = Some (observed, next) ->
  global_node_refs current target = global_node_refs (commit current before next after) target +
    if Nat.eq_dec id target then 1 else 0.
Proof.
  intros selected ready work stepped.
  assert (extracted : extract (project current local (before ++ after)) = Some (observed, next)).
  { unfold step in stepped.
    assert (pc : control (project current local (before ++ after)) = Ready) by exact (proj1 ready).
    now rewrite pc in stepped. }
  pose proof (extraction_consumes_exactly_one_reference_and_preserves_all_other_counts
    (project current local (before ++ after)) observed next id rest target ready work extracted) as consumed.
  pose proof (proj1 (selected_projection_counts_every_other_drain_token_as_external_ownership
    current before local after target selected)) as old_count.
  pose proof (proj1 (selected_projection_counts_every_other_drain_token_as_external_ownership
    (commit current before next after) before (capture next) after target eq_refl)) as new_count.
  rewrite (committing_and_reprojecting_the_selected_drain_is_exact
    current before local after observed next stepped) in new_count.
  now rewrite old_count, new_count in consumed.
Qed.

Theorem the_explicit_arc_contract_selects_the_exact_global_extraction_route
    current before local after observed next id rest kind after_count outcome :
  drains current = before ++ local :: after ->
  idle (bare local) -> local_pending local = id :: rest ->
  step (project current local (before ++ after)) = Some (observed, next) ->
  arc_consumption_contract kind (global_node_refs current id) after_count id outcome ->
  native_route kind observed = Some outcome /\
  after_count = global_node_refs (commit current before next after) id.
Proof.
  intros selected ready work stepped native_contract.
  pose proof (global_extraction_consumes_exactly_one_token_across_all_drains
    current before local after observed next id rest id selected ready work stepped) as consumed.
  destruct (Nat.eq_dec id id); [|contradiction].
  split; [|inversion native_contract; subst; lia].
  pose proof (extraction_identifies_the_exact_predecrement_owner_count
    (project current local (before ++ after)) observed next id rest ready work stepped) as classified.
  rewrite (proj1 (selected_projection_counts_every_other_drain_token_as_external_ownership
    current before local after id selected)) in classified.
  destruct classified as [[count final_event]|[count nonfinal_event]];
    subst observed; inversion native_contract; subst; cbn [native_route]; reflexivity || lia.
Qed.

Theorem ordinary_final_drop_calls_the_node_destructor_but_into_inner_moves_its_payload id :
  arc_consumption_contract OrdinaryDrop 1 0 id (PopulatedCallback id) /\
  arc_consumption_contract IntoInner 1 0 id (MovedPayload id) /\
  native_route OrdinaryDrop (FinalExtraction id) <> native_route IntoInner (FinalExtraction id).
Proof. repeat split; constructor || discriminate. Qed.

Inductive entry_context := ExternalOwner | ActiveDrain.
Definition node_owned_protocol context kind :=
  match context with ExternalOwner => True | ActiveDrain => kind = IntoInner end.

Theorem active_drains_never_reenter_a_populated_child_destructor_under_the_native_contract
    kind before_count after_count id outcome :
  node_owned_protocol ActiveDrain kind ->
  arc_consumption_contract kind before_count after_count id outcome ->
  forall child, outcome <> PopulatedCallback child.
Proof. cbn [node_owned_protocol]. intros -> native_contract child. inversion native_contract; discriminate. Qed.

Theorem two_failed_uniqueness_checks_can_still_end_in_a_nested_populated_callback id :
  (2 =? 1) = false /\ (2 =? 1) = false /\
  arc_consumption_contract OrdinaryDrop 2 1 id NoPayload /\
  arc_consumption_contract OrdinaryDrop 1 0 id (PopulatedCallback id) /\
  ~ node_owned_protocol ActiveDrain OrdinaryDrop.
Proof. repeat split; try reflexivity; try constructor. discriminate. Qed.

Theorem closed_interleavings_keep_external_owners_and_the_drain_family_fixed initial events final :
  global_execution initial events final ->
  outside_nodes final = outside_nodes initial /\ outside_sources final = outside_sources initial /\
  length (drains final) = length (drains initial).
Proof.
  intro ran. induction ran; [repeat split; reflexivity|].
  destruct IHran as [nodes [sources count]]. rewrite nodes, sources, count.
  pose proof (global_steps_keep_the_finite_drain_family_fixed _ _ _ H) as family.
  revert family. destruct H. intro family. repeat split; assumption || reflexivity.
Qed.

Print Assumptions selected_projection_counts_every_other_drain_token_as_external_ownership.
Print Assumptions every_global_step_preserves_all_live_node_references.
Print Assumptions every_global_step_conserves_owned_source_leases_across_all_drains.
Print Assumptions every_global_step_preserves_unique_claims_and_all_cleared_slots.
Print Assumptions every_global_step_preserves_the_complete_aggregate_invariant.
Print Assumptions each_local_release_step_consumes_exactly_one_unit_of_physical_work.
Print Assumptions every_global_step_consumes_exactly_one_unit_of_total_physical_work.
Print Assumptions every_finite_interleaving_preserves_aggregate_ownership_and_lease_accounting.
Print Assumptions finite_interleaving_length_plus_remaining_physical_work_is_exact.
Print Assumptions every_finite_closed_family_of_drains_reaches_quiescence_without_fuel.
Print Assumptions heap_work_counts_physical_nodes_child_occurrences_and_source_actions.
Print Assumptions physical_work_bound_for_idle_initial_drains_counts_no_logical_tree_expansion.
Print Assumptions a_global_step_preserves_each_drain_frontier_under_arbitrary_interleaving.
Print Assumptions every_reachable_family_bounds_combined_local_staged_and_pending_storage.
Print Assumptions global_extraction_consumes_exactly_one_token_across_all_drains.
Print Assumptions the_explicit_arc_contract_selects_the_exact_global_extraction_route.
Print Assumptions ordinary_final_drop_calls_the_node_destructor_but_into_inner_moves_its_payload.
Print Assumptions active_drains_never_reenter_a_populated_child_destructor_under_the_native_contract.
Print Assumptions two_failed_uniqueness_checks_can_still_end_in_a_nested_populated_callback.
Print Assumptions closed_interleavings_keep_external_owners_and_the_drain_family_fixed.

End ConcurrentCanonicalSourceRelease.
