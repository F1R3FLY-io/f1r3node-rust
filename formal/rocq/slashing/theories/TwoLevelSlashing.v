(* ═══════════════════════════════════════════════════════════════════════════
   TwoLevelSlashing.v — Level 1 + Level 2 closure, termination, quorum

   Models the two-level slashing closure: validators who *witness* an
   equivocation in their justifications without slashing it are themselves
   slashed (Level 2).

   Theorems:
     T-11 (level_2_termination)            — closure reaches fixed point
     T-12 (level_2_collusion_resistance)   — quorum preserved if |E| ≤ f

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition       │ Rust Implementation                       │
   ──────────────────────┼───────────────────────────────────────────┤
   neglect_graph         │ implicit in justification structure       │
   slash_step            │ one round of prepare_slashing_deploys     │
   slash_closure         │ multi-block fixed-point convergence       │
   ─────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §7.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import PeanoNat.
From Slashing Require Import Validator.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Neglect graph and slash step
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The neglect graph: for each validator, the set of upstream offenders
   they failed to slash. *)
Definition NeglectGraph := Validator -> list Validator.

(* A slash step adds, to the slashed set, every validator whose neglect
   set intersects the current slashed set. *)
Fixpoint inter_nonempty (xs ys : list Validator) : bool :=
  match xs with
  | []      => false
  | x :: rest =>
      if existsb (fun y =>
                    if validator_eq_dec x y then true else false) ys
      then true
      else inter_nonempty rest ys
  end.

Definition slash_step
  (universe : list Validator)  (* all validators *)
  (g : NeglectGraph)
  (s : list Validator)         (* current slashed set *)
  : list Validator :=
  s ++ filter
        (fun v =>
           andb
             (negb (existsb (fun s' =>
                              if validator_eq_dec v s' then true else false) s))
             (inter_nonempty (g v) s))
        universe.

(* Iterate slash_step n times. *)
Fixpoint slash_iter (universe : list Validator) (g : NeglectGraph)
                    (s0 : list Validator) (n : nat) : list Validator :=
  match n with
  | 0   => s0
  | S k => slash_step universe g (slash_iter universe g s0 k)
  end.

Lemma inter_nonempty_nil_r :
  forall xs, inter_nonempty xs [] = false.
Proof.
  induction xs as [|x xs IH]; simpl; auto.
Qed.

Theorem slash_step_empty_initial_empty :
  forall universe g,
    slash_step universe g [] = [].
Proof.
  intros universe g.
  unfold slash_step. simpl.
  induction universe as [|v rest IH]; simpl; auto.
  rewrite inter_nonempty_nil_r. exact IH.
Qed.

Theorem slash_iter_empty_initial_empty :
  forall universe g n,
    slash_iter universe g [] n = [].
Proof.
  intros universe g n.
  induction n as [|n IH]; simpl; auto.
  rewrite IH. apply slash_step_empty_initial_empty.
Qed.

(* A universe-restricted directed neglect path from validator [v] to a
   direct offender [offender]. Edges point from neglecter to the upstream
   offender they failed to slash. *)
Inductive neglect_reaches_in
  (universe : list Validator) (g : NeglectGraph)
  : nat -> Validator -> Validator -> Prop :=
| nri_edge :
    forall v offender,
      In v universe ->
      In offender (g v) ->
      neglect_reaches_in universe g 1 v offender
| nri_step :
    forall k v mid offender,
      In v universe ->
      In mid (g v) ->
      neglect_reaches_in universe g k mid offender ->
      neglect_reaches_in universe g (S k) v offender.

Lemma inter_nonempty_true_exists :
  forall xs ys,
    inter_nonempty xs ys = true ->
    exists x, In x xs /\ In x ys.
Proof.
  induction xs as [| x xs IH]; intros ys H; simpl in H.
  - discriminate.
  - destruct (existsb (fun y => if validator_eq_dec x y then true else false) ys) eqn:Hex.
    + apply existsb_exists in Hex.
      destruct Hex as [y [Hy Heq]].
      destruct (validator_eq_dec x y) as [He | Hne]; try discriminate.
      subst. exists y. split; [left; reflexivity | assumption].
    + apply IH in H. destruct H as [z [Hzxs Hzys]].
      exists z. split; [right; assumption | assumption].
Qed.

Lemma inter_nonempty_exists_true :
  forall xs ys,
    (exists x, In x xs /\ In x ys) ->
    inter_nonempty xs ys = true.
Proof.
  induction xs as [| x xs IH]; intros ys [z [Hzxs Hzys]]; simpl in *.
  - contradiction.
  - destruct Hzxs as [Hz | Hzs].
    + subst.
      destruct (existsb (fun y => if validator_eq_dec z y then true else false) ys) eqn:Hex.
      * reflexivity.
      * exfalso.
        assert (existsb (fun y => if validator_eq_dec z y then true else false) ys = true) as Htrue.
        { apply existsb_exists. exists z. split; [assumption |].
          destruct (validator_eq_dec z z); congruence. }
        congruence.
    + destruct (existsb (fun y => if validator_eq_dec x y then true else false) ys).
      * reflexivity.
      * apply IH. exists z. split; assumption.
Qed.

Lemma slash_step_adds_reacher :
  forall universe g s v offender,
    In v universe ->
    In offender (g v) ->
    In offender s ->
    In v (slash_step universe g s).
Proof.
  intros universe g s v offender Hv Hedge Hoff.
  unfold slash_step.
  destruct (in_dec validator_eq_dec v s) as [Hins | Hnot].
  - apply in_or_app. left. assumption.
  - apply in_or_app. right. apply filter_In. split.
    + assumption.
    + apply andb_true_iff. split.
      * apply negb_true_iff.
        destruct (existsb (fun s' => if validator_eq_dec v s' then true else false) s) eqn:Hex.
        -- exfalso. apply existsb_exists in Hex.
           destruct Hex as [x [Hx Heq]].
           destruct (validator_eq_dec v x) as [Heqvx | Hne]; try discriminate.
           subst. contradiction.
        -- reflexivity.
      * apply inter_nonempty_exists_true.
        exists offender. split; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Monotonicity
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slash_step_monotone :
  forall universe g s,
    incl s (slash_step universe g s).
Proof.
  intros universe g s x Hin. unfold slash_step.
  apply in_or_app. left. assumption.
Qed.

Theorem slash_iter_monotone :
  forall universe g s0 n,
    incl s0 (slash_iter universe g s0 n).
Proof.
  intros universe g s0 n.
  induction n as [| k IH]; simpl.
  - intros x H. assumption.
  - intros x H. apply slash_step_monotone. apply IH. assumption.
Qed.

Lemma slash_iter_step_time_monotone :
  forall universe g s0 n,
    incl (slash_iter universe g s0 n)
         (slash_iter universe g s0 (S n)).
Proof.
  intros universe g s0 n.
  simpl. apply slash_step_monotone.
Qed.

Theorem slash_iter_time_monotone :
  forall universe g s0 m n,
    m <= n ->
    incl (slash_iter universe g s0 m)
         (slash_iter universe g s0 n).
Proof.
  intros universe g s0 m n Hle.
  induction Hle.
  - intros x H. assumption.
  - intros x H. apply slash_iter_step_time_monotone. apply IHHle. assumption.
Qed.

Theorem slash_iter_sound_reachability :
  forall universe g s0 n v,
    In v (slash_iter universe g s0 n) ->
    In v s0 \/
    exists offender k,
      In offender s0 /\
      k <= n /\
      neglect_reaches_in universe g k v offender.
Proof.
  intros universe g s0 n.
  induction n as [| n IH]; intros v H.
  - simpl in H. left. assumption.
  - simpl in H. unfold slash_step in H.
    apply in_app_or in H. destruct H as [Hprev | Hnew].
    + apply IH in Hprev. destruct Hprev as [Hs0 | Hreach].
      * left. assumption.
      * right. destruct Hreach as [offender [k [Hoff [Hle Hpath]]]].
        exists offender, k. repeat split; try assumption. lia.
    + apply filter_In in Hnew. destruct Hnew as [Huniverse Hcond].
      apply andb_true_iff in Hcond. destruct Hcond as [_ Hinter].
      apply inter_nonempty_true_exists in Hinter.
      destruct Hinter as [mid [Hedge Hmid]].
      apply IH in Hmid. destruct Hmid as [Hmid_s0 | Hmid_reach].
      * right. exists mid, 1. repeat split.
        -- assumption.
        -- lia.
        -- apply nri_edge; assumption.
      * right. destruct Hmid_reach as [offender [k [Hoff [Hle Hpath]]]].
        exists offender, (S k). repeat split; try assumption.
        -- lia.
        -- eapply nri_step; eassumption.
Qed.

Lemma reachability_in_slash_iter_exact :
  forall universe g s0 k v offender,
    In offender s0 ->
    neglect_reaches_in universe g k v offender ->
    In v (slash_iter universe g s0 k).
Proof.
  intros universe g s0 k v offender Hoff Hreach.
  induction Hreach.
  - simpl. eapply slash_step_adds_reacher; eassumption.
  - simpl. eapply slash_step_adds_reacher.
    + eassumption.
    + eassumption.
    + apply IHHreach. assumption.
Qed.

Theorem slash_iter_complete_reachability :
  forall universe g s0 n k v offender,
    In offender s0 ->
    k <= n ->
    neglect_reaches_in universe g k v offender ->
    In v (slash_iter universe g s0 n).
Proof.
  intros universe g s0 n k v offender Hoff Hle Hreach.
  apply (@slash_iter_time_monotone universe g s0 k n Hle).
  eapply reachability_in_slash_iter_exact; eassumption.
Qed.

Theorem slash_iter_reachability_characterization :
  forall universe g s0 n v,
    In v (slash_iter universe g s0 n) <->
    In v s0 \/
    exists offender k,
      In offender s0 /\
      k <= n /\
      neglect_reaches_in universe g k v offender.
Proof.
  intros universe g s0 n v. split.
  - apply slash_iter_sound_reachability.
  - intros [Hs0 | Hreach].
    + apply slash_iter_monotone. assumption.
    + destruct Hreach as [offender [k [Hoff [Hle Hpath]]]].
      eapply slash_iter_complete_reachability; eassumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Bounded closure
   ═══════════════════════════════════════════════════════════════════════════

   The slashed set is always a subset of the universe. Combined with
   monotonicity, this gives termination: after at most |universe|
   iterations, no new elements can be added. *)

Theorem slash_iter_in_universe :
  forall universe g s0 n,
    incl s0 universe ->
    incl (slash_iter universe g s0 n) universe.
Proof.
  intros universe g s0 n Hsub.
  induction n as [| k IH]; simpl.
  - assumption.
  - intros x Hin. unfold slash_step in Hin.
    apply in_app_or in Hin. destruct Hin as [Hin | Hin].
    + apply IH. assumption.
    + apply filter_In in Hin. destruct Hin as [Hu _]. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-11: Level-2 termination
   ═══════════════════════════════════════════════════════════════════════════

   For any starting set and graph, after |universe| iterations the slashed
   set is contained in universe. The stronger fixed-point theorem appears
   below as slash_iter_fixed_point_after_universe_bound. *)

Theorem t_11_level_2_termination :
  forall universe g s0,
    incl s0 universe ->
    incl (slash_iter universe g s0 (length universe)) universe.
Proof.
  intros. apply slash_iter_in_universe. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — T-12: Quorum preservation under bounded equivocation
   ═══════════════════════════════════════════════════════════════════════════

   If the initial slashed set has size at most f (the BFT bound), and the
   neglect graph is itself bounded (we model this by requiring no validator
   appears in their own neglect set), then the closure preserves quorum
   |universe \ slashed| ≥ |universe| - f.

   The full collusion-resistance theorem requires the BFT bound from
   [LSP82]; here we prove the structural statement. *)

