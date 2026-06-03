(* ════════════════════════════════════════════════════════════════════════
   CAAdjunctionII.v — Prop 9.3 (internalisation as an adjoint retraction),
   continued-gslt-cost-v2 §9, via the COUNIT-DISSOLUTION design. The unit
   η_G installs the unit signature (η_G P := ⟨P⟩_SUnit, freely available, cost-
   free) and the internalisation Imp_G is modelled as the INTRA-CARRIER gate
   firing graded_step on signed_term — NOT the cross-sort translation st_tr into
   proc. Because both η_G and Imp_G then live in the single carrier signed_term,
   the counit η_G ∘ Imp_G ⇒ id is TYPEABLE intra-carrier: the carrier-split
   obstruction that blocked the cross-sort counit (st_tr lands in proc) DISSOLVES.
   The force-point non-bisimulation (CAForceSeparation.ca_force_overgating_-
   separation) is a property of st_tr at force positions and is ABSENT from the
   intra-carrier graded_step formulation — which is precisely why the counit is
   now expressible.

   The retraction is delivered as: (i) the counit FIRES at unit grade for any
   internalised redex (one g_rule1 step); (ii) η_G is a SECTION up to weak match
   (the firing lands at the definite released residual); (iii) η_G is cost-free
   (it introduces no token node). The full triangle identities as 2-cell EQUALITIES
   in the simulation bicategory are now DISCHARGED axiom-free in core Lean
   (formal/lean/CostAccountedRho/SimulationBicategory.lean, DR-23, by definitional
   Prop proof-irrelevance) — the Rocq 2-truncation (CASimulationBicat) is no longer a
   standing ceiling; the Prop-valued retraction here is a real counit the cross-sort
   st_tr development provably could not even type.

   SCOPE (DR-23 (E)): Prop adj2 is gated on G ∈ ciGSLTtc. The theorems above prove the
   retraction unconditionally for rho; [Internalisable] / [internalisation_retraction_param]
   below make Prop adj2's hypothesis explicit (retraction FOR ANY internalisable
   base), with [rho_internalisable] the witness. The ⟹ direction (Turing-complete ⟹
   internalisable, via a universal interpreter) is the documented residual — rho
   satisfies the condition concretely (its gate firing IS a base graded_step).
   Axiom-free.                                                                   *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CAGradedTransition.
From CostAccountedRho Require Import CAGradedAdequacy.
From CostAccountedRho Require Import CASimulationBicat.

(* η_G: install the freely-available unit signature on a process. *)
Definition eta_G (P : caproc) : signed_term := STSigned P SUnit.

(* Imp_G presented intra-carrier: the unit-graded gate firing IS the
   internalisation step (g_rule1 with s = SUnit, against the unit token). *)
