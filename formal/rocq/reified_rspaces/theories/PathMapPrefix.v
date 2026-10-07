(* ═══════════════════════════════════════════════════════════════════════════
   PathMapPrefix.v — the pure layer of the PathMap spaces (P5e, wave W2)

   In a PathMap space a channel is a path, and a send on a path is also a
   send on every prefix of the path (FIPS, "PathMap (For MeTTa)"). This
   module models the parts of the PathMap outer store that read and write
   no store: the path codec, the logical PathMap with the adapter of the
   EPathMap of this tree, and the re-rooting of a delivered argument. The
   engine (markers, token, delivery) adds its headlines in wave W3.

     path_key(p)   = seg(e_1) ‖ … ‖ seg(e_n) ‖ 0x00
     prefix_key(q) = seg(e_1) ‖ … ‖ seg(e_k)
     R(ε, v) = v
     R(s, {||}) = {|s|}                        (s ≠ ε, the FIPS unit rule)
     R(s, m) = {| s ++ elems(e) : e ∈ m |}     (s ≠ ε, m a PathMap)
     R(s, v) = [s_1, …, s_k, v]                (s ≠ ε, v not a PathMap)

   Sources
   - Design: P5 design v3 A.11 and A.13 (the P5e row), P5 design v2 §3.4,
     P5 design report B.7 and F9.
   - Plan: decision 9.3 (FIPS-exact PathMap prefix delivery behind an
     EPathMap adapter that is forward-compatible with EPM1), the section
     "PathMap delivery", decisions 40 (Q3) and 42.
   - EPM1: the segment codec of models/src/rust/canonical_path.rs at
     11c92e7cc on feature/mettail (the tag table, write_uv, zigzag64, the
     stable arms and the escape arm), read only.
   - Code: rholang/src/rust/interpreter/spaces/pathmap/ of this tree. The
     line numbers below are the line numbers of the files of wave W2.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code (spaces/pathmap/…)
   ─────────────────────────────────────────────────────────────────────────
   uv                 path_codec.rs:1146-1170 (uv_len, write_uv)
   read_uv, read_bytes, parse_ser, parse_seg
                      none: the readers exist for the proof. EPM1 has them
                      (read_uv and the trie decoder).
   zigzag             path_codec.rs:1143 (zigzag)
   term               the closed Par that a path holds, as structural_arm
                      and expression_arm classify it (path_codec.rs:988-1091).
                      TList is a plain list (plain_list_elements,
                      path_codec.rs:153-196).
   structural         scan_element (path_codec.rs:514-544) with
                      structural_arm and expression_arm
   ser                Arm::write and write_structural (path_codec.rs:921-961,
                      1095-1109). The arm of TOther stands for the canonical
                      bytes and never starts a Rust segment.
   seg                segment, plan_segment, write_segment and SegmentPlan
                      (path_codec.rs:369-371, 390-408, 460-493). The escape
                      payload is escape_payload (path_codec.rs:1114-1127).
   prefix_key, path_key
                      prefix_key, path_key, entry_key, encode_elements
                      (path_codec.rs:350-366, 413-456)
   is_prefix          is_prefix_key (path_codec.rs:376-379)
   lex_cmp            compare_keys (path_codec.rs:384-387), the Ord of Vec<u8>
   elems              path_elements, into_path_elements, channel_path
                      (path_codec.rs:200-226, 281-286)
   canon              canonical_entry (adapter.rs:267-278) with plain_list
                      (path_codec.rs:258-276)
   insert_path, sort_dedup
                      sorted_unique and LogicalPathMap::set (adapter.rs:138-149,
                      213-262)
   head_decode, head_encode
                      HeadEPathMapAdapter::decode and ::encode
                      (adapter.rs:296-336)
   rl                 LogicalPathMap::prefixed, LogicalPath::prefixed and
                      LogicalPathMap::singleton (adapter.rs:81-106, 166-206)
   pval, reroot       reroot, is_pathmap (reroot.rs:51-75, 93-95)
   cand_lt            compare_keys and the order of a discipline inside one
                      path (the consume scan of the engine, wave W3)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A byte is a nat. No theorem needs the bound 256, because the parser
     delimits every payload by its length.
   - TOther stands for every closed Par outside the structural grammar, with
     its canonical bytes. The Rust escape payload is the prost encoding of
     the sorted Par. The model takes ser instead. Both are injective, and
     the theorems need only injectivity and the length in front.
   - The Rust code rejects a path element that holds an EPathMap or an
     EZipper (UnsupportedPathElement) and a channel path or a prefix of more
     than 64 elements (PathTooDeep, F9). A term holds no PathMap, and no
     theorem needs the depth bound, so both rules are outside the model. The
     Rust tests pathmaps_and_zippers_are_unsupported_at_any_depth and
     the_depth_cap_bounds_channel_paths_and_prefixes cover them.
   - The metering of decision 42 is outside the model (AgentReadCoverage.v
     states the rule). The Rust cut tests cover each metered function.
   - sort_dedup keeps the last entry of a key and the Rust code keeps the
     first. Equal keys hold equal paths (path_key_injective), so the two
     results are equal lists.
   - A send argument is a pval: PMap es for a value that is one EPathMap with
     the entries es (its ps), and POther x for every other value.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. prefix_key_iff_prefix   q ⪯ p exactly when prefix_key(q) is a byte
                               prefix of path_key(p)
    2. reroot_ident            R(ε, v) = v
    3. reroot_compose          R(s1, R(s2, m)) = R(s1 ++ s2, m) for a PathMap,
                               and R(s1 ++ s2, v) prepends s1 to the list
                               R(s2, v) for every other value
    4. reroot_injective        equal deliveries under one suffix come from
                               logically equal values, and under a suffix
                               that is not empty logically equal values have
                               equal deliveries
    5. fips_example            the FIPS example and the FIPS equation
                               @[0,1,2]!({|"hi"|}) = @[0,1,2,"hi"]!({||})
    6. candidate_order_total   the order of the candidates of a receive is
                               a strict total order, the exact path first

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. The file
   imports only the Rocq standard library. The section TermInd states an
   induction principle, and its hypotheses are parameters of that principle.

   Failed strategies (kept so that they are not tried again)
   - Path elements in segment normal form (a constructor SEscape for every
     element outside the structural grammar) cannot hold a plain list with
     such an element. The Rust code splits that list as a channel or an
     entry. The term model with structural replaces it.
   - simpl unfolds 2 * k and 0 ?= t into matches. The proofs use cbn with a
     list of constants, or replace.
   - Bullets whose number depends on the reduction of rl break under the
     negative controls. rl_unit_injective solves each case with
     first [ discriminate | … ].
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat ZArith.
Import ListNotations.

(* ── 1. Bytes, LEB128 and zigzag ──────────────────────────────────────── *)

(* A byte list. A byte of the code is a u8. The model takes nat bytes, and
   no theorem of this module needs the bound 256: the parser delimits every
   payload by its length. *)
Definition bytes : Type := list nat.

(* uv: the minimal unsigned LEB128 form of n (write_uv). The fuel n + 1 is
   more than the number of groups of n. *)
Fixpoint uv_fuel (fuel n : nat) : bytes :=
  match fuel with
  | O => [n]
  | S f => if n <? 128 then [n] else (128 + n mod 128) :: uv_fuel f (n / 128)
  end.

Definition uv (n : nat) : bytes := uv_fuel (S n) n.

(* The reader of a LEB128 number: the groups until a byte below 128. *)
Fixpoint read_uv (bs : bytes) : option (nat * bytes) :=
  match bs with
  | [] => None
  | b :: r =>
      if b <? 128 then Some (b, r)
      else match read_uv r with
           | Some (m, r') => Some (b - 128 + 128 * m, r')
           | None => None
           end
  end.

Lemma read_uv_fuel :
  forall f n r, n < f -> read_uv (uv_fuel f n ++ r) = Some (n, r).
Proof.
  induction f as [| f IH]; intros n r Hn; [lia |].
  cbn [uv_fuel]. destruct (n <? 128) eqn:Hsmall.
  - cbn [app read_uv]. rewrite Hsmall. reflexivity.
  - apply Nat.ltb_ge in Hsmall.
    cbn [app read_uv].
    assert (Hb : (128 + n mod 128 <? 128) = false) by (apply Nat.ltb_ge; lia).
    rewrite Hb.
    assert (Hdiv : n / 128 < f).
    { assert (n / 128 < n) by (apply Nat.div_lt; lia). lia. }
    rewrite (IH (n / 128) r Hdiv).
    f_equal. f_equal.
    pose proof (Nat.div_mod_eq n 128) as Hdm. lia.
Qed.

Lemma read_uv_uv : forall n r, read_uv (uv n ++ r) = Some (n, r).
Proof. intros n r. apply read_uv_fuel. lia. Qed.

(* A payload with its length in front. *)
Definition read_bytes (bs : bytes) : option (bytes * bytes) :=
  match read_uv bs with
  | Some (n, r) => if n <=? length r then Some (firstn n r, skipn n r) else None
  | None => None
  end.

Lemma firstn_app_length :
  forall (l1 l2 : bytes), firstn (length l1) (l1 ++ l2) = l1.
Proof.
  induction l1 as [| x l1 IH]; intros l2; simpl; [reflexivity |].
  rewrite IH. reflexivity.
Qed.

Lemma skipn_app_length :
  forall (l1 l2 : bytes), skipn (length l1) (l1 ++ l2) = l2.
Proof. induction l1 as [| x l1 IH]; intros l2; simpl; [reflexivity | apply IH]. Qed.

Lemma read_bytes_uv :
  forall b r, read_bytes (uv (length b) ++ b ++ r) = Some (b, r).
Proof.
  intros b r. unfold read_bytes. rewrite read_uv_uv.
  assert (Hle : (length b <=? length (b ++ r)) = true)
    by (apply Nat.leb_le; rewrite length_app; lia).
  rewrite Hle, firstn_app_length, skipn_app_length. reflexivity.
Qed.

(* zigzag64 of the codec: a signed integer as an unsigned one. For an i64
   value v, ((v << 1) ^ (v >> 63)) as u64 is 2v when v >= 0 and -2v - 1
   when v < 0. *)
Definition zigzag (z : Z) : nat :=
  match z with
  | Z0 => 0
  | Zpos p => 2 * Pos.to_nat p
  | Zneg p => 2 * Pos.to_nat p - 1
  end.

Definition unzigzag (n : nat) : Z :=
  if Nat.even n then Z.of_nat (Nat.div2 n) else Z.opp (Z.of_nat (Nat.div2 (S n))).

Lemma unzigzag_zigzag : forall z, unzigzag (zigzag z) = z.
Proof.
  intros [| p | p]; unfold unzigzag, zigzag.
  - reflexivity.
  - replace (Nat.even (2 * Pos.to_nat p)) with true
      by (rewrite Nat.even_mul; reflexivity).
    rewrite Nat.div2_double. apply positive_nat_Z.
  - pose proof (Pos2Nat.is_pos p) as Hpos.
    destruct (Pos.to_nat p) as [| k] eqn:Hk; [lia |].
    replace (2 * S k - 1) with (S (2 * k)) by lia.
    replace (Nat.even (S (2 * k))) with false
      by (rewrite Nat.even_succ, Nat.odd_mul; reflexivity).
    replace (S (S (2 * k))) with (2 * S k) by lia.
    rewrite Nat.div2_double, <- Hk, positive_nat_Z. reflexivity.
Qed.

(* ── 2. Path elements and the segment codec ──────────────────────────── *)

(* A closed Rust Par without a PathMap or a zipper, as the codec sees it.
   TList is a plain list (one EList with no remainder, empty bit sets and no
   other part) of any elements. TTuple is a ground tuple. TOther is every
   other Par: a set, a map, a process, a name that is not a GPrivate, a
   GFloat32, a Par with more than one part or with bit sets, or a list that
   is not plain. It holds the canonical bytes of the Par. The f64 bits are 8
   bytes. *)
Inductive term : Type :=
| TBool (b : bool)
| TInt (z : Z)
| TStr (s : bytes)
| TUri (s : bytes)
| TBytes (b : bytes)
| TDouble (b0 b1 b2 b3 b4 b5 b6 b7 : nat)
| TBigInt (b : bytes)
| TBigRat (num den : bytes)
| TFixed (unscaled : bytes) (scale : nat)
| TList (xs : list term)
| TTuple (xs : list term)
| TPriv (id : bytes)
| TOther (canonical : bytes).

(* The induction principle with a hypothesis for every element of a list or
   a tuple. *)
Section TermInd.
  Variable P : term -> Prop.
  Hypothesis case_bool : forall b, P (TBool b).
  Hypothesis case_int : forall z, P (TInt z).
  Hypothesis case_str : forall s, P (TStr s).
  Hypothesis case_uri : forall s, P (TUri s).
  Hypothesis case_bytes : forall b, P (TBytes b).
  Hypothesis case_double :
    forall b0 b1 b2 b3 b4 b5 b6 b7, P (TDouble b0 b1 b2 b3 b4 b5 b6 b7).
  Hypothesis case_bigint : forall b, P (TBigInt b).
  Hypothesis case_bigrat : forall n d, P (TBigRat n d).
  Hypothesis case_fixed : forall u s, P (TFixed u s).
  Hypothesis case_list : forall xs, Forall P xs -> P (TList xs).
  Hypothesis case_tuple : forall xs, Forall P xs -> P (TTuple xs).
  Hypothesis case_priv : forall id, P (TPriv id).
  Hypothesis case_other : forall b, P (TOther b).

  Fixpoint term_nested_ind (t : term) : P t :=
    match t with
    | TBool b => case_bool b
    | TInt z => case_int z
    | TStr s => case_str s
    | TUri s => case_uri s
    | TBytes b => case_bytes b
    | TDouble b0 b1 b2 b3 b4 b5 b6 b7 => case_double b0 b1 b2 b3 b4 b5 b6 b7
    | TBigInt b => case_bigint b
    | TBigRat n d => case_bigrat n d
    | TFixed u s => case_fixed u s
    | TList xs =>
        case_list xs
          ((fix all (l : list term) : Forall P l :=
              match l with
              | [] => Forall_nil P
              | x :: rest => Forall_cons x (term_nested_ind x) (all rest)
              end) xs)
    | TTuple xs =>
        case_tuple xs
          ((fix all (l : list term) : Forall P l :=
              match l with
              | [] => Forall_nil P
              | x :: rest => Forall_cons x (term_nested_ind x) (all rest)
              end) xs)
    | TPriv id => case_priv id
    | TOther b => case_other b
    end.
End TermInd.

(* A term takes the structural arms when every node of its spine is in the
   structural grammar (structural_arm and scan_element of path_codec.rs, the
   eval_stable_par grammar of EPM1). *)
Fixpoint structural (t : term) : bool :=
  match t with
  | TList xs => forallb structural xs
  | TTuple xs => forallb structural xs
  | TOther _ => false
  | _ => true
  end.

(* ser: the structural serialization of every term. A structural term has
   the bytes of its EPM1 segment (Arm::write). TOther stands for the
   canonical bytes, under the tag 16, which no segment starts with. *)
Fixpoint ser (t : term) : bytes :=
  match t with
  | TBool false => [1]
  | TBool true => [2]
  | TInt z => 3 :: uv (zigzag z)
  | TStr s => 4 :: uv (length s) ++ s
  | TUri s => 5 :: uv (length s) ++ s
  | TBytes b => 6 :: uv (length b) ++ b
  | TDouble b0 b1 b2 b3 b4 b5 b6 b7 => [7; b0; b1; b2; b3; b4; b5; b6; b7]
  | TBigInt b => 8 :: uv (length b) ++ b
  | TBigRat n d => 9 :: uv (length n) ++ n ++ uv (length d) ++ d
  | TFixed u sc => 10 :: uv (length u) ++ u ++ uv sc
  | TList xs => 11 :: uv (length xs) ++ flat_map ser xs
  | TTuple xs => 12 :: uv (length xs) ++ flat_map ser xs
  | TPriv id => 14 :: uv (length id) ++ id
  | TOther b => 16 :: uv (length b) ++ b
  end.

(* seg: the segment of a path element (segment and write_segment). A term
   outside the structural grammar takes the escape arm 0x0F with the length
   and the bytes of its serialization. The Rust escape payload is the prost
   encoding of the sorted Par, which ser stands for: both are injective. *)
Definition seg (t : term) : bytes :=
  if structural t then ser t else 15 :: uv (length (ser t)) ++ ser t.

(* The parser of k terms, with the parser p of one term. *)
Fixpoint parse_many (p : bytes -> option (term * bytes)) (k : nat) (bs : bytes)
  : option (list term * bytes) :=
  match k with
  | O => Some ([], bs)
  | S k' =>
      match p bs with
      | Some (x, r) =>
          match parse_many p k' r with
          | Some (xs, r') => Some (x :: xs, r')
          | None => None
          end
      | None => None
      end
  end.

(* The parser of a serialization. The fuel bounds the nesting depth. *)
Fixpoint parse_ser (fuel : nat) (bs : bytes) {struct fuel} : option (term * bytes) :=
  match fuel with
  | O => None
  | S f =>
      match bs with
      | [] => None
      | t :: r =>
          if t =? 1 then Some (TBool false, r)
          else if t =? 2 then Some (TBool true, r)
          else if t =? 3 then
            match read_uv r with
            | Some (n, r') => Some (TInt (unzigzag n), r')
            | None => None
            end
          else if t =? 4 then
            match read_bytes r with Some (s, r') => Some (TStr s, r') | None => None end
          else if t =? 5 then
            match read_bytes r with Some (s, r') => Some (TUri s, r') | None => None end
          else if t =? 6 then
            match read_bytes r with Some (b, r') => Some (TBytes b, r') | None => None end
          else if t =? 7 then
            match r with
            | b0 :: b1 :: b2 :: b3 :: b4 :: b5 :: b6 :: b7 :: r' =>
                Some (TDouble b0 b1 b2 b3 b4 b5 b6 b7, r')
            | _ => None
            end
          else if t =? 8 then
            match read_bytes r with Some (b, r') => Some (TBigInt b, r') | None => None end
          else if t =? 9 then
            match read_bytes r with
            | Some (n, r1) =>
                match read_bytes r1 with
                | Some (d, r2) => Some (TBigRat n d, r2)
                | None => None
                end
            | None => None
            end
          else if t =? 10 then
            match read_bytes r with
            | Some (u, r1) =>
                match read_uv r1 with
                | Some (sc, r2) => Some (TFixed u sc, r2)
                | None => None
                end
            | None => None
            end
          else if t =? 11 then
            match read_uv r with
            | Some (k, r1) =>
                match parse_many (parse_ser f) k r1 with
                | Some (xs, r2) => Some (TList xs, r2)
                | None => None
                end
            | None => None
            end
          else if t =? 12 then
            match read_uv r with
            | Some (k, r1) =>
                match parse_many (parse_ser f) k r1 with
                | Some (xs, r2) => Some (TTuple xs, r2)
                | None => None
                end
            | None => None
            end
          else if t =? 14 then
            match read_bytes r with Some (b, r') => Some (TPriv b, r') | None => None end
          else if t =? 16 then
            match read_bytes r with Some (b, r') => Some (TOther b, r') | None => None end
          else None
      end
  end.

(* The parser of a segment: the escape arm, or a structural serialization. *)
Definition parse_seg (fuel : nat) (bs : bytes) : option (term * bytes) :=
  match bs with
  | [] => None
  | t :: r =>
      if t =? 15 then
        match read_bytes r with
        | Some (b, r') =>
            match parse_ser fuel b with
            | Some (x, []) => Some (x, r')
            | _ => None
            end
        | None => None
        end
      else parse_ser fuel bs
  end.

(* The nesting depth of a term. *)
Fixpoint depth (t : term) : nat :=
  match t with
  | TList xs => S (list_max (map depth xs))
  | TTuple xs => S (list_max (map depth xs))
  | _ => 0
  end.

Lemma depth_in :
  forall x xs, In x xs -> depth x <= list_max (map depth xs).
Proof.
  intros x xs Hin.
  assert (Hall : Forall (fun k => k <= list_max (map depth xs)) (map depth xs))
    by (apply list_max_le; lia).
  rewrite Forall_forall in Hall. apply Hall. apply in_map. exact Hin.
Qed.

Lemma parse_many_flat_map :
  forall p xs r,
    Forall (fun x => forall r', p (ser x ++ r') = Some (x, r')) xs ->
    parse_many p (length xs) (flat_map ser xs ++ r) = Some (xs, r).
Proof.
  intros p xs r Hall. revert r.
  induction Hall as [| x xs Hx Hxs IH]; intros r; [reflexivity |].
  simpl. rewrite <- app_assoc, Hx, IH. reflexivity.
Qed.

(* The parser inverts the serialization: every part is self-delimiting. *)
Lemma parse_ser_ser :
  forall t f r, depth t < f -> parse_ser f (ser t ++ r) = Some (t, r).
Proof.
  intros t.
  induction t as [b | z | s | s | b | b0 b1 b2 b3 b4 b5 b6 b7 | b | n d | u sc
                 | xs IH | xs IH | id | b]
    using term_nested_ind;
    intros f r Hdepth; destruct f as [| f]; simpl in Hdepth; try lia.
  - destruct b; reflexivity.
  - simpl. rewrite read_uv_uv, unzigzag_zigzag. reflexivity.
  - simpl. rewrite <- app_assoc, read_bytes_uv. reflexivity.
  - simpl. rewrite <- app_assoc, read_bytes_uv. reflexivity.
  - simpl. rewrite <- app_assoc, read_bytes_uv. reflexivity.
  - reflexivity.
  - simpl. rewrite <- app_assoc, read_bytes_uv. reflexivity.
  - simpl. rewrite <- !app_assoc, read_bytes_uv, read_bytes_uv. reflexivity.
  - simpl. rewrite <- !app_assoc, read_bytes_uv, read_uv_uv. reflexivity.
  - simpl. rewrite <- app_assoc, read_uv_uv, parse_many_flat_map; [reflexivity |].
    rewrite Forall_forall in IH |- *. intros x Hin r'.
    apply IH; [exact Hin |].
    pose proof (depth_in x xs Hin). lia.
  - simpl. rewrite <- app_assoc, read_uv_uv, parse_many_flat_map; [reflexivity |].
    rewrite Forall_forall in IH |- *. intros x Hin r'.
    apply IH; [exact Hin |].
    pose proof (depth_in x xs Hin). lia.
  - simpl. rewrite <- app_assoc, read_bytes_uv. reflexivity.
  - simpl. rewrite <- app_assoc, read_bytes_uv. reflexivity.
Qed.

(* The serialization of a structural term starts with a structural tag in
   0x01..0x0E, never with the terminator 0x00 and never with the escape tag
   0x0F. *)
Lemma ser_tag :
  forall t, structural t = true -> exists u r, ser t = u :: r /\ 1 <= u <= 14.
Proof.
  intros [[|] | z | s | s | b | b0 b1 b2 b3 b4 b5 b6 b7 | b | n d | u sc | xs | xs
         | id | b] Hs;
    simpl in Hs; try discriminate;
    simpl; eexists; eexists; split; try reflexivity; lia.
Qed.

(* The parser inverts the segment codec. *)
Lemma parse_seg_seg :
  forall t f r, depth t < f -> parse_seg f (seg t ++ r) = Some (t, r).
Proof.
  intros t f r Hdepth. unfold seg.
  destruct (structural t) eqn:Hs.
  - destruct (ser_tag t Hs) as [u [rest [Hser Hu]]].
    assert (Hne : (u =? 15) = false) by (apply Nat.eqb_neq; lia).
    rewrite Hser. simpl. rewrite Hne.
    change (u :: rest ++ r) with ((u :: rest) ++ r). rewrite <- Hser.
    apply parse_ser_ser. exact Hdepth.
  - simpl. rewrite <- app_assoc, read_bytes_uv.
    rewrite <- (app_nil_r (ser t)), parse_ser_ser by exact Hdepth.
    reflexivity.
Qed.

(* The segments are a prefix code: a segment is never a proper prefix of
   another segment, and seg is injective. *)
Lemma seg_unique :
  forall a b x y, seg a ++ x = seg b ++ y -> a = b /\ x = y.
Proof.
  intros a b x y H.
  pose proof (parse_seg_seg a (S (depth a + depth b)) x ltac:(lia)) as Ha.
  pose proof (parse_seg_seg b (S (depth a + depth b)) y ltac:(lia)) as Hb.
  rewrite H in Ha. rewrite Ha in Hb. inversion Hb. auto.
Qed.

(* Every segment starts with a tag in 0x01..0x0F, never with the terminator
   0x00. *)
Lemma seg_tag :
  forall t, exists u r, seg t = u :: r /\ 1 <= u <= 15.
Proof.
  intros t. unfold seg. destruct (structural t) eqn:Hs.
  - destruct (ser_tag t Hs) as [u [r [Hser Hu]]].
    exists u, r. split; [exact Hser | lia].
  - exists 15, (uv (length (ser t)) ++ ser t). split; [reflexivity | lia].
Qed.

(* ── 3. Keys ──────────────────────────────────────────────────────────── *)

Definition path : Type := list term.

Definition prefix_key (q : path) : bytes := flat_map seg q.

Definition path_key (p : path) : bytes := prefix_key p ++ [0].

Definition is_prefix (a b : bytes) : Prop := exists z, b = a ++ z.

Definition list_prefix (q p : path) : Prop := exists s, p = q ++ s.

Lemma prefix_key_app : forall q s, prefix_key (q ++ s) = prefix_key q ++ prefix_key s.
Proof. intros q s. unfold prefix_key. apply flat_map_app. Qed.

Lemma path_key_app : forall s p, path_key (s ++ p) = prefix_key s ++ path_key p.
Proof.
  intros s p. unfold path_key. rewrite prefix_key_app, app_assoc. reflexivity.
Qed.

Lemma path_key_cons : forall e p, path_key (e :: p) = seg e ++ path_key p.
Proof. intros e p. unfold path_key, prefix_key. simpl. rewrite app_assoc. reflexivity. Qed.

(* HEADLINE 1: a prefix of a path is exactly a prefix key that is a byte
   prefix of the path key. *)
Theorem prefix_key_iff_prefix :
  forall q p, is_prefix (prefix_key q) (path_key p) <-> list_prefix q p.
Proof.
  intros q p. split.
  - revert p. induction q as [| a q IH]; intros p [z Hz].
    + exists p. reflexivity.
    + destruct p as [| b p].
      * exfalso.
        destruct (seg_tag a) as [t [r [Hseg Ht]]].
        unfold path_key, prefix_key in Hz. simpl in Hz. rewrite Hseg in Hz.
        simpl in Hz. inversion Hz. lia.
      * rewrite path_key_cons in Hz. unfold prefix_key in Hz. simpl in Hz.
        rewrite <- !app_assoc in Hz.
        destruct (seg_unique b a (path_key p) (flat_map seg q ++ z) Hz) as [Hab Hrest].
        subst b.
        destruct (IH p (ex_intro _ z Hrest)) as [s Hs].
        exists s. rewrite Hs. reflexivity.
  - intros [s Hs]. subst p. exists (path_key s). apply path_key_app.
Qed.

(* Two paths with one key are equal. *)
Lemma path_key_injective : forall p1 p2, path_key p1 = path_key p2 -> p1 = p2.
Proof.
  induction p1 as [| a p1 IH]; intros [| b p2] H.
  - reflexivity.
  - exfalso. rewrite path_key_cons in H.
    destruct (seg_tag b) as [t [r [Hseg Ht]]].
    rewrite Hseg in H. simpl in H. inversion H. lia.
  - exfalso. rewrite path_key_cons in H.
    destruct (seg_tag a) as [t [r [Hseg Ht]]].
    rewrite Hseg in H. simpl in H. inversion H. lia.
  - rewrite !path_key_cons in H.
    destruct (seg_unique a b _ _ H) as [Hab Hrest]. subst b.
    rewrite (IH p2 Hrest). reflexivity.
Qed.

(* ── 4. The byte order of keys ────────────────────────────────────────── *)

(* The byte order of Vec<u8>: lexicographic, a proper prefix first. *)
Fixpoint lex_cmp (a b : bytes) : comparison :=
  match a, b with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | x :: xs, y :: ys =>
      match Nat.compare x y with
      | Eq => lex_cmp xs ys
      | c => c
      end
  end.

Lemma lex_cmp_refl : forall a, lex_cmp a a = Eq.
Proof. induction a as [| x a IH]; simpl; [reflexivity |]. rewrite Nat.compare_refl. exact IH. Qed.

Lemma lex_cmp_eq : forall a b, lex_cmp a b = Eq -> a = b.
Proof.
  induction a as [| x a IH]; intros [| y b] H; simpl in H; try discriminate; [reflexivity |].
  destruct (Nat.compare x y) eqn:Hxy; try discriminate.
  apply Nat.compare_eq in Hxy. subst y. rewrite (IH b H). reflexivity.
Qed.

Lemma lex_cmp_sym : forall a b, lex_cmp b a = CompOpp (lex_cmp a b).
Proof.
  induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
  rewrite (Nat.compare_antisym x y).
  destruct (Nat.compare x y); simpl; try reflexivity. apply IH.
Qed.

Lemma lex_cmp_trans :
  forall a b c, lex_cmp a b = Lt -> lex_cmp b c = Lt -> lex_cmp a c = Lt.
Proof.
  induction a as [| x a IH]; intros [| y b] [| z c] Hab Hbc; simpl in *;
    try discriminate; try reflexivity.
  destruct (Nat.compare x y) eqn:Hxy; try discriminate;
    destruct (Nat.compare y z) eqn:Hyz; try discriminate.
  - apply Nat.compare_eq in Hxy. apply Nat.compare_eq in Hyz. subst.
    rewrite Nat.compare_refl. apply (IH b c Hab Hbc).
  - apply Nat.compare_eq in Hxy. subst. rewrite Hyz. reflexivity.
  - apply Nat.compare_eq in Hyz. subst. rewrite Hxy. reflexivity.
  - apply Nat.compare_lt_iff in Hxy. apply Nat.compare_lt_iff in Hyz.
    assert (Hxz : (x ?= z) = Lt) by (apply Nat.compare_lt_iff; lia).
    rewrite Hxz. reflexivity.
Qed.

(* A common byte prefix does not change the order. *)
Lemma lex_cmp_app : forall c a b, lex_cmp (c ++ a) (c ++ b) = lex_cmp a b.
Proof. induction c as [| x c IH]; intros a b; simpl; [reflexivity |]. rewrite Nat.compare_refl. apply IH. Qed.

(* The terminator 0x00 is below every tag, so the key of a path comes before
   the keys of its extensions. *)
Lemma exact_path_first :
  forall q s, s <> [] -> lex_cmp (path_key q) (path_key (q ++ s)) = Lt.
Proof.
  intros q [| a s] Hs; [contradiction |].
  rewrite path_key_app. unfold path_key at 1. rewrite lex_cmp_app.
  rewrite path_key_cons. destruct (seg_tag a) as [t [r [Hseg Ht]]].
  rewrite Hseg. cbn [app lex_cmp].
  assert (Hlt : (0 ?= t) = Lt) by (apply Nat.compare_lt_iff; lia).
  rewrite Hlt. reflexivity.
Qed.

(* ── 5. Logical maps and the adapter of this tree ─────────────────────── *)

(* elems: the path of an entry or a channel (path_elements). A plain list
   gives its elements, and every other element is a path of one element. *)
Definition elems (e : term) : path :=
  match e with
  | TList xs => xs
  | _ => [e]
  end.

(* The canonical entry form of a path (canonical_entry): a path of one
   element that is not a plain list is the element itself, and every other
   path is a plain list. *)
Definition canon (p : path) : term :=
  match p with
  | [TList xs] => TList [TList xs]
  | [e] => e
  | _ => TList p
  end.

Lemma elems_canon : forall p, elems (canon p) = p.
Proof. intros [| e [| e' t]]; [reflexivity | destruct e | destruct e]; reflexivity. Qed.

(* @c and @[c] name one path when c is not a plain list. *)
Example example_channel_equals_singleton_list :
  elems (TInt 5%Z) = elems (TList [TInt 5%Z]).
Proof. reflexivity. Qed.

(* A plain list with an element outside the structural grammar takes the
   escape arm as a path element, but it is still a plain list: a channel or
   an entry splits it, and a path of one such list keeps it in a list. *)
Example example_unstable_plain_list :
  seg (TList [TOther [7]]) = 15 :: uv (length (ser (TList [TOther [7]]))) ++ ser (TList [TOther [7]]) /\
  elems (TList [TOther [7]]) = [TOther [7]] /\
  canon [TList [TOther [7]]] = TList [TList [TOther [7]]].
Proof. repeat split; reflexivity. Qed.

Definition plt (p q : path) : Prop := lex_cmp (path_key p) (path_key q) = Lt.

(* A logical map is a list of paths in strictly ascending key order. *)
Definition hd_lt (p : path) (ps : list path) : Prop :=
  match ps with
  | [] => True
  | q :: _ => plt p q
  end.

Inductive sorted : list path -> Prop :=
| sorted_nil : sorted []
| sorted_cons : forall p ps, hd_lt p ps -> sorted ps -> sorted (p :: ps).

(* The insertion of a path. A path with the key of a stored path is a
   repeat, and the stored path stays (sorted_unique). *)
Fixpoint insert_path (p : path) (ps : list path) : list path :=
  match ps with
  | [] => [p]
  | q :: t =>
      match lex_cmp (path_key p) (path_key q) with
      | Lt => p :: q :: t
      | Eq => q :: t
      | Gt => q :: insert_path p t
      end
  end.

Definition sort_dedup (ps : list path) : list path := fold_right insert_path [] ps.

Lemma insert_hd :
  forall q p t, plt q p -> hd_lt q t -> hd_lt q (insert_path p t).
Proof.
  intros q p [| h t] Hqp Hqt; simpl; [exact Hqp |].
  destruct (lex_cmp (path_key p) (path_key h)); simpl; assumption.
Qed.

Lemma insert_sorted : forall p ps, sorted ps -> sorted (insert_path p ps).
Proof.
  intros p ps Hs. induction Hs as [| q t Hqt Ht IH]; simpl.
  - constructor; [exact I | constructor].
  - destruct (lex_cmp (path_key p) (path_key q)) eqn:Hpq.
    + constructor; assumption.
    + constructor; [exact Hpq | constructor; assumption].
    + constructor; [| exact IH].
      apply insert_hd; [| exact Hqt].
      unfold plt. rewrite lex_cmp_sym, Hpq. reflexivity.
Qed.

Lemma sort_dedup_sorted : forall ps, sorted (sort_dedup ps).
Proof.
  induction ps as [| p ps IH]; simpl; [constructor |]. apply insert_sorted. exact IH.
Qed.

Lemma sort_dedup_id : forall ps, sorted ps -> sort_dedup ps = ps.
Proof.
  intros ps Hs. induction Hs as [| p t Hpt Ht IH]; [reflexivity |].
  simpl. rewrite IH. destruct t as [| q t]; [reflexivity |].
  simpl. simpl in Hpt. unfold plt in Hpt. rewrite Hpt. reflexivity.
Qed.

(* The adapter of the EPathMap of this tree (HeadEPathMapAdapter): decode
   orders the paths of the entries and keeps one path for each key, and
   encode writes each path in its canonical entry form. *)
Definition head_decode (es : list term) : list path := sort_dedup (map elems es).

Definition head_encode (ps : list path) : list term := map canon ps.

Lemma head_decode_sorted : forall es, sorted (head_decode es).
Proof. intros es. apply sort_dedup_sorted. Qed.

Lemma head_roundtrip : forall ps, sorted ps -> head_decode (head_encode ps) = ps.
Proof.
  intros ps Hs. unfold head_decode, head_encode. rewrite map_map.
  rewrite (map_ext (fun p => elems (canon p)) (fun p => p) elems_canon ps), map_id.
  apply sort_dedup_id. exact Hs.
Qed.

(* ── 6. Re-rooting ────────────────────────────────────────────────────── *)

(* The logical re-rooting of the paths of a map under a suffix s: the unit
   rule for the empty map, and s ++ p for each path p otherwise. *)
Definition rl (s : path) (ps : list path) : list path :=
  match ps with
  | [] => [s]
  | _ => map (fun p => s ++ p) ps
  end.

Lemma rl_sorted : forall s ps, sorted ps -> sorted (rl s ps).
Proof.
  intros s ps Hs. destruct ps as [| p ps]; [simpl; repeat constructor |].
  unfold rl. induction Hs as [| q t Hqt Ht IH]; simpl; [constructor |].
  constructor.
  - destruct t as [| h t]; simpl; [exact I |].
    unfold plt. rewrite !path_key_app, lex_cmp_app. exact Hqt.
  - destruct t as [| h t]; [constructor |]. exact IH.
Qed.

Lemma rl_compose : forall s1 s2 ps, rl s1 (rl s2 ps) = rl (s1 ++ s2) ps.
Proof.
  intros s1 s2 [| p ps]; simpl; [reflexivity |].
  rewrite app_assoc. f_equal. rewrite map_map.
  apply map_ext. intros q. apply app_assoc.
Qed.

(* A value of a send argument: a PathMap with its entries in the order of
   ps, or any other value. *)
Inductive pval : Type :=
| PMap (es : list term)
| POther (x : term).

(* R(s, v) (reroot.rs, reroot). *)
Definition reroot (s : path) (v : pval) : pval :=
  match s with
  | [] => v
  | _ :: _ =>
      match v with
      | PMap es => PMap (head_encode (rl s (head_decode es)))
      | POther x => POther (TList (s ++ [x]))
      end
  end.

(* HEADLINE 2: R(ε, v) = v. *)
Theorem reroot_ident : forall v, reroot [] v = v.
Proof. reflexivity. Qed.

(* HEADLINE 3: re-rooting composes. For a PathMap the composition of two
   re-rootings is the re-rooting under the joined suffix. For every other
   value the delivery on a shorter prefix prepends the extra suffix to the
   list of the delivery on the longer prefix. *)
Theorem reroot_compose :
  (forall s1 s2 es, reroot s1 (reroot s2 (PMap es)) = reroot (s1 ++ s2) (PMap es)) /\
  (forall s1 s2 x y,
     s2 <> [] -> reroot s2 (POther x) = POther y ->
     reroot (s1 ++ s2) (POther x) = POther (TList (s1 ++ elems y))).
Proof.
  split.
  - intros s1 s2 es. destruct s2 as [| a s2].
    + rewrite app_nil_r. reflexivity.
    + destruct s1 as [| b s1]; [reflexivity |].
      simpl. rewrite head_roundtrip by (apply rl_sorted, head_decode_sorted).
      rewrite rl_compose. reflexivity.
  - intros s1 s2 x y Hs2 Hy. destruct s2 as [| a s2]; [contradiction |].
    simpl in Hy. inversion Hy. subst y. simpl.
    destruct s1 as [| b s1]; simpl; rewrite <- ?app_assoc; reflexivity.
Qed.

(* The empty map and the map of the empty path both stand for the unit, the
   set of the empty path (FIPS: {||} at the path p is the path p itself). *)
Definition unit_class (ps : list path) : list path :=
  match ps with
  | [] => [[]]
  | _ => ps
  end.

Inductive dval : Type :=
| DMap (ps : list path)
| DOther (x : term).

(* The logical value of a send argument. *)
Definition denote (v : pval) : dval :=
  match v with
  | PMap es => DMap (unit_class (head_decode es))
  | POther x => DOther x
  end.

Lemma map_app_injective :
  forall s ps1 ps2,
    map (fun p : path => s ++ p) ps1 = map (fun p => s ++ p) ps2 -> ps1 = ps2.
Proof.
  intros s ps1. induction ps1 as [| p ps1 IH]; intros [| q ps2] H;
    simpl in H; try discriminate; [reflexivity |].
  inversion H as [[Hpq Hrest]]. apply app_inv_head in Hpq. subst q.
  rewrite (IH ps2 Hrest). reflexivity.
Qed.

(* A map whose re-rooting is the single path s holds only the empty path. *)
Lemma singleton_rl :
  forall s q ps, [s] = map (fun p : path => s ++ p) (q :: ps) -> q :: ps = [[]].
Proof.
  intros s q ps H. simpl in H. inversion H as [[Hq Hrest]].
  symmetry in Hrest. apply map_eq_nil in Hrest. subst ps.
  rewrite <- (app_nil_r s) in Hq at 1. apply app_inv_head in Hq. subst q.
  reflexivity.
Qed.

Lemma rl_unit_injective :
  forall s ps1 ps2, rl s ps1 = rl s ps2 -> unit_class ps1 = unit_class ps2.
Proof.
  intros s [| p ps1] [| q ps2] H.
  - reflexivity.
  - simpl in H |- *. first [ discriminate H | symmetry; exact (singleton_rl s q ps2 H) ].
  - simpl in H |- *. first [ discriminate H | exact (singleton_rl s p ps1 (eq_sym H)) ].
  - exact (map_app_injective s (p :: ps1) (q :: ps2) H).
Qed.

Lemma rl_unit_class : forall s ps, rl s (unit_class ps) = rl s ps.
Proof. intros s [| p ps]; simpl; [rewrite app_nil_r |]; reflexivity. Qed.

(* HEADLINE 4: under a fixed suffix, re-rooting loses no logical
   information, and under a suffix that is not empty two values have equal
   deliveries exactly when they are logically equal. *)
Theorem reroot_injective :
  (forall s v1 v2, reroot s v1 = reroot s v2 -> denote v1 = denote v2) /\
  (forall s v1 v2, s <> [] -> denote v1 = denote v2 -> reroot s v1 = reroot s v2).
Proof.
  split.
  - intros [| a s] v1 v2 H; [simpl in H; subst; reflexivity |].
    destruct v1 as [es1 | x1], v2 as [es2 | x2]; simpl in H; try discriminate.
    + inversion H as [Henc].
      assert (Hdec : rl (a :: s) (head_decode es1) = rl (a :: s) (head_decode es2)).
      { rewrite <- (head_roundtrip (rl (a :: s) (head_decode es1)))
          by (apply rl_sorted, head_decode_sorted).
        rewrite <- (head_roundtrip (rl (a :: s) (head_decode es2)))
          by (apply rl_sorted, head_decode_sorted).
        rewrite Henc. reflexivity. }
      simpl. rewrite (rl_unit_injective _ _ _ Hdec). reflexivity.
    + inversion H as [Hx]. apply app_inv_head in Hx. inversion Hx. reflexivity.
  - intros [| a s] v1 v2 Hs H; [contradiction |].
    destruct v1 as [es1 | x1], v2 as [es2 | x2]; simpl in H; try discriminate.
    + inversion H as [Hclass]. simpl.
      rewrite <- (rl_unit_class (a :: s) (head_decode es1)), Hclass, rl_unit_class.
      reflexivity.
    + inversion H. reflexivity.
Qed.

(* ── 7. The FIPS example ──────────────────────────────────────────────── *)

Definition n (k : nat) : term := TInt (Z.of_nat k).
Definition hi : term := TStr [104; 105].
Definition hello : term := TStr [104; 101; 108; 108; 111].
Definition there : term := TStr [116; 104; 101; 114; 101].

(* HEADLINE 5: the FIPS example. A send on @[0,1,2] is a send on every
   prefix. @[0,1,2]!({|"hi"|}), @[0,1,2]!({|"hello"|}) and
   @[0,1,3]!({|"there"|}) received on @[0,1] bind {|[2,"hi"]|},
   {|[2,"hello"]|} and {|[3,"there"]|}. @[0,1,2]!(42) binds [2, 42], and
   @[0,1,2,"hi"]!({||}) binds {|[2,"hi"]|}. @[0,1,2]!({|"hi"|}) and
   @[0,1,2,"hi"]!({||}) deliver one value on every prefix of [0,1,2]. *)
Theorem fips_example :
  path_key [n 0; n 1; n 2] = [3; 0; 3; 2; 3; 4; 0] /\
  prefix_key [n 0; n 1] = [3; 0; 3; 2] /\
  is_prefix (prefix_key [n 0; n 1]) (path_key [n 0; n 1; n 2]) /\
  is_prefix (prefix_key [n 0; n 1]) (path_key [n 0; n 1; n 3]) /\
  lex_cmp (path_key [n 0; n 1; n 2]) (path_key [n 0; n 1; n 3]) = Lt /\
  reroot [n 2] (PMap [hi]) = PMap [TList [n 2; hi]] /\
  reroot [n 2] (PMap [hello]) = PMap [TList [n 2; hello]] /\
  reroot [n 3] (PMap [there]) = PMap [TList [n 3; there]] /\
  reroot [n 2] (POther (n 42)) = POther (TList [n 2; n 42]) /\
  reroot [n 2; hi] (PMap []) = PMap [TList [n 2; hi]] /\
  reroot [] (PMap []) = PMap [] /\
  reroot [n 0; n 1; n 2] (PMap [hi]) = PMap [TList [n 0; n 1; n 2; hi]] /\
  (forall k, k <= 3 ->
     reroot (skipn k [n 0; n 1; n 2]) (PMap [hi]) =
     reroot (skipn k [n 0; n 1; n 2] ++ [hi]) (PMap [])).
Proof.
  repeat split; try (vm_compute; reflexivity).
  - exists [3; 4; 0]. vm_compute. reflexivity.
  - exists [3; 6; 0]. vm_compute. reflexivity.
  - intros k Hk.
    destruct k as [| [| [| [| k]]]]; try lia; vm_compute; reflexivity.
Qed.

(* The FIPS equation @p!({|a|}) = @(p ++ [a])!({||}) for an element a that
   is not a plain list, on every suffix. *)
Lemma unit_shift :
  forall s a, (forall xs, a <> TList xs) ->
    reroot s (PMap [a]) = reroot (s ++ [a]) (PMap []).
Proof.
  intros s a Ha.
  destruct a as [b | z | t | t | b | b0 b1 b2 b3 b4 b5 b6 b7 | b | m d | u sc | xs | xs | id | b];
    try (exfalso; eapply Ha; reflexivity);
    destruct s as [| e s]; reflexivity.
Qed.

(* ── 8. The candidate order ───────────────────────────────────────────── *)

Definition strict_total {A : Type} (lt : A -> A -> Prop) : Prop :=
  (forall x, ~ lt x x) /\
  (forall x y z, lt x y -> lt y z -> lt x z) /\
  (forall x y, x <> y -> lt x y \/ lt y x).

(* The candidates of a receive on a prefix: a stored datum at its path, with
   its rank in the order of its discipline. They order by the key of the
   path, then by the rank. *)
Definition cand_lt {R : Type} (rlt : R -> R -> Prop) (a b : path * R) : Prop :=
  match lex_cmp (path_key (fst a)) (path_key (fst b)) with
  | Lt => True
  | Eq => rlt (snd a) (snd b)
  | Gt => False
  end.

(* HEADLINE 6: for every strict total order of the ranks, the candidate
   order is a strict total order, and the exact path comes first. *)
Theorem candidate_order_total :
  forall (R : Type) (rlt : R -> R -> Prop),
    strict_total rlt ->
    strict_total (cand_lt rlt) /\
    (forall q s r1 r2, s <> [] -> cand_lt rlt (q, r1) (q ++ s, r2)).
Proof.
  intros R rlt [Hirr [Htrans Htot]]. split; [split; [| split] |].
  - intros [p r]. unfold cand_lt. simpl. rewrite lex_cmp_refl. apply Hirr.
  - intros [p1 r1] [p2 r2] [p3 r3]. unfold cand_lt. simpl.
    destruct (lex_cmp (path_key p1) (path_key p2)) eqn:H12; try contradiction;
      destruct (lex_cmp (path_key p2) (path_key p3)) eqn:H23; try contradiction;
      intros H1 H2.
    + apply lex_cmp_eq in H12. apply lex_cmp_eq in H23.
      rewrite H12, H23, lex_cmp_refl. apply (Htrans r1 r2 r3 H1 H2).
    + apply lex_cmp_eq in H12. rewrite H12, H23. exact I.
    + apply lex_cmp_eq in H23. rewrite <- H23, H12. exact I.
    + rewrite (lex_cmp_trans _ _ _ H12 H23). exact I.
  - intros [p1 r1] [p2 r2] Hne. unfold cand_lt. simpl.
    destruct (lex_cmp (path_key p1) (path_key p2)) eqn:H12.
    + pose proof (lex_cmp_eq _ _ H12) as Hkeys.
      apply path_key_injective in Hkeys. subst p2.
      rewrite lex_cmp_refl.
      assert (Hr : r1 <> r2) by (intros Hr; apply Hne; subst; reflexivity).
      apply Htot. exact Hr.
    + left. exact I.
    + right. rewrite lex_cmp_sym, H12. exact I.
  - intros q s r1 r2 Hs. unfold cand_lt. simpl.
    rewrite (exact_path_first q s Hs). exact I.
Qed.

(* ── 9. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions prefix_key_iff_prefix.
Print Assumptions reroot_ident.
Print Assumptions reroot_compose.
Print Assumptions reroot_injective.
Print Assumptions fips_example.
Print Assumptions candidate_order_total.
