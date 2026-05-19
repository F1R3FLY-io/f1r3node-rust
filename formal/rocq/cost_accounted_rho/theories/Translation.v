(* ═══════════════════════════════════════════════════════════════════════════
   Translation.v — Compositional translation of cost-accounted rho calculus
                   back into pure rho calculus
   ═══════════════════════════════════════════════════════════════════════════

   Defines the compositional translation that erases the cost-accounted
   layer of the calculus by encoding signatures as channels and tokens as
   outputs on those channels. The translation has four components, one for
   each of the syntactic categories introduced in CostAccountedSyntax.v:

       N⟦·⟧ : sig    → name      (signatures become channels)
       T⟦·⟧ : token  → proc      (tokens become parallel outputs)
       P⟦·⟧ : signed → proc      (signed processes become fuel-gated inputs)
       S⟦·⟧ : system → proc      (systems become parallel processes)

   The high-level intuition is that each unit of fuel in the cost-accounted
   calculus is realised in the pure calculus as a single COMM redex on the
   channel that encodes its authorising signature. A signed process [P^s]
   is wrapped in an input on N⟦s⟧ that, once a token output is provided,
   fires the body P together with the released payload.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Paper Notation        │ Runtime traceability
   ─────────────────────────┼───────────────────────┼─────────────────────
   N_tr                     │ N⟦·⟧                  │ signature-channel helper
   T_tr                     │ T⟦·⟧                  │ token-marker helper
   P_tr                     │ P⟦·⟧                  │ legacy local gate shape
   S_tr                     │ S⟦·⟧                  │ legacy compositional image
   hash_process             │ H_σ (canonical proc)  │ Hash::to_process
   Split                    │ Split(s₁, s₂)         │ Mediator::split
   Join                     │ Join(s₁, s₂)          │ Mediator::join
   S_tr_compositional       │ S⟦S₁ ∥ S₂⟧ = …        │ (definitional)
   ─────────────────────────────────────────────────────────────────────────

   Cryptographic assumption. The paper writes N⟦hash(σ)⟧ = @H_σ where H_σ
   is "the canonical process determined by the digital signature σ". We
   refrain from formalising any concrete cryptographic hash function and
   instead introduce, inside a Section, a hypothesis that produces such a
   canonical process from a byte string and is injective on byte strings.
   Section variables of type Prop are discharged at the end of the section,
   so they appear in `Print Assumptions` of any client lemma that depends
   on them, keeping the audit story honest.

   The business-critical whole-system implementation target is the
   recursive metered relation in TranslationFaithfulness.v, not the raw
   compositional S_tr image. This file remains the paper-trace layer and
   supplies the local signature/token/gate definitions used by the proofs.

   Compound-signature mediation. The paper handles compound signatures by
   pairs of mediator processes, Split and Join, that decompose and
   recompose tokens on N⟦s₁ & s₂⟧ into tokens on N⟦s₁⟧ and N⟦s₂⟧. We
   define both Split and Join as named processes here at the system level
   so that the simpler atomic-fuel-gate translation P⟦·⟧ used below
   uniformly applies to every signature shape. Compound mediation can then
   be inserted explicitly by clients of this module when proving that a
   particular cost-accounted reduction is simulated in the pure calculus.

   Dependencies: Rocq 9.1.1 stdlib, RhoSyntax, CostAccountedSyntax,
                 RhoReduction (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import RhoReduction.

(* ═══════════════════════════════════════════════════════════════════════════
   Section TranslationDefs
   ═══════════════════════════════════════════════════════════════════════════

   The hash hypothesis is introduced inside a section so that all
   definitions and lemmas that depend on it are parameterised by the hash
   function in a clean, audit-friendly way. Closing the section abstracts
   the hypothesis and exposes section-polymorphic versions of every
   definition that mentions it.                                              *)

Section TranslationDefs.

(* The canonical process associated with a byte string. This stands in for
   the "canonical process determined by digital signature σ" in the paper.
   We do not commit to any concrete construction; the only property we
   require is injectivity, which we state as a separate hypothesis below.
   We use [Variable] (rather than [Hypothesis]) here because the carrier
   has type [list bool -> proc] rather than [Prop]. *)
Variable hash_process : list bool -> proc.

(* Hash injectivity: distinct byte strings give rise to distinct canonical
   processes. This is the cryptographic assumption that the translation
   relies on; without it, two atomic signatures could collide and the
   translation would no longer be sound. We state it inside the section so
   that `Print Assumptions` of any client lemma exposes it. *)
Hypothesis hash_process_injective :
  forall b1 b2, hash_process b1 = hash_process b2 -> b1 = b2.

(* Hash processes are CLOSED: a hash of a byte string is a deterministic
   encoding that does not refer to bound name variables from any
   surrounding scope. This is a property of any concrete cryptographic hash
   construction (the output is a function of the input bytes only).

   Stated as a Hypothesis inside the Section so it appears in
   [Print Assumptions] of every client theorem that depends on it. *)
Hypothesis hash_process_closed : forall bs, closed_proc (hash_process bs).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Signature Translation N⟦·⟧
   ═══════════════════════════════════════════════════════════════════════════

   A signature is encoded as the name (i.e. the quoted process) on which
   tokens authorising that signature are passed. The three cases mirror
   the three signature constructors:

   - SUnit         ↦  @0           — the unit signature is the channel
                                      whose underlying process is nil.
   - SHash bs      ↦  @H_bs        — atomic signatures are the canonical
                                      processes given by hash_process.
   - SAnd s1 s2    ↦  @( *N⟦s1⟧ | *N⟦s2⟧ )
                                   — compound signatures dereference the
                                      two component channels in parallel,
                                      then re-quote the result.            *)

Fixpoint N_tr (s : sig) : name :=
  match s with
  | SUnit       => Quote PNil
  | SHash bs    => Quote (hash_process bs)
  | SAnd s1 s2  => Quote (PPar (PDeref (N_tr s1)) (PDeref (N_tr s2)))
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Token Translation T⟦·⟧
   ═══════════════════════════════════════════════════════════════════════════

   A token is a right-nested stack of signature gates. Its translation
   peels the stack one gate at a time and emits an output on the channel
   N⟦s⟧, carrying the translated remainder of the stack as payload:

   - TUnit       ↦  0
   - TGate s t   ↦  N⟦s⟧!( T⟦t⟧ )

   In particular, an empty token translates to the stopped process, and
   a one-gate token  s : ()  translates to a single output  N⟦s⟧!(0).    *)

