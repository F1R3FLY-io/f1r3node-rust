(* C7a (epic 8946, B1 Phase C; decision record DR-85): native causal paths
   carry a chained digest, and the recording occurrence index compares the
   digest instead of the path.

   Each node of a causal path stores the digest of its path:
   - the root digest for the empty path;
   - child_path_digest(parent, segment) for the path parent . segment.
   The Rust digest is Blake2b-256 over a domain tag, a node-kind byte (0 for
   the root, 1 for a child), the parent digest and the two segment words.
   The recording occurrence index keys each occurrence by its session, the
   digest, the depth, the last segment and the stage, so a comparison reads
   a fixed number of bytes.

   The model takes the digest as two abstract functions. Their collision
   freedom is a premise of each theorem, not an axiom. The premises state
   that a child digest determines its parent digest and its segment, and
   that no child digest equals the root digest. The Rust code keeps the root
   and child inputs apart with the node-kind byte, and Blake2b-256 collision
   resistance justifies the premise for the digests that an execution
   computes.

   Results (part 1, C7a):
   - chain_snoc: the digest of p ++ [s] is the child digest of the digest of
     p and s.
   - digest_chain_correct: two paths have equal chained digests exactly when
     they are equal.
   - occurrence_key_correct: two occurrence keys (session, digest, depth,
     last segment, stage) are equal exactly when their sessions, paths and
     stages are equal.
   - unchained_digest_collides: a negative control. A key that hashes only
     the last segment gives two different paths the same key.

   Rust correspondence: root_path_digest, child_path_digest and
   CausalPath::digest in rspace++/src/rspace/operation_context.rs;
   OccurrenceKey in rholang/src/rust/interpreter/accounting/native_runtime/
   index.rs and its use in recording.rs (reserve_record, capture_recording).
   The extracted tests are path_digest_folds_the_segments (rspace++),
   digest_keyed_occurrences_match_path_keyed_index and
   digest_keyed_lookup_charge_is_independent_of_depth (rholang). *)

From Stdlib Require Import Lists.List.
Import ListNotations.

Definition segment := (nat * nat)%type.
Definition path := list segment.

Section DigestChain.

  Variable digest : Type.
  Variable root : digest.
  Variable child : digest -> segment -> digest.

  Hypothesis child_injective : forall d e s t, child d s = child e t -> d = e /\ s = t.
  Hypothesis child_not_root : forall d s, child d s <> root.

  Definition chain (p : path) : digest := fold_left child p root.

  Lemma chain_snoc : forall p s, chain (p ++ [s]) = child (chain p) s.
  Proof.
    intros p s; unfold chain; rewrite fold_left_app; reflexivity.
  Qed.

  Theorem digest_chain_correct : forall p q, chain p = chain q <-> p = q.
  Proof.
    intros p q; split; [|intros ->; reflexivity].
    revert q.
    induction p as [|s p IH] using rev_ind; intros q Same;
      destruct q as [|t q _] using rev_ind.
    - reflexivity.
    - rewrite chain_snoc in Same; cbn in Same.
      exfalso; apply (child_not_root (chain q) t); symmetry; exact Same.
    - rewrite chain_snoc in Same; cbn in Same.
      exfalso; apply (child_not_root (chain p) s); exact Same.
    - rewrite !chain_snoc in Same.
      apply child_injective in Same as [Parents Segments].
      subst t; f_equal; apply IH; exact Parents.
  Qed.

  Definition last_segment (p : path) : option segment :=
    match rev p with
    | [] => None
    | s :: _ => Some s
    end.

  (* The occurrence key of the Rust recording index. The stage is a natural
     number tag here. *)
  Definition occurrence_key (session : nat) (p : path) (stage : nat) :=
    (session, chain p, length p, last_segment p, stage).

  Theorem occurrence_key_correct : forall session p stage session' q stage',
    occurrence_key session p stage = occurrence_key session' q stage' <->
    session = session' /\ p = q /\ stage = stage'.
  Proof.
    intros session p stage session' q stage'; unfold occurrence_key; split.
    - intros Same; injection Same as Sessions Digests _ _ Stages.
      split; [exact Sessions|].
      split; [apply digest_chain_correct; exact Digests | exact Stages].
    - intros (-> & -> & ->); reflexivity.
  Qed.

End DigestChain.

(* A key that hashes only the last segment is not injective: the identity
   on the last segment stands for any such hash. *)
Theorem unchained_digest_collides :
  let unchained (p : path) := last_segment p in
  unchained [(1, 0); (2, 0)] = unchained [(3, 0); (2, 0)] /\
  [(1, 0); (2, 0)] <> [(3, 0); (2, 0)].
