(* ═══════════════════════════════════════════════════════════════════════════
   CounterConflict.v — cross-branch conflicts of ordered collections (M-1)

   This module models plan revision M-1 for the queue collections of P4.
   Every operation on an ordered collection, peeks and fires that change no
   state included (decision 31), consumes the collection counter S(sid,k)
   and produces it again with the next value, in its own transaction. The
   first operation on a collection consumes the space counter S(sid)
   instead, and opens S(sid,k) at 1. HEAD's merge check #1 then reports a
   race between two branches that use one collection, because both consume
   the same pre-state counter produce. No casper, merger or event change is
   needed (M-1).

   The module restates HEAD's checks #1 and #2, depends and the state
   merge, models the counter bump of each queue operation, and proves:
   - two branches that use one collection race on its counter, for writes
     and for reads (A2)
   - two branches whose first operations open collections of one space
     race on the space counter (A25)
   - two deploys of one block that use one collection form one chain
   - writes without counters (bag stores) commute
   - a merge without a race is the serial run of the two branches
   - a peek that does not bump the counter admits write skew (the reason
     for A2, decision 31).

   Sources
   - Design: the P4 design report B.3 (counters), D.2 (check #1 and the
     two A2 counterexamples), D.3 (one block, one chain) and E.1 (the
     headlines of this module).
   - Plan: revision M-1 ("How M-1 works"), decisions 31 and 34 (A25).
   - Code: the P4 commit of this repository. The line numbers below are the
     line numbers of that commit. The merger files are HEAD files that the
     campaign does not change.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   produce, consume, event  rspace++/src/rspace/trace/event.rs:207-217,
                            315-319, 29-34 (Produce: channel_hash, hash,
                            persistent. Consume: channel_hashes, hash,
                            persistent. COMM: consume, produces, peeks)
   index sets               rspace++/src/rspace/merger/event_log_index.rs:
                            148-381, 253-272 (EventLogIndex::new. A COMM
                            files its produces under produces_peeked when
                            it has peeks, else under produces_consumed)
   check1                   rspace++/src/rspace/merger/merging_logic.rs:
                            279-386 (check #1: the shared non-persistent
                            IO events minus those mergeable in both)
   check2, potential_comms  rspace++/src/rspace/merger/merging_logic.rs:
                            388-429 (check #2: a produce created and not
                            destroyed in one branch on a channel of a
                            consume created and not destroyed in the
                            other)
   produces_created_and_not_destroyed, consumes_created_and_not_destroyed
                            rspace++/src/rspace/merger/merging_logic.rs:
                            526-553, 557-579
   depends                  rspace++/src/rspace/merger/merging_logic.rs:
                            67-138
   in_pre_state             casper/src/rust/merging/block_index.rs:61-65
                            (produce_exists_in_pre_state)
   exec (the bump)          rholang/src/rust/interpreter/spaces/counter.rs:
                            165-245, 251-309 (bump, and take: the COMM has
                            no peeks)
   exec (the plan)          rholang/src/rust/interpreter/spaces/generic.rs:
                            1784-1892 (run_with: bump, then plan)
   list_diff                rspace++/src/rspace/merger/channel_change.rs:
                            74-82 (vec_diff), rspace++/src/rspace/merger/
                            state_change.rs:318-357 (multiset_diff gives
                            the same list)
   vec_union, cancel_common rspace++/src/rspace/merger/channel_change.rs:
                            52-59, 61-72
   combine                  rspace++/src/rspace/merger/channel_change.rs:
                            17-20, 22-28, 36-42 (combine is join, then
                            normalized)
   diff_change              rspace++/src/rspace/merger/state_change.rs:
                            359-392 (compute_value_change)
   entry_of                 rspace++/src/rspace/merger/state_change.rs:
                            294-295 (StateChange::new drops no-op changes)
   combine_entry            rspace++/src/rspace/merger/state_change.rs:
                            436-622 (merge: a channel of one side is kept
                            as it is, and a channel of both sides gets
                            ChannelChange::combine),
                            casper/src/rust/merging/conflict_set_merger.rs:
                            473-505 (the fold from the empty change)
   outcome, apply_change    rspace++/src/rspace/merger/
                            state_change_merger.rs:236-247, 389-509 (a
                            netted change is a no-op. make_trie_action
                            gives the base minus removed, plus added. A
                            removal that the base does not hold, and an
                            unchanged value, are merge errors)
   bag_run                  HEAD's default produce: no counter
   exec_cf, run_cf          the counter-free peek that decision 31 rejects
                            (no code)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A produce and a consume are their channel hashes, their identity hash
     and their persistent flag. The identity is a structured value instead
     of a Blake2b hash: a counter produce names its channel, its value and
     the deploy that made it (Base for the pre-state). Two deploys make
     different counter records, because each record carries the random
     state of its operation. Blake2b is collision free in the model. A
     deploy tag stands for a deploy.
   - Reified channels are GPrivate ids with the tag 0xE0. They are never
     mergeable (reduce.rs is_mergeable_channel matches only ETuple channels
     with a mergeable tag, consensus checklist item 19), so the mergeable
     predicate of the queue model is false.
   - The plan of a collection is a section variable: any deterministic
     function of the operation, its ord and the records of the collection.
     The events of the plan are a section variable too. The race theorems
     need only the counter events, and check #1 never loses a race when a
     log gains events. The chain theorem needs the premise that the plan
     events consume no counter produce (plan_events_data). The FIFO plan
     of WriteSkew satisfies it (qevents_data).
   - The records of D(sid,k) stand for the data, the waiters and the joins
     of the collection: the merge theorem shows that no channel of a
     collection changes in both branches, so it does not need to split
     them. Every consume of the model names one channel, so no join of
     length > 1 exists and check #3 has nothing to see.
   - The merged state of HEAD applies, per channel, the multiset changes of
     the branches to the pre-state (outcome). The merge folds the changes
     in a sorted order. The combine_entry function is symmetric when one
     side has no change, which is the only case that the merge theorem
     meets.
   - The OrderDependenceGuard of conflict_set_merger.rs only observes the
     changes. It does not change the post-state, so the model omits it.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. check1_race_iff                    check #1 finds a race exactly for a
                                          shared non-persistent IO event that
                                          is not mergeable in both logs
    2. ordered_writes_race_on_counter     two branches that write one queue
                                          race on its counter
    3. ordered_reads_race_on_counter      so do two branches that read it
    4. first_ops_race_on_space_counter    first operations race on S(sid)
    5. crossed_collections_race           crossed queues race on both
    6. same_block_ops_chain               two deploys of a block chain
    7. unordered_writes_commute           bag stores need no counter
    8. merge_without_race_is_serializable a merged state is a serial state
    9. counter_free_peek_admits_write_skew  the reason for A2 (computed)

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `destruct (t) eqn:E` also rewrites t in the hypotheses (Rocq 9.1.1):
     a premise `t = None -> P` became `None = None -> P`, and `exact E`
     then fails. Use `first [exact E | reflexivity]`.
   - consumed_counter_below first rewrote with exec_sc_self at once, but
     the operation that consumed the counter can be on another collection.
     Derive the collection first (exec_consumed_counter).
   - `set` followed by `destruct` of the set variable is awkward. Use
     `remember`.
   - An outcome without the netted no-op guard makes a merged change that
     cancels to nothing a merge error. HEAD skips such a change
     (state_change_merger.rs:236-247), so outcome skips it too.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

(* ── 1. Events ─────────────────────────────────────────────────────────── *)

Record produce (Chan Pid : Type) : Type := mkProduce { p_ch : Chan; p_id : Pid; p_pers : bool }.
Record consume (Chan Cid : Type) : Type := mkConsume { c_chs : list Chan; c_id : Cid; c_pers : bool }.

Arguments mkProduce {Chan Pid}.
Arguments p_ch {Chan Pid}.
Arguments p_id {Chan Pid}.
Arguments p_pers {Chan Pid}.
Arguments mkConsume {Chan Cid}.
Arguments c_chs {Chan Cid}.
Arguments c_id {Chan Cid}.
Arguments c_pers {Chan Cid}.

Inductive event (Chan Pid Cid : Type) : Type :=
| EvProduce : produce Chan Pid -> event Chan Pid Cid
| EvConsume : consume Chan Cid -> event Chan Pid Cid
| EvComm : consume Chan Cid -> list (produce Chan Pid) -> bool -> event Chan Pid Cid.

Arguments EvProduce {Chan Pid Cid}.
Arguments EvConsume {Chan Pid Cid}.
Arguments EvComm {Chan Pid Cid}.

Section CheckOne.
  Variables Chan Pid Cid : Type.
  Variable chan_eq_dec : forall x y : Chan, {x = y} + {x <> y}.
  Variable pid_eq_dec : forall x y : Pid, {x = y} + {x <> y}.
  Variable cid_eq_dec : forall x y : Cid, {x = y} + {x <> y}.
  (* The mergeable channels of the block (the number channels). *)
  Variable mergeable : Chan -> bool.
  (* produce_exists_in_pre_state of block_index.rs. *)
  Variable in_pre_state : produce Chan Pid -> bool.

  Notation produce := (produce Chan Pid).
  Notation consume := (consume Chan Cid).
  Notation event := (event Chan Pid Cid).

  Definition produce_eq_dec : forall x y : produce, {x = y} + {x <> y}.
  Proof.
    intros [c1 i1 p1] [c2 i2 p2].
    destruct (chan_eq_dec c1 c2) as [-> | Hc]; [| right; intros H; injection H; intros; contradiction].
    destruct (pid_eq_dec i1 i2) as [-> | Hi]; [| right; intros H; injection H; intros; contradiction].
    destruct (Bool.bool_dec p1 p2) as [-> | Hp]; [left; reflexivity |].
    right; intros H; injection H; intros; contradiction.
  Defined.

  Definition consume_eq_dec : forall x y : consume, {x = y} + {x <> y}.
  Proof.
    intros [c1 i1 p1] [c2 i2 p2].
    destruct (list_eq_dec chan_eq_dec c1 c2) as [-> | Hc];
      [| right; intros H; injection H; intros; contradiction].
    destruct (cid_eq_dec i1 i2) as [-> | Hi]; [| right; intros H; injection H; intros; contradiction].
    destruct (Bool.bool_dec p1 p2) as [-> | Hp]; [left; reflexivity |].
    right; intros H; injection H; intros; contradiction.
  Defined.

  Definition inp (p : produce) (l : list produce) : bool :=
    existsb (fun q => if produce_eq_dec p q then true else false) l.
  Definition inc (c : consume) (l : list consume) : bool :=
    existsb (fun d => if consume_eq_dec c d then true else false) l.

  Lemma inp_iff : forall p l, inp p l = true <-> In p l.
  Proof.
    intros p l. unfold inp. rewrite existsb_exists. split.
    - intros [q [Hin Heq]]. destruct (produce_eq_dec p q) as [-> | _]; [exact Hin | discriminate].
    - intros Hin. exists p. split; [exact Hin |]. destruct (produce_eq_dec p p); [reflexivity | contradiction].
  Qed.

  Lemma inc_iff : forall c l, inc c l = true <-> In c l.
  Proof.
    intros c l. unfold inc. rewrite existsb_exists. split.
    - intros [d [Hin Heq]]. destruct (consume_eq_dec c d) as [-> | _]; [exact Hin | discriminate].
    - intros Hin. exists c. split; [exact Hin |]. destruct (consume_eq_dec c c); [reflexivity | contradiction].
  Qed.

  (* ── 2. The event-log index (EventLogIndex::new) ─────────────────────── *)

  Definition produces_linear (log : list event) : list produce :=
    flat_map (fun e => match e with EvProduce p => if p_pers p then [] else [p] | _ => [] end) log.
  Definition produces_persistent (log : list event) : list produce :=
    flat_map (fun e => match e with EvProduce p => if p_pers p then [p] else [] | _ => [] end) log.
  Definition produces_copied_by_peek (log : list event) : list produce :=
    flat_map (fun e => match e with EvProduce p => if in_pre_state p then [p] else [] | _ => [] end) log.
  Definition consumes_linear_and_peeks (log : list event) : list consume :=
    flat_map (fun e => match e with EvConsume c => if c_pers c then [] else [c] | _ => [] end) log.
  Definition consumes_persistent (log : list event) : list consume :=
    flat_map (fun e => match e with EvConsume c => if c_pers c then [c] else [] | _ => [] end) log.
  (* A COMM files its produces under produces_peeked when it has peeks and
     under produces_consumed otherwise. Every COMM files its consume. *)
  Definition produces_consumed (log : list event) : list produce :=
    flat_map (fun e => match e with EvComm _ ps false => ps | _ => [] end) log.
  Definition produces_peeked (log : list event) : list produce :=
    flat_map (fun e => match e with EvComm _ ps true => ps | _ => [] end) log.
  Definition consumes_produced (log : list event) : list consume :=
    flat_map (fun e => match e with EvComm c _ _ => [c] | _ => [] end) log.

  Definition produces_mergeable (log : list event) : list produce :=
    filter (fun p => mergeable (p_ch p))
      (produces_linear log ++ produces_persistent log ++ produces_consumed log ++ produces_peeked log).
  Definition consumes_mergeable (log : list event) : list consume :=
    filter (fun c => existsb mergeable (c_chs c))
      (consumes_linear_and_peeks log ++ consumes_persistent log ++ consumes_produced log).

  (* ── 3. Check #1 and depends ───────────────────────────────────────── *)

  Definition produce_races (a b : list event) : list produce :=
    filter (fun p => inp p (produces_consumed b)
                     && negb (inp p (produces_mergeable a) && inp p (produces_mergeable b))
                     && negb (p_pers p))
           (produces_consumed a).

  Definition consume_races (a b : list event) : list consume :=
    filter (fun c => inc c (consumes_produced b)
                     && negb (inc c (consumes_mergeable a) && inc c (consumes_mergeable b))
                     && negb (c_pers c))
           (consumes_produced a).

  (* Check #1: the channels of the races for one IO event. *)
  Definition check1 (a b : list event) : list Chan :=
    flat_map (fun c => c_chs c) (consume_races a b) ++ map (fun p => p_ch p) (produce_races a b).

  Definition produces_created_and_not_destroyed (log : list event) : list produce :=
    filter (fun p => negb (inp p (produces_copied_by_peek log)))
      (filter (fun p => negb (inp p (produces_consumed log))) (produces_linear log)
       ++ produces_persistent log).

  Definition consumes_created_and_not_destroyed (log : list event) : list consume :=
    filter (fun c => negb (inc c (consumes_produced log))) (consumes_linear_and_peeks log)
    ++ consumes_persistent log.

  (* depends(target, source). *)
  Definition depends (target source : list event) : bool :=
    existsb (fun p => inp p (produces_consumed target) && negb (inp p (produces_mergeable source)))
      (filter (fun p => negb (inp p (produces_mergeable source)))
         (produces_created_and_not_destroyed source))
    || existsb (fun c => inc c (consumes_produced target)) (consumes_created_and_not_destroyed source).

  Lemma app_neq_nil : forall (A : Type) (l1 l2 : list A), l1 ++ l2 <> [] <-> l1 <> [] \/ l2 <> [].
  Proof.
    intros A l1 l2. split.
    - intros H. destruct l1 as [| x l1]; [right; exact H | left; discriminate].
    - intros [H | H] Heq; apply app_eq_nil in Heq as [H1 H2]; contradiction.
  Qed.

  Lemma neq_nil_exists : forall (A : Type) (l : list A), l <> [] <-> exists x, In x l.
  Proof.
    intros A [| x l]; split; intros H.
    - contradiction H; reflexivity.
    - destruct H as [y []].
    - exists x; left; reflexivity.
    - discriminate.
  Qed.

  Lemma flat_map_neq_nil :
    forall (A B : Type) (f : A -> list B) l, flat_map f l <> [] <-> exists x, In x l /\ f x <> [].
  Proof.
    intros A B f l. rewrite neq_nil_exists. split.
    - intros [y Hy]. apply in_flat_map in Hy as [x [Hx Hy]]. exists x. split; [exact Hx |].
      intros Hnil. rewrite Hnil in Hy. destruct Hy.
    - intros [x [Hx Hne]]. apply neq_nil_exists in Hne as [y Hy].
      exists y. apply in_flat_map. exists x. split; assumption.
  Qed.

  Lemma map_neq_nil : forall (A B : Type) (f : A -> B) l, map f l <> [] <-> l <> [].
  Proof. intros A B f [| x l]; simpl; split; intros H; try discriminate; contradiction H; reflexivity. Qed.

  (* Headline 1. check #1 reports a channel exactly when the two logs share
     a non-persistent produce that both consume without a peek, or a
     non-persistent consume that both COMM, and the shared event is not
     mergeable in both logs. *)
  Theorem check1_race_iff :
    forall a b,
      check1 a b <> [] <->
      (exists p, In p (produces_consumed a) /\ In p (produces_consumed b) /\
                 ~ (In p (produces_mergeable a) /\ In p (produces_mergeable b)) /\
                 p_pers p = false) \/
      (exists c, In c (consumes_produced a) /\ In c (consumes_produced b) /\
                 ~ (In c (consumes_mergeable a) /\ In c (consumes_mergeable b)) /\
                 c_pers c = false /\ c_chs c <> []).
  Proof.
    intros a b. unfold check1. rewrite app_neq_nil, flat_map_neq_nil, map_neq_nil, neq_nil_exists.
    unfold produce_races, consume_races. split.
    - intros [[c [Hc Hne]] | [p Hp]].
      + right. apply filter_In in Hc as [Hin Hcond].
        apply andb_true_iff in Hcond as [Hcond Hpers]. apply andb_true_iff in Hcond as [Hb Hm].
        apply inc_iff in Hb. apply negb_true_iff in Hpers. apply negb_true_iff in Hm.
        exists c. split; [exact Hin | split; [exact Hb | split; [| split; [exact Hpers | exact Hne]]]].
        intros [Ha' Hb']. apply inc_iff in Ha'. apply inc_iff in Hb'. rewrite Ha', Hb' in Hm. discriminate.
      + left. apply filter_In in Hp as [Hin Hcond].
        apply andb_true_iff in Hcond as [Hcond Hpers]. apply andb_true_iff in Hcond as [Hb Hm].
        apply inp_iff in Hb. apply negb_true_iff in Hpers. apply negb_true_iff in Hm.
        exists p. split; [exact Hin | split; [exact Hb | split; [| exact Hpers]]].
        intros [Ha' Hb']. apply inp_iff in Ha'. apply inp_iff in Hb'. rewrite Ha', Hb' in Hm. discriminate.
    - intros [[p [H1 [H2 [H3 H4]]]] | [c [H1 [H2 [H3 [H4 H5]]]]]].
      + right. exists p. apply filter_In. split; [exact H1 |].
        rewrite (proj2 (inp_iff p _) H2), H4. simpl. rewrite andb_true_r.
        destruct (inp p (produces_mergeable a)) eqn:Ea; destruct (inp p (produces_mergeable b)) eqn:Eb;
          try reflexivity.
        exfalso. apply H3. split; apply inp_iff; assumption.
      + left. exists c. split; [| exact H5]. apply filter_In. split; [exact H1 |].
        rewrite (proj2 (inc_iff c _) H2), H4. simpl. rewrite andb_true_r.
        destruct (inc c (consumes_mergeable a)) eqn:Ea; destruct (inc c (consumes_mergeable b)) eqn:Eb;
          try reflexivity.
        exfalso. apply H3. split; apply inc_iff; assumption.
  Qed.

  (* A non-persistent produce on a channel that is not mergeable, consumed
     without a peek in both logs, is a race of check #1. *)
  Lemma check1_produce_race :
    forall a b p,
      In p (produces_consumed a) -> In p (produces_consumed b) ->
      p_pers p = false -> mergeable (p_ch p) = false -> check1 a b <> [].
  Proof.
    intros a b p Ha Hb Hp Hm. apply check1_race_iff. left. exists p.
    split; [exact Ha | split; [exact Hb | split; [| exact Hp]]].
    intros [Hma _]. unfold produces_mergeable in Hma. apply filter_In in Hma as [_ Hm']. congruence.
  Qed.

  Lemma produces_consumed_app :
    forall l1 l2, produces_consumed (l1 ++ l2) = produces_consumed l1 ++ produces_consumed l2.
  Proof. intros l1 l2. unfold produces_consumed. apply flat_map_app. Qed.

  (* A target depends on a source when the target consumes, without a peek,
     a produce that the source made, did not consume, did not copy from the
     pre-state, and did not mark mergeable. *)
  Lemma depends_of_produce :
    forall target source p,
      In p (produces_linear source) -> ~ In p (produces_consumed source) ->
      ~ In p (produces_copied_by_peek source) -> mergeable (p_ch p) = false ->
      In p (produces_consumed target) -> depends target source = true.
  Proof.
    intros target source p Hlin Hncons Hncopy Hm Htarget.
    unfold depends. apply orb_true_iff. left. apply existsb_exists. exists p.
    assert (Hnm : inp p (produces_mergeable source) = false).
    { destruct (inp p (produces_mergeable source)) eqn:E; [| reflexivity].
      apply inp_iff in E. unfold produces_mergeable in E. apply filter_In in E as [_ E]. congruence. }
    split.
    - apply filter_In. split; [| rewrite Hnm; reflexivity].
      unfold produces_created_and_not_destroyed. apply filter_In. split.
      + apply in_or_app. left. apply filter_In. split; [exact Hlin |].
        destruct (inp p (produces_consumed source)) eqn:E; [| reflexivity].
        apply inp_iff in E. contradiction.
      + destruct (inp p (produces_copied_by_peek source)) eqn:E; [| reflexivity].
        apply inp_iff in E. contradiction.
    - rewrite (proj2 (inp_iff p _) Htarget), Hnm. reflexivity.
  Qed.

  (* Check #2 (potential COMMs): a produce that one branch creates and does
     not destroy, on a channel of a consume that the other branch creates
     and does not destroy. Both directions. *)
  Definition potential_comms (left right : list event) : list Chan :=
    flat_map (fun p => if existsb (fun c => existsb (fun ch => if chan_eq_dec ch (p_ch p) then true else false)
                                                      (c_chs c))
                                  (consumes_created_and_not_destroyed right)
                       then [p_ch p] else [])
             (produces_created_and_not_destroyed left).

  Definition check2 (a b : list event) : list Chan := potential_comms a b ++ potential_comms b a.
End CheckOne.

(* ── 3b. Multisets: vec_diff, vec_union, cancel_common ──────────────── *)

Section Multiset.
  Variable A : Type.
  Variable eq_dec : forall x y : A, {x = y} + {x <> y}.

  Fixpoint remove_first (x : A) (l : list A) : list A :=
    match l with
    | [] => []
    | y :: rest => if eq_dec x y then rest else y :: remove_first x rest
    end.

  (* vec_diff of channel_change.rs: remove each element of r once from l,
     at its first position. multiset_diff of state_change.rs gives the
     same list: it also drops the first occurrences. *)
  Definition list_diff (l r : list A) : list A := fold_left (fun acc x => remove_first x acc) r l.

  (* vec_union of channel_change.rs. *)
  Definition vec_union (left right : list A) : list A := left ++ list_diff right left.

  (* cancel_common of channel_change.rs walks added in order. An element
     that removed holds leaves both lists (removed loses its first copy). *)
  Fixpoint cancel_common (added removed : list A) : list A * list A :=
    match added with
    | [] => ([], removed)
    | a :: rest =>
        if in_dec eq_dec a removed then cancel_common rest (remove_first a removed)
        else let r := cancel_common rest removed in (a :: fst r, snd r)
    end.

  (* No removed item is absent from the base (the unmatched check of
     make_trie_action in state_change_merger.rs). *)
  Definition sub_multiset (r l : list A) : bool :=
    forallb (fun x => count_occ eq_dec r x <=? count_occ eq_dec l x) r.

  Lemma cnt_cons :
    forall x l y, count_occ eq_dec (x :: l) y = (if eq_dec x y then 1 else 0) + count_occ eq_dec l y.
  Proof. intros x l y. cbn. destruct (eq_dec x y); reflexivity. Qed.

  Lemma cnt_remove_first :
    forall x l y,
      count_occ eq_dec (remove_first x l) y =
      if eq_dec x y then count_occ eq_dec l y - 1 else count_occ eq_dec l y.
  Proof.
    intros x l y. induction l as [| z rest IH]; cbn [remove_first].
    - destruct (eq_dec x y); reflexivity.
    - destruct (eq_dec x z) as [<- | Hxz].
      + rewrite cnt_cons. destruct (eq_dec x y); lia.
      + rewrite !cnt_cons, IH. destruct (eq_dec x y) as [<- | Hxy]; [| reflexivity].
        destruct (eq_dec z x) as [-> | _]; [contradiction | lia].
  Qed.

  Lemma cnt_list_diff :
    forall r l y, count_occ eq_dec (list_diff l r) y = count_occ eq_dec l y - count_occ eq_dec r y.
  Proof.
    induction r as [| x r IH]; intros l y.
    - cbn. lia.
    - change (list_diff l (x :: r)) with (list_diff (remove_first x l) r).
      rewrite IH, cnt_remove_first, cnt_cons. destruct (eq_dec x y); lia.
  Qed.

  Lemma cnt_zero_nil : forall l, (forall y, count_occ eq_dec l y = 0) -> l = [].
  Proof.
    intros [| x l] H; [reflexivity |]. specialize (H x). rewrite cnt_cons in H.
    destruct (eq_dec x x); [lia | contradiction].
  Qed.

  Lemma perm_iff_cnt :
    forall l1 l2, Permutation l1 l2 <-> forall y, count_occ eq_dec l1 y = count_occ eq_dec l2 y.
  Proof. intros l1 l2. apply Permutation_count_occ. Qed.

  Lemma sub_multiset_iff :
    forall r l, sub_multiset r l = true <-> forall y, count_occ eq_dec r y <= count_occ eq_dec l y.
  Proof.
    intros r l. unfold sub_multiset. rewrite forallb_forall. split.
    - intros H y. destruct (in_dec eq_dec y r) as [Hin | Hnot].
      + apply Nat.leb_le. exact (H y Hin).
      + apply (count_occ_not_In eq_dec) in Hnot. rewrite Hnot. lia.
    - intros H x _. apply Nat.leb_le. apply H.
  Qed.

  Lemma list_diff_self : forall l, list_diff l l = [].
  Proof. intros l. apply cnt_zero_nil. intros y. rewrite cnt_list_diff. lia. Qed.

  Lemma cancel_common_nil_r : forall l, cancel_common l [] = (l, []).
  Proof.
    induction l as [| a rest IH]; [reflexivity |]. cbn [cancel_common].
    destruct (in_dec eq_dec a []) as [[] |]. rewrite IH. reflexivity.
  Qed.
End Multiset.

(* ── 4. The queue spaces and the counter bump ──────────────────────────── *)

Section Spaces.
  Variables Coll Space D O : Type.
  Variable coll_eq_dec : forall x y : Coll, {x = y} + {x <> y}.
  Variable space_eq_dec : forall x y : Space, {x = y} + {x <> y}.
  Variable d_eq_dec : forall x y : D, {x = y} + {x <> y}.
  Variable space_of : Coll -> Space.

  (* The operations of a queue: an enqueue writes, a dequeue removes, a peek
     reads. Each runs in its own transaction. *)
  Inductive qop : Type := QEnqueue (d : D) | QDequeue | QPeek.

  (* The channels of a space: D(sid,k), S(sid,k) and S(sid). *)
  Inductive chan : Type := ChD (k : Coll) | ChS (k : Coll) | ChSP (s : Space).

  (* The maker of a record: the pre-state, or the deploy with a tag. *)
  Inductive creator : Type := Base | Made (tag : nat).

  (* The identity of a produce. A counter record names its channel, its
     value and its maker. A data record has an opaque identity. *)
  Inductive pid : Type :=
  | PidCounter : chan -> nat -> creator -> pid
  | PidData : nat -> creator -> pid.

  (* The identity of a consume: the counter continuation of a deploy, or an
     opaque continuation. *)
  Inductive cid : Type :=
  | CidCounter : nat -> chan -> nat -> cid
  | CidData : nat -> nat -> cid.

  Definition chan_eq_dec : forall x y : chan, {x = y} + {x <> y}.
  Proof. decide equality. Defined.
  Definition creator_eq_dec : forall x y : creator, {x = y} + {x <> y}.
  Proof. decide equality; apply Nat.eq_dec. Defined.
  Definition pid_eq_dec : forall x y : pid, {x = y} + {x <> y}.
  Proof.
    decide equality; try apply Nat.eq_dec; try apply creator_eq_dec; apply chan_eq_dec.
  Defined.
  Definition cid_eq_dec : forall x y : cid, {x = y} + {x <> y}.
  Proof. decide equality; try apply Nat.eq_dec; apply chan_eq_dec. Defined.

  Notation ev := (event chan pid cid).

  (* Reified channels are never mergeable. *)
  Definition reified_mergeable (c : chan) : bool := false.

  (* The plan of a collection operation: the new records of the collection
     and the observation (the fires and replies) of the operation. *)
  Variable plan : qop -> nat -> list D -> list D * O.
  (* The events of the plan: the store, park and fire events on D(sid,k). *)
  Variable plan_events : nat -> Coll -> qop -> nat -> list D -> list ev.

  (* The space state: the records of each collection, the collection
     counters (None before the first operation) and the space counters
     (made by the factory). Each counter carries its maker. *)
  Record store : Type := mkStore {
    sd : Coll -> list D;
    sc : Coll -> option (nat * creator);
    ss : Space -> nat * creator
  }.

  Definition upd_c {A : Type} (f : Coll -> A) (k : Coll) (a : A) : Coll -> A :=
    fun k' => if coll_eq_dec k k' then a else f k'.
  Definition upd_s {A : Type} (f : Space -> A) (s : Space) (a : A) : Space -> A :=
    fun s' => if space_eq_dec s s' then a else f s'.

  Lemma upd_c_eq : forall (A : Type) f k (a : A), upd_c f k a k = a.
  Proof. intros A f k a. unfold upd_c. destruct (coll_eq_dec k k); [reflexivity | contradiction]. Qed.
  Lemma upd_c_neq : forall (A : Type) f k k' (a : A), k <> k' -> upd_c f k a k' = f k'.
  Proof. intros A f k k' a H. unfold upd_c. destruct (coll_eq_dec k k'); [contradiction | reflexivity]. Qed.
  Lemma upd_s_eq : forall (A : Type) f s (a : A), upd_s f s a s = a.
  Proof. intros A f s a. unfold upd_s. destruct (space_eq_dec s s); [reflexivity | contradiction]. Qed.
  Lemma upd_s_neq : forall (A : Type) f s s' (a : A), s <> s' -> upd_s f s a s' = f s'.
  Proof. intros A f s s' a H. unfold upd_s. destruct (space_eq_dec s s'); [contradiction | reflexivity]. Qed.

  Definition counter_produce (c : chan) (v : nat) (cr : creator) : produce chan pid :=
    mkProduce c (PidCounter c v cr) false.

  Record step_result : Type := mkStep { x_store : store; x_obs : O; x_log : list ev }.

  (* One collection operation of the deploy tag (counter.rs bump, then the
     plan of generic.rs). A collection with a counter v takes the ord v,
     consumes S(sid,k) = v and produces v + 1. The first operation on a
     collection consumes S(sid) = m, produces m + 1, opens S(sid,k) at 1,
     and takes the ord 0. The counter COMM has no peeks. *)
  Definition exec (tag : nat) (st : store) (k : Coll) (q : qop) : step_result :=
    match sc st k with
    | Some (v, cr) =>
        let kont := mkConsume [ChS k] (CidCounter tag (ChS k) v) false in
        let pl := plan q v (sd st k) in
        mkStep (mkStore (upd_c (sd st) k (fst pl)) (upd_c (sc st) k (Some (S v, Made tag))) (ss st))
          (snd pl)
          ([EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] false;
            EvProduce (counter_produce (ChS k) (S v) (Made tag))] ++ plan_events tag k q v (sd st k))
    | None =>
        let s := space_of k in
        let m := fst (ss st s) in
        let crs := snd (ss st s) in
        let kont := mkConsume [ChSP s] (CidCounter tag (ChSP s) m) false in
        let pl := plan q 0 (sd st k) in
        mkStep (mkStore (upd_c (sd st) k (fst pl)) (upd_c (sc st) k (Some (1, Made tag)))
                        (upd_s (ss st) s (S m, Made tag)))
          (snd pl)
          ([EvConsume kont; EvComm kont [counter_produce (ChSP s) m crs] false;
            EvProduce (counter_produce (ChSP s) (S m) (Made tag));
            EvProduce (counter_produce (ChS k) 1 (Made tag))] ++ plan_events tag k q 0 (sd st k))
    end.

  Record run_result : Type := mkRun { r_store : store; r_obs : list O; r_log : list ev }.

  (* A deploy: its operations, in order, each in its own transaction. *)
  Fixpoint run (tag : nat) (st : store) (ops : list (Coll * qop)) : run_result :=
    match ops with
    | [] => mkRun st [] []
    | (k, q) :: rest =>
        let x := exec tag st k q in
        let r := run tag (x_store x) rest in
        mkRun (r_store r) (x_obs x :: r_obs r) (x_log x ++ r_log r)
    end.

  Definition consumed (log : list ev) : list (produce chan pid) := produces_consumed chan pid cid log.

  Definition check1_spaces (a b : list ev) : list chan :=
    check1 chan pid cid chan_eq_dec pid_eq_dec cid_eq_dec reified_mergeable a b.

  Lemma consumed_app : forall l1 l2, consumed (l1 ++ l2) = consumed l1 ++ consumed l2.
  Proof. intros l1 l2. apply produces_consumed_app. Qed.

  Lemma exec_sc_other :
    forall tag st k q k', k <> k' -> sc (x_store (exec tag st k q)) k' = sc st k'.
  Proof.
    intros tag st k q k' H. unfold exec.
    destruct (sc st k) as [[v cr] |]; cbn [x_store sc]; apply upd_c_neq; exact H.
  Qed.

  Lemma exec_ss_other :
    forall tag st k q s, (sc st k <> None \/ space_of k <> s) -> ss (x_store (exec tag st k q)) s = ss st s.
  Proof.
    intros tag st k q s H. unfold exec.
    destruct (sc st k) as [[v cr] |] eqn:E; cbn [x_store ss]; [reflexivity |].
    apply upd_s_neq. destruct H as [H | H]; [contradiction | exact H].
  Qed.

  (* The first operation on k in a deploy consumes the counter of k that it
     finds. *)
  Lemma counter_consumed :
    forall tag ops st k v cr,
      sc st k = Some (v, cr) -> In k (map fst ops) ->
      In (counter_produce (ChS k) v cr) (consumed (r_log (run tag st ops))).
  Proof.
    induction ops as [| [k' q] rest IH]; intros st k v cr Hsc Hin; [destruct Hin |].
    cbn [run r_log]. rewrite consumed_app. apply in_or_app.
    destruct (coll_eq_dec k' k) as [-> | Hne].
    - left. unfold exec. rewrite Hsc. cbn [x_log]. unfold consumed.
      rewrite produces_consumed_app. apply in_or_app. left. cbn. left. reflexivity.
    - right. apply IH; [rewrite exec_sc_other by exact Hne; exact Hsc |].
      destruct Hin as [Heq | Hin]; [cbn in Heq; contradiction | exact Hin].
  Qed.

  (* A deploy that opens a collection of the space s consumes the space
     counter of s that it finds. *)
  Lemma space_counter_consumed :
    forall tag ops st s,
      (exists k, In k (map fst ops) /\ space_of k = s /\ sc st k = None) ->
      In (counter_produce (ChSP s) (fst (ss st s)) (snd (ss st s))) (consumed (r_log (run tag st ops))).
  Proof.
    induction ops as [| [k' q] rest IH]; intros st s [k [Hin [Hs Hnone]]]; [destruct Hin |].
    cbn [run r_log]. rewrite consumed_app. apply in_or_app.
    destruct (sc st k') as [[v cr] |] eqn:Ek'.
    - right. rewrite <- (exec_ss_other tag st k' q s) by (left; rewrite Ek'; discriminate).
      apply IH. exists k. destruct Hin as [Heq | Hin].
      + cbn in Heq. subst k'. rewrite Hnone in Ek'. discriminate.
      + split; [exact Hin | split; [exact Hs |]].
        rewrite exec_sc_other; [exact Hnone |]. intros ->. rewrite Hnone in Ek'. discriminate.
    - destruct (space_eq_dec (space_of k') s) as [Hs' | Hs'].
      + left. rewrite <- Hs'. unfold exec. rewrite Ek'. cbn [x_log]. unfold consumed.
        rewrite produces_consumed_app. apply in_or_app. left. cbn. left. reflexivity.
      + right. rewrite <- (exec_ss_other tag st k' q s) by (right; exact Hs').
        apply IH. exists k. destruct Hin as [Heq | Hin].
        * cbn in Heq. subst k'. contradiction.
        * split; [exact Hin | split; [exact Hs |]].
          rewrite exec_sc_other; [exact Hnone |]. intros ->. contradiction.
  Qed.

  Lemma filter_false : forall (A : Type) (l : list A), filter (fun _ => false) l = [].
  Proof. induction l as [| x l IH]; [reflexivity | exact IH]. Qed.

  Lemma no_mergeable_produces : forall l, produces_mergeable chan pid cid reified_mergeable l = [].
  Proof. intros l. unfold produces_mergeable, reified_mergeable. apply filter_false. Qed.

  Lemma check1_counter_race :
    forall a b c v cr,
      In (counter_produce c v cr) (consumed a) -> In (counter_produce c v cr) (consumed b) ->
      In c (check1_spaces a b).
  Proof.
    intros a b c v cr Ha Hb. unfold check1_spaces, check1. apply in_or_app. right.
    apply in_map_iff. exists (counter_produce c v cr). split; [reflexivity |].
    apply filter_In. split; [exact Ha |].
    rewrite (proj2 (inp_iff _ _ _ _ _ _) Hb), !no_mergeable_produces. reflexivity.
  Qed.

  (* Two deploys from one pre-state that both use the collection k race on
     the counter of k, or on the space counter when k has no counter yet. *)
  Lemma shared_collection_race :
    forall tA tB base opsA opsB k,
      In k (map fst opsA) -> In k (map fst opsB) ->
      match sc base k with
      | Some _ => In (ChS k) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB)))
      | None => In (ChSP (space_of k)) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB)))
      end.
  Proof.
    intros tA tB base opsA opsB k HA HB.
    destruct (sc base k) as [[v cr] |] eqn:E.
    - apply (check1_counter_race _ _ (ChS k) v cr); apply counter_consumed; assumption.
    - apply (check1_counter_race _ _ (ChSP (space_of k)) (fst (ss base (space_of k))) (snd (ss base (space_of k))));
        apply space_counter_consumed; exists k; split; [exact HA | split; [reflexivity | exact E] | exact HB |
          split; [reflexivity | exact E]].
  Qed.

  Definition is_write (q : qop) : bool := match q with QEnqueue _ | QDequeue => true | QPeek => false end.

  (* Headline 2. Two branches from one pre-state that write one queue (an
     enqueue or a dequeue each) race on the counter S(sid,k) of the queue:
     both consume the pre-state counter produce without a peek. *)
  Theorem ordered_writes_race_on_counter :
    forall tA tB base opsA opsB k v cr qa qb,
      sc base k = Some (v, cr) ->
      In (k, qa) opsA -> In (k, qb) opsB -> is_write qa = true -> is_write qb = true ->
      In (ChS k) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB))).
  Proof.
    intros tA tB base opsA opsB k v cr qa qb E HA HB _ _.
    pose proof (shared_collection_race tA tB base opsA opsB k
                  (in_map fst _ (k, qa) HA) (in_map fst _ (k, qb) HB)) as H.
    rewrite E in H. exact H.
  Qed.

  (* Headline 3. A peek also bumps the counter (A2, decision 31), so a
     branch that only reads a queue races with every branch that uses it. *)
  Theorem ordered_reads_race_on_counter :
    forall tA tB base opsA opsB k v cr qb,
      sc base k = Some (v, cr) ->
      In (k, QPeek) opsA -> In (k, qb) opsB ->
      In (ChS k) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB))).
  Proof.
    intros tA tB base opsA opsB k v cr qb E HA HB.
    pose proof (shared_collection_race tA tB base opsA opsB k
                  (in_map fst _ (k, QPeek) HA) (in_map fst _ (k, qb) HB)) as H.
    rewrite E in H. exact H.
  Qed.

  (* Headline 4. Two branches whose first operations open collections of
     one space, one collection or two, race on the space counter S(sid)
     (A25). *)
  Theorem first_ops_race_on_space_counter :
    forall tA tB base opsA opsB k1 k2,
      sc base k1 = None -> sc base k2 = None -> space_of k1 = space_of k2 ->
      In k1 (map fst opsA) -> In k2 (map fst opsB) ->
      In (ChSP (space_of k1)) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB))).
  Proof.
    intros tA tB base opsA opsB k1 k2 E1 E2 Hs HA HB.
    apply (check1_counter_race _ _ (ChSP (space_of k1)) (fst (ss base (space_of k1))) (snd (ss base (space_of k1)))).
    - apply space_counter_consumed. exists k1. split; [exact HA | split; [reflexivity | exact E1]].
    - apply space_counter_consumed. exists k2. split; [exact HB | split; [symmetry; exact Hs | exact E2]].
  Qed.

  (* Headline 5. Crossed queues: branch A uses q1 and q2 and branch B uses
     q2 and q1, in any order. The merge sees a race on both counters. *)
  Theorem crossed_collections_race :
    forall tA tB base opsA opsB k1 k2 v1 cr1 v2 cr2,
      sc base k1 = Some (v1, cr1) -> sc base k2 = Some (v2, cr2) ->
      In k1 (map fst opsA) -> In k2 (map fst opsA) ->
      In k1 (map fst opsB) -> In k2 (map fst opsB) ->
      In (ChS k1) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB))) /\
      In (ChS k2) (check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB))).
  Proof.
    intros tA tB base opsA opsB k1 k2 v1 cr1 v2 cr2 E1 E2 HA1 HA2 HB1 HB2.
    pose proof (shared_collection_race tA tB base opsA opsB k1 HA1 HB1) as H1.
    pose proof (shared_collection_race tA tB base opsA opsB k2 HA2 HB2) as H2.
    rewrite E1 in H1. rewrite E2 in H2. split; assumption.
  Qed.
  (* ── 5. Headline 6: one block, one chain ─────────────────────────── *)

  (* The produces of the pre-state of a block were made by Base. *)
  Definition in_pre_state (p : produce chan pid) : bool :=
    match p_id p with
    | PidCounter _ _ Base | PidData _ Base => true
    | _ => false
    end.

  Definition depends_spaces (target source : list ev) : bool :=
    depends chan pid cid chan_eq_dec pid_eq_dec cid_eq_dec reified_mergeable in_pre_state target source.

  Definition check2_spaces (a b : list ev) : list chan :=
    check2 chan pid cid chan_eq_dec pid_eq_dec cid_eq_dec in_pre_state a b.

  Definition counter_pid (i : pid) : bool := match i with PidCounter _ _ _ => true | PidData _ _ => false end.

  (* The plan events stage stores, parks and fires on D(sid,k). They never
     consume a counter produce. *)
  Definition plan_events_data : Prop :=
    forall tag k q v l e, In e (plan_events tag k q v l) ->
      match e with
      | EvComm _ ps _ => forall p, In p ps -> counter_pid (p_id p) = false
      | _ => True
      end.

  Definition counter_of (st : store) (k : Coll) : nat :=
    match sc st k with Some (v, _) => v | None => 0 end.

  Definition linear (log : list ev) : list (produce chan pid) := produces_linear chan pid cid log.

  Lemma linear_app : forall l1 l2, linear (l1 ++ l2) = linear l1 ++ linear l2.
  Proof. intros l1 l2. unfold linear, produces_linear. apply flat_map_app. Qed.

  Lemma run_sc_other :
    forall tag ops st k, ~ In k (map fst ops) -> sc (r_store (run tag st ops)) k = sc st k.
  Proof.
    induction ops as [| [k' q] rest IH]; intros st k Hnot; [reflexivity |].
    cbn [run r_store]. rewrite IH.
    - apply exec_sc_other. intros ->. apply Hnot. left. reflexivity.
    - intros Hin. apply Hnot. right. exact Hin.
  Qed.

  Lemma exec_sc_self :
    forall tag st k q, sc (x_store (exec tag st k q)) k = Some (S (counter_of st k), Made tag).
  Proof.
    intros tag st k q. unfold exec, counter_of.
    destruct (sc st k) as [[v cr] |]; cbn [x_store sc]; apply upd_c_eq.
  Qed.

  Lemma exec_makes_counter :
    forall tag st k q,
      In (counter_produce (ChS k) (S (counter_of st k)) (Made tag)) (linear (x_log (exec tag st k q))).
  Proof.
    intros tag st k q. unfold exec, counter_of.
    destruct (sc st k) as [[v cr] |]; cbn [x_log]; rewrite linear_app; apply in_or_app; left; cbn.
    - left. reflexivity.
    - right. left. reflexivity.
  Qed.

  (* After a deploy that used k, the counter of k is a record that the
     deploy made, and the log of the deploy produced it. *)
  Lemma run_last_counter :
    forall tag ops st k,
      In k (map fst ops) ->
      exists v, sc (r_store (run tag st ops)) k = Some (v, Made tag) /\
                In (counter_produce (ChS k) v (Made tag)) (linear (r_log (run tag st ops))).
  Proof.
    induction ops as [| [k' q] rest IH]; intros st k Hin; [destruct Hin |].
    cbn [run r_store r_log]. rewrite linear_app.
    destruct (in_dec coll_eq_dec k (map fst rest)) as [Hrest | Hrest].
    - destruct (IH (x_store (exec tag st k' q)) k Hrest) as [v [Hsc Hlin]].
      exists v. split; [exact Hsc | apply in_or_app; right; exact Hlin].
    - destruct Hin as [Heq | Hin]; [| contradiction]. cbn in Heq. subst k'.
      exists (S (counter_of st k)). split.
      + rewrite run_sc_other by exact Hrest. apply exec_sc_self.
      + apply in_or_app. left. apply exec_makes_counter.
  Qed.

  Lemma counter_of_exec :
    forall tag st k q k', counter_of st k' <= counter_of (x_store (exec tag st k q)) k'.
  Proof.
    intros tag st k q k'. destruct (coll_eq_dec k k') as [<- | Hne].
    - unfold counter_of at 2. rewrite exec_sc_self. unfold counter_of. destruct (sc st k) as [[v cr] |]; lia.
    - unfold counter_of. rewrite exec_sc_other by exact Hne. lia.
  Qed.

  Lemma counter_of_run :
    forall tag ops st k, counter_of st k <= counter_of (r_store (run tag st ops)) k.
  Proof.
    induction ops as [| [k2 q2] rest IH]; intros st k; [cbn; lia |].
    cbn [run r_store]. eapply Nat.le_trans; [apply (counter_of_exec tag st k2 q2 k) | apply IH].
  Qed.

  (* The plan events of an operation consume no counter produce. *)
  Lemma plan_events_consume_no_counter :
    forall tag k q v l c v' cr,
      plan_events_data ->
      ~ In (counter_produce c v' cr) (consumed (plan_events tag k q v l)).
  Proof.
    intros tag k q v l c v' cr Hdata Hin. unfold consumed, produces_consumed in Hin.
    apply in_flat_map in Hin as [e [He Hp]].
    specialize (Hdata tag k q v l e He).
    destruct e as [p | c0 | c0 ps peeks]; cbn in Hp; try destruct Hp.
    destruct peeks; cbn in Hp; [destruct Hp |].
    specialize (Hdata _ Hp). cbn in Hdata. discriminate.
  Qed.

  (* The counter produce of a collection that one operation consumes is the
     counter record that the operation found. *)
  Lemma exec_consumed_counter :
    forall tag st k' q k v cr,
      plan_events_data ->
      In (counter_produce (ChS k) v cr) (consumed (x_log (exec tag st k' q))) ->
      k' = k /\ sc st k = Some (v, cr).
  Proof.
    intros tag st k' q k v cr Hdata Hin. unfold exec in Hin.
    destruct (sc st k') as [[v' cr'] |] eqn:E; cbn [x_log] in Hin; rewrite consumed_app in Hin;
      apply in_app_or in Hin as [Hin | Hin].
    - cbn in Hin. destruct Hin as [Hin | []]. inversion Hin; subst. split; [reflexivity | exact E].
    - exfalso. exact (plan_events_consume_no_counter _ _ _ _ _ _ _ _ Hdata Hin).
    - cbn in Hin. destruct Hin as [Hin | []]. inversion Hin.
    - exfalso. exact (plan_events_consume_no_counter _ _ _ _ _ _ _ _ Hdata Hin).
  Qed.

  (* Within one deploy, every counter produce of k that the deploy consumes
     has a value below the final counter value of k, so the deploy never
     consumes its last counter record. *)
  Lemma consumed_counter_below :
    forall tag ops st k v cr,
      plan_events_data ->
      In (counter_produce (ChS k) v cr) (consumed (r_log (run tag st ops))) ->
      v < counter_of (r_store (run tag st ops)) k.
  Proof.
    induction ops as [| [k' q] rest IH]; intros st k v cr Hdata Hin; [destruct Hin |].
    cbn [run r_log r_store] in *. rewrite consumed_app in Hin. apply in_app_or in Hin as [Hin | Hin].
    - destruct (exec_consumed_counter tag st k' q k v cr Hdata Hin) as [<- E].
      eapply Nat.lt_le_trans; [| apply counter_of_run].
      unfold counter_of. rewrite exec_sc_self. unfold counter_of. rewrite E. lia.
    - exact (IH _ k v cr Hdata Hin).
  Qed.

  (* Headline 6. Two deploys of one block that use one collection form one
     chain: the second deploy consumes, without a peek, the counter record
     that the first deploy made last, which the first deploy did not
     consume and which is not in the pre-state. HEAD's depends then links
     them (D.3), so the merge accepts or rejects them together. *)
  Theorem same_block_ops_chain :
    forall t1 t2 base ops1 ops2 k,
      plan_events_data -> In k (map fst ops1) -> In k (map fst ops2) ->
      let r1 := run t1 base ops1 in
      let r2 := run t2 (r_store r1) ops2 in
      depends_spaces (r_log r2) (r_log r1) = true.
  Proof.
    intros t1 t2 base ops1 ops2 k Hdata H1 H2. cbv zeta.
    destruct (run_last_counter t1 ops1 base k H1) as [v [Hsc Hlin]].
    apply (depends_of_produce chan pid cid chan_eq_dec pid_eq_dec cid_eq_dec reified_mergeable
             in_pre_state _ _ (counter_produce (ChS k) v (Made t1))).
    - exact Hlin.
    - intros Hcons. pose proof (consumed_counter_below t1 ops1 base k v (Made t1) Hdata Hcons) as Hlt.
      unfold counter_of in Hlt. rewrite Hsc in Hlt. lia.
    - intros Hcopy. unfold produces_copied_by_peek in Hcopy. apply in_flat_map in Hcopy as [e [_ He]].
      destruct e as [p | c | c ps peeks]; cbn in He; try destruct He.
      destruct (in_pre_state p) eqn:Ep; cbn in He; [| destruct He].
      destruct He as [He | []]. subst p. cbn in Ep. discriminate.
    - reflexivity.
    - apply counter_consumed; [exact Hsc | exact H2].
  Qed.

  (* ── 6. Headline 8: a merge without a race is a serial run ─────────── *)

  (* The records on a channel: D(sid,k) holds the records of the
     collection, S(sid,k) at most one counter record, and S(sid) one space
     counter record. *)
  Inductive item : Type := DItem (d : D) | CItem (v : nat) (cr : creator).

  Definition item_eq_dec : forall x y : item, {x = y} + {x <> y}.
  Proof. decide equality; try apply Nat.eq_dec; try apply creator_eq_dec; apply d_eq_dec. Defined.

  Definition view (st : store) (c : chan) : list item :=
    match c with
    | ChD k => map DItem (sd st k)
    | ChS k => match sc st k with Some (v, cr) => [CItem v cr] | None => [] end
    | ChSP s => [CItem (fst (ss st s)) (snd (ss st s))]
    end.

  Definition ldiff (l r : list item) : list item := list_diff item item_eq_dec l r.

  Lemma cnt_ldiff :
    forall l r y, count_occ item_eq_dec (ldiff l r) y = count_occ item_eq_dec l y - count_occ item_eq_dec r y.
  Proof. intros l r y. apply cnt_list_diff. Qed.

  (* ChannelChange of channel_change.rs. *)
  Record change : Type := mkChange { added : list item; removed : list item }.

  (* compute_value_change of state_change.rs: added = end - start and
     removed = start - end, as multisets. *)
  Definition diff_change (pre post : list item) : change := mkChange (ldiff post pre) (ldiff pre post).

  Definition is_noop (ch : change) : bool :=
    match added ch, removed ch with [], [] => true | _, _ => false end.

  (* StateChange::new keeps the change of a channel only when it is not a
     no-op. *)
  Definition entry_of (pre post : list item) : option change :=
    let ch := diff_change pre post in if is_noop ch then None else Some ch.

  (* ChannelChange::combine: join (vec_union on each side), then
     normalized (cancel_common). *)
  Definition combine (x y : change) : change :=
    let r := cancel_common item item_eq_dec (vec_union item item_eq_dec (added x) (added y))
                                            (vec_union item item_eq_dec (removed x) (removed y)) in
    mkChange (fst r) (snd r).

  (* StateChange::combine (normalize mode) folded over the empty change and
     the two branches (conflict_set_merger.rs compute_merged_state): a
     channel that one branch changes keeps the change of that branch. *)
  Definition combine_entry (a b : option change) : option change :=
    match a, b with
    | Some x, Some y => Some (combine x y)
    | Some x, None => Some x
    | None, y => y
    end.

  (* make_trie_action of state_change_merger.rs: the new value is the base
     minus removed, plus added. A removal that the base does not hold, or
     a change that gives the base again, is a merge error (None). *)
  Definition apply_change (init : list item) (ch : change) : option (list item) :=
    if sub_multiset item item_eq_dec (removed ch) init then
      let new_val := ldiff init (removed ch) ++ added ch in
      if list_eq_dec item_eq_dec init new_val then None else Some new_val
    else None.

  (* compute_trie_actions of state_change_merger.rs. No change, or a change
     that nets to nothing, leaves the base value. Any other change goes
     through make_trie_action. *)
  Definition outcome (init : list item) (o : option change) : option (list item) :=
    match o with
    | None => Some init
    | Some ch => if is_noop ch then Some init else apply_change init ch
    end.

  (* The merged records of a channel. *)
  Definition merged (base endA endB : store) (c : chan) : option (list item) :=
    outcome (view base c)
      (combine_entry (entry_of (view base c) (view endA c)) (entry_of (view base c) (view endB c))).

  Lemma is_noop_true : forall ch, is_noop ch = true <-> added ch = [] /\ removed ch = [].
  Proof.
    intros [[| a l1] [| b l2]]; cbn; split; intros H;
      solve [split; reflexivity | reflexivity | discriminate H | destruct H as [H1 H2]; discriminate].
  Qed.

  Lemma noop_iff_perm : forall pre post, is_noop (diff_change pre post) = true <-> Permutation pre post.
  Proof.
    intros pre post. rewrite is_noop_true, (perm_iff_cnt item item_eq_dec). cbn [added removed diff_change].
    split.
    - intros [E1 E2] y.
      assert (H1 : count_occ item_eq_dec (ldiff post pre) y = 0) by (rewrite E1; reflexivity).
      assert (H2 : count_occ item_eq_dec (ldiff pre post) y = 0) by (rewrite E2; reflexivity).
      rewrite cnt_ldiff in H1, H2. lia.
    - intros H. split; apply (cnt_zero_nil item item_eq_dec); intros y; rewrite cnt_ldiff; specialize (H y); lia.
  Qed.

  Lemma entry_same : forall l, entry_of l l = None.
  Proof.
    intros l. unfold entry_of. rewrite (proj2 (noop_iff_perm l l) (Permutation_refl l)). reflexivity.
  Qed.

  Lemma combine_entry_none_r : forall a, combine_entry a None = a.
  Proof. intros [x |]; reflexivity. Qed.

  (* A change of one branch applies to the base without an error, and the
     result holds the records of the end of that branch. *)
  Lemma entry_outcome :
    forall pre post, exists l, outcome pre (entry_of pre post) = Some l /\ Permutation l post.
  Proof.
    intros pre post. unfold entry_of.
    destruct (is_noop (diff_change pre post)) eqn:Hn.
    - exists pre. split; [reflexivity |]. apply noop_iff_perm. exact Hn.
    - cbn [outcome]. rewrite Hn. unfold apply_change. cbn [removed added diff_change].
      assert (Hperm : Permutation (ldiff pre (ldiff pre post) ++ ldiff post pre) post).
      { apply (perm_iff_cnt item item_eq_dec). intros y. rewrite count_occ_app, !cnt_ldiff. lia. }
      assert (Hsub : sub_multiset item item_eq_dec (ldiff pre post) pre = true).
      { apply sub_multiset_iff. intros y. rewrite cnt_ldiff. lia. }
      rewrite Hsub.
      destruct (list_eq_dec item_eq_dec pre (ldiff pre (ldiff pre post) ++ ldiff post pre)) as [Heq | Hne].
      + exfalso. rewrite <- Heq in Hperm. apply noop_iff_perm in Hperm. congruence.
      + exists (ldiff pre (ldiff pre post) ++ ldiff post pre). split; [reflexivity | exact Hperm].
  Qed.

  (* The collections that a deploy uses and the spaces in which it opens a
     collection. *)
  Definition inb (k : Coll) (ops : list (Coll * qop)) : bool :=
    if in_dec coll_eq_dec k (map fst ops) then true else false.

  Lemma inb_iff : forall k ops, inb k ops = true <-> In k (map fst ops).
  Proof.
    intros k ops. unfold inb.
    destruct (in_dec coll_eq_dec k (map fst ops)) as [H | H]; split; intros H';
      [exact H | reflexivity | discriminate H' | contradiction].
  Qed.

  Definition opens_at (st : store) (s : Space) (k : Coll) : bool :=
    if space_eq_dec (space_of k) s then match sc st k with None => true | Some _ => false end else false.

  Definition opensb (st : store) (ops : list (Coll * qop)) (s : Space) : bool :=
    existsb (opens_at st s) (map fst ops).

  Lemma opens_at_iff : forall st s k, opens_at st s k = true <-> space_of k = s /\ sc st k = None.
  Proof.
    intros st s k. unfold opens_at.
    destruct (space_eq_dec (space_of k) s) as [Hs | Hs]; destruct (sc st k) as [p |].
    - split; [intros H; discriminate H | intros [_ H]; discriminate H].
    - split; [intros _; split; [exact Hs | reflexivity] | intros _; reflexivity].
    - split; [intros H; discriminate H | intros [_ H]; discriminate H].
    - split; [intros H; discriminate H | intros [H _]; contradiction].
  Qed.

  Lemma opensb_iff :
    forall st ops s, opensb st ops s = true <-> exists k, In k (map fst ops) /\ space_of k = s /\ sc st k = None.
  Proof.
    intros st ops s. unfold opensb. rewrite existsb_exists. split.
    - intros [k [Hin H]]. apply opens_at_iff in H. exists k. split; [exact Hin | exact H].
    - intros [k [Hin H]]. exists k. split; [exact Hin | apply opens_at_iff; exact H].
  Qed.

  Lemma opensb_ext :
    forall st1 st2 ops s,
      (forall k, In k (map fst ops) -> sc st1 k = sc st2 k) -> opensb st1 ops s = opensb st2 ops s.
  Proof.
    intros st1 st2 ops s. unfold opensb. generalize (map fst ops) as l.
    induction l as [| k rest IH]; intros H; [reflexivity |]. cbn [existsb]. f_equal.
    - unfold opens_at. rewrite (H k (or_introl eq_refl)). reflexivity.
    - apply IH. intros k' Hin. apply H. right. exact Hin.
  Qed.

  (* The channels that a deploy can change. *)
  Definition touched (st : store) (ops : list (Coll * qop)) (c : chan) : bool :=
    match c with
    | ChD k | ChS k => inb k ops
    | ChSP s => opensb st ops s
    end.

  Lemma touched_ext :
    forall st1 st2 ops c,
      (forall k, In k (map fst ops) -> sc st1 k = sc st2 k) -> touched st1 ops c = touched st2 ops c.
  Proof. intros st1 st2 ops [k | k | s] H; [reflexivity | reflexivity | apply opensb_ext; exact H]. Qed.

  Lemma exec_sd_other :
    forall tag st k q k', k <> k' -> sd (x_store (exec tag st k q)) k' = sd st k'.
  Proof.
    intros tag st k q k' H. unfold exec.
    destruct (sc st k) as [[v cr] |]; cbn [x_store sd]; apply upd_c_neq; exact H.
  Qed.

  Lemma run_sd_other :
    forall tag ops st k, ~ In k (map fst ops) -> sd (r_store (run tag st ops)) k = sd st k.
  Proof.
    induction ops as [| [k' q] rest IH]; intros st k Hnot; [reflexivity |].
    cbn [run r_store]. rewrite IH.
    - apply exec_sd_other. intros ->. apply Hnot. left. reflexivity.
    - intros Hin. apply Hnot. right. exact Hin.
  Qed.

  Lemma run_ss_other :
    forall tag ops st s, opensb st ops s = false -> ss (r_store (run tag st ops)) s = ss st s.
  Proof.
    induction ops as [| [k q] rest IH]; intros st s Hno; [reflexivity |].
    cbn [run r_store].
    assert (Hhead : opens_at st s k = false /\ opensb st rest s = false).
    { unfold opensb in Hno |- *. cbn [map existsb] in Hno. apply orb_false_iff in Hno. exact Hno. }
    destruct Hhead as [Hk Hrest].
    rewrite IH.
    - apply exec_ss_other. revert Hk. unfold opens_at.
      destruct (space_eq_dec (space_of k) s) as [Hs | Hs]; [| intros _; right; exact Hs].
      destruct (sc st k) as [p |]; intros Hk; [left; discriminate | discriminate Hk].
    - destruct (opensb (x_store (exec tag st k q)) rest s) eqn:Ho; [| reflexivity]. exfalso.
      apply opensb_iff in Ho as [k' [Hin [Hs Hn]]].
      assert (Hne : k <> k') by (intros <-; rewrite exec_sc_self in Hn; discriminate Hn).
      rewrite exec_sc_other in Hn by exact Hne.
      assert (Hyes : opensb st rest s = true)
        by (apply opensb_iff; exists k'; split; [exact Hin | split; [exact Hs | exact Hn]]).
      congruence.
  Qed.

  (* A deploy leaves every channel that it does not touch as it was. *)
  Lemma run_view_other :
    forall tag ops st c, touched st ops c = false -> view (r_store (run tag st ops)) c = view st c.
  Proof.
    intros tag ops st [k | k | s] H; cbn [touched] in H; cbn [view].
    - rewrite run_sd_other; [reflexivity |]. intros Hin. apply inb_iff in Hin. congruence.
    - rewrite run_sc_other; [reflexivity |]. intros Hin. apply inb_iff in Hin. congruence.
    - rewrite run_ss_other by exact H. reflexivity.
  Qed.

  Lemma exec_local :
    forall tag st1 st2 k q,
      sd st1 k = sd st2 k -> sc st1 k = sc st2 k ->
      (sc st1 k = None -> ss st1 (space_of k) = ss st2 (space_of k)) ->
      x_obs (exec tag st1 k q) = x_obs (exec tag st2 k q) /\
      x_log (exec tag st1 k q) = x_log (exec tag st2 k q) /\
      sd (x_store (exec tag st1 k q)) k = sd (x_store (exec tag st2 k q)) k /\
      (sc st1 k = None ->
       ss (x_store (exec tag st1 k q)) (space_of k) = ss (x_store (exec tag st2 k q)) (space_of k)).
  Proof.
    intros tag st1 st2 k q Hd Hc Hs. unfold exec. rewrite <- Hd, <- Hc.
    destruct (sc st1 k) as [[v cr] |] eqn:E.
    - cbn [x_obs x_log x_store sd ss]. rewrite !upd_c_eq.
      split; [reflexivity | split; [reflexivity | split; [reflexivity | intros H; discriminate H]]].
    - specialize (Hs eq_refl). cbn [x_obs x_log x_store sd ss]. rewrite <- Hs, !upd_c_eq, !upd_s_eq.
      split; [reflexivity | split; [reflexivity | split; [reflexivity | intros _; reflexivity]]].
  Qed.

  (* A deploy behaves alike from two stores that agree on the collections
     that it uses and on the space counters of the spaces where it opens a
     collection: the same observations, the same log, and the same records
     on every channel that it touches. *)
  Lemma run_local :
    forall tag ops st1 st2,
      (forall k, In k (map fst ops) -> sd st1 k = sd st2 k /\ sc st1 k = sc st2 k) ->
      (forall s, opensb st1 ops s = true -> ss st1 s = ss st2 s) ->
      r_obs (run tag st1 ops) = r_obs (run tag st2 ops) /\
      r_log (run tag st1 ops) = r_log (run tag st2 ops) /\
      (forall c, touched st1 ops c = true ->
                 view (r_store (run tag st1 ops)) c = view (r_store (run tag st2 ops)) c).
  Proof.
    induction ops as [| [k q] rest IH]; intros st1 st2 Hagree Hss.
    - split; [reflexivity | split; [reflexivity |]]. intros [k | k | s] H; cbn in H; discriminate H.
    - assert (Hk : In k (map fst ((k, q) :: rest))) by (left; reflexivity).
      destruct (Hagree k Hk) as [Hd Hc].
      assert (Hs0 : sc st1 k = None -> ss st1 (space_of k) = ss st2 (space_of k)).
      { intros E. apply Hss. apply opensb_iff. exists k. split; [exact Hk | split; [reflexivity | exact E]]. }
      destruct (exec_local tag st1 st2 k q Hd Hc Hs0) as [Hobs [Hlog [Hsd Hs1]]].
      assert (Hsc1 : sc (x_store (exec tag st1 k q)) k = sc (x_store (exec tag st2 k q)) k).
      { rewrite !exec_sc_self. unfold counter_of. rewrite Hc. reflexivity. }
      assert (Hagree' : forall k', In k' (map fst rest) ->
                sd (x_store (exec tag st1 k q)) k' = sd (x_store (exec tag st2 k q)) k' /\
                sc (x_store (exec tag st1 k q)) k' = sc (x_store (exec tag st2 k q)) k').
      { intros k' Hin. destruct (coll_eq_dec k k') as [<- | Hne].
        - split; [exact Hsd | exact Hsc1].
        - rewrite !exec_sd_other, !exec_sc_other by exact Hne. apply Hagree. right. exact Hin. }
      assert (Hss' : forall s, opensb (x_store (exec tag st1 k q)) rest s = true ->
                ss (x_store (exec tag st1 k q)) s = ss (x_store (exec tag st2 k q)) s).
      { intros s Ho. apply opensb_iff in Ho as [k' [Hin [Hs En]]].
        assert (Hne : k <> k') by (intros <-; rewrite exec_sc_self in En; discriminate En).
        rewrite exec_sc_other in En by exact Hne.
        assert (Hopen : opensb st1 ((k, q) :: rest) s = true)
          by (apply opensb_iff; exists k'; split; [right; exact Hin | split; [exact Hs | exact En]]).
        destruct (sc st1 k) as [p |] eqn:E1.
        - rewrite !exec_ss_other by (left; congruence). apply Hss. exact Hopen.
        - destruct (space_eq_dec (space_of k) s) as [<- | Hsk].
          + apply Hs1. first [exact E1 | reflexivity].
          + rewrite !exec_ss_other by (right; exact Hsk). apply Hss. exact Hopen. }
      destruct (IH (x_store (exec tag st1 k q)) (x_store (exec tag st2 k q)) Hagree' Hss')
        as [Iobs [Ilog Iview]].
      cbn [run r_obs r_log r_store].
      split; [rewrite Hobs, Iobs; reflexivity | split; [rewrite Hlog, Ilog; reflexivity |]].
      intros c Ht.
      destruct (touched (x_store (exec tag st1 k q)) rest c) eqn:Ht'; [apply Iview; exact Ht' |].
      assert (Ht2 : touched (x_store (exec tag st2 k q)) rest c = false).
      { rewrite <- (touched_ext _ _ rest c (fun k' Hin => proj2 (Hagree' k' Hin))). exact Ht'. }
      rewrite (run_view_other tag rest _ c Ht'), (run_view_other tag rest _ c Ht2).
      destruct c as [k' | k' | s]; cbn [touched] in Ht, Ht'.
      + apply inb_iff in Ht. destruct Ht as [Heq | Hin].
        * cbn in Heq. subst k'. cbn [view]. rewrite Hsd. reflexivity.
        * apply inb_iff in Hin. congruence.
      + apply inb_iff in Ht. destruct Ht as [Heq | Hin].
        * cbn in Heq. subst k'. cbn [view]. rewrite Hsc1. reflexivity.
        * apply inb_iff in Hin. congruence.
      + apply opensb_iff in Ht as [k'' [Hin [Hs En]]]. subst s.
        assert (Hkk : k = k'').
        { destruct Hin as [Heq | Hin]; [exact Heq |].
          destruct (coll_eq_dec k k'') as [Heq | Hne]; [exact Heq |]. exfalso.
          assert (Hyes : opensb (x_store (exec tag st1 k q)) rest (space_of k'') = true).
          { apply opensb_iff. exists k''. split; [exact Hin | split; [reflexivity |]].
            rewrite exec_sc_other by exact Hne. exact En. }
          congruence. }
        subst k''. cbn [view]. rewrite (Hs1 En). reflexivity.
  Qed.

  (* Without a race, the two branches use disjoint collections and open
     collections in disjoint spaces. *)
  Lemma no_race_disjoint_colls :
    forall tA tB base opsA opsB k,
      check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB)) = [] ->
      In k (map fst opsA) -> ~ In k (map fst opsB).
  Proof.
    intros tA tB base opsA opsB k Hnr HA HB.
    pose proof (shared_collection_race tA tB base opsA opsB k HA HB) as H.
    rewrite Hnr in H. destruct (sc base k); destruct H.
  Qed.

  Lemma no_race_disjoint_opens :
    forall tA tB base opsA opsB s,
      check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB)) = [] ->
      opensb base opsA s = true -> opensb base opsB s = false.
  Proof.
    intros tA tB base opsA opsB s Hnr HA.
    destruct (opensb base opsB s) eqn:HB; [| reflexivity]. exfalso.
    apply opensb_iff in HA as [k1 [H1 [Hs1 E1]]]. apply opensb_iff in HB as [k2 [H2 [Hs2 E2]]].
    pose proof (first_ops_race_on_space_counter tA tB base opsA opsB k1 k2 E1 E2
                  (eq_trans Hs1 (eq_sym Hs2)) H1 H2) as H.
    rewrite Hnr in H. destruct H.
  Qed.

  Lemma no_race_touched :
    forall tA tB base opsA opsB c,
      check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB)) = [] ->
      touched base opsA c = true -> touched base opsB c = false.
  Proof.
    intros tA tB base opsA opsB [k | k | s] Hnr H; cbn [touched] in H |- *.
    1, 2: destruct (inb k opsB) eqn:E; [| reflexivity]; exfalso;
          apply inb_iff in H; apply inb_iff in E; exact (no_race_disjoint_colls tA tB base opsA opsB k Hnr H E).
    exact (no_race_disjoint_opens tA tB base opsA opsB s Hnr H).
  Qed.

  (* Headline 8. When check #1 finds no race between two branches of queue
     operations, the merge applies without an error, and its records are,
     on every channel, the records of the serial run A then B and of the
     serial run B then A (as multisets: list order is not state, because
     the head is the least ord). Each deploy also observes in the serial
     run what it observed in its branch. *)
  Theorem merge_without_race_is_serializable :
    forall tA tB base opsA opsB,
      check1_spaces (r_log (run tA base opsA)) (r_log (run tB base opsB)) = [] ->
      let endA := r_store (run tA base opsA) in
      let endB := r_store (run tB base opsB) in
      r_obs (run tB endA opsB) = r_obs (run tB base opsB) /\
      r_obs (run tA endB opsA) = r_obs (run tA base opsA) /\
      (forall c, exists l, merged base endA endB c = Some l /\
                 Permutation l (view (r_store (run tB endA opsB)) c) /\
                 Permutation l (view (r_store (run tA endB opsA)) c)).
  Proof.
    intros tA tB base opsA opsB Hnr. cbv zeta.
    assert (HscB : forall k, In k (map fst opsB) ->
              sd (r_store (run tA base opsA)) k = sd base k /\ sc (r_store (run tA base opsA)) k = sc base k).
    { intros k HB. assert (HA : ~ In k (map fst opsA))
        by (intros HA; exact (no_race_disjoint_colls tA tB base opsA opsB k Hnr HA HB)).
      split; [apply run_sd_other | apply run_sc_other]; exact HA. }
    assert (HscA : forall k, In k (map fst opsA) ->
              sd (r_store (run tB base opsB)) k = sd base k /\ sc (r_store (run tB base opsB)) k = sc base k).
    { intros k HA. pose proof (no_race_disjoint_colls tA tB base opsA opsB k Hnr HA) as HB.
      split; [apply run_sd_other | apply run_sc_other]; exact HB. }
    assert (HssB : forall s, opensb base opsB s = true -> ss (r_store (run tA base opsA)) s = ss base s).
    { intros s HB. apply run_ss_other. destruct (opensb base opsA s) eqn:EA; [| reflexivity].
      rewrite (no_race_disjoint_opens tA tB base opsA opsB s Hnr EA) in HB. discriminate HB. }
    assert (HssA : forall s, opensb base opsA s = true -> ss (r_store (run tB base opsB)) s = ss base s).
    { intros s HA. apply run_ss_other. exact (no_race_disjoint_opens tA tB base opsA opsB s Hnr HA). }
    destruct (run_local tB opsB base (r_store (run tA base opsA))) as [ObsB [_ ViewB]].
    { intros k HB. destruct (HscB k HB) as [H1 H2]. split; symmetry; assumption. }
    { intros s HB. symmetry. exact (HssB s HB). }
    destruct (run_local tA opsA base (r_store (run tB base opsB))) as [ObsA [_ ViewA]].
    { intros k HA. destruct (HscA k HA) as [H1 H2]. split; symmetry; assumption. }
    { intros s HA. symmetry. exact (HssA s HA). }
    split; [symmetry; exact ObsB | split; [symmetry; exact ObsA |]].
    intros c. unfold merged.
    destruct (touched base opsA c) eqn:TA.
    - assert (TB : touched base opsB c = false) by exact (no_race_touched tA tB base opsA opsB c Hnr TA).
      assert (TB' : touched (r_store (run tA base opsA)) opsB c = false).
      { rewrite (touched_ext _ base opsB c (fun k HB => proj2 (HscB k HB))). exact TB. }
      rewrite (run_view_other tB opsB base c TB), entry_same, combine_entry_none_r.
      destruct (entry_outcome (view base c) (view (r_store (run tA base opsA)) c)) as [l [Hl Hp]].
      exists l. split; [exact Hl | split].
      + rewrite (run_view_other tB opsB _ c TB'). exact Hp.
      + rewrite <- (ViewA c TA). exact Hp.
    - assert (TA' : touched (r_store (run tB base opsB)) opsA c = false).
      { rewrite (touched_ext _ base opsA c (fun k HA => proj2 (HscA k HA))). exact TA. }
      rewrite (run_view_other tA opsA base c TA), entry_same. cbn [combine_entry].
      destruct (touched base opsB c) eqn:TB.
      + destruct (entry_outcome (view base c) (view (r_store (run tB base opsB)) c)) as [l [Hl Hp]].
        exists l. split; [exact Hl | split].
        * rewrite <- (ViewB c TB). exact Hp.
        * rewrite (run_view_other tA opsA _ c TA'). exact Hp.
      + assert (TB' : touched (r_store (run tA base opsA)) opsB c = false).
        { rewrite (touched_ext _ base opsB c (fun k HB => proj2 (HscB k HB))). exact TB. }
        rewrite (run_view_other tB opsB base c TB), entry_same. exists (view base c).
        split; [reflexivity | split].
        * rewrite (run_view_other tB opsB _ c TB'), (run_view_other tA opsA base c TA). apply Permutation_refl.
        * rewrite (run_view_other tA opsA _ c TA'), (run_view_other tB opsB base c TB). apply Permutation_refl.
  Qed.

  (* ── 7. Headline 7: writes to a bag need no counter ────────────────── *)

  (* A write to an unordered collection (a bag, which is HEAD's default
     tuple space): a produce of the record, with no counter. The identity
     of the produce names the deploy and the position of the write. *)
  Fixpoint bag_run (tag n : nat) (st : store) (ws : list (Coll * D)) : store * list ev :=
    match ws with
    | [] => (st, [])
    | (k, d) :: rest =>
        let st' := mkStore (upd_c (sd st) k (sd st k ++ [d])) (sc st) (ss st) in
        let r := bag_run tag (S n) st' rest in
        (fst r, EvProduce (mkProduce (ChD k) (PidData n (Made tag)) false) :: snd r)
    end.

  Definition writes_to (k : Coll) (ws : list (Coll * D)) : list D :=
    map snd (filter (fun kd => if coll_eq_dec (fst kd) k then true else false) ws).

  Definition bag_added (ws : list (Coll * D)) (c : chan) : list item :=
    match c with ChD k => map DItem (writes_to k ws) | _ => [] end.

  Lemma in_writes_to : forall k d ws, In d (writes_to k ws) -> In (k, d) ws.
  Proof.
    intros k d ws H. unfold writes_to in H. apply in_map_iff in H as [[k' d'] [Heq Hin]].
    cbn in Heq. subst d'. apply filter_In in Hin as [Hin Hf]. cbn in Hf.
    destruct (coll_eq_dec k' k) as [-> | _]; [exact Hin | discriminate Hf].
  Qed.

  Lemma bag_view :
    forall tag ws n st c, view (fst (bag_run tag n st ws)) c = view st c ++ bag_added ws c.
  Proof.
    induction ws as [| [k' d] rest IH]; intros n st c.
    - cbn [bag_run fst]. destruct c; cbn [bag_added]; rewrite app_nil_r; reflexivity.
    - cbn [bag_run fst]. rewrite IH. destruct c as [k | k | s]; cbn [view bag_added sd sc ss].
      + unfold writes_to. cbn [filter fst]. destruct (coll_eq_dec k' k) as [<- | Hne].
        * rewrite upd_c_eq. cbn [map snd]. rewrite map_app, <- app_assoc. reflexivity.
        * rewrite upd_c_neq by exact Hne. reflexivity.
      + reflexivity.
      + reflexivity.
  Qed.

  Lemma bag_log_no_comm :
    forall tag ws n st,
      produces_consumed chan pid cid (snd (bag_run tag n st ws)) = [] /\
      consumes_produced chan pid cid (snd (bag_run tag n st ws)) = [].
  Proof.
    induction ws as [| [k d] rest IH]; intros n st; [split; reflexivity |].
    cbn [bag_run snd]. unfold produces_consumed, consumes_produced in *. cbn [flat_map app]. apply IH.
  Qed.

  Lemma entry_app_nonempty :
    forall pre a, a <> [] -> entry_of pre (pre ++ a) = Some (diff_change pre (pre ++ a)).
  Proof.
    intros pre a Ha. unfold entry_of. destruct (is_noop (diff_change pre (pre ++ a))) eqn:Hn; [| reflexivity].
    exfalso. apply noop_iff_perm in Hn. apply Permutation_length in Hn. rewrite length_app in Hn.
    destruct a; [contradiction Ha; reflexivity | cbn in Hn; lia].
  Qed.

  (* Two branches that add disjoint records to one channel merge into the
     base plus both additions. *)
  Lemma bag_merge :
    forall pre a b, (forall y, In y a -> In y b -> False) ->
      exists l, outcome pre (combine_entry (entry_of pre (pre ++ a)) (entry_of pre (pre ++ b))) = Some l /\
                Permutation l (pre ++ a ++ b).
  Proof.
    intros pre a b Hdisj.
    destruct (list_eq_dec item_eq_dec a []) as [-> | Ha].
    - rewrite app_nil_r, entry_same. cbn [combine_entry app]. exact (entry_outcome pre (pre ++ b)).
    - destruct (list_eq_dec item_eq_dec b []) as [-> | Hb].
      + rewrite !app_nil_r, entry_same, combine_entry_none_r. exact (entry_outcome pre (pre ++ a)).
      + rewrite (entry_app_nonempty pre a Ha), (entry_app_nonempty pre b Hb). cbn [combine_entry outcome].
        unfold combine. cbn [added removed diff_change].
        assert (E1 : ldiff pre (pre ++ a) = []).
        { apply (cnt_zero_nil item item_eq_dec). intros y. rewrite cnt_ldiff, count_occ_app. lia. }
        assert (E2 : ldiff pre (pre ++ b) = []).
        { apply (cnt_zero_nil item item_eq_dec). intros y. rewrite cnt_ldiff, count_occ_app. lia. }
        rewrite E1, E2.
        change (vec_union item item_eq_dec [] []) with (@nil item).
        rewrite cancel_common_nil_r. cbn [fst snd].
        assert (Hd : forall y, count_occ item_eq_dec a y = 0 \/ count_occ item_eq_dec b y = 0).
        { intros y. destruct (count_occ item_eq_dec a y) eqn:Ca; [left; reflexivity | right].
          apply (count_occ_not_In item_eq_dec). intros Hyb. apply (Hdisj y); [| exact Hyb].
          apply (count_occ_In item_eq_dec). lia. }
        remember (vec_union item item_eq_dec (ldiff (pre ++ a) pre) (ldiff (pre ++ b) pre)) as X eqn:EX.
        assert (HX : forall y, count_occ item_eq_dec X y = count_occ item_eq_dec a y + count_occ item_eq_dec b y).
        { intros y. rewrite EX. unfold vec_union.
          rewrite count_occ_app, cnt_list_diff, !cnt_ldiff, !count_occ_app. destruct (Hd y); lia. }
        destruct X as [| x0 X0].
        * exfalso. destruct a as [| y0 a0]; [contradiction Ha; reflexivity |].
          specialize (HX y0). rewrite cnt_cons in HX.
          destruct (item_eq_dec y0 y0); [cbn in HX; lia | contradiction].
        * cbn [is_noop added removed]. unfold apply_change. cbn [added removed sub_multiset forallb].
          change (ldiff pre []) with pre.
          destruct (list_eq_dec item_eq_dec pre (pre ++ x0 :: X0)) as [Heq | Hne].
          -- exfalso. apply (f_equal (@length item)) in Heq. rewrite length_app in Heq. cbn in Heq. lia.
          -- exists (pre ++ x0 :: X0). split; [reflexivity |]. apply Permutation_app_head.
             apply (perm_iff_cnt item item_eq_dec). intros y. rewrite HX, count_occ_app. reflexivity.
  Qed.

  (* Headline 7. Writes to a bag carry no counter, and they commute: two
     branches of bag writes never race in check #1, and the merge holds,
     on every channel, the records of both serial orders. Every record
     carries the random state of its deploy, so two deploys never write
     equal records (the premise). *)
  Theorem unordered_writes_commute :
    forall tA tB base wsA wsB,
      (forall k d, In (k, d) wsA -> In (k, d) wsB -> False) ->
      let endA := fst (bag_run tA 0 base wsA) in
      let endB := fst (bag_run tB 0 base wsB) in
      check1_spaces (snd (bag_run tA 0 base wsA)) (snd (bag_run tB 0 base wsB)) = [] /\
      (forall c, exists l, merged base endA endB c = Some l /\
                 Permutation l (view (fst (bag_run tB 0 endA wsB)) c) /\
                 Permutation l (view (fst (bag_run tA 0 endB wsA)) c)).
  Proof.
    intros tA tB base wsA wsB Hdisj. cbv zeta. split.
    - destruct (bag_log_no_comm tA wsA 0 base) as [HpA HcA].
      unfold check1_spaces, check1, produce_races, consume_races. rewrite HpA, HcA. reflexivity.
    - intros c. unfold merged. rewrite !bag_view.
      assert (Hd : forall y, In y (bag_added wsA c) -> In y (bag_added wsB c) -> False).
      { intros y HA HB. destruct c as [k | k | s]; cbn [bag_added] in HA, HB; [| destruct HA | destruct HA].
        apply in_map_iff in HA as [d [<- HA]]. apply in_map_iff in HB as [d' [Heq HB]].
        injection Heq as ->. exact (Hdisj k d (in_writes_to k d wsA HA) (in_writes_to k d wsB HB)). }
      destruct (bag_merge (view base c) (bag_added wsA c) (bag_added wsB c) Hd) as [l [Hl Hp]].
      exists l. split; [exact Hl | split].
      + rewrite <- app_assoc. exact Hp.
      + rewrite <- app_assoc. eapply Permutation_trans; [exact Hp |].
        apply Permutation_app_head. apply Permutation_app_comm.
  Qed.

  (* ── 8. Headline 9: a peek without the counter admits write skew ────── *)

  (* The counter-free peek that decision 31 rejects: a peek reads the
     collection with a peek COMM and leaves the counter alone. Every other
     operation bumps the counter as exec does. *)
  Definition exec_cf (tag : nat) (st : store) (k : Coll) (q : qop) : step_result :=
    match q with
    | QPeek =>
        let v := counter_of st k in
        let pl := plan q v (sd st k) in
        mkStep (mkStore (upd_c (sd st) k (fst pl)) (sc st) (ss st)) (snd pl) (plan_events tag k q v (sd st k))
    | _ => exec tag st k q
    end.

  Fixpoint run_cf (tag : nat) (st : store) (ops : list (Coll * qop)) : run_result :=
    match ops with
    | [] => mkRun st [] []
    | (k, q) :: rest =>
        let x := exec_cf tag st k q in
        let r := run_cf tag (x_store x) rest in
        mkRun (r_store r) (x_obs x :: r_obs r) (x_log x ++ r_log r)
    end.
End Spaces.

Arguments QEnqueue {D} d.
Arguments QDequeue {D}.
Arguments QPeek {D}.
Arguments ChD {Coll Space} k.
Arguments ChS {Coll Space} k.
Arguments ChSP {Coll Space} s.
Arguments PidCounter {Coll Space} _ _ _.
Arguments PidData {Coll Space} _ _.
Arguments CidCounter {Coll Space} _ _ _.
Arguments CidData {Coll Space} _ _.
Arguments mkStore {Coll Space D} sd sc ss.
Arguments r_store {Coll Space D O} _.
Arguments r_obs {Coll Space D O} _.
Arguments r_log {Coll Space D O} _.

(* The computed counterexample of D.2 (peek/dequeue write skew). Two queues
   1 and 2 of one space hold `[10; 11]` and `[20; 21]`, and both counters are
   at 2. Branch A peeks queue 1 and dequeues queue 2. Branch B peeks queue
   2 and dequeues queue 1. *)
Module WriteSkew.
  Definition space_of (k : nat) : nat := 0.

  (* A FIFO queue: an enqueue appends, a dequeue takes the head, a peek
     reads the head. The observation is the head that the operation read. *)
  Definition qplan (q : qop nat) (v : nat) (l : list nat) : list nat * option nat :=
    match q with
    | QEnqueue d => (l ++ [d], None)
    | QDequeue => match l with h :: rest => (rest, Some h) | [] => (l, None) end
    | QPeek => (l, hd_error l)
    end.

  Definition qev : Type := event (chan nat nat) (pid nat nat) (cid nat nat).

  (* The events of the plan: a store, a fire without a peek (dequeue), a
     fire with a peek (peek), or a parked receive. *)
  Definition qevents (tag k : nat) (q : qop nat) (v : nat) (l : list nat) : list qev :=
    let kc := mkConsume [ChD k] (CidData tag v) false in
    match q, l with
    | QEnqueue d, _ => [EvProduce (mkProduce (ChD k) (PidData d (Made tag)) false)]
    | QDequeue, h :: _ => [EvConsume kc; EvComm kc [mkProduce (ChD k) (PidData h Base) false] false]
    | QPeek, h :: _ => [EvConsume kc; EvComm kc [mkProduce (ChD k) (PidData h Base) false] true]
    | _, [] => [EvConsume kc]
    end.

  Definition base : store nat nat nat :=
    mkStore (fun k => match k with 1 => [10; 11] | 2 => [20; 21] | _ => [] end)
            (fun k => match k with 1 | 2 => Some (2, Base) | _ => None end)
            (fun _ => (2, Base)).

  Definition opsA : list (nat * qop nat) := [(1, QPeek); (2, QDequeue)].
  Definition opsB : list (nat * qop nat) := [(2, QPeek); (1, QDequeue)].

  Definition run_p4 := run nat nat nat (option nat) Nat.eq_dec Nat.eq_dec space_of qplan qevents.
  Definition run_free := run_cf nat nat nat (option nat) Nat.eq_dec Nat.eq_dec space_of qplan qevents.
  Definition races := check1_spaces nat nat Nat.eq_dec Nat.eq_dec.
  Definition comms := check2_spaces nat nat Nat.eq_dec Nat.eq_dec.

  (* Headline 9. With counter-free peeks, checks #1 and #2 find nothing
     (and no consume of the two logs names more than one channel, so check
     #3 has no join to see), yet no serial order gives both branches their
     observations. After A, B peeks 21 instead of 20. After B, A peeks 11
     instead of 10. With the bumping peek of the P4 code (decision 31),
     the same branches race on both counters. *)
  Theorem counter_free_peek_admits_write_skew :
    let endA := r_store (run_free 1 base opsA) in
    let endB := r_store (run_free 2 base opsB) in
    races (r_log (run_free 1 base opsA)) (r_log (run_free 2 base opsB)) = [] /\
    comms (r_log (run_free 1 base opsA)) (r_log (run_free 2 base opsB)) = [] /\
    Forall (fun e => match e with EvConsume c | EvComm c _ _ => length (c_chs c) = 1 | _ => True end)
      (r_log (run_free 1 base opsA) ++ r_log (run_free 2 base opsB)) /\
    r_obs (run_free 2 endA opsB) <> r_obs (run_free 2 base opsB) /\
    r_obs (run_free 1 endB opsA) <> r_obs (run_free 1 base opsA) /\
    In (ChS 1) (races (r_log (run_p4 1 base opsA)) (r_log (run_p4 2 base opsB))) /\
    In (ChS 2) (races (r_log (run_p4 1 base opsA)) (r_log (run_p4 2 base opsB))).
  Proof.
    cbv zeta.
    split; [vm_compute; reflexivity |].
    split; [vm_compute; reflexivity |].
    split; [vm_compute; repeat constructor |].
    split; [intros H; vm_compute in H; discriminate H |].
    split; [intros H; vm_compute in H; discriminate H |].
    split; vm_compute; repeat (first [left; reflexivity | right]).
  Qed.

  (* The premise of same_block_ops_chain holds for the FIFO plan events. *)
  Lemma qevents_data : plan_events_data nat nat nat qevents.
  Proof.
    intros tag k q v l e He. destruct q as [d | |]; destruct l as [| h l];
      cbn in He; repeat destruct He as [<- | He]; try destruct He; cbn; try exact I;
      intros p [<- | []]; reflexivity.
  Qed.

  (* D.2: unrelated queues still merge. Dequeues of queue 1 and queue 2 do
     not race. *)
  Lemma unrelated_queues_do_not_race :
    races (r_log (run_p4 1 base [(1, QDequeue)])) (r_log (run_p4 2 base [(2, QDequeue)])) = [].
  Proof. vm_compute. reflexivity. Qed.
End WriteSkew.

(* ── Headline assumptions ────────────────────────────────────────────── *)
Print Assumptions check1_race_iff.
Print Assumptions ordered_writes_race_on_counter.
Print Assumptions ordered_reads_race_on_counter.
Print Assumptions first_ops_race_on_space_counter.
Print Assumptions crossed_collections_race.
Print Assumptions same_block_ops_chain.
Print Assumptions unordered_writes_commute.
Print Assumptions merge_without_race_is_serializable.
Print Assumptions WriteSkew.counter_free_peek_admits_write_skew.
