(* ═══════════════════════════════════════════════════════════════════════════
   CostAccountedReduction.v — Operational semantics of the cost-accounted
   rho calculus
   ═══════════════════════════════════════════════════════════════════════════

   Defines the small-step reduction relation [ca_step] for the cost-accounted
   rho calculus. Where the pure rho calculus has a single computational rule
   (COMM), the cost-accounted calculus refines that rule into FIVE variants
   that differ in how the redex is signed and how the authorising fuel
   token(s) are presented:

     ca_rule1  Atomic signature, whole redex, single token.
     ca_rule2  Compound signature, whole redex, split tokens.
     ca_rule3  Compound signature, whole redex, combined token.
     ca_rule4  Compound signature, split processes, combined token.
     ca_rule5  Compound signature, split processes, split tokens.

   May-2026 spec alignment: the constructor suffixes 4/5 retain the April-draft
   numbering. The May-2026 paper "Cost-Accounted Rho Calculus: A Spectral
   Decomposition of Phlogiston" (Section 3.6) swaps the labels of Rules 4 and 5
   — the set of five rules is identical, only the numbering differs. The
   spec-to-constructor mapping is therefore:
       paper Rule 4 (split processes, SPLIT tokens)    = ca_rule5
       paper Rule 5 (split processes, COMBINED token)  = ca_rule4
   Renaming the constructors is intentionally avoided (it would churn the
   positional case analyses in Confluence.v, StepDeterminism.v,
   FuelEventDecomposition.v, and TokenConservation.v with no change in content);
   the mapping is recorded here and in the traceability table below.

   In every rule the underlying computational effect is the same as in the
   pure calculus: a receiver  for(y ← x){P}  meets a sender  x!(Q)  and the
   bound variable y (de Bruijn index 0) is replaced inside P by the
   dereferenced quotation of Q. The accompanying token(s) advance by one
   gate, exposing the suffix that was previously guarded by the matched
   signature(s). The total number of fuel units in the system therefore
   decreases by exactly one with every step — the property formalised
   separately in TokenConservation.v.

   Reduction is also closed under parallel composition on either side, so
   subterms may take cost-accounted steps independently inside a larger
   system.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Paper Notation                     │ Notes
   ─────────────────────────┼────────────────────────────────────┼────────
   ca_step / "S ⤳ T"        │ S ⤳ T (cost-accounted step)        │
   ca_rule1                 │ Rule 1 (atomic, joined, single)    │
   ca_rule2                 │ Rule 2 (compound, joined, split)   │
   ca_rule3                 │ Rule 3 (compound, joined, combined)│
   ca_rule4                 │ May Rule 5 (compound, split, combined) [April Rule 4]
   ca_rule5                 │ May Rule 4 (compound, split, split)    [April Rule 5]
   ca_par_l, ca_par_r       │ PAR rule (system level)            │
   ca_reachable / "⤳*"      │ ⤳* (many cost-accounted steps)     │
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.1 stdlib, RhoSyntax, CostAccountedSyntax,
                 RhoReduction (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import RhoReduction.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: The Five Cost-Accounted Reduction Rules
   ═══════════════════════════════════════════════════════════════════════════

   Each rule corresponds to one of the five forms in which a COMM redex
   can appear in the cost-accounted calculus. They differ along two axes:

     Axis A — Signature shape on the redex:
       atomic (a single signature s) vs. compound (SAnd s1 s2).

     Axis B — Process layout:
       joined  — both halves of the redex live under one signature
                 SSigned (PPar (PInput x P) (POutput x Q)) _
       split   — the input and output are signed independently
                 SPar (SSigned (PInput x P) s1)
                      (SSigned (POutput x Q) s2)

     Axis C — Token layout:
       single   — one TGate s carries the matching fuel
       split    — two separate gates SToken (TGate s1 _), SToken (TGate s2 _)
       combined — one outer gate TGate (SAnd s1 s2) _ matches both halves

   The five rules cover the meaningful combinations of these axes that
   appear in the paper.                                                      *)

Reserved Notation "S '⤳' T" (at level 70, no associativity).

Inductive ca_step : system -> system -> Prop :=

  (* Rule 1: Atomic signature, whole redex, single token.

       (for(y ← x){P} | x!(Q))^s | s:T  ⤳  (P{@Q/y})^s | T

     The simplest case: a joined redex sealed under an atomic signature s
     consumes a single token gate guarded by the same signature s. *)
  | ca_rule1 : forall (x : name) (P Q : proc) (s : sig) (t : token),
      ca_step
        (SPar (SSigned (PPar (PInput x P) (POutput x Q)) s)
              (SToken (TGate s t)))
        (SPar (SSigned (subst_proc P 0 (Quote Q)) s)
              (SToken t))

  (* Rule 2: Compound signature, whole redex, split tokens.

       (for(y ← x){P} | x!(Q))^(s1 & s2) | s1:T1 | s2:T2
       ⤳  (P{@Q/y})^(s1 & s2) | T1 | T2

     The redex is sealed under a conjoined signature (SAnd s1 s2) and the
     fuel is supplied by two independent token gates, one for each
     conjunct. Each gate advances by one. *)
  | ca_rule2 : forall (x : name) (P Q : proc) (s1 s2 : sig) (t1 t2 : token),
      ca_step
        (SPar (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
                    (SToken (TGate s1 t1)))
              (SToken (TGate s2 t2)))
        (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                    (SToken t1))
              (SToken t2))

  (* Rule 3: Compound signature, whole redex, combined token.

       (for(y ← x){P} | x!(Q))^(s1 & s2) | (s1 & s2):T
       ⤳  (P{@Q/y})^(s1 & s2) | T

     Same redex shape as Rule 2, but the matching fuel arrives as a single
     gate whose own guard is already the compound signature SAnd s1 s2. *)
  | ca_rule3 : forall (x : name) (P Q : proc) (s1 s2 : sig) (t : token),
      ca_step
        (SPar (SSigned (PPar (PInput x P) (POutput x Q)) (SAnd s1 s2))
              (SToken (TGate (SAnd s1 s2) t)))
        (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
              (SToken t))

  (* Rule 4: Compound signature, split processes, combined token.

       (for(y ← x){P})^s1 | (x!(Q))^s2 | (s1 & s2):T
       ⤳  (P{@Q/y})^(s1 & s2) | T

     The two halves of the redex are signed independently (s1 for the
     receiver, s2 for the sender) and meet via a single combined fuel gate
     bearing SAnd s1 s2. The result is fused under the compound signature. *)
  | ca_rule4 : forall (x : name) (P Q : proc) (s1 s2 : sig) (t : token),
      ca_step
        (SPar (SPar (SSigned (PInput x P) s1)
                    (SSigned (POutput x Q) s2))
              (SToken (TGate (SAnd s1 s2) t)))
        (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
              (SToken t))

  (* Rule 5: Compound signature, split processes, split tokens.

       (for(y ← x){P})^s1 | (x!(Q))^s2 | s1:T1 | s2:T2
       ⤳  (P{@Q/y})^(s1 & s2) | T1 | T2

     The fully decomposed case: receiver and sender each carry their own
     atomic signature, and the matching fuel is provided by two
     independent gates. After the step the (now substituted) body is
     sealed under the compound signature. *)
  | ca_rule5 : forall (x : name) (P Q : proc) (s1 s2 : sig) (t1 t2 : token),
      ca_step
        (SPar (SPar (SPar (SSigned (PInput x P) s1)
                          (SSigned (POutput x Q) s2))
                    (SToken (TGate s1 t1)))
              (SToken (TGate s2 t2)))
        (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                    (SToken t1))
              (SToken t2))

  (* PAR rule (left): contextual closure of cost-accounted reduction
     under the left side of system-level parallel composition. *)
  | ca_par_l : forall S1 S1' S2,
      ca_step S1 S1' ->
      ca_step (SPar S1 S2) (SPar S1' S2)

  (* PAR rule (right): symmetric to ca_par_l. *)
  | ca_par_r : forall S1 S2 S2',
      ca_step S2 S2' ->
      ca_step (SPar S1 S2) (SPar S1 S2')

where "S '⤳' T" := (ca_step S T).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Reflexive-Transitive Closure
   ═══════════════════════════════════════════════════════════════════════════

   [ca_reachable S T] holds when T is obtained from S by zero or more
   cost-accounted reduction steps. This is the standard inductive
   definition of the reflexive-transitive closure of [ca_step], mirroring
   [rho_reachable] from RhoReduction.v.                                    *)

Inductive ca_reachable : system -> system -> Prop :=
  | car_refl : forall S, ca_reachable S S
  | car_step : forall S1 S2 S3,
      ca_step S1 S2 ->
      ca_reachable S2 S3 ->
      ca_reachable S1 S3.

Notation "S '⤳*' T" := (ca_reachable S T) (at level 70, no associativity).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Reachability Lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

(* A single step is a reachability sequence of length one. *)
Lemma ca_reachable_one : forall S1 S2,
  ca_step S1 S2 -> ca_reachable S1 S2.
Proof.
  intros S1 S2 Hstep.
  eapply car_step.
  - exact Hstep.
  - apply car_refl.
Qed.

(* Reachability is transitive: composing two reduction sequences yields a
   reduction sequence. The proof proceeds by induction on the first
   sequence; at each step we re-apply the inductive hypothesis to the
   second sequence, which we revert into the goal so that it tracks the
   intermediate term faithfully. This mirrors [rho_reachable_trans] in
   RhoReduction.v. *)
Lemma ca_reachable_trans : forall S1 S2 S3,
  ca_reachable S1 S2 ->
  ca_reachable S2 S3 ->
  ca_reachable S1 S3.
Proof.
  intros S1 S2 S3 H1.
  revert S3.
  induction H1 as [S0 | S0 Sm Sn Hstep Hreach IH]; intros S3 H2.
  - (* car_refl: the first sequence is empty, so the second IS the result. *)
    exact H2.
  - (* car_step: S0 ⤳ Sm and Sm ⤳* Sn; we need S0 ⤳* S3 given Sn ⤳* S3. *)
    eapply car_step.
    + exact Hstep.
    + apply IH. exact H2.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Sanity Check Lemmas
   ═══════════════════════════════════════════════════════════════════════════

   Small lemmas that simply repackage the rule constructors. They serve as
   smoke tests that the inductive definitions are usable from client
   modules.                                                                 *)

