(* ════════════════════════════════════════════════════════════════════════
   CACostFunctorCI.v — Thm 7.1 ON THE CONCRETE ciGSLT category (continued-gslt-
   cost-v2 §6/§7, the "Cost is an endofunctor on ciGSLT" claim).

   CACostFunctor.v proves the functor laws for the writer presentation of Cost on
   TypeCatL (types + functions) — the algebraic skeleton. The arbitration of the
   DR-23 cross-validation review found the LOAD-BEARING categorical obligation the
   paper emphasises (tex:769-777: Cost(f) preserves the gated transition AND the
   behavioural equivalence) was NOT transported onto the concrete ciGSLT category
   CICat (CACategory) — CICat stood disconnected from Cost. This module discharges
   exactly that obligation: a genuine endofunctor CostCI : Functor CICat CICat whose
   morphism action CostMor f is, BY CONSTRUCTION, a CIMor — transition-preserving
   (mor_pres) and bisimulation-preserving (mor_cong).

   CICat's cstep is already SIGNATURE-GRADED (carrier -> sig -> carrier -> Prop), so
   the abstractly-definable cost endofunctor on it is the SIGNATURE-ACCUMULATING
   writer: Cost(G) adjoins to each state the accumulated spatial signature, and a
   transition appends its consumed signature via the free `SAnd` tensor — the
   spatial monoid ∘ of the calculus, read at the abstract transition-system level.
   Quote-faithfulness (the second ciGSLT-morphism condition) is not a field of the
   skeletal CIMor (CACategory scopes the §6 object to the fragment the abstract
   proofs touch), so it remains that module's standing scope boundary; the
   bisimulation-preservation half — the part the review flagged as missing — is here.
   Axiom-free.                                                                    *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CACategory.
From CostAccountedRho Require Import CategoryInterface.

(* ── Cost on objects: adjoin the accumulated spatial signature; a transition
   appends its consumed signature to the accumulator (the ∘ = SAnd monoid). The
   behavioural equivalence projects to the base state (the grade is bookkeeping). *)
Definition CostObj (G : CIObj) : CIObj :=
  {| carrier := (carrier G * sig)%type;
     cstep   := fun p s p' => cstep G (fst p) s (fst p') /\ snd p' = SAnd (snd p) s;
     cbisim  := fun p q => cbisim G (fst p) (fst q);
     cbisim_refl  := fun p => cbisim_refl G (fst p);
     cbisim_sym   := fun p q H => cbisim_sym G _ _ H;
     cbisim_trans := fun p q r H1 H2 => cbisim_trans G _ _ _ H1 H2 |}.

(* ── Cost on morphisms: act on the base state by f, carry the grade unchanged.
   The result is a CIMor — it preserves transitions (mor_pres f on the base + the
   grade bookkeeping transfers verbatim) and bisimulation (mor_cong f). *)
Definition CostMor {G H : CIObj} (f : CIMor G H) : CIMor (CostObj G) (CostObj H) :=
  @Build_CIMor (CostObj G) (CostObj H)
    (fun p => (mor_map f (fst p), snd p))
    (fun p s p' Hs => conj (mor_pres f (proj1 Hs)) (proj2 Hs))
    (fun p q Hb => mor_cong f Hb).

(* ── Thm 7.1 (on CICat): Cost is an endofunctor on the concrete ciGSLT category. *)
Definition CostCI : Functor CICat CICat.
Proof.
  refine (@Build_Functor CICat CICat CostObj (@CostMor) _ _ _).
  - (* Fmor_proper *) intros G H f g Hfg p. simpl. exact (Hfg (fst p)).
  - (* Fmor_id *)     intros G p. simpl. apply cbisim_refl.
  - (* Fmor_comp *)   intros G H K f g p. simpl. apply cbisim_refl.
Defined.

(* The load-bearing obligation, stated explicitly: Cost(f) preserves behavioural
   equivalence on the concrete ciGSLT category (the part the review flagged absent). *)
Theorem cost_ci_preserves_bisim :
  forall (G H : CIObj) (f : CIMor G H) (p q : carrier (CostObj G)),
    cbisim (CostObj G) p q ->
    cbisim (CostObj H) (mor_map (CostMor f) p) (mor_map (CostMor f) q).
Proof. intros G H f p q Hb. exact (mor_cong (CostMor f) Hb). Qed.

(* And it preserves the gated transition (the simulation half). *)
Theorem cost_ci_preserves_step :
  forall (G H : CIObj) (f : CIMor G H) (p : carrier (CostObj G)) (s : sig)
         (p' : carrier (CostObj G)),
    cstep (CostObj G) p s p' ->
    cstep (CostObj H) (mor_map (CostMor f) p) s (mor_map (CostMor f) p').
Proof. intros G H f p s p' Hs. exact (mor_pres (CostMor f) Hs). Qed.

(* Non-vacuity: Cost applied to the concrete rho ciGSLT object is a live CICat
   object (the re-metered rho calculus, state = signed_term × accumulated sig). *)
Theorem cost_ci_nonvacuous : exists G : Obj CICat, G = CostObj Rho_ciGSLT.
Proof. exists (CostObj Rho_ciGSLT). reflexivity. Qed.