Fixpoint T_tr (t : token) : proc :=
  match t with
  | TUnit       => PNil
  | TGate s t'  => POutput (N_tr s) (T_tr t')
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Signed Process Translation P⟦·⟧
   ═══════════════════════════════════════════════════════════════════════════

   A signed process [P^s] is translated to a fuel-gated input on the
   signature channel N⟦s⟧:

       P⟦P^s⟧ = for(t ← N⟦s⟧){ P | *t }

   Operationally, the input waits for a token to arrive on N⟦s⟧; once it
   does, the COMM rule of the pure rho calculus substitutes the bound
   variable (de Bruijn index 0) with the dereference of the quoted
   payload, and the body P runs in parallel with that released payload.

   We use this uniform atomic-fuel-gate shape for every signature, even
   compound ones. Compound mediation between N⟦s₁ & s₂⟧ and the pair of
   channels (N⟦s₁⟧, N⟦s₂⟧) is provided separately by the Split and Join
   processes defined below; clients that need to simulate a step on a
   compound signature can compose these mediators with the atomic
   translation explicitly.                                                   *)

(* The signed-process translation. For atomic signatures, the body P
   is wrapped in a single fuel gate that releases the dequoted payload
   in parallel with P. For compound signatures (s1 & s2), the gates are
   nested: the outer waits on N_tr s1, the inner on N_tr s2, and the
   innermost body releases both payloads in parallel before P runs.

   Bound name variables (NVar) refer to the received fuel name from the
   surrounding fuel-gate input. NVar 0 is the most recently bound; NVar 1
   is the next outer one; etc. *)

