(* ===========================================================================
   Floor.v - Frontier-cache TRANSPARENCY (T-CACHE): the Phase-2 warm up-walk
   returns the identical block the cold down-walk would.

   The Phase-2 fix (floor.rs) resolves a parent's finalized frontier two ways:

     COLD  (cold_parent_frontier): scan the main-parent spine from the top
           (`parent`) downward, one clique-oracle call per step, return the
           FIRST witnessed-finalized block. This is the original algorithm.

     WARM  (incremental_frontier): read the cached pivot F(parent), then UP-walk
           the spine from the pivot toward `parent`, advancing while each block
           stays finalized, return the HIGHEST finalized block.

   The cache is sound only if these AGREE for every input (else two nodes -- one
   warm, one cold -- could derive different floors and FORK). This module proves
   they agree.

   Why they agree: along the spine, finalization is DOWNWARD-CLOSED (L-ANC,
   CliqueOracle.v): if a block is finalized then its main parent is finalized.
   Reading the band bottom-to-top from a finalized pivot, the finalization flags
   are therefore `true..true false..false` -- trues (up to the frontier) then
   falses. Call that shape `TruesThenFalses`. On such a band:
     * the cold walk (first finalized from the TOP) returns the topmost `true`;
     * the warm up-walk (advance from the pivot while `true`) returns the topmost
       `true` of the contiguous run it climbs;
   and these are the same block -- the boundary between the trues and the falses.

   We model the band bottom-to-top as `list (BlockHash * bool)` (block, is-final),
   `upgo` as the warm up-walk, and `lastTrue` as the cold walk's result (the
   topmost `true`, i.e. the first `true` seen scanning from the top). The
   capstone `frontier_cache_transparent` shows warm = cold whenever the band's
   flags satisfy adjacent downward-closure -- exactly what L-ANC delivers for
   consecutive spine blocks.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                        | Rust Implementation (floor.rs)
   ----------------------------+--------------------------------------------
   upgo best band              | incremental_frontier up-walk loop (best/advance)
   lastTrue band               | cold_parent_frontier first-finalized result
   AdjDC                       | L-ANC applied to each (block, main_parent) pair
   frontier_cache_transparent  | warm frontier == cold frontier (no fork)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.

(* A band entry: a spine block paired with its finalized flag over a fixed
   snapshot/committee. Bands are ordered BOTTOM->TOP (the pivot is the head; each
   later element is its spine child, one step closer to `parent`/the tip). *)
Definition Entry := (BlockHash * bool)%type.

(* ---------------------------------------------------------------------------
   The two walks.
   --------------------------------------------------------------------------- *)

