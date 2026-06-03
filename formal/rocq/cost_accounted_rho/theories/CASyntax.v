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
From Stdlib Require Import Lists.List.
Import ListNotations.
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
  | CPJoin   : list caname -> signed_term -> caproc (* for(y1<-x1 & … & yN<-xN){T}
                                                       — N-ary join (spec §4.8 Def 4.6);
                                                       channels only (binders yi positional,
                                                       de Bruijn yi := CNVar (N-i)). *)
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

(* ── Section 2b: Forall-enriched deep induction ─────────────────────────────
   The combined scheme treats the [CPJoin] channel list opaquely (no per-element
   IH for [list caname]). This principle supplies [Forall Pn xs] in the join
   case, so list-valued obligations (map over the channels) discharge by a
   Forall-induction calling the [caname] IH. *)

Section CADeepInd.
  Variables (Pp : caproc -> Prop) (Pn : caname -> Prop) (Ps : signed_term -> Prop).
  (* Interleaved (arg, IH, …) to mirror [ca_mutind] exactly, so existing
     [ca_mutind] proofs become [ca_deep_ind] proofs with only a new CPJoin case. *)
  Hypothesis Hnil   : Pp CPNil.
  Hypothesis Hinp   : forall x, Pn x -> forall T, Ps T -> Pp (CPInput x T).
  Hypothesis Hout   : forall x, Pn x -> forall U, Ps U -> Pp (CPOutput x U).
  Hypothesis Hpar   : forall P1, Pp P1 -> forall P2, Pp P2 -> Pp (CPPar P1 P2).
  Hypothesis Href   : forall c, Pn c -> Pp (CPDeref c).
  Hypothesis Hjoin  : forall xs, Forall Pn xs -> forall T, Ps T -> Pp (CPJoin xs T).
  Hypothesis Hquote : forall T, Ps T -> Pn (CQuote T).
  Hypothesis Hnvar  : forall n, Pn (CNVar n).
  Hypothesis Hsig   : forall P, Pp P -> forall s, Ps (STSigned P s).
  Hypothesis Hstpar : forall T1, Ps T1 -> forall T2, Ps T2 -> Ps (STPar T1 T2).
  Hypothesis Hstack : forall t, Ps (STStack t).

  Fixpoint caproc_deep (P : caproc) : Pp P :=
    match P with
    | CPNil        => Hnil
    | CPInput x T  => Hinp x (caname_deep x) T (st_deep T)
    | CPOutput x U => Hout x (caname_deep x) U (st_deep U)
    | CPPar P1 P2  => Hpar P1 (caproc_deep P1) P2 (caproc_deep P2)
    | CPDeref x    => Href x (caname_deep x)
    | CPJoin xs T  =>
        Hjoin xs
          ((fix lcn (l : list caname) : Forall Pn l :=
              match l with
              | nil       => Forall_nil Pn
              | cons x l' => Forall_cons x (caname_deep x) (lcn l')
              end) xs)
          T (st_deep T)
    end
  with caname_deep (x : caname) : Pn x :=
    match x with
    | CQuote T => Hquote T (st_deep T)
    | CNVar n  => Hnvar n
    end
  with st_deep (T : signed_term) : Ps T :=
    match T with
    | STSigned P s => Hsig P (caproc_deep P) s
    | STPar T1 T2  => Hstpar T1 (st_deep T1) T2 (st_deep T2)
    | STStack t    => Hstack t
    end.
End CADeepInd.

