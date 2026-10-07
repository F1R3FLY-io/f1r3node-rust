From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingDiscovery FundingSearchOutcome.
Import ListNotations.

Definition fifo_queue_valid count (queue : list nat) cursor :=
  NoDup queue /\ Forall (fun vertex => vertex < count) queue /\ cursor <= length queue.

Definition fifo_queue_step count before cursor after next_cursor :=
  cursor < length before /\
  (exists suffix, after = before ++ suffix) /\
  next_cursor = S cursor /\ fifo_queue_valid count after next_cursor.

Theorem unique_bounded_queue_length : forall count queue,
  NoDup queue -> Forall (fun vertex => vertex < count) queue -> length queue <= count.
Proof.
  intros count queue distinct bounded.
  assert (included : incl queue (seq 0 count)).
  { intros vertex member. apply in_seq. rewrite Forall_forall in bounded. specialize (bounded vertex member). lia. }
  pose proof (NoDup_incl_length distinct included) as size. now rewrite length_seq in size.
Qed.

Lemma discovery_history_extends_queue : forall operations edge state,
  exists suffix, discovery_queue (discovery_history edge state operations) = discovery_queue state ++ suffix.
Proof.
  induction operations as [|[source target] rest IH]; intros edge state; simpl.
  - exists []. now rewrite app_nil_r.
  - destruct (IH edge (discovery_step edge state source target)) as [suffix equal].
    unfold discovery_step in equal |- *.
    destruct (discovery_known state source && edge source target && negb (discovery_known state target)).
    + cbn [discovery_insert discovery_queue] in equal. exists (target :: suffix).
      rewrite equal, <- app_assoc. reflexivity.
    + now exists suffix.
Qed.

Lemma queue_prefix_next : forall cursor (queue suffix : list nat) (fallback : nat),
  cursor < length queue ->
  firstn (S cursor) (queue ++ suffix) = firstn cursor queue ++ [nth cursor queue fallback].
Proof.
  induction cursor as [|cursor IH]; intros queue suffix fallback inside; destruct queue as [|head tail]; simpl in *; try lia.
  - reflexivity.
  - f_equal. apply IH. lia.
Qed.

Lemma current_vertex_is_not_processed : forall cursor (queue : list nat) (fallback : nat),
  NoDup queue -> cursor < length queue -> ~ In (nth cursor queue fallback) (firstn cursor queue).
Proof.
  induction cursor as [|cursor IH]; intros queue fallback distinct inside.
  - simpl. tauto.
  - destruct queue as [|head tail]; [simpl in inside; lia|].
    simpl in inside |- *. apply NoDup_cons_iff in distinct. destruct distinct as [absent distinct].
    intros [equal|included].
    + apply absent. rewrite equal. apply nth_In. lia.
    + exact (IH tail fallback distinct ltac:(lia) included).
Qed.

Theorem fifo_step_processes_exactly_current : forall count before cursor after next_cursor fallback,
  fifo_queue_step count before cursor after next_cursor ->
  firstn next_cursor after = firstn cursor before ++ [nth cursor before fallback].
Proof.
  intros count before cursor after next_cursor fallback [pending [[suffix extends] [advanced valid]]].
  subst after next_cursor. now apply queue_prefix_next.
Qed.

Theorem fifo_current_is_valid_and_fresh : forall count queue cursor fallback,
  fifo_queue_valid count queue cursor -> cursor < length queue ->
  nth cursor queue fallback < count /\ ~ In (nth cursor queue fallback) (firstn cursor queue).
Proof.
  intros count queue cursor fallback [distinct [bounded limited]] pending. split.
  - rewrite Forall_forall in bounded. apply bounded. now apply nth_In.
  - now apply current_vertex_is_not_processed.
Qed.

Theorem discovery_scan_gives_fifo_step : forall count root edge state cursor targets,
  discovery_valid root edge state ->
  Forall (fun vertex => vertex < count) (discovery_queue state) ->
  cursor < length (discovery_queue state) ->
  Forall (fun target => target < count) targets ->
  let source := nth cursor (discovery_queue state) root in
  fifo_queue_step count (discovery_queue state) cursor
    (discovery_queue (discovery_scan edge state source targets)) (S cursor).