(* The user process P is lifted before being placed under the fuel
   gate(s). This ensures that any free NVar k in P (a bound name from
   an outer context) is correctly shifted past the new binders
   introduced by the gate's PInput. Without this lift, substitution
   for the fuel-token (at index 0) could accidentally affect bound
   variables inside P that have nothing to do with the fuel gate.

   For atomic signatures, P crosses ONE binder (the gate's PInput),
   so we lift by 1 with cutoff 0. For compound signatures (SAnd s1 s2),
   P crosses TWO binders (the outer + inner gates), so we lift by 2. *)

Definition P_tr (P : proc) (s : sig) : proc :=
  match s with
  | SUnit =>
      PInput (N_tr SUnit)
        (PPar (lift_proc 1 0 P) (PDeref (NVar 0)))
  | SHash bs =>
      PInput (N_tr (SHash bs))
        (PPar (lift_proc 1 0 P) (PDeref (NVar 0)))
  | SAnd s1 s2 =>
      (* Outer fuel gate on N_tr s1; inside, inner fuel gate on N_tr s2.
         Inside the inner body, NVar 0 is the s2 token, NVar 1 is the
         s1 token. We release both payloads in parallel with P (lifted
         by 2 since it crosses both gate binders). *)
      PInput (N_tr s1)
        (PInput (N_tr s2)
          (PPar (lift_proc 2 0 P)
            (PPar (PDeref (NVar 1)) (PDeref (NVar 0)))))
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: System Translation S⟦·⟧
   ═══════════════════════════════════════════════════════════════════════════

   A system is translated component-wise. Signed processes use the
   fuel-gated input from P_tr, free tokens become parallel outputs via
   T_tr, and parallel composition of systems is mapped to parallel
   composition of processes. The third clause is what makes the
   translation compositional.                                                *)

Fixpoint S_tr (sys : system) : proc :=
  match sys with
  | SSigned P s => P_tr P s
  | SToken t    => T_tr t
  | SPar S1 S2  => PPar (S_tr S1) (S_tr S2)
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Compositionality
   ═══════════════════════════════════════════════════════════════════════════

   The compositionality theorem states that the translation distributes
   over parallel composition of systems. This holds by definition of
   S_tr, but we record it as a named theorem because it is the headline
   property of the translation and is referenced by client modules.       *)

Theorem S_tr_compositional : forall S1 S2,
  S_tr (SPar S1 S2) = PPar (S_tr S1) (S_tr S2).
Proof. reflexivity. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Mediator Processes Split and Join
   ═══════════════════════════════════════════════════════════════════════════

   Compound signatures of the form  s₁ & s₂  are mediated by two named
   processes that translate between the compound channel N⟦s₁ & s₂⟧ and
   the pair of component channels (N⟦s₁⟧, N⟦s₂⟧).

   Split takes a token on the compound channel and produces tokens on
   each component channel. Concretely, it inputs on N⟦s₁ & s₂⟧, then in
   the body emits an empty signal on N⟦s₁⟧ and forwards the released
   payload to N⟦s₂⟧. The de Bruijn index 0 inside the body refers to the
   bound variable of the input.

   Join is the inverse: it inputs first on N⟦s₁⟧ and then on N⟦s₂⟧, and
   in the innermost body emits a single output on N⟦s₁ & s₂⟧ whose
   payload is the parallel composition of the two released payloads.
   Inside the inner body, de Bruijn index 0 refers to the second bound
   variable (most recently bound) and de Bruijn index 1 to the first.   *)

(* Split takes a token on the compound channel and produces tokens on
   each component channel. The bound NVar 0 inside the body is the
   received compound token's payload, which we forward to N_tr s2. *)
Definition Split (s1 s2 : sig) : proc :=
  PInput (N_tr (SAnd s1 s2))
    (PPar (POutput (N_tr s1) PNil)
          (POutput (N_tr s2) (PDeref (NVar 0)))).

(* Join is the inverse: it inputs first on N_tr s1 (binding NVar 0,
   later NVar 1) and then on N_tr s2 (binding NVar 0). The innermost
   body emits a single output on N_tr (SAnd s1 s2) carrying the
   parallel composition of both released payloads. *)
Definition Join (s1 s2 : sig) : proc :=
  PInput (N_tr s1)
    (PInput (N_tr s2)
      (POutput (N_tr (SAnd s1 s2))
        (PPar (PDeref (NVar 1)) (PDeref (NVar 0))))).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: Helper Lemmas
   ═══════════════════════════════════════════════════════════════════════════

   Small computational lemmas that unfold each translation function on
   its individual constructors. These are stated explicitly so that
   client proofs can rewrite with them by name rather than relying on
   ad-hoc unfolding tactics.                                                *)

