From Stdlib Require Import Bool List Arith Lia.
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
