(* ═══════════════════════════════════════════════════════════════════════════
   Encoding.v — Marker-tuple encoding of the Reified RSpaces surface syntax

   This module models the wire encoding of the Reified RSpaces syntax (phase
   P2). Plan decision 18 encodes each Reified construct as an ETuple inside
   the existing Par fields. The head of the tuple is an unforgeable 2-byte
   marker GPrivate([0xE0, m]). Plan decision 21 corrects the encoding table.
   This module models the corrected table:

     use s { P }                     (USE, s, P)
     new x_1[: s_1], ... in P        (NEWSPACE, [a_1, ..., a_k], Par{news: [New]})
       a_i is [] or [s_i], one entry for each plain binder, at least one [s_i]
     x!(d...; p..., k=v...)          trailing data element (PARAMS, [p...], {k: v...})
       the encoder omits the element when the send has no params
     for (y <- ch ~ (q...; k=v...))  bind source (QUERY, ch, [q...], {k: v...})
       the payload and the params are never both empty
     free Th(a...)                   the value (FREE, "Th", [a...])
       Th is an identifier

   The markers are USE = [0xE0, 0xD0], NEWSPACE = [0xE0, 0xD1],
   PARAMS = [0xE0, 0xD2], QUERY = [0xE0, 0xD3] and FREE = [0xE0, 0xD4].

   Sources
   - Design: the P2 design report (second pass), sections 1, 2, 4 and 5.
     Section 5 ("-fv") lists the 12 headline theorems of the first version
     of this module. Plan revision I-20 adds the derived connective_used
     flags (section 8b, headlines 13 to 15).
   - Plan: decisions 18, 21 and 22, revision I-20, and the section
     "Frontend-neutral syntax layer" of the campaign plan.
   - Code: P2, P3b and P4 have landed. models/src/rust/reified/encoding.rs
     holds the encoder and the strict decoders, the hooks run in reduce.rs
     and reified/reducer_hooks.rs, and the normalizer builds NEWSPACE. The
     line numbers below are those of the P3b commit of this repository. The
     files that P3b did not change have the line numbers of the P4 commit.
   - Grammar: rholang-tree-sitter/grammar.js in the parser v3 worktree
     (rholang-rs-reified-rspaces). This module cites grammar rules by name,
     not by line.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability: Rocq definition → design section → Rust code
   ─────────────────────────────────────────────────────────────────────────
   bytes, bytes_compare, bytes_lt (§1.1)
                            models/src/rust/rholang/sorter/score_tree.rs:
                            69-85, 77 (ScoreAtom::compare, where line 77
                            compares GString keys with String::cmp, the
                            bytewise UTF-8 order)
   is_var_token (§2(d), §4.2)
                            grammar.js rule var, and
                            models/src/rust/reified/encoding.rs:378-386
                            (is_identifier)
   WordLists, is_identifier, is_theory_name (plan decision 27)
                            models/src/rust/reified/validation.rs:18-45,
                            50-53, 99-101, 104-106 (REJECTED_PARAM_KEYS,
                            REJECTED_THEORY_NAMES, is_param_key,
                            is_theory_name)
   par, par_eqb (§5)        models/src/lib.rs:210-225, 466-473, 662-668,
                            678-682, 707-713 (the Eq of Par, New, EList,
                            ETuple and EMap)
   marker, marker_id, marker_of_id, marker_tuple_arity (§4.1, §4.2)
                            models/src/rust/reified/encoding.rs:29, 34-45,
                            58, 61-66, 81-88, 346-356 (REIFIED_TAG, Marker,
                            Marker::id, Marker::tuple_arity,
                            Marker::from_id, marker_of)
   reserved_system_indices  plan section "Namespace (tag 0xE0)"
   map_values, canon_map, keys_strictly_sorted (§1.1)
                            models/src/rust/sorted_par_map.rs:60-74,
                            models/src/rust/par_map.rs:48-55,
                            models/src/rust/rholang/sorter/ordering.rs:58-76,
                            models/src/rust/rholang/sorter/score_tree.rs:
                            170-207, models/src/rust/rholang/sorter/
                            expr_sort_matcher.rs:433-476, 711-716
   construct, wf, wfb (§1, §4.2)
                            models/src/rust/reified/encoding.rs:110-113,
                            118-121, 125-128, 132-136, 140-143 (UseBlock,
                            NewSpace, SendParams, QuerySpec, FreeTheory)
                            and the validators of the decoders
   encode, encode_annotation (§4.1)
                            models/src/rust/reified/encoding.rs:510-512,
                            514-530, 532-544, 546-560, 562-573, 576-590
                            (use_tuple, new_space_tuple, params_tuple,
                            query_tuple, free_tuple, encode)
   build_params, send_data, build_query (§1.1-1.3)
                            models/src/rust/reified/builders.rs:58-64,
                            67-74 (send_params, query_spec)
   parse, decode (§4.2)     models/src/rust/reified/encoding.rs:692-705,
                            707-718, 720-750, 752-769, 771-789, 791-813,
                            816-827, 830-838, 841-848, 851-858 (the strict
                            decoders, each with its final NonCanonical
                            check)
   head_marker (§4.2)       models/src/rust/reified/encoding.rs:369-374
                            (sole_marker_tuple)
   position, recognize (§4.3, §4.4)
                            rholang/src/rust/interpreter/reduce.rs:1224-1250,
                            1241, 1300-1319, 1653-1672, 1655, 1656, 1658
                            (H2: the tuple arm of the term match at 1255.
                            H3: the data evaluation, then reified_produce.
                            H4: at 1681, after the substitution at 1680
                            and before single_bundle at 1683)
   split_trailing_params (§4.3 H3)
                            models/src/rust/reified/encoding.rs:830-838
   head_process_exprb, process_marker_tupleb, evaluated_process_exprb
   (§2(g), §4.3 H1)         rholang/src/rust/interpreter/util/mod.rs:56-64,
                            100, 145 (is_evaluated_process_expr in both
                            filter closures), models/src/rust/reified/
                            encoding.rs:361-366 (is_process_marker_tuple)
   evaluation_terms_with (§2(g))
                            rholang/src/rust/interpreter/util/mod.rs:66-118,
                            120-160 (evaluation_terms and
                            owned_evaluation_terms)
   evaluation_random, term_schedule (§2(g))
                            rholang/src/rust/interpreter/util/mod.rs:162-181,
                            rholang/src/rust/interpreter/reduce.rs:360-457,
                            367, 375-379, 384, 407, 430 (eval_inner)
   lexical_schedule_count_iff (§2(h), H-1)
                            rholang/src/rust/interpreter/accounting/
                            lexical.rs:44, 105-114, 125-129
   subst (Section Variable, §2(e))
                            rholang/src/rust/interpreter/substitute.rs:
                            456-537, 522, 1342-1363, 1365-1384, 1407-1430,
                            1423, 1457, 646-665, 656 (Par with the
                            unforgeables kept at 520, EList, ETuple, EMap
                            re-sorted by create_from_vec at 1421, ground
                            terms, the New body under env.shift at 654)
   fval, fconstruct, derived_flags (I-20)
                            models/src/rust/reified/encoding.rs:394-404,
                            407-416, 418-430, 436-444, 447-456, 458-460,
                            462-472, 475-482 (bits_of, expr_par, list_par,
                            map_par, new_par, annotation_par, marker_tuple,
                            tuple_par: each flag is the disjunction over
                            the children)
   refresh, sdecode (I-20)  rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:1737-1748, 1751-1759, 1844-1884,
                            1905-1937, 1940-1942, 1976-1984, 1987-1996, 2008-2017,
                            2034-2043, 2047-2064 (Field, marker_fields,
                            derived_flag, refresh_field,
                            derived_flag_of_new, refresh_tuple,
                            refreshed_tuple, refresh_tuple_par,
                            refreshed_marker_par, trailing_params)
   fsubst (I-20)            rholang/src/rust/interpreter/substitute.rs:527,
                            997-1018, 1020-1039, 1062-1085, 1342-1363,
                            1365-1384, 1407-1430 and
                            rholang/src/rust/interpreter/reduce.rs:3119-3141,
                            3143-3164, 3189-3209 (substitution and
                            evaluation keep the connective_used flags of
                            the Par, the list, the tuple and the map)
   name_decl, binder_order, normalize_new (§1.4)
                            rholang/src/rust/interpreter/compiler/
                            normalizer/processes/p_new_normalizer.rs:47-50,
                            62-65, 67-68, 93-99, 101-104 (the stable sort,
                            the uris, put_all_pos and new_count, the New,
                            the NEWSPACE tuple), rholang/src/rust/
                            interpreter/compiler/normalizer/processes/
                            reified_syntax.rs:203-236, 240-247
                            (normalize_space_annotations in the outer
                            scope), models/src/rust/reified/builders.rs:
                            28-54 (new_space keeps the plain prefix and
                            rejects an annotation on a URI binder),
                            rholang/src/rust/interpreter/compiler/
                            bound_map.rs:30-42, 63-69 (one index for each
                            binding), util/mod.rs:183-245
                            (allocate_new_bindings)
   plan_encode_newspace     the superseded plan table ("s_i or Nil")
   ─────────────────────────────────────────────────────────────────────────

   Abstraction function. The model does not contain the Rust Par type. The
   function α below maps each Rust Par to one model term.
   - α maps a Par with exactly one GPrivate, no other component and the
     connective_used flag false to PPriv. marker_of (encoding.rs:346-356)
     rejects a head with the flag true, so α maps such a Par to POther. No
     normalizer and no evaluation gives a GPrivate the flag true.
   - α maps a Par with exactly one expression to the matching constructor.
     The expression must be one of these: a GString, an EList without
     remainder, an ETuple, an EVar, an EMethod, or an EMap without remainder
     and with GString keys only. α forgets the connective_used flags of the
     expression and of its Par.
   - α maps a Par with exactly one New and no other component to PNew. The
     model keeps the injections map of the New as an opaque code.
   - α maps Par::default() to PNil.
   - α maps every other Par to POther with an injective code.
   Equality of model terms thus agrees with the Rust Par equality up to
   locally_free (models/src/lib.rs:210-225) and up to the connective_used
   flags of expressions. The canonical encoder derives those flags from the
   children, so a canonical marker tuple has one flag vector for each model
   term. Section 8b keeps the flags of the top level of a marker tuple,
   where the strict decoder compares them.

   Modeling notes
   - The model represents each Par by its canonical (sorted) form. Rust
     sorting (ParSortMatcher::sort_match) is thus the identity on model terms.
   - The design lists the constructors Priv, Str, List, Tuple, Map, Var, New
     and Other. The model adds PMethod and PNil. The HEAD filter keeps EMethod
     expressions, so PMethod is necessary for theorem 10. The superseded
     encoding uses Nil, so PNil is necessary for theorem 12.
   - I-20. The flag connective_used of a marker tuple, of its list and map
     fields, of the NEWSPACE annotations and of the holder Pars is the
     disjunction over the children. Ordinary data may use connectives
     (x!(Int)), so a canonical marker tuple can have the flag true, and the
     hooks recognize it (connective_marker_tuple_recognized). HEAD
     evaluation and substitution keep these flags when the children change,
     so they can go stale. The hooks therefore recompute the derived flags
     before the strict decode. The decoder still accepts only the exact
     canonical image of its encoder: refresh_strict_decode_erase shows that
     the strict decode of a refreshed tuple is the decode of this module,
     and that a tuple with a stale flag is accepted only after the refresh.
   - Section 8b keeps the flags only where the encoder derives them: the
     holder Par, the tuple and the container fields of marker_fields
     (reducer_hooks.rs:1751-1759). A value keeps its own flag, which no hook
     changes, so a flagged construct pairs each value with its flag. A map
     key is a GString, whose flag is false.
   - The substitution is a Section Variable. Its equations are premises of
     decode_commutes_with_subst. They hold up to the connective_used flags,
     which α forgets: HEAD keeps the flags of a substituted tuple, list or
     map. The hooks recompute the derived flags before the strict decode,
     so the premises need no flag. The theorem
     refreshed_decode_commutes_with_subst proves the flagged statement.
     The corollaries decode_commutes_with_concrete_subst and
     refreshed_decode_commutes_with_concrete_subst discharge every premise
     with a concrete substitution. This shows that the premises are
     consistent.
   - The P2 reducer never recognizes FREE. P4 factories read it. The position
     PosFactoryTheory models that read.
   - binder_order is a stable insertion sort. Rust sorts with the stable merge
     sort of slice::sort_by. A stable sort under one comparator has exactly one
     result, so the two sorts agree.
   - is_var_token follows the regular expression of design section 4.2. Plan
     decision 27 removes the words that the parser cannot produce in each
     position: is_identifier (param keys) and is_theory_name (theory names)
     are the regular expression minus the two lists of WordLists. Both lists
     come from the 32 keywords of grammar 02cef80, as measured on parser
     6548aec. The builtin theories Nat, Int, String, Bool and Any are theory
     names.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. decode_encode                        decode is a left inverse of encode
    2. encode_injective                     encode is injective
    3. decode_sound                         decode accepts only the image
    4. markers_pairwise_distinct            the five marker ids are distinct
    5. markers_disjoint_reserved            no marker equals another id
    6. named_params_canonical               one key order for named params
    7. recognize_complete                   syntax is recognized in place
    8. marker_forging_grants_no_capability  a recognized term is syntax
    9. marker_inert_elsewhere               markers are inert elsewhere
   10. schedule_unchanged_marker_free       HEAD schedule for marker-free Pars
   11. decode_commutes_with_subst           decoding commutes with subst
   12. plan_nil_annotation_not_injective    the superseded encoding fails
   13. refresh_strict_decode_erase          the refreshed strict decode is
                                            decode, and it rejects stale flags
   14. connective_marker_tuple_recognized   a tuple with the flag true is
                                            syntax
   15. refreshed_decode_commutes_with_subst the refresh makes the flagged
                                            decode commute with subst

   The module has no unfinished proof and no assumed constant. The
   Print Assumptions lines at the end confirm this for each headline.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `cbn [derived_flags ...]` reduces only one side of tuple_flags_eqb, so
     `rewrite tuple_flags_eqb_refl` then finds no match. Destruct the value
     flags and let the flags compute, or rewrite before the reduction.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Strings.String Strings.Ascii.
Import ListNotations.

(* ── 1. Bytes, the bytewise order and identifiers ─────────────────────── *)

(* A byte list models a GPrivate id and the UTF-8 bytes of a GString. *)
Definition bytes : Type := list nat.

(* bytes_compare is the lexicographic order on byte lists. It is the order of
   String::cmp, which compares the UTF-8 bytes of two strings. *)
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

Definition bytes_lt (a b : bytes) : Prop := bytes_compare a b = Lt.

Definition bytes_ltb (a b : bytes) : bool :=
  match bytes_compare a b with Lt => true | _ => false end.

Definition bytes_eqb (a b : bytes) : bool :=
  match bytes_compare a b with Eq => true | _ => false end.

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

Lemma bytes_lt_irrefl : forall a, ~ bytes_lt a a.
Proof.
  intros a H. unfold bytes_lt in H. rewrite bytes_compare_refl in H. discriminate.
Qed.

Lemma bytes_lt_trans : forall a b c, bytes_lt a b -> bytes_lt b c -> bytes_lt a c.
Proof. unfold bytes_lt. exact bytes_compare_lt_trans. Qed.

Lemma bytes_lt_asym : forall a b, bytes_lt a b -> ~ bytes_lt b a.
Proof.
  unfold bytes_lt. intros a b H H'.
  rewrite bytes_compare_antisym, H in H'. discriminate.
Qed.

Lemma bytes_ltb_iff : forall a b, bytes_ltb a b = true <-> bytes_lt a b.
Proof.
  intros a b. unfold bytes_ltb, bytes_lt.
  destruct (bytes_compare a b); split; intro H; try reflexivity; discriminate.
Qed.

Lemma bytes_eqb_iff : forall a b, bytes_eqb a b = true <-> a = b.
Proof.
  intros a b. unfold bytes_eqb. split.
  - destruct (bytes_compare a b) eqn:E; intro H; try discriminate.
    apply bytes_compare_eq. exact E.
  - intros ->. rewrite bytes_compare_refl. reflexivity.
Qed.

(* is_var_token accepts exactly the strings of the grammar token
   var = /[a-zA-Z]([a-zA-Z0-9_'])*|_([a-zA-Z0-9_'])+/ (grammar.js rule var). *)
Definition is_ascii_letter (b : nat) : bool :=
  ((65 <=? b) && (b <=? 90)) || ((97 <=? b) && (b <=? 122)).

Definition is_ascii_digit (b : nat) : bool := (48 <=? b) && (b <=? 57).

Definition is_identifier_tail_byte (b : nat) : bool :=
  is_ascii_letter b || is_ascii_digit b || (b =? 95) || (b =? 39).

Definition is_var_token (s : bytes) : bool :=
  match s with
  | [] => false
  | b :: rest =>
      (is_ascii_letter b && forallb is_identifier_tail_byte rest)
      || ((b =? 95)
          && negb (match rest with [] => true | _ :: _ => false end)
          && forallb is_identifier_tail_byte rest)
  end.

(* The word lists of plan decision 27, measured on parser 6548aec over the 32
   keywords of grammar 02cef80. A decoder accepts a param key or a theory name
   only when the parser accepts it there. The lists are the constants
   REJECTED_PARAM_KEYS and REJECTED_THEORY_NAMES of
   models/src/rust/reified/validation.rs, in the same order. *)
Module WordLists.
  Import (notations) Stdlib.Strings.String.
  Local Open Scope string_scope.

  Definition word (s : String.string) : bytes :=
    map Ascii.nat_of_ascii (String.list_ascii_of_string s).

  (* Bool ByteArray Int Nil String Uri true false, and the 18 other keywords
     that are not catch, constructor, default, in, method or private. *)
  Definition rejected_param_keys : list bytes :=
    map word ["Bool"; "ByteArray"; "Int"; "Nil"; "Set"; "String"; "Uri"; "agent";
              "and"; "bundle"; "bundle0"; "contract"; "else"; "false"; "for"; "if";
              "let"; "match"; "matches"; "new"; "not"; "or"; "select"; "true"; "try";
              "where"].

  (* Every keyword except Bool, ByteArray, Int, Nil, String, Uri, agent,
     bundle0, catch, constructor, default, method, private and try. *)
  Definition rejected_theory_names : list bytes :=
    map word ["Set"; "and"; "bundle"; "contract"; "else"; "false"; "for"; "if";
              "in"; "let"; "match"; "matches"; "new"; "not"; "or"; "select"; "true";
              "where"].
End WordLists.

Definition in_word_list (words : list bytes) (s : bytes) : bool :=
  existsb (bytes_eqb s) words.

(* is_identifier accepts a key of a named param: a var token outside the
   param-key list (models/src/rust/reified/validation.rs, is_param_key). *)
Definition is_identifier (s : bytes) : bool :=
  is_var_token s && negb (in_word_list WordLists.rejected_param_keys s).

(* is_theory_name accepts the theory name of free Th(a...): a var token
   outside the theory-name list (validation.rs, is_theory_name). *)
Definition is_theory_name (s : bytes) : bool :=
  is_var_token s && negb (in_word_list WordLists.rejected_theory_names s).

(* ── 2. The Par model ──────────────────────────────────────────────────── *)

(* An EVar holds a bound variable, a free variable or a wildcard. *)
Inductive var : Type :=
| VBound : nat -> var
| VFree : nat -> var
| VWildcard : var.

(* Each constructor is the α-image of one Rust Par shape (see the header). *)
Inductive par : Type :=
| PPriv : bytes -> par
| PStr : bytes -> par
| PList : list par -> par
| PTuple : list par -> par
| PMap : list (bytes * par) -> par
| PVar : var -> par
| PMethod : bytes -> par -> list par -> par
| PNew : nat -> list bytes -> nat -> par -> par
| PNil : par
| POther : nat -> par.

(* The automatic induction principle of a nested type gives no hypothesis
   for the list elements. This principle gives one for each element. *)
Section ParNestedInduction.
  Variable P : par -> Prop.
  Hypothesis case_priv : forall id, P (PPriv id).
  Hypothesis case_str : forall s, P (PStr s).
  Hypothesis case_list : forall ps, Forall P ps -> P (PList ps).
  Hypothesis case_tuple : forall ps, Forall P ps -> P (PTuple ps).
  Hypothesis case_map : forall kvs, Forall (fun kv => P (snd kv)) kvs -> P (PMap kvs).
  Hypothesis case_var : forall v, P (PVar v).
  Hypothesis case_method :
    forall name target args, P target -> Forall P args -> P (PMethod name target args).
  Hypothesis case_new : forall k uris inj body, P body -> P (PNew k uris inj body).
  Hypothesis case_nil : P PNil.
  Hypothesis case_other : forall n, P (POther n).

  Fixpoint par_nested_ind (t : par) : P t :=
    let fix on_list (ps : list par) : Forall P ps :=
      match ps as l return Forall P l with
      | [] => Forall_nil P
      | p :: rest => @Forall_cons par P p rest (par_nested_ind p) (on_list rest)
      end in
    let fix on_map (kvs : list (bytes * par)) : Forall (fun kv => P (snd kv)) kvs :=
      match kvs as l return Forall (fun kv => P (snd kv)) l with
      | [] => Forall_nil _
      | kv :: rest =>
          match kv as kv0 return Forall (fun kv => P (snd kv)) (kv0 :: rest) with
          | (k, v) =>
              @Forall_cons (bytes * par) (fun kv => P (snd kv)) (k, v) rest
                (par_nested_ind v) (on_map rest)
          end
      end in
    match t as t0 return P t0 with
    | PPriv id => case_priv id
    | PStr s => case_str s
    | PList ps => case_list ps (on_list ps)
    | PTuple ps => case_tuple ps (on_list ps)
    | PMap kvs => case_map kvs (on_map kvs)
    | PVar v => case_var v
    | PMethod name target args =>
        case_method name target args (par_nested_ind target) (on_list args)
    | PNew k uris inj body => case_new k uris inj body (par_nested_ind body)
    | PNil => case_nil
    | POther n => case_other n
    end.
End ParNestedInduction.

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
  - apply andb_true_iff in H as [H1 H2].
    f_equal; [apply Hx; exact H1 | apply IH; exact H2].
Qed.

Lemma list_eqb_refl :
  forall (A : Type) (eqb : A -> A -> bool) (xs : list A),
    Forall (fun x => eqb x x = true) xs -> list_eqb eqb xs xs = true.
Proof.
  intros A eqb xs HF. induction HF as [| x xs Hx HF IH]; simpl; [reflexivity |].
  rewrite Hx, IH. reflexivity.
Qed.

Definition var_eqb (v w : var) : bool :=
  match v, w with
  | VBound i, VBound j => i =? j
  | VFree i, VFree j => i =? j
  | VWildcard, VWildcard => true
  | _, _ => false
  end.

Lemma var_eqb_iff : forall v w, var_eqb v w = true <-> v = w.
Proof.
  intros [i | i |] [j | j |]; simpl; split; intro H; try discriminate;
    try reflexivity.
  - apply Nat.eqb_eq in H. subst. reflexivity.
  - injection H as ->. apply Nat.eqb_refl.
  - apply Nat.eqb_eq in H. subst. reflexivity.
  - injection H as ->. apply Nat.eqb_refl.
Qed.

(* par_eqb decides the equality of model terms. It models the equality
   check `encode(decoded) == input` of the planned decoder. *)
Fixpoint par_eqb (x y : par) {struct x} : bool :=
  match x, y with
  | PPriv a, PPriv b => bytes_eqb a b
  | PStr a, PStr b => bytes_eqb a b
  | PList xs, PList ys => list_eqb par_eqb xs ys
  | PTuple xs, PTuple ys => list_eqb par_eqb xs ys
  | PMap xs, PMap ys =>
      list_eqb
        (fun kv1 kv2 => bytes_eqb (fst kv1) (fst kv2) && par_eqb (snd kv1) (snd kv2))
        xs ys
  | PVar v, PVar w => var_eqb v w
  | PMethod n1 t1 a1, PMethod n2 t2 a2 =>
      bytes_eqb n1 n2 && par_eqb t1 t2 && list_eqb par_eqb a1 a2
  | PNew k1 u1 i1 b1, PNew k2 u2 i2 b2 =>
      (k1 =? k2) && list_eqb bytes_eqb u1 u2 && (i1 =? i2) && par_eqb b1 b2
  | PNil, PNil => true
  | POther n1, POther n2 => n1 =? n2
  | _, _ => false
  end.

Lemma list_bytes_eqb_iff : forall us vs, list_eqb bytes_eqb us vs = true <-> us = vs.
Proof.
  intros us vs. split.
  - apply list_eqb_sound. apply Forall_forall. intros u _ v. apply bytes_eqb_iff.
  - intros ->. apply list_eqb_refl. apply Forall_forall. intros u _.
    apply bytes_eqb_iff. reflexivity.
Qed.

Lemma par_eqb_sound : forall x y, par_eqb x y = true -> x = y.
Proof.
  induction x as [id | s | ps IH | ps IH | kvs IH | v | name target args IHt IHa
                  | k uris inj body IHb | | n] using par_nested_ind;
    intros [id' | s' | ps' | ps' | kvs' | v' | name' target' args'
            | k' uris' inj' body' | | n'] H; simpl in H; try discriminate.
  - apply bytes_eqb_iff in H. subst. reflexivity.
  - apply bytes_eqb_iff in H. subst. reflexivity.
  - f_equal. exact (list_eqb_sound _ _ _ IH _ H).
  - f_equal. exact (list_eqb_sound _ _ _ IH _ H).
  - f_equal. refine (list_eqb_sound _ _ _ _ _ H).
    apply Forall_forall. intros [kk vv] Hin [kk' vv'] Hkv. simpl in Hkv.
    apply andb_true_iff in Hkv as [Hk Hv].
    apply bytes_eqb_iff in Hk. subst kk'.
    rewrite Forall_forall in IH. specialize (IH (kk, vv) Hin). simpl in IH.
    rewrite (IH vv' Hv). reflexivity.
  - apply var_eqb_iff in H. subst. reflexivity.
  - apply andb_true_iff in H as [H H3]. apply andb_true_iff in H as [H1 H2].
    apply bytes_eqb_iff in H1. subst name'.
    rewrite (IHt _ H2). f_equal. exact (list_eqb_sound _ _ _ IHa _ H3).
  - apply andb_true_iff in H as [H H4]. apply andb_true_iff in H as [H H3].
    apply andb_true_iff in H as [H1 H2].
    apply Nat.eqb_eq in H1. apply list_bytes_eqb_iff in H2. apply Nat.eqb_eq in H3.
    subst. rewrite (IHb _ H4). reflexivity.
  - reflexivity.
  - apply Nat.eqb_eq in H. subst. reflexivity.
Qed.

Lemma par_eqb_refl : forall x, par_eqb x x = true.
Proof.
  induction x as [id | s | ps IH | ps IH | kvs IH | v | name target args IHt IHa
                  | k uris inj body IHb | | n] using par_nested_ind; simpl.
  - apply bytes_eqb_iff. reflexivity.
  - apply bytes_eqb_iff. reflexivity.
  - apply list_eqb_refl. exact IH.
  - apply list_eqb_refl. exact IH.
  - apply list_eqb_refl. apply Forall_forall. intros [kk vv] Hin. simpl.
    rewrite Forall_forall in IH. specialize (IH (kk, vv) Hin). simpl in IH.
    rewrite IH. rewrite (proj2 (bytes_eqb_iff kk kk) eq_refl). reflexivity.
  - apply var_eqb_iff. reflexivity.
  - rewrite (proj2 (bytes_eqb_iff name name) eq_refl), IHt, (list_eqb_refl _ _ _ IHa).
    reflexivity.
  - rewrite Nat.eqb_refl, (proj2 (list_bytes_eqb_iff uris uris) eq_refl), Nat.eqb_refl, IHb.
    reflexivity.
  - reflexivity.
  - apply Nat.eqb_refl.
Qed.

Lemma par_eqb_iff : forall x y, par_eqb x y = true <-> x = y.
Proof.
  intros x y. split; [apply par_eqb_sound | intros ->; apply par_eqb_refl].
Qed.

(* ── 3. Markers ────────────────────────────────────────────────────────── *)

Inductive marker : Type := MUse | MNewSpace | MParams | MQuery | MFree.

(* 0xE0 is the Reified namespace tag (plan decision 7). *)
Definition reified_tag : nat := 224.

(* 0xD0 to 0xD4 are the marker indices (plan decision 18). *)
Definition marker_index (m : marker) : nat :=
  match m with
  | MUse => 208
  | MNewSpace => 209
  | MParams => 210
  | MQuery => 211
  | MFree => 212
  end.

Definition marker_id (m : marker) : bytes := [reified_tag; marker_index m].

Definition marker_par (m : marker) : par := PPriv (marker_id m).

Definition all_markers : list marker := [MUse; MNewSpace; MParams; MQuery; MFree].

(* The arity of each marker tuple, including the head (design section 4.2). *)
Definition marker_tuple_arity (m : marker) : nat :=
  match m with
  | MUse => 3
  | MNewSpace => 3
  | MParams => 3
  | MQuery => 4
  | MFree => 3
  end.

Definition marker_of_index (b : nat) : option marker :=
  if b =? 208 then Some MUse
  else if b =? 209 then Some MNewSpace
  else if b =? 210 then Some MParams
  else if b =? 211 then Some MQuery
  else if b =? 212 then Some MFree
  else None.

(* marker_of_id models Marker::from_id. It accepts an id only when the id has
   exactly two bytes. This agrees with the design rule that every capability
   check tests the length and the KIND, never only the first byte. *)
Definition marker_of_id (id : bytes) : option marker :=
  match id with
  | [t; b] => if t =? reified_tag then marker_of_index b else None
  | _ => None
  end.

Lemma marker_of_id_marker_id : forall m, marker_of_id (marker_id m) = Some m.
Proof. intros []; reflexivity. Qed.

Lemma marker_of_index_inv : forall b m, marker_of_index b = Some m -> b = marker_index m.
Proof.
  intros b m H. unfold marker_of_index in H.
  destruct (b =? 208) eqn:E0; [injection H as <-; apply Nat.eqb_eq; exact E0 |].
  destruct (b =? 209) eqn:E1; [injection H as <-; apply Nat.eqb_eq; exact E1 |].
  destruct (b =? 210) eqn:E2; [injection H as <-; apply Nat.eqb_eq; exact E2 |].
  destruct (b =? 211) eqn:E3; [injection H as <-; apply Nat.eqb_eq; exact E3 |].
  destruct (b =? 212) eqn:E4; [injection H as <-; apply Nat.eqb_eq; exact E4 |].
  discriminate.
Qed.

Lemma marker_of_id_inv : forall id m, marker_of_id id = Some m -> id = marker_id m.
Proof.
  intros [| t [| b [| x rest]]] m H; simpl in H; try discriminate.
  destruct (t =? reified_tag) eqn:Et; [| discriminate].
  apply Nat.eqb_eq in Et. subst t.
  apply marker_of_index_inv in H. subst b. reflexivity.
Qed.

Lemma marker_index_range : forall m, 208 <= marker_index m <= 212.
Proof. intros []; simpl; lia. Qed.

Lemma marker_id_injective : forall m1 m2, marker_id m1 = marker_id m2 -> m1 = m2.
Proof.
  intros m1 m2 H.
  pose proof (marker_of_id_marker_id m1) as H1.
  rewrite H, marker_of_id_marker_id in H1. injection H1 as ->. reflexivity.
Qed.

(* Headline 4. The five marker ids are pairwise distinct, the list of markers
   is complete, and the id decoder inverts marker_id. *)
Theorem markers_pairwise_distinct :
  NoDup (map marker_id all_markers) /\
  (forall m1 m2 : marker, marker_id m1 = marker_id m2 -> m1 = m2) /\
  (forall m : marker, In m all_markers) /\
  (forall m : marker, marker_of_id (marker_id m) = Some m).
Proof.
  split; [| split; [exact marker_id_injective | split]].
  - simpl. repeat constructor; simpl; intro H;
      repeat (destruct H as [H | H]; [injection H; lia |]); exact H.
  - intros []; simpl; tauto.
  - exact marker_of_id_marker_id.
Qed.

(* The other [0xE0, idx] ids of the plan: 0x00 to 0x25 (factories,
   getSpaceAgent, assertEqual, vector, embed, ann), 0x30 to 0x32 (internal
   ops) and 0xFF (AGENT_TAG). *)
Definition reserved_system_indices : list nat := seq 0 38 ++ [48; 49; 50] ++ [255].

(* Headline 5. No marker id equals another [0xE0, idx] id. No marker id
   equals a 1-byte name, a 32-byte new id or an id of 34 bytes or more
   (storage channels, MeTTaIL ids). The last conjunct covers every length
   other than 2. *)
Theorem markers_disjoint_reserved :
  forall m : marker,
    (forall idx : nat, In idx reserved_system_indices -> marker_id m <> [reified_tag; idx]) /\
    length (marker_id m) = 2 /\
    (forall id : bytes,
        length id = 1 \/ length id = 32 \/ 34 <= length id -> marker_id m <> id) /\
    (forall id : bytes, length id <> 2 -> marker_id m <> id).
Proof.
  intros m. split; [| split; [| split]].
  - intros idx Hin Heq. unfold marker_id in Heq. injection Heq as Hidx.
    pose proof (marker_index_range m) as Hr.
    unfold reserved_system_indices in Hin.
    apply in_app_or in Hin as [Hin | Hin].
    + apply in_seq in Hin. lia.
    + simpl in Hin. lia.
  - reflexivity.
  - intros id Hlen Heq. subst id. simpl in Hlen. lia.
  - intros id Hlen Heq. subst id. apply Hlen. reflexivity.
Qed.

(* ── 4. Canonical string-keyed maps ────────────────────────────────────── *)

Definition map_values (f : par -> par) (kvs : list (bytes * par)) : list (bytes * par) :=
  map (fun kv => (fst kv, f (snd kv))) kvs.

(* The keys of a canonical map increase strictly in the bytewise order. *)
Fixpoint keys_strictly_sorted (kvs : list (bytes * par)) : bool :=
  match kvs with
  | [] => true
  | kv :: rest =>
      match rest with
      | [] => true
      | kv' :: _ => bytes_ltb (fst kv) (fst kv') && keys_strictly_sorted rest
      end
  end.

Lemma keys_strictly_sorted_cons :
  forall kv l,
    keys_strictly_sorted (kv :: l) = true <->
    keys_strictly_sorted l = true /\ Forall (fun kv' => bytes_lt (fst kv) (fst kv')) l.
Proof.
  intros kv l. revert kv. induction l as [| kv' l IH]; intros kv.
  - simpl. split; [intros _; split; [reflexivity | constructor] | intros _; reflexivity].
  - change (keys_strictly_sorted (kv :: kv' :: l))
      with (bytes_ltb (fst kv) (fst kv') && keys_strictly_sorted (kv' :: l)).
    rewrite andb_true_iff, bytes_ltb_iff. split.
    + intros [Hlt Hs]. split; [exact Hs |].
      apply IH in Hs as [_ Hall]. constructor; [exact Hlt |].
      eapply Forall_impl; [| exact Hall].
      intros x Hx. eapply bytes_lt_trans; [exact Hlt | exact Hx].
    + intros [Hs Hall]. inversion Hall as [| ? ? Hlt _]; subst. split; assumption.
Qed.

Lemma map_values_fst : forall f kvs, map fst (map_values f kvs) = map fst kvs.
Proof.
  intros f kvs. unfold map_values. rewrite map_map. simpl. reflexivity.
Qed.

Lemma keys_strictly_sorted_map_values :
  forall f kvs, keys_strictly_sorted (map_values f kvs) = keys_strictly_sorted kvs.
Proof.
  intros f kvs. induction kvs as [| kv kvs IH]; [reflexivity |].
  destruct kvs as [| kv' kvs]; [reflexivity |].
  change (bytes_ltb (fst kv) (fst kv') && keys_strictly_sorted (map_values f (kv' :: kvs)) =
          bytes_ltb (fst kv) (fst kv') && keys_strictly_sorted (kv' :: kvs)).
  rewrite IH. reflexivity.
Qed.

(* insert_kv_keep inserts one entry into a sorted list. When the key is
   already present, the existing entry stays. *)
Fixpoint insert_kv_keep (kv : bytes * par) (l : list (bytes * par)) : list (bytes * par) :=
  match l with
  | [] => [kv]
  | kv' :: rest =>
      match bytes_compare (fst kv) (fst kv') with
      | Lt => kv :: kv' :: rest
      | Eq => kv' :: rest
      | Gt => kv' :: insert_kv_keep kv rest
      end
  end.

(* canon_map models SortedParMap::create_from_vec on GString keys
   (sorted_par_map.rs:71-74). The HashMap collect keeps the last value of a
   duplicate key. The fold inserts the entries from right to left and keeps
   an existing entry, so the last value also wins here. Ordering::sort_map
   then sorts the entries by key score (ordering.rs:68-76). For two GString
   keys that order is String::cmp (score_tree.rs:77). *)
Definition canon_map (kvs : list (bytes * par)) : list (bytes * par) :=
  fold_right insert_kv_keep [] kvs.

Lemma insert_kv_keep_in :
  forall kv l x, In x (insert_kv_keep kv l) -> x = kv \/ In x l.
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

Lemma insert_kv_keep_sorted :
  forall kv l,
    keys_strictly_sorted l = true -> keys_strictly_sorted (insert_kv_keep kv l) = true.
Proof.
  intros kv l. induction l as [| kv' l IH]; intros Hs; [reflexivity |].
  apply keys_strictly_sorted_cons in Hs as [Hs Hall]. simpl.
  destruct (bytes_compare (fst kv) (fst kv')) eqn:E.
  - apply keys_strictly_sorted_cons. split; assumption.
  - apply keys_strictly_sorted_cons. split.
    + apply keys_strictly_sorted_cons. split; assumption.
    + constructor; [exact E |].
      eapply Forall_impl; [| exact Hall].
      intros x Hx. eapply bytes_lt_trans; [exact E | exact Hx].
  - apply keys_strictly_sorted_cons. split; [apply IH; exact Hs |].
    apply Forall_forall. intros x Hx. apply insert_kv_keep_in in Hx as [-> | Hx].
    + unfold bytes_lt. rewrite bytes_compare_antisym, E. reflexivity.
    + rewrite Forall_forall in Hall. apply Hall. exact Hx.
Qed.

Lemma canon_map_sorted : forall kvs, keys_strictly_sorted (canon_map kvs) = true.
Proof.
  induction kvs as [| kv kvs IH]; [reflexivity |].
  simpl. apply insert_kv_keep_sorted. exact IH.
Qed.

Lemma canon_map_id : forall kvs, keys_strictly_sorted kvs = true -> canon_map kvs = kvs.
Proof.
  induction kvs as [| kv kvs IH]; intros Hs; [reflexivity |].
  apply keys_strictly_sorted_cons in Hs as [Hs Hall].
  simpl. fold (canon_map kvs). rewrite (IH Hs).
  destruct kvs as [| kv' kvs]; [reflexivity |].
  inversion Hall as [| ? ? Hlt _]; subst. simpl.
  unfold bytes_lt in Hlt. rewrite Hlt. reflexivity.
Qed.

Lemma insert_kv_keep_perm :
  forall kv l, ~ In (fst kv) (map fst l) -> Permutation (insert_kv_keep kv l) (kv :: l).
Proof.
  intros kv l. induction l as [| kv' l IH]; intros Hnot; [reflexivity |].
  simpl in Hnot. simpl.
  destruct (bytes_compare (fst kv) (fst kv')) eqn:E.
  - apply bytes_compare_eq in E. exfalso. apply Hnot. left. symmetry. exact E.
  - reflexivity.
  - eapply perm_trans; [apply perm_skip; apply IH; intro H; apply Hnot; right; exact H |].
    apply perm_swap.
Qed.

Lemma canon_map_perm : forall kvs, NoDup (map fst kvs) -> Permutation (canon_map kvs) kvs.
Proof.
  induction kvs as [| kv kvs IH]; intros Hnd; [reflexivity |].
  simpl in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
  simpl. fold (canon_map kvs).
  specialize (IH Hnd).
  eapply perm_trans.
  - apply insert_kv_keep_perm. intro H. apply Hnot.
    apply (Permutation_in _ (Permutation_map fst IH)) in H. exact H.
  - apply perm_skip. exact IH.
Qed.

(* Two strictly sorted lists with the same elements are equal. *)
Lemma sorted_perm_eq :
  forall l1 l2,
    keys_strictly_sorted l1 = true -> keys_strictly_sorted l2 = true ->
    Permutation l1 l2 -> l1 = l2.
Proof.
  induction l1 as [| a l1 IH]; intros l2 Hs1 Hs2 Hp.
  - apply Permutation_nil in Hp. symmetry. exact Hp.
  - destruct l2 as [| b l2].
    + apply Permutation_sym, Permutation_nil in Hp. discriminate.
    + apply keys_strictly_sorted_cons in Hs1 as [Hs1 Hall1].
      apply keys_strictly_sorted_cons in Hs2 as [Hs2 Hall2].
      assert (Hb : In b (a :: l1))
        by (eapply Permutation_in; [apply Permutation_sym; exact Hp | left; reflexivity]).
      assert (Ha : In a (b :: l2)) by (eapply Permutation_in; [exact Hp | left; reflexivity]).
      destruct Hb as [Hab | Hb].
      * subst b. f_equal. apply IH; [exact Hs1 | exact Hs2 |].
        eapply Permutation_cons_inv. exact Hp.
      * destruct Ha as [Hba | Ha].
        -- subst b. f_equal. apply IH; [exact Hs1 | exact Hs2 |].
           eapply Permutation_cons_inv. exact Hp.
        -- rewrite Forall_forall in Hall1, Hall2.
           exfalso. apply (bytes_lt_asym (fst a) (fst b));
             [apply Hall1; exact Hb | apply Hall2; exact Ha].
Qed.

Lemma canon_map_nil_iff : forall kvs, canon_map kvs = [] <-> kvs = [].
Proof.
  intros [| kv kvs]; simpl; split; intro H; try reflexivity; try discriminate.
  destruct (canon_map kvs) as [| kv' l]; simpl in H; [discriminate |].
  destruct (bytes_compare (fst kv) (fst kv')); discriminate.
Qed.

Lemma canon_map_keys_in :
  forall kvs k, In k (map fst (canon_map kvs)) -> In k (map fst kvs).
Proof.
  induction kvs as [| kv kvs IH]; intros k H; [exact H |].
  simpl in H. fold (canon_map kvs) in H.
  apply in_map_iff in H as [x [Hx Hin]]. subst k.
  apply insert_kv_keep_in in Hin as [-> | Hin]; [left; reflexivity |].
  right. apply IH. apply in_map. exact Hin.
Qed.

Lemma canon_map_permutation_invariant :
  forall kvs1 kvs2,
    NoDup (map fst kvs1) -> Permutation kvs1 kvs2 -> canon_map kvs1 = canon_map kvs2.
Proof.
  intros kvs1 kvs2 Hnd Hp.
  assert (Hnd2 : NoDup (map fst kvs2))
    by (eapply Permutation_NoDup; [apply Permutation_map; exact Hp | exact Hnd]).
  apply sorted_perm_eq; [apply canon_map_sorted | apply canon_map_sorted |].
  eapply perm_trans; [apply canon_map_perm; exact Hnd |].
  eapply perm_trans; [exact Hp |].
  apply Permutation_sym. apply canon_map_perm. exact Hnd2.
Qed.

(* ── 5. Constructs, well-formedness and the encoder ────────────────────── *)

(* The model has one constructor for each Reified construct (design
   section 4.2). The arguments are these:
   - CUse: the space and the body.
   - CNewSpace: the annotations, bind_count, the URIs, the injections and the
     body of the New.
   - CParams: the positional params and the named params.
   - CQuery: the channel, the payload and the named params.
   - CFree: the theory name and the arguments. *)
Inductive construct : Type :=
| CUse : par -> par -> construct
| CNewSpace : list (option par) -> nat -> list bytes -> nat -> par -> construct
| CParams : list par -> list (bytes * par) -> construct
| CQuery : par -> list par -> list (bytes * par) -> construct
| CFree : bytes -> list par -> construct.

Definition construct_marker (c : construct) : marker :=
  match c with
  | CUse _ _ => MUse
  | CNewSpace _ _ _ _ _ => MNewSpace
  | CParams _ _ => MParams
  | CQuery _ _ _ => MQuery
  | CFree _ _ => MFree
  end.

(* Decision 21: an annotation entry is [] (no annotation) or [s] (space s). *)
Definition encode_annotation (a : option par) : par :=
  match a with
  | None => PList []
  | Some space => PList [space]
  end.

(* The corrected encoding table of decision 21. *)
Definition encode (c : construct) : par :=
  match c with
  | CUse space body => PTuple [marker_par MUse; space; body]
  | CNewSpace annotations k uris injections body =>
      PTuple [marker_par MNewSpace; PList (map encode_annotation annotations);
              PNew k uris injections body]
  | CParams positional named => PTuple [marker_par MParams; PList positional; PMap named]
  | CQuery channel payload named =>
      PTuple [marker_par MQuery; channel; PList payload; PMap named]
  | CFree theory args => PTuple [marker_par MFree; PStr theory; PList args]
  end.

Definition is_annotated (a : option par) : bool :=
  match a with Some _ => true | None => false end.

Definition is_nil {A : Type} (l : list A) : bool :=
  match l with [] => true | _ :: _ => false end.

(* The keys of named params are identifiers in the canonical order. The
   parser v3 AST builder rejects other keys (NonIdentifierParamKey). *)
Definition named_params_ok (named : list (bytes * par)) : bool :=
  keys_strictly_sorted named && forallb is_identifier (map fst named).

(* wf states the decoder obligations of design section 2(d): identifiers,
   counts and empty forms. The remainder obligation holds because α maps a
   collection with a remainder to POther. *)
Definition wf (c : construct) : Prop :=
  match c with
  | CUse _ _ => True
  | CNewSpace annotations k uris _ _ =>
      k = length annotations + length uris /\ existsb is_annotated annotations = true
  | CParams positional named =>
      (positional <> [] \/ named <> []) /\ named_params_ok named = true
  | CQuery _ payload named =>
      (payload <> [] \/ named <> []) /\ named_params_ok named = true
  | CFree theory _ => is_theory_name theory = true
  end.

Definition wfb (c : construct) : bool :=
  match c with
  | CUse _ _ => true
  | CNewSpace annotations k uris _ _ =>
      (k =? length annotations + length uris) && existsb is_annotated annotations
  | CParams positional named =>
      negb (is_nil positional && is_nil named) && named_params_ok named
  | CQuery _ payload named =>
      negb (is_nil payload && is_nil named) && named_params_ok named
  | CFree theory _ => is_theory_name theory
  end.

Lemma nonempty_sections_iff :
  forall (A B : Type) (xs : list A) (ys : list B),
    negb (is_nil xs && is_nil ys) = true <-> (xs <> [] \/ ys <> []).
Proof.
  intros A B [| x xs] [| y ys]; simpl; split; intro H; try discriminate;
    try (right; discriminate); try (left; discriminate); try reflexivity.
  destruct H as [H | H]; contradiction.
Qed.

Lemma wfb_spec : forall c, wfb c = true <-> wf c.
Proof.
  intros [s p | a k u i b | pos named | ch q named | th args]; simpl.
  - split; intros _; [exact I | reflexivity].
  - rewrite andb_true_iff, Nat.eqb_eq. reflexivity.
  - rewrite andb_true_iff, nonempty_sections_iff. reflexivity.
  - rewrite andb_true_iff, nonempty_sections_iff. reflexivity.
  - reflexivity.
Qed.

(* Builders (design section 4.2). A send without params gets no PARAMS
   element. The builders sort the named params with canon_map. *)
Definition build_params (positional : list par) (named : list (bytes * par))
  : option construct :=
  if is_nil positional && is_nil named then None
  else Some (CParams positional (canon_map named)).

Definition send_data (data positional : list par) (named : list (bytes * par)) : list par :=
  match build_params positional named with
  | Some c => data ++ [encode c]
  | None => data
  end.

(* The parser rejects the empty query group (MalformedQuerySpec), so the
   builder returns None for it. *)
Definition build_query (channel : par) (payload : list par) (named : list (bytes * par))
  : option construct :=
  if is_nil payload && is_nil named then None
  else Some (CQuery channel payload (canon_map named)).

Lemma is_nil_perm :
  forall (A : Type) (l1 l2 : list A), Permutation l1 l2 -> is_nil l1 = is_nil l2.
Proof.
  intros A l1 l2 Hp. destruct l1, l2; try reflexivity.
  - apply Permutation_nil in Hp. discriminate.
  - apply Permutation_sym, Permutation_nil in Hp. discriminate.
Qed.

Lemma build_params_wf :
  forall positional named c,
    forallb is_identifier (map fst named) = true ->
    build_params positional named = Some c -> wf c.
Proof.
  intros positional named c Hid H. unfold build_params in H.
  destruct (is_nil positional && is_nil named) eqn:Hn; [discriminate |].
  injection H as <-. simpl. split.
  - destruct positional as [| p positional]; [| left; discriminate].
    destruct named as [| kv named]; [simpl in Hn; discriminate |].
    right. intro Hc. rewrite canon_map_nil_iff in Hc. discriminate.
  - unfold named_params_ok. rewrite canon_map_sorted. simpl.
    apply forallb_forall. intros k Hk. apply canon_map_keys_in in Hk.
    rewrite forallb_forall in Hid. apply Hid. exact Hk.
Qed.

(* Design section 1.3: empty sections are erased. `x!(a;)` encodes as
   `x!(a)`, and the encoder never builds an empty PARAMS or QUERY tuple. *)
Lemma empty_sections_erased :
  (forall data, send_data data [] [] = data) /\
  build_params [] [] = None /\
  (forall channel, build_query channel [] [] = None).
Proof. split; [| split]; reflexivity. Qed.

(* Headline 6. canon_map sorts the named params by String::cmp. The result
   does not depend on the source order of distinct keys. The decoder accepts
   exactly the fixed points of canon_map. *)
Theorem named_params_canonical :
  (forall kvs, keys_strictly_sorted (canon_map kvs) = true) /\
  (forall kvs, canon_map kvs = kvs <-> keys_strictly_sorted kvs = true) /\
  (forall kvs1 kvs2,
      NoDup (map fst kvs1) -> Permutation kvs1 kvs2 -> canon_map kvs1 = canon_map kvs2) /\
  (forall kvs, NoDup (map fst kvs) -> Permutation (canon_map kvs) kvs) /\
  (forall data positional named1 named2,
      NoDup (map fst named1) -> Permutation named1 named2 ->
      send_data data positional named1 = send_data data positional named2) /\
  (forall positional named, wf (CParams positional named) -> canon_map named = named) /\
  (forall channel payload named, wf (CQuery channel payload named) -> canon_map named = named).
Proof.
  split; [exact canon_map_sorted |].
  split.
  { intros kvs. split; [intros H; rewrite <- H; apply canon_map_sorted | apply canon_map_id]. }
  split; [exact canon_map_permutation_invariant |].
  split; [exact canon_map_perm |].
  split.
  { intros data positional named1 named2 Hnd Hp. unfold send_data, build_params.
    rewrite (is_nil_perm _ _ _ Hp), (canon_map_permutation_invariant _ _ Hnd Hp).
    reflexivity. }
  split.
  { intros positional named [_ Hok]. apply canon_map_id.
    unfold named_params_ok in Hok. apply andb_true_iff in Hok as [Hs _]. exact Hs. }
  { intros channel payload named [_ Hok]. apply canon_map_id.
    unfold named_params_ok in Hok. apply andb_true_iff in Hok as [Hs _]. exact Hs. }
Qed.

(* ── 6. The decoder ────────────────────────────────────────────────────── *)

Definition parse_annotation (a : par) : option (option par) :=
  match a with
  | PList [] => Some None
  | PList [space] => Some (Some space)
  | _ => None
  end.

Fixpoint parse_annotations (l : list par) : option (list (option par)) :=
  match l with
  | [] => Some []
  | a :: rest =>
      match parse_annotation a, parse_annotations rest with
      | Some x, Some xs => Some (x :: xs)
      | _, _ => None
      end
  end.

(* parse_fields reads the fields after the head. A wrong arity, a wrong
   element kind or a wrong annotation shape gives None. *)
Definition parse_fields (m : marker) (fields : list par) : option construct :=
  match m, fields with
  | MUse, [space; body] => Some (CUse space body)
  | MNewSpace, [PList annotations; PNew k uris injections body] =>
      match parse_annotations annotations with
      | Some a => Some (CNewSpace a k uris injections body)
      | None => None
      end
  | MParams, [PList positional; PMap named] => Some (CParams positional named)
  | MQuery, [channel; PList payload; PMap named] => Some (CQuery channel payload named)
  | MFree, [PStr theory; PList args] => Some (CFree theory args)
  | _, _ => None
  end.

(* head_marker models sole_marker_tuple. The term must be exactly a tuple,
   and its head must be exactly a marker. *)
Definition head_marker (t : par) : option marker :=
  match t with
  | PTuple (PPriv id :: _) => marker_of_id id
  | _ => None
  end.

Definition parse (t : par) : option construct :=
  match t with
  | PTuple (PPriv id :: fields) =>
      match marker_of_id id with
      | Some m => parse_fields m fields
      | None => None
      end
  | _ => None
  end.

(* decode parses, checks wf and then checks canonicality: encode of the
   result must equal the input (the NonCanonical check of section 4.2). *)
Definition decode (t : par) : option construct :=
  match parse t with
  | Some c => if wfb c && par_eqb (encode c) t then Some c else None
  | None => None
  end.

Lemma parse_annotations_encode :
  forall a, parse_annotations (map encode_annotation a) = Some a.
Proof.
  induction a as [| [s |] a IH]; simpl;
    [reflexivity | rewrite IH; reflexivity | rewrite IH; reflexivity].
Qed.

Lemma parse_annotations_exact :
  forall l a, parse_annotations l = Some a -> map encode_annotation a = l.
Proof.
  induction l as [| x l IH]; intros a H; simpl in H.
  - injection H as <-. reflexivity.
  - destruct (parse_annotation x) as [ox |] eqn:Ex; [| discriminate].
    destruct (parse_annotations l) as [xs |] eqn:Exs; [| discriminate].
    injection H as <-. simpl. rewrite (IH xs eq_refl). f_equal.
    destruct x as [| | ps | | | | | | |]; simpl in Ex; try discriminate.
    destruct ps as [| p [| q ps]]; simpl in Ex; try discriminate.
    + injection Ex as <-. reflexivity.
    + injection Ex as <-. reflexivity.
Qed.

Lemma parse_encode : forall c, parse (encode c) = Some c.
Proof.
  intros [s p | a k u i b | pos named | ch q named | th args];
    unfold parse, encode, marker_par; rewrite marker_of_id_marker_id; simpl; try reflexivity.
  rewrite parse_annotations_encode. reflexivity.
Qed.

(* parse is exact: a successful parse already reproduces its input. *)
Lemma parse_exact : forall t c, parse t = Some c -> encode c = t.
Proof.
  intros t c H.
  destruct t as [| | | fields | | | | | |]; cbn in H; try discriminate.
  destruct fields as [| hd fields]; cbn in H; [discriminate |].
  destruct hd as [id | | | | | | | | |]; cbn in H; try discriminate.
  destruct (marker_of_id id) as [m |] eqn:Em; [| discriminate].
  apply marker_of_id_inv in Em. subst id.
  destruct m; cbn in H.
  - destruct fields as [| s [| p [| x rest]]]; cbn in H; try discriminate.
    injection H as <-. reflexivity.
  - destruct fields as [| f1 [| f2 [| x rest]]]; cbn in H; try discriminate;
      destruct f1 as [| | annotations | | | | | | |]; cbn in H; try discriminate;
      destruct f2 as [| | | | | | | k uris injections body | |]; cbn in H; try discriminate;
      destruct (parse_annotations annotations) as [a |] eqn:Ea; try discriminate.
    injection H as <-. cbn. rewrite (parse_annotations_exact _ _ Ea). reflexivity.
  - destruct fields as [| f1 [| f2 [| x rest]]]; cbn in H; try discriminate;
      destruct f1 as [| | positional | | | | | | |]; cbn in H; try discriminate;
      destruct f2 as [| | | | named | | | | |]; cbn in H; try discriminate.
    injection H as <-. reflexivity.
  - destruct fields as [| ch [| f1 [| f2 [| x rest]]]]; cbn in H; try discriminate;
      destruct f1 as [| | payload | | | | | | |]; cbn in H; try discriminate;
      destruct f2 as [| | | | named | | | | |]; cbn in H; try discriminate.
    injection H as <-. reflexivity.
  - destruct fields as [| f1 [| f2 [| x rest]]]; cbn in H; try discriminate;
      destruct f1 as [| theory | | | | | | | |]; cbn in H; try discriminate;
      destruct f2 as [| | args | | | | | | |]; cbn in H; try discriminate.
    injection H as <-. reflexivity.
Qed.

(* Headline 1. decode inverts encode on well-formed constructs. *)
Theorem decode_encode : forall c, wf c -> decode (encode c) = Some c.
Proof.
  intros c Hwf. unfold decode. rewrite parse_encode.
  apply wfb_spec in Hwf. rewrite Hwf, par_eqb_refl. reflexivity.
Qed.

(* Headline 2. encode is injective on all constructs. The proof does not
   need wf. *)
Theorem encode_injective : forall c1 c2, encode c1 = encode c2 -> c1 = c2.
Proof.
  intros c1 c2 H.
  pose proof (parse_encode c1) as H1. pose proof (parse_encode c2) as H2.
  rewrite H in H1. rewrite H1 in H2. injection H2 as ->. reflexivity.
Qed.

(* Headline 3. decode accepts only the canonical image of encode. *)
Theorem decode_sound : forall t c, decode t = Some c -> t = encode c /\ wf c.
Proof.
  intros t c H. unfold decode in H.
  destruct (parse t) as [c' |] eqn:Ep; [| discriminate].
  destruct (wfb c' && par_eqb (encode c') t) eqn:Ec; [| discriminate].
  injection H as <-.
  apply andb_true_iff in Ec as [Hwf Heq].
  split; [symmetry; apply par_eqb_sound; exact Heq | apply wfb_spec; exact Hwf].
Qed.

Corollary decode_spec : forall t c, decode t = Some c <-> wf c /\ t = encode c.
Proof.
  intros t c. split.
  - intros H. apply decode_sound in H as [Ht Hwf]. split; assumption.
  - intros [Hwf ->]. apply decode_encode. exact Hwf.
Qed.

(* The canonicality check never rejects a term that parse and wfb accept.
   The check is a second defense in the planned Rust decoder. *)
Lemma canonicality_check_redundant :
  forall t c, parse t = Some c -> wfb c = true -> decode t = Some c.
Proof.
  intros t c Hp Hw. unfold decode. rewrite Hp, Hw.
  rewrite (parse_exact _ _ Hp), par_eqb_refl. reflexivity.
Qed.

Lemma head_marker_encode : forall c, head_marker (encode c) = Some (construct_marker c).
Proof. intros []; reflexivity. Qed.

Definition tuple_elements (t : par) : list par :=
  match t with
  | PTuple ps => ps
  | _ => []
  end.

(* Every decoded tuple has the arity of its marker (WrongArity). *)
Lemma decode_arity :
  forall t c, decode t = Some c -> length (tuple_elements t) = marker_tuple_arity (construct_marker c).
Proof.
  intros t c H. apply decode_sound in H as [-> _]. destruct c; reflexivity.
Qed.

(* ── 7. Positions and recognition ──────────────────────────────────────── *)

(* Inert positions (design section 4.4). The runtime never decodes a term
   in these positions. *)
Inductive inert_position : Type :=
| InertSendChannel
| InertSendDataNotLast
| InertBindPattern
| InertGuard
| InertCollectionElement
| InertMethodArgument
| InertIfCondition
| InertMatchTarget
| InertNonSendProduce.

(* PosProcessExpr: an element of Par.exprs in a Par under evaluation as a
   process (hooks H1 and H2). PosSendLastData: the last evaluated and
   substituted data element of a send (H3). PosBindSource: the evaluated and
   substituted source of a receive bind, before single_bundle (H4).
   PosFactoryTheory: the theory argument that a P4 factory reads. *)
Inductive position : Type :=
| PosProcessExpr
| PosSendLastData
| PosBindSource
| PosFactoryTheory
| PosInert : inert_position -> position.

Definition position_accepts_marker (pos : position) (m : marker) : bool :=
  match pos, m with
  | PosProcessExpr, MUse => true
  | PosProcessExpr, MNewSpace => true
  | PosSendLastData, MParams => true
  | PosBindSource, MQuery => true
  | PosFactoryTheory, MFree => true
  | _, _ => false
  end.

Definition allowed (pos : position) (c : construct) : Prop :=
  position_accepts_marker pos (construct_marker c) = true.

(* RInert: HEAD behavior applies. RConstruct: the hook gets the construct.
   RMalformed: the hook fails closed with a typed error. *)
Inductive recognition : Type :=
| RInert : recognition
| RConstruct : construct -> recognition
| RMalformed : recognition.

(* A hook looks only at the head of the term in its position. A matching
   head commits the hook to decode the term. *)
Definition recognize (pos : position) (t : par) : recognition :=
  match head_marker t with
  | Some m =>
      if position_accepts_marker pos m then
        match decode t with
        | Some c => RConstruct c
        | None => RMalformed
        end
      else RInert
  | None => RInert
  end.

(* H3: split_trailing_params examines only the last data element. *)
Definition split_trailing_params (data : list par) : recognition * list par :=
  match rev data with
  | [] => (RInert, data)
  | last :: earlier =>
      match recognize PosSendLastData last with
      | RInert => (RInert, data)
      | outcome => (outcome, rev earlier)
      end
  end.

(* H1: the HEAD filter keeps EVar and EMethod expressions
   (util/mod.rs:56-64, 100 and 145). *)
Definition head_process_exprb (e : par) : bool :=
  match e with
  | PVar _ => true
  | PMethod _ _ _ => true
  | _ => false
  end.

(* H1: is_process_marker_tuple accepts a tuple headed by USE or NEWSPACE. *)
Definition process_marker_tupleb (e : par) : bool :=
  match head_marker e with
  | Some m => position_accepts_marker PosProcessExpr m
  | None => false
  end.

(* H1: the shared predicate is_evaluated_process_expr. *)
Definition evaluated_process_exprb (e : par) : bool :=
  head_process_exprb e || process_marker_tupleb e.

Lemma recognize_process_inert_iff :
  forall t, recognize PosProcessExpr t = RInert <-> process_marker_tupleb t = false.
Proof.
  intros t. unfold recognize, process_marker_tupleb.
  destruct (head_marker t) as [m |]; [| split; reflexivity].
  destruct (position_accepts_marker PosProcessExpr m); [| split; reflexivity].
  destruct (decode t); split; intro H; discriminate.
Qed.

(* Headline 7. Each well-formed construct in its own position is
   recognized as itself. *)
Theorem recognize_complete :
  forall pos c, wf c -> allowed pos c -> recognize pos (encode c) = RConstruct c.
Proof.
  intros pos c Hwf Hallowed. unfold recognize. rewrite head_marker_encode.
  unfold allowed in Hallowed. rewrite Hallowed, (decode_encode c Hwf). reflexivity.
Qed.

(* Headline 8. A recognized term is the encoding of a well-formed construct
   that its position allows. The reducer is a function of the term, so a
   forged term behaves as the syntax that it equals. *)
Theorem marker_forging_grants_no_capability :
  forall pos t c, recognize pos t = RConstruct c -> t = encode c /\ wf c /\ allowed pos c.
Proof.
  intros pos t c H. unfold recognize in H.
  destruct (head_marker t) as [m |] eqn:Hm; [| discriminate].
  destruct (position_accepts_marker pos m) eqn:Hacc; [| discriminate].
  destruct (decode t) as [c' |] eqn:Hd; [| discriminate].
  injection H as <-.
  apply decode_sound in Hd as [Ht Hwf].
  split; [exact Ht | split; [exact Hwf |]].
  rewrite Ht, head_marker_encode in Hm. injection Hm as Hm.
  unfold allowed. rewrite Hm. exact Hacc.
Qed.

Corollary forged_term_indistinguishable :
  forall (A : Type) (observe : position -> par -> A) pos t c,
    recognize pos t = RConstruct c -> observe pos t = observe pos (encode c).
Proof.
  intros A observe pos t c H.
  apply marker_forging_grants_no_capability in H as [-> _]. reflexivity.
Qed.

(* A marker head in an accepting position with a malformed body fails
   closed. *)
Lemma recognize_fails_closed :
  forall pos t m,
    head_marker t = Some m -> position_accepts_marker pos m = true -> decode t = None ->
    recognize pos t = RMalformed.
Proof.
  intros pos t m Hm Hacc Hd. unfold recognize. rewrite Hm, Hacc, Hd. reflexivity.
Qed.

(* Headline 9. A marker tuple outside its recognized position is inert
   data. Inert positions never decode. A construct in a position that does
   not allow it is inert. A term without a marker head is inert, so a marker
   tuple inside a list, a map or another tuple is inert. In process
   position, the H1 filter drops a PARAMS, QUERY or FREE tuple, as HEAD
   drops every other expression. *)
Theorem marker_inert_elsewhere :
  (forall (k : inert_position) (t : par), recognize (PosInert k) t = RInert) /\
  (forall (pos : position) (c : construct), ~ allowed pos c -> recognize pos (encode c) = RInert) /\
  (forall (pos : position) (t : par), head_marker t = None -> recognize pos t = RInert) /\
  (forall (pos : position) (ps : list par), recognize pos (PList ps) = RInert) /\
  (forall (pos : position) (kvs : list (bytes * par)), recognize pos (PMap kvs) = RInert) /\
  (forall c : construct, ~ allowed PosProcessExpr c -> evaluated_process_exprb (encode c) = false).
Proof.
  split.
  { intros k t. unfold recognize. destruct (head_marker t); reflexivity. }
  split.
  { intros pos c Hnot. unfold recognize. rewrite head_marker_encode.
    unfold allowed in Hnot.
    destruct (position_accepts_marker pos (construct_marker c));
      [contradiction Hnot; reflexivity | reflexivity]. }
  split.
  { intros pos t Hnone. unfold recognize. rewrite Hnone. reflexivity. }
  split; [intros; reflexivity |].
  split; [intros; reflexivity |].
  intros c Hnot. unfold allowed in Hnot. unfold evaluated_process_exprb, process_marker_tupleb.
  rewrite head_marker_encode.
  destruct (position_accepts_marker PosProcessExpr (construct_marker c));
    [contradiction Hnot; reflexivity |].
  destruct c; reflexivity.
Qed.

(* H3 round trip: the send hook finds exactly the params that the builder
   appended, and it returns the original data. *)
Lemma send_params_roundtrip :
  forall data positional named c,
    forallb is_identifier (map fst named) = true ->
    build_params positional named = Some c ->
    split_trailing_params (send_data data positional named) = (RConstruct c, data).
Proof.
  intros data positional named c Hid Hb.
  pose proof (build_params_wf _ _ _ Hid Hb) as Hwf.
  assert (Hallowed : allowed PosSendLastData c).
  { unfold build_params in Hb. destruct (is_nil positional && is_nil named); [discriminate |].
    injection Hb as <-. reflexivity. }
  unfold send_data. rewrite Hb. unfold split_trailing_params.
  rewrite rev_unit, (recognize_complete _ _ Hwf Hallowed), rev_involutive. reflexivity.
Qed.

(* ── 8. The term schedule (hook H1) ────────────────────────────────────── *)

Inductive generated_message : Type :=
| GMSend : par -> generated_message
| GMReceive : par -> generated_message
| GMNew : par -> generated_message
| GMMatch : par -> generated_message
| GMIf : par -> generated_message
| GMBundle : par -> generated_message
| GMExpr : par -> generated_message
| GMCostSignedTerm : par -> generated_message
| GMCostStack : par -> generated_message.

(* A Par in process position. The model represents each component that is
   not an expression by an opaque par. The schedule never inspects such a
   component. *)
Record process_par : Type := mkProcessPar {
  pp_sends : list par;
  pp_receives : list par;
  pp_news : list par;
  pp_matches : list par;
  pp_conditionals : list par;
  pp_bundles : list par;
  pp_exprs : list par;
  pp_cost_signed_terms : list par;
  pp_cost_stacks : list par
}.

Definition schedule_prefix (pp : process_par) : list generated_message :=
  map GMSend (pp_sends pp) ++ map GMReceive (pp_receives pp) ++ map GMNew (pp_news pp)
  ++ map GMMatch (pp_matches pp) ++ map GMIf (pp_conditionals pp)
  ++ map GMBundle (pp_bundles pp).

Definition schedule_suffix (pp : process_par) : list generated_message :=
  map GMCostSignedTerm (pp_cost_signed_terms pp) ++ map GMCostStack (pp_cost_stacks pp).

(* evaluation_terms and owned_evaluation_terms build the same list in this
   order (util/mod.rs:66-118 and 120-160). *)
Definition evaluation_terms_with (keep : par -> bool) (pp : process_par)
  : list generated_message :=
  schedule_prefix pp ++ map GMExpr (filter keep (pp_exprs pp)) ++ schedule_suffix pp.

Definition evaluation_terms_head (pp : process_par) : list generated_message :=
  evaluation_terms_with head_process_exprb pp.

Definition evaluation_terms_reified (pp : process_par) : list generated_message :=
  evaluation_terms_with evaluated_process_exprb pp.

(* marker_freeb checks every subterm that the model represents. It accepts
   every opaque term. A Rust Par without a marker thus satisfies it. *)
Fixpoint marker_freeb (t : par) : bool :=
  match t with
  | PPriv id => match marker_of_id id with Some _ => false | None => true end
  | PStr _ => true
  | PList ps => forallb marker_freeb ps
  | PTuple ps => forallb marker_freeb ps
  | PMap kvs => forallb (fun kv => marker_freeb (snd kv)) kvs
  | PVar _ => true
  | PMethod _ target args => marker_freeb target && forallb marker_freeb args
  | PNew _ _ _ body => marker_freeb body
  | PNil => true
  | POther _ => true
  end.

Definition process_par_marker_free (pp : process_par) : Prop :=
  Forall (fun t => marker_freeb t = true)
    (pp_sends pp ++ pp_receives pp ++ pp_news pp ++ pp_matches pp ++ pp_conditionals pp
     ++ pp_bundles pp ++ pp_exprs pp ++ pp_cost_signed_terms pp ++ pp_cost_stacks pp).

Lemma marker_free_not_process_marker :
  forall e, marker_freeb e = true -> process_marker_tupleb e = false.
Proof.
  intros e H. destruct e as [| | | ps | | | | | |]; try reflexivity.
  destruct ps as [| hd ps]; [reflexivity |].
  destruct hd as [id | | | | | | | | |]; try reflexivity.
  unfold process_marker_tupleb, head_marker.
  simpl in H. destruct (marker_of_id id); [discriminate | reflexivity].
Qed.

Lemma head_process_not_marker :
  forall e, head_process_exprb e = true -> process_marker_tupleb e = false.
Proof. intros [] H; try discriminate; reflexivity. Qed.

Lemma filter_or_disjoint_length :
  forall (A : Type) (f g : A -> bool) (l : list A),
    (forall x, In x l -> f x = true -> g x = false) ->
    length (filter (fun x => f x || g x) l) = length (filter f l) + length (filter g l).
Proof.
  intros A f g l. induction l as [| x l IH]; intros Hdisj; [reflexivity |].
  simpl. assert (IH' := IH (fun y Hy => Hdisj y (or_intror Hy))).
  destruct (f x) eqn:Ef; simpl.
  - rewrite (Hdisj x (or_introl eq_refl) Ef). simpl. rewrite IH'. reflexivity.
  - destruct (g x); simpl; rewrite IH'; lia.
Qed.

Lemma filter_all_false :
  forall (A : Type) (f : A -> bool) (l : list A),
    (forall x, In x l -> f x = false) -> filter f l = [].
Proof.
  intros A f l H. induction l as [| x l IH]; [reflexivity |].
  simpl. rewrite (H x (or_introl eq_refl)).
  apply IH. intros y Hy. apply H. right. exact Hy.
Qed.

(* The two filters select the same terms exactly when no expression of the
   Par is a USE or NEWSPACE tuple. *)
Lemma evaluation_terms_eq_iff :
  forall pp,
    evaluation_terms_reified pp = evaluation_terms_head pp <->
    (forall e, In e (pp_exprs pp) -> process_marker_tupleb e = false).
Proof.
  intros pp. unfold evaluation_terms_reified, evaluation_terms_head, evaluation_terms_with.
  split.
  - intros Heq e Hin.
    apply app_inv_head in Heq. apply app_inv_tail in Heq.
    apply (f_equal (@length generated_message)) in Heq.
    rewrite !length_map in Heq.
    unfold evaluated_process_exprb in Heq.
    rewrite filter_or_disjoint_length in Heq
      by (intros x _ Hx; apply head_process_not_marker; exact Hx).
    destruct (process_marker_tupleb e) eqn:Ee; [| reflexivity].
    assert (Hpos : In e (filter process_marker_tupleb (pp_exprs pp)))
      by (apply filter_In; split; assumption).
    destruct (filter process_marker_tupleb (pp_exprs pp)) as [| x rest];
      [contradiction | simpl in Heq; lia].
  - intros Hnone. f_equal. f_equal. f_equal.
    apply filter_ext_in. intros e Hin. unfold evaluated_process_exprb.
    rewrite (Hnone e Hin), orb_false_r. reflexivity.
Qed.

Definition i16_max : nat := 2 ^ 15 - 1.

(* evaluation_random (util/mod.rs:162-181). An index is always below the
   term count. The casts `index as u8` and `index as u16 as i16` thus do not
   change it, and the model passes the index unchanged. *)
Definition evaluation_random {R : Type} (split_byte split_short : R -> nat -> R)
  (r : R) (index term_count : nat) : option R :=
  if (i16_max <? term_count) || (term_count <=? index) then None
  else if term_count =? 1 then Some r
  else if 256 <? term_count then Some (split_short r index)
  else Some (split_byte r index).

(* eval_inner gives term i the random state evaluation_random r i n, where n
   is the length of the term list (reduce.rs:367-430). The partition of the
   cost stacks keeps the original indices. *)
Definition term_schedule {R : Type} (split_byte split_short : R -> nat -> R) (r : R)
  (terms : list generated_message) : list (generated_message * option R) :=
  map (fun it => (snd it, evaluation_random split_byte split_short r (fst it) (length terms)))
    (combine (seq 0 (length terms)) terms).

(* Headline 10. For a marker-free Par, the shared predicate selects the same
   terms as the HEAD filter. The indices, the term count and the random
   states of all terms therefore stay as at HEAD. *)
Theorem schedule_unchanged_marker_free :
  forall pp : process_par,
    process_par_marker_free pp ->
    evaluation_terms_reified pp = evaluation_terms_head pp /\
    (forall (R : Type) (split_byte split_short : R -> nat -> R) (r : R),
        term_schedule split_byte split_short r (evaluation_terms_reified pp) =
        term_schedule split_byte split_short r (evaluation_terms_head pp)).
Proof.
  intros pp Hfree.
  assert (Heq : evaluation_terms_reified pp = evaluation_terms_head pp).
  { apply evaluation_terms_eq_iff. intros e Hin.
    apply marker_free_not_process_marker.
    unfold process_par_marker_free in Hfree. rewrite Forall_forall in Hfree.
    apply Hfree. rewrite !in_app_iff. tauto. }
  split; [exact Heq |]. intros R split_byte split_short r. rewrite Heq. reflexivity.
Qed.

(* Finding H-1. accounting/lexical.rs takes its term count from
   evaluation_terms (lexical.rs:44) but counts the expressions with the HEAD
   filter (lexical.rs:105-114). The two counts agree exactly when the Par has
   no USE or NEWSPACE tuple. Otherwise lexical.rs:125-129 fails closed with
   BugFoundError. *)
Lemma lexical_schedule_count_iff :
  forall pp,
    length (evaluation_terms_reified pp) = length (evaluation_terms_head pp) <->
    (forall e, In e (pp_exprs pp) -> process_marker_tupleb e = false).
Proof.
  intros pp. unfold evaluation_terms_reified, evaluation_terms_head, evaluation_terms_with.
  rewrite !length_app, !length_map.
  unfold evaluated_process_exprb.
  rewrite filter_or_disjoint_length by (intros x _ Hx; apply head_process_not_marker; exact Hx).
  split.
  - intros Hlen e Hin. destruct (process_marker_tupleb e) eqn:Ee; [| reflexivity].
    assert (Hpos : In e (filter process_marker_tupleb (pp_exprs pp)))
      by (apply filter_In; split; assumption).
    destruct (filter process_marker_tupleb (pp_exprs pp)) as [| x rest];
      [contradiction | simpl in Hlen; lia].
  - intros Hnone.
    rewrite (filter_all_false _ _ (pp_exprs pp) Hnone). simpl. lia.
Qed.

(* ── 8b. The derived connective_used flags of a marker tuple (I-20) ─── *)

(* A value of the program: its model term and its own connective_used
   flag. No hook changes a value. *)
Definition fval : Type := par * bool.

(* A construct whose values carry their flags. *)
Inductive fconstruct : Type :=
| FCUse : fval -> fval -> fconstruct
| FCNewSpace : list (option fval) -> nat -> list bytes -> nat -> fval -> fconstruct
| FCParams : list fval -> list (bytes * fval) -> fconstruct
| FCQuery : fval -> list fval -> list (bytes * fval) -> fconstruct
| FCFree : bytes -> list fval -> fconstruct.

Definition erase_kv (kv : bytes * fval) : bytes * par := (fst kv, fst (snd kv)).

Definition erase_c (fc : fconstruct) : construct :=
  match fc with
  | FCUse s b => CUse (fst s) (fst b)
  | FCNewSpace anns k uris inj body => CNewSpace (map (option_map fst) anns) k uris inj (fst body)
  | FCParams pos named => CParams (map fst pos) (map erase_kv named)
  | FCQuery ch pay named => CQuery (fst ch) (map fst pay) (map erase_kv named)
  | FCFree th args => CFree th (map fst args)
  end.

(* The flags of the fields after the head, as the Rust Par holds them
   (enum Field of reducer_hooks.rs). A list or a map has the flag of its
   Par and the flag of its expression. The NEWSPACE annotations have these
   two flags and the two flags of each entry [] or [s]. Par{news: [New]}
   has one flag. A value field has no derived flag. *)
Inductive field_flags : Type :=
| FFValue : field_flags
| FFList : bool -> bool -> field_flags
| FFMap : bool -> bool -> field_flags
| FFAnnotations : bool -> bool -> list (bool * bool) -> field_flags
| FFNewBody : bool -> field_flags.

(* The flag of the holder Par, the flag of the tuple and the flags of the
   fields. The head is the marker name, whose flag is false. *)
Record tuple_flags : Type := mkFlags { tf_par : bool; tf_tuple : bool; tf_fields : list field_flags }.

(* The top level of a marker tuple in a Rust Par: the construct with its
   values, and the container flags as HEAD left them. *)
Record ftuple : Type := mkFTuple { ft_construct : fconstruct; ft_flags : tuple_flags }.

Definition vflag (v : fval) : bool := snd v.
Definition entry_flag (a : option fval) : bool := match a with Some v => vflag v | None => false end.
Definition list_flag (vs : list fval) : bool := existsb vflag vs.
Definition map_flag (kvs : list (bytes * fval)) : bool := existsb (fun kv => vflag (snd kv)) kvs.

(* The flags that the canonical encoder derives: each container takes the
   disjunction over its children (encoding.rs bits_of, list_par, map_par,
   new_par, annotation_par, marker_tuple and tuple_par). *)
Definition derived_flags (fc : fconstruct) : tuple_flags :=
  match fc with
  | FCUse s b =>
      let t := vflag s || vflag b in
      mkFlags t t [FFValue; FFValue]
  | FCNewSpace anns _ _ _ body =>
      let a := existsb entry_flag anns in
      let t := a || vflag body in
      mkFlags t t [FFAnnotations a a (map (fun x => (entry_flag x, entry_flag x)) anns);
                   FFNewBody (vflag body)]
  | FCParams pos named =>
      let l := list_flag pos in
      let m := map_flag named in
      let t := l || m in
      mkFlags t t [FFList l l; FFMap m m]
  | FCQuery ch pay named =>
      let l := list_flag pay in
      let m := map_flag named in
      let t := vflag ch || l || m in
      mkFlags t t [FFValue; FFList l l; FFMap m m]
  | FCFree _ args =>
      let l := list_flag args in
      mkFlags l l [FFValue; FFList l l]
  end.

Definition bool_pair_eqb (x y : bool * bool) : bool := Bool.eqb (fst x) (fst y) && Bool.eqb (snd x) (snd y).

Definition field_flags_eqb (a b : field_flags) : bool :=
  match a, b with
  | FFValue, FFValue => true
  | FFList p e, FFList p' e' => Bool.eqb p p' && Bool.eqb e e'
  | FFMap p e, FFMap p' e' => Bool.eqb p p' && Bool.eqb e e'
  | FFAnnotations p e es, FFAnnotations p' e' es' =>
      Bool.eqb p p' && Bool.eqb e e' && list_eqb bool_pair_eqb es es'
  | FFNewBody p, FFNewBody p' => Bool.eqb p p'
  | _, _ => false
  end.

Definition tuple_flags_eqb (a b : tuple_flags) : bool :=
  Bool.eqb (tf_par a) (tf_par b) && Bool.eqb (tf_tuple a) (tf_tuple b) &&
  list_eqb field_flags_eqb (tf_fields a) (tf_fields b).

Lemma bool_pair_eqb_refl : forall x, bool_pair_eqb x x = true.
Proof. intros [a b]. unfold bool_pair_eqb. cbn. rewrite !eqb_reflx. reflexivity. Qed.

Lemma bool_pair_eqb_sound : forall x y, bool_pair_eqb x y = true -> x = y.
Proof.
  intros [a b] [c d] H. unfold bool_pair_eqb in H. cbn in H.
  apply andb_true_iff in H as [H1 H2]. apply eqb_prop in H1. apply eqb_prop in H2. subst. reflexivity.
Qed.

Lemma field_flags_eqb_refl : forall a, field_flags_eqb a a = true.
Proof.
  intros [| p e | p e | p e es | p]; cbn; rewrite ?eqb_reflx; try reflexivity.
  apply list_eqb_refl. apply Forall_forall. intros x _. apply bool_pair_eqb_refl.
Qed.

Lemma field_flags_eqb_sound : forall a b, field_flags_eqb a b = true -> a = b.
Proof.
  intros [| p e | p e | p e es | p] [| p' e' | p' e' | p' e' es' | p'] H; cbn in H; try discriminate H.
  - reflexivity.
  - apply andb_true_iff in H as [H1 H2]. apply eqb_prop in H1. apply eqb_prop in H2. subst. reflexivity.
  - apply andb_true_iff in H as [H1 H2]. apply eqb_prop in H1. apply eqb_prop in H2. subst. reflexivity.
  - apply andb_true_iff in H as [H H3]. apply andb_true_iff in H as [H1 H2].
    apply eqb_prop in H1. apply eqb_prop in H2. subst.
    apply (list_eqb_sound _ bool_pair_eqb) in H3; [subst; reflexivity |].
    apply Forall_forall. intros x _ y. apply bool_pair_eqb_sound.
  - apply eqb_prop in H. subst. reflexivity.
Qed.

Lemma tuple_flags_eqb_refl : forall a, tuple_flags_eqb a a = true.
Proof.
  intros [p t fs]. unfold tuple_flags_eqb. cbn. rewrite !eqb_reflx. cbn.
  apply list_eqb_refl. apply Forall_forall. intros x _. apply field_flags_eqb_refl.
Qed.

Lemma tuple_flags_eqb_sound : forall a b, tuple_flags_eqb a b = true -> a = b.
Proof.
  intros [p t fs] [p' t' fs'] H. unfold tuple_flags_eqb in H. cbn in H.
  apply andb_true_iff in H as [H H3]. apply andb_true_iff in H as [H1 H2].
  apply eqb_prop in H1. apply eqb_prop in H2. subst.
  apply (list_eqb_sound _ field_flags_eqb) in H3; [subst; reflexivity |].
  apply Forall_forall. intros x _ y. apply field_flags_eqb_sound.
Qed.

(* The model term of the tuple: α forgets the flags. *)
Definition ferase (x : ftuple) : par := encode (erase_c (ft_construct x)).

(* refresh_tuple_par of reducer_hooks.rs: every derived flag takes its
   derived value, and the values keep theirs. *)
Definition refresh (x : ftuple) : ftuple := mkFTuple (ft_construct x) (derived_flags (ft_construct x)).

(* The strict decoders of encoding.rs accept only the canonical image of
   their encoder, flags included. The values of the image are the values
   of the input, so only the container flags can differ. *)
Definition sdecode (x : ftuple) : option construct :=
  if tuple_flags_eqb (ft_flags x) (derived_flags (ft_construct x)) then decode (ferase x) else None.

(* Headline 13. After the refresh, the strict decode of a marker tuple is
   the decode of this module on its model term, so headlines 1 to 12 hold
   for the refreshed decode. The refresh changes no value. The strict
   decoder still accepts only derived flags: a tuple that it accepts is
   already refreshed, and its term is the encoding of the result. *)
Theorem refresh_strict_decode_erase :
  forall x,
    sdecode (refresh x) = decode (ferase x) /\
    ferase (refresh x) = ferase x /\
    (forall c, sdecode x = Some c -> refresh x = x /\ ferase x = encode c /\ wf c).
Proof.
  intros [fc fl]. unfold sdecode, refresh, ferase. cbn [ft_construct ft_flags].
  split; [rewrite tuple_flags_eqb_refl; reflexivity |].
  split; [reflexivity |].
  intros c H. destruct (tuple_flags_eqb fl (derived_flags fc)) eqn:E; [| discriminate H].
  apply tuple_flags_eqb_sound in E. subst fl.
  apply decode_sound in H as [Ht Hwf]. split; [reflexivity | split; [exact Ht | exact Hwf]].
Qed.

(* Headline 14. A canonical marker tuple is recognized whatever its derived
   flags are. A use block whose body uses a connective (x!(Int)) gives a
   tuple with the flag true, and the hooks recognize it. With a stale flag
   the strict decoder rejects it, and after the refresh it accepts it. *)
Theorem connective_marker_tuple_recognized :
  (forall fc, wf (erase_c fc) -> sdecode (mkFTuple fc (derived_flags fc)) = Some (erase_c fc)) /\
  (forall space body fs,
     tf_par (derived_flags (FCUse (space, fs) (body, true))) = true /\
     tf_tuple (derived_flags (FCUse (space, fs) (body, true))) = true /\
     sdecode (mkFTuple (FCUse (space, fs) (body, true)) (derived_flags (FCUse (space, fs) (body, true)))) =
     Some (CUse space body)) /\
  (forall space body fs,
     sdecode (mkFTuple (FCUse (space, fs) (body, true)) (mkFlags false false [FFValue; FFValue])) = None /\
     sdecode (refresh (mkFTuple (FCUse (space, fs) (body, true)) (mkFlags false false [FFValue; FFValue]))) =
     Some (CUse space body)).
Proof.
  split; [| split].
  - intros fc Hwf. unfold sdecode, ferase. cbn [ft_construct ft_flags].
    rewrite tuple_flags_eqb_refl. apply decode_encode. exact Hwf.
  - intros space body fs.
    destruct fs; (split; [reflexivity | split; [reflexivity |]]);
      unfold sdecode, ferase; cbn [ft_construct ft_flags]; rewrite tuple_flags_eqb_refl;
      apply decode_encode; exact I.
  - intros space body fs. split.
    + destruct fs; reflexivity.
    + rewrite (proj1 (refresh_strict_decode_erase _)). unfold ferase. cbn [ft_construct erase_c fst].
      apply decode_encode. exact I.
Qed.

(* ── 9. Substitution ───────────────────────────────────────────────────── *)

Section Substitution.
  (* subst sigma t models Substitute::substitute_and_charge at depth 0 with
     the environment sigma (substitute.rs:102-131). shift k sigma models
     env.shift(k). Each premise below states one equation of HEAD
     substitution on a shape that the encoding uses. The equations hold up
     to the connective_used flags, which α forgets: HEAD keeps the flags of
     a substituted Par, tuple, list and map (substitute.rs:527, 997-1085,
     1342-1430), so a substituted marker tuple can carry stale flags. The
     hooks recompute the derived flags before the strict decode (I-20), so
     the premises need no flag. Headline 15 states the flagged form.
     The storage substitution of a receive body (reduce.rs:1420-1424) uses
     substitute_no_sort_and_charge. That variant satisfies the same premises
     on USE and NEWSPACE tuples, which contain no map. It also sorts the map
     values in create_from_vec. The hooks decode a PARAMS or QUERY map only
     after the sorting variant (reduce.rs:1305 and 1655). *)
  Variable Env : Type.
  Variable subst : Env -> par -> par.
  Variable shift : nat -> Env -> Env.

  (* substitute.rs:522 copies the unforgeables. *)
  Hypothesis subst_priv : forall sigma id, subst sigma (PPriv id) = PPriv id.
  (* substitute.rs:1457 returns a ground expression unchanged. *)
  Hypothesis subst_str : forall sigma s, subst sigma (PStr s) = PStr s.
  (* substitute.rs:1342-1363 substitutes each list element. *)
  Hypothesis subst_list :
    forall sigma ps, subst sigma (PList ps) = PList (map (subst sigma) ps).
  (* substitute.rs:1365-1384 substitutes each tuple element in place. *)
  Hypothesis subst_tuple :
    forall sigma ps, subst sigma (PTuple ps) = PTuple (map (subst sigma) ps).
  (* substitute.rs:1407-1430 substitutes each value and re-sorts the map. *)
  Hypothesis subst_map :
    forall sigma kvs, subst sigma (PMap kvs) = PMap (canon_map (map_values (subst sigma) kvs)).
  (* substitute.rs:646-665 substitutes the body under env.shift(bind_count). *)
  Hypothesis subst_new :
    forall sigma k uris injections body,
      subst sigma (PNew k uris injections body) =
      PNew k uris injections (subst (shift k sigma) body).

  (* The annotations stay in the outer scope (design section 1.4). Only the
     body of the New moves under the binders. *)
  Definition subst_construct (sigma : Env) (c : construct) : construct :=
    match c with
    | CUse space body => CUse (subst sigma space) (subst sigma body)
    | CNewSpace annotations k uris injections body =>
        CNewSpace (map (option_map (subst sigma)) annotations) k uris injections
          (subst (shift k sigma) body)
    | CParams positional named =>
        CParams (map (subst sigma) positional) (map_values (subst sigma) named)
    | CQuery channel payload named =>
        CQuery (subst sigma channel) (map (subst sigma) payload)
          (map_values (subst sigma) named)
    | CFree theory args => CFree theory (map (subst sigma) args)
    end.

  Lemma map_nonempty :
    forall (A B : Type) (f : A -> B) (l : list A), l <> [] -> map f l <> [].
  Proof. intros A B f [| x l] H; [contradiction | discriminate]. Qed.

  Lemma map_values_nonempty :
    forall f kvs, kvs <> [] -> map_values f kvs <> [].
  Proof. intros f [| kv kvs] H; [contradiction | discriminate]. Qed.

  Lemma named_params_ok_map_values :
    forall f named, named_params_ok (map_values f named) = named_params_ok named.
  Proof.
    intros f named. unfold named_params_ok.
    rewrite keys_strictly_sorted_map_values, map_values_fst. reflexivity.
  Qed.

  Lemma existsb_is_annotated_map :
    forall f annotations,
      existsb is_annotated (map (option_map f) annotations) = existsb is_annotated annotations.
  Proof.
    intros f annotations.
    induction annotations as [| [a |] rest IH]; simpl; [reflexivity | reflexivity | exact IH].
  Qed.

  (* Substitution keeps every construct well formed. With the [] or [s]
     annotation form, the count of annotations does not depend on the value
     of s. *)
  Lemma subst_construct_wf : forall sigma c, wf c -> wf (subst_construct sigma c).
  Proof.
    intros sigma [s p | a k u i b | pos named | ch q named | th args] Hwf; simpl in *.
    - exact I.
    - destruct Hwf as [Hk Hann]. split.
      + rewrite length_map. exact Hk.
      + rewrite existsb_is_annotated_map. exact Hann.
    - destruct Hwf as [Hne Hok]. split.
      + destruct Hne as [Hne | Hne];
          [left; apply map_nonempty | right; apply map_values_nonempty]; exact Hne.
      + rewrite named_params_ok_map_values. exact Hok.
    - destruct Hwf as [Hne Hok]. split.
      + destruct Hne as [Hne | Hne];
          [left; apply map_nonempty | right; apply map_values_nonempty]; exact Hne.
      + rewrite named_params_ok_map_values. exact Hok.
    - exact Hwf.
  Qed.

  Lemma subst_annotations :
    forall sigma annotations,
      map (subst sigma) (map encode_annotation annotations) =
      map encode_annotation (map (option_map (subst sigma)) annotations).
  Proof.
    intros sigma annotations.
    induction annotations as [| [a |] rest IH]; simpl; [reflexivity | |];
      rewrite IH, subst_list; reflexivity.
  Qed.

  Lemma subst_canonical_map :
    forall sigma named,
      named_params_ok named = true ->
      subst sigma (PMap named) = PMap (map_values (subst sigma) named).
  Proof.
    intros sigma named Hok. rewrite subst_map. f_equal. apply canon_map_id.
    rewrite keys_strictly_sorted_map_values.
    unfold named_params_ok in Hok. apply andb_true_iff in Hok as [Hs _]. exact Hs.
  Qed.

  (* This lemma does not use subst_new. Evaluation (eval_expr,
     reduce.rs:3119-3209 and 7805-7824) satisfies the five equations that the
     proof uses. Thus PARAMS, QUERY, USE and FREE decoding also commutes with
     evaluation. *)
  Lemma subst_encode_except_newspace :
    forall sigma c,
      wf c -> construct_marker c <> MNewSpace ->
      subst sigma (encode c) = encode (subst_construct sigma c).
  Proof.
    intros sigma [s p | a k u i b | pos named | ch q named | th args] Hwf Hm;
      [| contradiction Hm; reflexivity | | |];
      simpl; rewrite subst_tuple; simpl; unfold marker_par; rewrite subst_priv.
    - reflexivity.
    - destruct Hwf as [_ Hok]. rewrite subst_list, (subst_canonical_map _ _ Hok). reflexivity.
    - destruct Hwf as [_ Hok]. rewrite subst_list, (subst_canonical_map _ _ Hok). reflexivity.
    - rewrite subst_str, subst_list. reflexivity.
  Qed.

  Lemma subst_encode :
    forall sigma c, wf c -> subst sigma (encode c) = encode (subst_construct sigma c).
  Proof.
    intros sigma c Hwf.
    destruct c as [s p | a k u i b | pos named | ch q named | th args];
      try (apply subst_encode_except_newspace; [exact Hwf | discriminate]).
    simpl. rewrite subst_tuple. simpl. unfold marker_par.
    rewrite subst_priv, subst_list, subst_new, subst_annotations. reflexivity.
  Qed.

  Lemma decode_commutes_except_newspace :
    forall sigma t c,
      decode t = Some c -> construct_marker c <> MNewSpace ->
      decode (subst sigma t) = Some (subst_construct sigma c).
  Proof.
    intros sigma t c H Hm.
    apply decode_sound in H as [-> Hwf].
    rewrite (subst_encode_except_newspace sigma c Hwf Hm).
    apply decode_encode. apply subst_construct_wf. exact Hwf.
  Qed.

  (* Headline 11. Substitution maps a decodable term to the encoding of the
     substituted construct. *)
  Theorem decode_commutes_with_subst :
    forall sigma t c, decode t = Some c -> decode (subst sigma t) = Some (subst_construct sigma c).
  Proof.
    intros sigma t c H.
    apply decode_sound in H as [-> Hwf].
    rewrite (subst_encode sigma c Hwf).
    apply decode_encode. apply subst_construct_wf. exact Hwf.
  Qed.

  (* The flag of a value after the substitution. HEAD derives it for the
     value. The theorem below holds for every flag function. *)
  Variable subst_flag : Env -> par -> bool -> bool.

  Definition fsubst_val (sigma : Env) (v : fval) : fval :=
    (subst sigma (fst v), subst_flag sigma (fst v) (snd v)).
  Definition fsubst_kv (sigma : Env) (kv : bytes * fval) : bytes * fval := (fst kv, fsubst_val sigma (snd kv)).

  Definition fsubst_c (sigma : Env) (fc : fconstruct) : fconstruct :=
    match fc with
    | FCUse s b => FCUse (fsubst_val sigma s) (fsubst_val sigma b)
    | FCNewSpace anns k uris inj body =>
        FCNewSpace (map (option_map (fsubst_val sigma)) anns) k uris inj (fsubst_val (shift k sigma) body)
    | FCParams pos named => FCParams (map (fsubst_val sigma) pos) (map (fsubst_kv sigma) named)
    | FCQuery ch pay named =>
        FCQuery (fsubst_val sigma ch) (map (fsubst_val sigma) pay) (map (fsubst_kv sigma) named)
    | FCFree th args => FCFree th (map (fsubst_val sigma) args)
    end.

  (* HEAD substitution and evaluation keep the container flags of the
     tuple (substitute.rs:997-1085, reduce.rs:3119-3209), so they can go
     stale. *)
  Definition fsubst (sigma : Env) (x : ftuple) : ftuple := mkFTuple (fsubst_c sigma (ft_construct x)) (ft_flags x).

  Lemma erase_fsubst_c : forall sigma fc, erase_c (fsubst_c sigma fc) = subst_construct sigma (erase_c fc).
  Proof.
    intros sigma [s b | anns k uris inj body | pos named | ch pay named | th args]; cbn [fsubst_c erase_c subst_construct].
    - reflexivity.
    - f_equal. rewrite !map_map. apply map_ext. intros [a |]; reflexivity.
    - unfold map_values. rewrite !map_map. reflexivity.
    - unfold map_values. rewrite !map_map. reflexivity.
    - rewrite !map_map. reflexivity.
  Qed.

  (* Headline 15. HEAD substitution of a decodable marker tuple keeps its
     container flags. The model term of the result is the substitution of
     the model term, and after the refresh the strict decode gives the
     substituted construct. *)
  Theorem refreshed_decode_commutes_with_subst :
    forall sigma x c,
      sdecode x = Some c ->
      ferase (fsubst sigma x) = subst sigma (ferase x) /\
      sdecode (refresh (fsubst sigma x)) = Some (subst_construct sigma c).
  Proof.
    intros sigma x c H.
    destruct (proj2 (proj2 (refresh_strict_decode_erase x)) c H) as [_ [Hx Hwf]].
    assert (Hc : erase_c (ft_construct x) = c) by (apply encode_injective; exact Hx).
    assert (He : ferase (fsubst sigma x) = subst sigma (ferase x)).
    { rewrite Hx. unfold ferase, fsubst. cbn [ft_construct]. rewrite erase_fsubst_c, Hc.
      symmetry. apply subst_encode. exact Hwf. }
    split; [exact He |].
    rewrite (proj1 (refresh_strict_decode_erase (fsubst sigma x))), He, Hx.
    apply decode_commutes_with_subst. apply decode_encode. exact Hwf.
  Qed.
End Substitution.

(* A concrete substitution. Bound variable i takes the value sigma i. *)
Definition concrete_env : Type := nat -> option par.

Definition concrete_shift (k : nat) (sigma : concrete_env) : concrete_env :=
  fun i => if i <? k then None else sigma (i - k).

Fixpoint concrete_subst (sigma : concrete_env) (t : par) {struct t} : par :=
  match t with
  | PPriv id => PPriv id
  | PStr s => PStr s
  | PList ps => PList (map (concrete_subst sigma) ps)
  | PTuple ps => PTuple (map (concrete_subst sigma) ps)
  | PMap kvs => PMap (canon_map (map_values (concrete_subst sigma) kvs))
  | PVar (VBound i) =>
      match sigma i with
      | Some value => value
      | None => PVar (VBound i)
      end
  | PVar v => PVar v
  | PMethod name target args =>
      PMethod name (concrete_subst sigma target) (map (concrete_subst sigma) args)
  | PNew k uris injections body =>
      PNew k uris injections (concrete_subst (concrete_shift k sigma) body)
  | PNil => PNil
  | POther n => POther n
  end.

(* The concrete substitution satisfies all six premises by computation. *)
Corollary decode_commutes_with_concrete_subst :
  forall sigma t c,
    decode t = Some c ->
    decode (concrete_subst sigma t) =
    Some (subst_construct concrete_env concrete_subst concrete_shift sigma c).
Proof.
  apply decode_commutes_with_subst; reflexivity.
Qed.

(* The concrete substitution also discharges the premises of headline 15,
   with the flag function that keeps the flag of each value. *)
Corollary refreshed_decode_commutes_with_concrete_subst :
  forall sigma x c,
    sdecode x = Some c ->
    ferase (fsubst concrete_env concrete_subst concrete_shift (fun _ _ f => f) sigma x) =
    concrete_subst sigma (ferase x) /\
    sdecode (refresh (fsubst concrete_env concrete_subst concrete_shift (fun _ _ f => f) sigma x)) =
    Some (subst_construct concrete_env concrete_subst concrete_shift sigma c).
Proof.
  intros sigma x c H. apply refreshed_decode_commutes_with_subst; try reflexivity. exact H.
Qed.

(* Finding H-3. Recognition depends on the substitution time. A tuple whose
   head is a variable is inert. After substitution of a marker for the
   variable, the hook recognizes the tuple. The outcome is deterministic and
   equals the syntax (headline 8). *)
Lemma recognition_depends_on_substitution_timing :
  let t := PTuple [PVar (VBound 0); POther 1; PNil] in
  let sigma : concrete_env := fun i => if i =? 0 then Some (marker_par MUse) else None in
  recognize PosProcessExpr t = RInert /\
  recognize PosProcessExpr (concrete_subst sigma t) = RConstruct (CUse (POther 1) PNil).
Proof. split; reflexivity. Qed.

(* ── 10. NEWSPACE binder order (design section 1.4) ────────────────────── *)

(* One declaration of a new: `x`, `x : s` or `x(uri)`. The record can also
   hold a URI and an annotation together, which the normalizer rejects. The
   name itself does not affect the encoding. *)
Record name_decl : Type := mkNameDecl {
  decl_uri : option bytes;
  decl_space : option par
}.

(* The order of Option<String>: None first, then URIs by String::cmp. *)
Definition uri_key_le (a b : option bytes) : bool :=
  match a, b with
  | None, _ => true
  | Some _, None => false
  | Some x, Some y => match bytes_compare x y with Gt => false | _ => true end
  end.

(* insert_decl puts d before the first element with a key that is not
   smaller. The sort is thus stable. *)
Fixpoint insert_decl (d : name_decl) (l : list name_decl) : list name_decl :=
  match l with
  | [] => [d]
  | x :: rest =>
      if uri_key_le (decl_uri d) (decl_uri x) then d :: x :: rest else x :: insert_decl d rest
  end.

(* binder_order models sorted_bindings.sort_by(|a, b| a.0.cmp(&b.0))
   (p_new_normalizer.rs:47-50). put_all_pos gives one de Bruijn slot to each
   element in this order (bound_map.rs:30-42 and 63-69).
   allocate_new_bindings draws the fresh names of the plain binders in the
   same order (util/mod.rs:198-204). *)
Fixpoint binder_order (l : list name_decl) : list name_decl :=
  match l with
  | [] => []
  | d :: rest => insert_decl d (binder_order rest)
  end.

Definition is_plain_decl (d : name_decl) : bool :=
  match decl_uri d with None => true | Some _ => false end.

Fixpoint uris_of (l : list name_decl) : list bytes :=
  match l with
  | [] => []
  | d :: rest =>
      match decl_uri d with
      | Some u => u :: uris_of rest
      | None => uris_of rest
      end
  end.

Definition annotation_on_uri (d : name_decl) : bool :=
  match decl_uri d, decl_space d with
  | Some _, Some _ => true
  | _, _ => false
  end.

Inductive new_normalization : Type :=
| NormalizedNew : par -> new_normalization
| NormalizedNewSpace : par -> new_normalization
| SpaceTypeOnUriBinding : new_normalization.

(* The planned normalizer arm for new. An annotation on a URI binder is an
   error. A new without annotations stays a plain New. The annotation list
   follows the plain binders in de Bruijn order. *)
Definition normalize_new (decls : list name_decl) (injections : nat) (body : par)
  : new_normalization :=
  if existsb annotation_on_uri decls then SpaceTypeOnUriBinding
  else
    let ordered := binder_order decls in
    let annotations := map decl_space (filter is_plain_decl ordered) in
    if existsb is_annotated annotations
    then NormalizedNewSpace
           (encode (CNewSpace annotations (length decls) (uris_of ordered) injections body))
    else NormalizedNew (PNew (length decls) (uris_of ordered) injections body).

Lemma insert_decl_plain :
  forall d l, is_plain_decl d = true -> insert_decl d l = d :: l.
Proof.
  intros [[u |] s] [| x l] H; simpl in *; try discriminate; reflexivity.
Qed.

Lemma insert_decl_over_plain :
  forall d plain rest,
    is_plain_decl d = false -> Forall (fun x => is_plain_decl x = true) plain ->
    insert_decl d (plain ++ rest) = plain ++ insert_decl d rest.
Proof.
  intros d plain rest Hd Hplain.
  induction Hplain as [| x plain Hx Hplain IH]; [reflexivity |].
  simpl. destruct d as [[u |] s]; simpl in Hd; [| discriminate].
  destruct x as [[v |] t]; simpl in Hx; [discriminate |]. simpl.
  rewrite IH. reflexivity.
Qed.

Lemma insert_decl_in :
  forall d l x, In x (insert_decl d l) -> x = d \/ In x l.
Proof.
  intros d l x. induction l as [| y l IH]; simpl.
  - intros [H | []]. left. symmetry. exact H.
  - destruct (uri_key_le (decl_uri d) (decl_uri y)); simpl.
    + intros [H | [H | H]];
        [left; symmetry; exact H | right; left; exact H | right; right; exact H].
    + intros [H | H]; [right; left; exact H |].
      apply IH in H as [H | H]; [left; exact H | right; right; exact H].
Qed.

Lemma insert_decl_perm : forall d l, Permutation (insert_decl d l) (d :: l).
Proof.
  intros d l. induction l as [| y l IH]; simpl; [reflexivity |].
  destruct (uri_key_le (decl_uri d) (decl_uri y)); [reflexivity |].
  eapply perm_trans; [apply perm_skip; exact IH | apply perm_swap].
Qed.

Lemma binder_order_perm : forall l, Permutation (binder_order l) l.
Proof.
  induction l as [| d l IH]; simpl; [reflexivity |].
  eapply perm_trans; [apply insert_decl_perm | apply perm_skip; exact IH].
Qed.

(* The plain binders come first, in source order. The URI binders follow. *)
Lemma binder_order_split :
  forall l,
    exists uri_part,
      Forall (fun x => is_plain_decl x = false) uri_part /\
      binder_order l = filter is_plain_decl l ++ uri_part.
Proof.
  induction l as [| d l IH]; [exists []; split; [constructor | reflexivity] |].
  destruct IH as [uri_part [Huri Hsplit]].
  simpl. rewrite Hsplit.
  destruct (is_plain_decl d) eqn:Ed.
  - exists uri_part. split; [exact Huri |].
    rewrite insert_decl_plain by exact Ed. reflexivity.
  - exists (insert_decl d uri_part). split.
    + apply Forall_forall. intros x Hx. apply insert_decl_in in Hx as [-> | Hx]; [exact Ed |].
      rewrite Forall_forall in Huri. apply Huri. exact Hx.
    + apply insert_decl_over_plain; [exact Ed |].
      apply Forall_forall. intros x Hx. apply filter_In in Hx as [_ Hx]. exact Hx.
Qed.

Lemma filter_all_true :
  forall (A : Type) (f : A -> bool) (l : list A),
    (forall x, In x l -> f x = true) -> filter f l = l.
Proof.
  intros A f l H. induction l as [| x l IH]; [reflexivity |].
  simpl. rewrite (H x (or_introl eq_refl)). f_equal.
  apply IH. intros y Hy. apply H. right. exact Hy.
Qed.

Lemma binder_order_plain_prefix :
  forall l, filter is_plain_decl (binder_order l) = filter is_plain_decl l.
Proof.
  intros l. destruct (binder_order_split l) as [uri_part [Huri ->]].
  rewrite filter_app.
  rewrite (filter_all_false _ _ uri_part) by (rewrite Forall_forall in Huri; exact Huri).
  rewrite app_nil_r. apply filter_all_true.
  intros x Hx. apply filter_In in Hx as [_ Hx]. exact Hx.
Qed.

Lemma uris_of_length :
  forall l, length (uris_of l) + length (filter is_plain_decl l) = length l.
Proof.
  induction l as [| [[u |] s] l IH]; simpl; lia.
Qed.

Lemma binder_order_length : forall l, length (binder_order l) = length l.
Proof. intros l. apply Permutation_length. apply binder_order_perm. Qed.

(* Entry i of the annotation list annotates plain binder i in source order.
   The list has bind_count - |uri| entries, and the construct is well
   formed. *)
Theorem normalize_new_binder_order :
  forall decls injections body t,
    normalize_new decls injections body = NormalizedNewSpace t ->
    let annotations := map decl_space (filter is_plain_decl decls) in
    let c := CNewSpace annotations (length decls) (uris_of (binder_order decls))
               injections body in
    t = encode c /\ wf c /\
    length annotations = length decls - length (uris_of (binder_order decls)).
Proof.
  intros decls injections body t H annotations c.
  unfold normalize_new in H.
  destruct (existsb annotation_on_uri decls); [discriminate |].
  rewrite binder_order_plain_prefix in H. fold annotations in H.
  destruct (existsb is_annotated annotations) eqn:Hann; [| discriminate].
  injection H as <-.
  assert (Hlen : length decls = length annotations + length (uris_of (binder_order decls))).
  { pose proof (uris_of_length (binder_order decls)) as Hu.
    rewrite binder_order_length, binder_order_plain_prefix in Hu.
    unfold annotations. rewrite length_map. lia. }
  split; [reflexivity |]. split.
  - split; [exact Hlen | exact Hann].
  - lia.
Qed.

Lemma normalize_new_rejects_uri_annotation :
  forall decls injections body,
    existsb annotation_on_uri decls = true ->
    normalize_new decls injections body = SpaceTypeOnUriBinding.
Proof. intros decls injections body H. unfold normalize_new. rewrite H. reflexivity. Qed.

Lemma normalize_new_unannotated_stays_plain :
  forall decls injections body,
    (forall d, In d decls -> decl_space d = None) ->
    normalize_new decls injections body =
    NormalizedNew (PNew (length decls) (uris_of (binder_order decls)) injections body).
Proof.
  intros decls injections body Hnone. unfold normalize_new.
  assert (Huri : existsb annotation_on_uri decls = false).
  { apply Bool.not_true_iff_false. intro H. apply existsb_exists in H as [d [Hin Hd]].
    unfold annotation_on_uri in Hd. rewrite (Hnone d Hin) in Hd.
    destruct (decl_uri d); discriminate. }
  rewrite Huri, binder_order_plain_prefix.
  assert (Hann : existsb is_annotated (map decl_space (filter is_plain_decl decls)) = false).
  { apply Bool.not_true_iff_false. intro H. apply existsb_exists in H as [a [Hin Ha]].
    apply in_map_iff in Hin as [d [<- Hin]]. apply filter_In in Hin as [Hin _].
    rewrite (Hnone d Hin) in Ha. discriminate. }
  rewrite Hann. reflexivity.
Qed.

(* ── 11. The superseded "s_i or Nil" encoding (finding C3) ─────────────── *)

(* The superseded plan put s_i in the slot of an annotated binder and Nil in
   the slot of an unannotated binder. *)
Definition plan_nil_annotation (a : option par) : par :=
  match a with
  | Some space => space
  | None => PNil
  end.

Definition plan_encode_newspace (annotations : list (option par)) (k : nat)
  (uris : list bytes) (injections : nat) (body : par) : par :=
  PTuple [marker_par MNewSpace; PList (map plan_nil_annotation annotations);
          PNew k uris injections body].

Definition plan_decode_annotation (a : par) : option par :=
  match a with
  | PNil => None
  | space => Some space
  end.

(* nil_binding binds the variable with index 0 to Nil. A receive that gets
   Nil for s does this to its body (storage substitution,
   reduce.rs:1420-1424). *)
Definition nil_binding : concrete_env := fun i => if i =? 0 then Some PNil else None.

(* Headline 12. The superseded encoding maps the annotation Nil and the
   absence of an annotation to one term. Substitution of Nil for an
   annotation variable thus erases the annotation, so the EmptySpaceRef error
   is silently lost. The corrected [] or [s] encoding keeps both
   distinctions. *)
Theorem plan_nil_annotation_not_injective :
  (forall k uris injections body,
      plan_encode_newspace [Some PNil] k uris injections body =
      plan_encode_newspace [None] k uris injections body) /\
  [Some PNil] <> [None] /\
  (forall k uris injections,
      concrete_subst nil_binding
        (plan_encode_newspace [Some (PVar (VBound 0))] k uris injections PNil) =
      plan_encode_newspace [None] k uris injections PNil) /\
  map plan_decode_annotation
    (map (concrete_subst nil_binding) (map plan_nil_annotation [Some (PVar (VBound 0))])) =
  [None] /\
  map (option_map (concrete_subst nil_binding)) [Some (PVar (VBound 0))] = [Some PNil] /\
  (forall k uris injections body,
      encode (CNewSpace [Some PNil] k uris injections body) <>
      encode (CNewSpace [None] k uris injections body)) /\
  (forall k uris injections,
      concrete_subst nil_binding
        (encode (CNewSpace [Some (PVar (VBound 0))] k uris injections PNil)) =
      encode (CNewSpace [Some PNil] k uris injections PNil)).
Proof.
  split; [intros; reflexivity |].
  split; [discriminate |].
  split; [intros; reflexivity |].
  split; [reflexivity |].
  split; [reflexivity |].
  split.
  - intros k uris injections body H. apply encode_injective in H. discriminate.
  - intros; reflexivity.
Qed.

(* ── 12. Worked examples and negative controls ─────────────────────────── *)

Import (notations) Stdlib.Strings.String.

(* str gives the bytes of an ASCII string literal. *)
Definition str (s : String.string) : bytes :=
  map Ascii.nat_of_ascii (String.list_ascii_of_string s).

(* `ch!(d; 3)` gives the data `[d, (PARAMS, [3], {})]`. *)
Example send_positional_param :
  send_data [POther 1] [POther 3] [] =
  [POther 1; PTuple [marker_par MParams; PList [POther 3]; PMap []]].
Proof. reflexivity. Qed.

(* `ch!(d; 3, priority=4)` keeps both values. *)
Example send_positional_and_named_param :
  send_data [POther 1] [POther 3] [(str "priority", POther 4)] =
  [POther 1; PTuple [marker_par MParams; PList [POther 3]; PMap [(str "priority", POther 4)]]].
Proof. reflexivity. Qed.

(* `ch!(d, 3)` stays distinct from `ch!(d; 3)`. *)
Example send_data_without_params :
  send_data [POther 1; POther 3] [] [] = [POther 1; POther 3].
Proof. reflexivity. Qed.

(* `ch!(; k=1)` gives `[(PARAMS, [], {"k": 1})]`. *)
Example send_only_named_param :
  send_data [] [] [(str "k", POther 1)] =
  [PTuple [marker_par MParams; PList []; PMap [(str "k", POther 1)]]].
Proof. reflexivity. Qed.

(* The bytewise order puts uppercase letters before lowercase letters. *)
Example named_params_bytewise_order :
  canon_map [(str "priority", POther 1); (str "k", POther 2); (str "Z", POther 3)] =
  [(str "Z", POther 3); (str "k", POther 2); (str "priority", POther 1)].
Proof. reflexivity. Qed.

Example canon_map_last_value_wins :
  canon_map [(str "k", POther 1); (str "k", POther 2)] = [(str "k", POther 2)].
Proof. reflexivity. Qed.

(* `ch ~ (v1, v2; k=3)`, `ch ~ ((v1, v2); k=3)` and `ch ~ ([v1, v2]; k=3)`
   have three distinct encodings. *)
Example query_payload_forms_distinct :
  encode (CQuery (POther 0) [POther 1; POther 2] [(str "k", POther 3)]) <>
  encode (CQuery (POther 0) [PTuple [POther 1; POther 2]] [(str "k", POther 3)]) /\
  encode (CQuery (POther 0) [PTuple [POther 1; POther 2]] [(str "k", POther 3)]) <>
  encode (CQuery (POther 0) [PList [POther 1; POther 2]] [(str "k", POther 3)]) /\
  encode (CQuery (POther 0) [POther 1; POther 2] [(str "k", POther 3)]) <>
  encode (CQuery (POther 0) [PList [POther 1; POther 2]] [(str "k", POther 3)]).
Proof.
  split; [| split]; intro H; apply encode_injective in H; injection H; intros; discriminate.
Qed.

Example decode_rejects_empty_params :
  decode (PTuple [marker_par MParams; PList []; PMap []]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_empty_query :
  forall channel, decode (PTuple [marker_par MQuery; channel; PList []; PMap []]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_unsorted_keys :
  decode (PTuple [marker_par MParams; PList []; PMap [(str "priority", PNil); (str "k", PNil)]])
  = None.
Proof. reflexivity. Qed.

Example decode_rejects_duplicate_keys :
  decode (PTuple [marker_par MParams; PList []; PMap [(str "k", PNil); (str "k", PNil)]]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_non_identifier_key :
  decode (PTuple [marker_par MParams; PList []; PMap [(str "1k", PNil)]]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_non_identifier_theory :
  decode (PTuple [marker_par MFree; PStr (str "Nat-1"); PList []]) = None.
Proof. reflexivity. Qed.

Example decode_accepts_identifier_theory :
  decode (PTuple [marker_par MFree; PStr (str "Nat"); PList []]) = Some (CFree (str "Nat") []).
Proof. reflexivity. Qed.

(* Plan decision 27: a word of the param-key list is not a key. The keyword
   in is outside that list, so it is a key. *)
Example decode_rejects_keyword_key :
  decode (PTuple [marker_par MParams; PList []; PMap [(str "true", PNil)]]) = None.
Proof. reflexivity. Qed.

Example decode_accepts_keyword_key :
  decode (PTuple [marker_par MParams; PList []; PMap [(str "in", PNil)]]) =
  Some (CParams [] [(str "in", PNil)]).
Proof. reflexivity. Qed.

(* new is in the theory-name list. try is a keyword outside it. *)
Example decode_rejects_keyword_theory :
  decode (PTuple [marker_par MFree; PStr (str "new"); PList []]) = None.
Proof. reflexivity. Qed.

Example decode_accepts_keyword_theory :
  decode (PTuple [marker_par MFree; PStr (str "try"); PList []]) = Some (CFree (str "try") []).
Proof. reflexivity. Qed.

Example builtin_theories_are_theory_names :
  forallb is_theory_name [str "Nat"; str "Int"; str "String"; str "Bool"; str "Any"] = true.
Proof. reflexivity. Qed.

Example word_list_sizes :
  length WordLists.rejected_param_keys = 26 /\ length WordLists.rejected_theory_names = 18.
Proof. split; reflexivity. Qed.

Example decode_rejects_newspace_without_annotation :
  decode (PTuple [marker_par MNewSpace; PList [PList []]; PNew 1 [] 0 PNil]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_annotation_count :
  decode (PTuple [marker_par MNewSpace; PList [PList [POther 5]]; PNew 2 [] 0 PNil]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_annotation_shape :
  decode (PTuple [marker_par MNewSpace; PList [PList [POther 5; POther 6]]; PNew 1 [] 0 PNil])
  = None.
Proof. reflexivity. Qed.

Example decode_rejects_plan_nil_annotation_form :
  decode (PTuple [marker_par MNewSpace; PList [POther 5]; PNew 1 [] 0 PNil]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_wrong_arity :
  decode (PTuple [marker_par MUse; POther 5]) = None.
Proof. reflexivity. Qed.

Example decode_rejects_unknown_marker_index :
  decode (PTuple [PPriv [224; 213]; POther 5; POther 6]) = None.
Proof. reflexivity. Qed.

Example decode_accepts_newspace :
  decode (PTuple [marker_par MNewSpace; PList [PList [POther 5]; PList []];
                  PNew 3 [str "rho:io:stdout"] 0 PNil]) =
  Some (CNewSpace [Some (POther 5); None] 3 [str "rho:io:stdout"] 0 PNil).
Proof. reflexivity. Qed.

Example params_inert_in_process_position :
  recognize PosProcessExpr (encode (CParams [POther 3] [])) = RInert /\
  evaluated_process_exprb (encode (CParams [POther 3] [])) = false.
Proof. split; reflexivity. Qed.

Example use_inert_as_last_send_data :
  recognize PosSendLastData (encode (CUse (POther 1) PNil)) = RInert.
Proof. reflexivity. Qed.

Example malformed_use_fails_closed :
  recognize PosProcessExpr (PTuple [marker_par MUse; POther 1]) = RMalformed.
Proof. reflexivity. Qed.

(* new a(`rho:io:stdout`), x : s, y in P. The URI binder sorts last. The
   annotation list covers x and y in source order. *)
Example normalize_new_example :
  normalize_new [mkNameDecl (Some (str "rho:io:stdout")) None; mkNameDecl None (Some (POther 7));
                 mkNameDecl None None] 0 PNil =
  NormalizedNewSpace
    (encode (CNewSpace [Some (POther 7); None] 3 [str "rho:io:stdout"] 0 PNil)).
Proof. reflexivity. Qed.

Example normalize_new_rejects_uri_annotation_example :
  normalize_new [mkNameDecl (Some (str "rho:io:stdout")) (Some (POther 7))] 0 PNil =
  SpaceTypeOnUriBinding.
Proof. reflexivity. Qed.

(* ── 13. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions decode_encode.
Print Assumptions encode_injective.
Print Assumptions decode_sound.
Print Assumptions markers_pairwise_distinct.
Print Assumptions markers_disjoint_reserved.
Print Assumptions named_params_canonical.
Print Assumptions recognize_complete.
Print Assumptions marker_forging_grants_no_capability.
Print Assumptions marker_inert_elsewhere.
Print Assumptions schedule_unchanged_marker_free.
Print Assumptions decode_commutes_with_subst.
Print Assumptions plan_nil_annotation_not_injective.
Print Assumptions refresh_strict_decode_erase.
Print Assumptions connective_marker_tuple_recognized.
Print Assumptions refreshed_decode_commutes_with_subst.
