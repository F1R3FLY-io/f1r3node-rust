(* ===========================================================================
   CliqueOracle.v - Agreeing sets, quorum finalization, and the two
   monotonicity lemmas the frontier cache rests on: L-ANC and L-SNAP.

   The finalized floor is derived by walking a parent's main-parent spine and
   asking, at each block, whether the clique oracle certifies it finalized over
   a frozen justification snapshot (`CliqueOracle::ft_witnessed`, clique_oracle.rs
   :437). The Phase-2 fix caches the per-block frontier and resolves later
   frontiers by an incremental UP-walk from that pivot instead of a full
   down-walk. That cache is only sound if it is TRANSPARENT: the warm up-walk
   must return the identical block the cold down-walk would. Transparency rests
   on two facts about finalization:

     L-ANC  (ancestor-monotone):  if a block is finalized then every ancestor
            of it on the spine is finalized too (over the same snapshot and the
            same committee). => finalized blocks form a downward-closed prefix,
            so "highest finalized" is well defined and the up-walk may stop at
            the first non-finalized block.

     L-SNAP (snapshot-monotone):  a block finalized over a snapshot J stays
            finalized over any larger snapshot J' \supseteq J. => the cached
            pivot F(parent) (finalized over parent's own snapshot) is still
            finalized over the child's larger snapshot, so it is a valid pivot.

   ---------------------------------------------------------------------------
   Faithful abstraction of the clique oracle
   ---------------------------------------------------------------------------
   `ft_witnessed(b,J) >= t` holds when a max-weight clique of validators that
   AGREE on `b` (each has `b` in the DAG-past of its latest message in J) carries
   > 1/2 of the committee's stake. We model finalization as:

       Finalized c J b  :=  there is a majority-weight sub-committee Q of c
                            every member of which agrees on b.

   A clique is a special such Q, so this is the clique rule with the pairwise-
   compatibility structure abstracted away. Crucially, L-ANC and L-SNAP hold by
   the SAME-QUORUM argument: the identical validators Q that finalized `b` also
   agree on every ancestor of `b` (they have `b`, hence its ancestors, in their
   past) and still agree under a larger snapshot. This is exactly WHY the lemmas
   are true for the real oracle -- the witnessing set is reused unchanged, so
   the pairwise-clique refinement carries over verbatim. Committee weights never
   enter these proofs (Q, its weight, and the total are all unchanged); only the
   `agrees` predicate moves, and it moves monotonically.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                    | Paper / Spec            | Rust Implementation
   ------------------------+-------------------------+-------------------------
   anc_of                  | DAG ancestry <=         | is_dag_ancestor (bdkvs.rs:517)
   agrees                  | validator agrees on msg | agreeing_weight_map_f (clique:445)
   Finalized               | ft_witnessed >= t       | CliqueOracle::ft_witnessed (:437)
   L_ANC                   | L-ANC                   | floor.rs warm up-walk stop rule
   L_SNAP                  | L-SNAP                  | floor.rs warm pivot guard
   snap_extends            | just(B) \supseteq just(P)| child snapshot dominates parent
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import ZArith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
From Stdlib Require Import Psatz.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import FtExact.

(* The default parsing scope of THIS file is `nat`: every committee weight, quorum
   bound (`2 * cweight Q > cweight c`), and agreement predicate is over `nat`.
   `ZArith`/`Psatz` are required only so the θ-exact finalization test (Section 7)
   can talk to `FtExact.ft_exact_ge`, which is stated over `Z` (i128 in Rust). We
   force `nat_scope` back to the top of the scope stack so the existing quorum
   arithmetic keeps its `nat` meaning, and annotate the handful of `Z` comparisons
   with `%Z`. (`FtExact` opens then CLOSES `Z_scope`, so requiring it leaves the
   scope stack unchanged.) *)
Open Scope nat_scope.

(* ===========================================================================
   Section 1 - DAG ancestry (general, over all parent edges)

   `walk_spine` (Foundation) descends only the MAIN-parent spine; agreement is
   about GENERAL DAG ancestry (a validator's latest message may DAG-descend from
   `b` via any parent path). `anc_of d a x` reads "a is an ancestor-or-self of x".
   =========================================================================== *)

Inductive anc_of (d : DAG) : BlockHash -> BlockHash -> Prop :=
  | anc_refl : forall x, anc_of d x x
  | anc_par  : forall a x b p,
      lookup d x = Some b ->
      In p (blk_parents b) ->
      anc_of d a p ->
      anc_of d a x.

(* Ancestry is transitive: standard for a reachability closure. *)
Lemma anc_of_trans :
  forall d a b c, anc_of d a b -> anc_of d b c -> anc_of d a c.
Proof.
  intros d a b c Hab Hbc.
  induction Hbc as [x | a' x bb p Hlook Hin Hanc IH].
  - exact Hab.
  - eapply anc_par; eauto.
Qed.

(* ===========================================================================
   Section 2 - Well-formed main parent, and: the main parent is an ancestor

   In a signed block, parents[0] is the main parent; Foundation carries it as a
   separate field `blk_main_parent`. `wf_mainparent` ties them together (the main
   parent is among the parents), which is all we need to conclude the main parent
   is a general DAG ancestor.
   =========================================================================== *)

Definition wf_mainparent (d : DAG) : Prop :=
  forall x b ph,
    lookup d x = Some b -> blk_main_parent b = Some ph -> In ph (blk_parents b).

Lemma mainparent_anc :
  forall d x b ph,
    wf_mainparent d ->
    lookup d x = Some b ->
    blk_main_parent b = Some ph ->
    anc_of d ph x.
Proof.
  intros d x b ph Hwf Hlook Hmp.
  eapply anc_par.
  - exact Hlook.
  - apply (Hwf x b ph Hlook Hmp).
  - apply anc_refl.
Qed.

(* ===========================================================================
   Section 3 - Justification snapshots and agreement
   =========================================================================== *)

Definition Snapshot := list (Validator * BlockHash).

Fixpoint snap_get (J : Snapshot) (v : Validator) : option BlockHash :=
  match J with
  | [] => None
  | (v', h) :: rest => if Nat.eqb v' v then Some h else snap_get rest v
  end.

(* J' extends J: every binding of J is preserved in J' (the child's snapshot
   dominates the parent's; new validators may be added). *)
Definition snap_extends (J' J : Snapshot) : Prop :=
  forall v h, snap_get J v = Some h -> snap_get J' v = Some h.

Lemma snap_extends_refl : forall J, snap_extends J J.
Proof. intros J v h H. exact H. Qed.

Lemma snap_extends_trans :
  forall J1 J2 J3, snap_extends J2 J1 -> snap_extends J3 J2 -> snap_extends J3 J1.
Proof. intros J1 J2 J3 H12 H23 v h H. apply H23, H12, H. Qed.

(* A validator agrees on `b` (over snapshot J) iff `b` is a DAG-ancestor of the
   validator's latest message in J. *)
Definition agrees (d : DAG) (J : Snapshot) (v : Validator) (b : BlockHash) : Prop :=
  exists h, snap_get J v = Some h /\ anc_of d b h.

(* Agreement is monotone downward along ancestry: if `b'` is an ancestor of `b`
   and v agrees on `b`, then v agrees on `b'` (it has `b`, hence `b'`, in past). *)
Lemma agrees_anc_mono :
  forall d J v b b', anc_of d b' b -> agrees d J v b -> agrees d J v b'.
Proof.
  intros d J v b b' Hanc [h [Hh Hbh]].
  exists h. split; [exact Hh |].
  eapply anc_of_trans; [exact Hanc | exact Hbh].
Qed.

(* Agreement is monotone under snapshot growth. *)
Lemma agrees_snap_mono :
  forall d J J' v b, snap_extends J' J -> agrees d J v b -> agrees d J' v b.
Proof.
  intros d J J' v b Hext [h [Hh Hbh]].
  exists h. split; [apply Hext; exact Hh | exact Hbh].
Qed.

(* ===========================================================================
   Section 4 - Committees, quorums, and finalization
   =========================================================================== *)

Definition Committee := list (Validator * nat).

Fixpoint cweight (c : Committee) : nat :=
  match c with
  | [] => 0
  | (_, w) :: rest => w + cweight rest
  end.

(* Q is a majority-weight sub-committee of c. `incl` keeps the proof of L-ANC/
   L-SNAP weight-free: we reuse the SAME Q, so its inclusion and weight are
   unchanged and never need re-derivation.

   NoDup (Tier-2 faithfulness): `NoDup (map fst Q)` states the quorum's VALIDATORS
   are pairwise distinct — matching the code, where the agreeing/clique set is a
   `WeightMap = HashMap<V, i64>` (clique_oracle.rs) whose KEYS are distinct
   validators by construction (a HashMap cannot hold two entries for one key). It
   is a faithful structural fact of the code's quorum representation, not a
   weakening: it is carried opaquely through L-ANC/L-SNAP (same Q) and is exactly
   the distinctness a quorum-INTERSECTION argument would need (see the SCOPE
   DISCLOSURE in the verification dossier — no such intersection theorem is claimed
   here; the capstones prove DETERMINISM, not CBC finalization safety). *)
Definition is_quorum (c Q : Committee) : Prop :=
  incl Q c /\ NoDup (map fst Q) /\ 2 * cweight Q > cweight c.

(* Finalization: some majority-weight sub-committee all agree on `b`. Faithful
   monotone abstraction of `ft_witnessed(b,J) >= t` (a clique is such a Q). *)
Definition Finalized (d : DAG) (c : Committee) (J : Snapshot) (b : BlockHash) : Prop :=
  exists Q, is_quorum c Q /\ (forall v w, In (v, w) Q -> agrees d J v b).

(* ===========================================================================
   Section 5 - L-ANC : ancestor-monotone finalization

   The SAME quorum that finalizes `b` finalizes every ancestor `b'` of `b`: each
   member agrees on `b`, hence on `b'` (agrees_anc_mono). Committee and weights
   untouched. This is the downward-closure that makes the warm up-walk's
   stop-at-first-non-finalized rule equal to the cold down-walk.
   =========================================================================== *)

Theorem L_ANC :
  forall d c J b b',
    anc_of d b' b -> Finalized d c J b -> Finalized d c J b'.
Proof.
  intros d c J b b' Hanc [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_anc_mono; [exact Hanc | apply (Hag v w Hin)].
Qed.

(* Spine specialization: a finalized block's main parent is finalized. This is
   the exact step the warm up-walk relies on when advancing/stopping. *)
Corollary L_ANC_mainparent :
  forall d c J x b ph,
    wf_mainparent d ->
    lookup d x = Some b ->
    blk_main_parent b = Some ph ->
    Finalized d c J x ->
    Finalized d c J ph.
Proof.
  intros d c J x b ph Hwf Hlook Hmp Hfin.
  eapply L_ANC; [eapply mainparent_anc; eauto | exact Hfin].
Qed.

(* ===========================================================================
   Section 6 - L-SNAP : snapshot-monotone finalization

   The SAME quorum finalizes `b` over any larger snapshot: each member still
   agrees (agrees_snap_mono). This is the pivot guard's justification: F(parent),
   finalized over parent's own snapshot, remains finalized over the child's
   larger snapshot, so it is a sound up-walk pivot.
   =========================================================================== *)

Theorem L_SNAP :
  forall d c J J' b,
    snap_extends J' J -> Finalized d c J b -> Finalized d c J' b.
Proof.
  intros d c J J' b Hext [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_snap_mono; [exact Hext | apply (Hag v w Hin)].
Qed.

(* Combined: finalization is monotone in BOTH arguments at once - the exact
   shape the frontier cache uses (a lower ancestor over a larger snapshot). *)
Corollary L_ANC_SNAP :
  forall d c J J' b b',
    snap_extends J' J -> anc_of d b' b ->
    Finalized d c J b -> Finalized d c J' b'.
Proof.
  intros d c J J' b b' Hext Hanc Hfin.
  eapply L_SNAP; [exact Hext |].
  eapply L_ANC; [exact Hanc | exact Hfin].
Qed.

(* ===========================================================================
   Section 7 - C1 : the θ-exact fault-tolerance finalization test (Finalized_ft).

   Sections 4-6 finalize a block when a strict-majority-weight quorum agrees on it
   (`is_quorum`'s `2 * cweight Q > cweight c`). That strict-majority test is the
   θ = 0 CORNER of the fault-tolerance threshold the NODE actually evaluates. The
   real node decision (A9, FtExact.v) is the exact-integer test

       ft_exact_ge q S num den   :=   2*q*den >= S*(den + num)      (over Z / i128)

   with q = agreeing (clique) weight, S = total committee stake, θ = num/den =
   ppm/1e6. For num > 0 this is STRICTER than strict majority: it demands a margin
   S·θ ABOVE half, not merely above half. We lift finalization to this exact test
   as `Finalized_ft`, re-prove L-ANC / L-SNAP for it (still quorum-OPAQUE — the
   witnessing set Q, its `incl`, and the ft-inequality are carried through
   untouched, exactly as for `Finalized`; committee weights never enter), and then
   BRIDGE it back to the strict-majority `Finalized`, so every θ-finalized block
   inherits L-ANC, L-SNAP, L-ANC-SNAP, T-CACHE (frontier_cache_transparent) and
   every downstream capstone WITHOUT re-proof. Thus T-CACHE's no-fork guarantee
   rests on the actual node test, not merely on the strict-majority proxy.

   Rocq                           | Rust (safety/clique_oracle.rs)
   -------------------------------+--------------------------------------------
   is_quorum_ft / Finalized_ft    | ft_witnessed(b,J): 2q·den ≥ S(den+num)  (:437)
   L_ANC_ft / L_SNAP_ft           | warm up-walk stop/pivot rules, exact test
   Finalized_ft_refines_Finalized | θ-test ⇒ strict-majority proxy (num,den,stake>0)
   =========================================================================== *)

(* A θ-exact quorum: an included sub-committee whose weight clears the exact FT
   test at threshold num/den. `Z.of_nat` injects the (nat) weights into Z, where
   FtExact states the test. The `incl Q c` half is IDENTICAL to `is_quorum`. *)
Definition is_quorum_ft (c Q : Committee) (num den : Z) : Prop :=
  incl Q c /\ NoDup (map fst Q) /\
  ft_exact_ge (Z.of_nat (cweight Q)) (Z.of_nat (cweight c)) num den.

(* θ-exact finalization: some θ-exact quorum all agree on `b`. This is the test
   the NODE runs; `Finalized` (Section 4) is its strict-majority (θ = 0⁺) proxy. *)
Definition Finalized_ft (d : DAG) (c : Committee) (J : Snapshot) (b : BlockHash)
                        (num den : Z) : Prop :=
  exists Q, is_quorum_ft c Q num den /\ (forall v w, In (v, w) Q -> agrees d J v b).

(* L-ANC for the exact test — SAME quorum, weight-free (mirrors L_ANC). *)
Theorem L_ANC_ft :
  forall d c J b b' num den,
    anc_of d b' b -> Finalized_ft d c J b num den -> Finalized_ft d c J b' num den.
Proof.
  intros d c J b b' num den Hanc [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_anc_mono; [exact Hanc | apply (Hag v w Hin)].
Qed.

(* Spine specialization for the exact test: a θ-finalized block's main parent is
   θ-finalized (mirrors L_ANC_mainparent) — the step the warm up-walk relies on. *)
Corollary L_ANC_ft_mainparent :
  forall d c J x b ph num den,
    wf_mainparent d ->
    lookup d x = Some b ->
    blk_main_parent b = Some ph ->
    Finalized_ft d c J x num den ->
    Finalized_ft d c J ph num den.
Proof.
  intros d c J x b ph num den Hwf Hlook Hmp Hfin.
  eapply L_ANC_ft; [eapply mainparent_anc; eauto | exact Hfin].
Qed.

(* L-SNAP for the exact test — SAME quorum, weight-free (mirrors L_SNAP). *)
Theorem L_SNAP_ft :
  forall d c J J' b num den,
    snap_extends J' J -> Finalized_ft d c J b num den -> Finalized_ft d c J' b num den.
Proof.
  intros d c J J' b num den Hext [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_snap_mono; [exact Hext | apply (Hag v w Hin)].
Qed.

(* Combined monotonicity for the exact test (mirrors L_ANC_SNAP). *)
Corollary L_ANC_SNAP_ft :
  forall d c J J' b b' num den,
    snap_extends J' J -> anc_of d b' b ->
    Finalized_ft d c J b num den -> Finalized_ft d c J' b' num den.
Proof.
  intros d c J J' b b' num den Hext Hanc Hfin.
  eapply L_SNAP_ft; [exact Hext |].
  eapply L_ANC_ft; [exact Hanc | exact Hfin].
Qed.

(* ---------------------------------------------------------------------------
   The refinement bridge:  θ-exact test  ⇒  strict-majority proxy.

   ARITHMETIC CORE (pure Z). At θ = 0 (num = 0) the exact test is the NON-strict
   `2q ≥ S`; for num > 0 it strengthens to the STRICT `2q > S`. We prove both.

   NOTE (faithful, necessary side-condition — mirrors FtExact.v's `0 <= den` NOTE).
   The strict form `2q > S` additionally needs `0 < S` (a committee with positive
   total stake). It is GENUINELY necessary, not a convenience: with S = 0 the test
   `2q·den ≥ 0` is trivially true (e.g. the empty quorum Q = []), yet strict
   majority `2q > 0` fails, so `Finalized_ft` with a zero-stake committee does NOT
   imply `Finalized`. In the system every committee has positive total stake — an
   empty / zero-stake committee finalizes nothing — so `0 < S`, equivalently
   `0 < cweight c`, is a faithful domain fact, not a weakening of the finalization
   semantics. It is the ONLY hypothesis added beyond `0 < num` and `0 < den`
   (θ = num/den ∈ (0,1) needs num, den > 0). `Finalized` and its proofs are left
   entirely unchanged; the bridge only ADDS a route into them.
   --------------------------------------------------------------------------- *)

(* θ = 0 corner: the exact test is at least strict majority, NON-strictly. This
   is the `>=`-finalizer boundary (FtExact `ft_exact_ge` is the floor test). *)
Lemma ft_exact_ge_weak_majority :
  forall q S num den,
    (0 <= num)%Z -> (0 < den)%Z -> (0 <= S)%Z ->
    ft_exact_ge q S num den -> (2 * q >= S)%Z.
Proof. intros q S num den Hnum Hden HS Hft. unfold ft_exact_ge in Hft. nia. Qed.

(* num > 0 : the exact test is STRICTLY above majority, given positive stake `0<S`
   (see the NOTE above for why `0 < S` is faithful and necessary). *)
Lemma ft_exact_ge_strict_majority :
  forall q S num den,
    (0 < num)%Z -> (0 < den)%Z -> (0 < S)%Z ->
    ft_exact_ge q S num den -> (2 * q > S)%Z.
Proof. intros q S num den Hnum Hden HS Hft. unfold ft_exact_ge in Hft. nia. Qed.

(* THE BRIDGE. Every θ-finalized block (θ = num/den, num,den>0, positive committee
   stake) is finalized under the strict-majority `Finalized`, hence enjoys L_ANC,
   L_SNAP, L_ANC_SNAP, T-CACHE and every downstream capstone WITHOUT re-proof. The
   quorum witness Q is reused verbatim; only its weight bound is upgraded from the
   exact inequality to strict majority (`ft_exact_ge_strict_majority`, then a
   linear nat↔Z step). *)
Theorem Finalized_ft_refines_Finalized :
  forall d c J b num den,
    (0 < num)%Z -> (0 < den)%Z -> 0 < cweight c ->
    Finalized_ft d c J b num den -> Finalized d c J b.
Proof.
  intros d c J b num den Hnum Hden Hc Hfin.
  destruct Hfin as [Q [Hq Hag]].
  unfold is_quorum_ft in Hq. destruct Hq as [Hincl [Hnodup Hft]].
  exists Q. split; [| exact Hag].
  unfold is_quorum. split; [exact Hincl | split; [exact Hnodup |]].
  assert (HS : (0 < Z.of_nat (cweight c))%Z) by lia.
  pose proof (ft_exact_ge_strict_majority _ _ _ _ Hnum Hden HS Hft) as Hstrict.
  lia.
Qed.

(* ---------------------------------------------------------------------------
   Weight-monotonicity of the exact test (uses FtExact.ft_exact_mono_q): more
   agreeing stake never un-finalizes. A faithful companion, at the quorum /
   finalization level, of `ft_exact_mono_q`'s "more agreeing stake never
   un-finalizes"; `0 <= den` is inherited from `ft_exact_mono_q` (den = 1e6 ≥ 0 in
   the system). Kept because it is the natural monotonicity of the exact test.
   --------------------------------------------------------------------------- *)
Lemma is_quorum_ft_mono_weight :
  forall c Q Q' num den,
    (0 <= den)%Z ->
    incl Q' c ->
    NoDup (map fst Q') ->
    cweight Q <= cweight Q' ->
    is_quorum_ft c Q num den ->
    is_quorum_ft c Q' num den.
Proof.
  intros c Q Q' num den Hden Hincl' Hnodup' Hle [Hincl [Hnodup Hft]].
  split; [exact Hincl' | split; [exact Hnodup' |]].
  eapply ft_exact_mono_q with (q := Z.of_nat (cweight Q));
    [exact Hden | lia | exact Hft].
Qed.

(* Consequently: if a θ-exact quorum Q witnesses finalizability and a heavier (by
   weight) included sub-committee Q' all agree on `b`, then `b` is θ-finalized via
   Q'. Demonstrates that `Finalized_ft` is monotone in the agreeing weight. *)
Lemma Finalized_ft_enlarge :
  forall d c J b num den Q Q',
    (0 <= den)%Z ->
    incl Q' c ->
    NoDup (map fst Q') ->
    cweight Q <= cweight Q' ->
    is_quorum_ft c Q num den ->
    (forall v w, In (v, w) Q' -> agrees d J v b) ->
    Finalized_ft d c J b num den.
Proof.
  intros d c J b num den Q Q' Hden Hincl' Hnodup' Hle Hq Hag'.
  exists Q'. split; [| exact Hag'].
  eapply is_quorum_ft_mono_weight with (Q := Q);
    [exact Hden | exact Hincl' | exact Hnodup' | exact Hle | exact Hq].
Qed.

(* ===========================================================================
   Section 8 - C5 : snapshot ADVANCEMENT (latest-message monotonicity).

   `snap_extends` (Section 3) models snapshot growth as binding PRESERVATION: a
   validator keeps its EXACT latest message and new validators may appear. Real
   justification snapshots ADVANCE — a validator's latest message moves FORWARD to
   a DAG-DESCENDANT as it observes more of the DAG. We model that as
   `snap_advances`: every binding of the old snapshot is superseded by a binding
   of the new one on a DAG-descendant. Agreement, and hence finalization, is
   monotone under advancement: the moved-forward latest message still DAG-descends
   from any block it descended from before (`anc_of_trans`). Preservation is the
   REFLEXIVE (h' = h) special case, so `snap_advances` GENERALIZES `snap_extends`,
   and the Section-6 `L_SNAP` is recovered as a corollary (`L_SNAP_of_extends`).

   Rocq                          | Rust (engine/.../snapshot.rs)
   ------------------------------+-------------------------------------------
   snap_advances                 | latest messages advance to descendants
   agrees_snap_advance_mono      | an agreeing validator stays agreeing after advance
   L_SNAP_advance                | a finalized pivot stays finalized as snapshots advance
   snap_extends_snap_advances    | preservation ⇒ advancement (reflexive descendant)
   =========================================================================== *)

(* J' advances J over DAG d: every binding v ↦ h of J is superseded in J' by a
   binding v ↦ h' on a DAG-DESCENDANT of h (`anc_of d h h'` reads "h is an
   ancestor-or-self of h'"). New validators may still be added (they are simply
   unconstrained here, exactly as `snap_extends` leaves them unconstrained). *)
Definition snap_advances (d : DAG) (J' J : Snapshot) : Prop :=
  forall v h, snap_get J v = Some h ->
    exists h', snap_get J' v = Some h' /\ anc_of d h h'.

(* Agreement is monotone under advancement: if v agrees on `b` over J (`b` is an
   ancestor of v's latest message h) and h advances to a descendant h', then `b`
   is still an ancestor of h' (`anc_of_trans`), so v agrees on `b` over J'. *)
Lemma agrees_snap_advance_mono :
  forall d J J' v b,
    snap_advances d J' J -> agrees d J v b -> agrees d J' v b.
Proof.
  intros d J J' v b Hadv [h [Hh Hbh]].
  destruct (Hadv v h Hh) as [h' [Hh' Hhh']].
  exists h'. split; [exact Hh' |].
  eapply anc_of_trans; [exact Hbh | exact Hhh'].
Qed.

(* Preservation ⇒ advancement: take the descendant to be the block itself
   (`anc_refl`). Hence `snap_advances` GENERALIZES `snap_extends`. *)
Lemma snap_extends_snap_advances :
  forall d J' J, snap_extends J' J -> snap_advances d J' J.
Proof.
  intros d J' J Hext v h Hh.
  exists h. split; [apply Hext; exact Hh | apply anc_refl].
Qed.

(* L-SNAP under ADVANCEMENT, for the strict-majority `Finalized` — SAME quorum,
   weight-free (mirrors L_SNAP, with advancement in place of extension). *)
Theorem L_SNAP_advance :
  forall d c J J' b,
    snap_advances d J' J -> Finalized d c J b -> Finalized d c J' b.
Proof.
  intros d c J J' b Hadv [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_snap_advance_mono; [exact Hadv | apply (Hag v w Hin)].
Qed.

Corollary L_ANC_SNAP_advance :
  forall d c J J' b b',
    snap_advances d J' J -> anc_of d b' b ->
    Finalized d c J b -> Finalized d c J' b'.
Proof.
  intros d c J J' b b' Hadv Hanc Hfin.
  eapply L_SNAP_advance; [exact Hadv |].
  eapply L_ANC; [exact Hanc | exact Hfin].
Qed.

(* The Section-6 `L_SNAP` (preservation) is the REFLEXIVE-DESCENDANT corollary of
   advancement: `snap_extends ⇒ snap_advances`, then `L_SNAP_advance`. The original
   `L_SNAP` is retained verbatim (the capstone consumes it); this shows it is
   SUBSUMED by the more faithful advancement model. *)
Corollary L_SNAP_of_extends :
  forall d c J J' b,
    snap_extends J' J -> Finalized d c J b -> Finalized d c J' b.
Proof.
  intros d c J J' b Hext Hfin.
  eapply L_SNAP_advance; [apply snap_extends_snap_advances; exact Hext | exact Hfin].
Qed.

(* L-SNAP under ADVANCEMENT, for the θ-exact test (mirrors L_SNAP_ft). *)
Theorem L_SNAP_advance_ft :
  forall d c J J' b num den,
    snap_advances d J' J ->
    Finalized_ft d c J b num den -> Finalized_ft d c J' b num den.
Proof.
  intros d c J J' b num den Hadv [Q [Hq Hag]].
  exists Q. split; [exact Hq |].
  intros v w Hin.
  eapply agrees_snap_advance_mono; [exact Hadv | apply (Hag v w Hin)].
Qed.

Corollary L_ANC_SNAP_advance_ft :
  forall d c J J' b b' num den,
    snap_advances d J' J -> anc_of d b' b ->
    Finalized_ft d c J b num den -> Finalized_ft d c J' b' num den.
Proof.
  intros d c J J' b b' num den Hadv Hanc Hfin.
  eapply L_SNAP_advance_ft; [exact Hadv |].
  eapply L_ANC_ft; [exact Hanc | exact Hfin].
Qed.

(* ===========================================================================
   Section 9 - C1' : the θ-INDEPENDENT hard majority gate — θ ≤ 0 coverage.

   `Finalized_ft_refines_Finalized` (Section 7) needs `0 < num` (θ > 0). At the
   DEFAULT θ = 0 (num = 0) the θ-exact test degrades to the NON-strict `2q ≥ S`,
   and for the negative-θ sentinels (num < 0; `ft_decides_exact`, clique_oracle.rs
   :72-78, e.g. θ = -1 "finalize on any majority clique") it is weaker still — so
   the θ-test ALONE does NOT imply the strict-majority `Finalized` at θ ≤ 0, and
   that refinement is VACUOUS there.

   But the node does NOT finalize on the θ-test alone. `ft_decides_exact`
   (clique_oracle.rs:79-81) first applies a θ-INDEPENDENT HARD GATE

       if (agreeing as i128) * 2 <= s then return false      (agreeing ≤ S/2 ⇒ MIN)

   where `agreeing` = the TOTAL weight of validators agreeing on the target (the
   `agreeing_weight_map`, :578) and the θ-tested clique weight `q = max_clique_weight`
   is a SUB-part of it (`q ≤ agreeing`; the call sites pass them SEPARATELY,
   `ft_decides_exact(agreeing, max_clique_weight, …)` at :584/:629). So the real
   decision is `Finalized_ft ∧ (2·agreeing > S)`, and the hard gate — a strict
   majority of the AGREEING set — is EXACTLY the strict-majority `Finalized`
   (Section 4), for ALL num.

   We model the gate faithfully as `hard_gate` (an agreeing majority sub-committee),
   prove it equivalent to `Finalized`, and derive the θ ≤ 0 bridge
   `Finalized_ft_hg_refines_Finalized` for ALL num — no `0 < num`, no positive-stake
   side-condition. (Independently, T-CACHE holds DIRECTLY over `Finalized_ft` for all
   num via `L_ANC_ft`/`L_SNAP_ft` — see GuardBridge.BridgeFt — so cache transparency
   never needed the num>0 bridge; this section additionally recovers the strict-
   majority proxy at θ ≤ 0.)

   Rocq                              | Rust (safety/clique_oracle.rs)
   ----------------------------------+--------------------------------------------
   hard_gate (2·agreeing > S)        | ft_decides_exact :79-81 (agreeing*2 ≤ s ⇒ false)
   Finalized_ft_hg                   | ft_decides_exact = θ-test ∧ hard gate
   Finalized_ft_hg_refines_Finalized | θ-finalized ⇒ strict-majority (ALL num, θ ≤ 0)
   =========================================================================== *)

(* The θ-INDEPENDENT hard majority gate: a sub-committee A of the AGREEING
   validators carries strict-majority weight (`2·cweight A > cweight c`, i.e.
   `2·agreeing > S`) and every member agrees on `b`. Its shape is identical to a
   strict-majority quorum, so it is provably `Finalized`. *)
Definition hard_gate (d : DAG) (c : Committee) (J : Snapshot) (b : BlockHash) : Prop :=
  exists A, incl A c /\ NoDup (map fst A) /\ 2 * cweight A > cweight c
         /\ (forall v w, In (v, w) A -> agrees d J v b).

Lemma hard_gate_iff_Finalized :
  forall d c J b, hard_gate d c J b <-> Finalized d c J b.
Proof.
  intros d c J b. unfold hard_gate, Finalized, is_quorum. split.
  - intros [A [Hincl [Hnodup [Hmaj Hag]]]].
    exists A. split; [split; [exact Hincl | split; [exact Hnodup | exact Hmaj]] | exact Hag].
  - intros [Q [[Hincl [Hnodup Hmaj]] Hag]].
    exists Q. split; [exact Hincl | split; [exact Hnodup | split; [exact Hmaj | exact Hag]]].
Qed.

(* The node's REAL finalization decision: the θ-exact clique test AND the
   θ-independent hard majority gate (clique_oracle.rs:79-81). *)
Definition Finalized_ft_hg (d : DAG) (c : Committee) (J : Snapshot) (b : BlockHash)
                           (num den : Z) : Prop :=
  Finalized_ft d c J b num den /\ hard_gate d c J b.

(* THE θ ≤ 0 BRIDGE. The hard gate ALONE supplies strict-majority `Finalized`, for
   ALL num (num = 0 = default θ, and num < 0 sentinels) — NO `0 < num`, NO positive-
   stake side-condition. This closes the θ ≤ 0 seam `Finalized_ft_refines_Finalized`
   left open, so every hard-gated θ-finalized block inherits L-ANC, L-SNAP, T-CACHE
   and every downstream capstone regardless of θ. *)
Theorem Finalized_ft_hg_refines_Finalized :
  forall d c J b num den, Finalized_ft_hg d c J b num den -> Finalized d c J b.
Proof.
  intros d c J b num den [_ Hhg]. apply hard_gate_iff_Finalized. exact Hhg.
Qed.

(* The hard-gated decision is ancestor-monotone (L-ANC) for ALL num: both conjuncts
   are (Finalized_ft via L_ANC_ft; hard_gate = Finalized via L_ANC). *)
Theorem L_ANC_ft_hg :
  forall d c J b b' num den,
    anc_of d b' b -> Finalized_ft_hg d c J b num den -> Finalized_ft_hg d c J b' num den.
Proof.
  intros d c J b b' num den Hanc [Hft Hhg]. split.
  - exact (L_ANC_ft d c J b b' num den Hanc Hft).
  - apply hard_gate_iff_Finalized. apply hard_gate_iff_Finalized in Hhg.
    exact (L_ANC d c J b b' Hanc Hhg).
Qed.

(* The hard-gated decision is snapshot-monotone (L-SNAP) for ALL num. *)
Theorem L_SNAP_ft_hg :
  forall d c J J' b num den,
    snap_extends J' J -> Finalized_ft_hg d c J b num den -> Finalized_ft_hg d c J' b num den.
Proof.
  intros d c J J' b num den Hext [Hft Hhg]. split.
  - exact (L_SNAP_ft d c J J' b num den Hext Hft).
  - apply hard_gate_iff_Finalized. apply hard_gate_iff_Finalized in Hhg.
    exact (L_SNAP d c J J' b Hext Hhg).
Qed.

Section StatePreservingSupport.

Variable d : DAG.
Variable state_ancestor : BlockHash -> BlockHash -> Prop.

Definition state_agrees
  (J : Snapshot) (v : Validator) (b : BlockHash) : Prop :=
  exists h, snap_get J v = Some h /\ state_ancestor b h.

Definition StateFinalized_ft
  (c : Committee) (J : Snapshot) (b : BlockHash) (num den : Z) : Prop :=
  exists Q,
    is_quorum_ft c Q num den /\
    (forall v w, In (v, w) Q -> state_agrees J v b).

Definition state_hard_gate
  (c : Committee) (J : Snapshot) (b : BlockHash) : Prop :=
  exists A,
    incl A c /\
    NoDup (map fst A) /\
    2 * cweight A > cweight c /\
    (forall v w, In (v, w) A -> state_agrees J v b).

Definition StateFinalized_ft_hg
  (c : Committee) (J : Snapshot) (b : BlockHash) (num den : Z) : Prop :=
  StateFinalized_ft c J b num den /\ state_hard_gate c J b.

Theorem state_agreement_refines_causal_agreement :
  (forall ancestor descendant,
    state_ancestor ancestor descendant -> anc_of d ancestor descendant) ->
  forall J v b,
    state_agrees J v b -> agrees d J v b.
Proof.
  intros Hrefines J v b [h [Hlatest Hstate]].
  exists h. split.
  - exact Hlatest.
  - apply Hrefines. exact Hstate.
Qed.

Theorem state_finalization_refines_causal_finalization :
  (forall ancestor descendant,
    state_ancestor ancestor descendant -> anc_of d ancestor descendant) ->
  forall c J b num den,
    StateFinalized_ft_hg c J b num den ->
    Finalized_ft_hg d c J b num den.
Proof.
  intros Hrefines c J b num den [[Q [Hquorum Hagrees]]
    [A [Hin [Hnodup [Hmajority Hstate_agrees]]]]].
  split.
  - exists Q. split.
    + exact Hquorum.
    + intros v w Hmember.
      apply state_agreement_refines_causal_agreement.
      * exact Hrefines.
      * exact (Hagrees v w Hmember).
  - exists A. split.
    + exact Hin.
    + split.
      * exact Hnodup.
      * split.
        -- exact Hmajority.
        -- intros v w Hmember.
           apply state_agreement_refines_causal_agreement.
           ++ exact Hrefines.
           ++ exact (Hstate_agrees v w Hmember).
Qed.

End StatePreservingSupport.