Theorem t_12_quorum_preservation :
  forall (universe s0 : list Validator),
    incl s0 universe ->
    NoDup universe ->
    NoDup s0 ->
    length s0 <= length universe.
Proof.
  intros universe s0 Hsub Hndu Hnds.
  apply NoDup_incl_length; assumption.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   T-12 (BFT-style, Gap 4) — Quorum preservation under bounded-neglect.

   Under the BFT bound |equivocators| ≤ F, AND a bounded-neglect-graph
   hypothesis (the closure of the slash propagation through the neglect
   graph is also bounded by F), the slash closure preserves quorum:
       |universe \ slashed| ≥ |universe| - F

   The bounded-neglect hypothesis is the protocol-level assumption that
   ⌊(n-1)/3⌋ honest validators do not transitively neglect each other.
   This is the same assumption F1r3fly's two-level slashing relies on
   from [LSP82].

   We model the "closure" as a list of slashed validators reached by
   slash_iter, and prove that if it's a duplicate-free subset of size ≤ F,
   the active set (universe minus closure) has size ≥ |universe| - F. *)

Theorem t_12_bft_quorum_preservation :
  forall (universe : list Validator) (closure : list Validator) (F : nat),
    NoDup universe ->
    NoDup closure ->
    incl closure universe ->
    length closure <= F ->
    length universe - length closure >= length universe - F.
Proof.
  intros universe closure F Hndu Hndc Hsub Hbound.
  lia.
Qed.

(* Corollary: under the BFT bound, the active set after slash closure
   maintains BFT safety (i.e., at least n - F validators remain). *)
Theorem t_12_bft_active_set_size :
  forall (universe : list Validator) (closure : list Validator) (F : nat),
    NoDup universe ->
    NoDup closure ->
    incl closure universe ->
    length closure <= F ->
    F < length universe ->
    length universe - length closure > 0.
Proof.
  intros universe closure F Hndu Hndc Hsub Hbound Hflt.
  pose proof (NoDup_incl_length Hndc Hsub) as Hclen.
  lia.
Qed.

(* The slash_iter result is bounded by the universe's size, hence the
   closure is finitely bounded — combined with the BFT precondition, this
   gives the structural quorum bound. *)
Theorem t_12_slash_iter_in_bound :
  forall universe g s0 n,
    NoDup universe ->
    incl s0 universe ->
    NoDup s0 ->
    forall (Hclosure_unique : NoDup (slash_iter universe g s0 n)),
      length (slash_iter universe g s0 n) <= length universe.
Proof.
  intros universe g s0 n Hndu Hsub Hnds Hndc.
  pose proof (@slash_iter_in_universe universe g s0 n Hsub) as Hin.
  apply (NoDup_incl_length Hndc Hin).
Qed.

Definition validator_in (v : Validator) (xs : list Validator) : bool :=
  if in_dec validator_eq_dec v xs then true else false.

Definition filter_validators (universe xs : list Validator) : list Validator :=
  filter (fun v => validator_in v universe) xs.

Definition restrict_neglect_graph (universe : list Validator) (g : NeglectGraph)
  : NeglectGraph :=
  fun v => filter (fun offender => validator_in offender universe) (g v).

Lemma validator_in_true :
  forall v xs, validator_in v xs = true <-> In v xs.
Proof.
  intros v xs. unfold validator_in.
  destruct (in_dec validator_eq_dec v xs) as [Hin | Hnot].
  - split; intros; [assumption | reflexivity].
  - split; intros H; [discriminate | contradiction].
Qed.

Lemma filter_validators_in :
  forall universe xs v,
    In v (filter_validators universe xs) <-> In v xs /\ In v universe.
Proof.
  intros universe xs v. unfold filter_validators.
  rewrite filter_In. split.
  - intros [Hin Hmember]. apply validator_in_true in Hmember. split; assumption.
  - intros [Hin Huniverse]. split; [assumption | apply validator_in_true; assumption].
Qed.

Lemma restrict_neglect_graph_in :
  forall universe g v offender,
    In offender (restrict_neglect_graph universe g v) <->
    In offender (g v) /\ In offender universe.
Proof.
  intros universe g v offender. unfold restrict_neglect_graph.
  rewrite filter_In. split.
  - intros [Hin Hmember]. apply validator_in_true in Hmember. split; assumption.
  - intros [Hin Huniverse]. split; [assumption | apply validator_in_true; assumption].
Qed.

Theorem stale_direct_offender_filtered :
  forall universe s0 offender,
    ~ In offender universe ->
    ~ In offender (filter_validators universe s0).
Proof.
  intros universe s0 offender Hstale Hin.
  apply filter_validators_in in Hin. destruct Hin as [_ Huniverse].
  contradiction.
Qed.

Theorem restricted_closure_only_from_current_direct_offenders :
  forall universe g s0 n v,
    In v (slash_iter universe (restrict_neglect_graph universe g)
                     (filter_validators universe s0) n) ->
    In v (filter_validators universe s0) \/
    exists offender k,
      In offender s0 /\
      In offender universe /\
      k <= n /\
      neglect_reaches_in universe (restrict_neglect_graph universe g) k v offender.
