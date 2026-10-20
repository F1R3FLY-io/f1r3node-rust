(** * CursorLinearity

    DR-120 (gap G9), law L7: the fast path of the v6 merge never composes a
    scope copy of a deploy whose settlement the base already holds.

    Dev's merge drops such a copy through its settled-sig probes
    (dag_merger.rs:984-1150). The fast path runs no probe. It relies on the
    fee cursor instead:

    - Every accepted offer pays a fee of 1 (acceptance.rs:22,414), so its
      committed funding case records a fee-cursor transition, which the fast
      path checks (P2, evidence.rs).
    - Each settlement consumes the cohort's current revision datum and
      produces the next revision (SystemVault.rho:176,268, cursor.rs:78-92).
      A datum's value holds its revision, so the datums of two revisions
      differ.

    Take the common history of the base and the scope copy, then the base's
    settlements on the same cohort after it, and the scope's settlements
    after it. If the base copy lies in the common history, the scope copy
    repeats a deploy of its own history, and the repeat rule rejects its
    block (validate.rs:601-717). Otherwise the base copy lies on the base
    side, so the base advanced the cursor past the divergence point. The
    first scope-side settlement then consumes a datum that the base no
    longer holds, and dev's availability walk rejects it (P6). A first use
    has the same shape: both sides consume the creation lock's datum at the
    divergence point.

    The model is a line of settlements per cohort. Revisions are natural
    numbers, and the datum of a revision is the revision itself, which
    models a datum whose value holds its revision. *)

From Stdlib Require Import Lists.List PeanoNat Lia.
Import ListNotations.

(** A settlement on one cohort's fee cursor: the deploy it settles and the
    revision whose datum it consumes. It produces the next revision. *)
Record settlement : Type := {
  deploy : nat;
  consumed : nat
}.

(** A line of settlements from revision [start]: each settlement consumes
    the revision that the previous one produced. *)
Fixpoint chained (start : nat) (line : list settlement) : Prop :=
  match line with
  | [] => True
  | x :: rest => consumed x = start /\ chained (S start) rest
  end.

(** The revision that a state holds after a line from [start]. *)
Definition final_revision (start : nat) (line : list settlement) : nat :=
  start + length line.

Lemma chained_app :
  forall start prefix suffix,
    chained start (prefix ++ suffix) <->
    chained start prefix /\ chained (start + length prefix) suffix.
Proof.
  intros start prefix.
  revert start.
  induction prefix as [| x prefix IH]; intros start suffix; simpl.
  - rewrite Nat.add_0_r. tauto.
  - rewrite IH. replace (S start + length prefix) with (start + S (length prefix)) by lia.
    tauto.
Qed.

(** The i-th settlement of a chained line consumes revision start + i. *)
Lemma chained_nth :
  forall line start i x,
    chained start line -> nth_error line i = Some x -> consumed x = start + i.
Proof.
  induction line as [| y line IH]; intros start i x Hchain Hnth.
  - destruct i; discriminate.
  - destruct Hchain as [Hy Hrest].
    destruct i as [| i]; simpl in Hnth.
    + injection Hnth as Hnth. subst x. rewrite Nat.add_0_r. exact Hy.
    + rewrite (IH (S start) i x Hrest Hnth). lia.
Qed.

(** The revision strictly rises along a line. *)
Theorem revision_strictly_rises :
  forall line start i j x y,
    chained start line ->
    nth_error line i = Some x -> nth_error line j = Some y ->
    i < j -> consumed x < consumed y.
Proof.
  intros line start i j x y Hchain Hx Hy Hij.
  rewrite (chained_nth line start i x Hchain Hx).
  rewrite (chained_nth line start j y Hchain Hy).
  lia.
Qed.

Section Datums.

  (** The datum of a revision on the cursor cell. Its value holds the
      revision, so the encoding is injective. *)
  Variable Datum : Type.
  Variable revision_datum : nat -> Datum.
  Hypothesis revision_datum_injective :
    forall r s, revision_datum r = revision_datum s -> r = s.

  Theorem distinct_revisions_distinct_datums :
    forall r s, r <> s -> revision_datum r <> revision_datum s.
  Proof.
    intros r s Hneq Heq.
    exact (Hneq (revision_datum_injective r s Heq)).
  Qed.

