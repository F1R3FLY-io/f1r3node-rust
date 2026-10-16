(* H1, bug 11420 (DR-126, G3 design v5 section 1.1): the forward selection of
   measured prepaid cells.

   The selection `select_measured_demand` (casper prepaid_receipts
   inventory/demand.rs) gives the cells of a captured stack prefix to measured
   demand occurrences. Each occurrence position has one match class: its
   resource shape and its measured authority form. A cell has the match class
   of its own resource and of the current form of its stack cell. A cell can
   take only a free position of its own class. "Free" means outside the
   positions that earlier stacks used and outside the positions that this
   record took.

   The old rule let cell j take the least free position of its class anywhere.
   So a stack [a; b] against occurrences ordered b before a gave the positions
   [1; 0]. The encoder copies positions in cell order, and the canonical check
   of the evidence needs strictly increasing positions in each draw. The run
   then failed after execution with no charge.

   The forward rule lets cell j take the least free position of its class
   above the position of cell j - 1. A miss ends the prefix. So its positions
   strictly increase.

   This module proves the claims of the design:
   - forward_agrees_on_increasing_prefix (Lemma 1): where the old positions
     of a record strictly increase, the forward rule gives the same
     positions.
   - forward_never_lengthens (Lemma 2): the forward prefix of a record is
     never longer than the old prefix.
   - forward_preserves_canonical_selection (the theorem): where every draw of
     the old selection is canonical, the forward selection is the same
     selection. So no valid evidence changes. The draw limit of the Rust scan
     sees the same draws.
   - forward_selection_canonical: every draw of the forward selection is
     canonical.
   The negative control nc_cell_order_positions_noncanonical computes the
   positions [1; 0] of the old rule on the example of the bug.

   Classes and positions are natural numbers. Only the equality of classes
   matters, so this abstraction loses nothing that the selection reads. The
   host work of the scan is outside this model: the forward scan reads fewer
   positions, and no evidence records host work. *)

(* DR-131: host work is removed, so the remark on host work above is
   historical. *)

From Stdlib Require Import Arith Lia Bool List Sorted.
Import ListNotations.

(** * The search for one cell *)

(** Whether position [p] is in the list [ps]. *)
Definition mem (p : nat) (ps : list nat) : bool := existsb (Nat.eqb p) ps.

Lemma mem_true_iff : forall p ps, mem p ps = true <-> In p ps.
Proof.
  intros p ps. unfold mem. rewrite existsb_exists.
  split.
  - intros [q [member same]]. apply Nat.eqb_eq in same. subst. exact member.
  - intros member. exists p. split; [exact member | apply Nat.eqb_refl].
Qed.

Lemma mem_false_iff : forall p ps, mem p ps = false <-> ~ In p ps.
Proof.
  intros p ps. rewrite <- mem_true_iff. destruct (mem p ps); split; congruence.
Qed.

(** Whether position [p] can take a cell of [class]: the occurrence at [p] has
    [class], and [blocked] does not block [p]. *)
Definition candidate (occ : list nat) (class : nat) (blocked : nat -> bool) (p : nat) : bool :=
  match nth_error occ p with
  | Some found => Nat.eqb found class && negb (blocked p)
  | None => false
  end.

(** The least candidate position at or above [from]. *)
Definition search (occ : list nat) (from class : nat) (blocked : nat -> bool) : option nat :=
  find (candidate occ class blocked) (seq from (length occ - from)).

Lemma candidate_in_range : forall occ class blocked p,
  candidate occ class blocked p = true -> p < length occ.
Proof.
  intros occ class blocked p accepted. unfold candidate in accepted.
  destruct (nth_error occ p) eqn:found; [|discriminate].
  apply nth_error_Some. rewrite found. discriminate.
Qed.

(** [find] over a run of consecutive positions returns its least accepted
    position. *)
Lemma find_seq_spec : forall (f : nat -> bool) count start p,
  find f (seq start count) = Some p <->
  start <= p < start + count /\ f p = true /\
  (forall q, start <= q < p -> f q = false).
Proof.
  intros f count. induction count as [|count IH]; intros start p; simpl.
  - split; [discriminate | lia].
  - destruct (f start) eqn:head.
    + split.
      * intros same. inversion same; subst. split; [lia|]. split; [exact head|].
        intros q range. lia.
      * intros [range [accepted least]].
        destruct (Nat.eq_dec p start) as [same|different]; [subst; reflexivity|].
        assert (f start = false) as rejected by (apply least; lia).
        congruence.
    + rewrite IH. split.
      * intros [range [accepted least]]. split; [lia|]. split; [exact accepted|].
        intros q qrange. destruct (Nat.eq_dec q start) as [same|different].
        -- subst. exact head.
        -- apply least. lia.
      * intros [range [accepted least]].
        destruct (Nat.eq_dec p start) as [same|different]; [subst; congruence|].
        split; [lia|]. split; [exact accepted|].
        intros q qrange. apply least. lia.
