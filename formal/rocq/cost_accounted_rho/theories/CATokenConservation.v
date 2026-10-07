(* ════════════════════════════════════════════════════════════════════════
   CATokenConservation.v — native fuel measures + conservation (DR-21 Stage 3).

   The old [TokenConservation]'s `token_strictly_decreases` is FALSE natively:
   a `for(y<-x){T}` whose continuation T is a located purse (STStack t') RELEASES
   spine fuel when it fires, and `st_token_count` can strictly increase. So the
   native termination/conservation story is genuinely different (and richer): it
   is CONDITIONAL on the linearly-funded fragment (the consensus-relevant class —
   only funded deploys are admitted, LinearLogicResources strict-reject).

   This module supplies the measures and the unconditional facts:
   - [st_total_fuel]: counts ALL gates (guarded ones too) — the SN measure base.
   - [st_token_count_subst_invariant]: substitution preserves the free spine
     count (it duplicates no top-level tokens — the spine-invariance cornerstone).
   - [ca_step_needs_fuel]: no step without a co-present gate (unconditional).
   - [deref_count_*] / [linear_cont] / [funded_linear]: the linear-funding
     discipline (each received quote forced at most once + no self-replenishing
     purse), the term-level image of LinearLogicResources' no-contraction.
   The conditional strict-decrease / SN / confluence live in CAStrongNormalization
   and CAConfluence. Axiom-free.                                                *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CABinding.
From CostAccountedRho Require Import CAReduction.

(* ── Section 1: total fuel — counts EVERY gate, guarded or free ──────────── *)

Fixpoint caproc_total_fuel (P : caproc) : nat :=
  match P with
  | CPNil         => 0
  | CPInput x T   => caname_total_fuel x + st_total_fuel T
  | CPOutput x U  => caname_total_fuel x + st_total_fuel U
  | CPPar P1 P2   => caproc_total_fuel P1 + caproc_total_fuel P2
  | CPDeref x     => caname_total_fuel x
  | CPJoin xs T   => fold_right (fun x acc => caname_total_fuel x + acc) 0 xs
                     + st_total_fuel T
  end
with caname_total_fuel (x : caname) : nat :=
  match x with
  | CQuote T => st_total_fuel T
  | CNVar _  => 0
  end
with st_total_fuel (T : signed_term) : nat :=
  match T with
  | STSigned P _ => caproc_total_fuel P
  | STPar T1 T2  => st_total_fuel T1 + st_total_fuel T2
  | STStack t    => token_size t
  end.

(* ── Section 2: spine-invariance of substitution ─────────────────────────
   [st_token_count] (the FREE spine count, STSigned => 0) is unchanged by
   substitution: subst_st only rewrites guarded caproc interiors and is the
   identity on STStack, so it duplicates no top-level tokens. *)

Lemma st_token_count_subst_invariant :
  forall (T : signed_term) (n : nat) (N : caname),
    st_token_count (subst_st T n N) = st_token_count T.
Proof.
  intros T n N. induction T as [P s | T1 IH1 T2 IH2 | t]; simpl.
  - reflexivity.                 (* STSigned: both sides 0 *)
  - rewrite IH1, IH2. reflexivity.
  - reflexivity.                 (* STStack: subst is identity here *)
Qed.

(* ── Section 3: no step without fuel (unconditional no-leak, quantitative) ─
   Every ca_step requires a co-present token gate, so the total fuel of any
   reducible term is at least 1. (The qualitative form is
   WrappingSubjectReduction.no_leak_requires_token.) *)

Lemma ca_step_needs_fuel : forall S S', ca_step S S' -> 1 <= st_total_fuel S.
Proof.
  intros S S' H. induction H; simpl in *; lia.
Qed.

(* ── Section 4: the linear-funding discipline ───────────────────────────── *)

(* Number of FORCING occurrences (dereferences) of the de Bruijn name [n].
   The CPInput binder shifts the index in its continuation. *)
Fixpoint deref_count_caproc (n : nat) (P : caproc) : nat :=
  match P with
  | CPNil         => 0
  | CPInput x T   => deref_count_caname n x + deref_count_st (S n) T
  | CPOutput x U  => deref_count_caname n x + deref_count_st n U
  | CPPar P1 P2   => deref_count_caproc n P1 + deref_count_caproc n P2
  | CPDeref x     => deref_count_caname n x
  | CPJoin xs T   => fold_right (fun x acc => deref_count_caname n x + acc) 0 xs
                     + deref_count_st (length xs + n) T
  end
with deref_count_caname (n : nat) (x : caname) : nat :=
  match x with
  | CQuote T => deref_count_st n T
  | CNVar k  => if Nat.eqb k n then 1 else 0
  end
with deref_count_st (n : nat) (T : signed_term) : nat :=
  match T with
  | STSigned P _ => deref_count_caproc n P
  | STPar T1 T2  => deref_count_st n T1 + deref_count_st n T2
  | STStack _    => 0
  end.

(* A continuation is LINEAR at its bound variable when it forces it at most once
   — the term-level image of LinearLogicResources' no-contraction. *)
Definition linear_cont (T : signed_term) : Prop := deref_count_st 0 T <= 1.

(* A term is LINEARLY FUNDED when every for-continuation is linear, no
   for-continuation is a self-replenishing purse (a bare STStack continuation
   must be the empty stack — the restrictive, provably-sound clause; terminal
   located purses are admitted as STStack TUnit), AND every SEND carries a CLOSED
   payload ([closed_st U] in the CPOutput clause). The send-closedness is what the
   N-ary join's strong-normalization argument consumes (the join fires for
   arbitrary/open payloads operationally, but SN needs the consumed payloads to be
   closed so the simultaneous substitution adds no spurious fuel — see
   [linear_subst_many_fuel_le] and [funded_step_decreases]); transmitted values in
   a funded deploy are closed, so this is no real restriction on the funded class.
   Recursive over the structure.  *)
Fixpoint funded_linear_caproc (P : caproc) : Prop :=
  match P with
  | CPNil         => True
  | CPInput x T   =>
      linear_cont T /\ funded_linear_caname x /\ funded_linear_st T
  | CPOutput x U  => funded_linear_caname x /\ closed_st U /\ funded_linear_st U
  | CPPar P1 P2   => funded_linear_caproc P1 /\ funded_linear_caproc P2
  | CPDeref x     => funded_linear_caname x
  | CPJoin xs T   =>
      (forall i, i < length xs -> deref_count_st i T <= 1)
      /\ fold_right (fun x acc => funded_linear_caname x /\ acc) True xs
      /\ funded_linear_st T
  end
with funded_linear_caname (x : caname) : Prop :=
  match x with
  | CQuote T => funded_linear_st T
  | CNVar _  => True
  end
with funded_linear_st (T : signed_term) : Prop :=
  match T with
  | STSigned P _ => funded_linear_caproc P
  | STPar T1 T2  => funded_linear_st T1 /\ funded_linear_st T2
  | STStack _    => True
  end.

Definition funded_linear (T : signed_term) : Prop := funded_linear_st T.

(* funded_linear is decidable (deref_count is a nat fixpoint; the structure is
   finite) — the basis for the acceptance gate's static check. *)
Lemma linear_cont_dec : forall T, {linear_cont T} + {~ linear_cont T}.
Proof. intro T. unfold linear_cont. apply le_dec. Qed.

(* ── send-closedness extraction for the N-ary join (Risk: the SN bridge) ─────
   The funded fragment now records [closed_st] of every send (the strengthened
   CPOutput clause). These two lemmas read that closedness back out of a join's
   sender bundle, so [funded_step_decreases]'s join cases can feed [Forall
   closed_st Us] to [linear_subst_many_fuel_le] WITHOUT a rule premise. The
   matching-arity hypothesis [length xs = length Us] is required: at mismatched
   arities [join_sends]/[signed_sends] truncate to an inert nil whose fundedness
   says nothing about the surplus payloads — but the join rules always supply the
   length equation, so this is exactly the operative case. *)
Lemma funded_join_sends_closed : forall xs Us,
  length xs = length Us ->
  funded_linear_caproc (join_sends xs Us) -> Forall closed_st Us.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] Hlen Hf; simpl in *;
    try discriminate.
  - constructor.
  - destruct Hf as [Hhd Htl]. destruct Hhd as [_ [HclU _]].
    constructor; [ exact HclU | apply IH; [ lia | exact Htl ] ].
Qed.

Lemma funded_signed_sends_closed : forall xs Us ts,
  length xs = length Us -> length xs = length ts ->
  funded_linear_st (signed_sends xs Us ts) -> Forall closed_st Us.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] [| t ts'] HU Ht Hf;
    simpl in *; try discriminate.
  - constructor.
  - destruct Hf as [Hhd Htl]. destruct Hhd as [_ [HclU _]].
    constructor; [ exact HclU | apply IH with (ts := ts'); [ lia | lia | exact Htl ] ].
Qed.
