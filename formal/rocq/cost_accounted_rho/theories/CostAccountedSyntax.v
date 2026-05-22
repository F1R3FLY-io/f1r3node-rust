(* ═══════════════════════════════════════════════════════════════════════════
   CostAccountedSyntax.v — Syntax of the cost-accounted rho calculus
   ═══════════════════════════════════════════════════════════════════════════

   Extends the pure rho calculus (RhoSyntax.v) with the syntactic machinery
   needed to track and conserve computational fuel. The cost-accounted
   calculus introduces three new categories of terms:

   - signatures (sig)    : algebraic representations of digital signatures
                           used to authorize fuel consumption
   - tokens   (token)    : ordered stacks of signature-guarded fuel units
                           that act as the "currency" of computation
   - systems  (system)   : configurations pairing processes with the
                           signatures that authenticate them and the
                           free tokens that are available for spending

   A token of the form  s₁:s₂:s₃:...:()  represents three units of fuel,
   each of which can only be released by presenting the corresponding
   signature s_i. The empty token () carries no remaining fuel.

   Systems compose in parallel just like processes. The crucial invariant
   that this module enables (and that is later proved in TokenConservation.v)
   is that every cost-accounted reduction rule preserves the total token
   count of the system.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Paper Notation        │ Rust Implementation
   ─────────────────────────┼───────────────────────┼─────────────────────
   SUnit                    │ () (unit signature)   │ Sig::Unit
   SHash bs                 │ hash(σ)               │ Sig::Hash(bytes)
   SAnd s1 s2               │ s₁ & s₂               │ Sig::And(s1, s2)
   TUnit                    │ () (empty token)      │ Token::Unit
   TGate s t                │ s : T                 │ Token::Gate(s, t)
   SSigned P s              │ P^s                   │ System::Signed(P, s)
   SToken t                 │ T                     │ System::Token(t)
   SPar S1 S2               │ S₁ ∥ S₂               │ System::Par(s1, s2)
   sig_size                 │ |s|                   │ Sig::size()
   token_size               │ |T| (fuel count)      │ Token::size()
   system_token_count       │ ‖S‖ (total fuel)      │ System::token_count()
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.1 stdlib, RhoSyntax (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Signatures
   ═══════════════════════════════════════════════════════════════════════════

   A [sig] is the algebraic shadow of a digital signature. We do not commit
   to any particular cryptographic scheme; instead, we model signatures
   abstractly as one of three constructors:

   - [SUnit]      — the trivial unit signature, used to seal terms that
                    require no authentication.
   - [SHash bs]   — an atomic signature represented as the byte string [bs]
                    (the hash of some underlying signature value σ). We use
                    [list bool] for the byte string to remain free of any
                    external library dependencies.
   - [SAnd s1 s2] — the conjunction of two signatures. A holder of [SAnd]
                    is one who possesses both component signatures.

   Decidable equality on signatures is essential because reduction rules
   need to compare the signature on a token gate with the signature on a
   signed process to decide whether the gate fires.                          *)

Inductive sig : Type :=
  | SUnit  : sig                    (* () — unit signature *)
  | SHash  : list bool -> sig       (* hash(σ) — atomic signature derived from byte string *)
  | SAnd   : sig -> sig -> sig.     (* s₁ & s₂ — compound signature *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Tokens
   ═══════════════════════════════════════════════════════════════════════════

   A [token] is a right-nested list of signature gates, terminated by the
   unit constructor. Concretely, a token of shape

       TGate s₁ (TGate s₂ (TGate s₃ TUnit))

   represents three fuel units, each guarded by its own signature. The
   reduction rules of the cost-accounted calculus consume one gate per
   step by matching the outermost signature against an authenticating
   signed process. When the outer gate is stripped, the remaining token
   is the suffix that follows it.                                          *)

Inductive token : Type :=
  | TUnit : token                   (* () — empty token (no remaining fuel) *)
  | TGate : sig -> token -> token.  (* s:T — signed gate over remaining balance *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Systems
   ═══════════════════════════════════════════════════════════════════════════

   A [system] is the top-level term of the cost-accounted calculus. It
   layers signatures and tokens on top of the pure rho calculus processes
   provided by [RhoSyntax]:

   - [SSigned P s] — the process [P] sealed under the signature [s]. The
                     signature acts as an authorisation: only a token gate
                     bearing this signature may interact with [P].
   - [SToken t]    — a free standing token (a stack of fuel) available for
                     consumption by signed processes elsewhere in the system.
   - [SPar S1 S2]  — parallel composition of two systems. The total fuel of
                     a parallel composition is the sum of the fuel in its
                     components, a property formalised below as
                     [system_token_count_par].                              *)

Inductive system : Type :=
  | SSigned : proc -> sig -> system          (* P^s — process P signed under s *)
  | SToken  : token -> system                (* T — free token in the system *)
  | SPar    : system -> system -> system.    (* S₁ ∥ S₂ — parallel system composition *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Decidable Equality
   ═══════════════════════════════════════════════════════════════════════════

   Decidable equality on each of the three new categories. Signatures and
   tokens are simple algebraic data types and yield to [decide equality]
   directly (after teaching it how to compare lists of booleans). Systems
   reference the [proc] type from [RhoSyntax], so we feed the existing
   [proc_eq_dec] lemma to [decide equality] as a hint, alongside the
   freshly minted [sig_eq_dec] and [token_eq_dec].                          *)

Definition sig_eq_dec : forall (s1 s2 : sig), {s1 = s2} + {s1 <> s2}.
Proof.
  decide equality.
  - decide equality. decide equality.
Defined.

Definition token_eq_dec : forall (t1 t2 : token), {t1 = t2} + {t1 <> t2}.
Proof.
  decide equality.
  apply sig_eq_dec.
Defined.

Definition system_eq_dec : forall (S1 S2 : system), {S1 = S2} + {S1 <> S2}.
Proof.
  decide equality.
  - apply sig_eq_dec.
  - apply proc_eq_dec.
  - apply token_eq_dec.
Defined.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Size Functions
   ═══════════════════════════════════════════════════════════════════════════

   Size functions measure the structural complexity of cost-accounted
   terms.

   - [sig_size]            — counts constructors in a signature; bounded
                             below by 1 because every signature has at
                             least one constructor.
   - [token_size]          — counts the gates in a token, which is
                             precisely the amount of fuel it carries. The
                             empty token has size 0.
   - [system_token_count]  — sums the fuel of all free tokens contained in
                             a system. Signed processes contribute zero
                             because their signature is an authorisation,
                             not a fuel deposit. This function is the
                             measure used by TokenConservation.v to prove
                             that every reduction step preserves total fuel.
                                                                            *)