Qed.

Lemma find_seq_none : forall (f : nat -> bool) count start,
  find f (seq start count) = None <->
  forall q, start <= q < start + count -> f q = false.
Proof.
  intros f count. induction count as [|count IH]; intros start; simpl.
  - split; [intros _ q range; lia | reflexivity].
  - destruct (f start) eqn:head.
    + split; [discriminate|]. intros none. rewrite none in head; [discriminate|lia].
    + rewrite IH. split.
      * intros rest q range. destruct (Nat.eq_dec q start) as [same|different].
        -- subst. exact head.
        -- apply rest. lia.
      * intros all q range. apply all. lia.
Qed.

(** [search] returns the least candidate position at or above [from]. *)
Lemma search_spec : forall occ from class blocked p,
  search occ from class blocked = Some p <->
  from <= p /\ candidate occ class blocked p = true /\
  (forall q, from <= q < p -> candidate occ class blocked q = false).
Proof.
  intros occ from class blocked p. unfold search. rewrite find_seq_spec.
  split.
  - intros [range [accepted least]]. split; [lia|]. split; assumption.
  - intros [low [accepted least]]. pose proof (candidate_in_range _ _ _ _ accepted).
    split; [lia|]. split; assumption.
Qed.

Lemma search_none : forall occ from class blocked,
  search occ from class blocked = None <->
  forall q, from <= q -> candidate occ class blocked q = false.
Proof.
  intros occ from class blocked. unfold search. rewrite find_seq_none.
  split.
  - intros none q low. destruct (candidate occ class blocked q) eqn:accepted; [|reflexivity].
    pose proof (candidate_in_range _ _ _ _ accepted). rewrite <- accepted. apply none. lia.
  - intros none q range. apply none. lia.
Qed.

(** A search from a higher start finds the same position when that position
    lies at or above the start. *)
Lemma search_from_above : forall occ from class blocked p,
  search occ 0 class blocked = Some p -> from <= p ->
  search occ from class blocked = Some p.
Proof.
  intros occ from class blocked p found above.
  apply search_spec in found as [_ [accepted least]].
  apply search_spec. split; [exact above|]. split; [exact accepted|].
  intros q range. apply least. lia.
Qed.

(** A search that finds nothing from position 0 finds nothing from any start. *)
Lemma search_from_none : forall occ from class blocked,
  search occ 0 class blocked = None -> search occ from class blocked = None.
Proof.
  intros occ from class blocked none. apply search_none.
  intros q _. apply (proj1 (search_none occ 0 class blocked) none). lia.
Qed.

(** * The prefix of one record *)

(** The positions that [used] or [taken] block. *)
Definition blocked_by (used taken : list nat) (p : nat) : bool := mem p used || mem p taken.

(** The old rule: cell j takes the least free position of its class. *)
Fixpoint old_prefix (occ used cells taken : list nat) : list nat :=
  match cells with
  | [] => []
  | class :: rest =>
      match search occ 0 class (blocked_by used taken) with
      | None => []
      | Some p => p :: old_prefix occ used rest (taken ++ [p])
      end
  end.

(** The forward rule: cell j takes the least free position of its class at or
    above [from], the position after the previous cell. The rule still checks
    [taken], as the Rust scan checks its candidate markers. *)
Fixpoint forward_prefix (occ used cells taken : list nat) (from : nat) : list nat :=
  match cells with
  | [] => []
  | class :: rest =>
      match search occ from class (blocked_by used taken) with
      | None => []
      | Some p => p :: forward_prefix occ used rest (taken ++ [p]) (S p)
      end
  end.

(** The forward positions strictly increase, and each lies at or above the
    start. *)
Lemma forward_prefix_increasing : forall occ used cells taken from,
  StronglySorted lt (forward_prefix occ used cells taken from) /\
  Forall (le from) (forward_prefix occ used cells taken from).
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken from; simpl.
  - split; constructor.
  - destruct (search occ from class (blocked_by used taken)) as [p|] eqn:found;
      [|split; constructor].
    apply search_spec in found as [low _].
    destruct (IH (taken ++ [p]) (S p)) as [sorted above].
    split.
    + constructor; [exact sorted|]. eapply Forall_impl; [|exact above].
      intros q le_q. cbv beta in *. lia.
    + constructor; [exact low|]. eapply Forall_impl; [|exact above].
      intros q le_q. cbv beta in *. lia.