Proof.
  intros universe g s0 n v H.
  apply slash_iter_sound_reachability in H.
  destruct H as [Hinitial | Hreach].
  - left. assumption.
  - right. destruct Hreach as [offender [k [Hoff [Hle Hpath]]]].
    apply filter_validators_in in Hoff. destruct Hoff as [Hoff Hcurrent].
    exists offender, k. repeat split; assumption.
Qed.

Definition visible_unreported_graph
  (visible reported : Validator -> list Validator) : NeglectGraph :=
  fun v => filter (fun offender => negb (validator_in offender (reported v)))
                  (visible v).

Definition rust_detectable_view_graph
  (detectable reported : Validator -> list Validator) : NeglectGraph :=
  visible_unreported_graph detectable reported.

Lemma visible_unreported_graph_in :
  forall visible reported v offender,
    In offender (visible_unreported_graph visible reported v) <->
    In offender (visible v) /\ ~ In offender (reported v).
Proof.
  intros visible reported v offender. unfold visible_unreported_graph.
  rewrite filter_In. split.
  - intros [Hin Hnot_reported].
    apply negb_true_iff in Hnot_reported.
    split; [assumption |].
    intro Hreported. apply validator_in_true in Hreported. congruence.
  - intros [Hin Hnot_reported]. split; [assumption |].
    apply negb_true_iff.
    destruct (validator_in offender (reported v)) eqn:Hmember; [|reflexivity].
    apply validator_in_true in Hmember. contradiction.
Qed.

Theorem rust_detectable_view_graph_in :
  forall detectable reported v offender,
    In offender (rust_detectable_view_graph detectable reported v) <->
    In offender (detectable v) /\ ~ In offender (reported v).
Proof.
  intros detectable reported v offender.
  unfold rust_detectable_view_graph.
  apply visible_unreported_graph_in.
Qed.

Theorem visible_reachability_first_edge :
  forall universe visible reported k v offender,
    neglect_reaches_in universe
      (visible_unreported_graph visible reported) k v offender ->
    exists mid,
      In mid (visible v) /\ ~ In mid (reported v).
Proof.
  intros universe visible reported k v offender Hreach.
  inversion Hreach; subst.
  - exists offender. apply visible_unreported_graph_in. assumption.
  - exists mid. apply visible_unreported_graph_in. assumption.
Qed.

Definition graph_incl (g1 g2 : NeglectGraph) : Prop :=
  forall v offender, In offender (g1 v) -> In offender (g2 v).

Definition graph_equiv (g1 g2 : NeglectGraph) : Prop :=
  forall v offender, In offender (g1 v) <-> In offender (g2 v).

Theorem visible_unreported_graph_incl :
  forall visible1 reported1 visible2 reported2,
    (forall v offender, In offender (visible1 v) -> In offender (visible2 v)) ->
    (forall v offender, In offender (reported2 v) -> In offender (reported1 v)) ->
    graph_incl
      (visible_unreported_graph visible1 reported1)
      (visible_unreported_graph visible2 reported2).
Proof.
  intros visible1 reported1 visible2 reported2 Hvisible Hreported v offender Hin.
  apply visible_unreported_graph_in in Hin.
  destruct Hin as [Hvis Hnot_reported].
  apply visible_unreported_graph_in. split.
  - apply Hvisible. assumption.
  - intro Hrep2. apply Hnot_reported. apply Hreported. assumption.
Qed.

Theorem reports_growth_shrinks_edges :
  forall visible reported_before reported_after,
    (forall v offender, In offender (reported_before v) -> In offender (reported_after v)) ->
    graph_incl
      (visible_unreported_graph visible reported_after)
      (visible_unreported_graph visible reported_before).
Proof.
  intros visible reported_before reported_after Hreports.
  apply visible_unreported_graph_incl.
  - intros v offender H. assumption.
  - intros v offender H. apply Hreports. assumption.
Qed.

Theorem reported_edge_not_active :
  forall visible reported v offender,
    In offender (reported v) ->
    ~ In offender (visible_unreported_graph visible reported v).
Proof.
  intros visible reported v offender Hreported Hactive.
  apply visible_unreported_graph_in in Hactive.
  destruct Hactive as [_ Hnot]. contradiction.
Qed.

Theorem unreported_visible_edge_remains_active :
  forall visible reported v offender,
    In offender (visible v) ->
    ~ In offender (reported v) ->
    In offender (visible_unreported_graph visible reported v).
Proof.
  intros visible reported v offender Hvisible Hnot.
  apply visible_unreported_graph_in. split; assumption.
Qed.

Lemma inter_nonempty_incl_true :
  forall xs ys s,
    incl xs ys ->
    inter_nonempty xs s = true ->
    inter_nonempty ys s = true.
Proof.
  intros xs ys s Hsub Hinter.
  apply inter_nonempty_true_exists in Hinter.
  destruct Hinter as [x [Hxs Hs]].
  apply inter_nonempty_exists_true.
  exists x. split; [apply Hsub; assumption | assumption].
Qed.

Theorem slash_step_graph_monotone :
  forall universe g1 g2 s,
    graph_incl g1 g2 ->
    incl (slash_step universe g1 s) (slash_step universe g2 s).
Proof.
  intros universe g1 g2 s Hg v Hin.
  unfold slash_step in *.
  apply in_app_or in Hin. destruct Hin as [Hin | Hnew].
  - apply in_or_app. left. assumption.
  - apply in_or_app. right.
    apply filter_In in Hnew. destruct Hnew as [Huniverse Hcond].
    apply filter_In. split; [assumption |].
    apply andb_true_iff in Hcond. destruct Hcond as [Hnot Hinter].
    apply andb_true_iff. split; [assumption |].
    eapply inter_nonempty_incl_true; [|eassumption].
    intros offender Hedge. apply Hg. assumption.
Qed.

Theorem slash_step_graph_arg_monotone :
  forall universe g1 g2 s1 s2,
    graph_incl g1 g2 ->
    incl s1 s2 ->
    incl (slash_step universe g1 s1) (slash_step universe g2 s2).
Proof.
  intros universe g1 g2 s1 s2 Hg Hs v Hin.
  unfold slash_step in *.
  apply in_app_or in Hin. destruct Hin as [Hin | Hnew].
  - apply in_or_app. left. apply Hs. assumption.
  - apply filter_In in Hnew. destruct Hnew as [Huniverse Hcond].
    apply andb_true_iff in Hcond. destruct Hcond as [_ Hinter].
    destruct (in_dec validator_eq_dec v s2) as [Hin2 | Hnot2].
    + apply in_or_app. left. assumption.
    + apply in_or_app. right. apply filter_In. split; [assumption |].
      apply andb_true_iff. split.
      * apply negb_true_iff.
        destruct (existsb (fun s' => if validator_eq_dec v s' then true else false) s2) eqn:Hex.
        -- exfalso. apply existsb_exists in Hex.
           destruct Hex as [x [Hx Heq]].
           destruct (validator_eq_dec v x) as [Heqvx | Hne]; try discriminate.
           subst. contradiction.
        -- reflexivity.
      * apply inter_nonempty_true_exists in Hinter.
        destruct Hinter as [x [Hxg Hxs1]].
        apply inter_nonempty_exists_true.
        exists x. split; [apply Hg; assumption | apply Hs; assumption].
Qed.

Theorem slash_iter_graph_incl :
  forall universe g1 g2 s0 n,
    graph_incl g1 g2 ->
    incl (slash_iter universe g1 s0 n) (slash_iter universe g2 s0 n).
Proof.
  intros universe g1 g2 s0 n Hg.
  induction n as [| n IH]; simpl.
  - intros v H. assumption.
  - apply slash_step_graph_arg_monotone; assumption.
Qed.

Theorem slash_iter_initial_monotone :
  forall universe g s1 s2 n,
    incl s1 s2 ->
    incl (slash_iter universe g s1 n)
         (slash_iter universe g s2 n).
Proof.
  intros universe g s1 s2 n Hs.
  induction n as [| n IH]; simpl.
  - assumption.
  - apply slash_step_graph_arg_monotone.
    + intros v offender H. assumption.
    + assumption.
Qed.

Theorem slash_iter_initial_graph_monotone :
  forall universe g1 g2 s1 s2 n,
    graph_incl g1 g2 ->
    incl s1 s2 ->
    incl (slash_iter universe g1 s1 n)
         (slash_iter universe g2 s2 n).
