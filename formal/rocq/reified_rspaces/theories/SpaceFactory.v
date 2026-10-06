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
   - Code: the P4 commit of this repository. The line numbers below are the
     line numbers of that commit.

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
                            constructor.rs:124-142 (family_of)
   factory_op               rholang/src/rust/interpreter/spaces/factory/urn.rs:
                            29-33, 37-39 (the factory channel and the
                            capability that getSpaceAgent replies)
   validate_qualifier, durability_of
                            rholang/src/rust/interpreter/spaces/factory/
                            constructor.rs:48-92, 95-100
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
                            1994-2109

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