Qed.

(** Lemma 1, generalized over the taken positions and the start: when every
    old position lies at or above the start and the old positions strictly
    increase, the forward rule gives the same positions. *)
Lemma forward_agrees_from : forall occ used cells taken from,
  Forall (le from) (old_prefix occ used cells taken) ->
  StronglySorted lt (old_prefix occ used cells taken) ->
  forward_prefix occ used cells taken from = old_prefix occ used cells taken.
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken from above sorted;
    simpl in *; [reflexivity|].
  destruct (search occ 0 class (blocked_by used taken)) as [p|] eqn:old_found.
  - inversion above as [|p' l low rest_above]; subst.
    rewrite (search_from_above _ _ _ _ _ old_found low).
    f_equal. inversion sorted as [|p' l rest_sorted head_below]; subst.
    apply IH; [|exact rest_sorted].
    eapply Forall_impl; [|exact head_below]. intros q below. cbv beta in *. lia.
  - rewrite (search_from_none _ from _ _ old_found). reflexivity.
Qed.

(** Lemma 1, agreement (design section 1.1): where the old positions of a
    record strictly increase, the forward rule gives the same positions and
    the same length. *)
Theorem forward_agrees_on_increasing_prefix : forall occ used cells,
  StronglySorted lt (old_prefix occ used cells []) ->
  forward_prefix occ used cells [] 0 = old_prefix occ used cells [].
Proof.
  intros occ used cells sorted. apply forward_agrees_from; [|exact sorted].
  apply Forall_forall. intros q _. lia.
Qed.

(** ** The counting argument of Lemma 2 *)

(** [placed occ used cells ps]: [ps] gives positions to a prefix of [cells].
    Each position holds the class of the cell at the same index and lies
    outside [used]. *)
Fixpoint placed (occ used cells ps : list nat) : Prop :=
  match cells, ps with
  | _, [] => True
  | class :: rest, p :: positions =>
      nth_error occ p = Some class /\ ~ In p used /\ placed occ used rest positions
  | [], _ :: _ => False
  end.

Lemma candidate_true : forall occ class used taken p,
  candidate occ class (blocked_by used taken) p = true ->
  nth_error occ p = Some class /\ ~ In p used /\ ~ In p taken.
Proof.
  intros occ class used taken p accepted. unfold candidate, blocked_by in accepted.
  destruct (nth_error occ p) as [found|] eqn:at_p; [|discriminate].
  apply andb_true_iff in accepted as [same free].
  apply Nat.eqb_eq in same. subst.
  apply negb_true_iff, orb_false_iff in free as [unused untaken].
  split; [reflexivity|]. split; apply mem_false_iff; assumption.
Qed.

Lemma old_prefix_placed : forall occ used cells taken,
  placed occ used cells (old_prefix occ used cells taken).
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken; simpl; [exact I|].
  destruct (search occ 0 class (blocked_by used taken)) as [p|] eqn:found; [|exact I].
  apply search_spec in found as [_ [accepted _]].
  destruct (candidate_true _ _ _ _ _ accepted) as [at_p [unused _]].
  split; [exact at_p|]. split; [exact unused|]. apply IH.
Qed.

Lemma forward_prefix_placed : forall occ used cells taken from,
  placed occ used cells (forward_prefix occ used cells taken from).
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken from; simpl;
    [exact I|].
  destruct (search occ from class (blocked_by used taken)) as [p|] eqn:found; [|exact I].
  apply search_spec in found as [_ [accepted _]].
  destruct (candidate_true _ _ _ _ _ accepted) as [at_p [unused _]].
  split; [exact at_p|]. split; [exact unused|]. apply IH.
Qed.

Lemma nodup_snoc : forall (taken : list nat) p,
  NoDup taken -> ~ In p taken -> NoDup (taken ++ [p]).
Proof.
  intros taken p distinct fresh. apply NoDup_app; [exact distinct | constructor; [easy|constructor] |].
  intros q left right. destruct right as [same|[]]. subst. contradiction.
Qed.

Lemma forward_prefix_nodup : forall occ used cells taken from,
  NoDup taken -> NoDup (taken ++ forward_prefix occ used cells taken from).
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken from distinct; simpl.
  - rewrite app_nil_r. exact distinct.
  - destruct (search occ from class (blocked_by used taken)) as [p|] eqn:found.
    + apply search_spec in found as [_ [accepted _]].
      destruct (candidate_true _ _ _ _ _ accepted) as [_ [_ untaken]].
      pose proof (IH (taken ++ [p]) (S p) (nodup_snoc _ _ distinct untaken)) as rest_distinct.
      rewrite <- app_assoc in rest_distinct. exact rest_distinct.
    + rewrite app_nil_r. exact distinct.
Qed.

Lemma forward_prefix_length : forall occ used cells taken from,
  length (forward_prefix occ used cells taken from) <= length cells.
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken from; simpl; [lia|].
  destruct (search occ from class (blocked_by used taken)); simpl; [|lia].
  specialize (IH (taken ++ [n]) (S n)). lia.
Qed.

(** Where the old prefix ends before the last cell, no free position of the
    next cell's class is left. *)
Lemma old_prefix_stop : forall occ used cells taken,
  length (old_prefix occ used cells taken) < length cells ->
  exists class,
    nth_error cells (length (old_prefix occ used cells taken)) = Some class /\
    search occ 0 class (blocked_by used (taken ++ old_prefix occ used cells taken)) = None.
Proof.
  intros occ used cells. induction cells as [|class rest IH]; intros taken shorter; simpl in *.
  - lia.
  - destruct (search occ 0 class (blocked_by used taken)) as [p|] eqn:found.
    + simpl in *. destruct (IH (taken ++ [p])) as [next [at_next none]]; [lia|].
      exists next. split; [exact at_next|]. rewrite <- app_assoc in none. exact none.
    + exists class. split; [reflexivity|]. rewrite app_nil_r. exact found.
Qed.

Lemma search_none_blocks : forall occ class blocked q,
  search occ 0 class blocked = None -> nth_error occ q = Some class -> blocked q = true.
Proof.
  intros occ class blocked q none at_q.
  pose proof (proj1 (search_none occ 0 class blocked) none q (Nat.le_0_l q)) as rejected.
  unfold candidate in rejected. rewrite at_q, Nat.eqb_refl in rejected. simpl in rejected.
  apply negb_false_iff in rejected. exact rejected.
Qed.

(** The positions of [ps] whose cells have [class]. *)
Fixpoint class_positions (class : nat) (cells ps : list nat) : list nat :=
  match cells, ps with
  | cell :: rest, p :: positions =>
      if Nat.eqb cell class
      then p :: class_positions class rest positions
      else class_positions class rest positions
  | _, _ => []
  end.

Lemma class_positions_in : forall class cells ps q,
  In q (class_positions class cells ps) -> In q ps.
Proof.
  intros class cells. induction cells as [|cell rest IH]; intros ps q member;
    destruct ps as [|p positions]; simpl in *; try contradiction.
  destruct (Nat.eqb cell class); [destruct member as [same|deeper]|]; auto.
Qed.

Lemma class_positions_nodup : forall class cells ps,
  NoDup ps -> NoDup (class_positions class cells ps).
Proof.
  intros class cells. induction cells as [|cell rest IH]; intros ps distinct;
    destruct ps as [|p positions]; simpl; try constructor.
  apply NoDup_cons_iff in distinct as [fresh rest_distinct].
  destruct (Nat.eqb cell class).
  - constructor; [|apply IH; exact rest_distinct].
    intros member. apply fresh. eapply class_positions_in. exact member.
  - apply IH. exact rest_distinct.
Qed.

Lemma class_positions_placed : forall occ used class cells ps q,
  placed occ used cells ps -> In q (class_positions class cells ps) ->
  nth_error occ q = Some class /\ ~ In q used.
Proof.
  intros occ used class cells. induction cells as [|cell rest IH]; intros ps q held member;
    destruct ps as [|p positions]; simpl in *; try contradiction.
  destruct held as [at_p [unused rest_held]].
  destruct (Nat.eqb cell class) eqn:same.
  - apply Nat.eqb_eq in same. subst. destruct member as [same|deeper].
    + subst. split; assumption.
    + eapply IH; eassumption.
  - eapply IH; eassumption.
Qed.

Lemma class_positions_complete : forall occ used class cells ps q,
  placed occ used cells ps -> In q ps -> nth_error occ q = Some class ->
  In q (class_positions class cells ps).
Proof.
  intros occ used class cells. induction cells as [|cell rest IH]; intros ps q held member at_q;
    destruct ps as [|p positions]; simpl in *; try contradiction.
  destruct held as [at_p [_ rest_held]].
  destruct member as [same|deeper].
  - subst. rewrite at_q in at_p. inversion at_p; subst. rewrite Nat.eqb_refl. left. reflexivity.
  - destruct (Nat.eqb cell class); [right|]; eapply IH; eassumption.
Qed.

Lemma class_positions_length : forall class cells ps,
  length ps <= length cells ->
  length (class_positions class cells ps) = count_occ Nat.eq_dec (firstn (length ps) cells) class.
Proof.
  intros class cells. induction cells as [|cell rest IH]; intros ps fits;
    destruct ps as [|p positions]; simpl in *; try lia.
  destruct (Nat.eq_dec cell class) as [same|different].
  - subst. rewrite Nat.eqb_refl. simpl. f_equal. apply IH. lia.
  - apply Nat.eqb_neq in different as different'. rewrite different'. apply IH. lia.
Qed.

Lemma placed_firstn : forall occ used cells ps n,
  placed occ used cells ps -> placed occ used cells (firstn n ps).
Proof.
  intros occ used cells. induction cells as [|cell rest IH]; intros ps n held;
    destruct ps as [|p positions]; destruct n as [|n]; simpl in *; try easy.
  destruct held as [at_p [unused rest_held]]. split; [exact at_p|].
  split; [exact unused|]. apply IH. exact rest_held.
Qed.

Lemma nodup_firstn : forall (ps : list nat) n, NoDup ps -> NoDup (firstn n ps).
Proof.
  intros ps n distinct. rewrite <- (firstn_skipn n ps) in distinct.
  exact (NoDup_app_remove_r _ _ distinct).
Qed.

Lemma firstn_succ_nth : forall (cells : list nat) k class,
  nth_error cells k = Some class -> firstn (S k) cells = firstn k cells ++ [class].
Proof.
  intros cells. induction cells as [|cell rest IH]; intros k class at_k;
    destruct k as [|k]; simpl in *; try discriminate.
  - inversion at_k; subst. reflexivity.
  - f_equal. apply IH. exact at_k.
Qed.

(** Lemma 2, no gain (design section 1.1): the forward prefix of a record is
    never longer than the old prefix. Suppose the forward rule placed cells
    0..k while the old rule stopped at cell k, of class c. Each free position
    of class c then holds one of the old cells before k of class c. The
    forward rule put the class c cells among 0..k on distinct free positions
    of class c. Those cells are one more than the old ones, which is
    impossible. *)
Theorem forward_never_lengthens : forall occ used cells,
  length (forward_prefix occ used cells [] 0) <= length (old_prefix occ used cells []).
Proof.
  intros occ used cells.
  set (old := old_prefix occ used cells []).
  set (fwd := forward_prefix occ used cells [] 0).
  destruct (Nat.le_gt_cases (length fwd) (length old)) as [done|longer]; [exact done|].
  exfalso.
  pose proof (forward_prefix_length occ used cells [] 0) as fwd_fits. fold fwd in fwd_fits.
  assert (stop : length old < length cells) by lia.
  destruct (old_prefix_stop occ used cells [] stop) as [class [at_k none]].
  fold old in at_k, none. simpl in none.
  set (k := length old) in *.
  set (F := class_positions class cells (firstn (S k) fwd)).
  set (O := class_positions class cells old).
  assert (distinct : NoDup F).
  { apply class_positions_nodup, nodup_firstn.
    pose proof (forward_prefix_nodup occ used cells [] 0 (NoDup_nil _)) as all.
    exact all. }
  assert (covered : incl F O).
  { intros q member.
    pose proof (placed_firstn occ used cells fwd (S k) (forward_prefix_placed _ _ _ _ _))
      as held.
    destruct (class_positions_placed _ _ _ _ _ _ held member) as [at_q unused].
    pose proof (search_none_blocks _ _ _ _ none at_q) as blocked.
    unfold blocked_by in blocked. apply orb_true_iff in blocked as [in_used|in_old].
    - apply mem_true_iff in in_used. contradiction.
    - apply mem_true_iff in in_old.
      exact (class_positions_complete _ _ _ _ _ _ (old_prefix_placed _ _ _ _) in_old at_q). }
  pose proof (NoDup_incl_length distinct covered) as bound.
  unfold F, O in bound.
  rewrite (class_positions_length class cells (firstn (S k) fwd)) in bound;
    [|rewrite length_firstn; lia].
  rewrite (class_positions_length class cells old) in bound; [|lia].
  rewrite length_firstn, Nat.min_l in bound; [|lia].
  fold k in bound.
  rewrite (firstn_succ_nth cells k class at_k), count_occ_app in bound.
  simpl in bound. destruct (Nat.eq_dec class class) as [_|absurd]; [lia|congruence].
Qed.

(** * The selection over captured stacks *)

(** The best record so far: its index and its prefix. *)
Definition best_length (best : option (nat * list nat)) : nat :=
  match best with
  | None => 0
  | Some (_, ps) => length ps
  end.

(** The record at [index] replaces the best record only when its prefix is
    strictly longer, as `>` does in the Rust scan. *)
Definition keep (best : option (nat * list nat)) (index : nat) (ps : list nat) :=
  if best_length best <? length ps then Some (index, ps) else best.

Lemma keep_length : forall best index ps,
  best_length (keep best index ps) = Nat.max (best_length best) (length ps).
Proof.
  intros best index ps. unfold keep.
  destruct (best_length best <? length ps) eqn:longer.
  - apply Nat.ltb_lt in longer. simpl. lia.
  - apply Nat.ltb_ge in longer. lia.
Qed.

(** The choice of one stack among the records of its source. [scan] gives
    the prefix of a record. [skip] names the records that the Rust scan skips:
    a record that an earlier stack drew, and a record whose length differs
    from the stack. *)
Fixpoint choose (scan : list nat -> list nat) (skip : nat -> list nat -> bool)
    (records : list (list nat)) (index : nat) (best : option (nat * list nat))
    : option (nat * list nat) :=
  match records with
  | [] => best
  | record :: rest =>
      choose scan skip rest (S index)
        (if skip index record then best else keep best index (scan record))
  end.

Lemma keep_cases : forall best index ps,
  (best_length best < length ps /\ keep best index ps = Some (index, ps)) \/
  (length ps <= best_length best /\ keep best index ps = best).
Proof.
  intros best index ps. unfold keep.
  destruct (best_length best <? length ps) eqn:longer.
  - left. apply Nat.ltb_lt in longer. split; [exact longer | reflexivity].
  - right. apply Nat.ltb_ge in longer. split; [exact longer | reflexivity].
Qed.

Lemma choose_length_ge : forall scan skip records index best,
  best_length best <= best_length (choose scan skip records index best).
Proof.
  intros scan skip records. induction records as [|record rest IH]; intros index best;
    simpl; [lia|].
  destruct (skip index record).
  - apply IH.
  - eapply Nat.le_trans; [|apply IH]. rewrite keep_length. lia.
Qed.

Lemma choose_from : forall scan skip records index best i ps,
  choose scan skip records index best = Some (i, ps) ->
  best = Some (i, ps) \/ exists record, In record records /\ ps = scan record.
Proof.
  intros scan skip records. induction records as [|record rest IH];
    intros index best i ps chosen; simpl in *; [left; exact chosen|].
  destruct (IH _ _ _ _ chosen) as [same|[found [member same]]].
  - destruct (skip index record); [left; exact same|].
    unfold keep in same. destruct (best_length best <? length (scan record)).
    + inversion same; subst. right. exists record. split; [left|]; reflexivity.
    + left. exact same.
  - right. exists found. split; [right; exact member | exact same].
Qed.

Definition option_eq_dec :
  forall left right : option (nat * list nat), {left = right} + {left <> right}.
Proof.
  decide equality. decide equality;
    first [apply Nat.eq_dec | apply (list_eq_dec Nat.eq_dec)].
Defined.

(** The choice under the forward rule equals the choice under the old rule
    when the forward prefixes are never longer, agree on increasing old
    prefixes, and the old choice is canonical. The two scans may keep
    different best records for a while: a forward best can be shorter. Then
    the old scan must replace its best later, with a canonical prefix that
    the forward scan also reaches. *)
Lemma choose_dominated : forall old fwd skip records index bo bf,
  (forall record, In record records -> length (fwd record) <= length (old record)) ->
  (forall record, In record records ->
     StronglySorted lt (old record) -> fwd record = old record) ->
  best_length bf <= best_length bo ->
  (bf = bo \/ choose old skip records index bo <> bo) ->
  (forall i ps, choose old skip records index bo = Some (i, ps) ->
     Some (i, ps) <> bo -> StronglySorted lt ps) ->
  choose fwd skip records index bf = choose old skip records index bo.
Proof.
  intros old fwd skip records. induction records as [|record rest IH];
    intros index bo bf shorter agree dominated same_or_later canonical.
  - cbn [choose] in *. destruct same_or_later as [same|absurd]; [exact same | congruence].
  - cbn [choose] in *.
    assert (rest_shorter : forall r, In r rest -> length (fwd r) <= length (old r))
      by (intros r member; apply shorter; right; exact member).
    assert (rest_agree : forall r, In r rest -> StronglySorted lt (old r) -> fwd r = old r)
      by (intros r member; apply agree; right; exact member).
    assert (record_shorter : length (fwd record) <= length (old record))
      by (apply shorter; left; reflexivity).
    assert (record_agree : StronglySorted lt (old record) -> fwd record = old record)
      by (apply agree; left; reflexivity).
    revert same_or_later canonical.
    destruct (skip index record) eqn:skipped; cbv iota; intros same_or_later canonical.
    + apply IH; assumption.
    + destruct (keep_cases bo index (old record)) as [[longer kept]|[not_longer kept]];
        rewrite kept in *.
      * (* The old scan replaces its best record with this one. *)
        destruct (option_eq_dec (choose old skip rest (S index) (Some (index, old record)))
                    (Some (index, old record))) as [stays|moves].
        -- (* The old scan keeps this record to the end, so its prefix is
              canonical, and the forward scan reaches the same prefix. *)
           assert (sorted : StronglySorted lt (old record)).
           { apply (canonical index (old record)); [exact stays|].
             intros equal. rewrite <- equal in longer. simpl in longer. lia. }
           rewrite (record_agree sorted).
           destruct (keep_cases bf index (old record)) as [[flonger fkept]|[fnot fkept]];
             rewrite fkept.
           ++ apply IH.
              ** exact rest_shorter.
              ** exact rest_agree.
              ** lia.
              ** left. reflexivity.
              ** intros i ps chosen differs. rewrite stays in chosen.
                 inversion chosen; subst. contradiction.
           ++ exfalso. lia.
        -- (* The old scan replaces this record later. *)
           apply IH.
           ++ exact rest_shorter.
           ++ exact rest_agree.
           ++ destruct (keep_cases bf index (fwd record)) as [[flonger fkept]|[fnot fkept]];
                rewrite fkept; simpl; lia.
           ++ right. exact moves.
           ++ intros i ps chosen differs. apply (canonical i ps chosen).
              intros equal.
              pose proof (choose_length_ge old skip rest (S index) (Some (index, old record)))
                as grows.
              rewrite chosen in grows. rewrite <- equal in longer. simpl in grows, longer. lia.
      * (* The old scan keeps its best record. *)
        apply IH.
        -- exact rest_shorter.
        -- exact rest_agree.
        -- destruct (keep_cases bf index (fwd record)) as [[flonger fkept]|[fnot fkept]];
             rewrite fkept; simpl; lia.
        -- destruct same_or_later as [same|moved].
           ++ left. subst bf.
              destruct (keep_cases bo index (fwd record)) as [[flonger fkept]|[fnot fkept]].
              ** exfalso. lia.
              ** exact fkept.
           ++ right. exact moved.
        -- exact canonical.
Qed.

(** One captured stack: its identity, its source, its cell count and the
    match classes of each receipt record of its source, as this stack sees
    them. *)
Record stack := {
  stack_id : nat;
  stack_source : nat;
  stack_length : nat;
  stack_records : list (list nat);
}.

(** One draw of the selection: the stack, the receipt record and the
    positions of its prefix. *)
Record draw := {
  draw_stack : nat;
  draw_receipt : nat;
  draw_positions : list nat;
}.

(** Whether an earlier stack of [source] drew the record at [index]. *)
Definition drawn (selected : list (nat * nat)) (source index : nat) : bool :=
  existsb (fun entry => Nat.eqb (fst entry) source && Nat.eqb (snd entry) index) selected.

Definition skip_record (s : stack) (selected : list (nat * nat)) (index : nat)
    (record : list nat) : bool :=
  drawn selected (stack_source s) index || negb (Nat.eqb (length record) (stack_length s)).

(** The selection: each stack in captured order takes its best record, if
    any. The positions of its prefix become used. *)
Fixpoint select (prefix : list nat -> list nat -> list nat) (stacks : list stack)
    (used : list nat) (selected : list (nat * nat)) : list draw :=
  match stacks with
  | [] => []
  | s :: rest =>
      match choose (prefix used) (skip_record s selected) (stack_records s) 0 None with
      | None => select prefix rest used selected
      | Some (index, ps) =>
          {| draw_stack := stack_id s; draw_receipt := index; draw_positions := ps |}
          :: select prefix rest (used ++ ps) ((stack_source s, index) :: selected)
      end
  end.

Definition old_scan (occ used record : list nat) : list nat := old_prefix occ used record [].

Definition forward_scan (occ used record : list nat) : list nat :=
  forward_prefix occ used record [] 0.

(** The canonical check of the evidence (`sections.rs`): the positions of a
    draw strictly increase. *)
Definition canonical (d : draw) : Prop := Sorted lt (draw_positions d).

(** The theorem of the design: where every draw of the old selection is
    canonical, the forward selection is the same selection. Every committed
    block passed the canonical check, so no committed block changes. *)
Theorem forward_preserves_canonical_selection : forall occ stacks used selected,
  Forall canonical (select (old_scan occ) stacks used selected) ->
  select (forward_scan occ) stacks used selected = select (old_scan occ) stacks used selected.
Proof.
  intros occ stacks. induction stacks as [|s rest IH]; intros used selected all_canonical;
    simpl in *; [reflexivity|].
  assert (same : choose (forward_scan occ used) (skip_record s selected) (stack_records s) 0 None
               = choose (old_scan occ used) (skip_record s selected) (stack_records s) 0 None).
  { apply choose_dominated.
    - intros record _. apply forward_never_lengthens.
    - intros record _ sorted. apply forward_agrees_on_increasing_prefix. exact sorted.
    - simpl. lia.
    - left. reflexivity.
    - intros i positions chosen _. rewrite chosen in all_canonical. simpl in all_canonical.
      inversion all_canonical as [|d ds head_canonical rest_canonical]; subst.
      unfold canonical in head_canonical. simpl in head_canonical.
      apply Sorted_StronglySorted; [exact Nat.lt_trans | exact head_canonical]. }
  rewrite same. revert all_canonical.
  destruct (choose (old_scan occ used) (skip_record s selected) (stack_records s) 0 None)
    as [[index ps]|] eqn:old_choice; intros all_canonical.
  - inversion all_canonical as [|d ds head_canonical rest_canonical]; subst.
    f_equal. apply IH. exact rest_canonical.
  - apply IH. exact all_canonical.
Qed.

(** Every draw of the forward selection is canonical. *)
Theorem forward_selection_canonical : forall occ stacks used selected,
  Forall canonical (select (forward_scan occ) stacks used selected).
Proof.
  intros occ stacks. induction stacks as [|s rest IH]; intros used selected; simpl;
    [constructor|].
  destruct (choose (forward_scan occ used) (skip_record s selected) (stack_records s) 0 None)
    as [[index ps]|] eqn:choice; [|apply IH].
  constructor; [|apply IH].
  destruct (choose_from _ _ _ _ _ _ _ choice) as [absurd|[record [_ same]]];
    [discriminate|].
  unfold canonical. simpl. subst ps. unfold forward_scan.
  apply StronglySorted_Sorted. apply forward_prefix_increasing.
Qed.

(** * Controls *)

(** The negative control: the example of bug 11420. The stack holds a cell of
    class 1 (lane a) and then a cell of class 2 (lane b). The run measured b
    at position 0 and a at position 1. The old rule gives the positions
    [1; 0], which the canonical check rejects. *)
Theorem nc_cell_order_positions_noncanonical :
  old_prefix [2; 1] [] [1; 2] [] = [1; 0] /\ ~ Sorted lt [1; 0].
Proof.
  split; [reflexivity|].
  intros sorted. inversion sorted as [|x l rest head]; subst.
  inversion head as [|y l' below]; subst. lia.
Qed.

(** The same example under the forward rule: cell a takes position 1, and
    cell b finds no free position above it, so the prefix ends. The selection
    draws one cell. *)
Theorem forward_selection_on_the_bug_example :
  let stacks := [{| stack_id := 0; stack_source := 0; stack_length := 2;
                    stack_records := [[1; 2]] |}] in
  select (old_scan [2; 1]) stacks [] [] =
    [{| draw_stack := 0; draw_receipt := 0; draw_positions := [1; 0] |}] /\
  select (forward_scan [2; 1]) stacks [] [] =
    [{| draw_stack := 0; draw_receipt := 0; draw_positions := [1] |}].
Proof. split; reflexivity. Qed.

Print Assumptions forward_agrees_on_increasing_prefix.
Print Assumptions forward_never_lengthens.
Print Assumptions forward_preserves_canonical_selection.
Print Assumptions forward_selection_canonical.
Print Assumptions nc_cell_order_positions_noncanonical.
Print Assumptions forward_selection_on_the_bug_example.
