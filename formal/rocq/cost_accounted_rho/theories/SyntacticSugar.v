(* ═══════════════════════════════════════════════════════════════════════════
   SyntacticSugar.v — Section 3.8 syntactic sugar at the translation level
   ═══════════════════════════════════════════════════════════════════════════

   Section 3.8 of the spec introduces two abbreviations for signing a
   for-comprehension together with its continuation:

     UNIFORM signing (Def., eq. sugar-uniform):
       {for(y ← x){P}}_s   ≜   {for(y ← x){ {P}_s }}_s

     LINEAR TRANSFER ⊸ (Def., eq. sugar-lollipop):
       {for(y ← x){P}}_{s₁ ⊸ s₂}   ≜   {for(y ← x){ {P}_s₂ }}_s₁

   In this development a signed term [SSigned : proc -> sig -> system] signs a
   BARE process; [for]/[send] live at the [proc] level and cannot carry a
   signed-term CONTINUATION, so neither defining equation is expressible as a
   [system]-equation directly (the right-hand sides sign a continuation that
   sits inside a [for] body). This is the repo's "extension of pure rho"
   modelling choice versus the spec's native "signed terms pervade syntax"
   (§3.1).

   FOLLOWING THE PERSISTED DESIGN (option A), we discharge the §3.8 defining
   equations at the TRANSLATION level. Each sugar form is given meaning by its
   Appendix-A image in the pure rho calculus, and we prove that the desugared
   right-hand side and the sugar left-hand side denote STRUCTURALLY EQUIVALENT
   processes ([≡] on [proc], RhoSyntax.v). Crucially, the linear transfer ⊸
   desugars to a pair of NESTED PLAIN-SIGNATURE fuel-gate layers (outer [s₁],
   inner [s₂]) — never a dedicated [Sig::Lolly] node — so it coexists with the
   DR-10 ILLE extension connective without introducing a new signature
   constructor. Uniform signing is the [s₁ = s₂ = s] instance.

   The Appendix-A image of [{for(y ← x){ {P}_s_in }}_s_out] is, by
   eq. app-st-signed-atomic + eq. app-p-recv:

       for(t ← N⟦s_out⟧){ *t | for(y ← N⟦x⟧){ T⟦{P}_s_in⟧ } }

   where the inner [T⟦{P}_s_in⟧] is itself the atomic fuel gate
   [for(t' ← N⟦s_in⟧){ *t' | P }]. The [P_tr] translation of Translation.v
   realises exactly these atomic fuel gates, so we build both images from
   [P_tr] / [N_tr] and the result is a definitional equality, hence [≡].

   Dependencies: RhoSyntax, CostAccountedSyntax, RhoReduction, Translation
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import RhoReduction.
From CostAccountedRho Require Import Translation.

Section SugarTranslation.

(* Same translation parameters as Translation.v: the two reflection-axis
   canonical-process families. No hypotheses are used in this file, so every
   theorem here is unconditional (the parameters become universally quantified
   binders after the section closes). *)
Variable hash_process : list bool -> proc.
Variable ground_process : list bool -> proc.

Notation N := (N_tr hash_process ground_process).
Notation Pf := (P_tr hash_process ground_process).
Notation Sy := (S_tr hash_process ground_process).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Translation-level images of a signed for-comprehension
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The Appendix-A image of the INNER signed continuation [{P}_s_in] is the
   atomic fuel gate [T⟦{P}_s_in⟧]. We reuse [P_tr] for this: by definition
   [P_tr P (atomic s_in) = for(_ ← N s_in){ lift₁ P | *(NVar 0) }], which is
   exactly eq. app-st-signed-atomic. *)
Definition signed_continuation_image (P : proc) (s_in : sig) : proc :=
  Pf P s_in.

(* The Appendix-A image of [{for(y ← x){ {P}_s_in }}_s_out]: an OUTER atomic
   fuel gate on [N s_out] whose released body runs [for(y ← N x){ inner }],
   where [inner] is the signed-continuation image above (lifted to cross the
   outer gate's binder). This is eq. app-st-signed-atomic composed with
   eq. app-p-recv. *)
Definition signed_for_image (x : name) (P : proc) (s_out s_in : sig) : proc :=
  PInput (N s_out)
    (PPar (lift_proc 1 0 (PInput x (signed_continuation_image P s_in)))
          (PDeref (NVar 0))).

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Uniform signing (eq. sugar-uniform)
   ═══════════════════════════════════════════════════════════════════════════

   LHS [{for(y ← x){P}}_s]: the sugar's option-A meaning is the desugared
   image with BOTH gates carrying the same signature [s].
   RHS [{for(y ← x){ {P}_s }}_s]: the explicit desugaring.                    *)

(* The sugar image: outer + inner gates both on [s]. *)
Definition uniform_sugar_image (x : name) (P : proc) (s : sig) : proc :=
  signed_for_image x P s s.

(* The desugared image: the same construction, written through the explicit
   inner signed continuation [{P}_s]. *)
