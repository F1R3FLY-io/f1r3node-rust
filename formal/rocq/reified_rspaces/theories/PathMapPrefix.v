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

(* ═══════════════════════════════════════════════════════════════════════════
   Wave W3: the markers, the delivery engine and the merge contact (P5e)

   Wave W3 adds the engine of a PathMap space: the marker record and the
   delivery vehicle, the path rules of a channel, the state of a space with
   its marker invariant, the events and the shape of every COMM, prefix
   contact, the second send of a peek (decision 45), the scan driver of a
   bag or a set on prefixes, the cascade of the head-only disciplines on
   prefixes, the token and the re-entry of decision 43, and the metering of
   the prefix scan (decision 42). The sections above stay as wave W2 wrote
   them, so their line citations stay valid.

     marker of d on q   = [EList[tag_I, GByteArray(src d)], EList(p∖q)]
     vehicle of d on q  = marker of d on q ++ payload(d)     (never stored)

   Sources
   - Design: P5 design v3 A.11 and A.13 (the P5e row), P5 design v2 §3.4,
     P5 design report B.7, the joins plan §3, §4 and §9 (the prefix source).
   - Plan: the section "PathMap delivery", decisions 37, 40 (Q3: the vehicle
     carries the payload), 41, 42, 43, 45 (Rule E and its second send) and
     49, and F4, F9 and F16 of decision 39.
   - Code: rholang/src/rust/interpreter/spaces/pathmap/{markers,delivery}.rs
     and rholang/src/rust/interpreter/spaces/outer/pathmap.rs.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code (wave W3)
   ─────────────────────────────────────────────────────────────────────────
   cand_lt            candidate_cmp (delivery.rs:630-636) and the order of
                      consume_candidates (delivery.rs:175-182, 190-249)
   rpar, marker_tag, record_tag, envelope, marker_pars
                      MARKER_TAG, marker_tag, marker_envelope, envelope_of
                      and marker_record (markers.rs:61, 98-118, 152-165)
   decode_marker, marker_part, elems_of
                      decode_marker, marker_parts, strict_list
                      (markers.rs:278-284, 344-392)
   vehicle_pars, decode_vehicle
                      vehicle and decode_vehicle (markers.rs:197-213,
                      297-306)
   size_pars          the introduction bytes of a record: the observer
                      charges the size of the record of each produce
   pelem, holds_map, check_path
                      check_key of PathMapOuter (pathmap.rs:201-210) with
                      channel_path and encode_path (path_codec.rs:281-286,
                      343-347)
   prefixes           EncodedPath::prefix_keys (path_codec.rs:328-334) and
                      PathMapOuter::path_channels (pathmap.rs:127-142)
   bprefix, prefixb   is_prefix_key (path_codec.rs:376-379)
   path_eqb           the byte equality of path keys
   datum, waiter, marker, state
                      the records of D(p), the waiters and the markers of
                      I(q), and the token S(sid) of the staged view
   marker_at, markers_of
                      store_markers (delivery.rs:460-487) with
                      marker_record_metered (markers.rs:170-192)
   MI                 the marker invariant of the module docs of delivery.rs
   retired_depths     retire_plan (delivery.rs:542-551)
   store_datum, deliver_remove, take_marker, put_marker, spend_after_peek
                      the stagings of the prefix source of the join engine
                      (Phase 2, see the addendum before section 18)
   chan, ev, *_events the channels D, I and S (PathMapOuter::path_data,
                      ::index, ::token, pathmap.rs:85-96) and the events of
                      the substrate
   send_prefixes, offer_list
                      produce_waiters (delivery.rs:389-431)
   candidates         consume_candidates (delivery.rs:175-182, 190-249)
   offer_loop, receive_loop
                      OFFER and RECEIVE of the scan shapes on prefixes
   window, fireable, inv_head, clean, change, dirty_of, cascade
                      the windows, FIREABLE, NEXT-FIRE and DRAIN-AND-CASCADE
                      of the join engine on prefixes (joins plan 3.2, 4.9)
   oldest             Discipline::head_metered with head_order
                      (delivery.rs:577-610)
   bump, operation_ord, operation_ord_scan, at_head_queue
                      the token bump, generic.rs operation_ord and
                      Cx::at_head with QueueDiscipline::precedes
   tev, covered, prefix_scan_trace
                      the reservations of consume_candidates and of the plan
                      around it (decision 42)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction (wave W3)
   ─────────────────────────────────────────────────────────────────────────
   - A term holds no PathMap. pelem adds the PathMap and the zipper to state
     the path rules (UnsupportedPathElement, PathTooDeep, F9).
   - The engine model (sections 12 to 15) is a section over the payloads V,
     the patterns, the guards, the spatial match, the guard check, the
     waiter order inside a prefix (DR-75), the candidate order of a receive
     and the head order. The theorems hold for every such parameter. The
     orders need only be permutations of their input.
   - A datum carries its identity d_id: the source hash of its produce on
     D(p). Two datums with one identity are one datum (wf_data).
   - A fire of the cascade is one direct COMM (lift_reoffer_equals_direct_comm
     of JoinCascade.v). A peek fire in the cascade removes the datum and
     sends it again. The next round offers it. FIRE_ROUND_CAP is a fuel: a
     run out of fuel re-issues, and cascade_terminates shows that a fuel one
     more than the measure never runs out.
   - The second send of a peek (decision 45) restores the marker of its
     COMM or spends the datum (put_marker, spend_after_peek). At the cap the
     marker of the COMM is pending: storing it restores the invariant
     (marker_invariant_preserved). The addendum before section 18 adds O-2.
   - The metering of decision 42 is the trace model of section 17
     (AgentReadCoverage.v states the rule). The Rust cut tests cover each
     metered function.

   Headline theorems of wave W3 (each one has a Print Assumptions line at
   the end, and headlines 1 to 6 are the headlines of wave W2 above)
    7. marker_record_roundtrip the strict marker decoder inverts its encoder
                               and accepts only its image, the encoder is
                               injective, and the tag is the record tag of
                               the kind I
    8. vehicle_carries_payload the vehicle is the marker followed by the
                               payload, it decodes to its parts, its size
                               covers the payload, and it is never a marker
    9. path_element_rules      a channel is accepted exactly when its path
                               has at most 64 elements and no PathMap or
                               zipper, accepted paths are prefix-closed, and
                               a datum has at most 65 markers
   10. delivery_vehicle_single_channel
                               every event of the engine is single-channel,
                               a delivery is a COMM on I(q) over a record of
                               I(q), and the engine produces the vehicle on
                               I(q) only and never stores it
   11. prefix_contact_anchored every stored datum holds a marker on each
                               prefix of its path, every marker belongs to a
                               stored datum under its prefix, and the index
                               channels are injective
   12. peek_restores_delivered_marker
                               the second send of a peek restores the marker
                               of its COMM (the state is unchanged up to the
                               order) or spends the datum with all of its
                               markers, and it meets the prefixes of the path
                               once each, the most specific first
   13. no_match_remains        the scan plans on prefixes keep no pending
                               match for waiters without a guard
   14. marker_invariant_preserved
                               every staging and every plan keeps the marker
                               invariant, or leaves exactly the marker of a
                               peek pending at the cap
   15. heads_no_match          dirty coverage, and the cascade over the dirty
                               prefixes ends with no fireable head waiter
                               within the measure
   16. token_ord_reentry       the token gives distinct orders to the whole
                               space, keeps the first-round order of a
                               re-issue, and a re-entry meets no tie
   17. prefix_scan_trace_covered
                               the prefix scan reserves before it reads
                               (decision 42)

   The section Engine has variables only, so each theorem of the engine
   holds for all of their values.

   Failed strategies of wave W3 (kept so that they are not tried again)
   - A receive loop over a fixed candidate list needs the invariant that the
     list stays inside the data after a removal, which needs distinct
     identities in the list. The loop recomputes its candidates from the
     state of each round instead, which gives the same choices, because the
     candidates before a fired datum did not match.
   - cbn after the unfolding of MI unfolds markers_of and flat_map, and
     Permutation_app_head no longer applies. The marker proofs use
     cbn [marks data flat_map].
   - remove_nth with the default decreasing argument (the position) does not
     reduce on the empty list. The definition is structural on the list.
   - A mutant that drops the recursive call of a Fixpoint makes Rocq print a
     warning on the Fixpoint first, and the negative-control harness takes
     the first location. The mutants keep the call in a match with two
     equal branches instead.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Sorting.Permutation.


(* ── 9. The marker record and the vehicle (wave W3) ──────────────────── *)

(* The pars of a space record, as the strict decoders of markers.rs see
   them: a byte array (GByteArray), a list with the flags of the normalizer
   (EList), or a path element. *)
Inductive rpar : Type :=
| RBytes (b : bytes)
| RList (xs : list rpar)
| RElem (t : term).

(* The record tags [0xE0, 0x52, kind, 1] (record.rs, RecordKind::tag_bytes)
   and the marker tag of markers.rs with the kind I = 0x49. *)
Definition record_tag (kind : nat) : bytes := [224; 82; kind; 1].
Definition kind_datum : nat := 68.
Definition kind_continuation : nat := 75.
Definition kind_counter : nat := 83.
Definition kind_config : nat := 67.
Definition kind_index : nat := 73.
Definition marker_tag : bytes := record_tag kind_index.

(* The source hash of a datum has 32 bytes (SOURCE_LEN). *)
Definition source_len : nat := 32.

Definition bytes_eqb (a b : bytes) : bool :=
  if list_eq_dec Nat.eq_dec a b then true else false.

Lemma bytes_eqb_eq : forall a b, bytes_eqb a b = true <-> a = b.
Proof.
  intros a b. unfold bytes_eqb. destruct (list_eq_dec Nat.eq_dec a b); split;
    intros H; congruence.
Qed.

(* The envelope EList[tag_I, GByteArray(src)] and the marker pars
   [envelope, EList(p∖q)] (marker_envelope, marker_record). *)
Definition envelope (src : bytes) : rpar := RList [RBytes marker_tag; RBytes src].
Definition marker_pars (src : bytes) (s : path) : list rpar :=
  [envelope src; RList (map RElem s)].

(* The vehicle of a datum: the marker pars, then the payload (vehicle). *)
Definition vehicle_pars (src : bytes) (s : path) (payload : list rpar) : list rpar :=
  marker_pars src s ++ payload.

(* The path elements of a suffix list. *)
Fixpoint elems_of (xs : list rpar) : option path :=
  match xs with
  | [] => Some []
  | RElem t :: rest =>
      match elems_of rest with
      | Some ts => Some (t :: ts)
      | None => None
      end
  | _ :: _ => None
  end.

(* The marker part of a record: the tag must be the marker tag and the
   source must have 32 bytes (marker_parts). *)
Definition marker_part (env suffix : rpar) : option (bytes * path) :=
  match env, suffix with
  | RList [RBytes t; RBytes src], RList xs =>
      if bytes_eqb t marker_tag && (length src =? source_len) then
        match elems_of xs with
        | Some ts => Some (src, ts)
        | None => None
        end
      else None
  | _, _ => None
  end.

(* decode_marker: exactly the envelope and the suffix list. *)
Definition decode_marker (ps : list rpar) : option (bytes * path) :=
  match ps with
  | [env; suffix] => marker_part env suffix
  | _ => None
  end.

(* decode_vehicle: the marker part, then any payload. *)
Definition decode_vehicle (ps : list rpar) : option (bytes * path * list rpar) :=
  match ps with
  | env :: suffix :: payload =>
      match marker_part env suffix with
      | Some (src, ts) => Some (src, ts, payload)
      | None => None
      end
  | _ => None
  end.

Lemma elems_of_map : forall s, elems_of (map RElem s) = Some s.
Proof. induction s as [| t s IH]; cbn; [reflexivity |]. rewrite IH. reflexivity. Qed.

Lemma elems_of_inv : forall xs ts, elems_of xs = Some ts -> xs = map RElem ts.
Proof.
  induction xs as [| x xs IH]; intros ts H; cbn in H.
  - injection H as <-. reflexivity.
  - destruct x as [b | ys | t]; try discriminate.
    destruct (elems_of xs) as [rest |] eqn:E; [| discriminate].
    injection H as <-. cbn. rewrite (IH rest eq_refl). reflexivity.
Qed.

Lemma marker_part_inv :
  forall env suffix src ts,
    marker_part env suffix = Some (src, ts) ->
    env = envelope src /\ suffix = RList (map RElem ts) /\ length src = source_len.
Proof.
  intros env suffix src ts H. unfold marker_part in H.
  destruct env as [b | ys | t]; try discriminate.
  destruct ys as [| [t' | | ] [| [src' | | ] [| ]]]; try discriminate.
  destruct suffix as [b | xs | t]; try discriminate.
  destruct (bytes_eqb t' marker_tag) eqn:Et; [| discriminate].
  destruct (length src' =? source_len) eqn:El; [| discriminate].
  cbn in H. destruct (elems_of xs) as [ts' |] eqn:Ex; [| discriminate].
  injection H as Hs Ht. subst src' ts'. apply bytes_eqb_eq in Et. apply Nat.eqb_eq in El.
  subst t'. split; [reflexivity |].
  split; [rewrite (elems_of_inv xs ts Ex); reflexivity | exact El].
Qed.

(* The size of the pars of a record: the bytes that its introduction
   charges, up to the constant of each node. *)
Fixpoint rsize (x : rpar) : nat :=
  match x with
  | RBytes b => S (length b)
  | RList xs => S (fold_right (fun y acc => rsize y + acc) 0 xs)
  | RElem t => S (length (ser t))
  end.

Definition size_pars (ps : list rpar) : nat := fold_right (fun y acc => rsize y + acc) 0 ps.

Lemma size_pars_app : forall a b, size_pars (a ++ b) = size_pars a + size_pars b.
Proof. induction a as [| x a IH]; intros b; cbn; [reflexivity |]. unfold size_pars in *. rewrite IH. lia. Qed.

(* HEADLINE 7 (marker_record_roundtrip): the strict decoder of a marker
   inverts its encoder for a 32-byte source. A decoded record is exactly the
   image of the encoder. The encoder is injective. The marker tag is the
   record tag of the kind I and differs from the tags of the datum, the
   continuation, the counter and the config records. A marker has two
   pars, so it is never a single number. *)
Theorem marker_record_roundtrip :
  (forall src s, length src = source_len -> decode_marker (marker_pars src s) = Some (src, s)) /\
  (forall ps src s, decode_marker ps = Some (src, s) ->
     ps = marker_pars src s /\ length src = source_len) /\
  (forall src1 s1 src2 s2, marker_pars src1 s1 = marker_pars src2 s2 -> src1 = src2 /\ s1 = s2) /\
  (marker_tag = record_tag kind_index /\
   Forall (fun k => marker_tag <> record_tag k)
     [kind_datum; kind_continuation; kind_counter; kind_config]) /\
  (forall src s, length (marker_pars src s) = 2).
Proof.
  split; [| split; [| split; [| split]]].
  - intros src s Hlen. cbn. unfold marker_tag, record_tag.
    replace (bytes_eqb [224; 82; kind_index; 1] [224; 82; kind_index; 1]) with true
      by (symmetry; apply bytes_eqb_eq; reflexivity).
    rewrite Hlen, Nat.eqb_refl. cbn. rewrite elems_of_map. reflexivity.
  - intros ps src s H. destruct ps as [| env [| suffix [| x rest]]]; try discriminate.
    cbn in H. destruct (marker_part_inv env suffix src s H) as [He [Hs Hl]].
    subst. split; reflexivity || exact Hl.
  - intros src1 s1 src2 s2 H. unfold marker_pars, envelope in H.
    injection H as Hsrc Hs. split; [exact Hsrc |].
    apply (f_equal elems_of) in Hs. rewrite !elems_of_map in Hs. injection Hs as Hs. exact Hs.
  - split; [reflexivity |].
    repeat constructor; unfold marker_tag, record_tag; cbn; intros H; discriminate H.
  - reflexivity.
Qed.

(* HEADLINE 8 (vehicle_carries_payload): the vehicle of a datum on a prefix
   is the marker of the datum on that prefix followed by the whole payload
   (decision 40, Q3). It decodes to its three parts. Its size is the size of
   the marker plus the size of the payload, so its introduction covers the
   payload as the equivalent host produce does (DR-47). A vehicle with a
   payload is never a stored marker. *)
Theorem vehicle_carries_payload :
  (forall src s payload,
     firstn 2 (vehicle_pars src s payload) = marker_pars src s /\
     skipn 2 (vehicle_pars src s payload) = payload) /\
  (forall src s payload, length src = source_len ->
     decode_vehicle (vehicle_pars src s payload) = Some (src, s, payload)) /\
  (forall src s payload,
     size_pars (vehicle_pars src s payload) = size_pars (marker_pars src s) + size_pars payload) /\
  (forall src s payload, payload <> [] -> decode_marker (vehicle_pars src s payload) = None).
Proof.
  split; [| split; [| split]].
  - intros src s payload. split; reflexivity.
  - intros src s payload Hlen. cbn. unfold marker_tag, record_tag.
    replace (bytes_eqb [224; 82; kind_index; 1] [224; 82; kind_index; 1]) with true
      by (symmetry; apply bytes_eqb_eq; reflexivity).
    rewrite Hlen, Nat.eqb_refl. cbn. rewrite elems_of_map. reflexivity.
  - intros src s payload. unfold vehicle_pars. apply size_pars_app.
  - intros src s [| x rest] Hne; [contradiction |]. reflexivity.
Qed.

(* ── 10. The path rules of a PathMap channel ─────────────────────────── *)

(* A path element as the store sees it before the encode: a closed term, a
   PathMap, a zipper, or a composite that holds other elements. The encoder
   rejects a PathMap and a zipper at any depth (UnsupportedPathElement). *)
Inductive pelem : Type :=
| PTerm (t : term)
| PMapElem
| PZipElem
| PNest (xs : list pelem).

Fixpoint holds_map (e : pelem) : bool :=
  match e with
  | PTerm _ => false
  | PMapElem => true
  | PZipElem => true
  | PNest xs => existsb holds_map xs
  end.

(* The depth cap of F9 (PATH_DEPTH_CAP). *)
Definition depth_cap : nat := 64.

Inductive path_error : Type := PathTooDeep | UnsupportedPathElement.

(* check_key of outer/pathmap.rs: channel_path checks the depth first, then
   encode_path scans every element. *)
Definition check_path (p : list pelem) : option path_error :=
  if depth_cap <? length p then Some PathTooDeep
  else if existsb holds_map p then Some UnsupportedPathElement
  else None.

(* The prefixes of a path in depth order: ε first, p last. *)
Definition prefixes {A : Type} (p : list A) : list (list A) :=
  map (fun k => firstn k p) (seq 0 (S (length p))).

Lemma prefixes_length : forall {A : Type} (p : list A), length (prefixes p) = S (length p).
Proof. intros A p. unfold prefixes. rewrite length_map, length_seq. reflexivity. Qed.

Lemma existsb_firstn_false :
  forall (f : pelem -> bool) p k, existsb f p = false -> existsb f (firstn k p) = false.
Proof.
  intros f p k. revert p. induction k as [| k IH]; intros [| x p] H; cbn in *; try reflexivity.
  apply orb_false_iff in H as [Hx Hp]. rewrite Hx, (IH p Hp). reflexivity.
Qed.

(* HEADLINE 9 (path_element_rules): the store accepts a channel exactly when
   its path has at most 64 elements and no element holds a PathMap or a
   zipper. A path deeper than the cap fails with PathTooDeep before the
   element scan. Every prefix of an accepted path is accepted, so every
   index channel of a stored datum is a valid prefix. A stored datum has at
   most 65 markers, one for each prefix. A PathMap at the top of an
   element, or a zipper inside a composite element, fails with
   UnsupportedPathElement. *)
Theorem path_element_rules :
  (forall p, check_path p = None <->
     length p <= depth_cap /\ forall e, In e p -> holds_map e = false) /\
  (forall p, depth_cap < length p -> check_path p = Some PathTooDeep) /\
  (forall p k, check_path p = None -> check_path (firstn k p) = None) /\
  (forall p, check_path p = None -> length (prefixes p) <= S depth_cap) /\
  (forall p, length p <= depth_cap -> In PMapElem p -> check_path p = Some UnsupportedPathElement) /\
  (forall p xs, length p <= depth_cap -> In (PNest xs) p -> In PZipElem xs ->
     check_path p = Some UnsupportedPathElement).
Proof.
  split; [| split; [| split; [| split; [| split]]]].
  - intros p. unfold check_path. split.
    + destruct (depth_cap <? length p) eqn:Ed; [discriminate |].
      destruct (existsb holds_map p) eqn:Ee; [discriminate |]. intros _.
      apply Nat.ltb_ge in Ed. split; [exact Ed |].
      intros e Hin. destruct (holds_map e) eqn:He; [| reflexivity].
      assert (Hx : existsb holds_map p = true) by (apply existsb_exists; exists e; split; assumption).
      congruence.
    + intros [Hlen Hall]. apply Nat.ltb_ge in Hlen. rewrite Hlen.
      destruct (existsb holds_map p) eqn:Ee; [| reflexivity].
      apply existsb_exists in Ee as [e [Hin He]]. rewrite (Hall e Hin) in He. discriminate.
  - intros p H. unfold check_path. apply Nat.ltb_lt in H. rewrite H. reflexivity.
  - intros p k H. unfold check_path in *.
    destruct (depth_cap <? length p) eqn:Ed; [discriminate |].
    destruct (existsb holds_map p) eqn:Ee; [discriminate |].
    apply Nat.ltb_ge in Ed.
    assert (Hk : (depth_cap <? length (firstn k p)) = false).
    { apply Nat.ltb_ge. rewrite length_firstn. lia. }
    rewrite Hk, (existsb_firstn_false holds_map p k Ee). reflexivity.
  - intros p H. rewrite prefixes_length. unfold check_path in H.
    destruct (depth_cap <? length p) eqn:Ed; [discriminate |]. apply Nat.ltb_ge in Ed. lia.
  - intros p Hlen Hin. unfold check_path. apply Nat.ltb_ge in Hlen. rewrite Hlen.
    replace (existsb holds_map p) with true; [reflexivity |].
    symmetry. apply existsb_exists. exists PMapElem. split; [exact Hin | reflexivity].
  - intros p xs Hlen Hin Hz. unfold check_path. apply Nat.ltb_ge in Hlen. rewrite Hlen.
    replace (existsb holds_map p) with true; [reflexivity |].
    symmetry. apply existsb_exists. exists (PNest xs). split; [exact Hin |].
    cbn. apply existsb_exists. exists PZipElem. split; [exact Hz | reflexivity].
Qed.

(* ── 11. Prefix tests, path equality and the prefixes of a path ─────── *)

(* The byte-prefix test of is_prefix_key (path_codec.rs). *)
Fixpoint bprefix (a b : bytes) : bool :=
  match a, b with
  | [], _ => true
  | x :: a', y :: b' => Nat.eqb x y && bprefix a' b'
  | _ :: _, [] => false
  end.

Lemma bprefix_spec : forall a b, bprefix a b = true <-> is_prefix a b.
Proof.
  induction a as [| x a IH]; intros b; cbn.
  - split; [intros _; exists b; reflexivity | reflexivity].
  - destruct b as [| y b].
    + split; [discriminate | intros [z Hz]; discriminate].
    + rewrite andb_true_iff, Nat.eqb_eq, IH. split.
      * intros [<- [z Hz]]. exists z. rewrite Hz. reflexivity.
      * intros [z Hz]. injection Hz as -> Hz. split; [reflexivity | exists z; exact Hz].
Qed.

(* q ⪯ p, decided on the keys as is_prefix_key decides it. *)
Definition prefixb (q p : path) : bool := bprefix (prefix_key q) (path_key p).

Lemma prefixb_spec : forall q p, prefixb q p = true <-> list_prefix q p.
Proof. intros q p. unfold prefixb. rewrite bprefix_spec. apply prefix_key_iff_prefix. Qed.

(* Two paths are equal exactly when their keys are equal (path_key_injective). *)
Definition path_eqb (p q : path) : bool := bytes_eqb (path_key p) (path_key q).

Lemma path_eqb_eq : forall p q, path_eqb p q = true <-> p = q.
Proof.
  intros p q. unfold path_eqb. rewrite bytes_eqb_eq. split.
  - apply path_key_injective.
  - intros ->. reflexivity.
Qed.

Lemma path_eqb_refl : forall p, path_eqb p p = true.
Proof. intros p. apply path_eqb_eq. reflexivity. Qed.

Definition path_eq_dec (p q : path) : {p = q} + {p <> q}.
Proof.
  destruct (path_eqb p q) eqn:E.
  - left. apply path_eqb_eq. exact E.
  - right. intros H. subst q. rewrite path_eqb_refl in E. discriminate.
Defined.

Lemma prefix_key_injective : forall q1 q2, prefix_key q1 = prefix_key q2 -> q1 = q2.
Proof.
  intros q1 q2 H.
  assert (Hp : path_key q1 = path_key q2) by (unfold path_key; rewrite H; reflexivity).
  exact (path_key_injective q1 q2 Hp).
Qed.

Lemma list_prefix_firstn : forall (p : path) k, list_prefix (firstn k p) p.
Proof. intros p k. exists (skipn k p). symmetry. apply firstn_skipn. Qed.

Lemma in_prefixes :
  forall (q p : path), In q (prefixes p) <-> list_prefix q p.
Proof.
  intros q p. unfold prefixes. rewrite in_map_iff. split.
  - intros [k [<- _]]. apply list_prefix_firstn.
  - intros [s ->]. exists (length q). split.
    + rewrite firstn_app, Nat.sub_diag, firstn_O, app_nil_r, firstn_all. reflexivity.
    + apply in_seq. rewrite length_app. lia.
Qed.

Lemma firstn_seq_injective :
  forall (p : path) i j, i <= length p -> j <= length p -> firstn i p = firstn j p -> i = j.
Proof.
  intros p i j Hi Hj H. apply (f_equal (@length term)) in H.
  rewrite !length_firstn in H. lia.
Qed.

Lemma nodup_map_injective_on :
  forall {A B : Type} (f : A -> B) (l : list A),
    NoDup l -> (forall x y, In x l -> In y l -> f x = f y -> x = y) -> NoDup (map f l).
Proof.
  intros A B f l Hnd. induction Hnd as [| x l Hx Hnd IH]; intros Hinj; cbn; [constructor |].
  constructor.
  - intros Hin. apply in_map_iff in Hin as [y [Hy Hiny]].
    assert (x = y) as -> by (apply Hinj; [left; reflexivity | right; exact Hiny | symmetry; exact Hy]).
    contradiction.
  - apply IH. intros a b Ha Hb. apply Hinj; right; assumption.
Qed.

Lemma prefixes_nodup : forall (p : path), NoDup (prefixes p).
Proof.
  intros p. unfold prefixes. apply nodup_map_injective_on; [apply seq_NoDup |].
  intros i j Hi Hj H. apply in_seq in Hi. apply in_seq in Hj.
  apply (firstn_seq_injective p i j); [lia | lia | exact H].
Qed.

(* ── 12. The state of a PathMap space and the marker invariant ──────── *)

Section Engine.
  Variable V : Type.   (* the payload of a datum *)
  Variable Pat : Type. (* the pattern of a waiter *)
  Variable Grd : Type. (* the guard of a waiter *)

  (* A stored datum (a record on D(p)), with its identity, its payload, its
     persistence and its order. *)
  Record datum : Type := mkD {
    d_id : nat; d_path : path; d_val : V; d_pers : bool; d_ord : nat
  }.

  (* A waiter parked on I(q). *)
  Record waiter : Type := mkW {
    w_id : nat; w_pre : path; w_pat : Pat; w_guard : option Grd;
    w_pers : bool; w_peek : bool; w_ord : nat
  }.

  (* A marker on I(m_pre): the source of its datum and the suffix p∖q. *)
  Record marker : Type := mkM { m_pre : path; m_src : nat; m_suf : path }.

  (* The staged view of one space: D(p) for every p, I(q) for every q, the
     waiters, and the token S(sid). *)
  Record state : Type := mkS {
    data : list datum; marks : list marker; wtrs : list waiter; token : nat
  }.

  (* The marker of the datum d on the prefix of depth k (marker_record). *)
  Definition marker_at (d : datum) (k : nat) : marker :=
    mkM (firstn k (d_path d)) (d_id d) (skipn k (d_path d)).

  (* The markers that the store of d adds, in depth order (store_markers). *)
  Definition markers_of (d : datum) : list marker :=
    map (marker_at d) (seq 0 (S (length (d_path d)))).

  (* The marker invariant: the markers are exactly one marker for each
     stored datum and each prefix of its path, as a multiset. *)
  Definition MI (st : state) : Prop := Permutation (marks st) (flat_map markers_of (data st)).

  Definition marker_eqb (a b : marker) : bool :=
    path_eqb (m_pre a) (m_pre b) && Nat.eqb (m_src a) (m_src b) && path_eqb (m_suf a) (m_suf b).

  Lemma marker_eqb_eq : forall a b, marker_eqb a b = true <-> a = b.
  Proof.
    intros [p1 s1 x1] [p2 s2 x2]. unfold marker_eqb. cbn.
    rewrite !andb_true_iff, !path_eqb_eq, Nat.eqb_eq. split.
    - intros [[-> ->] ->]. reflexivity.
    - intros H. injection H as -> -> ->. auto.
  Qed.

  (* The removal of one marker equal to m (the COMM or the K_retire of a
     marker removes it). *)
  Fixpoint remove1 (m : marker) (ms : list marker) : list marker :=
    match ms with
    | [] => []
    | x :: rest => if marker_eqb m x then rest else x :: remove1 m rest
    end.

  Lemma remove1_perm : forall m ms, In m ms -> Permutation ms (m :: remove1 m ms).
  Proof.
    intros m ms. induction ms as [| x rest IH]; intros Hin; [destruct Hin |]. cbn.
    destruct (marker_eqb m x) eqn:E.
    - apply marker_eqb_eq in E. subst. reflexivity.
    - destruct Hin as [-> | Hin].
      + rewrite (proj2 (marker_eqb_eq m m) eq_refl) in E. discriminate.
      + eapply Permutation_trans; [| apply perm_swap]. apply perm_skip. apply IH. exact Hin.
  Qed.

  Lemma remove1_app_perm :
    forall m ms rest, Permutation ms (m :: rest) -> Permutation (remove1 m ms) rest.
  Proof.
    intros m ms rest H.
    assert (Hin : In m ms) by (apply (Permutation_in m (Permutation_sym H)); left; reflexivity).
    pose proof (remove1_perm m ms Hin) as Hr.
    apply (Permutation_cons_inv (a := m)). exact (Permutation_trans (Permutation_sym Hr) H).
  Qed.

  (* The removal of a list of markers in order. *)
  Definition remove_all (xs ms : list marker) : list marker := fold_left (fun acc m => remove1 m acc) xs ms.

  Lemma remove_all_perm :
    forall xs ms rest, Permutation ms (xs ++ rest) -> Permutation (remove_all xs ms) rest.
  Proof.
    induction xs as [| x xs IH]; intros ms rest H; cbn; [exact H |].
    apply IH. apply remove1_app_perm. exact H.
  Qed.

  (* The removal of the element at a position of a list. *)
  Fixpoint remove_nth {A : Type} (n : nat) (l : list A) {struct l} : list A :=
    match l, n with
    | [], _ => []
    | _ :: rest, 0 => rest
    | x :: rest, S n' => x :: remove_nth n' rest
    end.

  Lemma remove_nth_perm :
    forall {A : Type} n (l : list A) x, nth_error l n = Some x -> Permutation l (x :: remove_nth n l).
  Proof.
    intros A n. induction n as [| n IH]; intros [| y l] x H; cbn in H; try discriminate.
    - injection H as ->. reflexivity.
    - cbn. eapply Permutation_trans; [| apply perm_swap]. apply perm_skip. apply IH. exact H.
  Qed.

  Lemma remove_nth_incl :
    forall {A : Type} n (l : list A) x, In x (remove_nth n l) -> In x l.
  Proof.
    intros A n. induction n as [| n IH]; intros [| y l] x H; cbn in *; try contradiction.
    - right. exact H.
    - destruct H as [-> | H]; [left; reflexivity | right; exact (IH l x H)].
  Qed.

  (* The depths of the markers that a removal retires (retire_plan): every
     depth of the path but the delivered one. *)
  Definition retired_depths (n : nat) (delivered : option nat) : list nat :=
    filter (fun j => match delivered with Some k => negb (Nat.eqb j k) | None => true end)
      (seq 0 (S n)).

  Lemma filter_neq_perm :
    forall (l : list nat) k, NoDup l -> In k l -> Permutation l (k :: filter (fun j => negb (Nat.eqb j k)) l).
  Proof.
    induction l as [| x l IH]; intros k Hnd Hin; [destruct Hin |].
    inversion Hnd as [| x' l' Hx Hnd']. subst. cbn.
    destruct (Nat.eqb x k) eqn:E.
    - apply Nat.eqb_eq in E. subst. cbn.
      assert (Hf : filter (fun j => negb (Nat.eqb j k)) l = l).
      { apply forallb_filter_id. apply forallb_forall. intros y Hy.
        destruct (Nat.eqb y k) eqn:Ey; [apply Nat.eqb_eq in Ey; subst; contradiction | reflexivity]. }
      rewrite Hf. reflexivity.
    - cbn. destruct Hin as [-> | Hin]; [rewrite Nat.eqb_refl in E; discriminate |].
      eapply Permutation_trans; [| apply perm_swap]. apply perm_skip. apply IH; assumption.
  Qed.

  (* The delivered marker and the retired markers are all the markers of the
     datum, each once. *)
  Lemma retire_plan_partition :
    forall n k, k <= n -> Permutation (seq 0 (S n)) (k :: retired_depths n (Some k)).
  Proof.
    intros n k Hk. unfold retired_depths. apply filter_neq_perm; [apply seq_NoDup |].
    apply in_seq. lia.
  Qed.

  Lemma retired_depths_none : forall n, retired_depths n None = seq 0 (S n).
  Proof. intros n. unfold retired_depths. apply forallb_filter_id. apply forallb_forall. reflexivity. Qed.

  (* ── 12.1 The effects of the stagings on D and I ── *)

  (* A store of d: D(p):d and the markers of every prefix of p. *)
  Definition store_datum (d : datum) (st : state) : state :=
    mkS (d :: data st) (markers_of d ++ marks st) (wtrs st) (token st).

  (* A consume-side delivery of the datum at position n on the prefix of
     depth k, with the datum removed: the COMM on I(q) removes the marker of
     depth k, and the retire plan removes D(p):d and the other markers. *)
  Definition deliver_remove (n k : nat) (st : state) : state :=
    match nth_error (data st) n with
    | Some d =>
        mkS (remove_nth n (data st))
          (remove_all (map (marker_at d) (retired_depths (length (d_path d)) (Some k)))
             (remove1 (marker_at d k) (marks st)))
          (wtrs st) (token st)
    | None => st
    end.

  (* The COMM of a consume-side peek on the prefix of depth k removes the
     marker of depth k (Rule E). *)
  Definition take_marker (d : datum) (k : nat) (st : state) : state :=
    mkS (data st) (remove1 (marker_at d k) (marks st)) (wtrs st) (token st).

  (* The second send meets no waiter: the marker is stored again with its
     identity. *)
  Definition put_marker (d : datum) (k : nat) (st : state) : state :=
    mkS (data st) (marker_at d k :: marks st) (wtrs st) (token st).

  (* The second send fires a waiter that does not peek: the datum is spent,
     and the retire plan removes D(p):d and the markers that the peek left. *)
  Definition spend_after_peek (n k : nat) (st : state) : state :=
    match nth_error (data st) n with
    | Some d =>
        mkS (remove_nth n (data st))
          (remove_all (map (marker_at d) (retired_depths (length (d_path d)) (Some k))) (marks st))
          (wtrs st) (token st)
    | None => st
    end.

  Lemma flat_map_remove_nth :
    forall n (ds : list datum) d,
      nth_error ds n = Some d ->
      Permutation (flat_map markers_of ds) (markers_of d ++ flat_map markers_of (remove_nth n ds)).
  Proof.
    intros n ds d H. pose proof (remove_nth_perm n ds d H) as Hp.
    rewrite (Permutation_flat_map markers_of Hp). reflexivity.
  Qed.

  Lemma markers_of_split :
    forall d k, k <= length (d_path d) ->
      Permutation (markers_of d)
        (marker_at d k :: map (marker_at d) (retired_depths (length (d_path d)) (Some k))).
  Proof.
    intros d k Hk. unfold markers_of.
    change (marker_at d k :: map (marker_at d) (retired_depths (length (d_path d)) (Some k)))
      with (map (marker_at d) (k :: retired_depths (length (d_path d)) (Some k))).
    apply Permutation_map. apply retire_plan_partition. exact Hk.
  Qed.

  (* The effects of the stagings keep the marker invariant. A store adds one
     marker for each prefix. A consume-side delivery that removes its datum
     removes the delivered marker by its COMM and the record and every other
     marker by the retire plan. A peek COMM removes its marker. The second
     send then restores it, or a fire of the second send spends the datum,
     and the retire plan removes the record and the other markers. A waiter
     change and a token bump touch no marker. The headline
     marker_invariant_preserved lifts these effects to the plans. *)
  Lemma marker_effects_keep_mi :
    (forall d st, MI st -> MI (store_datum d st)) /\
    (forall n k d st, MI st -> nth_error (data st) n = Some d -> k <= length (d_path d) ->
       MI (deliver_remove n k st)) /\
    (forall d k st, MI st -> In d (data st) -> k <= length (d_path d) ->
       MI (put_marker d k (take_marker d k st))) /\
    (forall n k d st, MI st -> nth_error (data st) n = Some d -> k <= length (d_path d) ->
       MI (spend_after_peek n k (take_marker d k st))) /\
    (forall st ws t, MI st -> MI (mkS (data st) (marks st) ws t)).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros d st H. unfold MI, store_datum in *. cbn [marks data flat_map].
      apply Permutation_app_head. exact H.
    - intros n k d st H Hn Hk. unfold MI, deliver_remove in *. rewrite Hn. cbn [marks data].
      apply remove_all_perm. apply remove1_app_perm.
      eapply Permutation_trans; [exact H |].
      eapply Permutation_trans; [apply (flat_map_remove_nth n (data st) d Hn) |].
      rewrite app_comm_cons. apply Permutation_app_tail. apply markers_of_split. exact Hk.
    - intros d k st H Hin Hk. unfold MI, put_marker, take_marker in *. cbn [marks data].
      assert (Hm : In (marker_at d k) (marks st)).
      { apply (Permutation_in _ (Permutation_sym H)). apply in_flat_map. exists d.
        split; [exact Hin |]. unfold markers_of. apply in_map. apply in_seq. lia. }
      eapply Permutation_trans; [apply Permutation_sym, remove1_perm; exact Hm | exact H].
    - intros n k d st H Hn Hk. unfold MI, spend_after_peek, take_marker in *.
      cbn [marks data]. rewrite Hn. cbn [marks data].
      apply remove_all_perm.
      apply (Permutation_cons_inv (a := marker_at d k)).
      assert (Hm : In (marker_at d k) (marks st)).
      { apply (Permutation_in _ (Permutation_sym H)). apply in_flat_map. exists d.
        split; [exact (nth_error_In _ _ Hn) |]. unfold markers_of. apply in_map. apply in_seq. lia. }
      eapply Permutation_trans; [apply Permutation_sym, remove1_perm; exact Hm |].
      eapply Permutation_trans; [exact H |].
      eapply Permutation_trans; [apply (flat_map_remove_nth n (data st) d Hn) |].
      rewrite app_comm_cons. apply Permutation_app_tail. apply markers_of_split. exact Hk.
    - intros st ws t H. exact H.
  Qed.

  (* ── 12.2 The channels and the events of the stagings ── *)

  Inductive chan : Type := ChD (k : bytes) | ChI (k : bytes) | ChS.

  Definition data_chan (p : path) : chan := ChD (path_key p).
  Definition index_chan (q : path) : chan := ChI (prefix_key q).

  (* An event of the substrate: a produce of a record on a channel, a consume
     on a channel group, and a COMM with its consume channels and the
     channels of its produces. *)
  Inductive ev : Type :=
  | EvProduce (c : chan)
  | EvConsume (cs : list chan)
  | EvComm (cs : list chan) (ps : list chan).

  (* The events of a store (plan, "Events"): Produce(D(p)), then
     Produce(I(q)) for each q ⪯ p in depth order. *)
  Definition store_events (p : path) : list ev :=
    EvProduce (data_chan p) :: map (fun q => EvProduce (index_chan q)) (prefixes p).

  (* A waiter parks on I(q). *)
  Definition park_events (q : path) : list ev := [EvConsume [index_chan q]].

  (* A consume-side delivery on I(q): the Consume of the waiter, then the
     COMM over the stored marker m_q. *)
  Definition consume_delivery_events (q : path) : list ev :=
    [EvConsume [index_chan q]; EvComm [index_chan q] [index_chan q]].

  (* A produce-side delivery on I(q): the Produce of the vehicle on I(q),
     then the COMM of the waiter over the vehicle. *)
  Definition produce_delivery_events (q : path) : list ev :=
    [EvProduce (index_chan q); EvComm [index_chan q] [index_chan q]].

  (* One K_retire: a single-channel Consume and its COMM over one record. *)
  Definition retire_events (c : chan) : list ev := [EvConsume [c]; EvComm [c] [c]].

  (* The retire plan of a datum on p after a delivery on the depth k. *)
  Definition retire_plan_events (p : path) (delivered : option nat) : list ev :=
    retire_events (data_chan p)
    ++ flat_map (fun j => retire_events (index_chan (firstn j p))) (retired_depths (length p) delivered).

  (* The token bump: Consume, COMM and Produce on S(sid). *)
  Definition token_events : list ev := [EvConsume [ChS]; EvComm [ChS] [ChS]; EvProduce ChS].

  (* A COMM on one channel whose produces are on that channel, and a consume
     on one channel (I-16). *)
  Definition single_channel (e : ev) : Prop :=
    match e with
    | EvComm [c] ps => Forall (eq c) ps
    | EvComm _ _ => False
    | EvConsume [c] => True
    | EvConsume _ => False
    | EvProduce _ => True
    end.

  Lemma retire_events_single : forall c, Forall single_channel (retire_events c).
  Proof. intros c. repeat constructor. Qed.

  Lemma flat_map_single :
    forall {A : Type} (f : A -> list ev) xs,
      (forall x, Forall single_channel (f x)) -> Forall single_channel (flat_map f xs).
  Proof.
    intros A f xs H. induction xs as [| x xs IH]; cbn; [constructor |].
    apply Forall_app. split; [apply H | exact IH].
  Qed.

  (* HEADLINE 10 (delivery_vehicle_single_channel): every event of the
     engine is single-channel (F4, I-16). A delivery COMM, on either side, is
     a COMM on I(q) over a record of I(q): the stored marker m_q on the
     consume side and the vehicle on the produce side. The produce-side
     delivery produces the vehicle on I(q) and on no data channel, and it
     changes no datum and no marker, so the vehicle is never stored. Each
     retire and each token step is a single-channel COMM. *)
  Theorem delivery_vehicle_single_channel :
    (forall q, Forall single_channel (consume_delivery_events q) /\
               In (EvComm [index_chan q] [index_chan q]) (consume_delivery_events q)) /\
    (forall q, Forall single_channel (produce_delivery_events q) /\
               In (EvComm [index_chan q] [index_chan q]) (produce_delivery_events q) /\
               (forall p, ~ In (EvProduce (data_chan p)) (produce_delivery_events q))) /\
    (forall p delivered, Forall single_channel (retire_plan_events p delivered)) /\
    (forall p, Forall single_channel (store_events p)) /\
    (forall q, Forall single_channel (park_events q)) /\
    Forall single_channel token_events /\
    (forall st ws, data (mkS (data st) (marks st) ws (token st)) = data st /\
                   marks (mkS (data st) (marks st) ws (token st)) = marks st).
  Proof.
    split; [| split; [| split; [| split; [| split; [| split]]]]].
    - intros q. split; [repeat constructor | right; left; reflexivity].
    - intros q. split; [repeat constructor |]. split; [right; left; reflexivity |].
      intros p [H | [H | []]]; discriminate H.
    - intros p delivered. unfold retire_plan_events. apply Forall_app. split.
      + apply retire_events_single.
      + apply flat_map_single. intros j. apply retire_events_single.
    - intros p. unfold store_events. constructor; [exact I |].
      apply Forall_forall. intros e He. apply in_map_iff in He as [q [<- _]]. exact I.
    - intros q. repeat constructor.
    - repeat constructor.
    - intros st ws. split; reflexivity.
  Qed.

  (* ── 12.3 Prefix contact ── *)

  (* HEADLINE 11 (prefix_contact_anchored): the markers anchor the contact of
     a datum and a waiter. Every stored datum on p holds a marker on I(q) for
     each q ⪯ p, so its store produced on the channel I(q) where a waiter on
     q parks, and merge check #2 sees a new datum on one branch and a new
     waiter on q ⪯ p on the other (plan, "Cross-branch contact"). Every
     marker belongs to a stored datum under its prefix, and the index
     channels are injective, so no datum touches the index channel of a
     prefix that is not a prefix of its path. *)
  Theorem prefix_contact_anchored :
    (forall st d q, MI st -> In d (data st) -> list_prefix q (d_path d) ->
       exists m, In m (marks st) /\ m_pre m = q /\ m_src m = d_id d /\ d_path d = q ++ m_suf m) /\
    (forall st m, MI st -> In m (marks st) ->
       exists d, In d (data st) /\ m_src m = d_id d /\ d_path d = m_pre m ++ m_suf m) /\
    (forall q1 q2, index_chan q1 = index_chan q2 -> q1 = q2) /\
    (forall p q, list_prefix q p ->
       In (EvProduce (index_chan q)) (store_events p) /\
       In (EvConsume [index_chan q]) (park_events q)) /\
    (forall p c, In (EvProduce c) (store_events p) ->
       c = data_chan p \/ exists q, c = index_chan q /\ list_prefix q p).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros st d q H Hin [s Hs]. exists (marker_at d (length q)).
      split.
      + apply (Permutation_in _ (Permutation_sym H)). apply in_flat_map. exists d.
        split; [exact Hin |]. unfold markers_of. apply in_map. apply in_seq.
        rewrite Hs, length_app. lia.
      + unfold marker_at. cbn. rewrite Hs.
        rewrite firstn_app, Nat.sub_diag, firstn_O, app_nil_r, firstn_all.
        rewrite skipn_app, Nat.sub_diag, skipn_O, skipn_all.
        split; [reflexivity | split; reflexivity].
    - intros st m H Hin. apply (Permutation_in _ H) in Hin. apply in_flat_map in Hin as [d [Hd Hm]].
      exists d. split; [exact Hd |]. unfold markers_of in Hm. apply in_map_iff in Hm as [k [<- _]].
      unfold marker_at. cbn. split; [reflexivity |]. symmetry. apply firstn_skipn.
    - intros q1 q2 H. injection H as H. apply prefix_key_injective. exact H.
    - intros p q Hq. split.
      + unfold store_events. right. apply in_map_iff. exists q.
        split; [reflexivity | apply in_prefixes; exact Hq].
      + left. reflexivity.
    - intros p c H. unfold store_events in H. destruct H as [H | H].
      + left. injection H as H. symmetry. exact H.
      + right. apply in_map_iff in H as [q [Hq Hin]]. injection Hq as <-.
        exists q. split; [reflexivity |]. apply in_prefixes. exact Hin.
  Qed.

  (* ── 12.4 The second send of a peek ── *)

  (* The prefixes that the second send of a datum on p meets, the most
     specific first (produce_waiters). *)
  Definition send_prefixes (p : path) : list path := rev (prefixes p).

  Lemma send_prefixes_spec :
    forall q p, In q (send_prefixes p) <-> list_prefix q p.
  Proof. intros q p. unfold send_prefixes. rewrite <- ?in_rev. apply in_prefixes. Qed.

  Lemma send_prefixes_depths :
    forall (p : path), map (@length term) (send_prefixes p) = rev (seq 0 (S (length p))).
  Proof.
    intros p. unfold send_prefixes, prefixes. rewrite map_rev, map_map. f_equal.
    rewrite <- (map_id (seq 0 (S (length p)))) at 2. apply map_ext_in.
    intros k Hk. apply in_seq in Hk. rewrite length_firstn. lia.
  Qed.

  (* HEADLINE 12 (peek_restores_delivered_marker, restated for the second
     send of decision 45): a consume-side peek of a datum d on the prefix of
     depth k removes the marker m_k in its COMM. The second send of d meets
     the waiters of the prefixes of the path of d, each prefix once, the
     most specific first. When no waiter takes it, the marker is stored again
     with its identity, and the data and the markers are the data and the
     markers before the peek, up to the order. When a waiter that does not
     peek takes it, the datum leaves with all of its markers. *)
  Theorem peek_restores_delivered_marker :
    (forall d k st, MI st -> In d (data st) -> k <= length (d_path d) ->
       data (put_marker d k (take_marker d k st)) = data st /\
       Permutation (marks (put_marker d k (take_marker d k st))) (marks st)) /\
    (forall n k d st, MI st -> nth_error (data st) n = Some d -> k <= length (d_path d) ->
       data (spend_after_peek n k (take_marker d k st)) = remove_nth n (data st) /\
       Permutation (marks st) (markers_of d ++ marks (spend_after_peek n k (take_marker d k st)))) /\
    (forall p q, In q (send_prefixes p) <-> list_prefix q p) /\
    (forall p, NoDup (send_prefixes p)) /\
    (forall p, map (@length term) (send_prefixes p) = rev (seq 0 (S (length p)))).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros d k st H Hin Hk. split; [reflexivity |]. unfold put_marker, take_marker.
      cbn [marks data].
      assert (Hm : In (marker_at d k) (marks st)).
      { apply (Permutation_in _ (Permutation_sym H)). apply in_flat_map. exists d.
        split; [exact Hin |]. unfold markers_of. apply in_map. apply in_seq. lia. }
      apply Permutation_sym. apply remove1_perm. exact Hm.
    - intros n k d st H Hn Hk.
      pose proof (proj1 (proj2 (proj2 (proj2 marker_effects_keep_mi))) n k d st H Hn Hk) as Hmi.
      unfold spend_after_peek, take_marker in *. cbn [marks data] in *. rewrite Hn in *.
      cbn [marks data] in *.
      split; [reflexivity |]. unfold MI in Hmi. cbn [marks data] in Hmi.
      eapply Permutation_trans; [exact H |].
      eapply Permutation_trans; [apply (flat_map_remove_nth n (data st) d Hn) |].
      apply Permutation_app_head. apply Permutation_sym. exact Hmi.
    - intros p q. apply send_prefixes_spec.
    - intros p. unfold send_prefixes. apply NoDup_rev. apply prefixes_nodup.
    - exact send_prefixes_depths.
  Qed.

  (* ── 13. The scan driver on prefixes (bag, set) ─────────────────────── *)

  (* The spatial match of a pattern against the re-rooted datum R(s, v) and
     the where guard of decision 37 (SpaceMatcher::bind, ::admits). *)
  Variable spatial : Pat -> path -> V -> bool.
  Variable admits : Grd -> path -> V -> bool.

  (* The waiter order of the discipline within one prefix (DR-75 for a bag
     and a set) and the candidate order of a receive (the path key, then the
     discipline). The theorems hold for every order that permutes its input. *)
  Variable worder : list waiter -> list waiter.
  Variable dorder : list datum -> list datum.

  Definition suffix_of (w : waiter) (d : datum) : path := skipn (length (w_pre w)) (d_path d).

  (* A waiter on q sees the datums on p ⪰ q (CONSUME-CANDIDATES). *)
  Definition visible (w : waiter) (d : datum) : bool := prefixb (w_pre w) (d_path d).

  Definition spatial_ok (w : waiter) (d : datum) : bool :=
    visible w d && spatial (w_pat w) (suffix_of w d) (d_val d).

  Definition guard_ok (w : waiter) (d : datum) : bool :=
    match w_guard w with
    | None => true
    | Some g => admits g (suffix_of w d) (d_val d)
    end.

  Definition fires (w : waiter) (d : datum) : bool := spatial_ok w d && guard_ok w d.

  (* No waiter without a guard has a spatial match with a stored datum under
     its prefix (FIPS:97-107 for prefixes, decision 37). *)
  Definition no_match (st : state) : Prop :=
    forall w d, In w (wtrs st) -> In d (data st) -> w_guard w = None -> spatial_ok w d = false.

  (* The waiters of one prefix. *)
  Definition waiters_on (ws : list waiter) (q : path) : list waiter :=
    filter (fun w => path_eqb (w_pre w) q) ws.

  (* PRODUCE-WAITERS: the waiters of each prefix of p, the most specific
     prefix first, each prefix in the waiter order of the discipline. *)
  Definition offer_list (ws : list waiter) (p : path) : list waiter :=
    flat_map (fun q => worder (waiters_on ws q)) (send_prefixes p).

  (* CONSUME-CANDIDATES: the datums under q in the candidate order. *)
  Definition candidates (ds : list datum) (q : path) : list datum :=
    dorder (filter (fun d => prefixb q (d_path d)) ds).

  Lemma offer_list_complete :
    (forall ws, Permutation (worder ws) ws) ->
    forall ws p w, In w (offer_list ws p) <-> In w ws /\ list_prefix (w_pre w) p.
  Proof.
    intros Hperm ws p w. unfold offer_list. rewrite in_flat_map. split.
    - intros [q [Hq Hw]]. apply (Permutation_in _ (Hperm _)) in Hw.
      unfold waiters_on in Hw. apply filter_In in Hw as [Hw He]. apply path_eqb_eq in He.
      subst q. split; [exact Hw | apply send_prefixes_spec; exact Hq].
    - intros [Hw Hp]. exists (w_pre w). split; [apply send_prefixes_spec; exact Hp |].
      apply (Permutation_in _ (Permutation_sym (Hperm _))). unfold waiters_on.
      apply filter_In. split; [exact Hw | apply path_eqb_refl].
  Qed.

  Lemma candidates_complete :
    (forall ds, Permutation (dorder ds) ds) ->
    forall ds q d, In d (candidates ds q) <-> In d ds /\ list_prefix q (d_path d).
  Proof.
    intros Hperm ds q d. unfold candidates. split.
    - intros H. apply (Permutation_in _ (Hperm _)) in H. apply filter_In in H as [Hd Hp].
      split; [exact Hd | apply prefixb_spec; exact Hp].
    - intros [Hd Hp]. apply (Permutation_in _ (Permutation_sym (Hperm _))). apply filter_In.
      split; [exact Hd | apply prefixb_spec; exact Hp].
  Qed.

  Fixpoint first_fire (d : datum) (ws : list waiter) : option (waiter * list waiter) :=
    match ws with
    | [] => None
    | w :: rest => if fires w d then Some (w, rest) else first_fire d rest
    end.

  Lemma first_fire_none : forall d ws, first_fire d ws = None -> forall w, In w ws -> fires w d = false.
  Proof.
    intros d ws. induction ws as [| x rest IH]; intros H w Hin; [destruct Hin |]. cbn in H.
    destruct (fires x d) eqn:E; [discriminate |].
    destruct Hin as [-> | Hin]; [exact E | exact (IH H w Hin)].
  Qed.

  Lemma first_fire_some :
    forall d ws w rest, first_fire d ws = Some (w, rest) ->
      exists pre, ws = pre ++ w :: rest /\ (forall x, In x pre -> fires x d = false) /\ fires w d = true.
  Proof.
    intros d ws. induction ws as [| x ws IH]; intros w rest H; cbn in H; [discriminate |].
    destruct (fires x d) eqn:E.
    - injection H as <- <-. exists []. split; [reflexivity |]. split; [intros y [] | exact E].
    - destruct (IH w rest H) as [pre [Hws [Hpre Hw]]]. exists (x :: pre). split; [rewrite Hws; reflexivity |].
      split; [| exact Hw]. intros y [<- | Hy]; [exact E | exact (Hpre y Hy)].
  Qed.

  Fixpoint first_spatial (w : waiter) (ds : list datum) : option (datum * list datum) :=
    match ds with
    | [] => None
    | d :: rest => if spatial_ok w d then Some (d, rest) else first_spatial w rest
    end.

  Lemma first_spatial_none :
    forall w ds, first_spatial w ds = None -> forall d, In d ds -> spatial_ok w d = false.
  Proof.
    intros w ds. induction ds as [| x rest IH]; intros H d Hin; [destruct Hin |]. cbn in H.
    destruct (spatial_ok w x) eqn:E; [discriminate |].
    destruct Hin as [-> | Hin]; [exact E | exact (IH H d Hin)].
  Qed.

  Lemma first_spatial_some :
    forall w ds d rest, first_spatial w ds = Some (d, rest) ->
      exists pre, ds = pre ++ d :: rest /\ (forall x, In x pre -> spatial_ok w x = false) /\
                  spatial_ok w d = true.
  Proof.
    intros w ds. induction ds as [| x ds IH]; intros d rest H; cbn in H; [discriminate |].
    destruct (spatial_ok w x) eqn:E.
    - injection H as <- <-. exists []. split; [reflexivity |]. split; [intros y [] | exact E].
    - destruct (IH d rest H) as [pre [Hds [Hpre Hd]]]. exists (x :: pre). split; [rewrite Hds; reflexivity |].
      split; [| exact Hd]. intros y [<- | Hy]; [exact E | exact (Hpre y Hy)].
  Qed.

  Inductive outcome : Type := Done | Reissue.

  (* A waiter that leaves: its fire spent it. *)
  Definition drop_waiter (i : nat) (st : state) : state :=
    mkS (data st) (marks st) (filter (fun w => negb (Nat.eqb (w_id w) i)) (wtrs st)) (token st).

  (* A waiter that parks on its prefix. *)
  Definition park (w : waiter) (st : state) : state :=
    mkS (data st) (marks st) (w :: wtrs st) (token st).

  (* The produce plan of a scan discipline on a PathMap space: OFFER-SCAN
     over PRODUCE-WAITERS. The datum fires the first waiter of the list that
     it matches and whose guard admits it. A refusal moves on (decision 37).
     land stores the datum when no waiter takes it, and spend removes it
     when a waiter that does not peek takes it. A fresh send lands by a
     store. The second send of a peek lands by the restore of its marker. *)
  Fixpoint offer_loop (fuel : nat) (d : datum) (land spend : state -> state)
    (cands : list waiter) (st : state) : state * outcome :=
    match fuel with
    | 0 => (st, Reissue)
    | S f =>
        match first_fire d cands with
        | None => (land st, Done)
        | Some (w, rest) =>
            let st1 := if w_pers w then st else drop_waiter (w_id w) st in
            if negb (d_pers d) && negb (w_peek w) then (spend st1, Done)
            else if w_pers w then (st1, Reissue)
            else offer_loop f d land spend rest st1
        end
    end.

  (* The position of the datum with the identity i. *)
  Fixpoint find_pos (i : nat) (ds : list datum) : option nat :=
    match ds with
    | [] => None
    | d :: rest =>
        if Nat.eqb (d_id d) i then Some 0
        else match find_pos i rest with Some n => Some (S n) | None => None end
    end.

  (* The consume plan of a scan discipline on a PathMap space: RECEIVE-SCAN
     over CONSUME-CANDIDATES. The waiter takes the first candidate that it
     matches spatially and checks its guard once (decision 37). A refusal
     parks it. A fire removes a datum that is not persistent with all of its
     markers. A peek of a datum that is not persistent removes the marker of
     its COMM, and the second send offers the datum to PRODUCE-WAITERS
     (decision 45, the PathMap analogue of RESTORE-SCAN). A persistent
     receive takes the next candidate in the next round. The candidates of
     each round come from the state of the round: the candidates before a
     fired datum did not match, so the choices equal those of a scan that
     continues after the fired datum (the tested prefix of scan.rs). *)
  Fixpoint receive_loop (fuel : nat) (w : waiter) (st : state) : state * outcome :=
    match fuel with
    | 0 => (st, Reissue)
    | S f =>
        match first_spatial w (candidates (data st) (w_pre w)) with
        | None => (park w st, Done)
        | Some (d, _) =>
            if negb (guard_ok w d) then (park w st, Done)
            else if d_pers d then (st, if w_pers w then Reissue else Done)
            else match find_pos (d_id d) (data st) with
                 | None => (st, Reissue)
                 | Some n =>
                     let k := length (w_pre w) in
                     if w_peek w then
                       offer_loop f d (put_marker d k) (spend_after_peek n k)
                         (offer_list (wtrs st) (d_path d)) (take_marker d k st)
                     else if w_pers w then receive_loop f w (deliver_remove n k st)
                     else (deliver_remove n k st, Done)
                 end
        end
    end.

  Definition wf_data (st : state) : Prop := NoDup (map d_id (data st)).

  Lemma no_match_subset :
    forall st st', no_match st -> incl (wtrs st') (wtrs st) -> incl (data st') (data st) -> no_match st'.
  Proof. intros st st' H Hw Hd w d Hwin Hdin Hg. apply H; [apply Hw | apply Hd |]; assumption. Qed.

  Lemma drop_waiter_incl : forall i st, incl (wtrs (drop_waiter i st)) (wtrs st).
  Proof. intros i st w Hw. unfold drop_waiter in Hw. cbn in Hw. apply filter_In in Hw. apply Hw. Qed.

  Lemma drop_waiter_gone : forall i st w, In w (wtrs (drop_waiter i st)) -> w_id w <> i.
  Proof.
    intros i st w Hw. unfold drop_waiter in Hw. cbn in Hw. apply filter_In in Hw as [_ H].
    apply negb_true_iff, Nat.eqb_neq in H. exact H.
  Qed.

  Lemma deliver_remove_incl :
    forall n k st, incl (data (deliver_remove n k st)) (data st) /\ wtrs (deliver_remove n k st) = wtrs st.
  Proof.
    intros n k st. unfold deliver_remove. destruct (nth_error (data st) n); cbn.
    - split; [intros x Hx; exact (remove_nth_incl n (data st) x Hx) | reflexivity].
    - split; [intros x Hx; exact Hx | reflexivity].
  Qed.

  Lemma spend_after_peek_incl :
    forall n k st, incl (data (spend_after_peek n k st)) (data st) /\ wtrs (spend_after_peek n k st) = wtrs st.
  Proof.
    intros n k st. unfold spend_after_peek. destruct (nth_error (data st) n); cbn.
    - split; [intros x Hx; exact (remove_nth_incl n (data st) x Hx) | reflexivity].
    - split; [intros x Hx; exact Hx | reflexivity].
  Qed.

  (* The invariant of the offer loop: every waiter of the state under the
     path of the datum is still a candidate, or it refuses the datum. *)
  Definition offer_inv (d : datum) (cands : list waiter) (st : state) : Prop :=
    forall w, In w (wtrs st) -> list_prefix (w_pre w) (d_path d) -> In w cands \/ fires w d = false.

  Lemma offer_loop_no_match :
    forall fuel d land spend cands st st',
      (forall s, no_match s -> (forall w, In w (wtrs s) -> w_guard w = None -> spatial_ok w d = false) ->
         no_match (land s)) ->
      (forall s, no_match s -> no_match (spend s)) ->
      no_match st -> offer_inv d cands st ->
      offer_loop fuel d land spend cands st = (st', Done) -> no_match st'.
  Proof.
    induction fuel as [| fuel IH]; intros d land spend cands st st' Hland Hspend Hnm Hinv H;
      cbn in H; [discriminate |].
    destruct (first_fire d cands) as [[w rest] |] eqn:Ef.
    - destruct (first_fire_some d cands w rest Ef) as [pre [Hc [Hpre Hw]]].
      set (st1 := if w_pers w then st else drop_waiter (w_id w) st) in H.
      assert (Hnm1 : no_match st1).
      { unfold st1. destruct (w_pers w); [exact Hnm |].
        apply (no_match_subset st); [exact Hnm | apply drop_waiter_incl | intros x Hx; exact Hx]. }
      destruct (negb (d_pers d) && negb (w_peek w)) eqn:Espent.
      + injection H as <-. apply Hspend. exact Hnm1.
      + destruct (w_pers w) eqn:Ep; [discriminate |].
        apply (IH d land spend rest st1 st' Hland Hspend Hnm1); [| exact H].
        intros x Hx Hpx. unfold st1 in Hx.
        pose proof (drop_waiter_gone _ _ _ Hx) as Hne.
        destruct (Hinv x (drop_waiter_incl _ _ x Hx) Hpx) as [Hin | Hr]; [| right; exact Hr].
        rewrite Hc in Hin. apply in_app_or in Hin as [Hin | [Heq | Hin]].
        * right. exact (Hpre x Hin).
        * subst x. contradiction.
        * left. exact Hin.
    - injection H as <-. apply Hland; [exact Hnm |].
      intros x Hx Hg. destruct (spatial_ok x d) eqn:Es; [| reflexivity].
      assert (Hpx : list_prefix (w_pre x) (d_path d)).
      { unfold spatial_ok, visible in Es. apply andb_true_iff in Es as [Hv _]. apply prefixb_spec. exact Hv. }
      destruct (Hinv x Hx Hpx) as [Hin | Hr].
      + pose proof (first_fire_none d cands Ef x Hin) as Hr. unfold fires, guard_ok in Hr.
        rewrite Hg, Es in Hr. discriminate.
      + unfold fires, guard_ok in Hr. rewrite Hg, Es in Hr. discriminate.
  Qed.

  (* The store of a fresh datum keeps no_match when no waiter without a guard
     has a spatial match with it. *)
  Lemma store_no_match :
    forall d s, no_match s -> (forall w, In w (wtrs s) -> w_guard w = None -> spatial_ok w d = false) ->
      no_match (store_datum d s).
  Proof.
    intros d s Hnm Hd w x Hw Hx Hg. cbn in Hw, Hx. destruct Hx as [<- | Hx]; [exact (Hd w Hw Hg) |].
    exact (Hnm w x Hw Hx Hg).
  Qed.

  Lemma spend_no_match : forall n k s, no_match s -> no_match (spend_after_peek n k s).
  Proof.
    intros n k s Hnm. destruct (spend_after_peek_incl n k s) as [Hd Hw].
    apply (no_match_subset s); [exact Hnm | rewrite Hw; intros x Hx; exact Hx | exact Hd].
  Qed.

  Lemma offer_inv_start :
    (forall ws, Permutation (worder ws) ws) ->
    forall d st, offer_inv d (offer_list (wtrs st) (d_path d)) st.
  Proof.
    intros Hperm d st w Hw Hp. left. apply (offer_list_complete Hperm). split; assumption.
  Qed.

  Lemma receive_loop_no_match :
    (forall ws, Permutation (worder ws) ws) ->
    (forall ds, Permutation (dorder ds) ds) ->
    forall fuel w st st', no_match st -> receive_loop fuel w st = (st', Done) -> no_match st'.
  Proof.
    intros Hwp Hdp. induction fuel as [| fuel IH]; intros w st st' Hnm H; cbn in H; [discriminate |].
    destruct (first_spatial w (candidates (data st) (w_pre w))) as [[d rest] |] eqn:Es.
    - destruct (guard_ok w d) eqn:Eg; cbn in H.
      + destruct (d_pers d) eqn:Ep.
        * destruct (w_pers w); injection H as <-; exact Hnm.
        * destruct (find_pos (d_id d) (data st)) as [n |] eqn:En; [| discriminate].
          destruct (w_peek w) eqn:Ek.
          -- apply (offer_loop_no_match fuel d (put_marker d (length (w_pre w)))
                     (spend_after_peek n (length (w_pre w)))
                     (offer_list (wtrs st) (d_path d)) (take_marker d (length (w_pre w)) st) st').
             ++ intros s Hs _. exact Hs.
             ++ intros s Hs. exact (spend_no_match n _ s Hs).
             ++ exact Hnm.
             ++ intros y Hy Hpy. left. apply (offer_list_complete Hwp). split; assumption.
             ++ exact H.
          -- destruct (deliver_remove_incl n (length (w_pre w)) st) as [Hdi Hwe].
             assert (Hnm1 : no_match (deliver_remove n (length (w_pre w)) st)).
             { apply (no_match_subset st); [exact Hnm | rewrite Hwe; intros y Hy; exact Hy | exact Hdi]. }
             destruct (w_pers w).
             ++ exact (IH w _ st' Hnm1 H).
             ++ injection H as <-. exact Hnm1.
      + injection H as <-. intros x y Hx Hy Hg. cbn in Hx. destruct Hx as [Heq | Hx].
        * subst x. unfold guard_ok in Eg. rewrite Hg in Eg. discriminate.
        * exact (Hnm x y Hx Hy Hg).
    - injection H as <-. intros x y Hx Hy Hg. cbn in Hx. destruct Hx as [Heq | Hx].
      + subst x. destruct (spatial_ok w y) eqn:Es2; [| reflexivity].
        assert (Hpy : list_prefix (w_pre w) (d_path y)).
        { unfold spatial_ok, visible in Es2. apply andb_true_iff in Es2 as [Hv _].
          apply prefixb_spec. exact Hv. }
        assert (Hin : In y (candidates (data st) (w_pre w))).
        { apply (candidates_complete Hdp). split; assumption. }
        rewrite (first_spatial_none w _ Es y Hin) in Es2. discriminate.
      + exact (Hnm x y Hx Hy Hg).
  Qed.

  (* HEADLINE 13 (no_match_remains): the plans of a bag or a set on a PathMap
     space keep the rule of no pending match. A send meets every waiter under
     its path (PRODUCE-WAITERS is complete) and lands only when no waiter
     takes it, and a receive meets every datum under its prefix
     (CONSUME-CANDIDATES is complete) and parks only when no candidate
     matches it spatially or its guard refuses. So after every plan that
     completes, no waiter without a guard has a spatial match with a stored
     datum under its prefix. A guarded waiter may keep a pending match
     (decision 37, readiness review S3). *)
  Theorem no_match_remains :
    (forall ws, Permutation (worder ws) ws) ->
    (forall ds, Permutation (dorder ds) ds) ->
    (forall fuel d st st', no_match st ->
       offer_loop fuel d (store_datum d) (fun s => s) (offer_list (wtrs st) (d_path d)) st = (st', Done) ->
       no_match st') /\
    (forall fuel w st st', no_match st -> receive_loop fuel w st = (st', Done) -> no_match st') /\
    (forall ws p w, In w (offer_list ws p) <-> In w ws /\ list_prefix (w_pre w) p) /\
    (forall ds q d, In d (candidates ds q) <-> In d ds /\ list_prefix q (d_path d)) /\
    (forall st w d, no_match st -> In w (wtrs st) -> In d (data st) -> w_guard w = None ->
       list_prefix (w_pre w) (d_path d) -> spatial (w_pat w) (suffix_of w d) (d_val d) = false).
  Proof.
    intros Hwp Hdp. split; [| split; [| split; [| split]]].
    - intros fuel d st st' Hnm H.
      apply (offer_loop_no_match fuel d (store_datum d) (fun s => s)
               (offer_list (wtrs st) (d_path d)) st st');
        [intros s Hs Hd; exact (store_no_match d s Hs Hd) | intros s Hs; exact Hs | exact Hnm
        | apply offer_inv_start; exact Hwp | exact H].
    - exact (receive_loop_no_match Hwp Hdp).
    - exact (offer_list_complete Hwp).
    - exact (candidates_complete Hdp).
    - intros st w d Hnm Hw Hd Hg Hp. pose proof (Hnm w d Hw Hd Hg) as H.
      unfold spatial_ok, visible in H. rewrite (proj2 (prefixb_spec _ _) Hp) in H. exact H.
  Qed.

  (* ── 14. The marker invariant of the plans ─────────────────────────── *)

  Lemma find_pos_spec :
    forall i ds n, find_pos i ds = Some n -> exists d, nth_error ds n = Some d /\ d_id d = i.
  Proof.
    intros i ds. induction ds as [| x rest IH]; intros n H; cbn in H; [discriminate |].
    destruct (Nat.eqb (d_id x) i) eqn:E.
    - injection H as <-. exists x. split; [reflexivity | apply Nat.eqb_eq; exact E].
    - destruct (find_pos i rest) as [m |] eqn:Em; [| discriminate]. injection H as <-.
      destruct (IH m eq_refl) as [y [Hy Hi]]. exists y. split; [exact Hy | exact Hi].
  Qed.

  Lemma remove_nth_nodup :
    forall (ds : list datum) n, NoDup (map d_id ds) -> NoDup (map d_id (remove_nth n ds)).
  Proof.
    induction ds as [| z ds IH]; intros n Hnd; [constructor |].
    inversion Hnd as [| z' l Hz Hnd']. subst. destruct n as [| n]; cbn; [exact Hnd' |].
    constructor; [| apply IH; exact Hnd'].
    intros Hin. apply Hz. apply in_map_iff in Hin as [y [Hy Hiny]]. rewrite <- Hy. apply in_map.
    exact (remove_nth_incl n ds y Hiny).
  Qed.

  Lemma deliver_remove_wf : forall n k st, wf_data st -> wf_data (deliver_remove n k st).
  Proof.
    intros n k st H. unfold wf_data, deliver_remove in *. destruct (nth_error (data st) n); cbn;
      [apply remove_nth_nodup; exact H | exact H].
  Qed.

  Lemma drop_waiter_dm :
    forall i s, data (drop_waiter i s) = data s /\ marks (drop_waiter i s) = marks s.
  Proof. intros i s. split; reflexivity. Qed.

  (* The end of an offer loop: a loop that completes applies land or spend
     to a state of the loop, and a loop that re-issues ends in a state of
     the loop. P holds in every state of the loop, because a fire only drops
     waiters. *)
  Lemma offer_loop_ends :
    forall (P Q : state -> Prop) fuel d land spend cands st st' o,
      (forall s i, P s -> P (drop_waiter i s)) ->
      (forall s, P s -> Q (land s)) -> (forall s, P s -> Q (spend s)) ->
      P st -> offer_loop fuel d land spend cands st = (st', o) ->
      (o = Done /\ Q st') \/ (o = Reissue /\ P st').
  Proof.
    intros P Q. induction fuel as [| fuel IH]; intros d land spend cands st st' o Hdrop Hland Hspend Hp H;
      cbn in H.
    - injection H as <- <-. right. split; [reflexivity | exact Hp].
    - destruct (first_fire d cands) as [[w rest] |].
      + set (st1 := if w_pers w then st else drop_waiter (w_id w) st) in H.
        assert (Hp1 : P st1) by (unfold st1; destruct (w_pers w); [exact Hp | apply Hdrop; exact Hp]).
        destruct (negb (d_pers d) && negb (w_peek w)).
        * injection H as <- <-. left. split; [reflexivity | apply Hspend; exact Hp1].
        * destruct (w_pers w).
          -- injection H as <- <-. right. split; [reflexivity | exact Hp1].
          -- exact (IH d land spend rest st1 st' o Hdrop Hland Hspend Hp1 H).
      + injection H as <- <-. left. split; [reflexivity | apply Hland; exact Hp].
  Qed.

  Lemma mi_drop : forall s i, MI s -> MI (drop_waiter i s).
  Proof. intros s i H. exact H. Qed.

  Lemma nodup_id_unique :
    forall (ds : list datum) a b, NoDup (map d_id ds) -> In a ds -> In b ds -> d_id a = d_id b -> a = b.
  Proof.
    induction ds as [| x ds IH]; intros a b Hnd Ha Hb Hid; [destruct Ha |].
    inversion Hnd as [| x' l Hx Hnd']. subst.
    destruct Ha as [<- | Ha]; destruct Hb as [<- | Hb].
    - reflexivity.
    - exfalso. apply Hx. rewrite Hid. apply in_map. exact Hb.
    - exfalso. apply Hx. rewrite <- Hid. apply in_map. exact Ha.
    - exact (IH a b Hnd' Ha Hb Hid).
  Qed.

  Lemma visible_depth :
    forall w d, visible w d = true -> length (w_pre w) <= length (d_path d).
  Proof.
    intros w d H. unfold visible in H. apply prefixb_spec in H as [s Hs].
    rewrite Hs, length_app. lia.
  Qed.

  Lemma first_spatial_in :
    forall w ds d rest, first_spatial w ds = Some (d, rest) -> In d ds /\ spatial_ok w d = true.
  Proof.
    intros w ds d rest H. destruct (first_spatial_some w ds d rest H) as [pre [Hds [_ Hd]]].
    split; [rewrite Hds; apply in_or_app; right; left; reflexivity | exact Hd].
  Qed.

  (* The receive loop keeps the marker invariant when it completes. When it
     re-issues, either the invariant holds, or the second send of a peek
     re-issues at the cap with the marker of its COMM pending: storing that
     marker restores the invariant. *)
  Lemma receive_loop_mi :
    (forall ds, Permutation (dorder ds) ds) ->
    forall fuel w st st' o, MI st -> wf_data st -> receive_loop fuel w st = (st', o) ->
      (o = Done /\ MI st') \/
      (o = Reissue /\ (MI st' \/ exists d k, k <= length (d_path d) /\ MI (put_marker d k st'))).
  Proof.
    intros Hdp. induction fuel as [| fuel IH]; intros w st st' o Hmi Hwf H; cbn in H.
    - injection H as <- <-. right. split; [reflexivity | left; exact Hmi].
    - destruct (first_spatial w (candidates (data st) (w_pre w))) as [[d rest] |] eqn:Es.
      + destruct (first_spatial_in _ _ _ _ Es) as [Hdc Hsp].
        apply (candidates_complete Hdp) in Hdc as [Hd Hpre].
        assert (Hk : length (w_pre w) <= length (d_path d)).
        { apply visible_depth. unfold spatial_ok in Hsp. apply andb_true_iff in Hsp. apply Hsp. }
        destruct (guard_ok w d); cbn in H.
        * destruct (d_pers d).
          -- destruct (w_pers w); injection H as <- <-.
             ++ right. split; [reflexivity | left; exact Hmi].
             ++ left. split; [reflexivity | exact Hmi].
          -- destruct (find_pos (d_id d) (data st)) as [n |] eqn:En;
               [| injection H as <- <-; right; split; [reflexivity | left; exact Hmi]].
             destruct (find_pos_spec _ _ _ En) as [x [Hx Hid]].
             assert (Hxd : x = d)
               by (apply (nodup_id_unique (data st)); [exact Hwf | exact (nth_error_In _ _ Hx) | exact Hd | exact Hid]).
             subst x.
             destruct (w_peek w).
             ++ set (k := length (w_pre w)) in *.
                set (P := fun s => data s = data st /\ marks s = remove1 (marker_at d k) (marks st)).
                destruct (offer_loop_ends P MI fuel d (put_marker d k) (spend_after_peek n k)
                            (offer_list (wtrs st) (d_path d)) (take_marker d k st) st' o)
                  as [[Ho Hq] | [Ho Hp]].
                ** intros s i [Hds Hms]. split; assumption.
                ** intros s [Hds Hms].
                   pose proof (proj1 (proj2 (proj2 marker_effects_keep_mi)) d k st Hmi Hd Hk) as Hput.
                   unfold MI, put_marker, take_marker in *. cbn [data marks] in *. rewrite Hds, Hms. exact Hput.
                ** intros s [Hds Hms].
                   pose proof (proj1 (proj2 (proj2 (proj2 marker_effects_keep_mi))) n k d st Hmi Hx Hk) as Hsp2.
                   unfold MI, spend_after_peek, take_marker in *. cbn [data marks] in *.
                   rewrite Hds, Hx. cbn [data marks]. rewrite Hms. rewrite Hx in Hsp2. exact Hsp2.
                ** split; reflexivity.
                ** exact H.
                ** left. split; assumption.
                ** right. split; [exact Ho |]. right. exists d, k. split; [exact Hk |].
                   destruct Hp as [Hds Hms].
                   pose proof (proj1 (proj2 (proj2 marker_effects_keep_mi)) d k st Hmi Hd Hk) as Hput.
                   unfold MI, put_marker, take_marker in *. cbn [data marks] in *. rewrite Hds, Hms. exact Hput.
             ++ assert (Hmi1 : MI (deliver_remove n (length (w_pre w)) st))
                  by exact (proj1 (proj2 marker_effects_keep_mi) n _ d st Hmi Hx Hk).
                destruct (w_pers w).
                ** exact (IH w _ st' o Hmi1 (deliver_remove_wf n _ st Hwf) H).
                ** injection H as <- <-. left. split; [reflexivity | exact Hmi1].
        * injection H as <- <-. left. split; [reflexivity | exact Hmi].
      + injection H as <- <-. left. split; [reflexivity | exact Hmi].
  Qed.

  (* HEADLINE 14 (marker_invariant_preserved): every staging and every plan
     of the engine keeps the marker invariant (plan, "Markers exist exactly
     for live datums"). The effects are these. A store adds one marker for each
     prefix. A consume-side delivery that removes its datum removes the
     delivered marker by its COMM and the record and every other marker by
     the retire plan. A peek COMM removes its marker, and the second send
     restores it or spends the datum with the retire plan. The plans are
     these. A send keeps the invariant in every outcome, because a datum in
     flight has no marker. A receive keeps it when it completes. A receive
     that re-issues keeps it, or leaves exactly the marker of a peek COMM
     pending for its second send. *)
  Theorem marker_invariant_preserved :
    (forall ds, Permutation (dorder ds) ds) ->
    (forall d st, MI st -> MI (store_datum d st)) /\
    (forall n k d st, MI st -> nth_error (data st) n = Some d -> k <= length (d_path d) ->
       MI (deliver_remove n k st)) /\
    (forall d k st, MI st -> In d (data st) -> k <= length (d_path d) ->
       MI (put_marker d k (take_marker d k st))) /\
    (forall n k d st, MI st -> nth_error (data st) n = Some d -> k <= length (d_path d) ->
       MI (spend_after_peek n k (take_marker d k st))) /\
    (forall fuel d st st' o, MI st ->
       offer_loop fuel d (store_datum d) (fun s => s) (offer_list (wtrs st) (d_path d)) st = (st', o) ->
       MI st') /\
    (forall fuel w st st', MI st -> wf_data st -> receive_loop fuel w st = (st', Done) -> MI st') /\
    (forall fuel w st st', MI st -> wf_data st -> receive_loop fuel w st = (st', Reissue) ->
       MI st' \/ exists d k, k <= length (d_path d) /\ MI (put_marker d k st')).
  Proof.
    intros Hdp.
    destruct marker_effects_keep_mi as [Hstore [Hdel [Hput [Hspend _]]]].
    split; [exact Hstore |]. split; [exact Hdel |]. split; [exact Hput |]. split; [exact Hspend |].
    split; [| split].
    - intros fuel d st st' o Hmi H.
      destruct (offer_loop_ends MI MI fuel d (store_datum d) (fun s => s) _ st st' o mi_drop
                  (fun s Hs => Hstore d s Hs) (fun s Hs => Hs) Hmi H) as [[_ Hq] | [_ Hp]];
        assumption.
    - intros fuel w st st' Hmi Hwf H.
      destruct (receive_loop_mi Hdp fuel w st st' Done Hmi Hwf H) as [[_ Hq] | [Ho _]];
        [exact Hq | discriminate].
    - intros fuel w st st' Hmi Hwf H.
      destruct (receive_loop_mi Hdp fuel w st st' Reissue Hmi Hwf H) as [[Ho _] | [_ Hr]];
        [discriminate | exact Hr].
  Qed.

  (* ── 15. Head shapes on prefixes: dirty prefixes and the cascade ───── *)

  (* The head order of a head-only discipline (decision 11): older a b holds
     when the entry with the order a interacts before the entry with the
     order b (a < b for a queue, a > b for a stack). The theorems hold for every such
     relation. *)
  Variable older : nat -> nat -> bool.

  (* The head of a side: the oldest entry, the first one on a tie. *)
  Fixpoint oldest {A : Type} (ord : A -> nat) (xs : list A) : option A :=
    match xs with
    | [] => None
    | x :: rest =>
        match oldest ord rest with
        | Some y => if older (ord y) (ord x) then Some y else Some x
        | None => Some x
        end
    end.

  (* The records of the path p: D(p). *)
  Definition data_at (st : state) (p : path) : list datum :=
    filter (fun d => path_eqb (d_path d) p) (data st).

  Definition dhead (st : state) (p : path) : option datum := oldest d_ord (data_at st p).
  Definition whead (st : state) (q : path) : option waiter := oldest w_ord (waiters_on (wtrs st) q).

  Definition is_dhead (st : state) (d : datum) : bool :=
    match dhead st (d_path d) with
    | Some h => Nat.eqb (d_id h) (d_id d)
    | None => false
    end.

  (* The window of the prefix q for one bind: the head of each path p ⪰ q, in
     the candidate order (CONSUME-CANDIDATES with the head order inside a
     path, decision 11 for each path). *)
  Definition window (st : state) (q : path) : list datum :=
    dorder (filter (fun d => prefixb q (d_path d) && is_dhead st d) (data st)).

  (* FIREABLE: the head waiter of a prefix takes the first head of its window
     that it matches spatially, and its guard admits it (decision 37: the
     guard once, on the greedy choice). *)
  Definition fireable (st : state) (w : waiter) : bool :=
    match first_spatial w (window st (w_pre w)) with
    | Some (d, _) => guard_ok w d
    | None => false
    end.

  (* INV_head: no head waiter of any prefix is fireable. *)
  Definition inv_head (st : state) : Prop :=
    forall q w, whead st q = Some w -> fireable st w = false.

  (* The prefixes outside dirty have no fireable head waiter. *)
  Definition clean (st : state) (dirty : list path) : Prop :=
    forall q w, ~ In q dirty -> whead st q = Some w -> fireable st w = false.

  (* A change that a plan or a cascade stages, as its effect on the view. *)
  Inductive change : Type :=
  | AddDatum (d : datum)
  | DelDatum (i : nat)
  | AddWaiter (w : waiter)
  | DelWaiter (i : nat).

  Definition del_datum (i : nat) (st : state) : state :=
    match find_pos i (data st) with
    | Some n => mkS (remove_nth n (data st)) (marks st) (wtrs st) (token st)
    | None => st
    end.

  Definition apply_change (c : change) (st : state) : state :=
    match c with
    | AddDatum d => mkS (d :: data st) (marks st) (wtrs st) (token st)
    | DelDatum i => del_datum i st
    | AddWaiter w => park w st
    | DelWaiter i => drop_waiter i st
    end.

  (* The dirty prefixes of a change: a change on D(p) changes the window of
     every q ⪯ p, and a change on I(q) changes the head waiter of q. *)
  Definition dirty_of (st : state) (c : change) : list path :=
    match c with
    | AddDatum d => prefixes (d_path d)
    | DelDatum i =>
        match find_pos i (data st) with
        | Some n => match nth_error (data st) n with Some d => prefixes (d_path d) | None => [] end
        | None => []
        end
    | AddWaiter w => [w_pre w]
    | DelWaiter i => map w_pre (filter (fun w => Nat.eqb (w_id w) i) (wtrs st))
    end.

  Fixpoint apply_all (cs : list change) (st : state) (dirty : list path) : state * list path :=
    match cs with
    | [] => (st, dirty)
    | c :: rest => apply_all rest (apply_change c st) (dirty ++ dirty_of st c)
    end.

  Lemma filter_remove_nth :
    forall {A : Type} (f : A -> bool) n (l : list A) x,
      nth_error l n = Some x -> f x = false -> filter f (remove_nth n l) = filter f l.
  Proof.
    intros A f n. induction n as [| n IH]; intros [| y l] x H Hf; cbn in H; try discriminate.
    - injection H as ->. cbn. rewrite Hf. reflexivity.
    - cbn. destruct (f y); [f_equal |]; apply (IH l x H Hf).
  Qed.

  Lemma filter_and :
    forall {A : Type} (f g : A -> bool) (l : list A),
      filter (fun x => f x && g x) l = filter g (filter f l).
  Proof.
    intros A f g l. induction l as [| x l IH]; cbn; [reflexivity |].
    destruct (f x); cbn; [destruct (g x); [f_equal |] |]; exact IH.
  Qed.

  Lemma oldest_in : forall {A : Type} (ord : A -> nat) xs x, oldest ord xs = Some x -> In x xs.
  Proof.
    intros A ord xs. induction xs as [| y rest IH]; intros x H; cbn in H; [discriminate |].
    destruct (oldest ord rest) as [z |] eqn:E.
    - destruct (older (ord z) (ord y)); injection H as <-; [right; exact (IH z eq_refl) | left; reflexivity].
    - injection H as <-. left. reflexivity.
  Qed.

  Lemma whead_pre : forall st q w, whead st q = Some w -> w_pre w = q /\ In w (wtrs st).
  Proof.
    intros st q w H. apply oldest_in in H. unfold waiters_on in H. apply filter_In in H as [Hw Hq].
    split; [apply path_eqb_eq; exact Hq | exact Hw].
  Qed.

  (* The window of q is the same in two states that hold the same datums
     under q. *)
  Lemma window_same :
    forall st st' q,
      (forall p, list_prefix q p -> data_at st' p = data_at st p) ->
      filter (fun d => prefixb q (d_path d)) (data st') = filter (fun d => prefixb q (d_path d)) (data st) ->
      window st' q = window st q.
  Proof.
    intros st st' q Hat Hf. unfold window. f_equal. rewrite !filter_and, Hf.
    apply filter_ext_in. intros d Hd. apply filter_In in Hd as [_ Hp].
    apply prefixb_spec in Hp. unfold is_dhead, dhead. rewrite (Hat (d_path d) Hp). reflexivity.
  Qed.

  (* A fireable waiter stays fireable, and a waiter that is not fireable
     stays so, when the window of its prefix does not change. *)
  Lemma fireable_same :
    forall st st' w, window st' (w_pre w) = window st (w_pre w) -> fireable st' w = fireable st w.
  Proof. intros st st' w H. unfold fireable. rewrite H. reflexivity. Qed.

  Lemma not_prefix_path :
    forall q p, ~ In q (prefixes p) -> forall p', list_prefix q p' -> path_eqb p p' = false.
  Proof.
    intros q p Hq p' Hp'. destruct (path_eqb p p') eqn:E; [| reflexivity].
    apply path_eqb_eq in E. subst p'. exfalso. apply Hq. apply in_prefixes. exact Hp'.
  Qed.

  Lemma not_prefix_key :
    forall q p, ~ In q (prefixes p) -> prefixb q p = false.
  Proof.
    intros q p Hq. destruct (prefixb q p) eqn:E; [| reflexivity].
    exfalso. apply Hq. apply in_prefixes. apply prefixb_spec. exact E.
  Qed.

  (* DIRTY COVERAGE: a change can make a head waiter fireable only on a
     prefix of its dirty set. A change on D(p) touches the windows of the
     prefixes of p only, and a change on I(q) touches the head waiter of q
     only. *)
  Lemma clean_change :
    forall c st dirty, clean st dirty -> clean (apply_change c st) (dirty ++ dirty_of st c).
  Proof.
    intros c st dirty Hc q w Hq Hw.
    assert (Hq1 : ~ In q dirty) by (intros H; apply Hq; apply in_or_app; left; exact H).
    assert (Hq2 : ~ In q (dirty_of st c)) by (intros H; apply Hq; apply in_or_app; right; exact H).
    destruct c as [d0 | i | w0 | i]; cbn in Hw, Hq2 |- *.
    - (* AddDatum *)
      assert (Hw0 : whead st q = Some w) by exact Hw.
      destruct (whead_pre st q w Hw0) as [Hpre _].
      rewrite (fireable_same st _ w); [exact (Hc q w Hq1 Hw0) |].
      rewrite Hpre. apply window_same.
      + intros p Hp. unfold data_at. cbn. rewrite (not_prefix_path q (d_path d0) Hq2 p Hp). reflexivity.
      + cbn. rewrite (not_prefix_key q (d_path d0) Hq2). reflexivity.
    - (* DelDatum *)
      unfold del_datum in *. destruct (find_pos i (data st)) as [n |] eqn:En.
      + destruct (nth_error (data st) n) as [d0 |] eqn:Ed.
        * assert (Hw0 : whead st q = Some w) by exact Hw.
          destruct (whead_pre st q w Hw0) as [Hpre _].
          rewrite (fireable_same st _ w); [exact (Hc q w Hq1 Hw0) |].
          rewrite Hpre. apply window_same.
          -- intros p Hp. unfold data_at. cbn.
             apply (filter_remove_nth _ n (data st) d0 Ed). apply (not_prefix_path q (d_path d0) Hq2 p Hp).
          -- cbn. apply (filter_remove_nth _ n (data st) d0 Ed). apply not_prefix_key. exact Hq2.
        * destruct (find_pos_spec _ _ _ En) as [x [Hx _]]. rewrite Hx in Ed. discriminate.
      + exact (Hc q w Hq1 Hw).
    - (* AddWaiter *)
      unfold park, whead, waiters_on in Hw. cbn in Hw.
      assert (Hne : path_eqb (w_pre w0) q = false).
      { destruct (path_eqb (w_pre w0) q) eqn:E; [| reflexivity].
        apply path_eqb_eq in E. subst q. exfalso. apply Hq2. left. reflexivity. }
      rewrite Hne in Hw. unfold park, fireable, window. cbn.
      exact (Hc q w Hq1 Hw).
    - (* DelWaiter *)
      unfold drop_waiter, whead, waiters_on in Hw. cbn in Hw.
      assert (Hsame : filter (fun x => path_eqb (w_pre x) q) (filter (fun x => negb (Nat.eqb (w_id x) i)) (wtrs st))
                      = filter (fun x => path_eqb (w_pre x) q) (wtrs st)).
      { clear Hw. induction (wtrs st) as [| x rest IH]; cbn in *; [reflexivity |].
        destruct (Nat.eqb (w_id x) i) eqn:Ei; cbn.
        - destruct (path_eqb (w_pre x) q) eqn:Ep.
          + exfalso. apply Hq2. left. apply path_eqb_eq in Ep. exact Ep.
          + apply IH. intros H. apply Hq2. right. exact H.
        - destruct (path_eqb (w_pre x) q); [f_equal |]; apply IH; exact Hq2. }
      rewrite Hsame in Hw. unfold drop_waiter, fireable, window. cbn.
      exact (Hc q w Hq1 Hw).
  Qed.

  Lemma clean_apply_all :
    forall cs st dirty st' dirty',
      clean st dirty -> apply_all cs st dirty = (st', dirty') -> clean st' dirty'.
  Proof.
    induction cs as [| c cs IH]; intros st dirty st' dirty' Hc H; cbn in H.
    - injection H as <- <-. exact Hc.
    - apply (IH (apply_change c st) (dirty ++ dirty_of st c) st' dirty'); [apply clean_change; exact Hc | exact H].
  Qed.

  (* NEXT-FIRE: the first dirty prefix whose head waiter is fireable, with
     the waiter and its greedy choice. *)
  Fixpoint next_fire (st : state) (qs : list path) : option (waiter * datum) :=
    match qs with
    | [] => None
    | q :: rest =>
        match whead st q with
        | Some w =>
            match first_spatial w (window st (w_pre w)) with
            | Some (d, _) => if guard_ok w d then Some (w, d) else next_fire st rest
            | None => next_fire st rest
            end
        | None => next_fire st rest
        end
    end.

  Lemma next_fire_none :
    forall st qs, next_fire st qs = None -> forall q w, In q qs -> whead st q = Some w -> fireable st w = false.
  Proof.
    intros st qs. induction qs as [| q0 rest IH]; intros H q w Hq Hw; [destruct Hq |]. cbn in H.
    destruct Hq as [<- | Hq].
    - rewrite Hw in H. unfold fireable.
      destruct (first_spatial w (window st (w_pre w))) as [[d r] |].
      + destruct (guard_ok w d); [discriminate | reflexivity].
      + reflexivity.
    - destruct (whead st q0) as [w0 |].
      + destruct (first_spatial w0 (window st (w_pre w0))) as [[d r] |].
        * destruct (guard_ok w0 d); [discriminate | exact (IH H q w Hq Hw)].
        * exact (IH H q w Hq Hw).
      + exact (IH H q w Hq Hw).
  Qed.

  Lemma next_fire_some :
    forall st qs w d, next_fire st qs = Some (w, d) ->
      In w (wtrs st) /\ In d (window st (w_pre w)) /\ guard_ok w d = true /\ spatial_ok w d = true.
  Proof.
    intros st qs. induction qs as [| q rest IH]; intros w d H; cbn in H; [discriminate |].
    destruct (whead st q) as [w0 |] eqn:Ew.
    - destruct (first_spatial w0 (window st (w_pre w0))) as [[d0 r] |] eqn:Es.
      + destruct (guard_ok w0 d0) eqn:Eg.
        * injection H as <- <-. destruct (whead_pre st q w0 Ew) as [_ Hin].
          destruct (first_spatial_in _ _ _ _ Es) as [Hd Hs].
          repeat split; assumption.
        * exact (IH w d H).
      + exact (IH w d H).
    - exact (IH w d H).
  Qed.

  (* The changes of one fire of the cascade, as one direct COMM
     (lift_reoffer_equals_direct_comm of JoinCascade.v for one bind): the
     waiter leaves when it is not persistent. The datum leaves when it is
     not persistent and the waiter does not peek. A peek of a datum that is
     not persistent removes it and sends it again (Rule E), which the next
     round of the cascade offers. *)
  Definition fire_changes (w : waiter) (d : datum) : list change :=
    (if w_pers w then [] else [DelWaiter (w_id w)])
    ++ (if d_pers d then []
        else if w_peek w then [DelDatum (d_id d); AddDatum d]
        else [DelDatum (d_id d)]).

  Inductive cascade_end : Type := Quiet | Divergent | OutOfFuel.

  (* The cascade over the dirty prefixes (DRAIN-AND-CASCADE for prefixes):
     it fires the first fireable head waiter of a dirty prefix, adds the
     dirty prefixes of the fire, and stops when no dirty prefix has a
     fireable head waiter, or at a divergent pair (A10). *)
  Fixpoint cascade (fuel : nat) (st : state) (dirty : list path) : state * cascade_end :=
    match fuel with
    | 0 => (st, OutOfFuel)
    | S f =>
        match next_fire st dirty with
        | None => (st, Quiet)
        | Some (w, d) =>
            if w_pers w && d_pers d then (st, Divergent)
            else let (st', dirty') := apply_all (fire_changes w d) st dirty in cascade f st' dirty'
        end
    end.

  Lemma cascade_quiet :
    forall fuel st dirty st', clean st dirty -> cascade fuel st dirty = (st', Quiet) -> inv_head st'.
  Proof.
    induction fuel as [| fuel IH]; intros st dirty st' Hc H; cbn in H; [discriminate |].
    destruct (next_fire st dirty) as [[w d] |] eqn:En.
    - destruct (w_pers w && d_pers d); [discriminate |].
      destruct (apply_all (fire_changes w d) st dirty) as [st1 dirty1] eqn:Ea.
      exact (IH st1 dirty1 st' (clean_apply_all _ st dirty st1 dirty1 Hc Ea) H).
    - injection H as <-. intros q w Hw.
      destruct (In_dec path_eq_dec q dirty) as [Hin | Hout].
      + exact (next_fire_none st dirty En q w Hin Hw).
      + exact (Hc q w Hout Hw).
  Qed.

  (* ── 15.1 Termination of the cascade ── *)

  (* The measure of the cascade: the waiters and the datums of the view. *)
  Definition measure (st : state) : nat := length (wtrs st) + length (data st).

  Lemma filter_length_lt :
    forall {A : Type} (f : A -> bool) (l : list A) x, In x l -> f x = false -> length (filter f l) < length l.
  Proof.
    intros A f l. induction l as [| y l IH]; intros x Hx Hf; [destruct Hx |]. cbn.
    destruct Hx as [<- | Hx].
    - rewrite Hf. pose proof (filter_length_le f l) as Hle. cbn in *. lia.
    - destruct (f y); cbn; [specialize (IH x Hx Hf); lia |].
      pose proof (filter_length_le f l) as Hle. lia.
  Qed.

  Lemma find_pos_in : forall ds d, In d ds -> exists n, find_pos (d_id d) ds = Some n.
  Proof.
    induction ds as [| x ds IH]; intros d Hd; [destruct Hd |]. cbn.
    destruct (Nat.eqb (d_id x) (d_id d)) eqn:E; [exists 0; reflexivity |].
    destruct Hd as [<- | Hd]; [rewrite Nat.eqb_refl in E; discriminate |].
    destruct (IH d Hd) as [n Hn]. rewrite Hn. exists (S n). reflexivity.
  Qed.

  Lemma remove_nth_length :
    forall {A : Type} n (l : list A) x, nth_error l n = Some x -> length (remove_nth n l) = pred (length l).
  Proof.
    intros A n. induction n as [| n IH]; intros [| y l] x H; cbn in H; try discriminate; cbn.
    - reflexivity.
    - rewrite (IH l x H). destruct l; [destruct n; discriminate | reflexivity].
  Qed.

  Lemma del_datum_length :
    forall d st, In d (data st) -> length (data (del_datum (d_id d) st)) = pred (length (data st)).
  Proof.
    intros d st Hd. unfold del_datum. destruct (find_pos_in (data st) d Hd) as [n Hn]. rewrite Hn.
    destruct (find_pos_spec _ _ _ Hn) as [x [Hx _]]. cbn. exact (remove_nth_length n (data st) x Hx).
  Qed.

  Lemma drop_waiter_length :
    forall w st, In w (wtrs st) -> length (wtrs (drop_waiter (w_id w) st)) < length (wtrs st).
  Proof.
    intros w st Hw. unfold drop_waiter. cbn.
    apply (filter_length_lt _ (wtrs st) w Hw). cbn. rewrite Nat.eqb_refl. reflexivity.
  Qed.

  Lemma window_incl :
    (forall ds, Permutation (dorder ds) ds) -> forall st q d, In d (window st q) -> In d (data st).
  Proof.
    intros Hdp st q d H. unfold window in H. apply (Permutation_in _ (Hdp _)) in H.
    apply filter_In in H. apply H.
  Qed.

  Lemma del_datum_wtrs : forall i st, wtrs (del_datum i st) = wtrs st.
  Proof. intros i st. unfold del_datum. destruct (find_pos i (data st)); reflexivity. Qed.

  Lemma measure_drop : forall w st, In w (wtrs st) -> measure (drop_waiter (w_id w) st) < measure st.
  Proof.
    intros w st Hw. unfold measure. pose proof (drop_waiter_length w st Hw) as H.
    replace (data (drop_waiter (w_id w) st)) with (data st) by reflexivity. lia.
  Qed.

  Lemma measure_del : forall d st, In d (data st) -> S (measure (del_datum (d_id d) st)) = measure st.
  Proof.
    intros d st Hd. unfold measure. rewrite del_datum_wtrs, (del_datum_length d st Hd).
    destruct (data st) as [| y ys]; [destruct Hd | cbn; lia].
  Qed.

  (* One fire of the cascade lowers the measure by at least one, when the
     waiter is well formed (no receive peeks and persists). The fire only
     removes waiters. *)
  Lemma fire_lowers_measure :
    forall w d st dirty st' dirty',
      In w (wtrs st) -> In d (data st) -> negb (w_pers w && w_peek w) = true ->
      w_pers w && d_pers d = false ->
      apply_all (fire_changes w d) st dirty = (st', dirty') -> measure st' < measure st /\
      incl (wtrs st') (wtrs st).
  Proof.
    intros w d st dirty st' dirty' Hw Hd Hwf Hdiv H. unfold fire_changes in H.
    destruct (w_pers w) eqn:Ep, (d_pers d) eqn:Edp, (w_peek w) eqn:Ek; cbn in Hdiv, Hwf, H;
      try discriminate; injection H as <- _.
    - (* a persistent waiter and a datum that leaves *)
      cbn [apply_change]. split.
      + pose proof (measure_del d st Hd). lia.
      + rewrite del_datum_wtrs. intros x Hx. exact Hx.
    - (* a waiter that leaves and a persistent datum, a peek *)
      cbn [apply_change]. split; [apply measure_drop; exact Hw | apply drop_waiter_incl].
    - (* a waiter that leaves and a persistent datum *)
      cbn [apply_change]. split; [apply measure_drop; exact Hw | apply drop_waiter_incl].
    - (* a waiter that leaves, and a peeked datum that leaves and is sent again (Rule E) *)
      cbn [apply_change]. split.
      + pose proof (measure_drop w st Hw) as H1.
        pose proof (measure_del d (drop_waiter (w_id w) st) Hd) as H2.
        unfold measure in *. cbn [wtrs data length]. lia.
      + cbn [wtrs]. rewrite del_datum_wtrs. apply drop_waiter_incl.
    - (* a waiter that leaves and a datum that leaves *)
      cbn [apply_change]. split.
      + pose proof (measure_drop w st Hw) as H1.
        pose proof (measure_del d (drop_waiter (w_id w) st) Hd) as H2. lia.
      + rewrite del_datum_wtrs. apply drop_waiter_incl.
  Qed.

  (* The cascade ends within the measure: it never runs out of a fuel of one
     more than the measure, so it ends quiet or at a divergent pair. *)
  Lemma cascade_terminates :
    (forall ds, Permutation (dorder ds) ds) ->
    forall fuel st dirty, measure st < fuel ->
      (forall w, In w (wtrs st) -> negb (w_pers w && w_peek w) = true) ->
      snd (cascade fuel st dirty) <> OutOfFuel.
  Proof.
    intros Hdp. induction fuel as [| fuel IH]; intros st dirty Hm Hwf; [lia |]. cbn.
    destruct (next_fire st dirty) as [[w d] |] eqn:En; [| discriminate].
    destruct (next_fire_some st dirty w d En) as [Hw [Hdw _]].
    pose proof (window_incl Hdp st (w_pre w) d Hdw) as Hd.
    destruct (w_pers w && d_pers d) eqn:Ediv; [discriminate |].
    destruct (apply_all (fire_changes w d) st dirty) as [st1 dirty1] eqn:Ea.
    destruct (fire_lowers_measure w d st dirty st1 dirty1 Hw Hd (Hwf w Hw) Ediv Ea) as [Hlt Hincl].
    apply IH; [lia |]. intros x Hx. apply Hwf. apply Hincl. exact Hx.
  Qed.

  Lemma inv_head_clean : forall st, inv_head st <-> clean st [].
  Proof.
    intros st. split.
    - intros H q w _ Hw. exact (H q w Hw).
    - intros H q w Hw. exact (H q w (fun x => x) Hw).
  Qed.

  (* HEADLINE 15 (heads_no_match): head-only disciplines on a PathMap space
     keep INV_head, no head waiter of any prefix is fireable, after every
     operation that ends quiet. A change on D(p) can make a head waiter
     fireable only on a prefix of p, and a change on I(q) only on q (DIRTY
     COVERAGE). So a plan that stages any changes and then runs the cascade
     over their dirty prefixes ends with no fireable head waiter. The
     cascade lowers the measure with each fire, so it ends quiet or at a
     divergent pair (A10) within the measure. *)
  Theorem heads_no_match :
    (forall ds, Permutation (dorder ds) ds) ->
    (forall c st dirty, clean st dirty -> clean (apply_change c st) (dirty ++ dirty_of st c)) /\
    (forall st, inv_head st <-> clean st []) /\
    (forall fuel st dirty st', clean st dirty -> cascade fuel st dirty = (st', Quiet) -> inv_head st') /\
    (forall cs fuel st st1 dirty1 st', inv_head st -> apply_all cs st [] = (st1, dirty1) ->
       cascade fuel st1 dirty1 = (st', Quiet) -> inv_head st') /\
    (forall fuel st dirty, measure st < fuel ->
       (forall w, In w (wtrs st) -> negb (w_pers w && w_peek w) = true) ->
       snd (cascade fuel st dirty) <> OutOfFuel).
  Proof.
    intros Hdp. split; [exact clean_change |]. split; [exact inv_head_clean |].
    split; [exact cascade_quiet |]. split; [| exact (cascade_terminates Hdp)].
    intros cs fuel st st1 dirty1 st' Hinv Ha Hc.
    apply (cascade_quiet fuel st1 dirty1 st'); [| exact Hc].
    apply (clean_apply_all cs st [] st1 dirty1); [apply inv_head_clean; exact Hinv | exact Ha].
  Qed.

End Engine.

(* ── 16. The token and the re-entry of decision 43 ────────────────────── *)

(* A bump of the token S(sid): it returns the value of the token, which is
   the order of an operation of a head-only discipline, and stores the next
   value. One token orders every path and every prefix of a PathMap space. *)
Definition bump (t : nat) : nat * nat := (t, S t).

(* operation_ord (generic.rs): a re-issued persistent operation keeps the
   order of its first round, which is an earlier value of the token. A
   larger value is Inconsistent (None). A fresh operation takes its bump. *)
Definition operation_ord (reissued : option nat) (bumped : nat) : option nat :=
  match reissued with
  | None => Some bumped
  | Some o => if o <? bumped then Some o else None
  end.

(* operation_ord without a counter (bag and set): every order is 0, and a
   re-issued order other than 0 is Inconsistent (None). *)
Definition operation_ord_scan (reissued : option nat) : option nat :=
  match reissued with
  | None => Some 0
  | Some 0 => Some 0
  | Some (S _) => None
  end.

(* The orders of the stored entries of a space: pairwise distinct and below
   the token. *)
Definition ords_ok (t : nat) (os : list nat) : Prop := NoDup os /\ forall o, In o os -> o < t.

(* The head of a side of a queue: the least order (head_metered). *)
Fixpoint least (os : list nat) : option nat :=
  match os with
  | [] => None
  | o :: rest => match least rest with Some m => Some (Nat.min o m) | None => Some o end
  end.

(* at_head of a re-entered element with the first-round order o on a side of
   a queue (generic.rs, Cx::at_head with QueueDiscipline::precedes). *)
Definition at_head_queue (o : nat) (os : list nat) : bool :=
  match least os with None => true | Some m => o <? m end.

Lemma least_spec :
  forall os m, least os = Some m -> In m os /\ forall h, In h os -> m <= h.
Proof.
  induction os as [| o rest IH]; intros m H; cbn in H; [discriminate |].
  destruct (least rest) as [m' |] eqn:E.
  - injection H as <-. destruct (IH m' eq_refl) as [Hin Hle]. split.
    + destruct (Nat.min_spec o m') as [[_ ->] | [_ ->]]; [left; reflexivity | right; exact Hin].
    + intros h [<- | Hh]; [lia |]. specialize (Hle h Hh). lia.
  - injection H as <-. split; [left; reflexivity |]. intros h [<- | Hh]; [lia |].
    destruct rest as [| x rest']; [destruct Hh |]. cbn in E.
    destruct (least rest'); discriminate.
Qed.

Lemma least_none : forall os, least os = None -> os = [].
Proof. intros [| o rest] H; [reflexivity | cbn in H; destruct (least rest); discriminate]. Qed.

(* HEADLINE 16 (token_ord_reentry): the token gives the orders of decision
   43 on a PathMap space. A bump returns the token and stores the next
   value, so the orders of the stored entries of the whole space stay
   pairwise distinct and below the token, whether an operation is fresh or a
   re-issued persistent operation that keeps its first-round order.
   operation_ord keeps a first-round order below the bump and refuses any
   other. A re-entered element meets no equal order on any side of the
   space, and on a queue side it re-enters at the head exactly when it is
   older than every stored entry. A bag or a set keeps the order 0 in every
   round (operation_ord_scan), so a re-issued element keeps its record and
   its identity. *)
Theorem token_ord_reentry :
  (forall t, bump t = (t, S t)) /\
  (forall o b, o < b -> operation_ord (Some o) b = Some o) /\
  (forall o b, b <= o -> operation_ord (Some o) b = None) /\
  (forall b, operation_ord None b = Some b) /\
  (forall t os, ords_ok t os -> ords_ok (S t) (t :: os)) /\
  (forall t os o, ords_ok t os -> o < t -> ~ In o os -> ords_ok (S t) (o :: os)) /\
  (forall t os o h, ords_ok t os -> ~ In o os -> In h os -> o <> h) /\
  (forall o os, ~ In o os -> at_head_queue o os = true <-> forall h, In h os -> o < h) /\
  (forall r, operation_ord_scan r = Some 0 <-> r = None \/ r = Some 0).
Proof.
  split; [reflexivity |]. split; [| split; [| split; [| split; [| split; [| split; [| split]]]]]].
  - intros o b H. unfold operation_ord. rewrite (proj2 (Nat.ltb_lt o b) H). reflexivity.
  - intros o b H. unfold operation_ord.
    destruct (o <? b) eqn:E; [apply Nat.ltb_lt in E; lia | reflexivity].
  - reflexivity.
  - intros t os [Hnd Hlt]. split.
    + constructor; [intros Hin; specialize (Hlt t Hin); lia | exact Hnd].
    + intros o [<- | Ho]; [lia | specialize (Hlt o Ho); lia].
  - intros t os o [Hnd Hlt] Ho Hnin. split.
    + constructor; [exact Hnin | exact Hnd].
    + intros x [<- | Hx]; [lia | specialize (Hlt x Hx); lia].
  - intros t os o h _ Hnin Hh Heq. subst h. contradiction.
  - intros o os Hnin. unfold at_head_queue. destruct (least os) as [m |] eqn:E.
    + destruct (least_spec os m E) as [Hm Hle]. split.
      * intros Hlt h Hh. apply Nat.ltb_lt in Hlt. specialize (Hle h Hh). lia.
      * intros Hall. apply Nat.ltb_lt. exact (Hall m Hm).
    + apply least_none in E. subst os. split; [intros _ h [] | reflexivity].
  - intros [[| o] |]; cbn; split; intros H; try discriminate; try reflexivity.
    + right. reflexivity.
    + destruct H as [H | H]; discriminate.
    + left. reflexivity.
Qed.

(* ── 17. Metering of the prefix scan (decision 42) ────────────────────── *)

(* The metering rule of DR-88, as AgentReadCoverage.v and DataCollection.v
   state it: a trace of reservations and reads is covered when every prefix
   reads at most what it reserved. *)
Inductive tev := TReserve (units : nat) | TRead (units : nat).

Definition reserved_of (e : tev) : nat := match e with TReserve u => u | TRead _ => 0 end.
Definition read_of (e : tev) : nat := match e with TRead u => u | TReserve _ => 0 end.

Definition sum_of (project : tev -> nat) (trace : list tev) : nat :=
  fold_right Nat.add 0 (map project trace).

Definition covered (trace : list tev) : Prop :=
  forall pre post, trace = pre ++ post -> sum_of read_of pre <= sum_of reserved_of pre.

Lemma sum_of_app : forall f a b, sum_of f (a ++ b) = sum_of f a + sum_of f b.
Proof.
  intros f a b. unfold sum_of. rewrite map_app, fold_right_app.
  induction a as [| x a IH]; cbn; [reflexivity |]. unfold sum_of in IH. rewrite IH. lia.
Qed.

Lemma covered_app :
  forall a b, covered a -> sum_of read_of a <= sum_of reserved_of a -> covered b -> covered (a ++ b).
Proof.
  intros a b Ha Hbal Hb pre post Hsplit.
  apply app_eq_app in Hsplit as [mid [[Hpre Hpost] | [Hpre Hb']]].
  - exact (Ha pre mid Hpre).
  - subst pre. pose proof (Hb mid post Hb') as Hm. rewrite !sum_of_app. lia.
Qed.

(* A metered step: reserve the units, then read them. *)
Definition mstep (units : nat) : list tev := [TReserve units; TRead units].

Lemma mstep_covered : forall u, covered (mstep u) /\ sum_of read_of (mstep u) <= sum_of reserved_of (mstep u).
Proof.
  intros u. split; [| cbn; lia].
  intros pre post H. unfold mstep in H.
  destruct pre as [| a [| b [| c rest]]]; cbn in H.
  - cbn. lia.
  - injection H as <- _. cbn. lia.
  - injection H as <- <- _. cbn. lia.
  - injection H as <- <- Hn. destruct rest; discriminate.
Qed.

Lemma msteps_covered :
  forall us, covered (flat_map mstep us) /\
             sum_of read_of (flat_map mstep us) <= sum_of reserved_of (flat_map mstep us).
Proof.
  induction us as [| u rest [IHc IHb]]; cbn [flat_map].
  - split; [intros pre post H; destruct pre; [cbn; lia | discriminate] | cbn; lia].
  - destruct (mstep_covered u) as [Hc Hb]. split.
    + apply covered_app; assumption.
    + rewrite !sum_of_app. lia.
Qed.

(* The parts of the scan of a receive on a prefix, in their units
   (consume_candidates and the plan around it): the decode and the path-key
   encode of each marker of I(q), each key comparison of the sort and of
   the grouping, for each path the joined key, each comparison of the
   pairing of records and markers, the order of the discipline and the
   suffix copy, for each candidate the payload decode, the re-rooting and
   the metered match, the guard, the copies of the staged operations, and
   the outcome that the plan prepays before the commit. *)
Record prefix_scan := {
  ps_markers : list (nat * nat);
  ps_compares : list nat;
  ps_paths : list (list nat);
  ps_candidates : list (nat * nat * nat);
  ps_guard : nat;
  ps_staging : list nat;
  ps_outcome : nat
}.

Definition prefix_scan_trace (s : prefix_scan) : list tev :=
  flat_map (fun dk => mstep (fst dk) ++ mstep (snd dk)) (ps_markers s)
  ++ flat_map mstep (ps_compares s)
  ++ flat_map (flat_map mstep) (ps_paths s)
  ++ flat_map (fun c => mstep (fst (fst c)) ++ mstep (snd (fst c)) ++ mstep (snd c)) (ps_candidates s)
  ++ mstep (ps_guard s)
  ++ flat_map mstep (ps_staging s)
  ++ [TReserve (ps_outcome s)] ++ [TRead (ps_outcome s)].

(* A covered trace that reads at most what it reserves. *)
Definition balanced (trace : list tev) : Prop :=
  covered trace /\ sum_of read_of trace <= sum_of reserved_of trace.

Lemma balanced_app : forall a b, balanced a -> balanced b -> balanced (a ++ b).
Proof.
  intros a b [Ha Ba] [Hb Bb]. split; [apply covered_app; assumption |]. rewrite !sum_of_app. lia.
Qed.

Lemma balanced_flat_map :
  forall {A : Type} (f : A -> list tev) xs, (forall x, balanced (f x)) -> balanced (flat_map f xs).
Proof.
  intros A f xs H. induction xs as [| x xs IH]; cbn [flat_map].
  - split; [intros pre post Hs; destruct pre; [cbn; lia | discriminate] | cbn; lia].
  - apply balanced_app; [apply H | exact IH].
Qed.

Lemma balanced_mstep : forall u, balanced (mstep u).
Proof. intros u. exact (mstep_covered u). Qed.

(* HEADLINE 17 (prefix_scan_trace_covered): every prefix of the trace of the
   scan of a receive on a prefix, the reads after the commit included,
   reads at most what it reserved (decision 42). So a host-work budget that
   is smaller than the reservations of the scan is exceeded by a
   reservation before the commit, and the operation has no effect. *)
Theorem prefix_scan_trace_covered : forall s, covered (prefix_scan_trace s).
Proof.
  intros s. unfold prefix_scan_trace.
  change ([TReserve (ps_outcome s)] ++ [TRead (ps_outcome s)]) with (mstep (ps_outcome s)).
  assert (H : balanced
    (flat_map (fun dk => mstep (fst dk) ++ mstep (snd dk)) (ps_markers s)
     ++ flat_map mstep (ps_compares s)
     ++ flat_map (flat_map mstep) (ps_paths s)
     ++ flat_map (fun c => mstep (fst (fst c)) ++ mstep (snd (fst c)) ++ mstep (snd c)) (ps_candidates s)
     ++ mstep (ps_guard s)
     ++ flat_map mstep (ps_staging s)
     ++ mstep (ps_outcome s))).
  { repeat apply balanced_app; try apply balanced_mstep;
      repeat (apply balanced_flat_map; intros); repeat apply balanced_app; apply balanced_mstep. }
  exact (proj1 H).
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Addendum of Phase 2 of wave W3: the prefix source of the join engine

   Phase 2 connects the engine model of sections 12 to 15 to the join engine
   of spaces/join.rs through the prefix source of
   rholang/src/rust/interpreter/spaces/pathmap/join.rs. The text sits here,
   so the line citations of the headlines above stay valid.

   Traceability: Rocq definition → Rust code (Phase 2)
   store_datum        land of the prefix source (STORE-MARKERS,
                      pathmap/join.rs:2784-2823) with datum_markers
                      (delivery.rs:494-508)
   deliver_remove, take_marker
                      before_fire and retire_spent (pathmap/join.rs:
                      2828-2885, 2890-2923) with retire_datum
                      (RETIRE-PLAN, pathmap/join.rs:1930-1963)
   put_marker         restore_marker (pathmap/join.rs:1839-1858) with
                      datum_marker (delivery.rs:515-534)
   spend_after_peek   spent_element (pathmap/join.rs:2039-2060)
   send_prefixes, offer_list
                      offer_order of the prefix source (pathmap/join.rs:
                      2312-2342), the most specific prefix first
   window, fireable   window and path_window of the prefix source
                      (pathmap/join.rs:2236-2296, 1275-1395): for each path
                      p under q in path-key order, the window of the
                      discipline on D(p), without the datums in flight
   bump               touch and finish of the prefix source
                      (pathmap/join.rs:2112-2128, 2741-2762)

   Abstraction (Phase 2)
   - A marker carries the cost authority of the record of its datum
     (marker_authority, markers.rs:130-132). The store of the markers and
     the restore of a marker take the authority from this one function, so
     a restored marker has the bytes and the produce identity of the stored
     one. The model keeps the pars of a record. The Rust test
     pathmap_peek_restores_marker checks the equal bytes for no authority,
     an empty authority and a region.
   - Open item O-2 (a second send at the fire cap) is closed by a choice of
     the implementation. A transaction that stops at FIRE_ROUND_CAP stores
     each pending marker again in FINISH, before its commit. So no commit
     leaves a pending marker, and MI holds in every state that another job
     can read. The settle job carries only the offer of the datum. When
     another job spent the datum first, the offer meets no waiter and stores
     nothing. The step that stores the pending marker is the restore step of
     marker_invariant_preserved.
   ═══════════════════════════════════════════════════════════════════════ *)

(* ── 18. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions prefix_key_iff_prefix.
Print Assumptions reroot_ident.
Print Assumptions reroot_compose.
Print Assumptions reroot_injective.
Print Assumptions fips_example.
Print Assumptions candidate_order_total.
Print Assumptions marker_record_roundtrip.
Print Assumptions vehicle_carries_payload.
Print Assumptions path_element_rules.
Print Assumptions delivery_vehicle_single_channel.
Print Assumptions prefix_contact_anchored.
Print Assumptions peek_restores_delivered_marker.
Print Assumptions no_match_remains.
Print Assumptions marker_invariant_preserved.
Print Assumptions heads_no_match.
Print Assumptions token_ord_reentry.
Print Assumptions prefix_scan_trace_covered.