Proof.
  cbn; split; [reflexivity | discriminate].
Qed.

(* Part 2 (C7b, DR-86): the decoder interns the paths of a recording and its
   journal into one hash-consed trie, and the evidence names each path by
   its node.

   A trie is a list of entries (parent, segment). Entry k is node S k, and
   node 0 is the root, the empty path. Each parent precedes its child
   (wf_trie), and no entry repeats (NoDup), so each (parent, segment) pair
   has one node. path_of t i is the path of node i.

   Results (part 2, C7b):
   - node_identity_is_path_equality: two nodes of a well-formed,
     hash-consed trie have equal paths exactly when they are the same node.
   - child_spec and intern_spec: interning a segment or a suffix keeps the
     trie well-formed and hash-consed, keeps the path of every old node, and
     returns the node of the extended path.
   - trie_decode_denotes_paths: decoding a delta chain into the trie accepts
     exactly the chains that the vector decoder accepts (decode_all of
     NativePathDeltaCodec), and each returned node denotes the decoded path.
   - trie_nodes_bounded_by_suffixes: the decode adds at most the total
     suffix length Σ s_i of the chain as nodes.
   - trie_dfs_is_lex_sort: a depth-first walk that visits the children of
     each node by increasing segment lists the paths of a tree in strictly
     increasing lexicographic order, so it lists each path once, and it
     lists every path of the tree.
   - materialized_segments_quadratic: a chain of n paths that each extend
     the previous path by one segment has Σ s_i = n new segments but
     n (n + 1) / 2 materialized segments. For n = 724 the materialized
     count is 262,450, above the journal limit of 262,144.

   Rust correspondence: NativePathTrie (child, intern, intern_delta,
   ancestor, preorder) in rholang/src/rust/interpreter/accounting/
   native_runtime/path_trie.rs; read_path_delta and read_path in wire.rs;
   check_sizes and check_link in checked_operations.rs; bind_trace in
   checked_operations/trace.rs; the replay digest index in
   checked_operations/trace/replay.rs. The extracted tests are
   trie_decode_round_trips_v1_v2 and delta_limit_checked_before_allocation
   (wire.rs), trie_order_equals_vec_sort and ids_equal_iff_paths_equal
   (tests/path_trie.rs), and replay_digest_lookup_equals_binary_search
   (tests/checked_trace.rs). *)

From Stdlib Require Import Arith.PeanoNat Arith.Wf_nat Lia Sorting.Sorted.
From CostAccountedRho Require Import NativePathDeltaCodec.

Definition seg_eq_dec : forall x y : segment, {x = y} + {x <> y}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition entry_eq_dec : forall x y : nat * segment, {x = y} + {x <> y}.
Proof. decide equality; [apply seg_eq_dec | apply Nat.eq_dec]. Defined.

Definition trie := list (nat * segment).

Definition wf_trie (t : trie) : Prop :=
  forall k p s, nth_error t k = Some (p, s) -> p <= k.

Fixpoint path_fuel (t : trie) (fuel i : nat) : path :=
  match fuel with
  | 0 => []
  | S fuel' =>
      match i with
      | 0 => []
      | S k =>
          match nth_error t k with
          | Some (p, s) => path_fuel t fuel' p ++ [s]
          | None => []
          end
      end
  end.

Definition path_of (t : trie) (i : nat) : path := path_fuel t i i.

Lemma path_fuel_stable : forall t, wf_trie t ->
  forall f1 f2 i, i <= f1 -> i <= f2 -> path_fuel t f1 i = path_fuel t f2 i.
Proof.
  intros t Wf f1.
  induction f1 as [|f1 IH]; intros f2 i Le1 Le2.
  - assert (i = 0) as -> by lia. destruct f2; reflexivity.
  - destruct i as [|k]; [destruct f2; reflexivity|].
    destruct f2 as [|f2]; [lia|].
    cbn [path_fuel].
    destruct (nth_error t k) as [[p s]|] eqn:Entry; [|reflexivity].
    pose proof (Wf k p s Entry).
    rewrite (IH f2 p); [reflexivity | lia | lia].
Qed.

Lemma path_of_child : forall t k p s, wf_trie t ->
  nth_error t k = Some (p, s) -> path_of t (S k) = path_of t p ++ [s].
Proof.
  intros t k p s Wf Entry.
  unfold path_of. cbn [path_fuel]. rewrite Entry.
  pose proof (Wf k p s Entry).
  rewrite (path_fuel_stable t Wf k p p); [reflexivity | lia | lia].
Qed.

Lemma path_of_root : forall t, path_of t 0 = [].
Proof. reflexivity. Qed.

Lemma path_fuel_extend : forall t u, wf_trie t ->
  forall f i, i <= length t -> path_fuel (t ++ u) f i = path_fuel t f i.