(* Rule 1 reduces a token by exactly one outermost gate while substituting
   the dereferenced sent process for the bound variable in the receiver. *)
Lemma rule1_consumes_one_token : forall x P Q s t,
  ca_step
    (SPar (SSigned (PPar (PInput x P) (POutput x Q)) s) (SToken (TGate s t)))
    (SPar (SSigned (subst_proc P 0 (Quote Q)) s) (SToken t)).
Proof.
  intros x P Q s t.
  apply ca_rule1.
Qed.

(* Rule 5 — the fully decomposed case — likewise consumes one outermost
   gate from each of the two side-by-side tokens. *)
Lemma rule5_consumes_two_token_gates : forall x P Q s1 s2 t1 t2,
  ca_step
    (SPar (SPar (SPar (SSigned (PInput x P) s1)
                      (SSigned (POutput x Q) s2))
                (SToken (TGate s1 t1)))
          (SToken (TGate s2 t2)))
    (SPar (SPar (SSigned (subst_proc P 0 (Quote Q)) (SAnd s1 s2))
                (SToken t1))
          (SToken t2)).
Proof.
  intros x P Q s1 s2 t1 t2.
  apply ca_rule5.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Terminal Systems
   ═══════════════════════════════════════════════════════════════════════════

   A system is terminal when no cost-accounted reduction step is possible.
   This is the standard notion of a normal form for the [ca_step] relation.
   Used by the confluence and cost-determinism theorems in Confluence.v.    *)

Definition ca_terminal (S : system) : Prop :=
  forall S', ~ ca_step S S'.

(* An equivalent characterisation: a system is terminal iff its set of
   one-step reducts is empty. *)
Lemma ca_terminal_iff_no_step : forall S,
  ca_terminal S <-> (forall S', ~ ca_step S S').
Proof.
  intros S. unfold ca_terminal. tauto.
Qed.
