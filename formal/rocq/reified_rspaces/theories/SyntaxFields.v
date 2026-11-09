(* ═══════════════════════════════════════════════════════════════════════════
   SyntaxFields.v — the Reified RSpaces syntax as protobuf fields

   Decision 112 moves the wire form of the Reified RSpaces syntax from the
   marker tuples of decision 18 into protobuf fields of the term model, as
   the base carries its own syntax. This module is the model of that form.
   It takes the place of Encoding.v in the gate. Encoding.v stays on disk as
   the frozen evidence of the marker encoding.

   The fields of decision 112:

     use s { P }                       Par.use_blocks, number 15. Each
                                       UseBlock holds space (1) and body (2),
                                       and both parts are always present.
     new x_1[: s_1], ... in P          New.space_types, number 6. An entry
                                       holds the space of one plain binder.
                                       The space of a binder without an
                                       annotation is absent. The list is
                                       empty, or it has one entry for each
                                       plain binder with at least one space.
     x!(d...; p..., k=v...)            Send.params, number 8 (SendParams:
                                       positional 1, named 2). It is absent
                                       when both parts are empty.
     for (y <- ch ~ (q...; k=v...))    ReceiveBind.query_spec, number 6
                                       (QuerySpec: payload 1, named 2). It
                                       is absent without the query marker
                                       and never has both parts empty.
     free Th(a...)                     Expr.e_free_body, number 44 (EFree:
                                       theory 1, args 2).
     params the reducer forwards       Expr.e_params_body, number 45.

   A new field takes the next free number of its message and is declared
   last in it. No new message carries its own derived flags. The holder of
   a field keeps locally_free and connective_used, as the base does for its
   cost terms.

   Sources
   - Decisions 112 (the fields), 15, 21 and 22 (the canonical rules, which
     stay), 27 (the param-key and theory-name rules) and I-20 (the refresh
     of the marker tuples, which has no counterpart here).
   - Code, named by item:
     models/src/main/protobuf/RhoTypes.proto (the messages and their
     numbers),
     models/src/rust/reified/encoding.rs (decode_use_block,
     decode_new_space, annotation_count_matches, check_send_params,
     check_query_spec, decode_free_value, encode_params, encode_free,
     sole_expr),
     models/src/rust/reified/builders.rs (the send and query builders),
     models/src/rust/reified/params.rs (collect_send_params),
     models/src/rust/reified/validation.rs (is_param_key, is_theory_name),
     rholang/src/rust/interpreter/util/mod.rs (is_evaluated_process_expr,
     evaluation_terms, evaluation_random),
     rholang/src/rust/interpreter/substitute.rs (the substitution of New).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability: Rocq definition → Rust or proto item
   ─────────────────────────────────────────────────────────────────────────
   par_numbers and the tables
                            RhoTypes.proto, the messages Par, Send,
                            ReceiveBind, New, Expr, UseBlock, SendParams,
                            QuerySpec, SpaceAnnotation, EFree
   is_param_key, is_theory_name
                            validation.rs, is_param_key, is_theory_name
   par and its messages     the generated Rust types of RhoTypes.proto
   carrier                  the five fields of decision 112
   construct, wf            encoding.rs, the types UseBlock, NewSpace,
                            SendParams, QuerySpec, FreeTheory
   encode                   encoding.rs and builders.rs, the writers of the
                            fields
   decode                   encoding.rs, decode_use_block, decode_new_space,
                            check_send_params, check_query_spec,
                            decode_free_value
   sole_expr                encoding.rs, sole_expr (with sole_component)
   canon_map                the BTreeMap of params.rs, collect_send_params
   is_evaluated_process_expr
                            util/mod.rs, is_evaluated_process_expr
   evaluation_terms         util/mod.rs, evaluation_terms
   subst_new_block          substitute.rs, the substitution of New
   proto, bincode_len       prost and bincode, the writers of the two
                            schemas

   ─────────────────────────────────────────────────────────────────────────
   Headline theorems (one Print Assumptions line per headline at the end)
   ─────────────────────────────────────────────────────────────────────────
    1 field_numbers_distinct          each changed message has distinct numbers
    2 field_numbers_avoid_reserved    the new numbers avoid the reserved ones
    3 field_decode_encode             the decoder inverts the encoder
    4 field_encode_injective          different constructs, different fields
    5 field_decode_sound              the decoder accepts exactly the
                                      canonical images
    6 named_params_canonical          named keys sorted and unique, positional
                                      order kept
    7 construct_has_one_carrier       each construct has one field, and the
                                      reducer reads it only there
    8 former_marker_tuple_is_ordinary a former marker tuple is an ordinary
                                      tuple
    9 fields_inert_outside_process_position
                                      a use block in data or channel position
                                      never runs
   10 schedule_unchanged_field_free   ordinary terms keep their indices, use
                                      blocks come after them
   11 fields_commute_with_subst       decoding commutes with substitution,
                                      annotations use the outer env
   12 absent_annotation_not_nil       Nil-for-absent is not injective, the
                                      option form is
   13 prost_bytes_unchanged_field_free
                                      a term without new fields has the same
                                      proto3 bytes in both schemas
   14 bincode_growth_field_free       the growth formula of the bincode form
   15 decode_ignores_flags            no decoder reads a derived flag

   The theorems appear in dependency order, so the numbers above are labels
   and do not give the file order. Headlines 4 and 12 precede headline 3 so
   that a mutant of the annotation encoder fails at the injectivity claims
   first.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `cbn` with vec_len in its list does not unfold vec_len applied to a list
     that is no constructor, and lia then reports "Cannot find witness".
     Unfold vec_len with `unfold vec_len` before lia.
   - A `rewrite` with a hypothesis about `named_params_ok named` finds no
     subterm when `simpl` has evaluated `named_params_ok []` in the case of
     an empty named list. Case split on the named list before the rewrite,
     or state the decoder through decode_params_some and decode_query_some.
   - `repeat split` closes a final equation by eq_refl, so a tactic after it
     reports "No such goal". The proofs name each conjunct with `split`.
   - A proof of field_decode_encode that rewrites with the whole decoder
     check breaks on any weaker decoder (the negative control of the count
     check). The proof splits the check and rewrites with the two conjuncts
     separately, so only field_decode_sound depends on the exact check.
   - `destruct c eqn:E` replaces c in every hypothesis that mentions it. With
     a decoder that checks only `existsb is_some annots`, the hypothesis Hany
     becomes `false = true` in the false branch, and `discriminate E` reports
     Not a discriminable equality. The proof ends with
     `first [discriminate Echeck | discriminate Hany]`.
   - `simpl in E` leaves `build_params p n` folded when p and n are variables
     and the body has no match to reduce (a mutant without the empty case), so
     congruence sees no constructors. The injectivity proof unfolds
     build_params before the case split.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Strings.String Strings.Ascii.
Import ListNotations.

(* ── 1. Bytes, the bytewise order and identifiers ─────────────────────── *)

(* A byte list models a GPrivate id and the UTF-8 bytes of a string. *)
Definition bytes : Type := list nat.

(* bytes_compare is the lexicographic order on byte lists, the order of
   String::cmp and of the BTreeMap keys. *)
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

(* is_var_token accepts exactly the strings of the grammar token var
   (grammar.js rule var): [a-zA-Z]([a-zA-Z0-9_'])* or _([a-zA-Z0-9_'])+ . *)
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

(* The word lists of decision 27. They are the constants REJECTED_PARAM_KEYS
   and REJECTED_THEORY_NAMES of validation.rs, in the same order. *)
Module WordLists.
  Import (notations) Stdlib.Strings.String.
  Local Open Scope string_scope.

  Definition word (s : String.string) : bytes :=
    map Ascii.nat_of_ascii (String.list_ascii_of_string s).

  Definition rejected_param_keys : list bytes :=
    map word ["Bool"; "ByteArray"; "Int"; "Nil"; "Set"; "String"; "Uri"; "agent";
              "and"; "bundle"; "bundle0"; "contract"; "else"; "false"; "for"; "if";
              "let"; "match"; "matches"; "new"; "not"; "or"; "select"; "true"; "try";
              "where"].

  Definition rejected_theory_names : list bytes :=
    map word ["Set"; "and"; "bundle"; "contract"; "else"; "false"; "for"; "if";
              "in"; "let"; "match"; "matches"; "new"; "not"; "or"; "select"; "true";
              "where"].
End WordLists.

Definition in_word_list (words : list bytes) (s : bytes) : bool :=
  existsb (bytes_eqb s) words.

(* is_param_key accepts a key of a named param (validation.rs, is_param_key):
   a var token outside the param-key list. *)
Definition is_param_key (s : bytes) : bool :=
  is_var_token s && negb (in_word_list WordLists.rejected_param_keys s).

(* is_theory_name accepts the theory name of free Th(a...) (validation.rs,
   is_theory_name): a var token outside the theory-name list. *)
Definition is_theory_name (s : bytes) : bool :=
  is_var_token s && negb (in_word_list WordLists.rejected_theory_names s).

(* ── 2. The term model ─────────────────────────────────────────────────── *)

(* The model keeps the messages that the fields of decision 112 change, and
   the fields of those messages that the theorems read. It omits the fields
   that no theorem reads (matches, conditionals, connectives, bundles, the
   receive flags, the free counts, the cost terms). An omitted field has the
   same encoded form in both schemas. It adds the same bytes to both sides
   of the claims about the wire form and the same prefix to the term list
   of the schedule. *)

Inductive var : Type :=
| VBound : nat -> var
| VFree : nat -> var
| VWildcard : var.

(* Each message is generic in the type T of a nested Par. The recursive type
   par closes the recursion, so one nested induction principle covers all
   messages. *)
Inductive send_params_of (T : Type) : Type :=
| mkSendParams : list T -> list (bytes * T) -> send_params_of T.

Inductive query_spec_of (T : Type) : Type :=
| mkQuerySpec : list T -> list (bytes * T) -> query_spec_of T.

(* The prost type of a message field is an Option, so the model keeps the
   absence of a part, which the decoder rejects. *)
Inductive use_block_of (T : Type) : Type :=
| mkUseBlock : option T -> option T -> use_block_of T.

Inductive efree_of (T : Type) : Type :=
| mkEFree : bytes -> list T -> efree_of T.

Inductive send_of (T : Type) : Type :=
| mkSend : T -> list T -> bool -> option (send_params_of T) -> send_of T.

Inductive bind_of (T : Type) : Type :=
| mkBind : list T -> T -> option (query_spec_of T) -> bind_of T.

Inductive receive_of (T : Type) : Type :=
| mkReceive : list (bind_of T) -> T -> receive_of T.

(* mkNew holds the bind count, the body, the uris, the locally_free set of
   the holder, and space_types. An entry of space_types is the option of a
   SpaceAnnotation message. *)
Inductive new_block_of (T : Type) : Type :=
| mkNew : nat -> T -> list bytes -> list nat -> list (option T) -> new_block_of T.

Inductive expr_of (T : Type) : Type :=
| EVar : var -> expr_of T
| EMethod : bytes -> T -> list T -> expr_of T
| ETuple : list T -> expr_of T
| EFreeBody : efree_of T -> expr_of T
| EParamsBody : send_params_of T -> expr_of T
| EOther : nat -> expr_of T.

Arguments mkSendParams {T} _ _.
Arguments mkQuerySpec {T} _ _.
Arguments mkUseBlock {T} _ _.
Arguments mkEFree {T} _ _.
Arguments mkSend {T} _ _ _ _.
Arguments mkBind {T} _ _ _.
Arguments mkReceive {T} _ _.
Arguments mkNew {T} _ _ _ _ _.
Arguments EVar {T} _.
Arguments EMethod {T} _ _ _.
Arguments ETuple {T} _.
Arguments EFreeBody {T} _.
Arguments EParamsBody {T} _.
Arguments EOther {T} _.

(* mkPar holds sends, receives, news, exprs, unforgeables (the ids of
   GPrivate names), the locally_free set, the connective_used flag, and
   use_blocks, the new field. *)
Inductive par : Type :=
| mkPar :
    list (send_of par) -> list (receive_of par) -> list (new_block_of par) ->
    list (expr_of par) -> list bytes -> list nat -> bool ->
    list (use_block_of par) -> par.

Notation send := (send_of par).
Notation bind := (bind_of par).
Notation receive := (receive_of par).
Notation new_block := (new_block_of par).
Notation expr := (expr_of par).
Notation use_block := (use_block_of par).
Notation send_params := (send_params_of par).
Notation query_spec := (query_spec_of par).
Notation efree := (efree_of par).

Definition use_blocks_of (p : par) : list use_block :=
  match p with mkPar _ _ _ _ _ _ _ ubs => ubs end.

(* The Nil process has no field set. *)
Definition nil_par : par := mkPar [] [] [] [] [] [] false [].

(* A GPrivate name as a Par. *)
Definition unforgeable_par (id : bytes) : par := mkPar [] [] [] [] [id] [] false [].

(* A Par that holds one expression. *)
Definition expr_par (e : expr) : par := mkPar [] [] [] [e] [] [] false [].

(* The automatic induction principle of a nested type gives no hypothesis
   for a nested Par. This principle gives one for each nested Par of each
   message. *)
Section ParNestedInduction.
  Variable P : par -> Prop.

  Definition holds_named (kvs : list (bytes * par)) : Prop :=
    Forall (fun kv => P (snd kv)) kvs.

  Definition holds_params (sp : send_params) : Prop :=
    match sp with mkSendParams pos named => Forall P pos /\ holds_named named end.

  Definition holds_query (q : query_spec) : Prop :=
    match q with mkQuerySpec payload named => Forall P payload /\ holds_named named end.

  Definition holds_option {A : Type} (holds : A -> Prop) (o : option A) : Prop :=
    match o with Some x => holds x | None => True end.

  Definition holds_send (s : send) : Prop :=
    match s with
    | mkSend chan data _ params =>
        P chan /\ Forall P data /\ holds_option holds_params params
    end.

  Definition holds_bind (b : bind) : Prop :=
    match b with
    | mkBind patterns source query =>
        Forall P patterns /\ P source /\ holds_option holds_query query
    end.

  Definition holds_receive (r : receive) : Prop :=
    match r with mkReceive binds body => Forall holds_bind binds /\ P body end.

  Definition holds_new (n : new_block) : Prop :=
    match n with mkNew _ body _ _ annots => P body /\ Forall (holds_option P) annots end.

  Definition holds_expr (e : expr) : Prop :=
    match e with
    | EVar _ => True
    | EMethod _ target args => P target /\ Forall P args
    | ETuple ps => Forall P ps
    | EFreeBody (mkEFree _ args) => Forall P args
    | EParamsBody sp => holds_params sp
    | EOther _ => True
    end.

  Definition holds_use (u : use_block) : Prop :=
    match u with mkUseBlock space body => holds_option P space /\ holds_option P body end.

  Hypothesis step :
    forall sends receives news exprs unforgeables locally_free connective_used use_blocks,
      Forall holds_send sends ->
      Forall holds_receive receives ->
      Forall holds_new news ->
      Forall holds_expr exprs ->
      Forall holds_use use_blocks ->
      P (mkPar sends receives news exprs unforgeables locally_free connective_used use_blocks).

  Fixpoint par_nested_ind (t : par) : P t :=
    let fix on_pars (ps : list par) : Forall P ps :=
      match ps as l return Forall P l with
      | [] => Forall_nil P
      | p :: rest => @Forall_cons par P p rest (par_nested_ind p) (on_pars rest)
      end in
    let fix on_named (kvs : list (bytes * par)) : holds_named kvs :=
      match kvs as l return holds_named l with
      | [] => Forall_nil _
      | kv :: rest =>
          match kv as kv0 return holds_named (kv0 :: rest) with
          | (k, v) =>
              @Forall_cons (bytes * par) (fun kv' => P (snd kv')) (k, v) rest
                (par_nested_ind v) (on_named rest)
          end
      end in
    let on_params (sp : send_params) : holds_params sp :=
      match sp as s0 return holds_params s0 with
      | mkSendParams pos named => conj (on_pars pos) (on_named named)
      end in
    let on_query (q : query_spec) : holds_query q :=
      match q as q0 return holds_query q0 with
      | mkQuerySpec payload named => conj (on_pars payload) (on_named named)
      end in
    let on_opt_par (o : option par) : holds_option P o :=
      match o as o0 return holds_option P o0 with
      | Some x => par_nested_ind x
      | None => I
      end in
    let fix on_annots (annots : list (option par)) : Forall (holds_option P) annots :=
      match annots as l return Forall (holds_option P) l with
      | [] => Forall_nil _
      | a :: rest =>
          @Forall_cons _ (holds_option P) a rest (on_opt_par a) (on_annots rest)
      end in
    let on_send (s : send) : holds_send s :=
      match s as s0 return holds_send s0 with
      | mkSend chan data _ params =>
          conj (par_nested_ind chan)
            (conj (on_pars data)
               (match params as o return holds_option holds_params o with
                | Some sp => on_params sp
                | None => I
                end))
      end in
    let fix on_sends (ss : list send) : Forall holds_send ss :=
      match ss as l return Forall holds_send l with
      | [] => Forall_nil _
      | s :: rest => @Forall_cons _ holds_send s rest (on_send s) (on_sends rest)
      end in
    let on_bind (b : bind) : holds_bind b :=
      match b as b0 return holds_bind b0 with
      | mkBind patterns source query =>
          conj (on_pars patterns)
            (conj (par_nested_ind source)
               (match query as o return holds_option holds_query o with
                | Some q => on_query q
                | None => I
                end))
      end in
    let fix on_binds (bs : list bind) : Forall holds_bind bs :=
      match bs as l return Forall holds_bind l with
      | [] => Forall_nil _
      | b :: rest => @Forall_cons _ holds_bind b rest (on_bind b) (on_binds rest)
      end in
    let on_receive (r : receive) : holds_receive r :=
      match r as r0 return holds_receive r0 with
      | mkReceive binds body => conj (on_binds binds) (par_nested_ind body)
      end in
    let fix on_receives (rs : list receive) : Forall holds_receive rs :=
      match rs as l return Forall holds_receive l with
      | [] => Forall_nil _
      | r :: rest => @Forall_cons _ holds_receive r rest (on_receive r) (on_receives rest)
      end in
    let on_new (n : new_block) : holds_new n :=
      match n as n0 return holds_new n0 with
      | mkNew _ body _ _ annots => conj (par_nested_ind body) (on_annots annots)
      end in
    let fix on_news (ns : list new_block) : Forall holds_new ns :=
      match ns as l return Forall holds_new l with
      | [] => Forall_nil _
      | n :: rest => @Forall_cons _ holds_new n rest (on_new n) (on_news rest)
      end in
    let on_expr (e : expr) : holds_expr e :=
      match e as e0 return holds_expr e0 with
      | EVar _ => I
      | EMethod _ target args => conj (par_nested_ind target) (on_pars args)
      | ETuple ps => on_pars ps
      | EFreeBody free =>
          match free as f0 return holds_expr (EFreeBody f0) with
          | mkEFree _ args => on_pars args
          end
      | EParamsBody sp => on_params sp
      | EOther _ => I
      end in
    let fix on_exprs (es : list expr) : Forall holds_expr es :=
      match es as l return Forall holds_expr l with
      | [] => Forall_nil _
      | e :: rest => @Forall_cons _ holds_expr e rest (on_expr e) (on_exprs rest)
      end in
    let on_use (u : use_block) : holds_use u :=
      match u as u0 return holds_use u0 with
      | mkUseBlock space body => conj (on_opt_par space) (on_opt_par body)
      end in
    let fix on_uses (us : list use_block) : Forall holds_use us :=
      match us as l return Forall holds_use l with
      | [] => Forall_nil _
      | u :: rest => @Forall_cons _ holds_use u rest (on_use u) (on_uses rest)
      end in
    match t as t0 return P t0 with
    | mkPar sends receives news exprs ufs lf cu ubs =>
        step sends receives news exprs ufs lf cu ubs
          (on_sends sends) (on_receives receives) (on_news news) (on_exprs exprs) (on_uses ubs)
    end.
