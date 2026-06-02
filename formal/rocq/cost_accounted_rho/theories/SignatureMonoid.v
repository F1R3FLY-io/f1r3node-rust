(* ════════════════════════════════════════════════════════════════════════
   SignatureMonoid.v — the two monoids the Cost monad descends from (CL2).

   The monad paper (continued-gslt-cost-v2.tex) proves the cost monad's laws
   "descend from the laws of the two constituent monoids" (Prop "the cost
   monad", :1064-1071): the SIGNATURE commutative monoid (Sig, *, ()) compounding
   signatures, and the TEMPORAL token-stack FREE monoid (cons, ++, ()) — "a free
   monoid (a list), never commutative" (:523). This module supplies both
   natively over [CostAccountedSyntax]'s [sig] (with [*]=[SAnd], []=[SUnit]) and
   [token] (the stack [() | s:S] = TUnit/TGate, [++]=[tok_concat]).

   Because [SAnd] is a FREE binary constructor (NOT quotiented in
   CostAccountedSyntax), the signature monoid laws cannot hold as Leibniz
   equalities; they hold up to a congruence [sig_equiv] (≡sig), exactly as the
   spec's structural equivalence makes parallel composition a monoid up to ≡.
   The free token-stack monoid laws DO hold as Leibniz equalities (a list).
   Axiom-free.                                                                *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Setoid.
From Stdlib Require Import Morphisms.
From CostAccountedRho Require Import CostAccountedSyntax.

(* ── The signature commutative monoid (Sig, *, ()) up to ≡sig ───────────── *)

Reserved Notation "s '≡sig' t" (at level 70, no associativity).

Inductive sig_equiv : sig -> sig -> Prop :=
  | sige_refl  : forall s, s ≡sig s
  | sige_sym   : forall s t, s ≡sig t -> t ≡sig s
  | sige_trans : forall s t u, s ≡sig t -> t ≡sig u -> s ≡sig u
  | sige_and_comm   : forall s t, SAnd s t ≡sig SAnd t s
  | sige_and_assoc  : forall s t u, SAnd (SAnd s t) u ≡sig SAnd s (SAnd t u)
  | sige_and_unit_l : forall s, SAnd SUnit s ≡sig s
  | sige_and_unit_r : forall s, SAnd s SUnit ≡sig s
  | sige_and_cong   : forall s s' t t', s ≡sig s' -> t ≡sig t' -> SAnd s t ≡sig SAnd s' t'
where "s '≡sig' t" := (sig_equiv s t).

(* The commutative-monoid laws (the headline facts the monad's unit/assoc
   reduce to). Each is a single constructor. *)
Theorem sig_monoid_comm : forall s t, SAnd s t ≡sig SAnd t s.
Proof. apply sige_and_comm. Qed.
Theorem sig_monoid_assoc : forall s t u, SAnd (SAnd s t) u ≡sig SAnd s (SAnd t u).
Proof. apply sige_and_assoc. Qed.
Theorem sig_monoid_unit_l : forall s, SAnd SUnit s ≡sig s.
Proof. apply sige_and_unit_l. Qed.
Theorem sig_monoid_unit_r : forall s, SAnd s SUnit ≡sig s.
Proof. apply sige_and_unit_r. Qed.

Add Parametric Relation : sig sig_equiv
  reflexivity proved by sige_refl
  symmetry proved by sige_sym
  transitivity proved by sige_trans
  as sig_equiv_rel.

Add Parametric Morphism : SAnd with signature
  sig_equiv ==> sig_equiv ==> sig_equiv as SAnd_morphism.
Proof. intros. apply sige_and_cong; assumption. Qed.

(* ── The temporal token-stack FREE monoid (token, tok_concat, TUnit) ─────── *)

Fixpoint tok_concat (t u : token) : token :=
  match t with
  | TUnit      => u
  | TGate s t' => TGate s (tok_concat t' u)
  end.

Theorem tok_concat_unit_l : forall t, tok_concat TUnit t = t.
Proof. reflexivity. Qed.

Theorem tok_concat_unit_r : forall t, tok_concat t TUnit = t.
Proof. induction t as [| s t' IH]; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

Theorem tok_concat_assoc : forall t u v,
  tok_concat (tok_concat t u) v = tok_concat t (tok_concat u v).
Proof. induction t as [| s t' IH]; intros; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

(* [token_size] is a monoid homomorphism into (nat, +, 0) — this is what lets
   the monad's temporal grade (the consumed-stack length, the modulus) add up. *)
Theorem token_size_concat : forall t u,
  token_size (tok_concat t u) = token_size t + token_size u.
Proof. induction t as [| s t' IH]; intros; simpl; [reflexivity | rewrite IH; lia]. Qed.

(* The free monoid is NOT commutative (continued-gslt-cost-v2.tex:523 — the
   temporal stack records consumption order). A concrete witness: swapping two
   distinct gates changes the stack. *)
Theorem tok_concat_not_commutative :
  exists t u, tok_concat t u <> tok_concat u t.
Proof.
  exists (TGate SUnit TUnit), (TGate (SGround nil) TUnit).
  simpl. intro H. inversion H.
Qed.
