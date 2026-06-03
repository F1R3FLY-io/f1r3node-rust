(* ════════════════════════════════════════════════════════════════════════
   CAInternalisation.v — Adjunction II: internalise ⊣ include (CL5, Prop. adj2).

   continued-gslt-cost-v2 §"The cost monad and two adjunctions", Proposition
   "Internalisation as an adjoint retraction":

       Imp_G ∘ η_G  ≈  id_G   up to weak bisimulation,

   exhibiting Cost(G) as a BEHAVIOURAL RETRACT of its Turing-complete base G.
   The unit η_G is an (iso-up-to-bisimulation) SECTION; the apparatus dissolves
   into the base's own computation, the agreement holding up to the
   administrative reductions of the interpreter (i.e. up to weak bisimulation).

   For the rho instance:
     • η_G(P)  = STSigned P SUnit — the unmetered embedding: install the gating
       apparatus at the UNIT signature, "against the freely available unit
       token", with NO net resource (η is cost-free: st_token_count = 0).
     • Imp_G   = st_tr — the internalisation that realises the cost-accounted
       semantics inside the un-metered base by simulation (tokens → encoded
       data, gated rules → an interpreter loop).
     • The freely-available unit token = T_tr (TGate SUnit TUnit).

   The retraction is the s = SUnit instance of the single-gate bisimulation
   (CABisimulation.ca_single_gate_bisimilar): the internalised unit-graded
   embedding, presented against the freely-available unit token, makes the
   gate-firing administrative reduction (rho_reachable) to a residue STRONGLY
   bisimilar to Pt P — the base image of id_G(P). At the unit grade the
   force-point over-gating obstruction (docs §3a, a property of the FULL metered
   translation at arbitrary grades, NOT claimed by Adjunction II) does not arise:
   the freely-available unit token fires the gate as an administrative step.

   Axiom-free, and in fact fully GENERAL over the hash/ground encoders: the
   retraction needs none of the collision-resistance hypotheses (the unit-grade
   gate firing is independent of channel injectivity), so it holds for every
   choice of encoder — a strictly stronger closure than the rest of the
   translation development.                                                      *)

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATranslation.
From CostAccountedRho Require Import CATranslationFaithfulness.
From CostAccountedRho Require Import Bisimulation.
From CostAccountedRho Require Import CABisimulation.

Section CAInternalisationSec.

(* The hash/ground encoders parameterise the translation; the retraction needs
   none of their collision-resistance properties (see header), so only the bare
   encoder functions are taken here. *)
Variable hash_process : list bool -> proc.
Variable ground_process : list bool -> proc.

Local Notation Pt := (p_tr hash_process ground_process).
Local Notation St := (st_tr hash_process ground_process).
Local Notation Tt := (T_tr hash_process ground_process).

(* η_G — the unmetered embedding: install the apparatus at the unit signature. *)
Definition ca_eta_embed (P : caproc) : signed_term := STSigned P SUnit.

(* The freely-available unit token η_G presents the unit signature against. *)
Definition ca_unit_supply : token := TGate SUnit TUnit.

(* η_G is cost-free: the embedding installs at the unit signature with NO net
   resource — the unmetered embedding holds no token (st_token_count = 0). This
   is what makes η the "cost-free section" of the retraction. *)
Theorem ca_eta_cost_free : forall P, st_token_count (ca_eta_embed P) = 0.
Proof. intro P. reflexivity. Qed.

(* ── Adjunction II (Prop. adj2): Imp_G ∘ η_G ≈ id_G up to weak bisimulation. ──

   The internalisation of the unit-graded embedding, presented against the
   freely-available unit token, administratively reduces (the gate-firing
   τ-steps) to a state strongly bisimilar to Pt P — the base image of id_G(P).
   Hence η_G is a section up to weak bisimulation and Cost(G) is a behavioural
   retract of its Turing-complete base. *)
Theorem ca_internalisation_retraction : forall P,
  exists W,
    rho_reachable (PPar (St (ca_eta_embed P)) (Tt ca_unit_supply)) W
    /\ bisim W (Pt P).
Proof.
  intro P. unfold ca_eta_embed, ca_unit_supply.
  apply (ca_single_gate_bisimilar hash_process ground_process P SUnit).
  intros a b. discriminate.
Qed.

(* The retraction, packaged as the adjoint-retraction SECTION property: η_G
   followed by Imp_G recovers id_G's base image up to weak bisimulation —
   "an (iso-up-to-bisimulation) section" (Prop. adj2). The two conjuncts make
   the "up to weak bisimulation" explicit: a finite administrative run
   (rho_reachable) followed by strong bisimilarity of the residue. *)
Corollary ca_eta_is_weak_bisim_section : forall P,
  exists W,
    rho_reachable (PPar (St (ca_eta_embed P)) (Tt ca_unit_supply)) W
    /\ bisim W (Pt P)
    /\ st_token_count (ca_eta_embed P) = 0.
Proof.
  intro P. destruct (ca_internalisation_retraction P) as [W [Hreach Hbisim]].
  exists W. split; [ exact Hreach | split; [ exact Hbisim | apply ca_eta_cost_free ] ].
Qed.

End CAInternalisationSec.