Definition uniform_desugar_image (x : name) (P : proc) (s : sig) : proc :=
  PInput (N s)
    (PPar (lift_proc 1 0 (PInput x (Pf P s)))
          (PDeref (NVar 0))).

(* The §3.8 uniform-signing defining equation holds at the translation level:
   the desugared image is structurally equivalent to the sugar image. Both
   sides are gated by the SAME signature [s] (the single-party case where the
   communication and its continuation are funded by one signer). *)
Theorem uniform_sugar_translation_equiv : forall x P s,
  uniform_desugar_image x P s ≡ uniform_sugar_image x P s.
Proof.
  intros x P s.
  unfold uniform_desugar_image, uniform_sugar_image, signed_for_image,
         signed_continuation_image.
  apply se_refl.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Linear transfer of authority ⊸ (eq. sugar-lollipop)
   ═══════════════════════════════════════════════════════════════════════════

   LHS [{for(y ← x){P}}_{s₁ ⊸ s₂}]: the ⊸ sugar's option-A meaning is the
   desugared image with OUTER gate [s₁] (funds the rendezvous) and INNER gate
   [s₂] (funds the continuation).
   RHS [{for(y ← x){ {P}_s₂ }}_s₁]: the explicit desugaring.

   The ⊸ desugars to the gate pair built from PLAIN signatures [s₁], [s₂] —
   there is NO [Sig::Lolly]/[ASLolly] node anywhere in the image, so this
   coexists with the DR-10 ILLE [⊸] connective (which lives only in the
   runtime [sig_algebra], never in a translated gate).                       *)

(* The ⊸ sugar image: outer gate on [s₁], inner gate on [s₂]. *)
Definition lollipop_sugar_image (x : name) (P : proc) (s1 s2 : sig) : proc :=
  signed_for_image x P s1 s2.

(* The desugared image: the same construction through the explicit inner
   signed continuation [{P}_s₂] under the outer [s₁] gate. *)
Definition lollipop_desugar_image (x : name) (P : proc) (s1 s2 : sig) : proc :=
  PInput (N s1)
    (PPar (lift_proc 1 0 (PInput x (Pf P s2)))
          (PDeref (NVar 0))).

(* The §3.8 linear-transfer defining equation holds at the translation level:
   the desugared image (outer [s₁], inner [s₂]) is structurally equivalent to
   the ⊸ sugar image, with both gate layers built from PLAIN signatures. *)
Theorem lollipop_sugar_translation_equiv : forall x P s1 s2,
  lollipop_desugar_image x P s1 s2 ≡ lollipop_sugar_image x P s1 s2.
Proof.
  intros x P s1 s2.
  unfold lollipop_desugar_image, lollipop_sugar_image, signed_for_image,
         signed_continuation_image.
  apply se_refl.
Qed.

(* Uniform signing is exactly the [s₁ = s₂] instance of the linear transfer:
   funding the rendezvous and the continuation with the same signer. This
   records that the two §3.8 sugars share one desugaring rule. *)
Theorem uniform_is_lollipop_diagonal : forall x P s,
  uniform_sugar_image x P s = lollipop_sugar_image x P s s.
Proof. intros. reflexivity. Qed.

(* The ⊸ image NEVER mentions a dedicated lollipop signature constructor: for
   an atomic continuation signature [s2], the inner gate is exactly the PLAIN
   fuel gate [PInput (N s2) ...], exhibited explicitly. This is the structural
   witness that option-A desugaring introduces no [Sig::Lolly]/[ASLolly] node —
   the inner layer is a plain-signature [N s2] channel. *)
Theorem lollipop_image_inner_gate_is_plain_unit : forall x P s1,
  lollipop_sugar_image x P s1 SUnit
    = PInput (N s1)
        (PPar (lift_proc 1 0 (PInput x
                 (PInput (N SUnit)
                    (PPar (lift_proc 1 0 P) (PDeref (NVar 0))))))
              (PDeref (NVar 0))).
Proof. intros. reflexivity. Qed.

Theorem lollipop_image_inner_gate_is_plain_ground : forall x P s1 bs,
  lollipop_sugar_image x P s1 (SGround bs)
    = PInput (N s1)
        (PPar (lift_proc 1 0 (PInput x
                 (PInput (N (SGround bs))
                    (PPar (lift_proc 1 0 P) (PDeref (NVar 0))))))
              (PDeref (NVar 0))).
Proof. intros. reflexivity. Qed.

Theorem lollipop_image_inner_gate_is_plain_quote : forall x P s1 bs,
  lollipop_sugar_image x P s1 (SQuote bs)
    = PInput (N s1)
        (PPar (lift_proc 1 0 (PInput x
                 (PInput (N (SQuote bs))
                    (PPar (lift_proc 1 0 P) (PDeref (NVar 0))))))
              (PDeref (NVar 0))).
Proof. intros. reflexivity. Qed.

End SugarTranslation.
