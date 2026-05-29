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
   SGround bs               │ g (ground signature)  │ Sig::Ground(bytes)
   SQuote bs                │ #P (cryptographic     │ Sig::Quote(bytes)
                            │     quote of process) │
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
   - [SGround bs] — a GROUND signature [g ∈ G] (Def 3.3): an opaque value
                    drawn from the cryptographic backend (e.g. an Ed25519
                    public key, a secp256k1 key hash). We model it as the
                    byte string [bs] to remain free of any external library
                    dependency.
   - [SQuote bs]  — a CRYPTOGRAPHIC QUOTE [#P] (Def 3.3): the hash of a
                    process, the authentication-axis analogue of structural
                    quoting [@P]. We model it as the byte string [bs] (the
                    digest), and realise [FN_s(#P) = FN(P)] at the source
                    level (see SystemStructEquiv.v) via the [hash_process]
                    bridge rather than carrying a [proc] inside the atom.
   - [SAnd s1 s2] — the conjunction of two signatures. A holder of [SAnd]
                    is one who possesses both component signatures.

   The two atomic byte-carrying constructors [SGround] and [SQuote] are the
   two axes of Definition 3.3's signature grammar
   [s(G) ::= g | #P | s & s]: [SGround] is the ground axis [g] and [SQuote]
   is the cryptographic-quote axis [#P]. Cost behaviour is IDENTICAL for the
   two — every cost rule treats a signature opaquely and each atomic
   signature gates exactly one fuel token — so the split is a matter of
   grammar fidelity (Def 3.3) and the wire-level [AtomKind], not of cost.

   Decidable equality on signatures is essential because reduction rules
   need to compare the signature on a token gate with the signature on a
   signed process to decide whether the gate fires.                          *)

Inductive sig : Type :=
  | SUnit   : sig                   (* () — unit signature *)
  | SGround : list bool -> sig      (* g — ground signature (Def 3.3 ground axis) *)
  | SQuote  : list bool -> sig      (* #P — cryptographic quote (Def 3.3 quote axis) *)
  | SAnd    : sig -> sig -> sig.    (* s₁ & s₂ — compound signature *)

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
  | SGround _   => 1
  | SQuote _    => 1
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

Inductive sig_choice : Type :=
  | ChooseLeft
  | ChooseRight.

Definition sig_choice_eq_dec :
  forall (c1 c2 : sig_choice), {c1 = c2} + {c1 <> c2}.
Proof.
  decide equality.
Defined.

(* The runtime signature-algebra carrier. Atoms are split along the same
   two Def-3.3 axes as [sig]: [ASGround] (ground axis [g]) and [ASQuote]
   (cryptographic-quote axis [#P]). Both atom arms behave IDENTICALLY in
   every fixpoint below — each contributes a single required/presented atom
   — because the linear-resource demand [Δ_s] is insensitive to the axis. *)
Inductive sig_algebra : Type :=
  | ASUnit : sig_algebra
  | ASGround : nat -> sig_algebra
  | ASQuote : nat -> sig_algebra
  | ASAnd : sig_algebra -> sig_algebra -> sig_algebra
  | ASThreshold : nat -> list sig_algebra -> sig_algebra
  | ASPlus : sig_choice -> sig_algebra -> sig_algebra -> sig_algebra
  | ASWith : sig_algebra -> sig_algebra -> sig_algebra
  | ASBang : sig_algebra -> sig_algebra
  | ASWhyNot : sig_algebra -> sig_algebra
  | ASLolly : sig_algebra -> sig_algebra -> sig_algebra.

Fixpoint sig_algebra_atoms (s : sig_algebra) : list nat :=
  match s with
  | ASUnit => []
  | ASGround a => [a]
  | ASQuote a => [a]
  | ASAnd s1 s2 => sig_algebra_atoms s1 ++ sig_algebra_atoms s2
  | ASThreshold _ members => concat (map sig_algebra_atoms members)
  | ASPlus _ s1 s2 => sig_algebra_atoms s1 ++ sig_algebra_atoms s2
  | ASWith s1 s2 => sig_algebra_atoms s1 ++ sig_algebra_atoms s2
  | ASBang s' => sig_algebra_atoms s'
  | ASWhyNot s' => sig_algebra_atoms s'
  | ASLolly s1 s2 => sig_algebra_atoms s1 ++ sig_algebra_atoms s2
  end.

Fixpoint sig_algebra_min_required (s : sig_algebra) : nat :=
  match s with
  | ASUnit => 0
  | ASGround _ => 1
  | ASQuote _ => 1
  | ASAnd s1 s2 => sig_algebra_min_required s1 + sig_algebra_min_required s2
  | ASThreshold k _ => k
  | ASPlus ChooseLeft s1 _ => sig_algebra_min_required s1
  | ASPlus ChooseRight _ s2 => sig_algebra_min_required s2
  | ASWith s1 s2 => sig_algebra_min_required s1 + sig_algebra_min_required s2
  | ASBang s' => sig_algebra_min_required s'
  | ASWhyNot _ => 0
  | ASLolly s1 s2 => sig_algebra_min_required s1 + sig_algebra_min_required s2
  end.

Fixpoint sig_algebra_all_required (s : sig_algebra) : bool :=
  match s with
  | ASUnit => true
  | ASGround _ => true
  | ASQuote _ => true
  | ASAnd s1 s2 => sig_algebra_all_required s1 && sig_algebra_all_required s2
  | ASThreshold _ _ => false
  | ASPlus _ _ _ => false
  | ASWith s1 s2 => sig_algebra_all_required s1 && sig_algebra_all_required s2
  | ASBang s' => sig_algebra_all_required s'
  | ASWhyNot _ => false
  | ASLolly s1 s2 => sig_algebra_all_required s1 && sig_algebra_all_required s2
  end.

Fixpoint sig_algebra_valid (s : sig_algebra) : bool :=
  match s with
  | ASUnit => true
  | ASGround _ => true
  | ASQuote _ => true
  | ASAnd s1 s2 => sig_algebra_valid s1 && sig_algebra_valid s2
  | ASThreshold k members =>
      (1 <=? k) && (k <=? length members) && forallb sig_algebra_valid members
  | ASPlus _ s1 s2 => sig_algebra_valid s1 && sig_algebra_valid s2
  | ASWith s1 s2 => sig_algebra_valid s1 && sig_algebra_valid s2
  | ASBang s' => sig_algebra_valid s'
  | ASWhyNot s' => sig_algebra_valid s'
  | ASLolly s1 s2 => sig_algebra_valid s1 && sig_algebra_valid s2
  end.

Fixpoint sig_algebra_presented_atoms (s : sig_algebra) : list nat :=
  match s with
  | ASUnit => []
  | ASGround a => [a]
  | ASQuote a => [a]
  | ASAnd s1 s2 => sig_algebra_presented_atoms s1 ++ sig_algebra_presented_atoms s2
  | ASThreshold _ members => concat (map sig_algebra_presented_atoms members)
  | ASPlus ChooseLeft s1 _ => sig_algebra_presented_atoms s1
  | ASPlus ChooseRight _ s2 => sig_algebra_presented_atoms s2
  | ASWith s1 s2 => sig_algebra_presented_atoms s1 ++ sig_algebra_presented_atoms s2
  | ASBang s' => sig_algebra_presented_atoms s'
  | ASWhyNot _ => []
  | ASLolly s1 s2 => sig_algebra_presented_atoms s1 ++ sig_algebra_presented_atoms s2
  end.

Lemma sig_algebra_plus_left_min_required :
  forall s1 s2,
    sig_algebra_min_required (ASPlus ChooseLeft s1 s2) =
    sig_algebra_min_required s1.
Proof. reflexivity. Qed.

Lemma sig_algebra_plus_right_min_required :
  forall s1 s2,
    sig_algebra_min_required (ASPlus ChooseRight s1 s2) =
    sig_algebra_min_required s2.
Proof. reflexivity. Qed.

Lemma sig_algebra_with_min_required :
  forall s1 s2,
    sig_algebra_min_required (ASWith s1 s2) =
    sig_algebra_min_required s1 + sig_algebra_min_required s2.
Proof. reflexivity. Qed.

Lemma sig_algebra_bang_min_required :
  forall s,
    sig_algebra_min_required (ASBang s) = sig_algebra_min_required s.
Proof. reflexivity. Qed.

Lemma sig_algebra_whynot_min_required_zero :
  forall s, sig_algebra_min_required (ASWhyNot s) = 0.
Proof. reflexivity. Qed.

Lemma sig_algebra_lolly_min_required :
  forall s_from s_to,
    sig_algebra_min_required (ASLolly s_from s_to) =
    sig_algebra_min_required s_from + sig_algebra_min_required s_to.
Proof. reflexivity. Qed.

Lemma sig_algebra_threshold_min_required :
  forall k members,
    sig_algebra_min_required (ASThreshold k members) = k.
Proof. reflexivity. Qed.

Lemma sig_algebra_threshold_valid_bounds :
  forall k members,
    sig_algebra_valid (ASThreshold k members) = true ->
    1 <= k /\ k <= length members.
Proof.
  intros k members Hvalid.
  cbn in Hvalid.
  apply andb_prop in Hvalid as [Hbounds _].
  apply andb_prop in Hbounds as [Hlower Hupper].
  split; apply Nat.leb_le; assumption.
Qed.

Lemma sig_algebra_all_required_min_required_atoms :
  forall s,
    sig_algebra_all_required s = true ->
    sig_algebra_min_required s = length (sig_algebra_atoms s).
Proof.
  induction s as
    [|a|a|s1 IH1 s2 IH2|k members|choice s1 IH1 s2 IH2
     |s1 IH1 s2 IH2|s IH|s IH|s1 IH1 s2 IH2]; intros Hall; cbn in *.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - apply andb_prop in Hall as [H1 H2].
    rewrite IH1 by exact H1. rewrite IH2 by exact H2.
    rewrite app_length. reflexivity.
  - discriminate.
  - discriminate.
  - apply andb_prop in Hall as [H1 H2].
    rewrite IH1 by exact H1. rewrite IH2 by exact H2.
    rewrite app_length. reflexivity.
  - apply IH. exact Hall.
  - discriminate.
  - apply andb_prop in Hall as [H1 H2].
    rewrite IH1 by exact H1. rewrite IH2 by exact H2.
    rewrite app_length. reflexivity.
Qed.


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
