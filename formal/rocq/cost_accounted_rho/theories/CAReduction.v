(* ════════════════════════════════════════════════════════════════════════
   CAReduction.v — Native cost-accounted reduction (DR-21 Option B).

   The five gated COMM rules of the cost-accounted calculus (spec §3.6
   Rules 1-5; monad paper R1-R3 in interaction-cut form), stated NATIVELY over
   [signed_term]. The decisive difference from the old [CostAccountedReduction]
   (on [system], with bare-proc continuations): here the receiver's continuation
   [T] and the sent payload [U] are SIGNED TERMS that carry their OWN seals, so
   a COMM produces `T{@U/y} = subst_st T 0 (CQuote U)` — the continuation keeps
   its own signature. There is NO `SAnd s1 s2` re-seal in the split-process
   rules (old ca_rule4/ca_rule5). **GAP-2 dissolves syntactically**, exactly as
   the monad paper states ("There is no re-wrapping step and no lifted
   contraction", continued-gslt-cost-v2.tex:429-430) and DR-20(b) anticipated.

   The contraction is Milner's pseudo-application: the COMM substitutes the
   quoted payload `@U` for the receiver's bound variable (CNVar 0) in `T`.
   Each rule consumes exactly one token gate (the authorizing fuel). The
   relation is closed under STPar (the spatial monoid), but NOT under an
   unforced wrapper — a wrapped redex never fires without a co-present token
   (the no-leak invariant, proved in WrappingSubjectReduction). Axiom-free.    *)

From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.

(* The N senders of a whole-join redex: x1!(U1) | … | xN!(UN). *)
Fixpoint join_sends (xs : list caname) (Us : list signed_term) : caproc :=
  match xs, Us with
  | cons x xs', cons U Us' => CPPar (CPOutput x U) (join_sends xs' Us')
  | _, _ => CPNil
  end.

Reserved Notation "S '⤳ca' T" (at level 70, no associativity).

Inductive ca_step : signed_term -> signed_term -> Prop :=

  (* Rule 1 — atomic signature, whole redex, single token.
       {for(y<-x){T} | x!(U)}_s ∥ s:S  ⤳  T{@U/y} ∥ S                       *)
  | ca_rule1 : forall (x : caname) (T U : signed_term) (s : sig) (t : token),
      ca_step
        (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) s)
               (STStack (TGate s t)))
        (STPar (subst_st T 0 (CQuote U)) (STStack t))

  (* Rule 2 — compound signature, whole redex, split tokens. *)
  | ca_rule2 : forall (x : caname) (T U : signed_term) (s1 s2 : sig) (t1 t2 : token),
      ca_step
        (STPar (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
                      (STStack (TGate s1 t1)))
               (STStack (TGate s2 t2)))
        (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2))

  (* Rule 3 — compound signature, whole redex, combined token. *)
  | ca_rule3 : forall (x : caname) (T U : signed_term) (s1 s2 : sig) (t : token),
      ca_step
        (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
               (STStack (TGate (SAnd s1 s2) t)))
        (STPar (subst_st T 0 (CQuote U)) (STStack t))

  (* Rule 4 — compound signature, SPLIT processes, combined token.
     The receiver and sender are signed independently; the continuation T
     keeps its OWN seal in the residual (NO SAnd re-seal — GAP-2 dissolved). *)
  | ca_rule4 : forall (x : caname) (T U : signed_term) (s1 s2 : sig) (t : token),
      ca_step
        (STPar (STPar (STSigned (CPInput x T) s1)
                      (STSigned (CPOutput x U) s2))
               (STStack (TGate (SAnd s1 s2) t)))
        (STPar (subst_st T 0 (CQuote U)) (STStack t))

  (* Rule 5 — compound signature, SPLIT processes, split tokens.
     Likewise, T keeps its own seal — no SAnd re-seal (GAP-2 dissolved). *)
  | ca_rule5 : forall (x : caname) (T U : signed_term) (s1 s2 : sig) (t1 t2 : token),
      ca_step
        (STPar (STPar (STPar (STSigned (CPInput x T) s1)
                             (STSigned (CPOutput x U) s2))
                      (STStack (TGate s1 t1)))
               (STStack (TGate s2 t2)))
        (STPar (STPar (subst_st T 0 (CQuote U)) (STStack t1)) (STStack t2))

  (* Join J1 — N-ary whole-join, single funding signature (spec §4.8, the N-ary
     analogue of Rule 1). TEMPORARILY DISABLED (commented, not deleted): adding
     this constructor makes the [ca_local_confluence] / determinism proofs'
     `inversion` over the join LHS (whose pattern embeds the `join_sends` Fixpoint)
     non-terminating. Landing it needs a dedicated determinism lemma built on
     `join_sends` injectivity plus a non-looping inversion strategy (Stage B/D,
     Risk R3/R4) — tracked separately so the metatheory sweep (Stage A) stays
     gate-green. The grammar former CPJoin + subst_st_many + the full syntactic
     metatheory are landed (committed); only the REDUCTION rule waits on R3/R4.
  | ca_join1 : forall (xs : list caname) (Us : list signed_term) (T : signed_term)
                      (s : sig) (t : token),
      length xs = length Us ->
      ca_step
        (STPar (STSigned (CPPar (CPJoin xs T) (join_sends xs Us)) s)
               (STStack (TGate s t)))
        (STPar (subst_st_many T Us) (STStack t))
  *)

  (* PAR closure (spatial monoid), left and right. *)
  | ca_par_l : forall S1 S1' S2, ca_step S1 S1' -> ca_step (STPar S1 S2) (STPar S1' S2)
  | ca_par_r : forall S1 S2 S2', ca_step S2 S2' -> ca_step (STPar S1 S2) (STPar S1 S2')

where "S '⤳ca' T" := (ca_step S T).

(* ── reflexive-transitive closure ───────────────────────────────────────── *)

Inductive ca_reachable : signed_term -> signed_term -> Prop :=
  | car_refl : forall S, ca_reachable S S
  | car_step : forall S1 S2 S3, ca_step S1 S2 -> ca_reachable S2 S3 -> ca_reachable S1 S3.

Lemma car_one : forall S1 S2, ca_step S1 S2 -> ca_reachable S1 S2.
Proof. intros. eapply car_step; [ eassumption | apply car_refl ]. Qed.

Lemma car_trans : forall S1 S2 S3,
  ca_reachable S1 S2 -> ca_reachable S2 S3 -> ca_reachable S1 S3.
Proof.
  intros S1 S2 S3 H12 H23. induction H12; [ assumption |].
  eapply car_step; [ eassumption | auto ].
Qed.