End ParNestedInduction.

(* ── 3. The field numbers of the changed messages ─────────────────────── *)

Definition par_old_numbers : list nat :=
  [1; 2; 4; 5; 6; 7; 11; 8; 12; 9; 10; 13; 14].

Definition use_blocks_number : nat := 15.

Definition par_numbers : list nat := par_old_numbers ++ [use_blocks_number].

Definition send_old_numbers : list nat := [1; 2; 3; 5; 6].

Definition params_number : nat := 8.

Definition send_numbers : list nat := send_old_numbers ++ [params_number].

Definition bind_old_numbers : list nat := [1; 2; 3; 4; 5].

Definition query_spec_number : nat := 6.

Definition bind_numbers : list nat := bind_old_numbers ++ [query_spec_number].

Definition new_old_numbers : list nat := [1; 2; 3; 4; 5].

Definition space_types_number : nat := 6.

Definition new_numbers : list nat := new_old_numbers ++ [space_types_number].

Definition expr_old_numbers : list nat :=
  [1; 2; 3; 4; 25; 5; 6; 7; 8; 9; 10; 11; 12; 13; 14; 15; 16; 17; 18; 19; 20; 21;
   22; 23; 24; 32; 33; 27; 28; 29; 30; 31; 34; 35; 36; 37; 38; 39; 40; 41; 42; 43].