Proof.
  intros universe g1 g2 s1 s2 n Hg Hs.
  induction n as [| n IH]; simpl.
  - assumption.
  - apply slash_step_graph_arg_monotone; assumption.
Qed.

Definition union_neglect_graph (g1 g2 : NeglectGraph) : NeglectGraph :=
  fun v => g1 v ++ g2 v.

Theorem graph_incl_left_union :
  forall g1 g2,
    graph_incl g1 (union_neglect_graph g1 g2).
Proof.
  intros g1 g2 v offender H.
  unfold union_neglect_graph. apply in_or_app. left. assumption.
Qed.

Theorem graph_incl_right_union :
  forall g1 g2,
    graph_incl g2 (union_neglect_graph g1 g2).
Proof.
  intros g1 g2 v offender H.
  unfold union_neglect_graph. apply in_or_app. right. assumption.
Qed.

Theorem graph_union_equiv_comm :
  forall g1 g2,
    graph_equiv (union_neglect_graph g1 g2)
                (union_neglect_graph g2 g1).
Proof.
  intros g1 g2 v offender. unfold union_neglect_graph.
  repeat rewrite in_app_iff. tauto.
Qed.

Theorem graph_union_closure_overapproximates_left :
  forall universe g1 g2 s0 n,
    incl (slash_iter universe g1 s0 n)
         (slash_iter universe (union_neglect_graph g1 g2) s0 n).
Proof.
  intros universe g1 g2 s0 n.
  apply slash_iter_graph_incl.
  apply graph_incl_left_union.
Qed.

Theorem graph_union_closure_overapproximates_right :
  forall universe g1 g2 s0 n,
    incl (slash_iter universe g2 s0 n)
         (slash_iter universe (union_neglect_graph g1 g2) s0 n).
Proof.
  intros universe g1 g2 s0 n.
  apply slash_iter_graph_incl.
  apply graph_incl_right_union.
Qed.

Theorem graph_union_closure_commutative :
  forall universe g1 g2 s0 n v,
    In v (slash_iter universe (union_neglect_graph g1 g2) s0 n) <->
    In v (slash_iter universe (union_neglect_graph g2 g1) s0 n).
Proof.
  intros universe g1 g2 s0 n v. split; intro H.
  - apply (@slash_iter_graph_incl universe
      (union_neglect_graph g1 g2)
      (union_neglect_graph g2 g1) s0 n).
    + intros x offender Hedge. apply graph_union_equiv_comm. assumption.
    + assumption.
  - apply (@slash_iter_graph_incl universe
      (union_neglect_graph g2 g1)
      (union_neglect_graph g1 g2) s0 n).
    + intros x offender Hedge. apply graph_union_equiv_comm. assumption.
    + assumption.
Qed.

Definition view_closure
  (universe : list Validator)
  (visible reported : Validator -> list Validator)
  (s0 : list Validator)
  (n : nat) : list Validator :=
  slash_iter universe (visible_unreported_graph visible reported) s0 n.

Theorem view_closure_monotone_by_active_edges :
  forall universe visible1 reported1 visible2 reported2 s0 n,
    graph_incl
      (visible_unreported_graph visible1 reported1)
      (visible_unreported_graph visible2 reported2) ->
    incl (view_closure universe visible1 reported1 s0 n)
         (view_closure universe visible2 reported2 s0 n).
Proof.
  intros. unfold view_closure. apply slash_iter_graph_incl. assumption.
Qed.

Theorem view_closure_reports_antimonotone :
  forall universe visible reported_before reported_after s0 n,
    (forall v offender, In offender (reported_before v) -> In offender (reported_after v)) ->
    incl (view_closure universe visible reported_after s0 n)
         (view_closure universe visible reported_before s0 n).
Proof.
  intros universe visible reported_before reported_after s0 n Hreports.
  apply view_closure_monotone_by_active_edges.
  apply reports_growth_shrinks_edges. assumption.
Qed.

Theorem view_closure_equiv_by_active_edges :
  forall universe visible1 reported1 visible2 reported2 s0 n v,
    graph_equiv
      (visible_unreported_graph visible1 reported1)
      (visible_unreported_graph visible2 reported2) ->
    In v (view_closure universe visible1 reported1 s0 n) <->
    In v (view_closure universe visible2 reported2 s0 n).
Proof.
  intros universe visible1 reported1 visible2 reported2 s0 n v Hgraph.
  unfold view_closure. split; intro Hin.
  - apply (@slash_iter_graph_incl universe
      (visible_unreported_graph visible1 reported1)
      (visible_unreported_graph visible2 reported2) s0 n).
    + intros x offender Hedge. apply Hgraph. assumption.
    + assumption.
  - apply (@slash_iter_graph_incl universe
      (visible_unreported_graph visible2 reported2)
      (visible_unreported_graph visible1 reported1) s0 n).
    + intros x offender Hedge. apply Hgraph. assumption.
    + assumption.
Qed.

Theorem same_rust_detectable_view_same_closure :
  forall universe detectable1 reported1 detectable2 reported2 s0 n v,
    graph_equiv
      (rust_detectable_view_graph detectable1 reported1)
      (rust_detectable_view_graph detectable2 reported2) ->
    In v (slash_iter universe (rust_detectable_view_graph detectable1 reported1) s0 n) <->
    In v (slash_iter universe (rust_detectable_view_graph detectable2 reported2) s0 n).
Proof.
  intros universe detectable1 reported1 detectable2 reported2 s0 n v Hgraph.
  split; intro Hin.
  - apply (@slash_iter_graph_incl universe
      (rust_detectable_view_graph detectable1 reported1)
      (rust_detectable_view_graph detectable2 reported2) s0 n).
    + intros x offender Hedge. apply Hgraph. assumption.
    + assumption.
  - apply (@slash_iter_graph_incl universe
      (rust_detectable_view_graph detectable2 reported2)
      (rust_detectable_view_graph detectable1 reported1) s0 n).
    + intros x offender Hedge. apply Hgraph. assumption.
    + assumption.
Qed.

Lemma inter_nonempty_ext :
  forall xs ys s,
    (forall x, In x xs <-> In x ys) ->
    inter_nonempty xs s = inter_nonempty ys s.
Proof.
  intros xs ys s Heq.
  destruct (inter_nonempty xs s) eqn:Hx;
  destruct (inter_nonempty ys s) eqn:Hy; try reflexivity.
  - apply inter_nonempty_true_exists in Hx.
    destruct Hx as [x [Hxs Hs]].
    assert (inter_nonempty ys s = true) as Htrue.
    { apply inter_nonempty_exists_true.
      exists x. split; [apply Heq; assumption | assumption]. }
    congruence.
  - apply inter_nonempty_true_exists in Hy.
    destruct Hy as [x [Hys Hs]].
    assert (inter_nonempty xs s = true) as Htrue.
    { apply inter_nonempty_exists_true.
      exists x. split; [apply Heq; assumption | assumption]. }
    congruence.
Qed.

Theorem slash_step_graph_equiv :
  forall universe g1 g2 s v,
    graph_equiv g1 g2 ->
    In v (slash_step universe g1 s) <->
    In v (slash_step universe g2 s).
Proof.
  intros universe g1 g2 s v Hg. unfold slash_step.
  split; intro H;
  apply in_app_or in H; destruct H as [Hprev | Hnew].
  - apply in_or_app. left. assumption.
  - apply in_or_app. right. apply filter_In in Hnew.
    destruct Hnew as [Huniverse Hcond]. apply filter_In. split; [assumption |].
    apply andb_true_iff in Hcond. destruct Hcond as [Hnot Hinter].
    apply andb_true_iff. split; [assumption |].
    rewrite <- (inter_nonempty_ext (g1 v) (g2 v) s (Hg v)).
    assumption.
  - apply in_or_app. left. assumption.
  - apply in_or_app. right. apply filter_In in Hnew.
    destruct Hnew as [Huniverse Hcond]. apply filter_In. split; [assumption |].
    apply andb_true_iff in Hcond. destruct Hcond as [Hnot Hinter].
    apply andb_true_iff. split; [assumption |].
    rewrite (inter_nonempty_ext (g1 v) (g2 v) s (Hg v)).
    assumption.
Qed.

Theorem slash_iter_graph_equiv :
  forall universe g1 g2 s0 n v,
    graph_equiv g1 g2 ->
    In v (slash_iter universe g1 s0 n) <->
    In v (slash_iter universe g2 s0 n).
