From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import GenerationCheckpointPublication IncrementalLedgerTrial
  IncrementalDomainComposition.

Import ListNotations.

(**
  Credited cold construction of the per-domain reachability ledgers.

  Cold admission verifies one state manifest per epoch: the current state
  first, then every historical state.  The structural verifier visits the
  whole closure of each state, so a reachability count cannot be incremented
  at every visit: an object shared by two states would be counted once per
  state instead of once per active parent.  The verifier therefore tags each
  work item with a domain credit.  An epoch credits the root occurrences of
  its manifest; an object whose count crosses from zero to one in a domain
  credits each of its child occurrences in that domain, and no other visit
  credits anything.

  [credited_epoch] is the resulting equation on counts: the new count of a
  vertex is its old count, plus its occurrences among the epoch's added roots,
  plus one per child occurrence of every vertex that crossed in the epoch.
  The theorems below show that this equation, applied to a coherent quiescent
  predecessor, yields the coherent quiescent ledger for the added roots, that
  the union inventory grows by exactly the crossed objects, that the crossed
  objects are exactly the newly reachable ones, and that a cold build folding
  one epoch per retention entry realizes the current and retained root
  counters of [CheckedDomainComposition].  The two fixtures at the end are
  negative controls: crediting every visit of an epoch double-counts a shared
  child, and a crossing whose child occurrences are not credited leaves an
  active parent with an inactive child.
*)

Module CreditedClosureLedger.

Import IncrementalReachability.
Import CheckedReachability.
Import CheckedDomainComposition.

Definition crossed (before after : ledger) : membership :=
  fun vertex => negb (active before vertex) && active after vertex.

Definition empty_ledger : ledger := {|
  counts := fun _ => 0;
  active := fun _ => false
|}.

Definition zero_roots : counter := fun _ => 0.

Section Credited.
Variable children : nat -> list nat.

Definition credited_epoch (vertices added : list nat) (before after : ledger) : Prop :=
  forall vertex, counts after vertex =
    counts before vertex + occurrences added vertex +
    incoming children vertices (crossed before after) vertex.

Inductive credited_build (vertices : list nat) : list (list nat) -> ledger -> Prop :=
| credited_build_start : credited_build vertices [] empty_ledger
| credited_build_epoch : forall epochs added before after,
    credited_build vertices epochs before ->
    coherent after ->
    credited_epoch vertices added before after ->
    credited_build vertices (epochs ++ [added]) after.

Lemma incoming_split : forall vertices left right query,
  incoming children vertices (fun vertex => left vertex || right vertex) query +
  incoming children vertices (fun vertex => left vertex && right vertex) query =
  incoming children vertices left query + incoming children vertices right query.
Proof.
  induction vertices as [|head tail IH]; intros left right query; [reflexivity|].
  cbn [incoming]. specialize (IH left right query).
  destruct (left head), (right head); cbn; lia.
Qed.

Lemma incoming_nowhere : forall vertices query,
  incoming children vertices (fun _ => false) query = 0.
Proof.
  induction vertices as [|head tail IH]; intros query; [reflexivity|].
  cbn [incoming]. now rewrite IH.
Qed.

Lemma credited_epoch_counts_grow : forall vertices added before after,
  credited_epoch vertices added before after ->
  forall vertex, counts before vertex <= counts after vertex.
Proof.
  intros vertices added before after epoch vertex. rewrite (epoch vertex). lia.
Qed.

Lemma credited_epoch_keeps_active : forall vertices added before after,
  coherent before -> coherent after -> credited_epoch vertices added before after ->
  forall vertex, active before vertex = true -> active after vertex = true.
Proof.
  intros vertices added before after before_coherent after_coherent epoch vertex enabled.
  rewrite (before_coherent vertex) in enabled. apply Nat.ltb_lt in enabled.
  rewrite (after_coherent vertex). apply Nat.ltb_lt.
  pose proof (credited_epoch_counts_grow vertices added before after epoch vertex) as grow.
  lia.
Qed.

Lemma active_after_is_before_or_crossed : forall vertices added before after,
  coherent before -> coherent after -> credited_epoch vertices added before after ->
  forall vertex, active after vertex = active before vertex || crossed before after vertex.