Definition e_free_body_number : nat := 44.

Definition e_params_body_number : nat := 45.

Definition expr_numbers : list nat :=
  expr_old_numbers ++ [e_free_body_number; e_params_body_number].

Definition use_block_numbers : list nat := [1; 2].

Definition send_params_numbers : list nat := [1; 2].

Definition query_spec_numbers : list nat := [1; 2].

Definition space_annotation_numbers : list nat := [1].

Definition efree_numbers : list nat := [1; 2].

Fixpoint nodupb (l : list nat) : bool :=
  match l with
  | [] => true
  | x :: rest => negb (existsb (Nat.eqb x) rest) && nodupb rest
  end.

Lemma nodupb_sound : forall l, nodupb l = true -> NoDup l.
Proof.
  induction l as [| x rest IH]; intros H; [constructor |].
  simpl in H. apply andb_true_iff in H as [Hn Hr].
  constructor; [| exact (IH Hr)].
  intro Hin. apply negb_true_iff in Hn.
  assert (Hex : existsb (Nat.eqb x) rest = true).
  { apply existsb_exists. exists x. split; [exact Hin | apply Nat.eqb_refl]. }
  rewrite Hex in Hn. discriminate.
Qed.

(* Headline 1. Each message that decision 112 changes keeps pairwise
   distinct field numbers. *)
Theorem field_numbers_distinct :
  NoDup par_numbers /\ NoDup send_numbers /\ NoDup bind_numbers /\ NoDup new_numbers /\
  NoDup expr_numbers /\ NoDup use_block_numbers /\ NoDup send_params_numbers /\
  NoDup query_spec_numbers /\ NoDup space_annotation_numbers /\ NoDup efree_numbers.
Proof.
  repeat split; apply nodupb_sound; reflexivity.
Qed.

(* Headline 2. The new numbers avoid the numbers that the protocol reserves:
   Par 3, Send 4 and 7, and Expr 32 and 33. *)
Theorem field_numbers_avoid_reserved :
  use_blocks_number <> 3 /\
  params_number <> 4 /\ params_number <> 7 /\
  e_free_body_number <> 32 /\ e_free_body_number <> 33 /\
  e_params_body_number <> 32 /\ e_params_body_number <> 33.
Proof.
  unfold use_blocks_number, params_number, e_free_body_number, e_params_body_number.
  repeat split; discriminate.
Qed.

(* ── 4. Canonical string-keyed maps ────────────────────────────────────── *)

(* The named params are a BTreeMap (models/build.rs, btree_map). A canonical
   entry list holds strictly increasing keys in the bytewise order. *)
Definition map_values (f : par -> par) (kvs : list (bytes * par)) : list (bytes * par) :=
  map (fun kv => (fst kv, f (snd kv))) kvs.

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

(* A strictly sorted entry list has pairwise distinct keys. *)
Lemma keys_strictly_sorted_nodup :
  forall kvs, keys_strictly_sorted kvs = true -> NoDup (map fst kvs).
Proof.
  induction kvs as [| kv kvs IH]; intros Hs; [constructor |].
  apply keys_strictly_sorted_cons in Hs as [Hs Hall].
  simpl. constructor; [| exact (IH Hs)].
  intro Hin. apply in_map_iff in Hin as [x [Hx Hinx]].
  rewrite Forall_forall in Hall.
  specialize (Hall x Hinx). unfold bytes_lt in Hall.
  rewrite Hx, bytes_compare_refl in Hall. discriminate.
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

(* canon_map models the insertion into a BTreeMap. The fold inserts the
   entries from right to left and keeps an existing entry, so the last value
   of a duplicate key wins, as in the BTreeMap collect. *)
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

(* ── 5. Constructs, fields, the encoder and the decoder ────────────────── *)

Definition is_some {A : Type} (o : option A) : bool :=
  match o with Some _ => true | None => false end.

Definition is_nil {A : Type} (l : list A) : bool :=
  match l with [] => true | _ :: _ => false end.

(* The keys of named params are param keys in the canonical order. The
   BTreeMap keeps the order, and the checks of the decoders keep the rule of
   decision 27. *)
Definition named_params_ok (named : list (bytes * par)) : bool :=
  keys_strictly_sorted named && forallb is_param_key (map fst named).

(* A construct of the source syntax. CNewSpace holds the annotations, the
   bind count, the uris, the body and the locally_free set of the holder, as
   the Rust NewSpace keeps its plain New. *)
Inductive construct : Type :=
| CUse : par -> par -> construct
| CNewSpace : list (option par) -> nat -> list bytes -> par -> list nat -> construct
| CParams : list par -> list (bytes * par) -> construct
| CQuery : list par -> list (bytes * par) -> construct
| CFree : bytes -> list par -> construct.

(* The one field that carries each construct (decision 112). *)
Inductive carrier : Type :=
| FUse : use_block -> carrier
| FNew : new_block -> carrier
| FParams : option send_params -> carrier
| FQuery : option query_spec -> carrier
| FFree : efree -> carrier.

(* wf states the obligations of the decoders: the identifier rules, the count
   of the annotations, and the empty forms. *)
Definition wf (c : construct) : bool :=
  match c with
  | CUse _ _ => true
  | CNewSpace annots k uris _ _ =>
      (k =? length annots + length uris) && existsb is_some annots
  | CParams _ named => named_params_ok named
  | CQuery payload named => negb (is_nil payload && is_nil named) && named_params_ok named
  | CFree th _ => is_theory_name th
  end.

(* The annotation of one binder is the option of its SpaceAnnotation. *)
Definition encode_annotation (a : option par) : option par := a.

(* build_params writes the params field of a send. Empty params write no
   field, so `x!(a;)` erases to `x!(a)`. *)
Definition build_params (positional : list par) (named : list (bytes * par))
  : option send_params :=
  match positional, named with
  | [], [] => None
  | _, _ => Some (mkSendParams positional named)
  end.

Definition encode (c : construct) : carrier :=
  match c with
  | CUse space body => FUse (mkUseBlock (Some space) (Some body))
  | CNewSpace annots k uris body lf =>
      FNew (mkNew k body uris lf (map encode_annotation annots))
  | CParams positional named => FParams (build_params positional named)
  | CQuery payload named => FQuery (Some (mkQuerySpec payload named))
  | CFree theory args => FFree (mkEFree theory args)
  end.

Definition decode_annotation (a : option par) : option par := a.

(* decode_use_block: both parts must be present. *)
Definition decode_use (u : use_block) : option construct :=
  match u with
  | mkUseBlock (Some space) (Some body) => Some (CUse space body)
  | _ => None
  end.

(* decode_new_space: the count check of annotation_count_matches and at
   least one annotation. The decoder keeps the flag of the holder and reads
   it nowhere. *)
Definition decode_new_space (n : new_block) : option construct :=
  match n with
  | mkNew k body uris lf annots =>
      if (k =? length annots + length uris) && existsb is_some annots
      then Some (CNewSpace (map decode_annotation annots) k uris body lf)
      else None
  end.

(* check_send_params: an absent field is the empty params, a present field
   holds at least one param, and each key is a param key. *)
Definition decode_params (o : option send_params) : option construct :=
  match o with
  | None => Some (CParams [] [])
  | Some (mkSendParams [] []) => None
  | Some (mkSendParams positional named) =>
      if named_params_ok named then Some (CParams positional named) else None
  end.

(* check_query_spec: an absent field is no query, a present field holds a
   payload or a named param, and each key is a param key. *)
Definition decode_query (o : option query_spec) : option construct :=
  match o with
  | None => None
  | Some (mkQuerySpec [] []) => None
  | Some (mkQuerySpec payload named) =>
      if named_params_ok named then Some (CQuery payload named) else None
  end.

(* decode_free_expr: the theory name rule of decision 27. *)
Definition decode_free (f : efree) : option construct :=
  match f with
  | mkEFree theory args => if is_theory_name theory then Some (CFree theory args) else None
  end.

Definition decode (f : carrier) : option construct :=
  match f with
  | FUse u => decode_use u
  | FNew n => decode_new_space n
  | FParams o => decode_params o
  | FQuery o => decode_query o
  | FFree e => decode_free e
  end.

Lemma map_injective :
  forall (A B : Type) (f : A -> B),
    (forall x y, f x = f y -> x = y) ->
    forall l1 l2, map f l1 = map f l2 -> l1 = l2.
Proof.
  intros A B f Hf. induction l1 as [| x l1 IH]; intros [| y l2] H;
    simpl in H; try discriminate; [reflexivity |].
  injection H as H1 H2. f_equal; [exact (Hf _ _ H1) | exact (IH _ H2)].
Qed.

(* Headline 4. Two well-formed constructs with the same field are equal. The
   field keeps every part of its construct. An absent annotation stays
   apart from every present one. *)
Theorem field_encode_injective :
  forall c1 c2, wf c1 = true -> wf c2 = true -> encode c1 = encode c2 -> c1 = c2.
Proof.
  assert (Hann : forall a b : option par, encode_annotation a = encode_annotation b -> a = b).
  { intros a b H. exact H. }
  intros [s1 b1 | a1 k1 u1 b1 l1 | p1 n1 | p1 n1 | t1 x1]
         [s2 b2 | a2 k2 u2 b2 l2 | p2 n2 | p2 n2 | t2 x2] _ _ E;
    simpl in E; try discriminate E.
  - injection E as -> ->. reflexivity.
  - injection E as E1 E2 E3 E4 E5. subst k2 b2 u2 l2.
    f_equal. exact (map_injective _ _ _ Hann _ _ E5).
  - unfold build_params in E. destruct p1, n1, p2, n2; simpl in E; congruence.
  - injection E as -> ->. reflexivity.
  - injection E as -> ->. reflexivity.
Qed.

(* The superseded encoding: Nil stands for an absent annotation. *)
Definition nil_for_absent (a : option par) : par :=
  match a with Some space => space | None => nil_par end.