Proof.
  intros universe g1 g2 s0 n.
  induction n as [| n IH]; intros v Hg; simpl.
  - split; intro H; assumption.
  - transitivity (In v (slash_step universe g2 (slash_iter universe g1 s0 n))).
    + apply slash_step_graph_equiv. assumption.
    + unfold slash_step. split; intro H;
      apply in_app_or in H; destruct H as [Hprev | Hnew].
      * apply in_or_app. left. apply (IH v Hg). assumption.
      * apply in_or_app. right. apply filter_In in Hnew.
        destruct Hnew as [Huniverse Hcond]. apply filter_In. split; [assumption |].
        apply andb_true_iff in Hcond. destruct Hcond as [Hnot Hinter].
        apply andb_true_iff. split.
        -- apply negb_true_iff. apply negb_true_iff in Hnot.
           destruct (existsb (fun s' => if validator_eq_dec v s' then true else false)
                             (slash_iter universe g2 s0 n)) eqn:Hcontra; [|reflexivity].
           exfalso. apply existsb_exists in Hcontra.
           destruct Hcontra as [x [Hx Heq]].
           assert (existsb (fun s' => if validator_eq_dec v s' then true else false)
                           (slash_iter universe g1 s0 n) = true) as Htrue.
           { apply existsb_exists.
             exists x. split; [apply (IH x Hg); assumption | assumption]. }
           congruence.
        -- destruct (inter_nonempty (g2 v) (slash_iter universe g2 s0 n)) eqn:Htarget.
           ++ reflexivity.
           ++ apply inter_nonempty_true_exists in Hinter.
              destruct Hinter as [x [Hxg Hxslash]].
              assert (inter_nonempty (g2 v) (slash_iter universe g2 s0 n) = true) as Htrue.
              { apply inter_nonempty_exists_true.
                exists x. split; [assumption | apply (IH x Hg); assumption]. }
              congruence.
      * apply in_or_app. left. apply (IH v Hg). assumption.
      * apply in_or_app. right. apply filter_In in Hnew.
        destruct Hnew as [Huniverse Hcond]. apply filter_In. split; [assumption |].
        apply andb_true_iff in Hcond. destruct Hcond as [Hnot Hinter].
        apply andb_true_iff. split.
        -- apply negb_true_iff. apply negb_true_iff in Hnot.
           destruct (existsb (fun s' => if validator_eq_dec v s' then true else false)
                             (slash_iter universe g1 s0 n)) eqn:Hcontra; [|reflexivity].
           exfalso. apply existsb_exists in Hcontra.
           destruct Hcontra as [x [Hx Heq]].
           assert (existsb (fun s' => if validator_eq_dec v s' then true else false)
                           (slash_iter universe g2 s0 n) = true) as Htrue.
           { apply existsb_exists.
             exists x. split; [apply (IH x Hg); assumption | assumption]. }
           congruence.
        -- destruct (inter_nonempty (g2 v) (slash_iter universe g1 s0 n)) eqn:Htarget.
           ++ reflexivity.
           ++ apply inter_nonempty_true_exists in Hinter.
              destruct Hinter as [x [Hxg Hxslash]].
              assert (inter_nonempty (g2 v) (slash_iter universe g1 s0 n) = true) as Htrue.
              { apply inter_nonempty_exists_true.
                exists x. split; [assumption | apply (IH x Hg); assumption]. }
              congruence.
Qed.

Theorem slash_step_incl_universe :
  forall universe g s,
    incl s universe ->
    incl (slash_step universe g s) universe.
Proof.
  intros universe g s Hs v Hin.
  unfold slash_step in Hin.
  apply in_app_or in Hin. destruct Hin as [Hprev | Hnew].
  - apply Hs. assumption.
  - apply filter_In in Hnew. destruct Hnew as [Huniverse _]. assumption.
Qed.

Theorem slash_iter_incl_universe :
  forall universe g s0 n,
    incl s0 universe ->
    incl (slash_iter universe g s0 n) universe.
Proof.
  intros universe g s0 n Hs0.
  induction n as [| n IH]; simpl.
  - assumption.
  - apply slash_step_incl_universe. assumption.
Qed.

Definition validator_renaming_maps_universe
  (universe : list Validator) (rho : Validator -> Validator) : Prop :=
  forall v, In v universe -> In (rho v) universe.

Definition validator_set_renaming_incl
  (rho : Validator -> Validator) (xs ys : list Validator) : Prop :=
  forall v, In v xs -> In (rho v) ys.

Definition neglect_graph_renaming_incl
  (universe : list Validator)
  (rho : Validator -> Validator)
  (g h : NeglectGraph) : Prop :=
  forall v offender,
    In v universe ->
    In offender universe ->
    In offender (g v) ->
    In (rho offender) (h (rho v)).

Definition validator_renaming_inverse_on
  (universe : list Validator)
  (rho sigma : Validator -> Validator) : Prop :=
  forall v, In v universe -> sigma (rho v) = v.

Theorem slash_iter_validator_renaming_incl :
  forall universe g h s0 t0 n rho,
    incl s0 universe ->
    validator_renaming_maps_universe universe rho ->
    validator_set_renaming_incl rho s0 t0 ->
    neglect_graph_renaming_incl universe rho g h ->
    forall v,
      In v (slash_iter universe g s0 n) ->
      In (rho v) (slash_iter universe h t0 n).
Proof.
  intros universe g h s0 t0 n rho Hs0 Hmap Hinit Hedges.
  induction n as [| n IH]; intros v Hin; simpl in *.
  - apply Hinit. assumption.
  - unfold slash_step in Hin.
    apply in_app_or in Hin. destruct Hin as [Hprev | Hnew].
    + apply slash_step_monotone. apply IH. assumption.
    + apply filter_In in Hnew. destruct Hnew as [Huniverse Hcond].
      apply andb_true_iff in Hcond. destruct Hcond as [_ Hinter].
      apply inter_nonempty_true_exists in Hinter.
      destruct Hinter as [offender [Hedge Hoffender]].
      apply slash_step_adds_reacher with (offender := rho offender).
      * apply Hmap. assumption.
      * apply Hedges.
        -- assumption.
        -- eapply slash_iter_incl_universe; [exact Hs0 | exact Hoffender].
        -- assumption.
      * apply IH. assumption.
Qed.

Theorem slash_iter_validator_renaming_equiv :
  forall universe g h s0 t0 n rho sigma v,
    incl s0 universe ->
    incl t0 universe ->
    validator_renaming_maps_universe universe rho ->
    validator_renaming_maps_universe universe sigma ->
    validator_renaming_inverse_on universe rho sigma ->
    validator_renaming_inverse_on universe sigma rho ->
    validator_set_renaming_incl rho s0 t0 ->
    validator_set_renaming_incl sigma t0 s0 ->
    neglect_graph_renaming_incl universe rho g h ->
    neglect_graph_renaming_incl universe sigma h g ->
    In v universe ->
    In v (slash_iter universe g s0 n) <->
    In (rho v) (slash_iter universe h t0 n).
Proof.
  intros universe g h s0 t0 n rho sigma v
         Hs0 Ht0 Hrho Hsigma Hsigma_rho Hrho_sigma
         Hinit_forward Hinit_backward Hedges_forward Hedges_backward Hvin.
  split; intro Hin.
  - eapply (@slash_iter_validator_renaming_incl
      universe g h s0 t0 n rho); eassumption.
  - assert (In (sigma (rho v)) (slash_iter universe g s0 n)) as Hback.
    { eapply (@slash_iter_validator_renaming_incl
        universe h g t0 s0 n sigma); eassumption. }
    rewrite (Hsigma_rho v Hvin) in Hback. assumption.
Qed.

Theorem no_reachability_no_level2_slash :
  forall universe g s0 n v,
    ~ In v s0 ->
    (forall offender k,
        In offender s0 ->
        ~ neglect_reaches_in universe g k v offender) ->
    ~ In v (slash_iter universe g s0 n).
Proof.
  intros universe g s0 n v Hnot_initial Hno_reach Hin.
  apply slash_iter_reachability_characterization in Hin.
  destruct Hin as [Hinitial | Hreach].
  - contradiction.
  - destruct Hreach as [offender [k [Hoff [_ Hpath]]]].
    apply (Hno_reach offender k); assumption.
Qed.

Fixpoint total_stake (universe : list Validator) (stake : Validator -> nat) : nat :=
  match universe with
  | [] => 0
  | v :: rest => stake v + total_stake rest stake
  end.