(* Translating the empty token yields the stopped process. *)
Lemma T_tr_unit : T_tr TUnit = PNil.
Proof. reflexivity. Qed.

(* Translating a non-empty token yields a single output gating the
   translation of the remaining token. *)
Lemma T_tr_gate : forall s t,
  T_tr (TGate s t) = POutput (N_tr s) (T_tr t).
Proof. intros. reflexivity. Qed.

(* Defining equations of P_tr, useful as rewrite rules. The user
   process [P] is lifted by 1 (atomic case) or 2 (compound case) to
   account for the binders introduced by the gate's PInput(s). *)
Lemma P_tr_unit : forall P,
  P_tr P SUnit
    = PInput (N_tr SUnit) (PPar (lift_proc 1 0 P) (PDeref (NVar 0))).
Proof. intros. reflexivity. Qed.

Lemma P_tr_hash : forall P bs,
  P_tr P (SHash bs)
    = PInput (N_tr (SHash bs)) (PPar (lift_proc 1 0 P) (PDeref (NVar 0))).
Proof. intros. reflexivity. Qed.

Lemma P_tr_and : forall P s1 s2,
  P_tr P (SAnd s1 s2) =
    PInput (N_tr s1)
      (PInput (N_tr s2)
        (PPar (lift_proc 2 0 P)
          (PPar (PDeref (NVar 1)) (PDeref (NVar 0))))).
Proof. intros. reflexivity. Qed.

(* The defining equation of S_tr on signed processes. *)
Lemma S_tr_signed : forall P s,
  S_tr (SSigned P s) = P_tr P s.
Proof. intros. reflexivity. Qed.

(* The defining equation of S_tr on free tokens. *)
Lemma S_tr_token : forall t,
  S_tr (SToken t) = T_tr t.
Proof. intros. reflexivity. Qed.

(* Restatement of compositionality in pointwise rewrite form. *)
Lemma S_tr_par : forall S1 S2,
  S_tr (SPar S1 S2) = PPar (S_tr S1) (S_tr S2).
Proof. intros. reflexivity. Qed.

(* Translating the unit signature gives the quotation of the stopped
   process; useful as a rewrite for unfolding witnesses about SUnit. *)
Lemma N_tr_unit : N_tr SUnit = Quote PNil.
Proof. reflexivity. Qed.

(* Translating a hash signature exposes the canonical process supplied
   by the hash hypothesis. *)
Lemma N_tr_hash : forall bs,
  N_tr (SHash bs) = Quote (hash_process bs).
Proof. intros. reflexivity. Qed.

(* Translating a compound signature dereferences both component
   translations in parallel and re-quotes the result. *)
Lemma N_tr_and : forall s1 s2,
  N_tr (SAnd s1 s2) = Quote (PPar (PDeref (N_tr s1)) (PDeref (N_tr s2))).
Proof. intros. reflexivity. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 8: Closure of N_tr and T_tr
   ═══════════════════════════════════════════════════════════════════════════

   The signature-name translation [N_tr] and the token translation [T_tr]
   produce CLOSED terms — terms with no free name variables. This is
   because both N_tr and T_tr are built from [Quote], [hash_process bs]
   (assumed closed), [PNil], and constructors that combine closed
   subterms. There are no [NVar] occurrences anywhere in their output.

   Closed-ness is the load-bearing property that lets the
   compound-signature simulation proceed: when the outer fuel gate
   fires and the substitution propagates through the inner gate's
   binder, the lifted token payloads from outer scope must be left
   untouched. They are untouched precisely because they are closed.   *)

Lemma N_tr_closed : forall s, closed_name (N_tr s).
Proof.
  induction s as [| bs | s1 IHs1 s2 IHs2]; simpl.
  - (* SUnit -> Quote PNil *) apply closed_Quote, closed_PNil.
  - (* SHash bs -> Quote (hash_process bs) *)
    apply closed_Quote, hash_process_closed.
  - (* SAnd s1 s2 -> Quote (PPar (PDeref (N_tr s1)) (PDeref (N_tr s2))) *)
    apply closed_Quote, closed_PPar.
    + apply closed_PDeref, IHs1.
    + apply closed_PDeref, IHs2.