Proof.
  intros vertices added before after before_coherent after_coherent epoch vertex.
  unfold crossed. destruct (active before vertex) eqn:old.
  - rewrite (credited_epoch_keeps_active vertices added before after before_coherent
      after_coherent epoch vertex old).
    reflexivity.
  - cbn. reflexivity.
Qed.

Theorem credited_epoch_is_quiescent_for_added_roots :
  forall vertices roots added before after,
  coherent before -> quiescent children vertices roots before ->
  coherent after -> credited_epoch vertices added before after ->
  quiescent children vertices (add_roots roots added) after.
Proof.
  intros vertices roots added before after before_coherent before_balance after_coherent
    epoch vertex.
  assert (union : incoming children vertices (active after) vertex =
    incoming children vertices
      (fun parent => active before parent || crossed before after parent) vertex).
  { apply incoming_extensional. intros parent.
    exact (active_after_is_before_or_crossed vertices added before after before_coherent
      after_coherent epoch parent). }
  assert (disjoint : incoming children vertices
      (fun parent => active before parent && crossed before after parent) vertex = 0).
  { transitivity (incoming children vertices (fun _ => false) vertex);
      [|apply incoming_nowhere].
    apply incoming_extensional. intros parent. cbv beta. unfold crossed.
    destruct (active before parent); reflexivity. }
  pose proof (incoming_split vertices (active before) (crossed before after) vertex)
    as split.
  unfold add_roots. rewrite (epoch vertex), (before_balance vertex), union. lia.
Qed.

Theorem credited_epoch_agrees_with_cold_ledger :
  forall vertices roots added before after cold rank,
  coherent before -> quiescent children vertices roots before ->
  coherent after -> credited_epoch vertices added before after ->
  coherent cold -> quiescent children vertices (add_roots roots added) cold ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  forall vertex, counts after vertex = counts cold vertex /\
    active after vertex = active cold vertex.
Proof.
  intros vertices roots added before after cold rank before_coherent before_balance
    after_coherent epoch cold_coherent cold_balance descending.
  apply (@quiescent_ledger_is_unique children vertices (add_roots roots added) after cold
    rank after_coherent).
  - exact (credited_epoch_is_quiescent_for_added_roots vertices roots added before after
      before_coherent before_balance after_coherent epoch).
  - exact cold_coherent.
  - exact cold_balance.
  - exact descending.
Qed.

Theorem credited_epoch_inventory_grows_by_crossed_objects :
  forall vertices added before after weight,
  coherent before -> coherent after -> credited_epoch vertices added before after ->
  inventory vertices after weight =
    inventory vertices before weight + selected_sum vertices (crossed before after) weight.
Proof.
  intros vertices added before after weight before_coherent after_coherent epoch.
  unfold inventory.
  rewrite (selected_sum_partition vertices (active after) (active before) weight).
  - f_equal. apply selected_sum_extensional. intros vertex. cbv beta. unfold crossed.
    apply andb_comm.
  - intros vertex enabled.
    exact (credited_epoch_keeps_active vertices added before after before_coherent
      after_coherent epoch vertex enabled).
Qed.

Lemma reachable_add_roots : forall vertices roots added vertex,
  reachable children vertices (add_roots roots added) vertex <->
  reachable children vertices roots vertex \/
  reachable children vertices (occurrences added) vertex.
Proof.
  intros vertices roots added vertex. split.
  - intros path. induction path as [vertex root|parent child path IH member edge].
    + unfold add_roots in root.
      destruct (Nat.lt_ge_cases 0 (roots vertex)) as [positive|zero].
      * left. now apply reachable_root.
      * right. apply reachable_root. lia.
    + destruct IH as [left_path|right_path].
      * left. now apply (@reachable_child children vertices roots parent child).
      * right. now apply (@reachable_child children vertices (occurrences added) parent
          child).
  - intros [left_path|right_path].
    + induction left_path as [vertex root|parent child path IH member edge].
      * apply reachable_root. unfold add_roots. lia.
      * now apply (@reachable_child children vertices (add_roots roots added) parent child).
    + induction right_path as [vertex root|parent child path IH member edge].
      * apply reachable_root. unfold add_roots. lia.
      * now apply (@reachable_child children vertices (add_roots roots added) parent child).
Qed.

Theorem credited_crossings_are_newly_reachable_objects :
  forall vertices roots added before after rank,
  coherent before -> quiescent children vertices roots before ->
  coherent after -> credited_epoch vertices added before after ->
  (forall parent child, In parent vertices -> In child (children parent) ->
    rank child < rank parent) ->
  forall vertex, crossed before after vertex = true <->
    reachable children vertices (occurrences added) vertex /\
    active before vertex = false.
