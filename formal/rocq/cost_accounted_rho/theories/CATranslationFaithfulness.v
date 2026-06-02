(* ════════════════════════════════════════════════════════════════════════
   CATranslationFaithfulness.v — native translation faithfulness (Stage 4b,
   design doc §3 module 2).

   Builds, in one Section over the audited hash/ground hypotheses, toward the
   forward-simulation headline (Thm A): every native [ca_step] is matched, up to
   strong bisimulation, by a rho_reachable run of the translated source. Stage:
   foundation — the N_tr/T_tr lift/subst invariance lemmas (the translated
   signature/token images are closed, hence inert under the substitutions a COMM
   performs). The depth-aware commutation (L3), the dequote-collapse bisimilarity
   (L4), the per-rule simulations and the headline build on these.

   Closedness is re-proven in-Section (mirroring CATranslation.N_tr_closed) to
   keep the audited hypotheses as the Section's own Variables/Hypotheses, so the
   headlines discharge to "Closed under the global context". Axiom-free.        *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CATranslation.
From CostAccountedRho Require Import CATranslationLemmas.

Section CATranslationFaithfulnessSec.

Variable hash_process : list bool -> proc.
Hypothesis hash_process_injective :
  forall b1 b2, hash_process b1 = hash_process b2 -> b1 = b2.
Hypothesis hash_process_closed : forall bs, closed_proc (hash_process bs).
Variable ground_process : list bool -> proc.
Hypothesis ground_process_injective :
  forall b1 b2, ground_process b1 = ground_process b2 -> b1 = b2.
Hypothesis ground_process_closed : forall bs, closed_proc (ground_process bs).
Hypothesis ground_hash_disjoint :
  forall b1 b2, ground_process b1 <> hash_process b2.

(* The translation functions specialised to this Section's hash/ground. *)
Local Notation Nt := (N_tr hash_process ground_process).
Local Notation Tt := (T_tr hash_process ground_process).
Local Notation Pt := (p_tr hash_process ground_process).
Local Notation Ct := (caname_tr hash_process ground_process).
Local Notation St := (st_tr hash_process ground_process).

(* ── Closedness of the signature/token images (in-Section) ──────────────── *)

Lemma Nt_closed : forall s, closed_name (Nt s).
Proof.
  induction s; simpl.
  - unfold closed_name; simpl; exact I.
  - apply closed_Quote, ground_process_closed.
  - apply closed_Quote, hash_process_closed.
  - apply closed_Quote. apply closed_PPar; apply closed_PDeref; assumption.
Qed.

Lemma Tt_closed : forall t, closed_proc (Tt t).
Proof.
  induction t; simpl.
  - apply closed_PNil.
  - apply closed_POutput; [ apply Nt_closed | assumption ].
Qed.

(* ── L (invariance): the closed images are inert under COMM's substitutions ── *)

Lemma Nt_lift_inv : forall s d c, lift_name d c (Nt s) = Nt s.
Proof. intros; apply closed_name_lift_zero, Nt_closed. Qed.

Lemma Nt_subst_inv : forall s k N, subst_name (Nt s) k N = Nt s.
Proof. intros; apply closed_name_subst_zero, Nt_closed. Qed.

Lemma Tt_lift_inv : forall t d c, lift_proc d c (Tt t) = Tt t.
Proof. intros; apply closed_proc_lift_zero, Tt_closed. Qed.

Lemma Tt_subst_inv : forall t k N, subst_proc (Tt t) k N = Tt t.
Proof. intros; apply closed_proc_subst_zero, Tt_closed. Qed.

End CATranslationFaithfulnessSec.
