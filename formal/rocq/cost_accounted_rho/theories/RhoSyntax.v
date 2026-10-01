(* ═══════════════════════════════════════════════════════════════════════════
   RhoSyntax.v — Foundational module for the pure rho calculus
   ═══════════════════════════════════════════════════════════════════════════

   Formalizes the syntax, substitution, and structural equivalence of the
   reflective higher-order process calculus (rho calculus).

   The rho calculus is distinguished by its reflective nature: names are
   either quoted processes (@P) or bound name variables (introduced by the
   input prefix), and processes can dereference names ( *x ) to obtain the
   process they represent. This creates a symmetric relationship between
   names and processes captured by the mutual induction between the [name]
   and [proc] types.

   Binding representation: locally nameless with de Bruijn indices for
   bound name variables. The input prefix [PInput x P] binds the name
   variable that occurs as [NVar 0] in [P].

   Substitution: when the COMM rule fires, the bound name variable in the
   receiver's body is replaced with the name that was sent. Substitution
   is therefore NAME-substitution: subst_proc P 0 N replaces every NVar 0
   in P with the name N (lifted appropriately when crossing binders).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Paper Notation        │ Rust Implementation
   ─────────────────────────┼───────────────────────┼─────────────────────
   Quote P                  │ @P (quotation)        │ Par (quoted name)
   NVar k                   │ y_k (bound name)      │ VarInstance::BoundVar
   PNil                     │ 0 (stopped process)   │ Par::default()
   PInput x P               │ for(y ← x){P}        │ Receive
   POutput x Q              │ x!(Q)                 │ Send
   PPar P Q                 │ P | Q                 │ Par.procs
   PDeref x                 │ *x (dereference)      │ Eval (name deref)
   PReplicate P              │ !P (replication)      │ (replicated proc)
   lift_proc                │ ↑^d_c(P)             │ (implicit in Env)
   subst_proc               │ P{N/y_n}              │ Substitute trait
   struct_equiv             │ P ≡ Q                 │ (normalized form)
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.1 stdlib
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Setoid.
From Stdlib Require Import Morphisms.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Core Syntax — Mutual Inductive Types
   ═══════════════════════════════════════════════════════════════════════════

   The rho calculus syntax is defined by two mutually recursive types:
   - [name]: channel names, either quoted processes or bound variables
   - [proc]: processes, the units of concurrent computation                  *)

Inductive name : Type :=
  | Quote : proc -> name        (* @P — quotation: turn a process into a name *)
  | NVar  : nat -> name         (* bound name variable (de Bruijn index) *)
with proc : Type :=
  | PNil    : proc              (* 0 — the stopped process *)
  | PInput  : name -> proc -> proc
      (* for(y <- x){P} — input: receive on channel x, binding the
         received name as NVar 0 in P *)
  | POutput : name -> proc -> proc
      (* x!(Q) — output: send process Q on channel x *)
  | PPar    : proc -> proc -> proc
      (* P | Q — parallel composition *)
  | PDeref  : name -> proc      (* *x — dereference: turn a name back into a process *)
  | PReplicate : proc -> proc.  (* !P — replication: infinite parallel copies of P

     NOTE (two-lens design). [PReplicate] is retained as a primitive
     constructor because Rholang's [contract x(y) = { P }] compiles
     to a persistent-receive runtime node ([Receive { persistent :=
     true }]), matching the semantics of [PReplicate (PInput x P)]
     directly. The paper's §5 Remark (cost-accounted-rho.tex,
     lines 540–545) instead cites the standard reflection-based
     encoding (cf. Meredith-Radestock 2005):

         bang_encoding c P :=
             c ! @P  ||  for t <- c do P || * t || c ! * t end

     Module [Replication.v] defines [bang_encoding], proves its one-step
     operational unfold, and proves the axiom-free forward weak-barb
     result that every weak input/output barb of [P] propagates to both
     [PReplicate P] and [bang_encoding c P]. This lets the mechanization
     simultaneously match Rholang's primitive-replication runtime and
     the paper's reflective encoding without assuming a bidirectional
     equivalence between the two wrappers. *)

(* Generate mutual induction schemes for [proc] and [name]. *)
Scheme proc_ind_mut := Induction for proc Sort Prop
  with name_ind_mut := Induction for name Sort Prop.

Combined Scheme proc_name_mutind from proc_ind_mut, name_ind_mut.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Decidable Equality
   ═══════════════════════════════════════════════════════════════════════════ *)

Fixpoint proc_eq_dec (P Q : proc) : {P = Q} + {P <> Q}
with name_eq_dec (x y : name) : {x = y} + {x <> y}.
Proof.
  - decide equality.
  - decide equality. apply Nat.eq_dec.
