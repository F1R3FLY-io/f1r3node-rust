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
From CostAccountedRho Require Import CABinding.

(* The N senders of a whole-join redex: x1!(U1) | … | xN!(UN). *)
Fixpoint join_sends (xs : list caname) (Us : list signed_term) : caproc :=
  match xs, Us with
  | cons x xs', cons U Us' => CPPar (CPOutput x U) (join_sends xs' Us')
  | _, _ => CPNil
  end.

(* Recover the payloads from a sender bundle — the partial inverse of join_sends,
   exact on well-formed bundles (matching arities). The graded successor
   enumeration uses it to read the N payloads back out of a join redex. *)
Fixpoint extract_sends (P : caproc) : list signed_term :=
  match P with
  | CPPar (CPOutput _ U) rest => U :: extract_sends rest
  | _ => nil
  end.

Lemma extract_sends_join_sends : forall xs Us,
  length xs = length Us -> extract_sends (join_sends xs Us) = Us.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] Hlen; simpl in *; try discriminate.
  - reflexivity.
  - rewrite (IH Us' ltac:(lia)). reflexivity.
Qed.

(* ── J2: separately-signed senders + a single combined token (spec §4.8 J2) ──
   The N senders of a J2 redex are SEPARATELY signed — ⟨x1!(U1)⟩_{t1} ∥ … ∥
   ⟨xN!(UN)⟩_{tN} — built as a right-nested STPar terminated by the inert nil
   signing (no token node, zero fuel, so the J2 redex still holds exactly ONE
   token cell — admissible under single_token_st, unlike Rules 2/5). *)
Fixpoint signed_sends (xs : list caname) (Us : list signed_term) (ts : list sig)
  : signed_term :=
  match xs, Us, ts with
  | cons x xs', cons U Us', cons t ts' =>
      STPar (STSigned (CPOutput x U) t) (signed_sends xs' Us' ts')
  | _, _, _ => STSigned CPNil SUnit
  end.

(* The combined funding key s1 ∘ t1 ∘ … ∘ tN (the fusion of the receiver
   authority with every sender authority), left-associated as the spec writes it. *)
Definition join_token_key (s1 : sig) (ts : list sig) : sig :=
  fold_left (fun acc t => SAnd acc t) ts s1.

(* Recover the channels / payloads / sender-signatures from a separately-signed
   bundle — the partial inverse of signed_sends, exact at matching arities. *)
Fixpoint sb_chans (S : signed_term) : list caname :=
  match S with
  | STPar (STSigned (CPOutput x _) _) rest => x :: sb_chans rest
  | _ => nil
  end.
Fixpoint sb_pays (S : signed_term) : list signed_term :=
  match S with
  | STPar (STSigned (CPOutput _ U) _) rest => U :: sb_pays rest
  | _ => nil
  end.
Fixpoint sb_sigs (S : signed_term) : list sig :=
  match S with
  | STPar (STSigned (CPOutput _ _) t) rest => t :: sb_sigs rest
  | _ => nil
  end.

Lemma sb_chans_signed_sends : forall xs Us ts,
  length xs = length Us -> length xs = length ts ->
  sb_chans (signed_sends xs Us ts) = xs.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] [| t ts'] HU Ht;
    simpl in *; try discriminate; try reflexivity.
  rewrite (IH Us' ts' ltac:(lia) ltac:(lia)). reflexivity.
Qed.
Lemma sb_pays_signed_sends : forall xs Us ts,
  length xs = length Us -> length xs = length ts ->
  sb_pays (signed_sends xs Us ts) = Us.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] [| t ts'] HU Ht;
    simpl in *; try discriminate; try reflexivity.
  rewrite (IH Us' ts' ltac:(lia) ltac:(lia)). reflexivity.
Qed.
Lemma sb_sigs_signed_sends : forall xs Us ts,
  length xs = length Us -> length xs = length ts ->
  sb_sigs (signed_sends xs Us ts) = ts.
Proof.
  induction xs as [| x xs' IH]; intros [| U Us'] [| t ts'] HU Ht;
    simpl in *; try discriminate; try reflexivity.
  rewrite (IH Us' ts' ltac:(lia) ltac:(lia)). reflexivity.
Qed.

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

  (* SCOPE (spec §4.8 join cost schema). The two rules below realize the J1 and J2
     CORNERS of Def 4.6 — whole-join/single-seal/single-token, and separately-signed/
     single-combined-token. The GENERAL partition schema of Def 4.6 (quantified over
     arbitrary receiver-clause/send/token-presentation partitions π_C/π_B/π_A) is NOT
     given a per-partition reduction constructor — a partition-indexed LHS would make
     `inversion` over the join redex non-terminating and wreck the determinism/
     confluence metatheory. The general schema's content is instead discharged at the
     MULTISET level by CAJoinConservation (Prop 4.7 `join_authority_conserved` +
     `join_authority_conserved_operational`): grouping along any axis never changes
     the consumed authority multiset. So the OPERATIONAL join is the J1/J2 corners;
     the GENERAL schema is the conservation invariant over them. *)

  (* Join J1 — N-ary whole-join, single funding signature (spec §4.8, the N-ary
     analogue of Rule 1): the join receiver and its N senders sit under one seal s,
     funded by one s-token; the continuation T keeps its own seal, N payloads
     substituted simultaneously (subst_st_many). N=1 is Rule 1. The senders are a
     VARIABLE [snds] constrained by an equation (snds = join_sends xs Us), NOT the
     Fixpoint embedded in the LHS pattern — this keeps `inversion` over the join
     redex clean (a Fixpoint index makes inversion non-terminating, Risk R3/R4).
     The join now fires for ARBITRARY (possibly-OPEN) payloads, matching the spec's
     Def 4.6: [subst_st_many] is the GENUINE simultaneous substitution (per-step
     lifting, [subst_st_many_cons]), so it is capture-free for open payloads too —
     no closedness premise is needed on the operational rule. The closed-payload
     condition is supplied, only where strong normalization needs it, by the funded
     fragment (funded_linear's CPOutput clause carries [closed_st] of every send).
     At N=1 this is exactly [ca_rule1]. *)
  | ca_join1 : forall (xs : list caname) (Us : list signed_term) (T : signed_term)
                      (s : sig) (t : token) (snds : caproc),
      snds = join_sends xs Us ->
      length xs = length Us ->
      ca_step
        (STPar (STSigned (CPPar (CPJoin xs T) snds) s)
               (STStack (TGate s t)))
        (STPar (subst_st_many T Us) (STStack t))

  (* Join J2 — N-ary join, separately-signed participants, single COMBINED token
     (spec §4.8 J2, the N-ary analogue of Rule 3 / eq:join-J2): the receiver join is
     signed s1, each sender ⟨xi!(Ui)⟩ is signed ti independently, and ONE token keyed
     to the fused signature s1 ∘ t1 ∘ … ∘ tN funds the whole join atomically (no
     partial funding). The continuation keeps its own seal; N payloads substituted
     simultaneously. Senders are the VARIABLE [snds] = signed_sends xs Us ts (the
     snds-variable form keeps inversion terminating). Like J1, this fires for
     ARBITRARY (possibly-OPEN) payloads — [subst_st_many] is genuinely simultaneous
     (per-step lifting), so capture-free without a closedness premise; closedness is
     supplied where SN needs it by funded_linear's CPOutput clause. At N=1 this is
     Rule 3. *)
  | ca_join2 : forall (xs : list caname) (Us : list signed_term) (ts : list sig)
                      (T : signed_term) (s1 : sig) (t : token) (snds : signed_term),
      snds = signed_sends xs Us ts ->
      length xs = length Us ->
      length xs = length ts ->
      ca_step
        (STPar (STPar (STSigned (CPJoin xs T) s1) snds)
               (STStack (TGate (join_token_key s1 ts) t)))
        (STPar (subst_st_many T Us) (STStack t))

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