Proof.
  intros t u Wf f.
  induction f as [|f IH]; intros i Le; [reflexivity|].
  destruct i as [|k]; [reflexivity|].
  cbn [path_fuel].
  rewrite nth_error_app1 by lia.
  destruct (nth_error t k) as [[p s]|] eqn:Entry; [|reflexivity].
  pose proof (Wf k p s Entry).
  rewrite IH by lia. reflexivity.
Qed.

Lemma path_of_extend : forall t u i, wf_trie t -> i <= length t ->
  path_of (t ++ u) i = path_of t i.
Proof. intros. unfold path_of. apply path_fuel_extend; assumption. Qed.

Lemma path_of_length : forall t, wf_trie t ->
  forall i, i <= length t -> length (path_of t i) <= i.
Proof.
  intros t Wf i.
  induction i as [i IH] using lt_wf_ind; intros Le.
  destruct i as [|k]; [cbn; lia|].
  destruct (nth_error t k) as [[p s]|] eqn:Entry.
  - rewrite (path_of_child t k p s Wf Entry), length_app.
    pose proof (Wf k p s Entry).
    specialize (IH p ltac:(lia) ltac:(lia)). cbn. lia.
  - unfold path_of. cbn [path_fuel]. rewrite Entry. cbn. lia.
Qed.

Lemma valid_entry : forall (t : trie) k, k < length t -> exists p s, nth_error t k = Some (p, s).
Proof.
  intros t k Lt.
  destruct (nth_error t k) as [[p s]|] eqn:Entry.
  - exists p, s; reflexivity.
  - apply nth_error_None in Entry. lia.
Qed.

(* Node identity: equal paths name the same node. *)
Theorem node_identity_is_path_equality : forall t, wf_trie t -> NoDup t ->
  forall i j, i <= length t -> j <= length t ->
  path_of t i = path_of t j <-> i = j.
Proof.
  intros t Wf Unique i.
  induction i as [i IH] using lt_wf_ind; intros j Li Lj.
  split; [|intros ->; reflexivity].
  intros Same.
  destruct i as [|k], j as [|m]; [reflexivity| | |].
  - destruct (valid_entry t m ltac:(lia)) as (p & s & Entry).
    rewrite path_of_root, (path_of_child t m p s Wf Entry) in Same.
    destruct (path_of t p); discriminate.
  - destruct (valid_entry t k ltac:(lia)) as (p & s & Entry).
    rewrite path_of_root, (path_of_child t k p s Wf Entry) in Same.
    destruct (path_of t p); discriminate.
  - destruct (valid_entry t k ltac:(lia)) as (p & s & EntryK).
    destruct (valid_entry t m ltac:(lia)) as (q & r & EntryM).
    rewrite (path_of_child t k p s Wf EntryK), (path_of_child t m q r Wf EntryM) in Same.
    apply app_inj_tail in Same as [Parents Segments]. subst r.
    pose proof (Wf k p s EntryK). pose proof (Wf m q s EntryM).
    assert (p = q) as <-.
    { apply (IH p ltac:(lia) q ltac:(lia) ltac:(lia)). exact Parents. }
    f_equal.
    eapply NoDup_nth_error; [exact Unique | apply nth_error_Some; congruence |].
    congruence.
Qed.

Fixpoint index_of (t : trie) (e : nat * segment) : option nat :=
  match t with
  | [] => None
  | x :: rest => if entry_eq_dec x e then Some 0 else option_map S (index_of rest e)
  end.

Lemma index_of_some : forall t e k, index_of t e = Some k -> nth_error t k = Some e.
Proof.
  induction t as [|x rest IH]; intros e k Found; [discriminate|].
  cbn in Found. destruct (entry_eq_dec x e) as [->|Different].
  - injection Found as <-. reflexivity.
  - destruct (index_of rest e) as [m|] eqn:Rest; [|discriminate].
    injection Found as <-. cbn. apply IH. exact Rest.
Qed.

Lemma index_of_none : forall t e, index_of t e = None -> ~ In e t.
Proof.
  induction t as [|x rest IH]; intros e Missing Member; [contradiction|].
  cbn in Missing. destruct (entry_eq_dec x e) as [->|Different]; [discriminate|].
  destruct (index_of rest e) eqn:Rest; [discriminate|].
  destruct Member as [<-|Member]; [contradiction | exact (IH e Rest Member)].
Qed.

Definition child (t : trie) (p : nat) (s : segment) : trie * nat :=
  match index_of t (p, s) with
  | Some k => (t, S k)
  | None => (t ++ [(p, s)], S (length t))
  end.

