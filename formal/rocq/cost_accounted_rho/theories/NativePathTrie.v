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