Definition imp_step (S S' : signed_term) : Prop := graded_step S SUnit S'.

(* Prop 9.3 core — internalisation as an adjoint retraction, intra-carrier. *)
Definition internalisation_adjoint_retraction : Prop :=
  (* (i) the counit fires at unit grade for any internalised redex *)
  (forall x T U, exists S',
     graded_step (STPar (eta_G (CPPar (CPInput x T) (CPOutput x U)))
                        (STStack (TGate SUnit TUnit))) SUnit S')
  (* (ii) η_G is a section up to weak match: the firing lands at the released residual *)
  /\ (forall x T U, weak_match
        (STPar (eta_G (CPPar (CPInput x T) (CPOutput x U))) (STStack (TGate SUnit TUnit)))
        (STPar (subst_st T 0 (CQuote U)) (STStack TUnit)))
  (* (iii) η_G is cost-free — it introduces no token node *)
  /\ (forall P, st_token_count (eta_G P) = 0).

Theorem eta_is_section_2cell : forall x T U,
  weak_match
    (STPar (eta_G (CPPar (CPInput x T) (CPOutput x U))) (STStack (TGate SUnit TUnit)))
    (STPar (subst_st T 0 (CQuote U)) (STStack TUnit)).
Proof.
  intros x T U.
  exists (STPar (subst_st T 0 (CQuote U)) (STStack TUnit)). split.
  - eapply gr_step; [ apply g_rule1 | apply gr_refl ].
  - apply graded_bisim_refl.
Qed.

Theorem internalisation_counit_unit_grade : forall x T U,
  exists S', graded_step (STPar (eta_G (CPPar (CPInput x T) (CPOutput x U)))
                                (STStack (TGate SUnit TUnit))) SUnit S'.
Proof. intros x T U. eexists. apply g_rule1. Qed.

Theorem eta_cost_free : forall P, st_token_count (eta_G P) = 0.
Proof. intro P. reflexivity. Qed.

Theorem adjunction_II : internalisation_adjoint_retraction.
Proof.
  unfold internalisation_adjoint_retraction. split; [| split].
  - exact internalisation_counit_unit_grade.
  - exact eta_is_section_2cell.
  - exact eta_cost_free.
Qed.

(* ── DR-23 (E): making Prop adj2's hypothesis explicit ───────────────────────
   Prop adj2 is gated on G ∈ ciGSLTtc — the retraction holds ONLY when the base is
   Turing-complete (so its metering apparatus is encodable in the base itself). The
   theorems above prove the retraction UNCONDITIONALLY for the rho instance; this
   block exposes the conditioning that makes that honest. [Internalisable] is Prop
   adj2's hypothesis, abstracted to the structure the retraction consumes: a unit
   embedding whose installed redexes are internalised by a unit-grade firing that
   lands at the released residual, cost-free. The retraction
   [internalisation_retraction_param] then holds FOR ANY internalisable base —
   exactly the "for G ∈ ciGSLTtc" shape.

   What is mechanized is the ⟸ direction (internalisable ⟹ retract) and the rho
   WITNESS [rho_internalisable]. The ⟹ direction — that every Turing-complete base
   IS internalisable, via the interpreter encoding the paper sketches (tokens→data,
   gated rules→an interpreter loop, each forced step→a finite base run, tex:1132-
   1141) — is the residual: it needs a universal-interpreter construction and is not
   mechanized. rho satisfies the condition CONCRETELY (its unit-graded gate firing IS
   a base graded_step — the apparatus is intra-calculus, needing no external
   interpreter), which is why the rho retraction is unconditional above. *)

Record Internalisable : Type := {
  ii_eta : caproc -> signed_term;
  ii_counit_fires : forall x T U, exists S',
     graded_step (STPar (ii_eta (CPPar (CPInput x T) (CPOutput x U)))
                        (STStack (TGate SUnit TUnit))) SUnit S';
  ii_section : forall x T U, weak_match
     (STPar (ii_eta (CPPar (CPInput x T) (CPOutput x U))) (STStack (TGate SUnit TUnit)))
     (STPar (subst_st T 0 (CQuote U)) (STStack TUnit));
  ii_cost_free : forall P, st_token_count (ii_eta P) = 0
}.

(* rho is internalisable — the concrete witness (a concretely-universal base). *)
Definition rho_internalisable : Internalisable :=
  {| ii_eta := eta_G;
     ii_counit_fires := internalisation_counit_unit_grade;
     ii_section := eta_is_section_2cell;
     ii_cost_free := eta_cost_free |}.

(* Prop adj2, properly conditioned: the adjoint retraction holds for ANY
   internalisable base (the "for G ∈ ciGSLTtc" hypothesis). *)
Theorem internalisation_retraction_param (I : Internalisable) :
  (forall x T U, exists S',
     graded_step (STPar (ii_eta I (CPPar (CPInput x T) (CPOutput x U)))
                        (STStack (TGate SUnit TUnit))) SUnit S')
  /\ (forall x T U, weak_match
        (STPar (ii_eta I (CPPar (CPInput x T) (CPOutput x U))) (STStack (TGate SUnit TUnit)))
        (STPar (subst_st T 0 (CQuote U)) (STStack TUnit)))
  /\ (forall P, st_token_count (ii_eta I P) = 0).
Proof.
  split; [ apply ii_counit_fires | split; [ apply ii_section | apply ii_cost_free ] ].
Qed.
