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
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import CATranslation.
From CostAccountedRho Require Import CATranslationLemmas.

(* Reachability is a congruence for parallel composition (RhoReduction provides
   only the single-step rs_par_l/r); needed to lift sub-reductions (Split firing,
   the ca_par congruence rules) under a parallel context. *)
Lemma rho_reachable_par_l : forall P P' Q,
  rho_reachable P P' -> rho_reachable (PPar P Q) (PPar P' Q).
Proof.
  intros P P' Q H. induction H.
  - apply rr_refl.
  - eapply rr_step; [ apply rs_par_l; eassumption | assumption ].
Qed.

Lemma rho_reachable_par_r : forall P Q Q',
  rho_reachable Q Q' -> rho_reachable (PPar P Q) (PPar P Q').
Proof.
  intros P Q Q' H. induction H.
  - apply rr_refl.
  - eapply rr_step; [ apply rs_par_r; eassumption | assumption ].
Qed.

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
  | CPJoin xs T  => lift_proc d c (Pt (CPJoin xs T))
                    (* the join's depth-indexed image IS the lift of its plain
                       translation (the bridge holds definitionally here; the join
                       is not destructured by the per-rule reachability lemmas). *)
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
  - (* CPJoin xs T — bridge holds definitionally (p_trd join arm = lift of Pt) *)
    intros xs T IHT d c; reflexivity.
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

(* Rule 5 (split processes, split tokens; both signatures atomic). The two
   separate gates each fire against their own token (no Split mediator needed —
   each gate channel equals its token channel), then the released for|send fires.
   Three COMMs, with ≡-rearrangement (se_par_assoc/se_par_cross) pairing each gate
   with its token and then the receiver with the sender. *)
Lemma rule5_reachable : forall x T U s1 s2 t1 t2,
  (forall a b, s1 <> SAnd a b) -> (forall a b, s2 <> SAnd a b) ->
  rho_reachable
    (St (STPar (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
                      (STStack (TGate s1 t1)))
               (STStack (TGate s2 t2))))
    (PPar (subst_proc (St T) 0 (Quote (St U))) (PPar (Tt t1) (Tt t2))).
Proof.
  intros x T U s1 s2 t1 t2 Hns1 Hns2.
  assert (fire5 : forall n1 n2,
    rho_reachable
      (PPar (PPar (PPar (PInput n1 (PPar (lift_proc 1 0 (PInput (Ct x) (St T))) (PDeref (NVar 0))))
                        (PInput n2 (PPar (lift_proc 1 0 (POutput (Ct x) (St U))) (PDeref (NVar 0)))))
                  (POutput n1 (Tt t1)))
            (POutput n2 (Tt t2)))
      (PPar (subst_proc (St T) 0 (Quote (St U))) (PPar (Tt t1) (Tt t2)))).
  { intros n1 n2.
    eapply rr_step.
    { eapply rs_struct.
      - eapply se_trans. { apply se_par_assoc. } apply se_par_cross.
      - apply rs_par_l. apply rs_comm.
      - apply se_refl. }
    rewrite gate_body_subst.
    eapply rr_step.
    { apply rs_par_r. apply rs_comm. }
    rewrite gate_body_subst.
    eapply rr_step.
    { eapply rs_struct.
      - apply se_par_cross.
      - apply rs_par_l. apply rs_comm.
      - apply se_refl. }
    apply rr_refl. }
  destruct s1 as [| b1 | b1 | a1 c1]; try (exfalso; eapply Hns1; reflexivity);
  destruct s2 as [| b2 | b2 | a2 c2]; try (exfalso; eapply Hns2; reflexivity);
  apply fire5.
Qed.

(* The compound (SAnd) gate's OUTER firing: the outer gate consumes its token and
   exposes the inner gate, the body's lift dropping 2→1 (subst_lift_two_one), the
   first payload deref releasing the closed token Q1. *)
Lemma nested_gate_subst : forall n2 A Q1,
  closed_name n2 -> closed_proc Q1 ->
  subst_proc (PInput n2 (PPar (lift_proc 2 0 A) (PPar (PDeref (NVar 1)) (PDeref (NVar 0))))) 0 (Quote Q1)
  = PInput n2 (PPar (lift_proc 1 0 A) (PPar Q1 (PDeref (NVar 0)))).
Proof.
  intros n2 A Q1 Hn Hq. simpl.
  rewrite (closed_name_subst_zero n2 0 (Quote Q1) Hn).
  rewrite subst_lift_two_one.
  rewrite (closed_proc_lift_zero Q1 1 0 Hq).
  reflexivity.