(* Headline 12. The Nil-for-absent form merges an absent annotation with the
   annotation Nil, so substitution of Nil for an annotation variable would
   erase an annotation unobserved. The option form of SpaceAnnotation
   keeps the two apart and is injective. *)
Theorem absent_annotation_not_nil :
  nil_for_absent None = nil_for_absent (Some nil_par) /\
  (None : option par) <> Some nil_par /\
  (forall a b : option par, encode_annotation a = encode_annotation b -> a = b) /\
  encode_annotation None <> encode_annotation (Some nil_par).
Proof.
  split; [reflexivity |].
  split; [discriminate |].
  split.
  - intros a b H. exact H.
  - unfold encode_annotation. discriminate.
Qed.

Lemma encode_annotations_id : forall annots, map encode_annotation annots = annots.
Proof. intros annots. unfold encode_annotation. apply map_id. Qed.

Lemma decode_annotations_id : forall annots, map decode_annotation annots = annots.
Proof. intros annots. unfold decode_annotation. apply map_id. Qed.

(* Headline 3. The decoder inverts the encoder on each well-formed construct. *)
Theorem field_decode_encode : forall c, wf c = true -> decode (encode c) = Some c.
Proof.
  intros [space body | annots k uris body lf | positional named | payload named | theory args]
    Hwf; simpl in Hwf.
  - reflexivity.
  - unfold encode, decode, decode_new_space.
    rewrite encode_annotations_id.
    apply andb_true_iff in Hwf as [Hcount Hany].
    match goal with
    | |- (if ?c then _ else _) = _ => destruct c eqn:Echeck
    end.
    + rewrite decode_annotations_id. reflexivity.
    + try rewrite Hcount in Echeck; try rewrite Hany in Echeck; simpl in Echeck;
        first [discriminate Echeck | discriminate Hany].
  - unfold encode, decode.
    destruct positional as [| p ps]; destruct named as [| kv kvs]; simpl; try reflexivity;
      rewrite Hwf; reflexivity.
  - unfold encode, decode.
    destruct payload as [| p ps]; destruct named as [| kv kvs]; simpl in Hwf;
      try discriminate Hwf; simpl; try reflexivity; rewrite Hwf; reflexivity.
  - unfold encode, decode, decode_free. rewrite Hwf. reflexivity.
Qed.

(* Headline 5. The decoder accepts exactly the canonical images: a field
   decodes to a construct exactly when the construct is well formed and the
   encoder writes that field. *)
Theorem field_decode_sound :
  forall f c, decode f = Some c <-> wf c = true /\ encode c = f.
Proof.
  intros f c. split.
  - destruct f as [u | n | o | o | e]; simpl; intro H.
    + destruct u as [s b]. destruct s as [s |]; [| discriminate].
      destruct b as [b |]; [| discriminate]. simpl in H.
      injection H as <-. split; reflexivity.
    + destruct n as [k body uris lf annots]. unfold decode_new_space in H.
      destruct ((k =? length annots + length uris) && existsb is_some annots) eqn:Hc;
        [| discriminate].
      injection H as <-. split.
      * simpl. rewrite decode_annotations_id. exact Hc.
      * unfold encode. rewrite encode_annotations_id, decode_annotations_id. reflexivity.
    + destruct o as [[positional named] |].
      * destruct positional as [| p ps]; destruct named as [| kv kvs]; simpl in H.
        -- discriminate.
        -- destruct (named_params_ok (kv :: kvs)) eqn:Hk; [| discriminate].
           injection H as <-. split; [exact Hk | reflexivity].
        -- injection H as <-. split; reflexivity.
        -- destruct (named_params_ok (kv :: kvs)) eqn:Hk; [| discriminate].
           injection H as <-. split; [exact Hk | reflexivity].
      * injection H as <-. split; reflexivity.
    + destruct o as [[payload named] |]; [| discriminate].
      destruct payload as [| p ps]; destruct named as [| kv kvs]; simpl in H.
      * discriminate.
      * destruct (named_params_ok (kv :: kvs)) eqn:Hk; [| discriminate].
        injection H as <-. split; [simpl; exact Hk | reflexivity].
      * injection H as <-. split; reflexivity.
      * destruct (named_params_ok (kv :: kvs)) eqn:Hk; [| discriminate].
        injection H as <-. split; [simpl; exact Hk | reflexivity].
    + destruct e as [theory args]. unfold decode_free in H.
      destruct (is_theory_name theory) eqn:Ht; [| discriminate].
      injection H as <-. split; [exact Ht | reflexivity].
  - intros [Hwf Henc]. rewrite <- Henc. exact (field_decode_encode c Hwf).
Qed.

(* params_of_source models collect_send_params: a BTreeMap collects the
   named params, so their keys are sorted and unique. The positional params
   keep the source order. *)
Definition params_of_source (positional : list par) (kvs : list (bytes * par)) : construct :=
  CParams positional (canon_map kvs).

(* Headline 6. Named keys are sorted and unique, the source order of the
   named params does not matter, the positional order stays, and a decoder
   accepts only named params in the canonical form. *)
Theorem named_params_canonical :
  (forall positional kvs,
      match params_of_source positional kvs with
      | CParams pos named =>
          pos = positional /\ keys_strictly_sorted named = true /\ NoDup (map fst named)
      | _ => False
      end) /\
  (forall kvs, canon_map kvs = kvs <-> keys_strictly_sorted kvs = true) /\
  (forall kvs1 kvs2,
      NoDup (map fst kvs1) -> Permutation kvs1 kvs2 -> canon_map kvs1 = canon_map kvs2) /\
  (forall kvs, NoDup (map fst kvs) -> Permutation (canon_map kvs) kvs) /\
  (forall positional named, wf (CParams positional named) = true -> canon_map named = named) /\
  (forall payload named, wf (CQuery payload named) = true -> canon_map named = named).
Proof.
  split.
  { intros positional kvs. simpl. split; [reflexivity |].
    pose proof (canon_map_sorted kvs) as Hs. split; [exact Hs |].
    apply keys_strictly_sorted_nodup. exact Hs. }
  split.
  { intros kvs. split; [intros H; rewrite <- H; apply canon_map_sorted | apply canon_map_id]. }
  split; [exact canon_map_permutation_invariant |].
  split; [exact canon_map_perm |].
  split.
  { intros positional named H. apply canon_map_id. simpl in H.
    unfold named_params_ok in H. apply andb_true_iff in H as [Hs _]. exact Hs. }
  { intros payload named H. apply canon_map_id. simpl in H.
    apply andb_true_iff in H as [_ H].
    unfold named_params_ok in H. apply andb_true_iff in H as [Hs _]. exact Hs. }
Qed.

(* The one field of each construct. *)
Inductive field_id : Type :=
| FieldUseBlocks
| FieldSpaceTypes
| FieldSendParams
| FieldQuerySpec
| FieldEFreeBody.

Definition carrier_field (f : carrier) : field_id :=
  match f with
  | FUse _ => FieldUseBlocks
  | FNew _ => FieldSpaceTypes
  | FParams _ => FieldSendParams
  | FQuery _ => FieldQuerySpec
  | FFree _ => FieldEFreeBody
  end.

Definition construct_field (c : construct) : field_id :=
  match c with
  | CUse _ _ => FieldUseBlocks
  | CNewSpace _ _ _ _ _ => FieldSpaceTypes
  | CParams _ _ => FieldSendParams
  | CQuery _ _ => FieldQuerySpec
  | CFree _ _ => FieldEFreeBody
  end.

(* Headline 7. The encoder writes each construct into its own field only,
   and a decoder reads a construct only from that field. No construct has a
   second carrier. *)
Theorem construct_has_one_carrier :
  (forall c, carrier_field (encode c) = construct_field c) /\
  (forall f c, decode f = Some c -> carrier_field f = construct_field c).
Proof.
  split.
  - intros [| | | |]; reflexivity.
  - intros f c H. apply field_decode_sound in H as [_ Henc].
    rewrite <- Henc. destruct c; reflexivity.
Qed.

(* ── 6. The derived flags of the holders ───────────────────────────────── *)

(* No new message has derived flags. Par, New and the other old messages
   keep locally_free and connective_used. sole_expr reads the one expression
   of a Par and ignores the two flags of the Par, as sole_component does. *)
Definition sole_expr (p : par) : option expr :=
  match p with
  | mkPar [] [] [] [e] [] _ _ [] => Some e
  | _ => None
  end.

(* decode_free_value: the value of free Th(a...). *)
Definition decode_free_value (value : par) : option construct :=
  match sole_expr value with
  | Some (EFreeBody free) => decode_free free
  | _ => None
  end.

(* split_trailing_params: the forwarded params of a send (the last data
   element). *)
Definition decode_params_value (value : par) : option construct :=
  match sole_expr value with
  | Some (EParamsBody sp) => decode_params (Some sp)
  | _ => None
  end.

(* A stale flag is any other value of the derived fields of a holder. *)
Definition with_flags (p : par) (lf : list nat) (cu : bool) : par :=
  match p with
  | mkPar ss rs ns es ufs _ _ ubs => mkPar ss rs ns es ufs lf cu ubs
  end.

Definition with_new_flag (n : new_block) (lf : list nat) : new_block :=
  match n with
  | mkNew k body uris _ annots => mkNew k body uris lf annots
  end.

Definition with_construct_flag (lf : list nat) (c : construct) : construct :=
  match c with
  | CNewSpace annots k uris body _ => CNewSpace annots k uris body lf
  | other => other
  end.

Definition with_carrier_flag (lf : list nat) (f : carrier) : carrier :=
  match f with
  | FNew n => FNew (with_new_flag n lf)
  | other => other
  end.

(* Headline 15. No decoder reads a derived flag of a holder. The decoders
   read the fields of the new messages, which have no flags, and the two
   holder paths (the sole expression of a Par and the New of a space
   annotation) do not read a flag. So a stale flag never rejects a legal
   construct. This replaces the three I-20 headlines of the marker tuples. *)
Theorem decode_ignores_flags :
  (forall p lf cu, sole_expr (with_flags p lf cu) = sole_expr p) /\
  (forall v lf cu, decode_free_value (with_flags v lf cu) = decode_free_value v) /\
  (forall v lf cu, decode_params_value (with_flags v lf cu) = decode_params_value v) /\
  (forall n lf,
      decode_new_space (with_new_flag n lf) =
      option_map (with_construct_flag lf) (decode_new_space n)) /\
  (forall c lf, wf c = true ->
      decode (with_carrier_flag lf (encode c)) = Some (with_construct_flag lf c)).
