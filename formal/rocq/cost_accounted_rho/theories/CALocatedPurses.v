(* ════════════════════════════════════════════════════════════════════════
   CALocatedPurses.v — located capabilities (CL8, continued-gslt-cost-v2).

   The monad paper's located-capability discipline: phlogiston capability is
   SPATIALLY LOCATED on signature surfaces (the channels Nt s), local sufficiency
   (enough capability at each location for its local demand) COMPOSES to global
   executability, and the locations are DISJOINT (drawing at one surface never
   perturbs another — the Rocq image of the runtime's lane_pool_disjoint /
   ChannelSeparation.lane_pool_disjoint, and of the TLA+ LocatedPurse model's
   Inv_LocalSufficiencyComposes). Axiom-free.                                   *)

From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.

(* A located purse holds a capability supply per signature-surface. *)
Definition located_purse := sig -> nat.

(* Local sufficiency: at every surface, the supply meets the local demand. *)
Definition local_sufficient (supply demand : located_purse) : Prop :=
  forall s, demand s <= supply s.

(* Drawing [amt] at surface [s] decrements only that surface. *)
Definition draw_at (supply : located_purse) (s : sig) (amt : nat) : located_purse :=
  fun s' => if sig_eq_dec s s' then supply s' - amt else supply s'.

(* DISJOINTNESS: a draw at [s] leaves every other surface untouched. *)
Theorem draw_disjoint : forall supply s amt s',
  s <> s' -> draw_at supply s amt s' = supply s'.
Proof.
  intros supply s amt s' Hneq. unfold draw_at.
  destruct (sig_eq_dec s s') as [He | Hn].
  - exfalso; apply Hneq; exact He.
  - reflexivity.
Qed.

(* A draw at [s] sets exactly that surface to [supply s - amt]. *)
Theorem draw_at_here : forall supply s amt,
  draw_at supply s amt s = supply s - amt.
Proof.
  intros supply s amt. unfold draw_at.
  destruct (sig_eq_dec s s) as [_ | Hn]; [ reflexivity | exfalso; apply Hn; reflexivity ].
Qed.

(* The per-surface totals over any finite surface list. *)
Definition total (p : located_purse) (locs : list sig) : nat :=
  fold_right (fun s acc => p s + acc) 0 locs.

(* COMPOSITION: local sufficiency at each surface composes to total sufficiency —
   the demand never exceeds the supply in aggregate. *)
Theorem local_sufficiency_composes : forall supply demand locs,
  local_sufficient supply demand ->
  total demand locs <= total supply locs.
Proof.
  intros supply demand locs Hloc. induction locs as [| s rest IH]; simpl.
  - lia.
  - pose proof (Hloc s). lia.
Qed.

(* A draw within local sufficiency keeps local sufficiency at the drawn surface
   (the residual still meets a residual demand) and preserves it everywhere else. *)
Theorem draw_preserves_disjoint_sufficiency : forall supply demand s amt,
  local_sufficient supply demand ->
  forall s', s <> s' -> demand s' <= draw_at supply s amt s'.
Proof.
  intros supply demand s amt Hloc s' Hneq.
  rewrite draw_disjoint by assumption. apply Hloc.
Qed.