Proof.
  intros count root edge state cursor targets valid bounded pending targets_bounded source.
  pose proof valid as initial_valid.
  destruct valid as [distinct [root_known [membership [ranks parents]]]].
  assert (source_known : discovery_known state source = true).
  { apply membership. apply nth_In. exact pending. }
  assert (next_valid : discovery_valid root edge (discovery_scan edge state source targets)).
  { apply discovery_scan_preserves_parents. exact initial_valid. }
  destruct next_valid as [next_distinct [_ [next_membership _]]].
  destruct (discovery_history_extends_queue (map (fun target => (source, target)) targets) edge state) as [suffix extends].
  change (discovery_queue (discovery_scan edge state source targets) = discovery_queue state ++ suffix) in extends.
  unfold fifo_queue_step. split; [exact pending|]. split; [now exists suffix|]. split; [reflexivity|].
  unfold fifo_queue_valid. split; [exact next_distinct|]. split.
  - rewrite Forall_forall. intros vertex member. apply next_membership in member.
    rewrite discovery_scan_known in member by exact source_known.
    apply orb_true_iff in member. destruct member as [old|fresh].
    + rewrite Forall_forall in bounded. apply bounded. now apply membership.
    + apply andb_true_iff in fresh. destruct fresh as [_ appears].
      apply existsb_exists in appears. destruct appears as [target [included same]].
      apply Nat.eqb_eq in same. subst target. rewrite Forall_forall in targets_bounded. now apply targets_bounded.
  - rewrite extends, length_app. lia.
Qed.

Inductive fifo_queue_history count : list nat -> nat -> list nat -> nat -> nat -> Prop :=
| fifo_queue_history_empty : forall queue cursor,
    fifo_queue_history count queue cursor queue cursor 0
| fifo_queue_history_next : forall initial initial_cursor before cursor after next_cursor steps,
    fifo_queue_history count initial initial_cursor before cursor steps ->
    fifo_queue_step count before cursor after next_cursor ->
    fifo_queue_history count initial initial_cursor after next_cursor (S steps).

Theorem fifo_history_counts_processed_vertices : forall count initial initial_cursor final final_cursor steps,
  fifo_queue_history count initial initial_cursor final final_cursor steps ->
  final_cursor = initial_cursor + steps.
Proof.
  intros count initial initial_cursor final final_cursor steps history.
  induction history as [queue cursor|initial initial_cursor before cursor after next_cursor steps history IH step]; [lia|].
  destruct step as [_ [_ [advanced _]]]. lia.
Qed.

Theorem fifo_history_preserves_queue_validity : forall count initial initial_cursor final final_cursor steps,
  fifo_queue_history count initial initial_cursor final final_cursor steps ->
  fifo_queue_valid count initial initial_cursor -> fifo_queue_valid count final final_cursor.
Proof.
  intros count initial initial_cursor final final_cursor steps history valid.
  destruct history as [queue cursor|initial initial_cursor before cursor after next_cursor steps history step]; [exact valid|].
  exact (proj2 (proj2 (proj2 step))).
Qed.

Theorem fifo_history_has_at_most_vertex_count_scans : forall count initial initial_cursor final final_cursor steps,
  fifo_queue_history count initial initial_cursor final final_cursor steps ->
  fifo_queue_valid count initial initial_cursor -> initial_cursor + steps <= count.
Proof.
  intros count initial initial_cursor final final_cursor steps history valid.
  pose proof (fifo_history_counts_processed_vertices _ _ _ _ _ _ history) as processed.
  pose proof (fifo_history_preserves_queue_validity _ _ _ _ _ _ history valid) as [distinct [bounded limited]].
  pose proof (unique_bounded_queue_length count final distinct bounded). lia.
Qed.

Theorem exhausted_fifo_has_processed_every_known_vertex : forall count queue cursor vertex,
  fifo_queue_valid count queue cursor -> ~ cursor < length queue ->
  In vertex queue <-> In vertex (firstn cursor queue).
Proof.
  intros count queue cursor vertex [_ [_ bounded]] exhausted.
  assert (same : cursor = length queue) by lia. subst cursor. now rewrite firstn_all.
Qed.

Print Assumptions unique_bounded_queue_length.
Print Assumptions fifo_step_processes_exactly_current.
Print Assumptions fifo_current_is_valid_and_fresh.
Print Assumptions discovery_scan_gives_fifo_step.
Print Assumptions fifo_history_counts_processed_vertices.
Print Assumptions fifo_history_has_at_most_vertex_count_scans.
Print Assumptions exhausted_fifo_has_processed_every_known_vertex.
