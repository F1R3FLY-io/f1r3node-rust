(** Finite host registration admission, not an allocator/hash-table model.

    Source: rho_runtime::validate_extra_system_processes. The outer Rust loop
    visits extras in caller order. Its known roster is built-ins plus earlier
    extras; this model retains the same membership with earlier extras reversed.
    The only reported failure identifies the current candidate, not an earlier
    collision partner, so that representation does not change error selection.
    Reserved names/channels are basic_processes keys, values and bundle bodies.

    Channel equality is parameterized by decidable structural equality. Source
    correspondence must justify the actual Par Eq implementation and enumerate
    the same built-in constructors; neither is asserted by this model. Service
    callbacks and global publication happen only after this pure check returns.
    Existing LanguageProviderLifecycle supplies owner/rollback laws separately.
*)
From Stdlib Require Import List String ZArith Bool.
Import ListNotations.
Set Implicit Arguments.

Section Admission.
Context {Channel : Type}.
Variable channel_eq_dec : forall x y : Channel, {x = y} + {x <> y}.

Record registration := {
  urn : string;
  body_ref : Z;
  channel : Channel
}.

Definition channel_eqb (x y : Channel) := if channel_eq_dec x y then true else false.
Definition collides (x y : registration) :=
  String.eqb (urn x) (urn y) || Z.eqb (body_ref x) (body_ref y) ||
  channel_eqb (channel x) (channel y).

Definition separated (x y : registration) :=
  urn x <> urn y /\ body_ref x <> body_ref y /\ channel x <> channel y.

Lemma channel_eqb_false : forall x y, channel_eqb x y = false <-> x <> y.
Proof. intros x y; unfold channel_eqb; destruct (channel_eq_dec x y); intuition discriminate. Qed.

Lemma collision_false_exact : forall x y, collides x y = false <-> separated x y.
Proof.
  intros; unfold collides, separated.
  repeat rewrite Bool.orb_false_iff.
  rewrite String.eqb_neq, Z.eqb_neq, channel_eqb_false.
  tauto.
Qed.

Lemma existsb_false_exact {A} (f : A -> bool) : forall xs,
  existsb f xs = false <-> forall x, In x xs -> f x = false.
Proof.
  induction xs as [|a xs IH]; cbn.
  - split; intros; [contradiction|reflexivity].
  - rewrite Bool.orb_false_iff, IH.
    split.
    + intros [Ha Hxs] x [Hax|Hx]; [subst; exact Ha|apply Hxs; exact Hx].
    + intros H; split; [apply H; left; reflexivity|].
      intros x Hx; apply H; right; exact Hx.
Qed.

Variable reserved_names : list string.
Variable reserved_channels : list Channel.

Definition reserved (candidate : registration) :=
  existsb (String.eqb (urn candidate)) reserved_names ||
  existsb (channel_eqb (channel candidate)) reserved_channels.

Lemma reserved_false_exact : forall candidate,
  reserved candidate = false <->
  ~ In (urn candidate) reserved_names /\ ~ In (channel candidate) reserved_channels.
Proof.
  intro c; unfold reserved; rewrite Bool.orb_false_iff.
  repeat rewrite existsb_false_exact.
  split.
  - intros [Hnames Hchannels]; split; intro H.
    + specialize (Hnames _ H); rewrite String.eqb_refl in Hnames; discriminate.
    + specialize (Hchannels _ H); apply channel_eqb_false in Hchannels.
      apply Hchannels; reflexivity.
  - intros [Hnames Hchannels]; split; intros x Hx.
    + apply String.eqb_neq; intro Heq; subst; contradiction.
    + apply channel_eqb_false; intro Heq; subst; contradiction.
Qed.

Fixpoint validate (known remaining : list registration) : bool :=
  match remaining with
  | [] => true
  | candidate :: rest =>
      negb (reserved candidate || existsb (collides candidate) known) &&
      validate (candidate :: known) rest
  end.

Fixpoint safe (known remaining : list registration) : Prop :=
  match remaining with
  | [] => True
  | candidate :: rest =>
      (~ In (urn candidate) reserved_names /\
       ~ In (channel candidate) reserved_channels) /\
      (forall existing, In existing known -> separated candidate existing) /\
      safe (candidate :: known) rest
  end.

Theorem accepted_exactly_safe : forall remaining known,
  validate known remaining = true <-> safe known remaining.
Proof.
  induction remaining as [|candidate rest IH]; intro known; cbn.
  - tauto.
  - rewrite Bool.andb_true_iff, Bool.negb_true_iff, Bool.orb_false_iff.
    rewrite reserved_false_exact, existsb_false_exact, IH.
    setoid_rewrite collision_false_exact.
    tauto.
Qed.

Definition publish (known extras : list registration) :=
  if validate known extras then Some extras else None.

Theorem publication_preserves_exact_roster : forall known extras output,
  publish known extras = Some output -> output = extras /\ safe known extras.
Proof.
  intros known extras output; unfold publish.
  destruct (validate known extras) eqn:H; intro Hout; [|discriminate].
  inversion Hout; subst; split; [reflexivity|].
  apply accepted_exactly_safe; exact H.
Qed.

Theorem refused_registration_publishes_nothing : forall known extras,
  ~ safe known extras -> publish known extras = None.
Proof.
  intros known extras H; unfold publish.
  destruct (validate known extras) eqn:Hvalid; [|reflexivity].
  exfalso; apply H; apply accepted_exactly_safe; exact Hvalid.
Qed.

Theorem empty_extension_is_admitted : forall known, publish known [] = Some [].
Proof. reflexivity. Qed.
End Admission.

Print Assumptions collision_false_exact.
Print Assumptions reserved_false_exact.
Print Assumptions accepted_exactly_safe.
Print Assumptions publication_preserves_exact_roster.
Print Assumptions refused_registration_publishes_nothing.
Print Assumptions empty_extension_is_admitted.
