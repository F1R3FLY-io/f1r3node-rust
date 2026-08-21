(* ═══════════════════════════════════════════════════════════════════════════
   Replication.v — Meredith's reflection-based replication encoding
   ═══════════════════════════════════════════════════════════════════════════

   Mechanizes the replication encoding from Meredith-Radestock 2005
   ["A Reflective Higher-Order Calculus", ENTCS 141(5):49-67], §3,
   page 10, which is the authoritative source cited by the
   cost-accounted rho paper's §5 Remark (lines 540-545).

   The encoding:

       D(x)   ≜  x(y).(x[y] | *y)                where  x[y] ≜ x⟨|*y|⟩
       !P(x)  ≜  x⟨|D(x) | P|⟩ | D(x)

   **Key trace** (using R.1's semantic substitution):

       !P(x) = x⟨|D(x) | P|⟩ | D(x)
             ≡  D(x) | x⟨|D(x) | P|⟩            (se_par_comm)
             →  (x[y] | *y){⌜D(x)|P⌝/y}          (rs_comm)
             =  x⟨|D(x) | P|⟩ | D(x) | P         (semantic subst collapses *⌜…⌝)
             ≡  !P(x) | P                        (se_par_assoc)

   So one rho_step of `bang_encoding x P` produces `PPar (bang_encoding
   x P) P` — exactly matching `rs_replicate : PReplicate P →
   PPar P (PReplicate P)`. This is the operational step-matching fact
   that underlies the encoding's correctness.

   Section 12 gives the mechanized, axiom-free observational fact used
   by the verification boundary: every weak input or output barb of the
   body [P] lifts to a weak barb of both [PReplicate P] and
   [bang_encoding x P]. This is the proved direction of
   Meredith-Radestock's §3 encoding that is needed by the
   cost-accounting development. Strict barbed bisimilarity is not a
   faithful statement in this syntax because [bang_encoding x P] has
   top-level barbs on the coordination channel [x] that [PReplicate P]
   lacks.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition                          │ Paper Notation
   ─────────────────────────────────────────┼──────────────────────────
   D_encoding                               │ D(x)
   bang_encoding                            │ !P(x)
   name_not_free_in                         │ x ∉ FN(P)  (structural freshness)
   bang_encoding_unfolds                    │ !P(x) → !P(x) | P
   preplicate_bang_encoding_body_barbs_sound│ body-to-wrapper weak barbs
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: RhoSyntax, RhoReduction, Bisimulation, WeakBarbedEquiv
   (all this project).
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import StructEquivInversion.
From CostAccountedRho Require Import StructEquivHeads.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import Bisimulation.
From CostAccountedRho Require Import WeakBarbedEquiv.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: The encoding
   ═══════════════════════════════════════════════════════════════════════════ *)

(* D(x) ≜ x(y).(x[y] | *y)   where  x[y] ≜ x⟨|*y|⟩.

   Under the input binder [PInput x ·], the bound name y is NVar 0 in
   the body. The outer channel [x] referenced inside the body therefore
   shifts to [lift_name 1 0 x] to account for the extra binder level. *)
Definition D_encoding (x : name) : proc :=
  PInput x
    (PPar
      (POutput (lift_name 1 0 x) (PDeref (NVar 0)))   (* x⟨|*y|⟩ *)
      (PDeref (NVar 0))).                              (* *y *)

(* !P(x) ≜ x⟨|D(x) | P|⟩ | D(x). *)
Definition bang_encoding (x : name) (P : proc) : proc :=
  PPar
    (POutput x (PPar (D_encoding x) P))
    (D_encoding x).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Structural freshness predicate
   ═══════════════════════════════════════════════════════════════════════════

   [name_not_free_in_proc x P] holds when [x] does not occur as a
   channel (the target of any PInput or POutput) anywhere in [P],
   including nested inside any Quote embedded in P's channels. This
   is the freshness condition needed for strong bisimilarity of
   [bang_encoding x P] with [PReplicate P]: without it, some
   [x]-action inside P could fire an unintended COMM with the
   encoding's top-level send or receive on [x], creating a step of
   [bang_encoding x P] that cannot be matched on the [PReplicate P]
   side.                                                                   *)

(* Freshness uses ≡N-non-equivalence (not Leibniz) at the channel level,
   so the predicate is transport-stable under structural equivalence.
   Leibniz [x <> y] would not suffice: [y ≡N y'] via [se_name_quote]
   does not preserve Leibniz inequality, making the [_se] preservation
   lemma false under the naive predicate. *)

(* The "v3" syntactic freshness: adds [~ (x ≡N Quote Q)] at POutput
   payload positions. Rationale: an rs_comm step sends Q as a new
   quoted name to the receiver; for freshness of x to survive, we
   must already rule out x ≡N Quote Q at the source POutput. *)

Fixpoint name_not_free_in_proc (x : name) (P : proc) : Prop :=
  match P with
  | PNil          => True
  | PInput y B    => ~ (x ≡N y) /\ name_not_free_in_name x y
                     /\ name_not_free_in_proc x B
  | POutput y Q   => ~ (x ≡N y) /\ name_not_free_in_name x y
                     /\ ~ (x ≡N Quote Q)
                     /\ name_not_free_in_proc x Q
  | PPar P1 P2    => name_not_free_in_proc x P1 /\ name_not_free_in_proc x P2
  | PDeref y      => name_not_free_in_name x y
  | PReplicate B  => name_not_free_in_proc x B
  end
with name_not_free_in_name (x : name) (y : name) : Prop :=
  match y with
  | Quote Q => name_not_free_in_proc x Q
  | NVar _  => True
  end.

(* "P1" quoted-channels closure: every Quote channel in P has a
   closed body, hereditarily. This is the invariant that rules out
   the counter-example where a free NVar inside a quoted channel body
   could synthesize, under substitution, a term ≡ to x's quoted body.
   The hereditary conjunct [quoted_channels_closed Q] inside the Quote
   case is critical: it makes the predicate stable under lift/subst
   because lifting and substituting closed processes is identity. *)

Fixpoint quoted_channels_closed (P : proc) : Prop :=
  match P with
  | PNil          => True
  | PInput y B    => quoted_name_closed y /\ quoted_channels_closed B
  | POutput y Q   => quoted_name_closed y /\ quoted_channels_closed Q
  | PPar P1 P2    => quoted_channels_closed P1 /\ quoted_channels_closed P2
  | PDeref y      => quoted_name_closed y
  | PReplicate B  => quoted_channels_closed B
  end
with quoted_name_closed (y : name) : Prop :=
  match y with
  | Quote Q => closed_proc Q /\ quoted_channels_closed Q
  | NVar _  => True
  end.

(* The top-level freshness predicate requires:
   - [closed_name x]: x has no free de Bruijn variables.
   - [closed_proc P]: P has no free de Bruijn variables.
   - [quoted_name_closed x]: hereditary closedness of x's quoted body.
   - [quoted_channels_closed P]: every Quote channel in P has closed body.
   - [name_not_free_in_proc x P]: v3 structural predicate.

   Closedness plus hereditary quoted-channel closedness jointly rule out
   the counter-example where subst_proc_deref_nvar_eq_quote could collapse
   an embedded [PDeref (NVar 0)] to a term colliding with x's quoted
   body. *)

Definition name_not_free_in (x : name) (P : proc) : Prop :=
  closed_name x /\ closed_proc P /\
  quoted_name_closed x /\ quoted_channels_closed P /\
  name_not_free_in_proc x P.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Closedness of the encoding
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma D_encoding_closed : forall x,
  closed_name x -> closed_proc (D_encoding x).
Proof.
  intros x Hx. unfold D_encoding, closed_proc. simpl.
  split; [exact Hx|].
  split.
  - (* closed_proc_at 1 (POutput (lift_name 1 0 x) (PDeref (NVar 0))) *)
    split.
    + (* closed_name_at 1 (lift_name 1 0 x).
         Since [closed_name x] is [closed_name_at 0 x], lifting is
         the identity ([closed_name_lift_zero]); then monotonicity
         from 0 to 1. *)
      rewrite closed_name_lift_zero by exact Hx.
      apply (closed_name_at_mono _ 0 1); [lia | exact Hx].
    + (* closed_name_at 1 (NVar 0) = 0 < 1 *) simpl. lia.
  - (* closed_proc_at 1 (PDeref (NVar 0)) = 0 < 1 *) simpl. lia.
Qed.

Lemma bang_encoding_closed : forall x P,
  closed_name x -> closed_proc P ->
  closed_proc (bang_encoding x P).
Proof.
  intros x P Hx HP. unfold bang_encoding, closed_proc. simpl.
  split.
  - (* closed_proc_at 0 (POutput x (PPar (D_encoding x) P)) *)
    split; [exact Hx|].
    split; [apply D_encoding_closed; exact Hx | exact HP].
  - (* closed_proc_at 0 (D_encoding x) *)
    apply D_encoding_closed. exact Hx.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: One-step unfold theorem
   ═══════════════════════════════════════════════════════════════════════════

   The headline operational fact: one [rs_comm] step of
   [bang_encoding x P] produces [bang_encoding x P | P]. The trace
   is:

     bang_encoding x P
       = PPar (POutput x B) (D_encoding x)         with B = PPar (D_encoding x) P
       ≡ PPar (D_encoding x) (POutput x B)         (se_par_comm)
       → subst_proc body 0 (Quote B)               (rs_comm)

   where [body] is the inner PInput body. By [subst_proc_par],
   [subst_lift_zero_name], and [subst_proc_deref_nvar_eq_quote]
   (R.1's key lemma), the substituted body reduces to:

     PPar (POutput x B) B
       = PPar (POutput x B) (PPar (D_encoding x) P)
       ≡ PPar (PPar (POutput x B) (D_encoding x)) P   (se_par_assoc^-1)
       = PPar (bang_encoding x P) P                    (by def).

   The rho_step is packaged as rs_struct around the underlying
   rs_comm, absorbing both the se_par_comm pre-swap and the final
   se_par_assoc post-transport.                                           *)

Theorem bang_encoding_unfolds : forall x P,
  closed_name x -> closed_proc P ->
  rho_step (bang_encoding x P) (PPar (bang_encoding x P) P).
Proof.
  intros x P Hx HP.
  (* Let B = PPar (D_encoding x) P. *)
  set (B := PPar (D_encoding x) P).
  (* The inner input body, with y = NVar 0 under the binder. *)
  set (body :=
    PPar (POutput (lift_name 1 0 x) (PDeref (NVar 0)))
         (PDeref (NVar 0))).
  (* The raw rs_comm result: subst_proc body 0 (Quote B). *)
  assert (HsubstBody :
    subst_proc body 0 (Quote B) = PPar (POutput x B) B).
  { unfold body. rewrite subst_proc_par.
    (* Replace each factor by its substitution result. *)
    replace (subst_proc (POutput (lift_name 1 0 x) (PDeref (NVar 0))) 0 (Quote B))
      with (POutput x B).
    2: { change (subst_proc (POutput (lift_name 1 0 x) (PDeref (NVar 0))) 0 (Quote B))
           with (POutput
                   (subst_name (lift_name 1 0 x) 0 (Quote B))
                   (subst_proc (PDeref (NVar 0)) 0 (Quote B))).
         rewrite subst_lift_zero_name.
         rewrite subst_proc_deref_nvar_eq_quote.
         reflexivity. }
    replace (subst_proc (PDeref (NVar 0)) 0 (Quote B)) with B.
    2: { symmetry. apply subst_proc_deref_nvar_eq_quote. }
    reflexivity. }
  (* Assemble the rho_step with rs_struct absorbing pre-swap and
     post-transport. *)
  eapply rs_struct.
  - (* Pre-equivalence: bang_encoding x P ≡ PPar D_encoding (POutput x B) *)
    apply se_par_comm.
  - (* Core rs_comm: fires on channel x. *)
    change (D_encoding x)
      with (PInput x body) at 1.
    apply (rs_comm x body B).
  - (* Post-equivalence: subst_proc body 0 (Quote B) = PPar (POutput x B) B,
       which is structurally equivalent to PPar (bang_encoding x P) P. *)
    rewrite HsubstBody.
    unfold bang_encoding.
    apply se_sym. apply se_par_assoc.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Strong bisimilarity preparation
   ═══════════════════════════════════════════════════════════════════════════

   We prove [bisim (PReplicate P) (bang_encoding x P)] by exhibiting an
   explicit witness relation and showing it is a STRONG bisimulation
   (step-for-step matching). This is the standard technique in process-
   calculus mechanizations because naive [cofix] guardedness checks
   fail in the presence of parallel-composition congruence (which is
   itself a non-trivial property and not generally provable for strong
   bisim without stronger invariants).

   The helper [bisimulation_implies_bisim_strong] converts any
   single-step matching relation into a [bisim]. This avoids the
   well-known limitation that [bisimulation] in [Bisimulation.v:100]
   is defined with [rho_reachable] (weak) and so gives weak bisim,
   not strong.                                                            *)