Qed.

(* The compound gate's INNER firing: the inner gate consumes its token, the body's
   lift dropping 1→0, the second payload deref releasing Q2; the already-released
   first token R (closed) is inert. *)
Lemma gate2_body_subst : forall A R Q2,
  closed_proc R ->
  subst_proc (PPar (lift_proc 1 0 A) (PPar R (PDeref (NVar 0)))) 0 (Quote Q2)
  = PPar A (PPar R Q2).
Proof.
  intros A R Q2 HR. simpl.
  rewrite subst_lift_zero.
  rewrite (closed_proc_subst_zero R 0 (Quote Q2) HR).
  reflexivity.
Qed.

(* Rule 2 (combined process gate signed SAnd s1 s2, but SPLIT tokens TGate s1 /
   TGate s2). The nested two-gate fires against the two pre-split tokens — outer
   on Nt s1, inner on Nt s2 — needing NO Split (the gate channels equal the split
   token channels for any s1, s2), then the released for|send fires. Three COMMs. *)
Lemma rule2_reachable : forall x T U s1 s2 t1 t2,
  rho_reachable
    (St (STPar (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
                      (STStack (TGate s1 t1)))
               (STStack (TGate s2 t2))))
    (PPar (subst_proc (St T) 0 (Quote (St U))) (PPar (Tt t1) (Tt t2))).
Proof.
  intros x T U s1 s2 t1 t2.
  (* Step 1: outer gate (Nt s1) | tok1, inside the inner PPar *)
  eapply rr_step.
  { apply rs_par_l. apply rs_comm. }
  rewrite nested_gate_subst by (try apply Nt_closed; apply Tt_closed).
  (* Step 2: inner gate (Nt s2) | tok2, now adjacent at the top PPar *)
  eapply rr_step.
  { apply rs_comm. }
  rewrite gate2_body_subst by apply Tt_closed.
  (* Step 3: the released for|send *)
  eapply rr_step.
  { apply rs_par_l. apply rs_comm. }
  apply rr_refl.
Qed.

(* ── The Split mediator (for the combined-token rules 3 and 4) ──────────────
   Split receives a token on the compound channel Nt (SAnd s1 s2) and produces an
   s1-token with empty payload and an s2-token forwarding the received payload —
   the native port of Translation.Split. The combined-token rules route their
   token through Split (in parallel context) before the nested/split gates fire. *)

Definition Split (s1 s2 : sig) : proc :=
  PInput (Nt (SAnd s1 s2))
    (PPar (POutput (Nt s1) PNil)
          (POutput (Nt s2) (PDeref (NVar 0)))).

Lemma Split_closed : forall s1 s2, closed_proc (Split s1 s2).
Proof.
  intros s1 s2. unfold Split. apply closed_PInput.
  - apply Nt_closed.
  - simpl. repeat split.
    + apply closed_name_at_mono with (k := 0); [ lia | apply Nt_closed ].
    + apply closed_name_at_mono with (k := 0); [ lia | apply Nt_closed ].
    + simpl; lia.
Qed.

(* Split fires against a combined token, emitting the two component tokens (the
   s1-token empty, the s2-token carrying the forwarded — dequoted — payload). *)
Lemma Split_fires : forall s1 s2 Q,
  rho_reachable
    (PPar (Split s1 s2) (POutput (Nt (SAnd s1 s2)) Q))
    (PPar (POutput (Nt s1) PNil) (POutput (Nt s2) Q)).
Proof.
  intros s1 s2 Q. unfold Split. eapply rr_step.
  { apply rs_comm. }
  simpl.
  rewrite (Nt_subst_inv s1 0 (Quote Q)), (Nt_subst_inv s2 0 (Quote Q)).
  apply rr_refl.
Qed.

Lemma split_body_subst : forall s1 s2 Q,
  subst_proc (PPar (POutput (Nt s1) PNil) (POutput (Nt s2) (PDeref (NVar 0)))) 0 (Quote Q)
  = PPar (POutput (Nt s1) PNil) (POutput (Nt s2) Q).
Proof.
  intros s1 s2 Q. simpl.
  rewrite (Nt_subst_inv s1 0 (Quote Q)), (Nt_subst_inv s2 0 (Quote Q)).
  reflexivity.
Qed.

