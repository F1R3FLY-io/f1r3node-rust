From Stdlib Require Import Bool List Arith Lia.

Import ListNotations.

(**
  Path-exact incremental closure: the exact certificate before the overlay.

  The incremental closure verifier of a sealed transaction
  ([verify_incremental_closure], rspace++/src/rspace/persistence/closure.rs)
  walks every logical occurrence of the target manifest.  An occurrence is a
  reference together with its expectation (the manifest, or a node or a
  content blob of one namespace) and its exact absolute logical path.  The
  verifier accepts an occurrence in one of two ways: it reads a staged object
  and checks the object's children at their extended paths, or it stops at an
  inherited-boundary certificate naming the exact tuple (namespace, boundary
  kind, path, reference) that the private rewrite copied from the verified
  base.  After the walk it refuses any staged object the walk did not reach as
  staged.

  Algorithm 3 of engineering journal J-037 consults the staging overlay first.
  The overlay is content-addressed, so a node a rewrite emits at one path can
  be byte-identical to an untouched base subtree the rewrite certified at
  another path.  The staged-first walk then descends into the certified
  subtree, whose descendants are neither staged nor certified at their own
  paths, and refuses a valid delta.  The corrected walk consults the exact
  certificate first.

  - [covered] is the semantic contract of exact-path provenance: every
    occurrence carries an exact certificate, or its object is staged and every
    child occurrence is covered.  [delta_reaches] is the delta closure: the
    references reached through uncertified staged objects.  A delta is valid
    when the manifest is covered and the delta closure reaches every staged
    object.
  - The certificate-first verifier accepts exactly the valid deltas, and the
    references an accepted walk reaches are exactly the delta closure.
  - The staged-first verifier is sound, but it refuses the valid delta of the
    coincidence fixture with the missing object the Rust verifier reported.
  - When no certificate names a staged reference, the two verifiers return
    identical outcomes, including the visit count, the reached references and
    the refusal.
  - Two negative controls: a certificate lookup that ignores the exact path
    accepts an uncovered occurrence, and a verifier without the final
    reachability check accepts a staged object outside the delta closure.
  - The correction changes one more outcome: a staged object whose every
    target occurrence is certified is outside the delta closure.  The
    staged-first verifier accepts it; the certificate-first verifier refuses
    it in its final check.

  Object authentication (declared length and digest), the completed-key memo,
  the active-digest cycle check, node-context and endpoint-policy validation,
  the payload kind of content and the resource limits are outside the model.
  A staged object is modelled by its parsed form; the canonical decoder makes
  child edges strictly increasing, so distinct occurrences have distinct keys
  and the memo never skips one; and fuel bounds the descent depth where the
  Rust walk relies on the finite, acyclic overlay.  Exact paths are byte
  lists: the Rust path arena gives two paths the same identity exactly when
  their bytes are equal.

  The certificates are an input.  How the private rewrite records them (the
  roots of namespaces it leaves unmutated, and the child and content
  references of the base nodes it loads that reappear at the same exact
  position in the nodes it builds) is not modelled, and neither is the claim
  that every rewrite output is a valid delta.  Both are
  executable premises: the generated transitions of property PX-05 derive the
  certificates independently from the base and target images and seal every
  transition.
*)

Module PathExactIncrementalClosure.

(** * Occurrences, objects and certificates *)

(** A reference names one canonical object by digest, with its declared
    length folded in.  A namespace selects one root of the manifest.  A
    logical path is the exact absolute byte path of an occurrence. *)
Definition reference := nat.
Definition semantic_namespace := nat.
Definition logical_path := list nat.

Inductive boundary_kind := NodeEntry | ContentAtEndpoint.

Inductive expectation :=
| ExpectManifest
| ExpectNode (space : semantic_namespace)
| ExpectBlob (space : semantic_namespace).

(** A parsed canonical object.  A node carries its prefix, an optional
    content reference at its endpoint and its children as (edge, reference)
    pairs; a multiplicity endpoint holds no reference. *)
Inductive canonical_object :=
| ManifestObject (roots : list (semantic_namespace * reference))
| NodeObject (prefix : list nat) (content : option reference)
    (children : list (nat * reference))
| BlobObject.

Record occurrence := Occurrence {
  occurrence_reference : reference;
  occurrence_expectation : expectation;
  occurrence_path : logical_path
}.

Record certificate := Certificate {
  certificate_namespace : semantic_namespace;
  certificate_kind : boundary_kind;
  certificate_path : logical_path;
  certificate_reference : reference
}.

(** The staging overlay maps references to staged objects. *)
Definition overlay := list (reference * canonical_object).

Fixpoint lookup (staged : overlay) (target : reference) : option canonical_object :=
  match staged with
  | [] => None
  | (key, object) :: rest => if Nat.eqb key target then Some object else lookup rest target
  end.

Definition staged_references (staged : overlay) : list reference := map fst staged.

Lemma lookup_some_in :
  forall staged key object, lookup staged key = Some object -> In (key, object) staged.
Proof.
  induction staged as [|[stored stored_object] rest IH]; intros key object found;
    [discriminate|].
  cbn in found. revert found.
  destruct (Nat.eqb stored key) eqn:same; intros found.
  - injection found as found. subst stored_object. apply Nat.eqb_eq in same. subst stored.
    left. reflexivity.
  - right. exact (IH key object found).
Qed.

(** ** Exact certificate equality *)

Fixpoint path_eqb (left right : logical_path) : bool :=
  match left, right with
  | [], [] => true
  | left_byte :: left_rest, right_byte :: right_rest =>
      Nat.eqb left_byte right_byte && path_eqb left_rest right_rest
  | _, _ => false
  end.

Lemma path_eqb_true : forall left right, path_eqb left right = true -> left = right.
Proof.
  induction left as [|left_byte left_rest IH]; intros [|right_byte right_rest] equal;
    cbn in equal; try discriminate; [reflexivity|].
  apply andb_prop in equal as [same_byte same_rest].
  apply Nat.eqb_eq in same_byte. rewrite same_byte, (IH right_rest same_rest). reflexivity.
Qed.

Definition boundary_kind_eqb (left right : boundary_kind) : bool :=
  match left, right with
  | NodeEntry, NodeEntry => true
  | ContentAtEndpoint, ContentAtEndpoint => true
  | _, _ => false
  end.