Defined.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Lifting (Shifting) de Bruijn Indices
   ═══════════════════════════════════════════════════════════════════════════

   Lifting (also called shifting) adjusts de Bruijn indices when moving a
   term under additional binders. [lift_proc d c P] increments all NVar
   indices >= c (the cutoff) by d (the shift amount).

   Parameters:
   - d: the shift amount (how many new binders we are crossing)
   - c: the cutoff (NVar indices below c are bound and should not shift) *)

Fixpoint lift_proc (d c : nat) (P : proc) : proc :=
  match P with
  | PNil         => PNil
  | PInput x P'  => PInput (lift_name d c x) (lift_proc d (S c) P')
  | POutput x Q  => POutput (lift_name d c x) (lift_proc d c Q)
  | PPar P' Q    => PPar (lift_proc d c P') (lift_proc d c Q)
  | PDeref x     => PDeref (lift_name d c x)
  | PReplicate P' => PReplicate (lift_proc d c P')
  end
with lift_name (d c : nat) (x : name) : name :=
  match x with
  | Quote P => Quote (lift_proc d c P)
  | NVar k  => if c <=? k then NVar (k + d) else NVar k
  end.

Lemma lift_zero_proc : forall P c, lift_proc 0 c P = P.
Proof.
  intro P.
  apply (proc_ind_mut
    (fun P => forall c, lift_proc 0 c P = P)
    (fun x => forall c, lift_name 0 c x = x));
    intros; simpl.
  - (* PNil *) reflexivity.
  - (* PInput *) rewrite H. rewrite H0. reflexivity.
  - (* POutput *) rewrite H. rewrite H0. reflexivity.
  - (* PPar *) rewrite H. rewrite H0. reflexivity.
  - (* PDeref *) rewrite H. reflexivity.
  - (* PReplicate *) rewrite H. reflexivity.
  - (* Quote *) rewrite H. reflexivity.
  - (* NVar *) destruct (c <=? n) eqn:Hcn.
    + rewrite Nat.add_0_r. reflexivity.
    + reflexivity.
Qed.

Lemma lift_zero_name : forall x c, lift_name 0 c x = x.
Proof.
  destruct x as [P | k]; simpl; intros.
  - rewrite lift_zero_proc. reflexivity.
  - destruct (c <=? k) eqn:Hck.
    + rewrite Nat.add_0_r. reflexivity.
    + reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Substitution
   ═══════════════════════════════════════════════════════════════════════════

   [subst_proc P n N] replaces NVar n in P with the name N (suitably
   lifted when going under binders). This corresponds to the paper's
   P{N/y_n} where y_n is a bound name variable and N is the substituent.

   The COMM rule (in RhoReduction.v) instantiates N with [Quote Q] for
   the sent process Q.

   Substitution is *semantic* in the sense of [4, §2.4 Remark]: when
   the substituted-for variable appears under a dequote and is being
   replaced with a quoted process, the dequote–quote pair collapses at
   the substitution site. Concretely, the paper's rule
     "drop y then apply-subst @Q for y" = Q
   fires only at the one syntactic location where the bound variable y
   appears directly under a dequote; in all other positions the dequote
   stays as-is. This is the minimal semantics needed to make the
   paper's reflection-based encoding of replication self-regenerate
   after one COMM step, and it keeps structural equivalence [≡]
   untouched (so the [N_tr] injectivity chain in
   TranslationFaithfulness.v is preserved — see the working notes in
   /var/tmp/r1_axiom_attempt_2026-04-14/README.md for the
   counter-example that an axiom-level collapse would have broken).

   Outside the specific case where [PDeref (NVar n)] meets a
   substitutand [Quote Q], the new substitution is pointwise identical
   to the old syntactic one. *)