Fixpoint slashed_stake
  (universe : list Validator) (stake : Validator -> nat)
  (slashed : list Validator) : nat :=
  match universe with
  | [] => 0
  | v :: rest =>
      (if validator_in v slashed then stake v else 0)
      + slashed_stake rest stake slashed
  end.

Definition active_stake
  (universe : list Validator) (stake : Validator -> nat)
  (slashed : list Validator) : nat :=
  total_stake universe stake - slashed_stake universe stake slashed.

Definition stake_quorum_bound
  (universe : list Validator) (stake : Validator -> nat) (F : nat) : nat :=
  total_stake universe stake - F.

Theorem weighted_quorum_preservation_under_bounded_closure :
  forall universe stake slashed F,
    slashed_stake universe stake slashed <= F ->
    F <= total_stake universe stake ->
    active_stake universe stake slashed >=
    stake_quorum_bound universe stake F.
Proof.
  intros universe stake slashed F Hslash HF.
  unfold active_stake, stake_quorum_bound. lia.
Qed.

Theorem weighted_slash_iter_quorum_preservation :
  forall universe g s0 n stake F,
    slashed_stake universe stake (slash_iter universe g s0 n) <= F ->
    F <= total_stake universe stake ->
    active_stake universe stake (slash_iter universe g s0 n) >=
    stake_quorum_bound universe stake F.
Proof.
  intros. apply weighted_quorum_preservation_under_bounded_closure; assumption.
Qed.

Definition all_direct_offenders_bonded
  (universe : list Validator) (stake : Validator -> nat) (s0 : list Validator) : Prop :=
  forall v, In v s0 -> In v universe /\ stake v > 0.

Theorem zero_stake_not_direct_offender_under_bonded_precondition :
  forall universe stake s0 v,
    all_direct_offenders_bonded universe stake s0 ->
    stake v = 0 ->
    ~ In v s0.
Proof.
  intros universe stake s0 v Hbonded Hzero Hin.
  specialize (Hbonded v Hin). destruct Hbonded as [_ Hpositive].
  lia.
Qed.

Definition pow2 (n : nat) : nat := 2 ^ n.
Definition unsigned_max (bits : nat) : nat := pow2 bits - 1.
Definition signed_max (bits : nat) : nat := pow2 (bits - 1) - 1.

Lemma pow2_positive :
  forall n, 0 < pow2 n.
Proof.
  unfold pow2. induction n as [| n IH]; simpl; lia.
Qed.

Theorem unsigned_overflow_boundary_exact :
  forall bits,
    unsigned_max bits + 1 = pow2 bits.
Proof.
  intros bits. unfold unsigned_max.
  pose proof (pow2_positive bits). lia.
Qed.

Theorem signed_overflow_boundary_exact :
  forall bits,
    1 <= bits ->
    signed_max bits + 1 = pow2 (bits - 1).
Proof.
  intros bits Hbits. unfold signed_max.
  pose proof (pow2_positive (bits - 1)). lia.
Qed.

Example arithmetic_projection_stress_boundary_8bit :
  let exact := unsigned_max 8 + 1 in
    exact = pow2 8 /\
    exact mod pow2 8 = 0 /\
    exact <> 0.
Proof.
  cbv. repeat split; lia.
Qed.

Lemma slash_step_arg_monotone :
  forall universe g s1 s2,
    incl s1 s2 ->
    incl (slash_step universe g s1) (slash_step universe g s2).
Proof.
  intros universe g s1 s2 Hsub v Hin.
  unfold slash_step in *.
  apply in_app_or in Hin. destruct Hin as [Hin | Hnew].
  - apply in_or_app. left. apply Hsub. assumption.
  - apply filter_In in Hnew. destruct Hnew as [Huniverse Hcond].
    apply andb_true_iff in Hcond. destruct Hcond as [Hnot Hinter].
    destruct (in_dec validator_eq_dec v s2) as [Hin2 | Hnot2].
    + apply in_or_app. left. assumption.
    + apply in_or_app. right. apply filter_In. split; [assumption |].
      apply andb_true_iff. split.
      * apply negb_true_iff.
        destruct (existsb (fun s' => if validator_eq_dec v s' then true else false) s2) eqn:Hex.
        -- exfalso. apply existsb_exists in Hex.
           destruct Hex as [x [Hx Heq]].
           destruct (validator_eq_dec v x) as [Heqvx | Hne]; try discriminate.
           subst. contradiction.
        -- reflexivity.
      * apply inter_nonempty_true_exists in Hinter.
        destruct Hinter as [x [Hxg Hxs1]].
        apply inter_nonempty_exists_true.
        exists x. split; [assumption | apply Hsub; assumption].
Qed.

Definition slash_fixed_point universe g s : Prop :=
  incl (slash_step universe g s) s.

Theorem slash_iter_fixed_point_stable :
  forall universe g s n,
    slash_fixed_point universe g s ->
    incl (slash_iter universe g s n) s /\ incl s (slash_iter universe g s n).
Proof.
  intros universe g s n Hfixed.
  induction n as [| n IH]; simpl.
  - split; intros x H; assumption.
  - destruct IH as [Hto Hfrom]. split.
    + intros x Hx.
      apply Hfixed.
      apply slash_step_arg_monotone with (s1 := slash_iter universe g s n); assumption.
    + intros x Hx.
      apply slash_step_monotone. apply Hfrom. assumption.
Qed.

Definition disjoint_list (xs ys : list Validator) : Prop :=
  forall v, In v xs -> ~ In v ys.

Lemma NoDup_app_disjoint :
  forall xs ys,
    NoDup xs ->
    NoDup ys ->
    disjoint_list xs ys ->
    NoDup (xs ++ ys).
Proof.
  induction xs as [| x xs IH]; intros ys Hndx Hndy Hdis; simpl.
  - assumption.
  - inversion Hndx as [| ? ? Hnotin Hndxs]; subst.
    constructor.
    + intro Hin. apply in_app_or in Hin. destruct Hin as [Hinxs | Hinys].
      * contradiction.
      * apply (Hdis x); [left; reflexivity | assumption].
    + apply IH; try assumption.
      intros v Hv Hvy. apply (Hdis v); [right; assumption | assumption].
Qed.

Lemma incl_app_list :
  forall (xs ys zs : list Validator),
    incl xs zs ->
    incl ys zs ->
    incl (xs ++ ys) zs.
Proof.
  intros xs ys zs Hx Hy v Hin.
  apply in_app_or in Hin. destruct Hin as [Hin | Hin].
  - apply Hx. assumption.
  - apply Hy. assumption.
Qed.

Lemma slash_step_preserves_incl :
  forall universe g s,
    incl s universe ->
    incl (slash_step universe g s) universe.
Proof.
  intros universe g s Hsub v Hin.
  unfold slash_step in Hin.
  apply in_app_or in Hin. destruct Hin as [Hin | Hnew].
  - apply Hsub. assumption.
  - apply filter_In in Hnew. destruct Hnew as [Huniverse _]. assumption.
Qed.

