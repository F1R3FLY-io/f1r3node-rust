(* ════════════════════════════════════════════════════════════════════════
   CASyntax.v — Native four-sort cost-accounted syntax (DR-17/DR-21 Option B).

   The spec's §3.1 grammar (cost-accounted-rho.tex:611-680) is a four-sort
   mutually-inductive syntax in which `for`/`send` continuation bodies are
   themselves SIGNED TERMS ("signed terms pervade the syntax", §1; Remark 3.8
   tex:638-647). The monad paper's "wrapping by construction"
   (continued-gslt-cost-v2.tex:376-409) makes the payoff precise: every
   continuation slot has the wrapped-term sort 𝕋, so "no leak" is a sorting
   invariant rather than a dynamic obligation.

   This module realizes that grammar natively. Per the carrier-split design
   (DR-21), the PURE rho calculus `proc`/`name` of [RhoSyntax] is kept UNCHANGED
   as the translation TARGET; this module introduces the cost-accounted SOURCE
   as three new mutually-inductive sorts reusing the existing [sig] and [token]
   of [CostAccountedSyntax]:

     - [caproc]      P ::= 0 | for(y<-x){T} | x!(U) | P|P | *x
     - [caname]      x ::= @T | y                  (names quote SIGNED TERMS)
     - [signed_term] T ::= {P}_s | T∥U | S         (the wrapped-term sort 𝕋)

   The token stack S of the paper IS the existing [token] = `() | s:S`
   ([CostAccountedSyntax] TUnit/TGate), reused via [STStack]; tokens carry no
   de Bruijn names, so binding/substitution never recurse into them. The
   wrapper is named [STSigned] (the old [system] constructor [SSigned] remains
   in scope during the incremental migration; the names must not clash).

   Stage 1 of the migration: parallel module, nothing downstream imports it yet,
   so the proof gate stays green. Axiom-free.                                  *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.

(* ── Section 1: the three native mutually-inductive sorts ───────────────────
   Reuses [sig] and [token] from CostAccountedSyntax; [token] is the paper's
   token stack `() | s:S`. *)

Inductive caproc : Type :=
  | CPNil    : caproc                              (* 0 *)
  | CPInput  : caname -> signed_term -> caproc     (* for(y<-x){T}: cont. is 𝕋 *)
  | CPOutput : caname -> signed_term -> caproc     (* x!(U): payload is 𝕋 *)
  | CPPar    : caproc -> caproc -> caproc          (* P | Q *)
  | CPDeref  : caname -> caproc                     (* *x *)
with caname : Type :=
  | CQuote   : signed_term -> caname               (* @T: quote of a signed term *)
  | CNVar    : nat -> caname                        (* bound name var (de Bruijn) *)
with signed_term : Type :=
  | STSigned : caproc -> sig -> signed_term         (* {P}_s — the wrapper 𝕋 *)
  | STPar    : signed_term -> signed_term -> signed_term (* T ∥ U *)
  | STStack  : token -> signed_term.                (* S — a token stack is a 𝕋 *)

(* ── Section 2: the 3-way combined induction scheme ─────────────────────────
   (token is non-mutual and already has its own induction principle.) *)

Scheme caproc_ind_mut := Induction for caproc Sort Prop
  with caname_ind_mut := Induction for caname Sort Prop
  with st_ind_mut     := Induction for signed_term Sort Prop.

Combined Scheme ca_mutind from caproc_ind_mut, caname_ind_mut, st_ind_mut.

(* ── Section 3: decidable equality (reuses sig_eq_dec / token_eq_dec) ─────── *)

Fixpoint caproc_eq_dec (P Q : caproc) : {P = Q} + {P <> Q}
with caname_eq_dec (x y : caname) : {x = y} + {x <> y}
with st_eq_dec (T U : signed_term) : {T = U} + {T <> U}.
Proof.
  - decide equality.
  - decide equality. apply Nat.eq_dec.
  - decide equality; [ apply sig_eq_dec | apply token_eq_dec ].
Defined.

(* ── Section 4: lifting (shifting) de Bruijn indices ────────────────────────
   3-way mutual over caproc/caname/signed_term; [token] carries no names, so
   [STStack t] is left untouched. [CPInput] binds [CNVar 0] in its continuation,
   so the cutoff increments under it (mirrors RhoSyntax.lift_proc:121). *)

Fixpoint lift_caproc (d c : nat) (P : caproc) : caproc :=
  match P with
  | CPNil         => CPNil
  | CPInput x T   => CPInput (lift_caname d c x) (lift_st d (S c) T)
  | CPOutput x U  => CPOutput (lift_caname d c x) (lift_st d c U)
  | CPPar P1 P2   => CPPar (lift_caproc d c P1) (lift_caproc d c P2)
  | CPDeref x     => CPDeref (lift_caname d c x)
  end
with lift_caname (d c : nat) (x : caname) : caname :=
  match x with
  | CQuote T => CQuote (lift_st d c T)
  | CNVar k  => if c <=? k then CNVar (k + d) else CNVar k
  end
with lift_st (d c : nat) (T : signed_term) : signed_term :=
  match T with
  | STSigned P s => STSigned (lift_caproc d c P) s
  | STPar T1 T2  => STPar (lift_st d c T1) (lift_st d c T2)
  | STStack t    => STStack t
  end.

(* ── Section 5: the dequote force target ────────────────────────────────────
   [st_to_proc T] extracts the process content of a signed term — the residue
   of forcing a quote `*(@T)`. A token stack carries no process residue. This
   is the resolution of the dequote sort-tension (Risk R4): the dequote of a
   quoted signed term `*x{@U/y}` lands in [caproc] via [st_to_proc]. *)

Fixpoint st_to_proc (T : signed_term) : caproc :=
  match T with
  | STSigned P _ => P
  | STPar T1 T2  => CPPar (st_to_proc T1) (st_to_proc T2)
  | STStack _    => CPNil
  end.

(* ── Section 6: capture-avoiding substitution ───────────────────────────────
   Substitutes a quoted signed term @U for [CNVar n]. The only non-trivial case
   is dequotation: `*x{@U/y} = st_to_proc U` (semantic collapse, spec
   tex:806-812), preserving Remark 3.8 provenance (a received term keeps its
   signature through communication; the explicit force point is *x).
   3-way mutual; [STStack t] is left untouched (tokens carry no names). *)

Fixpoint subst_caproc (P : caproc) (n : nat) (N : caname) : caproc :=
  match P with
  | CPNil         => CPNil
  | CPInput x T   =>
      CPInput (subst_caname x n N) (subst_st T (S n) (lift_caname 1 0 N))
  | CPOutput x U  =>
      CPOutput (subst_caname x n N) (subst_st U n N)
  | CPPar P1 P2   => CPPar (subst_caproc P1 n N) (subst_caproc P2 n N)
  | CPDeref x     =>
      match x with
      | CNVar k =>
          match Nat.compare k n with
          | Lt => CPDeref (CNVar k)
          | Eq =>
              match N with
              | CQuote U => st_to_proc U          (* semantic collapse → caproc *)
              | CNVar _  => CPDeref N
              end
          | Gt => CPDeref (CNVar (k - 1))
          end
      | CQuote T => CPDeref (CQuote (subst_st T n N))
      end
  end
with subst_caname (x : caname) (n : nat) (N : caname) : caname :=
  match x with
  | CQuote T => CQuote (subst_st T n N)
  | CNVar k  =>
      match Nat.compare k n with
      | Lt => CNVar k
      | Eq => N
      | Gt => CNVar (k - 1)
      end
  end
with subst_st (T : signed_term) (n : nat) (N : caname) : signed_term :=
  match T with
  | STSigned P s => STSigned (subst_caproc P n N) s
  | STPar T1 T2  => STPar (subst_st T1 n N) (subst_st T2 n N)
  | STStack t    => STStack t
  end.

(* ── Section 7: the token-count measure ─────────────────────────────────────
   Direct analog of [system_token_count] (CostAccountedSyntax:208): a wrapped
   process contributes 0 (its tokens are guarded), a top-level stack contributes
   its [token_size], parallel composition sums. The SN / conservation measure.  *)

Fixpoint st_token_count (T : signed_term) : nat :=
  match T with
  | STSigned _ _ => 0
  | STPar T1 T2  => st_token_count T1 + st_token_count T2
  | STStack t    => token_size t
  end.

(* ── Section 8: sanity — the combined scheme is usable; dequote typechecks ── *)

Lemma lift_zero_ca :
  (forall P c, lift_caproc 0 c P = P)
  /\ (forall x c, lift_caname 0 c x = x)
  /\ (forall T c, lift_st 0 c T = T).
Proof.
  apply ca_mutind; intros; simpl;
    repeat (match goal with H : forall _ : nat, _ = _ |- _ => rewrite H end);
    try reflexivity.
  - (* CNVar *) destruct (c <=? n) eqn:Hcn.
    + rewrite Nat.add_0_r. reflexivity.
    + reflexivity.
Qed.

Example dequote_collapses :
  forall (U : signed_term),
    subst_caproc (CPDeref (CNVar 0)) 0 (CQuote U) = st_to_proc U.
Proof. intro U. simpl. reflexivity. Qed.