Proof.
  assert (Hsole : forall p lf cu, sole_expr (with_flags p lf cu) = sole_expr p).
  { intros [ss rs ns es ufs l0 c0 ubs] lf cu. reflexivity. }
  assert (Hnew : forall n lf,
             decode_new_space (with_new_flag n lf) =
             option_map (with_construct_flag lf) (decode_new_space n)).
  { intros [k body uris l0 annots] lf. simpl.
    destruct ((k =? length annots + length uris) && existsb is_some annots); reflexivity. }
  split; [exact Hsole |].
  split.
  { intros v lf cu. unfold decode_free_value. rewrite Hsole. reflexivity. }
  split.
  { intros v lf cu. unfold decode_params_value. rewrite Hsole. reflexivity. }
  split; [exact Hnew |].
  intros c lf Hwf.
  destruct c as [space body | annots k uris body l0 | positional named | payload named
                 | theory args]; simpl in Hwf.
  - reflexivity.
  - assert (Hd : decode_new_space (mkNew k body uris l0 (map encode_annotation annots)) =
                 Some (CNewSpace annots k uris body l0))
      by exact (field_decode_encode (CNewSpace annots k uris body l0) Hwf).
    change (decode_new_space (with_new_flag (mkNew k body uris l0 (map encode_annotation annots)) lf) =
            Some (CNewSpace annots k uris body lf)).
    rewrite Hnew, Hd. reflexivity.
  - apply (field_decode_encode (CParams positional named)). exact Hwf.
  - apply (field_decode_encode (CQuery payload named)). exact Hwf.
  - apply (field_decode_encode (CFree theory args)). exact Hwf.
Qed.

(* ── 7. The term schedule of the reducer ───────────────────────────────── *)

(* The terms of a Par in process position, in the order of evaluation_terms.
   The omitted fields of the model come before the expressions and cost terms
   of the real Par, and the use blocks are last. *)
Inductive generated_message : Type :=
| GMSend : send -> generated_message
| GMReceive : receive -> generated_message
| GMNew : new_block -> generated_message
| GMExpr : expr -> generated_message
| GMUseBlock : use_block -> generated_message.

(* is_evaluated_process_expr: a variable or a method call. No marker tuple is
   a process after decision 112. *)
Definition is_evaluated_process_expr (e : expr) : bool :=
  match e with
  | EVar _ => true
  | EMethod _ _ _ => true
  | _ => false
  end.

(* ordinary_terms is the list of all terms that are not use blocks. *)
Definition ordinary_terms (p : par) : list generated_message :=
  match p with
  | mkPar sends receives news exprs _ _ _ _ =>
      map GMSend sends ++ map GMReceive receives ++ map GMNew news
      ++ map GMExpr (filter is_evaluated_process_expr exprs)
  end.

(* evaluation_terms: the use blocks are appended after every ordinary term. *)
Definition evaluation_terms (p : par) : list generated_message :=
  ordinary_terms p ++ map GMUseBlock (use_blocks_of p).

Definition i16_max : nat := 2 ^ 15 - 1.

(* evaluation_random (util/mod.rs): the random state of term `index` of a Par
   with `term_count` terms. *)
Definition evaluation_random {R : Type} (split_byte split_short : R -> nat -> R)
  (r : R) (index term_count : nat) : option R :=
  if (i16_max <? term_count) || (term_count <=? index) then None
  else if term_count =? 1 then Some r
  else if 256 <? term_count then Some (split_short r index)
  else Some (split_byte r index).

(* eval_inner gives term i the random state evaluation_random r i n. *)
Definition term_schedule {R : Type} (split_byte split_short : R -> nat -> R) (r : R)
  (terms : list generated_message) : list (generated_message * option R) :=
  map (fun it => (snd it, evaluation_random split_byte split_short r (fst it) (length terms)))
    (combine (seq 0 (length terms)) terms).

(* Headline 10. A Par without use blocks has the term list and the schedule of
   the base schema. A term that is not a use block keeps its index in every
   Par, and the use blocks follow every other term. *)
Theorem schedule_unchanged_field_free :
  (forall p, use_blocks_of p = [] -> evaluation_terms p = ordinary_terms p) /\
  (forall p i t,
      nth_error (ordinary_terms p) i = Some t -> nth_error (evaluation_terms p) i = Some t) /\
  (forall p i u,
      nth_error (use_blocks_of p) i = Some u ->
      nth_error (evaluation_terms p) (length (ordinary_terms p) + i) = Some (GMUseBlock u)) /\
  (forall (R : Type) (split_byte split_short : R -> nat -> R) (r : R) (p : par),
      use_blocks_of p = [] ->
      term_schedule split_byte split_short r (evaluation_terms p) =
      term_schedule split_byte split_short r (ordinary_terms p)).
Proof.
  assert (Hplain : forall p, use_blocks_of p = [] -> evaluation_terms p = ordinary_terms p).
  { intros p H. unfold evaluation_terms. rewrite H. apply app_nil_r. }
  split; [exact Hplain |].
  split.
  { intros p i t H. unfold evaluation_terms.
    rewrite nth_error_app1; [exact H |].
    apply nth_error_Some. rewrite H. discriminate. }
  split.
  { intros p i u H. unfold evaluation_terms.
    rewrite nth_error_app2 by lia.
    replace (length (ordinary_terms p) + i - length (ordinary_terms p)) with i by lia.
    rewrite nth_error_map, H. reflexivity. }
  intros R split_byte split_short r p H. rewrite (Hplain p H). reflexivity.
Qed.

(* ordinary_terms holds no use block. *)
Lemma ordinary_terms_no_use_block :
  forall p u, ~ In (GMUseBlock u) (ordinary_terms p).
Proof.
  intros [ss rs ns es ufs lf cu ubs] u H. unfold ordinary_terms in H.
  repeat rewrite in_app_iff in H.
  destruct H as [H | [H | [H | H]]]; apply in_map_iff in H as [x [Hx _]]; discriminate Hx.
Qed.

(* Headline 9. A use block runs only as an element of the use_blocks field of
   the Par under evaluation. A use block in the channel or in the data of a
   send is inert until that Par itself runs in process position. The list of
   generated terms has no other source of use blocks. *)
Theorem fields_inert_outside_process_position :
  (forall p u, In (GMUseBlock u) (evaluation_terms p) <-> In u (use_blocks_of p)) /\
  (forall chan data persistent params sends receives news exprs ufs lf cu ubs u,
      In (mkSend chan data persistent params) sends ->
      (In u (use_blocks_of chan) \/ (exists d, In d data /\ In u (use_blocks_of d))) ->
      ~ In u ubs ->
      ~ In (GMUseBlock u) (evaluation_terms (mkPar sends receives news exprs ufs lf cu ubs))).
Proof.
  assert (Hrun : forall p u, In (GMUseBlock u) (evaluation_terms p) <-> In u (use_blocks_of p)).
  { intros p u. unfold evaluation_terms. rewrite in_app_iff, in_map_iff. split.
    - intros [H | [x [Hx Hin]]].
      + exfalso. exact (ordinary_terms_no_use_block p u H).
      + injection Hx as ->. exact Hin.
    - intro H. right. exists u. split; [reflexivity | exact H]. }
  split; [exact Hrun |].
  intros chan data persistent params sends receives news exprs ufs lf cu ubs u _ _ Hnot H.
  apply Hrun in H. exact (Hnot H).
Qed.

(* ── 8. The former marker tuples ───────────────────────────────────────── *)

(* The heads of the marker tuples of decision 18. *)
Definition former_marker_ids : list bytes :=
  [[224; 208]; [224; 209]; [224; 210]; [224; 211]; [224; 212]].

(* route_class_of is the routing class of a channel: a sole unforgeable name
   routes by its id and every other channel routes as a forged channel (rule
   R5). *)
Inductive route_class : Type :=
| RouteUnforgeable : bytes -> route_class
| RouteForged : route_class.

Definition route_class_of (c : par) : route_class :=
  match c with
  | mkPar [] [] [] [] [id] _ _ [] => RouteUnforgeable id
  | _ => RouteForged
  end.

(* tuple_matches is the tuple arm of the spatial matcher (matcher/match.rs,
   EList and ETuple): the arity is equal and each element matches. No element
   receives a special treatment. *)
Fixpoint tuple_matches (elem : par -> par -> bool) (pattern value : list par) : bool :=
  match pattern, value with
  | [], [] => true
  | p :: ps, v :: vs => elem p v && tuple_matches elem ps vs
  | _, _ => false
  end.

(* Headline 8. A tuple with a former marker head is an ordinary tuple: in
   process position it is a value and no term, it routes as every other
   forged channel, and it matches element by element, the head included. The
   statement holds for every head, in particular for the five former marker
   ids of former_marker_ids. *)
Theorem former_marker_tuple_is_ordinary :
  forall (id : bytes) (args : list par),
    let t := ETuple (unforgeable_par id :: args) in
    is_evaluated_process_expr t = false /\
    route_class_of (expr_par t) = RouteForged /\
    (forall elem pat,
        tuple_matches elem (unforgeable_par id :: pat) (unforgeable_par id :: args) =
        elem (unforgeable_par id) (unforgeable_par id) && tuple_matches elem pat args).
Proof.
  intros id args. simpl.
  split; [reflexivity |].
  split; [reflexivity |].
  intros elem pat. reflexivity.
Qed.

(* ── 9. Substitution ───────────────────────────────────────────────────── *)

(* The substitution of a Par is a parameter: env is the type of the
   environments, shift pushes k binders onto an environment and subst
   substitutes in one Par. The carrier substitution below follows the rules
   of substitute.rs. The body of a New takes the environment under its
   binders. Every other nested Par takes the environment of the holder. In
   particular an annotation of a New takes the outer environment, because
   the annotation binds no name of the New. *)
