(* ════════════════════════════════════════════════════════════════════════
   CANearMatch.v — CA-P-178: the generic nearness gate near(I,J) on the rho
   object (continued-gslt-cost-v2 §"The nominal surface", :1329-1347 + the
   R1/R2/R3 premise L = near(I,J), :440/:466).

   The monad paper gates every metered cut on a NEARNESS operator near(I,J):
   "interaction is gated on its being defined — the gated rules of
   Section [wrapped] fire only when near(I,J) yields a surface, and that
   surface locates the purse" (:1339-1342). The operator specialises across
   the spectrum (:1344-1347): complementarity in CCS, INTERFACE AGREEMENT in
   the interaction categories, and — for rho/π — "NAME-EQUALITY of subjects".
   In the rho instance the receiver's surface is I ∼ (y,x) and the eliminand's
   is J ∼ x, so (Remark "Naming coincidences", :1388-1396) near(I,J) coincides
   with CHANNEL-NAME MATCHING: "the channel already does surface-like work."

   USER-CLARIFIED SEMANTICS for this rho instance: nearness means the send and
   the receive act on the SAME CHANNEL — same in terms of channel IDENTITY. So
   [near I J = Some _] iff I and J are the same [caname], [None] otherwise.

   This is a faithful MON-level (categorical, surface-only) restatement of the
   channel-identity match the native operational rules ALREADY enforce: in
   [ca_rule1] (CAReduction) the input [CPInput x T] and output [CPOutput x U]
   reuse the SAME bound channel variable [x], so the COMM pattern fires exactly
   when the two prefixes name one channel. We do NOT re-derive the operational
   semantics; we read the already-proven rho gate (CA-P-019…023, here the
   determinism inversion of [ca_rule1]) through [near] and show the metered cut
   fires IFF [near I J] is defined and BLOCKS (no step) when it is [None].
   Axiom-free.                                                                  *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.

(* ── The nearness operator on the rho object ────────────────────────────────
   A "surface" in the rho instance is a channel [caname]; the located surface
   the gate returns is that channel (the purse is located at it, :1342). The
   operator is defined (returns the meeting surface) exactly when the two
   surfaces are the SAME channel — channel identity — via the decidable
   [caname_eq_dec] of CASyntax. *)

Definition near (I J : caname) : option caname :=
  match caname_eq_dec I J with
  | left _  => Some I        (* defined: the surface at which I and J meet *)
  | right _ => None          (* undefined: distinct channels do not meet *)
  end.

(* near is DEFINED iff the two surfaces are channel-identical. *)
Lemma near_defined_iff_same_channel : forall I J,
  (exists L, near I J = Some L) <-> I = J.
Proof.
  intros I J. unfold near. split.
  - intros [L HL]. destruct (caname_eq_dec I J) as [Heq | Hneq].
    + exact Heq.
    + discriminate HL.
  - intro Heq. destruct (caname_eq_dec I J) as [_ | Hneq].
    + exists I. reflexivity.
    + exfalso. apply Hneq. exact Heq.
Qed.

(* When defined, near returns the common channel as the located surface. *)
Lemma near_same_channel : forall I, near I I = Some I.
Proof.
  intro I. unfold near. destruct (caname_eq_dec I I) as [_ | Hneq].
  - reflexivity.
  - exfalso. apply Hneq. reflexivity.
Qed.

(* When the channels differ, near is undefined (no meeting surface). *)
Lemma near_distinct_channel : forall I J, I <> J -> near I J = None.
Proof.
  intros I J Hneq. unfold near. destruct (caname_eq_dec I J) as [Heq | _].
  - exfalso. apply Hneq. exact Heq.
  - reflexivity.
Qed.

(* ── The metered cut as the rho object's gated COMM ──────────────────────────
   The R1-shaped metered cut: the receiver's introduction Kp(I, _) = for(y<-I){T}
   and the eliminand Ke(J, _) = J!(U) sit under one seal s, funded by one
   s-token. We name the LHS [metered_cut_redex I J T U s t] and the residual
   [metered_cut_residual T U t]. Firing the cut is a [ca_step] from the redex. *)

Definition metered_cut_redex (I J : caname) (T U : signed_term) (s : sig) (t : token)
  : signed_term :=
  STPar (STSigned (CPPar (CPInput I T) (CPOutput J U)) s) (STStack (TGate s t)).

Definition metered_cut_residual (T U : signed_term) (t : token) : signed_term :=
  STPar (subst_st T 0 (CQuote U)) (STStack t).

(* When the surfaces are near (same channel I = J), the metered cut FIRES: the
   rho gate [ca_rule1] applies (its pattern reuses the one channel I). This is
   the "near defined ⇒ rule fires" half of the gate. *)
Lemma metered_cut_fires_when_near : forall I T U s t,
  ca_step (metered_cut_redex I I T U s t) (metered_cut_residual T U t).
Proof.
  intros I T U s t. unfold metered_cut_redex, metered_cut_residual.
  apply ca_rule1.
Qed.

(* When the surfaces are NOT near (distinct channels I <> J), the metered cut
   is BLOCKED: NO [ca_step] reduces the redex. The only constructors whose LHS
   could match the redex shape are [ca_rule1] — which unifies the input and
   output channels, forcing I = J (contradiction) — and the spatial closures
   [ca_par_l]/[ca_par_r], blocked because a lone wrapper [STSigned _ _] and a
   lone token stack [STStack _] are inert ([no_leak_requires_token] /
   [no_leak_stack_inert]). This is the "near undefined ⇒ no step" half. *)
Lemma metered_cut_blocked_when_not_near : forall I J T U s t,
  I <> J ->
  forall S', ~ ca_step (metered_cut_redex I J T U s t) S'.
Proof.
  intros I J T U s t Hneq S' Hstep.
  unfold metered_cut_redex in Hstep.
  inversion Hstep; subst.
  - (* ca_rule1: the LHS pattern forces CPInput I T and CPOutput J U to share
       one channel, i.e. I = J — contradicting I <> J. *)
    contradiction.
  - (* ca_rule3: the compound-token COMM has the SAME whole-redex LHS shape
       (s = SAnd s1 s2), so it too unifies the input/output channels: I = J. *)
    contradiction.
  - (* ca_par_l on the wrapped redex STSigned _ _ — inert (no token within). *)
    eapply no_leak_requires_token; eassumption.
  - (* ca_par_r on the token stack STStack _ — inert (tokens never step). *)
    eapply no_leak_stack_inert; eassumption.
Qed.

(* ── CA-P-178 headline: the metered cut is gated on near ─────────────────────
   A metered cut (a [ca_step] COMM on the R1-shaped redex) fires IFF the two
   surfaces are near — [near I J] is [Some _] — and blocks (no step) when
   [near I J] is [None]. Instantiated to the rho object, this reduces to the
   channel-identity match the existing rules already enforce. *)
Theorem metered_cut_gated_on_near : forall I J T U s t,
  (* (a) fires ⟺ near defined (same channel), with the determined residual *)
  ( (exists S', ca_step (metered_cut_redex I J T U s t) S')
      <-> (exists L, near I J = Some L) )
  (* (b) when near is defined, the surface is the common channel and the cut
         steps to the canonical residual *)
  /\ ( forall L, near I J = Some L ->
         ca_step (metered_cut_redex I J T U s t) (metered_cut_residual T U t) )
  (* (c) when near is undefined, the cut is blocked (no step) *)
  /\ ( near I J = None ->
         forall S', ~ ca_step (metered_cut_redex I J T U s t) S' ).
Proof.
  intros I J T U s t. split; [| split].
  - (* (a) *)
    split.
    + (* a step exists ⇒ near defined. Contrapositive: if near is undefined
         (I <> J) then no step exists. *)
      intros [S' Hstep].
      destruct (caname_eq_dec I J) as [Heq | Hneq].
      * subst J. apply near_defined_iff_same_channel. reflexivity.
      * exfalso. eapply metered_cut_blocked_when_not_near; eassumption.
    + (* near defined ⇒ I = J ⇒ a step exists (ca_rule1 fires). *)
      intro Hdef. apply near_defined_iff_same_channel in Hdef. subst J.
      eexists. apply metered_cut_fires_when_near.
  - (* (b) *)
    intros L HL.
    assert (Hsame : I = J) by (apply near_defined_iff_same_channel; exists L; exact HL).
    subst J. apply metered_cut_fires_when_near.
  - (* (c) *)
    intro Hnone.
    assert (Hneq : I <> J).
    { intro Heq. subst J. rewrite near_same_channel in Hnone. discriminate Hnone. }
    apply metered_cut_blocked_when_not_near. exact Hneq.
Qed.