(* WARM up-walk: from the pivot `best`, climb the band (bottom->top) advancing
   the answer while the next block is finalized; stop at the first non-finalized.
   Mirrors incremental_frontier's `for candidate in spine.rev() { if ft>=t {best=..} else break }`. *)
Fixpoint upgo (best : BlockHash) (band : list Entry) : BlockHash :=
  match band with
  | [] => best
  | (h, f) :: rest => if f then upgo h rest else best
  end.

(* COLD result: the topmost finalized block = the last `true` scanning
   bottom->top = the first `true` the cold down-walk meets scanning from the top. *)
Fixpoint lastTrue (band : list Entry) : option BlockHash :=
  match band with
  | [] => None
  | (h, f) :: rest =>
      match lastTrue rest with
      | Some x => Some x
      | None => if f then Some h else None
      end
  end.

(* ---------------------------------------------------------------------------
   Flag shapes: all-false, and trues-then-falses (what L-ANC yields).
   --------------------------------------------------------------------------- *)

Inductive AllFalse : list Entry -> Prop :=
  | af_nil  : AllFalse []
  | af_cons : forall h r, AllFalse r -> AllFalse ((h, false) :: r).

Inductive TruesThenFalses : list Entry -> Prop :=
  | ttf_falses : forall l, AllFalse l -> TruesThenFalses l
  | ttf_true   : forall h r, TruesThenFalses r -> TruesThenFalses ((h, true) :: r).

(* On an all-false band both walks are trivial: cold finds nothing, warm never
   advances past the pivot. *)
Lemma allfalse_lastTrue_none : forall l, AllFalse l -> lastTrue l = None.
Proof.
  intros l H. induction H as [| h r Hr IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma allfalse_upgo_best : forall l best, AllFalse l -> upgo best l = best.
Proof.
  (* The head flag is false, so `upgo` returns `best` immediately; the recursion
     is never entered (no IH needed). *)
  intros l best H. revert best. induction H as [| h r Hr IH]; intros best; simpl; reflexivity.
Qed.

(* A trues-then-falses band whose head is false is in fact all-false: once the
   flags turn false (here at the head) they stay false. *)
Lemma ttf_head_false_allfalse :
  forall h r, TruesThenFalses ((h, false) :: r) -> AllFalse ((h, false) :: r).
Proof.
  (* inversion discharges the ttf_true case automatically (its head flag `true`
     cannot unify with `false`), leaving only the ttf_falses case. *)
  intros h r H. inversion H; subst; assumption.
Qed.

(* One-step unfolding of lastTrue on a cons, so the proofs rewrite exactly one
   layer without simpl over-reducing the recursive call. *)
Lemma lastTrue_cons :
  forall x b rest,
    lastTrue ((x, b) :: rest) =
      match lastTrue rest with
      | Some y => Some y
      | None => if b then Some x else None
      end.
Proof. reflexivity. Qed.

(* ===========================================================================
   T-CACHE core: on a trues-then-falses band, the WARM up-walk from a finalized
   pivot returns the SAME block as the COLD walk (the topmost finalized).
   =========================================================================== *)

Theorem warm_eq_cold :
  forall band, TruesThenFalses band ->
  forall pivot, lastTrue ((pivot, true) :: band) = Some (upgo pivot band).
Proof.
  intros band Httf. induction Httf as [l Haf | h r Hr IH]; intros pivot.
  - (* band all-false: cold sees only the pivot; warm never advances. *)
    rewrite lastTrue_cons.
    rewrite (allfalse_lastTrue_none l Haf).
    rewrite (allfalse_upgo_best l pivot Haf).
    reflexivity.
  - (* band = (h,true)::r : cold descends into the tail (IH with pivot := h),
       warm climbs into it (upgo pivot ((h,true)::r) = upgo h r). *)
    rewrite lastTrue_cons.
    rewrite (IH h).
    reflexivity.
Qed.

(* ===========================================================================
   Linking to L-ANC: adjacent downward-closure gives the TruesThenFalses shape.

   `AdjDC` states finalization is downward-closed between CONSECUTIVE spine
   blocks (bottom->top): if the upper block is finalized, so is the lower. This
   is precisely L-ANC (CliqueOracle.L_ANC_mainparent) applied to each block and
   its main parent. Any band satisfying it has the trues-then-falses shape.
   =========================================================================== *)

Inductive AdjDC : list Entry -> Prop :=
  | adc_nil  : AdjDC []
  | adc_one  : forall e, AdjDC [e]
  | adc_cons : forall h1 f1 h2 f2 r,
      (f2 = true -> f1 = true) ->
      AdjDC ((h2, f2) :: r) ->
      AdjDC ((h1, f1) :: (h2, f2) :: r).

Lemma adjdc_truesthenfalses : forall band, AdjDC band -> TruesThenFalses band.
Proof.
  intros band H. induction H as [| e | h1 f1 h2 f2 r Himp Hrest IH].
  - apply ttf_falses, af_nil.
  - (* singleton *)
    destruct e as [h f]. destruct f.
    + apply ttf_true, ttf_falses, af_nil.
    + apply ttf_falses, af_cons, af_nil.
  - (* (h1,f1)::(h2,f2)::r, with f2 -> f1, and IH : TTF ((h2,f2)::r) *)
    destruct f1.
    + apply ttf_true. exact IH.
    + (* f1 = false, so f2 = false (contrapositive), so the tail is all-false *)
      assert (Hf2 : f2 = false).
      { destruct f2; [ symmetry; apply Himp; reflexivity | reflexivity ]. }
      subst f2.
      apply ttf_falses, af_cons.
      apply (ttf_head_false_allfalse h2 r IH).
Qed.

(* ===========================================================================
   T-CACHE capstone: the frontier cache is TRANSPARENT.

   For any spine band that (i) begins at a finalized pivot and (ii) is
   downward-closed between consecutive blocks (L-ANC), the WARM up-walk
   (`upgo pivot band`) equals the COLD walk's result (`lastTrue` of the
   pivot-prefixed band = the topmost finalized block). Hence enabling the
   frontier cache cannot change a node's derived floor -- no fork.
   =========================================================================== *)

Theorem frontier_cache_transparent :
  forall pivot band,
    AdjDC band ->
    lastTrue ((pivot, true) :: band) = Some (upgo pivot band).
Proof.
  intros pivot band Hadc.
  apply warm_eq_cold.
  apply adjdc_truesthenfalses. exact Hadc.
Qed.