Fixpoint subst_proc (P : proc) (n : nat) (N : name) : proc :=
  match P with
  | PNil         => PNil
  | PInput x P'  =>
      PInput (subst_name x n N) (subst_proc P' (S n) (lift_name 1 0 N))
  | POutput x R  =>
      POutput (subst_name x n N) (subst_proc R n N)
  | PPar P1 P2   =>
      PPar (subst_proc P1 n N) (subst_proc P2 n N)
  | PDeref x     =>
      match x with
      | NVar k =>
          match Nat.compare k n with
          | Lt => PDeref (NVar k)
          | Eq =>
              match N with
              | Quote Q  => Q              (* semantic collapse *)
              | NVar _   => PDeref N       (* no collapse: substitutand
                                              is a bound-variable name,
                                              not a quoted process *)
              end
          | Gt => PDeref (NVar (k - 1))
          end
      | Quote P' => PDeref (Quote (subst_proc P' n N))
      end
  | PReplicate P' =>
      PReplicate (subst_proc P' n N)
  end
with subst_name (x : name) (n : nat) (N : name) : name :=
  match x with
  | Quote P => Quote (subst_proc P n N)
  | NVar k  =>
      match Nat.compare k n with
      | Lt => NVar k
      | Eq => N
      | Gt => NVar (k - 1)
      end
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4b: Lift-Substitution Cancellation
   ═══════════════════════════════════════════════════════════════════════════

   The key lemma that makes the fuel-gate simulation work:
   substituting at index 0 in a process that has been lifted by 1 with
   cutoff 0 yields the original process. This is because lifting by 1
   from cutoff 0 shifts every NVar k to NVar (k+1), so no NVar 0 remains
   in the lifted term. Substitution for NVar 0 then has nothing to do.

   The lemma is mutual: it holds simultaneously on processes and names.
   The substitution function recurses under PInput binders by shifting
   the substitution index up; the lift function recurses by shifting
   the cutoff up. The interaction is captured by a generalized version
   over arbitrary cutoff [c]: substituting at index [c] in [lift_proc 1 c P]
   yields [P].                                                              *)

Lemma subst_lift_cancel :
  forall P c N,
    subst_proc (lift_proc 1 c P) c N = P.
Proof.
  intro P.
  apply (proc_ind_mut
    (fun P => forall c N, subst_proc (lift_proc 1 c P) c N = P)
    (fun x => forall c N, subst_name (lift_name 1 c x) c N = x));
    intros; simpl.
  - (* PNil *) reflexivity.
  - (* PInput *) rewrite H. rewrite H0. reflexivity.
  - (* POutput *) rewrite H. rewrite H0. reflexivity.
  - (* PPar *) rewrite H. rewrite H0. reflexivity.
  - (* PDeref *)
    (* Under the new semantic [subst_proc], the [PDeref] case case-
       analyses on the name shape; we mirror that split here. H is the
       name-level IH. *)
    destruct n as [Pi | k]; simpl.
    + (* Quote Pi: subst_proc recurses into Pi in the Quote branch; the
         name-level IH, after simpl, collapses the inner substitution
         back to Pi. *)
      specialize (H c N). simpl in H. injection H as Hinner.
      rewrite Hinner. reflexivity.
    + (* NVar k: after lifting by 1 with cutoff c, the resulting NVar
         index is either [k+1] (when c <= k) or [k] (when c > k). In
         both cases the lifted index is distinct from c, so
         [Nat.compare] fires Gt or Lt — never Eq — and the semantic
         collapse never triggers. *)
      destruct (c <=? k) eqn:Hck; simpl.
      * apply Nat.leb_le in Hck.
        assert (Hgt : Nat.compare (k + 1) c = Gt)
          by (apply Nat.compare_gt_iff; lia).
        rewrite Hgt. f_equal. f_equal. lia.
      * apply Nat.leb_gt in Hck.
        assert (Hlt : Nat.compare k c = Lt)
          by (apply Nat.compare_lt_iff; lia).
        rewrite Hlt. reflexivity.
  - (* PReplicate *) rewrite H. reflexivity.
  - (* Quote *) rewrite H. reflexivity.
  - (* NVar k *)
    destruct (c <=? n) eqn:Hcn.
    + (* c <= n: lift becomes NVar (n + 1).
         compare (n + 1) c: since c <= n < n+1, we have n+1 > c,
         so Nat.compare (n+1) c = Gt, returning NVar (n+1-1) = NVar n. *)
      simpl. apply Nat.leb_le in Hcn.
      assert (Hgt: Nat.compare (n + 1) c = Gt).
      { apply Nat.compare_gt_iff. lia. }
      rewrite Hgt. f_equal. lia.
    + (* c > n: lift leaves NVar n unchanged.
         compare n c: since n < c, we have Nat.compare n c = Lt,
         returning NVar n. *)
      simpl. apply Nat.leb_gt in Hcn.
      assert (Hlt: Nat.compare n c = Lt).
      { apply Nat.compare_lt_iff. assumption. }
      rewrite Hlt. reflexivity.
Qed.

(* The convenient corollary at cutoff 0. *)
Lemma subst_lift_zero : forall P N,
  subst_proc (lift_proc 1 0 P) 0 N = P.
Proof.
  intros. apply subst_lift_cancel.
Qed.

(* The same for names. *)
Lemma subst_lift_zero_name : forall x N,
  subst_name (lift_name 1 0 x) 0 N = x.
