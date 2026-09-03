(* ═══════════════════════════════════════════════════════════════════════════
   StructEquivInversion.v — Structural Equivalence Inversion Machinery
   ═══════════════════════════════════════════════════════════════════════════

   Provides inversion lemmas for [struct_equiv] (≡) by characterising any
   process modulo ≡ via its multiset of "head" components (the non-PPar,
   non-PNil constructors that appear at the top level after flattening
   parallel composition).

   The standard π-calculus mechanisation trick: ≡ does not preserve syntax,
   but it DOES preserve the multiset of top-level component constructors
   (modulo permutation, identity for PNil, and ≡-equivalence on the
   components themselves). This module defines a [head_count] measure and
   a [head_kinds] view that enable the following kinds of inversion lemmas:

   - [PDeref x ≡ R] forces R to be of [PDeref] shape
   - [PNil ≡ R] forces R to have head_count 0
   - [PInput x B ≡ R] forces R to have a single PInput head
   - [PPar (PInput x B) Q ≡ R] forces R to contain a PInput head with
     the rest equivalent to Q

   These are the load-bearing inversion lemmas needed by:
   - [RhoReduction.v]'s stuck-process theorems (PDeref_stuck, PNil_stuck,
     PInput_alone_stuck, POutput_alone_stuck)
   - [FuelGateSafety.v]'s [fuel_gate_stuck] theorem
   - [Bisimulation.v]'s [par_with_stuck_residue_steps] auxiliary

   Dependencies: RhoSyntax (this project), Stdlib lists/permutation
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.

From CostAccountedRho Require Import RhoSyntax.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Head Count
   ═══════════════════════════════════════════════════════════════════════════

   The [head_count] of a process is the number of non-PPar/non-PNil
   constructors at the top level after flattening. PPar adds the counts;
   PNil contributes 0; PInput, POutput, PDeref, PReplicate each
   contribute 1.

   PReplicate is treated as an ATOMIC head (like PInput/POutput/PDeref)
   rather than transparent (like PPar). Without the structural equivalence
   axiom [!P ≡ P | !P], a PReplicate cannot be decomposed into its body
   modulo ≡, so it must be treated as an opaque unit at the top level.

   The key fact is that head_count is invariant under structural
   equivalence: [≡] only rearranges/identifies head components, never
   adds or removes them.                                                  *)

Fixpoint head_count (P : proc) : nat :=
  match P with
  | PNil          => 0
  | PInput _ _    => 1
  | POutput _ _   => 1
  | PDeref _      => 1
  | PPar P1 P2    => head_count P1 + head_count P2
  | PReplicate _  => 1
  end.

(* Structural equivalence preserves the head count. The proof is by
   induction on the [≡] derivation. Each rule either preserves the count
   structurally (PPar congruence rules), is identity (PInput/POutput/
   PDeref/PReplicate congruences which only modify components, not the
   count), or exploits the commutative monoid laws (assoc, comm, nil)
   which all preserve sum. *)
Lemma head_count_se : forall P Q, P ≡ Q -> head_count P = head_count Q.
Proof.
  intros P Q Heq.
  induction Heq; simpl; try lia; try reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Reductions Require At Least Two Heads
   ═══════════════════════════════════════════════════════════════════════════

   The fundamental fact: any [rho_step] requires the source to have at
   least two heads at the top level. The base case [rs_comm] needs a
   PInput and a POutput in parallel (count = 2). [rs_par_l] and [rs_par_r]
   inductively need a sub-component with count >= 2 plus another, so the
   total is at least 2. [rs_struct] preserves the count via [head_count_se]
   and inherits the bound from the inner step.                            *)

(* This is proved later in RhoReduction.v after rho_step is defined.
   We export head_count and head_count_se from here for use everywhere. *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Counting PDeref Heads
   ═══════════════════════════════════════════════════════════════════════════

   Counts the number of PDeref constructors at the top level (above PPars).
   PInput, POutput, PNil, PReplicate contribute 0; PDeref contributes 1;
   PPar adds.  PReplicate contributes 0 because it is an opaque head — its
   body's derefs are not exposed at the top level.
   Like head_count, this is preserved by structural equivalence.          *)

Fixpoint count_derefs (P : proc) : nat :=
  match P with
  | PNil          => 0
  | PInput _ _    => 0
  | POutput _ _   => 0
  | PDeref _      => 1
  | PPar P1 P2    => count_derefs P1 + count_derefs P2
  | PReplicate _  => 0
  end.

Lemma count_derefs_se : forall P Q, P ≡ Q -> count_derefs P = count_derefs Q.
Proof.
  intros P Q Heq.
  induction Heq; simpl; try lia; try reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Counting PInput and POutput Heads
   ═══════════════════════════════════════════════════════════════════════════ *)

Fixpoint count_inputs (P : proc) : nat :=
  match P with
  | PNil          => 0
  | PInput _ _    => 1
  | POutput _ _   => 0
  | PDeref _      => 0
  | PPar P1 P2    => count_inputs P1 + count_inputs P2
  | PReplicate _  => 0
  end.

Fixpoint count_outputs (P : proc) : nat :=
  match P with
  | PNil          => 0
  | PInput _ _    => 0
  | POutput _ _   => 1
  | PDeref _      => 0
  | PPar P1 P2    => count_outputs P1 + count_outputs P2
  | PReplicate _  => 0
  end.

