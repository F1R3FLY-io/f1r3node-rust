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
From CostAccountedRho Require Import RhoReduction.
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

(* ── The depth-indexed translation st_trd (d,c) ─────────────────────────────
   Mirrors St but threads a lift (shift d at cutoff c) so the L3 commutation has
   a clean structural IH. The bridge proves st_trd d c = lift_proc d c ∘ St — so
   it is a proof device, with St (= st_trd 0 0 by lift_zero) the public form.   *)

Fixpoint p_trd (d c : nat) (P : caproc) : proc :=
  match P with
  | CPNil        => PNil
  | CPInput x T  => PInput (cn_trd d c x) (st_trd d (S c) T)
  | CPOutput x U => POutput (cn_trd d c x) (st_trd d c U)
  | CPPar A B    => PPar (p_trd d c A) (p_trd d c B)
  | CPDeref x    => PDeref (cn_trd d c x)
  end
with cn_trd (d c : nat) (x : caname) : name :=
  match x with
  | CQuote T => Quote (st_trd d c T)
  | CNVar k  => if c <=? k then NVar (k + d) else NVar k
  end
with st_trd (d c : nat) (T : signed_term) : proc :=
  match T with
  | STSigned P s =>
      match s with
      | SAnd s1 s2 =>
          PInput (Nt s1) (PInput (Nt s2)
            (PPar (lift_proc 2 0 (p_trd d c P))
                  (PPar (PDeref (NVar 1)) (PDeref (NVar 0)))))
      | _ =>
          PInput (Nt s) (PPar (lift_proc 1 0 (p_trd d c P)) (PDeref (NVar 0)))
      end
  | STPar A B => PPar (st_trd d c A) (st_trd d c B)
  | STStack t => Tt t
  end.

(* The bridge: the depth-indexed translation equals the lift of the plain one. *)
Lemma trd_bridge :
  (forall P d c, p_trd d c P = lift_proc d c (Pt P))
  /\ (forall x d c, cn_trd d c x = lift_name d c (Ct x))
  /\ (forall T d c, st_trd d c T = lift_proc d c (St T)).
Proof.
  apply ca_mutind.
  - (* CPNil *) intros d c; reflexivity.
  - (* CPInput x T *) intros x IHx T IHT d c; simpl; rewrite IHx, IHT; reflexivity.
  - (* CPOutput x U *) intros x IHx U IHU d c; simpl; rewrite IHx, IHU; reflexivity.
  - (* CPPar A B *) intros A IHA B IHB d c; simpl; rewrite IHA, IHB; reflexivity.
  - (* CPDeref x *) intros x IHx d c; simpl; rewrite IHx; reflexivity.
  - (* CQuote T *) intros T IHT d c; simpl; rewrite IHT; reflexivity.
  - (* CNVar k *) intros k d c; reflexivity.
  - (* STSigned P s *)
    intros P IHP s d c; destruct s as [| bs | bs | s1 s2].
    + (* SUnit — channel Quote PNil, closed by computation *)
      simpl; rewrite IHP; f_equal; f_equal;
      symmetry; replace (S c) with (c + 1) by lia; apply lift_lift_comm_proc; lia.
    + (* SGround bs — channel Quote (ground_process bs) *)
      simpl; rewrite (closed_proc_lift_zero (ground_process bs) d c (ground_process_closed bs));
      rewrite IHP; f_equal; f_equal;
      symmetry; replace (S c) with (c + 1) by lia; apply lift_lift_comm_proc; lia.
    + (* SQuote bs — channel Quote (hash_process bs) *)
      simpl; rewrite (closed_proc_lift_zero (hash_process bs) d c (hash_process_closed bs));
      rewrite IHP; f_equal; f_equal;
      symmetry; replace (S c) with (c + 1) by lia; apply lift_lift_comm_proc; lia.
    + (* SAnd s1 s2 — nested two-gate, body lifted by 2 *)
      simpl;
      rewrite (Nt_lift_inv s1 d c), (Nt_lift_inv s2 d (S c));
      rewrite IHP; f_equal; f_equal; f_equal;
      symmetry; replace (S (S c)) with (c + 2) by lia; apply lift_lift_comm_proc; lia.
  - (* STPar A B *) intros A IHA B IHB d c; simpl; rewrite IHA, IHB; reflexivity.
  - (* STStack t *) intros t d c; simpl; symmetry; apply Tt_lift_inv.
Qed.

(* St is the depth-zero translation. *)
Lemma st_trd_zero : forall T, st_trd 0 0 T = St T.
Proof. intro T. rewrite (proj2 (proj2 trd_bridge) T 0 0). apply lift_zero_proc. Qed.

(* ── Per-rule operational simulation (forward reachability) ─────────────────
   The gate-firing reduction: a gate body's bound token-variable is replaced by
   the received (quoted) token; the lifted payload un-lifts (subst_lift_zero),
   and the *t deref SEMANTICALLY DEREFERENCES the received quote (subst_proc on
   PDeref (NVar 0) by Quote Q yields Q) — so the token Tt t is released live. *)

Lemma gate_body_subst : forall A Q,
  subst_proc (PPar (lift_proc 1 0 A) (PDeref (NVar 0))) 0 (Quote Q) = PPar A Q.
Proof. intros A Q; simpl; rewrite subst_lift_zero; reflexivity. Qed.

(* Rule 1 (any ATOMIC signature s — the gate channel Nt s equals the token
   channel, so it fires directly; SAnd is excluded as it routes a combined token
   and needs a Split mediator). The translated redex fires in two COMMs — the
   gate COMM consumes the token (releasing the stack tail Tt t live, via
   gate_body_subst), then the released for|send COMM substitutes the payload. The
   token part Tt t matches the target St(RHS) exactly; the only residual gap
   (payload body at *x-force positions) is the dequote-collapse handled by the
   bisimulation layer. *)
Lemma rule1_reachable : forall x T U s t,
  (forall a b, s <> SAnd a b) ->
  rho_reachable
    (St (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) s) (STStack (TGate s t))))
    (PPar (subst_proc (St T) 0 (Quote (St U))) (Tt t)).
Proof.
  intros x T U s t Hns.
  assert (fire : forall ss tt,
    rho_reachable
      (PPar (PInput (Nt ss)
               (PPar (lift_proc 1 0 (Pt (CPPar (CPInput x T) (CPOutput x U)))) (PDeref (NVar 0))))
            (POutput (Nt ss) (Tt tt)))
      (PPar (subst_proc (St T) 0 (Quote (St U))) (Tt tt))).
  { intros ss tt.
    eapply rr_step. { apply rs_comm. }
    rewrite gate_body_subst.
    eapply rr_step. { apply rs_par_l; apply rs_comm. }
    apply rr_refl. }
  destruct s as [| bs | bs | a b].
  - apply fire.
  - apply fire.
  - apply fire.
  - exfalso; apply (Hns a b); reflexivity.
Qed.

End CATranslationFaithfulnessSec.