Fixpoint sig_size (s : sig) : nat :=
  match s with
  | SUnit       => 1
  | SHash _     => 1
  | SAnd s1 s2  => 1 + sig_size s1 + sig_size s2
  end.

Fixpoint token_size (t : token) : nat :=
  match t with
  | TUnit       => 0
  | TGate _ t'  => 1 + token_size t'
  end.

Fixpoint system_token_count (S : system) : nat :=
  match S with
  | SSigned _ _ => 0
  | SToken t    => token_size t
  | SPar S1 S2  => system_token_count S1 + system_token_count S2
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Helper Lemmas
   ═══════════════════════════════════════════════════════════════════════════

   Small but useful facts about the size functions. These are the building
   blocks consumed by later modules (especially TokenConservation.v) when
   reasoning about how reduction steps interact with the fuel measure.    *)

Lemma sig_size_pos : forall s, sig_size s >= 1.
Proof.
  induction s; simpl; lia.
Qed.

Lemma token_size_unit : token_size TUnit = 0.
Proof. reflexivity. Qed.

Lemma token_size_gate : forall s t, token_size (TGate s t) = S (token_size t).
Proof. intros. simpl. reflexivity. Qed.

Lemma system_token_count_par :
  forall S1 S2, system_token_count (SPar S1 S2)
              = system_token_count S1 + system_token_count S2.
Proof. intros. simpl. reflexivity. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: Notation
   ═══════════════════════════════════════════════════════════════════════════

   Convenient surface syntax. We use [^] for signed processes (matching the
   paper notation P^s) and the Unicode parallel-bars [∥] for system-level
   parallel composition. Note that this is intentionally distinct from the
   [|] used at the process level so that the two layers can be
   syntactically distinguished in proofs and goals.                        *)

(* Use a custom notation to avoid conflict with stdlib's "^" power operator. *)
Notation "P '⟨^⟩' s" := (SSigned P s) (at level 60).
Notation "S1 '∥' S2" := (SPar S1 S2) (at level 65, left associativity).
