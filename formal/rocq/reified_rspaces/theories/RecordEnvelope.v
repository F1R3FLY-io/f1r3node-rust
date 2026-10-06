(* ═══════════════════════════════════════════════════════════════════════════
   RecordEnvelope.v — the space-owned record envelopes (M-2, P4)

   This module models the records of the native spaces (plan revision M-2,
   P4 design B.2, A11 and A20). A native space keeps its state in ordinary
   host records on its 0xE0 channels. Each record carries a space-owned
   envelope inside its payload, so the host structs, serializers and
   identities do not change:

     datum                 pars = [EList[tag_D, GInt(ord), Nil, Nil], payload...]
     parked continuation   ParBody.body = Par{exprs: [EList[tag_K, GInt(ord), Nil, body]]}
     counter               pars = [EList[tag_S], GInt(n)]
     config                pars = [EList[tag_C], config map]

   A tag is GByteArray([0xE0, 0x52, kind, 1]), version 1 of the envelope of
   its kind. The decoders are strict: a record that is not the exact image
   of its encoder is a MalformedRecord. The config map of I-17 is the
   canonical map {"spaceId", "outer", "inner", "qualifier", "params",
   "theory"} of SpaceConfig::to_par, and SpaceConfig::from_par accepts its
   exact image only.

   Sources
   - Design: the P4 design report B.2 (records), B.7 (authorities) and E.1
     (the headlines of this module). Plan revision M-2 and its section
     "Record model". A11 (a datum may have an empty payload), A20 (tags and
     the config shape) and I-17 (the config is a canonical Par record).
   - Code: the P4 commit of this repository. The line numbers below are the
     line numbers of that commit.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   record_kind, kind_byte, tag_bytes, tag
                            spaces/record.rs:55, 58, 63-68, 80-82, 85
                            (RECORD_TAG_MARK, RECORD_VERSION, RecordKind,
                            tag_bytes, tag)
   sole shapes of rpar      spaces/record.rs:107-142, 145-152 (sole_expr,
                            expr_par)
   list_par                 spaces/record.rs:156-179 (the bit sets of the
                            normalizer)
   list_elements            spaces/record.rs:184-203 (strict flags)
   is_tag, is_nil, natural_of
                            spaces/record.rs:205-210, 212, 214-219
   datum_envelope, wrap_datum, datum_ord, unwrap_datum
                            spaces/record.rs:224-231, 235-245, 248-258,
                            271-286
   continuation_envelope, wrap_continuation, continuation_parts,
   unwrap_continuation      spaces/record.rs:289-296, 301-321, 324-341,
                            359-374
   counter_record, counter_value
                            spaces/record.rs:383-394, 397-406
   config_record, config_map
                            spaces/record.rs:413-420, 423-436
   space_config, to_par, from_par
                            spaces/types/config.rs:31-49, 53-64, 85-106,
                            112-141, 160-208 (the keys, SpaceConfig,
                            to_par, from_par, and string_par, map_par,
                            string_of, bytes_of, map_entries)
   map_par flags            models/src/rust/par_map.rs:48-55, 67-70
                            (create_from_vec: any key or value flag)
   merger_number            rholang/src/rust/interpreter/merging/
                            rholang_merging_logic.rs:274-286, 288-293
                            (try_get_number_with_rnd and
                            encoded_datum_is_number),
                            models/src/rust/rholang/implicits.rs:49-63
                            (single_expr) and casper/src/rust/merging/
                            dag_merger.rs:55-60
                            (binary_data_is_single_number)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   The model does not contain the Rust Par type. The function α below maps
   each Rust Par to one model term.
   - α maps a Par with exactly one GByteArray expression and no other
     component to PBytes with its connective_used flag, and a Par with
     exactly one GInt expression and no other component to PInt with its
     flag. A Par with exactly one GString expression and the flag false is
     PStr.
   - α maps a Par with exactly one EList (one EMap) expression without a
     remainder and no other component to PList (PMap), with the flag of the
     list (map) and the flag of the Par.
   - α maps Par::default() to PNil and every other Par to POther with an
     injective code and its flag.
   α is injective. The Rust Par equality ignores only locally_free
   (models/src/lib.rs:210-225), and the decoders ignore locally_free, so the
   model has no locally_free.
   - A ListParWithRandom is its pars, its random state and two opaque codes
     for the cost authority and the cost stack. A TaggedContinuation is its
     ParBody (an optional body and a random state), a ScalaBodyRef, or no
     body, with an optional guard and an opaque cost authority.
   - An ord is a Z. The code stores an i64. The counters stop at i64::MAX
     with CounterOverflow (spaces/counter.rs:188, 221), so every stored
     ord is a natural number below 2^63.
   - The merger predicate single_gint is fixed by the code on every shape
     that the model names. On an opaque Par the model takes it as a section
     variable, other_single_gint, because no record of this module puts an
     opaque Par first.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. datum_unwrap_wrap               unwrap inverts wrap, and keeps the
                                       payload, the random state, the cost
                                       authority and the cost stack
    2. datum_wrap_injective            wrap is injective
    3. datum_unwrap_sound              unwrap accepts only the image of wrap
    4. continuation_unwrap_wrap        the same for a parked continuation
    5. counter_and_config_roundtrip    counter and config records, and the
                                       config map of I-17, are exact codecs
    6. envelope_kinds_disjoint         no decoder accepts a record of another
                                       kind
    7. no_reified_datum_is_single_number  the merger never reads a Reified
                                       record as a number cell
    8. unknown_version_rejected        a tag of another version or kind fails

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. The file
   imports only the Rocq standard library.

   Failed strategies (kept so that they are not tried again)
   - Deriving the equality of rpar with decide equality fails on the nested
     list of pairs. The module writes rpar_eqb with its own nested
     induction principle, as Encoding.v does.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat ZArith.
From Stdlib Require Strings.String Strings.Ascii.
Import ListNotations.

(* ── 1. Bytes and their order ──────────────────────────────────────────── *)

Definition bytes : Type := list nat.

Fixpoint bytes_compare (a b : bytes) : comparison :=
  match a, b with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | x :: a', y :: b' =>
      match Nat.compare x y with
      | Eq => bytes_compare a' b'
      | Lt => Lt
      | Gt => Gt
      end
  end.

Definition bytes_eqb (a b : bytes) : bool :=
  match bytes_compare a b with Eq => true | _ => false end.

Definition bytes_ltb (a b : bytes) : bool :=
  match bytes_compare a b with Lt => true | _ => false end.

Lemma bytes_compare_refl : forall a, bytes_compare a a = Eq.
Proof.
  induction a as [| x a IH]; simpl; [reflexivity |].
  rewrite Nat.compare_refl. exact IH.
Qed.

Lemma bytes_compare_eq : forall a b, bytes_compare a b = Eq -> a = b.
Proof.
  induction a as [| x a IH]; intros [| y b] H; simpl in H; try discriminate.
  - reflexivity.
  - destruct (Nat.compare x y) eqn:E; try discriminate.
    apply Nat.compare_eq_iff in E. subst y. f_equal. apply IH. exact H.
Qed.

Lemma bytes_compare_antisym :
  forall a b, bytes_compare b a = CompOpp (bytes_compare a b).
Proof.
  induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
  rewrite (Nat.compare_antisym x y).
  destruct (Nat.compare x y); simpl; [apply IH | reflexivity | reflexivity].
Qed.

Lemma bytes_compare_lt_trans :
  forall a b c,
    bytes_compare a b = Lt -> bytes_compare b c = Lt -> bytes_compare a c = Lt.
Proof.
  induction a as [| x a IH]; intros [| y b] [| z c] Hab Hbc; simpl in *;
    try discriminate; try reflexivity.
  destruct (Nat.compare x y) eqn:Exy; try discriminate;
  destruct (Nat.compare y z) eqn:Eyz; try discriminate.
  - apply Nat.compare_eq_iff in Exy. apply Nat.compare_eq_iff in Eyz. subst.
    rewrite Nat.compare_refl. exact (IH b c Hab Hbc).
  - apply Nat.compare_eq_iff in Exy. subst. rewrite Eyz. reflexivity.
  - apply Nat.compare_eq_iff in Eyz. subst. rewrite Exy. reflexivity.
  - apply Nat.compare_lt_iff in Exy. apply Nat.compare_lt_iff in Eyz.
    assert (Hxz : Nat.compare x z = Lt) by (apply Nat.compare_lt_iff; lia).
    rewrite Hxz. reflexivity.
Qed.

Lemma bytes_eqb_iff : forall a b, bytes_eqb a b = true <-> a = b.
Proof.
  intros a b. unfold bytes_eqb. split.
  - destruct (bytes_compare a b) eqn:E; intro H; try discriminate.
    apply bytes_compare_eq. exact E.
  - intros ->. rewrite bytes_compare_refl. reflexivity.
Qed.

Lemma bytes_ltb_irrefl : forall a, bytes_ltb a a = false.
Proof. intros a. unfold bytes_ltb. rewrite bytes_compare_refl. reflexivity. Qed.

Lemma bytes_ltb_trans :
  forall a b c, bytes_ltb a b = true -> bytes_ltb b c = true -> bytes_ltb a c = true.
Proof.
  unfold bytes_ltb. intros a b c H1 H2.
  destruct (bytes_compare a b) eqn:E1; try discriminate.
  destruct (bytes_compare b c) eqn:E2; try discriminate.
  rewrite (bytes_compare_lt_trans a b c E1 E2). reflexivity.
Qed.

Lemma bytes_ltb_asym : forall a b, bytes_ltb a b = true -> bytes_ltb b a = false.
Proof.
  unfold bytes_ltb. intros a b H. rewrite bytes_compare_antisym.
  destruct (bytes_compare a b); try discriminate. reflexivity.
Qed.

(* The ASCII bytes of the string constants of the code. The code compares
   String keys bytewise. *)
Module Str.
  Import (notations) Stdlib.Strings.String.
  Local Open Scope string_scope.
  Definition b (s : String.string) : list nat :=
    map Ascii.nat_of_ascii (String.list_ascii_of_string s).
  Definition space_id := b "spaceId".
  Definition outer := b "outer".
  Definition inner := b "inner".
  Definition qualifier := b "qualifier".
  Definition params := b "params".
  Definition theory := b "theory".
  Definition default := b "default".
  Definition temp := b "temp".
  Definition seq := b "seq".
End Str.

Definition key_space_id : bytes := Str.space_id.
Definition key_outer : bytes := Str.outer.
Definition key_inner : bytes := Str.inner.
Definition key_qualifier : bytes := Str.qualifier.
Definition key_params : bytes := Str.params.
Definition key_theory : bytes := Str.theory.

(* ── 2. The Par model ──────────────────────────────────────────────────── *)

(* Each constructor is the α-image of one Rust Par shape (see the header).
   The flags are connective_used flags. *)
Inductive rpar : Type :=
| PBytes : bytes -> bool -> rpar
| PInt : Z -> bool -> rpar
| PStr : bytes -> rpar
| PList : list rpar -> bool -> bool -> rpar
| PMap : list (rpar * rpar) -> bool -> bool -> rpar
| PNil : rpar
| POther : nat -> bool -> rpar.

Section RparNestedInduction.
  Variable P : rpar -> Prop.
  Hypothesis case_bytes : forall b f, P (PBytes b f).
  Hypothesis case_int : forall z f, P (PInt z f).
  Hypothesis case_str : forall s, P (PStr s).
  Hypothesis case_list : forall ps lf pf, Forall P ps -> P (PList ps lf pf).
  Hypothesis case_map :
    forall kvs mf pf, Forall (fun kv => P (fst kv) /\ P (snd kv)) kvs -> P (PMap kvs mf pf).
  Hypothesis case_nil : P PNil.
  Hypothesis case_other : forall n f, P (POther n f).

  Fixpoint rpar_nested_ind (t : rpar) : P t :=
    let fix on_list (ps : list rpar) : Forall P ps :=
      match ps as l return Forall P l with
      | [] => Forall_nil P
      | p :: rest => @Forall_cons rpar P p rest (rpar_nested_ind p) (on_list rest)
      end in
    let fix on_map (kvs : list (rpar * rpar)) : Forall (fun kv => P (fst kv) /\ P (snd kv)) kvs :=
      match kvs as l return Forall (fun kv => P (fst kv) /\ P (snd kv)) l with
      | [] => Forall_nil _
      | kv :: rest =>
          match kv as kv0 return Forall (fun kv => P (fst kv) /\ P (snd kv)) (kv0 :: rest) with
          | (k, v) =>
              @Forall_cons (rpar * rpar) (fun kv => P (fst kv) /\ P (snd kv)) (k, v) rest
                (conj (rpar_nested_ind k) (rpar_nested_ind v)) (on_map rest)
          end
      end in
    match t as t0 return P t0 with
    | PBytes b f => case_bytes b f
    | PInt z f => case_int z f
    | PStr s => case_str s
    | PList ps lf pf => case_list ps lf pf (on_list ps)
    | PMap kvs mf pf => case_map kvs mf pf (on_map kvs)
    | PNil => case_nil
    | POther n f => case_other n f
    end.
End RparNestedInduction.

Fixpoint list_eqb {A : Type} (eqb : A -> A -> bool) (xs ys : list A) : bool :=
  match xs, ys with
  | [], [] => true
  | x :: xs', y :: ys' => eqb x y && list_eqb eqb xs' ys'
  | _, _ => false
  end.

Lemma list_eqb_sound :
  forall (A : Type) (eqb : A -> A -> bool) (xs : list A),
    Forall (fun x => forall y, eqb x y = true -> x = y) xs ->
    forall ys, list_eqb eqb xs ys = true -> xs = ys.
Proof.
  intros A eqb xs HF. induction HF as [| x xs Hx HF IH]; intros [| y ys] H;
    simpl in H; try discriminate.
  - reflexivity.
  - apply andb_true_iff in H as [H1 H2]. f_equal; [apply Hx; exact H1 | apply IH; exact H2].
Qed.

Lemma list_eqb_refl :
  forall (A : Type) (eqb : A -> A -> bool) (xs : list A),
    Forall (fun x => eqb x x = true) xs -> list_eqb eqb xs xs = true.
Proof.
  intros A eqb xs HF. induction HF as [| x xs Hx HF IH]; simpl; [reflexivity |].
  rewrite Hx, IH. reflexivity.
Qed.

(* rpar_eqb decides the Rust Par equality, which compares the flags. *)
Fixpoint rpar_eqb (x y : rpar) {struct x} : bool :=
  match x, y with
  | PBytes a f, PBytes b g => bytes_eqb a b && Bool.eqb f g
  | PInt a f, PInt b g => Z.eqb a b && Bool.eqb f g
  | PStr a, PStr b => bytes_eqb a b
  | PList xs lf pf, PList ys lg pg =>
      list_eqb rpar_eqb xs ys && Bool.eqb lf lg && Bool.eqb pf pg
  | PMap xs mf pf, PMap ys mg pg =>
      list_eqb (fun kv1 kv2 => rpar_eqb (fst kv1) (fst kv2) && rpar_eqb (snd kv1) (snd kv2)) xs ys
      && Bool.eqb mf mg && Bool.eqb pf pg
  | PNil, PNil => true
  | POther a f, POther b g => Nat.eqb a b && Bool.eqb f g
  | _, _ => false
  end.

Lemma rpar_eqb_sound : forall x y, rpar_eqb x y = true -> x = y.
Proof.
  induction x as [b f | z f | s | ps lf pf IH | kvs mf pf IH | | n f] using rpar_nested_ind;
    intros [b' f' | z' f' | s' | ps' lf' pf' | kvs' mf' pf' | | n' f'] H;
    simpl in H; try discriminate.
  - apply andb_true_iff in H as [H1 H2]. apply bytes_eqb_iff in H1. apply eqb_prop in H2.
    subst. reflexivity.
  - apply andb_true_iff in H as [H1 H2]. apply Z.eqb_eq in H1. apply eqb_prop in H2.
    subst. reflexivity.
  - apply bytes_eqb_iff in H. subst. reflexivity.
  - apply andb_true_iff in H as [H H3]. apply andb_true_iff in H as [H1 H2].
    apply eqb_prop in H2. apply eqb_prop in H3. subst.
    f_equal. exact (list_eqb_sound _ _ _ IH _ H1).
  - apply andb_true_iff in H as [H H3]. apply andb_true_iff in H as [H1 H2].
    apply eqb_prop in H2. apply eqb_prop in H3. subst.
    f_equal. refine (list_eqb_sound _ _ _ _ _ H1).
    apply Forall_forall. intros [k v] Hin [k' v'] Hkv. simpl in Hkv.
    apply andb_true_iff in Hkv as [Hk Hv].
    rewrite Forall_forall in IH. destruct (IH (k, v) Hin) as [IHk IHv]. simpl in IHk, IHv.
    rewrite (IHk k' Hk), (IHv v' Hv). reflexivity.
  - reflexivity.
  - apply andb_true_iff in H as [H1 H2]. apply Nat.eqb_eq in H1. apply eqb_prop in H2.
    subst. reflexivity.
Qed.

Lemma rpar_eqb_refl : forall x, rpar_eqb x x = true.
Proof.
  induction x as [b f | z f | s | ps lf pf IH | kvs mf pf IH | | n f] using rpar_nested_ind;
    simpl.
  - rewrite (proj2 (bytes_eqb_iff b b) eq_refl), eqb_reflx. reflexivity.
  - rewrite Z.eqb_refl, eqb_reflx. reflexivity.
  - apply bytes_eqb_iff. reflexivity.
  - rewrite (list_eqb_refl _ _ _ IH), !eqb_reflx. reflexivity.
  - rewrite list_eqb_refl, !eqb_reflx; [reflexivity |].
    apply Forall_forall. intros [k v] Hin. rewrite Forall_forall in IH.
    destruct (IH (k, v) Hin) as [IHk IHv]. simpl in *. rewrite IHk, IHv. reflexivity.
  - reflexivity.
  - rewrite Nat.eqb_refl, eqb_reflx. reflexivity.
Qed.

Lemma rpar_eqb_iff : forall x y, rpar_eqb x y = true <-> x = y.
Proof. intros x y. split; [apply rpar_eqb_sound | intros ->; apply rpar_eqb_refl]. Qed.

(* The connective_used flag of the Par. *)
Definition flag (p : rpar) : bool :=
  match p with
  | PBytes _ f | PInt _ f => f
  | PStr _ | PNil => false
  | PList _ _ pf | PMap _ _ pf => pf
  | POther _ f => f
  end.

(* list_par: a list with the bit sets of the normalizer. The flags of the
   list and of the Par are the disjunction of the element flags. *)
Definition list_par (ps : list rpar) : rpar :=
  PList ps (existsb flag ps) (existsb flag ps).

(* list_elements: the elements of exactly one list without a remainder,
   whose two flags are the disjunction of the element flags. *)
Definition list_elements (p : rpar) : option (list rpar) :=
  match p with
  | PList ps lf pf =>
      if Bool.eqb lf (existsb flag ps) && Bool.eqb pf (existsb flag ps) then Some ps else None
  | _ => None
  end.

Lemma list_elements_list_par : forall ps, list_elements (list_par ps) = Some ps.
Proof. intros ps. unfold list_elements, list_par. rewrite !eqb_reflx. reflexivity. Qed.

Lemma list_elements_sound : forall p ps, list_elements p = Some ps -> p = list_par ps.
Proof.
  intros [b f | z f | s | qs lf pf | kvs mf pf | | n f] ps H; simpl in H; try discriminate.
  destruct (Bool.eqb lf (existsb flag qs) && Bool.eqb pf (existsb flag qs)) eqn:E;
    [| discriminate].
  injection H as <-. apply andb_true_iff in E as [E1 E2].
  apply eqb_prop in E1. apply eqb_prop in E2. subst. reflexivity.
Qed.

(* ── 3. Record kinds and tags ──────────────────────────────────────────── *)

Inductive record_kind : Type := RDatum | RContinuation | RCounter | RConfig.

(* The kind byte of a tag: the ASCII letter D, K, S or C. *)
Definition kind_byte (k : record_kind) : nat :=
  match k with RDatum => 68 | RContinuation => 75 | RCounter => 83 | RConfig => 67 end.

Definition reified_tag : nat := 224.
Definition record_tag_mark : nat := 82.
Definition record_version : nat := 1.

(* RecordKind::tag_bytes and RecordKind::tag. *)
Definition tag_bytes (k : record_kind) : bytes :=
  [reified_tag; record_tag_mark; kind_byte k; record_version].

Definition tag (k : record_kind) : rpar := PBytes (tag_bytes k) false.

(* is_tag: exactly the bytes of the tag, with the flag false. *)
Definition is_tag (p : rpar) (k : record_kind) : bool :=
  match p with
  | PBytes b false => bytes_eqb b (tag_bytes k)
  | _ => false
  end.

Definition is_nil (p : rpar) : bool := match p with PNil => true | _ => false end.

(* natural_of: a GInt of at least 0, with the flag false. *)
Definition natural_of (p : rpar) : option Z :=
  match p with
  | PInt z false => if (0 <=? z)%Z then Some z else None
  | _ => None
  end.

Definition int_par (z : Z) : rpar := PInt z false.

Lemma is_tag_iff : forall p k, is_tag p k = true <-> p = tag k.
Proof.
  intros [b [|] | z f | s | ps lf pf | kvs mf pf | | n f] k; simpl; split; intro H;
    try discriminate; try (inversion H; fail).
  - apply bytes_eqb_iff in H. subst. reflexivity.
  - injection H as ->. apply bytes_eqb_iff. reflexivity.
Qed.

Lemma natural_of_int : forall z, (0 <= z)%Z -> natural_of (int_par z) = Some z.
Proof. intros z Hz. unfold natural_of, int_par. apply Z.leb_le in Hz. rewrite Hz. reflexivity. Qed.

Lemma natural_of_sound : forall p z, natural_of p = Some z -> p = int_par z /\ (0 <= z)%Z.
Proof.
  intros [b f | z' [|] | s | ps lf pf | kvs mf pf | | n f] z H; simpl in H; try discriminate.
  destruct (0 <=? z')%Z eqn:E; [| discriminate]. injection H as <-.
  split; [reflexivity | apply Z.leb_le; exact E].
Qed.

(* ── 4. Records ────────────────────────────────────────────────────────── *)

(* A ListParWithRandom: its pars, its random state, and opaque codes for
   its cost authority and its cost stack. *)
Record lpr : Type := mkLpr {
  lp_pars : list rpar;
  lp_random : bytes;
  lp_authority : option nat;
  lp_stack : option nat
}.

(* The envelope of a datum: [tag_D, GInt(ord), Nil, Nil]. *)
Definition datum_envelope (ord : Z) : rpar :=
  list_par [tag RDatum; int_par ord; PNil; PNil].

Definition wrap_datum (ord : Z) (d : lpr) : lpr :=
  mkLpr (datum_envelope ord :: lp_pars d) (lp_random d) (lp_authority d) (lp_stack d).

(* datum_ord: only the envelope is decoded. *)
Definition datum_ord (r : lpr) : option Z :=
  match lp_pars r with
  | first :: _ =>
      match list_elements first with
      | Some [t; o; priority; embedding] =>
          if is_tag t RDatum && is_nil priority && is_nil embedding then natural_of o else None
      | _ => None
      end
  | [] => None
  end.

(* unwrap_datum: the order and the datum as the program sent it. *)
Definition unwrap_datum (r : lpr) : option (Z * lpr) :=
  match datum_ord r with
  | Some ord =>
      match lp_pars r with
      | _ :: payload => Some (ord, mkLpr payload (lp_random r) (lp_authority r) (lp_stack r))
      | [] => None
      end
  | None => None
  end.

(* A TaggedContinuation: a ParBody (an optional body and a random state), a
   ScalaBodyRef, or no body, with a guard and an opaque cost authority. *)
Inductive tcont : Type :=
| TParBody : option rpar -> bytes -> tcont
| TScala : nat -> tcont
| TNone : tcont.

Record tagged : Type := mkTagged {
  t_cont : tcont;
  t_guard : option rpar;
  t_authority : option nat
}.

Definition continuation_envelope (ord : Z) (body : rpar) : rpar :=
  list_par [tag RContinuation; int_par ord; PNil; body].

Definition wrap_continuation (ord : Z) (k : tagged) : option tagged :=
  match t_cont k with
  | TParBody (Some body) rnd =>
      Some (mkTagged (TParBody (Some (continuation_envelope ord body)) rnd)
              (t_guard k) (t_authority k))
  | TParBody None _ | TScala _ | TNone => None
  end.

(* continuation_parts: the order, the original body and the random state. *)
Definition continuation_parts (k : tagged) : option (Z * rpar * bytes) :=
  match t_cont k with
  | TParBody (Some b) rnd =>
      match list_elements b with
      | Some [t; o; policy; original] =>
          if is_tag t RContinuation && is_nil policy then
            match natural_of o with Some z => Some (z, original, rnd) | None => None end
          else None
      | _ => None
      end
  | TParBody None _ | TScala _ | TNone => None
  end.

Definition unwrap_continuation (k : tagged) : option (Z * tagged) :=
  match continuation_parts k with
  | Some (z, original, rnd) =>
      Some (z, mkTagged (TParBody (Some original) rnd) (t_guard k) (t_authority k))
  | None => None
  end.

(* counter_record and counter_value. *)
Definition counter_record (value : Z) (rnd : bytes) (authority : option nat) : lpr :=
  mkLpr [list_par [tag RCounter]; int_par value] rnd authority None.

Definition counter_value (r : lpr) : option Z :=
  match lp_pars r, lp_stack r with
  | [envelope; value], None =>
      match list_elements envelope with
      | Some [t] => if is_tag t RCounter then natural_of value else None
      | _ => None
      end
  | _, _ => None
  end.

(* config_record and config_map. The record has no cost authority. *)
Definition config_record (config : rpar) (rnd : bytes) : lpr :=
  mkLpr [list_par [tag RConfig]; config] rnd None None.

Definition config_map (r : lpr) : option rpar :=
  match lp_pars r, lp_authority r, lp_stack r with
  | [envelope; map], None, None =>
      match list_elements envelope with
      | Some [t] => if is_tag t RConfig then Some map else None
      | _ => None
      end
  | _, _, _ => None
  end.

(* ── 5. Headlines 1 to 3: datum records ────────────────────────────────── *)

Lemma datum_ord_wrap : forall ord d, (0 <= ord)%Z -> datum_ord (wrap_datum ord d) = Some ord.
Proof.
  intros ord d Hord. unfold wrap_datum, datum_envelope. cbn [datum_ord lp_pars].
  rewrite list_elements_list_par. cbn -[natural_of]. apply natural_of_int. exact Hord.
Qed.

(* Headline 1. unwrap_datum inverts wrap_datum: the order comes back, and
   the payload, the random state, the cost authority and the cost stack of
   the datum stay as they are. *)
Theorem datum_unwrap_wrap :
  forall ord d, (0 <= ord)%Z -> unwrap_datum (wrap_datum ord d) = Some (ord, d).
Proof.
  intros ord [pars rnd auth stack] Hord. unfold unwrap_datum.
  rewrite (datum_ord_wrap ord _ Hord). reflexivity.
Qed.

(* Headline 2. wrap_datum is injective in the order and the datum. *)
Theorem datum_wrap_injective :
  forall o1 o2 d1 d2, wrap_datum o1 d1 = wrap_datum o2 d2 -> o1 = o2 /\ d1 = d2.
Proof.
  intros o1 o2 [p1 r1 a1 s1] [p2 r2 a2 s2] H.
  unfold wrap_datum, datum_envelope, list_par in H. simpl in H.
  injection H as Ho Hp Hr Ha Hs. subst.
  split; reflexivity.
Qed.

(* Headline 3. unwrap_datum accepts exactly the image of wrap_datum with a
   natural order. *)
Theorem datum_unwrap_sound :
  forall r ord d, unwrap_datum r = Some (ord, d) -> r = wrap_datum ord d /\ (0 <= ord)%Z.
Proof.
  intros [pars rnd auth stack] ord d H. unfold unwrap_datum, datum_ord in H. simpl in H.
  destruct pars as [| first payload]; [discriminate |].
  destruct (list_elements first) as [es |] eqn:Ees; [| discriminate].
  destruct es as [| t [| o [| pr [| em [| x rest]]]]]; try discriminate.
  destruct (is_tag t RDatum && is_nil pr && is_nil em) eqn:Echk; [| discriminate].
  destruct (natural_of o) as [z |] eqn:Eo; [| discriminate].
  injection H as <- <-.
  apply andb_true_iff in Echk as [Echk Hem]. apply andb_true_iff in Echk as [Ht Hpr].
  apply is_tag_iff in Ht. destruct pr; try discriminate. destruct em; try discriminate.
  apply natural_of_sound in Eo as [-> Hz]. subst t.
  apply list_elements_sound in Ees. subst first.
  split; [reflexivity | exact Hz].
Qed.

(* ── 6. Headline 4: parked continuations ───────────────────────────────── *)

(* Headline 4. A ParBody with a body wraps. unwrap_continuation inverts
   wrap_continuation and accepts only its image, and a ScalaBodyRef or a
   missing body has no continuation record. *)
Theorem continuation_unwrap_wrap :
  (forall ord k k', (0 <= ord)%Z -> wrap_continuation ord k = Some k' ->
     unwrap_continuation k' = Some (ord, k)) /\
  (forall k' ord k, unwrap_continuation k' = Some (ord, k) ->
     wrap_continuation ord k = Some k' /\ (0 <= ord)%Z) /\
  (forall ord k, wrap_continuation ord k = None <->
     match t_cont k with TParBody (Some _) _ => False | _ => True end).
Proof.
  split; [| split].
  - intros ord [[[body |] rnd | n |] guard auth] k' Hord H; simpl in H; try discriminate.
    injection H as <-. unfold continuation_envelope, unwrap_continuation, continuation_parts.
    cbn [t_cont t_guard t_authority].
    rewrite list_elements_list_par. cbn -[natural_of]. rewrite natural_of_int by exact Hord.
    reflexivity.
  - intros [[[b |] rnd | n |] guard auth] ord k H;
      unfold unwrap_continuation, continuation_parts in H; simpl in H; try discriminate.
    destruct (list_elements b) as [es |] eqn:Ees; [| discriminate].
    destruct es as [| t [| o [| pr [| original [| x rest]]]]]; try discriminate.
    destruct (is_tag t RContinuation && is_nil pr) eqn:Echk; [| discriminate].
    destruct (natural_of o) as [z |] eqn:Eo; [| discriminate].
    injection H as <- <-.
    apply andb_true_iff in Echk as [Ht Hpr]. apply is_tag_iff in Ht. destruct pr; try discriminate.
    apply natural_of_sound in Eo as [-> Hz]. subst t. apply list_elements_sound in Ees. subst b.
    split; [reflexivity | exact Hz].
  - intros ord [[[body |] rnd | n |] guard auth]; simpl; split; intro H;
      try discriminate; try contradiction; reflexivity.
Qed.

(* ── 7. The config map of I-17 ─────────────────────────────────────────── *)

Inductive qualifier : Type := QDefault | QTemp | QSeq.

Definition qualifier_str (q : qualifier) : bytes :=
  match q with QDefault => Str.default | QTemp => Str.temp | QSeq => Str.seq end.

Definition parse_qualifier (s : bytes) : option qualifier :=
  if bytes_eqb s Str.default then Some QDefault
  else if bytes_eqb s Str.temp then Some QTemp
  else if bytes_eqb s Str.seq then Some QSeq
  else None.

Lemma parse_qualifier_str : forall q, parse_qualifier (qualifier_str q) = Some q.
Proof. intros []; reflexivity. Qed.

(* Entries keyed by strings, as a BTreeMap<String, Par> holds them. *)
Definition entries : Type := list (bytes * rpar).

(* The keys of a canonical map increase strictly. *)
Fixpoint keys_sorted (kvs : entries) : bool :=
  match kvs with
  | [] => true
  | kv :: rest =>
      match rest with
      | [] => true
      | kv' :: _ => bytes_ltb (fst kv) (fst kv') && keys_sorted rest
      end
  end.

(* Insertion that keeps an existing key, so the last value of a duplicate
   key wins in a right fold (the HashMap collect of create_from_vec). *)
Fixpoint insert_entry (kv : bytes * rpar) (l : entries) : entries :=
  match l with
  | [] => [kv]
  | kv' :: rest =>
      match bytes_compare (fst kv) (fst kv') with
      | Lt => kv :: kv' :: rest
      | Eq => kv' :: rest
      | Gt => kv' :: insert_entry kv rest
      end
  end.

Definition sort_entries (kvs : entries) : entries := fold_right insert_entry [] kvs.

Lemma keys_sorted_cons_cons :
  forall kv kv' rest,
    keys_sorted (kv :: kv' :: rest) = bytes_ltb (fst kv) (fst kv') && keys_sorted (kv' :: rest).
Proof. reflexivity. Qed.

Lemma insert_entry_head :
  forall kv kv' rest,
    bytes_ltb (fst kv) (fst kv') = true -> insert_entry kv (kv' :: rest) = kv :: kv' :: rest.
Proof.
  intros kv kv' rest H. unfold bytes_ltb in H. simpl.
  destruct (bytes_compare (fst kv) (fst kv')); try discriminate. reflexivity.
Qed.

Lemma sort_entries_sorted : forall kvs, keys_sorted kvs = true -> sort_entries kvs = kvs.
Proof.
  induction kvs as [| kv rest IH]; intros H; [reflexivity |].
  simpl. destruct rest as [| kv' rest'].
  - reflexivity.
  - rewrite keys_sorted_cons_cons in H. apply andb_true_iff in H as [Hlt Hs].
    fold (sort_entries (kv' :: rest')). rewrite (IH Hs).
    apply insert_entry_head. exact Hlt.
Qed.

(* map_par: one map whose entries ParMap::create_from_vec sorts by key. The
   keys are strings. The flags of the map and the Par are the disjunction
   of the key and value flags (par_map.rs:67-70). A string key has the flag
   false. *)
Definition map_par (kvs : entries) : rpar :=
  let sorted := sort_entries kvs in
  PMap (map (fun kv => (PStr (fst kv), snd kv)) sorted)
    (existsb (fun kv => flag (snd kv)) sorted) (existsb (fun kv => flag (snd kv)) sorted).

(* string_keys: every key is exactly one string with the flag false. *)
Fixpoint string_keys (kvs : list (rpar * rpar)) : option entries :=
  match kvs with
  | [] => Some []
  | (PStr s, v) :: rest =>
      match string_keys rest with Some l => Some ((s, v) :: l) | None => None end
  | _ :: _ => None
  end.

Fixpoint keys_distinct (kvs : entries) : bool :=
  match kvs with
  | [] => true
  | kv :: rest => negb (existsb (fun kv' => bytes_eqb (fst kv) (fst kv')) rest) && keys_distinct rest
  end.

(* map_entries: the entries of exactly one map whose keys are distinct
   strings, in the bytewise order of a BTreeMap. *)
Definition map_entries (p : rpar) : option entries :=
  match p with
  | PMap kvs _ _ =>
      match string_keys kvs with
      | Some l => if keys_distinct l then Some (sort_entries l) else None
      | None => None
      end
  | _ => None
  end.

Fixpoint lookup (key : bytes) (kvs : entries) : option rpar :=
  match kvs with
  | [] => None
  | kv :: rest => if bytes_eqb key (fst kv) then Some (snd kv) else lookup key rest
  end.

Definition string_of (p : rpar) : option bytes := match p with PStr s => Some s | _ => None end.

Definition bytes_of (p : rpar) : option bytes :=
  match p with PBytes b false => Some b | _ => None end.

Record space_config : Type := mkConfig {
  cfg_sid : bytes;
  cfg_outer : bytes;
  cfg_inner : bytes;
  cfg_qualifier : qualifier;
  cfg_params : entries
}.

(* A config that SpaceConfig can hold: a 32-byte space id and params with
   strictly increasing keys (a BTreeMap). *)
Definition config_ok (c : space_config) : Prop :=
  length (cfg_sid c) = 32 /\ keys_sorted (cfg_params c) = true.

(* SpaceConfig::to_par. *)
Definition to_par (c : space_config) : rpar :=
  map_par [(key_space_id, PBytes (cfg_sid c) false);
           (key_outer, PStr (cfg_outer c));
           (key_inner, PStr (cfg_inner c));
           (key_qualifier, PStr (qualifier_str (cfg_qualifier c)));
           (key_params, map_par (cfg_params c));
           (key_theory, PNil)].

(* SpaceConfig::from_par: the entries, their count, the six values, the Nil
   theory, and the final check to_par(config) == map. *)
Definition from_par (m : rpar) : option space_config :=
  match map_entries m with
  | None => None
  | Some es =>
      if negb (length es =? 6) then None else
      match lookup key_space_id es, lookup key_outer es, lookup key_inner es,
            lookup key_qualifier es, lookup key_params es, lookup key_theory es with
      | Some sidv, Some outerv, Some innerv, Some qv, Some paramsv, Some theoryv =>
          match bytes_of sidv, string_of outerv, string_of innerv, string_of qv,
                map_entries paramsv with
          | Some sid, Some outer, Some inner, Some qs, Some params =>
              if negb (length sid =? 32) then None else
              match parse_qualifier qs with
              | Some q =>
                  let c := mkConfig sid outer inner q params in
                  if is_nil theoryv && rpar_eqb (to_par c) m then Some c else None
              | None => None
              end
          | _, _, _, _, _ => None
          end
      | _, _, _, _, _, _ => None
      end
  end.

(* The six keys of a config map sort as inner, outer, params, qualifier,
   spaceId, theory. *)
Lemma to_par_shape :
  forall c, exists f,
    to_par c =
    PMap [(PStr key_inner, PStr (cfg_inner c)); (PStr key_outer, PStr (cfg_outer c));
          (PStr key_params, map_par (cfg_params c));
          (PStr key_qualifier, PStr (qualifier_str (cfg_qualifier c)));
          (PStr key_space_id, PBytes (cfg_sid c) false); (PStr key_theory, PNil)] f f.
Proof. intros [sid outer inner q params]. eexists. reflexivity. Qed.

Lemma keys_sorted_cons :
  forall kv l,
    keys_sorted (kv :: l) = true <->
    keys_sorted l = true /\ Forall (fun kv' => bytes_ltb (fst kv) (fst kv') = true) l.
Proof.
  intros kv l. revert kv. induction l as [| kv' l IH]; intros kv.
  - simpl. split; [intros _; split; [reflexivity | constructor] | intros _; reflexivity].
  - rewrite keys_sorted_cons_cons. rewrite andb_true_iff. split.
    + intros [Hlt Hs]. split; [exact Hs |].
      apply IH in Hs as [_ Hall]. constructor; [exact Hlt |].
      eapply Forall_impl; [| exact Hall].
      intros x Hx. exact (bytes_ltb_trans _ _ _ Hlt Hx).
    + intros [Hs Hall]. inversion Hall as [| ? ? Hlt _]; subst. split; assumption.
Qed.

Lemma insert_entry_in :
  forall kv l x, In x (insert_entry kv l) -> x = kv \/ In x l.
Proof.
  intros kv l x. induction l as [| kv' l IH]; simpl.
  - intros [H | []]. left. symmetry. exact H.
  - destruct (bytes_compare (fst kv) (fst kv')); simpl.
    + intros [H | H]; [right; left; exact H | right; right; exact H].
    + intros [H | [H | H]];
        [left; symmetry; exact H | right; left; exact H | right; right; exact H].
    + intros [H | H]; [right; left; exact H |].
      apply IH in H as [H | H]; [left; exact H | right; right; exact H].
Qed.

Lemma insert_entry_sorted :
  forall kv l, keys_sorted l = true -> keys_sorted (insert_entry kv l) = true.
Proof.
  intros kv l. induction l as [| kv' l IH]; intros Hs; [reflexivity |].
  apply keys_sorted_cons in Hs as [Hs Hall]. simpl.
  destruct (bytes_compare (fst kv) (fst kv')) eqn:E.
  - apply keys_sorted_cons. split; assumption.
  - apply keys_sorted_cons. split.
    + apply keys_sorted_cons. split; assumption.
    + constructor; [unfold bytes_ltb; rewrite E; reflexivity |].
      eapply Forall_impl; [| exact Hall].
      intros x Hx. apply (bytes_ltb_trans _ (fst kv')); [unfold bytes_ltb; rewrite E; reflexivity | exact Hx].
  - apply keys_sorted_cons. split; [apply IH; exact Hs |].
    apply Forall_forall. intros x Hx. apply insert_entry_in in Hx as [-> | Hx].
    + unfold bytes_ltb. rewrite bytes_compare_antisym, E. reflexivity.
    + rewrite Forall_forall in Hall. apply Hall. exact Hx.
Qed.

Lemma sort_entries_keys_sorted : forall l, keys_sorted (sort_entries l) = true.
Proof.
  induction l as [| kv l IH]; [reflexivity |].
  simpl. apply insert_entry_sorted. exact IH.
Qed.

Lemma string_keys_map :
  forall kvs, string_keys (map (fun kv => (PStr (fst kv), snd kv)) kvs) = Some kvs.
Proof.
  induction kvs as [| [k v] rest IH]; [reflexivity |]. simpl. rewrite IH. reflexivity.
Qed.

Lemma keys_sorted_tail : forall kv rest, keys_sorted (kv :: rest) = true -> keys_sorted rest = true.
Proof.
  intros kv [| kv' rest] H; [reflexivity |].
  rewrite keys_sorted_cons_cons in H. apply andb_true_iff in H as [_ H]. exact H.
Qed.

Lemma keys_sorted_head_lt :
  forall kv rest, keys_sorted (kv :: rest) = true ->
    Forall (fun kv' => bytes_ltb (fst kv) (fst kv') = true) rest.
Proof.
  intros kv rest. revert kv. induction rest as [| kv' rest IH]; intros kv H; [constructor |].
  rewrite keys_sorted_cons_cons in H. apply andb_true_iff in H as [Hlt Hs].
  constructor; [exact Hlt |].
  eapply Forall_impl; [| exact (IH kv' Hs)].
  intros x Hx. exact (bytes_ltb_trans _ _ _ Hlt Hx).
Qed.

Lemma keys_sorted_distinct : forall kvs, keys_sorted kvs = true -> keys_distinct kvs = true.
Proof.
  induction kvs as [| kv rest IH]; intros H; [reflexivity |].
  simpl. rewrite (IH (keys_sorted_tail kv rest H)), andb_true_r.
  apply negb_true_iff. apply Bool.not_true_iff_false. intros Hex.
  apply existsb_exists in Hex as [kv' [Hin Heq]]. apply bytes_eqb_iff in Heq.
  pose proof (keys_sorted_head_lt kv rest H) as Hall. rewrite Forall_forall in Hall.
  specialize (Hall kv' Hin). rewrite Heq, bytes_ltb_irrefl in Hall. discriminate.
Qed.

Lemma map_entries_map_par :
  forall kvs, keys_sorted kvs = true -> map_entries (map_par kvs) = Some kvs.
Proof.
  intros kvs Hs. unfold map_entries, map_par. rewrite (sort_entries_sorted kvs Hs).
  rewrite string_keys_map, (keys_sorted_distinct kvs Hs), (sort_entries_sorted kvs Hs).
  reflexivity.
Qed.

Lemma map_entries_config_shape :
  forall a b cc d e g f,
    map_entries (PMap [(PStr key_inner, a); (PStr key_outer, b); (PStr key_params, cc);
                       (PStr key_qualifier, d); (PStr key_space_id, e); (PStr key_theory, g)] f f) =
    Some [(key_inner, a); (key_outer, b); (key_params, cc); (key_qualifier, d);
          (key_space_id, e); (key_theory, g)].
Proof. intros. vm_compute. reflexivity. Qed.

Lemma lookup_config_shape :
  forall a b cc d e g,
    let es := [(key_inner, a); (key_outer, b); (key_params, cc); (key_qualifier, d);
               (key_space_id, e); (key_theory, g)] in
    lookup key_space_id es = Some e /\ lookup key_outer es = Some b /\
    lookup key_inner es = Some a /\ lookup key_qualifier es = Some d /\
    lookup key_params es = Some cc /\ lookup key_theory es = Some g /\ length es = 6.
Proof. intros. repeat split; vm_compute; reflexivity. Qed.

Lemma from_par_to_par : forall c, config_ok c -> from_par (to_par c) = Some c.
Proof.
  intros c [Hsid Hparams]. destruct (to_par_shape c) as [f Hshape].
  unfold from_par. rewrite Hshape at 1. rewrite map_entries_config_shape.
  destruct (lookup_config_shape (PStr (cfg_inner c)) (PStr (cfg_outer c)) (map_par (cfg_params c))
              (PStr (qualifier_str (cfg_qualifier c))) (PBytes (cfg_sid c) false) PNil)
    as [L1 [L2 [L3 [L4 [L5 [L6 L7]]]]]].
  rewrite L1, L2, L3, L4, L5, L6, L7. cbn [negb Nat.eqb bytes_of string_of].
  rewrite (map_entries_map_par _ Hparams), Hsid. cbn [negb Nat.eqb].
  rewrite parse_qualifier_str. cbn [is_nil andb].
  destruct c as [sid outer inner q params]. cbn [cfg_sid cfg_outer cfg_inner cfg_qualifier cfg_params].
  rewrite rpar_eqb_refl. reflexivity.
Qed.

Lemma from_par_sound : forall m c, from_par m = Some c -> m = to_par c /\ config_ok c.
Proof.
  intros m c H. unfold from_par in H.
  destruct (map_entries m) as [es |] eqn:Ees; [| discriminate].
  destruct (negb (length es =? 6)); [discriminate |].
  destruct (lookup key_space_id es) as [sidv |]; [| discriminate].
  destruct (lookup key_outer es) as [outerv |]; [| discriminate].
  destruct (lookup key_inner es) as [innerv |]; [| discriminate].
  destruct (lookup key_qualifier es) as [qv |]; [| discriminate].
  destruct (lookup key_params es) as [paramsv |]; [| discriminate].
  destruct (lookup key_theory es) as [theoryv |]; [| discriminate].
  destruct (bytes_of sidv) as [sid |]; [| discriminate].
  destruct (string_of outerv) as [outer |]; [| discriminate].
  destruct (string_of innerv) as [inner |]; [| discriminate].
  destruct (string_of qv) as [qs |]; [| discriminate].
  destruct (map_entries paramsv) as [params |] eqn:Eparams; [| discriminate].
  destruct (negb (length sid =? 32)) eqn:Elen; [discriminate |].
  destruct (parse_qualifier qs) as [q |]; [| discriminate].
  destruct (is_nil theoryv && rpar_eqb (to_par (mkConfig sid outer inner q params)) m) eqn:Echk;
    [| discriminate].
  injection H as <-. apply andb_true_iff in Echk as [_ Heq]. apply rpar_eqb_sound in Heq.
  split; [symmetry; exact Heq |].
  split.
  - apply negb_false_iff in Elen. apply Nat.eqb_eq in Elen. exact Elen.
  - simpl. unfold map_entries in Eparams. destruct paramsv; try discriminate.
    destruct (string_keys l) as [l' |]; [| discriminate].
    destruct (keys_distinct l'); [| discriminate]. injection Eparams as <-.
    apply sort_entries_keys_sorted.
Qed.

(* ── 8. Headline 5: counter and config records ─────────────────────────── *)

(* Headline 5. counter_value inverts counter_record and accepts only its
   image. config_map inverts config_record and accepts only its image.
   SpaceConfig::from_par inverts SpaceConfig::to_par and accepts only its
   image (I-17). *)
Theorem counter_and_config_roundtrip :
  (forall v rnd auth, (0 <= v)%Z -> counter_value (counter_record v rnd auth) = Some v) /\
  (forall r v, counter_value r = Some v ->
     r = counter_record v (lp_random r) (lp_authority r) /\ (0 <= v)%Z) /\
  (forall m rnd, config_map (config_record m rnd) = Some m) /\
  (forall r m, config_map r = Some m -> r = config_record m (lp_random r)) /\
  (forall c, config_ok c -> from_par (to_par c) = Some c) /\
  (forall m c, from_par m = Some c -> m = to_par c /\ config_ok c).
Proof.
  split; [| split; [| split; [| split; [| split]]]].
  - intros v rnd auth Hv. unfold counter_record. cbn [counter_value lp_pars lp_stack].
    rewrite list_elements_list_par. cbn -[natural_of]. apply natural_of_int. exact Hv.
  - intros [pars rnd auth stack] v H. unfold counter_value in H. simpl in H.
    destruct pars as [| envelope [| value [| x rest]]]; try discriminate.
    destruct stack; [discriminate |].
    destruct (list_elements envelope) as [es |] eqn:Ees; [| discriminate].
    destruct es as [| t [| y es']]; try discriminate.
    destruct (is_tag t RCounter) eqn:Ht; [| discriminate].
    apply is_tag_iff in Ht. apply natural_of_sound in H as [-> Hv].
    apply list_elements_sound in Ees. subst. split; [reflexivity | exact Hv].
  - intros m rnd. unfold config_record. cbn [config_map lp_pars lp_authority lp_stack].
    rewrite list_elements_list_par. reflexivity.
  - intros [pars rnd auth stack] m H. unfold config_map in H. simpl in H.
    destruct pars as [| envelope [| map [| x rest]]]; try discriminate.
    destruct auth; [discriminate |]. destruct stack; [discriminate |].
    destruct (list_elements envelope) as [es |] eqn:Ees; [| discriminate].
    destruct es as [| t [| y es']]; try discriminate.
    destruct (is_tag t RConfig) eqn:Ht; [| discriminate].
    injection H as <-. apply is_tag_iff in Ht. apply list_elements_sound in Ees. subst.
    reflexivity.
  - exact from_par_to_par.
  - exact from_par_sound.
Qed.

(* ── 9. Headlines 6 and 8: kinds and versions ─────────────────────────── *)

Lemma tag_bytes_injective : forall k1 k2, tag_bytes k1 = tag_bytes k2 -> k1 = k2.
Proof. intros [] [] H; simpl in H; try discriminate; reflexivity. Qed.

(* Headline 6. The four tags are pairwise distinct, and no decoder accepts a
   record that the encoder of another kind made: a datum never decodes as
   a counter or a config record, a counter or a config record never decodes
   as a datum, and the datum envelope is never a continuation envelope. *)
Theorem envelope_kinds_disjoint :
  (forall k1 k2, tag k1 = tag k2 -> k1 = k2) /\
  (forall v rnd auth, datum_ord (counter_record v rnd auth) = None) /\
  (forall m rnd, datum_ord (config_record m rnd) = None) /\
  (forall ord d, counter_value (wrap_datum ord d) = None) /\
  (forall m rnd, counter_value (config_record m rnd) = None) /\
  (forall ord d, config_map (wrap_datum ord d) = None) /\
  (forall v rnd auth, config_map (counter_record v rnd auth) = None) /\
  (forall ord rnd guard auth,
     continuation_parts (mkTagged (TParBody (Some (datum_envelope ord)) rnd) guard auth) = None) /\
  (forall ord body d,
     datum_ord (mkLpr [continuation_envelope ord body] (lp_random d) (lp_authority d)
                  (lp_stack d)) = None).
Proof.
  split; [| split; [| split; [| split; [| split; [| split; [| split; [| split]]]]]]].
  - intros [] [] H; try reflexivity; discriminate.
  - intros v rnd auth. unfold counter_record. cbn [datum_ord lp_pars].
    rewrite list_elements_list_par. reflexivity.
  - intros m rnd. unfold config_record. cbn [datum_ord lp_pars].
    rewrite list_elements_list_par. reflexivity.
  - intros ord [pars rnd auth stack]. unfold wrap_datum. cbn [counter_value lp_pars lp_stack].
    destruct pars as [| p [| q rest]]; try reflexivity.
    destruct stack; [reflexivity |].
    unfold datum_envelope. rewrite list_elements_list_par. reflexivity.
  - intros m rnd. unfold config_record. cbn [counter_value lp_pars lp_stack].
    rewrite list_elements_list_par. reflexivity.
  - intros ord [pars rnd auth stack]. unfold wrap_datum.
    cbn [config_map lp_pars lp_authority lp_stack].
    destruct pars as [| p [| q rest]]; try reflexivity.
    destruct auth; [reflexivity |]. destruct stack; [reflexivity |].
    unfold datum_envelope. rewrite list_elements_list_par. reflexivity.
  - intros v rnd [a |]; unfold counter_record; cbn [config_map lp_pars lp_authority lp_stack];
      [reflexivity |].
    rewrite list_elements_list_par. reflexivity.
  - intros ord rnd guard auth. unfold datum_envelope, continuation_parts. cbn [t_cont].
    rewrite list_elements_list_par. reflexivity.
  - intros ord body d. unfold continuation_envelope. cbn [datum_ord lp_pars].
    rewrite list_elements_list_par. reflexivity.
Qed.

(* A tag of the kind byte kb and the version v. *)
Definition versioned_tag (kb v : nat) : rpar :=
  PBytes [reified_tag; record_tag_mark; kb; v] false.

(* The four records with the tag t in place of the tag of their kind. *)
Definition datum_with_tag (t : rpar) (ord : Z) (d : lpr) : lpr :=
  mkLpr (list_par [t; int_par ord; PNil; PNil] :: lp_pars d) (lp_random d) (lp_authority d) (lp_stack d).

Definition continuation_with_tag (t : rpar) (ord : Z) (body : rpar) (rnd : bytes) : tagged :=
  mkTagged (TParBody (Some (list_par [t; int_par ord; PNil; body])) rnd) None None.

Definition counter_with_tag (t : rpar) (v : Z) (rnd : bytes) : lpr :=
  mkLpr [list_par [t]; int_par v] rnd None None.

Definition config_with_tag (t : rpar) (m : rpar) (rnd : bytes) : lpr :=
  mkLpr [list_par [t]; m] rnd None None.

Lemma is_tag_versioned :
  forall kb v k, is_tag (versioned_tag kb v) k = true -> kb = kind_byte k /\ v = record_version.
Proof.
  intros kb v k H. apply is_tag_iff in H. injection H as Hkb Hv. split; assumption.
Qed.

(* Headline 8. A tag with another version (or another kind byte) is not the
   tag of the kind, and every decoder rejects a record that carries it. The
   version slot lets a later envelope add fields without a migration. *)
Theorem unknown_version_rejected :
  forall kb v, v <> record_version ->
    (forall k, versioned_tag kb v <> tag k) /\
    (forall ord d, datum_ord (datum_with_tag (versioned_tag kb v) ord d) = None) /\
    (forall ord body rnd,
       continuation_parts (continuation_with_tag (versioned_tag kb v) ord body rnd) = None) /\
    (forall n rnd, counter_value (counter_with_tag (versioned_tag kb v) n rnd) = None) /\
    (forall m rnd, config_map (config_with_tag (versioned_tag kb v) m rnd) = None).
Proof.
  intros kb v Hv.
  assert (Hnot : forall k, is_tag (versioned_tag kb v) k = false).
  { intros k. destruct (is_tag (versioned_tag kb v) k) eqn:E; [| reflexivity].
    apply is_tag_versioned in E as [_ E]. contradiction. }
  split; [| split; [| split; [| split]]].
  - intros k H. pose proof (Hnot k) as Hk. rewrite H in Hk.
    assert (Htrue : is_tag (tag k) k = true) by (apply is_tag_iff; reflexivity).
    rewrite Htrue in Hk. discriminate.
  - intros ord d. unfold datum_with_tag. cbn [datum_ord lp_pars].
    rewrite list_elements_list_par. cbn -[is_tag versioned_tag]. rewrite (Hnot RDatum).
    reflexivity.
  - intros ord body rnd. unfold continuation_with_tag, continuation_parts. cbn [t_cont].
    rewrite list_elements_list_par. cbn -[is_tag versioned_tag]. rewrite (Hnot RContinuation).
    reflexivity.
  - intros n rnd. unfold counter_with_tag. cbn [counter_value lp_pars lp_stack].
    rewrite list_elements_list_par. cbn -[is_tag versioned_tag]. rewrite (Hnot RCounter).
    reflexivity.
  - intros m rnd. unfold config_with_tag. cbn [config_map lp_pars lp_authority lp_stack].
    rewrite list_elements_list_par. cbn -[is_tag versioned_tag]. rewrite (Hnot RConfig).
    reflexivity.
Qed.

(* ── 10. Headline 7: the merger number predicate ───────────────────────── *)

Section MergerNumber.
  (* single_expr (implicits.rs:49-63) ignores the unforgeables, the
     connectives and the conditionals, so an opaque Par can be a single
     GInt. other_single_gint is that unknown predicate on opaque codes. *)
  Variable other_single_gint : nat -> bool -> bool.

  (* RhoNumber::unapply: single_expr is a GInt. *)
  Definition single_gint (p : rpar) : bool :=
    match p with
    | PInt _ _ => true
    | POther n f => other_single_gint n f
    | PBytes _ _ | PStr _ | PList _ _ _ | PMap _ _ _ | PNil => false
    end.

  (* try_get_number_with_rnd(d).is_some(): exactly one par, and it is a
     number. The merger treats such a channel as a single-value cell. *)
  Definition merger_number (d : lpr) : bool :=
    match lp_pars d with
    | [p] => single_gint p
    | _ => false
    end.

  (* Headline 7. The first par of every Reified record is a list, so the
     merger never reads a datum, counter or config record as a number cell,
     also when the payload of a datum is empty (A11). *)
  Theorem no_reified_datum_is_single_number :
    (forall ord d, merger_number (wrap_datum ord d) = false) /\
    (forall v rnd auth, merger_number (counter_record v rnd auth) = false) /\
    (forall m rnd, merger_number (config_record m rnd) = false) /\
    (forall ord d, exists es, hd_error (lp_pars (wrap_datum ord d)) = Some (list_par es)) /\
    (forall v rnd auth, exists es, hd_error (lp_pars (counter_record v rnd auth)) = Some (list_par es)) /\
    (forall m rnd, exists es, hd_error (lp_pars (config_record m rnd)) = Some (list_par es)).
  Proof.
    split; [| split; [| split; [| split; [| split]]]].
    - intros ord [[| p pars] rnd auth stack]; reflexivity.
    - reflexivity.
    - reflexivity.
    - intros ord d. eexists. reflexivity.
    - intros v rnd auth. eexists. reflexivity.
    - intros m rnd. eexists. reflexivity.
  Qed.
End MergerNumber.

(* ── 11. Worked examples and negative controls ─────────────────────────── *)

Example datum_with_empty_payload :
  unwrap_datum (wrap_datum 5 (mkLpr [] [1; 2] None None)) = Some (5%Z, mkLpr [] [1; 2] None None).
Proof. reflexivity. Qed.

Example negative_ord_rejected :
  unwrap_datum (wrap_datum (-1) (mkLpr [PNil] [] None None)) = None.
Proof. reflexivity. Qed.

Example stale_envelope_flag_rejected :
  datum_ord (mkLpr [PList [tag RDatum; int_par 3; PNil; PNil] true true] [] None None) = None.
Proof. reflexivity. Qed.

Example version_two_rejected :
  datum_ord (datum_with_tag (versioned_tag 68 2) 3 (mkLpr [] [] None None)) = None.
Proof. reflexivity. Qed.

Example counter_with_stack_rejected :
  counter_value (mkLpr [list_par [tag RCounter]; int_par 4] [] None (Some 1)) = None.
Proof. reflexivity. Qed.

Example config_with_authority_rejected :
  config_map (mkLpr [list_par [tag RConfig]; PNil] [] (Some 1) None) = None.
Proof. reflexivity. Qed.

(* ── 12. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions datum_unwrap_wrap.
Print Assumptions datum_wrap_injective.
Print Assumptions datum_unwrap_sound.
Print Assumptions continuation_unwrap_wrap.
Print Assumptions counter_and_config_roundtrip.
Print Assumptions envelope_kinds_disjoint.
Print Assumptions no_reified_datum_is_single_number.
Print Assumptions unknown_version_rejected.