Proof.
  intro x. destruct x as [P | k]; intro N; simpl.
  - rewrite subst_lift_zero. reflexivity.
  - destruct (0 <=? k) eqn:H0k.
    + assert (Hgt: Nat.compare (k + 1) 0 = Gt).
      { apply Nat.compare_gt_iff. lia. }
      rewrite Hgt. f_equal. lia.
    + apply Nat.leb_gt in H0k. lia.
Qed.

(* Substitution distributes through PPar (defining clause). *)
Lemma subst_proc_par : forall P Q n N,
  subst_proc (PPar P Q) n N = PPar (subst_proc P n N) (subst_proc Q n N).
Proof. reflexivity. Qed.

(* Substitution on [PDeref] under semantic substitution: collapses when
   the substituted-for index lines up with a dequoted bound variable
   AND the substitutand is a [Quote Q]. The four cases below are the
   mechanical rewrite lemmas for each branch of the semantic definition;
   they replace the single old unconditional [subst_proc_deref] which
   is no longer a theorem (false when [x = NVar n, N = Quote Q]). *)

Lemma subst_proc_deref_quote : forall P n N,
  subst_proc (PDeref (Quote P)) n N
    = PDeref (Quote (subst_proc P n N)).
Proof. reflexivity. Qed.

Lemma subst_proc_deref_nvar_lt : forall k n N,
  k < n ->
  subst_proc (PDeref (NVar k)) n N = PDeref (NVar k).
Proof.
  intros k n N H. simpl.
  (* Nat.compare_spec yields Eq, Lt, Gt in that order. *)
  destruct (Nat.compare_spec k n); [ lia | reflexivity | lia ].
Qed.

Lemma subst_proc_deref_nvar_gt : forall k n N,
  k > n ->
  subst_proc (PDeref (NVar k)) n N = PDeref (NVar (k - 1)).
Proof.
  intros k n N H. simpl.
  destruct (Nat.compare_spec k n); [ lia | lia | reflexivity ].
Qed.

(* The semantic-collapse rewrite: exactly at the binder, substituting a
   quoted process drops the dequote. *)
Lemma subst_proc_deref_nvar_eq_quote : forall n Q,
  subst_proc (PDeref (NVar n)) n (Quote Q) = Q.
Proof.
  intros n Q. simpl. rewrite Nat.compare_refl. reflexivity.
Qed.

(* Fallback when the substitutand is a bound-variable name (not a
   quoted process): no collapse, just produce [PDeref N]. *)
Lemma subst_proc_deref_nvar_eq_nvar : forall n k,
  subst_proc (PDeref (NVar n)) n (NVar k) = PDeref (NVar k).
Proof.
  intros n k. simpl. rewrite Nat.compare_refl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4c: Generalized Lift-Substitution Cancellation
   ═══════════════════════════════════════════════════════════════════════════

   The key fact for compound-signature simulations: when a term has been
   lifted by [d+1] from cutoff [c], substituting at index [k] for any
   [k] satisfying [c <= k < c + d + 1] cancels exactly one of the lifts,
   yielding [lift_proc d c P]. This is the standard "lift-subst commutation"
   lemma in locally-nameless formalizations.

   The previous [subst_lift_cancel] is the special case [d = 0, k = c].   *)

Lemma subst_lift_strong :
  forall P d c k N,
    c <= k -> k < c + S d ->
    subst_proc (lift_proc (S d) c P) k N = lift_proc d c P.
