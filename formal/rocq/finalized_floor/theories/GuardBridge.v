(* ===========================================================================
   GuardBridge.v - the committee-constancy guard DERIVES Floor.v's `AdjDC`
   premise from L-ANC, closing the "Rocq assumes what the Rust enforces" seam.

   `Floor.frontier_cache_transparent` takes `AdjDC band` (adjacent downward-closure
   of the finalization flags) as a HYPOTHESIS. The Rust `incremental_frontier` does
   not get that for free: it holds only when the committee is CONSTANT across the
   band, which the Rust ENFORCES with the committee-constancy guard
   (`floor.rs` — `get_corresponding_weight_map` compared across the band; on a
   mismatch it falls back to the cold walk). This module proves that, under a
   single (constant) committee `c`, finalization along a main-parent chain IS
   downward-closed — i.e. the `AdjDC` premise is DERIVED from `L_ANC`, not assumed.

   `finb` is the finalization DECISION (the Rust `ft_witnessed > t` bool); it is a
   Section parameter with `finb_spec` reflecting it into `CliqueOracle.Finalized`
   for the fixed committee `c` (finalization is decidable — finite committee /
   finite sub-committees — so a correct decision procedure exists; keeping it
   parametric with `finb_spec` introduces NO axiom, it becomes a premise).

   ---------------------------------------------------------------------------
   Rocq                         | Rust
   -----------------------------+--------------------------------------------
   c (single committee)         | committee-constancy guard (floor.rs)
   finb / finb_spec             | ft_witnessed > ft_threshold (the decision)
   chain_adj                    | main_parent_chain band (bottom->top)
   chain_adj_AdjDC              | guard establishes Floor.v's AdjDC premise
   guard_constant_committee_transparent | warm up-walk == cold walk under the guard
   =========================================================================== *)

From Stdlib Require Import Lists.List.
From Stdlib Require Import ZArith.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.
From FinalizedFloor Require Import Floor.

Section Bridge.
  Variable d : DAG.
  Variable c : Committee.               (* the SINGLE committee — constancy across the band *)
  Variable J : Snapshot.
  Variable finb : BlockHash -> bool.    (* the finalization decision (ft_witnessed > t) *)
  Hypothesis finb_spec : forall b, finb b = true <-> Finalized d c J b.

  (* Band bottom->top; consecutive blocks are ancestry-adjacent — the lower block
     is a DAG-ancestor of the upper (the main-parent spine). *)
  Fixpoint chain_adj (l : list BlockHash) : Prop :=
    match l with
    | [] => True
    | lo :: rest =>
        match rest with
        | [] => True
        | up :: _ => anc_of d lo up /\ chain_adj rest
        end
    end.

  Definition flagged (l : list BlockHash) : list Entry :=
    map (fun b => (b, finb b)) l.

  (* Constant committee (single `c`) + L-ANC ⇒ the finalization flags along the
     band are downward-closed (AdjDC): if the upper block is finalized, so is the
     lower. This is exactly the premise Floor.v assumes — now derived. *)
  Lemma chain_adj_AdjDC : forall l, chain_adj l -> AdjDC (flagged l).
  Proof.
    induction l as [| lo tl IH]; intros Hchain.
    - apply adc_nil.
    - destruct tl as [| up rest].
      + apply adc_one.
      + simpl in Hchain. destruct Hchain as [Hanc Hrest].
        simpl. apply adc_cons.
        * intro Hup. apply finb_spec.
          apply finb_spec in Hup.
          exact (L_ANC d c J up lo Hanc Hup).
        * exact (IH Hrest).
  Qed.

  (* The bridge capstone: for a main-parent-chain band under a constant committee,
     the warm up-walk from a finalized pivot equals the cold walk
     (Floor.frontier_cache_transparent) — with the `AdjDC` premise DERIVED here,
     not assumed. So the Rust guard (constant committee) is exactly what makes the
     frontier cache transparent (no fork). *)
  Theorem guard_constant_committee_transparent :
    forall band pivot,
      chain_adj band ->
      lastTrue ((pivot, true) :: flagged band) = Some (upgo pivot (flagged band)).
  Proof.
    intros band pivot Hchain.
    apply frontier_cache_transparent.
    apply chain_adj_AdjDC. exact Hchain.
  Qed.

  (* ---- T-FIN: the frontier is FINALIZED -------------------------------------
     The warm up-walk starts at the finalized pivot and only advances to blocks
     whose flag is true (= finalized), so its result is always finalized. Hence a
     floor selected among such frontiers is finalized (the T-FIN premise
     `Forall Finalized cands` that Selection.select_finalized consumes is
     discharged here, no longer merely assumed). *)
  Lemma upgo_flag_true :
    forall l pivot, finb pivot = true -> finb (upgo pivot (flagged l)) = true.
  Proof.
    induction l as [| h rest IH]; intros pivot Hpiv.
    - simpl. exact Hpiv.
    - simpl. destruct (finb h) eqn:Efh.
      + apply IH. exact Efh.
      + exact Hpiv.
  Qed.

  Theorem upgo_finalized :
    forall l pivot, finb pivot = true -> Finalized d c J (upgo pivot (flagged l)).
  Proof.
    intros l pivot Hpiv. apply finb_spec. apply upgo_flag_true. exact Hpiv.
  Qed.
