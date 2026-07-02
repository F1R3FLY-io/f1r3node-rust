(* ===========================================================================
   Lca.v - The lowest universal common ancestor (LCUA) and the depth cutoff.

   Fork choice starts its upward rank from the LCA of the (filtered) latest
   messages (estimator.rs:96, calculate_lca -> rank_forkchoices([lca])). The LCA
   must be a COMMON ANCESTOR of every latest message, else the upward walk would
   start off-chain and miss reachable tips. `calculate_lca` (estimator.rs:184):

     * filters out latest messages older than LATEST_MESSAGE_MAX_DEPTH (=1000)
       below the DAG tip (`msg.block_number > top - 1000`), then
     * returns genesis if none remain, else
       DagOperations::lowest_universal_common_ancestor_many(...).

   We model faithfully what the fork choice DEPENDS ON:

     * `depth_filter`  - the concrete `> top - 1000` cutoff (exactly reproduced),
                         and its determinism in `top` (`lca_depth_filter_deterministic`);
     * `lcua_many`     - the LCUA-many BTreeSet fold, MODELED CONCRETELY (below):
                         a fuel-bounded reduction that repeatedly pops the
                         highest-block-number element and re-inserts its parents,
                         mirroring the `BTreeSet<ReverseOrderedBlockMetadata>`
                         fold at dag_operations.rs:172-207 (see the note below);
     * `lca`           - LCUA-many with the empty->genesis fallback wired in;
     * `lca_is_common_ancestor` - the LCUA is a common ancestor of the filtered
                         messages, PROVED from the fold model via the covering
                         invariant (no longer an assumed `lcua_common` premise);
     * `lca_below_has_zero_rank_influence` - a message strictly below a block is
                         NOT in that block's past, so it never scores it.

   ---------------------------------------------------------------------------
   Model note: pairwise_lca <-> ReverseOrderedBlockMetadata fold (dag_operations.rs:172-207)
   ---------------------------------------------------------------------------
   The Rust LCUA-many keeps a `BTreeSet<ReverseOrderedBlockMetadata>` ordered so
   the highest block number is first (ReverseOrderedBlockMetadata = ordering_by_num
   reversed; ordering_by_num = (block_number, then block_hash)). Each loop
   iteration (`:180-204`): while the set has >1 element, pop the FIRST element
   (`current.iter().next()` = the highest-numbered), keep the tail, and insert
   each of that element's parents; stop when one element remains (`:181`), which
   is returned as the LCUA. Our `select_max`/`step`/`reduce` reproduce this exactly
   on a list: `select_max` extracts the max-`numof` element (the ReverseOrdered
   head), `step` replaces it by `parents_of` it (dag_operations.rs:197-200), and
   `reduce` iterates to the single survivor (dag_operations.rs:181,205-207). The
   binary `lowest_universal_common_ancestor` (`:214`) is the two-element instance,
   so pairwise LCA <-> this fold on a 2-element set.

   ---------------------------------------------------------------------------
   RESIDUAL (disclosed premises; the strongest provable form)
   ---------------------------------------------------------------------------
   FULLY UNCONDITIONAL common-ancestor is PROVABLY FALSE for this fold, and the
   Rust code is likewise only correct on a single-rooted DAG: a DAG with two
   DISTINCT parentless blocks a<>b (both well-formed: main_parent None, num 0) and
   ms=[a;b] makes the fold return b (or a), which is NOT a common ancestor of the
   other. So we prove the STRONGEST provable form under the faithful blockchain
   precondition that there is a SINGLE genesis (`single_root d root`: the only
   parentless block in the DAG is root). These hold for every real F1R3FLY DAG
   (one genesis, from which every block descends). FOUR premises are DERIVED, not
   assumed: (1) the `lcua_common` "assume the output is a common ancestor"
   hypothesis (from the covering invariant); (2) the fold's TERMINATION — the fuel
   `lcua_fuel d = (dag_max_num d + 1)*(length d + 1)` is proved adequate
   (`reduce_converges`) via the lexicographic measure `mu = (max_numof,
   count_at_max)` scalar-encoded on ℕ, so no `reduce ... = [g]` premise remains;
   (3) `common_ancestor d ms root` (from `single_root` + `wf_dag` + `all_real`,
   `descends_from_root`); (4) LOWEST-ness maximality (`lcua_many_is_max`, no
   longer a hypothesis). LOWEST-ness (that no lower common ancestor exists) is
   proved under `single_parent_spine` (the main-parent TREE the LUCA descends) +
   `NoDup ms`; the weaker `spine_linear` is provably INSUFFICIENT (straddling old
   parents make the fold over-descend — see Section 7's RESIDUAL note).

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                            | Rust (casper/src/rust/estimator.rs, util/dag_operations.rs)
   --------------------------------+-------------------------------------------
   depth_filter (> top - 1000)     | filter msg.block_number > top - 1000 (estimator:201)
   lca (empty -> genesis)          | calculate_lca (estimator:204-210)
   select_max / step / reduce      | ReverseOrdered BTreeSet fold (dag_operations:172-207)
   lcua_many                       | lowest_universal_common_ancestor_many (dag_operations:158)
   lca_is_common_ancestor          | the LCUA is a common ancestor of the tips
   lca_below_has_zero_rank_influence| hash_parents cutoff below LCA (estimator:230)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Arith.Wf_nat.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From ForkChoice Require Import Foundation.

(* The DAG tip height minus the max look-back window (LATEST_MESSAGE_MAX_DEPTH). *)
Definition LATEST_MESSAGE_MAX_DEPTH : nat := 1000.

(* Keep only latest messages whose block sits within the look-back window of the
   tip: `numof(msg) > top - 1000`. Exactly estimator.rs:201. *)
Definition depth_filter (d : DAG) (top : nat) (lms : list (Validator * BlockHash))
  : list (Validator * BlockHash) :=
  filter (fun e => Nat.ltb (top - LATEST_MESSAGE_MAX_DEPTH) (numof d (snd e))) lms.

(* ===========================================================================
   Section 1 - Common ancestry and the concrete LCUA-many fold model
   =========================================================================== *)

(* `c` is a common ancestor of every hash in `ms`. *)
Definition common_ancestor (d : DAG) (ms : list BlockHash) (c : BlockHash) : Prop :=
  forall m, In m ms -> anc_of d c m.

(* The parents of a hash (empty if the hash is absent), = `head.parents` lookups
   at dag_operations.rs:197-199. *)
Definition parents_of (d : DAG) (h : BlockHash) : list BlockHash :=
  match lookup d h with
  | Some b => blk_parents b
  | None => []
  end.

(* Extract the max-`numof` element of a list as (element, rest). This is the
   ReverseOrderedBlockMetadata head (`current.iter().next()`, highest block
   number) plus the remaining set (`tail`), dag_operations.rs:185-193. *)
Fixpoint select_max (d : DAG) (work : list BlockHash)
  : option (BlockHash * list BlockHash) :=
  match work with
  | [] => None
  | x :: xs =>
    match select_max d xs with
    | None => Some (x, [])
    | Some (m, rest) =>
      if Nat.ltb (numof d m) (numof d x)
      then Some (x, m :: rest)
      else Some (m, x :: rest)
    end
  end.

(* One fold iteration: pop the max element `e`, re-insert its parents (`next`),
   then DEDUP the working set, faithful to the `BTreeSet` (dag_operations.rs
   :195-203). The dedup is load-bearing for termination: it caps the count of
   elements at any block-number level by `length d`, which bounds the fuel. *)
Definition step (d : DAG) (work : list BlockHash) : list BlockHash :=
  nodup Nat.eq_dec
    (match select_max d work with
     | None => []
     | Some (e, rest) => parents_of d e ++ rest
     end).

(* Iterate `step` until a single survivor remains (`current.len() == 1`,
   dag_operations.rs:181), fuel-bounded. The fuel `lcua_fuel d` is the scalar
   encoding of the lexicographic measure `(max_numof, count_at_max)` (see
   `reduce_converges`): each `step` pops the unique arg-max and replaces it by
   strictly-lower parents, so `mu` strictly decreases and the fold reaches a
   singleton within `lcua_fuel d` steps. *)
Fixpoint reduce (d : DAG) (fuel : nat) (work : list BlockHash) {struct fuel}
  : list BlockHash :=
  match work with
  | [] => []
  | _ :: [] => work
  | _ :: _ :: _ =>
    match fuel with
    | 0 => work
    | S f => reduce d f (step d work)
    end
  end.

(* The fuel bound: `(dag_max_num d + 1) * (length d + 1)`, an upper bound on the
   lex measure `mu` of any deduped real working set (`mu_bound_real`). *)
Definition lcua_fuel (d : DAG) : nat := (dag_max_num d + 1) * (length d + 1).

(* The LCUA-many result: the single survivor of the fold (genesis-default 0 on
   the empty input, which never arises for a nonempty `ms`). Convergence to that
   survivor within `lcua_fuel d` is proved (`reduce_converges`). *)
Definition lcua_many (d : DAG) (ms : list BlockHash) : BlockHash :=
  match reduce d (lcua_fuel d) ms with
  | g :: _ => g
  | [] => 0
  end.

(* The LCA: the LCUA-many result when the filtered set is nonempty, else genesis
   (estimator.rs:204). *)
Definition lca (genesis lcua : BlockHash) (d : DAG) (top : nat)
               (lms : list (Validator * BlockHash)) : BlockHash :=
  match depth_filter d top lms with
  | [] => genesis
  | _  => lcua
  end.

(* ===========================================================================
   Section 2 - Well-formed-DAG helper facts (all axiom-free, wf_dag/wf_lookup)
   =========================================================================== *)

(* A parent is an ancestor of its child. *)
Lemma anc_of_parent :
  forall d x b p, lookup d x = Some b -> In p (blk_parents b) -> anc_of d p x.
Proof.
  intros d x b p Hl Hin. eapply anc_par; [exact Hl | exact Hin | apply anc_refl].
Qed.

(* Coverage is parent-monotone: if a covering element `e` is replaced by a parent
   `p`, then `p` still covers `m` (p <= e <= m). One `anc_par` step + trans. *)
Lemma cover_par_mono :
  forall d e b p m,
    lookup d e = Some b -> In p (blk_parents b) -> anc_of d e m -> anc_of d p m.
Proof.
  intros d e b p m Hl Hin Hanc.
  eapply anc_of_trans; [ eapply anc_of_parent; eauto | exact Hanc ].
Qed.

(* A block of number 0 is parentless (parents would need a smaller number). *)
Lemma numof0_parentless :
  forall d h b, wf_dag d -> lookup d h = Some b -> blk_num b = 0 -> blk_parents b = [].
Proof.
  intros d h b Hwf Hl H0.
  destruct (blk_parents b) as [|ph tl] eqn:E; [reflexivity|].
  assert (Hin : In b d) by (eapply lookup_In; eauto).
  destruct (proj1 (Hwf b Hin) ph) as [p [Hlp Hlt]]. { rewrite E; left; reflexivity. }
  lia.
Qed.

(* A parentless block has number 0 (its main parent must be None). *)
Lemma parentless_numof0 :
  forall d h b, wf_dag d -> lookup d h = Some b -> blk_parents b = [] -> blk_num b = 0.
Proof.
  intros d h b Hwf Hl Hpar.
  assert (Hin : In b d) by (eapply lookup_In; eauto).
  destruct (Hwf b Hin) as [_ Hmp].
  destruct (blk_main_parent b) as [mph|] eqn:E.
  - destruct Hmp as [_ Hhd]. rewrite Hpar in Hhd. simpl in Hhd. discriminate.
  - exact Hmp.
Qed.

(* A proper ancestor has a strictly smaller number (used for lowest-ness). *)
Lemma anc_of_proper_lt :
  forall d, wf_dag d -> forall a x, anc_of d a x -> a = x \/ numof d a < numof d x.
Proof.
  intros d Hwf a x H. induction H as [x | a x b p Hlk Hin Hanc IH].
  - left; reflexivity.
  - right.
    assert (Hinb : In b d) by (eapply lookup_In; eauto).
    destruct (proj1 (Hwf b Hinb) p Hin) as [p' [Hlp Hlt]].
    assert (Hnp : numof d p = blk_num p') by (unfold numof; rewrite Hlp; reflexivity).
    assert (Hnx : numof d x = blk_num b) by (unfold numof; rewrite Hlk; reflexivity).
    destruct IH as [Heq | Hlt2]; [subst a|]; lia.
Qed.

(* ===========================================================================
   Section 3 - select_max specification (permutation + maximality)
   =========================================================================== *)

Lemma select_max_none : forall d work, select_max d work = None <-> work = [].
Proof.
  intros d work; split; intro H.
  - destruct work as [|x xs]; [reflexivity|]. simpl in H.
    destruct (select_max d xs) as [[m rest]|];
      [destruct (Nat.ltb (numof d m) (numof d x))|]; discriminate.
  - subst; reflexivity.
Qed.

Lemma select_max_some :
  forall d work, work <> [] -> exists e rest, select_max d work = Some (e, rest).
Proof.
  intros d work Hne. destruct (select_max d work) as [[e rest]|] eqn:E.
  - exists e, rest; reflexivity.
  - apply select_max_none in E. contradiction.
Qed.

Lemma select_max_perm :
  forall d work e rest, select_max d work = Some (e, rest) -> Permutation work (e :: rest).
Proof.
  induction work as [|x xs IH]; intros e rest H; simpl in H; [discriminate|].
  destruct (select_max d xs) as [[m r']|] eqn:E.
  - destruct (Nat.ltb (numof d m) (numof d x)) eqn:Elt; inversion H; subst.
    + apply perm_skip. apply IH; reflexivity.
    + eapply Permutation_trans; [apply perm_skip; apply IH; reflexivity | apply perm_swap].
  - inversion H; subst. apply select_max_none in E. subst xs. apply Permutation_refl.
Qed.

Lemma select_max_max :
  forall d work e rest,
    select_max d work = Some (e, rest) -> forall y, In y work -> numof d y <= numof d e.
Proof.
  induction work as [|x xs IH]; intros e rest H y Hy; simpl in H; [inversion Hy|].
  destruct (select_max d xs) as [[m r']|] eqn:E.
  - destruct (Nat.ltb (numof d m) (numof d x)) eqn:Elt; inversion H; subst; clear H;
      apply Nat.ltb_lt in Elt || apply Nat.ltb_ge in Elt.
    + destruct Hy as [->|Hin]; [lia|]. specialize (IH m r' eq_refl y Hin). lia.
    + destruct Hy as [->|Hin]; [lia|]. specialize (IH e r' eq_refl y Hin). lia.
  - inversion H; subst. apply select_max_none in E. subst xs. destruct Hy as [->|[]]. lia.
Qed.

(* ===========================================================================
   Section 4 - The covering invariant and its preservation

   `covers d ms work` := every message in `ms` has a covering ancestor in `work`.
   It holds at init (each m covers itself) and is preserved by `step` under the
   single-genesis precondition: a popped non-genesis element is replaced by a
   parent that still covers (cover_par_mono); a popped genesis element is the max,
   hence every worker has number 0, hence every remaining worker is genesis
   (single_root), and genesis covers every message (common_ancestor .. root). At a
   single survivor, coverage IS common-ancestor-ness.
   =========================================================================== *)

(* Every worker resolves to a real block (preserved by wf_dag parent expansion). *)
Definition all_real (d : DAG) (work : list BlockHash) : Prop :=
  forall e, In e work -> exists b, lookup d e = Some b.

(* Single genesis: the only parentless block in the DAG is `root`
   (validate.rs::block_number enforces one genesis; every block descends from it). *)
Definition single_root (d : DAG) (root : BlockHash) : Prop :=
  forall h b, lookup d h = Some b -> blk_parents b = [] -> h = root.

Definition covers (d : DAG) (ms work : list BlockHash) : Prop :=
  forall m, In m ms -> exists e, In e work /\ anc_of d e m.

Lemma covers_refl : forall d ms, covers d ms ms.
Proof. intros d ms m Hm. exists m. split; [exact Hm | apply anc_refl]. Qed.

Lemma covers_perm :
  forall d ms w1 w2, Permutation w1 w2 -> covers d ms w1 -> covers d ms w2.
Proof.
  intros d ms w1 w2 Hp Hc m Hm. destruct (Hc m Hm) as [e [He Hanc]].
  exists e. split; [eapply Permutation_in; eauto | exact Hanc].
Qed.

Lemma covers_singleton : forall d ms g, covers d ms [g] -> common_ancestor d ms g.
Proof.
  intros d ms g Hc m Hm. destruct (Hc m Hm) as [e [He Hanc]].
  destruct He as [->|[]]. exact Hanc.
Qed.

Lemma step_preserves_real :
  forall d work, wf_dag d -> all_real d work -> all_real d (step d work).
Proof.
  intros d work Hwf Har. unfold step, all_real. intros x Hx. rewrite nodup_In in Hx.
  destruct (select_max d work) as [[e rest]|] eqn:E.
  - assert (Hperm : Permutation work (e :: rest)) by (eapply select_max_perm; eauto).
    apply in_app_or in Hx. destruct Hx as [Hxp | Hxr].
    + (* x is a parent of e : real by wf_dag *)
      unfold parents_of in Hxp. destruct (lookup d e) as [be|] eqn:Ee; [| destruct Hxp].
      assert (Hin : In be d) by (eapply lookup_In; eauto).
      destruct (proj1 (Hwf be Hin) x Hxp) as [p [Hlp _]]. exists p; exact Hlp.
    + (* x is in rest : real because it was in work *)
      apply Har. eapply Permutation_in; [apply Permutation_sym; exact Hperm | right; exact Hxr].
  - destruct Hx.
Qed.

Lemma step_preserves_covers :
  forall d root ms work,
    wf_dag d -> wf_lookup d -> single_root d root ->
    common_ancestor d ms root ->
    all_real d work -> 2 <= length work ->
    covers d ms work -> covers d ms (step d work).
Proof.
  intros d root ms work Hwf Hwl Hsr Hca Har Hlen Hcov.
  assert (Hne : work <> []) by (destruct work; simpl in Hlen; [lia | discriminate]).
  destruct (select_max_some d work Hne) as [e [rest E]].
  assert (Hperm : Permutation work (e :: rest)) by (eapply select_max_perm; eauto).
  (* rest is nonempty (length work >= 2) *)
  assert (Hlrest : rest <> []).
  { intro Hr. subst rest.
    assert (Hl2 : length work = length [e]) by (apply Permutation_length; exact Hperm).
    simpl in Hl2. lia. }
  (* coverage transported to the (e :: rest) view *)
  assert (Hcov' : covers d ms (e :: rest)) by (eapply covers_perm; eauto).
  unfold step. rewrite E.
  intros m Hm.
  (* the dedup preserves membership; discharge coverage into `parents_of e ++ rest` *)
  cut (exists e1, In e1 (parents_of d e ++ rest) /\ anc_of d e1 m).
  { intros [e1 [Hi Ha]]. exists e1. split; [ rewrite nodup_In; exact Hi | exact Ha ]. }
  destruct (Hcov' m Hm) as [e0 [He0 Hanc0]].
  destruct He0 as [Heq | He0rest].
  - (* e0 = e : the popped max *)
    subst e0.
    assert (Hine : In e work) by
      (eapply Permutation_in; [apply Permutation_sym; exact Hperm | left; reflexivity]).
    destruct (Har e Hine) as [be Hbe].
    unfold parents_of. rewrite Hbe.
    destruct (blk_parents be) as [| p ptl] eqn:Epar.
    + (* e is parentless => e is the max => all workers have number 0 => all = root *)
      assert (Hne0 : numof d e = 0).
      { unfold numof. rewrite Hbe. eapply parentless_numof0; eauto. }
      (* pick the head of rest; it is in work, has number <= numof e = 0, so 0 *)
      destruct rest as [| r0 rtl]; [contradiction|].
      assert (Hinr0 : In r0 work) by
        (eapply Permutation_in; [apply Permutation_sym; exact Hperm | right; left; reflexivity]).
      assert (Hr0max : numof d r0 <= numof d e) by (eapply select_max_max; eauto).
      assert (Hr0z : numof d r0 = 0) by lia.
      destruct (Har r0 Hinr0) as [br0 Hbr0].
      assert (Hbr0n : blk_num br0 = 0) by (unfold numof in Hr0z; rewrite Hbr0 in Hr0z; exact Hr0z).
      assert (Hr0par : blk_parents br0 = []) by (eapply numof0_parentless; eauto).
      assert (Hr0root : r0 = root) by (eapply Hsr; eauto).
      exists r0. split.
      * apply in_or_app. right. left. reflexivity.
      * rewrite Hr0root. apply Hca. exact Hm.
    + (* e has a parent p : p still covers m *)
      exists p. split.
      * apply in_or_app. left. left. reflexivity.
      * eapply cover_par_mono; [ exact Hbe | rewrite Epar; left; reflexivity | exact Hanc0 ].
  - (* e0 in rest : survives into the tail *)
    exists e0. split; [ apply in_or_app; right; exact He0rest | exact Hanc0 ].
Qed.

(* Fold-unfolding equations (definitional). *)
Lemma reduce_0 : forall d work, reduce d 0 work = work.
Proof. intros d work. destruct work as [|a [|b l]]; reflexivity. Qed.

Lemma reduce_unfold_2 :
  forall d f a b l, reduce d (S f) (a :: b :: l) = reduce d f (step d (a :: b :: l)).
Proof. reflexivity. Qed.

Lemma reduce_preserves_covers :
  forall fuel d root ms work,
    wf_dag d -> wf_lookup d -> single_root d root ->
    common_ancestor d ms root ->
    all_real d work -> covers d ms work ->
    covers d ms (reduce d fuel work).
Proof.
  induction fuel as [|f IH]; intros d root ms work Hwf Hwl Hsr Hca Har Hcov.
  - rewrite reduce_0. exact Hcov.
  - destruct work as [|a [|b l]].
    + simpl. exact Hcov.
    + simpl. exact Hcov.
    + rewrite reduce_unfold_2.
      eapply IH; eauto.
      * apply step_preserves_real; assumption.
      * eapply step_preserves_covers; eauto. simpl; lia.
Qed.

(* ===========================================================================
   Section 4b - Fold TERMINATION: the lexicographic measure and convergence

   The disclosed convergence premise `reduce ... = [g]` is DISCHARGED here. The
   measure is `mu(W) = max_numof(W) * (|d|+1) + count_at_max(W)` on ℕ (the scalar
   encoding of the lex order `(max_numof, count_at_max)` on ℕ×ℕ). Each `step`
   pops the unique arg-max (max `numof`, deterministic selection) and replaces it
   with STRICTLY-LOWER parents (wf_dag), then dedups:
     * count_at_max > 1  => count_at_max drops by 1, max unchanged  (mu lex-drops)
     * count_at_max = 1  => max strictly drops                      (mu lex-drops)
   The dedup (BTreeSet-faithful) caps count_at_max by `length d`, so
   `mu(W) <= lcua_fuel d`; since `mu` drops by >= 1 per step the fold reaches a
   singleton within `lcua_fuel d` steps.
   =========================================================================== *)

(* Maximum `numof` over a working list. *)
Fixpoint max_numof (d : DAG) (W : list BlockHash) : nat :=
  match W with [] => 0 | h :: t => Nat.max (numof d h) (max_numof d t) end.

Lemma numof_le_max_numof : forall d W h, In h W -> numof d h <= max_numof d W.
Proof.
  induction W as [|x xs IH]; simpl; intros h Hin; [contradiction|].
  destruct Hin as [->|Hin]; [apply Nat.le_max_l|].
  eapply Nat.le_trans; [apply IH; exact Hin | apply Nat.le_max_r].
Qed.

Lemma max_numof_le : forall d W B, (forall h, In h W -> numof d h <= B) -> max_numof d W <= B.
Proof.
  induction W as [|x xs IH]; simpl; intros B H; [apply Nat.le_0_l|].
  apply Nat.max_lub; [apply H; left; reflexivity | apply IH; intros h Hin; apply H; right; exact Hin].
Qed.

Lemma max_numof_In : forall d W, W <> [] -> exists h, In h W /\ numof d h = max_numof d W.
Proof.
  induction W as [|x xs IH]; intros Hne; [contradiction|].
  destruct xs as [|y ys].
  - exists x. split; [left; reflexivity|]. simpl. rewrite Nat.max_0_r. reflexivity.
  - destruct (IH ltac:(discriminate)) as [h [Hin Heq]].
    change (max_numof d (x :: y :: ys)) with (Nat.max (numof d x) (max_numof d (y::ys))).
    destruct (Nat.max_dec (numof d x) (max_numof d (y::ys))) as [Hm|Hm].
    + exists x. split; [left; reflexivity|]. rewrite Hm. reflexivity.
    + exists h. split; [right; exact Hin|]. rewrite Hm. exact Heq.
Qed.

(* Count of elements of `W` at block-number level `M` (resp. the max level). *)
Definition count_eq (d : DAG) (M : nat) (W : list BlockHash) : nat :=
  length (filter (fun h => Nat.eqb (numof d h) M) W).
Definition count_at_max (d : DAG) (W : list BlockHash) : nat := count_eq d (max_numof d W) W.

Lemma filter_le : forall (A:Type) (P:A->bool) L, length (filter P L) <= length L.
Proof. induction L as [|x xs IH]; simpl; [lia|]. destruct (P x); simpl; lia. Qed.

Lemma filter_none : forall (A:Type) (P:A->bool) L,
  (forall x, In x L -> P x = false) -> filter P L = [].
Proof.
  induction L as [|x xs IH]; simpl; intros H; [reflexivity|].
  rewrite (H x (or_introl eq_refl)). apply IH. intros y Hy. apply H. right; exact Hy.
Qed.

Lemma Permutation_filter : forall (A:Type) (P:A->bool) l l',
  Permutation l l' -> Permutation (filter P l) (filter P l').
Proof.
  intros A P l l' H. induction H; simpl.
  - apply perm_nil.
  - destruct (P x); [apply perm_skip; assumption | assumption].
  - destruct (P x); destruct (P y); try apply perm_swap; try apply Permutation_refl.
  - eapply Permutation_trans; eassumption.
Qed.

Lemma count_eq_perm : forall d M l l', Permutation l l' -> count_eq d M l = count_eq d M l'.
Proof. intros d M l l' H. unfold count_eq. apply Permutation_length, Permutation_filter, H. Qed.

Lemma count_at_max_pos : forall d W, W <> [] -> 1 <= count_at_max d W.
Proof.
  intros d W Hne. unfold count_at_max, count_eq.
  destruct (max_numof_In d W Hne) as [h [Hin Heq]].
  assert (Hf : In h (filter (fun x => Nat.eqb (numof d x) (max_numof d W)) W)).
  { apply filter_In. split; [exact Hin | apply Nat.eqb_eq; exact Heq]. }
  destruct (filter (fun x => Nat.eqb (numof d x) (max_numof d W)) W); [destruct Hf | simpl; lia].
Qed.

Lemma filter_nodup_le : forall d M L,
  length (filter (fun h => Nat.eqb (numof d h) M) (nodup Nat.eq_dec L))
  <= length (filter (fun h => Nat.eqb (numof d h) M) L).
Proof.
  intros d M L. apply NoDup_incl_length.
  - apply NoDup_filter, NoDup_nodup.
  - intros x Hx. apply filter_In in Hx. destruct Hx as [Hin Hp].
    apply filter_In. split; [ rewrite nodup_In in Hin; exact Hin | exact Hp ].
Qed.

Lemma real_nodup_length_bound : forall d W,
  NoDup W -> all_real d W -> length W <= length d.
Proof.
  intros d W Hnd Har.
  assert (Hincl : incl W (map blk_hash d)).
  { intros h Hin. destruct (Har h Hin) as [b Hb].
    apply lookup_hash in Hb as Hh. apply lookup_In in Hb as Hb'.
    rewrite <- Hh. apply in_map. exact Hb'. }
  pose proof (NoDup_incl_length Hnd Hincl) as HH. rewrite length_map in HH. exact HH.
Qed.

(* The scalar lexicographic measure. *)
Definition mu (d : DAG) (W : list BlockHash) : nat :=
  max_numof d W * (length d + 1) + count_at_max d W.

(* The popped arg-max carries the maximum `numof`. *)
Lemma select_max_numof_eq : forall d W e rest,
  select_max d W = Some (e, rest) -> numof d e = max_numof d W.
Proof.
  intros d W e rest H.
  assert (Hperm : Permutation W (e::rest)) by (eapply select_max_perm; eauto).
  assert (Hin : In e W) by (eapply Permutation_in; [apply Permutation_sym; exact Hperm|left; reflexivity]).
  apply Nat.le_antisymm.
  - apply numof_le_max_numof; exact Hin.
  - apply max_numof_le. intros h Hh. eapply select_max_max; eauto.
Qed.

(* Parents are strictly lower (wf_dag). *)
Lemma parents_numof_lt : forall d e be p,
  wf_dag d -> lookup d e = Some be -> In p (blk_parents be) -> numof d p < numof d e.
Proof.
  intros d e be p Hwf Hl Hin.
  assert (Hinb : In be d) by (eapply lookup_In; eauto).
  destruct (proj1 (Hwf be Hinb) p Hin) as [p' [Hlp Hlt]].
  assert (numof d p = blk_num p') by (unfold numof; rewrite Hlp; reflexivity).
  assert (numof d e = blk_num be) by (unfold numof; rewrite Hl; reflexivity). lia.
Qed.

(* (A) the max never increases. *)
Lemma step_max_le : forall d W, wf_dag d -> all_real d W -> max_numof d (step d W) <= max_numof d W.
Proof.
  intros d W Hwf Har. unfold step. destruct (select_max d W) as [[e rest]|] eqn:E.
  - assert (Hperm : Permutation W (e::rest)) by (eapply select_max_perm; eauto).
    assert (HeM : numof d e = max_numof d W) by (eapply select_max_numof_eq; eauto).
    assert (Hine : In e W) by (eapply Permutation_in; [apply Permutation_sym; exact Hperm|left; reflexivity]).
    destruct (Har e Hine) as [be Hbe].
    apply max_numof_le. intros h Hh. rewrite nodup_In in Hh. apply in_app_or in Hh.
    destruct Hh as [Hhp|Hhr].
    + unfold parents_of in Hhp. rewrite Hbe in Hhp.
      pose proof (parents_numof_lt _ _ _ _ Hwf Hbe Hhp) as Hlt. lia.
    + assert (In h W) by (eapply Permutation_in; [apply Permutation_sym; exact Hperm|right; exact Hhr]).
      apply numof_le_max_numof; assumption.
  - simpl. apply Nat.le_0_l.
Qed.

(* (D) the deduped max-level count is bounded by `length d`. *)
Lemma step_count_bound : forall d W, wf_dag d -> all_real d W -> count_at_max d (step d W) <= length d.
Proof.
  intros d W Hwf Har.
  assert (Hnd : NoDup (step d W)) by (unfold step; apply NoDup_nodup).
  assert (Hr : all_real d (step d W)) by (apply step_preserves_real; assumption).
  pose proof (real_nodup_length_bound d (step d W) Hnd Hr) as Hlen.
  unfold count_at_max, count_eq. eapply Nat.le_trans; [apply filter_le | exact Hlen].
Qed.

(* (C) if the max is preserved, the max-level count strictly drops. *)
Lemma step_count_lt : forall d W e rest,
  wf_dag d -> all_real d W -> NoDup W -> select_max d W = Some (e,rest) ->
  max_numof d (step d W) = max_numof d W -> count_at_max d (step d W) < count_at_max d W.
Proof.
  intros d W e rest Hwf Har Hnd Hsel HmaxEq.
  assert (Hperm : Permutation W (e::rest)) by (eapply select_max_perm; eauto).
  assert (HeM : numof d e = max_numof d W) by (eapply select_max_numof_eq; eauto).
  assert (Hine : In e W) by (eapply Permutation_in; [apply Permutation_sym; exact Hperm|left; reflexivity]).
  destruct (Har e Hine) as [be Hbe].
  set (M := max_numof d W) in *.
  assert (HcW : count_at_max d W = S (count_eq d M rest)).
  { unfold count_at_max. fold M. rewrite (count_eq_perm d M W (e::rest) Hperm).
    unfold count_eq. simpl filter. rewrite (proj2 (Nat.eqb_eq (numof d e) M) HeM).
    simpl length. reflexivity. }
  assert (Hstep : count_at_max d (step d W) <= count_eq d M rest).
  { unfold count_at_max. rewrite HmaxEq. fold M. unfold step. rewrite Hsel.
    eapply Nat.le_trans; [apply filter_nodup_le|].
    unfold count_eq. rewrite filter_app. rewrite length_app.
    assert (Hpar0 : filter (fun h => Nat.eqb (numof d h) M) (parents_of d e) = []).
    { apply filter_none. intros p Hp. unfold parents_of in Hp. rewrite Hbe in Hp.
      pose proof (parents_numof_lt _ _ _ _ Hwf Hbe Hp) as Hlt. apply Nat.eqb_neq. lia. }
    rewrite Hpar0. simpl. lia. }
  lia.
Qed.

(* mu strictly decreases per step (2 <= |W|). *)
Lemma step_mu_lt : forall d W, wf_dag d -> all_real d W -> NoDup W -> 2 <= length W ->
  mu d (step d W) < mu d W.
Proof.
  intros d W Hwf Har Hnd Hlen.
  assert (Hne : W <> []) by (destruct W; simpl in Hlen; [lia|discriminate]).
  destruct (select_max_some d W Hne) as [e [rest Hsel]].
  assert (HA : max_numof d (step d W) <= max_numof d W) by (apply step_max_le; assumption).
  assert (HD : count_at_max d (step d W) <= length d) by (apply step_count_bound; assumption).
  unfold mu. destruct (Nat.lt_ge_cases (max_numof d (step d W)) (max_numof d W)) as [Hlt|Hge].
  - nia.
  - assert (Heq : max_numof d (step d W) = max_numof d W) by lia.
    assert (Hcnt : count_at_max d (step d W) < count_at_max d W) by (eapply step_count_lt; eauto).
    nia.
Qed.

Lemma step_nonempty : forall d W, 2 <= length W -> step d W <> [].
Proof.
  intros d W Hlen.
  assert (Hne : W <> []) by (destruct W; simpl in Hlen; [lia|discriminate]).
  destruct (select_max_some d W Hne) as [e [rest Hsel]].
  assert (Hperm : Permutation W (e::rest)) by (eapply select_max_perm; eauto).
  assert (Hrest : rest <> []).
  { intro Hr. subst rest. apply Permutation_length in Hperm. simpl in Hperm. lia. }
  destruct rest as [|r0 rtl]; [contradiction|]. unfold step. rewrite Hsel. intro Hc.
  assert (In r0 (nodup Nat.eq_dec (parents_of d e ++ r0 :: rtl))).
  { rewrite nodup_In. apply in_or_app. right. left. reflexivity. }
  rewrite Hc in H. destruct H.
Qed.

(* mu of any deduped real working set is bounded by lcua_fuel d - 1. *)
Lemma mu_bound_real : forall d W, NoDup W -> all_real d W ->
  mu d W <= dag_max_num d * (length d + 1) + length d.
Proof.
  intros d W Hnd Har. unfold mu.
  assert (Hmax : max_numof d W <= dag_max_num d)
    by (apply max_numof_le; intros h _; apply numof_le_max).
  assert (Hcnt : count_at_max d W <= length d).
  { unfold count_at_max, count_eq.
    eapply Nat.le_trans; [apply filter_le | apply real_nodup_length_bound; assumption]. }
  nia.
Qed.

(* The fold reaches a singleton whenever the fuel dominates mu. *)
Lemma reduce_converges_aux : forall fuel d W,
  wf_dag d -> NoDup W -> all_real d W -> W <> [] -> mu d W <= fuel ->
  exists g, reduce d fuel W = [g].
Proof.
  induction fuel as [|f IH]; intros d W Hwf Hnd Har Hne Hmu.
  - destruct W as [|a [|b l]]; [contradiction | exists a; reflexivity |].
    exfalso. pose proof (count_at_max_pos d (a::b::l) ltac:(discriminate)) as Hp.
    unfold mu in Hmu. nia.
  - destruct W as [|a [|b l]]; [contradiction | exists a; reflexivity |].
    rewrite reduce_unfold_2.
    assert (Hlen : 2 <= length (a::b::l)) by (simpl; lia).
    apply (IH d (step d (a::b::l))).
    + exact Hwf.
    + unfold step; apply NoDup_nodup.
    + apply step_preserves_real; assumption.
    + apply step_nonempty; exact Hlen.
    + assert (Hlt : mu d (step d (a::b::l)) < mu d (a::b::l)) by (apply step_mu_lt; assumption). lia.
Qed.

(* MAIN discharge: for a nonempty real `ms`, the fold at fuel `lcua_fuel d`
   converges to a single survivor. *)
Theorem reduce_converges : forall d ms,
  wf_dag d -> all_real d ms -> ms <> [] -> exists g, reduce d (lcua_fuel d) ms = [g].
Proof.
  intros d ms Hwf Har Hne.
  destruct ms as [|a [|b l]]; [contradiction | |].
  - exists a. unfold lcua_fuel. destruct ((dag_max_num d + 1) * (length d + 1)); reflexivity.
  - remember (lcua_fuel d) as F eqn:EF.
    assert (HF1 : 1 <= F) by (subst F; unfold lcua_fuel; nia).
    destruct F as [|f]; [lia|]. rewrite reduce_unfold_2.
    assert (Hlen : 2 <= length (a::b::l)) by (simpl; lia).
    apply (reduce_converges_aux f d (step d (a::b::l))).
    + exact Hwf.
    + unfold step; apply NoDup_nodup.
    + apply step_preserves_real; assumption.
    + apply step_nonempty; exact Hlen.
    + assert (Hmu : mu d (step d (a::b::l)) <= dag_max_num d * (length d + 1) + length d)
        by (apply mu_bound_real; [unfold step; apply NoDup_nodup | apply step_preserves_real; assumption]).
      assert (Hf : S f = (dag_max_num d + 1) * (length d + 1))
        by (rewrite EF; unfold lcua_fuel; reflexivity).
      nia.
Qed.

(* ===========================================================================
   Section 5 - The kept determinism / genesis / cutoff theorems
   =========================================================================== *)

(* The depth cutoff is a pure function of `top`: equal tips give equal filters
   (determinism - two nodes with the same tip filter identically). *)
Theorem lca_depth_filter_deterministic :
  forall d top1 top2 lms,
    top1 = top2 -> depth_filter d top1 lms = depth_filter d top2 lms.
Proof. intros d top1 top2 lms H. subst. reflexivity. Qed.

(* When the filtered set is empty, the LCA is genesis (estimator.rs:204). *)
Theorem lca_empty_is_genesis :
  forall genesis lcua d top lms,
    depth_filter d top lms = [] -> lca genesis lcua d top lms = genesis.
Proof.
  intros genesis lcua d top lms H. unfold lca. rewrite H. reflexivity.
Qed.

(* ===========================================================================
   Section 5b - Every block descends from the unique genesis (C4)

   `common_ancestor d ms root` is no longer a premise: it is DERIVED from
   `single_root d root` + `wf_dag d` + `all_real d ms`. The key is that in a
   single-genesis DAG every block descends from `root` (a spine walk that always
   reaches genesis), proved by well-founded induction on `numof`.
   =========================================================================== *)

Lemma hd_error_in :
  forall (A : Type) (l : list A) x, hd_error l = Some x -> In x l.
Proof.
  intros A l x H. destruct l as [|y ys]; simpl in H; [discriminate|].
  inversion H; left; reflexivity.
Qed.

(* Every real block descends from the unique genesis `root`. Well-founded
   induction on `numof d b`: a genesis block IS root (single_root), else the
   strictly-lower main parent descends from root (IH) and one `anc_par` step
   lifts that to the block. *)
Lemma descends_from_root :
  forall d root, wf_dag d -> single_root d root ->
    forall b bb, lookup d b = Some bb -> anc_of d root b.
Proof.
  intros d root Hwf Hsr b.
  induction b as [b IH] using (well_founded_ind (well_founded_ltof BlockHash (numof d))).
  intros bb Hb.
  assert (Hin : In bb d) by (eapply lookup_In; eauto).
  destruct (Hwf bb Hin) as [Hpar Hmp].
  destruct (blk_main_parent bb) as [mph|] eqn:Emp.
  - destruct Hmp as [[p [Hlp Hlt]] Hhd].
    assert (Hmph_in : In mph (blk_parents bb)) by (apply hd_error_in; exact Hhd).
    assert (Hlt' : ltof BlockHash (numof d) mph b).
    { unfold ltof.
      assert (numof d mph = blk_num p) by (unfold numof; rewrite Hlp; reflexivity).
      assert (numof d b = blk_num bb) by (unfold numof; rewrite Hb; reflexivity). lia. }
    pose proof (IH mph Hlt' p Hlp) as Hanc_mph.
    eapply anc_par; [ exact Hb | exact Hmph_in | exact Hanc_mph ].
  - assert (Hpar0 : blk_parents bb = []) by (eapply numof0_parentless; eauto).
    assert (Hbroot : b = root) by (eapply Hsr; eauto).
    rewrite Hbroot. apply anc_of_refl.
Qed.

(* Hence genesis is a common ancestor of any real message set. *)
Lemma common_ancestor_root :
  forall d root ms, wf_dag d -> single_root d root -> all_real d ms ->
    common_ancestor d ms root.
Proof.
  intros d root ms Hwf Hsr Har m Hm.
  destruct (Har m Hm) as [bb Hb]. eapply descends_from_root; eauto.
Qed.

(* ===========================================================================
   Section 6 - LCUA-many IS a common ancestor (from the fold model)

   Both the `lcua_common` hypothesis AND the `common_ancestor d ms root` premise
   are gone: the former is DERIVED from the covering invariant, the latter from
   `single_root` + `wf_dag` + `all_real` (Section 5b). The fold's termination is
   likewise discharged (Section 4b). Only `single_root` + `all_real` (both
   necessary; see the RESIDUAL note) remain.
   =========================================================================== *)

Theorem lcua_many_common_ancestor :
  forall d root ms,
    wf_dag d -> wf_lookup d ->
    single_root d root ->
    all_real d ms ->
    common_ancestor d ms (lcua_many d ms).
Proof.
  intros d root ms Hwf Hwl Hsr Har m Hm.
  assert (Hca : common_ancestor d ms root) by (eapply common_ancestor_root; eauto).
  assert (Hne : ms <> []) by (intro E; rewrite E in Hm; destruct Hm).
  destruct (reduce_converges d ms Hwf Har Hne) as [g Hg].
  pose proof (reduce_preserves_covers (lcua_fuel d) d root ms ms
                Hwf Hwl Hsr Hca Har (covers_refl d ms)) as Hcov.
  rewrite Hg in Hcov.
  assert (Hlm : lcua_many d ms = g) by (unfold lcua_many; rewrite Hg; reflexivity).
  rewrite Hlm.
  pose proof (covers_singleton d ms g Hcov) as Hcs. unfold common_ancestor in Hcs.
  apply Hcs. exact Hm.
Qed.

(* The LCA is a common ancestor of every (filtered) latest message. The nonempty
   case is `lcua_many_common_ancestor`; the empty case never arises (a member of
   `depth_filter` witnesses nonemptiness). *)
Theorem lca_is_common_ancestor :
  forall genesis root d top lms,
    wf_dag d -> wf_lookup d ->
    single_root d root ->
    all_real d (map snd (depth_filter d top lms)) ->
    forall lm, In lm (depth_filter d top lms) ->
      anc_of d (lca genesis (lcua_many d (map snd (depth_filter d top lms))) d top lms) (snd lm).
Proof.
  intros genesis root d top lms Hwf Hwl Hsr Har lm Hin.
  pose proof (lcua_many_common_ancestor d root (map snd (depth_filter d top lms))
                Hwf Hwl Hsr Har) as Hca'.
  unfold common_ancestor in Hca'.
  (* the filter is nonempty (lm witnesses it), so lca = lcua_many *)
  unfold lca. destruct (depth_filter d top lms) eqn:E.
  - destruct Hin.
  - apply Hca'. apply in_map. exact Hin.
Qed.

(* ===========================================================================
   Section 7 - LOWEST-ness, DERIVED (no maximality premise) — C2

   `lca_is_lowest` no longer ASSUMES the fold's survivor is the numof-greatest
   common ancestor: that maximality is DERIVED. The bridge is
   `single_parent_spine` (each block's `blk_parents` is exactly its main parent
   — the main-parent TREE the LUCA descends) together with `NoDup ms` (the fold's
   input is a set, faithful to the Rust `BTreeSet`).

   RESIDUAL / why not `spine_linear`: the earlier `spine_linear [c; lcua_many]`
   premise is INSUFFICIENT to derive maximality — it is provably too weak. A
   well-formed single-genesis DAG whose blocks carry a "straddling" old parent
   (e ⊒ p' ⊒ c ⊒ p'' with p'' BOTH a parent of e and below the common ancestor c)
   satisfies `spine_linear` yet makes the fold (and the Rust `BTreeSet` fold)
   OVER-DESCEND past c to p'', so `numof c ≤ numof (survivor)` is FALSE. Hence the
   old maximality premise was silently false on such DAGs (making `lca_is_lowest`
   vacuous there). `single_parent_spine` rules straddling out: every block has a
   single parent, so peeling never drops below a common ancestor
   (`step_preserves_below_all`), and the survivor is ⊒ every common ancestor.
   =========================================================================== *)

(* Each block's parents are exactly its main parent (a tree), or empty at genesis
   — the main-parent spine the LUCA descends. *)
Definition single_parent_spine (d : DAG) : Prop :=
  forall h bb, lookup d h = Some bb ->
    match blk_main_parent bb with
    | None => blk_parents bb = []
    | Some mp => blk_parents bb = [mp]
    end.

(* `c` sits at or below every worker. *)
Definition below_all (d : DAG) (c : BlockHash) (work : list BlockHash) : Prop :=
  forall e, In e work -> anc_of d c e.

(* One-step inversion of ancestry. *)
Lemma anc_of_inv :
  forall d c x, anc_of d c x ->
    c = x \/ (exists b p, lookup d x = Some b /\ In p (blk_parents b) /\ anc_of d c p).
Proof.
  intros d c x H. inversion H; subst.
  - left; reflexivity.
  - right. eexists; eexists; split; [eassumption | split; eassumption].
Qed.

(* Through a single-parent block, a proper ancestor descends via that parent. *)
Lemma anc_of_step_parent :
  forall d c e be mp,
    lookup d e = Some be -> blk_parents be = [mp] -> anc_of d c e ->
    c = e \/ anc_of d c mp.
Proof.
  intros d c e be mp Hbe Hpar Hanc.
  destruct (anc_of_inv d c e Hanc) as [Heq | [b0 [p [Hl [Hp Ha]]]]].
  - left; exact Heq.
  - right. rewrite Hbe in Hl. inversion Hl; subst b0. rewrite Hpar in Hp.
    destruct Hp as [<-|[]]. exact Ha.
Qed.

(* `below_all c` is preserved by a step on a single-parent tree: the popped max
   `e` cannot equal `c` while |work| >= 2 (else all deduped workers would collapse
   to `c`, a singleton), so `e` is non-genesis with single parent `mp`, and
   `c <= e` (c <> e) forces `c <= mp`. *)
Lemma step_preserves_below_all :
  forall d root c work,
    wf_dag d -> single_root d root -> single_parent_spine d ->
    all_real d work -> NoDup work -> 2 <= length work ->
    below_all d c work -> below_all d c (step d work).
Proof.
  intros d root c work Hwf Hsr Hsp Har Hnd Hlen Hba.
  assert (Hne : work <> []) by (destruct work; simpl in Hlen; [lia|discriminate]).
  destruct (select_max_some d work Hne) as [e [rest Hsel]].
  assert (Hperm : Permutation work (e::rest)) by (eapply select_max_perm; eauto).
  assert (Hine : In e work) by
    (eapply Permutation_in; [apply Permutation_sym; exact Hperm|left; reflexivity]).
  assert (Hce : anc_of d c e) by (apply Hba; exact Hine).
  destruct (Har e Hine) as [be Hbe].
  assert (Hcne : c <> e).
  { intro Heqce. subst e.
    assert (Hall : forall w, In w work -> w = c).
    { intros w Hw. assert (Hcw : anc_of d c w) by (apply Hba; exact Hw).
      assert (Hwmax : numof d w <= numof d c) by (eapply select_max_max; eauto).
      destruct (anc_of_proper_lt d Hwf c w Hcw) as [Heq|Hlt]; [ symmetry; exact Heq | lia ]. }
    destruct work as [|w1 [|w2 tl]]; simpl in Hlen; try lia.
    inversion Hnd as [|x xs Hnotin Hnd']; subst. apply Hnotin.
    assert (w1 = c) by (apply Hall; left; reflexivity).
    assert (w2 = c) by (apply Hall; right; left; reflexivity). subst. left; reflexivity. }
  intros w Hw. unfold step in Hw. rewrite Hsel in Hw. rewrite nodup_In in Hw.
  apply in_app_or in Hw. destruct Hw as [Hwp | Hwr].
  - unfold parents_of in Hwp. rewrite Hbe in Hwp.
    pose proof (Hsp e be Hbe) as Hspe.
    destruct (blk_main_parent be) as [mp|] eqn:Emp.
    + rewrite Hspe in Hwp. destruct Hwp as [<-|[]].
      destruct (anc_of_step_parent d c e be mp Hbe Hspe Hce) as [Heq|Hanc];
        [ contradiction | exact Hanc ].
    + rewrite Hspe in Hwp. destruct Hwp.
  - apply Hba. eapply Permutation_in; [apply Permutation_sym; exact Hperm | right; exact Hwr].
Qed.

Lemma reduce_preserves_below_all :
  forall fuel d root c work,
    wf_dag d -> single_root d root -> single_parent_spine d ->
    all_real d work -> NoDup work -> below_all d c work ->
    below_all d c (reduce d fuel work).
Proof.
  induction fuel as [|f IH]; intros d root c work Hwf Hsr Hsp Har Hnd Hba.
  - rewrite reduce_0. exact Hba.
  - destruct work as [|a [|b l]].
    + intros e He; destruct He.
    + simpl. exact Hba.
    + rewrite reduce_unfold_2. eapply IH; eauto.
      * apply step_preserves_real; assumption.
      * unfold step; apply NoDup_nodup.
      * eapply step_preserves_below_all; eauto. simpl; lia.
Qed.

(* LOWEST-ness: every common ancestor is at or below the LCUA survivor. Derived
   from the fold via `below_all`; premises are the faithful main-parent-tree
   bridge `single_parent_spine` + distinct inputs `NoDup ms` (no `spine_linear`,
   no maximality premise). *)
Theorem lca_is_lowest :
  forall d root ms,
    wf_dag d -> wf_lookup d -> single_root d root -> single_parent_spine d ->
    all_real d ms -> NoDup ms -> ms <> [] ->
    forall c, common_ancestor d ms c -> anc_of d c (lcua_many d ms).
Proof.
  intros d root ms Hwf Hwl Hsr Hsp Har Hnd Hne c Hca.
  assert (Hba : below_all d c ms) by (intros m Hm; apply Hca; exact Hm).
  destruct ms as [|a [|b l]]; [ contradiction | | ].
  - (* singleton: lcua_many = a *)
    assert (Hr : reduce d (lcua_fuel d) [a] = [a]) by (destruct (lcua_fuel d); reflexivity).
    unfold lcua_many. rewrite Hr. simpl. unfold common_ancestor in Hca.
    apply Hca. left; reflexivity.
  - destruct (reduce_converges d (a::b::l) Hwf Har ltac:(discriminate)) as [g Hg].
    assert (Hbg : below_all d c (reduce d (lcua_fuel d) (a::b::l)))
      by (eapply reduce_preserves_below_all; eauto).
    rewrite Hg in Hbg.
    assert (Hlm : lcua_many d (a::b::l) = g) by (unfold lcua_many; rewrite Hg; reflexivity).
    rewrite Hlm. apply Hbg. left; reflexivity.
Qed.

(* Maximality DERIVED (was the old `lca_is_lowest` premise): the survivor is the
   numof-greatest common ancestor. *)
Theorem lcua_many_is_max :
  forall d root ms,
    wf_dag d -> wf_lookup d -> single_root d root -> single_parent_spine d ->
    all_real d ms -> NoDup ms -> ms <> [] ->
    forall c', common_ancestor d ms c' -> numof d c' <= numof d (lcua_many d ms).
Proof.
  intros d root ms Hwf Hwl Hsr Hsp Har Hnd Hne c' Hca.
  apply anc_of_num_le; [ exact Hwf | eapply lca_is_lowest; eauto ].
Qed.

(* A latest message strictly BELOW a block never lies in that block's past, so
   it contributes nothing to the block's fork-choice score (Score.contrib is 0).
   Stated at the Foundation ancestry level: num(h) < num(b) => b is not an
   ancestor of h. *)
Theorem lca_below_has_zero_rank_influence :
  forall d b h, wf_dag d -> numof d h < numof d b -> ~ anc_of d b h.
Proof.
  intros d b h Hwf Hlt Hanc.
  pose proof (anc_of_num_le Hwf Hanc) as Hle.
  lia.
Qed.