Section Substitution.
  Variable env : Type.
  Variable shift : nat -> env -> env.
  Variable subst : env -> par -> par.

  Definition subst_named (sigma : env) (kvs : list (bytes * par)) : list (bytes * par) :=
    map_values (subst sigma) kvs.

  Definition subst_params (sigma : env) (sp : send_params) : send_params :=
    match sp with
    | mkSendParams positional named =>
        mkSendParams (map (subst sigma) positional) (subst_named sigma named)
    end.

  Definition subst_query (sigma : env) (q : query_spec) : query_spec :=
    match q with
    | mkQuerySpec payload named =>
        mkQuerySpec (map (subst sigma) payload) (subst_named sigma named)
    end.

  Definition subst_annotation (sigma : env) (a : option par) : option par :=
    option_map (subst sigma) a.

  Definition subst_new_block (sigma : env) (n : new_block) : new_block :=
    match n with
    | mkNew k body uris lf annots =>
        mkNew k (subst (shift k sigma) body) uris lf (map (subst_annotation sigma) annots)
    end.

  Definition subst_carrier (sigma : env) (f : carrier) : carrier :=
    match f with
    | FUse (mkUseBlock space body) =>
        FUse (mkUseBlock (option_map (subst sigma) space) (option_map (subst sigma) body))
    | FNew n => FNew (subst_new_block sigma n)
    | FParams o => FParams (option_map (subst_params sigma) o)
    | FQuery o => FQuery (option_map (subst_query sigma) o)
    | FFree (mkEFree theory args) => FFree (mkEFree theory (map (subst sigma) args))
    end.

  (* The semantic substitution of a construct: where each part of the source
     form takes its environment. *)
  Definition subst_construct (sigma : env) (c : construct) : construct :=
    match c with
    | CUse space body => CUse (subst sigma space) (subst sigma body)
    | CNewSpace annots k uris body lf =>
        CNewSpace (map (subst_annotation sigma) annots) k uris (subst (shift k sigma) body) lf
    | CParams positional named =>
        CParams (map (subst sigma) positional) (subst_named sigma named)
    | CQuery payload named =>
        CQuery (map (subst sigma) payload) (subst_named sigma named)
    | CFree theory args => CFree theory (map (subst sigma) args)
    end.

  Lemma named_params_ok_subst :
    forall sigma named, named_params_ok (subst_named sigma named) = named_params_ok named.
  Proof.
    intros sigma named. unfold named_params_ok, subst_named.
    rewrite keys_strictly_sorted_map_values, map_values_fst. reflexivity.
  Qed.

  Lemma is_nil_map : forall (A B : Type) (f : A -> B) l, is_nil (map f l) = is_nil l.
  Proof. intros A B f [| x l]; reflexivity. Qed.

  Lemma decode_params_some :
    forall positional named,
      decode_params (Some (mkSendParams positional named)) =
      if is_nil positional && is_nil named then None
      else if named_params_ok named then Some (CParams positional named) else None.
  Proof. intros [| p ps] [| kv kvs]; reflexivity. Qed.

  Lemma decode_query_some :
    forall payload named,
      decode_query (Some (mkQuerySpec payload named)) =
      if is_nil payload && is_nil named then None
      else if named_params_ok named then Some (CQuery payload named) else None.
  Proof. intros [| p ps] [| kv kvs]; reflexivity. Qed.

  Lemma existsb_is_some_subst :
    forall sigma annots,
      existsb is_some (map (subst_annotation sigma) annots) = existsb is_some annots.
  Proof.
    intros sigma annots. induction annots as [| a rest IH]; [reflexivity |].
    simpl. rewrite IH. destruct a; reflexivity.
  Qed.

  (* Headline 11. Decoding commutes with substitution: the decoder accepts a
     substituted field exactly when it accepts the field, and the construct
     of the substituted field is the substitution of the construct. The two
     counts of the annotation check (the entries and the present spaces) do
     not change, and the annotations take the outer environment. *)
  Theorem fields_commute_with_subst :
    forall sigma f,
      decode (subst_carrier sigma f) = option_map (subst_construct sigma) (decode f).
  Proof.
    intros sigma f. destruct f as [u | n | o | o | e].
    - destruct u as [[s |] [b |]]; reflexivity.
    - destruct n as [k body uris lf annots]. unfold subst_carrier, subst_new_block, decode, decode_new_space.
      rewrite length_map, existsb_is_some_subst.
      destruct ((k =? length annots + length uris) && existsb is_some annots) eqn:Hc;
        [| reflexivity].
      simpl. rewrite !decode_annotations_id. reflexivity.
    - destruct o as [[positional named] |]; [| reflexivity].
      cbn [subst_carrier decode option_map subst_params].
      rewrite !decode_params_some, is_nil_map, named_params_ok_subst.
      unfold subst_named, map_values. rewrite is_nil_map.
      destruct (is_nil positional && is_nil named); [reflexivity |].
      destruct (named_params_ok named); reflexivity.
    - destruct o as [[payload named] |]; [| reflexivity].
      cbn [subst_carrier decode option_map subst_query].
      rewrite !decode_query_some, is_nil_map, named_params_ok_subst.
      unfold subst_named, map_values. rewrite is_nil_map.
      destruct (is_nil payload && is_nil named); [reflexivity |].
      destruct (named_params_ok named); reflexivity.
    - destruct e as [theory args]. unfold subst_carrier, decode, decode_free.
      destruct (is_theory_name theory); reflexivity.
  Qed.
End Substitution.

(* ── 10. The proto3 bytes and the bincode length ───────────────────────── *)

(* Both writers take the schema as an argument. The old schema has none of
   the new fields. A field that a schema does not have writes nothing. *)
Inductive schema : Type :=
| OldSchema
| NewSchema.

Definition when_new (s : schema) (out : list nat) : list nat :=
  match s with NewSchema => out | OldSchema => [] end.

Definition new_field_len (s : schema) (n : nat) : nat :=
  match s with NewSchema => n | OldSchema => 0 end.

(* A term without a new field is the common fragment of the two schemas. *)
Definition send_field_free (free : par -> bool) (m : send) : bool :=
  match m with
  | mkSend chan data _ params => free chan && forallb free data && negb (is_some params)
  end.

Definition bind_field_free (free : par -> bool) (b : bind) : bool :=
  match b with
  | mkBind patterns source query =>
      forallb free patterns && free source && negb (is_some query)
  end.

Definition receive_field_free (free : par -> bool) (r : receive) : bool :=
  match r with
  | mkReceive binds body => forallb (bind_field_free free) binds && free body
  end.

Definition new_field_free (free : par -> bool) (n : new_block) : bool :=
  match n with
  | mkNew _ body _ _ annots => free body && is_nil annots
  end.

Definition expr_field_free (free : par -> bool) (e : expr) : bool :=
  match e with
  | EVar _ => true
  | EMethod _ target args => free target && forallb free args
  | ETuple ps => forallb free ps
  | EFreeBody _ => false
  | EParamsBody _ => false
  | EOther _ => true
  end.

Fixpoint field_free (p : par) {struct p} : bool :=
  match p with
  | mkPar sends receives news exprs _ _ _ ubs =>
      forallb (send_field_free field_free) sends
      && forallb (receive_field_free field_free) receives
      && forallb (new_field_free field_free) news
      && forallb (expr_field_free field_free) exprs
      && is_nil ubs
  end.

(* The proto3 writer. frame writes one length-delimited field: the tag, the
   length and the payload. The default-field rule of proto3 appears in the
   writers of the empty values: an empty repeated field, an absent message
   and an unset oneof write nothing. *)
Definition frame (tag : nat) (payload : list nat) : list nat :=
  tag :: length payload :: payload.

Definition repeated (tag : nat) (items : list (list nat)) : list nat :=
  flat_map (frame tag) items.

Definition bytes_field (tag : nat) (b : list nat) : list nat :=
  if is_nil b then [] else frame tag b.

Definition bool_field (tag : nat) (b : bool) : list nat :=
  if b then [tag; 1] else [].

Definition scalar_field (tag : nat) (n : nat) : list nat :=
  if n =? 0 then [] else [tag; n].

Definition optional (tag : nat) (payload : option (list nat)) : list nat :=
  match payload with
  | None => []
  | Some bytes_out => frame tag bytes_out
  end.

Definition new_repeated (tag : nat) (items : list (list nat)) : list nat :=
  if is_nil items then [] else repeated tag items.

Definition proto_named (f : par -> list nat) (named : list (bytes * par)) : list nat :=
  repeated 2 (map (fun kv => frame 1 (fst kv) ++ frame 2 (f (snd kv))) named).

Definition proto_send_params (f : par -> list nat) (sp : send_params) : list nat :=
  match sp with
  | mkSendParams positional named => repeated 1 (map f positional) ++ proto_named f named
  end.

Definition proto_query_spec (f : par -> list nat) (q : query_spec) : list nat :=
  match q with
  | mkQuerySpec payload named => repeated 1 (map f payload) ++ proto_named f named
  end.

Definition proto_send (s : schema) (f : par -> list nat) (m : send) : list nat :=
  match m with
  | mkSend chan data persistent params =>
      frame 1 (f chan) ++ repeated 2 (map f data) ++ bool_field 3 persistent
      ++ when_new s (optional params_number (option_map (proto_send_params f) params))
  end.

Definition proto_bind (s : schema) (f : par -> list nat) (b : bind) : list nat :=
  match b with
  | mkBind patterns source query =>
      repeated 1 (map f patterns) ++ frame 2 (f source)
      ++ when_new s (optional query_spec_number (option_map (proto_query_spec f) query))
  end.

Definition proto_receive (s : schema) (f : par -> list nat) (r : receive) : list nat :=
  match r with
  | mkReceive binds body => repeated 1 (map (proto_bind s f) binds) ++ frame 2 (f body)
  end.

Definition proto_annotation (f : par -> list nat) (a : option par) : list nat :=
  optional 1 (option_map f a).

Definition proto_new (s : schema) (f : par -> list nat) (n : new_block) : list nat :=
  match n with
  | mkNew k body uris lf annots =>
      scalar_field 1 k ++ frame 2 (f body) ++ repeated 3 (map (frame 1) uris)
      ++ bytes_field 5 lf
      ++ when_new s (new_repeated space_types_number (map (proto_annotation f) annots))
  end.

Definition proto_efree (f : par -> list nat) (e : efree) : list nat :=
  match e with
  | mkEFree theory args => frame 1 theory ++ repeated 2 (map f args)
  end.

Definition proto_var (v : var) : list nat :=
  match v with
  | VBound i => [1; i]
  | VFree i => [2; i]
  | VWildcard => [3]
  end.

Definition proto_expr (s : schema) (f : par -> list nat) (e : expr) : list nat :=
  match e with
  | EVar v => frame 19 (proto_var v)
  | EMethod name target args =>
      frame 24 (frame 1 name ++ frame 2 (f target) ++ repeated 3 (map f args))
  | ETuple ps => frame 21 (repeated 1 (map f ps))
  | EFreeBody free => when_new s (frame e_free_body_number (proto_efree f free))
  | EParamsBody sp => when_new s (frame e_params_body_number (proto_send_params f sp))
  | EOther n => [n; 0]
  end.

