(* G1-4a (DR-122): the binder kinds of the linear cost estimate.

   Pass 1 of delta_sigma/estimate.rs gives every de Bruijn level a binder kind
   (design section 5.2) and checks rules X2 to X6 against kinds, never against
   values. This module models that analysis over de Bruijn terms and shows
   what each rule guarantees at run time.

   The model.
   - A term is a tree of parallel parts, a new with simple names and URIs, a
     send on an index with the free indices of its data, a receive on an index
     with its bind count, the free indices of its guard and its body, a match
     with one case and a fallback, and an if.
   - A kind is fresh, fresh hole, URN, injection, receive pattern or match
     pattern. A URN or an injection kind names its value. A new pushes its
     simple names first and then one level for each URI, in the order of
     util::allocate_new_bindings, so the innermost level is the last URI. A
     URI keeps its kind at every depth, also inside a receive body.
   - A run binds a fresh name to a fresh level, the URN value to a URN level,
     the injected value to an injection level, and any value to a pattern
     level.

   The theorems.
   1. [kinds_channels_fresh]: under the checks, every channel that a run
      sends or receives on holds a name (rule X2).
   2. [kinds_no_escape]: under the checks, no fresh level occurs in a guard,
      a match target or a condition, and data have no free level (rules X3
      and X4).
   3. [kinds_poison_free_introductions]: under the checks, the guard and the
      substituted body of every receive name no pattern level (rule X6). The
      receive's own levels stay variables inside its body, so a branch on its
      own value stays allowed.
   4. [full_environment_matches_runtime_free_levels]: the representative
      environment of the estimate, with a fresh name for each fresh level and
      the bound value for each URN or injection level, gives every value
      without a pattern level the size of every run.

   Negative controls.
   - [nc_uri_as_fresh_in_receive_body]: a kind machine that makes every level
     of a new inside a receive body a fresh hole accepts a URN channel there,
     and the run sends on the URN value, not on a name.
   - [nc_poison_flags_own_levels]: a poison check that reads the receive's own
     levels in its body rejects a branch on the received value, although its
     size does not depend on the run. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Bool Lia.
Import ListNotations.

Inductive kind : Type :=
| Fresh : kind
| FreshHole : kind
| Urn : nat -> kind
| Injection : nat -> kind
| RecvPat : kind
| MatchPat : kind.

Definition fresh_kind (k : kind) : bool :=
  match k with
  | Fresh | FreshHole => true
  | _ => false
  end.

Definition pattern_kind (k : kind) : bool :=
  match k with
  | RecvPat | MatchPat => true
  | _ => false
  end.

(** A URI bound by the URN map or by an injection, with the identity of its
    value. *)
Inductive uri : Type :=
| ByUrn : nat -> uri
| ByInjection : nat -> uri.

Definition uri_kind (binding : uri) : kind :=
  match binding with
  | ByUrn value => Urn value
  | ByInjection value => Injection value
  end.

Inductive tm : Type :=
| TNil : tm
| TPar : tm -> tm -> tm
| TNew : nat -> list uri -> tm -> tm
| TSend : nat -> list nat -> tm
| TRecv : nat -> nat -> list nat -> tm -> tm
| TMatch : list nat -> nat -> tm -> tm -> tm
| TIf : list nat -> tm -> tm -> tm.

(** The kinds of the levels that a new binds, innermost first: its URIs in
    reverse order, then its simple names. A simple name is fresh at a
    resolving position and a fresh hole inside a receive body. *)
Definition new_kinds (resolving : bool) (simple : nat) (uris : list uri) : list kind :=
  rev (map uri_kind uris) ++ repeat (if resolving then Fresh else FreshHole) simple.

(** The kind of free index [i] of a term under [shift] binders of its own. *)
Definition outer_kind (env : list kind) (shift i : nat) : option kind :=
  if i <? shift then None else nth_error env (i - shift).

Definition avoids (test : kind -> bool) (env : list kind) (shift : nat) (indices : list nat)
  : bool :=
  forallb (fun i => match outer_kind env shift i with
                    | Some k => negb (test k)
                    | None => true
                    end) indices.

Definition channel_ok (env : list kind) (chan : nat) : bool :=
  match nth_error env chan with
  | Some k => fresh_kind k
  | None => false
  end.

Definition closed (indices : list nat) : bool :=
  match indices with
  | [] => true
  | _ :: _ => false
  end.

Definition shift_down (n : nat) (indices : list nat) : list nat :=
  map (fun i => i - n) (filter (fun i => n <=? i) indices).

(** The free indices of a term, relative to its context. *)
Fixpoint fv (t : tm) : list nat :=
  match t with
  | TNil => []
  | TPar first second => fv first ++ fv second
  | TNew simple uris body => shift_down (simple + length uris) (fv body)
  | TSend chan data => chan :: data
  | TRecv chan binds guard body => chan :: guard ++ shift_down binds (fv body)
  | TMatch target binds case rest => target ++ shift_down binds (fv case) ++ fv rest
  | TIf cond yes no => cond ++ fv yes ++ fv no
  end.

(** The checks of pass 1. A receive body is substituted with the environment
    shifted by its bind count, so its own levels stay variables, and only an
    outer pattern level in it is poison. *)
Fixpoint check (env : list kind) (resolving : bool) (t : tm) : bool :=
  match t with
  | TNil => true
  | TPar first second => check env resolving first && check env resolving second
  | TNew simple uris body => check (new_kinds resolving simple uris ++ env) resolving body
  | TSend chan data => channel_ok env chan && closed data
  | TRecv chan binds guard body =>
      channel_ok env chan && avoids fresh_kind env 0 guard && avoids pattern_kind env 0 guard &&
      avoids pattern_kind env binds (fv body) &&
      check (repeat RecvPat binds ++ env) false body
  | TMatch target binds case rest =>
      avoids fresh_kind env 0 target && check (repeat MatchPat binds ++ env) resolving case &&
      check env resolving rest
  | TIf cond yes no => avoids fresh_kind env 0 cond && check env resolving yes && check env resolving no
  end.

(** * Runs *)

Inductive value : Type :=
| VName : nat -> value
| VUrn : nat -> value
| VInj : nat -> value
| VData : nat -> value.

Definition consistent_value (k : kind) (v : value) : Prop :=
  match k, v with
  | Fresh, VName _ | FreshHole, VName _ => True
  | Urn u, VUrn u' => u = u'
  | Injection i, VInj i' => i = i'
  | RecvPat, _ | MatchPat, _ => True
  | _, _ => False
  end.

Definition consistent (env : list kind) (vals : list value) : Prop :=
  Forall2 consistent_value env vals.

Lemma consistent_lookup : forall env vals i k,
  consistent env vals -> nth_error env i = Some k ->
  exists v, nth_error vals i = Some v /\ consistent_value k v.
Proof.
  intros env vals i k aligned. revert i.
  induction aligned as [| k' v' env' vals' head tail IH]; intros [| i] found; simpl in found.
  - discriminate.
  - discriminate.
  - inversion found; subst. exists v'. split; [reflexivity | exact head].
  - simpl. apply IH. exact found.
Qed.

Lemma consistent_app : forall env env' vals vals',
  consistent env vals -> consistent env' vals' -> consistent (env ++ env') (vals ++ vals').
Proof. intros. unfold consistent in *. apply Forall2_app; assumption. Qed.

Lemma consistent_patterns : forall k received,
  pattern_kind k = true -> consistent (repeat k (length received)) received.
Proof.
  intros k received pattern. unfold consistent.
  induction received as [| v received IH]; simpl; constructor.
  - destruct k; simpl in pattern; try discriminate; exact I.
  - exact IH.
Qed.

(** The channels of every send and receive that a run executes hold names.
    A new may bind any values consistent with its kinds, and a receive or a
    match case any values for its pattern levels. *)
Fixpoint channels_named (env : list kind) (resolving : bool) (vals : list value) (t : tm) : Prop :=
  match t with
  | TNil => True
  | TPar first second => channels_named env resolving vals first /\ channels_named env resolving vals second
  | TNew simple uris body =>
      forall ext, consistent (new_kinds resolving simple uris) ext ->
      channels_named (new_kinds resolving simple uris ++ env) resolving (ext ++ vals) body
  | TSend chan _ => exists n, nth_error vals chan = Some (VName n)
  | TRecv chan binds _ body =>
      (exists n, nth_error vals chan = Some (VName n)) /\
      forall received, length received = binds ->
      channels_named (repeat RecvPat binds ++ env) false (received ++ vals) body
  | TMatch _ binds case rest =>
      (forall matched, length matched = binds ->
       channels_named (repeat MatchPat binds ++ env) resolving (matched ++ vals) case) /\
      channels_named env resolving vals rest
  | TIf _ yes no => channels_named env resolving vals yes /\ channels_named env resolving vals no
  end.

Lemma channel_holds_a_name : forall env vals chan,
  consistent env vals -> channel_ok env chan = true -> exists n, nth_error vals chan = Some (VName n).
Proof.
  intros env vals chan aligned ok. unfold channel_ok in ok.
  destruct (nth_error env chan) as [k |] eqn:found; [| discriminate].
  destruct (consistent_lookup env vals chan k aligned found) as [v [at_chan value_kind]].
  destruct k; simpl in ok; try discriminate;
    destruct v as [n | n | n | n]; simpl in value_kind; try contradiction; exists n; exact at_chan.
Qed.

Theorem kinds_channels_fresh : forall t env resolving vals,
  consistent env vals -> check env resolving t = true -> channels_named env resolving vals t.
Proof.
  induction t as [| first IHfirst second IHsecond | simple uris body IH | chan data
                 | chan binds guard body IH | target binds case IHcase rest IHrest
                 | cond yes IHyes no IHno];
    intros env resolving vals aligned checked; simpl in checked |- *.
  - exact I.
  - apply andb_true_iff in checked. destruct checked as [first_ok second_ok].
    split; [apply IHfirst | apply IHsecond]; assumption.
  - intros ext ext_aligned. apply IH; [apply consistent_app; assumption | exact checked].
  - apply andb_true_iff in checked. destruct checked as [chan_ok _].
    exact (channel_holds_a_name env vals chan aligned chan_ok).
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [exact (channel_holds_a_name env vals chan aligned checked) |].
    intros received count. apply IH; [| assumption].
    apply consistent_app; [| exact aligned].
    rewrite <- count. apply consistent_patterns. reflexivity.
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split.
    + intros matched count. apply IHcase; [| assumption].
      apply consistent_app; [| exact aligned].
      rewrite <- count. apply consistent_patterns. reflexivity.
    + apply IHrest; assumption.
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [apply IHyes | apply IHno]; assumption.
Qed.

(** * Rules X3 and X4: no fresh level reaches an expression *)

Definition no_fresh_level (env : list kind) (shift : nat) (indices : list nat) : Prop :=
  forall i k, In i indices -> outer_kind env shift i = Some k -> fresh_kind k = false.

Fixpoint expressions_unescaped (env : list kind) (resolving : bool) (t : tm) : Prop :=
  match t with
  | TNil => True
  | TPar first second =>
      expressions_unescaped env resolving first /\ expressions_unescaped env resolving second
  | TNew simple uris body => expressions_unescaped (new_kinds resolving simple uris ++ env) resolving body
  | TSend _ data => data = []
  | TRecv _ binds guard body =>
      no_fresh_level env 0 guard /\ expressions_unescaped (repeat RecvPat binds ++ env) false body
  | TMatch target binds case rest =>
      no_fresh_level env 0 target /\
      expressions_unescaped (repeat MatchPat binds ++ env) resolving case /\
      expressions_unescaped env resolving rest
  | TIf cond yes no =>
      no_fresh_level env 0 cond /\ expressions_unescaped env resolving yes /\
      expressions_unescaped env resolving no
  end.

Lemma avoids_sound : forall test env shift indices,
  avoids test env shift indices = true ->
  forall i k, In i indices -> outer_kind env shift i = Some k -> test k = false.
Proof.
  intros test env shift indices avoided i k inside found.
  unfold avoids in avoided. rewrite forallb_forall in avoided.
  specialize (avoided i inside). rewrite found in avoided.
  destruct (test k); [discriminate | reflexivity].
Qed.

Theorem kinds_no_escape : forall t env resolving,
  check env resolving t = true -> expressions_unescaped env resolving t.
Proof.
  induction t as [| first IHfirst second IHsecond | simple uris body IH | chan data
                 | chan binds guard body IH | target binds case IHcase rest IHrest
                 | cond yes IHyes no IHno];
    intros env resolving checked; simpl in checked |- *.
  - exact I.
  - apply andb_true_iff in checked. destruct checked as [first_ok second_ok].
    split; [apply IHfirst | apply IHsecond]; assumption.
  - apply IH. exact checked.
  - apply andb_true_iff in checked. destruct checked as [_ data_closed].
    destruct data; [reflexivity | discriminate].
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [| apply IH; assumption].
    intros i k inside found. eapply avoids_sound; eassumption.
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [intros i k inside found; eapply avoids_sound; eassumption |].
    split; [apply IHcase | apply IHrest]; assumption.
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [intros i k inside found; eapply avoids_sound; eassumption |].
    split; [apply IHyes | apply IHno]; assumption.
Qed.

(** * Rule X6: introduced values name no pattern level *)

Definition no_pattern_level (env : list kind) (shift : nat) (indices : list nat) : Prop :=
  forall i k, In i indices -> outer_kind env shift i = Some k -> pattern_kind k = false.

Fixpoint introductions_poison_free (env : list kind) (resolving : bool) (t : tm) : Prop :=
  match t with
  | TNil => True
  | TPar first second =>
      introductions_poison_free env resolving first /\ introductions_poison_free env resolving second
  | TNew simple uris body =>
      introductions_poison_free (new_kinds resolving simple uris ++ env) resolving body
  | TSend _ data => data = []
  | TRecv _ binds guard body =>
      no_pattern_level env 0 guard /\ no_pattern_level env binds (fv body) /\
      introductions_poison_free (repeat RecvPat binds ++ env) false body
  | TMatch _ binds case rest =>
      introductions_poison_free (repeat MatchPat binds ++ env) resolving case /\
      introductions_poison_free env resolving rest
  | TIf _ yes no =>
      introductions_poison_free env resolving yes /\ introductions_poison_free env resolving no
  end.

Theorem kinds_poison_free_introductions : forall t env resolving,
  check env resolving t = true -> introductions_poison_free env resolving t.
Proof.
  induction t as [| first IHfirst second IHsecond | simple uris body IH | chan data
                 | chan binds guard body IH | target binds case IHcase rest IHrest
                 | cond yes IHyes no IHno];
    intros env resolving checked; simpl in checked |- *.
  - exact I.
  - apply andb_true_iff in checked. destruct checked as [first_ok second_ok].
    split; [apply IHfirst | apply IHsecond]; assumption.
  - apply IH. exact checked.
  - apply andb_true_iff in checked. destruct checked as [_ data_closed].
    destruct data; [reflexivity | discriminate].
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split.
    { intros i k inside found.
      exact (avoids_sound pattern_kind env 0 guard ltac:(assumption) i k inside found). }
    split.
    { intros i k inside found.
      exact (avoids_sound pattern_kind env binds (fv body) ltac:(assumption) i k inside found). }
    apply IH. assumption.
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [apply IHcase | apply IHrest]; assumption.
  - repeat (apply andb_true_iff in checked; destruct checked as [checked ?]).
    split; [apply IHyes | apply IHno]; assumption.
Qed.

(** * The representative environment has the sizes of every run *)

Section Sizes.

(** The prost length of a URN value and of an injected value. *)
Variable urn_size injection_size : nat -> nat.

(** A private name has a 32-byte identity, and so a fixed length. *)
Definition name_size : nat := 38.

Definition size_of (v : value) : nat :=
  match v with
  | VName _ => name_size
  | VUrn u => urn_size u
  | VInj i => injection_size i
  | VData d => d
  end.

(** The length that a substitution adds for the free indices of a value: an
    own level stays a variable of one byte, and an outer level takes the
    length of its value. *)
Definition substituted_size (vals : list value) (shift : nat) (indices : list nat) : nat :=
  fold_right (fun i total =>
                (if i <? shift then 1
                 else match nth_error vals (i - shift) with
                      | Some v => size_of v
                      | None => 1
                      end) + total) 0 indices.

(** The representative value of a kind: a name for a fresh kind, the bound
    value for a URN or an injection, and poison for a pattern kind. *)
Lemma substituted_size_cons : forall vals shift i indices,
  substituted_size vals shift (i :: indices) =
  (if i <? shift then 1
   else match nth_error vals (i - shift) with
        | Some v => size_of v
        | None => 1
        end) + substituted_size vals shift indices.
Proof. reflexivity. Qed.

Definition representative_value (k : kind) : value :=
  match k with
  | Fresh | FreshHole => VName 0
  | Urn u => VUrn u
  | Injection i => VInj i
  | RecvPat | MatchPat => VData 0
  end.

Definition representative (env : list kind) : list value := map representative_value env.

Definition sizes_agree (env : list kind) (first second : list value) : Prop :=
  forall i k, nth_error env i = Some k -> pattern_kind k = false ->
  option_map size_of (nth_error first i) = option_map size_of (nth_error second i).

Lemma representative_agrees : forall env vals,
  consistent env vals -> sizes_agree env (representative env) vals.
Proof.
  intros env vals aligned i k found not_pattern.
  unfold representative. rewrite nth_error_map, found.
  destruct (consistent_lookup env vals i k aligned found) as [v [at_i value_kind]].
  rewrite at_i. simpl. f_equal.
  destruct k; destruct v; simpl in value_kind, not_pattern |- *; try contradiction;
    try discriminate; subst; reflexivity.
Qed.

Lemma lengths_agree : forall env vals, consistent env vals -> length vals = length env.
Proof.
  intros env vals aligned. unfold consistent in aligned.
  induction aligned; simpl; [reflexivity | f_equal; assumption].
Qed.

Theorem full_environment_matches_runtime_free_levels : forall env vals shift indices,
  consistent env vals -> no_pattern_level env shift indices ->
  substituted_size (representative env) shift indices = substituted_size vals shift indices.
Proof.
  intros env vals shift indices aligned no_pattern.
  pose proof (representative_agrees env vals aligned) as agree.
  induction indices as [| i indices IH]; [reflexivity |].
  rewrite !substituted_size_cons.
  rewrite IH by (intros j k inside found; apply (no_pattern j k); [right; exact inside | exact found]).
  f_equal.
  destruct (i <? shift) eqn:own; [reflexivity |].
  destruct (nth_error env (i - shift)) as [k |] eqn:found.
  - assert (outer : outer_kind env shift i = Some k).
    { unfold outer_kind. rewrite own. exact found. }
    pose proof (no_pattern i k (or_introl eq_refl) outer) as not_pattern.
    pose proof (agree (i - shift) k found not_pattern) as same.
    destruct (nth_error (representative env) (i - shift)) as [rep |];
      destruct (nth_error vals (i - shift)) as [run |]; simpl in same; try discriminate;
      [injection same as equal; exact equal | reflexivity].
  - assert (rep_none : nth_error (representative env) (i - shift) = None).
    { unfold representative. rewrite nth_error_map, found. reflexivity. }
    assert (run_none : nth_error vals (i - shift) = None).
    { apply nth_error_None. apply nth_error_None in found.
      rewrite (lengths_agree env vals aligned). exact found. }
    rewrite rep_none, run_none. reflexivity.
Qed.

End Sizes.

(** * Negative controls *)

(** A kind machine that pushes a fresh hole for every level of a new inside a
    receive body, URIs included, as the funding resolver pushes holes there. *)
Definition hole_kinds (resolving : bool) (simple : nat) (uris : list uri) : list kind :=
  if resolving then new_kinds resolving simple uris
  else repeat FreshHole (simple + length uris).

Fixpoint check_holes (env : list kind) (resolving : bool) (t : tm) : bool :=
  match t with
  | TNil => true
  | TPar first second => check_holes env resolving first && check_holes env resolving second
  | TNew simple uris body => check_holes (hole_kinds resolving simple uris ++ env) resolving body
  | TSend chan data => channel_ok env chan && closed data
  | TRecv chan binds guard body =>
      channel_ok env chan && avoids fresh_kind env 0 guard && avoids pattern_kind env 0 guard &&
      avoids pattern_kind env binds (fv body) &&
      check_holes (repeat RecvPat binds ++ env) false body
  | TMatch target binds case rest =>
      avoids fresh_kind env 0 target && check_holes (repeat MatchPat binds ++ env) resolving case &&
      check_holes env resolving rest
  | TIf cond yes no =>
      avoids fresh_kind env 0 cond && check_holes env resolving yes && check_holes env resolving no
  end.

(** `new x in { x!(0) | for (_ <- x) { new out(rho:io:stdout) in { out!(0) } } }`,
    under the outer `new`: the receive is on level 0, and the inner send is on
    the URN level. *)
Definition urn_witness : tm := TRecv 0 0 [] (TNew 0 [ByUrn 7] (TSend 0 [])).

Theorem nc_uri_as_fresh_in_receive_body :
  check_holes [Fresh] true urn_witness = true /\
  check [Fresh] true urn_witness = false /\
  ~ channels_named [Fresh] true [VName 1] urn_witness.
Proof.
  split; [reflexivity |]. split; [reflexivity |].
  intros [_ body]. specialize (body [] eq_refl). simpl in body.
  assert (aligned : consistent [Urn 7] [VUrn 7]) by (repeat constructor).
  destruct (body [VUrn 7] aligned) as [n at_chan].
  simpl in at_chan. discriminate.
Qed.

(** A poison check that reads the free levels of a receive body in the body's
    own environment, so that it also flags the receive's own levels. *)
Fixpoint check_own_levels (env : list kind) (resolving : bool) (t : tm) : bool :=
  match t with
  | TNil => true
  | TPar first second => check_own_levels env resolving first && check_own_levels env resolving second
  | TNew simple uris body => check_own_levels (new_kinds resolving simple uris ++ env) resolving body
  | TSend chan data => channel_ok env chan && closed data
  | TRecv chan binds guard body =>
      channel_ok env chan && avoids fresh_kind env 0 guard && avoids pattern_kind env 0 guard &&
      avoids pattern_kind (repeat RecvPat binds ++ env) 0 (fv body) &&
      check_own_levels (repeat RecvPat binds ++ env) false body
  | TMatch target binds case rest =>
      avoids fresh_kind env 0 target &&
      check_own_levels (repeat MatchPat binds ++ env) resolving case &&
      check_own_levels env resolving rest
  | TIf cond yes no =>
      avoids fresh_kind env 0 cond && check_own_levels env resolving yes &&
      check_own_levels env resolving no
  end.

(** `for (@n <- x) { if (n > 3) { Nil } else { Nil } }` under the `new` of
    `x` (design section 10.2): the body branches on the receive's own level. *)
Definition branch_witness : tm := TRecv 0 1 [] (TIf [0] TNil TNil).

Theorem nc_poison_flags_own_levels :
  check [Fresh] true branch_witness = true /\
  check_own_levels [Fresh] true branch_witness = false /\
  forall urn_size injection_size vals, consistent [Fresh] vals ->
    substituted_size urn_size injection_size (representative [Fresh]) 1 (fv (TIf [0] TNil TNil)) =
    substituted_size urn_size injection_size vals 1 (fv (TIf [0] TNil TNil)).
Proof.
  split; [reflexivity |]. split; [reflexivity |].
  intros urn_size injection_size vals aligned. reflexivity.
Qed.

Print Assumptions consistent_lookup.
Print Assumptions kinds_channels_fresh.
Print Assumptions kinds_no_escape.
Print Assumptions kinds_poison_free_introductions.
Print Assumptions representative_agrees.
Print Assumptions full_environment_matches_runtime_free_levels.
Print Assumptions nc_uri_as_fresh_in_receive_body.
Print Assumptions nc_poison_flags_own_levels.
