(* ════════════════════════════════════════════════════════════════════════
   CAUntypedLambda.v — the untyped-λ "R1-only" cost instance (DR-25).

   The generic cost transform (the cost endofunctor on continued-interactive
   GSLTs; continued-gslt-cost-v2.tex §8) emits, for a calculus whose contact K
   is RIGID (the interaction head sits in no equation and is not associative-
   commutative) and whose environment-introduction is DEGENERATE, exactly ONE
   metered rule — the R1 shape — instead of the full five-rule lattice that an
   AC contact (rho's `|`) produces. The companion `mettail-rust` prototype
   (`cost-decoration/src/main.rs`) exhibits this operationally: its untyped-λ
   reification (`App` contact, in no equation, not a comm-collection) emits a
   single `Beta_R1`, whereas its communication calculus (`Par` is a comm-
   collection ⇒ AC) emits all five. The rho instance (all five rules) and the
   ABSTRACT endofunctor (`CACostFunctorCI.CostCI`) are mechanized elsewhere in
   this development; this module mechanizes the λ INSTANCE so the genericity
   claim has a second, structurally-minimal concrete witness.

   The R1 rule is the exact analogue of `CAReduction.ca_rule1`:

       {(App (Abs M) N)}_s ∥ s:T  ⤳  {M[N/0]}_s ∥ T          (one token gate spent)

   This is a SELF-CONTAINED parallel instance: it defines its own host syntax
   (`lterm`, with de Bruijn binders) and its own fuel wrapper (`lsys`), and
   reuses ONLY the calculus-agnostic fuel apparatus `sig`/`token`/`token_size`
   from CostAccountedSyntax (the fuel currency does not depend on the host
   calculus). It deliberately does NOT import CAReduction/CASyntax (whose
   `ca_step`/`subst_st`/`st_total_fuel` are rho-specific) — the three-line
   measure analogues are reproved locally.

   What is proved (all axiom-free):
     (a) R1-only — every step is the β-R1 contact (or a parallel-context lift of
         one); a lone wrapper is stuck and a lone stack is inert; a funded
         non-redex does not fire. There is no compound-signature / split-process
         rule (no ca_rule2..5 / ca_join analogue) BECAUSE the λ host has no AC
         operator and no independent environment-introduction (output) sort —
         this is the structural content of "rigid K ⇒ R1 only".
     (b) Funded bound — a configuration funded by a token stack of height h
         reduces in at most h steps (mirrors CAModulus.funded_run_bounded and
         the per-step fuel drop of CATokenConservation).
     (c) Funded strong normalization — every funded configuration is
         Acc-strongly-normalizing (mirrors CAStrongNormalization.ca_SN_funded).
         Here SN is UNCONDITIONAL — a λ term carries no fuel-bearing subterm, so
         the fuel measure can never rise, in contrast to rho where SN is
         conditional on the linearly-funded fragment
         (CAStrongNormalization.st_total_fuel_can_increase_off_funded). The seam
         is exhibited by Ω = (λx.x x)(λx.x x): pure-λ Ω β-reduces to itself
         (`omega_pure_diverges`), yet funded with one gate the configuration
         takes exactly one metered step and then halts.

   Mirrors: CAReduction.ca_rule1 (the R1 shape), CASyntax/CABinding (the de
   Bruijn lift/subst idiom), CATokenConservation/CAModulus (the fuel measure and
   run-bound), CAStrongNormalization (the wf_incl/ltof termination argument).
   ════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Wf_nat.
From Stdlib Require Import Wellfounded.
From CostAccountedRho Require Import CostAccountedSyntax.

(* ── Section 1: host syntax — untyped λ with de Bruijn indices ───────────────
   The de Bruijn convention mirrors CASyntax's caname/caproc: index 0 is the
   innermost binder; [llift] shifts free indices at/above a cutoff; [lsubst]
   uses [Nat.compare] and lifts the substituend by 1 when crossing a binder. *)

Inductive lterm : Type :=
  | LVar : nat -> lterm
  | LAbs : lterm -> lterm           (* λ. M — binds de Bruijn 0 in M *)
  | LApp : lterm -> lterm -> lterm.

Fixpoint llift (d c : nat) (t : lterm) : lterm :=
  match t with
  | LVar k   => if c <=? k then LVar (k + d) else LVar k
  | LAbs M   => LAbs (llift d (S c) M)
  | LApp M N => LApp (llift d c M) (llift d c N)
  end.

Fixpoint lsubst (t : lterm) (n : nat) (u : lterm) : lterm :=
  match t with
  | LVar k =>
      match Nat.compare k n with
      | Lt => LVar k
      | Eq => u
      | Gt => LVar (k - 1)
      end
  | LAbs M   => LAbs (lsubst M (S n) (llift 1 0 u))
  | LApp M N => LApp (lsubst M n u) (lsubst N n u)
  end.

(* ── Section 2: fuel wrapper — the cost layer over the λ host ────────────────
   Mirrors signed_term's three-constructor shape (CASyntax.STSigned/STPar/
   STStack) with an [lterm] interior, reusing [sig]/[token]. The total-fuel
   measure mirrors st_total_fuel (CATokenConservation): a wrapped term carries
   no fuel; only token stacks do. Note a λ wrapper has NO fuel-bearing subterm
   at all (an [lterm] contains no [token]), which is exactly why the measure can
   never rise and SN below is unconditional. *)

Inductive lsys : Type :=
  | LWrap  : lterm -> sig -> lsys       (* {M}_s — host term sealed under s *)
  | LSPar  : lsys -> lsys -> lsys       (* M ∥ N *)
  | LStack : token -> lsys.             (* s:T — a token stack *)

Fixpoint lsys_total_fuel (S : lsys) : nat :=
  match S with
  | LWrap _ _ => 0
  | LSPar A B => lsys_total_fuel A + lsys_total_fuel B
  | LStack t  => token_size t
  end.

(* ── Section 3: the single β-R1 contact rule (+ parallel-context closure) ────
   [lca_beta_r1] is the exact analogue of CAReduction.ca_rule1, with the rho
   continuation substitution [subst_st T 0 (CQuote U)] replaced by the host
   β-contractum [lsubst M 0 N]. There is deliberately NO ca_rule2..5 / ca_join
   analogue (see Section 4 for why this is forced, not an omission). *)

Inductive lca_step : lsys -> lsys -> Prop :=
  | lca_beta_r1 : forall (M N : lterm) (s : sig) (t : token),
      lca_step
        (LSPar (LWrap (LApp (LAbs M) N) s) (LStack (TGate s t)))
        (LSPar (LWrap (lsubst M 0 N) s)    (LStack t))
  | lca_par_l : forall S1 S1' S2,
      lca_step S1 S1' -> lca_step (LSPar S1 S2) (LSPar S1' S2)
  | lca_par_r : forall S1 S2 S2',
      lca_step S2 S2' -> lca_step (LSPar S1 S2) (LSPar S1 S2').

