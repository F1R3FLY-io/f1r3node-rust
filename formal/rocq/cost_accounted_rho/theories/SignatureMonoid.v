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

(* ── CA-P-180: the section/digest does NOT respect ≡, and MUST NOT ─────────────
   continued-gslt-cost-v2 §5 "Quotient and section" (:618-632). The signature
   constructor is the SECTION composed with a collision-resistant digest,
   [#] = digest ∘ cf : 𝕔Term → Sig — "a one-way commitment to a representative.
   It does NOT respect ≡ — and must not, since a commitment identifying
   congruent-but-distinct representatives would be no commitment."

   We mechanize the "must not" half as a THEOREM, parametrically over ANY
   would-be encoder [enc] of the signature AST that is injective on syntax (the
   defining property of a collision-resistant digest composed with a section: a
   one-way commitment that distinguishes distinct representatives). The point is
   that injectivity on the AST is INCOMPATIBLE with respecting ≡sig: the section
   commits to the SYNTACTIC representative, and ≡sig identifies syntactically
   distinct representatives (e.g. the two argument orders of a compound), so an
   ≡sig-respecting encoder would have to identify representatives that an
   injective one keeps apart. No fresh axiom is introduced: injectivity is a
   HYPOTHESIS on the supplied [enc], discharged by the caller (the runtime
   digest), exactly as DR-2/DR-16 abstract the digest section. *)

(* Injectivity of an encoder on the signature syntax (the AST), the
   commitment-distinguishes-representatives property of [digest ∘ cf]. *)
Definition injective_on_syntax {D : Type} (enc : sig -> D) : Prop :=
  forall a b, enc a = enc b -> a = b.

(* An encoder "respects ≡sig" iff it assigns equal codes to ≡sig-congruent
   signatures (the property a commitment must FAIL to have). *)
Definition respects_sig_equiv {D : Type} (enc : sig -> D) : Prop :=
  forall a b, a ≡sig b -> enc a = enc b.

(* CA-P-180 headline: a digest/section that is injective on the signature
   syntax does NOT respect ≡sig. Equivalently, no encoder can be both a
   faithful commitment (injective on representatives) AND ≡-coherent — which is
   precisely "a commitment identifying congruent-but-distinct representatives
   would be no commitment." *)
Theorem sig_section_not_respect_equiv :
  forall {D : Type} (enc : sig -> D),
    injective_on_syntax enc -> ~ respects_sig_equiv enc.
Proof.
  intros D enc Hinj Hresp.
  (* Two distinct ground signatures s, t and the congruent-but-distinct compound
     pair SAnd s t ≡sig SAnd t s with SAnd s t <> SAnd t s. *)
  set (s := SGround (cons true nil)).
  set (t := SGround (cons false nil)).
  (* ≡sig identifies the two compound orderings (commutativity constructor). *)
  assert (Hcong : SAnd s t ≡sig SAnd t s) by apply sige_and_comm.
  (* If enc respected ≡sig, the two distinct compounds would get equal codes; *)
  pose proof (Hresp _ _ Hcong) as Hcodes.
  (* injectivity then forces the two compounds to be syntactically equal, *)
  apply Hinj in Hcodes.
  (* whence the first arguments coincide — but s <> t, a contradiction (SAnd is
     a free constructor, so SAnd s t = SAnd t s forces s = t). *)
  injection Hcodes as Hs _. discriminate Hs.
Qed.

(* Restatement matching the spec's prose verbatim: there is a congruent-but-
   syntactically-distinct pair on which any injective digest disagrees, so the
   digest DISTINGUISHES congruent representatives (the desired non-coherence). *)
Corollary digest_distinguishes_congruent_reps :
  forall {D : Type} (enc : sig -> D),
    injective_on_syntax enc ->
    exists a b, a ≡sig b /\ a <> b /\ enc a <> enc b.
Proof.
  intros D enc Hinj.
  exists (SAnd (SGround (cons true nil)) (SGround (cons false nil))).
  exists (SAnd (SGround (cons false nil)) (SGround (cons true nil))).
  split; [ apply sige_and_comm |].
  split.
  - intro Heq. inversion Heq.
  - intro Hcodes. apply Hinj in Hcodes. inversion Hcodes.
Qed.