(* Rule 3 (combined process gate signed SAnd, COMBINED token) — needs the Split
   mediator (in parallel context). Four COMMs: Split fires (splitting the combined
   token into an s1-token [empty] and an s2-token [carrying Tt t]); the outer gate
   fires on Nt s1 with the empty token; the inner gate fires on Nt s2 with Tt t;
   the released for|send fires. The empty s1-token leaves an inert PNil residue. *)
Lemma rule3_reachable : forall x T U s1 s2 t,
  rho_reachable
    (PPar (St (STPar (STSigned (CPPar (CPInput x T) (CPOutput x U)) (SAnd s1 s2))
                     (STStack (TGate (SAnd s1 s2) t))))
          (Split s1 s2))
    (PPar (subst_proc (St T) 0 (Quote (St U))) (PPar PNil (Tt t))).
Proof.
  intros x T U s1 s2 t.
  (* Step 1: Split fires (bring SPLIT|TOK adjacent, fire) *)
  eapply rr_step.
  { eapply rs_struct.
    - eapply se_trans. { apply se_par_comm. }
      eapply se_trans. { apply se_par_cong_r. apply se_par_comm. }
      apply se_sym; apply se_par_assoc.
    - apply rs_par_l. apply rs_comm.
    - apply se_refl. }
  rewrite split_body_subst.
  (* Step 2: outer gate (Nt s1) | s1-token (empty) *)
  eapply rr_step.
  { eapply rs_struct.
    - eapply se_trans. { apply se_par_comm. } apply se_sym; apply se_par_assoc.
    - apply rs_par_l. apply rs_comm.
    - apply se_refl. }
  rewrite nested_gate_subst by (try apply Nt_closed; apply closed_PNil).
  (* Step 3: inner gate (Nt s2) | s2-token (Tt t) — now adjacent *)
  eapply rr_step.
  { apply rs_comm. }
  rewrite gate2_body_subst by apply closed_PNil.
  (* Step 4: the released for|send *)
  eapply rr_step.
  { apply rs_par_l. apply rs_comm. }
  apply rr_refl.
Qed.

(* Rule 4 (SPLIT processes, COMBINED token; both signatures atomic) — the
   receiver and sender are separately signed (s1, s2), the token is combined, so
   it needs the Split mediator. Four COMMs: Split fires (combined → s1-token empty
   + s2-token Tt t), the receiver gate fires on Nt s1 (empty token), the sender
   gate fires on Nt s2 (Tt t), the released for|send fires. No Join (GAP-2: the
   continuation keeps its own seal). The empty s1-token leaves an inert PNil. *)
Lemma rule4_reachable : forall x T U s1 s2 t,
  (forall a b, s1 <> SAnd a b) -> (forall a b, s2 <> SAnd a b) ->
  rho_reachable
    (PPar (St (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
                     (STStack (TGate (SAnd s1 s2) t))))
          (Split s1 s2))
    (PPar (subst_proc (St T) 0 (Quote (St U))) (PPar PNil (Tt t))).
Proof.
  intros x T U s1 s2 t Hns1 Hns2.
  assert (fire4 : forall ss1 ss2,
    rho_reachable
      (PPar (PPar (PPar (PInput (Nt ss1) (PPar (lift_proc 1 0 (PInput (Ct x) (St T))) (PDeref (NVar 0))))
                        (PInput (Nt ss2) (PPar (lift_proc 1 0 (POutput (Ct x) (St U))) (PDeref (NVar 0)))))
                  (POutput (Nt (SAnd ss1 ss2)) (Tt t)))
            (Split ss1 ss2))
      (PPar (subst_proc (St T) 0 (Quote (St U))) (PPar PNil (Tt t)))).
  { intros ss1 ss2.
    (* Step 1: Split fires *)
    eapply rr_step.
    { eapply rs_struct.
      - eapply se_trans. { apply se_par_comm. }
        eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
        apply se_sym; apply se_par_assoc.
      - apply rs_par_l. apply rs_comm.
      - apply se_refl. }
    rewrite split_body_subst.
    (* Step 2: receiver gate (Nt ss1) | s1-token (empty) *)
    eapply rr_step.
    { eapply rs_struct.
      - eapply se_trans. { apply se_par_cross. } apply se_par_cong; apply se_par_comm.
      - apply rs_par_l. apply rs_comm.
      - apply se_refl. }
    rewrite gate_body_subst.
    (* Step 3: sender gate (Nt ss2) | s2-token (Tt t) *)
    eapply rr_step.
    { apply rs_par_r. apply rs_comm. }
    rewrite gate_body_subst.
    (* Step 4: the released for|send *)
    eapply rr_step.
    { eapply rs_struct.
      - apply se_par_cross.
      - apply rs_par_l. apply rs_comm.
      - apply se_refl. }
    apply rr_refl. }
  destruct s1 as [| b1 | b1 | a1 c1]; try (exfalso; eapply Hns1; reflexivity);
  destruct s2 as [| b2 | b2 | a2 c2]; try (exfalso; eapply Hns2; reflexivity);
  apply fire4.