Proof.
  intro P.
  apply (proc_ind_mut
    (fun P => forall d c k N,
      c <= k -> k < c + S d ->
      subst_proc (lift_proc (S d) c P) k N = lift_proc d c P)
    (fun x => forall d c k N,
      c <= k -> k < c + S d ->
      subst_name (lift_name (S d) c x) k N = lift_name d c x));
    intros.
  - (* PNil *) reflexivity.
  - (* PInput *)
    simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* POutput *)
    simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* PPar *)
    simpl. rewrite H by lia. rewrite H0 by lia. reflexivity.
  - (* PDeref *)
    (* Under semantic subst, [subst_proc (PDeref _)] case-analyses on
       the name shape. Split. *)
    destruct n as [Pi | k0].
    + (* Quote Pi: recurses into Pi; the name-level IH after simpl
         gives the inner equality. *)
      simpl.
      specialize (H d c k N H0 H1). simpl in H. injection H as Hinner.
      rewrite Hinner. reflexivity.
    + (* NVar k0: after the S d lift with cutoff c, the resulting
         index is [k0 + S d] (if c <= k0) or [k0] (otherwise). With
         c <= k < c + S d, compare gives Lt or Gt — never Eq — so the
         semantic collapse never triggers. *)
      simpl.
      destruct (c <=? k0) eqn:Hck0.
      * apply Nat.leb_le in Hck0.
        assert (Hgt : Nat.compare (k0 + S d) k = Gt)
          by (apply Nat.compare_gt_iff; lia).
        rewrite Hgt.
        destruct (c <=? k0) eqn:Hck0'; [| apply Nat.leb_gt in Hck0'; lia].
        f_equal. f_equal. lia.
      * apply Nat.leb_gt in Hck0.
        assert (Hlt : Nat.compare k0 k = Lt)
          by (apply Nat.compare_lt_iff; lia).
        rewrite Hlt.
        destruct (c <=? k0) eqn:Hck0'; [apply Nat.leb_le in Hck0'; lia |].
        reflexivity.
  - (* PReplicate *)
    simpl. rewrite H by lia. reflexivity.
  - (* Quote *)
    simpl. rewrite H by lia. reflexivity.
  - (* NVar n *)
    (* Fully unfolded case analysis on c <=? n. *)
    destruct (Nat.leb_spec c n) as [Hcn | Hcn].
    + (* c <= n *)
      assert (Hcn_eq : (c <=? n) = true) by (apply Nat.leb_le; assumption).
      cbn [lift_name]. rewrite Hcn_eq.
      cbn [subst_name].
      assert (Hgt: Nat.compare (n + S d) k = Gt).
      { apply Nat.compare_gt_iff. lia. }
      rewrite Hgt.
      f_equal. lia.
    + (* n < c *)
      assert (Hcn_eq : (c <=? n) = false) by (apply Nat.leb_gt; assumption).
      cbn [lift_name]. rewrite Hcn_eq.
      cbn [subst_name].
      assert (Hlt: Nat.compare n k = Lt).
      { apply Nat.compare_lt_iff. lia. }
      rewrite Hlt.
      reflexivity.
Qed.

(* The specific case we need most often: lift by 2, substitute at 1, get
   lift by 1. Used in compound P_tr simulation. *)
Lemma subst_lift_two_one : forall P N,
  subst_proc (lift_proc 2 0 P) 1 N = lift_proc 1 0 P.
Proof.
  intros.
  apply (subst_lift_strong P 1 0 1 N); lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4d: Closed Process Predicate
   ═══════════════════════════════════════════════════════════════════════════

   A process is "closed at level k" when no NVar j with j >= k appears
   free. The predicate [closed_proc P := closed_proc_at 0 P] captures
   "no free NVars at all". Closed processes are invariant under both
   substitution and lifting. We use this for the translation of tokens
   and signature names, both of which contain only Quote-wrapped
   constructions and never reference outer-scope NVars.                  *)

Fixpoint closed_proc_at (k : nat) (P : proc) : Prop :=
  match P with
  | PNil          => True
  | PInput x P'   => closed_name_at k x /\ closed_proc_at (S k) P'
  | POutput x Q   => closed_name_at k x /\ closed_proc_at k Q
  | PPar P1 P2    => closed_proc_at k P1 /\ closed_proc_at k P2
  | PDeref x      => closed_name_at k x
  | PReplicate P' => closed_proc_at k P'
  end
with closed_name_at (k : nat) (x : name) : Prop :=
  match x with
  | Quote P => closed_proc_at k P
  | NVar j  => j < k
  end.

Definition closed_proc (P : proc) : Prop := closed_proc_at 0 P.
Definition closed_name (x : name) : Prop := closed_name_at 0 x.

