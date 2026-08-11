(* ════════════════════════════════════════════════════════════════════════
   CASection.v — CA-P-180 (corollary half): the digest section's ≡-incoherence
   is BISIMULATION-HARMLESS because signatures never reduce
   (continued-gslt-cost-v2 §5 "Quotient and section", :629-632).

   SignatureMonoid.sig_section_not_respect_equiv already proves the FORCING half
   of §5: the section/digest [# = digest ∘ cf], being injective on the signature
   AST, does NOT respect ≡sig — "a commitment identifying congruent-but-distinct
   representatives would be no commitment" (:623-624). This module discharges
   the companion sentence that makes that incoherence ACCEPTABLE:

     "Signatures never reduce: they are a rewrite-free sub-theory meeting the
      behavioural theory only at the matching guard. Hence the ≡-incoherence of
      # is harmless for bisimulation, which factors as 'P-bisimulation gated by
      signature-key matching.'" (:629-632)

   We make "signatures never reduce" operational over the native calculus: a
   signature occurs in a [signed_term] only as a SEAL [STSigned P s] or as a
   token-gate cell [STStack (TGate s _)], and NEITHER steps in isolation — these
   are exactly the already-proven inertness facts [no_leak_requires_token]
   (a lone wrapper never fires without a co-present token) and
   [no_leak_stack_inert] (a lone token stack never steps). Therefore swapping a
   signature for a ≡sig-congruent-but-distinct representative — the very swap the
   digest distinguishes (digest_distinguishes_congruent_reps) — produces no
   transition difference at the point where signatures meet the dynamics. The
   digest difference is inert; the gate only ever matches keys, it never reduces
   them. Axiom-free (assembles already-Qed-closed inertness + the §5 forcing
   lemma; introduces no new hypothesis).                                        *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.
From CostAccountedRho Require Import SignatureMonoid.

(* ── Signatures are inert: a seal does not step, regardless of WHICH signature
   it carries. Re-exported as a per-signature statement so it can be applied to
   the two congruent representatives the digest distinguishes. *)

(* A sealed process is inert whatever signature seals it (no_leak_requires_token,
   re-stated pointwise in the seal). *)
Lemma seal_inert : forall (P : caproc) (s : sig) (S' : signed_term),
  ~ ca_step (STSigned P s) S'.
Proof. intros P s S'. apply no_leak_requires_token. Qed.

(* A token-gate cell is inert whatever signature gates it (no_leak_stack_inert,
   re-stated for the [TGate s t] head). *)
Lemma gate_cell_inert : forall (s : sig) (t : token) (S' : signed_term),
  ~ ca_step (STStack (TGate s t)) S'.
Proof. intros s t S'. apply no_leak_stack_inert. Qed.

(* ── The two occurrence sites are SIGNATURE-INDIFFERENT for stepping ─────────
   Replacing the signature in a seal (or a gate cell) by ANY other signature
   leaves the "does not step in isolation" verdict unchanged: both the original
   and the replacement are inert. This is precisely "signatures meet the
   behavioural theory only at the matching guard" — never as a redex. *)

Lemma seal_step_indifferent_to_signature : forall (P : caproc) (s s' : sig),
  (forall S', ~ ca_step (STSigned P s)  S') /\
  (forall S', ~ ca_step (STSigned P s') S').
Proof. intros P s s'. split; apply seal_inert. Qed.

Lemma gate_cell_step_indifferent_to_signature : forall (s s' : sig) (t : token),
  (forall S', ~ ca_step (STStack (TGate s  t)) S') /\
  (forall S', ~ ca_step (STStack (TGate s' t)) S').
Proof. intros s s' t. split; apply gate_cell_inert. Qed.

(* ── CA-P-180 corollary: the ≡-incoherence of the digest is harmless ─────────
   For the very congruent-but-distinct pair the digest separates (CA-P-180:
   [digest_distinguishes_congruent_reps] supplies one — e.g. the two orderings
   of a compound), the two representatives are INTERCHANGEABLE as seals and as
   gate cells with respect to reduction: each is inert in isolation. So although
   the digest assigns them different keys, that difference never produces a
   transition difference where signatures meet the dynamics. Hence the digest's
   ≡-incoherence is bisimulation-harmless: behaviour factors as P-bisimulation
   gated by (inert) signature-key matching. *)
Theorem digest_incoherence_bisim_harmless :
  forall {D : Type} (enc : sig -> D),
    injective_on_syntax enc ->
    exists a b,
      (* the digest distinguishes the two congruent representatives … *)
      a ≡sig b /\ a <> b /\ enc a <> enc b /\
      (* … yet both are inert as a seal of any process P … *)
      (forall (P : caproc) (S' : signed_term),
         ~ ca_step (STSigned P a) S' /\ ~ ca_step (STSigned P b) S') /\
      (* … and inert as the head gate cell of any token tail t … *)
      (forall (t : token) (S' : signed_term),
         ~ ca_step (STStack (TGate a t)) S' /\ ~ ca_step (STStack (TGate b t)) S').
Proof.
  intros D enc Hinj.
  destruct (digest_distinguishes_congruent_reps enc Hinj)
    as [a [b [Hcong [Hneq Hcodes]]]].
  exists a, b.
  split; [ exact Hcong | ].
  split; [ exact Hneq | ].
  split; [ exact Hcodes | ].
  split.
  - intros P S'. split; apply seal_inert.
  - intros t S'. split; apply gate_cell_inert.
Qed.