Qed.

(* ── General faithfulness headline: every native ca_step's translation makes
   progress (no deadlock) ─────────────────────────────────────────────────────
   For each ca_step S S' there is a closed mediating context Ctx (PNil for the
   directly-firing rules, the Split mediator for the combined-token rules; for a
   compound-signed gate the Split is the one for that gate's outermost ∧) such
   that the translated system PPar (St S) Ctx takes a real rho step. The
   congruence rules lift the inner step through PPar via rs_struct. This packages
   the five per-rule simulations into one theorem over the whole reduction
   relation, axiom-free. *)
(* The combined J2 funding key is ∧-headed once any sender is present, so a Split
   mediator can decompose it (the N=0 corner collapses the key to the receiver
   seal s1). *)
Lemma fold_left_SAnd_is_and : forall ts a b,
  exists c d, fold_left (fun acc t => SAnd acc t) ts (SAnd a b) = SAnd c d.
Proof.
  induction ts as [| u us IH]; intros a b; simpl.
  - exists a, b. reflexivity.
  - apply IH.
Qed.

Lemma join_token_key_cons_and : forall s1 t ts,
  exists a b, join_token_key s1 (t :: ts) = SAnd a b.
Proof. intros s1 t ts. unfold join_token_key. simpl. apply fold_left_SAnd_is_and. Qed.

Theorem ca_translation_progresses : forall S S',
  ca_step S S' -> exists Ctx W, closed_proc Ctx /\ rho_step (PPar (St S) Ctx) W.