End Datums.

(** The base holds one cursor datum: the datum of its final revision. A
    datum of another revision is absent from the base. *)
Definition held_by_base (start : nat) (base_line : list settlement) (r : nat) : Prop :=
  r = final_revision start base_line.

(** P6 on the cursor: the first scope-side settlement consumes a datum that
    the base holds. Later scope-side settlements consume the datums that the
    scope side produced, as the chained line states. *)
Definition p6_cursor
    (start : nat) (common base_side scope_side : list settlement) : Prop :=
  forall x rest,
    scope_side = x :: rest -> held_by_base start (common ++ base_side) (consumed x).

(** The first scope-side settlement after a divergence consumes the datum at
    the divergence point. When the base side also settled after it, that
    datum is absent from the base. *)
Theorem first_divergent_consumer_removes_absent_datum :
  forall start common base_side scope_side x rest,
    chained start (common ++ base_side) ->
    chained start (common ++ scope_side) ->
    base_side <> [] -> scope_side = x :: rest ->
    ~ held_by_base start (common ++ base_side) (consumed x).
Proof.
  intros start common base_side scope_side x rest Hbase Hscope Hside Hfirst Hheld.
  apply chained_app in Hscope as [_ Hscope].
  rewrite Hfirst in Hscope.
  destruct Hscope as [Hx _].
  unfold held_by_base, final_revision in Hheld.
  rewrite length_app in Hheld.
  destruct base_side as [| b base_side].
  - apply Hside. reflexivity.
  - simpl in Hheld. lia.
Qed.

(** No deploy settles twice in one history: the repeat rule of a valid
    block, over the deploys of its ancestry. *)
Definition repeat_free (line : list settlement) : Prop := NoDup (map deploy line).

Lemma no_dup_app_disjoint :
  forall (l1 l2 : list nat) a, NoDup (l1 ++ l2) -> In a l1 -> In a l2 -> False.