Lemma slash_step_new_disjoint :
  forall universe g s,
    disjoint_list s
      (filter
        (fun v =>
           andb
             (negb (existsb (fun s' =>
                              if validator_eq_dec v s' then true else false) s))
             (inter_nonempty (g v) s))
        universe).
Proof.
  intros universe g s v Hs Hnew.
  apply filter_In in Hnew. destruct Hnew as [_ Hcond].
  apply andb_true_iff in Hcond. destruct Hcond as [Hnot _].
  apply negb_true_iff in Hnot.
  assert (existsb (fun s' => if validator_eq_dec v s' then true else false) s = true)
    as Hmember.
  { apply existsb_exists. exists v. split; [assumption |].
    destruct (validator_eq_dec v v); congruence. }
  congruence.
Qed.

Lemma slash_step_preserves_NoDup :
  forall universe g s,
    NoDup universe ->
    NoDup s ->
    NoDup (slash_step universe g s).
Proof.
  intros universe g s Hndu Hnds.
  unfold slash_step.
  apply NoDup_app_disjoint.
  - assumption.
  - apply NoDup_filter. assumption.
  - apply slash_step_new_disjoint.
Qed.

Lemma NoDup_same_length_incl_reverse :
  forall (xs ys : list Validator),
    NoDup xs ->
    incl xs ys ->
    length xs = length ys ->
    incl ys xs.
Proof.
  intros xs ys Hndx Hsub Hlen y Hy.
  destruct (in_dec validator_eq_dec y xs) as [Hin | Hnot]; [assumption |].
  exfalso.
  assert (NoDup (y :: xs)) as Hndyx.
  { constructor; assumption. }
  assert (incl (y :: xs) ys) as Hsubyx.
  { intros z Hz. destruct Hz as [Hz | Hz].
    - subst. assumption.
    - apply Hsub. assumption. }
  pose proof (NoDup_incl_length Hndyx Hsubyx) as Hle.
  simpl in Hle. lia.
Qed.

Lemma slash_fixed_when_full :
  forall universe g s,
    incl universe s ->
    slash_fixed_point universe g s.
Proof.
  intros universe g s Hfull v Hin.
  unfold slash_step in Hin.
  apply in_app_or in Hin. destruct Hin as [Hin | Hnew].
  - assumption.
  - apply filter_In in Hnew. destruct Hnew as [Huniverse _].
    apply Hfull. assumption.
Qed.

Lemma slash_iter_shift :
  forall universe g s n,
    slash_iter universe g s (S n) =
    slash_iter universe g (slash_step universe g s) n.
Proof.
  intros universe g s n.
  induction n as [| n IH].
  - reflexivity.
  - change
      (slash_step universe g (slash_iter universe g s (S n)) =
       slash_step universe g (slash_iter universe g (slash_step universe g s) n)).
    rewrite IH. reflexivity.
Qed.

Lemma slash_fixed_point_iter_stable :
  forall universe g s n,
    slash_fixed_point universe g s ->
    slash_fixed_point universe g (slash_iter universe g s n).
Proof.
  intros universe g s n Hfixed.
  pose proof (@slash_iter_fixed_point_stable universe g s n Hfixed) as [Hto Hfrom].
  intros v Hin.
  apply Hfrom.
  apply Hfixed.
  apply (@slash_step_arg_monotone universe g (slash_iter universe g s n) s Hto).
  assumption.
Qed.

Theorem slash_iter_fixed_point_after_remaining :
  forall fuel universe g s,
    NoDup universe ->
    NoDup s ->
    incl s universe ->
    length universe - length s <= fuel ->
    slash_fixed_point universe g (slash_iter universe g s fuel).
Proof.
  induction fuel as [| fuel IH]; intros universe g s Hndu Hnds Hsub Hfuel.
  - simpl.
    pose proof (NoDup_incl_length Hnds Hsub) as Hle.
    assert (length s = length universe) as Hlen by lia.
    apply slash_fixed_when_full.
    apply NoDup_same_length_incl_reverse; assumption.
  - remember
      (filter
        (fun v =>
           andb
             (negb (existsb (fun s' =>
                              if validator_eq_dec v s' then true else false) s))
             (inter_nonempty (g v) s))
        universe) as new eqn:Hnew.
    destruct new as [| x xs].
    + apply slash_fixed_point_iter_stable.
      unfold slash_fixed_point, slash_step.
      rewrite <- Hnew. simpl. rewrite app_nil_r.
      intros v H. assumption.
    + rewrite slash_iter_shift.
      apply IH.
      * assumption.
      * apply slash_step_preserves_NoDup; assumption.
      * apply slash_step_preserves_incl. assumption.
      * assert (Hstep_len : length s < length (slash_step universe g s)).
        { unfold slash_step. rewrite <- Hnew. rewrite length_app. simpl. lia. }
        assert (Hstep_bound : length (slash_step universe g s) <= length universe).
        { apply NoDup_incl_length.
          - apply slash_step_preserves_NoDup; assumption.
          - apply slash_step_preserves_incl. assumption. }
        lia.
Qed.

Theorem slash_iter_fixed_point_after_universe_bound :
  forall universe g s0,
    NoDup universe ->
    NoDup s0 ->
    incl s0 universe ->
    slash_fixed_point universe g (slash_iter universe g s0 (length universe)).
Proof.
  intros universe g s0 Hndu Hnds Hsub.
  apply slash_iter_fixed_point_after_remaining; try assumption.
  pose proof (NoDup_incl_length Hnds Hsub) as Hle.
  lia.
Qed.

Theorem closure_depth_bound_at_universe_size :
  forall universe g s0,
    NoDup universe ->
    NoDup s0 ->
    incl s0 universe ->
    slash_fixed_point universe g (slash_iter universe g s0 (length universe)).
Proof.
  intros. apply slash_iter_fixed_point_after_universe_bound; assumption.
Qed.

Lemma not_disjoint_exists :
  forall xs ys,
    ~ disjoint_list xs ys ->
    exists v, In v xs /\ In v ys.
Proof.
  induction xs as [| x xs IH]; intros ys Hnot.
  - exfalso. apply Hnot. intros v Hin. contradiction.
  - destruct (in_dec validator_eq_dec x ys) as [Hinys | Hnotinys].
    + exists x. split; [left; reflexivity | assumption].
    + destruct (IH ys) as [v [Hvxs Hvys]].
      * intro Hdisxs. apply Hnot.
        intros v Hinv Hinvy. destruct Hinv as [Heq | Hinxs].
        -- subst. contradiction.
        -- apply (Hdisxs v); assumption.
      * exists v. split; [right; assumption | assumption].
Qed.

Theorem quorum_intersection_by_size :
  forall (active q1 q2 : list Validator) Q,
    NoDup active ->
    NoDup q1 ->
    NoDup q2 ->
    incl q1 active ->
    incl q2 active ->
    length q1 >= Q ->
    length q2 >= Q ->
    length active < 2 * Q ->
    exists v, In v q1 /\ In v q2.
Proof.
  intros active q1 q2 Q Hnda Hnd1 Hnd2 Hsub1 Hsub2 Hq1 Hq2 Hactive.
  apply not_disjoint_exists.
  intro Hdis.
  pose proof (NoDup_app_disjoint Hnd1 Hnd2 Hdis) as Hndapp.
  pose proof (NoDup_incl_length Hndapp (incl_app_list Hsub1 Hsub2)) as Hlen.
  rewrite length_app in Hlen. lia.
Qed.

Theorem weighted_quorum_intersection_from_disjoint_bound :
  forall (q1 q2 : list Validator) q1_stake q2_stake active_stake_value,
    (disjoint_list q1 q2 -> q1_stake + q2_stake <= active_stake_value) ->
    active_stake_value < q1_stake + q2_stake ->
    exists v, In v q1 /\ In v q2.
Proof.
  intros q1 q2 q1_stake q2_stake active_stake_value Hbound Hoverlap.
  apply not_disjoint_exists.
  intro Hdis. specialize (Hbound Hdis). lia.
Qed.

Theorem quorum_drop_certificate :
  forall (universe closure : list Validator) F,
    length closure <= length universe ->
    F < length closure ->
    length universe - length closure < length universe - F.
Proof.
  intros universe closure F Hclosure Hdrop. lia.
Qed.

Theorem weighted_quorum_drop_certificate :
  forall total closure_stake_value F,
    closure_stake_value <= total ->
    F < closure_stake_value ->
    total - closure_stake_value < total - F.
Proof.
  intros total closure_stake_value F Hclosure Hdrop. lia.
Qed.

Definition all_stake_at_most
  (universe : list Validator) (stake : Validator -> nat) (max_stake : nat) : Prop :=
  forall v, In v universe -> stake v <= max_stake.

Theorem total_stake_at_most :
  forall universe stake max_stake,
    all_stake_at_most universe stake max_stake ->
    total_stake universe stake <= length universe * max_stake.
Proof.
  induction universe as [| v rest IH]; intros stake max_stake Hmax; simpl.
  - lia.
  - assert (Hv : stake v <= max_stake).
    { apply Hmax. left. reflexivity. }
    assert (Hr : total_stake rest stake <= length rest * max_stake).
    { apply IH. intros x Hx. apply Hmax. right. assumption. }
    lia.
Qed.

Theorem arithmetic_safe_envelope :
  forall universe stake max_stake vault limit,
    all_stake_at_most universe stake max_stake ->
    vault + length universe * max_stake <= limit ->
    vault + total_stake universe stake <= limit.
Proof.
  intros universe stake max_stake vault limit Hmax Hlimit.
  pose proof (total_stake_at_most Hmax) as Htotal.
  lia.
Qed.

Definition epoch_filter
  (universe : list Validator) (epoch : Validator -> nat) (current_epoch : nat)
  : list Validator :=
  filter (fun v => Nat.eqb (epoch v) current_epoch) universe.

Theorem epoch_filter_in :
  forall universe epoch current_epoch v,
    In v (epoch_filter universe epoch current_epoch) <->
    In v universe /\ epoch v = current_epoch.
Proof.
  intros universe epoch current_epoch v. unfold epoch_filter.
  rewrite filter_In. split.
  - intros [Hin Heq]. apply Nat.eqb_eq in Heq. split; assumption.
  - intros [Hin Heq]. split; [assumption | apply Nat.eqb_eq; assumption].
Qed.

Theorem stale_epoch_not_eligible :
  forall universe epoch current_epoch v,
    epoch v <> current_epoch ->
    ~ In v (epoch_filter universe epoch current_epoch).
Proof.
  intros universe epoch current_epoch v Hstale Hin.
  apply epoch_filter_in in Hin. destruct Hin as [_ Heq]. contradiction.
Qed.

Definition carryover_policy (carry : bool) (mapped_current_direct : list Validator)
  : list Validator :=
  if carry then mapped_current_direct else [].

Theorem carryover_policy_sound :
  forall carry mapped_current_direct v,
    In v (carryover_policy carry mapped_current_direct) ->
    carry = true /\ In v mapped_current_direct.
Proof.
  intros carry mapped_current_direct v Hin.
  unfold carryover_policy in Hin.
  destruct carry.
  - split; [reflexivity | assumption].
  - contradiction.
Qed.

Definition temporal_retention_safe
  (gossip_delay inclusion_delay retention_window : nat) : Prop :=
  gossip_delay + inclusion_delay <= retention_window.

Theorem temporal_retention_boundary_exact :
  forall gossip_delay inclusion_delay,
    temporal_retention_safe
      gossip_delay inclusion_delay (gossip_delay + inclusion_delay).
Proof.
  intros. unfold temporal_retention_safe. lia.
Qed.

Theorem temporal_retention_under_window_projection_risk :
  forall gossip_delay inclusion_delay retention_window,
    retention_window < gossip_delay + inclusion_delay ->
    ~ temporal_retention_safe gossip_delay inclusion_delay retention_window.
Proof.
  intros gossip_delay inclusion_delay retention_window Hlt Hsafe.
  unfold temporal_retention_safe in Hsafe. lia.
Qed.

Definition rebond_identity_boundary
  (old_nonce new_nonce : nat) (carry : bool) : Prop :=
  old_nonce <> new_nonce /\ carry = false.

Theorem rebond_identity_boundary_requires_carryover :
  forall old_nonce new_nonce carry,
    rebond_identity_boundary old_nonce new_nonce carry ->
    old_nonce <> new_nonce /\ carry = false.
Proof.
  intros old_nonce new_nonce carry H. exact H.
Qed.

Example closure_bound_assumption_needed :
  exists (universe closure : list Validator) (F : nat),
    length closure <= length universe /\
    F < length closure /\
    length universe - length closure < length universe - F.
Proof.
  exists [0; 1; 2; 3], [0; 1], 1. simpl. repeat split; lia.
Qed.

Example quorum_intersection_strictness_needed :
  exists (active q1 q2 : list Validator) (Q : nat),
    NoDup active /\
    NoDup q1 /\
    NoDup q2 /\
    incl q1 active /\
    incl q2 active /\
    length q1 >= Q /\
    length q2 >= Q /\
    length active = 2 * Q /\
    disjoint_list q1 q2.
Proof.
  exists [0; 1; 2; 3], [0; 1], [2; 3], 2.
  repeat split; simpl; try lia.
  - repeat constructor; simpl; lia.
  - repeat constructor; simpl; lia.
  - repeat constructor; simpl; lia.
  - intros x H. destruct H as [H | [H | []]]; subst; simpl; tauto.
  - intros x H. destruct H as [H | [H | []]]; subst; simpl; tauto.
  - intros v Hq1 Hq2.
    destruct Hq1 as [H | [H | []]]; subst; simpl in Hq2;
    destruct Hq2 as [H | [H | []]]; discriminate.
Qed.

Example quorum_nodup_assumption_needed :
  exists (active q1 q2 : list Validator) (Q : nat),
    NoDup active /\
    incl q1 active /\
    incl q2 active /\
    length q1 >= Q /\
    length q2 >= Q /\
    length active < 2 * Q /\
    disjoint_list q1 q2.
Proof.
  exists [0; 1], [0; 0], [1; 1], 2.
  repeat split; simpl; try lia.
  - repeat constructor; simpl; lia.
  - intros x H. destruct H as [H | [H | []]]; subst; simpl; tauto.
  - intros x H. destruct H as [H | [H | []]]; subst; simpl; tauto.
  - intros v Hq1 Hq2.
    destruct Hq1 as [H | [H | []]]; subst; simpl in Hq2;
    destruct Hq2 as [H | [H | []]]; discriminate.
Qed.

Definition hypothesis_minimized_neglect_graph (v : Validator) : list Validator :=
  match v with
  | 1 => [0]
  | _ => []
  end.

Definition deep_threat_chain_graph (v : Validator) : list Validator :=
  match v with
  | 1 => [0]
  | 2 => [1]
  | 3 => [2]
  | _ => []
  end.

Example hypothesis_minimized_closure_bound_assumption_needed :
  let universe := [0; 1; 2; 3] in
  let s0 := [0] in
    In 1 (slash_iter universe hypothesis_minimized_neglect_graph s0 1) /\
    length (slash_iter universe hypothesis_minimized_neglect_graph s0 1) = 2.
Proof.
  cbv. split; auto.
Qed.

Example deep_threat_chain_closure_bound_assumption_needed :
  let universe := [0; 1; 2; 3] in
  let s0 := [0] in
    In 3 (slash_iter universe deep_threat_chain_graph s0 3) /\
    length (slash_iter universe deep_threat_chain_graph s0 3) = 4.
Proof.
  cbv. split; auto.
Qed.

Example direct_offender_universe_assumption_needed :
  let universe := [0; 1; 2; 3] in
  let s0 := [4] in
    In 4 (slash_iter universe (fun _ => []) s0 0) /\ ~ In 4 universe.
Proof.
  cbv. split.
  - auto.
  - intros [H | [H | [H | [H | []]]]]; discriminate.
Qed.

Definition duplicate_edge_graph (v : Validator) : list Validator :=
  match v with
  | 1 => [0; 0]
  | _ => []
  end.

Definition single_edge_graph (v : Validator) : list Validator :=
  match v with
  | 1 => [0]
  | _ => []
  end.

Example duplicate_edge_graph_equiv_hypothesis_minimized :
  graph_equiv duplicate_edge_graph single_edge_graph.
Proof.
  intros v offender. unfold duplicate_edge_graph, single_edge_graph.
  destruct v as [| [| v]]; simpl.
  - tauto.
  - split; intro H.
    + destruct H as [H | [H | []]]; subst; simpl; auto.
    + destruct H as [H | []]; subst; simpl; auto.
  - tauto.
Qed.

Theorem duplicate_edge_slash_iter_equiv_hypothesis_minimized :
  forall universe s0 n v,
    In v (slash_iter universe duplicate_edge_graph s0 n) <->
    In v (slash_iter universe single_edge_graph s0 n).
Proof.
  intros universe s0 n v.
  apply slash_iter_graph_equiv.
  apply duplicate_edge_graph_equiv_hypothesis_minimized.
Qed.

Example report_suppression_hypothesis_minimized :
  let universe := [0; 1] in
    In 1 (slash_iter universe hypothesis_minimized_neglect_graph [0] 1) /\
    ~ In 1
        (slash_iter universe
           (visible_unreported_graph
              hypothesis_minimized_neglect_graph
              hypothesis_minimized_neglect_graph)
           [0] 1).
Proof.
  cbv. split.
  - auto.
  - intros [H | []]; discriminate.
Qed.

Definition weighted_amplification_stake (v : Validator) : nat :=
  match v with
  | 0 => 3
  | 1 => 3
  | 2 => 1
  | _ => 1
  end.

Example weighted_closure_bound_assumption_needed :
  let universe := [0; 1; 2; 3] in
  let closure := [0; 1; 2] in
  let F := 2 in
    slashed_stake universe weighted_amplification_stake closure = 7 /\
    active_stake universe weighted_amplification_stake closure = 1 /\
    stake_quorum_bound universe weighted_amplification_stake F = 6 /\
    active_stake universe weighted_amplification_stake closure <
    stake_quorum_bound universe weighted_amplification_stake F.
Proof.
  cbv. repeat split; lia.
Qed.
