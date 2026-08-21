(* ════════════════════════════════════════════════════════════════════════
   CAAbstractCapstone.v — the §6-§9 abstract category-theory results of
   continued-gslt-cost-v2 assembled into one axiom-free conjunction, mirroring
   ContinuedGSLTCapstone / GSLTOSLFCapstone. Each conjunct is an already-closed
   headline from the CT-layer modules; the capstone adds no new mathematics and
   no axioms. Conjoins:
     • Thm 7.1   — Cost is an endofunctor          (CACostFunctor.cost_is_endofunctor)
     • Prop 6.2  — grade-carrier closure           (CACostFunctor.cost_obj_closure)
     • Prop 6.1  — U faithful, not full, not-eso    (CAProperSubcategory.proper_subcategory)
     • Prop 9.1  — Cost is a monad                  (CACostMonadCat.cost_is_monad)
     • Prop 9.2  — Free ⊣ Forget                    (CAAdjunctionI.free_forget_adjunction)
     • Prop 9.3  — internalisation adjoint retraction (CAAdjunctionII.adjunction_II)
     • the 2-truncated simulation 2-cells          (CASimulationBicat.sim_2cells_form_setoid)
   The full setoid-bicategory coherence is the 2-truncation ceiling stated in
   CASimulationBicat / CAAdjunctionII and routed to Lean/Isabelle (the foundations
   permitting funext/UIP); the Rocq deliverable is the Prop-valued conjunction
   above. Axiom-free.                                                            *)

From CostAccountedRho Require Import CACostFunctor.
From CostAccountedRho Require Import CAProperSubcategory.
From CostAccountedRho Require Import CACostMonadCat.
From CostAccountedRho Require Import CAAdjunctionI.
From CostAccountedRho Require Import CASimulationBicat.
From CostAccountedRho Require Import CAAdjunctionII.

(* The conjunction's type is inferred from the conjoined proofs (each an
   already-closed headline); no statement is restated. *)
Definition continued_gslt_cost_abstract_capstone :=
  conj cost_is_endofunctor
  (conj cost_obj_closure
  (conj proper_subcategory
  (conj cost_is_monad
  (conj free_forget_adjunction
  (conj adjunction_II
        sim_2cells_form_setoid))))).