Definition strong_simulation (R : proc -> proc -> Prop) : Prop :=
  forall P Q, R P Q ->
  forall P', rho_step P P' ->
  exists Q', rho_step Q Q' /\ R P' Q'.

Definition strong_bisimulation (R : proc -> proc -> Prop) : Prop :=
  strong_simulation R /\ strong_simulation (fun P Q => R Q P).

Lemma strong_bisimulation_implies_bisim :
  forall R, strong_bisimulation R ->
  forall P Q, R P Q -> bisim P Q.
Proof.
  intros R [HF HB]. cofix CH. intros P Q HR.
  apply bisim_intro.
  - intros P' HstepP.
    destruct (HF P Q HR P' HstepP) as [Q' [HstepQ HR']].
    exists Q'. split; [exact HstepQ | apply (CH _ _ HR')].
  - intros Q' HstepQ.
    destruct (HB Q P HR Q' HstepQ) as [P' [HstepP HR']].
    exists P'. split; [exact HstepP | apply (CH _ _ HR')].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Auxiliary lemmas — freshness transport & closedness
   ═══════════════════════════════════════════════════════════════════════════ *)

(* ─────────────────────────────────────────────────────────────────────────
   L1: freshness is preserved by structural equivalence (bidirectional).

   Must be bidirectional because [se_sym] flips the direction, and the
   naive one-directional formulation fails guardedness under
   [se_combined_mut].                                                        *)

Lemma name_not_free_in_se_bidir :
  (forall P Q (_ : P ≡ Q), forall x,
     (name_not_free_in_proc x P -> name_not_free_in_proc x Q) /\
     (name_not_free_in_proc x Q -> name_not_free_in_proc x P)) /\
  (forall n m (_ : n ≡N m), forall x,
     (name_not_free_in_name x n -> name_not_free_in_name x m) /\
     (name_not_free_in_name x m -> name_not_free_in_name x n)).
