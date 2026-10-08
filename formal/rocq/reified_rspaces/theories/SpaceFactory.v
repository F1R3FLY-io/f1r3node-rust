(* ═══════════════════════════════════════════════════════════════════════════
   SpaceFactory.v — space families, factory constructors and space creation (P4)

   This module models the factories of the native spaces (A13). A factory
   URN names an outer store and an inner collection, outer first:
   rho:space:<outer>:<inner>. The 32 supported pairs are the space families,
   and family i has the factory channel [0xE0, i]. Two FIPS aliases name
   hashmap:bag and pathmap:bag. A factory call carries one constructor list
   [q?][FREE?][PARAMS?]. The space id is a draw of the random state of the
   call (I-19). A persistent send to an op that takes calls aborts before the
   call (decision 32, A21). A creation whose config channel C(sid) already
   holds a record aborts with SpaceAlreadyExists in play and in replay
   alike, so no call repeats (the restatement of readiness review §4).

   Sources
   - Design: the P4 design report A.5 (families and channels), C.7 (the
     factory call and the constructor), B.8 (the space id) and E.1 (the
     headlines of this module). Readiness review §4 restates the headline
     space_already_exists_stops_reissue as persistent_op_call_aborts and
     space_already_exists_is_deterministic_abort.
   - Plan: decisions 7, 18, 21 and 32, A13, A21 and I-19.
   - Code: this repository. The line numbers below are the lines of the tree
     of the P5 FACTORY batch (wave W2).

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   outer, inner, qualifier  models/src/rust/space_factory_urn.rs:45-51, 95-103,
                            153-161
   outer_name, inner_name, outer_of_name, inner_of_name
                            models/src/rust/space_factory_urn.rs:64-72, 75-79,
                            118-128, 131-135
   family_base, ordinal     models/src/rust/space_factory_urn.rs:82-90, 138-148
   is_supported_pair, supports_qualifier
                            models/src/rust/space_factory_urn.rs:267-276,
                            280-289
   family_index, from_family_index, all_families
                            models/src/rust/space_factory_urn.rs:293-298,
                            301-303, 306-313
   urn_prefix, aliases, parse_urn
                            models/src/rust/space_factory_urn.rs:32, 35, 38,
                            225-263
   family_of_names          rholang/src/rust/interpreter/spaces/factory/
                            constructor.rs:163-181 (family_of)
   factory_op               rholang/src/rust/interpreter/spaces/factory/urn.rs:
                            190-194, 198-200 (the factory channel and the
                            capability that getSpaceAgent replies)
   validate_qualifier, durability_of
                            rholang/src/rust/interpreter/spaces/factory/
                            constructor.rs:60-112, 115-120
   parse_elements, to_elements, qual_of
                            models/src/rust/space_factory_urn.rs:368-398,
                            405-417, 428-438 (parse_elements, to_elements,
                            qualifier_of)
   derive_space_id          rholang/src/rust/interpreter/spaces/types/id.rs:
                            13-22, rholang/src/rust/interpreter/spaces/
                            namespace.rs:397-422, 426-428 (RandomPurpose and
                            derive)
   check_persistent_send    rholang/src/rust/interpreter/reified/
                            system_channels.rs:101, 105, 110-122
   prepare_create           rholang/src/rust/interpreter/spaces/generic.rs:
                            2591-2709

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A string is the list of its ASCII bytes.
   - The constructor decoders are section variables: the PARAMS decoder
     (split_trailing_params), the FREE decoder (decode_free_value) and
     the name check of qualifier_of (qual_name_of). The section
     hypotheses state what the code and Encoding.v establish: each strict
     decoder accepts only the canonical encoding of what it returns, and
     the decoders of different markers do not accept each other's
     elements. The corollaries constructor_parse_canonical_instance and
     constructor_parse_sound_instance discharge them with a concrete
     element type.
   - The FREE and PARAMS decoders compare under Par equality, which
     ignores locally_free (Encoding.v), so their elements carry no bit set
     in the model. qualifier_of
     (models/src/rust/space_factory_urn.rs:428-438) also checks the
     locally_free bits of the qualifier element: a string has no free
     variables, so its canonical element has none. The model states the
     bit check as the definition qual_of over the section variable clean
     (the bit set is empty), and the rest of qualifier_of (sole_expr,
     connective_used and SpaceQualifier::parse) as the section variable
     qual_name_of. The name check fixes every part of an element but its
     bit set (qual_strict), so the bit check makes the parser sound:
     constructor_parse_sound has no premise on the bits, and a qualifier
     element with bits is NonCanonical (qualifier_bits_rejected).
   - The random state of a call is a section variable type with split_byte
     and next. The section hypothesis next_length (a Blake2b512 draw has 64
     bytes) is discharged by an instance.
   - prepare_create: the config records on C(sid) are a list, and the
     config check and the family check are booleans.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. family_index_bijective             the 32 families have the indices
                                          0 to 31, one each
    2. aliases_map_to_family              URNs and aliases parse to their
                                          families
    3. qualifier_matrix                   HashSet takes only seq, and P4
                                          creates only default spaces
    4. constructor_parse_canonical        parse ∘ to_elements = id
    5. constructor_parse_sound            to_elements ∘ parse = id
    6. sid_derivation_deterministic       the sid is a 32-byte draw of the
                                          call alone
    7. persistent_op_call_aborts          `!!` on a call op or a handle aborts
    8. space_already_exists_is_deterministic_abort
                                          a second creation of a sid aborts,
                                          in play and replay alike

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - A rewrite under a match that is not reduced yet (after `rewrite
     qual_not_params`) finds no subterm. Reduce with `cbn` between the
     rewrites.
   - `intros [] q` names the arguments of the constructors itself, and the
     name q clashes. Name every argument in the pattern.
   - `apply (proj1 (thm _))` cannot infer a section hypothesis from `_`.
     Prove the hypothesis first and pass it.
   - `From Stdlib Require String` alone does not declare the delimiting key
     `%string`. Import the notations in a module (Module Str), as
     RecordEnvelope.v does.
   - A bit check only in the instance (the premise clean of the soundness
     theorem, or a check inside the toy decoder) leaves
     constructor_parse_sound provable without it. A mutant that drops the
     check then fails only in constructor_parse_sound_instance. The check
     is therefore the section definition qual_of.
   - qual_of_elem by `unfold qual_of; rewrite qual_elem_clean` compiles,
     but the mutant without the bit check then fails in qual_of_elem
     ("Found no subterm matching clean (qual_elem _)") and never reaches
     constructor_parse_sound. Destruct the check instead: completeness
     does not depend on it.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Strings.String Strings.Ascii.
Import ListNotations.

Definition bytes : Type := list nat.

Definition bytes_eqb (a b : bytes) : bool := if list_eq_dec Nat.eq_dec a b then true else false.

Lemma bytes_eqb_refl : forall a, bytes_eqb a a = true.
Proof. intros a. unfold bytes_eqb. destruct (list_eq_dec Nat.eq_dec a a); [reflexivity | contradiction]. Qed.

(* The ASCII bytes of the string literals. *)
Module Str.
  Import (notations) Stdlib.Strings.String.
  Local Open Scope string_scope.
  Definition b (s : String.string) : list nat :=
    map Ascii.nat_of_ascii (String.list_ascii_of_string s).
  Definition prefix := b "rho:space:".
  Definition hashmap_bag_alias := b "rho:space:HashMapBagSpace".
  Definition pathmap_bag_alias := b "rho:space:PathMapSpace".
  Definition hashmap := b "hashmap".
  Definition pathmap := b "pathmap".
  Definition array := b "array".
  Definition vector := b "vector".
  Definition hashset := b "hashset".
  Definition bag := b "bag".
  Definition queue := b "queue".
  Definition stack := b "stack".
  Definition set := b "set".
  Definition cell := b "cell".
  Definition priorityqueue := b "priorityqueue".
  Definition vectordb := b "vectordb".
End Str.

(* ── 1. Families ───────────────────────────────────────────────────────── *)

Inductive outer : Type := HashMap | PathMap | Array | Vector | HashSet.
Inductive inner : Type := Bag | Queue | Stack | SetC | Cell | PriorityQueue | VectorDb.
Inductive qualifier : Type := QDefault | QTemp | QSeq.

Record family : Type := mkFamily { f_outer : outer; f_inner : inner }.

Definition all_outers : list outer := [HashMap; PathMap; Array; Vector; HashSet].
Definition all_inners : list inner := [Bag; Queue; Stack; SetC; Cell; PriorityQueue; VectorDb].

Definition outer_name (o : outer) : bytes :=
  match o with
  | HashMap => Str.hashmap | PathMap => Str.pathmap | Array => Str.array
  | Vector => Str.vector | HashSet => Str.hashset
  end.

Definition inner_name (i : inner) : bytes :=
  match i with
  | Bag => Str.bag | Queue => Str.queue | Stack => Str.stack | SetC => Str.set | Cell => Str.cell
  | PriorityQueue => Str.priorityqueue | VectorDb => Str.vectordb
  end.

Definition outer_of_name (s : bytes) : option outer := find (fun o => bytes_eqb (outer_name o) s) all_outers.
Definition inner_of_name (s : bytes) : option inner := find (fun i => bytes_eqb (inner_name i) s) all_inners.

Definition family_base (o : outer) : nat :=
  match o with HashMap => 0 | PathMap => 7 | Array => 13 | Vector => 19 | HashSet => 26 end.

Definition ordinal (i : inner) : nat :=
  match i with
  | Bag => 0 | Queue => 1 | Stack => 2 | SetC => 3 | Cell => 4 | PriorityQueue => 5 | VectorDb => 6
  end.

Definition is_supported_pair (f : family) : bool :=
  match f_outer f, f_inner f with
  | HashMap, _ | Vector, _ => true
  | _, VectorDb => false
  | _, _ => true
  end.

Definition supports_qualifier (f : family) (q : qualifier) : bool :=
  match f_outer f, q with
  | HashSet, QSeq => true
  | HashSet, _ => false
  | _, _ => true
  end.

Definition family_index (f : family) : option nat :=
  if is_supported_pair f then Some (family_base (f_outer f) + ordinal (f_inner f)) else None.

Definition all_families : list family :=
  flat_map (fun o => filter is_supported_pair (map (fun i => mkFamily o i) all_inners)) all_outers.

Definition from_family_index (n : nat) : option family :=
  find (fun f => match family_index f with Some m => m =? n | None => false end) all_families.

(* The op of the factory channel [0xE0, i] and of the capability
   bundle+{[0xE0, i]} that getSpaceAgent replies. *)
Definition factory_op (f : family) : option nat := family_index f.

Lemma all_families_supported : forall f, In f all_families -> is_supported_pair f = true.
Proof.
  intros f H. unfold all_families in H. apply in_flat_map in H as [o [_ H]].
  apply filter_In in H as [_ H]. exact H.
Qed.

Lemma supported_in_all : forall f, is_supported_pair f = true -> In f all_families.
Proof.
  intros [[] []] H; cbn in H; try discriminate H; cbn; repeat (first [left; reflexivity | right]).
Qed.

Lemma from_family_index_of : forall f n, family_index f = Some n -> from_family_index n = Some f.
Proof. intros [[] []] n H; cbn in H; try discriminate H; injection H as <-; reflexivity. Qed.

(* Headline 1. The supported families, in index order, have the indices 0
   to 31. from_family_index inverts family_index, every index below 32
   names one family, and no other index names one. So two families never
   share a factory channel or a capability. *)
Theorem family_index_bijective :
  map family_index all_families = map Some (seq 0 32) /\
  (forall f, is_supported_pair f = true <-> In f all_families) /\
  (forall f n, family_index f = Some n -> from_family_index n = Some f) /\
  (forall n, n < 32 -> exists f, from_family_index n = Some f /\ family_index f = Some n) /\
  (forall n, 32 <= n -> from_family_index n = None) /\
  (forall f1 f2 n, factory_op f1 = Some n -> factory_op f2 = Some n -> f1 = f2).
Proof.
  split; [reflexivity |].
  split; [intros f; split; [apply supported_in_all | apply all_families_supported] |].
  split; [exact from_family_index_of |].
  split.
  - intros n Hn.
    do 32 (destruct n as [| n]; [eexists; split; reflexivity |]). lia.
  - split.
    + intros n Hn. replace n with (32 + (n - 32)) by lia. generalize (n - 32). intros k. reflexivity.
    + intros f1 f2 n H1 H2. unfold factory_op in *.
      apply from_family_index_of in H1. apply from_family_index_of in H2. congruence.
Qed.

(* ── 2. URNs and aliases ───────────────────────────────────────────────── *)

Inductive urn_error : Type := InvalidForm | UnknownOuter | UnknownInner | UnsupportedPair.
Inductive urn_result : Type := UrnOk : family -> urn_result | UrnErr : urn_error -> urn_result.

Fixpoint strip_prefix (p s : bytes) : option bytes :=
  match p, s with
  | [], _ => Some s
  | x :: p', y :: s' => if x =? y then strip_prefix p' s' else None
  | _ :: _, [] => None
  end.

(* str::split(':'), empty parts included. *)
Fixpoint split_colon (s : bytes) : list bytes :=
  match s with
  | [] => [[]]
  | c :: rest =>
      if c =? 58 then [] :: split_colon rest
      else match split_colon rest with
           | p :: ps => (c :: p) :: ps
           | [] => [[c]]
           end
  end.

Definition is_nil (s : bytes) : bool := match s with [] => true | _ => false end.

Definition parse_urn (s : bytes) : urn_result :=
  if bytes_eqb s Str.hashmap_bag_alias then UrnOk (mkFamily HashMap Bag)
  else if bytes_eqb s Str.pathmap_bag_alias then UrnOk (mkFamily PathMap Bag)
  else match strip_prefix Str.prefix s with
       | None => UrnErr InvalidForm
       | Some rest =>
           match split_colon rest with
           | [o; i] =>
               if is_nil o || is_nil i then UrnErr InvalidForm else
               match outer_of_name o, inner_of_name i with
               | None, _ => UrnErr UnknownOuter
               | Some _, None => UrnErr UnknownInner
               | Some ou, Some inn =>
                   if is_supported_pair (mkFamily ou inn) then UrnOk (mkFamily ou inn)
                   else UrnErr UnsupportedPair
               end
           | _ => UrnErr InvalidForm
           end
       end.

(* The Display of SpaceFactoryUrn. *)
Definition urn_of (f : family) : bytes := Str.prefix ++ outer_name (f_outer f) ++ [58] ++ inner_name (f_inner f).

(* constructor.rs family_of: the family that the outer and inner names of a
   stored config name. getSpaceAgent replies the capability of it. *)
Definition family_of_names (o i : bytes) : option family :=
  match outer_of_name o, inner_of_name i with
  | Some ou, Some inn => if is_supported_pair (mkFamily ou inn) then Some (mkFamily ou inn) else None
  | _, _ => None
  end.

(* Headline 2. The FIPS aliases name hashmap:bag (index 0) and pathmap:bag
   (index 7). The URN of every family parses back to the family, an
   unsupported pair is rejected, and the names of a family in a config give
   the family again, so getSpaceAgent replies the capability of the family
   of the space. *)
Theorem aliases_map_to_family :
  parse_urn Str.hashmap_bag_alias = UrnOk (mkFamily HashMap Bag) /\
  parse_urn Str.pathmap_bag_alias = UrnOk (mkFamily PathMap Bag) /\
  family_index (mkFamily HashMap Bag) = Some 0 /\
  family_index (mkFamily PathMap Bag) = Some 7 /\
  (forall f, In f all_families -> parse_urn (urn_of f) = UrnOk f) /\
  (forall f, is_supported_pair f = false -> parse_urn (urn_of f) = UrnErr UnsupportedPair) /\
  (forall f, In f all_families -> family_of_names (outer_name (f_outer f)) (inner_name (f_inner f)) = Some f).
Proof.
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [| split].
  - intros f Hf. unfold all_families in Hf. cbn in Hf.
    repeat (destruct Hf as [<- | Hf]; [vm_compute; reflexivity |]). destruct Hf.
  - intros [[] []] H; cbn in H; try discriminate H; vm_compute; reflexivity.
  - intros f Hf. unfold all_families in Hf. cbn in Hf.
    repeat (destruct Hf as [<- | Hf]; [vm_compute; reflexivity |]). destruct Hf.
Qed.

(* ── 3. Qualifiers ─────────────────────────────────────────────────────── *)

Inductive qual_check : Type := QualOk | QualBadArgument | QualUnavailable.

(* FactoryConstructor::validate, step 3: the family takes the qualifier,
   and the qualifier is default (temp and seq arrive in P7). *)
Definition validate_qualifier (f : family) (q : qualifier) : qual_check :=
  if negb (supports_qualifier f q) then QualBadArgument
  else match q with QDefault => QualOk | QTemp | QSeq => QualUnavailable end.

Inductive durability : Type := Durable | Ephemeral.

Definition durability_of (q : qualifier) : durability :=
  match q with QDefault => Durable | QTemp | QSeq => Ephemeral end.

(* Headline 3. HashSet takes only seq, every other store takes every
   qualifier. In P4 a factory accepts only default, so a HashSet family can
   create no space, and every new space is durable. *)
Theorem qualifier_matrix :
  (forall f q, supports_qualifier f q = true <-> (f_outer f <> HashSet \/ q = QSeq)) /\
  (forall f q, validate_qualifier f q = QualOk <-> (q = QDefault /\ f_outer f <> HashSet)) /\
  (forall f, f_outer f = HashSet -> forall q, validate_qualifier f q <> QualOk) /\
  (forall q, durability_of q = Durable <-> q = QDefault).
Proof.
  split; [| split; [| split]].
  - intros [[] i] []; cbn; split; intros H;
      solve [ reflexivity | left; discriminate | right; reflexivity | discriminate H
            | destruct H as [H | H]; [contradiction H; reflexivity | discriminate H] ].
  - intros [[] i] []; cbn; split; intros H;
      solve [ reflexivity | split; [reflexivity | discriminate] | discriminate H
            | destruct H as [H1 H2]; first [discriminate H1 | contradiction H2; reflexivity] ].
  - intros [o i] Ho q. cbn in Ho. subst o. destruct q; cbn; discriminate.
  - intros []; cbn; split; intros H; solve [reflexivity | discriminate H].
Qed.

(* ── 4. The constructor [q?][FREE?][PARAMS?] ───────────────────────────── *)

Inductive qual_result : Type := QOk : qualifier -> qual_result | QUnknown | QNonCanonical.
Inductive ctor_error : Type := MalformedParams | MalformedTheory | UnknownQualifier | NonCanonical.

Definition last_elem {A : Type} (l : list A) : option A :=
  match rev l with [] => None | x :: _ => Some x end.

Lemma last_elem_snoc : forall (A : Type) (l : list A) x, last_elem (l ++ [x]) = Some x.
Proof. intros A l x. unfold last_elem. rewrite rev_app_distr. reflexivity. Qed.

Lemma snoc_cases : forall (A : Type) (l : list A), l = [] \/ exists init x, l = init ++ [x].
Proof.
  intros A [| a l']; [left; reflexivity | right].
  destruct (exists_last (l := a :: l')) as [init [x E]]; [discriminate | exists init, x; exact E].
Qed.

Section Constructor.
  Variable elem params theory : Type.
  Variable params_empty : params -> bool.
  (* split_trailing_params on the last element: Some None for an element
     that is no PARAMS tuple, Some (Some ps) for a canonical PARAMS tuple,
     None for a malformed one. *)
  Variable params_of : elem -> option (option params).
  (* decode_free_value, with the same convention. *)
  Variable free_of : elem -> option (option theory).
  (* qualifier_of without its bit check: sole_expr, the connective_used
     flag and SpaceQualifier::parse. *)
  Variable qual_name_of : elem -> qual_result.
  Variable params_elem : params -> elem.
  Variable free_elem : theory -> elem.
  (* string_par(q.as_str()). *)
  Variable qual_elem : qualifier -> elem.
  (* The bit check of qualifier_of: the locally_free bit set is empty. *)
  Variable clean : elem -> bool.

  Hypothesis params_roundtrip : forall ps, params_empty ps = false -> params_of (params_elem ps) = Some (Some ps).
  Hypothesis params_strict : forall e ps, params_of e = Some (Some ps) -> e = params_elem ps /\ params_empty ps = false.
  Hypothesis free_roundtrip : forall th, free_of (free_elem th) = Some (Some th).
  Hypothesis free_strict : forall e th, free_of e = Some (Some th) -> e = free_elem th.
  Hypothesis free_not_params : forall th, params_of (free_elem th) = Some None.
  Hypothesis qual_not_params : forall q, params_of (qual_elem q) = Some None.
  Hypothesis qual_not_free : forall q, free_of (qual_elem q) = Some None.
  Hypothesis qual_roundtrip : forall q, qual_name_of (qual_elem q) = QOk q.
  (* string_par makes an element with an empty bit set. *)
  Hypothesis qual_elem_clean : forall q, clean (qual_elem q) = true.
  (* The name check fixes every part of an element but its bit set. *)
  Hypothesis qual_strict : forall e q, qual_name_of e = QOk q -> clean e = true -> e = qual_elem q.

  (* qualifier_of: an element with a nonempty bit set is NonCanonical,
     whatever its name. *)
  Definition qual_of (e : elem) : qual_result := if clean e then qual_name_of e else QNonCanonical.

  (* Every canonical qualifier element passes the bit check. The proof
     holds with and without the check, so only soundness depends on it. *)
  Lemma qual_of_elem : forall q, qual_of (qual_elem q) = QOk q.
  Proof.
    intros q. unfold qual_of. destruct (clean (qual_elem q)) eqn:Hc.
    - apply qual_roundtrip.
    - rewrite qual_elem_clean in Hc. discriminate Hc.
  Qed.

  Record ctor : Type := mkCtor { c_qualifier : option qualifier; c_theory : option theory; c_params : option params }.
  Inductive ctor_result : Type := CtorOk : ctor -> ctor_result | CtorErr : ctor_error -> ctor_result.

  Definition parse_elements (els : list elem) : ctor_result :=
    let pp := match last_elem els with None => Some None | Some last => params_of last end in
    match pp with
    | None => CtorErr MalformedParams
    | Some p =>
        let rest := match p with Some _ => removelast els | None => els end in
        let th := match last_elem rest with
                  | None => Some (None, rest)
                  | Some last =>
                      match free_of last with
                      | None => None
                      | Some (Some t) => Some (Some t, removelast rest)
                      | Some None => Some (None, rest)
                      end
                  end in
        match th with
        | None => CtorErr MalformedTheory
        | Some (t, rest2) =>
            match rest2 with
            | [] => CtorOk (mkCtor None t p)
            | [v] =>
                match qual_of v with
                | QOk q => CtorOk (mkCtor (Some q) t p)
                | QUnknown => CtorErr UnknownQualifier
                | QNonCanonical => CtorErr NonCanonical
                end
            | _ => CtorErr NonCanonical
            end
        end
    end.

  (* Empty params have no element. *)
  Definition to_elements (c : ctor) : list elem :=
    match c_qualifier c with Some q => [qual_elem q] | None => [] end ++
    match c_theory c with Some t => [free_elem t] | None => [] end ++
    match c_params c with Some p => if params_empty p then [] else [params_elem p] | None => [] end.

  Definition canonical_ctor (c : ctor) : Prop :=
    match c_params c with Some p => params_empty p = false | None => True end.

  (* Headline 4. Every canonical constructor (no empty params) parses back
     from its elements. *)
  Theorem constructor_parse_canonical : forall c, canonical_ctor c -> parse_elements (to_elements c) = CtorOk c.
  Proof.
    intros [q t p] Hc. unfold canonical_ctor in Hc. cbn [c_params] in Hc.
    unfold to_elements, parse_elements. cbn [c_qualifier c_theory c_params].
    destruct p as [ps |].
    - rewrite Hc. rewrite app_assoc, last_elem_snoc, (params_roundtrip ps Hc), removelast_last.
      destruct t as [th |].
      + rewrite last_elem_snoc, free_roundtrip, removelast_last.
        destruct q as [q |]; cbn; [rewrite qual_of_elem |]; reflexivity.
      + rewrite app_nil_r. destruct q as [q |]; cbn.
        * rewrite qual_not_free. cbn. rewrite qual_of_elem. reflexivity.
        * reflexivity.
    - rewrite app_nil_r. destruct t as [th |].
      + rewrite last_elem_snoc, free_not_params, last_elem_snoc, free_roundtrip, removelast_last.
        destruct q as [q |]; cbn; [rewrite qual_of_elem |]; reflexivity.
      + rewrite app_nil_r. destruct q as [q |]; cbn.
        * rewrite qual_not_params. cbn. rewrite qual_not_free. cbn. rewrite qual_of_elem. reflexivity.
        * reflexivity.
  Qed.

  (* Headline 5. parse_elements accepts only the canonical sequence: the
     elements of a parsed constructor are exactly its canonical elements.
     The bit check of qual_of rejects a qualifier element with a nonempty
     bit set, so the theorem needs no premise on the bits. *)
  Theorem constructor_parse_sound :
    forall els c, parse_elements els = CtorOk c -> to_elements c = els.
  Proof.
    intros els c Hp. unfold parse_elements in Hp.
    (* the qualifier part: a rest of at most one element *)
    assert (Hq : forall rest2 t p,
              match rest2 with
              | [] => CtorOk (mkCtor None t p)
              | [v] => match qual_of v with
                       | QOk q => CtorOk (mkCtor (Some q) t p)
                       | QUnknown => CtorErr UnknownQualifier
                       | QNonCanonical => CtorErr NonCanonical
                       end
              | _ => CtorErr NonCanonical
              end = CtorOk c ->
              c_theory c = t /\ c_params c = p /\
              match c_qualifier c with Some q => [qual_elem q] | None => [] end = rest2).
    { intros rest2 t p H. destruct rest2 as [| v [| w rest3]].
      - injection H as <-. split; [reflexivity | split; reflexivity].
      - destruct (qual_of v) as [q | |] eqn:Eq; try discriminate H. injection H as <-.
        unfold qual_of in Eq. destruct (clean v) eqn:Hcl; [| discriminate Eq].
        rewrite (qual_strict v q Eq Hcl). split; [reflexivity | split; reflexivity].
      - discriminate H. }
    destruct (snoc_cases _ els) as [-> | [init [last ->]]].
    - cbn in Hp. injection Hp as <-. reflexivity.
    - rewrite last_elem_snoc in Hp.
      destruct (params_of last) as [[ps |] |] eqn:Ep; [| | discriminate Hp].
      + apply params_strict in Ep as [-> Hne]. rewrite removelast_last in Hp.
        destruct (snoc_cases _ init) as [-> | [init2 [l2 ->]]].
        * cbn in Hp. injection Hp as <-. cbn. rewrite Hne. reflexivity.
        * rewrite last_elem_snoc in Hp.
          destruct (free_of l2) as [[th |] |] eqn:Ef; [| | discriminate Hp].
          -- apply free_strict in Ef. subst l2. rewrite removelast_last in Hp.
             destruct (Hq init2 (Some th) (Some ps) Hp) as [Ht [Hpp Hqq]].
             unfold to_elements. rewrite Ht, Hpp, Hne, Hqq, <- app_assoc. reflexivity.
          -- destruct (Hq (init2 ++ [l2]) None (Some ps) Hp) as [Ht [Hpp Hqq]].
             unfold to_elements. rewrite Ht, Hpp, Hne, Hqq. reflexivity.
      + destruct (snoc_cases _ (init ++ [last])) as [Hnil | _];
          [apply app_eq_nil in Hnil as [_ Hnil]; discriminate Hnil |].
        rewrite last_elem_snoc in Hp.
        destruct (free_of last) as [[th |] |] eqn:Ef; [| | discriminate Hp].
        * apply free_strict in Ef. subst last. rewrite removelast_last in Hp.
          destruct (Hq init (Some th) None Hp) as [Ht [Hpp Hqq]].
          unfold to_elements. rewrite Ht, Hpp, Hqq, app_nil_r. reflexivity.
        * destruct (Hq (init ++ [last]) None None Hp) as [Ht [Hpp Hqq]].
          unfold to_elements. rewrite Ht, Hpp, Hqq, !app_nil_r. reflexivity.
  Qed.
End Constructor.

(* An instance of the decoders: the hypotheses hold, so the two theorems
   are not vacuous. TQual carries the bit set of its string. TFree and
   TParams carry none: the FREE and PARAMS decoders compare under Par
   equality. *)
Inductive toy_elem : Type :=
| TQual : qualifier -> list nat -> toy_elem
| TBadString : toy_elem
| TFree : nat -> toy_elem
| TParams : list nat -> toy_elem
| TBadTuple : toy_elem.

Definition toy_params_empty (ps : list nat) : bool := match ps with [] => true | _ => false end.
Definition toy_params_of (e : toy_elem) : option (option (list nat)) :=
  match e with
  | TParams [] | TBadTuple => None
  | TParams ps => Some (Some ps)
  | _ => Some None
  end.
Definition toy_free_of (e : toy_elem) : option (option nat) :=
  match e with TFree t => Some (Some t) | _ => Some None end.
(* The name check reads the string and ignores the bit set. *)
Definition toy_qual_name_of (e : toy_elem) : qual_result :=
  match e with TQual q _ => QOk q | TBadString => QUnknown | _ => QNonCanonical end.
Definition toy_clean (e : toy_elem) : bool := match e with TQual _ [] => true | TQual _ _ => false | _ => true end.

Definition toy_parse := parse_elements toy_elem (list nat) nat toy_params_of toy_free_of toy_qual_name_of toy_clean.
Definition toy_to_elements := to_elements toy_elem (list nat) nat toy_params_empty TParams TFree (fun q => TQual q []).

Corollary constructor_parse_canonical_instance :
  forall c, canonical_ctor (list nat) nat toy_params_empty c -> toy_parse (toy_to_elements c) = CtorOk _ _ c.
Proof.
  apply constructor_parse_canonical.
  - intros [| x ps] H; [discriminate H | reflexivity].
  - intros th. reflexivity.
  - intros th. reflexivity.
  - intros q. reflexivity.
  - intros q. reflexivity.
  - intros q. reflexivity.
  - intros q. reflexivity.
Qed.

Corollary constructor_parse_sound_instance :
  forall els c, toy_parse els = CtorOk _ _ c -> toy_to_elements c = els.
Proof.
  apply constructor_parse_sound.
  - intros [q0 l0 | | t0 | l0 | ] ps H; try discriminate H.
    destruct l0 as [| x l0]; [discriminate H | injection H as <-; split; reflexivity].
  - intros [q0 l0 | | t0 | l0 | ] th H; try discriminate H. injection H as <-. reflexivity.
  - intros [q0 l0 | | t0 | l0 | ] q H Hc; try discriminate H. injection H as <-.
    destruct l0; [reflexivity | discriminate Hc].
Qed.

(* qualifier_of rejects a qualifier element with a nonempty bit set: the
   element list fails with NonCanonical, whatever theory and params follow
   the qualifier. Its clean twin parses (constructor_parse_canonical_instance). *)
Lemma qualifier_bits_rejected :
  forall q b bits t p,
    toy_parse (TQual q (b :: bits) :: toy_to_elements (mkCtor _ _ None t p)) = CtorErr _ _ NonCanonical.
Proof. intros q b bits [th |] [[| x ps] |]; reflexivity. Qed.

(* ── 5. The space id (types/id.rs, I-19) ────────────────────────────────── *)

Section Sid.
  Variable rnd : Type.
  Variable split_byte : rnd -> nat -> rnd.
  (* Blake2b512Random::next: one 64-byte draw. *)
  Variable next : rnd -> bytes.
  Hypothesis next_length : forall r, length (next r) = 64.

  (* RandomPurpose::derive: random.split_byte(0xE0).split_byte(purpose). *)
  Definition purpose_state (r : rnd) (p : nat) : rnd := split_byte (split_byte r 224) p.

  Definition derive_space_id (r : rnd) : bytes := firstn 32 (next (purpose_state r 1)).

  (* Headline 6. The space id is the first 32 bytes of the SpaceId draw of
     the call: a function of the random state of the call alone, so play
     and replay derive the same id. Two calls get two ids when the draw of
     their SpaceId states does not collide (Blake2b-256 is collision free
     in the model), and the other purposes (config 0x05, handle 0x06,
     counters 0x07 and 0x08) draw from other states when split_byte is
     injective. *)
  Theorem sid_derivation_deterministic :
    (forall r, length (derive_space_id r) = 32) /\
    (forall r1 r2, purpose_state r1 1 = purpose_state r2 1 -> derive_space_id r1 = derive_space_id r2) /\
    (forall r1 r2, (firstn 32 (next (purpose_state r1 1)) = firstn 32 (next (purpose_state r2 1)) ->
                    purpose_state r1 1 = purpose_state r2 1) ->
                   derive_space_id r1 = derive_space_id r2 -> purpose_state r1 1 = purpose_state r2 1) /\
    ((forall r1 r2 b1 b2, split_byte r1 b1 = split_byte r2 b2 -> r1 = r2 /\ b1 = b2) ->
     forall r p, p <> 1 -> purpose_state r p <> purpose_state r 1).
  Proof.
    split; [| split; [| split]].
    - intros r. unfold derive_space_id. rewrite length_firstn, next_length. reflexivity.
    - intros r1 r2 H. unfold derive_space_id. rewrite H. reflexivity.
    - intros r1 r2 Hcol H. apply Hcol. exact H.
    - intros Hinj r p Hp H. unfold purpose_state in H. apply Hinj in H as [_ H]. contradiction.
  Qed.
End Sid.

(* The hypothesis holds for a draw that pads the state to 64 bytes. *)
Corollary sid_derivation_deterministic_instance :
  forall r : list nat,
    length (derive_space_id (list nat) (fun r b => r ++ [b]) (fun r => firstn 64 (r ++ repeat 0 64)) r) = 32.
Proof.
  intros r.
  assert (Hn : forall r' : list nat, length (firstn 64 (r' ++ repeat 0 64)) = 64).
  { intros r'. rewrite length_firstn, length_app, repeat_length. lia. }
  exact (proj1 (sid_derivation_deterministic (list nat) (fun r b => r ++ [b])
                  (fun r => firstn 64 (r ++ repeat 0 64)) Hn) r).
Qed.

(* ── 6. Persistent sends to system ops (system_channels.rs, A21) ──────── *)

(* The decoded id of a channel after the unbundling of eval_send. *)
Inductive chan_id : Type := IdOp : nat -> chan_id | IdHandle | IdOther.

Inductive send_check : Type := SendOk | PersistentFactoryCall | PersistentOpCall | PersistentHandleCall.

Definition native_engine_factory : nat := 32.
Definition vector_op : nat := 35.
Definition handle_op : nat := 48.
Definition sink_op : nat := 50.

Definition is_factory_op (op : nat) : bool := op <=? native_engine_factory.
Definition is_call_op (op : nat) : bool := (op <=? vector_op) || ((handle_op <=? op) && (op <=? sink_op)).

Definition check_persistent_send (c : chan_id) (persistent : bool) : send_check :=
  if negb persistent then SendOk else
  match c with
  | IdOp op => if is_factory_op op then PersistentFactoryCall
               else if is_call_op op then PersistentOpCall else SendOk
  | IdHandle => PersistentHandleCall
  | IdOther => SendOk
  end.

(* Headline 7. A persistent send aborts on every op that takes calls
   ([E0, 00..23] and [E0, 30..32]: the factories, the engine factory,
   getSpaceAgent, assertEqual, vector, HANDLE_OP, the matcher and the sink)
   and on every handle, before the call runs, so no call repeats. Markers
   (0xD0..0xD4) and the agent tag (0xFF) take no calls, and a send that is
   not persistent never aborts here. *)
Theorem persistent_op_call_aborts :
  (forall c, check_persistent_send c false = SendOk) /\
  (forall op, op <= 32 -> check_persistent_send (IdOp op) true = PersistentFactoryCall) /\
  (forall op, (33 <= op <= 35 \/ 48 <= op <= 50) -> check_persistent_send (IdOp op) true = PersistentOpCall) /\
  check_persistent_send IdHandle true = PersistentHandleCall /\
  (forall op, (36 <= op <= 47 \/ 51 <= op) -> check_persistent_send (IdOp op) true = SendOk) /\
  (forall op, is_call_op op = true -> check_persistent_send (IdOp op) true <> SendOk) /\
  (forall m, In m [208; 209; 210; 211; 212; 255] -> check_persistent_send (IdOp m) true = SendOk).
Proof.
  split; [intros c; reflexivity |].
  split.
  { intros op H. cbn. unfold is_factory_op, native_engine_factory.
    replace (op <=? 32) with true by (symmetry; apply Nat.leb_le; exact H). reflexivity. }
  split.
  { intros op H. cbn. unfold is_factory_op, is_call_op, native_engine_factory, vector_op, handle_op, sink_op.
    replace (op <=? 32) with false by (symmetry; apply Nat.leb_gt; lia).
    destruct H as [H | H].
    - replace (op <=? 35) with true by (symmetry; apply Nat.leb_le; lia). reflexivity.
    - replace (op <=? 35) with false by (symmetry; apply Nat.leb_gt; lia).
      replace (48 <=? op) with true by (symmetry; apply Nat.leb_le; lia).
      replace (op <=? 50) with true by (symmetry; apply Nat.leb_le; lia). reflexivity. }
  split; [reflexivity |].
  split.
  { intros op H. cbn. unfold is_factory_op, is_call_op, native_engine_factory, vector_op, handle_op, sink_op.
    replace (op <=? 32) with false by (symmetry; apply Nat.leb_gt; lia).
    replace (op <=? 35) with false by (symmetry; apply Nat.leb_gt; lia).
    destruct H as [H | H].
    - replace (48 <=? op) with false by (symmetry; apply Nat.leb_gt; lia). reflexivity.
    - replace (op <=? 50) with false by (symmetry; apply Nat.leb_gt; lia).
      rewrite andb_false_r. reflexivity. }
  split.
  { intros op H. cbn. destruct (is_factory_op op); [discriminate |]. rewrite H. discriminate. }
  intros m Hm. repeat (destruct Hm as [<- | Hm]; [reflexivity |]). destruct Hm.
Qed.

(* ── 7. Space creation (generic.rs prepare_create) ─────────────────────── *)

Inductive create_error : Type := SpaceAlreadyExists | ConfigInvalid | ConfigNamesOtherSpace | FamilyUnavailable.

(* The staged operations: the config record on C(sid), S(sid) = 0, and the
   handle continuation parked on H(sid). *)
Inductive staged : Type := StoreConfig : bytes -> staged | StoreCounter : nat -> staged | ParkHandle : staged.

Inductive create_result : Type := Staged : list staged -> create_result | CreateErr : create_error -> create_result.

Inductive receipt : Type := RStored | RParked | ROther.

Section Create.
  Variable cfg : Type.
  (* SpaceConfig::from_par: the space id that the config names. *)
  Variable config_sid : cfg -> option bytes.
  (* check_family: the agent implements the family of the config. *)
  Variable family_ok : cfg -> bool.
  (* The canonical map of the config, stored on C(sid). *)
  Variable config_map : cfg -> bytes.

  (* The records on C(sid) in the staged view, then the creation. *)
  Definition prepare_create (on_config : list bytes) (sid : bytes) (c : cfg) : create_result :=
    match on_config with
    | _ :: _ => CreateErr SpaceAlreadyExists
    | [] =>
        match config_sid c with
        | None => CreateErr ConfigInvalid
        | Some s =>
            if negb (bytes_eqb s sid) then CreateErr ConfigNamesOtherSpace
            else if negb (family_ok c) then CreateErr FamilyUnavailable
            else Staged [StoreConfig (config_map c); StoreCounter 0; ParkHandle]
        end
    end.

  (* The receipt check of the Prepared value: exactly two stores and one
     park. *)
  Definition receipt_ok (rs : list receipt) : bool :=
    match rs with [RStored; RStored; RParked] => true | _ => false end.

  (* The config records on C(sid) after a commit of the staged operations. *)
  Definition apply_config (on_config : list bytes) (ops : list staged) : list bytes :=
    on_config ++ flat_map (fun o => match o with StoreConfig m => [m] | _ => [] end) ops.

  (* Headline 8. A creation whose C(sid) holds a record aborts with
     SpaceAlreadyExists and stages nothing. The outcome depends only on the
     records of C(sid) and on the config, so play and replay, which see the
     same C(sid), take the same outcome. A successful creation stores one
     config record, so a second creation of the same sid aborts. The commit
     accepts only the receipt of all three operations. *)
  Theorem space_already_exists_is_deterministic_abort :
    (forall rec recs sid c, prepare_create (rec :: recs) sid c = CreateErr SpaceAlreadyExists) /\
    (forall st1 st2 : bytes -> list bytes, forall sid c,
        st1 sid = st2 sid -> prepare_create (st1 sid) sid c = prepare_create (st2 sid) sid c) /\
    (forall sid c ops, prepare_create [] sid c = Staged ops ->
        apply_config [] ops = [config_map c] /\
        forall c', prepare_create (apply_config [] ops) sid c' = CreateErr SpaceAlreadyExists) /\
    (forall sid c ops, prepare_create [] sid c = Staged ops ->
        length ops = 3 /\ forall rs, receipt_ok rs = true -> rs = [RStored; RStored; RParked]).
  Proof.
    split; [intros; reflexivity |].
    split; [intros st1 st2 sid c H; rewrite H; reflexivity |].
    split.
    - intros sid c ops H. unfold prepare_create in H.
      destruct (config_sid c) as [s |]; [| discriminate H].
      destruct (negb (bytes_eqb s sid)); [discriminate H |].
      destruct (negb (family_ok c)); [discriminate H |].
      injection H as <-. split; [reflexivity | intros c'; reflexivity].
    - intros sid c ops H. unfold prepare_create in H.
      destruct (config_sid c) as [s |]; [| discriminate H].
      destruct (negb (bytes_eqb s sid)); [discriminate H |].
      destruct (negb (family_ok c)); [discriminate H |].
      injection H as <-. split; [reflexivity |].
      intros [| [] [| [] [| [] [| r rs]]]] Hr; cbn in Hr; try discriminate Hr; reflexivity.
  Qed.
End Create.

(* ── Headline assumptions ────────────────────────────────────────────── *)
Print Assumptions family_index_bijective.
Print Assumptions aliases_map_to_family.
Print Assumptions qualifier_matrix.
Print Assumptions constructor_parse_canonical.
Print Assumptions constructor_parse_sound.
Print Assumptions sid_derivation_deterministic.
Print Assumptions persistent_op_call_aborts.
Print Assumptions space_already_exists_is_deterministic_abort.

(* ═══════════════════════════════════════════════════════════════════════════
   P5 — registration, the FIPS alias and the constructor params

   This section extends the module for phase P5. It comes after the P4
   module, so no line of P4 moves: documents cite those lines. Its
   headlines are 9 to 13.

   Sources
   - Design: P5 design v3 A.7 (the FACTORY row: registration, the alias
     HashMapBagSpace, the schemas of bag, set and cell), A.8 (stack and the
     priority queue, priorities of I-10), A.10 (the Array and Vector
     families, size and cyclic of F7) and A.13 (SpaceFactory.v: three
     headlines in P5a, one in P5b, one in P5d); P5 design v2 §2.5 (families,
     aliases and getSpaceAgent) and §2.8.1 (F26 to F28).
   - Plan: decisions 7, 9.4 and 33, I-10 and the P5 choice F7.
   - Code: the tree of the P5 FACTORY batch (wave W2).

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   sys_channel, body_ref    rholang/src/rust/interpreter/reified/
                            system_channels.rs:30, 34-38, 42
   p4_rows, p4_others       rholang/src/rust/interpreter/spaces/factory/
                            urn.rs:35-36 (REGISTERED_FAMILIES),
                            rholang/src/rust/interpreter/reified/
                            system_channels.rs:49-53, 56-60, 63-67, 70-74,
                            77-82 (P4_SYSTEM_CHANNELS)
   p5_rows                  rholang/src/rust/interpreter/spaces/factory/
                            urn.rs:47-140 (P5_FAMILY_ROWS),
                            rholang/src/rust/interpreter/reified/
                            system_channels.rs:125-283 (the factories)
   alias_rows               rholang/src/rust/interpreter/spaces/factory/
                            urn.rs:144-153 (P5_ALIAS_ROWS),
                            rholang/src/rust/interpreter/reified/
                            system_channels.rs:289-293, 323-349
   schema_of, required_of   rholang/src/rust/interpreter/spaces/factory/
                            defaults.rs:68-99, 103-105, 109-111
   wired                    families::Family::of_urn, through
                            rholang/src/rust/interpreter/spaces/factory/
                            urn.rs:156 (is_wired)
   registered, is_registered, registered_aliases
                            rholang/src/rust/interpreter/spaces/factory/
                            urn.rs:160-164, 168-172, 176-180, 183-187
   definitions, binds       rholang/src/rust/interpreter/
                            reified/system_processes.rs:63-104 (definitions),
                            rholang/src/rust/
                            interpreter/system_processes.rs:491-499 (to_urn_map)
   get_space_agent_of       rholang/src/rust/interpreter/
                            reified/system_processes.rs:164-200 (get_space_agent),
                            rholang/src/rust/interpreter/spaces/factory/
                            constructor.rs:163-181 (family_of), urn.rs:198-200
                            (factory_capability)
   rule, rule_of, canonical rholang/src/rust/interpreter/spaces/factory/
                            defaults.rs:115-122, 126-133, 138-150 (ParamRule)
   suggest, least, within, suggestion_bound
                            rholang/src/rust/interpreter/spaces/factory/
                            defaults.rs:39, 170-179, 184-211
   validate_params, canonical_all
                            rholang/src/rust/interpreter/spaces/factory/
                            constructor.rs:60-112 (validate), 146-160
                            (canonical_params)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - The dispatch arms are the section variable wired, with the hypothesis
     that hashmap:queue is wired (S1). The corollaries registration_today and
     registration_all_wired compute the registration for the arms of today
     and for every row wired, so the theorems of the section are not
     vacuous.
   - A definition is its system channel and the family that its factory
     creates (None for getSpaceAgent, assertEqual and the handle process).
     The URN map is the relation binds over the definitions:
     Definition::to_urn_map binds the URN to the write bundle of the op
     channel, so to the op. all_definitions_functional shows that one URN
     binds one op for every wiring.
   - A param value is its shape: a GInt (VInt), a GBool (VBool), another
     value (VOther: a string, Nil, two expressions or a process) or a value
     with the connective flag (VFlagged). The bit sets are not modeled:
     ParamRule::canonical ignores them, and the canonical value has none. A
     GInt is an i64, so the rule [1, i64_max] accepts every GInt of at
     least 1.
   - The Levenshtein distance is the section variable dist, so the
     suggestion headline holds for every distance function.
     distance_within computes it over chars, and its unit tests pin it.
   - validate_params models the gates 1 and 5 to 8 of validate. The gates 2
     to 4 (the parse, the qualifier and the theory) are the P4 model above
     (parse_elements, validate_qualifier).

   Headline theorems (each one has a Print Assumptions line at the end)
    9. registered_families_complete      F26: a family is registered exactly
                                          when it has a row and an arm, with
                                          its op, body ref and schema, and
                                          nothing else is defined
   10. aliases_bind_family_capability    F27: an alias binds the capability
                                          of its family, and getSpaceAgent
                                          replies it
   11. constructor_params_validated      F28: positional, unknown keys with
                                          the least suggestion, missing keys,
                                          then canonical values
   12. priorities_validated              I-10: the optional priorities of a
                                          priority queue, a GInt >= 1
   13. array_size_validated              F7: the required size of an Array,
                                          a GInt in [1, i64_max], and the
                                          optional cyclic GBool

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `apply in_map` cannot unify `?f ?x` with a pair of `snd r` and
     `Some (fst r)`. Use in_map_iff with the witness.
   - `injection H as <- <-` on `(c0, Some g) = (c, Some f)` replaces the
     named c and f by the fresh c0 and g. Use `-> ->`.
   - A plain cbn unfolds is_registered, Nat.ltb, canonical and the byte
     strings of StrP5 into Pos.to_nat terms, so later rewrites find no
     subterm. Name the constants: `cbn [snd]`, `cbn [bytes_ltb]`,
     `cbn [fst snd]`, `cbn [canonical_all]`, `cbn [schema_of f_outer
     f_inner]`.
   - `destruct (least w rest); discriminate` leaves the `if lex_ltb …` of a
     Some branch. Destruct the condition too.
   - `intros []` on `In k []` does not reduce In. Use `cbn [In] in H` and
     destruct H.
   - Inside try, `exact (Ho' Ho)` with Ho' an equation and Ho a negation
     fails silently and leaves the case open: the contradiction is
     `Ho Ho'`.
   - The bullets of a split over the four cases of schema_cases do not
     follow the order of the open goals once try closes some of them. Use
     one `first [...]` over the alternatives.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import ZArith.

(* The ASCII bytes of the names of P5. *)
Module StrP5.
  Import (notations) Stdlib.Strings.String.
  Local Open Scope string_scope.
  Definition size := Str.b "size".
  Definition cyclic := Str.b "cyclic".
  Definition priorities := Str.b "priorities".
  Definition get_space_agent := Str.b "rho:space:getSpaceAgent".
  Definition assert_equal := Str.b "rho:assert:assertEqual".
  Definition handle_op := Str.b "rho:reified:internal:handleOp".
End StrP5.

(* ── 8. Helpers: byte lists, families and lists ───────────────────────── *)

Lemma bytes_eqb_true : forall a b, bytes_eqb a b = true -> a = b.
Proof. intros a b. unfold bytes_eqb. destruct (list_eq_dec Nat.eq_dec a b); [auto | discriminate]. Qed.

(* The membership of a name in a list of names. *)
Definition mem (k : bytes) (l : list bytes) : bool := existsb (bytes_eqb k) l.

Lemma mem_spec : forall k l, mem k l = true <-> In k l.
Proof.
  intros k l. unfold mem. rewrite existsb_exists. split.
  - intros [x [Hin Heq]]. apply bytes_eqb_true in Heq. subst x. exact Hin.
  - intros Hin. exists k. split; [exact Hin | apply bytes_eqb_refl].
Qed.

Definition outer_eqb (a b : outer) : bool :=
  match a, b with
  | HashMap, HashMap | PathMap, PathMap | Array, Array | Vector, Vector | HashSet, HashSet => true
  | _, _ => false
  end.

Definition inner_eqb (a b : inner) : bool :=
  match a, b with
  | Bag, Bag | Queue, Queue | Stack, Stack | SetC, SetC | Cell, Cell
  | PriorityQueue, PriorityQueue | VectorDb, VectorDb => true
  | _, _ => false
  end.

Definition family_eqb (f g : family) : bool := outer_eqb (f_outer f) (f_outer g) && inner_eqb (f_inner f) (f_inner g).

Lemma outer_eqb_eq : forall a b, outer_eqb a b = true <-> a = b.
Proof. intros [] []; cbn; split; intros H; solve [reflexivity | discriminate H]. Qed.

Lemma inner_eqb_eq : forall a b, inner_eqb a b = true <-> a = b.
Proof. intros [] []; cbn; split; intros H; solve [reflexivity | discriminate H]. Qed.

Lemma family_eqb_eq : forall f g, family_eqb f g = true <-> f = g.
Proof.
  intros [o1 i1] [o2 i2]. unfold family_eqb. cbn. rewrite andb_true_iff, outer_eqb_eq, inner_eqb_eq.
  split; [intros [-> ->]; reflexivity | intros H; injection H as -> ->; split; reflexivity].
Qed.

(* A list of numbers without repetition, as a boolean check. *)
Fixpoint nodupb (l : list nat) : bool :=
  match l with [] => true | x :: rest => negb (existsb (Nat.eqb x) rest) && nodupb rest end.

Lemma nodupb_NoDup : forall l, nodupb l = true -> NoDup l.
Proof.
  induction l as [| x rest IH]; intros H; [constructor |].
  cbn in H. apply andb_true_iff in H as [Hx Hr]. constructor; [| exact (IH Hr)].
  intros Hin. apply negb_true_iff in Hx. rewrite <- not_true_iff_false, existsb_exists in Hx.
  apply Hx. exists x. split; [exact Hin | apply Nat.eqb_refl].
Qed.

Lemma find_none_of : forall (A : Type) (p : A -> bool) l, (forall x, In x l -> p x = false) -> find p l = None.
Proof.
  intros A p l. induction l as [| x rest IH]; intros H; [reflexivity |].
  cbn. rewrite (H x (or_introl eq_refl)). apply IH. intros y Hy. exact (H y (or_intror Hy)).
Qed.

(* A filter of the second part of a list keeps the elements distinct. *)
Lemma nodup_map_app_filter :
  forall (A B : Type) (g : A -> B) (p : A -> bool) l1 l2,
    NoDup (map g (l1 ++ l2)) -> NoDup (map g (l1 ++ filter p l2)).
Proof.
  intros A B g p l1 l2. induction l1 as [| a l1 IH]; cbn; intros H.
  - induction l2 as [| b l2 IH2]; cbn in *; [constructor |].
    inversion H as [| x l Hb Hl]; subst.
    destruct (p b); cbn; [constructor |]; try exact (IH2 Hl).
    intros Hin. apply Hb. apply in_map_iff in Hin as [y [Hy Hin]]. apply filter_In in Hin as [Hin _].
    rewrite <- Hy. apply in_map. exact Hin.
  - inversion H as [| x l Ha Hl]; subst. constructor; [| exact (IH Hl)].
    intros Hin. apply Ha. apply in_map_iff in Hin as [y [Hy Hin]]. rewrite <- Hy. apply in_map.
    apply in_app_iff in Hin as [Hin | Hin]; apply in_app_iff; [left; exact Hin |].
    apply filter_In in Hin as [Hin _]. right. exact Hin.
Qed.

(* ── 9. Registration (spaces/factory/urn.rs, reified/system_channels.rs) ── *)

(* A system channel: its URN, its op and its arity. *)
Record sys_channel : Type := mkChan { ch_urn : bytes; ch_op : nat; ch_arity : nat }.

(* system_channels.rs body_ref: the body ref of the op op is 200 + op. *)
Definition body_ref (c : sys_channel) : nat := 200 + ch_op c.

(* The factory channel of the family f with the op op: the URN of f, arity 2. *)
Definition factory_chan (f : family) (op : nat) : sys_channel := mkChan (urn_of f) op 2.

Definition row (o : outer) (i : inner) (op : nat) : family * sys_channel :=
  (mkFamily o i, factory_chan (mkFamily o i) op).

Definition queue_family : family := mkFamily HashMap Queue.
Definition hashmap_bag : family := mkFamily HashMap Bag.

(* REGISTERED_FAMILIES: the row of P4. *)
Definition p4_rows : list (family * sys_channel) := [row HashMap Queue 1].

(* The three other processes of P4_SYSTEM_CHANNELS. *)
Definition p4_others : list sys_channel :=
  [mkChan StrP5.get_space_agent 33 2; mkChan StrP5.assert_equal 34 3; mkChan StrP5.handle_op 48 2].

(* P5_FAMILY_ROWS, in family-index order. *)
Definition p5_rows : list (family * sys_channel) :=
  [row HashMap Bag 0; row HashMap Stack 2; row HashMap SetC 3; row HashMap Cell 4;
   row HashMap PriorityQueue 5;
   row Array Bag 13; row Array Queue 14; row Array Stack 15; row Array SetC 16;
   row Array Cell 17; row Array PriorityQueue 18;
   row Vector Bag 19; row Vector Queue 20; row Vector Stack 21; row Vector SetC 22;
   row Vector Cell 23; row Vector PriorityQueue 24].

(* P5_ALIAS_ROWS: the channel of the alias and the family that it names. *)
Definition alias_rows : list (sys_channel * family) := [(mkChan Str.hashmap_bag_alias 0 2, hashmap_bag)].

(* defaults.rs schema: the named params of the factory of f in bytewise
   order, and the params that it requires. *)
Definition schema_of (f : family) : option (list bytes * list bytes) :=
  match f_outer f, f_inner f with
  | (HashMap | Vector), (Bag | Queue | Stack | SetC | Cell) => Some ([], [])
  | (HashMap | Vector), PriorityQueue => Some ([StrP5.priorities], [])
  | Array, (Bag | Queue | Stack | SetC | Cell) => Some ([StrP5.cyclic; StrP5.size], [StrP5.size])
  | Array, PriorityQueue => Some ([StrP5.cyclic; StrP5.priorities; StrP5.size], [StrP5.size])
  | _, _ => None
  end.

(* Every row has the URN of its family, the op of its index, the body ref
   200 + op, arity 2 and a constructor schema. *)
Lemma rows_facts :
  forall f c, In (f, c) (p4_rows ++ p5_rows) ->
    ch_urn c = urn_of f /\ family_index f = Some (ch_op c) /\ body_ref c = 200 + ch_op c /\
    ch_arity c = 2 /\ schema_of f <> None.
Proof.
  intros f c H. cbn in H.
  repeat (destruct H as [H | H];
          [injection H as <- <-; repeat split; solve [reflexivity | discriminate] |]).
  destruct H.
Qed.

Lemma alias_rows_facts :
  forall c f, In (c, f) alias_rows ->
    parse_urn (ch_urn c) = UrnOk f /\ family_index f = Some (ch_op c) /\ ch_arity c = 2 /\
    ch_urn c <> urn_of f.
Proof.
  intros c f H. cbn in H. destruct H as [H | H]; [| destruct H].
  injection H as <- <-. repeat split; try reflexivity. vm_compute. discriminate.
Qed.

Lemma rows_ops_distinct : NoDup (map (fun r => ch_op (snd r)) (p4_rows ++ p5_rows)).
Proof. apply nodupb_NoDup. vm_compute. reflexivity. Qed.

(* Every definition of the tables, as if every row were registered. *)
Definition all_definitions : list (sys_channel * option family) :=
  map (fun r => (snd r, Some (fst r))) p4_rows ++ map (fun c => (c, None)) p4_others ++
  map (fun r => (snd r, Some (fst r))) p5_rows ++ map (fun a => (fst a, Some (snd a))) alias_rows.

Definition functional_b (l : list (sys_channel * option family)) : bool :=
  forallb (fun d1 => forallb (fun d2 =>
    negb (bytes_eqb (ch_urn (fst d1)) (ch_urn (fst d2))) || (ch_op (fst d1) =? ch_op (fst d2))) l) l.

Lemma functional_b_spec :
  forall l, functional_b l = true ->
    forall d1 d2, In d1 l -> In d2 l -> ch_urn (fst d1) = ch_urn (fst d2) -> ch_op (fst d1) = ch_op (fst d2).
Proof.
  intros l H d1 d2 H1 H2 Hu. unfold functional_b in H. rewrite forallb_forall in H.
  specialize (H d1 H1). rewrite forallb_forall in H. specialize (H d2 H2).
  rewrite Hu, bytes_eqb_refl in H. cbn in H. apply Nat.eqb_eq. exact H.
Qed.

(* The URN map of all definitions is a function: one URN binds one op. *)
Lemma all_definitions_functional :
  forall d1 d2, In d1 all_definitions -> In d2 all_definitions ->
    ch_urn (fst d1) = ch_urn (fst d2) -> ch_op (fst d1) = ch_op (fst d2).
Proof. apply functional_b_spec. vm_compute. reflexivity. Qed.

Section Registration.
  (* families.rs Family::of_urn: the families whose dispatch arm is wired. *)
  Variable wired : family -> bool.
  (* S1 wires hashmap:queue, the family of P4. *)
  Hypothesis queue_wired : wired queue_family = true.

  (* urn.rs registered_families: the row of P4, then the rows of P5 whose
     family is wired. *)
  Definition registered : list (family * sys_channel) := p4_rows ++ filter (fun r => wired (fst r)) p5_rows.

  Definition is_registered (f : family) : bool := existsb (fun r => family_eqb (fst r) f) registered.

  (* urn.rs registered_aliases: the alias rows whose family is registered. *)
  Definition registered_aliases : list (sys_channel * family) :=
    filter (fun a => is_registered (snd a)) alias_rows.

  (* system_processes.rs definitions: the channel of each definition and the
     family that its factory creates (None for the three other processes of
     P4), in the order of the code. *)
  Definition definitions : list (sys_channel * option family) :=
    map (fun r => (snd r, Some (fst r))) p4_rows ++ map (fun c => (c, None)) p4_others ++
    map (fun r => (snd r, Some (fst r))) (filter (fun r => wired (fst r)) p5_rows) ++
    map (fun a => (fst a, Some (snd a))) registered_aliases.

  (* Definition::to_urn_map: the URN of a definition binds the write bundle
     of its channel, so the op of its channel. *)
  Definition binds (urn : bytes) (op : nat) : Prop :=
    exists d, In d definitions /\ ch_urn (fst d) = urn /\ ch_op (fst d) = op.

  Lemma registered_rows :
    forall f c, In (f, c) registered -> In (f, c) (p4_rows ++ p5_rows) /\ wired f = true.
  Proof.
    intros f c H. unfold registered in H. apply in_app_iff in H as [H | H].
    - split; [apply in_app_iff; left; exact H |].
      cbn in H. destruct H as [H | H]; [| destruct H]. injection H as <- _. exact queue_wired.
    - apply filter_In in H as [Hin Hw]. split; [apply in_app_iff; right; exact Hin | exact Hw].
  Qed.

  Lemma is_registered_spec : forall f, is_registered f = true <-> exists c, In (f, c) registered.
  Proof.
    intros f. unfold is_registered. rewrite existsb_exists. split.
    - intros [[g c] [Hin Heq]]. apply family_eqb_eq in Heq. cbn in Heq. subst g. exists c. exact Hin.
    - intros [c Hin]. exists (f, c). split; [exact Hin | apply family_eqb_eq; reflexivity].
  Qed.

  Lemma definitions_incl : forall d, In d definitions -> In d all_definitions.
  Proof.
    intros d H. unfold definitions in H. unfold all_definitions.
    rewrite !in_app_iff in *. destruct H as [H | [H | [H | H]]]; [left | right; left | right; right; left | right; right; right];
      try exact H.
    - apply in_map_iff in H as [r [<- Hin]]. apply filter_In in Hin as [Hin _].
      apply in_map_iff. exists r. split; [reflexivity | exact Hin].
    - apply in_map_iff in H as [a [<- Hin]]. apply filter_In in Hin as [Hin _].
      apply in_map_iff. exists a. split; [reflexivity | exact Hin].
  Qed.

  (* Headline 9 (F26). A registered family has the URN of its family, the op
     of its index, the body ref 200 + op, arity 2, a constructor schema and
     a wired dispatch arm. A family is registered exactly when it has a row
     and an arm, and no two registered families share an op. Every factory
     definition, an alias included, creates a registered family whose index
     is its op, so the dispatch table, keyed by the body ref, is a function.
     Nothing else is defined: the other definitions are the three other
     processes of P4. *)
  Theorem registered_families_complete :
    (forall f c, In (f, c) registered ->
       ch_urn c = urn_of f /\ family_index f = Some (ch_op c) /\ body_ref c = 200 + ch_op c /\
       ch_arity c = 2 /\ schema_of f <> None /\ wired f = true) /\
    (forall f, (exists c, In (f, c) registered) <->
       ((exists c, In (f, c) (p4_rows ++ p5_rows)) /\ wired f = true)) /\
    NoDup (map (fun r => ch_op (snd r)) registered) /\
    (forall c f, In (c, Some f) definitions ->
       family_index f = Some (ch_op c) /\ exists c', In (f, c') registered /\ ch_op c' = ch_op c) /\
    (forall c, In (c, None) definitions -> In c p4_others).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros f c H. destruct (registered_rows f c H) as [Hrow Hw].
      destruct (rows_facts f c Hrow) as [Hu [Hi [Hb [Ha Hs]]]]. repeat split; assumption.
    - intros f. split.
      + intros [c H]. destruct (registered_rows f c H) as [Hrow Hw]. split; [exists c; exact Hrow | exact Hw].
      + intros [[c Hrow] Hw]. exists c. unfold registered. apply in_app_iff in Hrow as [H | H].
        * apply in_app_iff. left. exact H.
        * apply in_app_iff. right. apply filter_In. split; [exact H | exact Hw].
    - unfold registered. apply nodup_map_app_filter. exact rows_ops_distinct.
    - intros c f H. unfold definitions in H. rewrite !in_app_iff in H.
      destruct H as [H | [H | [H | H]]].
      + apply in_map_iff in H as [[g c0] [Heq Hin]]. cbn in Heq. injection Heq as -> ->.
        destruct (rows_facts f c (proj2 (in_app_iff _ _ _) (or_introl Hin))) as [_ [Hi _]].
        split; [exact Hi |]. exists c. split; [| reflexivity]. unfold registered. apply in_app_iff. left. exact Hin.
      + apply in_map_iff in H as [c0 [Heq _]]. discriminate Heq.
      + apply in_map_iff in H as [[g c0] [Heq Hin]]. cbn in Heq. injection Heq as -> ->.
        apply filter_In in Hin as [Hin Hw].
        destruct (rows_facts f c (proj2 (in_app_iff _ _ _) (or_intror Hin))) as [_ [Hi _]].
        split; [exact Hi |]. exists c. split; [| reflexivity].
        unfold registered. apply in_app_iff. right. apply filter_In. split; [exact Hin | exact Hw].
      + apply in_map_iff in H as [[c0 g] [Heq Hin]]. cbn in Heq. injection Heq as -> ->.
        apply filter_In in Hin as [Hin Hreg]. cbn in Hreg.
        destruct (alias_rows_facts c f Hin) as [_ [Hi _]]. split; [exact Hi |].
        apply is_registered_spec in Hreg as [c' Hc'].
        destruct (registered_rows f c' Hc') as [Hrow _].
        destruct (rows_facts f c' Hrow) as [_ [Hi' _]].
        exists c'. split; [exact Hc' | congruence].
    - intros c H. unfold definitions in H. rewrite !in_app_iff in H.
      destruct H as [H | [H | [H | H]]]; apply in_map_iff in H as [x [Heq Hin]].
      + discriminate Heq.
      + injection Heq as ->. exact Hin.
      + discriminate Heq.
      + discriminate Heq.
  Qed.

  (* get_space_agent: the capability of the family that a config names
     (constructor.rs family_of, urn.rs factory_capability). The factory of a
     definition (c, Some f) stores a config that names f (factory.rs,
     FactoryConstructor::config). *)
  Definition get_space_agent_of (f : family) : option nat :=
    match family_of_names (outer_name (f_outer f)) (inner_name (f_inner f)) with
    | Some g => factory_op g
    | None => None
    end.

  (* Headline 10 (F27). The alias names the op of its family and is not the
     URN of the family. An alias is registered exactly when its family is
     registered. The URN map of the definitions is a function, the alias
     binds the op of its family, and so does the URN of the family: both
     bind the capability bundle+{[0xE0, op]}. getSpaceAgent of a space that
     any factory definition creates, through an alias or through the URN of
     the family, replies the op of that definition (decision 9.4), so
     assertEqual(getSpaceAgent(space), alias) holds (decision 33). *)
  Theorem aliases_bind_family_capability :
    (forall c f, In (c, f) alias_rows ->
       parse_urn (ch_urn c) = UrnOk f /\ family_index f = Some (ch_op c) /\ ch_arity c = 2 /\
       ch_urn c <> urn_of f) /\
    (forall c f, In (c, f) registered_aliases <->
       (In (c, f) alias_rows /\ exists cf, In (f, cf) registered)) /\
    (forall u o1 o2, binds u o1 -> binds u o2 -> o1 = o2) /\
    (forall c f, In (c, f) registered_aliases ->
       binds (ch_urn c) (ch_op c) /\ binds (urn_of f) (ch_op c) /\ factory_op f = Some (ch_op c)) /\
    (forall c f, In (c, Some f) definitions -> get_space_agent_of f = Some (ch_op c)).
  Proof.
    destruct registered_families_complete as [Hreg [_ [_ [Hdef _]]]].
    split; [exact alias_rows_facts |].
    split; [| split; [| split]].
    - intros c f. unfold registered_aliases. rewrite filter_In. cbn [snd]. rewrite is_registered_spec. tauto.
    - intros u o1 o2 [d1 [H1 [Hu1 Ho1]]] [d2 [H2 [Hu2 Ho2]]]. subst o1 o2.
      apply all_definitions_functional; [apply definitions_incl; exact H1 | apply definitions_incl; exact H2 | congruence].
    - intros c f H.
      assert (Hdef_alias : In (c, Some f) definitions).
      { unfold definitions. rewrite !in_app_iff. right. right. right.
        apply in_map_iff. exists (c, f). split; [reflexivity | exact H]. }
      destruct (Hdef c f Hdef_alias) as [Hi [c' [Hc' Hop]]].
      destruct (Hreg f c' Hc') as [Hu' _].
      split; [exists (c, Some f); split; [exact Hdef_alias | split; reflexivity] |].
      split; [| exact Hi].
      assert (Hdef_family : In (c', Some f) definitions).
      { unfold registered in Hc'. unfold definitions. rewrite !in_app_iff.
        apply in_app_iff in Hc' as [Hc' | Hc'].
        - left. apply in_map_iff. exists (f, c'). split; [reflexivity | exact Hc'].
        - right. right. left. apply in_map_iff. exists (f, c'). split; [reflexivity | exact Hc']. }
      exists (c', Some f). split; [exact Hdef_family |]. split; [exact Hu' | exact Hop].
    - intros c f H. destruct (Hdef c f H) as [Hi _].
      assert (Hsup : is_supported_pair f = true).
      { unfold family_index in Hi. destruct (is_supported_pair f); [reflexivity | discriminate Hi]. }
      destruct aliases_map_to_family as [_ [_ [_ [_ [_ [_ Hnames]]]]]].
      unfold get_space_agent_of. rewrite (Hnames f (supported_in_all f Hsup)). exact Hi.
  Qed.
End Registration.

(* The registration of today (S1 wires only hashmap:queue) and the
   registration when every row is wired: the theorems of the section hold
   for both, so they are not vacuous. *)
Corollary registration_today :
  registered (fun f => family_eqb f queue_family) = p4_rows /\
  registered_aliases (fun f => family_eqb f queue_family) = [].
Proof. split; vm_compute; reflexivity. Qed.

Corollary registration_all_wired :
  registered (fun _ => true) = p4_rows ++ p5_rows /\ registered_aliases (fun _ => true) = alias_rows.
Proof. split; vm_compute; reflexivity. Qed.

(* ── 10. The constructor params (defaults.rs, constructor.rs) ──────────── *)

(* The greatest GInt. A GInt is an i64, so the rules below accept
   [1, i64_max] exactly when they accept every value of at least 1. *)
Definition i64_max : Z := 9223372036854775807%Z.

(* A param value: a GInt, a GBool, another value (a string, Nil, several
   expressions or a process), or a value with a connective flag. The bit
   sets of a value are not modeled: the rules ignore them, and the canonical
   form has none. *)
Inductive pvalue : Type :=
| VInt : Z -> pvalue
| VBool : bool -> pvalue
| VOther : nat -> pvalue
| VFlagged : pvalue -> pvalue.

(* ParamRule. *)
Inductive rule : Type := RSize | RCyclic | RPriorities.

(* ParamRule::of. *)
Definition rule_of (k : bytes) : option rule :=
  if bytes_eqb k StrP5.size then Some RSize
  else if bytes_eqb k StrP5.cyclic then Some RCyclic
  else if bytes_eqb k StrP5.priorities then Some RPriorities
  else None.

(* ParamRule::canonical: size is a GInt in [1, i64_max] (F7), priorities is
   a GInt of at least 1 (I-10), cyclic is a GBool (F7). *)
Definition canonical (r : rule) (v : pvalue) : option pvalue :=
  match r, v with
  | RSize, VInt z => if (1 <=? z)%Z && (z <=? i64_max)%Z then Some (VInt z) else None
  | RPriorities, VInt z => if (1 <=? z)%Z && (z <=? i64_max)%Z then Some (VInt z) else None
  | RCyclic, VBool b => Some (VBool b)
  | _, _ => None
  end.

(* The required params of a family (defaults.rs required_params). *)
Definition required_of (f : family) : list bytes :=
  match schema_of f with Some (_, required) => required | None => [] end.

(* Bytewise order of names, as the order of Rust strings. *)
Fixpoint bytes_ltb (a b : bytes) : bool :=
  match a, b with
  | [], [] => false
  | [], _ :: _ => true
  | _ :: _, [] => false
  | x :: a', y :: b' => (x <? y) || ((x =? y) && bytes_ltb a' b')
  end.

Lemma bytes_ltb_irrefl : forall a, bytes_ltb a a = false.
Proof. induction a as [| x a IH]; [reflexivity |]. cbn [bytes_ltb]. rewrite Nat.ltb_irrefl, Nat.eqb_refl. exact IH. Qed.

Lemma bytes_ltb_trans :
  forall a b c, bytes_ltb a b = true -> bytes_ltb b c = true -> bytes_ltb a c = true.
Proof.
  induction a as [| x a IH]; intros [| y b] [| z c] H1 H2; cbn [bytes_ltb] in *; try discriminate; try reflexivity.
  apply orb_true_iff in H1, H2. apply orb_true_iff.
  destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
  - left. apply Nat.ltb_lt in H1, H2. apply Nat.ltb_lt. lia.
  - left. apply andb_true_iff in H2 as [E2 _]. apply Nat.eqb_eq in E2. subst z. exact H1.
  - left. apply andb_true_iff in H1 as [E1 _]. apply Nat.eqb_eq in E1. subst y. exact H2.
  - right. apply andb_true_iff in H1 as [E1 H1], H2 as [E2 H2]. apply Nat.eqb_eq in E1, E2. subst y z.
    rewrite Nat.eqb_refl. exact (IH b c H1 H2).
Qed.

Lemma bytes_ltb_total : forall a b, bytes_ltb a b = false -> bytes_ltb b a = false -> a = b.
Proof.
  induction a as [| x a IH]; intros [| y b] H1 H2; cbn [bytes_ltb] in *; try discriminate; try reflexivity.
  apply orb_false_iff in H1 as [L1 R1], H2 as [L2 R2]. apply Nat.ltb_ge in L1, L2.
  assert (x = y) by lia. subst y. rewrite Nat.eqb_refl in R1, R2. cbn in R1, R2.
  rewrite (IH b R1 R2). reflexivity.
Qed.

(* The order of the suggestion: the least distance, then the bytewise least
   candidate, as the order of the Rust tuple (usize, &str). *)
Definition lex_ltb (a b : nat * bytes) : bool :=
  (fst a <? fst b) || ((fst a =? fst b) && bytes_ltb (snd a) (snd b)).

Definition lex_leb (a b : nat * bytes) : bool := negb (lex_ltb b a).

Lemma lex_ltb_trans : forall a b c, lex_ltb a b = true -> lex_ltb b c = true -> lex_ltb a c = true.
Proof.
  intros [n a] [m b] [k c]. unfold lex_ltb. cbn [fst snd]. intros H1 H2.
  apply orb_true_iff in H1, H2. apply orb_true_iff.
  destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
  - left. apply Nat.ltb_lt in H1, H2. apply Nat.ltb_lt. lia.
  - left. apply andb_true_iff in H2 as [E2 _]. apply Nat.eqb_eq in E2. subst k. exact H1.
  - left. apply andb_true_iff in H1 as [E1 _]. apply Nat.eqb_eq in E1. subst m. exact H2.
  - right. apply andb_true_iff in H1 as [E1 H1], H2 as [E2 H2]. apply Nat.eqb_eq in E1, E2. subst m k.
    rewrite Nat.eqb_refl. exact (bytes_ltb_trans a b c H1 H2).
Qed.

Lemma lex_total : forall a b, lex_ltb a b = false -> lex_ltb b a = false -> a = b.
Proof.
  intros [n a] [m b]. unfold lex_ltb. cbn [fst snd]. intros H1 H2.
  apply orb_false_iff in H1 as [L1 R1], H2 as [L2 R2]. apply Nat.ltb_ge in L1, L2.
  assert (n = m) by lia. subst m. rewrite Nat.eqb_refl in R1, R2. cbn in R1, R2.
  rewrite (bytes_ltb_total a b R1 R2). reflexivity.
Qed.

Lemma lex_ltb_irrefl : forall a, lex_ltb a a = false.
Proof. intros [n a]. unfold lex_ltb. cbn [fst snd]. rewrite Nat.ltb_irrefl, Nat.eqb_refl, bytes_ltb_irrefl. reflexivity. Qed.

Lemma lex_ltb_asym : forall a b, lex_ltb a b = true -> lex_ltb b a = false.
Proof.
  intros a b H. destruct (lex_ltb b a) eqn:E; [| reflexivity].
  pose proof (lex_ltb_trans a b a H E) as Haa. rewrite lex_ltb_irrefl in Haa. discriminate Haa.
Qed.

Lemma lex_leb_refl : forall a, lex_leb a a = true.
Proof. intros [n a]. unfold lex_leb, lex_ltb. cbn [fst snd]. rewrite Nat.ltb_irrefl, Nat.eqb_refl, bytes_ltb_irrefl. reflexivity. Qed.

(* The result of the gates of the params. *)
Inductive perror : Type :=
| EUnavailable
| EPositional
| EUnknown (k : bytes) (s : option bytes)
| EMissing (k : bytes)
| EInvalid (k : bytes)
| EInconsistent.

Inductive presult : Type := POk (ps : list (bytes * pvalue)) | PErr (e : perror).

(* constructor.rs canonical_params: each named param in the canonical form of
   its rule, in key order. A key without a rule is inconsistent. *)
Fixpoint canonical_all (named : list (bytes * pvalue)) : presult :=
  match named with
  | [] => POk []
  | (k, v) :: rest =>
      match rule_of k with
      | None => PErr EInconsistent
      | Some r =>
          match canonical r v with
          | None => PErr (EInvalid k)
          | Some v' => match canonical_all rest with POk ps => POk ((k, v') :: ps) | PErr e => PErr e end
          end
      end
  end.

Lemma canonical_all_ok :
  forall named ps, canonical_all named = POk ps ->
    map fst ps = map fst named /\
    Forall2 (fun kv kv' => exists r, rule_of (fst kv) = Some r /\ canonical r (snd kv) = Some (snd kv')) named ps.
Proof.
  induction named as [| [k v] rest IH]; intros ps H; cbn in H.
  - injection H as <-. split; [reflexivity | constructor].
  - destruct (rule_of k) as [r |] eqn:Er; [| discriminate H].
    destruct (canonical r v) as [v' |] eqn:Ev; [| discriminate H].
    destruct (canonical_all rest) as [ps' |] eqn:Erest; [| discriminate H].
    injection H as <-. destruct (IH ps' eq_refl) as [Hk Hf].
    split; [cbn; rewrite Hk; reflexivity |]. constructor; [| exact Hf].
    exists r. split; [exact Er | exact Ev].
Qed.

Lemma canonical_all_consistent :
  forall named, (forall kv, In kv named -> rule_of (fst kv) <> None) -> canonical_all named <> PErr EInconsistent.
Proof.
  induction named as [| [k v] rest IH]; intros H; cbn; [discriminate |].
  destruct (rule_of k) as [r |] eqn:Er; [| exfalso; exact (H (k, v) (or_introl eq_refl) Er)].
  destruct (canonical r v) as [v' |]; [| discriminate].
  specialize (IH (fun kv Hin => H kv (or_intror Hin))).
  destruct (canonical_all rest) as [ps | e]; [discriminate |]. exact IH.
Qed.

(* Every key of a schema has a rule. *)
Lemma schema_keys_ruled : forall f s r k, schema_of f = Some (s, r) -> In k s -> rule_of k <> None.
Proof.
  intros [[] []] s r k H Hk; cbn in H; try discriminate H; injection H as <- <-; cbn in Hk;
    repeat (destruct Hk as [<- | Hk]; [vm_compute; discriminate |]); destruct Hk.
Qed.

(* Every required key is a key of the schema. *)
Lemma required_in_schema : forall f s r k, schema_of f = Some (s, r) -> In k r -> In k s.
Proof.
  intros [[] []] s r k H Hk; cbn in H; try discriminate H; injection H as <- <-; cbn in Hk;
    repeat (destruct Hk as [<- | Hk]; [cbn; tauto |]); destruct Hk.
Qed.

Section Params.
  (* defaults.rs distance_within: the Levenshtein distance of two names. *)
  Variable dist : bytes -> bytes -> nat.

  (* SUGGESTION_DISTANCE. *)
  Definition suggestion_bound : nat := 2.

  Definition within (w c : bytes) : bool := dist w c <=? suggestion_bound.

  (* defaults.rs suggestion: Iterator::min over the pairs (distance,
     candidate) of the candidates within the bound. *)
  Fixpoint least (w : bytes) (cs : list bytes) : option (nat * bytes) :=
    match cs with
    | [] => None
    | c :: rest =>
        match within w c, least w rest with
        | true, Some b => if lex_ltb b (dist w c, c) then Some b else Some (dist w c, c)
        | true, None => Some (dist w c, c)
        | false, best => best
        end
    end.

  Definition suggest (w : bytes) (cs : list bytes) : option bytes := option_map snd (least w cs).

  Lemma least_spec :
    forall w cs p, least w cs = Some p ->
      In (snd p) cs /\ within w (snd p) = true /\ fst p = dist w (snd p) /\
      forall c', In c' cs -> within w c' = true -> lex_leb p (dist w c', c') = true.
  Proof.
    intros w. induction cs as [| c rest IH]; intros p H; cbn in H; [discriminate H |].
    destruct (within w c) eqn:Ec.
    - destruct (least w rest) as [b |] eqn:Eb.
      + destruct (IH b eq_refl) as [Hin [Hw [Hd Hle]]].
        destruct (lex_ltb b (dist w c, c)) eqn:Elt; injection H as <-.
        * split; [right; exact Hin |]. split; [exact Hw |]. split; [exact Hd |].
          intros c' [<- | Hin'] Hw'.
          -- unfold lex_leb. rewrite (lex_ltb_asym _ _ Elt). reflexivity.
          -- exact (Hle c' Hin' Hw').
        * split; [left; reflexivity |]. split; [exact Ec |]. split; [reflexivity |].
          intros c' [<- | Hin'] Hw'; [apply lex_leb_refl |].
          unfold lex_leb. apply negb_true_iff. destruct (lex_ltb (dist w c', c') (dist w c, c)) eqn:E2; [| reflexivity].
          exfalso. specialize (Hle c' Hin' Hw'). unfold lex_leb in Hle. apply negb_true_iff in Hle.
          assert (Hbc : lex_ltb b (dist w c, c) = true).
          { destruct (lex_ltb b (dist w c', c')) eqn:E3.
            - exact (lex_ltb_trans _ _ _ E3 E2).
            - rewrite (lex_total _ _ E3 Hle). exact E2. }
          rewrite Hbc in Elt. discriminate Elt.
      + injection H as <-. split; [left; reflexivity |]. split; [exact Ec |]. split; [reflexivity |].
        intros c' [<- | Hin'] Hw'; [apply lex_leb_refl |].
        exfalso. clear IH. induction rest as [| x rest' IHr]; [destruct Hin' |].
        cbn in Eb. destruct (within w x) eqn:Ex.
        * destruct (least w rest') as [b |]; [destruct (lex_ltb b (dist w x, x)) |]; discriminate Eb.
        * destruct Hin' as [<- | Hin']; [rewrite Ex in Hw'; discriminate Hw' | exact (IHr Eb Hin')].
    - destruct (IH p H) as [Hin [Hw [Hd Hle]]].
      split; [right; exact Hin |]. split; [exact Hw |]. split; [exact Hd |].
      intros c' [<- | Hin'] Hw'; [rewrite Ec in Hw'; discriminate Hw' | exact (Hle c' Hin' Hw')].
  Qed.

  Lemma least_none : forall w cs, least w cs = None -> forall c, In c cs -> within w c = false.
  Proof.
    intros w. induction cs as [| x rest IH]; intros H c Hin; [destruct Hin |].
    cbn in H. destruct (within w x) eqn:Ex.
    - destruct (least w rest) as [b |]; [destruct (lex_ltb b (dist w x, x)) |]; discriminate H.
    - destruct Hin as [<- | Hin]; [exact Ex | exact (IH H c Hin)].
  Qed.

  (* The suggestion is the least (distance, key) within the bound: a function
     of the name and of the set of candidates. *)
  Lemma suggest_least :
    forall w cs c, suggest w cs = Some c <->
      (In c cs /\ within w c = true /\
       forall c', In c' cs -> within w c' = true -> lex_leb (dist w c, c) (dist w c', c') = true).
  Proof.
    intros w cs c. unfold suggest. split.
    - destruct (least w cs) as [p |] eqn:E; cbn; intros H; [| discriminate H].
      injection H as <-. destruct (least_spec w cs p E) as [Hin [Hw [Hd Hle]]].
      split; [exact Hin |]. split; [exact Hw |]. intros c' Hin' Hw'.
      rewrite <- Hd. replace (fst p, snd p) with p by (destruct p; reflexivity). exact (Hle c' Hin' Hw').
    - intros [Hin [Hw Hle]]. destruct (least w cs) as [p |] eqn:E.
      + destruct (least_spec w cs p E) as [Hin' [Hw' [Hd Hle']]]. cbn. f_equal.
        specialize (Hle (snd p) Hin' Hw'). specialize (Hle' c Hin Hw).
        unfold lex_leb in Hle, Hle'. apply negb_true_iff in Hle, Hle'.
        rewrite <- Hd in Hle. replace (fst p, snd p) with p in Hle by (destruct p; reflexivity).
        pose proof (lex_total _ _ Hle' Hle) as Heq. rewrite <- Heq. reflexivity.
      + rewrite (least_none w cs E c Hin) in Hw. discriminate Hw.
  Qed.

  (* constructor.rs validate, the gates 1 and 5 to 8: the family has a
     schema, the params are not positional, every key is in the schema, every
     required key is present, and every value is accepted. The gates 2 to 4
     (the parse, the qualifier and the theory) are the P4 model above. *)
  Definition validate_params (f : family) (positional : list pvalue) (named : list (bytes * pvalue)) : presult :=
    match schema_of f with
    | None => PErr EUnavailable
    | Some (schema, required) =>
        match positional with
        | _ :: _ => PErr EPositional
        | [] =>
            match find (fun kv => negb (mem (fst kv) schema)) named with
            | Some (k, _) => PErr (EUnknown k (suggest k schema))
            | None =>
                match find (fun k => negb (mem k (map fst named))) required with
                | Some k => PErr (EMissing k)
                | None => canonical_all named
                end
            end
        end
    end.

  (* Headline 11 (F28). A family without a schema is unavailable. Positional
     params fail first. The first unknown key fails with the least (distance,
     key) suggestion within two edits, a deterministic function of the key
     and of the schema. An accepted call keeps the keys in their order, every
     key is in the schema, every required key is present, and every value is
     the canonical form of the value of its rule. The gates are total: no
     call is inconsistent. *)
  Theorem constructor_params_validated :
    (forall f pos named, schema_of f = None -> validate_params f pos named = PErr EUnavailable) /\
    (forall f s r p pos named, schema_of f = Some (s, r) ->
       validate_params f (p :: pos) named = PErr EPositional) /\
    (forall f s r named k v, schema_of f = Some (s, r) ->
       find (fun kv => negb (mem (fst kv) s)) named = Some (k, v) ->
       validate_params f [] named = PErr (EUnknown k (suggest k s))) /\
    (forall w cs c, suggest w cs = Some c <->
       (In c cs /\ within w c = true /\
        forall c', In c' cs -> within w c' = true -> lex_leb (dist w c, c) (dist w c', c') = true)) /\
    (forall f s r named ps, schema_of f = Some (s, r) -> validate_params f [] named = POk ps ->
       map fst ps = map fst named /\ (forall k, In k (map fst ps) -> In k s) /\
       (forall k, In k r -> In k (map fst ps)) /\
       Forall2 (fun kv kv' => exists rl, rule_of (fst kv) = Some rl /\ canonical rl (snd kv) = Some (snd kv'))
         named ps) /\
    (forall f pos named, validate_params f pos named <> PErr EInconsistent).
  Proof.
    split; [intros f pos named H; unfold validate_params; rewrite H; reflexivity |].
    split; [intros f s r p pos named H; unfold validate_params; rewrite H; reflexivity |].
    split; [intros f s r named k v H Hf; unfold validate_params; rewrite H, Hf; reflexivity |].
    split; [exact suggest_least |].
    split.
    - intros f s r named ps H Hok. unfold validate_params in Hok. rewrite H in Hok.
      destruct (find (fun kv => negb (mem (fst kv) s)) named) as [[k v] |] eqn:Eu; [discriminate Hok |].
      destruct (find (fun k => negb (mem k (map fst named))) r) as [k |] eqn:Em; [discriminate Hok |].
      destruct (canonical_all_ok named ps Hok) as [Hkeys Hvals].
      split; [exact Hkeys |]. split; [| split; [| exact Hvals]].
      + intros k Hk. rewrite Hkeys in Hk. apply in_map_iff in Hk as [[k' v'] [<- Hin]].
        pose proof (find_none _ _ Eu (k', v') Hin) as Hm. cbn in Hm.
        apply negb_false_iff, mem_spec in Hm. exact Hm.
      + intros k Hk. rewrite Hkeys. pose proof (find_none _ _ Em k Hk) as Hm.
        apply negb_false_iff, mem_spec in Hm. exact Hm.
    - intros f pos named. unfold validate_params.
      destruct (schema_of f) as [[s r] |] eqn:H; [| discriminate].
      destruct pos; [| discriminate].
      destruct (find (fun kv => negb (mem (fst kv) s)) named) as [[k v] |] eqn:Eu; [discriminate |].
      destruct (find (fun k => negb (mem k (map fst named))) r) as [k |]; [discriminate |].
      apply canonical_all_consistent. intros [k v] Hin.
      pose proof (find_none _ _ Eu (k, v) Hin) as Hm. cbn in Hm. apply negb_false_iff, mem_spec in Hm.
      exact (schema_keys_ruled f s r k H Hm).
  Qed.

  Lemma canonical_int_rule :
    forall r, r = RSize \/ r = RPriorities ->
      forall v v', canonical r v = Some v' <-> exists z, v = VInt z /\ (1 <= z <= i64_max)%Z /\ v' = VInt z.
  Proof.
    intros r Hr v v'. destruct Hr as [-> | ->]; destruct v as [z | b | n | v0]; cbn;
      try (split; [discriminate | intros [z' [Hz _]]; discriminate Hz]).
    - destruct ((1 <=? z)%Z && (z <=? i64_max)%Z) eqn:E.
      + apply andb_true_iff in E as [E1 E2]. apply Z.leb_le in E1, E2. split.
        * intros H. injection H as <-. exists z. repeat split; assumption.
        * intros [z' [Hz [_ ->]]]. injection Hz as <-. reflexivity.
      + split; [discriminate |]. intros [z' [Hz [[H1 H2] _]]]. injection Hz as <-.
        apply Z.leb_le in H1, H2. rewrite H1, H2 in E. discriminate E.
    - destruct ((1 <=? z)%Z && (z <=? i64_max)%Z) eqn:E.
      + apply andb_true_iff in E as [E1 E2]. apply Z.leb_le in E1, E2. split.
        * intros H. injection H as <-. exists z. repeat split; assumption.
        * intros [z' [Hz [_ ->]]]. injection Hz as <-. reflexivity.
      + split; [discriminate |]. intros [z' [Hz [[H1 H2] _]]]. injection Hz as <-.
        apply Z.leb_le in H1, H2. rewrite H1, H2 in E. discriminate E.
  Qed.

  (* The concrete facts of the schemas and the rules, one per statement, so
     each proof below names the fact that it uses. *)
  Lemma rule_of_size : rule_of StrP5.size = Some RSize.
  Proof. vm_compute. reflexivity. Qed.

  Lemma rule_of_cyclic : rule_of StrP5.cyclic = Some RCyclic.
  Proof. vm_compute. reflexivity. Qed.

  Lemma rule_of_priorities : rule_of StrP5.priorities = Some RPriorities.
  Proof. vm_compute. reflexivity. Qed.

  Lemma canonical_in_range :
    forall r z, r = RSize \/ r = RPriorities -> (1 <= z <= i64_max)%Z -> canonical r (VInt z) = Some (VInt z).
  Proof.
    intros r z Hr Hz. apply (canonical_int_rule r Hr). exists z. split; [reflexivity | split; [exact Hz | reflexivity]].
  Qed.

  Lemma find_unknown_none :
    forall s (named : list (bytes * pvalue)), (forall kv, In kv named -> In (fst kv) s) ->
      find (fun kv => negb (mem (fst kv) s)) named = None.
  Proof. intros s named H. apply find_none_of. intros kv Hin. apply negb_false_iff, mem_spec. exact (H kv Hin). Qed.

  Lemma find_required_none :
    forall r keys, (forall k, In k r -> In k keys) -> find (fun k => negb (mem k keys)) r = None.
  Proof. intros r keys H. apply find_none_of. intros k Hin. apply negb_false_iff, mem_spec. exact (H k Hin). Qed.

  (* The names of the params differ. *)
  Lemma param_names_distinct :
    StrP5.size <> StrP5.cyclic /\ StrP5.size <> StrP5.priorities /\ StrP5.cyclic <> StrP5.priorities.
  Proof. repeat split; intros H; vm_compute in H; discriminate H. Qed.

  (* The schema of every family with a schema, by outer store and inner
     collection. *)
  Lemma schema_cases :
    forall f s r, schema_of f = Some (s, r) ->
      (f_outer f = Array /\ f_inner f <> PriorityQueue /\ s = [StrP5.cyclic; StrP5.size] /\ r = [StrP5.size]) \/
      (f_outer f = Array /\ f_inner f = PriorityQueue /\
       s = [StrP5.cyclic; StrP5.priorities; StrP5.size] /\ r = [StrP5.size]) \/
      (f_outer f <> Array /\ f_inner f <> PriorityQueue /\ s = [] /\ r = []) \/
      (f_outer f <> Array /\ f_inner f = PriorityQueue /\ s = [StrP5.priorities] /\ r = []).
  Proof.
    intros [[] []] s r H; cbn [schema_of f_outer f_inner] in H; try discriminate H; injection H as <- <-;
      cbn [f_outer f_inner];
      first [ left; repeat split; solve [reflexivity | discriminate]
            | right; left; repeat split; solve [reflexivity | discriminate]
            | right; right; left; repeat split; solve [reflexivity | discriminate]
            | right; right; right; repeat split; solve [reflexivity | discriminate] ].
  Qed.

  Lemma priorities_in_schema :
    forall f s r, schema_of f = Some (s, r) -> (In StrP5.priorities s <-> f_inner f = PriorityQueue).
  Proof.
    destruct param_names_distinct as [_ [Hsp Hcp]].
    intros f s r H. destruct (schema_cases f s r H) as [[_ [Hi [-> _]]] | [[_ [Hi [-> _]]] | [[_ [Hi [-> _]]] | [_ [Hi [-> _]]]]]];
      split; intros Hx;
      first [ exact Hi
            | exfalso; apply Hi; exact Hx
            | left; reflexivity
            | right; left; reflexivity
            | exfalso; destruct Hx as [Hx | [Hx | []]]; [exact (Hcp Hx) | exact (Hsp Hx)]
            | destruct Hx ].
  Qed.

  (* Headline 12 (I-10). Exactly the priority queue families take the
     optional priorities, which no family requires. Its rule accepts exactly
     the GInt values in [1, i64_max], that is every GInt of at least 1, and
     the config keeps the canonical value. Without it the call is valid, and
     the levels have no bound. Every other family refuses priorities as an
     unknown key. *)
  Theorem priorities_validated :
    (forall f s r, schema_of f = Some (s, r) -> (In StrP5.priorities s <-> f_inner f = PriorityQueue)) /\
    (forall f s r, schema_of f = Some (s, r) -> ~ In StrP5.priorities r) /\
    rule_of StrP5.priorities = Some RPriorities /\
    (forall v v', canonical RPriorities v = Some v' <->
       exists z, v = VInt z /\ (1 <= z <= i64_max)%Z /\ v' = VInt z) /\
    (forall f s v, schema_of f = Some (s, []) -> f_inner f = PriorityQueue ->
       validate_params f [] [(StrP5.priorities, v)] =
       match canonical RPriorities v with
       | Some v' => POk [(StrP5.priorities, v')]
       | None => PErr (EInvalid StrP5.priorities)
       end) /\
    (forall f s r v z, schema_of f = Some (s, r) -> f_inner f = PriorityQueue -> f_outer f = Array ->
       (1 <= z <= i64_max)%Z ->
       validate_params f [] [(StrP5.priorities, v); (StrP5.size, VInt z)] =
       match canonical RPriorities v with
       | Some v' => POk [(StrP5.priorities, v'); (StrP5.size, VInt z)]
       | None => PErr (EInvalid StrP5.priorities)
       end) /\
    (forall f s, schema_of f = Some (s, []) -> validate_params f [] [] = POk []) /\
    (forall f s r v, schema_of f = Some (s, r) -> f_inner f <> PriorityQueue ->
       validate_params f [] [(StrP5.priorities, v)] = PErr (EUnknown StrP5.priorities (suggest StrP5.priorities s))).
  Proof.
    split; [exact priorities_in_schema |].
    split.
    { destruct param_names_distinct as [Hsc [Hsp Hcp]].
      intros f s r H Hin. destruct (schema_cases f s r H) as [[_ [_ [_ ->]]] | [[_ [_ [_ ->]]] | [[_ [_ [_ ->]]] | [_ [_ [_ ->]]]]]];
        cbn [In] in Hin; [destruct Hin as [Hin | []]; exact (Hsp Hin) .. | destruct Hin | destruct Hin]. }
    split; [exact rule_of_priorities |].
    split; [apply canonical_int_rule; right; reflexivity |].
    split.
    { intros f s v H Hi. unfold validate_params. rewrite H.
      rewrite find_unknown_none.
      - cbn [find canonical_all]. rewrite rule_of_priorities.
        destruct (canonical RPriorities v); reflexivity.
      - intros kv [<- | []]. cbn [fst]. apply (priorities_in_schema f s [] H). exact Hi. }
    split.
    { intros f s r v z H Hi Ho Hz.
      destruct (schema_cases f s r H) as [[_ [Hi' _]] | [[_ [_ [-> ->]]] | [[Ho' _] | [Ho' _]]]];
        [exfalso; exact (Hi' Hi) | | exfalso; exact (Ho' Ho) | exfalso; exact (Ho' Ho)].
      unfold validate_params. rewrite H.
      rewrite find_unknown_none, find_required_none.
      - cbn [canonical_all]. rewrite rule_of_priorities.
        destruct (canonical RPriorities v) as [v' |]; [| reflexivity].
        cbn [canonical_all]. rewrite rule_of_size, (canonical_in_range RSize z (or_introl eq_refl) Hz).
        reflexivity.
      - intros k [<- | []]. cbn [map fst]. right. left. reflexivity.
      - intros kv [<- | [<- | []]]; cbn [fst]; [right; left | right; right; left]; reflexivity. }
    split.
    { intros f s H. unfold validate_params. rewrite H. reflexivity. }
    { intros f s r v H Hi. unfold validate_params. rewrite H.
      assert (Hm : mem StrP5.priorities s = false).
      { apply not_true_iff_false. rewrite mem_spec. intros Hin.
        exact (Hi (proj1 (priorities_in_schema f s r H) Hin)). }
      cbn [find fst]. rewrite Hm. reflexivity. }
  Qed.

  (* The schema of an Array family and of every other family with a schema. *)
  Lemma array_schema :
    forall f s r, f_outer f = Array -> schema_of f = Some (s, r) ->
      In StrP5.size s /\ In StrP5.cyclic s /\ r = [StrP5.size].
  Proof.
    intros f s r Ho H.
    destruct (schema_cases f s r H) as [[_ [_ [-> ->]]] | [[_ [_ [-> ->]]] | [[Ho' _] | [Ho' _]]]];
      try (exfalso; exact (Ho' Ho)).
    - split; [right; left; reflexivity | split; [left; reflexivity | reflexivity]].
    - split; [right; right; left; reflexivity | split; [left; reflexivity | reflexivity]].
  Qed.

  Lemma other_schema :
    forall f s r, f_outer f <> Array -> schema_of f = Some (s, r) ->
      ~ In StrP5.size s /\ ~ In StrP5.cyclic s /\ r = [].
  Proof.
    destruct param_names_distinct as [Hsc [Hsp Hcp]].
    intros f s r Ho H.
    destruct (schema_cases f s r H) as [[Ho' _] | [[Ho' _] | [[_ [_ [-> ->]]] | [_ [_ [-> ->]]]]]];
      try (exfalso; exact (Ho Ho')).
    - split; [intros Hx; cbn [In] in Hx; destruct Hx |].
      split; [intros Hx; cbn [In] in Hx; destruct Hx | reflexivity].
    - split; [intros Hx; cbn [In] in Hx; destruct Hx as [Hx | []]; exact (Hsp (eq_sym Hx)) |].
      split; [intros Hx; cbn [In] in Hx; destruct Hx as [Hx | []]; exact (Hcp (eq_sym Hx)) | reflexivity].
  Qed.

  Lemma canonical_cyclic_rule :
    forall v v', canonical RCyclic v = Some v' <-> exists b, v = VBool b /\ v' = VBool b.
  Proof.
    intros [z | b | n | v0] v'; cbn [canonical];
      try (split; [discriminate | intros [b' [Hb _]]; discriminate Hb]).
    split; [intros H; injection H as <-; exists b; split; reflexivity |].
    intros [b' [Hb ->]]. injection Hb as <-. reflexivity.
  Qed.

  (* Headline 13 (F7). Every Array family requires size and takes the
     optional cyclic, and every other family takes neither. size accepts
     exactly the GInt values in [1, i64_max], and cyclic exactly the GBool
     values. A call without size fails with MissingParam after the key
     check. A call with a valid size and no cyclic flag is valid, and the
     space is a fixed array. The values are checked in key order, so a bad
     cyclic flag fails before a bad size. *)
  Theorem array_size_validated :
    (forall f s r, f_outer f = Array -> schema_of f = Some (s, r) ->
       In StrP5.size s /\ In StrP5.cyclic s /\ r = [StrP5.size]) /\
    (forall f s r, f_outer f <> Array -> schema_of f = Some (s, r) ->
       ~ In StrP5.size s /\ ~ In StrP5.cyclic s /\ r = []) /\
    rule_of StrP5.size = Some RSize /\ rule_of StrP5.cyclic = Some RCyclic /\
    (forall v v', canonical RSize v = Some v' <-> exists z, v = VInt z /\ (1 <= z <= i64_max)%Z /\ v' = VInt z) /\
    (forall v v', canonical RCyclic v = Some v' <-> exists b, v = VBool b /\ v' = VBool b) /\
    (forall f s r named, f_outer f = Array -> schema_of f = Some (s, r) ->
       (forall kv, In kv named -> In (fst kv) s) -> ~ In StrP5.size (map fst named) ->
       validate_params f [] named = PErr (EMissing StrP5.size)) /\
    (forall f s r z, f_outer f = Array -> schema_of f = Some (s, r) -> (1 <= z <= i64_max)%Z ->
       validate_params f [] [(StrP5.size, VInt z)] = POk [(StrP5.size, VInt z)]) /\
    (forall f s r v w, f_outer f = Array -> schema_of f = Some (s, r) -> canonical RCyclic v = None ->
       validate_params f [] [(StrP5.cyclic, v); (StrP5.size, w)] = PErr (EInvalid StrP5.cyclic)).
  Proof.
    split; [exact array_schema |].
    split; [exact other_schema |].
    split; [exact rule_of_size |].
    split; [exact rule_of_cyclic |].
    split; [apply canonical_int_rule; left; reflexivity |].
    split; [exact canonical_cyclic_rule |].
    split.
    { intros f s r named Ho H Hkeys Hsize. unfold validate_params. rewrite H.
      rewrite (find_unknown_none s named Hkeys).
      destruct (array_schema f s r Ho H) as [_ [_ ->]].
      cbn [find].
      replace (mem StrP5.size (map fst named)) with false
        by (symmetry; apply not_true_iff_false; rewrite mem_spec; exact Hsize).
      reflexivity. }
    split.
    { intros f s r z Ho H Hz. destruct (array_schema f s r Ho H) as [Hs [_ ->]].
      unfold validate_params. rewrite H.
      rewrite find_unknown_none, find_required_none.
      - cbn [canonical_all]. rewrite rule_of_size, (canonical_in_range RSize z (or_introl eq_refl) Hz).
        reflexivity.
      - intros k [<- | []]. cbn [map fst]. left. reflexivity.
      - intros kv [<- | []]. exact Hs. }
    { intros f s r v w Ho H Hc. destruct (array_schema f s r Ho H) as [Hs [Hcs ->]].
      unfold validate_params. rewrite H.
      rewrite find_unknown_none, find_required_none.
      - cbn [canonical_all]. rewrite rule_of_cyclic, Hc. reflexivity.
      - intros k [<- | []]. cbn [map fst]. right. left. reflexivity.
      - intros kv [<- | [<- | []]]; cbn [fst]; [exact Hcs | exact Hs]. }
  Qed.
End Params.

(* ── Headline assumptions (P5) ───────────────────────────────────────── *)
Print Assumptions registered_families_complete.
Print Assumptions aliases_bind_family_capability.
Print Assumptions constructor_params_validated.
Print Assumptions priorities_validated.
Print Assumptions array_size_validated.