Proof.
  intros vertices roots added before after rank before_coherent before_balance
    after_coherent epoch descending vertex.
  pose proof (credited_epoch_is_quiescent_for_added_roots vertices roots added before after
    before_coherent before_balance after_coherent epoch) as after_balance.
  pose proof (@quiescent_support_is_exact_root_reachability children vertices
    (add_roots roots added) after rank after_coherent after_balance descending vertex)
    as after_support.
  pose proof (@quiescent_support_is_exact_root_reachability children vertices roots before
    rank before_coherent before_balance descending vertex) as before_support.
  rewrite reachable_add_roots in after_support.
  unfold crossed. destruct (active before vertex) eqn:old.
  - cbn. split; [discriminate|intros [_ impossible]; discriminate].
  - cbn. split.
    + intros enabled. split; [|reflexivity].
      apply after_support in enabled. destruct enabled as [old_path|new_path];
        [|exact new_path].
      apply before_support in old_path. discriminate.
    + intros [new_path _]. apply after_support. now right.
Qed.

Theorem credited_build_is_quiescent : forall vertices epochs state,
  credited_build vertices epochs state ->
  coherent state /\ quiescent children vertices (occurrences (concat epochs)) state.
Proof.
  intros vertices epochs state build.
  induction build as [|epochs added before after build IH after_coherent epoch].
  - split.
    + intros vertex. reflexivity.
    + intros vertex. cbn [counts active empty_ledger concat occurrences].
      now rewrite incoming_nowhere.
  - destruct IH as [before_coherent before_balance]. split; [exact after_coherent|].
    intros vertex.
    rewrite (credited_epoch_is_quiescent_for_added_roots vertices
      (occurrences (concat epochs)) added before after before_coherent before_balance
      after_coherent epoch vertex).
    unfold add_roots. rewrite concat_app, occurrences_app. cbn [concat].
    rewrite app_nil_r. lia.
Qed.

Lemma quiescent_roots_extensional : forall vertices left right state,
  (forall vertex, left vertex = right vertex) ->
  quiescent children vertices left state -> quiescent children vertices right state.
Proof.
  intros vertices left right state same balance vertex.
  rewrite (balance vertex), (same vertex). reflexivity.
Qed.

Lemma concat_singleton_manifests : forall entries,
  concat (map (fun entry => [entry_manifest entry]) entries) = entry_manifests entries.
Proof.
  induction entries as [|entry rest IH]; [reflexivity|].
  cbn [map concat app]. unfold entry_manifests in *. cbn [map]. now rewrite IH.
Qed.

Definition current_epochs (state : retention_entries) : list (list nat) :=
  [[entry_manifest (current_entry state)]].

Definition retained_epochs (state : retention_entries) : list (list nat) :=
  [entry_manifest (current_entry state)] ::
    map (fun entry => [entry_manifest entry]) (historical_entries state).

Theorem credited_cold_build_realizes_entry_domains :
  forall vertices state current retained,
  credited_build vertices (current_epochs state) current ->
  credited_build vertices (retained_epochs state) retained ->
  (coherent current /\ quiescent children vertices (current_roots state) current) /\
  (coherent retained /\ quiescent children vertices (retained_roots state) retained).
Proof.
  intros vertices state current retained current_build retained_build.
  destruct (credited_build_is_quiescent vertices (current_epochs state) current
    current_build) as [current_coherent current_balance].
  destruct (credited_build_is_quiescent vertices (retained_epochs state) retained
    retained_build) as [retained_coherent retained_balance].
  split; split.
  - exact current_coherent.
  - apply (quiescent_roots_extensional vertices
      (occurrences (concat (current_epochs state)))); [|exact current_balance].
    intros vertex. unfold current_epochs, current_roots. cbn [concat].
    now rewrite app_nil_r.
  - exact retained_coherent.
  - apply (quiescent_roots_extensional vertices
      (occurrences (concat (retained_epochs state)))); [|exact retained_balance].
    intros vertex. unfold retained_epochs, retained_roots, all_entries.
    cbn [concat]. rewrite concat_singleton_manifests, entry_manifests_app, !occurrences_app.
    change (entry_manifests [current_entry state]) with
      [entry_manifest (current_entry state)].
    lia.