Fixpoint intern (t : trie) (base : nat) (segments : path) : trie * nat :=
  match segments with
  | [] => (t, base)
  | s :: rest => let (t', node) := child t base s in intern t' node rest
  end.

Definition trie_ok (t : trie) : Prop := wf_trie t /\ NoDup t.

Lemma wf_snoc : forall t p s, wf_trie t -> p <= length t -> wf_trie (t ++ [(p, s)]).
Proof.
  intros t p s Wf Le k q r Entry.
  destruct (Nat.lt_ge_cases k (length t)) as [Lt|Ge].
  - rewrite nth_error_app1 in Entry by lia. exact (Wf k q r Entry).
  - rewrite nth_error_app2 in Entry by lia.
    destruct (k - length t) as [|n] eqn:Offset.
    + cbn in Entry. injection Entry as <- <-. lia.
    + destruct n; discriminate.
Qed.

Lemma nodup_snoc : forall (t : trie) e, NoDup t -> ~ In e t -> NoDup (t ++ [e]).
Proof.
  intros t e Unique Missing.
  apply NoDup_app; [exact Unique | constructor; [intros []|constructor] |].
  intros x Member [<-|[]]. contradiction.
Qed.

(* The child of a valid node: the extended path, the old nodes kept. *)
Lemma child_spec : forall t p s, trie_ok t -> p <= length t ->
  let (t', node) := child t p s in
  trie_ok t' /\ (exists u, t' = t ++ u) /\ length t' <= S (length t) /\
  node <= length t' /\ path_of t' node = path_of t p ++ [s].
Proof.
  intros t p s [Wf Unique] Le. unfold child.
  destruct (index_of t (p, s)) as [k|] eqn:Found.
  - pose proof (index_of_some t (p, s) k Found) as Entry.
    split; [split; assumption|].
    split; [exists []; rewrite app_nil_r; reflexivity|].
    split; [lia|].
    split; [assert (k < length t) by (apply nth_error_Some; congruence); lia|].
    exact (path_of_child t k p s Wf Entry).
  - pose proof (index_of_none t (p, s) Found) as Missing.
    split; [split; [apply wf_snoc; assumption | apply nodup_snoc; assumption]|].
    split; [exists [(p, s)]; reflexivity|].
    rewrite length_app. cbn [length].
    split; [lia|].
    split; [lia|].
    rewrite (path_of_child (t ++ [(p, s)]) (length t) p s).
    + rewrite path_of_extend by assumption. reflexivity.
    + apply wf_snoc; assumption.
    + rewrite nth_error_app2, Nat.sub_diag by lia. reflexivity.
Qed.

Lemma intern_spec : forall segments t base, trie_ok t -> base <= length t ->
  let (t', node) := intern t base segments in
  trie_ok t' /\ (exists u, t' = t ++ u) /\ length t' <= length t + length segments /\
  node <= length t' /\ path_of t' node = path_of t base ++ segments.
Proof.
  induction segments as [|s rest IH]; intros t base Ok Le.
  - cbn. split; [exact Ok|].
    split; [exists []; rewrite app_nil_r; reflexivity|].
    rewrite app_nil_r. repeat split; lia.
  - cbn [intern].
    pose proof (child_spec t base s Ok Le) as Child.
    destruct (child t base s) as [t1 node1].
    destruct Child as (Ok1 & (u1 & ->) & Len1 & Node1 & Path1).
    specialize (IH (t ++ u1) node1 Ok1 Node1).
    destruct (intern (t ++ u1) node1 rest) as [t2 node2].
    destruct IH as (Ok2 & (u2 & ->) & Len2 & Node2 & Path2).
    split; [exact Ok2|].
    split; [exists (u1 ++ u2); rewrite app_assoc; reflexivity|].
    split; [cbn; lia|].
    split; [exact Node2|].
    rewrite Path2, Path1, <- app_assoc. reflexivity.
Qed.

Definition parent (t : trie) (i : nat) : nat :=
  match i with
  | 0 => 0
  | S k =>
      match nth_error t k with
      | Some (p, _) => p
      | None => 0
      end
  end.

Definition ancestor (t : trie) (i n : nat) : nat := Nat.iter n (parent t) i.

Lemma parent_le : forall t i, wf_trie t -> parent t i <= i.
Proof.
  intros t [|k] Wf; cbn; [lia|].
  destruct (nth_error t k) as [[p s]|] eqn:Entry; [|lia].
  pose proof (Wf k p s Entry). lia.
Qed.

Lemma ancestor_le : forall t n i, wf_trie t -> ancestor t i n <= i.
Proof.
  intros t n i Wf. unfold ancestor.
  induction n as [|n IH]; [cbn; lia|].
  rewrite Nat.iter_succ.
  pose proof (parent_le t (Nat.iter n (parent t) i) Wf). lia.
Qed.

Lemma path_of_parent : forall t i, wf_trie t ->
  path_of t (parent t i) = removelast (path_of t i).
Proof.
  intros t [|k] Wf; [reflexivity|].
  cbn [parent].
  destruct (nth_error t k) as [[p s]|] eqn:Entry.
  - rewrite (path_of_child t k p s Wf Entry), removelast_last. reflexivity.
  - unfold path_of at 2. cbn [path_fuel]. rewrite Entry. reflexivity.
Qed.

Lemma removelast_firstn_any : forall (l : path) k, k <= length l ->
  removelast (firstn k l) = firstn (k - 1) l.
Proof.
  intros l [|k] Le; [reflexivity|].
  rewrite removelast_firstn by lia. f_equal. lia.
Qed.

Lemma path_of_ancestor : forall t n i, wf_trie t ->
  path_of t (ancestor t i n) = firstn (length (path_of t i) - n) (path_of t i).
Proof.
  intros t n i Wf. induction n as [|n IH].
  - cbn. rewrite Nat.sub_0_r, firstn_all. reflexivity.
  - unfold ancestor in *. rewrite Nat.iter_succ.
    rewrite path_of_parent, IH by assumption.
    rewrite removelast_firstn_any by lia. f_equal. lia.
Qed.

(* The trie decoder: a delta entry keeps the first [prefix] segments of the
   previous path and interns its suffix below that ancestor. It accepts an
   entry exactly when the vector decoder of NativePathDeltaCodec does. *)
Definition decode_trie_one (maximum : nat) (t : trie) (previous : nat)
    (entry : nat * path) : option (trie * nat) :=
  match decode_one seg_eq_dec maximum (path_of t previous) entry with
  | Some _ =>
      let (prefix, suffix) := entry in
      Some (intern t (ancestor t previous (length (path_of t previous) - prefix)) suffix)
  | None => None
  end.

Fixpoint decode_trie_all (maximum : nat) (t : trie) (previous : nat)
    (entries : list (nat * path)) : option (trie * list nat) :=
  match entries with
  | [] => Some (t, [])
  | entry :: rest =>
      match decode_trie_one maximum t previous entry with
      | Some (t', node) =>
          match decode_trie_all maximum t' node rest with
          | Some (t'', nodes) => Some (t'', node :: nodes)
          | None => None
          end
      | None => None
      end
  end.

Fixpoint suffix_total (entries : list (nat * path)) : nat :=
  match entries with
  | [] => 0
  | (_, suffix) :: rest => length suffix + suffix_total rest
  end.

Lemma decode_one_shape : forall maximum previous prefix suffix path,
  decode_one seg_eq_dec maximum previous (prefix, suffix) = Some path ->
  prefix <= length previous /\ path = firstn prefix previous ++ suffix.
Proof.
  intros maximum previous prefix suffix path Decoded.
  unfold decode_one in Decoded.
  destruct (Nat.leb prefix (Nat.min (length previous) maximum)) eqn:Bound; [|discriminate].
  destruct (Nat.leb (length suffix) (maximum - prefix)); [|discriminate].
  destruct (prefix_canonical seg_eq_dec previous prefix suffix); [|discriminate].
  cbn in Decoded. injection Decoded as <-.
  apply Nat.leb_le in Bound. split; [lia | reflexivity].
Qed.

Theorem trie_decode_denotes_paths : forall maximum entries t previous paths,
  trie_ok t -> previous <= length t ->
  decode_all seg_eq_dec maximum (path_of t previous) entries = Some paths ->
  exists t' nodes,
    decode_trie_all maximum t previous entries = Some (t', nodes) /\
    map (path_of t') nodes = paths /\
    trie_ok t' /\ (exists u, t' = t ++ u) /\
    length t' <= length t + suffix_total entries /\
    Forall (fun node => node <= length t') nodes.
