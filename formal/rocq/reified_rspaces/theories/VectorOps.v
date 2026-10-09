(* ═══════════════════════════════════════════════════════════════════════════
   VectorOps.v — the replies of the process rho:lang:vector (P6, step 8d)

   rho:lang:vector computes add, scale and dot exactly and replies in the
   widest kind of its inputs (plan decision 66, design v3 4.7): integers give
   an integer, any rational or fixed-point input gives a rational, and any
   float input gives a float, the rounding to nearest (RN_53) of the exact
   value. norm and distance reply with the rounding of a square root. This
   module proves two claims:
   - the reply of add, scale and dot has the widest kind of the inputs, it
     is the exact value for the integer and the rational kinds, and it is
     one rounding of the exact value for the float kind (no rounding of a
     partial sum, decision 54);
   - the midpoint characterization of the nearest root: when the squares of
     the midpoints between a reply r and its neighbours bound x, no
     representable value is nearer to the root of x than r. This is the
     check of the property of the correctly rounded roots.

   Sources
   - Design: .reified-rspaces-work/p6/design-v3.md, section 4.7.
   - Plan: decisions 54 (exact arithmetic), 66 (the widest kind).
   - Code: the paths below are relative to rholang/src/rust/interpreter/.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   kind, krank            tensor/ops.rs:95-99 (Kind: Integer, Rational and
                          Float in the order of their width)
   kind_of                tensor/ops.rs:103-115 (kind_of: GInt, GBigInt and
                          GBool are integers, GBigRat and GFixedPoint are
                          rationals, GDouble and GFloat32 are floats)
   kjoin, widest          tensor/ops.rs:331-343, 368-380, 383-397
                          (vector_operand: the widest kind of the elements,
                          Integer without elements; add and scale: the
                          wider kind of the two operands)
   val                    tensor/num.rs:243-292 (number_of: the exact value
                          of each numeric expression)
   encode                 tensor/ops.rs:401-428 (number_par: GInt or GBigInt,
                          GBigRat, or the GDouble of round_to_f64)
   rnd                    tensor/num.rs:465-480 (round_to_f64, RN_53)
   qsum                   tensor/exact.rs:265-291 (dot: the exact sum of the
                          products, with no rounding)
   add_reply, scale_reply,
   dot_reply              tensor/ops.rs:234-272 (evaluate: the operations),
                          reified/system_processes.rs:316-340 (the process
                          rho:lang:vector replies the value)
   root_midpoint_nearest  tensor/num.rs:484-511 (sqrt_to_f64: the mantissa
                          is rounded by the square of the odd midpoint)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A number is an integer, a rational (a fixed point is a rational), or a
     float with its exact value (a dyadic rational). The model computes in
     the rationals of the standard library (QArith).
   - The model covers the calls whose two vectors have one length. A call
     with two lengths fails with DimensionMismatch before any arithmetic
     (vector_pair), so combine never drops an element of a modeled call.
   - For the integer kind, number_par fails with Inconsistent when the value
     is not an integer. Headline 1 proves that this branch is unreachable:
     the value of an integer reply is integral.
   - The rounding to nearest is an abstract function rnd. The module never
     uses a property of rnd: the claim is that the float reply is rnd of
     the exact value, whatever rnd is.
   - A representable value is an abstract predicate F. The midpoint
     theorem needs only the order of the representable values next to the
     reply: no representable value lies strictly between a neighbour and
     the reply.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. vector_replies_exact   the reply kind is the widest kind of the
                              inputs, and the reply is the exact value for
                              the integer and rational kinds and rnd of the
                              exact value for the float kind
    2. root_midpoint_nearest  when the squares of the midpoints next to r
                              bound x = s * s with s >= 0, no representable
                              value is nearer to s than r

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. The file
   imports only the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - lra does not read x / 2 (Qinv 2) as a constant ("Cannot find
     witness" on a linear goal). Write a midpoint as (a + b) * (1 # 2).
   - rewrite (Qabs_pos u) unfolds Qeq into its integer equation and finds
     no subterm. Keep Qabs u == u as a hypothesis for lra (qabs_sides).
   - QArith opens Q_scope, so <= in a lemma about nat means Qle. Open
     nat_scope for the kinds and Q_scope from section 2 on.
   - In intro patterns, [<- | H] replaces the variable of the equation
     with the other side, so the next tactic must name that side.
   - A goal selector after a long ; chain did not see the three goals of
     encode_spec, and contradiction H already closes a goal that the next
     tactic then misses. Use all: first [...] and exfalso; apply H.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import QArith Qabs ZArith List Bool Lia Lqa.
Import ListNotations.
Local Open Scope nat_scope.

(* ── 1. Kinds ────────────────────────────────────────────────────────── *)

Inductive kind : Type := KInt | KRat | KFloat.

Definition krank (k : kind) : nat := match k with KInt => 0 | KRat => 1 | KFloat => 2 end.

(* The wider of two kinds. *)
Definition kjoin (a b : kind) : kind := if Nat.ltb (krank a) (krank b) then b else a.

(* The widest kind of a call: the integer kind when it has no input. *)
Definition widest (ks : list kind) : kind := fold_left kjoin ks KInt.

Lemma kjoin_rank : forall a b, krank (kjoin a b) = Nat.max (krank a) (krank b).
Proof.
  intros a b. unfold kjoin. destruct (Nat.ltb (krank a) (krank b)) eqn:E.
  - apply Nat.ltb_lt in E. lia.
  - apply Nat.ltb_ge in E. lia.
Qed.

Lemma krank_inj : forall a b, krank a = krank b -> a = b.
Proof. intros [] [] H; cbn in H; congruence. Qed.

Lemma widest_rank_acc :
  forall ks acc, krank (fold_left kjoin ks acc) = fold_left Nat.max (map krank ks) (krank acc).
Proof.
  induction ks as [| k ks IH]; intros acc; [reflexivity |].
  cbn [fold_left map]. rewrite IH, kjoin_rank. reflexivity.
Qed.

Lemma fold_max_ge_acc : forall l acc, acc <= fold_left Nat.max l acc.
Proof.
  induction l as [| x l IH]; intros acc; [cbn; lia |].
  cbn [fold_left]. specialize (IH (Nat.max acc x)). lia.
Qed.

Lemma fold_max_ge_in : forall l acc x, In x l -> x <= fold_left Nat.max l acc.
Proof.
  induction l as [| y l IH]; intros acc x H; [destruct H |].
  cbn [fold_left]. destruct H as [<- | H].
  - pose proof (fold_max_ge_acc l (Nat.max acc y)). lia.
  - exact (IH _ x H).
Qed.

Lemma fold_max_in : forall l acc, fold_left Nat.max l acc = acc \/ In (fold_left Nat.max l acc) l.
Proof.
  induction l as [| x l IH]; intros acc; [left; reflexivity |].
  cbn [fold_left]. destruct (IH (Nat.max acc x)) as [H | H].
  - rewrite H. destruct (Nat.max_spec acc x) as [[_ ->] | [_ ->]]; [right; left; reflexivity | left; reflexivity].
  - right. right. exact H.
Qed.

(* The widest kind is the input kind of the greatest rank: it is an input
   kind (or the integer kind without inputs), and no input kind is wider. *)
Lemma widest_is_join :
  forall ks, (forall k, In k ks -> krank k <= krank (widest ks)) /\
             (widest ks = KInt \/ In (widest ks) ks).
Proof.
  intros ks. unfold widest. split.
  - intros k Hk. rewrite widest_rank_acc. apply fold_max_ge_in. apply in_map. exact Hk.
  - destruct (fold_max_in (map krank ks) (krank KInt)) as [H | H].
    + left. apply krank_inj. rewrite widest_rank_acc. exact H.
    + right. rewrite <- widest_rank_acc in H. apply in_map_iff in H as (k & Hk & Hin).
      rewrite <- (krank_inj _ _ Hk). exact Hin.
Qed.

(* ── 2. Numbers and replies ─────────────────────────────────────────── *)

Local Open Scope Q_scope.

Inductive num : Type := NInt (z : Z) | NRat (q : Q) | NFloat (q : Q).

Definition kind_of (n : num) : kind := match n with NInt _ => KInt | NRat _ => KRat | NFloat _ => KFloat end.
Definition val (n : num) : Q := match n with NInt z => inject_Z z | NRat q => q | NFloat q => q end.

Definition kinds (v : list num) : list kind := map kind_of v.

Definition qsum (l : list Q) : Q := fold_right Qplus 0 l.

Section Replies.
  Variable rnd : Q -> Q.

  (* The reply of the exact value v in the kind k. An integer reply holds
     the numerator of v (exact when its denominator is 1). *)
  Definition encode (k : kind) (v : Q) : num :=
    match k with
    | KInt => NInt (Qnum v)
    | KRat => NRat v
    | KFloat => NFloat (rnd v)
    end.

  Definition add_reply (a b : list num) : list num :=
    let k := widest (kinds a ++ kinds b) in
    map (fun p => encode k (val (fst p) + val (snd p))) (combine a b).

  Definition scale_reply (c : num) (a : list num) : list num :=
    let k := widest (kind_of c :: kinds a) in
    map (fun x => encode k (val c * val x)) a.

  Definition dot_reply (a b : list num) : num :=
    let k := widest (kinds a ++ kinds b) in
    encode k (qsum (map (fun p => val (fst p) * val (snd p)) (combine a b))).

  (* The value of an integer input has the denominator 1, and sums and
     products keep it. *)
  Definition integral (q : Q) : Prop := Qden q = 1%positive.

  Lemma integral_inject : forall z, integral (inject_Z z).
  Proof. intros z. reflexivity. Qed.

  Lemma integral_plus : forall p q, integral p -> integral q -> integral (p + q).
  Proof. intros [pn pd] [qn qd] Hp Hq. unfold integral in *. cbn in *. subst. reflexivity. Qed.

  Lemma integral_mult : forall p q, integral p -> integral q -> integral (p * q).
  Proof. intros [pn pd] [qn qd] Hp Hq. unfold integral in *. cbn in *. subst. reflexivity. Qed.

  Lemma integral_qsum : forall l, (forall q, In q l -> integral q) -> integral (qsum l).
  Proof.
    induction l as [| q l IH]; intros H; [reflexivity |].
    cbn [qsum fold_right]. apply integral_plus; [apply H; left; reflexivity |].
    apply IH. intros x Hx. apply H. right. exact Hx.
  Qed.

  Lemma integral_exact : forall q, integral q -> inject_Z (Qnum q) == q.
  Proof. intros [n d] H. unfold integral in H. cbn in H. subst. reflexivity. Qed.

  (* The widest kind is the integer kind only when every input is an
     integer. *)
  Lemma widest_int : forall ks, widest ks = KInt -> forall k, In k ks -> k = KInt.
  Proof.
    intros ks H k Hk. destruct (widest_is_join ks) as [Hge _].
    specialize (Hge k Hk). rewrite H in Hge. cbn in Hge. destruct k; [reflexivity | cbn in Hge; lia | cbn in Hge; lia].
  Qed.

  Lemma int_val_integral : forall n, kind_of n = KInt -> integral (val n).
  Proof. intros [z | q | q] H; cbn in H; try discriminate H. reflexivity. Qed.

  (* The reply of the exact value v in the kind k: it has the kind k, its
     value is v for the rational kind and for the integer kind when v is
     integral, and rnd v for the float kind. *)
  Lemma encode_spec :
    forall k v,
      kind_of (encode k v) = k /\
      (k = KInt -> integral v -> val (encode k v) == v) /\
      (k = KRat -> val (encode k v) == v) /\
      (k = KFloat -> val (encode k v) = rnd v).
  Proof.
    intros k v. destruct k; cbn [encode kind_of val]; repeat split; intros H; try discriminate H.
    all: first [intros Hv; apply integral_exact; exact Hv | reflexivity].
  Qed.

  Lemma in_combine_kinds :
    forall (a b : list num) x y, In (x, y) (combine a b) -> In (kind_of x) (kinds a ++ kinds b) /\ In (kind_of y) (kinds a ++ kinds b).
  Proof.
    intros a b x y H. split; apply in_or_app.
    - left. apply in_map. exact (in_combine_l _ _ _ _ H).
    - right. apply in_map. exact (in_combine_r _ _ _ _ H).
  Qed.

  (* Headline 1 (S27). The reply of add, scale and dot has the widest kind
     of the inputs. It is the exact value for the integer and the rational
     kinds, and one rounding of the exact value for the float kind: the
     exact sum of dot is rounded once, never a partial sum (decision 54). *)
  Theorem vector_replies_exact :
    forall a b c,
      (let k := widest (kinds a ++ kinds b) in
       let exact := qsum (map (fun p => val (fst p) * val (snd p)) (combine a b)) in
       kind_of (dot_reply a b) = k /\
       (k = KFloat -> val (dot_reply a b) = rnd exact) /\
       (k <> KFloat -> val (dot_reply a b) == exact)) /\
      (forall n, In n (add_reply a b) ->
         kind_of n = widest (kinds a ++ kinds b) /\
         exists x y, In (x, y) (combine a b) /\
           (widest (kinds a ++ kinds b) = KFloat -> val n = rnd (val x + val y)) /\
           (widest (kinds a ++ kinds b) <> KFloat -> val n == val x + val y)) /\
      (forall n, In n (scale_reply c a) ->
         kind_of n = widest (kind_of c :: kinds a) /\
         exists x, In x a /\
           (widest (kind_of c :: kinds a) = KFloat -> val n = rnd (val c * val x)) /\
           (widest (kind_of c :: kinds a) <> KFloat -> val n == val c * val x)).
  Proof.
    intros a b c. split; [| split].
    - cbn zeta. set (k := widest (kinds a ++ kinds b)).
      set (exact := qsum (map (fun p => val (fst p) * val (snd p)) (combine a b))).
      unfold dot_reply. fold k. fold exact.
      destruct (encode_spec k exact) as (Hk & Hi & Hr & Hf). split; [exact Hk |]. split; [exact Hf |].
      intros Hnf. destruct k eqn:Ek.
      + apply Hi; [reflexivity |]. apply integral_qsum. intros q Hq.
        apply in_map_iff in Hq as ([x y] & <- & Hxy). cbn [fst snd].
        destruct (in_combine_kinds a b x y Hxy) as [Hx Hy].
        apply integral_mult; apply int_val_integral; apply (widest_int _ Ek); assumption.
      + apply Hr. reflexivity.
      + exfalso. apply Hnf. reflexivity.
    - intros n Hn. unfold add_reply in Hn. apply in_map_iff in Hn as ([x y] & <- & Hxy). cbn [fst snd].
      set (k := widest (kinds a ++ kinds b)).
      destruct (encode_spec k (val x + val y)) as (Hk & Hi & Hr & Hf). split; [exact Hk |].
      exists x, y. split; [exact Hxy |]. split; [exact Hf |].
      intros Hnf. destruct k eqn:Ek.
      + apply Hi; [reflexivity |]. destruct (in_combine_kinds a b x y Hxy) as [Hx Hy].
        apply integral_plus; apply int_val_integral; apply (widest_int _ Ek); assumption.
      + apply Hr. reflexivity.
      + exfalso. apply Hnf. reflexivity.
    - intros n Hn. unfold scale_reply in Hn. apply in_map_iff in Hn as (x & <- & Hx).
      set (k := widest (kind_of c :: kinds a)).
      destruct (encode_spec k (val c * val x)) as (Hk & Hi & Hr & Hf). split; [exact Hk |].
      exists x. split; [exact Hx |]. split; [exact Hf |].
      intros Hnf. destruct k eqn:Ek.
      + apply Hi; [reflexivity |].
        apply integral_mult; apply int_val_integral; apply (widest_int _ Ek);
          [left; reflexivity | right; apply in_map; exact Hx].
      + apply Hr. reflexivity.
      + exfalso. apply Hnf. reflexivity.
  Qed.
End Replies.

(* ── 3. The nearest root ─────────────────────────────────────────────── *)

(* The absolute value of s - a on each side of a, as two equations that
   lra can use (a rewrite with Qabs_pos unfolds Qeq). *)
Lemma qabs_sides :
  forall u : Q, (0 <= u -> Qabs u == u) /\ (u <= 0 -> Qabs u == - u).
Proof. intros u. split; [apply Qabs_pos | apply Qabs_neg]. Qed.

(* For a < b, s is at least as near to a as to b exactly when s lies at or
   below their midpoint. *)
Lemma nearer_iff_below_mid :
  forall a b s : Q, a < b -> (Qabs (s - a) <= Qabs (s - b) <-> s <= (a + b) * (1 # 2)).
Proof.
  intros a b s Hab.
  destruct (qabs_sides (s - a)) as [A1 A2], (qabs_sides (s - b)) as [B1 B2].
  destruct (Qlt_le_dec s a) as [Hsa | Has].
  - pose proof (A2 ltac:(lra)). pose proof (B2 ltac:(lra)). split; intros; lra.
  - destruct (Qlt_le_dec s b) as [Hsb | Hbs].
    + pose proof (A1 ltac:(lra)). pose proof (B2 ltac:(lra)). split; intros; lra.
    + pose proof (A1 ltac:(lra)). pose proof (B1 ltac:(lra)). split; intros; lra.
Qed.

(* For a < b, s is at least as near to b as to a exactly when s lies at or
   above their midpoint. *)
Lemma nearer_iff_above_mid :
  forall a b s : Q, a < b -> (Qabs (s - b) <= Qabs (s - a) <-> (a + b) * (1 # 2) <= s).
Proof.
  intros a b s Hab.
  destruct (qabs_sides (s - a)) as [A1 A2], (qabs_sides (s - b)) as [B1 B2].
  destruct (Qlt_le_dec s a) as [Hsa | Has].
  - pose proof (A2 ltac:(lra)). pose proof (B2 ltac:(lra)). split; intros; lra.
  - destruct (Qlt_le_dec s b) as [Hsb | Hbs].
    + pose proof (A1 ltac:(lra)). pose proof (B2 ltac:(lra)). split; intros; lra.
    + pose proof (A1 ltac:(lra)). pose proof (B1 ltac:(lra)). split; intros; lra.
Qed.

(* For s, m >= 0, the order of the squares is the order of the values. *)
Lemma sq_le_iff : forall s m : Q, 0 <= s -> 0 <= m -> (s * s <= m * m <-> s <= m).
Proof.
  intros s m Hs Hm. split; intros H.
  - destruct (Qlt_le_dec m s) as [Hlt | Hle]; [| exact Hle].
    exfalso. apply (Qle_not_lt _ _ H). nra.
  - nra.
Qed.

(* Headline 2 (S28). The midpoint characterization of the nearest root:
   r is representable, lo and hi are its neighbours (no representable value
   lies strictly between them and r), s >= 0 is the root of x, the square
   of the midpoint below r bounds x from below when that midpoint is
   positive, and the square of the midpoint above r bounds x from above.
   Then no representable value is nearer to s than r. *)
Theorem root_midpoint_nearest :
  forall (F : Q -> Prop) (r lo hi s x : Q),
    lo < r -> r < hi -> 0 <= r ->
    (forall f, F f -> f <= lo \/ f == r \/ hi <= f) ->
    0 <= s -> s * s == x ->
    (0 < (lo + r) * (1 # 2) -> ((lo + r) * (1 # 2)) * ((lo + r) * (1 # 2)) <= x) ->
    x <= ((r + hi) * (1 # 2)) * ((r + hi) * (1 # 2)) ->
    forall f, F f -> Qabs (s - r) <= Qabs (s - f).
Proof.
  intros F r lo hi s x Hlo Hhi Hr Hgap Hs Hsx Hbelow Habove f Hf.
  assert (Hup : s <= (r + hi) * (1 # 2)).
  { apply (proj1 (sq_le_iff s ((r + hi) * (1 # 2)) Hs ltac:(lra))). rewrite Hsx. exact Habove. }
  assert (Hdown : (lo + r) * (1 # 2) <= s).
  { destruct (Qlt_le_dec 0 ((lo + r) * (1 # 2))) as [Hpos | Hneg]; [| lra].
    apply (proj1 (sq_le_iff ((lo + r) * (1 # 2)) s ltac:(lra) Hs)). rewrite Hsx. exact (Hbelow Hpos). }
  destruct (Hgap f Hf) as [Hf_lo | [Hf_r | Hf_hi]].
  - (* f <= lo < r: s lies at or above the midpoint of f and r. *)
    apply (proj2 (nearer_iff_above_mid f r s ltac:(lra))). lra.
  - (* f == r: the same distance. *)
    assert (E : Qabs (s - f) == Qabs (s - r)) by (apply Qabs_wd; lra). lra.
  - (* r < hi <= f: s lies at or below the midpoint of r and f. *)
    apply (proj2 (nearer_iff_below_mid r f s ltac:(lra))). lra.
Qed.

(* ── Headline assumptions ────────────────────────────────────────────── *)

Print Assumptions vector_replies_exact.
Print Assumptions root_midpoint_nearest.