Definition ca_deep_ind
  (Pp : caproc -> Prop) (Pn : caname -> Prop) (Ps : signed_term -> Prop)
  (Hnil : Pp CPNil)
  (Hinp : forall x, Pn x -> forall T, Ps T -> Pp (CPInput x T))
  (Hout : forall x, Pn x -> forall U, Ps U -> Pp (CPOutput x U))
  (Hpar : forall P1, Pp P1 -> forall P2, Pp P2 -> Pp (CPPar P1 P2))
  (Href : forall c, Pn c -> Pp (CPDeref c))
  (Hjoin : forall xs, Forall Pn xs -> forall T, Ps T -> Pp (CPJoin xs T))
  (Hquote : forall T, Ps T -> Pn (CQuote T))
  (Hnvar : forall n, Pn (CNVar n))
  (Hsig : forall P, Pp P -> forall s, Ps (STSigned P s))
  (Hstpar : forall T1, Ps T1 -> forall T2, Ps T2 -> Ps (STPar T1 T2))
  (Hstack : forall t, Ps (STStack t))
  : (forall P, Pp P) /\ (forall x, Pn x) /\ (forall T, Ps T) :=
  conj (caproc_deep Pp Pn Ps Hnil Hinp Hout Hpar Href Hjoin Hquote Hnvar Hsig Hstpar Hstack)
  (conj (caname_deep Pp Pn Ps Hnil Hinp Hout Hpar Href Hjoin Hquote Hnvar Hsig Hstpar Hstack)
        (st_deep Pp Pn Ps Hnil Hinp Hout Hpar Href Hjoin Hquote Hnvar Hsig Hstpar Hstack)).

(* ── Section 3: decidable equality (reuses sig_eq_dec / token_eq_dec) ─────── *)

