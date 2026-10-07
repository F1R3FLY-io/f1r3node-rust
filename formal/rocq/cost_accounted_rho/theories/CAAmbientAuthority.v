(* ════════════════════════════════════════════════════════════════════════
   CAAmbientAuthority.v — CA-P-179: no ambient authority under a non-free K
   (continued-gslt-cost-v2 §"The leak of ambient authority", sec:ac-leak,
   :1299-1327; §"Located resource stacks", :1370-1397).

   Under a non-free K (rho's K = ∥ is AC, the maximal case), POSITION is no
   longer exclusive: an equation on K "lets a program drift into contact with a
   stack it did not structurally neighbour" (:1303-1304), so in the freely-mixed
   soup "every program [is] adjacent to every stack" (:1309-1310). Hence "Mere
   adjacency can no longer be the capability… Taking proximity as authority here
   is exactly AMBIENT AUTHORITY, and it is a leak" (:1322-1325).

   The recovery (:1370-1386): a stack is LOCATED at an interaction surface,
   S(I, s∷S'), "a stack indexed by the surface I that may draw on it… authority
   over a resource is NAMED IN THE TERM rather than conferred by the geometry of
   the bag." So the right to draw is carried by a MATCHING SURFACE, never by
   co-location.

   This module mechanizes that security claim over the located-purse model of
   CALocatedPurses (the Rocq image of the runtime lane_pool_disjoint and the
   TLA+ LocatedPurse model). The content: a draw moves supply at a surface IFF
   the draw is TARGETED at that surface (the drawer names a matching surface);
   merely being a different, co-located surface confers nothing. We COMPOSE the
   already-proven located-purse lemmas (draw_at_here / draw_disjoint /
   local_sufficiency_composes); we do not re-prove them. Axiom-free.            *)

From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CALocatedPurses.

(* ── Drawing affects exactly the named surface ──────────────────────────────
   A draw is parameterized by the surface [d] it TARGETS. "Authority" over a
   surface [s] is the ability to change the supply located there. The located
   model says drawing at [d] changes the supply at [s] iff [d = s]: authority
   is conferred by NAMING the surface, never by adjacency of a different one. *)

(* The surface a draw actually perturbs is exactly its target: a draw at [d]
   moves the supply at [s] (strictly, when there is fuel to draw) ONLY when
   [s = d]. Off the target the supply is untouched (draw_disjoint). *)
Theorem draw_authority_requires_named_surface : forall supply d amt s,
  draw_at supply d amt s <> supply s -> s = d.
Proof.
  intros supply d amt s Hchanged.
  destruct (sig_eq_dec d s) as [Heq | Hneq].
  - subst d. reflexivity.
  - exfalso. apply Hchanged. apply draw_disjoint. exact Hneq.
Qed.

(* Contrapositive, the "adjacency confers nothing" face: a draw aimed at a
   DIFFERENT surface [d] leaves surface [s] exactly as it was — proximity in the
   bag does not let a draw reach a neighbouring located stack. *)
Theorem adjacency_confers_no_authority : forall supply d amt s,
  d <> s -> draw_at supply d amt s = supply s.
Proof.
  intros supply d amt s Hneq. apply draw_disjoint. exact Hneq.
Qed.

(* ── The located surface as the carrier of the right to draw ─────────────────
   We make "holding a matching surface" precise as a [located_surface]: a draw
   request bundled with the surface it names. A request [mk_draw d amt] is
   AUTHORIZED to draw at surface [s] iff its named surface matches, [d = s]. *)

Record located_draw : Type := mk_draw {
  draw_surface : sig;    (* the surface the holder names (its capability) *)
  draw_amount  : nat     (* how much it asks to draw *)
}.

(* A draw is authorized at surface [s] iff it names that very surface. *)
Definition authorized_at (r : located_draw) (s : sig) : Prop :=
  draw_surface r = s.

(* Applying an authorized draw at its surface decrements exactly there
   (draw_at_here), and nowhere else (draw_disjoint) — the located semantics. *)
Definition apply_draw (supply : located_purse) (r : located_draw) : located_purse :=
  draw_at supply (draw_surface r) (draw_amount r).

(* CA-P-179 headline — no ambient authority: whenever a draw actually moves the
   supply located at surface [s] (exercises authority there), the drawer MUST
   hold a matching surface located at [s] (it named [s]); authority is never
   conferred by mere adjacency. The right to draw is carried by a NAMED matching
   surface, exactly as the located form requires. *)
Theorem no_ambient_authority : forall supply r s,
  apply_draw supply r s <> supply s ->
  authorized_at r s.
Proof.
  intros supply r s Hexercised.
  unfold apply_draw, authorized_at in *.
  (* The perturbed surface is exactly the drawn (named) surface. *)
  symmetry. apply (draw_authority_requires_named_surface supply (draw_surface r)
                     (draw_amount r) s). exact Hexercised.
Qed.

(* The converse face: an authorized draw at its matching surface does exercise
   authority there precisely when there is fuel and a positive request — i.e. a
   matching surface is what LICENSES the draw. (Stated as the located
   decrement: at the matching surface the supply becomes [supply s - amt].) *)
Theorem matching_surface_licenses_draw : forall supply r s,
  authorized_at r s ->
  apply_draw supply r s = supply s - draw_amount r.
Proof.
  intros supply r s Hauth. unfold apply_draw, authorized_at in *.
  subst s. apply draw_at_here.
Qed.

(* ── Composition with sufficiency: authority is per-surface and separating ───
   The located discipline factors resource-sufficiency into a separating
   conjunction of per-surface proofs (:1618-1620). We surface that here: if the
   located supply is locally sufficient for a located demand, then a draw aimed
   at one surface leaves the sufficiency of every OTHER surface intact —
   authority does not bleed across surfaces (the separating-conjunction face). *)
Theorem authority_separates_across_surfaces : forall supply demand r s,
  local_sufficient supply demand ->
  draw_surface r <> s ->
  demand s <= apply_draw supply r s.
Proof.
  intros supply demand r s Hloc Hneq. unfold apply_draw.
  apply draw_preserves_disjoint_sufficiency; assumption.
Qed.

(* And the aggregate sufficiency still composes from the per-surface supplies
   (local_sufficiency_composes), so naming authority per surface is compatible
   with global executability — the located form is sound, not just safe. *)
Theorem located_authority_composes : forall supply demand locs,
  local_sufficient supply demand ->
  total demand locs <= total supply locs.
Proof.
  exact local_sufficiency_composes.
Qed.