(* Closedness is monotone: closed at k implies closed at any k' >= k. *)
Lemma closed_proc_at_mono : forall P k k',
  k <= k' -> closed_proc_at k P -> closed_proc_at k' P.
Proof.
  intro P.
  apply (proc_ind_mut
    (fun P => forall k k', k <= k' -> closed_proc_at k P -> closed_proc_at k' P)
    (fun x => forall k k', k <= k' -> closed_name_at k x -> closed_name_at k' x));
    intros; simpl in *.
  - (* PNil *) exact I.
  - (* PInput *)
    destruct H2.
    split; [eapply H; eauto | eapply H0; [|eassumption]; lia].
  - (* POutput *)
    destruct H2.
    split; [eapply H; eauto | eapply H0; eauto].
  - (* PPar *)
    destruct H2.
    split; [eapply H; eauto | eapply H0; eauto].
  - (* PDeref *)
    eapply H; eauto.
  - (* PReplicate *)
    eapply H; eauto.
  - (* Quote *)
    eapply H; eauto.
  - (* NVar *)
    lia.
Qed.

(* Substitution is identity on closed processes. *)
Lemma closed_proc_subst : forall P k N,
  closed_proc_at k P -> subst_proc P k N = P.
Proof.
  intro P.
  apply (proc_ind_mut
    (fun P => forall k N, closed_proc_at k P -> subst_proc P k N = P)
    (fun x => forall k N, closed_name_at k x -> subst_name x k N = x));
    intros; simpl in *.
  - (* PNil *) reflexivity.
  - (* PInput *)
    destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* POutput *)
    destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* PPar *)
    destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* PDeref *)
    (* Under semantic subst, case-split on the name. *)
    destruct n as [Pi | j]; simpl in *.
    + (* Quote Pi: the name-level IH gives
         subst_name (Quote Pi) k N = Quote Pi, which implies
         subst_proc Pi k N = Pi. *)
      specialize (H k N H0). simpl in H. injection H as Hinner.
      rewrite Hinner. reflexivity.
    + (* NVar j: j < k (from closed_name_at), so compare j k = Lt. *)
      assert (Hlt : Nat.compare j k = Lt)
        by (apply Nat.compare_lt_iff; assumption).
      rewrite Hlt. reflexivity.
  - (* PReplicate *)
    rewrite H by assumption. reflexivity.
  - (* Quote *)
    rewrite H by assumption. reflexivity.
  - (* NVar *)
    assert (Hlt : Nat.compare n k = Lt).
    { apply Nat.compare_lt_iff. assumption. }
    rewrite Hlt. reflexivity.
Qed.

Lemma closed_name_subst : forall x k N,
  closed_name_at k x -> subst_name x k N = x.
Proof.
  intros x k N H.
  destruct x as [P | j]; simpl in *.
  - rewrite closed_proc_subst by assumption. reflexivity.
  - assert (Hlt : Nat.compare j k = Lt).
    { apply Nat.compare_lt_iff. assumption. }
    rewrite Hlt. reflexivity.
Qed.

(* Lifting is identity on closed processes. We need closed_proc_at c P
   for the lift to leave it alone (cutoff matches). *)
Lemma closed_proc_lift : forall P d c,
  closed_proc_at c P -> lift_proc d c P = P.
Proof.
  intro P.
  apply (proc_ind_mut
    (fun P => forall d c, closed_proc_at c P -> lift_proc d c P = P)
    (fun x => forall d c, closed_name_at c x -> lift_name d c x = x));
    intros; simpl in *.
  - (* PNil *) reflexivity.
  - (* PInput *)
    destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* POutput *)
    destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* PPar *)
    destruct H1. rewrite H by assumption. rewrite H0 by assumption. reflexivity.
  - (* PDeref *)
    rewrite H by assumption. reflexivity.
  - (* PReplicate *)
    rewrite H by assumption. reflexivity.
  - (* Quote *)
    rewrite H by assumption. reflexivity.
  - (* NVar *)
    assert (Hleb : c <=? n = false).
    { apply Nat.leb_gt. assumption. }
    rewrite Hleb. reflexivity.
Qed.

Lemma closed_name_lift : forall x d c,
  closed_name_at c x -> lift_name d c x = x.
Proof.
  intros x d c H.
  destruct x as [P | j]; simpl in *.
  - rewrite closed_proc_lift by assumption. reflexivity.
  - assert (Hleb : c <=? j = false).
    { apply Nat.leb_gt. assumption. }
    rewrite Hleb. reflexivity.
Qed.

(* Convenient closed-at-0 corollaries used by the translation modules. *)
Lemma closed_proc_subst_zero : forall P k N,
  closed_proc P -> subst_proc P k N = P.
Proof.
  unfold closed_proc. intros.
  apply closed_proc_subst.
  eapply closed_proc_at_mono; [| eassumption]. lia.
Qed.

Lemma closed_proc_lift_zero : forall P d c,
  closed_proc P -> lift_proc d c P = P.
Proof.
  unfold closed_proc. intros.
  apply closed_proc_lift.
  eapply closed_proc_at_mono; [| eassumption]. lia.
Qed.

Lemma closed_name_subst_zero : forall x k N,
  closed_name x -> subst_name x k N = x.
Proof.
  unfold closed_name. intros.
  apply closed_name_subst.
  destruct x as [P | j]; simpl in *.
  - eapply closed_proc_at_mono; [| eassumption]. lia.
  - lia.
Qed.

Lemma closed_name_lift_zero : forall x d c,
  closed_name x -> lift_name d c x = x.