Qed.

End Credited.

Definition fixture_children (vertex : nat) : list nat :=
  if Nat.eqb vertex 1 then [0] else
  if Nat.eqb vertex 2 then [1] else
  if Nat.eqb vertex 3 then [2] else [].

Definition fixture_vertices : list nat := [0; 1; 2; 3].

Definition epoch_counts (before : ledger) (added : list nat) (credited : membership)
  : counter :=
  fun vertex => counts before vertex + occurrences added vertex +
    incoming fixture_children fixture_vertices credited vertex.

Definition support (values : counter) : membership := fun vertex => 0 <? values vertex.

Definition shared_child_predecessor : ledger := {|
  counts := fun vertex => if Nat.eqb vertex 0 then 1 else if Nat.eqb vertex 1 then 1 else 0;
  active := fun vertex => Nat.eqb vertex 0 || Nat.eqb vertex 1
|}.

Definition shared_child_crossings : membership := fun vertex => Nat.eqb vertex 2.

Definition shared_child_epoch_visits : membership :=
  fun vertex => Nat.eqb vertex 0 || Nat.eqb vertex 1 || Nat.eqb vertex 2.

Theorem epoch_wide_credit_mutant_double_counts_a_shared_child :
  epoch_counts shared_child_predecessor [2] shared_child_crossings 0 = 1 /\
  epoch_counts shared_child_predecessor [2] shared_child_epoch_visits 0 = 2 /\
  add_roots (occurrences [1]) [2] 0 +
    incoming fixture_children fixture_vertices
      (support (epoch_counts shared_child_predecessor [2] shared_child_epoch_visits)) 0 = 1 /\
  (forall vertex, In vertex fixture_vertices ->
    epoch_counts shared_child_predecessor [2] shared_child_crossings vertex =
      add_roots (occurrences [1]) [2] vertex +
      incoming fixture_children fixture_vertices
        (support (epoch_counts shared_child_predecessor [2] shared_child_crossings)) vertex).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intros vertex member. cbn [fixture_vertices In] in member.
  destruct member as [same|[same|[same|[same|impossible]]]];
    [subst vertex; vm_compute; reflexivity ..|contradiction].
Qed.

Definition chain_crossings : membership :=
  fun vertex => Nat.eqb vertex 0 || Nat.eqb vertex 1 || Nat.eqb vertex 2 ||
    Nat.eqb vertex 3.

Definition no_credits : membership := fun _ => false.

Theorem unexpanded_crossing_mutant_leaves_an_uncredited_child :
  epoch_counts empty_ledger [3] chain_crossings 2 = 1 /\
  epoch_counts empty_ledger [3] no_credits 3 = 1 /\
  epoch_counts empty_ledger [3] no_credits 2 = 0 /\
  add_roots zero_roots [3] 2 +
    incoming fixture_children fixture_vertices
      (support (epoch_counts empty_ledger [3] no_credits)) 2 = 1 /\
  (forall vertex, In vertex fixture_vertices ->
    epoch_counts empty_ledger [3] chain_crossings vertex =
      add_roots zero_roots [3] vertex +
      incoming fixture_children fixture_vertices
        (support (epoch_counts empty_ledger [3] chain_crossings)) vertex).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intros vertex member. cbn [fixture_vertices In] in member.
  destruct member as [same|[same|[same|[same|impossible]]]];
    [subst vertex; vm_compute; reflexivity ..|contradiction].
Qed.

End CreditedClosureLedger.

Print Assumptions CreditedClosureLedger.credited_epoch_is_quiescent_for_added_roots.
Print Assumptions CreditedClosureLedger.credited_epoch_agrees_with_cold_ledger.
Print Assumptions CreditedClosureLedger.credited_epoch_inventory_grows_by_crossed_objects.
Print Assumptions CreditedClosureLedger.credited_crossings_are_newly_reachable_objects.
Print Assumptions CreditedClosureLedger.credited_build_is_quiescent.
Print Assumptions CreditedClosureLedger.credited_cold_build_realizes_entry_domains.
Print Assumptions CreditedClosureLedger.epoch_wide_credit_mutant_double_counts_a_shared_child.
Print Assumptions CreditedClosureLedger.unexpanded_crossing_mutant_leaves_an_uncredited_child.