Lemma boundary_kind_eqb_true :
  forall left right, boundary_kind_eqb left right = true -> left = right.
Proof. intros [|] [|] equal; cbn in equal; try discriminate; reflexivity. Qed.

Definition certificate_eqb (left right : certificate) : bool :=
  Nat.eqb (certificate_namespace left) (certificate_namespace right) &&
  boundary_kind_eqb (certificate_kind left) (certificate_kind right) &&
  path_eqb (certificate_path left) (certificate_path right) &&
  Nat.eqb (certificate_reference left) (certificate_reference right).

Lemma certificate_eqb_true :
  forall left right, certificate_eqb left right = true -> left = right.
Proof.
  intros [left_space left_kind left_path left_reference]
    [right_space right_kind right_path right_reference] equal.
  unfold certificate_eqb in equal. cbn in equal.
  apply andb_prop in equal as [equal same_reference].
  apply andb_prop in equal as [equal same_path].
  apply andb_prop in equal as [same_space same_kind].
  apply Nat.eqb_eq in same_space. apply boundary_kind_eqb_true in same_kind.
  apply path_eqb_true in same_path. apply Nat.eqb_eq in same_reference.
  subst. reflexivity.
Qed.

(** The exact certificate an occurrence needs: a node occurrence needs a
    [NodeEntry] certificate and a content occurrence a [ContentAtEndpoint]
    certificate, at its namespace, path and reference.  The manifest has no
    boundary ([expectation_boundary] in the Rust verifier). *)
Definition occurrence_certificate (occ : occurrence) : option certificate :=
  match occurrence_expectation occ with
  | ExpectManifest => None
  | ExpectNode space =>
      Some (Certificate space NodeEntry (occurrence_path occ) (occurrence_reference occ))
  | ExpectBlob space =>
      Some (Certificate space ContentAtEndpoint (occurrence_path occ) (occurrence_reference occ))
  end.

Definition certified (certificates : list certificate) (occ : occurrence) : bool :=
  match occurrence_certificate occ with
  | None => false
  | Some exact => existsb (certificate_eqb exact) certificates
  end.

Lemma certified_names_the_reference :
  forall certificates occ,
    certified certificates occ = true ->
    exists exact, In exact certificates /\ certificate_reference exact = occurrence_reference occ.
Proof.
  intros certificates occ found. unfold certified in found. revert found.
  destruct (occurrence_certificate occ) as [exact|] eqn:boundary;
    intros found; [|discriminate found].
  apply existsb_exists in found as (candidate & inside & same).
  apply certificate_eqb_true in same. subst candidate.
  exists exact. split; [exact inside|].
  destruct occ as [reference expected path]; destruct expected as [|space|space];
    unfold occurrence_certificate in boundary; cbn in boundary; try discriminate boundary;
    injection boundary as boundary; subst exact; reflexivity.
Qed.

(** * Child occurrences

    A manifest schedules its roots in order at the empty path.  A node at entry
    path [p] with prefix [q] has node path [p ++ q]; its children are entered
    at [p ++ q ++ [edge]] and its content at [p ++ q].  The Rust walk pushes
    the content and then the children on a stack, so it visits the children in
    reverse order and the content last. *)
Inductive expansion :=
| Children (pending : list occurrence)
| WrongKind.

Definition child_occurrences (space : semantic_namespace) (node_path : logical_path)
    (children : list (nat * reference)) : list occurrence :=
  map (fun child => Occurrence (snd child) (ExpectNode space) (node_path ++ [fst child]))
    children.

Definition content_occurrence (space : semantic_namespace) (node_path : logical_path)
    (content : option reference) : list occurrence :=
  match content with
  | Some blob => [Occurrence blob (ExpectBlob space) node_path]
  | None => []
  end.

Definition expand (occ : occurrence) (object : canonical_object) : expansion :=
  match occurrence_expectation occ, object with
  | ExpectManifest, ManifestObject roots =>
      Children (map (fun root => Occurrence (snd root) (ExpectNode (fst root)) []) roots)
  | ExpectNode space, NodeObject prefix content children =>
      Children (rev (child_occurrences space (occurrence_path occ ++ prefix) children) ++
        content_occurrence space (occurrence_path occ ++ prefix) content)
  | ExpectBlob _, BlobObject => Children []
  | _, _ => WrongKind
  end.

Definition object_references (object : canonical_object) : list reference :=
  match object with
  | ManifestObject roots => map snd roots
  | NodeObject _ content children =>
      map snd children ++ match content with Some blob => [blob] | None => [] end
  | BlobObject => []
  end.

Lemma expanded_children_are_object_references :
  forall occ object pending child,
    expand occ object = Children pending ->
    In child pending ->
    In (occurrence_reference child) (object_references object).
Proof.
  intros [reference expected path] object pending child expanded inside.
  unfold expand in expanded.
  destruct expected as [|space|space]; destruct object as [roots|prefix content children|];
    cbn [occurrence_expectation occurrence_path] in expanded; try discriminate;
    injection expanded as same; subst pending.
  - apply in_map_iff in inside as (root & same & member). subst child. cbn.
    apply in_map. exact member.
  - apply in_app_or in inside as [front|back].
    + apply in_rev in front. unfold child_occurrences in front.
      apply in_map_iff in front as (edge_child & same & member). subst child. cbn.
      apply in_or_app. left. apply in_map. exact member.
    + destruct content as [blob|]; cbn in back; [|contradiction].
      destruct back as [same|[]]. subst child. cbn.
      apply in_or_app. right. left. reflexivity.
  - cbn in inside. contradiction.
Qed.

(** * The walk

    A classifier decides one occurrence: descend into a staged object, stop at
    an inherited boundary, or refuse.  The staged-first classifier is J-037's
    Algorithm 3; the certificate-first classifier is the corrected walk. *)
Inductive decision :=
| Descend (object : canonical_object)
| StopInherited
| Refuse.

Definition staged_first (staged : overlay) (certificates : list certificate)
    (occ : occurrence) : decision :=
  match lookup staged (occurrence_reference occ) with
  | Some object => Descend object
  | None => if certified certificates occ then StopInherited else Refuse
  end.

Definition certificate_first (staged : overlay) (certificates : list certificate)
    (occ : occurrence) : decision :=
  if certified certificates occ then StopInherited
  else
    match lookup staged (occurrence_reference occ) with
    | Some object => Descend object
    | None => Refuse
    end.