Proof.
  unfold closed_name. intros.
  apply closed_name_lift.
  destruct x as [P | j]; simpl in *.
  - eapply closed_proc_at_mono; [| eassumption]. lia.
  - lia.
Qed.

(* PNil is closed. *)
Lemma closed_PNil : closed_proc PNil.
Proof. unfold closed_proc. simpl. exact I. Qed.

(* PDeref of a closed name is a closed proc. *)
Lemma closed_PDeref : forall x, closed_name x -> closed_proc (PDeref x).
Proof. unfold closed_proc, closed_name. intros. simpl. assumption. Qed.

(* Quote of a closed proc is a closed name. *)
Lemma closed_Quote : forall P, closed_proc P -> closed_name (Quote P).
Proof. unfold closed_proc, closed_name. intros. simpl. assumption. Qed.

(* PPar of two closed processes is closed. *)
Lemma closed_PPar : forall P Q,
  closed_proc P -> closed_proc Q -> closed_proc (PPar P Q).
Proof. unfold closed_proc. intros. simpl. split; assumption. Qed.

(* POutput of a closed name and a closed payload is closed. *)
Lemma closed_POutput : forall x P,
  closed_name x -> closed_proc P -> closed_proc (POutput x P).
Proof. unfold closed_proc, closed_name. intros. simpl. split; assumption. Qed.

(* Monotonicity of closed_name_at — derived from the mutual induction in
   closed_proc_at_mono via the (Quote/NVar) case analysis. Useful when
   building closedness proofs that need to lift name closedness across
   binder boundaries. *)
Lemma closed_name_at_mono : forall x k k',
  k <= k' -> closed_name_at k x -> closed_name_at k' x.
Proof.
  intros x k k' Hle Hclosed. destruct x as [P | j]; simpl in *.
  - eapply closed_proc_at_mono; eauto.
  - lia.
Qed.

(* PInput closedness: a PInput is closed iff its channel is a closed name
   and its body is closed at level [S 0] = 1 (i.e., the binder eats one
   level of de Bruijn index). *)
Lemma closed_PInput : forall x P,
  closed_name x -> closed_proc_at 1 P -> closed_proc (PInput x P).
Proof. unfold closed_proc, closed_name. intros. simpl. split; assumption. Qed.

(* Convenience: a [PDeref (Quote P)] for closed [P] is itself closed. This
   is the residue shape produced by gate firings (a token's payload becomes
   [PDeref (Quote payload)] after the COMM substitution). Proves to a 1-line
   discharge of closedness obligations in the compound rule simulations. *)
Lemma closed_PDeref_Quote : forall P,
  closed_proc P -> closed_proc (PDeref (Quote P)).
Proof. intros. apply closed_PDeref. apply closed_Quote. assumption. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Structural Equivalence
   ═══════════════════════════════════════════════════════════════════════════

   Structural equivalence (≡) identifies processes that are considered
   identical modulo the algebraic laws of parallel composition and
   congruence under all process constructors.

   - PPar is commutative, associative, with PNil as the identity
   - Reflexivity, symmetry, transitivity (an equivalence relation)
   - Congruence under all process and name constructors                     *)

Reserved Notation "P ≡ Q" (at level 70, no associativity).
Reserved Notation "x ≡N y" (at level 70, no associativity).

Inductive struct_equiv : proc -> proc -> Prop :=
  (* --- Equivalence relation --- *)
  | se_refl  : forall P, P ≡ P
  | se_sym   : forall P Q, P ≡ Q -> Q ≡ P
  | se_trans : forall P Q R, P ≡ Q -> Q ≡ R -> P ≡ R

  (* --- Parallel composition is a commutative monoid --- *)
  | se_par_comm  : forall P Q, PPar P Q ≡ PPar Q P
  | se_par_assoc : forall P Q R,
      PPar (PPar P Q) R ≡ PPar P (PPar Q R)
  | se_par_nil   : forall P, PPar P PNil ≡ P

  (* --- Congruence rules --- *)
  | se_par_cong    : forall P P' Q Q',
      P ≡ P' -> Q ≡ Q' -> PPar P Q ≡ PPar P' Q'
  | se_input_cong  : forall x x' P P',
      x ≡N x' -> P ≡ P' -> PInput x P ≡ PInput x' P'
  | se_output_cong : forall x x' Q Q',
      x ≡N x' -> Q ≡ Q' -> POutput x Q ≡ POutput x' Q'
  | se_deref_cong  : forall x x',
      x ≡N x' -> PDeref x ≡ PDeref x'
  | se_replicate_cong : forall P P',
      P ≡ P' -> PReplicate P ≡ PReplicate P'
