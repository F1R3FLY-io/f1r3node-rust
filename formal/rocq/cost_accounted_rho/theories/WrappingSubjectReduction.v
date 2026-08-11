(* ════════════════════════════════════════════════════════════════════════
   WrappingSubjectReduction.v — "no leak" as a sorting invariant (DR-21).

   Mechanizes the monad paper's Lemma "Subject reduction for wrapping" and
   Corollary "No leak, by construction" (continued-gslt-cost-v2.tex:461-480)
   for the native calculus.

   The decisive point of the native four-sort grammar: a [caproc] can ONLY occur
   inside an [STSigned] wrapper (a [signed_term] is STSigned/STPar/STStack), so
   "every redex occurrence lies inside a wrapper {·}_s" is a SORTING invariant —
   true of every term by construction, hence trivially preserved by reduction.
   The operational content of "no leak" is therefore: a wrapped redex cannot
   fire WITHOUT a co-present token gate (the cost-accounting steps only ever
   unwrap; there is no free reduction and no dynamic re-wrap). Axiom-free.      *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.

(* ── well-wrapping is a sorting invariant (universal by construction) ─────── *)

Fixpoint well_wrapped (T : signed_term) : Prop :=
  match T with
  | STSigned _ _ => True           (* a redex here is guarded by its wrapper *)
  | STPar T1 T2  => well_wrapped T1 /\ well_wrapped T2
  | STStack _    => True
  end.

Theorem well_wrapped_universal : forall T, well_wrapped T.
Proof. induction T; simpl; auto. Qed.

(* Subject reduction for wrapping: well-wrapping is preserved by [ca_step].
   By construction every continuation slot has sort [signed_term], so the
   residual is again well-wrapped — the monad paper's "for free". *)
Theorem subject_reduction_wrapping :
  forall S S', ca_step S S' -> well_wrapped S -> well_wrapped S'.
Proof. intros S S' _ _. apply well_wrapped_universal. Qed.

(* ── no leak: a redex cannot fire without consuming a token gate ─────────── *)

(* A lone wrapped redex never steps: every rule requires a co-present token
   stack [STStack (TGate ...)], so a wrapper in isolation is stuck. This is the
   operational "no leak" — no redex fires without being unwrapped by a token. *)
Theorem no_leak_requires_token :
  forall (P : caproc) (s : sig) (S' : signed_term), ~ ca_step (STSigned P s) S'.
Proof. intros P s S' H. inversion H. Qed.

(* A lone token stack never steps either (tokens are inert under reduction). *)
Theorem no_leak_stack_inert :
  forall (t : token) (S' : signed_term), ~ ca_step (STStack t) S'.
Proof. intros t S' H. inversion H. Qed.

(* ── GAP-2 dissolved: the split-process COMM keeps the continuation's seal ── *)

(* The old ca_rule4 / ca_rule5 re-sealed the bare-proc continuation under the
   COMPOUND `SAnd s1 s2`. Natively the receiver's continuation [T] is a
   signed_term carrying its OWN seal, so the residual is `T{@U/y}` with NO
   compound re-seal. These witnesses exhibit the residual shape: a bare
   `subst_st T 0 (CQuote U)`, never an `STSigned _ (SAnd s1 s2)` introduced by
   the rule. (Old [Rule45ContinuationAdequacy] proved that re-seal cost-benign;
   here the re-seal is simply absent.) *)
Corollary gap2_split_combined_keeps_own_seal :
  forall (x : caname) (T U : signed_term) (s1 s2 : sig) (t : token),
    ca_step
      (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
             (STStack (TGate (SAnd s1 s2) t)))
      (STPar (subst_st T 0 (CQuote U)) (STStack t)).
Proof. intros. apply ca_rule4. Qed.

Corollary gap2_split_split_keeps_own_seal :
  forall (x : caname) (T U : signed_term) (s1 s2 : sig) (t1 t2 : token),
    ca_step
      (STPar (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
                    (STStack (TGate s1 t1)))
             (STStack (TGate s2 t2)))
      (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2)).
Proof. intros. apply ca_rule5. Qed.
