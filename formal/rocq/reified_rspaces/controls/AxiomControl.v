(* AxiomControl.v — the negative control of the axiom checks of the proofs gate

   This module is a fixture, not a theory. It lies outside theories/ and
   outside _CoqProject, so the library never contains it. It states one
   axiom on purpose. The proofs gate runs its hygiene check and rocqchk on
   this module and passes only when both report the axiom: the hygiene check
   names an axiom declaration, and rocqchk lists the axiom. So the gate shows
   that the two checks that keep the theories axiom-free can fail.
   Plan: the bootstrap leaf and plan:932 (rocqchk must reject the fixture);
   P10 design v2, section 2.2. *)

Axiom control_axiom : forall n : nat, n = n + 0.

Theorem control_uses_the_axiom : 3 = 3 + 0.
Proof. apply control_axiom. Qed.

Print Assumptions control_uses_the_axiom.