Proof.
  apply (se_combined_mut
    (fun P Q _ => forall x,
       (name_not_free_in_proc x P -> name_not_free_in_proc x Q) /\
       (name_not_free_in_proc x Q -> name_not_free_in_proc x P))
    (fun n m _ => forall x,
       (name_not_free_in_name x n -> name_not_free_in_name x m) /\
       (name_not_free_in_name x m -> name_not_free_in_name x n))).
  - (* se_refl *) intros P x. split; intro; assumption.
  - (* se_sym *) intros P Q _ IH x. destruct (IH x) as [Hf Hb]. split; assumption.
  - (* se_trans *)
    intros P Q R _ IH1 _ IH2 x.
    destruct (IH1 x) as [Hf1 Hb1]. destruct (IH2 x) as [Hf2 Hb2].
    split.
    + intro HP. apply Hf2, Hf1, HP.
    + intro HR. apply Hb1, Hb2, HR.
  - (* se_par_comm *)
    intros P Q x. split.
    + intros [HP HQ]. split; assumption.
    + intros [HQ HP]. split; assumption.
  - (* se_par_assoc *)
    intros P Q R x. split.
    + intros [[HP HQ] HR]. split; [exact HP | split; assumption].
    + intros [HP [HQ HR]]. split; [split; assumption | exact HR].
  - (* se_par_nil *)
    intros P x. split.
    + intros [HP _]. exact HP.
    + intro HP. split; [exact HP | exact I].
  - (* se_par_cong *)
    intros P P' Q Q' _ IHP _ IHQ x.
    destruct (IHP x) as [HfP HbP]. destruct (IHQ x) as [HfQ HbQ].
    split.
    + intros [HaP HaQ]. split; [apply HfP | apply HfQ]; assumption.
    + intros [HaP' HaQ']. split; [apply HbP | apply HbQ]; assumption.
  - (* se_input_cong *)
    intros n m P P' Hnm IHn _ IHP x.
    destruct (IHn x) as [Hfn Hbn]. destruct (IHP x) as [HfP HbP].
    split.
    + intros [Hne [Hnn HPa]]. simpl. split; [| split].
      * intro Hxm.
        apply Hne. eapply se_name_trans; [exact Hxm | apply se_name_sym; exact Hnm].
      * apply Hfn. exact Hnn.
      * apply HfP. exact HPa.
    + intros [Hne [Hnn HPa]]. simpl. split; [| split].
      * intro Hxn.
        apply Hne. eapply se_name_trans; [exact Hxn | exact Hnm].
      * apply Hbn. exact Hnn.
      * apply HbP. exact HPa.
  - (* se_output_cong — v3 predicate adds [~ (x ≡N Quote P)] clause *)
    intros n m P P' Hnm IHn HPP' IHP x.
    destruct (IHn x) as [Hfn Hbn]. destruct (IHP x) as [HfP HbP].
    split.
    + intros [Hne [Hnn [Hqne HPa]]]. simpl. split; [| split; [| split]].
      * intro Hxm.
        apply Hne. eapply se_name_trans; [exact Hxm | apply se_name_sym; exact Hnm].
      * apply Hfn. exact Hnn.
      * (* ~ (x ≡N Quote P') given Hqne : ~ (x ≡N Quote P) and HPP' : P ≡ P' *)
        intro HxqP'. apply Hqne.
        eapply se_name_trans; [exact HxqP' |].
        apply se_name_quote. apply se_sym. exact HPP'.
      * apply HfP. exact HPa.
    + intros [Hne [Hnn [Hqne HPa]]]. simpl. split; [| split; [| split]].
      * intro Hxn.
        apply Hne. eapply se_name_trans; [exact Hxn | exact Hnm].
      * apply Hbn. exact Hnn.
      * intro HxqP. apply Hqne.
        eapply se_name_trans; [exact HxqP |].
        apply se_name_quote. exact HPP'.
      * apply HbP. exact HPa.
  - (* se_deref_cong *)
    intros n m Hnm IHn x.
    destruct (IHn x) as [Hfn Hbn]. split; simpl; assumption.
  - (* se_replicate_cong *)
    intros P P' _ IHP x.
    destruct (IHP x) as [HfP HbP]. split; simpl; assumption.
  - (* se_name_quote *)
    intros P P' _ IHP x.
    destruct (IHP x) as [HfP HbP]. split; simpl; assumption.
  - (* se_name_var_refl *)
    intros k x. split; intro; exact I.
Qed.

Lemma name_not_free_in_proc_se : forall x P Q,
  P ≡ Q -> name_not_free_in_proc x P -> name_not_free_in_proc x Q.
Proof.
  intros x P Q Heq HP.
  destruct name_not_free_in_se_bidir as [HPr _].
  destruct (HPr P Q Heq x) as [Hf _]. exact (Hf HP).
Qed.

Lemma name_not_free_in_name_se : forall x n m,
  n ≡N m -> name_not_free_in_name x n -> name_not_free_in_name x m.
Proof.
  intros x n m Heq HN.
  destruct name_not_free_in_se_bidir as [_ HNr].
  destruct (HNr n m Heq x) as [Hf _]. exact (Hf HN).
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: par_many — iterated parallel composition
   ═══════════════════════════════════════════════════════════════════════════ *)

Fixpoint par_many (Ps : list proc) : proc :=
  match Ps with
  | []      => PNil
  | P :: Qs => PPar P (par_many Qs)
  end.

Lemma par_many_app : forall L1 L2,
  par_many (L1 ++ L2) ≡ PPar (par_many L1) (par_many L2).
Proof.
  induction L1 as [|h t IH]; intros L2; simpl.
  - apply se_sym, se_nil_par.
  - eapply se_trans. { apply se_par_cong_r. apply IH. }
    apply se_sym, se_par_assoc.
Qed.

Lemma par_many_perm : forall L1 L2,
  Permutation L1 L2 -> par_many L1 ≡ par_many L2.
Proof.
  intros L1 L2 H. induction H; simpl.
  - apply se_refl.
  - apply se_par_cong_r. exact IHPermutation.
  - (* perm_swap: y::x::l vs x::y::l — swap neighbours *)
    eapply se_trans. { apply se_sym. apply se_par_assoc. }
    eapply se_trans. { apply se_par_cong_l. apply se_par_comm. }
    apply se_par_assoc.
  - eapply se_trans; eassumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 8: Closedness transport under struct equiv and reduction
   ═══════════════════════════════════════════════════════════════════════════ *)

(* closed_proc_at and closed_name_at are preserved by structural
   equivalence.  Bidirectional via se_combined_mut. *)

Lemma closed_at_se_bidir :
  (forall P Q (_ : P ≡ Q), forall k,
     (closed_proc_at k P -> closed_proc_at k Q) /\
     (closed_proc_at k Q -> closed_proc_at k P)) /\
  (forall n m (_ : n ≡N m), forall k,
     (closed_name_at k n -> closed_name_at k m) /\
     (closed_name_at k m -> closed_name_at k n)).
Proof.
  apply (se_combined_mut
    (fun P Q _ => forall k,
       (closed_proc_at k P -> closed_proc_at k Q) /\
       (closed_proc_at k Q -> closed_proc_at k P))
    (fun n m _ => forall k,
       (closed_name_at k n -> closed_name_at k m) /\
       (closed_name_at k m -> closed_name_at k n))).
  - (* se_refl *) intros P k. split; intro; assumption.
  - (* se_sym *) intros P Q _ IH k. destruct (IH k) as [Hf Hb]. split; assumption.
  - (* se_trans *)
    intros P Q R _ IH1 _ IH2 k.
    destruct (IH1 k) as [Hf1 Hb1]. destruct (IH2 k) as [Hf2 Hb2].
    split; [intro H; apply Hf2, Hf1, H | intro H; apply Hb1, Hb2, H].
  - (* se_par_comm *)
    intros P Q k. split.
    + intros [HP HQ]. split; assumption.
    + intros [HQ HP]. split; assumption.
  - (* se_par_assoc *)
    intros P Q R k. split.
    + intros [[HP HQ] HR]. split; [exact HP | split; assumption].
    + intros [HP [HQ HR]]. split; [split; assumption | exact HR].
  - (* se_par_nil *)
    intros P k. split.
    + intros [HP _]. exact HP.
    + intro HP. split; [exact HP | exact I].
  - (* se_par_cong *)
    intros P P' Q Q' _ IHP _ IHQ k.
    destruct (IHP k) as [HfP HbP]. destruct (IHQ k) as [HfQ HbQ].
    split.
    + intros [HaP HaQ]. split; [apply HfP | apply HfQ]; assumption.
    + intros [HaP' HaQ']. split; [apply HbP | apply HbQ]; assumption.
  - (* se_input_cong *)
    intros n m P P' _ IHn _ IHP k.
    destruct (IHn k) as [Hfn Hbn]. destruct (IHP (S k)) as [HfP HbP].
    split.
    + intros [Hxn HPc]. split; [apply Hfn | apply HfP]; assumption.
    + intros [Hxm HPc]. split; [apply Hbn | apply HbP]; assumption.
  - (* se_output_cong *)
    intros n m P P' _ IHn _ IHP k.
    destruct (IHn k) as [Hfn Hbn]. destruct (IHP k) as [HfP HbP].
    split.
    + intros [Hxn HPc]. split; [apply Hfn | apply HfP]; assumption.
    + intros [Hxm HPc]. split; [apply Hbn | apply HbP]; assumption.
  - (* se_deref_cong *)
    intros n m _ IHn k.
    destruct (IHn k) as [Hfn Hbn]. split; simpl; assumption.
  - (* se_replicate_cong *)
    intros P P' _ IHP k.
    destruct (IHP k) as [HfP HbP]. split; simpl; assumption.
  - (* se_name_quote *)
    intros P P' _ IHP k.
    destruct (IHP k) as [HfP HbP]. split; simpl; assumption.
  - (* se_name_var_refl *)
    intros j k. split; intro; exact H.
Qed.

Lemma closed_proc_at_se : forall P Q k,
  P ≡ Q -> closed_proc_at k P -> closed_proc_at k Q.
Proof.
  intros P Q k Heq HP.
  destruct closed_at_se_bidir as [HPr _].
  destruct (HPr P Q Heq k) as [Hf _]. exact (Hf HP).
Qed.

Lemma closed_name_at_se : forall n m k,
  n ≡N m -> closed_name_at k n -> closed_name_at k m.
Proof.
  intros n m k Heq HN.
  destruct closed_at_se_bidir as [_ HNr].
  destruct (HNr n m Heq k) as [Hf _]. exact (Hf HN).
Qed.

(* closed_proc_at is preserved by substitution, provided the substituent
   is closed at level 0. *)

Lemma closed_proc_at_subst : forall P k N,
  closed_proc_at (S k) P -> closed_name_at 0 N ->
  closed_proc_at k (subst_proc P k N).
Proof.
  intro P0.
  apply (proc_ind_mut
    (fun P => forall k N,
       closed_proc_at (S k) P -> closed_name_at 0 N ->
       closed_proc_at k (subst_proc P k N))
    (fun x => forall k N,
       closed_name_at (S k) x -> closed_name_at 0 N ->
       closed_name_at k (subst_name x k N))); clear P0.
  - (* PNil *) intros k N _ _. simpl. exact I.
  - (* PInput *)
    intros x IHx Pb IHPb k N [Hx HPc] HN. simpl.
    split; [apply IHx; assumption |].
    apply IHPb; [exact HPc |].
    (* closed_name N at 0 means lift N is identity (closed_name_lift_zero). *)
    rewrite closed_name_lift_zero by exact HN. exact HN.
  - (* POutput *)
    intros x IHx Q IHQ k N [Hx HQc] HN. simpl.
    split; [apply IHx | apply IHQ]; assumption.
  - (* PPar *)
    intros P1 IH1 P2 IH2 k N [H1 H2] HN. simpl.
    split; [apply IH1 | apply IH2]; assumption.
  - (* PDeref *)
    intros x IHx k N Hxc HN. simpl.
    destruct x as [Pi | j].
    + (* Quote Pi: subst gives PDeref (Quote (subst_proc Pi k N)) *)
      simpl. simpl in Hxc. apply IHx; assumption.
    + (* NVar j: three cases on compare j k *)
      simpl in Hxc. destruct (PeanoNat.Nat.compare_spec j k) as [Heq | Hlt | Hgt].
      * (* j = k: result is match N with Quote Q => Q | NVar _ => PDeref N *)
        destruct N as [NP | j']; simpl.
        -- (* Quote NP — need closed_proc_at k NP.  HN : closed_proc_at 0 NP *)
           apply (closed_proc_at_mono _ 0 k); [lia | exact HN].
        -- (* NVar j' — need closed_name_at k (NVar j') = j' < k.
              HN : j' < 0 is False. *)
           simpl in HN. lia.
      * (* j < k *) simpl. exact Hlt.
      * (* j > k *) simpl. lia.
  - (* PReplicate *)
    intros Pb IHPb k N HPc HN. simpl. apply IHPb; assumption.
  - (* Quote P *)
    intros Pb IHPb k N HP HN. simpl. apply IHPb; assumption.
  - (* NVar j (name level) *)
    intros j k N HN HN0. simpl.
    simpl in HN.
    destruct (PeanoNat.Nat.compare_spec j k) as [Heq | Hlt | Hgt].
    + apply (closed_name_at_mono _ 0 k); [lia | exact HN0].
    + exact Hlt.
    + lia.
Qed.

(* Closedness is preserved under rho_step. *)

Lemma closed_proc_step : forall P P',
  rho_step P P' -> closed_proc P -> closed_proc P'.
Proof.
  intros P P' Hstep. induction Hstep; intros Hcl; unfold closed_proc in *.
  - (* rs_comm: closed (PPar (PInput x P) (POutput x Q)) ->
                closed (subst_proc P 0 (Quote Q)) *)
    destruct Hcl as [[Hx HPb] [_ HQc]]. simpl.
    apply closed_proc_at_subst; [exact HPb |].
    simpl. exact HQc.
  - (* rs_par_l *)
    destruct Hcl as [H1 H2]. simpl. split; [apply IHHstep | exact H2]; assumption.
  - (* rs_par_r *)
    destruct Hcl as [H1 H2]. simpl. split; [exact H1 | apply IHHstep]; assumption.
  - (* rs_struct: Hcl : closed P, H : P ≡ P', Hstep : P' ⇝ Q', H0 : Q' ≡ Q *)
    apply (closed_proc_at_se Q' Q 0); [exact H0 |].
    apply IHHstep.
    apply (closed_proc_at_se P P' 0); [exact H | exact Hcl].
  - (* rs_replicate *)
    simpl in Hcl. simpl. split; exact Hcl.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9: Freshness preservation under lift, substitution, and steps
   ═══════════════════════════════════════════════════════════════════════════ *)

(* A closed lifted name is structurally equivalent to its source, given
   the outer name is closed.  This uses the key fact that lifting only
   shifts NVar indices upward — and closed [x] rules out NVar collisions. *)

Lemma lift_closed_inv_combined :
  (forall P d c k,
     closed_proc_at k (lift_proc d c P) -> closed_proc_at k P) /\
  (forall x d c k,
     closed_name_at k (lift_name d c x) -> closed_name_at k x).
Proof.
  apply (proc_name_mutind
    (fun P => forall d c k,
       closed_proc_at k (lift_proc d c P) -> closed_proc_at k P)
    (fun x => forall d c k,
       closed_name_at k (lift_name d c x) -> closed_name_at k x)).
  - (* PNil *) intros; exact I.
  - (* PInput *)
    intros y IHy B IHB d c k [Hy HB]. simpl. split.
    + eapply IHy; eauto.
    + eapply IHB; eauto.
  - (* POutput *)
    intros y IHy Q IHQ d c k [Hy HQ]. simpl. split;
      [eapply IHy | eapply IHQ]; eauto.
  - (* PPar *)
    intros P1 IH1 P2 IH2 d c k [H1 H2]. simpl. split;
      [eapply IH1 | eapply IH2]; eauto.
  - (* PDeref *)
    intros y IHy d c k Hy. simpl in *. eapply IHy; eauto.
  - (* PReplicate *)
    intros P IHP d c k HP. simpl in *. eapply IHP; eauto.
  - (* Quote *)
    intros P IHP d c k HP. simpl in *. eapply IHP; eauto.
  - (* NVar *)
    intros j d c k Hj. simpl in *. destruct (c <=? j) eqn:Hcj.
    + simpl in Hj. lia.
    + simpl in Hj. exact Hj.
Qed.

Lemma closed_proc_at_lift_inv : forall P d c k,
  closed_proc_at k (lift_proc d c P) -> closed_proc_at k P.
Proof. apply lift_closed_inv_combined. Qed.

Lemma closed_name_at_lift_inv : forall x d c k,
  closed_name_at k (lift_name d c x) -> closed_name_at k x.
Proof. apply lift_closed_inv_combined. Qed.

(* Contrapositive of structural disequivalence under lift, for closed x. *)

Lemma se_name_lift_inv_closed : forall x y d c,
  closed_name x -> x ≡N lift_name d c y -> x ≡N y.
Proof.
  intros x y d c Hclx Heq.
  destruct y as [Py | k]; simpl in Heq.
  - (* y = Quote Py ; Heq : x ≡N Quote (lift_proc d c Py) *)
    inversion Heq as [Px P0 Hproc Hx0 Hx1 | k0 ]; subst.
    (* x = Quote Px, Hproc : Px ≡ lift_proc d c Py *)
    unfold closed_name in Hclx. simpl in Hclx.
    assert (Hc : closed_proc_at 0 (lift_proc d c Py)).
    { apply (closed_proc_at_se Px (lift_proc d c Py) 0); [exact Hproc | exact Hclx]. }
    apply closed_proc_at_lift_inv in Hc.
    rewrite (closed_proc_lift_zero _ d c Hc) in Hproc.
    apply se_name_quote. exact Hproc.
  - (* y = NVar k ; Heq : x ≡N (if c <=? k then NVar (k+d) else NVar k) *)
    destruct (c <=? k) eqn:Hck.
    + inversion Heq; subst. unfold closed_name in Hclx. simpl in Hclx. lia.
    + inversion Heq; subst. unfold closed_name in Hclx. simpl in Hclx. lia.
Qed.

Lemma ne_lift_name_preserves_closed : forall x y d c,
  closed_name x -> ~ (x ≡N y) -> ~ (x ≡N lift_name d c y).
Proof.
  intros x y d c Hcl Hne HL.
  apply Hne. eapply se_name_lift_inv_closed; eauto.
Qed.

(* Freshness preserved by lift, given closed x. *)

Lemma name_not_free_in_lift_combined : forall x,
  closed_name x ->
  (forall P d c,
     name_not_free_in_proc x P ->
     name_not_free_in_proc x (lift_proc d c P)) /\
  (forall y d c,
     name_not_free_in_name x y ->
     name_not_free_in_name x (lift_name d c y)).
Proof.
  intros x Hcl.
  apply (proc_name_mutind
    (fun P => forall d c,
       name_not_free_in_proc x P ->
       name_not_free_in_proc x (lift_proc d c P))
    (fun y => forall d c,
       name_not_free_in_name x y ->
       name_not_free_in_name x (lift_name d c y))).
  - (* PNil *) intros; exact I.
  - (* PInput *)
    intros y IHy B IHB d c [Hne [Hnn HB]]. simpl. split; [|split].
    + apply ne_lift_name_preserves_closed; assumption.
    + apply IHy; exact Hnn.
    + apply IHB; exact HB.
  - (* POutput — v3 predicate has extra [~ (x ≡N Quote Q)] clause *)
    intros y IHy Q IHQ d c [Hne [Hnn [Hqne HQ]]]. simpl.
    split; [|split; [|split]].
    + apply ne_lift_name_preserves_closed; assumption.
    + apply IHy; exact Hnn.
    + (* ~ (x ≡N Quote (lift_proc d c Q)) —
         lift_name d c (Quote Q) = Quote (lift_proc d c Q), so use
         ne_lift_name_preserves_closed at the name level. *)
      change (~ (x ≡N Quote (lift_proc d c Q)))
        with (~ (x ≡N lift_name d c (Quote Q))).
      apply ne_lift_name_preserves_closed; assumption.
    + apply IHQ; exact HQ.
  - (* PPar *)
    intros P1 IH1 P2 IH2 d c [H1 H2]. simpl. split; auto.
  - (* PDeref *)
    intros y IHy d c Hy. simpl. apply IHy; exact Hy.
  - (* PReplicate *)
    intros P IHP d c HP. simpl. apply IHP; exact HP.
  - (* Quote *)
    intros P IHP d c HP. simpl. apply IHP; exact HP.
  - (* NVar *)
    intros k d c _. simpl. destruct (c <=? k); exact I.
Qed.

Lemma name_not_free_in_proc_lift : forall x P d c,
  closed_name x ->
  name_not_free_in_proc x P ->
  name_not_free_in_proc x (lift_proc d c P).
Proof.
  intros x P d c Hcl HP.
  destruct (name_not_free_in_lift_combined x Hcl) as [Hlift _].
  apply Hlift. exact HP.
Qed.

Lemma name_not_free_in_name_lift : forall x y d c,
  closed_name x ->
  name_not_free_in_name x y ->
  name_not_free_in_name x (lift_name d c y).
Proof.
  intros x y d c Hcl HN.
  destruct (name_not_free_in_lift_combined x Hcl) as [_ Hlift].
  apply Hlift. exact HN.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   Freshness preservation under substitution, given the target is closed.
   Key insight: under closed_proc_at k P, every [Quote Q'] subterm has
   closed_proc_at k Q', so subst_proc Q' k N = Q' by closed_proc_subst.
   This sidesteps the counter-example where non-closed embedded quotes
   could suffer semantic collapse to match x.                              *)

Lemma name_not_free_in_proc_subst_closed : forall x P k N,
  closed_name x ->
  closed_proc_at k P ->
  closed_name_at 0 N ->
  name_not_free_in_proc x P ->
  name_not_free_in_name x N ->
  name_not_free_in_proc x (subst_proc P k N).
Proof.
  intros x P k N Hxcl HPcl HNcl HPfr HNfr.
  rewrite closed_proc_subst by exact HPcl.
  exact HPfr.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9b: P1 (quoted_channels_closed) preservation lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

(* SE preservation: bidirectional via se_combined_mut. *)

Lemma quoted_channels_closed_se_bidir :
  (forall P Q (_ : P ≡ Q),
     (quoted_channels_closed P -> quoted_channels_closed Q) /\
     (quoted_channels_closed Q -> quoted_channels_closed P)) /\
  (forall n m (_ : n ≡N m),
     (quoted_name_closed n -> quoted_name_closed m) /\
     (quoted_name_closed m -> quoted_name_closed n)).
Proof.
  apply (se_combined_mut
    (fun P Q _ =>
       (quoted_channels_closed P -> quoted_channels_closed Q) /\
       (quoted_channels_closed Q -> quoted_channels_closed P))
    (fun n m _ =>
       (quoted_name_closed n -> quoted_name_closed m) /\
       (quoted_name_closed m -> quoted_name_closed n))).
  - intros P. split; intro; assumption.
  - intros P Q _ [Hf Hb]. split; assumption.
  - intros P Q R _ [Hf1 Hb1] _ [Hf2 Hb2].
    split; [intro H; apply Hf2, Hf1, H | intro H; apply Hb1, Hb2, H].
  - intros P Q. split; intros [H1 H2]; split; assumption.
  - intros P Q R. split.
    + intros [[H1 H2] H3]. split; [exact H1 | split; assumption].
    + intros [H1 [H2 H3]]. split; [split; assumption | exact H3].
  - intros P. split.
    + intros [HP _]. exact HP.
    + intro HP. split; [exact HP | exact I].
  - intros P P' Q Q' _ [HfP HbP] _ [HfQ HbQ]. split.
    + intros [H1 H2]. split; [apply HfP | apply HfQ]; assumption.
    + intros [H1 H2]. split; [apply HbP | apply HbQ]; assumption.
  - intros n m P P' _ [Hfn Hbn] _ [HfP HbP]. split.
    + intros [Hy HB]. split; [apply Hfn | apply HfP]; assumption.
    + intros [Hy HB]. split; [apply Hbn | apply HbP]; assumption.
  - intros n m P P' _ [Hfn Hbn] _ [HfP HbP]. split.
    + intros [Hy HQ]. split; [apply Hfn | apply HfP]; assumption.
    + intros [Hy HQ]. split; [apply Hbn | apply HbP]; assumption.
  - intros n m _ [Hfn Hbn]. split; simpl; assumption.
  - intros P P' _ [HfP HbP]. split; simpl; assumption.
  - (* se_name_quote: the Quote case of quoted_name_closed requires
       closed_proc P and quoted_channels_closed P; both transport via
       se-preservation. *)
    intros P P' Hpp [HfP HbP]. split.
    + intros [Hcl Hqc]. simpl. split.
      * apply (closed_proc_at_se P P' 0); assumption.
      * apply HfP. exact Hqc.
    + intros [Hcl Hqc]. simpl. split.
      * apply (closed_proc_at_se P' P 0); [apply se_sym; exact Hpp | exact Hcl].
      * apply HbP. exact Hqc.
  - intros k. split; intro; exact I.
Qed.

Lemma quoted_channels_closed_se : forall P Q,
  P ≡ Q -> quoted_channels_closed P -> quoted_channels_closed Q.
Proof.
  intros P Q Heq HP.
  destruct quoted_channels_closed_se_bidir as [HPr _].
  destruct (HPr P Q Heq) as [Hf _]. exact (Hf HP).
Qed.

Lemma quoted_name_closed_se : forall n m,
  n ≡N m -> quoted_name_closed n -> quoted_name_closed m.
Proof.
  intros n m Heq HN.
  destruct quoted_channels_closed_se_bidir as [_ HNr].
  destruct (HNr n m Heq) as [Hf _]. exact (Hf HN).
Qed.

(* Lift preservation: since quoted_name_closed requires closedness,
   and closed-proc lift is identity, lift preserves the predicate. *)

Lemma quoted_channels_closed_lift_combined :
  (forall P d c, quoted_channels_closed P ->
                 quoted_channels_closed (lift_proc d c P)) /\
  (forall y d c, quoted_name_closed y ->
                 quoted_name_closed (lift_name d c y)).
Proof.
  apply (proc_name_mutind
    (fun P => forall d c, quoted_channels_closed P ->
                          quoted_channels_closed (lift_proc d c P))
    (fun y => forall d c, quoted_name_closed y ->
                          quoted_name_closed (lift_name d c y))).
  - intros; exact I.
  - intros y IHy B IHB d c [Hy HB]. simpl. split; auto.
  - intros y IHy Q IHQ d c [Hy HQ]. simpl. split; auto.
  - intros P1 IH1 P2 IH2 d c [H1 H2]. simpl. split; auto.
  - intros y IHy d c Hy. simpl. apply IHy; exact Hy.
  - intros P IHP d c HP. simpl. apply IHP; exact HP.
  - (* Quote *)
    intros P IHP d c [Hcl Hqc]. simpl. split.
    + rewrite (closed_proc_lift_zero P d c Hcl). exact Hcl.
    + rewrite (closed_proc_lift_zero P d c Hcl). exact Hqc.
  - (* NVar *) intros k d c _. simpl. destruct (c <=? k); exact I.
Qed.

Lemma quoted_channels_closed_lift : forall P d c,
  quoted_channels_closed P -> quoted_channels_closed (lift_proc d c P).
Proof. apply quoted_channels_closed_lift_combined. Qed.

Lemma quoted_name_closed_lift : forall y d c,
  quoted_name_closed y -> quoted_name_closed (lift_name d c y).
Proof. apply quoted_channels_closed_lift_combined. Qed.

(* Subst preservation: analogous to lift — substituting into closed
   subterms is identity, so the predicate is preserved when the
   substituent itself satisfies the predicate. *)

Lemma quoted_channels_closed_subst_combined :
  (forall P k N, quoted_channels_closed P -> quoted_name_closed N ->
                 quoted_channels_closed (subst_proc P k N)) /\
  (forall y k N, quoted_name_closed y -> quoted_name_closed N ->
                 quoted_name_closed (subst_name y k N)).
Proof.
  apply (proc_name_mutind
    (fun P => forall k N, quoted_channels_closed P -> quoted_name_closed N ->
                          quoted_channels_closed (subst_proc P k N))
    (fun y => forall k N, quoted_name_closed y -> quoted_name_closed N ->
                          quoted_name_closed (subst_name y k N))).
  - intros; exact I.
  - (* PInput *)
    intros y IHy B IHB k N [Hy HB] HN. simpl. split.
    + apply IHy; assumption.
    + apply IHB; [exact HB |].
      destruct quoted_channels_closed_lift_combined as [_ Hl].
      apply Hl; exact HN.
  - (* POutput *)
    intros y IHy Q IHQ k N [Hy HQ] HN. simpl. split; auto.
  - (* PPar *)
    intros P1 IH1 P2 IH2 k N [H1 H2] HN. simpl. split; auto.
  - (* PDeref — three cases on name shape / compare *)
    intros y IHy k N Hy HN. destruct y as [Pi | j].
    + simpl. simpl in Hy. apply IHy; assumption.
    + simpl. simpl in Hy.
      destruct (PeanoNat.Nat.compare_spec j k) as [Heq | Hlt | Hgt].
      * destruct N as [NP | j']; simpl.
        -- destruct HN as [_ Hqc]. exact Hqc.
        -- exact I.
      * simpl. exact I.
      * simpl. exact I.
  - (* PReplicate *) intros P IHP k N HP HN. simpl. apply IHP; assumption.
  - (* Quote *)
    intros P IHP k N [Hcl Hqc] HN. simpl. split.
    + rewrite closed_proc_subst.
      * exact Hcl.
      * apply (closed_proc_at_mono _ 0 k); [lia | exact Hcl].
    + rewrite closed_proc_subst.
      * exact Hqc.
      * apply (closed_proc_at_mono _ 0 k); [lia | exact Hcl].
  - (* NVar *)
    intros j k N _ HN. simpl.
    destruct (PeanoNat.Nat.compare_spec j k) as [|_|_]; auto.
    + exact I.
    + exact I.
Qed.

Lemma quoted_channels_closed_subst : forall P k N,
  quoted_channels_closed P -> quoted_name_closed N ->
  quoted_channels_closed (subst_proc P k N).
Proof. apply quoted_channels_closed_subst_combined. Qed.

Lemma quoted_name_closed_subst : forall y k N,
  quoted_name_closed y -> quoted_name_closed N ->
  quoted_name_closed (subst_name y k N).
Proof. apply quoted_channels_closed_subst_combined. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 9c: Freshness preservation under substitution (v3 + P1)
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Subst-level disequivalence preservation. Given closed x, closed
   substituent N that is itself ≢N x and v3-compliant, show:
   if x ≢N y, then x ≢N subst_name y k N. *)

Lemma ne_subst_name_preserves_closed :
  forall x, closed_name x -> forall N,
  closed_name_at 0 N ->
  quoted_name_closed N ->
  ~ (x ≡N N) ->
  forall y k,
    closed_name_at (S k) y ->
    quoted_name_closed y ->
    ~ (x ≡N y) ->
    ~ (x ≡N subst_name y k N).
Proof.
  intros x Hxcl N HNcl HNqc HNne y k Hycl Hyqc Hyne.
  destruct y as [Q | j].
  - (* y = Quote Q: subst_name descends into Q.
       quoted_name_closed (Quote Q) gives closed_proc Q, so subst is identity. *)
    simpl. simpl in Hycl, Hyqc.
    destruct Hyqc as [HQcl HQqc].
    rewrite (closed_proc_subst Q k N) by
      (apply (closed_proc_at_mono _ 0 k); [lia | exact HQcl]).
    exact Hyne.
  - (* y = NVar j *)
    simpl. simpl in Hycl.
    destruct (PeanoNat.Nat.compare_spec j k) as [Heq | Hlt | Hgt]; simpl.
    + exact HNne.
    + intro Hx. unfold closed_name in Hxcl.
      inversion Hx; subst. simpl in Hxcl. lia.
    + intro Hx. unfold closed_name in Hxcl.
      inversion Hx; subst. simpl in Hxcl. lia.
Qed.

(* Combined step preservation: closedness AND P1. Both properties must
   be carried together because rs_comm's POutput payload Q needs to be
   closed (for subst arithmetic) AND P1-compliant (to preserve P1
   through the subst).  These are inseparable and proven jointly. *)

Lemma closed_and_P1_step : forall P P',
  rho_step P P' ->
  closed_proc P ->
  quoted_channels_closed P ->
  closed_proc P' /\ quoted_channels_closed P'.
Proof.
  intros P P' Hstep. induction Hstep; intros HPcl HPqc.
  - (* rs_comm *)
    destruct HPcl as [[Hxc HBcl] [_ HQcl]].
    destruct HPqc as [[Hxq HBq] [_ HQq]].
    split.
    + apply closed_proc_at_subst; [exact HBcl | simpl; exact HQcl].
    + apply quoted_channels_closed_subst; [exact HBq | simpl; split; assumption].
  - (* rs_par_l *)
    destruct HPcl as [H1c H2c]. destruct HPqc as [H1q H2q].
    destruct (IHHstep H1c H1q) as [Hc' Hq']. split; simpl; split; auto.
  - (* rs_par_r *)
    destruct HPcl as [H1c H2c]. destruct HPqc as [H1q H2q].
    destruct (IHHstep H2c H2q) as [Hc' Hq']. split; simpl; split; auto.
  - (* rs_struct *)
    assert (HPcl' : closed_proc P').
    { apply (closed_proc_at_se P P' 0); assumption. }
    assert (HPqc' : quoted_channels_closed P').
    { apply (quoted_channels_closed_se P P'); assumption. }
    destruct (IHHstep HPcl' HPqc') as [HQ'cl HQ'qc].
    split.
    + apply (closed_proc_at_se Q' Q 0); assumption.
    + apply (quoted_channels_closed_se Q' Q); assumption.
  - (* rs_replicate *)
    simpl in HPcl, HPqc. split; simpl; split; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 10: Strong bisim headline theorem — retired (see Section 12)
   ═══════════════════════════════════════════════════════════════════════════

   Original goal: [bisim (PReplicate P) (bang_encoding x P)] for closed
   [P] and freshness [name_not_free_in x P].

   **Retired 2026-04-20.** The strong-bisim claim is not a faithful
   statement in rho calculus: [bang_encoding x P] has a top-level
   output barb on the coordination channel [x] (from the
   [POutput x (PPar (D_encoding x) P)] factor) that [PReplicate P] lacks
   under the freshness hypothesis. No barb-sensitive equivalence —
   strong or weak — can hold without either (a) a [nu]/[PNew]
   name-restriction binder, or (b) an observation-restricted
   equivalence. Rho calculus was designed without restriction —
   Meredith-Radestock 2005 establishes reflection as the substitute for
   [nu] — so adding [PNew] would extend rho calculus with a
   pi-calculus construct its author deliberately omitted. Option (b)
   is the theoretically appropriate formulation for this calculus:
   hiding lives at the equivalence-relation level, not the syntax
   level.

   The headline result is now [preplicate_bang_encoding_body_barbs_sound]
   in Section 12: every weak barb of the body [P] lifts to a weak barb
   of both wrappers. The development deliberately avoids a stronger
   projection from all wrapper barbs back to one copy of [P], since
   replication can expose behavior that depends on several copies.

   ──────────────────────────────────────────────────────────────────
   Spec-to-Code
   ──────────────────────────────────────────────────────────────────
   Paper claim                       │ Rocq theorem
   ──────────────────────────────────┼─────────────────────────────────────
   §5 Remark: "replication via       │ bang_encoding_unfolds (proven)
   self-referencing through          │
   reflection applies directly"      │
   Meredith-Radestock §3 encoding    │ bang_encoding, D_encoding (defined)
   PReplicate and !P(x) expose       │ preplicate_bang_encoding_body_barbs_sound
   every body weak barb              │   (proven §12)
   ──────────────────────────────────────────────────────────────────      *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 11: Auxiliary infrastructure for par_many over repeats
   ═══════════════════════════════════════════════════════════════════════════

   This section collects trivial counting helpers for [par_many] over
   [List.repeat], plus ≡-inversion lemmas for [PReplicate]. These are
   independent of the core cost-accounted stack and are not used by any
   theorem in Sections 1-10; they are kept here as reusable infrastructure.

   Layout:
     * L11.A: par_many-over-repeat counting helpers.
     * L11.B: ≡-inversion for PReplicate.
   *)

(* ─── L11.A: Trivial helpers ─── *)

Lemma Forall_repeat_self :
  forall {A} (a : A) (n : nat),
  Forall (fun x => x = a) (List.repeat a n).
Proof.
  intros A a. induction n; simpl; constructor; auto.
Qed.

Lemma count_inputs_par_many_repeat : forall P n,
  count_inputs (par_many (List.repeat P n)) = n * count_inputs P.
Proof.
  intros P. induction n; simpl; [reflexivity | rewrite IHn; reflexivity].
Qed.

Lemma count_outputs_par_many_repeat : forall P n,
  count_outputs (par_many (List.repeat P n)) = n * count_outputs P.
Proof.
  intros P. induction n; simpl; [reflexivity | rewrite IHn; reflexivity].
Qed.

Lemma count_replicates_par_many_repeat : forall P n,
  count_replicates (par_many (List.repeat P n)) = n * count_replicates P.
Proof.
  intros P. induction n; simpl; [reflexivity | rewrite IHn; reflexivity].
Qed.

Lemma head_count_par_many_repeat : forall P n,
  head_count (par_many (List.repeat P n)) = n * head_count P.
Proof.
  intros P. induction n; simpl; [reflexivity | rewrite IHn; reflexivity].
Qed.

Lemma name_not_free_in_par_many_repeat : forall x P n,
  name_not_free_in_proc x P ->
  name_not_free_in_proc x (par_many (List.repeat P n)).
Proof.
  intros x P. induction n; simpl; intros HP; [exact I | split; [exact HP | apply IHn; exact HP]].
Qed.

(* ─── L11.B: PReplicate inversion (≡ and step) ─── *)

(* [se_PReplicate_inv_both] is the bidirectional ≡-inversion lemma for
   PReplicate, modelled on [se_PDeref_inv_both] at StructEquivHeads.v:342. *)
Lemma se_PReplicate_inv_both :
  forall P R, P ≡ R ->
    (forall B, P = PReplicate B ->
       exists B', R ≡ PReplicate B' /\ B ≡ B') /\
    (forall B, R = PReplicate B ->
       exists B', P ≡ PReplicate B' /\ B' ≡ B).
Proof.
  intros P R Heq. induction Heq.
  - (* se_refl P *)
    split; intros B0 Heqp; subst; exists B0; split; apply se_refl.
  - (* se_sym *)
    destruct IHHeq as [Hfwd Hbwd].
    split; intros B0 Heqp.
    + destruct (Hbwd B0 Heqp) as [B' [HeqP HeqB]].
      exists B'. split; [exact HeqP | apply se_sym; exact HeqB].
    + destruct (Hfwd B0 Heqp) as [B' [HeqQ HeqB]].
      exists B'. split; [exact HeqQ | apply se_sym; exact HeqB].
  - (* se_trans P Q R *)
    destruct IHHeq1 as [Hfwd1 Hbwd1].
    destruct IHHeq2 as [Hfwd2 Hbwd2].
    split; intros B0 Heqp.
    + destruct (Hfwd1 B0 Heqp) as [B_q [HeqQ HeqB1]].
      (* HeqQ : Q ≡ PReplicate B_q *)
      (* Want: exists B', R ≡ PReplicate B' /\ B0 ≡ B'. *)
      (* From Heq2 : Q ≡ R, and HeqQ : Q ≡ PReplicate B_q,
         chain to get R ≡ PReplicate B_q. *)
      exists B_q. split.
      * eapply se_trans; [apply se_sym; exact Heq2 | exact HeqQ].
      * exact HeqB1.
    + destruct (Hbwd2 B0 Heqp) as [B_q [HeqQ HeqB2]].
      (* HeqQ : Q ≡ PReplicate B_q *)
      exists B_q. split.
      * eapply se_trans; [exact Heq1 | exact HeqQ].
      * exact HeqB2.
  - (* se_par_comm *)
    split; intros B0 Heqp; discriminate.
  - (* se_par_assoc *)
    split; intros B0 Heqp; discriminate.
  - (* se_par_nil P : PPar P PNil ≡ P *)
    split; intros B0 Heqp.
    + (* PPar P PNil = PReplicate B0 — discriminate. *)
      discriminate.
    + (* P = PReplicate B0. So PPar (PReplicate B0) PNil ≡ PReplicate B0. *)
      subst. exists B0. split.
      * apply se_par_nil.
      * apply se_refl.
  - (* se_par_cong *)
    split; intros B0 Heqp; discriminate.
  - (* se_input_cong *)
    split; intros B0 Heqp; discriminate.
  - (* se_output_cong *)
    split; intros B0 Heqp; discriminate.
  - (* se_deref_cong *)
    split; intros B0 Heqp; discriminate.
  - (* se_replicate_cong P0 P0' Hpp IHpp *)
    rename P into P0. rename P' into P0'. rename Heq into Hpp.
    split; intros B0 Heqp.
    + injection Heqp as HB0; subst B0.
      exists P0'. split; [apply se_refl | exact Hpp].
    + injection Heqp as HB0; subst B0.
      exists P0. split; [apply se_refl | exact Hpp].
Qed.

(* Forward-direction corollary. *)
Lemma se_PReplicate_inv :
  forall B R, PReplicate B ≡ R -> exists B', R ≡ PReplicate B' /\ B ≡ B'.
Proof.
  intros B R Heq.
  destruct (se_PReplicate_inv_both _ _ Heq) as [Hfwd _].
  apply Hfwd. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 12: Weak barbed equivalence (coinduction-free)
   ═══════════════════════════════════════════════════════════════════════════

   This section gives the mechanized, axiom-free characterization of the
   Meredith-Radestock encoding used by this development: weak barbs of
   the body propagate to both the primitive and reflective wrappers.

   Why this formulation supersedes strong bisim:

   1. [bang_encoding x P] has a top-level output barb on x (from
      [POutput x (PPar (D_encoding x) P)]) that [PReplicate P] lacks under
      the freshness hypothesis [name_not_free_in_proc x P]. No barb-sensitive
      equivalence can hold without either (a) restricting observation on x,
      or (b) adding a name-restriction binder to the AST. The AST has no
      [PNew], so (a) is the honest choice.

   2. Strong bisim requires inverting [rho_step] on the encoding side to
      produce a matching step on the replicate side. This runs into the
      PReplicate-injectivity-modulo-≡ obstacle (Group B3 in the progress
      tracker), which is syntactic, not semantic.

   3. The coinduction-free barb characterization reduces the proof to
      structural reasoning about reachability — no cofix, no guardedness,
      no witness-relation bisimulation-up-to boilerplate.

   Scope of this section:

   - 12.A: Reachability lemmas — [PReplicate P] and [bang_encoding x P]
           both reach states with arbitrarily many copies of P in parallel.
   - 12.B: Forward (body ⇒ wrapper) barb lemmas — if P has a weak input/
           output barb on any y, both [PReplicate P] and [bang_encoding x P]
           have the corresponding barb on y.
   - 12.C: Iterated unfolding lemmas showing that both wrappers can
           accumulate arbitrarily many copies of the body.
   - 12.D: Headline theorem [preplicate_bang_encoding_body_barbs_sound]
           bundles the body-to-wrapper propagation result.

   This is intentionally not a theorem saying that every weak barb of
   a replicated wrapper comes from one copy of the body. That stronger
   statement is not a valid consequence of [!P ~ P | !P].                  *)

(* ─────────────────────────────────────────────────────────────────────────
   Section 12.A: Forward direction — body barbs propagate to both wrappers
   ─────────────────────────────────────────────────────────────────────────

   Whenever the body process [P] can eventually exhibit a barb
   (input or output) on any
   channel [y], both its replicated wrappers [PReplicate P] and
   [bang_encoding x P] also eventually exhibit that same barb.

   For [PReplicate P], this is a direct corollary of
   [weak_barb_input_replicate_body] / [weak_barb_output_replicate_body]
   in [WeakBarbedEquiv.v].

   For [bang_encoding x P], we use [bang_encoding_unfolds] to step once
   into [PPar (bang_encoding x P) P] and then propagate P's weak barb
   via the right parallel arm. No freshness or non-x hypothesis is
   required for this direction: the barb on y can occur on any channel,
   including x itself, and it still propagates.                              *)

(* [PReplicate P] exposes every weak input barb of P — direct corollary of
   [weak_barb_input_replicate_body]. *)
Lemma preplicate_weak_barb_input_from_body : forall P y,
  weak_barb_input P y -> weak_barb_input (PReplicate P) y.
Proof.
  intros P y H. apply weak_barb_input_replicate_body. exact H.
Qed.

(* [PReplicate P] exposes every weak output barb of P. *)
Lemma preplicate_weak_barb_output_from_body : forall P y,
  weak_barb_output P y -> weak_barb_output (PReplicate P) y.
Proof.
  intros P y H. apply weak_barb_output_replicate_body. exact H.
Qed.

(* [bang_encoding x P] exposes every weak input barb of P, using the
   first unfold step [bang_encoding x P ⇝ bang_encoding x P | P] to
   place P in the right parallel arm. *)
Lemma bang_encoding_weak_barb_input_from_body : forall x P y,
  closed_name x -> closed_proc P ->
  weak_barb_input P y -> weak_barb_input (bang_encoding x P) y.
Proof.
  intros x P y Hxcl HPcl [P' [y' [Hreach [Hxy Hbarb]]]].
  exists (PPar (bang_encoding x P) P'), y'.
  split.
  - (* bang_encoding x P ⇝ bang_encoding x P | P ⇝* bang_encoding x P | P' *)
    eapply rr_step.
    + apply bang_encoding_unfolds; assumption.
    + apply rho_reachable_par_r. exact Hreach.
  - split; [exact Hxy | apply input_barb_par_r; exact Hbarb].
Qed.

(* [bang_encoding x P] exposes every weak output barb of P. *)
Lemma bang_encoding_weak_barb_output_from_body : forall x P y,
  closed_name x -> closed_proc P ->
  weak_barb_output P y -> weak_barb_output (bang_encoding x P) y.
Proof.
  intros x P y Hxcl HPcl [P' [y' [Hreach [Hxy Hbarb]]]].
  exists (PPar (bang_encoding x P) P'), y'.
  split.
  - eapply rr_step.
    + apply bang_encoding_unfolds; assumption.
    + apply rho_reachable_par_r. exact Hreach.
  - split; [exact Hxy | apply output_barb_par_r; exact Hbarb].
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   Section 12.B: Forward direction for immediate barbs on the body
   ─────────────────────────────────────────────────────────────────────────

   A specialized corollary: if P itself already barbs on y (without
   needing any reduction), then both wrappers immediately contain a weak
   barb via zero or one additional reductions.                              *)

Lemma preplicate_weak_barb_input_from_immediate : forall P y,
  input_barb P y -> weak_barb_input (PReplicate P) y.
Proof.
  intros P y H.
  apply preplicate_weak_barb_input_from_body.
  apply input_barb_weak. exact H.
Qed.

Lemma preplicate_weak_barb_output_from_immediate : forall P y,
  output_barb P y -> weak_barb_output (PReplicate P) y.
Proof.
  intros P y H.
  apply preplicate_weak_barb_output_from_body.
  apply output_barb_weak. exact H.
Qed.

Lemma bang_encoding_weak_barb_input_from_immediate : forall x P y,
  closed_name x -> closed_proc P ->
  input_barb P y -> weak_barb_input (bang_encoding x P) y.
Proof.
  intros x P y Hxcl HPcl H.
  apply bang_encoding_weak_barb_input_from_body; [assumption | assumption |].
  apply input_barb_weak. exact H.
Qed.

Lemma bang_encoding_weak_barb_output_from_immediate : forall x P y,
  closed_name x -> closed_proc P ->
  output_barb P y -> weak_barb_output (bang_encoding x P) y.
Proof.
  intros x P y Hxcl HPcl H.
  apply bang_encoding_weak_barb_output_from_body; [assumption | assumption |].
  apply output_barb_weak. exact H.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   Section 12.C: Unfolding reachability — n copies of P accumulate
   ─────────────────────────────────────────────────────────────────────────

   Both [PReplicate P] and [bang_encoding x P] reach a state of the form
   [PPar W (par_many (List.repeat P n))] for any n, where W is the
   respective wrapper. These iterated-unfolding lemmas are useful
   operational facts about the two replication views.                       *)

(* One COMM step of [bang_encoding x P] exposes [P] on the right, as
   proven by [bang_encoding_unfolds]. Chaining n such steps accumulates
   n copies of [P] as [par_many (List.repeat P n)]. *)
Lemma bang_encoding_reaches_n_copies : forall x P n,
  closed_name x -> closed_proc P ->
  exists Q, rho_reachable (bang_encoding x P) Q /\
            Q ≡ PPar (bang_encoding x P) (par_many (List.repeat P n)).
Proof.
  intros x P n Hxcl HPcl. induction n as [| n IH].
  - (* n = 0: par_many [] = PNil, so target ≡ PPar (bang_encoding x P) PNil
       which is ≡ (bang_encoding x P) via se_par_nil.  Take Q = bang_encoding. *)
    exists (bang_encoding x P). split.
    + apply rr_refl.
    + simpl. apply se_sym. apply se_par_nil.
  - (* n = S n': from IH we have Q ≡ bang_encoding | par_many (repeat P n).
       One more unfold step gives bang_encoding ⇝ bang_encoding | P;
       combined with IH's par, the shape becomes
       (bang_encoding | P) | par_many (repeat P n)
       ≡ bang_encoding | (P | par_many (repeat P n))
       = bang_encoding | par_many (P :: repeat P n)
       = bang_encoding | par_many (repeat P (S n)). *)
    destruct IH as [Q [Hreach Hse]].
    exists (PPar (PPar (bang_encoding x P) P) (par_many (List.repeat P n))).
    split.
    + (* bang_encoding ⇝* Q (IH) ⇝? PPar (bang_encoding | P) (par_many _).
         From Q ≡ bang_encoding | par_many: apply bang_encoding_unfolds on
         the left factor. *)
      eapply rho_reachable_trans; [exact Hreach |].
      eapply rr_step.
      * eapply rs_struct; [exact Hse | | apply se_refl].
        apply rs_par_l. apply bang_encoding_unfolds; assumption.
      * apply rr_refl.
    + (* (bang_encoding | P) | par_many (repeat P n)
         ≡ bang_encoding | (P | par_many (repeat P n))
         = bang_encoding | par_many (repeat P (S n)). *)
      simpl. apply se_par_assoc.
Qed.

(* [PReplicate P] reaches a state of the same shape via n [rs_replicate] steps. *)
Lemma preplicate_reaches_n_copies : forall P n,
  exists Q, rho_reachable (PReplicate P) Q /\
            Q ≡ PPar (PReplicate P) (par_many (List.repeat P n)).
Proof.
  intros P n. induction n as [| n IH].
  - exists (PReplicate P). split.
    + apply rr_refl.
    + simpl. apply se_sym. apply se_par_nil.
  - destruct IH as [Q [Hreach Hse]].
    exists (PPar (PPar P (PReplicate P)) (par_many (List.repeat P n))).
    split.
    + eapply rho_reachable_trans; [exact Hreach |].
      eapply rr_step.
      * eapply rs_struct; [exact Hse | | apply se_refl].
        apply rs_par_l. apply rs_replicate.
      * apply rr_refl.
    + (* (P | PReplicate P) | par_many (repeat P n)
         ≡ PReplicate P | (P | par_many (repeat P n))  (via comm + assoc)
         = PReplicate P | par_many (repeat P (S n)). *)
      simpl. eapply se_trans.
      * apply se_par_cong_l. apply se_par_comm.
      * apply se_par_assoc.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   Section 12.D: Headline forward theorem
   ─────────────────────────────────────────────────────────────────────────

   Every weak barb of the body P is reflected as a weak barb of both
   wrappers. This is the rigorous, kernel-checked content of the
   replication encoding's body-to-wrapper observational correspondence.       *)

Theorem preplicate_bang_encoding_body_barbs_sound : forall x P y,
  closed_name x -> closed_proc P ->
  (weak_barb_input P y ->
     weak_barb_input (PReplicate P) y /\
     weak_barb_input (bang_encoding x P) y)
  /\ (weak_barb_output P y ->
     weak_barb_output (PReplicate P) y /\
     weak_barb_output (bang_encoding x P) y).
Proof.
  intros x P y Hxcl HPcl. split.
  - intros Hwbi. split.
    + apply preplicate_weak_barb_input_from_body. exact Hwbi.
    + apply bang_encoding_weak_barb_input_from_body; assumption.
  - intros Hwbo. split.
    + apply preplicate_weak_barb_output_from_body. exact Hwbo.
    + apply bang_encoding_weak_barb_output_from_body; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 13: Reverse-direction infrastructure — step inversion
   ═══════════════════════════════════════════════════════════════════════════

   The reverse direction of the weak barbed equivalence requires
   characterizing the reachable states of [PReplicate P] and of
   [bang_encoding x P]. The foundational fact for the PReplicate side is
   that any step from a process structurally equivalent to [PReplicate P]
   must land in a process structurally equivalent to [PPar P (PReplicate
   P)]: there is only one possible move.

   Proof technique: induction on [rho_step] with [onlyreplicate_se_both]
   + [se_PReplicate_inj] + [only_replicate_se_PReplicate] +
   [head_count_zero_se_nil] (all from StructEquivHeads.v Section 13)
   discharging the [rs_par_l] / [rs_par_r] cases via arm-decomposition.
   The [rs_comm] case falls to [count_replicates_se] (a comm redex has
   zero replicates while [PReplicate P] has one).                        *)

Lemma step_PReplicate_inv_se : forall S R,
  rho_step S R ->
  forall P, S ≡ PReplicate P -> R ≡ PPar P (PReplicate P).
Proof.
  intros S R Hstep. induction Hstep; intros P0 HeqS.
  - (* rs_comm: S = PPar (PInput x P) (POutput x Q).
       count_replicates S = 0, count_replicates (PReplicate P0) = 1.
       Contradiction via count_replicates_se. *)
    exfalso.
    pose proof (count_replicates_se _ _ HeqS) as Hcr.
    simpl in Hcr. lia.
  - (* rs_par_l: S = PPar P Q, step on P: PPar P Q ⇝ PPar P' Q.
       Decompose PPar P Q ≡ PReplicate P0 via only_replicate. *)
    destruct (onlyreplicate_se_both _ _ HeqS) as [_ Hbwd].
    destruct (Hbwd P0 (OR_base P0)) as [B' [Hor HeqB]].
    inversion Hor; subst.
    + (* OR_par_l: only_replicate P B', head_count Q = 0.
         By only_replicate_se_PReplicate: P ≡ PReplicate B'.
         Apply IH: P' ≡ PPar B' (PReplicate B').
         By head_count_zero_se_nil: Q ≡ PNil.
         Conclude: PPar P' Q ≡ PPar (PPar B' (PReplicate B')) PNil
           ≡ PPar B' (PReplicate B')
           ≡ PPar P0 (PReplicate P0). *)
      assert (HP_rep : P ≡ PReplicate B') by
        (apply only_replicate_se_PReplicate; assumption).
      specialize (IHHstep B' HP_rep).
      assert (HQ_nil : Q ≡ PNil) by
        (apply head_count_zero_se_nil; assumption).
      eapply se_trans.
      * apply se_par_cong; [exact IHHstep | exact HQ_nil].
      * eapply se_trans; [apply se_par_nil |].
        apply se_par_cong; [apply se_sym; exact HeqB |].
        apply se_replicate_cong. apply se_sym. exact HeqB.
    + (* OR_par_r: head_count P = 0, only_replicate Q B'.
         But head_count P = 0 ∧ rho_step P P' → contradiction via
         rho_step_head_count_ge_one. *)
      exfalso.
      pose proof (rho_step_head_count_ge_one _ _ Hstep) as HcP.
      lia.
  - (* rs_par_r: S = PPar Q P, step on P: PPar Q P ⇝ PPar Q P'.
       Symmetric to rs_par_l. *)
    destruct (onlyreplicate_se_both _ _ HeqS) as [_ Hbwd].
    destruct (Hbwd P0 (OR_base P0)) as [B' [Hor HeqB]].
    inversion Hor; subst.
    + (* OR_par_l: only_replicate Q B', head_count P = 0.
         Contradiction: rho_step P P' needs head_count P ≥ 1. *)
      exfalso.
      pose proof (rho_step_head_count_ge_one _ _ Hstep) as HcP.
      lia.
    + (* OR_par_r: head_count Q = 0, only_replicate P B'.
         By only_replicate_se_PReplicate: P ≡ PReplicate B'.
         Apply IH: P' ≡ PPar B' (PReplicate B').
         By head_count_zero_se_nil: Q ≡ PNil.
         Conclude via PPar Q P' ≡ PPar PNil (PPar B' (PReplicate B'))
           ≡ PPar B' (PReplicate B') ≡ PPar P0 (PReplicate P0). *)
      assert (HP_rep : P ≡ PReplicate B') by
        (apply only_replicate_se_PReplicate; assumption).
      specialize (IHHstep B' HP_rep).
      assert (HQ_nil : Q ≡ PNil) by
        (apply head_count_zero_se_nil; assumption).
      eapply se_trans.
      * apply se_par_cong; [exact HQ_nil | exact IHHstep].
      * eapply se_trans; [apply se_nil_par |].
        apply se_par_cong; [apply se_sym; exact HeqB |].
        apply se_replicate_cong. apply se_sym. exact HeqB.
  - (* rs_struct: P ≡ P', P' ⇝ Q', Q' ≡ Q.
       S = P, HeqS : P ≡ PReplicate P0.
       We need: Q ≡ PPar P0 (PReplicate P0).
       From Heq1 : P ≡ P' and HeqS : P ≡ PReplicate P0, get P' ≡ PReplicate P0.
       Apply IH on rho_step P' Q' with P' ≡ PReplicate P0:
         Q' ≡ PPar P0 (PReplicate P0).
       Chain with Heq2 : Q' ≡ Q by transitivity. *)
    assert (HP' : P' ≡ PReplicate P0) by
      (eapply se_trans; [apply se_sym; exact H | exact HeqS]).
    specialize (IHHstep P0 HP').
    eapply se_trans; [apply se_sym; exact H0 | exact IHHstep].
  - (* rs_replicate: S = PReplicate P, R = PPar P (PReplicate P).
       HeqS : PReplicate P ≡ PReplicate P0.
       By se_PReplicate_inj: P ≡ P0.
       So PPar P (PReplicate P) ≡ PPar P0 (PReplicate P0). *)
    apply se_PReplicate_inj in HeqS.
    apply se_par_cong; [exact HeqS |].
    apply se_replicate_cong. exact HeqS.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 14: Shape invariant for PReplicate-reachable states
   ═══════════════════════════════════════════════════════════════════════════

   The shape invariant for the reverse direction: any state reachable from
   [PReplicate body] is ≡-equivalent to [PPar (PReplicate body) R] for
   some process R reachable from a multi-copy parallel composition of
   [body]. The "multi-copy" formulation (as opposed to the naive
   "R is par_many of individual body-descendants") is crucial for the
   cross-COMM case: COMMs between two copies of body produce results
   that are reachable from two-body in parallel but not from a single body.

   This shape invariant is preserved under rho_step by systematic case
   analysis, discharging the critical cases via:
   - [step_PReplicate_inv_se] (Section 13) for the rs_par_l case.
   - [count_replicates_se] for the rs_comm / rs_replicate contradictions.
   - Direct chaining via [rho_step_struct] for rs_struct absorption.    *)

Definition preplicate_shape (body : proc) (Q : proc) : Prop :=
  exists n R,
    rho_reachable (par_many (List.repeat body n)) R /\
    Q ≡ PPar (PReplicate body) R.

(* Base case: [PReplicate body] is in shape with n = 0, R = PNil. *)
Lemma preplicate_shape_init : forall body,
  preplicate_shape body (PReplicate body).
Proof.
  intros body. exists 0, PNil.
  split.
  - (* rho_reachable (par_many []) PNil. par_many [] = PNil. rr_refl. *)
    simpl. apply rr_refl.
  - (* PReplicate body ≡ PPar (PReplicate body) PNil. By se_sym, se_par_nil. *)
    apply se_sym. apply se_par_nil.
Qed.

(* ───────────────────────────────────────────────────────────────────────
   Section 14.A: PReplicate-head locate lemma

   The critical structural lemma underlying the shape closure: if
   [PPar P Q ≡ PPar (PReplicate body) R], then the PReplicate body head
   must appear in exactly one of the two arms of the LHS, and the
   other arm's heads match R's heads (modulo permutation and ≡).

   Proof technique: use [struct_equiv_heads_perm] to reduce to a claim
   about permutations of the head lists, then locate [PReplicate body]
   by [in_app_or] after commuting through the [list_equiv]-[Permutation]
   zigzag.
   ─────────────────────────────────────────────────────────────────────── *)

(* Helper: if [heads P] contains a specific PReplicate head, extract a
   "rest" process that is the parallel composition of the other heads. *)
Lemma heads_PReplicate_inv : forall P body,
  In (PReplicate body) (heads P) ->
  exists P_rest, P ≡ PPar (PReplicate body) P_rest.
Proof.
  intros P body HIn.
  (* By heads_to_proc_heads_se: P ≡ heads_to_proc (heads P).
     By in_split on HIn: heads P = L1 ++ PReplicate body :: L2.
     Then heads_to_proc (L1 ++ PReplicate body :: L2) ≡
          PPar (heads_to_proc L1) (PPar (PReplicate body) (heads_to_proc L2))
     ≡ PPar (PReplicate body) (PPar (heads_to_proc L1) (heads_to_proc L2))
     via comm + assoc. Take P_rest := PPar (heads_to_proc L1) (heads_to_proc L2). *)
  pose proof (heads_to_proc_heads_se P) as Hse.
  apply in_split in HIn.
  destruct HIn as [L1 [L2 Hheads]].
  exists (PPar (heads_to_proc L1) (heads_to_proc L2)).
  rewrite Hheads in Hse.
  (* Hse : heads_to_proc (L1 ++ PReplicate body :: L2) ≡ P. *)
  apply se_sym in Hse.
  eapply se_trans; [exact Hse |].
  (* Goal: heads_to_proc (L1 ++ PReplicate body :: L2)
          ≡ PPar (PReplicate body) (PPar (heads_to_proc L1) (heads_to_proc L2)). *)
  eapply se_trans; [apply heads_to_proc_app |].
  (* PPar (heads_to_proc L1) (heads_to_proc (PReplicate body :: L2))
     = PPar (heads_to_proc L1) (PPar (PReplicate body) (heads_to_proc L2)) *)
  simpl.
  (* PPar (heads_to_proc L1) (PPar (PReplicate body) (heads_to_proc L2))
     ≡ PPar (PReplicate body) (PPar (heads_to_proc L1) (heads_to_proc L2))
     via par_comm + par_assoc. *)
  eapply se_trans; [apply se_sym; apply se_par_assoc |].
  eapply se_trans;
    [apply se_par_cong_l; apply se_par_comm |].
  apply se_par_assoc.
Qed.

(* Key decomposition: if PPar P Q ≡ PPar (PReplicate body) R, then the
   PReplicate body head lives in one of the two LHS arms. Which arm it
   lives in determines the "rest" shape. *)
Lemma se_par_preplicate_locate : forall P Q body R,
  PPar P Q ≡ PPar (PReplicate body) R ->
  (exists body' P_rest,
     body ≡ body' /\
     P ≡ PPar (PReplicate body') P_rest)
  \/ (exists body' Q_rest,
     body ≡ body' /\
     Q ≡ PPar (PReplicate body') Q_rest).
Proof.
  intros P Q body R Hse.
  (* Use struct_equiv_heads_perm to get perm_equiv of heads. *)
  pose proof (struct_equiv_heads_perm _ _ Hse) as Hperm.
  unfold perm_equiv in Hperm.
  destruct Hperm as [zs [Hle Hperm]].
  (* heads (PPar P Q) = heads P ++ heads Q.
     heads (PPar (PReplicate body) R) = [PReplicate body] ++ heads R
                                      = PReplicate body :: heads R. *)
  simpl in Hle, Hperm.
  (* Hle : list_equiv (heads P ++ heads Q) zs.
     Hperm : Permutation zs (PReplicate body :: heads R). *)
  (* Since Permutation is an equivalence, PReplicate body ∈ zs. *)
  assert (HIn_zs : In (PReplicate body) zs).
  { apply Permutation_sym in Hperm.
    apply Permutation_in with (l := PReplicate body :: heads R);
      [exact Hperm | left; reflexivity]. }
  (* Split zs via list_equiv_app_inv to find which arm's heads contain the image. *)
  destruct (list_equiv_app_inv _ _ _ Hle) as [zsP [zsQ [Heqzs [Hle_P Hle_Q]]]].
  subst zs.
  (* Now HIn_zs : In (PReplicate body) (zsP ++ zsQ).
     Split via in_app_or. *)
  apply in_app_or in HIn_zs.
  destruct HIn_zs as [HInP | HInQ].
  - (* PReplicate body ∈ zsP. By list_equiv_sym Hle_P (pointwise ≡ on zsP ~ heads P),
       there's some h ∈ heads P with h ≡ PReplicate body. By se_PReplicate_inv_both
       backward, h = PReplicate body' with body' ≡ body. Then apply heads_PReplicate_inv. *)
    apply list_equiv_sym in Hle_P.
    (* Hle_P : list_equiv zsP (heads P). *)
    (* Extract h ∈ heads P with h ≡ PReplicate body via list_equiv_in_transport. *)
    destruct (list_equiv_in_transport _ _ _ Hle_P HInP) as [h [HIn_h Hh_eq]].
    (* Hh_eq : PReplicate body ≡ h. Flip to h ≡ PReplicate body. *)
    apply se_sym in Hh_eq.
    (* Hh_rep : h ≡ PReplicate body'. *)
    (* But h itself is a head of heads P, which is always a head shape (not PPar / not PNil).
       We need h to be SYNTACTICALLY PReplicate body' to apply heads_PReplicate_inv.
       Actually heads_PReplicate_inv needs In (PReplicate body) (heads P) literally.
       We have h ≡ PReplicate body but not syntactic equality.
       We need another path: use se_PReplicate_inv_both differently. *)
    (* Fact: h ∈ heads P and h is a head shape (heads_are_heads). So h is one of
       PInput, POutput, PDeref, PReplicate (not PNil, not PPar). Since h ≡ PReplicate body,
       by cases on h: the only case compatible with h ≡ PReplicate body is h = PReplicate h_body.
       Then heads_PReplicate_inv applies with body := h_body. *)
    pose proof (heads_are_heads P h HIn_h) as [Hh_nnil Hh_npar].
    destruct h as [ | n_h B_h | n_h Q_h | h1 h2 | n_h | h_body ].
    + (* PNil: contradicts Hh_nnil. *)
      exfalso. apply Hh_nnil. reflexivity.
    + (* PInput: count_replicates = 0, but ≡ PReplicate body gives = 1. *)
      exfalso.
      pose proof (count_replicates_se _ _ Hh_eq) as Hcr.
      simpl in Hcr. lia.
    + (* POutput: same. *)
      exfalso.
      pose proof (count_replicates_se _ _ Hh_eq) as Hcr.
      simpl in Hcr. lia.
    + (* PPar: excluded by Hh_npar. *)
      exfalso. eapply Hh_npar. reflexivity.
    + (* PDeref: count_replicates = 0. *)
      exfalso.
      pose proof (count_replicates_se _ _ Hh_eq) as Hcr.
      simpl in Hcr. lia.
    + (* PReplicate h_body: h = PReplicate h_body. Then h ≡ PReplicate body gives
         h_body ≡ body by se_PReplicate_inj. Apply heads_PReplicate_inv. *)
      assert (Hbody_eq : h_body ≡ body) by (apply se_PReplicate_inj; exact Hh_eq).
      destruct (heads_PReplicate_inv P h_body HIn_h) as [P_rest HP_rest].
      left. exists h_body, P_rest.
      split; [apply se_sym; exact Hbody_eq | exact HP_rest].
  - (* Symmetric for Q. *)
    apply list_equiv_sym in Hle_Q.
    destruct (list_equiv_in_transport _ _ _ Hle_Q HInQ) as [h [HIn_h Hh_eq]].
    apply se_sym in Hh_eq.
    pose proof (heads_are_heads Q h HIn_h) as [Hh_nnil Hh_npar].
    destruct h as [ | n_h B_h | n_h Q_h | h1 h2 | n_h | h_body ].
    + exfalso. apply Hh_nnil. reflexivity.
    + exfalso.
      pose proof (count_replicates_se _ _ Hh_eq) as Hcr.
      simpl in Hcr. lia.
    + exfalso.
      pose proof (count_replicates_se _ _ Hh_eq) as Hcr.
      simpl in Hcr. lia.
    + exfalso. eapply Hh_npar. reflexivity.
    + exfalso.
      pose proof (count_replicates_se _ _ Hh_eq) as Hcr.
      simpl in Hcr. lia.
    + assert (Hbody_eq : h_body ≡ body) by (apply se_PReplicate_inj; exact Hh_eq).
      destruct (heads_PReplicate_inv Q h_body HIn_h) as [Q_rest HQ_rest].
      right. exists h_body, Q_rest.
      split; [apply se_sym; exact Hbody_eq | exact HQ_rest].
Qed.

(* ───────────────────────────────────────────────────────────────────────
   Section 14.B: Structural-equivalence closure of split barbs
   ───────────────────────────────────────────────────────────────────────

   Input and output barbs transport across [≡] modulo ≡N-shifting of the
   observed channel. The proof is a mutual induction on [≡] mirroring
   the structure of [onlyinput_se_both] and [onlyoutput_se_both] in
   [StructEquivHeads.v]. Each case either chains IHs or contradicts
   via a constructor mismatch (input_barb only builds from PInput/PPar/
   PReplicate, so se_output_cong / se_deref_cong / se_replicate_cong
   that would insert the wrong constructor are handled by inversion).
   ─────────────────────────────────────────────────────────────────────── *)

Lemma input_barb_se_both :
  forall P Q, P ≡ Q ->
    (forall y, input_barb P y -> exists y', y ≡N y' /\ input_barb Q y')
    /\ (forall y, input_barb Q y -> exists y', y ≡N y' /\ input_barb P y').
Proof.
  intros P Q Heq. induction Heq.
  - (* se_refl P *)
    split; intros y Hb; exists y; split;
      solve [apply se_name_refl | exact Hb].
  - (* se_sym *)
    destruct IHHeq as [Hf Hb]. split; assumption.
  - (* se_trans P Q R *)
    destruct IHHeq1 as [Hf1 Hb1]. destruct IHHeq2 as [Hf2 Hb2].
    split; intros y Hb.
    + destruct (Hf1 y Hb) as [yq [Hyq Hbq]].
      destruct (Hf2 yq Hbq) as [yr [Hyr Hbr]].
      exists yr. split; [eapply se_name_trans; eassumption | exact Hbr].
    + destruct (Hb2 y Hb) as [yq [Hyq Hbq]].
      destruct (Hb1 yq Hbq) as [yp [Hyp Hbp]].
      exists yp. split; [eapply se_name_trans; eassumption | exact Hbp].
  - (* se_par_comm P Q : PPar P Q ≡ PPar Q P *)
    split; intros y Hb; inversion Hb; subst.
    + exists y. split; [apply se_name_refl | apply input_barb_par_r; assumption].
    + exists y. split; [apply se_name_refl | apply input_barb_par_l; assumption].
    + exists y. split; [apply se_name_refl | apply input_barb_par_r; assumption].
    + exists y. split; [apply se_name_refl | apply input_barb_par_l; assumption].
  - (* se_par_assoc P Q R : PPar (PPar P Q) R ≡ PPar P (PPar Q R) *)
    split; intros y Hb; inversion Hb; subst.
    + (* input_barb (PPar P Q) y *)
      match goal with
      | [ Hi : input_barb (PPar P Q) y |- _ ] => inversion Hi; subst
      end.
      * exists y. split; [apply se_name_refl | apply input_barb_par_l; assumption].
      * exists y. split; [apply se_name_refl
                        | apply input_barb_par_r; apply input_barb_par_l; assumption].
    + exists y. split; [apply se_name_refl
                      | apply input_barb_par_r; apply input_barb_par_r; assumption].
    + exists y. split; [apply se_name_refl
                      | apply input_barb_par_l; apply input_barb_par_l; assumption].
    + match goal with
      | [ Hi : input_barb (PPar Q R) y |- _ ] => inversion Hi; subst
      end.
      * exists y. split; [apply se_name_refl
                        | apply input_barb_par_l; apply input_barb_par_r; assumption].
      * exists y. split; [apply se_name_refl | apply input_barb_par_r; assumption].
  - (* se_par_nil P : PPar P PNil ≡ P *)
    split; intros y Hb.
    + inversion Hb; subst.
      * exists y. split; [apply se_name_refl | assumption].
      * exfalso. eapply PNil_no_input_barb. eassumption.
    + exists y. split; [apply se_name_refl | apply input_barb_par_l; assumption].
  - (* se_par_cong P P' Q Q' *)
    destruct IHHeq1 as [Hf1 Hb1]. destruct IHHeq2 as [Hf2 Hb2].
    split; intros y Hb; inversion Hb as [| ? ? ? Hbi | ? ? ? Hbi |]; subst.
    + destruct (Hf1 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply input_barb_par_l; exact Hb'].
    + destruct (Hf2 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply input_barb_par_r; exact Hb'].
    + destruct (Hb1 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply input_barb_par_l; exact Hb'].
    + destruct (Hb2 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply input_barb_par_r; exact Hb'].
  - (* se_input_cong x x' P P' : PInput x P ≡ PInput x' P' with x ≡N x' *)
    rename H into Hxn.
    split; intros y Hb; inversion Hb; subst.
    + exists x'. split; [exact Hxn | apply input_barb_here].
    + exists x. split; [apply se_name_sym; exact Hxn | apply input_barb_here].
  - (* se_output_cong: input_barb can't come from POutput *)
    split; intros y Hb; inversion Hb.
  - (* se_deref_cong: input_barb can't come from PDeref *)
    split; intros y Hb; inversion Hb.
  - (* se_replicate_cong P P' : PReplicate P ≡ PReplicate P' *)
    destruct IHHeq as [Hf Hb].
    split; intros y Hi; inversion Hi as [| | | ? ? Hbi]; subst.
    + destruct (Hf y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply input_barb_replicate; exact Hb'].
    + destruct (Hb y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply input_barb_replicate; exact Hb'].
Qed.

Lemma output_barb_se_both :
  forall P Q, P ≡ Q ->
    (forall y, output_barb P y -> exists y', y ≡N y' /\ output_barb Q y')
    /\ (forall y, output_barb Q y -> exists y', y ≡N y' /\ output_barb P y').
Proof.
  intros P Q Heq. induction Heq.
  - split; intros y Hb; exists y; split;
      solve [apply se_name_refl | exact Hb].
  - destruct IHHeq as [Hf Hb]. split; assumption.
  - destruct IHHeq1 as [Hf1 Hb1]. destruct IHHeq2 as [Hf2 Hb2].
    split; intros y Hb.
    + destruct (Hf1 y Hb) as [yq [Hyq Hbq]].
      destruct (Hf2 yq Hbq) as [yr [Hyr Hbr]].
      exists yr. split; [eapply se_name_trans; eassumption | exact Hbr].
    + destruct (Hb2 y Hb) as [yq [Hyq Hbq]].
      destruct (Hb1 yq Hbq) as [yp [Hyp Hbp]].
      exists yp. split; [eapply se_name_trans; eassumption | exact Hbp].
  - split; intros y Hb; inversion Hb; subst.
    + exists y. split; [apply se_name_refl | apply output_barb_par_r; assumption].
    + exists y. split; [apply se_name_refl | apply output_barb_par_l; assumption].
    + exists y. split; [apply se_name_refl | apply output_barb_par_r; assumption].
    + exists y. split; [apply se_name_refl | apply output_barb_par_l; assumption].
  - split; intros y Hb; inversion Hb; subst.
    + match goal with
      | [ Hi : output_barb (PPar P Q) y |- _ ] => inversion Hi; subst
      end.
      * exists y. split; [apply se_name_refl | apply output_barb_par_l; assumption].
      * exists y. split; [apply se_name_refl
                        | apply output_barb_par_r; apply output_barb_par_l; assumption].
    + exists y. split; [apply se_name_refl
                      | apply output_barb_par_r; apply output_barb_par_r; assumption].
    + exists y. split; [apply se_name_refl
                      | apply output_barb_par_l; apply output_barb_par_l; assumption].
    + match goal with
      | [ Hi : output_barb (PPar Q R) y |- _ ] => inversion Hi; subst
      end.
      * exists y. split; [apply se_name_refl
                        | apply output_barb_par_l; apply output_barb_par_r; assumption].
      * exists y. split; [apply se_name_refl | apply output_barb_par_r; assumption].
  - split; intros y Hb.
    + inversion Hb; subst.
      * exists y. split; [apply se_name_refl | assumption].
      * exfalso. eapply PNil_no_output_barb. eassumption.
    + exists y. split; [apply se_name_refl | apply output_barb_par_l; assumption].
  - destruct IHHeq1 as [Hf1 Hb1]. destruct IHHeq2 as [Hf2 Hb2].
    split; intros y Hb; inversion Hb as [| ? ? ? Hbi | ? ? ? Hbi |]; subst.
    + destruct (Hf1 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply output_barb_par_l; exact Hb'].
    + destruct (Hf2 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply output_barb_par_r; exact Hb'].
    + destruct (Hb1 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply output_barb_par_l; exact Hb'].
    + destruct (Hb2 y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply output_barb_par_r; exact Hb'].
  - split; intros y Hb; inversion Hb.
  - (* se_output_cong x x' Q Q' : POutput x Q ≡ POutput x' Q' with x ≡N x' *)
    rename H into Hxn.
    split; intros y Hb; inversion Hb; subst.
    + exists x'. split; [exact Hxn | apply output_barb_here].
    + exists x. split; [apply se_name_sym; exact Hxn | apply output_barb_here].
  - split; intros y Hb; inversion Hb.
  - destruct IHHeq as [Hf Hb].
    split; intros y Hi; inversion Hi as [| | | ? ? Hbi]; subst.
    + destruct (Hf y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply output_barb_replicate; exact Hb'].
    + destruct (Hb y Hbi) as [y' [Hy' Hb']].
      exists y'. split; [exact Hy' | apply output_barb_replicate; exact Hb'].
Qed.

(* ───────────────────────────────────────────────────────────────────────
   Section 14.C: Step-inversion preserving the PReplicate body factor
   ───────────────────────────────────────────────────────────────────────

   Generalization of [step_PReplicate_inv_se] (Section 13): if the
   step source is only ≡-equivalent to [PPar (PReplicate body) P_rest]
   (with an arbitrary sibling P_rest), the result still contains a
   [PPar (PReplicate body) P_rest'] factor for some P_rest'.

   This is the "invariance of the replication factor under arbitrary
   steps" lemma — the structural analogue of Milner's expansion
   theorem for replication. Proved by induction on [rho_step] using
   [se_par_preplicate_locate] to route each rs_par sub-case.
   ─────────────────────────────────────────────────────────────────────── *)

Lemma step_PPar_PReplicate_inv_se : forall S R,
  rho_step S R ->
  forall body P_rest,
    S ≡ PPar (PReplicate body) P_rest ->
    exists P_rest', R ≡ PPar (PReplicate body) P_rest'.
Proof.
  intros S R Hstep. induction Hstep; intros body P_rest Hse.
  - (* rs_comm: source = PPar (PInput x P) (POutput x Q).
       count_replicates LHS = 0, RHS ≥ 1. Contradiction. *)
    exfalso.
    pose proof (count_replicates_se _ _ Hse) as Hcr.
    simpl in Hcr. lia.
  - (* rs_par_l: S = PPar P Q, step on P giving P'.
       Apply se_par_preplicate_locate to decide which arm holds PReplicate body. *)
    destruct (se_par_preplicate_locate P Q body P_rest Hse)
      as [[body' [P_rest_P [Hbody' HP_se]]] | [body' [Q_rest [Hbody' HQ_se]]]].
    + (* Arm (a): P ≡ PPar (PReplicate body') P_rest_P. Recurse via IH. *)
      destruct (IHHstep body' P_rest_P HP_se) as [P_rest_P' HP'_se].
      exists (PPar P_rest_P' Q).
      (* Goal: PPar P' Q ≡ PPar (PReplicate body) (PPar P_rest_P' Q).
         Chain: PPar P' Q ≡ PPar (PPar (PReplicate body') P_rest_P') Q  (by HP'_se)
                         ≡ PPar (PReplicate body') (PPar P_rest_P' Q)  (assoc)
                         ≡ PPar (PReplicate body)  (PPar P_rest_P' Q)  (via body ≡ body'). *)
      eapply se_trans.
      * apply se_par_cong; [exact HP'_se | apply se_refl].
      * eapply se_trans; [apply se_par_assoc |].
        apply se_par_cong;
          [apply se_replicate_cong; apply se_sym; exact Hbody'
          | apply se_refl].
    + (* Arm (b): Q ≡ PPar (PReplicate body') Q_rest. The step is on P (disjoint).
         Rebuild R = PPar P' Q ≡ PPar P' (PPar (PReplicate body') Q_rest)
                              ≡ PPar (PReplicate body') (PPar P' Q_rest)
                              ≡ PPar (PReplicate body)  (PPar P' Q_rest).
         Take P_rest' := PPar P' Q_rest. *)
      exists (PPar P' Q_rest).
      eapply se_trans.
      * apply se_par_cong; [apply se_refl | exact HQ_se].
      * eapply se_trans.
        -- apply se_sym. apply se_par_assoc.
        -- eapply se_trans.
           ++ apply se_par_cong_l. apply se_par_comm.
           ++ eapply se_trans; [apply se_par_assoc |].
              apply se_par_cong;
                [apply se_replicate_cong; apply se_sym; exact Hbody'
                | apply se_refl].
  - (* rs_par_r: S = PPar Q P, step on P. Symmetric. *)
    destruct (se_par_preplicate_locate Q P body P_rest Hse)
      as [[body' [Q_rest [Hbody' HQ_se]]] | [body' [P_rest_P [Hbody' HP_se]]]].
    + (* Arm (a): Q ≡ PPar (PReplicate body') Q_rest. Step on P (disjoint).
         R = PPar Q P' ≡ PPar (PPar (PReplicate body') Q_rest) P'
                      ≡ PPar (PReplicate body') (PPar Q_rest P'). *)
      exists (PPar Q_rest P').
      eapply se_trans.
      * apply se_par_cong; [exact HQ_se | apply se_refl].
      * eapply se_trans; [apply se_par_assoc |].
        apply se_par_cong;
          [apply se_replicate_cong; apply se_sym; exact Hbody'
          | apply se_refl].
    + (* Arm (b): P ≡ PPar (PReplicate body') P_rest_P. Recurse via IH. *)
      destruct (IHHstep body' P_rest_P HP_se) as [P_rest_P' HP'_se].
      exists (PPar Q P_rest_P').
      (* R = PPar Q P' ≡ PPar Q (PPar (PReplicate body') P_rest_P')
                      ≡ PPar (PReplicate body') (PPar Q P_rest_P')
         via comm+assoc tricks. *)
      eapply se_trans.
      * apply se_par_cong; [apply se_refl | exact HP'_se].
      * eapply se_trans; [apply se_sym; apply se_par_assoc |].
        eapply se_trans.
        -- apply se_par_cong_l. apply se_par_comm.
        -- eapply se_trans; [apply se_par_assoc |].
           apply se_par_cong;
             [apply se_replicate_cong; apply se_sym; exact Hbody'
             | apply se_refl].
  - (* rs_struct: S ≡ P' ⇝ Q' ≡ Q. Chain ≡ and recurse. *)
    assert (Hse_P' : P' ≡ PPar (PReplicate body) P_rest)
      by (eapply se_trans; [apply se_sym; exact H | exact Hse]).
    destruct (IHHstep body P_rest Hse_P') as [P_rest' HP_rest'].
    exists P_rest'.
    eapply se_trans; [apply se_sym; exact H0 | exact HP_rest'].
  - (* rs_replicate: S = PReplicate P, R = PPar P (PReplicate P).
       Hse : PReplicate P ≡ PPar (PReplicate body) P_rest.
       head_count LHS = 1, RHS = 1 + head_count P_rest. So head_count P_rest = 0,
       hence P_rest ≡ PNil. Then PReplicate P ≡ PReplicate body via se_par_nil
       + sym, and se_PReplicate_inj gives P ≡ body. *)
    pose proof (head_count_se _ _ Hse) as Hhc.
    simpl in Hhc.
    assert (HcP_rest : head_count P_rest = 0) by lia.
    assert (HP_rest_nil : P_rest ≡ PNil) by
      (apply head_count_zero_se_nil; assumption).
    assert (HPb : PReplicate P ≡ PReplicate body).
    { eapply se_trans; [exact Hse |].
      eapply se_trans; [apply se_par_cong; [apply se_refl | exact HP_rest_nil] |].
      apply se_par_nil. }
    assert (HPb_eq : P ≡ body) by (apply se_PReplicate_inj; exact HPb).
    exists P.
    (* Goal: PPar P (PReplicate P) ≡ PPar (PReplicate body) P. *)
    eapply se_trans; [apply se_par_comm |].
    apply se_par_cong; [apply se_replicate_cong; exact HPb_eq | apply se_refl].
Qed.

(* Iterated version: reachability preserves the PReplicate body factor. *)
Lemma reachable_PPar_PReplicate_inv_se : forall S Q,
  rho_reachable S Q ->
  forall body P_rest,
    S ≡ PPar (PReplicate body) P_rest ->
    exists P_rest', Q ≡ PPar (PReplicate body) P_rest'.
Proof.
  intros S Q Hreach. induction Hreach; intros body P_rest Hse.
  - (* rr_refl: Q = S. Use P_rest itself. *)
    exists P_rest. exact Hse.
  - (* rr_step: S ⇝ Q0 ⇝* R. Apply step-inversion on S ⇝ Q0, then recurse. *)
    destruct (step_PPar_PReplicate_inv_se _ _ H body P_rest Hse)
      as [Q0_rest HQ0_se].
    apply (IHHreach body Q0_rest HQ0_se).
Qed.

(* ───────────────────────────────────────────────────────────────────────
   Section 14.D: Closed verification boundary
   ───────────────────────────────────────────────────────────────────────

   The mechanized result in this file is the axiom-free forward barb
   propagation theorem [preplicate_bang_encoding_body_barbs_sound]: every
   weak input or output barb already available from the replicated body is
   also available from both wrappers, [PReplicate P] and [bang_encoding x P].

   We do not state a projection from all weak barbs of [PReplicate P] back
   to a single copy of [P]. That projection is stronger than the standard
   replication law [!P ~ P | !P]: multiple copies of a nondeterministic
   body may expose combined weak behavior that no one copy exposes alone.
   Removing that projection keeps the formal development axiom-free and
   avoids presenting a false strengthening as a theorem.                    *)

Theorem replication_encoding_forward_barb_sound :
  forall x P y,
    closed_name x -> closed_proc P ->
    (weak_barb_input P y ->
       weak_barb_input (PReplicate P) y /\
       weak_barb_input (bang_encoding x P) y) /\
    (weak_barb_output P y ->
       weak_barb_output (PReplicate P) y /\
       weak_barb_output (bang_encoding x P) y).
Proof.
  intros x P y Hxcl HPcl.
  apply preplicate_bang_encoding_body_barbs_sound; assumption.
Qed.