Proof.
  intros maximum entries.
  induction entries as [|[prefix suffix] rest IH]; intros t previous paths Ok Le Decoded.
  - cbn in Decoded. injection Decoded as <-.
    exists t, []. cbn.
    split; [reflexivity|]. split; [reflexivity|]. split; [exact Ok|].
    split; [exists []; rewrite app_nil_r; reflexivity|].
    split; [lia | constructor].
  - cbn [decode_all] in Decoded.
    destruct (decode_one seg_eq_dec maximum (path_of t previous) (prefix, suffix))
      as [path|] eqn:One; [|discriminate].
    destruct (decode_all seg_eq_dec maximum path rest) as [rest_paths|] eqn:Rest;
      [|discriminate].
    injection Decoded as <-.
    destruct (decode_one_shape _ _ _ _ _ One) as [Prefix ->].
    destruct Ok as [Wf Unique].
    set (base := ancestor t previous (length (path_of t previous) - prefix)).
    assert (base <= length t) as BaseLe.
    { pose proof (ancestor_le t (length (path_of t previous) - prefix) previous Wf). lia. }
    assert (path_of t base = firstn prefix (path_of t previous)) as BasePath.
    { unfold base. rewrite path_of_ancestor by exact Wf. f_equal. lia. }
    pose proof (intern_spec suffix t base (conj Wf Unique) BaseLe) as Interned.
    destruct (intern t base suffix) as [t1 node] eqn:InternEq.
    destruct Interned as (Ok1 & (u1 & ->) & Len1 & Node1 & Path1).
    rewrite BasePath in Path1.
    rewrite <- Path1 in Rest.
    destruct (IH (t ++ u1) node rest_paths Ok1 Node1 Rest)
      as (t2 & nodes & Decode2 & Map2 & Ok2 & (u2 & ->) & Len2 & Forall2).
    exists ((t ++ u1) ++ u2), (node :: nodes).
    split.
    { cbn [decode_trie_all]. unfold decode_trie_one. rewrite One. fold base.
      rewrite InternEq, Decode2. reflexivity. }
    split.
    { cbn [map]. rewrite Map2. f_equal.
      rewrite path_of_extend by (destruct Ok1; assumption). exact Path1. }
    split; [exact Ok2|].
    split; [exists (u1 ++ u2); rewrite app_assoc; reflexivity|].
    split; [cbn [suffix_total]; lia|].
    constructor; [rewrite length_app; lia | exact Forall2].