Inductive refusal :=
| MissingObject (missing : reference)
| UnexpectedKind (unexpected : reference)
| StagedOutsideClosure
| WorkExhausted.

(** An accepted walk reports its logical visits and the references it reached
    as staged, in visit order (the Rust [declared_lengths] keys). *)
Inductive outcome :=
| Accepted (visits : nat) (reached : list reference)
| Refused (reason : refusal).

Fixpoint walk_pending (step : occurrence -> outcome) (pending : list occurrence) : outcome :=
  match pending with
  | [] => Accepted 0 []
  | next :: rest =>
      match step next with
      | Refused reason => Refused reason
      | Accepted visits reached =>
          match walk_pending step rest with
          | Refused reason => Refused reason
          | Accepted rest_visits rest_reached =>
              Accepted (visits + rest_visits) (reached ++ rest_reached)
          end
      end
  end.

Fixpoint walk (classify : occurrence -> decision) (fuel : nat) (occ : occurrence) : outcome :=
  match fuel with
  | 0 => Refused WorkExhausted
  | S fuel' =>
      match classify occ with
      | Refuse => Refused (MissingObject (occurrence_reference occ))
      | StopInherited => Accepted 1 []
      | Descend object =>
          match expand occ object with
          | WrongKind => Refused (UnexpectedKind (occurrence_reference occ))
          | Children pending =>
              match walk_pending (walk classify fuel') pending with
              | Refused reason => Refused reason
              | Accepted visits reached =>
                  Accepted (S visits) (occurrence_reference occ :: reached)
              end
          end
      end
  end.

Lemma walk_unfold :
  forall classify fuel occ,
    walk classify (S fuel) occ =
      match classify occ with
      | Refuse => Refused (MissingObject (occurrence_reference occ))
      | StopInherited => Accepted 1 []
      | Descend object =>
          match expand occ object with
          | WrongKind => Refused (UnexpectedKind (occurrence_reference occ))
          | Children pending =>
              match walk_pending (walk classify fuel) pending with
              | Refused reason => Refused reason
              | Accepted visits reached =>
                  Accepted (S visits) (occurrence_reference occ :: reached)
              end
          end
      end.
Proof. reflexivity. Qed.

Definition manifest_occurrence (manifest : reference) : occurrence :=
  Occurrence manifest ExpectManifest [].

Definition reached_every_staged (staged : overlay) (reached : list reference) : bool :=
  forallb (fun key => existsb (Nat.eqb key) reached) (staged_references staged).

(** The verifier walks the manifest and then refuses any staged object it did
    not reach as staged. *)
Definition verify (classify : occurrence -> decision) (staged : overlay) (fuel : nat)
    (manifest : reference) : outcome :=
  match walk classify fuel (manifest_occurrence manifest) with
  | Refused reason => Refused reason
  | Accepted visits reached =>
      if reached_every_staged staged reached
      then Accepted visits reached
      else Refused StagedOutsideClosure
  end.

Lemma verify_accepted_walk :
  forall classify staged fuel manifest visits reached,
    verify classify staged fuel manifest = Accepted visits reached ->
    walk classify fuel (manifest_occurrence manifest) = Accepted visits reached /\
    reached_every_staged staged reached = true.
Proof.
  intros classify staged fuel manifest visits reached. unfold verify.
  destruct (walk classify fuel (manifest_occurrence manifest))
    as [walk_visits walk_reached|reason];
    [|intros refused; discriminate refused].
  destruct (reached_every_staged staged walk_reached) eqn:all_reached;
    [|intros refused; discriminate refused].
  intros accepted. injection accepted as same_visits same_reached. subst.
  split; [reflexivity|exact all_reached].
Qed.

(** * Exact-path provenance and the delta closure *)

Inductive covered (staged : overlay) (certificates : list certificate) : occurrence -> Prop :=
| CoveredByCertificate :
    forall occ,
      certified certificates occ = true ->
      covered staged certificates occ
| CoveredByStaging :
    forall occ object pending,
      lookup staged (occurrence_reference occ) = Some object ->
      expand occ object = Children pending ->
      (forall child, In child pending -> covered staged certificates child) ->
      covered staged certificates occ.

Inductive delta_reaches (staged : overlay) (certificates : list certificate) :
    occurrence -> reference -> Prop :=
| DeltaReachesHere :
    forall occ object pending,
      certified certificates occ = false ->
      lookup staged (occurrence_reference occ) = Some object ->
      expand occ object = Children pending ->
      delta_reaches staged certificates occ (occurrence_reference occ)
| DeltaReachesBelow :
    forall occ object pending child target,
      certified certificates occ = false ->
      lookup staged (occurrence_reference occ) = Some object ->
      expand occ object = Children pending ->
      In child pending ->
      delta_reaches staged certificates child target ->
      delta_reaches staged certificates occ target.

Definition valid_delta (staged : overlay) (certificates : list certificate)
    (manifest : reference) : Prop :=
  covered staged certificates (manifest_occurrence manifest) /\
  forall key, In key (staged_references staged) ->
    delta_reaches staged certificates (manifest_occurrence manifest) key.

Lemma covered_staged_children :
  forall staged certificates occ,
    covered staged certificates occ ->
    certified certificates occ = false ->
    forall object pending,
      lookup staged (occurrence_reference occ) = Some object ->
      expand occ object = Children pending ->
      forall child, In child pending -> covered staged certificates child.
Proof.
  intros staged certificates occ coverage.
  destruct coverage as [covered_occ certified_occ
    |covered_occ other other_pending other_staged other_expanded children_covered];
    intros uncertified object pending staged_object expanded child inside.
  - rewrite uncertified in certified_occ. discriminate.
  - rewrite staged_object in other_staged. injection other_staged as same. subst other.
    rewrite expanded in other_expanded. injection other_expanded as same.
    subst other_pending. exact (children_covered child inside).
Qed.

Lemma covered_requires_certificate_or_staging :
  forall staged certificates occ,
    covered staged certificates occ ->
    certified certificates occ = false ->
    lookup staged (occurrence_reference occ) = None ->
    False.
Proof.
  intros staged certificates occ coverage.
  destruct coverage as [covered_occ certified_occ
    |covered_occ object pending staged_object _ _];
    intros uncertified unstaged.
  - rewrite uncertified in certified_occ. discriminate.
  - rewrite unstaged in staged_object. discriminate.
Qed.

Lemma delta_reaches_uncertified :
  forall staged certificates occ key,
    delta_reaches staged certificates occ key -> certified certificates occ = false.
Proof.
  intros staged certificates occ key reaching.
  destruct reaching as [here_occ here_object here_pending uncertified _ _
    |below_occ below_object below_pending child below_key uncertified _ _ _ _];
    exact uncertified.
Qed.

Lemma delta_reaches_cases :
  forall staged certificates occ key,
    delta_reaches staged certificates occ key ->
    key = occurrence_reference occ \/
    exists object pending child,
      lookup staged (occurrence_reference occ) = Some object /\
      expand occ object = Children pending /\
      In child pending /\
      delta_reaches staged certificates child key.
Proof.
  intros staged certificates occ key reaching.
  destruct reaching as [here_occ here_object here_pending _ _ _
    |below_occ below_object below_pending child below_key _ found expanded inside below].
  - left. reflexivity.
  - right. exists below_object, below_pending, child. repeat split; assumption.
Qed.

Lemma delta_reached_reference_is_staged_or_referenced :
  forall staged certificates occ target,
    delta_reaches staged certificates occ target ->
    target = occurrence_reference occ \/
    exists key object, lookup staged key = Some object /\ In target (object_references object).
Proof.
  intros staged certificates occ target reaching.
  induction reaching as [here_occ here_object here_pending _ _ _
    |below_occ below_object below_pending child below_target _ found expanded inside _ IH].
  - left. reflexivity.
  - right. destruct IH as [same|deeper]; [|exact deeper].
    exists (occurrence_reference below_occ), below_object. split; [exact found|].
    rewrite same. exact (expanded_children_are_object_references _ _ _ _ expanded inside).
Qed.

(** * Walk lemmas *)

Lemma walk_pending_accepted :
  forall step pending visits reached,
    walk_pending step pending = Accepted visits reached ->
    (forall next, In next pending ->
       exists next_visits next_reached,
         step next = Accepted next_visits next_reached /\ incl next_reached reached) /\
    (forall key, In key reached ->
       exists next next_visits next_reached,
         In next pending /\ step next = Accepted next_visits next_reached /\
         In key next_reached).
Proof.
  intros step pending. induction pending as [|next rest IH]; intros visits reached walked.
  - cbn in walked. injection walked as same_visits same_reached. subst reached.
    split; [intros member []|intros key []].
  - cbn [walk_pending] in walked. revert walked.
    destruct (step next) as [next_visits next_reached|reason] eqn:stepped;
      [|intros refused; discriminate refused].
    destruct (walk_pending step rest) as [rest_visits rest_reached|reason] eqn:walked_rest;
      [|intros refused; discriminate refused].
    intros walked. injection walked as same_visits same_reached. subst reached.
    destruct (IH rest_visits rest_reached eq_refl) as [rest_members rest_from].
    split.
    + intros member [same|later].
      * subst member. exists next_visits, next_reached. split; [exact stepped|].
        intros key inside. apply in_or_app. left. exact inside.
      * destruct (rest_members member later)
          as (member_visits & member_reached & member_step & member_incl).
        exists member_visits, member_reached. split; [exact member_step|].
        intros key inside. apply in_or_app. right. exact (member_incl key inside).
    + intros key inside. apply in_app_or in inside as [front|back].
      * exists next, next_visits, next_reached.
        split; [left; reflexivity|]. split; [exact stepped|exact front].
      * destruct (rest_from key back)
          as (member & member_visits & member_reached & member_in & member_step & member_key).
        exists member, member_visits, member_reached.
        split; [right; exact member_in|]. split; [exact member_step|exact member_key].
Qed.

Lemma walk_pending_complete :
  forall step pending,
    (forall next, In next pending -> exists visits reached, step next = Accepted visits reached) ->
    exists visits reached, walk_pending step pending = Accepted visits reached.
Proof.
  intros step pending. induction pending as [|next rest IH]; intros accepted.
  - exists 0, []. reflexivity.
  - destruct (accepted next (or_introl eq_refl)) as (visits & reached & stepped).
    destruct IH as (rest_visits & rest_reached & walked).
    { intros member inside. exact (accepted member (or_intror inside)). }
    exists (visits + rest_visits), (reached ++ rest_reached).
    cbn [walk_pending]. rewrite stepped, walked. reflexivity.
Qed.

Lemma walk_pending_ext :
  forall left right pending,
    (forall next, In next pending -> left next = right next) ->
    walk_pending left pending = walk_pending right pending.
Proof.
  intros left right pending. induction pending as [|next rest IH]; intros same; [reflexivity|].
  cbn [walk_pending]. rewrite (same next (or_introl eq_refl)).
  rewrite IH; [reflexivity|]. intros member inside. exact (same member (or_intror inside)).
Qed.

Lemma walk_ext :
  forall left right,
    (forall occ, left occ = right occ) ->
    forall fuel occ, walk left fuel occ = walk right fuel occ.
Proof.
  intros left right same fuel. induction fuel as [|fuel IH]; intros occ; [reflexivity|].
  rewrite (walk_unfold left fuel occ), (walk_unfold right fuel occ), (same occ).
  destruct (right occ) as [object| |]; [|reflexivity|reflexivity].
  destruct (expand occ object) as [pending|]; [|reflexivity].
  rewrite (walk_pending_ext (walk left fuel) (walk right fuel) pending (fun next _ => IH next)).
  reflexivity.
Qed.

(** More fuel never changes a walk that did not run out of fuel. *)
Lemma walk_fuel_step :
  forall classify fuel occ,
    walk classify fuel occ <> Refused WorkExhausted ->
    walk classify (S fuel) occ = walk classify fuel occ.
Proof.
  intros classify fuel. induction fuel as [|fuel IH]; intros occ settled.
  - cbn in settled. contradiction.
  - assert (pending_step : forall pending,
        walk_pending (walk classify fuel) pending <> Refused WorkExhausted ->
        walk_pending (walk classify (S fuel)) pending =
          walk_pending (walk classify fuel) pending).
    { induction pending as [|next rest IH_rest]; intros pending_settled; [reflexivity|].
      revert pending_settled. cbn [walk_pending].
      destruct (walk classify fuel next) as [visits reached|reason] eqn:stepped;
        intros pending_settled.
      - rewrite (IH next) by (rewrite stepped; discriminate). rewrite stepped.
        destruct (walk_pending (walk classify fuel) rest)
          as [rest_visits rest_reached|rest_reason] eqn:walked.
        + rewrite IH_rest by discriminate. reflexivity.
        + rewrite IH_rest by exact pending_settled. reflexivity.
      - rewrite (IH next) by (rewrite stepped; exact pending_settled).
        rewrite stepped. reflexivity. }
    rewrite (walk_unfold classify fuel occ) in settled.
    rewrite (walk_unfold classify (S fuel) occ), (walk_unfold classify fuel occ).
    revert settled.
    destruct (classify occ) as [object| |]; [|intros; reflexivity|intros; reflexivity].
    destruct (expand occ object) as [pending|]; [|intros; reflexivity].
    destruct (walk_pending (walk classify fuel) pending)
      as [visits reached|reason] eqn:walked; intros settled.
    + rewrite pending_step by (rewrite walked; discriminate). rewrite walked. reflexivity.
    + rewrite pending_step by (rewrite walked; exact settled). rewrite walked. reflexivity.
Qed.

Lemma walk_fuel_monotone :
  forall classify fuel extra occ,
    walk classify fuel occ <> Refused WorkExhausted ->
    walk classify (extra + fuel) occ = walk classify fuel occ.
Proof.
  intros classify fuel extra occ settled. induction extra as [|extra IH]; [reflexivity|].
  cbn [Nat.add]. rewrite walk_fuel_step; [exact IH|]. rewrite IH. exact settled.
Qed.

Lemma common_fuel :
  forall classify pending,
    (forall next, In next pending ->
       exists fuel visits reached, walk classify fuel next = Accepted visits reached) ->
    exists fuel, forall next, In next pending ->
      exists visits reached, walk classify fuel next = Accepted visits reached.
Proof.
  intros classify pending. induction pending as [|next rest IH]; intros accepted.
  - exists 0. intros member [].
  - destruct (accepted next (or_introl eq_refl))
      as (next_fuel & next_visits & next_reached & next_walk).
    destruct IH as (rest_fuel & rest_walk).
    { intros member inside. exact (accepted member (or_intror inside)). }
    exists (next_fuel + rest_fuel). intros member [same|later].
    + subst member. exists next_visits, next_reached.
      rewrite Nat.add_comm, walk_fuel_monotone; [exact next_walk|].
      rewrite next_walk. discriminate.
    + destruct (rest_walk member later) as (visits & reached & walked).
      exists visits, reached. rewrite walk_fuel_monotone; [exact walked|].
      rewrite walked. discriminate.
Qed.

(** * Soundness of any classifier that stops only at exact certificates *)

Definition honest (staged : overlay) (certificates : list certificate)
    (classify : occurrence -> decision) : Prop :=
  forall occ,
    (classify occ = StopInherited -> certified certificates occ = true) /\
    (forall object, classify occ = Descend object ->
       lookup staged (occurrence_reference occ) = Some object).

Lemma staged_first_is_honest :
  forall staged certificates, honest staged certificates (staged_first staged certificates).
Proof.
  intros staged certificates occ. unfold staged_first.
  destruct (lookup staged (occurrence_reference occ)) as [object|] eqn:found.
  - split; [intros stopped; discriminate stopped|].
    intros other same. injection same as same. subst other. reflexivity.
  - destruct (certified certificates occ) eqn:certified_occ.
    + split; [intros _; reflexivity|]. intros other same. discriminate same.
    + split; [intros stopped; discriminate stopped|]. intros other same. discriminate same.
Qed.

Lemma certificate_first_is_honest :
  forall staged certificates, honest staged certificates (certificate_first staged certificates).
Proof.
  intros staged certificates occ. unfold certificate_first.
  destruct (certified certificates occ) eqn:certified_occ.
  - split; [intros _; reflexivity|]. intros other same. discriminate same.
  - destruct (lookup staged (occurrence_reference occ)) as [object|] eqn:found.
    + split; [intros stopped; discriminate stopped|].
      intros other same. injection same as same. subst other. reflexivity.
    + split; [intros stopped; discriminate stopped|]. intros other same. discriminate same.
Qed.

Lemma honest_walk_covers :
  forall staged certificates classify,
    honest staged certificates classify ->
    forall fuel occ visits reached,
      walk classify fuel occ = Accepted visits reached ->
      covered staged certificates occ.
Proof.
  intros staged certificates classify faithful fuel.
  induction fuel as [|fuel IH]; intros occ visits reached accepted;
    [discriminate accepted|].
  rewrite walk_unfold in accepted. revert accepted.
  destruct (classify occ) as [object| |] eqn:decided;
    [|intros _|intros refused; discriminate refused].
  - destruct (expand occ object) as [pending|] eqn:expanded;
      [|intros refused; discriminate refused].
    destruct (walk_pending (walk classify fuel) pending)
      as [child_visits child_reached|reason] eqn:walked;
      [|intros refused; discriminate refused].
    intros _.
    apply (CoveredByStaging staged certificates occ object pending).
    + exact (proj2 (faithful occ) object decided).
    + exact expanded.
    + intros child inside.
      destruct (proj1 (walk_pending_accepted _ _ _ _ walked) child inside)
        as (member_visits & member_reached & member_accepted & _).
      exact (IH child member_visits member_reached member_accepted).
  - apply CoveredByCertificate. exact (proj1 (faithful occ) decided).
Qed.

(** * The certificate-first verifier *)

Lemma certificate_first_stops :
  forall staged certificates occ,
    certified certificates occ = true ->
    certificate_first staged certificates occ = StopInherited.
Proof.
  intros staged certificates occ certified_occ. unfold certificate_first.
  rewrite certified_occ. reflexivity.
Qed.

Lemma certificate_first_descends :
  forall staged certificates occ object,
    certified certificates occ = false ->
    lookup staged (occurrence_reference occ) = Some object ->
    certificate_first staged certificates occ = Descend object.
Proof.
  intros staged certificates occ object uncertified found. unfold certificate_first.
  rewrite uncertified, found. reflexivity.
Qed.

Lemma certificate_first_refuses :
  forall staged certificates occ,
    certified certificates occ = false ->
    lookup staged (occurrence_reference occ) = None ->
    certificate_first staged certificates occ = Refuse.
Proof.
  intros staged certificates occ uncertified unstaged. unfold certificate_first.
  rewrite uncertified, unstaged. reflexivity.
Qed.

(** Capstone: the references an accepted certificate-first walk reaches are
    exactly its delta closure. *)
Theorem certificate_first_reached_is_delta_closure :
  forall staged certificates fuel occ visits reached,
    walk (certificate_first staged certificates) fuel occ = Accepted visits reached ->
    forall key, In key reached <-> delta_reaches staged certificates occ key.
Proof.
  intros staged certificates fuel.
  induction fuel as [|fuel IH]; intros occ visits reached accepted key;
    [discriminate accepted|].
  rewrite walk_unfold in accepted. revert accepted.
  destruct (certified certificates occ) eqn:certified_occ.
  - rewrite (certificate_first_stops staged certificates occ certified_occ).
    intros accepted. injection accepted as same_visits same_reached. subst reached.
    split; [intros []|].
    intros reaching. rewrite (delta_reaches_uncertified _ _ _ _ reaching) in certified_occ.
    discriminate certified_occ.
  - destruct (lookup staged (occurrence_reference occ)) as [object|] eqn:found.
    + rewrite (certificate_first_descends staged certificates occ object certified_occ found).
      cbv beta iota.
      destruct (expand occ object) as [pending|] eqn:expanded;
        [|intros refused; discriminate refused].
      destruct (walk_pending (walk (certificate_first staged certificates) fuel) pending)
        as [child_visits child_reached|reason] eqn:walked;
        [|intros refused; discriminate refused].
      intros accepted. injection accepted as same_visits same_reached. subst reached.
      destruct (walk_pending_accepted _ _ _ _ walked) as [members reached_from].
      split.
      * intros [here|deeper].
        -- subst key.
           exact (DeltaReachesHere staged certificates occ object pending
             certified_occ found expanded).
        -- destruct (reached_from key deeper)
             as (child & member_visits & member_reached & inside & member_accepted & member_key).
           apply (DeltaReachesBelow staged certificates occ object pending child key
             certified_occ found expanded inside).
           exact (proj1 (IH child member_visits member_reached member_accepted key) member_key).
      * intros reaching.
        destruct (delta_reaches_cases _ _ _ _ reaching)
          as [here|(other & other_pending & child & other_found & other_expanded & inside & below)].
        -- left. symmetry. exact here.
        -- rewrite found in other_found. injection other_found as same. subst other.
           rewrite expanded in other_expanded. injection other_expanded as same.
           subst other_pending. right.
           destruct (members child inside)
             as (member_visits & member_reached & member_accepted & member_incl).
           apply member_incl.
           exact (proj2 (IH child member_visits member_reached member_accepted key) below).
    + rewrite (certificate_first_refuses staged certificates occ certified_occ found).
      intros refused. discriminate refused.
Qed.

Lemma certificate_first_walk_complete :
  forall staged certificates occ,
    covered staged certificates occ ->
    exists fuel visits reached,
      walk (certificate_first staged certificates) fuel occ = Accepted visits reached.
Proof.
  intros staged certificates occ coverage.
  induction coverage as [occ certified_occ|occ object pending found expanded _ IH].
  - exists 1, 1, []. rewrite walk_unfold, (certificate_first_stops _ _ _ certified_occ).
    reflexivity.
  - destruct (certified certificates occ) eqn:certified_occ.
    + exists 1, 1, []. rewrite walk_unfold, (certificate_first_stops _ _ _ certified_occ).
      reflexivity.
    + destruct (common_fuel _ _ IH) as (fuel & accepted_children).
      destruct (walk_pending_complete _ _ accepted_children) as (visits & reached & walked).
      exists (S fuel), (S visits), (occurrence_reference occ :: reached).
      rewrite walk_unfold, (certificate_first_descends _ _ _ _ certified_occ found).
      cbv beta iota. rewrite expanded. cbv beta iota. rewrite walked. reflexivity.
Qed.

(** Capstone: an accepted certificate-first verification is a valid delta.
    Every occurrence of the target is covered by an exact certificate or by
    staging, and the delta closure reaches every staged object. *)
Theorem certificate_first_verifier_sound :
  forall staged certificates fuel manifest visits reached,
    verify (certificate_first staged certificates) staged fuel manifest =
      Accepted visits reached ->
    valid_delta staged certificates manifest.
Proof.
  intros staged certificates fuel manifest visits reached accepted.
  destruct (verify_accepted_walk _ _ _ _ _ _ accepted) as [walked all_reached].
  split.
  - exact (honest_walk_covers _ _ _ (certificate_first_is_honest staged certificates)
      _ _ _ _ walked).
  - intros key staged_key.
    apply (proj1 (certificate_first_reached_is_delta_closure _ _ _ _ _ _ walked key)).
    unfold reached_every_staged in all_reached. rewrite forallb_forall in all_reached.
    pose proof (all_reached key staged_key) as found.
    apply existsb_exists in found as (other & inside & same).
    apply Nat.eqb_eq in same. subst other. exact inside.
Qed.

(** Capstone: the certificate-first verifier accepts every valid delta once
    its fuel reaches the depth of the delta. *)
Theorem certificate_first_verifier_complete :
  forall staged certificates manifest,
    valid_delta staged certificates manifest ->
    exists bound, forall fuel, bound <= fuel ->
      exists visits reached,
        verify (certificate_first staged certificates) staged fuel manifest =
          Accepted visits reached.
Proof.
  intros staged certificates manifest [coverage closure].
  destruct (certificate_first_walk_complete _ _ _ coverage)
    as (bound & visits & reached & walked).
  exists bound. intros fuel enough. exists visits, reached.
  assert (all_reached : reached_every_staged staged reached = true).
  { unfold reached_every_staged. apply forallb_forall. intros key staged_key.
    apply existsb_exists. exists key. split; [|apply Nat.eqb_refl].
    apply (proj2 (certificate_first_reached_is_delta_closure _ _ _ _ _ _ walked key)).
    exact (closure key staged_key). }
  unfold verify. replace fuel with ((fuel - bound) + bound) by lia.
  rewrite walk_fuel_monotone by (rewrite walked; discriminate).
  rewrite walked. cbv beta iota. rewrite all_reached. reflexivity.
Qed.

(** Capstone: the staged-first verifier of J-037 is sound: it stops only at
    exact certificates and descends only into staged objects. *)
Theorem staged_first_verifier_sound :
  forall staged certificates fuel manifest visits reached,
    verify (staged_first staged certificates) staged fuel manifest = Accepted visits reached ->
    covered staged certificates (manifest_occurrence manifest).
Proof.
  intros staged certificates fuel manifest visits reached accepted.
  destruct (verify_accepted_walk _ _ _ _ _ _ accepted) as [walked _].
  exact (honest_walk_covers _ _ _ (staged_first_is_honest staged certificates) _ _ _ _ walked).
Qed.

(** Capstone: when no certificate names a staged reference, the two
    verifiers return identical outcomes, including the visit count, the
    reached references and the refusal. *)
Theorem verifiers_agree_without_coincidence :
  forall staged certificates,
    (forall exact, In exact certificates -> lookup staged (certificate_reference exact) = None) ->
    forall fuel manifest,
      verify (staged_first staged certificates) staged fuel manifest =
        verify (certificate_first staged certificates) staged fuel manifest.
Proof.
  intros staged certificates disjoint fuel manifest. unfold verify.
  rewrite (walk_ext (staged_first staged certificates) (certificate_first staged certificates)).
  - reflexivity.
  - intros occ. unfold staged_first, certificate_first.
    destruct (certified certificates occ) eqn:certified_occ.
    + destruct (certified_names_the_reference _ _ certified_occ) as (exact & inside & same).
      rewrite <- same, (disjoint exact inside). reflexivity.
    + destruct (lookup staged (occurrence_reference occ)); reflexivity.
Qed.

(** * The coincidence counterexample

    A base trie of one namespace holds the content [6] under two keys: the leaf
    [3] at entry [[1]] and the node [C] at entry [[2]] with prefix [[3]].  An
    insertion of the content [7] at [[2; 4]] splits [C].  The split node [4]
    has the edge [3] to [C'] and the edge [4] to the new leaf [5].  [C'] is the
    leaf with content [6], byte-identical to the untouched leaf [3], so the
    overlay stages it under the same reference.  The rewrite certifies the
    untouched leaf at [[1]] and [C]'s untouched content at [[2; 3]]. *)
Definition coincidence_overlay : overlay :=
  [(1, ManifestObject [(0, 2)]);
   (2, NodeObject [] None [(1, 3); (2, 4)]);
   (4, NodeObject [] None [(3, 3); (4, 5)]);
   (3, NodeObject [] (Some 6) []);
   (5, NodeObject [] (Some 7) []);
   (7, BlobObject)].

Definition coincidence_certificates : list certificate :=
  [Certificate 0 NodeEntry [1] 3; Certificate 0 ContentAtEndpoint [2; 3] 6].

(** Capstone: the staged-first verifier refuses a valid delta.  At [[1]] it
    finds the leaf [3] staged, descends into the certified subtree and misses
    the content [6], which is certified only at [[2; 3]].  No fuel makes it
    accept.  The certificate-first verifier stops at the exact certificate,
    reaches [3] at [[2; 3]] and accepts. *)
Theorem staged_first_refuses_a_valid_delta_at_a_coincident_certificate :
  valid_delta coincidence_overlay coincidence_certificates 1 /\
  verify (certificate_first coincidence_overlay coincidence_certificates)
    coincidence_overlay 5 1 = Accepted 8 [1; 2; 4; 5; 7; 3] /\
  verify (staged_first coincidence_overlay coincidence_certificates)
    coincidence_overlay 5 1 = Refused (MissingObject 6) /\
  (forall fuel visits reached,
     verify (staged_first coincidence_overlay coincidence_certificates)
       coincidence_overlay fuel 1 <> Accepted visits reached).
Proof.
  assert (corrected : verify (certificate_first coincidence_overlay coincidence_certificates)
    coincidence_overlay 5 1 = Accepted 8 [1; 2; 4; 5; 7; 3]) by (vm_compute; reflexivity).
  assert (refused : walk (staged_first coincidence_overlay coincidence_certificates) 5
    (manifest_occurrence 1) = Refused (MissingObject 6)) by (vm_compute; reflexivity).
  split; [exact (certificate_first_verifier_sound _ _ _ _ _ _ corrected)|].
  split; [exact corrected|].
  split; [unfold verify; rewrite refused; reflexivity|].
  intros fuel visits reached. destruct (le_lt_dec 5 fuel) as [enough|short].
  - unfold verify. replace fuel with ((fuel - 5) + 5) by lia.
    rewrite walk_fuel_monotone by (rewrite refused; discriminate).
    rewrite refused. discriminate.
  - destruct fuel as [|[|[|[|[|fuel]]]]]; [..|lia];
      intros accepted; vm_compute in accepted; discriminate accepted.
Qed.

(** * Negative controls *)

(** A lookup that matches a certificate by namespace, kind and reference but
    ignores the exact path: the digest-only shortcut J-037 rules out. *)
Definition same_reference_certificate (occ : occurrence) (candidate : certificate) : bool :=
  match occurrence_certificate occ with
  | None => false
  | Some exact =>
      Nat.eqb (certificate_namespace exact) (certificate_namespace candidate) &&
      boundary_kind_eqb (certificate_kind exact) (certificate_kind candidate) &&
      Nat.eqb (certificate_reference exact) (certificate_reference candidate)
  end.

Definition digest_only_certificate_mutant (staged : overlay) (certificates : list certificate)
    (occ : occurrence) : decision :=
  if existsb (same_reference_certificate occ) certificates then StopInherited
  else
    match lookup staged (occurrence_reference occ) with
    | Some object => Descend object
    | None => Refuse
    end.

(** One staged node [3] occurs at [[1]] and at [[2]]; its child leaf [4] is
    certified only below the first occurrence. *)
Definition shared_overlay : overlay :=
  [(1, ManifestObject [(0, 2)]);
   (2, NodeObject [] None [(1, 3); (2, 3)]);
   (3, NodeObject [] None [(5, 4)])].

Definition shared_certificates : list certificate := [Certificate 0 NodeEntry [1; 5] 4].

(** Capstone: the path-insensitive mutant accepts a target whose leaf at
    [[2; 5]] is neither staged nor certified at that path; the exact verifier
    refuses it. *)
Theorem digest_only_certificate_mutant_admits_an_uncovered_occurrence :
  verify (digest_only_certificate_mutant shared_overlay shared_certificates)
    shared_overlay 4 1 = Accepted 6 [1; 2; 3; 3] /\
  verify (certificate_first shared_overlay shared_certificates) shared_overlay 4 1 =
    Refused (MissingObject 4) /\
  ~ covered shared_overlay shared_certificates (manifest_occurrence 1).
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intros coverage.
  assert (root : covered shared_overlay shared_certificates (Occurrence 2 (ExpectNode 0) [])).
  { apply (covered_staged_children _ _ _ coverage eq_refl (ManifestObject [(0, 2)])
      [Occurrence 2 (ExpectNode 0) []] eq_refl eq_refl).
    left. reflexivity. }
  assert (second : covered shared_overlay shared_certificates (Occurrence 3 (ExpectNode 0) [2])).
  { apply (covered_staged_children _ _ _ root eq_refl (NodeObject [] None [(1, 3); (2, 3)])
      [Occurrence 3 (ExpectNode 0) [2]; Occurrence 3 (ExpectNode 0) [1]] eq_refl eq_refl).
    left. reflexivity. }
  assert (leaf : covered shared_overlay shared_certificates (Occurrence 4 (ExpectNode 0) [2; 5])).
  { apply (covered_staged_children _ _ _ second eq_refl (NodeObject [] None [(5, 4)])
      [Occurrence 4 (ExpectNode 0) [2; 5]] eq_refl eq_refl).
    left. reflexivity. }
  exact (covered_requires_certificate_or_staging _ _ _ leaf eq_refl eq_refl).
Qed.

(** The verifier without its final reachability check. *)
Definition verify_without_reachability (classify : occurrence -> decision) (fuel : nat)
    (manifest : reference) : outcome :=
  walk classify fuel (manifest_occurrence manifest).

(** The blob [8] is staged but no target occurrence references it. *)
Definition orphan_overlay : overlay :=
  [(1, ManifestObject [(0, 2)]); (2, NodeObject [9] None []); (8, BlobObject)].

(** Capstone: without the final check the walk accepts a delta that stages
    an object outside its delta closure; the verifier refuses it. *)
Theorem skipped_reachability_mutant_admits_an_unreached_staged_object :
  verify_without_reachability (certificate_first orphan_overlay []) 3 1 = Accepted 2 [1; 2] /\
  verify (certificate_first orphan_overlay []) orphan_overlay 3 1 = Refused StagedOutsideClosure /\
  ~ valid_delta orphan_overlay [] 1.
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intros [_ closure].
  assert (staged_orphan : In 8 (staged_references orphan_overlay)).
  { cbn. right. right. left. reflexivity. }
  destruct (delta_reached_reference_is_staged_or_referenced _ _ _ _ (closure 8 staged_orphan))
    as [same|(key & object & found & inside)].
  - discriminate same.
  - apply lookup_some_in in found. cbn in found.
    destruct found as [same|[same|[same|[]]]];
      injection same as same_key same_object; subst object; cbn in inside.
    + destruct inside as [bad|[]]. discriminate bad.
    + contradiction.
    + contradiction.
Qed.

(** * The second behaviour change *)

(** A staged node [3] whose every target occurrence is inherited: it occurs at
    [[1]] and at [[2]], and both occurrences, with the content below each,
    carry exact certificates.  The base owns every occurrence, so the object is
    not part of the delta. *)
Definition inherited_only_overlay : overlay :=
  [(1, ManifestObject [(0, 2)]);
   (2, NodeObject [] None [(1, 3); (2, 3)]);
   (3, NodeObject [] (Some 6) [])].

Definition inherited_only_certificates : list certificate :=
  [Certificate 0 NodeEntry [1] 3; Certificate 0 NodeEntry [2] 3;
   Certificate 0 ContentAtEndpoint [1] 6; Certificate 0 ContentAtEndpoint [2] 6].

(** Capstone: the staged-first verifier descends into [3] at both inherited
    positions and accepts.  The certificate-first verifier stops at both, so
    its final check finds [3] outside the delta closure, and the delta is not
    valid: no uncertified occurrence reaches [3]. *)
Theorem staged_first_accepts_a_staged_object_owned_only_at_inherited_positions :
  verify (staged_first inherited_only_overlay inherited_only_certificates)
    inherited_only_overlay 4 1 = Accepted 6 [1; 2; 3; 3] /\
  verify (certificate_first inherited_only_overlay inherited_only_certificates)
    inherited_only_overlay 4 1 = Refused StagedOutsideClosure /\
  ~ valid_delta inherited_only_overlay inherited_only_certificates 1.
Proof.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intros [_ closure].
  assert (staged_node : In 3 (staged_references inherited_only_overlay)).
  { cbn. right. right. left. reflexivity. }
  specialize (closure 3 staged_node).
  apply delta_reaches_cases in closure
    as [same|(object & pending & child & found & expanded & inside & below)].
  - discriminate same.
  - cbn in found. injection found as same_object. subst object.
    cbn in expanded. injection expanded as same_pending. subst pending.
    destruct inside as [same_child|[]]. subst child.
    apply delta_reaches_cases in below
      as [same|(object & pending & child & found & expanded & inside & leaf)].
    + discriminate same.
    + cbn in found. injection found as same_object. subst object.
      cbn in expanded. injection expanded as same_pending. subst pending.
      destruct inside as [same_child|[same_child|[]]]; subst child;
        apply delta_reaches_uncertified in leaf; vm_compute in leaf; discriminate leaf.
Qed.

End PathExactIncrementalClosure.

Print Assumptions PathExactIncrementalClosure.certificate_first_reached_is_delta_closure.
Print Assumptions PathExactIncrementalClosure.certificate_first_verifier_sound.
Print Assumptions PathExactIncrementalClosure.certificate_first_verifier_complete.
Print Assumptions PathExactIncrementalClosure.staged_first_verifier_sound.
Print Assumptions PathExactIncrementalClosure.verifiers_agree_without_coincidence.
Print Assumptions PathExactIncrementalClosure.staged_first_refuses_a_valid_delta_at_a_coincident_certificate.
Print Assumptions PathExactIncrementalClosure.digest_only_certificate_mutant_admits_an_uncovered_occurrence.
Print Assumptions PathExactIncrementalClosure.skipped_reachability_mutant_admits_an_unreached_staged_object.
Print Assumptions PathExactIncrementalClosure.staged_first_accepts_a_staged_object_owned_only_at_inherited_positions.