where "P ≡ Q" := (struct_equiv P Q)
with struct_equiv_name : name -> name -> Prop :=
  | se_name_quote : forall P P',
      P ≡ P' -> Quote P ≡N Quote P'
  | se_name_var_refl : forall k, NVar k ≡N NVar k
where "x ≡N y" := (struct_equiv_name x y).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Basic Equivalence Lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma se_name_refl : forall x, x ≡N x.
Proof.
  destruct x as [P | k].
  - constructor. apply se_refl.
  - apply se_name_var_refl.
Qed.

Lemma se_name_sym : forall x y, x ≡N y -> y ≡N x.
Proof.
  intros x y H. inversion H; subst.
  - constructor. apply se_sym. assumption.
  - apply se_name_var_refl.
Qed.

Lemma se_name_trans : forall x y z, x ≡N y -> y ≡N z -> x ≡N z.
Proof.
  intros x y z Hxy Hyz.
  inversion Hxy; subst; inversion Hyz; subst.
  - constructor. eapply se_trans; eauto.
  - apply se_name_var_refl.
Qed.

Lemma se_nil_par : forall P, PPar PNil P ≡ P.
Proof.
  intro.
  apply (se_trans _ (PPar P PNil) _).
  - apply se_par_comm.
  - apply se_par_nil.
Qed.

Lemma se_par_cong_l : forall P P' Q, P ≡ P' -> PPar P Q ≡ PPar P' Q.
Proof.
  intros. apply se_par_cong; [assumption | apply se_refl].
Qed.

Lemma se_par_cong_r : forall P Q Q', Q ≡ Q' -> PPar P Q ≡ PPar P Q'.
Proof.
  intros. apply se_par_cong; [apply se_refl | assumption].
Qed.

(* Structural rearrangement helpers used pervasively in the cost-accounted
   rule simulations. These pack the 4-element commutative-monoid
   gymnastics into single named lemmas. *)

(* 4-element cross swap: (A | B) | (C | D) ≡ (A | C) | (B | D). *)
Lemma se_par_cross : forall A B C D,
  PPar (PPar A B) (PPar C D) ≡ PPar (PPar A C) (PPar B D).
Proof.
  intros A B C D.
  (* (A | B) | (C | D) ≡ A | (B | (C | D))    by se_par_assoc
                       ≡ A | ((B | C) | D)    by se_par_assoc^-1
                       ≡ A | ((C | B) | D)    by se_par_comm
                       ≡ A | (C | (B | D))    by se_par_assoc
                       ≡ (A | C) | (B | D)    by se_par_assoc^-1 *)
  apply (se_trans _ (PPar A (PPar B (PPar C D)))).
  { apply se_par_assoc. }
  apply (se_trans _ (PPar A (PPar (PPar B C) D))).
  { apply se_par_cong_r. apply se_sym, se_par_assoc. }
  apply (se_trans _ (PPar A (PPar (PPar C B) D))).
  { apply se_par_cong_r. apply se_par_cong_l. apply se_par_comm. }
  apply (se_trans _ (PPar A (PPar C (PPar B D)))).
  { apply se_par_cong_r. apply se_par_assoc. }
  apply se_sym, se_par_assoc.
Qed.

(* Right-rotate triple: A | (B | C) ≡ B | (A | C). *)
Lemma se_par_rotr : forall A B C,
  PPar A (PPar B C) ≡ PPar B (PPar A C).
Proof.
  intros A B C.
  apply (se_trans _ (PPar (PPar A B) C)).
  { apply se_sym, se_par_assoc. }
  apply (se_trans _ (PPar (PPar B A) C)).
  { apply se_par_cong_l. apply se_par_comm. }
  apply se_par_assoc.
Qed.

(* Bring the rightmost element to the front: ((A | B) | C) ≡ C | (A | B). *)
Lemma se_par_swap_left : forall A B C,
  PPar (PPar A B) C ≡ PPar C (PPar A B).
Proof.
  intros. apply se_par_comm.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 7: Setoid Registration
   ═══════════════════════════════════════════════════════════════════════════ *)

Add Parametric Relation : proc struct_equiv
  reflexivity proved by se_refl
  symmetry proved by se_sym
  transitivity proved by se_trans
  as struct_equiv_rel.

Add Parametric Relation : name struct_equiv_name
  reflexivity proved by se_name_refl
  symmetry proved by se_name_sym
  transitivity proved by se_name_trans
  as struct_equiv_name_rel.

Add Parametric Morphism : PPar with signature
  struct_equiv ==> struct_equiv ==> struct_equiv as PPar_morphism.
Proof. intros. apply se_par_cong; assumption. Qed.