Qed.

Theorem trie_nodes_bounded_by_suffixes : forall maximum entries t previous t' nodes,
  trie_ok t -> previous <= length t ->
  decode_trie_all maximum t previous entries = Some (t', nodes) ->
  length t' <= length t + suffix_total entries.
Proof.
  intros maximum entries.
  induction entries as [|[prefix suffix] rest IH]; intros t previous t' nodes Ok Le Decoded.
  - cbn in Decoded. injection Decoded as <- _. lia.
  - cbn [decode_trie_all] in Decoded. unfold decode_trie_one in Decoded.
    destruct (decode_one seg_eq_dec maximum (path_of t previous) (prefix, suffix));
      [|discriminate].
    set (base := ancestor t previous (length (path_of t previous) - prefix)) in Decoded.
    assert (base <= length t) as BaseLe.
    { pose proof (ancestor_le t (length (path_of t previous) - prefix) previous (proj1 Ok)).
      unfold base. lia. }
    pose proof (intern_spec suffix t base Ok BaseLe) as Interned.
    destruct (intern t base suffix) as [t1 node].
    destruct Interned as (Ok1 & _ & Len1 & Node1 & _).
    destruct (decode_trie_all maximum t1 node rest) as [[t2 rest_nodes]|] eqn:Rest;
      [|discriminate].
    injection Decoded as <- _.
    pose proof (IH t1 node t2 rest_nodes Ok1 Node1 Rest). cbn [suffix_total]. lia.
Qed.

(* The depth-first walk. A trie node with its children, sorted by segment as
   the children map iterates them, is a tree. *)
Definition seg_lt (x y : segment) : Prop :=
  fst x < fst y \/ (fst x = fst y /\ snd x < snd y).

Inductive path_lt : path -> path -> Prop :=
| path_lt_nil : forall y ys, path_lt [] (y :: ys)
| path_lt_head : forall x xs y ys, seg_lt x y -> path_lt (x :: xs) (y :: ys)
| path_lt_tail : forall x xs ys, path_lt xs ys -> path_lt (x :: xs) (x :: ys).

Lemma path_lt_irrefl : forall p, ~ path_lt p p.
Proof.
  induction p as [|x p IH]; intros Less; inversion Less; subst.
  - unfold seg_lt in *. lia.
  - contradiction.
Qed.

Inductive rtree := RNode (children : list (segment * rtree)).

Section RTreeInduction.
  Variable P : rtree -> Prop.
  Hypothesis node_case : forall children,
    Forall (fun child => P (snd child)) children -> P (RNode children).

  Fixpoint rtree_induction (tree : rtree) : P tree :=
    match tree with
    | RNode children =>
        node_case children
          ((fix all (children : list (segment * rtree))
              : Forall (fun child => P (snd child)) children :=
              match children with
              | [] => Forall_nil _
              | (s, child) :: rest => Forall_cons (s, child) (rtree_induction child) (all rest)
              end) children)
    end.
End RTreeInduction.

Definition block (walk : rtree -> list path) (children : list (segment * rtree))
    : list path :=
  fold_right (fun child paths => map (cons (fst child)) (walk (snd child)) ++ paths) [] children.

Fixpoint preorder (tree : rtree) : list path :=
  match tree with
  | RNode children =>
      [] :: (fix walk_children (children : list (segment * rtree)) : list path :=
               match children with
               | [] => []
               | (s, child) :: rest => map (cons s) (preorder child) ++ walk_children rest
               end) children
  end.

Lemma preorder_node : forall children,
  preorder (RNode children) = [] :: block preorder children.
Proof.
  intros children. cbn [preorder]. f_equal.
  induction children as [|[s child] rest IH]; [reflexivity|].
  cbn. rewrite IH. reflexivity.
Qed.

Inductive sorted_tree : rtree -> Prop :=
| sorted_node : forall children,
    StronglySorted seg_lt (map fst children) ->
    Forall (fun child => sorted_tree (snd child)) children ->
    sorted_tree (RNode children).

Lemma strongly_sorted_map : forall {X Y} (R : X -> X -> Prop) (R' : Y -> Y -> Prop)
  (f : X -> Y) l,
  (forall a b, R a b -> R' (f a) (f b)) -> StronglySorted R l -> StronglySorted R' (map f l).
Proof.
  intros X Y R R' f l Monotone Sorted.
  induction Sorted as [|a l Sorted IH Head]; cbn; constructor; [exact IH|].
  apply Forall_map. eapply Forall_impl; [|exact Head]. intros b. apply Monotone.
Qed.

Lemma strongly_sorted_app : forall {X} (R : X -> X -> Prop) l1 l2,
  StronglySorted R l1 -> StronglySorted R l2 ->
  (forall x y, In x l1 -> In y l2 -> R x y) -> StronglySorted R (l1 ++ l2).
Proof.
  intros X R l1 l2 Sorted1 Sorted2 Cross.
  induction Sorted1 as [|a l1 Sorted1 IH Head]; [exact Sorted2|].
  cbn. constructor.
  - apply IH. intros x y Member1 Member2. apply Cross; [right|]; assumption.
  - apply Forall_app. split; [exact Head|].
    apply Forall_forall. intros y Member. apply Cross; [left; reflexivity | exact Member].
Qed.

Lemma block_heads : forall walk children p,
  In p (block walk children) ->
  exists s q, p = s :: q /\ In s (map fst children).
Proof.
  intros walk children.
  induction children as [|[s child] rest IH]; intros p Member; [contradiction|].
  cbn in Member. apply in_app_or in Member as [Member|Member].
  - apply in_map_iff in Member as (q & <- & _).
    exists s, q. split; [reflexivity | left; reflexivity].
  - destruct (IH p Member) as (s' & q & -> & Head).
    exists s', q. split; [reflexivity | right; exact Head].
Qed.

(* The walk lists the paths of a sorted tree in strictly increasing
   lexicographic order. *)
Theorem trie_dfs_is_lex_sort : forall tree,
  sorted_tree tree -> StronglySorted path_lt (preorder tree).
Proof.
  apply (rtree_induction (fun tree => sorted_tree tree -> StronglySorted path_lt (preorder tree))).
  intros children IH Sorted.
  inversion Sorted as [? SortedHeads SortedChildren]; subst.
  rewrite preorder_node.
  assert (StronglySorted path_lt (block preorder children)) as Block.
  { clear Sorted.
    induction children as [|[s child] rest IHrest]; [constructor|].
    cbn [block fold_right fst snd].
    inversion SortedHeads as [|? ? SortedRest HeadLess]; subst.
    inversion IH as [|? ? ChildIH RestIH]; subst.
    inversion SortedChildren as [|? ? ChildSorted RestSorted]; subst.
    apply strongly_sorted_app.
    - apply strongly_sorted_map with (R := path_lt).
      + intros a b Less. apply path_lt_tail. exact Less.
      + apply ChildIH. exact ChildSorted.
    - apply IHrest; assumption.
    - intros x y MemberX MemberY.
      apply in_map_iff in MemberX as (a & <- & _).
      destruct (block_heads preorder rest y MemberY) as (s' & b & -> & Head).
      apply path_lt_head.
      rewrite Forall_forall in HeadLess. apply HeadLess. exact Head. }
  constructor; [exact Block|].
  apply Forall_forall. intros p Member.
  destruct (block_heads preorder children p Member) as (s & q & -> & _).
  constructor.
Qed.

Corollary preorder_unique : forall tree, sorted_tree tree -> NoDup (preorder tree).
Proof.
  intros tree Sorted.
  pose proof (trie_dfs_is_lex_sort tree Sorted) as Strict.
  induction Strict as [|p rest Strict IH Head]; constructor; [|exact IH].
  intros Member. rewrite Forall_forall in Head.
  exact (path_lt_irrefl p (Head p Member)).
Qed.

Inductive tree_path : rtree -> path -> Prop :=
| tree_path_root : forall children, tree_path (RNode children) []
| tree_path_child : forall children s child p,
    In (s, child) children -> tree_path child p -> tree_path (RNode children) (s :: p).

(* The walk lists every path of the tree and only those paths. *)
Theorem preorder_complete : forall tree p, In p (preorder tree) <-> tree_path tree p.
Proof.
  apply (rtree_induction (fun tree => forall p, In p (preorder tree) <-> tree_path tree p)).
  intros children IH p.
  rewrite preorder_node. cbn [In].
  split.
  - intros [<-|Member]; [constructor|].
    generalize dependent p.
    induction children as [|[s child] rest IHrest]; intros p Member; [contradiction|].
    inversion IH as [|? ? ChildIH RestIH]; subst.
    cbn [block fold_right fst snd] in Member.
    apply in_app_or in Member as [Member|Member].
    + apply in_map_iff in Member as (q & <- & MemberQ).
      apply tree_path_child with child; [left; reflexivity|].
      apply ChildIH. exact MemberQ.
    + specialize (IHrest RestIH p Member).
      inversion IHrest as [|? s' child' q InRest PathQ]; subst.
      * constructor.
      * apply tree_path_child with child'; [right; exact InRest | exact PathQ].
  - intros Path. inversion Path as [|? s child q InChildren PathQ]; subst; [left; reflexivity|].
    right.
    clear Path. induction children as [|[s0 child0] rest IHrest]; [contradiction|].
    inversion IH as [|? ? ChildIH RestIH]; subst.
    cbn [block fold_right fst snd]. apply in_or_app.
    destruct InChildren as [Same|InRest].
    + injection Same as -> ->. left. apply in_map. apply ChildIH. exact PathQ.
    + right. apply IHrest; assumption.
Qed.

(* Counterexample: a staircase of paths, each one segment longer than the
   last, has one new segment per path but quadratically many materialized
   segments. *)
Definition staircase (n : nat) : list path := map (repeat (0, 0)) (seq 1 n).

Fixpoint depth_total (paths : list path) : nat :=
  match paths with
  | [] => 0
  | p :: rest => length p + depth_total rest
  end.

Lemma lcp_repeat_succ : forall (x : segment) k,
  lcp seg_eq_dec (repeat x (S k)) (repeat x k) = k.
Proof.
  intros x k. induction k as [|k IH]; [reflexivity|].
  cbn [repeat lcp]. destruct (seg_eq_dec x x) as [_|Different]; [|contradiction].
  f_equal. exact IH.
Qed.

Lemma staircase_suffixes : forall (x : segment) m k,
  suffix_total (encode_all seg_eq_dec (repeat x k) (map (repeat x) (seq (S k) m))) = m.
Proof.
  intros x m. induction m as [|m IH]; intros k; [reflexivity|].
  cbn [seq map encode_all suffix_total]. unfold encode_one.
  rewrite lcp_repeat_succ.
  replace (skipn k (repeat x (S k))) with [x].
  - cbn [length]. rewrite IH. reflexivity.
  - clear IH. induction k as [|k IHk]; [reflexivity|]. cbn. exact IHk.
Qed.

Lemma staircase_depths : forall (x : segment) m s,
  2 * depth_total (map (repeat x) (seq s m)) + m = m * (2 * s + m).
Proof.
  intros x m. induction m as [|m IH]; intros s; [reflexivity|].
  cbn [seq map depth_total]. rewrite repeat_length.
  specialize (IH (S s)). nia.
Qed.

Theorem materialized_segments_quadratic : forall n,
  suffix_total (encode_all seg_eq_dec [] (staircase n)) = n /\
  2 * depth_total (staircase n) = n * (n + 1).
Proof.
  intros n. split.
  - exact (staircase_suffixes (0, 0) n 0).
  - assert (2 * depth_total (staircase n) + n = n * (2 * 1 + n)) as Depths
      by exact (staircase_depths (0, 0) n 1).
    nia.
Qed.

(* The depth sum is computed: lia does not read the abstracted large nat
   literal 262144. *)
Example materialized_segments_exceed_journal_limit :
  262144 < depth_total (staircase 724) /\
  suffix_total (encode_all seg_eq_dec [] (staircase 724)) = 724.
Proof.
  split.
  - apply Nat.ltb_lt. vm_compute. reflexivity.
  - exact (proj1 (materialized_segments_quadratic 724)).
Qed.
