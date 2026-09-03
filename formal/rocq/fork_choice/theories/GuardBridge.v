(* ===========================================================================
   GuardBridge.v - the seams the Rust ENFORCES, DERIVED (not axiomatized) as the
   premises the Rocq development assumes. The "Rocq assumes what Rust enforces"
   pattern (as in finalized_floor/GuardBridge.v): each seam is a Section
   Variable/Hypothesis reflecting a Rust-checked invariant; on Section close it
   becomes a PREMISE of the closed term, never an axiom.

   Five seams:

   (1) validation_implies_wf_dag  - `validate.rs::block_number` (:698-703)
       enforces `num child = 1 + max(parent numbers)` along every edge, and looks
       up every parent (:684). From that validation predicate we DERIVE
       Foundation.wf_dag (parents present & strictly older; main parent = head).
       [NOTE: we model the FAITHFUL `1 + max parent num` (validate.rs:698-703),
       which is stronger than the task's shorthand `1 + main-parent num` and is
       exactly what makes EVERY parent - not just the main one - strictly older,
       so wf_dag follows.]

   (5) validation_implies_single_root - the approved-genesis pin. `single_root`
       (Lca.v: the ONLY parentless block in the DAG is the genesis) was formerly
       an ASSUMED premise of the Lca common-ancestor results and the
       `fork_choice_ghost_correct` capstone. It is now DERIVED from validation:
       we STRENGTHEN `validated_block`'s parentless (main-parent = None) branch to
       additionally require `blk_hash b = genesis_hash`, faithfully modeling the
       guard that ACTUALLY exists - `validate.rs::justification_follows`
       (:1135-1139), which runs BEFORE `Validate.parents` and rejects EVERY
       empty-parents block as `InvalidParents`. The unique genesis is admitted
       out-of-band via the signature-authenticated approved-block path
       (`initializing.rs:832-840`, bypassing the validation pipeline), so the
       ONLY parentless block that ever enters a validated DAG is the approved
       genesis. `genesis_hash` is a Section Variable (a PARAMETER of the closed
       terms), never an axiom. [NOTE: `block_number` (:698-721) only forces
       num==0 for empty parents; it does NOT pin the genesis hash - see the
       docs' recommended Rust hardening. The FV models the rejection that DOES
       exist (`justification_follows`), which is what makes single_root hold.]

   (2) weight_floor_structural   - fork choice reads only the certified
       finalized-floor authority map, so candidate and traversed-block weights
       cannot reweight a round.

   (3) consensus_parent_pipeline_preserves_ghost_head - the proposer's main
       parent is the LMD-GHOST argmax selected from the certified vote
       projection. Parent ordering is canonical under (is_main DESC, hash ASC),
       so input enumeration order cannot alter either the selected head or the
       complete ordered list. The former deploy-support post-sort promotion is
       retained below only as an executable unsafe control: it is deterministic,
       but it can replace the GHOST head and therefore does not model production.

   (4) honest_forkchoice_parents_validate - the REFRAMED T-VALID: a validator
       does NOT recompute fork choice; it RANGE-CHECKS the declared parents
       (snapshot.rs:315: "validators replay declared parents, not fork-choice").
       A proposer's depth-filtered parents (all within `mpd`) therefore
       ALWAYS pass the validator's buffered bound (`mpd + buf`), since mpd <=
       mpd + buf. No honest proposer is ever rejected, and no fork-choice
       recomputation is needed for validation.

   ---------------------------------------------------------------------------
   Rocq                             | Rust
   ---------------------------------+------------------------------------------
   validated_block / ...wf_dag      | validate.rs:681-723 block_number
   validation_implies_single_root   | validate.rs:1135-1139 justification_follows
                                    |   (+ initializing.rs:832-840 approved genesis)
   weight_floor_structural          | estimator.rs frozen context authority
   ghost_sort                       | snapshot.rs:325-331 sort_by (is_main,hash)
   ghost_sort_first_deterministic   | snapshot.rs order_parents_by_ghost_head
   consensus_parent_pipeline       | snapshot.rs order_parents_by_ghost_head
   consensus_parent_pipeline_preserves_ghost_head
                                    | snapshot.rs GHOST-head preservation guard
   pipeline_head_may_differ_from_ghost
                                    | unsafe historical promotion control
   honest_forkchoice_parents_validate| snapshot.rs:315 (no recompute)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

From ForkChoice Require Import Foundation.
From ForkChoice Require Import Score.
From ForkChoice Require Import TieBreak.
From ForkChoice Require Import Filter.
From ForkChoice Require Import Rank.
From ForkChoice Require Import Bound.
From ForkChoice Require Import Lca.

(* ===========================================================================
   Seam (1) - validation ENFORCES the well-formed DAG
   =========================================================================== *)

(* Max number over a parent list (validate.rs:698 fold(-1, max) then +1). *)
Definition maxpn (d : DAG) (parents : list BlockHash) : nat :=
  fold_right (fun ph acc => Nat.max (numof d ph) acc) 0 parents.

Lemma in_le_fold_max :
  forall d parents ph, In ph parents -> numof d ph <= maxpn d parents.
Proof.
  intros d parents ph Hin. unfold maxpn.
  induction parents as [| q qs IH]; simpl in *.
  - contradiction.
  - destruct Hin as [Heq | Hin].
    + subst q. apply Nat.le_max_l.
    + eapply Nat.le_trans; [apply IH; exact Hin | apply Nat.le_max_r].
Qed.

Lemma hd_error_in :
  forall (A : Type) (l : list A) (x : A), hd_error l = Some x -> In x l.
Proof.
  intros A l x H. destruct l as [| y ys]; simpl in H.
  - discriminate.
  - injection H as ->. left; reflexivity.
Qed.

(* The validation predicate mirroring validate.rs. `genesis_hash` is a Section
   Variable = a PARAMETER of the discharged terms (never an axiom). A block with
   NO main parent (None) is doubly constrained by the running node:
     * validate.rs::block_number (:698-703) forces `num = 1 + max parent num`, so
       an empty-parent block has number 0; AND
     * validate.rs::justification_follows (:1135-1139) - which runs BEFORE
       Validate.parents - rejects EVERY empty-parents block as InvalidParents.
   The ONLY parentless block that ever enters a validated DAG is therefore the
   approved genesis, admitted out-of-band via the signature-authenticated
   approved-block path (initializing.rs:832-840, bypassing the pipeline). We
   model that pin faithfully: a validated parentless block's hash IS the genesis
   hash. This is what makes single_root DERIVABLE (below) rather than assumed. *)
Section ValidatedBlock.
  Variable genesis_hash : BlockHash.

  Definition validated_block (d : DAG) (b : Block) : Prop :=
    (forall ph, In ph (blk_parents b) -> exists p, lookup d ph = Some p)
    /\ match blk_main_parent b with
       | None    => blk_parents b = [] /\ blk_num b = 0 /\ blk_hash b = genesis_hash
       | Some mph => hd_error (blk_parents b) = Some mph
                     /\ blk_num b = S (maxpn d (blk_parents b))
       end.
End ValidatedBlock.

Section Bridge.
  Variable genesis_hash : BlockHash.
  Variable d : DAG.
  Hypothesis all_validated : forall b, In b d -> validated_block genesis_hash d b.

  (* DERIVED: every validated DAG is well-formed (parents present & strictly
     older; main parent = head). No axiom - `all_validated` is a premise. The
     genesis-pin conjunct on the None branch is UNUSED here (wf_dag needs only
     `blk_num b = 0`); it is what `validation_implies_single_root` consumes. *)
  Theorem validation_implies_wf_dag : wf_dag d.
  Proof.
    intros b Hb. destruct (all_validated b Hb) as [Hpres Hmain].
    split.
    - (* every parent: present with strictly smaller number *)
      intros ph Hph. destruct (Hpres ph Hph) as [p Hlp]. exists p. split; [exact Hlp |].
      assert (Hnp : numof d ph = blk_num p) by (unfold numof; rewrite Hlp; reflexivity).
      assert (Hbound : numof d ph <= maxpn d (blk_parents b)) by (apply in_le_fold_max; exact Hph).
      destruct (blk_main_parent b) as [mph |] eqn:Emp.
      + destruct Hmain as [_ Hnum]. lia.
      + destruct Hmain as [Hnil _]. rewrite Hnil in Hph. destruct Hph.
    - (* main-parent clause *)
      destruct (blk_main_parent b) as [mph |] eqn:Emp.
      + destruct Hmain as [Hhd Hnum]. split; [| exact Hhd].
        assert (Hmphin : In mph (blk_parents b)) by (apply (hd_error_in _ (blk_parents b) mph); exact Hhd).
        destruct (Hpres mph Hmphin) as [p Hlp]. exists p. split; [exact Hlp |].
        assert (Hnp : numof d mph = blk_num p) by (unfold numof; rewrite Hlp; reflexivity).
        assert (Hbound : numof d mph <= maxpn d (blk_parents b)) by (apply in_le_fold_max; exact Hmphin).
        lia.
      + destruct Hmain as [_ [Hnum _]]. exact Hnum.
  Qed.

  (* DERIVED: the approved-genesis pin makes the validated DAG single-rooted.
     A parentless block b has main_parent = None (a Some main parent would
     require `hd_error (blk_parents b) = Some mph`, impossible for []), so the
     strengthened None branch gives `blk_hash b = genesis_hash`; with
     lookup_hash (blk_hash b = h), h = genesis_hash. Faithfully models
     validate.rs::justification_follows (:1135-1139) rejecting every OTHER
     parentless block as InvalidParents. No axiom - `all_validated` is a premise
     and `genesis_hash` a Section parameter. This is the seam threaded into the
     Lca common-ancestor results / the ghost capstone in place of an assumed
     `single_root`. *)
  Theorem validation_implies_single_root : single_root d genesis_hash.
  Proof.
    unfold single_root. intros h b Hlk Hpar.
    assert (Hin : In b d) by (eapply lookup_In; eauto).
    destruct (all_validated b Hin) as [_ Hmain].
    destruct (blk_main_parent b) as [mph |] eqn:Emp.
    - (* Some mph: hd_error (blk_parents b) = Some mph contradicts parents = [] *)
      destruct Hmain as [Hhd _]. rewrite Hpar in Hhd. simpl in Hhd. discriminate.
    - (* None: the genesis pin gives blk_hash b = genesis_hash = h *)
      destruct Hmain as [_ [_ Hgen]]. apply lookup_hash in Hlk. rewrite <- Hlk. exact Hgen.
  Qed.
End Bridge.

(* ===========================================================================
   Capstone-facing form: the LCA common-ancestor property with `single_root`
   DERIVED from block validation (via validation_implies_single_root) rather
   than ASSUMED. This is what MainTheorem.fork_choice_ghost_correct clause (d)
   is discharged by, so the capstone no longer takes `single_root` as a bare
   hypothesis. `root := genesis_hash` is instantiated from the pin.
   =========================================================================== *)
Theorem lca_is_common_ancestor_validated :
  forall genesis genesis_hash d top lms,
    wf_dag d -> wf_lookup d ->
    (forall b, In b d -> validated_block genesis_hash d b) ->
    all_real d (map snd (certified_messages d top lms)) ->
    forall lm, In lm (certified_messages d top lms) ->
      anc_of d (lca genesis (lcua_many d (map snd (certified_messages d top lms))) d top lms) (snd lm).
Proof.
  intros genesis genesis_hash d top lms Hwf Hwl Hval Har lm Hin.
  eapply lca_is_common_ancestor with (root := genesis_hash).
  - exact Hwf.
  - exact Hwl.
  - exact (validation_implies_single_root genesis_hash d Hval).
  - exact Har.
  - exact Hin.
Qed.

(* ===========================================================================
   Seam (2) - weight is frozen by the certified finalized-floor authority
   =========================================================================== *)

Corollary weight_floor_structural :
  forall authority1 authority2 v,
    authority1 = authority2 ->
    Score.weight authority1 v = Score.weight authority2 v.
Proof. exact Score.weight_is_pure. Qed.

Corollary candidate_bonds_cannot_reweight_round :
  forall authority candidate_bonds1 candidate_bonds2 v,
    Score.estimator_weight authority candidate_bonds1 v =
    Score.estimator_weight authority candidate_bonds2 v.
Proof. exact Score.candidate_bonds_noninterference. Qed.

(* ===========================================================================
   Seam (3) - the production parent order is deterministic and preserves the
   certified LMD-GHOST head. The removed deploy-support override is retained as
   an executable negative control before the production pipeline definition.
   =========================================================================== *)

(* ---------------------------------------------------------------------------
   Stage 1 - the (is_main DESC, hash ASC) sort (snapshot.rs:325-331)
   --------------------------------------------------------------------------- *)

(* A parent as a tie-break entry: score = 1 if it is the ghost main parent, else
   0; hash = the block hash. (is_main DESC, hash ASC) = TieBreak's (score,hash).
   `b_main.cmp(&a_main)` (:328-329) is DESCENDING on the is-main flag, and
   `a.block_hash.cmp(&b.block_hash)` (:330) is ASCENDING on the hash. *)
Definition parent_entry (main : BlockHash) (h : BlockHash) : entry :=
  ((if Nat.eqb h main then 1 else 0), h).

Lemma map_ehash_parent_entry :
  forall main l, map ehash (map (parent_entry main) l) = l.
Proof.
  intros main l. induction l as [| x xs IH]; simpl.
  - reflexivity.
  - f_equal. exact IH.
Qed.

(* Stage 1 as a list -> list function: sort the parents, read the hashes back. *)
Definition ghost_sort (main : BlockHash) (l : list BlockHash) : list BlockHash :=
  map ehash (sort (map (parent_entry main) l)).

(* The sorted parent list is canonical: permuted inputs sort to the identical
   list because (is_main DESC, hash ASC) is a total order on distinct hashes.
   Production uses this directly and proves below that the GHOST head remains
   first. The removed deploy-support override is modeled afterward only as a
   negative control. *)
Theorem ghost_sort_first_deterministic :
  forall main (l l' : list BlockHash),
    NoDup l -> Permutation l l' ->
    sort (map (parent_entry main) l) = sort (map (parent_entry main) l').
Proof.
  intros main l l' Hnd Hperm.
  apply output_indep_of_input_perm.
  - rewrite (map_ehash_parent_entry main l). exact Hnd.
  - apply Permutation_map. exact Hperm.
Qed.

Lemma ghost_sort_deterministic :
  forall main l l', NoDup l -> Permutation l l' -> ghost_sort main l = ghost_sort main l'.
Proof.
  intros main l l' Hnd Hperm. unfold ghost_sort. f_equal.
  apply ghost_sort_first_deterministic; assumption.
Qed.

Lemma ghost_sort_permutation :
  forall main l, Permutation (ghost_sort main l) l.
Proof.
  intros main l. unfold ghost_sort.
  eapply Permutation_trans with (l' := map ehash (map (parent_entry main) l)).
  - apply Permutation_map. apply sort_is_permutation.
  - rewrite map_ehash_parent_entry. apply Permutation_refl.
Qed.

(* ---------------------------------------------------------------------------
   Historical unsafe control - removed deploy-support promotion
   --------------------------------------------------------------------------- *)

(* `DeployBranchScore` (snapshot.rs:41-46). The Rust fields are
     deploy_sig_count           : usize
     latest_deploy_block_number : i64
     root_block_number          : i64
   and all three are modeled as `nat`. That is FAITHFUL for the order-theoretic
   content proved below - the Rust comparison is `Ord`'s lexicographic order, and
   `nat`'s order agrees with `usize`'s / `i64`'s on the non-negative range - and
   all three fields ARE non-negative: `deploy_sig_count` is a `HashSet::len()`
   (:118), and both block-number fields are a real block's
   `BlockMetadata::block_number` (:103-106, :120), which Foundation.v already
   models repo-wide as `blk_num : nat` (Foundation.v:50). (The one genuinely
   signed value in that function, `last_finalized_number`'s `unwrap_or(-1)` at
   :81, is a traversal CUTOFF (:97), not a score field, so it never enters the
   order.) *)
Record dscore : Type := mkDScore {
  d_sigs   : nat;   (* deploy_sig_count            *)
  d_latest : nat;   (* latest_deploy_block_number  *)
  d_root   : nat    (* root_block_number           *)
}.

(* `better_deploy_branch_score` (snapshot.rs:48-70) as a PROPOSITION:
   `dbetter (sa,ha) (sb,hb)` reads "candidate (sa,ha) BEATS current (sb,hb)".
   The Rust chains `candidate.cmp(current)` on deploy_sig_count (:52-55), then
   latest_deploy_block_number (:56-61), then root_block_number (:62-67) - each
   DESCENDING, bigger wins - then `current.1.cmp(candidate.1)` (:68), which is
   REVERSED, so the SMALLER hash wins - and takes `.is_gt()` (:69). *)
Definition dbetter (a b : dscore * BlockHash) : Prop :=
  let (sa, ha) := a in
  let (sb, hb) := b in
  d_sigs sb < d_sigs sa
  \/ (d_sigs sb = d_sigs sa /\ d_latest sb < d_latest sa)
  \/ (d_sigs sb = d_sigs sa /\ d_latest sb = d_latest sa /\ d_root sb < d_root sa)
  \/ (d_sigs sb = d_sigs sa /\ d_latest sb = d_latest sa /\ d_root sb = d_root sa
      /\ ha < hb).

Definition dbetterb (a b : dscore * BlockHash) : bool :=
  let (sa, ha) := a in
  let (sb, hb) := b in
  (d_sigs sb <? d_sigs sa)
  || ((d_sigs sb =? d_sigs sa) && (d_latest sb <? d_latest sa))
  || ((d_sigs sb =? d_sigs sa) && (d_latest sb =? d_latest sa) && (d_root sb <? d_root sa))
  || ((d_sigs sb =? d_sigs sa) && (d_latest sb =? d_latest sa) && (d_root sb =? d_root sa)
      && (ha <? hb)).

Lemma dbetterb_true_iff : forall a b, dbetterb a b = true <-> dbetter a b.
Proof.
  intros [sa ha] [sb hb]. unfold dbetterb, dbetter.
  rewrite !orb_true_iff, !andb_true_iff, !Nat.ltb_lt, !Nat.eqb_eq.
  split; intro H; tauto.
Qed.

Lemma dbetterb_false_iff : forall a b, dbetterb a b = false <-> ~ dbetter a b.
Proof.
  intros a b. split; intro H.
  - intro Hb. apply (dbetterb_true_iff a b) in Hb. rewrite Hb in H. discriminate.
  - destruct (dbetterb a b) eqn:E; [| reflexivity].
    exfalso. apply H. apply (dbetterb_true_iff a b). exact E.
Qed.

(* Irreflexivity, asymmetry and transitivity hold UNCONDITIONALLY: the order is a
   strict lexicographic order on a tuple of linearly-ordered keys. Only TOTALITY
   needs distinct hashes - which distinct blocks always have (a block hash is a
   cryptographic digest), exactly as in TieBreak.ord_total. *)
Lemma dbetter_irrefl : forall a, ~ dbetter a a.
Proof.
  intros [s h]. unfold dbetter.
  intros [H | [[_ H] | [[_ [_ H]] | [_ [_ [_ H]]]]]]; lia.
Qed.

Lemma dbetter_trans : forall a b c, dbetter a b -> dbetter b c -> dbetter a c.
Proof.
  intros [sa ha] [sb hb] [sk hk]. unfold dbetter.
  intros [H | [[H1 H] | [[H1 [H2 H]] | [H1 [H2 [H3 H]]]]]]
         [G | [[G1 G] | [[G1 [G2 G]] | [G1 [G2 [G3 G]]]]]]; lia.
Qed.

Lemma dbetter_asym : forall a b, dbetter a b -> ~ dbetter b a.
Proof.
  intros a b Hab Hba. apply (dbetter_irrefl a). eapply dbetter_trans; eauto.
Qed.

Lemma dbetter_total : forall a b, snd a <> snd b -> dbetter a b \/ dbetter b a.
Proof.
  intros [sa ha] [sb hb] Hne. simpl in Hne. unfold dbetter. lia.
Qed.

(* (b): `better_deploy_branch_score` IS a strict total order (on distinct hashes).
   This is what makes the promoted branch a UNIQUE argmax (dmax_unique below), and
   hence the promotion scan-order independent. Had it NOT been a total order, the
   promotion would have been a genuine consensus non-determinism bug. *)
Theorem dbetter_strict_total_order :
  (forall a, ~ dbetter a a)
  /\ (forall a b, dbetter a b -> ~ dbetter b a)
  /\ (forall a b c, dbetter a b -> dbetter b c -> dbetter a c)
  /\ (forall a b, snd a <> snd b -> dbetter a b \/ dbetter b a).
Proof.
  split; [exact dbetter_irrefl |].
  split; [exact dbetter_asym |].
  split; [exact dbetter_trans | exact dbetter_total].
Qed.

Section DeployPromotion.
  (* `branch_unfinalized_user_deploy_score(dag, block_store, ., last_finalized_block)`
     (snapshot.rs:72-122) at a FIXED (dag, block_store, last_finalized_block): a
     deterministic read-only DFS over an immutable DAG snapshot whose result depends
     only on the root hash. (Its `HashSet`s are consumed only by `len()` (:118) and
     `max` (:104-105) - both iteration-order insensitive - so the `HashSet`
     non-determinism cannot leak into the result.) It returns `None` EXACTLY when no
     unfinalized block in the branch carries a user deploy (:117-121).

     Modeling it as a Rocq FUNCTION of the hash is precisely what `the main parent is
     a pure function of (dag, parents, last_finalized_block)` MEANS. `branch_score` is
     a Section Variable - a PARAMETER of the discharged terms - never an axiom, the
     same discipline as `genesis_hash` above. *)
  Variable branch_score : BlockHash -> option dscore.

  (* The best-index scan (snapshot.rs:144-161). Left-to-right over the parents:
     `None`-scored parents are SKIPPED (:146-148, `let Some(score) = .. else
     { continue }`); the FIRST scored parent seeds `best` (:157, `.unwrap_or(true)`);
     a later parent replaces `best` only when STRICTLY better (:149-160). The Rust
     carries `(usize, &DeployBranchScore)` and re-reads `parents[best_idx].block_hash`
     (:154); carrying the hash alongside is equivalent, as the hash at a fixed index
     is fixed. *)
  Fixpoint dbest_from (i : nat) (best : option (nat * BlockHash * dscore))
                      (l : list BlockHash) : option (nat * BlockHash * dscore) :=
    match l with
    | [] => best
    | h :: rest =>
        let best' :=
          match branch_score h with
          | None => best
          | Some s =>
              match best with
              | None => Some (i, h, s)
              | Some (_, bh, bs) => if dbetterb (s, h) (bs, bh) then Some (i, h, s) else best
              end
          end in
        dbest_from (S i) best' rest
    end.

  Definition dbest (l : list BlockHash) : option (nat * BlockHash * dscore) :=
    dbest_from 0 None l.

  (* `Vec::remove(best_idx)` then `insert(0, _)` (snapshot.rs:173-174). `remove i`
     extracts `l[i]` and shifts the tail left, i.e. leaves `firstn i l ++ skipn (S i) l`;
     `insert 0 x` then conses `x` on the front. *)
  Definition promote (i : nat) (l : list BlockHash) : list BlockHash :=
    match nth_error l i with
    | None => l
    | Some x => x :: (firstn i l ++ skipn (S i) l)
    end.

  (* `prefer_deploy_support_main_parent` (snapshot.rs:124-185). The two early
     returns are faithful, though both are OPTIMIZATIONS rather than semantics:
     `promote` is already the identity at length <= 1 and at index 0. *)
  Definition prefer_deploy_support (l : list BlockHash) : list BlockHash :=
    if Nat.leb (length l) 1 then l              (* :130-132  parents.len() <= 1  *)
    else match dbest l with
         | None => l                            (* :163-165  best == None        *)
         | Some (i, _, _) =>
             if Nat.eqb i 0 then l              (* :166-168  best_idx == 0       *)
             else promote i l                   (* :170-184  remove + insert(0)  *)
         end.

  (* --- (a) stage 2 is a PERMUTATION: no parent lost, none duplicated ---------- *)

  Lemma promote_permutation : forall i l, Permutation (promote i l) l.
  Proof.
    intros i l. unfold promote. destruct (nth_error l i) as [x |] eqn:E;
      [| apply Permutation_refl].
    apply nth_error_split in E. destruct E as [l1 [l2 [Hl Hlen]]]. subst l i.
    rewrite firstn_app, Nat.sub_diag, firstn_all, firstn_O, app_nil_r.
    rewrite skipn_app, skipn_all2 by lia.
    replace (S (length l1) - length l1) with 1 by lia.
    cbn [skipn app].
    apply Permutation_middle.
  Qed.

  Theorem prefer_deploy_support_permutation :
    forall l, Permutation (prefer_deploy_support l) l.
  Proof.
    intros l. unfold prefer_deploy_support.
    destruct (Nat.leb (length l) 1); [apply Permutation_refl |].
    destruct (dbest l) as [[[i h] s] |]; [| apply Permutation_refl].
    destruct (Nat.eqb i 0); [apply Permutation_refl | apply promote_permutation].
  Qed.

  (* --- (c) the promoted branch is the UNIQUE argmax --------------------------- *)

  (* "h, scored s, is THE maximum scored parent of l". *)
  Definition dmax_of (l : list BlockHash) (h : BlockHash) (s : dscore) : Prop :=
    In h l /\ branch_score h = Some s
    /\ forall h' s', In h' l -> branch_score h' = Some s' ->
         h' = h \/ dbetter (s, h) (s', h').

  (* Uniqueness of the maximum under a strict total order. NOTE no `NoDup` premise
     is needed: the list elements ARE the hashes, so hash-equality IS element
     equality, and `dbetter_total` applies to any two DISTINCT elements. *)
  Lemma dmax_unique :
    forall l h1 s1 h2 s2, dmax_of l h1 s1 -> dmax_of l h2 s2 -> h1 = h2 /\ s1 = s2.
  Proof.
    intros l h1 s1 h2 s2 [Hin1 [Hsc1 Hmax1]] [Hin2 [Hsc2 Hmax2]].
    assert (Hh : h1 = h2).
    { destruct (Nat.eq_dec h1 h2) as [E | Hne]; [exact E |]. exfalso.
      destruct (Hmax1 h2 s2 Hin2 Hsc2) as [E | Hb1]; [congruence |].
      destruct (Hmax2 h1 s1 Hin1 Hsc1) as [E | Hb2]; [congruence |].
      exact (dbetter_asym _ _ Hb1 Hb2). }
    split; [exact Hh | congruence].
  Qed.

  (* The hash the running accumulator holds (its "domain"). *)
  Definition bdom (best : option (nat * BlockHash * dscore)) : list BlockHash :=
    match best with None => [] | Some (_, bh, _) => [bh] end.

  (* The scan invariant: `dbest_from` returns the MAXIMUM scored element over
     everything it has seen (the incoming accumulator's hash, plus `l`). Proved by
     induction on `l`, generalized over the index and the accumulator. *)
  Lemma dbest_from_correct :
    forall l i best,
      (forall bi bh bs, best = Some (bi, bh, bs) -> branch_score bh = Some bs) ->
      match dbest_from i best l with
      | None => forall h, In h (bdom best ++ l) -> branch_score h = None
      | Some (_, rh, rs) => dmax_of (bdom best ++ l) rh rs
      end.
  Proof.
    (* NOTE on the reduction discipline: `cbn [dbest_from]` unfolds ONE step of the
       fixpoint while leaving `dbetterb` FOLDED, so the `destruct (dbetterb ..)`
       below actually matches the `if` in the goal. A bare `simpl` here delta-expands
       `dbetterb` into its raw boolean tree and the destructs then bind nothing. *)
    induction l as [| h rest IH]; intros i best Hwf.
    - (* [] : the accumulator is returned unchanged (:144 `best` survives) *)
      cbn [dbest_from]. rewrite app_nil_r.
      destruct best as [[[bi bh] bs] |].
      + assert (Hbh : branch_score bh = Some bs) by (apply (Hwf bi bh bs); reflexivity).
        cbn [bdom]. split; [left; reflexivity |]. split; [exact Hbh |].
        intros h' s' [Hh' | []] _. left. symmetry. exact Hh'.
      + cbn [bdom]. intros h [].
    - (* h :: rest *)
      cbn [dbest_from]. destruct (branch_score h) as [s |] eqn:Es; cbn beta iota zeta.
      + (* h IS scored *)
        destruct best as [[[bi bh] bs] |]; cbn beta iota zeta.
        * assert (Hbh : branch_score bh = Some bs) by (apply (Hwf bi bh bs); reflexivity).
          destruct (dbetterb (s, h) (bs, bh)) eqn:Ecmp; cbn beta iota zeta.
          -- (* h BEATS the incumbent: the accumulator becomes (i,h,s) (:158-159) *)
             apply (dbetterb_true_iff (s,h) (bs,bh)) in Ecmp.
             assert (Hwf' : forall bi' bh' bs',
                       Some (i, h, s) = Some (bi', bh', bs') -> branch_score bh' = Some bs')
               by (intros bi' bh' bs' Hc; congruence).
             specialize (IH (S i) (Some (i, h, s)) Hwf'). cbn [bdom] in IH.
             destruct (dbest_from (S i) (Some (i, h, s)) rest) as [[[ri rh] rs] |].
             ++ destruct IH as [Hin [Hsc Hmax]].
                cbn [bdom app] in *. split; [right; exact Hin |]. split; [exact Hsc |].
                intros h' s' [Hbh' | Hh'] Hs'.
                ** (* h' = bh : chain rs >= s > bs *)
                   subst h'. right.
                   assert (Hs'bs : s' = bs) by congruence. subst s'.
                   destruct (Hmax h s (or_introl eq_refl) Es) as [Hhr | Hbet].
                   --- (* the running max IS h : rs = s, so rs beats bs directly *)
                       assert (Hrs : rs = s) by congruence. subst rs rh. exact Ecmp.
                   --- exact (dbetter_trans _ _ _ Hbet Ecmp).
                ** apply (Hmax h' s'); assumption.
             ++ (* None impossible: the accumulator already holds the scored h *)
                exfalso. specialize (IH h (or_introl eq_refl)). congruence.
          -- (* the INCUMBENT survives (:160 no replace) *)
             apply (dbetterb_false_iff (s,h) (bs,bh)) in Ecmp.
             assert (Hwf' : forall bi' bh' bs',
                       Some (bi, bh, bs) = Some (bi', bh', bs') -> branch_score bh' = Some bs')
               by (intros bi' bh' bs' Hc; congruence).
             specialize (IH (S i) (Some (bi, bh, bs)) Hwf'). cbn [bdom] in IH.
             destruct (dbest_from (S i) (Some (bi, bh, bs)) rest) as [[[ri rh] rs] |].
             ++ destruct IH as [Hin [Hsc Hmax]].
                cbn [bdom app] in *.
                split; [destruct Hin as [Hin | Hin];
                        [left; exact Hin | right; right; exact Hin] |].
                split; [exact Hsc |].
                intros h' s' [Hbh' | Hh'] Hs'.
                ** (* h' = bh : the IH's accumulator already covers it *)
                   subst h'. apply (Hmax bh s' (or_introl eq_refl) Hs').
                ** destruct Hh' as [Hh' | Hh'].
                   --- (* h' = h : h did NOT beat bh, and rs >= bs, so rs >= s *)
                       subst h'. assert (Hs's : s' = s) by congruence. subst s'.
                       destruct (Nat.eq_dec h rh) as [Hhr | Hhr]; [left; exact Hhr |].
                       right.
                       (* bs beats s, unless bh IS h (then bs = s) *)
                       assert (Hbs : dbetter (bs, bh) (s, h) \/ (bh = h /\ bs = s)).
                       { destruct (Nat.eq_dec bh h) as [Ebh | Nbh].
                         - right. split; [exact Ebh |]. congruence.
                         - left.
                           assert (Hne : snd (s, h) <> snd (bs, bh))
                             by (simpl; intro Heq; apply Nbh; symmetry; exact Heq).
                           destruct (dbetter_total (s, h) (bs, bh) Hne) as [Hx | Hx];
                             [contradiction | exact Hx]. }
                       destruct (Hmax bh bs (or_introl eq_refl) Hbh) as [Hbr | Hbet].
                       +++ (* bh = rh : so bs = rs *)
                           assert (Hrs : bs = rs) by congruence. subst rs.
                           destruct Hbs as [Hx | [Hx _]].
                           *** subst rh. exact Hx.
                           *** exfalso. apply Hhr. congruence.
                       +++ destruct Hbs as [Hx | [Hx Hy]].
                           *** exact (dbetter_trans _ _ _ Hbet Hx).
                           *** subst bh bs. exact Hbet.
                   --- apply (Hmax h' s'); [right; exact Hh' | exact Hs'].
             ++ exfalso. specialize (IH bh (or_introl eq_refl)). congruence.
        * (* NO incumbent: h SEEDS the accumulator (:157 `.unwrap_or(true)`).
             `bdom None ++ h :: rest` and `bdom (Some (i,h,s)) ++ rest` are the
             SAME list, so the IH applies verbatim. *)
          assert (Hwf' : forall bi' bh' bs',
                    Some (i, h, s) = Some (bi', bh', bs') -> branch_score bh' = Some bs')
            by (intros bi' bh' bs' Hc; congruence).
          specialize (IH (S i) (Some (i, h, s)) Hwf').
          cbn [bdom app] in IH |- *. exact IH.
      + (* h is NOT scored: SKIPPED (:146-148); it can never be the max *)
        specialize (IH (S i) best Hwf).
        destruct (dbest_from (S i) best rest) as [[[ri rh] rs] |].
        * destruct IH as [Hin [Hsc Hmax]]. split.
          -- apply in_app_iff. apply in_app_iff in Hin. destruct Hin as [Hin | Hin];
               [left; exact Hin | right; right; exact Hin].
          -- split; [exact Hsc |]. intros h' s' Hin' Hs'.
             apply in_app_iff in Hin'. destruct Hin' as [Hin' | [Hh' | Hin']].
             ++ apply (Hmax h' s'); [apply in_app_iff; left; exact Hin' | exact Hs'].
             ++ exfalso. subst h'. congruence.
             ++ apply (Hmax h' s'); [apply in_app_iff; right; exact Hin' | exact Hs'].
        * intros h0 Hin0. apply in_app_iff in Hin0.
          destruct Hin0 as [Hin0 | [Hh0 | Hin0]].
          -- apply IH. apply in_app_iff. left. exact Hin0.
          -- subst h0. exact Es.
          -- apply IH. apply in_app_iff. right. exact Hin0.
  Qed.

  (* `dbest` returns the maximum scored parent, or None when NOTHING scores. *)
  Theorem dbest_is_max :
    forall l, match dbest l with
              | None => forall h, In h l -> branch_score h = None
              | Some (_, rh, rs) => dmax_of l rh rs
              end.
  Proof.
    intros l. unfold dbest.
    pose proof (dbest_from_correct l 0 None (fun _ _ _ Hc => ltac:(discriminate))) as H.
    simpl in H. exact H.
  Qed.

  (* THE stage-2 determinism result: WHICH parent is promoted (its hash + score) is
     the UNIQUE argmax, hence INDEPENDENT of the order the scan visits the parents -
     only the INDEX moves with the input order. Note this needs NO `NoDup`: the
     elements ARE the hashes. *)
  Theorem dbest_hash_perm_invariant :
    forall l l', Permutation l l' ->
      option_map (fun r => (snd (fst r), snd r)) (dbest l)
      = option_map (fun r => (snd (fst r), snd r)) (dbest l').
  Proof.
    intros l l' Hperm.
    pose proof (dbest_is_max l) as Hl. pose proof (dbest_is_max l') as Hl'.
    destruct (dbest l) as [[[i h] s] |] eqn:El;
    destruct (dbest l') as [[[i' h'] s'] |] eqn:El'.
    - assert (Hml' : dmax_of l' h s).
      { destruct Hl as [Hin [Hsc Hmax]]. split; [| split].
        - apply (Permutation_in _ Hperm). exact Hin.
        - exact Hsc.
        - intros h0 s0 Hin0 Hsc0. apply Hmax; [| exact Hsc0].
          apply (Permutation_in _ (Permutation_sym Hperm)). exact Hin0. }
      destruct (dmax_unique l' h s h' s' Hml' Hl') as [-> ->]. reflexivity.
    - exfalso. destruct Hl as [Hin [Hsc _]].
      assert (Hin' : In h l') by (apply (Permutation_in _ Hperm); exact Hin).
      specialize (Hl' h Hin'). congruence.
    - exfalso. destruct Hl' as [Hin [Hsc _]].
      assert (Hin' : In h' l) by (apply (Permutation_in _ (Permutation_sym Hperm)); exact Hin).
      specialize (Hl h' Hin'). congruence.
    - reflexivity.
  Qed.

  (* --- the COMPOSED pipeline (snapshot.rs:317-337) ---------------------------- *)

  (* The proposer's real parent-ordering pipeline: stage 1 THEN stage 2.
     `main` abstracts the ghost head (:317-323, from the estimator over the dag);
     `branch_score` abstracts the branch scorer (:72-122, over dag+block_store+lfb).
     So `main_parent_pipeline` IS a pure function of (dag, parents,
     last_finalized_block), which is the seam's claim. *)
  Definition main_parent_pipeline (main : BlockHash) (l : list BlockHash) : list BlockHash :=
    prefer_deploy_support (ghost_sort main l).

  (* (a) The pipeline PERMUTES the parent list - the parent MULTISET is preserved.
     This is what makes (d) go through: `Selection.T_PS` (finalized_floor/theories/
     Selection.v:196) proves floor soundness for an UNCONSTRAINED parent oracle
     (forall parents), so a merely RE-ORDERED parent list is already inside the
     modeled domain and the promotion cannot break floor soundness. *)
  Theorem main_parent_pipeline_permutation :
    forall main l, Permutation (main_parent_pipeline main l) l.
  Proof.
    intros main l. unfold main_parent_pipeline.
    eapply Permutation_trans;
      [apply prefer_deploy_support_permutation | apply ghost_sort_permutation].
  Qed.

  (* (c) The WHOLE pipeline output - hence the main parent - is invariant under
     permutation of the input parents. Stage 2 alone is NOT (it returns `parents`
     untouched when nothing scores, :163-165); determinism comes from stage 1
     CANONICALIZING the list first. *)
  Theorem main_parent_pipeline_deterministic :
    forall main l l', NoDup l -> Permutation l l' ->
      main_parent_pipeline main l = main_parent_pipeline main l'.
  Proof.
    intros main l l' Hnd Hperm. unfold main_parent_pipeline. f_equal.
    apply ghost_sort_deterministic; assumption.
  Qed.

  Corollary main_parent_deterministic :
    forall main l l', NoDup l -> Permutation l l' ->
      hd_error (main_parent_pipeline main l) = hd_error (main_parent_pipeline main l').
  Proof.
    intros main l l' Hnd Hperm. f_equal.
    apply main_parent_pipeline_deterministic; assumption.
  Qed.

  (* Purity in the "two evaluations agree" sense (cf. Selection.select_deterministic):
     it IS a function of its inputs. Combined with `main_parent_pipeline_deterministic`
     (the input list matters only up to permutation), every node computing this
     pipeline on the same (dag, parent SET, lfb) gets the same main parent. *)
  Corollary main_parent_pure :
    forall main l r1 r2,
      main_parent_pipeline main l = r1 -> main_parent_pipeline main l = r2 -> r1 = r2.
  Proof. intros main l r1 r2 H1 H2. subst. reflexivity. Qed.
End DeployPromotion.

(* Executable negative control for the removed deploy-support override.
   Ghost head = hash 0; parents = {0, 1}; branch 1 carries an unfinalized user deploy
   and branch 0 does not. The unsafe two-stage pipeline promotes branch 1, proving
   that deterministic deploy scoring is insufficient to preserve the GHOST head.
   Production uses consensus_parent_pipeline below instead. *)
Example pipeline_head_may_differ_from_ghost :
  let sc := fun h => if Nat.eqb h 1 then Some (mkDScore 1 1 1) else None in
  ghost_sort 0 [0; 1] = [0; 1]
  /\ main_parent_pipeline sc 0 [0; 1] = [1; 0]
  /\ hd_error (main_parent_pipeline sc 0 [0; 1]) <> Some 0.
Proof. cbn. repeat split; discriminate. Qed.

Definition consensus_parent_pipeline
  (main : BlockHash) (parents : list BlockHash) : list BlockHash :=
  ghost_sort main parents.

Theorem consensus_parent_pipeline_permutation :
  forall main parents,
    Permutation (consensus_parent_pipeline main parents) parents.
Proof. intros; apply ghost_sort_permutation. Qed.

Theorem consensus_parent_pipeline_deterministic :
  forall main left right,
    NoDup left ->
    Permutation left right ->
    consensus_parent_pipeline main left = consensus_parent_pipeline main right.
Proof.
  intros main left right Hnodup Hperm.
  unfold consensus_parent_pipeline.
  apply ghost_sort_deterministic; assumption.
Qed.

Theorem consensus_parent_pipeline_preserves_ghost_head :
  forall main parents,
    NoDup parents ->
    In main parents ->
    hd_error (consensus_parent_pipeline main parents) = Some main.
Proof.
  intros main parents Hnodup Hmain.
  unfold consensus_parent_pipeline, ghost_sort.
  assert (HentryNodup : NoDup (map ehash (map (parent_entry main) parents))).
  { rewrite map_ehash_parent_entry. exact Hnodup. }
  pose proof (sort_argmax_unique (map (parent_entry main) parents) HentryNodup) as Hwinner.
  destruct (sort (map (parent_entry main) parents)) as [|winner rest] eqn:Hsorted.
  - pose proof (sort_is_permutation (map (parent_entry main) parents)) as Hperm.
    rewrite Hsorted in Hperm.
    apply Permutation_nil in Hperm.
    apply map_eq_nil in Hperm. subst. contradiction.
  - simpl. destruct Hwinner as [Hwinner Hmax].
    apply in_map_iff in Hwinner.
    destruct Hwinner as [winner_hash [Hwinner HwinnerIn]].
    subst winner.
    destruct (Nat.eq_dec winner_hash main) as [Heq | Hneq].
    + subst. reflexivity.
    + specialize (Hmax (parent_entry main main)).
      assert (HmainEntry : In (parent_entry main main) (map (parent_entry main) parents)).
      { apply in_map. exact Hmain. }
      specialize (Hmax HmainEntry).
      destruct Hmax as [Hequal | HoutRanks].
      * apply (f_equal ehash) in Hequal. simpl in Hequal.
        exfalso. apply Hneq. symmetry. exact Hequal.
      * unfold ord, escore, ehash, parent_entry in HoutRanks. simpl in HoutRanks.
        rewrite Nat.eqb_refl in HoutRanks.
        apply Nat.eqb_neq in Hneq. rewrite Hneq in HoutRanks. simpl in HoutRanks.
        lia.
Qed.

(* ===========================================================================
   Seam (4) - honest fork-choice parents pass the validator's bound-check
   =========================================================================== *)

(* A parent at height `pn` is within depth `mpd` of the tip `maxn`. *)
Definition within_depth (maxn mpd pn : nat) : bool := Nat.leb (maxn - pn) mpd.
Definition parents_ok (maxn mpd : nat) (nums : list nat) : bool :=
  forallb (within_depth maxn mpd) nums.
Definition prop_filter (maxn mpd : nat) (nums : list nat) : list nat :=
  filter (within_depth maxn mpd) nums.
Definition prop_filter_head (maxn mpd : nat) (nums : list nat) : list nat :=
  match nums with
  | [] => []
  | head :: tail => head :: prop_filter maxn mpd tail
  end.
Definition declared_parents_ok (maxn mpd : nat) (nums : list nat) : bool :=
  match nums with
  | [] => true
  | _ :: tail => parents_ok maxn mpd tail
  end.

Lemma within_depth_mono :
  forall maxn mpd buf pn,
    within_depth maxn mpd pn = true -> within_depth maxn (mpd + buf) pn = true.
Proof.
  intros maxn mpd buf pn H. unfold within_depth in *.
  apply Nat.leb_le in H. apply Nat.leb_le. lia.
Qed.

(* The reframed T-VALID: the proposer's depth-filtered parents (all within `mpd`)
   ALWAYS pass the validator's buffered acceptance (`mpd + buf`). Validators
   range-check declared parents; they never recompute fork choice. *)
Lemma filtered_tail_validates :
  forall maxn mpd buf nums,
    parents_ok maxn (mpd + buf) (prop_filter maxn mpd nums) = true.
Proof.
  intros maxn mpd buf nums. unfold parents_ok. rewrite forallb_forall.
  intros x Hx. unfold prop_filter in Hx. apply filter_In in Hx.
  destruct Hx as [_ Hp]. apply within_depth_mono. exact Hp.
Qed.

Theorem depth_filter_preserves_head :
  forall maxn mpd head tail,
    hd_error (prop_filter_head maxn mpd (head :: tail)) = Some head.
Proof. reflexivity. Qed.

Theorem honest_forkchoice_parents_validate :
  forall maxn mpd buf nums,
    declared_parents_ok maxn (mpd + buf) (prop_filter_head maxn mpd nums) = true.
Proof.
  intros maxn mpd buf nums. destruct nums as [| head tail]; simpl.
  - reflexivity.
  - apply filtered_tail_validates.
Qed.

(* ===========================================================================
   Seam (4b) - honest proposal construction refines receiver admission

   The proposer selects a complete protected frontier. The receiver replays the
   declared parents and checks that they carry the committed finalized floor.
   The receiver does not require equality with its local fork-choice result.
   Justifications remain the frozen authority input and do not become replay
   parents merely because a declared parent subset omits one justified sibling.
   =========================================================================== *)

Section ReceiverParentAdmission.
  Variables ParentHash FloorHash Justification ReplayState Authority : Type.
  Variable descends : FloorHash -> ParentHash -> Prop.
  Variable effective_floor : ParentHash -> FloorHash.
  Variable floor_leq : FloorHash -> FloorHash -> Prop.
  Variable state_preserved : FloorHash -> FloorHash -> Prop.
  Variable replay_from_parents : list ParentHash -> ReplayState.
  Variable authority_from_justifications : list Justification -> Authority.

  Definition floor_comparable (left right : FloorHash) : Prop :=
    floor_leq left right \/ floor_leq right left.

  Definition comparable_parent_floors (parents : list ParentHash) : Prop :=
    forall left right,
      In left parents ->
      In right parents ->
      floor_comparable (effective_floor left) (effective_floor right).

  Definition receiver_parent_frontier_admissible
    (floor : FloorHash) (parents : list ParentHash) : Prop :=
    parents <> [] /\
    Exists (descends floor) parents /\
    Forall
      (fun parent =>
         floor_leq (effective_floor parent) floor /\
         state_preserved (effective_floor parent) floor)
      parents /\
    comparable_parent_floors parents.

  Definition honest_proposer_frontier
    (floor : FloorHash) (protected : ParentHash) (secondary : list ParentHash) : Prop :=
    descends floor protected /\
    Forall
      (fun parent =>
         floor_leq (effective_floor parent) floor /\
         state_preserved (effective_floor parent) floor)
      (protected :: secondary) /\
    comparable_parent_floors (protected :: secondary).

  Theorem honest_proposer_frontier_refines_receiver_admission :
    forall floor protected secondary,
      honest_proposer_frontier floor protected secondary ->
      receiver_parent_frontier_admissible floor (protected :: secondary).
  Proof.
    intros floor protected secondary [Hprotected [Hpreserved Hcomparable]].
    unfold receiver_parent_frontier_admissible.
    split; [discriminate |].
    split.
    - constructor. exact Hprotected.
    - split; assumption.
  Qed.

  Definition receiver_admission_with_justifications
    (floor : FloorHash) (parents : list ParentHash)
    (_ : list Justification) : Prop :=
    receiver_parent_frontier_admissible floor parents.

  Theorem receiver_admission_justification_noninterference :
    forall floor parents first second,
      receiver_admission_with_justifications floor parents first <->
      receiver_admission_with_justifications floor parents second.
  Proof. intros; reflexivity. Qed.

  Definition candidate_semantics
    (parents : list ParentHash) (justifications : list Justification)
    : ReplayState * Authority :=
    (replay_from_parents parents, authority_from_justifications justifications).

  Theorem candidate_semantics_replay_projection :
    forall parents justifications,
      fst (candidate_semantics parents justifications) = replay_from_parents parents.
  Proof. reflexivity. Qed.

  Theorem candidate_semantics_authority_projection :
    forall parents justifications,
      snd (candidate_semantics parents justifications) =
      authority_from_justifications justifications.
  Proof. reflexivity. Qed.
End ReceiverParentAdmission.

Definition nat_descends (floor parent : nat) : Prop := floor <= parent.
Definition nat_effective_floor (_ : nat) : nat := 1.
Definition nat_floor_leq (left right : nat) : Prop := left <= right.
Definition nat_state_preserved (left right : nat) : Prop := left <= right.

Theorem replay_safe_parent_subset_witness :
  @receiver_parent_frontier_admissible
    nat nat
    nat_descends nat_effective_floor nat_floor_leq nat_state_preserved
    1 [3] /\
  ~ Permutation [3] [3; 4] /\
  In 4 [3; 4] /\
  ~ In 4 [3].
Proof.
  split.
  - unfold receiver_parent_frontier_admissible.
    split; [discriminate |].
    split.
    + constructor. unfold nat_descends; lia.
    + split.
      * constructor.
        -- split; unfold nat_effective_floor, nat_floor_leq, nat_state_preserved; lia.
        -- constructor.
      * intros left right Hleft Hright.
        destruct Hleft as [Hleft | Hleft]; [subst left | contradiction].
        destruct Hright as [Hright | Hright]; [subst right | contradiction].
        left. unfold nat_effective_floor, nat_floor_leq. lia.
  - split.
    + intro Hperm. apply Permutation_length in Hperm. simpl in Hperm. lia.
    + split.
      * simpl. auto.
      * simpl. lia.
Qed.

Theorem disconnected_parent_frontier_rejected :
  ~ @receiver_parent_frontier_admissible
      nat nat
      nat_descends nat_effective_floor nat_floor_leq nat_state_preserved
      2 [1].
Proof.
  intros [_ [Hdescends _]].
  inversion Hdescends as [head tail Hhead | head tail Htail]; subst.
  - change (2 <= 1) in Hhead. lia.
  - inversion Htail.
Qed.

(* ===========================================================================
   Pipeline seams: the whole estimator pipeline (filter -> score -> rank -> cap)
   composes with the bridges above. These re-exports make the Filter/Rank/Bound
   dependencies substantive - each names the exact Rust-enforced fact.
   =========================================================================== *)

(* Filter: the fork-choice input excludes slashed validators (T-10). *)
Corollary honest_lms_exclude_slashed :
  forall lms inv v h, In v inv -> ~ In (v, h) (filter_inv lms inv).
Proof. exact invalid_excluded. Qed.

Corollary forkchoice_ignores_receiver_invalid_cache :
  forall lms certified_exclusions receiver_cache1 receiver_cache2,
    projection_with_receiver_cache lms certified_exclusions receiver_cache1 =
    projection_with_receiver_cache lms certified_exclusions receiver_cache2.
Proof. exact receiver_cache_noninterference. Qed.

(* Rank: the fork-choice descent always terminates at a tip. *)
Corollary forkchoice_descent_reaches_tip :
  forall d score is_scored h,
    wf_dag d -> wf_lookup d ->
    best_child d score is_scored (rank d score is_scored (S (dag_max_num d)) h) = None.
Proof. exact rank_terminates. Qed.

(* Bound: a positive/unlimited cap of the honest depth-filtered parents STILL
   passes the validator's bound (firstn is a prefix; forallb survives it). *)
Lemma forallb_firstn :
  forall (A : Type) (p : A -> bool) (n : nat) (l : list A),
    forallb p l = true -> forallb p (firstn n l) = true.
Proof.
  intros A p n. induction n as [| k IH]; intros l H; simpl.
  - reflexivity.
  - destruct l as [| x xs]; simpl in *; [reflexivity |].
    apply andb_true_iff in H. destruct H as [Hx Hxs].
    apply andb_true_iff. split; [exact Hx | apply IH; exact Hxs].
Qed.

Corollary capped_parents_validate :
  forall maxn mpd buf n unlimited nums,
    declared_parents_ok maxn (mpd + buf)
      (cap_tips n unlimited (prop_filter_head maxn mpd nums)) = true.
Proof.
  intros maxn mpd buf n unlimited nums. unfold cap_tips. destruct unlimited.
  - apply honest_forkchoice_parents_validate.
  - destruct nums as [| head tail].
    + destruct n; reflexivity.
    + simpl.
    destruct n as [| n]; simpl; [reflexivity |].
    unfold parents_ok. apply forallb_firstn.
    exact (filtered_tail_validates maxn mpd buf tail).
Qed.