Proof.
  induction l1 as [| b l1 IH]; intros l2 a Hnodup Hin1 Hin2.
  - contradiction.
  - simpl in Hnodup.
    inversion Hnodup as [| b' rest Hnotin Hnodup']; subst.
    destruct Hin1 as [Hb | Hin1].
    + subst a. apply Hnotin. apply in_or_app. right. exact Hin2.
    + exact (IH l2 a Hnodup' Hin1 Hin2).
Qed.

(** When the base copy lies in the common history, the scope copy's own
    history contains it, so the scope copy is a repeat. *)
Theorem extension_contains_base_copy :
  forall common scope_side base_copy scope_copy,
    In base_copy common -> In scope_copy scope_side ->
    deploy base_copy = deploy scope_copy ->
    ~ repeat_free (common ++ scope_side).
Proof.
  intros common scope_side base_copy scope_copy Hbase Hscope Hsame Hfree.
  unfold repeat_free in Hfree.
  rewrite map_app in Hfree.
  apply (no_dup_app_disjoint (map deploy common) (map deploy scope_side) (deploy base_copy) Hfree).
  - apply in_map. exact Hbase.
  - rewrite Hsame. apply in_map. exact Hscope.
Qed.

(** L7: a scope copy of a deploy whose settlement the base holds never
    passes the fast path. Either the repeat rule rejects its block, or its
    first scope-side settlement consumes a datum absent from the base. *)
Theorem scope_copy_of_base_deploy_fails_fast_path :
  forall start common base_side scope_side base_copy scope_copy,
    chained start (common ++ base_side) ->
    chained start (common ++ scope_side) ->
    In base_copy (common ++ base_side) ->
    In scope_copy scope_side ->
    deploy base_copy = deploy scope_copy ->
    repeat_free (common ++ scope_side) ->
    ~ p6_cursor start common base_side scope_side.
Proof.
  intros start common base_side scope_side base_copy scope_copy
    Hbase Hscope Hin_base Hin_scope Hsame Hfree Hp6.
  apply in_app_or in Hin_base as [Hcommon | Hside].
  - exact (extension_contains_base_copy common scope_side base_copy scope_copy
             Hcommon Hin_scope Hsame Hfree).
  - destruct scope_side as [| x rest].
    + contradiction.
    + apply (first_divergent_consumer_removes_absent_datum
               start common base_side (x :: rest) x rest Hbase Hscope).
      * intros Hnil. subst base_side. contradiction.
      * reflexivity.
      * exact (Hp6 x rest eq_refl).
Qed.

(** First uses: each creation consumes the creation lock's datum and
    produces a new one, a line with the same shape as the revisions. Before
    DR-119 the lock is global; after DR-119 it is the bucket lock of the
    scope. Two creations after a divergence consume the same lock datum, so
    the scope-side creation consumes a datum absent from the base. *)
Theorem first_use_copies_collide :
  forall start common base_creations scope_creations x rest,
    chained start (common ++ base_creations) ->
    chained start (common ++ scope_creations) ->
    base_creations <> [] -> scope_creations = x :: rest ->
    ~ held_by_base start (common ++ base_creations) (consumed x).
Proof.
  exact first_divergent_consumer_removes_absent_datum.
Qed.

(** Negative control: without the repeat rule, a scope copy whose history
    holds the base copy passes the cursor check. Its first settlement
    consumes the datum that the base holds, because the base side settled
    nothing after the divergence. *)
Theorem nc_without_repeat_rule_extension_copy_passes :
  let base_copy := {| deploy := 7; consumed := 0 |} in
  let scope_copy := {| deploy := 7; consumed := 1 |} in
  chained 0 ([base_copy] ++ []) /\
  chained 0 ([base_copy] ++ [scope_copy]) /\
  p6_cursor 0 [base_copy] [] [scope_copy] /\
  ~ repeat_free ([base_copy] ++ [scope_copy]).
Proof.
  simpl.
  split; [split; [reflexivity | exact I] |].
  split; [split; [reflexivity | split; [reflexivity | exact I]] |].
  split.
  - intros x rest Hfirst. injection Hfirst as Hx _. subst x.
    unfold held_by_base, final_revision. reflexivity.
  - unfold repeat_free. simpl. intros Hnodup.
    inversion Hnodup as [| a l Hnotin _]. apply Hnotin. left. reflexivity.
Qed.

(** A settlement whose revision is a mergeable counter consumes no datum:
    the merge folds counter diffs (rholang_merging_logic.rs:116-176). Its
    availability check then accepts every line. *)
Definition p6_counter (_ : list settlement) : Prop := True.

(** The settlements of one deploy in a composed outcome. *)
Definition settlements_of (d : nat) (line : list settlement) : nat :=
  length (filter (fun x => Nat.eqb (deploy x) d) line).

(** Negative control: with a mergeable revision counter, a scope copy of a
    deploy that the base side settled passes, and the composed outcome
    settles the deploy twice. *)
Theorem nc_integer_add_revision_hides_copy :
  let base_side := [{| deploy := 7; consumed := 0 |}] in
  let scope_side := [{| deploy := 7; consumed := 0 |}] in
  p6_counter scope_side /\ settlements_of 7 (base_side ++ scope_side) = 2.
Proof. simpl. split; [exact I | reflexivity]. Qed.

(** A settlement without a cursor transition touches no cursor datum, so the
    cursor check has nothing to test. P2 excludes it from the fast path. *)
Definition p6_without_cursor (_ : list nat) : Prop := True.

(** Negative control: a deploy that settles without a cursor transition
    passes the cursor check in both branches, and the composed outcome
    settles it twice. *)
Theorem nc_cursor_free_settlement_hides_copy :
  let base_side := [7] in
  let scope_side := [7] in
  p6_without_cursor scope_side /\ length (filter (Nat.eqb 7) (base_side ++ scope_side)) = 2.
Proof. simpl. split; [exact I | reflexivity]. Qed.