Lemma count_inputs_se : forall P Q, P ≡ Q -> count_inputs P = count_inputs Q.
Proof.
  intros P Q Heq.
  induction Heq; simpl; try lia; try reflexivity.
Qed.

Lemma count_outputs_se : forall P Q, P ≡ Q -> count_outputs P = count_outputs Q.
Proof.
  intros P Q Heq.
  induction Heq; simpl; try lia; try reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4b: Counting PReplicate Heads
   ═══════════════════════════════════════════════════════════════════════════

   Counts the number of PReplicate constructors at the top level (above
   PPars). PReplicate contributes 1 regardless of its body; all other
   non-PPar constructors contribute 0; PPar adds. Like the other counts,
   this is preserved by structural equivalence.                             *)

Fixpoint count_replicates (P : proc) : nat :=
  match P with
  | PNil          => 0
  | PInput _ _    => 0
  | POutput _ _   => 0
  | PDeref _      => 0
  | PPar P1 P2    => count_replicates P1 + count_replicates P2
  | PReplicate _  => 1
  end.

Lemma count_replicates_se : forall P Q, P ≡ Q -> count_replicates P = count_replicates Q.
Proof.
  intros P Q Heq.
  induction Heq; simpl; try lia; try reflexivity.
Qed.

(* The "no PDeref content" predicate: a process whose count_derefs is 0
   contains no PDeref at the top level. *)
Definition no_top_derefs (P : proc) : Prop := count_derefs P = 0.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Extract a PDeref Residue
   ═══════════════════════════════════════════════════════════════════════════

   The key constructive lemma: any process with at least one top-level
   PDeref can be factored as [PPar Q (PDeref n)] modulo ≡, where Q has
   one fewer top-level PDeref. This is the structural decomposition that
   the bisimulation backward-simulation proof needs.                     *)

Lemma extract_deref :
  forall P, count_derefs P > 0 ->
  exists Q n, P ≡ PPar Q (PDeref n) /\ count_derefs Q = count_derefs P - 1.
Proof.
  induction P; intros Hcount.
  - (* PNil: count_derefs = 0, contradiction *)
    simpl in Hcount. lia.
  - (* PInput x P: count_derefs = 0, contradiction *)
    simpl in Hcount. lia.
  - (* POutput x P: count_derefs = 0, contradiction *)
    simpl in Hcount. lia.
  - (* PPar P1 P2 *)
    simpl in Hcount.
    destruct (Nat.eq_dec (count_derefs P1) 0) as [Heq1 | Hneq1].
    + (* count_derefs P1 = 0, so count_derefs P2 > 0. Apply IH on P2. *)
      assert (Hp2 : count_derefs P2 > 0) by lia.
      destruct (IHP2 Hp2) as [Q [n [Heq Hcq]]].
      exists (PPar P1 Q), n.
      split.
      * (* PPar P1 P2 ≡ PPar P1 (PPar Q (PDeref n)) by se_par_cong_r Heq
           ≡ PPar (PPar P1 Q) (PDeref n) by se_sym se_par_assoc *)
        eapply se_trans.
        { apply se_par_cong_r. exact Heq. }
        apply se_sym. apply se_par_assoc.
      * simpl. lia.
    + (* count_derefs P1 > 0. Apply IH on P1. *)
      assert (Hp1 : count_derefs P1 > 0) by lia.
      destruct (IHP1 Hp1) as [Q [n [Heq Hcq]]].
      exists (PPar Q P2), n.
      split.
      * (* PPar P1 P2 ≡ PPar (PPar Q (PDeref n)) P2 by se_par_cong_l Heq
           ≡ PPar Q (PPar (PDeref n) P2) by se_par_assoc
           ≡ PPar Q (PPar P2 (PDeref n)) by se_par_cong_r se_par_comm
           ≡ PPar (PPar Q P2) (PDeref n) by se_sym se_par_assoc *)
        eapply se_trans.
        { apply se_par_cong_l. exact Heq. }
        eapply se_trans. { apply se_par_assoc. }
        eapply se_trans. { apply se_par_cong_r. apply se_par_comm. }
        apply se_sym. apply se_par_assoc.
      * simpl. lia.
  - (* PDeref n *)
    exists PNil, n.
    split.
    + (* PDeref n ≡ PPar PNil (PDeref n) by se_sym se_nil_par *)
      apply se_sym. apply se_nil_par.
    + simpl. lia.
  - (* PReplicate P: count_derefs = 0, contradiction *)
    simpl in Hcount. lia.
Qed.

(* Specialised: factor out a PDeref residue from a process whose
   count_derefs is exactly 1. The "rest" Q has no top-level derefs. *)
Lemma extract_single_deref :
  forall P, count_derefs P = 1 ->
  exists Q n, P ≡ PPar Q (PDeref n) /\ no_top_derefs Q.
Proof.
  intros P Hcount.
  assert (H : count_derefs P > 0) by lia.
  destruct (extract_deref P H) as [Q [n [Heq Hcq]]].
  exists Q, n.
  split.
  - exact Heq.
  - unfold no_top_derefs. lia.
Qed.