Definition proto_use (f : par -> list nat) (u : use_block) : list nat :=
  match u with
  | mkUseBlock space body =>
      optional 1 (option_map f space) ++ optional 2 (option_map f body)
  end.

Fixpoint proto (s : schema) (p : par) {struct p} : list nat :=
  match p with
  | mkPar sends receives news exprs ufs lf cu ubs =>
      repeated 1 (map (proto_send s (proto s)) sends)
      ++ repeated 2 (map (proto_receive s (proto s)) receives)
      ++ repeated 4 (map (proto_new s (proto s)) news)
      ++ repeated 5 (map (proto_expr s (proto s)) exprs)
      ++ repeated 7 (map (frame 1) ufs)
      ++ bytes_field 9 lf
      ++ bool_field 10 cu
      ++ when_new s (new_repeated use_blocks_number (map (proto_use (proto s)) ubs))
  end.

(* The relation of the two functions on one layer of the term. *)
Lemma map_ext_holds :
  forall (A B : Type) (f g : A -> B) (holds : A -> Prop) (free : A -> bool),
    (forall x, holds x -> free x = true -> f x = g x) ->
    forall l, Forall holds l -> forallb free l = true -> map f l = map g l.
Proof.
  intros A B f g holds free Hsame l Hall Hfree.
  induction Hall as [| x rest Hx Hrest IH]; [reflexivity |].
  simpl in Hfree. apply andb_true_iff in Hfree as [Hx' Hr].
  simpl. rewrite (Hsame x Hx Hx'), (IH Hr). reflexivity.
Qed.

Section ProtoLayers.
  Variables (f g : par -> list nat) (free : par -> bool).

  Definition same_on_free (p : par) : Prop := free p = true -> f p = g p.

  Let same_list :
    forall l, Forall same_on_free l -> forallb free l = true -> map f l = map g l.
  Proof. apply map_ext_holds. intros x H Hf. exact (H Hf). Qed.

  Lemma proto_send_same :
    forall m, holds_send same_on_free m -> send_field_free free m = true ->
      proto_send NewSchema f m = proto_send OldSchema g m.
  Proof.
    intros [chan data persistent params] [Hc [Hd _]] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hfree Hn].
    apply andb_true_iff in Hfree as [Hc' Hd'].
    destruct params as [sp |]; [discriminate Hn |].
    cbn [proto_send when_new option_map optional].
    rewrite (Hc Hc'), (same_list data Hd Hd'). reflexivity.
  Qed.

  Lemma proto_bind_same :
    forall b, holds_bind same_on_free b -> bind_field_free free b = true ->
      proto_bind NewSchema f b = proto_bind OldSchema g b.
  Proof.
    intros [patterns source query] [Hp [Hs _]] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hfree Hn].
    apply andb_true_iff in Hfree as [Hp' Hs'].
    destruct query as [q |]; [discriminate Hn |].
    cbn [proto_bind when_new option_map optional].
    rewrite (same_list patterns Hp Hp'), (Hs Hs'). reflexivity.
  Qed.

  Lemma proto_receive_same :
    forall r, holds_receive same_on_free r -> receive_field_free free r = true ->
      proto_receive NewSchema f r = proto_receive OldSchema g r.
  Proof.
    intros [binds body] [Hb Hd] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hb' Hd'].
    cbn [proto_receive].
    rewrite (map_ext_holds _ _ _ _ _ _ proto_bind_same binds Hb Hb'), (Hd Hd').
    reflexivity.
  Qed.

  Lemma proto_new_same :
    forall n, holds_new same_on_free n -> new_field_free free n = true ->
      proto_new NewSchema f n = proto_new OldSchema g n.
  Proof.
    intros [k body uris lf annots] [Hb _] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hb' Hn].
    destruct annots as [| a rest]; [| discriminate Hn].
    cbn [proto_new when_new map new_repeated is_nil].
    rewrite (Hb Hb'). reflexivity.
  Qed.

  Lemma proto_expr_same :
    forall e, holds_expr same_on_free e -> expr_field_free free e = true ->
      proto_expr NewSchema f e = proto_expr OldSchema g e.
  Proof.
    intros [v | name target args | ps | fb | sp | n] H Hfree; simpl in Hfree;
      try discriminate Hfree.
    - reflexivity.
    - destruct H as [Ht Ha]. apply andb_true_iff in Hfree as [Ht' Ha'].
      cbn [proto_expr]. rewrite (Ht Ht'), (same_list args Ha Ha'). reflexivity.
    - cbn [proto_expr]. rewrite (same_list ps H Hfree). reflexivity.
    - reflexivity.
  Qed.
End ProtoLayers.

(* Headline 13. A term without a new field has the same proto3 bytes in the
   two schemas: every new field is empty or absent and writes nothing, and
   each old field writes its nested Pars with the same writer. *)
Theorem prost_bytes_unchanged_field_free :
  forall p, field_free p = true -> proto NewSchema p = proto OldSchema p.
Proof.
  refine (par_nested_ind (same_on_free (proto NewSchema) (proto OldSchema) field_free) _).
  intros sends receives news exprs ufs lf cu ubs Hs Hr Hn He _ Hfree.
  cbn [field_free] in Hfree.
  repeat rewrite andb_true_iff in Hfree.
  destruct Hfree as [[[[Hfs Hfr] Hfn] Hfe] Hu].
  destruct ubs as [| u us]; [| discriminate Hu].
  cbn [proto when_new map new_repeated is_nil].
  rewrite (map_ext_holds _ _ _ _ _ _ (proto_send_same _ _ _) sends Hs Hfs).
  rewrite (map_ext_holds _ _ _ _ _ _ (proto_receive_same _ _ _) receives Hr Hfr).
  rewrite (map_ext_holds _ _ _ _ _ _ (proto_new_same _ _ _) news Hn Hfn).
  rewrite (map_ext_holds _ _ _ _ _ _ (proto_expr_same _ _ _) exprs He Hfe).
  reflexivity.
Qed.

(* The bincode writer of serde. A Vec writes an 8-byte length and then its
   elements. An Option writes a 1-byte tag and then its value. A bool writes
   one byte. A string or a byte array is a Vec of bytes. *)
Definition sum_nat (l : list nat) : nat := fold_right Nat.add 0 l.

Definition vec_len (items : list nat) : nat := 8 + sum_nat items.

Definition option_len (o : option nat) : nat :=
  match o with Some n => 1 + n | None => 1 end.

Definition bytes_len (b : list nat) : nat := 8 + length b.

Definition named_len (f : par -> nat) (named : list (bytes * par)) : nat :=
  vec_len (map (fun kv => bytes_len (fst kv) + f (snd kv)) named).

Definition params_len (f : par -> nat) (sp : send_params) : nat :=
  match sp with
  | mkSendParams positional named => vec_len (map f positional) + named_len f named
  end.

Definition query_len (f : par -> nat) (q : query_spec) : nat :=
  match q with
  | mkQuerySpec payload named => vec_len (map f payload) + named_len f named
  end.

Definition send_len (s : schema) (f : par -> nat) (m : send) : nat :=
  match m with
  | mkSend chan data _ params =>
      f chan + vec_len (map f data) + 1
      + new_field_len s (option_len (option_map (params_len f) params))
  end.

Definition bind_len (s : schema) (f : par -> nat) (b : bind) : nat :=
  match b with
  | mkBind patterns source query =>
      vec_len (map f patterns) + f source
      + new_field_len s (option_len (option_map (query_len f) query))
  end.

Definition receive_len (s : schema) (f : par -> nat) (r : receive) : nat :=
  match r with
  | mkReceive binds body => vec_len (map (bind_len s f) binds) + f body
  end.

Definition annotation_len (f : par -> nat) (a : option par) : nat :=
  option_len (option_map f a).

Definition new_len (s : schema) (f : par -> nat) (n : new_block) : nat :=
  match n with
  | mkNew _ body uris lf annots =>
      4 + f body + vec_len (map bytes_len uris) + bytes_len lf
      + new_field_len s (vec_len (map (annotation_len f) annots))
  end.

Definition expr_len (f : par -> nat) (e : expr) : nat :=
  match e with
  | EVar _ => 4 + 9
  | EMethod name target args => 4 + bytes_len name + f target + vec_len (map f args)
  | ETuple ps => 4 + vec_len (map f ps)
  | EFreeBody (mkEFree theory args) => 4 + bytes_len theory + vec_len (map f args)
  | EParamsBody sp => 4 + params_len f sp
  | EOther _ => 4 + 8
  end.

Definition use_len (f : par -> nat) (u : use_block) : nat :=
  match u with
  | mkUseBlock space body => option_len (option_map f space) + option_len (option_map f body)
  end.

Fixpoint bincode_len (s : schema) (p : par) {struct p} : nat :=
  match p with
  | mkPar sends receives news exprs ufs lf _ ubs =>
      vec_len (map (send_len s (bincode_len s)) sends)
      + vec_len (map (receive_len s (bincode_len s)) receives)
      + vec_len (map (new_len s (bincode_len s)) news)
      + vec_len (map (expr_len (bincode_len s)) exprs)
      + vec_len (map bytes_len ufs) + bytes_len lf + 1
      + new_field_len s (vec_len (map (use_len (bincode_len s)) ubs))
  end.

(* The counts of the growth formula. count k p counts the nested nodes of
   kind k of p. Every nested node counts, also a node inside a new field. *)
Inductive node_kind : Type :=
| NodePar
| NodeSend
| NodeBind
| NodeNew.

Definition kind_eqb (a b : node_kind) : bool :=
  match a, b with
  | NodePar, NodePar => true
  | NodeSend, NodeSend => true
  | NodeBind, NodeBind => true
  | NodeNew, NodeNew => true
  | _, _ => false
  end.

Definition tick (k actual : node_kind) : nat := if kind_eqb k actual then 1 else 0.

Definition count_opt (g : par -> nat) (o : option par) : nat :=
  match o with Some x => g x | None => 0 end.

Definition count_params (g : par -> nat) (sp : send_params) : nat :=
  match sp with
  | mkSendParams positional named =>
      sum_nat (map g positional) + sum_nat (map (fun kv => g (snd kv)) named)
  end.

Definition count_query (g : par -> nat) (q : query_spec) : nat :=
  match q with
  | mkQuerySpec payload named =>
      sum_nat (map g payload) + sum_nat (map (fun kv => g (snd kv)) named)
  end.

Definition count_send (k : node_kind) (g : par -> nat) (m : send) : nat :=
  match m with
  | mkSend chan data _ params =>
      tick k NodeSend + g chan + sum_nat (map g data)
      + match params with Some sp => count_params g sp | None => 0 end
  end.

Definition count_bind (k : node_kind) (g : par -> nat) (b : bind) : nat :=
  match b with
  | mkBind patterns source query =>
      tick k NodeBind + sum_nat (map g patterns) + g source
      + match query with Some q => count_query g q | None => 0 end
  end.

Definition count_receive (k : node_kind) (g : par -> nat) (r : receive) : nat :=
  match r with
  | mkReceive binds body => sum_nat (map (count_bind k g) binds) + g body
  end.

Definition count_new (k : node_kind) (g : par -> nat) (n : new_block) : nat :=
  match n with
  | mkNew _ body _ _ annots =>
      tick k NodeNew + g body + sum_nat (map (count_opt g) annots)
  end.

Definition count_expr (g : par -> nat) (e : expr) : nat :=
  match e with
  | EVar _ => 0
  | EMethod _ target args => g target + sum_nat (map g args)
  | ETuple ps => sum_nat (map g ps)
  | EFreeBody (mkEFree _ args) => sum_nat (map g args)
  | EParamsBody sp => count_params g sp
  | EOther _ => 0
  end.

Definition count_use (g : par -> nat) (u : use_block) : nat :=
  match u with mkUseBlock space body => count_opt g space + count_opt g body end.

Fixpoint count (k : node_kind) (p : par) {struct p} : nat :=
  match p with
  | mkPar sends receives news exprs _ _ _ ubs =>
      tick k NodePar
      + sum_nat (map (count_send k (count k)) sends)
      + sum_nat (map (count_receive k (count k)) receives)
      + sum_nat (map (count_new k (count k)) news)
      + sum_nat (map (count_expr (count k)) exprs)
      + sum_nat (map (count_use (count k)) ubs)
  end.

Lemma sum_nat_map_growth :
  forall (A : Type) (gn go gP gS gB gN : A -> nat) (holds : A -> Prop) (free : A -> bool),
    (forall x, holds x -> free x = true ->
       gn x = go x + 8 * gP x + gS x + gB x + 8 * gN x) ->
    forall l, Forall holds l -> forallb free l = true ->
      sum_nat (map gn l) =
      sum_nat (map go l) + 8 * sum_nat (map gP l) + sum_nat (map gS l)
      + sum_nat (map gB l) + 8 * sum_nat (map gN l).
Proof.
  intros A gn go gP gS gB gN holds free Hstep l Hall Hfree.
  induction Hall as [| x rest Hx Hrest IH]; [reflexivity |].
  simpl in Hfree. apply andb_true_iff in Hfree as [Hx' Hr].
  specialize (IH Hr). specialize (Hstep x Hx Hx').
  unfold sum_nat in *. simpl. lia.
Qed.

(* The growth of one layer of the term. A field-free Send gains the 1-byte
   Option tag of params, a field-free ReceiveBind gains the 1-byte Option tag
   of query_spec, a field-free New gains the 8-byte length of space_types,
   and a field-free Par gains the 8-byte length of use_blocks. A nested Par
   grows by its own count. *)
Section BincodeLayers.
  Variables (fn fo cP cS cB cN : par -> nat) (free : par -> bool).

  Definition grows_on_free (p : par) : Prop :=
    free p = true -> fn p = fo p + 8 * cP p + cS p + cB p + 8 * cN p.

  Let grows_list :
    forall l, Forall grows_on_free l -> forallb free l = true ->
      sum_nat (map fn l) =
      sum_nat (map fo l) + 8 * sum_nat (map cP l) + sum_nat (map cS l)
      + sum_nat (map cB l) + 8 * sum_nat (map cN l).
  Proof. apply sum_nat_map_growth. intros x H Hf. exact (H Hf). Qed.

  Lemma send_len_growth :
    forall m, holds_send grows_on_free m -> send_field_free free m = true ->
      send_len NewSchema fn m =
      send_len OldSchema fo m + 8 * count_send NodePar cP m + count_send NodeSend cS m
      + count_send NodeBind cB m + 8 * count_send NodeNew cN m.
  Proof.
    intros [chan data persistent params] [Hc [Hd _]] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hfree Hn].
    apply andb_true_iff in Hfree as [Hc' Hd'].
    destruct params as [sp |]; [discriminate Hn |].
    specialize (Hc Hc'). pose proof (grows_list data Hd Hd') as HS.
    cbn [send_len count_send new_field_len option_map option_len tick kind_eqb].
    unfold vec_len. lia.
  Qed.

  Lemma bind_len_growth :
    forall b, holds_bind grows_on_free b -> bind_field_free free b = true ->
      bind_len NewSchema fn b =
      bind_len OldSchema fo b + 8 * count_bind NodePar cP b + count_bind NodeSend cS b
      + count_bind NodeBind cB b + 8 * count_bind NodeNew cN b.
  Proof.
    intros [patterns source query] [Hp [Hs _]] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hfree Hn].
    apply andb_true_iff in Hfree as [Hp' Hs'].
    destruct query as [q |]; [discriminate Hn |].
    specialize (Hs Hs'). pose proof (grows_list patterns Hp Hp') as HP.
    cbn [bind_len count_bind new_field_len option_map option_len tick kind_eqb].
    unfold vec_len. lia.
  Qed.

  Lemma receive_len_growth :
    forall r, holds_receive grows_on_free r -> receive_field_free free r = true ->
      receive_len NewSchema fn r =
      receive_len OldSchema fo r + 8 * count_receive NodePar cP r
      + count_receive NodeSend cS r + count_receive NodeBind cB r
      + 8 * count_receive NodeNew cN r.
  Proof.
    intros [binds body] [Hb Hd] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hb' Hd'].
    specialize (Hd Hd').
    pose proof (sum_nat_map_growth _ _ _ _ _ _ _ _ _ bind_len_growth binds Hb Hb') as HB.
    cbn [receive_len count_receive].
    unfold vec_len. lia.
  Qed.

  Lemma new_len_growth :
    forall n, holds_new grows_on_free n -> new_field_free free n = true ->
      new_len NewSchema fn n =
      new_len OldSchema fo n + 8 * count_new NodePar cP n + count_new NodeSend cS n
      + count_new NodeBind cB n + 8 * count_new NodeNew cN n.
  Proof.
    intros [k body uris lf annots] [Hb _] Hfree. simpl in Hfree.
    apply andb_true_iff in Hfree as [Hb' Hn].
    destruct annots as [| a rest]; [| discriminate Hn].
    specialize (Hb Hb').
    assert (Hz : sum_nat [] = 0) by reflexivity.
    cbn [new_len count_new new_field_len map tick kind_eqb].
    unfold vec_len. lia.
  Qed.

  Lemma expr_len_growth :
    forall e, holds_expr grows_on_free e -> expr_field_free free e = true ->
      expr_len fn e =
      expr_len fo e + 8 * count_expr cP e + count_expr cS e + count_expr cB e
      + 8 * count_expr cN e.
  Proof.
    intros [v | name target args | ps | fb | sp | n] H Hfree; simpl in Hfree;
      try discriminate Hfree.
    - reflexivity.
    - destruct H as [Ht Ha]. apply andb_true_iff in Hfree as [Ht' Ha'].
      specialize (Ht Ht'). pose proof (grows_list args Ha Ha') as HA.
      cbn [expr_len count_expr]. unfold vec_len. lia.
    - pose proof (grows_list ps H Hfree) as HP.
      cbn [expr_len count_expr]. unfold vec_len. lia.
    - reflexivity.
  Qed.
End BincodeLayers.

(* Headline 14. The growth formula of the bincode form. For a term without a
   new field,

     len_new = len_old + 8 * #Par + #Send + #ReceiveBind + 8 * #New,

   where the counts include every nested node. Every Par gains the empty
   use_blocks Vec of 8 bytes, every Send and every ReceiveBind the absent
   Option tag of 1 byte, every New the empty space_types Vec of 8 bytes. *)
Theorem bincode_growth_field_free :
  forall p, field_free p = true ->
    bincode_len NewSchema p =
    bincode_len OldSchema p + 8 * count NodePar p + count NodeSend p
    + count NodeBind p + 8 * count NodeNew p.
Proof.
  refine (par_nested_ind
            (grows_on_free (bincode_len NewSchema) (bincode_len OldSchema)
               (count NodePar) (count NodeSend) (count NodeBind) (count NodeNew) field_free)
            _).
  intros sends receives news exprs ufs lf cu ubs Hs Hr Hn He _ Hfree.
  cbn [field_free] in Hfree.
  repeat rewrite andb_true_iff in Hfree.
  destruct Hfree as [[[[Hfs Hfr] Hfn] Hfe] Hu].
  destruct ubs as [| u us]; [| discriminate Hu].
  pose proof (sum_nat_map_growth _ _ _ _ _ _ _ _ _ (send_len_growth _ _ _ _ _ _ _) sends Hs Hfs)
    as GS.
  pose proof (sum_nat_map_growth _ _ _ _ _ _ _ _ _ (receive_len_growth _ _ _ _ _ _ _) receives Hr Hfr)
    as GR.
  pose proof (sum_nat_map_growth _ _ _ _ _ _ _ _ _ (new_len_growth _ _ _ _ _ _ _) news Hn Hfn)
    as GN.
  pose proof (sum_nat_map_growth _ _ _ _ _ _ _ _ _ (expr_len_growth _ _ _ _ _ _ _) exprs He Hfe)
    as GE.
  assert (Hz : sum_nat [] = 0) by reflexivity.
  cbn [bincode_len count map tick kind_eqb new_field_len].
  unfold vec_len. lia.
Qed.

(* ── Assumption census ─────────────────────────────────────────────────── *)

Print Assumptions field_numbers_distinct.
Print Assumptions field_numbers_avoid_reserved.
Print Assumptions field_decode_encode.
Print Assumptions field_encode_injective.
Print Assumptions field_decode_sound.
Print Assumptions named_params_canonical.
Print Assumptions construct_has_one_carrier.
Print Assumptions former_marker_tuple_is_ordinary.
Print Assumptions fields_inert_outside_process_position.
Print Assumptions schedule_unchanged_field_free.
Print Assumptions fields_commute_with_subst.
Print Assumptions absent_annotation_not_nil.
Print Assumptions prost_bytes_unchanged_field_free.
Print Assumptions bincode_growth_field_free.
Print Assumptions decode_ignores_flags.
