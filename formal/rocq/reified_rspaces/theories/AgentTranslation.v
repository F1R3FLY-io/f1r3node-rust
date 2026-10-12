(* ═══════════════════════════════════════════════════════════════════════════
   AgentTranslation.v — the translation of receives on spaces that user
   agents implement (P8: design v2 W3, W5 and W6)

   A receive on a space of a user agent becomes one method call on the
   agent: consume(pairs, arrow, body). Each bind i gets a matcher
   capability m_i, whose op matches a process against the pattern of the
   bind with the injected matcher, checks the guard when the bind owns it,
   and replies the env of the bind. The body capability b is a receive
   whose pattern binds the union of the envs, so the original body runs
   with the values of the native receive. This module proves that the
   offsets of the binds partition the binder positions, that the envs of
   the binds are disjoint and cover the positions, that the body binds the
   values of the native receive, that the guard placement of TR-4 keeps the
   guard check of the native receive, that the matcher op replies exactly
   on a spatial match with a passing guard, and that the codec of a
   matcher state is exact. A `new` with binders of user agents (P8 step 4)
   becomes one gensym call per agent binder and a resume term, and the
   module proves that the resume term binds every binder of the `new` in
   its order (resume_binds_innermost).

   Sources
   - P8 design v2 (plan decision 77): TR-3 (offsets), TR-4 (the guard
     placement), TR-5 (the matcher state and its id), TR-6 (the body
     capability), MO-2 and MO-3 (the matcher op), HC-2 and HC-3 (the
     native consume), F6 to F8 and F20 (facts checked in code).
   - P8 design v2 TN-1 to TN-4 (the translation of `new`, P8 step 4).
   - Code: the tree of P8 step 4. The line numbers below are the line
     numbers of that tree.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   offsets_from, offsets, total
                            rholang/src/rust/interpreter/reified/
                            user_agent.rs:942-952 (offsets_of, TR-3)
   owner_from, owner        rholang/src/rust/interpreter/reified/
                            user_agent.rs:956-961 (owner_of)
   position, index_owners, place_guard
                            rholang/src/rust/interpreter/reified/
                            user_agent.rs:968-993 (place_guard, TR-4)
   indices                  rholang/src/rust/interpreter/reified/
                            user_agent.rs:640-663, 669-805, 809-938
                            (guard_indices, guard_par, guard_expr)
   env_of                   rholang/src/rust/interpreter/reified/
                            user_agent.rs:468-486 (env_reply, MO-3)
   union_env, body_binding  rholang/src/rust/interpreter/reified/
                            user_agent.rs:1000-1022 (body_pattern, TR-6);
                            rholang/src/rust/interpreter/reified/
                            handle.rs:697-725 (env_forwarder, HC-3)
   placeholders, guard_ok   rholang/src/rust/interpreter/reified/
                            user_agent.rs:428-463 (guard_passes)
   matcher_reply            rholang/src/rust/interpreter/reified/
                            user_agent.rs:363-391, 394-420
                            (MatcherProcess::call and answer, MO-3)
   to_sp, from_sp, consistent, encode, decode
                            rholang/src/rust/interpreter/reified/
                            user_agent.rs:153-173, 177-212, 217-229,
                            235-246 (MatcherState::to_par, from_par,
                            encode and decode, TR-5, MO-2)
   matcher_id, state_of_id  rholang/src/rust/interpreter/reified/
                            user_agent.rs:259-264 (matcher_state_of),
                            1088-1202 (translate_receive builds the id)
   binder, agents           rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:896-928, 945-992
                            (reified_new and agent_new: the agent
                            binders of a new, TN-1)
   targets_from, targets, tag
                            rholang/src/rust/interpreter/reified/
                            user_agent.rs:1272-1389 (translate_new, TN-2
                            to TN-4), 1220-1227 (agent_minted_name)
   resume_env, eval_target  the nested resume receives of translate_new
                            and the match of reduce.rs eval_match, whose
                            target the substitution evaluates
   match_env                reduce.rs eval_match (add_to_env pushes the
                            case bindings in order) and util/mod.rs
                            allocate_new_bindings (a native new pushes its
                            names in binder order)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - Values, patterns, processes and guards are section types. The
     injected matcher is a section function spatial, and the guard is a
     section function geval over the binder values in push order.
   - geval_local (a section hypothesis) states that a guard reads only the
     binders that its in-range BoundVar indices name. rho_pure_eval
     evaluates a guard in the env that guard_passes_metered pushes, so a
     BoundVar(k) reads the binder at position total - 1 - k (F7).
   - The key of an env entry is a number. The Rust key is the minimal
     decimal string of the number, an injective encoding.
   - prost is a pair of section functions penc and pdec with the section
     hypothesis that pdec inverts penc. The translation refuses a state
     deeper than the recursion limit of prost (AgentPatternTooDeep), so
     every state that the code encodes is in the domain of the hypothesis.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. offsets_partition        the ranges of the binds partition the
                                binder positions
    2. env_keys_disjoint_union  the env keys of the binds are disjoint and
                                cover the binder positions
    3. body_equivalence         the body binds the values of the native
                                receive, in binder order
    4. guard_fold_sound         the guard of the owner bind, over the
                                placeholders, equals the native guard
    5. matcher_sound_complete   the matcher replies exactly on a spatial
                                match with a passing guard
    6. matcher_state_roundtrip  the codec of a matcher state is exact
    7. resume_binds_innermost   the resume term of a translated new binds
                                every binder in its order (P8 step 4)

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `destruct (b1 && b2) eqn:E` after `cbn in H` leaves H unchanged: cbn
     unfolds andb in H, so the term no longer occurs. Use
     `cbn [owner_from] in H` and `revert H` before the destruct.
   - `injection` on `Some (to_sp s1) = Some (to_sp s2)` unfolds to_sp and
     splits the equation into fields. Compare the decoded options and
     finish with congruence instead.
   - This Stdlib has no map_fst_combine. The module proves
     map_fst_combine_eq and map_snd_combine_eq.
   - nth_error_seq gives `if k <? len then Some (start + k) else None`;
     replace the test with true from the bound before cbn.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
Import ListNotations.

(* ── 1. Offsets (TR-3) ─────────────────────────────────────────────────── *)

(* off_0 = o, off_(i+1) = off_i + f_i. *)
Fixpoint offsets_from (o : nat) (fs : list nat) : list nat :=
  match fs with
  | [] => []
  | f :: rest => o :: offsets_from (o + f) rest
  end.

Definition offsets (fs : list nat) : list nat := offsets_from 0 fs.

Fixpoint total (fs : list nat) : nat :=
  match fs with
  | [] => 0
  | f :: rest => f + total rest
  end.

(* The bind that owns the position p: the first i with off_i <= p < off_i + f_i. *)
Fixpoint owner_from (o i : nat) (fs : list nat) (p : nat) : option nat :=
  match fs with
  | [] => None
  | f :: rest => if (o <=? p) && (p <? o + f) then Some i else owner_from (o + f) (S i) rest p
  end.

Definition owner (fs : list nat) (p : nat) : option nat := owner_from 0 0 fs p.

Lemma offsets_from_length : forall fs o, length (offsets_from o fs) = length fs.
Proof. induction fs as [| f rest IH]; intros o; cbn; [reflexivity | rewrite IH; reflexivity]. Qed.

Lemma offsets_from_nth :
  forall fs o i, i < length fs -> nth i (offsets_from o fs) 0 = o + total (firstn i fs).
Proof.
  induction fs as [| f rest IH]; intros o i Hi; cbn in Hi; [lia |].
  destruct i as [| i]; cbn; [lia |].
  rewrite IH by lia. lia.
Qed.

Lemma total_firstn_le : forall fs i, total (firstn i fs) + nth i fs 0 <= total fs.
Proof.
  induction fs as [| f rest IH]; intros i; destruct i as [| i]; cbn; try lia.
  specialize (IH i). lia.
Qed.

Lemma owner_from_some :
  forall fs o i p j,
    owner_from o i fs p = Some j ->
    i <= j /\ j < i + length fs /\
    o + total (firstn (j - i) fs) <= p < o + total (firstn (j - i) fs) + nth (j - i) fs 0.
Proof.
  induction fs as [| f rest IH]; intros o i p j H; cbn [owner_from] in H; [discriminate H |].
  revert H. destruct ((o <=? p) && (p <? o + f)) eqn:E; intros H.
  - injection H as <-. apply andb_true_iff in E as [E1 E2].
    apply Nat.leb_le in E1. apply Nat.ltb_lt in E2.
    rewrite Nat.sub_diag. cbn. lia.
  - destruct (IH (o + f) (S i) p j H) as [Hl [Hu Hr]].
    replace (j - i) with (S (j - S i)) by lia. cbn [firstn total nth length]. lia.
Qed.

Lemma owner_from_none :
  forall fs o i p, owner_from o i fs p = None -> p < o \/ o + total fs <= p.
Proof.
  induction fs as [| f rest IH]; intros o i p H; cbn [owner_from total] in H |- *; [lia |].
  revert H. destruct ((o <=? p) && (p <? o + f)) eqn:E; intros H; [discriminate H |].
  destruct (IH (o + f) (S i) p H) as [Hlt | Hge].
  - apply andb_false_iff in E as [E | E].
    + apply Nat.leb_gt in E. lia.
    + apply Nat.ltb_ge in E. lia.
  - lia.
Qed.

Lemma owner_from_in_range :
  forall fs o i p k,
    k < length fs ->
    o + total (firstn k fs) <= p < o + total (firstn k fs) + nth k fs 0 ->
    owner_from o i fs p = Some (i + k).
Proof.
  induction fs as [| f rest IH]; intros o i p k Hk Hr; cbn in Hk; [lia |].
  cbn [owner_from].
  destruct k as [| k].
  - cbn in Hr. replace ((o <=? p) && (p <? o + f)) with true.
    + f_equal. lia.
    + symmetry. apply andb_true_iff. split; [apply Nat.leb_le | apply Nat.ltb_lt]; lia.
  - cbn [firstn total nth] in Hr.
    replace ((o <=? p) && (p <? o + f)) with false.
    + rewrite (IH (o + f) (S i) p k) by lia. f_equal. lia.
    + symmetry. apply andb_false_iff. right. apply Nat.ltb_ge. lia.
Qed.

(* Headline 1. The ranges [off_i, off_i + f_i) of the binds partition
   [0, total): every position below total has exactly one owner bind, the
   owner's range holds the position, and no position from total on has an
   owner. *)
Theorem offsets_partition :
  forall fs,
    length (offsets fs) = length fs /\
    (forall i, i < length fs ->
       nth i (offsets fs) 0 = total (firstn i fs) /\
       nth i (offsets fs) 0 + nth i fs 0 <= total fs) /\
    (forall p i, owner fs p = Some i ->
       i < length fs /\ nth i (offsets fs) 0 <= p < nth i (offsets fs) 0 + nth i fs 0) /\
    (forall p i, i < length fs ->
       nth i (offsets fs) 0 <= p < nth i (offsets fs) 0 + nth i fs 0 -> owner fs p = Some i) /\
    (forall p, p < total fs <-> owner fs p <> None).
Proof.
  intros fs. unfold offsets, owner.
  split; [apply offsets_from_length |].
  split.
  { intros i Hi. rewrite offsets_from_nth by exact Hi. split; [lia |].
    pose proof (total_firstn_le fs i). lia. }
  split.
  { intros p i H. destruct (owner_from_some fs 0 0 p i H) as [_ [Hu Hr]].
    rewrite Nat.sub_0_r in Hr. rewrite offsets_from_nth by lia. lia. }
  split.
  { intros p i Hi Hr. rewrite offsets_from_nth in Hr by exact Hi.
    rewrite (owner_from_in_range fs 0 0 p i Hi) by lia. reflexivity. }
  intros p. split.
  - intros Hp Hn. destruct (owner_from_none fs 0 0 p Hn); lia.
  - intros Hn. destruct (owner_from 0 0 fs p) as [j |] eqn:E; [| contradiction Hn; reflexivity].
    destruct (owner_from_some fs 0 0 p j E) as [_ [Hu Hr]].
    pose proof (total_firstn_le fs (j - 0)). lia.
Qed.

(* ── 2. The envs of the matchers and of the body (MO-3, TR-6) ──────────── *)

Section Envs.
  Variable value : Type.

  (* The env of a bind whose first binder has the global position o: the
     key o + j holds the j-th matched value. The Rust key is the minimal
     decimal string of o + j, an injective encoding of the number. *)
  Definition env_of (o : nat) (vs : list value) : list (nat * value) :=
    combine (seq o (length vs)) vs.

  Fixpoint union_from (o : nat) (vss : list (list value)) : list (nat * value) :=
    match vss with
    | [] => []
    | vs :: rest => env_of o vs ++ union_from (o + length vs) rest
    end.

  (* The union of the envs of every bind, in bind order. *)
  Definition union_env (vss : list (list value)) : list (nat * value) := union_from 0 vss.

  (* The free counts of the binds. *)
  Definition counts (vss : list (list value)) : list nat := map (@length value) vss.

  Lemma map_fst_combine_eq :
    forall (A B : Type) (l1 : list A) (l2 : list B),
      length l1 = length l2 -> map fst (combine l1 l2) = l1.
  Proof.
    intros A B l1. induction l1 as [| a rest IH]; intros l2 H; [reflexivity |].
    destruct l2 as [| b l2]; cbn in H; [discriminate H |].
    cbn. rewrite IH by lia. reflexivity.
  Qed.

  Lemma map_snd_combine_eq :
    forall (A B : Type) (l1 : list A) (l2 : list B),
      length l1 = length l2 -> map snd (combine l1 l2) = l2.
  Proof.
    intros A B l1. induction l1 as [| a rest IH]; intros l2 H.
    - destruct l2; [reflexivity | discriminate H].
    - destruct l2 as [| b l2]; cbn in H; [discriminate H |].
      cbn. rewrite IH by lia. reflexivity.
  Qed.

  Lemma env_of_keys : forall o vs, map fst (env_of o vs) = seq o (length vs).
  Proof.
    intros o vs. unfold env_of. apply map_fst_combine_eq. apply length_seq.
  Qed.

  Lemma env_of_values : forall o vs, map snd (env_of o vs) = vs.
  Proof.
    intros o vs. unfold env_of. apply map_snd_combine_eq. apply length_seq.
  Qed.

  Lemma union_from_keys :
    forall vss o, map fst (union_from o vss) = seq o (total (counts vss)).
  Proof.
    induction vss as [| vs rest IH]; intros o; cbn; [reflexivity |].
    rewrite map_app, env_of_keys, IH, seq_app. reflexivity.
  Qed.

  Lemma union_from_values : forall vss o, map snd (union_from o vss) = concat vss.
  Proof.
    induction vss as [| vs rest IH]; intros o; cbn; [reflexivity |].
    rewrite map_app, env_of_values, IH. reflexivity.
  Qed.

  (* Headline 2. The key sets of the matcher envs are disjoint and their
     union is exactly the binder positions [0, total) of the receive: the
     keys of bind i are [off_i, off_i + f_i). *)
  Theorem env_keys_disjoint_union :
    forall vss,
      map fst (union_env vss) = seq 0 (total (counts vss)) /\
      NoDup (map fst (union_env vss)) /\
      (forall i, i < length vss ->
         map fst (env_of (nth i (offsets (counts vss)) 0) (nth i vss [])) =
         seq (nth i (offsets (counts vss)) 0) (nth i (counts vss) 0)).
  Proof.
    intros vss. unfold union_env. rewrite union_from_keys.
    split; [reflexivity |]. split; [apply seq_NoDup |].
    intros i Hi. rewrite env_of_keys. f_equal. unfold counts.
    rewrite (nth_indep _ 0 (length (@nil value))) by (rewrite length_map; exact Hi).
    rewrite map_nth. reflexivity.
  Qed.

  (* The value at key k. *)
  Fixpoint lookup (k : nat) (env : list (nat * value)) : option value :=
    match env with
    | [] => None
    | (k', v) :: rest => if k =? k' then Some v else lookup k rest
    end.

  (* The binders of the body receive: its pattern {"g": FreeVar(g) | g <
     total} binds FreeVar(g) to the value at key g, in binder-level order. *)
  Definition body_binding (env : list (nat * value)) (t : nat) : list (option value) :=
    map (fun g => lookup g env) (seq 0 t).

  Lemma lookup_seq_combine :
    forall vs o k, k < length vs -> lookup (o + k) (combine (seq o (length vs)) vs) = nth_error vs k.
  Proof.
    induction vs as [| v rest IH]; intros o k Hk; cbn in Hk; [lia |].
    cbn [seq length combine lookup].
    destruct k as [| k].
    - rewrite Nat.add_0_r, Nat.eqb_refl. reflexivity.
    - replace (o + S k =? o) with false by (symmetry; apply Nat.eqb_neq; lia).
      replace (o + S k) with (S o + k) by lia. apply IH. lia.
  Qed.

  Lemma lookup_app_left :
    forall k l1 l2, In k (map fst l1) -> lookup k (l1 ++ l2) = lookup k l1.
  Proof.
    intros k l1 l2 H. induction l1 as [| [k' v] rest IH]; cbn in H |- *; [contradiction H |].
    destruct (k =? k') eqn:E; [reflexivity |].
    apply IH. destruct H as [H | H]; [apply Nat.eqb_neq in E; congruence | exact H].
  Qed.

  Lemma lookup_app_right :
    forall k l1 l2, ~ In k (map fst l1) -> lookup k (l1 ++ l2) = lookup k l2.
  Proof.
    intros k l1 l2 H. induction l1 as [| [k' v] rest IH]; cbn in H |- *; [reflexivity |].
    destruct (k =? k') eqn:E.
    - apply Nat.eqb_eq in E. subst. exfalso. apply H. left. reflexivity.
    - apply IH. intros Hin. apply H. right. exact Hin.
  Qed.

  Lemma union_from_lookup :
    forall vss o k, k < total (counts vss) ->
      lookup (o + k) (union_from o vss) = nth_error (concat vss) k.
  Proof.
    induction vss as [| vs rest IH]; intros o k Hk; unfold counts in *;
      cbn [map total] in Hk; [lia |].
    cbn [union_from concat].
    destruct (Nat.lt_ge_cases k (length vs)) as [Hlt | Hge].
    - rewrite lookup_app_left.
      + unfold env_of. rewrite lookup_seq_combine by exact Hlt.
        rewrite nth_error_app1 by exact Hlt. reflexivity.
      + rewrite env_of_keys. apply in_seq. lia.
    - rewrite lookup_app_right.
      + rewrite nth_error_app2 by exact Hge.
        replace (o + k) with ((o + length vs) + (k - length vs)) by lia.
        apply IH. lia.
      + rewrite env_of_keys. intros Hin. apply in_seq in Hin. lia.
  Qed.

  (* Headline 3. The body receive binds, in binder-level order, exactly the
     values that the native receive binds: the matched values of every bind,
     in bind order (the env that build_env gives the native body). *)
  Theorem body_equivalence :
    forall vss, body_binding (union_env vss) (total (counts vss)) = map Some (concat vss).
  Proof.
    intros vss. unfold body_binding, union_env.
    assert (Hlen : length (concat vss) = total (counts vss)).
    { clear. induction vss as [| vs rest IH]; cbn; [reflexivity |].
      rewrite length_app, IH. reflexivity. }
    apply nth_error_ext. intros k.
    destruct (Nat.lt_ge_cases k (total (counts vss))) as [Hk | Hk].
    - rewrite nth_error_map, nth_error_map, nth_error_seq.
      replace (k <? total (counts vss)) with true by (symmetry; apply Nat.ltb_lt; exact Hk).
      cbn [option_map]. rewrite Nat.add_0_l.
      pose proof (union_from_lookup vss 0 k Hk) as L. rewrite Nat.add_0_l in L.
      rewrite L.
      destruct (nth_error (concat vss) k) eqn:E; [reflexivity |].
      apply nth_error_None in E. lia.
    - rewrite (proj2 (nth_error_None _ _)) by (rewrite length_map, length_seq; exact Hk).
      rewrite (proj2 (nth_error_None _ _)) by (rewrite length_map; lia).
      reflexivity.
  Qed.
End Envs.

(* ── 3. Guard placement (TR-4) ─────────────────────────────────────────── *)

(* BoundVar(k) of a receive with t binders reads the binder at the global
   position t - 1 - k (guard_passes_metered pushes the values of every bind
   in bind order). An index from t on is unbound, as in a native receive
   (F7). *)
Definition position (t k : nat) : option nat := if k <? t then Some (t - 1 - k) else None.

(* The owner bind of each in-range index of a guard. *)
Definition index_owners (fs : list nat) (ks : list nat) : list nat :=
  flat_map (fun k => match position (total fs) k with
                     | Some p => match owner fs p with Some i => [i] | None => [] end
                     | None => []
                     end) ks.

Inductive placement : Type := PlaceAt : nat -> placement | SpansJoin.

(* TR-4: the one owner of the in-range indices takes the guard, a guard
   with no in-range index goes to bind 0, and two or more owners abort with
   AgentGuardSpansJoin. *)
Definition place_guard (fs : list nat) (ks : list nat) : placement :=
  match nodup Nat.eq_dec (index_owners fs ks) with
  | [] => PlaceAt 0
  | [i] => PlaceAt i
  | _ => SpansJoin
  end.

Lemma place_guard_owner :
  forall fs ks i k p,
    place_guard fs ks = PlaceAt i -> In k ks -> position (total fs) k = Some p ->
    owner fs p = Some i.
Proof.
  intros fs ks i k p Hp Hk Hpos. unfold place_guard in Hp.
  assert (Hin : forall j, owner fs p = Some j -> In j (nodup Nat.eq_dec (index_owners fs ks))).
  { intros j Hj. apply nodup_In. unfold index_owners. apply in_flat_map.
    exists k. split; [exact Hk |]. rewrite Hpos, Hj. left. reflexivity. }
  assert (Hlt : p < total fs).
  { unfold position in Hpos. destruct (k <? total fs) eqn:E; [| discriminate Hpos].
    injection Hpos as <-. apply Nat.ltb_lt in E. lia. }
  destruct (proj2 (proj2 (proj2 (proj2 (offsets_partition fs)))) p) as [Hsome _].
  destruct (owner fs p) as [j |] eqn:Ej; [| contradiction (Hsome Hlt); reflexivity].
  specialize (Hin j eq_refl).
  destruct (nodup Nat.eq_dec (index_owners fs ks)) as [| a [| b rest]]; cbn in Hin.
  - contradiction Hin.
  - injection Hp as <-. destruct Hin as [-> | []]. reflexivity.
  - discriminate Hp.
Qed.

(* The owner of a position shifts with the offset of the first bind. *)
Lemma owner_from_shift :
  forall fs o i p, o <= p -> owner_from o i fs p = option_map (Nat.add i) (owner_from 0 0 fs (p - o)).
Proof.
  induction fs as [| f rest IH]; intros o i p Hop; [reflexivity |].
  cbn [owner_from].
  replace (o <=? p) with true by (symmetry; apply Nat.leb_le; exact Hop).
  replace (0 <=? p - o) with true by (symmetry; apply Nat.leb_le; lia).
  cbn [andb].
  destruct (p <? o + f) eqn:E1; destruct (p - o <? 0 + f) eqn:E2.
  - cbn. f_equal. lia.
  - apply Nat.ltb_lt in E1. apply Nat.ltb_ge in E2. lia.
  - apply Nat.ltb_ge in E1. apply Nat.ltb_lt in E2. lia.
  - apply Nat.ltb_ge in E1. rewrite (IH (o + f) (S i) p) by lia.
    rewrite (IH (0 + f) 1 (p - o)) by lia.
    destruct (owner_from 0 0 rest (p - (o + f))) eqn:E3;
      replace (p - o - (0 + f)) with (p - (o + f)) by lia; rewrite E3; cbn; [f_equal; lia | reflexivity].
Qed.

Lemma owner_cons_succ :
  forall f fs p k, owner (f :: fs) p = Some (S k) -> f <= p /\ owner fs (p - f) = Some k.
Proof.
  intros f fs p k H. unfold owner in H. cbn [owner_from] in H.
  replace (0 <=? p) with true in H by (symmetry; apply Nat.leb_le; lia).
  cbn [andb] in H. destruct (p <? 0 + f) eqn:E; [discriminate H |].
  apply Nat.ltb_ge in E.
  rewrite owner_from_shift in H by lia.
  destruct (owner_from 0 0 fs (p - (0 + f))) as [j |] eqn:Ej; cbn in H; [| discriminate H].
  injection H as Hj. split; [lia |]. unfold owner.
  replace (p - f) with (p - (0 + f)) by lia. rewrite Ej. f_equal. lia.
Qed.

Lemma owner_cons_zero :
  forall f fs p, owner (f :: fs) p = Some 0 -> p < f.
Proof.
  intros f fs p H. unfold owner in H. cbn [owner_from] in H.
  destruct ((0 <=? p) && (p <? 0 + f)) eqn:E.
  - apply andb_true_iff in E as [_ E]. apply Nat.ltb_lt in E. lia.
  - rewrite owner_from_shift in H by (apply andb_false_iff in E as [E | E];
      [apply Nat.leb_gt in E; lia | apply Nat.ltb_ge in E; lia]).
    destruct (owner_from 0 0 fs (p - (0 + f))); cbn in H; discriminate H.
Qed.

Section Guards.
  Variable value : Type.
  (* Par::default(): the placeholder that the matcher of one bind pushes
     at each position of another bind (MO-3). *)
  Variable nil_value : value.
  Variable guard : Type.
  (* The free BoundVar indices of a guard: the charged walk of TR-4. *)
  Variable indices : guard -> list nat.
  (* The guard over the binder values in push order (rho_pure_eval). *)
  Variable geval : guard -> list value -> bool.
  (* Locality: a guard reads only the binders that its in-range indices
     name. *)
  Hypothesis geval_local :
    forall g e1 e2,
      length e1 = length e2 ->
      (forall k p, In k (indices g) -> position (length e1) k = Some p ->
                   nth_error e1 p = nth_error e2 p) ->
      geval g e1 = geval g e2.

  (* The values that the matcher of a bind pushes: its own values vs at its
     positions, and a placeholder at each position of another bind. The
     index counts down to the own bind, and None follows it. *)
  Fixpoint placeholders_at (fs : list nat) (i : option nat) (vs : list value) : list value :=
    match fs with
    | [] => []
    | f :: rest =>
        match i with
        | Some 0 => vs ++ placeholders_at rest None vs
        | Some (S k) => repeat nil_value f ++ placeholders_at rest (Some k) vs
        | None => repeat nil_value f ++ placeholders_at rest None vs
        end
    end.

  Definition placeholders (fs : list nat) (i : nat) (vs : list value) : list value :=
    placeholders_at fs (Some i) vs.

  Lemma placeholders_none_length :
    forall fs vs, length (placeholders_at fs None vs) = total fs.
  Proof.
    induction fs as [| f rest IH]; intros vs; [reflexivity |].
    cbn. rewrite length_app, repeat_length, IH. reflexivity.
  Qed.

  Lemma concat_length : forall vss : list (list value), length (concat vss) = total (counts value vss).
  Proof.
    induction vss as [| vs rest IH]; [reflexivity |].
    cbn. rewrite length_app, IH. reflexivity.
  Qed.

  (* The placeholders of bind i agree with the full env at every position
     that bind i owns, and they have the length of the full env. *)
  Lemma placeholders_agree :
    forall vss i,
      i < length vss ->
      length (placeholders (counts value vss) i (nth i vss [])) = length (concat vss) /\
      (forall p, owner (counts value vss) p = Some i ->
                 nth_error (placeholders (counts value vss) i (nth i vss [])) p =
                 nth_error (concat vss) p).
  Proof.
    unfold placeholders. intros vss i. revert vss.
    induction i as [| k IH]; intros vss Hi; destruct vss as [| vs rest]; cbn in Hi; try lia.
    - unfold counts. cbn [map placeholders_at concat nth].
      split.
      + rewrite !length_app, placeholders_none_length.
        f_equal. symmetry. apply concat_length.
      + intros p Hp. apply owner_cons_zero in Hp.
        rewrite !nth_error_app1 by exact Hp. reflexivity.
    - unfold counts. cbn [map placeholders_at concat nth].
      destruct (IH rest ltac:(lia)) as [Hlen Hagree].
      split.
      + rewrite !length_app, repeat_length. unfold counts in Hlen. rewrite Hlen. reflexivity.
      + intros p Hp. apply owner_cons_succ in Hp as [Hge Hp].
        rewrite nth_error_app2 by (rewrite repeat_length; exact Hge).
        rewrite nth_error_app2 by exact Hge.
        rewrite repeat_length. apply Hagree. exact Hp.
  Qed.

  (* Headline 4. The guard placement of TR-4 is sound: when the guard goes
     to bind i, its value in the matcher of bind i, over the placeholders,
     equals its value in the native receive, over the values of every bind
     in bind order. So the matcher filters each candidate as the native
     guard check does. *)
  Theorem guard_fold_sound :
    forall vss g i,
      i < length vss ->
      place_guard (counts value vss) (indices g) = PlaceAt i ->
      geval g (placeholders (counts value vss) i (nth i vss [])) = geval g (concat vss).
  Proof.
    intros vss g i Hi Hp.
    destruct (placeholders_agree vss i Hi) as [Hlen Hagree].
    apply geval_local; [exact Hlen |].
    intros k p Hk Hpos. apply Hagree.
    rewrite Hlen, concat_length in Hpos.
    exact (place_guard_owner (counts value vss) (indices g) i k p Hp Hk Hpos).
  Qed.
End Guards.

(* ── 4. The matcher op (MO-3) ──────────────────────────────────────────── *)

Section Matcher.
  Variable value pattern proc guard : Type.
  Variable nil_value : value.
  (* The injected metered matcher on the pattern of a bind: the values of
     its free variables in binder order, or None. *)
  Variable spatial : pattern -> proc -> option (list value).
  Variable geval : guard -> list value -> bool.

  (* The parts of a matcher state that the op reads. *)
  Record mstate : Type := MState {
    m_bind : nat;              (* i *)
    m_counts : list nat;       (* the free count of every bind *)
    m_pattern : pattern;       (* the pattern of bind i *)
    m_guard : option guard     (* the guard, when bind i owns it *)
  }.

  Definition m_offset (s : mstate) : nat := nth (m_bind s) (offsets (m_counts s)) 0.

  Definition guard_ok (s : mstate) (vs : list value) : bool :=
    match m_guard s with
    | None => true
    | Some g => geval g (placeholders value nil_value (m_counts s) (m_bind s) vs)
    end.

  (* MO-3: match the process with the pattern of the bind, then check the
     guard over the placeholders, then reply the env of the bind. *)
  Definition matcher_reply (s : mstate) (x : proc) : option (list (nat * value)) :=
    match spatial (m_pattern s) x with
    | Some vs => if guard_ok s vs then Some (env_of value (m_offset s) vs) else None
    | None => None
    end.

  (* Headline 5. The matcher replies (true, env) exactly when the spatial
     match of the injected matcher succeeds and the guard of the bind
     passes, and env then maps the global position of each binder of the
     bind to its value. With guard_fold_sound, the guard check equals the
     check of the native receive. *)
  Theorem matcher_sound_complete :
    forall s x env,
      matcher_reply s x = Some env <->
      exists vs, spatial (m_pattern s) x = Some vs /\ guard_ok s vs = true /\
                 env = env_of value (m_offset s) vs.
  Proof.
    intros s x env. unfold matcher_reply. split.
    - destruct (spatial (m_pattern s) x) as [vs |]; [| discriminate].
      destruct (guard_ok s vs) eqn:Eg; [| discriminate].
      intros H. injection H as <-. exists vs. auto.
    - intros [vs [Hs [Hg ->]]]. rewrite Hs, Hg. reflexivity.
  Qed.
End Matcher.

(* ── 5. The matcher state codec (TR-5, MO-2) ───────────────────────────── *)

Section Codec.
  (* A pattern or a guard: an opaque Par. *)
  Variable par : Type.
  (* The remainder Var of the patterns of a bind. *)
  Variable var : Type.

  (* The Par of a state, as MatcherState::to_par builds it: integers, the
     bytes of the receive tag, lists with a remainder, tuples, and opaque
     Pars. *)
  Inductive sp : Type :=
  | SInt : nat -> sp
  | SBytes : list nat -> sp
  | SList : list sp -> option var -> sp
  | STuple : list sp -> sp
  | SPar : par -> sp.

  Record state : Type := State {
    s_tag : list nat;          (* the receive tag, 32 bytes *)
    s_bind : nat;              (* i *)
    s_offset : nat;            (* off_i *)
    s_total : nat;             (* total *)
    s_binds : nat;             (* n *)
    s_patterns : list par;     (* the patterns of bind i *)
    s_remainder : option var;  (* their remainder *)
    s_free : nat;              (* the free count of bind i *)
    s_guard : option par       (* the guard, or none *)
  }.

  Definition version : nat := 1.
  Definition tag_len : nat := 32.

  (* MatcherState::to_par: (version, tag, i, off_i, total, n,
     (EList{patterns, remainder}, free_count), [] or [guard]). *)
  Definition to_sp (s : state) : sp :=
    STuple [SInt version; SBytes (s_tag s); SInt (s_bind s); SInt (s_offset s);
            SInt (s_total s); SInt (s_binds s);
            STuple [SList (map SPar (s_patterns s)) (s_remainder s); SInt (s_free s)];
            SList (match s_guard s with None => [] | Some g => [SPar g] end) None].

  Fixpoint all_pars (l : list sp) : option (list par) :=
    match l with
    | [] => Some []
    | SPar p :: rest => match all_pars rest with Some ps => Some (p :: ps) | None => None end
    | _ :: _ => None
    end.

  (* MatcherState::from_par: the exact shape, the version, and a tag of 32
     bytes. *)
  Definition from_sp (x : sp) : option state :=
    match x with
    | STuple [SInt v; SBytes tag; SInt i; SInt off; SInt t; SInt n;
              STuple [SList pats rem; SInt free]; SList gl None] =>
        if (v =? version) && (length tag =? tag_len) then
          match all_pars pats, gl with
          | Some ps, [] => Some (State tag i off t n ps rem free None)
          | Some ps, [SPar g] => Some (State tag i off t n ps rem free (Some g))
          | _, _ => None
          end
        else None
    | _ => None
    end.

  (* MatcherState::is_consistent: the bind is one of the n binds, and its
     binders lie inside the total binders. *)
  Definition consistent (s : state) : bool :=
    (s_bind s <? s_binds s) && (s_offset s + s_free s <=? s_total s).

  (* prost: an encoder and a decoder that inverts it on every message
     within its recursion limit (the translation refuses a deeper state
     with AgentPatternTooDeep before it mints the capability). *)
  Variable penc : sp -> list nat.
  Variable pdec : list nat -> option sp.
  Hypothesis pdec_penc : forall x, pdec (penc x) = Some x.

  Definition encode (s : state) : list nat := penc (to_sp s).

  Definition bytes_eqb (a b : list nat) : bool :=
    if list_eq_dec Nat.eq_dec a b then true else false.

  (* MatcherState::decode: the prost decode, the shape, the consistency,
     and a byte-equal re-encode. *)
  Definition decode (b : list nat) : option state :=
    match pdec b with
    | Some x =>
        match from_sp x with
        | Some s => if consistent s && bytes_eqb (encode s) b then Some s else None
        | None => None
        end
    | None => None
    end.

  (* The matcher id E0‖X‖state and its strict reader: uppercase X only. *)
  Definition reified_tag : nat := 224.
  Definition matcher_kind : nat := 88.     (* 'X' *)

  Definition matcher_id (s : state) : list nat := reified_tag :: matcher_kind :: encode s.

  Definition state_of_id (id : list nat) : option state :=
    match id with
    | t :: k :: rest => if (t =? reified_tag) && (k =? matcher_kind) then decode rest else None
    | _ => None
    end.

  Lemma all_pars_map : forall ps, all_pars (map SPar ps) = Some ps.
  Proof. induction ps as [| p rest IH]; cbn; [reflexivity | rewrite IH; reflexivity]. Qed.

  Lemma from_to_sp : forall s, length (s_tag s) = tag_len -> from_sp (to_sp s) = Some s.
  Proof.
    intros [tag i off t n ps rem free g] Htag. cbn in Htag. unfold to_sp, from_sp. cbn.
    rewrite Htag, Nat.eqb_refl. cbn. rewrite all_pars_map.
    destruct g; reflexivity.
  Qed.

  (* Headline 6. The codec of a matcher state is exact: a consistent state
     with a tag of 32 bytes decodes from its encoding, a decoded state
     re-encodes to the same bytes and is consistent, the encoding is
     injective, and the id of a matcher reads back its state. *)
  Theorem matcher_state_roundtrip :
    (forall s, length (s_tag s) = tag_len -> consistent s = true -> decode (encode s) = Some s) /\
    (forall b s, decode b = Some s -> encode s = b /\ consistent s = true) /\
    (forall s1 s2, length (s_tag s1) = tag_len -> length (s_tag s2) = tag_len ->
                   encode s1 = encode s2 -> s1 = s2) /\
    (forall s, length (s_tag s) = tag_len -> consistent s = true ->
               state_of_id (matcher_id s) = Some s).
  Proof.
    assert (Hround : forall s, length (s_tag s) = tag_len -> consistent s = true ->
                               decode (encode s) = Some s).
    { intros s Htag Hc. unfold decode. unfold encode at 1. rewrite pdec_penc, from_to_sp by exact Htag.
      rewrite Hc. unfold bytes_eqb. destruct (list_eq_dec Nat.eq_dec (encode s) (encode s)) as [_ | Hne];
        [reflexivity | contradiction Hne; reflexivity]. }
    split; [exact Hround |].
    split.
    { intros b s H. unfold decode in H.
      destruct (pdec b) as [x |]; [| discriminate H].
      destruct (from_sp x) as [s' |]; [| discriminate H].
      destruct (consistent s') eqn:Ec; cbn in H; [| discriminate H].
      unfold bytes_eqb in H. destruct (list_eq_dec Nat.eq_dec (encode s') b) as [He | _]; [| discriminate H].
      injection H as <-. split; [exact He | exact Ec]. }
    split.
    { intros s1 s2 H1 H2 He. unfold encode in He.
      assert (Hd : option_map from_sp (pdec (penc (to_sp s1))) =
                   option_map from_sp (pdec (penc (to_sp s2)))) by (rewrite He; reflexivity).
      rewrite !pdec_penc in Hd. cbn [option_map] in Hd.
      rewrite (from_to_sp s1 H1), (from_to_sp s2 H2) in Hd. congruence. }
    intros s Htag Hc. unfold state_of_id, matcher_id. cbn [Nat.eqb reified_tag matcher_kind andb].
    apply Hround; assumption.
  Qed.
End Codec.

(* ── 7. The resume term of a translated new (TN-3) ─────────────────────── *)

Section Resume.
  (* Values, and the agent-minted name of a reply raw of the agent a: the
     bundle with both flags around (AGENT_TAG, a, raw). SpaceRefs.v proves
     that R4 routes it back to a and raw (tag_untag_id). *)
  Variable V : Type.
  Variable tag : V -> V -> V.

  (* A binder of the new: an agent binder of the agent a, or a binder whose
     name the allocation gives (the name of HEAD, of a native space, or the
     value of a URI). *)
  Inductive binder : Type := BAgent (a : V) | BValue (v : V).

  (* An element of the target tuple of the resume match, as translate_new
     builds it: a bound name, or the agent-minted name of BoundVar(n) of the
     nested receives. *)
  Inductive target : Type := TVal (v : V) | TTag (a : V) (n : nat).

  Fixpoint agents (bs : list binder) : nat :=
    match bs with
    | [] => 0
    | BAgent _ :: rest => S (agents rest)
    | BValue _ :: rest => agents rest
    end.

  (* translate_new: inside the m nested receives, the reply of the agent
     binder of rank i is BoundVar(m - 1 - i). *)
  Fixpoint targets_from (m i : nat) (bs : list binder) : list target :=
    match bs with
    | [] => []
    | BAgent a :: rest => TTag a (m - 1 - i) :: targets_from m (S i) rest
    | BValue v :: rest => TVal v :: targets_from m i rest
    end.

  Definition targets (bs : list binder) : list target := targets_from (agents bs) 0 bs.

  (* The value of a target in an environment of the receives, whose index 0
     is the innermost binder. *)
  Definition eval_target (env : list V) (t : target) : option V :=
    match t with
    | TVal v => Some v
    | TTag a n => option_map (tag a) (nth_error env n)
    end.

  (* The receives bind the replies rs in the order of the agent binders, the
     outermost first, so the environment of the innermost body is rev rs. *)
  Definition resume_env (rs : list V) : list V := rev rs.

  (* The value of each binder after the resume: its bound name, or the
     agent-minted name of the reply of its rank. *)
  Fixpoint expected_from (i : nat) (bs : list binder) (rs : list V) : list (option V) :=
    match bs with
    | [] => []
    | BAgent a :: rest => option_map (tag a) (nth_error rs i) :: expected_from (S i) rest rs
    | BValue v :: rest => Some v :: expected_from i rest rs
    end.

  (* The match pushes its bindings in order (add_to_env of eval_match), and
     allocate_new_bindings pushes the names of a native new in binder order.
     So in both environments BoundVar(k - 1 - j) reads binder j. *)
  Definition match_env (values : list V) : list V := rev values.

  Lemma nth_error_rev_index :
    forall (l : list V) n, n < length l -> nth_error (rev l) n = nth_error l (length l - S n).
  Proof.
    intros l n Hn. destruct l as [| x rest]; [cbn in Hn; lia |].
    rewrite (nth_error_nth' (rev (x :: rest)) x) by (rewrite length_rev; exact Hn).
    rewrite (nth_error_nth' (x :: rest) x) by lia.
    rewrite rev_nth by exact Hn. reflexivity.
  Qed.

  Lemma targets_from_eval :
    forall bs m i rs,
      length rs = m -> i + agents bs = m ->
      map (eval_target (resume_env rs)) (targets_from m i bs) = expected_from i bs rs.
  Proof.
    induction bs as [| b rest IH]; intros m i rs Hlen Hcount; [reflexivity |].
    destruct b as [a | v]; cbn [targets_from expected_from map agents] in *.
    - cbn [eval_target]. unfold resume_env.
      rewrite nth_error_rev_index by (rewrite Hlen; lia).
      replace (length rs - S (m - 1 - i)) with i by lia.
      f_equal. apply IH; lia.
    - cbn [eval_target]. f_equal. apply IH; lia.
  Qed.

  Lemma expected_from_length : forall bs i rs, length (expected_from i bs rs) = length bs.
  Proof.
    induction bs as [| b rest IH]; intros i rs; [reflexivity |].
    destruct b; cbn [expected_from length]; rewrite IH; reflexivity.
  Qed.

  (* Headline 7 (P8 step 4, design v2 TN-3). The resume term binds every
     binder of the new, in its order. Inside the nested receives, which bind
     one reply per agent binder in the order of the agent binders, the target
     of each binder evaluates to its bound name, or to the agent-minted name
     of the reply of its rank. The match then pushes the values in binder
     order, so BoundVar(k - 1 - j) reads binder j, as in the environment of a
     native new. *)
  Theorem resume_binds_innermost :
    forall bs rs,
      length rs = agents bs ->
      map (eval_target (resume_env rs)) (targets bs) = expected_from 0 bs rs /\
      (forall values,
         map Some values = expected_from 0 bs rs ->
         forall j, j < length bs ->
           nth_error (match_env values) (length bs - 1 - j) = nth_error values j).
  Proof.
    intros bs rs Hlen. split.
    - unfold targets. apply targets_from_eval; [exact Hlen | lia].
    - intros values Hvalues j Hj.
      assert (Hl : length values = length bs).
      { rewrite <- (length_map Some values), Hvalues. apply expected_from_length. }
      unfold match_env. rewrite nth_error_rev_index by lia.
      f_equal. lia.
  Qed.
End Resume.

Print Assumptions offsets_partition.
Print Assumptions env_keys_disjoint_union.
Print Assumptions body_equivalence.
Print Assumptions guard_fold_sound.
Print Assumptions matcher_sound_complete.
Print Assumptions matcher_state_roundtrip.
Print Assumptions resume_binds_innermost.