Inductive lca_reachable_n : nat -> lsys -> lsys -> Prop :=
  | lcarn_refl : forall S, lca_reachable_n 0 S S
  | lcarn_step : forall n S1 S2 S3,
      lca_step S1 S2 -> lca_reachable_n n S2 S3 -> lca_reachable_n (S n) S1 S3.

(* ── Section 4: (a) R1-only ──────────────────────────────────────────────────
   The only computational rule is β-R1. A lone wrapper is stuck (no leak — cf.
   WrappingSubjectReduction.no_leak_requires_token) and a lone stack is inert
   (cf. no_leak_stack_inert); every step is therefore β-R1 under a parallel
   context. There is no compound-signature (ca_rule2/3) or split-process
   (ca_rule4/5) rule, and no join: the λ host provides no AC operator (so seals
   never need conjoining across independently-signed participants) and no
   independent environment-introduction/output sort (so there is no second
   signed process to bring into contact). The characterization theorem below IS
   that statement — R2..R5 are uninhabited by construction of the host. *)

Theorem lca_contact_requires_token :
  forall (M : lterm) (s : sig) (S' : lsys), ~ lca_step (LWrap M s) S'.
Proof. intros M s S' H. inversion H. Qed.

Theorem lca_stack_inert :
  forall (t : token) (S' : lsys), ~ lca_step (LStack t) S'.
Proof. intros t S' H. inversion H. Qed.

Theorem lca_only_beta_r1 : forall S S', lca_step S S' ->
     (exists M N s t,
        S  = LSPar (LWrap (LApp (LAbs M) N) s) (LStack (TGate s t))
     /\ S' = LSPar (LWrap (lsubst M 0 N) s)    (LStack t))
  \/ (exists S1 S1' S2, S = LSPar S1 S2 /\ S' = LSPar S1' S2 /\ lca_step S1 S1')
  \/ (exists S1 S2 S2', S = LSPar S1 S2 /\ S' = LSPar S1 S2' /\ lca_step S2 S2').
Proof.
  intros S S' H. destruct H as [M N s t | S1 S1' S2 Hsub | S1 S2 S2' Hsub].
  - left. exists M, N, s, t. split; reflexivity.
  - right; left. exists S1, S1', S2. split; [reflexivity | split; [reflexivity | exact Hsub]].
  - right; right. exists S1, S2, S2'. split; [reflexivity | split; [reflexivity | exact Hsub]].
Qed.

(* A funded NON-redex does not fire: the rigid contact reduces only the actual
   β-redex shape (App (Abs _) _), never a value/neutral term — even when a
   matching token gate is present. This is the pointed "rigid K fires only on
   the redex" companion to the characterization above. *)
Theorem lca_funded_nonredex_stuck :
  forall (M : lterm) (s : sig) (t : token) (S' : lsys),
    (forall M0 N0, M <> LApp (LAbs M0) N0) ->
    ~ lca_step (LSPar (LWrap M s) (LStack (TGate s t))) S'.
Proof.
  intros M s t S' Hnr H. inversion H; subst.
  - eapply Hnr; reflexivity.
  - eapply lca_contact_requires_token; eassumption.
  - eapply lca_stack_inert; eassumption.
Qed.

(* ── Section 5: (b) funded run-bound ─────────────────────────────────────────
   Every step needs a gate and strictly drops the fuel measure (the gate it
   consumes); hence a run is no longer than its initial fuel. Mirrors
   CATokenConservation (per-step fuel) and CAModulus.funded_run_bounded. *)

Lemma lca_step_needs_fuel : forall S S', lca_step S S' -> 1 <= lsys_total_fuel S.
Proof. intros S S' H. induction H; simpl in *; lia. Qed.

Lemma lca_step_decreases :
  forall S S', lca_step S S' -> lsys_total_fuel S' < lsys_total_fuel S.
Proof. intros S S' H. induction H; simpl in *; lia. Qed.

Theorem lca_funded_run_bounded :
  forall n S T, lca_reachable_n n S T -> n <= lsys_total_fuel S.
Proof.
  intros n S T H. induction H as [S | n' S1 S2 S3 Hstep Htail IH].
  - lia.
  - assert (Hdec : lsys_total_fuel S2 < lsys_total_fuel S1)
      by (apply lca_step_decreases; exact Hstep).
    lia.
Qed.

(* ── Section 6: (c) funded strong normalization + the Ω halting seam ─────────
   Well-foundedness via the fuel measure (the wf_incl/ltof argument of
   CAStrongNormalization.ca_well_founded_funded). UNCONDITIONAL here — no
   funded-linear premise — because lca_step_decreases holds for EVERY step (a λ
   wrapper carries 0 fuel, so substitution can never inject fuel). *)

Definition lstep_inv (T S : lsys) : Prop := lca_step S T.

Theorem lca_well_founded : well_founded lstep_inv.
Proof.
  apply (wf_incl lsys lstep_inv (ltof lsys lsys_total_fuel)).
  - intros x y H. unfold lstep_inv in H. unfold ltof.
    apply lca_step_decreases. exact H.
  - apply well_founded_ltof.
Qed.

Corollary lca_SN_funded : forall S, Acc lstep_inv S.
Proof. apply lca_well_founded. Qed.

(* Ω = (λx.x x)(λx.x x): the canonical non-normalizing untyped-λ term. *)
Definition omega_body : lterm := LApp (LVar 0) (LVar 0).
Definition omega_term : lterm := LApp (LAbs omega_body) (LAbs omega_body).

(* Pure-λ divergence: the β-contractum of Ω is Ω again (no normal form). *)
Lemma omega_pure_diverges :
  lsubst omega_body 0 (LAbs omega_body) = omega_term.
Proof. unfold omega_body, omega_term. vm_compute. reflexivity. Qed.

(* Funded with a single gate, the same Ω configuration takes EXACTLY ONE metered
   step (to Ω again, now with an empty stack) and is then stuck — finite funding
   tames the divergence. *)
Theorem lca_omega_funded_one_step : forall s,
     lca_step (LSPar (LWrap omega_term s) (LStack (TGate s TUnit)))
              (LSPar (LWrap omega_term s) (LStack TUnit))
  /\ (forall S', ~ lca_step (LSPar (LWrap omega_term s) (LStack TUnit)) S').
Proof.
  intro s. split.
  - change (LSPar (LWrap omega_term s) (LStack TUnit))
      with (LSPar (LWrap (lsubst omega_body 0 (LAbs omega_body)) s) (LStack TUnit)).
    apply lca_beta_r1.
  - intros S' H. inversion H; subst;
      first [ discriminate
            | solve [ eapply lca_contact_requires_token; eassumption ]
            | solve [ eapply lca_stack_inert; eassumption ] ].
Qed.

(* And the funded Ω configuration is in the strongly-normalizing domain
   (an instance of the unconditional funded SN above). *)
Theorem lca_omega_funded_halts : forall s,
  Acc lstep_inv (LSPar (LWrap omega_term s) (LStack (TGate s TUnit))).
Proof. intro s. apply lca_SN_funded. Qed.

(* ── Section 7: (d′) erasure — the metered layer faithfully decorates pure λ ──
   The cost β-R1 step, projected to the underlying λ-terms, is precisely a pure
   untyped-λ β-contraction: the token gate is administrative, adding metering on
   top of an unchanged contraction. So Cost decorates pure λ faithfully (the
   instance-level shadow of the abstract retraction CAInternalisation realizes
   for rho). *)

Inductive pure_beta : lterm -> lterm -> Prop :=
  | pb_beta : forall M N, pure_beta (LApp (LAbs M) N) (lsubst M 0 N)
  | pb_appl : forall M M' N, pure_beta M M' -> pure_beta (LApp M N) (LApp M' N)
  | pb_appr : forall M N N', pure_beta N N' -> pure_beta (LApp M N) (LApp M N')
  | pb_abs  : forall M M', pure_beta M M' -> pure_beta (LAbs M) (LAbs M').

Theorem lca_beta_r1_erasure : forall (M N : lterm) (s : sig) (t : token),
     lca_step (LSPar (LWrap (LApp (LAbs M) N) s) (LStack (TGate s t)))
              (LSPar (LWrap (lsubst M 0 N) s)    (LStack t))
  /\ pure_beta (LApp (LAbs M) N) (lsubst M 0 N).
Proof. intros M N s t. split; [ apply lca_beta_r1 | apply pb_beta ]. Qed.