Qed.

Lemma T_tr_closed : forall t, closed_proc (T_tr t).
Proof.
  induction t as [| s t IH]; simpl.
  - (* TUnit -> PNil *) apply closed_PNil.
  - (* TGate s t -> POutput (N_tr s) (T_tr t) *)
    apply closed_POutput.
    + apply N_tr_closed.
    + apply IH.
Qed.

(* Substitution and lifting are identities on N_tr and T_tr.
   These corollaries are what client modules actually use. *)

Lemma N_tr_subst : forall s k N, subst_name (N_tr s) k N = N_tr s.
Proof.
  intros. apply closed_name_subst_zero. apply N_tr_closed.
Qed.

Lemma N_tr_lift : forall s d c, lift_name d c (N_tr s) = N_tr s.
Proof.
  intros. apply closed_name_lift_zero. apply N_tr_closed.
Qed.

Lemma T_tr_subst : forall t k N, subst_proc (T_tr t) k N = T_tr t.
Proof.
  intros. apply closed_proc_subst_zero. apply T_tr_closed.
Qed.

Lemma T_tr_lift : forall t d c, lift_proc d c (T_tr t) = T_tr t.
Proof.
  intros. apply closed_proc_lift_zero. apply T_tr_closed.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9: Operational Behavior of Split
   ═══════════════════════════════════════════════════════════════════════════

   The Split mediator, when paired with a token output on the compound
   signature channel, fires via the COMM rule and produces two atomic
   token outputs: an empty PNil on N_tr s1 and the original token's
   dequoted payload on N_tr s2.

   This is the load-bearing operational fact for Rules 3 and 4, where
   a combined token must be split into atomic tokens before the
   nested fuel gates can consume them.                                   *)

(* Split fires on a token translation [T_tr (TGate (SAnd s1 s2) t)],
   which unfolds to [POutput (N_tr (SAnd s1 s2)) (T_tr t)]. After
   firing, the bound variable NVar 0 is replaced with [Quote (T_tr t)]
   under semantic substitution; the [*NVar 0] in the body collapses
   to [T_tr t] directly (paper §2.4 Remark), so the s2-output's
   payload is simply [T_tr t]. *)
Lemma Split_operational : forall s1 s2 t,
  rho_step
    (PPar (Split s1 s2) (T_tr (TGate (SAnd s1 s2) t)))
    (PPar (POutput (N_tr s1) PNil)
          (POutput (N_tr s2) (T_tr t))).
Proof.
  intros s1 s2 t.
  unfold Split.
  cbn [T_tr].
  apply (rs_struct
    _
    (PPar (PInput (N_tr (SAnd s1 s2))
                  (PPar (POutput (N_tr s1) PNil)
                        (POutput (N_tr s2) (PDeref (NVar 0)))))
          (POutput (N_tr (SAnd s1 s2)) (T_tr t)))
    (subst_proc (PPar (POutput (N_tr s1) PNil)
                      (POutput (N_tr s2) (PDeref (NVar 0))))
                0
                (Quote (T_tr t)))).
  - apply se_refl.
  - apply rs_comm.
  - rewrite subst_proc_par.
    cbn [subst_proc subst_name].
    rewrite (N_tr_subst s1).
    rewrite (N_tr_subst s2).
    (* Under semantic subst, [subst_proc (PDeref (NVar 0)) 0 (Quote (T_tr t))
       = T_tr t] definitionally (the Nat.compare 0 0 = Eq branch fires the
       Quote-collapse). *)
    apply se_refl.
Qed.

(* The Split mediator is a closed process: it has a single PInput on a
   closed channel binding NVar 0 in its body, and the body uses NVar 0
   in a PDeref under the binder (so closed at level 1) and otherwise
   contains only closed names (N_tr s1, N_tr s2) and PNil. This is the
   load-bearing closedness fact for the generic Rule 1 / Rule 4 dispatchers
   that need to thread Split into the simulation context. *)
(* The Split mediator fires on ANY closed payload [M], not just [T_tr t].
   This is the load-bearing operational fact for the generic Rule 4 / 5
   compound cases, where the supply on the compound channel is the result
   of an OUTER Split firing — not a token translation. *)