End Bridge.

(* ===========================================================================
   BridgeFt - T-CACHE DIRECTLY over the θ-exact `Finalized_ft`, for ALL num
   (including the DEFAULT θ = 0 and the negative-θ sentinels, i.e. θ ≤ 0).

   Section `Bridge` routes cache transparency through the strict-majority
   `Finalized`; for negative thresholds the θ-exact test alone does not imply that
   proxy. But the frontier cache needs only that finalization is downward-closed
   along the spine (`AdjDC`), and `CliqueOracle.L_ANC_ft` delivers that for
   `Finalized_ft` at ANY num. So we re-instantiate the SAME Floor.v machinery with
   a decision `finb_ft` reflecting `Finalized_ft` and obtain T-CACHE for all θ —
   with NO num>0 restriction and WITHOUT the strict-majority bridge. This is the
   θ ≤ 0 coverage of the cache: the default θ = 0 and the negative-θ sentinels are
   transparently cached exactly as θ > 0. (`chain_adj` depends only on the DAG, so
   the `chain_adj d` from Section Bridge is reused verbatim.)
   =========================================================================== *)
Section BridgeFt.
  Variable d : DAG.
  Variable c : Committee.
  Variable J : Snapshot.
  Variables num den : Z.
  Variable finb_ft : BlockHash -> bool.   (* the θ-exact decision (ft_witnessed) *)
  Hypothesis finb_ft_spec :
    forall b, finb_ft b = true <-> Finalized_ft d c J b num den.

  Definition flagged_ft (l : list BlockHash) : list Entry :=
    map (fun b => (b, finb_ft b)) l.

  (* Constant committee (single `c`) + L-ANC for the θ-EXACT test ⇒ the flags are
     downward-closed (AdjDC), at ANY num — including θ ≤ 0. *)
  Lemma chain_adj_AdjDC_ft : forall l, chain_adj d l -> AdjDC (flagged_ft l).
  Proof.
    induction l as [| lo tl IH]; intros Hchain.
    - apply adc_nil.
    - destruct tl as [| up rest].
      + apply adc_one.
      + simpl in Hchain. destruct Hchain as [Hanc Hrest].
        simpl. apply adc_cons.
        * intro Hup. apply finb_ft_spec. apply finb_ft_spec in Hup.
          exact (L_ANC_ft d c J up lo num den Hanc Hup).
        * exact (IH Hrest).
  Qed.

  (* T-CACHE over the θ-exact test, for ALL num: the warm up-walk from a θ-finalized
     pivot equals the cold walk — no num>0, no strict-majority bridge. *)
  Theorem guard_constant_committee_transparent_ft :
    forall band pivot,
      chain_adj d band ->
      lastTrue ((pivot, true) :: flagged_ft band) = Some (upgo pivot (flagged_ft band)).
  Proof.
    intros band pivot Hchain.
    apply frontier_cache_transparent.
    apply chain_adj_AdjDC_ft. exact Hchain.
  Qed.

  Lemma upgo_flag_true_ft :
    forall l pivot, finb_ft pivot = true -> finb_ft (upgo pivot (flagged_ft l)) = true.
  Proof.
    induction l as [| h rest IH]; intros pivot Hpiv.
    - simpl. exact Hpiv.
    - simpl. destruct (finb_ft h) eqn:Efh.
      + apply IH. exact Efh.
      + exact Hpiv.
  Qed.

  (* The frontier is θ-FINALIZED (T-FIN over the exact test), for ALL num. *)
  Theorem upgo_finalized_ft :
    forall l pivot, finb_ft pivot = true ->
      Finalized_ft d c J (upgo pivot (flagged_ft l)) num den.
  Proof.
    intros l pivot Hpiv. apply finb_ft_spec. apply upgo_flag_true_ft. exact Hpiv.
  Qed.
End BridgeFt.