Fixpoint caproc_eq_dec (P Q : caproc) : {P = Q} + {P <> Q}
with caname_eq_dec (x y : caname) : {x = y} + {x <> y}
with st_eq_dec (T U : signed_term) : {T = U} + {T <> U}.
Proof.
  - decide equality.
    (* CPJoin leftover: list caname equality (decide equality auto-handles the
       signed_term via st_eq_dec; only the channel list remains). *)
    match goal with
    | |- {?a = ?b} + {?a <> ?b} =>
        revert b; induction a as [| h t IHt]; intros [| h' t'];
          try (left; reflexivity); try (right; discriminate);
          destruct (caname_eq_dec h h') as [Hh | Hh];
            [ destruct (IHt t') as [Ht | Ht];
              [ left; subst; reflexivity
              | right; intro Heq; inversion Heq; contradiction ]
            | right; intro Heq; inversion Heq; contradiction ]
    end.
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
  | CPJoin xs T   => CPJoin (map (fun x => lift_caname d c x) xs)
                            (lift_st d (length xs + c) T)
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
  | CPJoin xs T   =>
      CPJoin (map (fun x => subst_caname x n N) xs)
             (subst_st T (length xs + n) (lift_caname (length xs) 0 N))
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
  apply ca_deep_ind; intros; simpl;
    repeat (match goal with H : forall _ : nat, _ = _ |- _ => rewrite H end);
    try reflexivity.
  - (* CPJoin: the channel list via Forall *)
    f_equal. induction H as [| x xs' Hx HF IH]; simpl;
      [ reflexivity | rewrite Hx, IH; reflexivity ].
  - (* CNVar *) destruct (c <=? n) eqn:Hcn.
    + rewrite Nat.add_0_r. reflexivity.
    + reflexivity.
Qed.

(* ── Section 8b: N-simultaneous substitution for the join continuation ───────
   [subst_st_many T Us] substitutes the quoted payloads @U1,…,@UN for the N
   binders y1,…,yN of a join, as a RIGHT FOLD of the binary [subst_st]: each
   step consumes the outermost binder (CNVar 0) and decrements the rest. The
   tail payloads are lifted by one level before the recursive call — per-step
   lifting keeps open payloads capture-free, so distinct binders are never
   conflated and the substitution is genuinely SIMULTANEOUS (Def 4.6) even for
   ARBITRARY, possibly-OPEN sent payloads. So the N-ary metatheory reduces to
   the binary lemma plus a list induction.

   Coq's guard checker does not see [map f Us'] as a structural subterm of
   [cons U Us'], so the naive [Fixpoint] recursing on the [map]ped tail is
   rejected ("Cannot guess decreasing argument of fix"). We instead recurse on
   the LITERAL tail [Us'] through the accumulator helper [subst_st_many_from k],
   which carries the per-step lift COUNT [k] and lifts the head [k] times
   ([Nat.iter k (lift_st 1 0)]) before substituting. [subst_st_many_cons] then
   recovers the intended one-step [map]-form unfolding equation (via the
   shift lemma [subst_st_many_from_lift]), so the metatheory reads exactly as
   the simultaneous-substitution design intends. *)

Fixpoint subst_st_many_from (k : nat) (T : signed_term) (Us : list signed_term)
  {struct Us} : signed_term :=
  match Us with
  | nil       => T
  | cons U Us' =>
      subst_st_many_from (S k) (subst_st T 0 (CQuote (Nat.iter k (lift_st 1 0) U))) Us'
  end.

Definition subst_st_many (T : signed_term) (Us : list signed_term) : signed_term :=
  subst_st_many_from 0 T Us.

Lemma subst_st_many_nil : forall T, subst_st_many T nil = T.
Proof. reflexivity. Qed.

Lemma subst_st_many_singleton : forall T U,
  subst_st_many T (cons U nil) = subst_st T 0 (CQuote U).
Proof. reflexivity. Qed.

(* Convention sanity (Risk R7): a concrete 2-ary fold is the nested double
   substitution, outermost binder first, the tail payload lifted by one. *)
Example subst_st_many_two : forall T U1 U2,
  subst_st_many T (cons U1 (cons U2 nil))
    = subst_st (subst_st T 0 (CQuote U1)) 0 (CQuote (lift_st 1 0 U2)).
Proof. reflexivity. Qed.

(* Shift lemma: starting the fold at count [S k] equals starting it at [k] with
   every payload pre-lifted once. The bridge [Nat.iter (S k) f U =
   Nat.iter k f (f U)] (one-more-application commutes) makes the substituted
   heads coincide step by step. *)
(* [Nat.iter] commutes with one extra application of its own function — the
   general [Nat.iter (S n) f x] read two ways. *)
Lemma iter_lift_commute : forall k U,
  Nat.iter k (fun V => lift_st 1 0 V) (lift_st 1 0 U)
    = lift_st 1 0 (Nat.iter k (fun V => lift_st 1 0 V) U).
Proof.
  intros k U. rewrite <- Nat.iter_succ_r. rewrite Nat.iter_succ. reflexivity.
Qed.

Lemma subst_st_many_from_lift : forall Us k T,
  subst_st_many_from (S k) T Us
    = subst_st_many_from k T (map (fun V => lift_st 1 0 V) Us).
Proof.
  induction Us as [| U Us' IH]; intros k T; simpl.
  - reflexivity.
  - (* both sides reduce to subst_st_many_from (S k) of the SAME substituted head:
       the LHS head is Nat.iter (S k) f U; the RHS head is Nat.iter k f (f U).
       Normalize the RHS head via iter_lift_commute + Nat.iter_succ, then IH. *)
    rewrite iter_lift_commute. rewrite <- Nat.iter_succ. apply IH.
Qed.

(* The intended one-step unfolding: peel the outermost binder, lift the tail
   payloads once (per-step lifting keeps open payloads capture-free), recurse. *)
Lemma subst_st_many_cons : forall U Us' T,
  subst_st_many T (cons U Us')
    = subst_st_many (subst_st T 0 (CQuote U)) (map (fun V => lift_st 1 0 V) Us').
Proof.
  intros U Us' T. unfold subst_st_many. simpl.
  rewrite subst_st_many_from_lift. reflexivity.
Qed.

Example dequote_collapses :
  forall (U : signed_term),
    subst_caproc (CPDeref (CNVar 0)) 0 (CQuote U) = st_to_proc U.
Proof. intro U. simpl. reflexivity. Qed.