Lemma Split_fires_closed : forall s1 s2 (M : proc),
  closed_proc M ->
  rho_step
    (PPar (Split s1 s2) (POutput (N_tr (SAnd s1 s2)) M))
    (PPar (POutput (N_tr s1) PNil)
          (POutput (N_tr s2) M)).
Proof.
  intros s1 s2 M HM.
  unfold Split.
  apply (rs_struct
    _
    (PPar (PInput (N_tr (SAnd s1 s2))
                  (PPar (POutput (N_tr s1) PNil)
                        (POutput (N_tr s2) (PDeref (NVar 0)))))
          (POutput (N_tr (SAnd s1 s2)) M))
    (subst_proc (PPar (POutput (N_tr s1) PNil)
                      (POutput (N_tr s2) (PDeref (NVar 0))))
                0
                (Quote M))).
  - apply se_refl.
  - apply rs_comm.
  - rewrite subst_proc_par.
    cbn [subst_proc subst_name].
    rewrite (N_tr_subst s1).
    rewrite (N_tr_subst s2).
    (* Under semantic subst, the [*NVar 0] dequote collapses to [M]. *)
    apply se_refl.
Qed.

Lemma Split_closed : forall s1 s2, closed_proc (Split s1 s2).
Proof.
  intros s1 s2.
  unfold Split.
  apply closed_PInput.
  - (* closed_name (N_tr (SAnd s1 s2)) *)
    apply N_tr_closed.
  - (* closed_proc_at 1 (PPar (POutput (N_tr s1) PNil)
                              (POutput (N_tr s2) (PDeref (NVar 0)))) *)
    split.
    + (* closed_proc_at 1 (POutput (N_tr s1) PNil) *)
      split.
      * (* closed_name_at 1 (N_tr s1) *)
        apply (closed_name_at_mono _ 0 1); [lia | apply N_tr_closed].
      * (* closed_proc_at 1 PNil *) exact I.
    + (* closed_proc_at 1 (POutput (N_tr s2) (PDeref (NVar 0))) *)
      split.
      * (* closed_name_at 1 (N_tr s2) *)
        apply (closed_name_at_mono _ 0 1); [lia | apply N_tr_closed].
      * (* closed_proc_at 1 (PDeref (NVar 0)) → closed_name_at 1 (NVar 0) → 0 < 1 *)
        simpl. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 10: Persistent Mediators (PersistentSplit, PersistentJoin)
   ═══════════════════════════════════════════════════════════════════════════

   The Split and Join mediators defined above fire once and are consumed.
   In many practical scenarios the mediator should persist — always ready
   to decompose or recompose compound-signature tokens. We achieve
   persistence by wrapping the one-shot mediator in a PReplicate.

   PersistentSplit s1 s2 = !Split(s1, s2)
   PersistentJoin  s1 s2 = !Join(s1, s2)

   Each unfolding of the replication produces a fresh one-shot mediator
   in parallel with the persistent copy, so compound-signature mediation
   is available an unlimited number of times.                                *)

Definition PersistentSplit (s1 s2 : sig) : proc :=
  PReplicate (Split s1 s2).

Definition PersistentJoin (s1 s2 : sig) : proc :=
  PReplicate (Join s1 s2).

(* Closedness of the persistent mediators follows immediately from the
   closedness of the one-shot versions (Split_closed above) and the
   fact that PReplicate P is closed iff P is closed. *)

Lemma PersistentSplit_closed : forall s1 s2, closed_proc (PersistentSplit s1 s2).
Proof.
  intros s1 s2.
  unfold PersistentSplit, closed_proc. simpl.
  apply Split_closed.
Qed.

Lemma Join_closed : forall s1 s2, closed_proc (Join s1 s2).
Proof.
  intros s1 s2. unfold Join.
  apply closed_PInput.
  - apply N_tr_closed.
  - split.
    + apply (closed_name_at_mono _ 0 1); [lia | apply N_tr_closed].
    + split.
      * apply (closed_name_at_mono _ 0 2); [lia | apply N_tr_closed].
      * split; simpl; lia.
Qed.

Lemma PersistentJoin_closed : forall s1 s2, closed_proc (PersistentJoin s1 s2).
Proof.
  intros s1 s2.
  unfold PersistentJoin, closed_proc. simpl.
  apply Join_closed.
Qed.

End TranslationDefs.