Proof.
  intros S S' H. induction H.
  - (* ca_rule1: x T U s t *)
    destruct s as [| bs | bs | a b].
    + exists PNil; eexists; split; [ apply closed_PNil | apply rs_par_l; apply rs_comm ].
    + exists PNil; eexists; split; [ apply closed_PNil | apply rs_par_l; apply rs_comm ].
    + exists PNil; eexists; split; [ apply closed_PNil | apply rs_par_l; apply rs_comm ].
    + (* SAnd a b — Split mediator *)
      exists (Split a b); eexists; split; [ apply Split_closed | ].
      eapply rs_struct.
      * eapply se_trans. { apply se_par_comm. }
        eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
        apply se_sym; apply se_par_assoc.
      * apply rs_par_l. apply rs_comm.
      * apply se_refl.
  - (* ca_rule2 — directly firing, no Split *)
    exists PNil; eexists; split;
      [ apply closed_PNil | apply rs_par_l; apply rs_par_l; apply rs_comm ].
  - (* ca_rule3 — combined token, Split mediator *)
    exists (Split s1 s2); eexists; split; [ apply Split_closed | ].
    eapply rs_struct.
    + eapply se_trans. { apply se_par_comm. }
      eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
      apply se_sym; apply se_par_assoc.
    + apply rs_par_l. apply rs_comm.
    + apply se_refl.
  - (* ca_rule4 — split processes, combined token, Split mediator *)
    exists (Split s1 s2); eexists; split; [ apply Split_closed | ].
    eapply rs_struct.
    + eapply se_trans. { apply se_par_comm. }
      eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
      apply se_sym; apply se_par_assoc.
    + apply rs_par_l. apply rs_comm.
    + apply se_refl.
  - (* ca_rule5: split processes, split tokens — first step depends on s1 *)
    destruct s1 as [| bs | bs | a b].
    1-3: (exists PNil; eexists; split; [ apply closed_PNil | ];
          eapply rs_struct;
          [ eapply se_trans; [ apply se_par_nil | ];
            eapply se_trans; [ apply se_par_assoc | ]; apply se_par_cross
          | apply rs_par_l; apply rs_comm
          | apply se_refl ]).
    (* SAnd a b — the s1-token is itself compound; Split it. Bring Split|tok1
       adjacent: (((g1|g2)|tok1)|tok2)|Split ≡ (Split|tok1)|((g1|g2)|tok2). *)
    exists (Split a b); eexists; split; [ apply Split_closed | ].
    eapply rs_struct.
    + eapply se_trans. { apply se_par_comm. }
      eapply se_trans.
      { apply se_par_cong_r.
        eapply se_trans. { apply se_par_cong_l; apply se_par_comm. }
        apply se_par_assoc. }
      apply se_sym; apply se_par_assoc.
    + apply rs_par_l. apply rs_comm.
    + apply se_refl.
  - (* ca_join1: the whole-join fires its gate↔token COMM exactly as ca_rule1 (the
       N=1 instance). The join body (CPJoin xs T | snds) sits INSIDE the gate and is
       untouched by this first step, so progress is uniform in the body — atomic s
       fires directly, compound s via the Split mediator. (Progress form: one real
       rho_step; the full N-step rendezvous is the join's reduction-side metatheory,
       CAReduction/CAStrongNormalization.) *)
    destruct s as [| bs | bs | a b].
    + exists PNil; eexists; split; [ apply closed_PNil | apply rs_par_l; apply rs_comm ].
    + exists PNil; eexists; split; [ apply closed_PNil | apply rs_par_l; apply rs_comm ].
    + exists PNil; eexists; split; [ apply closed_PNil | apply rs_par_l; apply rs_comm ].
    + exists (Split a b); eexists; split; [ apply Split_closed | ].
      eapply rs_struct.
      * eapply se_trans. { apply se_par_comm. }
        eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
        apply se_sym; apply se_par_assoc.
      * apply rs_par_l. apply rs_comm.
      * apply se_refl.
  - (* ca_join2: the combined token (keyed s1 ∘ t1 ∘ … ∘ tN) is consumed atomically.
       For N≥1 the key is ∧-headed → a Split mediator decomposes it (exactly the
       ca_rule3 first step, with the receiver|senders block as the opaque blob). For
       N=0 the key collapses to s1 and the receiver gate fires on its own seal. *)
    subst snds. destruct ts as [| t0 ts0].
    + (* N = 0: join_token_key s1 [] = s1 *)
      unfold join_token_key; simpl. destruct s1 as [| bs | bs | a b].
      * exists PNil; eexists; split; [ apply closed_PNil | ]. apply rs_par_l.
        eapply rs_struct.
        -- eapply se_trans. { apply se_par_assoc. }
           eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
           apply se_sym; apply se_par_assoc.
        -- apply rs_par_l. apply rs_comm.
        -- apply se_refl.
      * exists PNil; eexists; split; [ apply closed_PNil | ]. apply rs_par_l.
        eapply rs_struct.
        -- eapply se_trans. { apply se_par_assoc. }
           eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
           apply se_sym; apply se_par_assoc.
        -- apply rs_par_l. apply rs_comm.
        -- apply se_refl.
      * exists PNil; eexists; split; [ apply closed_PNil | ]. apply rs_par_l.
        eapply rs_struct.
        -- eapply se_trans. { apply se_par_assoc. }
           eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
           apply se_sym; apply se_par_assoc.
        -- apply rs_par_l. apply rs_comm.
        -- apply se_refl.
      * exists (Split a b); eexists; split; [ apply Split_closed | ].
        eapply rs_struct.
        -- eapply se_trans. { apply se_par_comm. }
           eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
           apply se_sym; apply se_par_assoc.
        -- apply rs_par_l. apply rs_comm.
        -- apply se_refl.
    + (* N ≥ 1: the combined key is ∧-headed — Split it (ca_rule3 first step) *)
      destruct (join_token_key_cons_and s1 t0 ts0) as [a [b HK]]. rewrite HK.
      exists (Split a b); eexists; split; [ apply Split_closed | ].
      eapply rs_struct.
      * eapply se_trans. { apply se_par_comm. }
        eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
        apply se_sym; apply se_par_assoc.
      * apply rs_par_l. apply rs_comm.
      * apply se_refl.
  - (* ca_par_l: S1 steps *)
    destruct IHca_step as [Ctx1 [W1 [Hcl Hstep]]].
    exists Ctx1; eexists; split; [ exact Hcl | ].
    eapply rs_struct.
    + eapply se_trans. { apply se_par_assoc. }
      eapply se_trans. { apply se_par_cong_r; apply se_par_comm. }
      apply se_sym; apply se_par_assoc.
    + apply rs_par_l. exact Hstep.
    + apply se_refl.
  - (* ca_par_r: S2 steps *)
    destruct IHca_step as [Ctx2 [W2 [Hcl Hstep]]].
    exists Ctx2; eexists; split; [ exact Hcl | ].
    eapply rs_struct.
    + apply se_par_assoc.
    + apply rs_par_r. exact Hstep.
    + apply se_refl.
Qed.

End CATranslationFaithfulnessSec.
