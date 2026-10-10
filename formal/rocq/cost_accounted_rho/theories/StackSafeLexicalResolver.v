(* G1-2 (DR-67): the funding resolver machine refines the recursive resolver.

   The pre-execution funding check resolves the names that a deploy binds
   with `new` before it analyzes the deploy's demand (lexical.rs). The
   resolver of HEAD 17e07307f was recursive: for each `new` it allocated the
   names, substituted them into the whole body, and then resolved the body.
   That work is quadratic in the nesting depth, and its stack grows with the
   depth. G1-2 replaces it with a machine that walks the term once, keeps one
   stack of binders, and rewrites only the signatures.

   Sections:
   1. Terms. A node is a list of items in the reducer's term order. Each item
      is one term: a send (its data are walked but not resolved), a receive
      (its bind signatures, its binder count and its body), a `new` (an
      allocation spec and a body), a match (cases with binder counts), a list
      of branches (an `if` or a bundle), a signed term, a token stack, or an
      expression term with no walked child.
   2. Environments and substitution. [subst] is the effect of the reducer's
      substitution on the walked positions and the signatures.
   3. The recursive resolver of HEAD ([r1]) and the one-pass resolver with an
      environment ([r2]). [resolver_linearization] proves that the one pass
      equals the substitution-based recursion on every term: substituting a
      `new`'s names into its body and resolving the result is the same as
      resolving the body with the names in the environment.
   4. The machine ([mrun]): a work stack of tasks, a scope stack of binders and
      a value stack of rebuilt parts. [machine_runs_its_denotation] proves that
      each step keeps the denotation of the work stack, so
      [machine_refines_one_pass_resolver] and
      [machine_refines_recursive_resolver] follow. This is the
      compile_run_equivalence pattern of StackSafePDA.v (feature/mettail) with
      a scope stack and allocation effects.
   5. The machine takes at most six steps per syntax node
      ([resolver_steps_are_linear]).
   6. Negative controls: a machine that skips the scope pop after a `new`
      leaks its binding to a later sibling, and a machine that visits the items
      of a node in reverse order reports another first error.

   The model abstracts the randomness ([split_rand], [allocate]) and the values of
   names. It does not model the copy of the fields that the resolver does not
   walk, and it does not model expressions. The Rust tests cover the
   dequotation of a resolved name. No axiom is used. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
Import ListNotations.

(** * 1. Terms *)

Definition name : Type := nat.

(** A cost signature: a bound level, a resolved name, or a ground signature. *)
Inductive signature : Type :=
| SBound : nat -> signature
| SName : name -> signature
| SGround : nat -> signature.

Inductive proc : Type :=
| Proc : items -> proc
with items : Type :=
| INil : items
| ICons : item -> items -> items
with item : Type :=
| ISend : procs -> item
| IReceive : list signature -> nat -> proc -> item
| INew : nat -> proc -> item
| IMatch : cases -> item
| IBranches : procs -> item
| ISigned : signature -> proc -> item
| IStack : list signature -> item
| IExpr : item
with procs : Type :=
| PNil : procs
| PCons : proc -> procs -> procs
with cases : Type :=
| CNil : cases
| CCons : nat -> proc -> cases -> cases.

Scheme proc_mut := Induction for proc Sort Prop
  with items_mut := Induction for items Sort Prop
  with item_mut := Induction for item Sort Prop
  with procs_mut := Induction for procs Sort Prop
  with cases_mut := Induction for cases Sort Prop.

Combined Scheme resolver_mutind from proc_mut, items_mut, item_mut, procs_mut, cases_mut.

Fixpoint item_count (children : items) : nat :=
  match children with
  | INil => 0
  | ICons _ rest => S (item_count rest)
  end.

Fixpoint list_of_items (children : items) : list item :=
  match children with
  | INil => []
  | ICons child rest => child :: list_of_items rest
  end.

Fixpoint items_of_list (children : list item) : items :=
  match children with
  | [] => INil
  | child :: rest => ICons child (items_of_list rest)
  end.

Fixpoint list_of_procs (ps : procs) : list proc :=
  match ps with
  | PNil => []
  | PCons q rest => q :: list_of_procs rest
  end.

Fixpoint procs_of_list (ps : list proc) : procs :=
  match ps with
  | [] => PNil
  | q :: rest => PCons q (procs_of_list rest)
  end.

Fixpoint case_counts (cs : cases) : list nat :=
  match cs with
  | CNil => []
  | CCons count _ rest => count :: case_counts rest
  end.

Fixpoint case_bodies (cs : cases) : list proc :=
  match cs with
  | CNil => []
  | CCons _ body rest => body :: case_bodies rest
  end.

Fixpoint cases_of_lists (counts : list nat) (bodies : list proc) : cases :=
  match counts, bodies with
  | count :: counts', body :: bodies' => CCons count body (cases_of_lists counts' bodies')
  | _, _ => CNil
  end.

Lemma items_round_trip : forall children, items_of_list (list_of_items children) = children.
Proof. induction children as [| child rest IH]; simpl; congruence. Qed.

Lemma procs_round_trip : forall ps, procs_of_list (list_of_procs ps) = ps.
Proof. induction ps as [| q rest IH]; simpl; congruence. Qed.

Lemma cases_round_trip : forall cs, cases_of_lists (case_counts cs) (case_bodies cs) = cs.
Proof. induction cs as [| count body rest IH]; simpl; congruence. Qed.

Lemma length_list_of_items : forall children, length (list_of_items children) = item_count children.
Proof. induction children as [| child rest IH]; simpl; congruence. Qed.

(** The size of a term: one per node and one per item. *)
Fixpoint size (p : proc) : nat :=
  match p with
  | Proc children => S (size_items children)
  end
with size_items (children : items) : nat :=
  match children with
  | INil => 0
  | ICons child rest => size_item child + size_items rest
  end
with size_item (child : item) : nat :=
  match child with
  | ISend data => S (size_procs data)
  | IReceive _ _ body => S (size body)
  | INew _ body => S (size body)
  | IMatch alternatives => S (size_cases alternatives)
  | IBranches bodies => S (size_procs bodies)
  | ISigned _ body => S (size body)
  | IStack _ => 1
  | IExpr => 1
  end
with size_procs (ps : procs) : nat :=
  match ps with
  | PNil => 0
  | PCons q rest => size q + size_procs rest
  end
with size_cases (cs : cases) : nat :=
  match cs with
  | CNil => 0
  | CCons _ body rest => size body + size_cases rest
  end.

(** * 2. Environments and substitution *)

(** An environment maps a de Bruijn index to the name that a resolved binder
    holds. [None] is a hole: a binder that the resolver leaves unresolved. *)
Definition env : Type := nat -> option name.

Definition no_names : env := fun _ => None.

Definition holes (count : nat) (e : env) : env :=
  fun index => if index <? count then None else e (index - count).

(** The names of a `new`, listed by de Bruijn index (index 0 first). *)
Definition bind_names (values : list name) (e : env) : env :=
  fun index => if index <? length values then nth_error values index else e (index - length values).

Definition overlay (first second : env) : env :=
  fun index => match first index with
               | Some value => Some value
               | None => second index
               end.

Definition same_env (e e' : env) : Prop := forall index, e index = e' index.

Lemma holes_same : forall count e e', same_env e e' -> same_env (holes count e) (holes count e').
Proof.
  intros count e e' same index. unfold holes. destruct (index <? count); [reflexivity | apply same].
Qed.

Lemma bind_names_same : forall values e e',
  same_env e e' -> same_env (bind_names values e) (bind_names values e').
Proof.
  intros values e e' same index. unfold bind_names.
  destruct (index <? length values); [reflexivity | apply same].
Qed.

Lemma overlay_holes : forall count first second,
  same_env (overlay (holes count first) (holes count second)) (holes count (overlay first second)).
Proof.
  intros count first second index. unfold overlay, holes.
  destruct (index <? count); reflexivity.
Qed.

Lemma holes_no_names : forall count, same_env (holes count no_names) no_names.
Proof.
  intros count index. unfold holes, no_names. destruct (index <? count); reflexivity.
Qed.

(** Filling the binders of a `new` after the outer substitution left them as
    holes gives the environment with the names in front. *)
Lemma overlay_bind_names : forall values e,
  same_env (overlay (holes (length values) e) (bind_names values no_names)) (bind_names values e).
Proof.
  intros values e index. unfold overlay, holes, bind_names, no_names.
  destruct (index <? length values); [reflexivity |].
  destruct (e (index - length values)); reflexivity.
Qed.

Definition subst_sig (e : env) (s : signature) : signature :=
  match s with
  | SBound index =>
      match e index with
      | Some value => SName value
      | None => SBound index
      end
  | _ => s
  end.

Lemma subst_sig_same : forall e e' s, same_env e e' -> subst_sig e s = subst_sig e' s.
Proof.
  intros e e' [index | value | ground] same; simpl; try reflexivity.
  rewrite same. reflexivity.
Qed.

Lemma subst_sig_compose : forall first second s,
  subst_sig second (subst_sig first s) = subst_sig (overlay first second) s.
Proof.
  intros first second [index | value | ground]; simpl; try reflexivity.
  unfold overlay. destruct (first index); reflexivity.
Qed.

Lemma subst_sig_no_names : forall s, subst_sig no_names s = s.
Proof. intros [index | value | ground]; reflexivity. Qed.

Lemma map_subst_sig_same : forall e e' sigs,
  same_env e e' -> map (subst_sig e) sigs = map (subst_sig e') sigs.
Proof.
  intros e e' sigs same. apply map_ext. intro s. apply subst_sig_same. exact same.
Qed.

Lemma map_subst_sig_compose : forall first second sigs,
  map (subst_sig second) (map (subst_sig first) sigs) = map (subst_sig (overlay first second)) sigs.
Proof.
  intros first second sigs. rewrite map_map. apply map_ext. intro s. apply subst_sig_compose.
Qed.

Lemma map_subst_sig_no_names : forall sigs, map (subst_sig no_names) sigs = sigs.
Proof.
  induction sigs as [| s rest IH]; simpl; [reflexivity |].
  rewrite subst_sig_no_names, IH. reflexivity.
Qed.

(** The resolver's parameters: the split of the randomness by term index and
    term count, the allocation of a `new`'s names (with the randomness that
    remains, or a failure such as an unbound URN), the binder count of a
    `new`, and whether receive bodies are resolved (G1-3 turns this off). *)
Record params (Seed : Type) : Type := {
  split_rand : Seed -> nat -> nat -> Seed;
  allocate : nat -> Seed -> option (list name * Seed);
  binders : nat -> nat;
  resolve_receive_bodies : bool
}.

Arguments split_rand {Seed} _ _ _ _.
Arguments allocate {Seed} _ _ _.
Arguments binders {Seed} _ _.
Arguments resolve_receive_bodies {Seed} _.

Inductive outcome (A : Type) : Type :=
| Ok : A -> outcome A
| Err : nat -> outcome A
| Stuck : outcome A.

Arguments Ok {A} _.
Arguments Err {A} _.
Arguments Stuck {A}.

Definition bind {A B : Type} (result : outcome A) (next : A -> outcome B) : outcome B :=
  match result with
  | Ok value => next value
  | Err code => Err code
  | Stuck => Stuck
  end.

Lemma bind_assoc : forall {A B C : Type} (result : outcome A) (first : A -> outcome B)
    (second : B -> outcome C),
  bind (bind result first) second = bind result (fun value => bind (first value) second).
Proof. intros A B C [value | code |] first second; reflexivity. Qed.

Lemma bind_ext : forall {A B : Type} (result : outcome A) (first second : A -> outcome B),
  (forall value, first value = second value) -> bind result first = bind result second.
Proof. intros A B [value | code |] first second same; simpl; auto. Qed.

(** A part of a term that the machine has rebuilt. *)
Inductive value : Type :=
| VProc : proc -> value
| VItem : item -> value
| VSig : signature -> value.

(** The tasks of the machine's work stack. *)
Inductive task (Seed : Type) : Type :=
| TVisit : proc -> option Seed -> task Seed
| TItem : item -> option Seed -> task Seed
| TEnter : nat -> proc -> option Seed -> task Seed
| TSigs : list signature -> task Seed
| THoles : nat -> task Seed
| TNames : list name -> task Seed
| TPop : nat -> task Seed
| TBuildNode : nat -> task Seed
| TBuildSend : nat -> task Seed
| TBuildReceive : nat -> nat -> task Seed
| TBuildNew : nat -> task Seed
| TBuildMatch : list nat -> task Seed
| TBuildBranches : nat -> task Seed
| TBuildSigned : task Seed
| TBuildStack : nat -> task Seed
| TBuildExpr : task Seed.

Arguments TVisit {Seed} _ _.
Arguments TItem {Seed} _ _.
Arguments TEnter {Seed} _ _ _.
Arguments TSigs {Seed} _.
Arguments THoles {Seed} _.
Arguments TNames {Seed} _.
Arguments TPop {Seed} _.
Arguments TBuildNode {Seed} _.
Arguments TBuildSend {Seed} _.
Arguments TBuildReceive {Seed} _ _.
Arguments TBuildNew {Seed} _.
Arguments TBuildMatch {Seed} _.
Arguments TBuildBranches {Seed} _.
Arguments TBuildSigned {Seed}.
Arguments TBuildStack {Seed} _.
Arguments TBuildExpr {Seed}.

Definition state : Type := (list (option name) * list value)%type.

(** The environment of a scope stack: index 0 is the innermost binder. *)
Definition env_of (scope : list (option name)) : env :=
  fun index => match nth_error scope index with
               | Some (Some value) => Some value
               | _ => None
               end.

Lemma env_of_holes : forall count scope,
  same_env (env_of (repeat None count ++ scope)) (holes count (env_of scope)).
Proof.
  intros count scope index. unfold env_of, holes.
  destruct (index <? count) eqn:inside.
  - apply Nat.ltb_lt in inside.
    rewrite nth_error_app1 by (rewrite repeat_length; exact inside).
    rewrite nth_error_repeat by exact inside. reflexivity.
  - apply Nat.ltb_ge in inside.
    rewrite nth_error_app2 by (rewrite repeat_length; exact inside).
    rewrite repeat_length. reflexivity.
Qed.

Lemma env_of_names : forall values scope,
  same_env (env_of (map Some values ++ scope)) (bind_names values (env_of scope)).
Proof.
  intros values scope index. unfold env_of, bind_names.
  destruct (index <? length values) eqn:inside.
  - apply Nat.ltb_lt in inside.
    rewrite nth_error_app1 by (rewrite length_map; exact inside).
    rewrite nth_error_map.
    destruct (nth_error values index); reflexivity.
  - apply Nat.ltb_ge in inside.
    rewrite nth_error_app2 by (rewrite length_map; exact inside).
    rewrite length_map. reflexivity.
Qed.

Lemma env_of_empty : same_env (env_of []) no_names.
Proof. intros [| index]; reflexivity. Qed.

Lemma skipn_repeat_app : forall {A : Type} count (filler : A) rest,
  skipn count (repeat filler count ++ rest) = rest.
Proof. induction count as [| count IH]; intros filler rest; simpl; auto. Qed.

Lemma skipn_map_app : forall {A B : Type} (wrap : A -> B) prefix rest,
  skipn (length prefix) (map wrap prefix ++ rest) = rest.
Proof. induction prefix as [| head prefix IH]; intros rest; simpl; auto. Qed.

(** Popping values: the [count] values on top of the stack, top first. *)
Fixpoint take_values (count : nat) (values : list value) : option (list value * list value) :=
  match count with
  | 0 => Some ([], values)
  | S count' =>
      match values with
      | [] => None
      | top :: rest =>
          match take_values count' rest with
          | Some (taken, remaining) => Some (top :: taken, remaining)
          | None => None
          end
      end
  end.

Lemma take_values_app : forall prefix rest,
  take_values (length prefix) (prefix ++ rest) = Some (prefix, rest).
Proof.
  induction prefix as [| top prefix IH]; intros rest; simpl; [reflexivity |].
  rewrite IH. reflexivity.
Qed.

Fixpoint as_procs (values : list value) : option (list proc) :=
  match values with
  | [] => Some []
  | VProc p :: rest => option_map (cons p) (as_procs rest)
  | _ :: _ => None
  end.

Fixpoint as_items (values : list value) : option (list item) :=
  match values with
  | [] => Some []
  | VItem child :: rest => option_map (cons child) (as_items rest)
  | _ :: _ => None
  end.

Fixpoint as_sigs (values : list value) : option (list signature) :=
  match values with
  | [] => Some []
  | VSig s :: rest => option_map (cons s) (as_sigs rest)
  | _ :: _ => None
  end.

Lemma as_procs_map : forall ps, as_procs (map VProc ps) = Some ps.
Proof. induction ps as [| q rest IH]; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

Lemma as_items_map : forall children, as_items (map VItem children) = Some children.
Proof.
  induction children as [| child rest IH]; simpl; [reflexivity | rewrite IH; reflexivity].
Qed.

Lemma as_sigs_map : forall sigs, as_sigs (map VSig sigs) = Some sigs.
Proof. induction sigs as [| s rest IH]; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

(** Popping the [length l] values that a sequence of pushes left on top. *)
Lemma take_pushed : forall {A : Type} (wrap : A -> value) (pushed : list A) rest,
  take_values (length pushed) (rev (map wrap pushed) ++ rest) = Some (rev (map wrap pushed), rest).
Proof.
  intros A wrap pushed rest.
  replace (length pushed) with (length (rev (map wrap pushed)))
    by (rewrite length_rev, length_map; reflexivity).
  apply take_values_app.
Qed.

(** * 3. The recursive resolver and the one-pass resolver *)

Section Resolver.

Context {Seed : Type} (P : params Seed).

Fixpoint subst (e : env) (p : proc) {struct p} : proc :=
  match p with
  | Proc children => Proc (subst_items e children)
  end
with subst_items (e : env) (children : items) {struct children} : items :=
  match children with
  | INil => INil
  | ICons child rest => ICons (subst_item e child) (subst_items e rest)
  end
with subst_item (e : env) (child : item) {struct child} : item :=
  match child with
  | ISend data => ISend (subst_procs e data)
  | IReceive sigs count body => IReceive (map (subst_sig e) sigs) count (subst (holes count e) body)
  | INew spec body => INew spec (subst (holes (binders P spec) e) body)
  | IMatch alternatives => IMatch (subst_cases e alternatives)
  | IBranches bodies => IBranches (subst_procs e bodies)
  | ISigned s body => ISigned (subst_sig e s) (subst e body)
  | IStack cells => IStack (map (subst_sig e) cells)
  | IExpr => IExpr
  end
with subst_procs (e : env) (ps : procs) {struct ps} : procs :=
  match ps with
  | PNil => PNil
  | PCons q rest => PCons (subst e q) (subst_procs e rest)
  end
with subst_cases (e : env) (cs : cases) {struct cs} : cases :=
  match cs with
  | CNil => CNil
  | CCons count body rest => CCons count (subst (holes count e) body) (subst_cases e rest)
  end.

Theorem subst_same :
  (forall p e e', same_env e e' -> subst e p = subst e' p) /\
  (forall children e e', same_env e e' -> subst_items e children = subst_items e' children) /\
  (forall child e e', same_env e e' -> subst_item e child = subst_item e' child) /\
  (forall ps e e', same_env e e' -> subst_procs e ps = subst_procs e' ps) /\
  (forall cs e e', same_env e e' -> subst_cases e cs = subst_cases e' cs).
Proof.
  apply resolver_mutind.
  - intros children IH e e' same. simpl. rewrite (IH e e' same). reflexivity.
  - intros e e' same. reflexivity.
  - intros child IHchild rest IHrest e e' same. simpl.
    rewrite (IHchild e e' same), (IHrest e e' same). reflexivity.
  - intros data IH e e' same. simpl. rewrite (IH e e' same). reflexivity.
  - intros sigs count body IH e e' same. simpl.
    rewrite (map_subst_sig_same e e' sigs same).
    rewrite (IH (holes count e) (holes count e') (holes_same count e e' same)). reflexivity.
  - intros spec body IH e e' same. simpl.
    rewrite (IH (holes (binders P spec) e) (holes (binders P spec) e')
                (holes_same (binders P spec) e e' same)).
    reflexivity.
  - intros alternatives IH e e' same. simpl. rewrite (IH e e' same). reflexivity.
  - intros bodies IH e e' same. simpl. rewrite (IH e e' same). reflexivity.
  - intros s body IH e e' same. simpl.
    rewrite (subst_sig_same e e' s same), (IH e e' same). reflexivity.
  - intros cells e e' same. simpl. rewrite (map_subst_sig_same e e' cells same). reflexivity.
  - intros e e' same. reflexivity.
  - intros e e' same. reflexivity.
  - intros q IHq rest IHrest e e' same. simpl.
    rewrite (IHq e e' same), (IHrest e e' same). reflexivity.
  - intros e e' same. reflexivity.
  - intros count body IHbody rest IHrest e e' same. simpl.
    rewrite (IHbody (holes count e) (holes count e') (holes_same count e e' same)).
    rewrite (IHrest e e' same). reflexivity.
Qed.

(** A substitution never rewrites a resolved name again, so two substitutions
    compose into one. *)
Theorem subst_compose :
  (forall p first second, subst second (subst first p) = subst (overlay first second) p) /\
  (forall children first second,
      subst_items second (subst_items first children) = subst_items (overlay first second) children) /\
  (forall child first second,
      subst_item second (subst_item first child) = subst_item (overlay first second) child) /\
  (forall ps first second,
      subst_procs second (subst_procs first ps) = subst_procs (overlay first second) ps) /\
  (forall cs first second,
      subst_cases second (subst_cases first cs) = subst_cases (overlay first second) cs).
Proof.
  apply resolver_mutind.
  - intros children IH first second. simpl. rewrite IH. reflexivity.
  - intros first second. reflexivity.
  - intros child IHchild rest IHrest first second. simpl.
    rewrite IHchild, IHrest. reflexivity.
  - intros data IH first second. simpl. rewrite IH. reflexivity.
  - intros sigs count body IH first second. simpl.
    rewrite map_subst_sig_compose, IH.
    rewrite (proj1 subst_same body _ (holes count (overlay first second)) (overlay_holes count first second)).
    reflexivity.
  - intros spec body IH first second. simpl. rewrite IH.
    rewrite (proj1 subst_same body _ (holes (binders P spec) (overlay first second))
               (overlay_holes (binders P spec) first second)).
    reflexivity.
  - intros alternatives IH first second. simpl. rewrite IH. reflexivity.
  - intros bodies IH first second. simpl. rewrite IH. reflexivity.
  - intros s body IH first second. simpl. rewrite subst_sig_compose, IH. reflexivity.
  - intros cells first second. simpl. rewrite map_subst_sig_compose. reflexivity.
  - intros first second. reflexivity.
  - intros first second. reflexivity.
  - intros q IHq rest IHrest first second. simpl. rewrite IHq, IHrest. reflexivity.
  - intros first second. reflexivity.
  - intros count body IHbody rest IHrest first second. simpl.
    rewrite IHbody, IHrest.
    rewrite (proj1 subst_same body _ (holes count (overlay first second)) (overlay_holes count first second)).
    reflexivity.
Qed.

Theorem subst_no_names :
  (forall p, subst no_names p = p) /\
  (forall children, subst_items no_names children = children) /\
  (forall child, subst_item no_names child = child) /\
  (forall ps, subst_procs no_names ps = ps) /\
  (forall cs, subst_cases no_names cs = cs).
Proof.
  apply resolver_mutind.
  - intros children IH. simpl. rewrite IH. reflexivity.
  - reflexivity.
  - intros child IHchild rest IHrest. simpl. rewrite IHchild, IHrest. reflexivity.
  - intros data IH. simpl. rewrite IH. reflexivity.
  - intros sigs count body IH. simpl. rewrite map_subst_sig_no_names.
    rewrite (proj1 subst_same body (holes count no_names) no_names (holes_no_names count)), IH.
    reflexivity.
  - intros spec body IH. simpl.
    rewrite (proj1 subst_same body (holes (binders P spec) no_names) no_names
               (holes_no_names (binders P spec))), IH.
    reflexivity.
  - intros alternatives IH. simpl. rewrite IH. reflexivity.
  - intros bodies IH. simpl. rewrite IH. reflexivity.
  - intros s body IH. simpl. rewrite subst_sig_no_names, IH. reflexivity.
  - intros cells. simpl. rewrite map_subst_sig_no_names. reflexivity.
  - reflexivity.
  - reflexivity.
  - intros q IHq rest IHrest. simpl. rewrite IHq, IHrest. reflexivity.
  - reflexivity.
  - intros count body IHbody rest IHrest. simpl.
    rewrite (proj1 subst_same body (holes count no_names) no_names (holes_no_names count)).
    rewrite IHbody, IHrest. reflexivity.
Qed.

Lemma item_count_subst : forall e children, item_count (subst_items e children) = item_count children.
Proof. intros e children. induction children as [| child rest IH]; simpl; congruence. Qed.

(** The split of a node's randomness that the reducer gives the term at
    [index] of [count] terms ([util::evaluation_random]). *)
Definition term_seed (seed : option Seed) (index count : nat) : option Seed :=
  match seed with
  | Some s => Some (split_rand P s index count)
  | None => None
  end.

Definition receive_seed (ts : option Seed) : option Seed :=
  if resolve_receive_bodies P then ts else None.

(** One level of HEAD's recursion. [recurse] resolves a child node. A `new`
    allocates its names, substitutes them into its body, and resolves the
    substituted body. Sends, token stacks and expressions are not walked. *)
Fixpoint r1_procs_step (recurse : Seed -> proc -> outcome proc) (seed : Seed) (ps : procs)
    {struct ps} : outcome procs :=
  match ps with
  | PNil => Ok PNil
  | PCons q rest =>
      bind (recurse seed q)
        (fun q' => bind (r1_procs_step recurse seed rest) (fun rest' => Ok (PCons q' rest')))
  end.

Fixpoint r1_cases_step (recurse : Seed -> proc -> outcome proc) (seed : Seed) (cs : cases)
    {struct cs} : outcome cases :=
  match cs with
  | CNil => Ok CNil
  | CCons count body rest =>
      bind (recurse seed body)
        (fun body' =>
           bind (r1_cases_step recurse seed rest) (fun rest' => Ok (CCons count body' rest')))
  end.

Definition r1_item_step (recurse : Seed -> proc -> outcome proc) (ts : Seed) (child : item)
    : outcome item :=
  match child with
  | ISend data => Ok (ISend data)
  | IReceive sigs count body =>
      bind (recurse ts body) (fun body' => Ok (IReceive sigs count body'))
  | INew spec body =>
      match allocate P spec ts with
      | Some (values, rest_seed) =>
          bind (recurse rest_seed (subst (bind_names values no_names) body))
            (fun body' => Ok (INew spec body'))
      | None => Err spec
      end
  | IMatch alternatives =>
      bind (r1_cases_step recurse ts alternatives) (fun alternatives' => Ok (IMatch alternatives'))
  | IBranches bodies => bind (r1_procs_step recurse ts bodies) (fun bodies' => Ok (IBranches bodies'))
  | ISigned s body => bind (recurse ts body) (fun body' => Ok (ISigned s body'))
  | IStack cells => Ok (IStack cells)
  | IExpr => Ok IExpr
  end.

Fixpoint r1_items_step (recurse : Seed -> proc -> outcome proc) (seed : Seed) (count index : nat)
    (children : items) {struct children} : outcome items :=
  match children with
  | INil => Ok INil
  | ICons child rest =>
      bind (r1_item_step recurse (split_rand P seed index count) child)
        (fun child' =>
           bind (r1_items_step recurse seed count (S index) rest)
             (fun rest' => Ok (ICons child' rest')))
  end.

(** HEAD's recursion re-substitutes a body before it resolves it, so it is not
    structural. The fuel bounds the nesting depth. *)
Fixpoint r1 (fuel : nat) (seed : Seed) (p : proc) : outcome proc :=
  match fuel with
  | 0 => Stuck
  | S fuel' =>
      match p with
      | Proc children =>
          bind (r1_items_step (r1 fuel') seed (item_count children) 0 children)
            (fun children' => Ok (Proc children'))
      end
  end.

(** The one-pass resolver: the binders of the term so far are in [e]. A `new`
    in a resolving position puts its names in front of the environment; every
    other binder adds holes. *)
Fixpoint r2 (e : env) (seed : option Seed) (p : proc) {struct p} : outcome proc :=
  match p with
  | Proc children =>
      bind (r2_items e seed (item_count children) 0 children) (fun children' => Ok (Proc children'))
  end
with r2_items (e : env) (seed : option Seed) (count index : nat) (children : items)
    {struct children} : outcome items :=
  match children with
  | INil => Ok INil
  | ICons child rest =>
      bind (r2_item e (term_seed seed index count) child)
        (fun child' =>
           bind (r2_items e seed count (S index) rest) (fun rest' => Ok (ICons child' rest')))
  end
with r2_item (e : env) (ts : option Seed) (child : item) {struct child} : outcome item :=
  match child with
  | ISend data => bind (r2_procs e None data) (fun data' => Ok (ISend data'))
  | IReceive sigs count body =>
      bind (r2 (holes count e) (receive_seed ts) body)
        (fun body' => Ok (IReceive (map (subst_sig e) sigs) count body'))
  | INew spec body =>
      match ts with
      | Some s =>
          match allocate P spec s with
          | Some (values, rest_seed) =>
              bind (r2 (bind_names values e) (Some rest_seed) body)
                (fun body' => Ok (INew spec body'))
          | None => Err spec
          end
      | None =>
          bind (r2 (holes (binders P spec) e) None body) (fun body' => Ok (INew spec body'))
      end
  | IMatch alternatives => bind (r2_cases e ts alternatives) (fun alternatives' => Ok (IMatch alternatives'))
  | IBranches bodies => bind (r2_procs e ts bodies) (fun bodies' => Ok (IBranches bodies'))
  | ISigned s body => bind (r2 e ts body) (fun body' => Ok (ISigned (subst_sig e s) body'))
  | IStack cells => Ok (IStack (map (subst_sig e) cells))
  | IExpr => Ok IExpr
  end
with r2_procs (e : env) (seed : option Seed) (ps : procs) {struct ps} : outcome procs :=
  match ps with
  | PNil => Ok PNil
  | PCons q rest =>
      bind (r2 e seed q)
        (fun q' => bind (r2_procs e seed rest) (fun rest' => Ok (PCons q' rest')))
  end
with r2_cases (e : env) (seed : option Seed) (cs : cases) {struct cs} : outcome cases :=
  match cs with
  | CNil => Ok CNil
  | CCons count body rest =>
      bind (r2 (holes count e) seed body)
        (fun body' => bind (r2_cases e seed rest) (fun rest' => Ok (CCons count body' rest')))
  end.

Theorem r2_same :
  (forall p e e' seed, same_env e e' -> r2 e seed p = r2 e' seed p) /\
  (forall children e e' seed count index,
      same_env e e' -> r2_items e seed count index children = r2_items e' seed count index children) /\
  (forall child e e' ts, same_env e e' -> r2_item e ts child = r2_item e' ts child) /\
  (forall ps e e' seed, same_env e e' -> r2_procs e seed ps = r2_procs e' seed ps) /\
  (forall cs e e' seed, same_env e e' -> r2_cases e seed cs = r2_cases e' seed cs).
Proof.
  apply resolver_mutind.
  - intros children IH e e' seed same. cbn [r2 r2_items r2_item r2_procs r2_cases].
    rewrite (IH e e' seed (item_count children) 0 same). reflexivity.
  - intros e e' seed count index same. reflexivity.
  - intros child IHchild rest IHrest e e' seed count index same. cbn [r2 r2_items r2_item r2_procs r2_cases].
    rewrite (IHchild e e' (term_seed seed index count) same). apply bind_ext. intro child'.
    rewrite (IHrest e e' seed count (S index) same). reflexivity.
  - intros data IH e e' ts same. cbn [r2 r2_items r2_item r2_procs r2_cases]. rewrite (IH e e' None same). reflexivity.
  - intros sigs count body IH e e' ts same. cbn [r2 r2_items r2_item r2_procs r2_cases].
    rewrite (IH (holes count e) (holes count e') (receive_seed ts) (holes_same count e e' same)).
    rewrite (map_subst_sig_same e e' sigs same). reflexivity.
  - intros spec body IH e e' ts same. destruct ts as [s |]; cbn [r2_item].
    + destruct (allocate P spec s) as [[values rest_seed] |]; [| reflexivity].
      rewrite (IH (bind_names values e) (bind_names values e') (Some rest_seed)
                  (bind_names_same values e e' same)).
      reflexivity.
    + rewrite (IH (holes (binders P spec) e) (holes (binders P spec) e') None
                  (holes_same (binders P spec) e e' same)).
      reflexivity.
  - intros alternatives IH e e' ts same. cbn [r2 r2_items r2_item r2_procs r2_cases]. rewrite (IH e e' ts same). reflexivity.
  - intros bodies IH e e' ts same. cbn [r2 r2_items r2_item r2_procs r2_cases]. rewrite (IH e e' ts same). reflexivity.
  - intros s body IH e e' ts same. cbn [r2 r2_items r2_item r2_procs r2_cases].
    rewrite (IH e e' ts same), (subst_sig_same e e' s same). reflexivity.
  - intros cells e e' ts same. cbn [r2 r2_items r2_item r2_procs r2_cases]. rewrite (map_subst_sig_same e e' cells same). reflexivity.
  - intros e e' ts same. reflexivity.
  - intros e e' seed same. reflexivity.
  - intros q IHq rest IHrest e e' seed same. cbn [r2 r2_items r2_item r2_procs r2_cases].
    rewrite (IHq e e' seed same). apply bind_ext. intro q'.
    rewrite (IHrest e e' seed same). reflexivity.
  - intros e e' seed same. reflexivity.
  - intros count body IHbody rest IHrest e e' seed same. cbn [r2 r2_items r2_item r2_procs r2_cases].
    rewrite (IHbody (holes count e) (holes count e') seed (holes_same count e e' same)).
    apply bind_ext. intro body'.
    rewrite (IHrest e e' seed same). reflexivity.
Qed.

Lemma receive_seed_none : receive_seed None = None.
Proof. unfold receive_seed. destruct (resolve_receive_bodies P); reflexivity. Qed.

(** Outside a resolving position the one-pass resolver allocates nothing: it
    substitutes the environment. *)
Theorem r2_none :
  (forall p e, r2 e None p = Ok (subst e p)) /\
  (forall children e count index, r2_items e None count index children = Ok (subst_items e children)) /\
  (forall child e, r2_item e None child = Ok (subst_item e child)) /\
  (forall ps e, r2_procs e None ps = Ok (subst_procs e ps)) /\
  (forall cs e, r2_cases e None cs = Ok (subst_cases e cs)).
Proof.
  apply resolver_mutind.
  - intros children IH e. cbn [r2]. rewrite IH. reflexivity.
  - intros e count index. reflexivity.
  - intros child IHchild rest IHrest e count index. cbn [r2_items term_seed].
    rewrite IHchild. cbn [bind]. rewrite IHrest. reflexivity.
  - intros data IH e. cbn [r2_item]. rewrite IH. reflexivity.
  - intros sigs count body IH e. cbn [r2_item]. rewrite receive_seed_none, IH. reflexivity.
  - intros spec body IH e. cbn [r2_item]. rewrite IH. reflexivity.
  - intros alternatives IH e. cbn [r2_item]. rewrite IH. reflexivity.
  - intros bodies IH e. cbn [r2_item]. rewrite IH. reflexivity.
  - intros s body IH e. cbn [r2_item]. rewrite IH. reflexivity.
  - intros cells e. reflexivity.
  - intros e. reflexivity.
  - intros e. reflexivity.
  - intros q IHq rest IHrest e. cbn [r2_procs]. rewrite IHq. cbn [bind]. rewrite IHrest.
    reflexivity.
  - intros e. reflexivity.
  - intros count body IHbody rest IHrest e. cbn [r2_cases].
    rewrite IHbody. cbn [bind]. rewrite IHrest. reflexivity.
Qed.

(** An allocation returns one name for each binder of the `new`, as
    [util::allocate_new_bindings] does. *)
Hypothesis alloc_binders : forall spec seed values rest_seed,
  allocate P spec seed = Some (values, rest_seed) -> length values = binders P spec.

(** The linearization: resolving a term after a substitution equals resolving
    it once with the substitution's environment. With the empty environment,
    HEAD's recursion equals the one pass. *)
Theorem resolver_linearization :
  resolve_receive_bodies P = true ->
  (forall p fuel e seed,
      size p <= fuel -> r1 fuel seed (subst e p) = r2 e (Some seed) p) /\
  (forall children fuel e seed count index,
      size_items children <= fuel ->
      r1_items_step (r1 fuel) seed count index (subst_items e children) =
      r2_items e (Some seed) count index children) /\
  (forall child fuel e ts,
      size_item child <= fuel ->
      r1_item_step (r1 fuel) ts (subst_item e child) = r2_item e (Some ts) child) /\
  (forall ps fuel e seed,
      size_procs ps <= fuel -> r1_procs_step (r1 fuel) seed (subst_procs e ps) = r2_procs e (Some seed) ps) /\
  (forall cs fuel e seed,
      size_cases cs <= fuel -> r1_cases_step (r1 fuel) seed (subst_cases e cs) = r2_cases e (Some seed) cs).
Proof.
  intro resolving.
  apply resolver_mutind.
  - (* Proc *)
    intros children IH fuel e seed bound. simpl in bound.
    destruct fuel as [| fuel]; [lia |]. simpl.
    rewrite item_count_subst. rewrite IH by lia. reflexivity.
  - (* INil *)
    intros fuel e seed count index bound. reflexivity.
  - (* ICons *)
    intros child IHchild rest IHrest fuel e seed count index bound. simpl in bound |- *.
    rewrite IHchild by lia. apply bind_ext. intro child'.
    rewrite IHrest by lia. reflexivity.
  - (* ISend *)
    intros data IH fuel e ts bound. simpl.
    rewrite (proj1 (proj2 (proj2 (proj2 r2_none)))). reflexivity.
  - (* IReceive *)
    intros sigs count body IH fuel e ts bound. simpl in bound |- *.
    unfold receive_seed. rewrite resolving.
    rewrite IH by lia. reflexivity.
  - (* INew *)
    intros spec body IH fuel e ts bound. simpl in bound |- *.
    destruct (allocate P spec ts) as [[values rest_seed] |] eqn:allocated; [| reflexivity].
    rewrite (proj1 subst_compose).
    rewrite (proj1 subst_same _ _ (bind_names values e)).
    + rewrite IH by lia. reflexivity.
    + rewrite <- (alloc_binders _ _ _ _ allocated). apply overlay_bind_names.
  - (* IMatch *)
    intros alternatives IH fuel e ts bound. simpl in bound |- *.
    rewrite IH by lia. reflexivity.
  - (* IBranches *)
    intros bodies IH fuel e ts bound. simpl in bound |- *.
    rewrite IH by lia. reflexivity.
  - (* ISigned *)
    intros s body IH fuel e ts bound. simpl in bound |- *.
    rewrite IH by lia. reflexivity.
  - (* IStack *)
    intros cells fuel e ts bound. reflexivity.
  - (* IExpr *)
    intros fuel e ts bound. reflexivity.
  - (* PNil *)
    intros fuel e seed bound. reflexivity.
  - (* PCons *)
    intros q IHq rest IHrest fuel e seed bound. simpl in bound |- *.
    rewrite IHq by lia. apply bind_ext. intro q'.
    rewrite IHrest by lia. reflexivity.
  - (* CNil *)
    intros fuel e seed bound. reflexivity.
  - (* CCons *)
    intros count body IHbody rest IHrest fuel e seed bound. simpl in bound |- *.
    rewrite IHbody by lia. apply bind_ext. intro body'.
    rewrite IHrest by lia. reflexivity.
Qed.

Corollary one_pass_equals_recursive_resolver : forall p seed,
  resolve_receive_bodies P = true ->
  r1 (size p) seed p = r2 no_names (Some seed) p.
Proof.
  intros p seed resolving.
  pose proof (proj1 (resolver_linearization resolving) p (size p) no_names seed (le_n _)) as same.
  rewrite (proj1 subst_no_names) in same. exact same.
Qed.

(** * 4. The machine *)

Fixpoint item_tasks (seed : option Seed) (count index : nat) (children : items) : list (task Seed) :=
  match children with
  | INil => []
  | ICons child rest => TItem child (term_seed seed index count) :: item_tasks seed count (S index) rest
  end.

Fixpoint visit_procs (ts : option Seed) (ps : procs) : list (task Seed) :=
  match ps with
  | PNil => []
  | PCons q rest => TVisit q ts :: visit_procs ts rest
  end.

Fixpoint visit_cases (ts : option Seed) (cs : cases) : list (task Seed) :=
  match cs with
  | CNil => []
  | CCons count body rest => THoles count :: TVisit body ts :: TPop count :: visit_cases ts rest
  end.

Fixpoint procs_count (ps : procs) : nat :=
  match ps with
  | PNil => 0
  | PCons _ rest => S (procs_count rest)
  end.

Lemma length_list_of_procs : forall ps, length (list_of_procs ps) = procs_count ps.
Proof. induction ps as [| q rest IH]; simpl; congruence. Qed.

(** The Rust [Visit]: the item tasks in term order, then the node's rebuild. *)
Definition expand_visit (p : proc) (seed : option Seed) : list (task Seed) :=
  match p with
  | Proc children => item_tasks seed (item_count children) 0 children ++ [TBuildNode (item_count children)]
  end.

(** The tasks of one item, in the order of the Rust schedule. The Rust
    [Build] does the item rebuilds and the node rebuild in one loop over the
    same values. *)
Definition expand_item (child : item) (ts : option Seed) : list (task Seed) :=
  match child with
  | ISend data => visit_procs None data ++ [TBuildSend (procs_count data)]
  | IReceive sigs count body =>
      [TSigs sigs; THoles count; TVisit body (receive_seed ts); TPop count;
       TBuildReceive count (length sigs)]
  | INew spec body => [TEnter spec body ts; TBuildNew spec]
  | IMatch alternatives => visit_cases ts alternatives ++ [TBuildMatch (case_counts alternatives)]
  | IBranches bodies => visit_procs ts bodies ++ [TBuildBranches (procs_count bodies)]
  | ISigned s body => [TSigs [s]; TVisit body ts; TBuildSigned]
  | IStack cells => [TSigs cells; TBuildStack (length cells)]
  | IExpr => [TBuildExpr]
  end.

(** The Rust [Enter]: allocate in a resolving position, else push holes. *)
Definition expand_enter (spec : nat) (body : proc) (ts : option Seed) : outcome (list (task Seed)) :=
  match ts with
  | Some s =>
      match allocate P spec s with
      | Some (values, rest_seed) => Ok [TNames values; TVisit body (Some rest_seed); TPop (length values)]
      | None => Err spec
      end
  | None => Ok [THoles (binders P spec); TVisit body None; TPop (binders P spec)]
  end.

(** The primitive tasks. *)
Definition exec (pending : task Seed) (current : state) : outcome state :=
  match current with
  | (scope, values) =>
      match pending with
      | TSigs sigs => Ok (scope, rev (map (fun s => VSig (subst_sig (env_of scope) s)) sigs) ++ values)
      | THoles count => Ok (repeat None count ++ scope, values)
      | TNames names => Ok (map Some names ++ scope, values)
      | TPop count => Ok (skipn count scope, values)
      | TBuildNode count =>
          match take_values count values with
          | Some (taken, rest) =>
              match as_items (rev taken) with
              | Some children => Ok (scope, VProc (Proc (items_of_list children)) :: rest)
              | None => Stuck
              end
          | None => Stuck
          end
      | TBuildSend count =>
          match take_values count values with
          | Some (taken, rest) =>
              match as_procs (rev taken) with
              | Some data => Ok (scope, VItem (ISend (procs_of_list data)) :: rest)
              | None => Stuck
              end
          | None => Stuck
          end
      | TBuildReceive count signatures =>
          match values with
          | VProc body :: rest =>
              match take_values signatures rest with
              | Some (taken, remaining) =>
                  match as_sigs (rev taken) with
                  | Some sigs => Ok (scope, VItem (IReceive sigs count body) :: remaining)
                  | None => Stuck
                  end
              | None => Stuck
              end
          | _ => Stuck
          end
      | TBuildNew spec =>
          match values with
          | VProc body :: rest => Ok (scope, VItem (INew spec body) :: rest)
          | _ => Stuck
          end
      | TBuildMatch counts =>
          match take_values (length counts) values with
          | Some (taken, rest) =>
              match as_procs (rev taken) with
              | Some bodies => Ok (scope, VItem (IMatch (cases_of_lists counts bodies)) :: rest)
              | None => Stuck
              end
          | None => Stuck
          end
      | TBuildBranches count =>
          match take_values count values with
          | Some (taken, rest) =>
              match as_procs (rev taken) with
              | Some bodies => Ok (scope, VItem (IBranches (procs_of_list bodies)) :: rest)
              | None => Stuck
              end
          | None => Stuck
          end
      | TBuildSigned =>
          match values with
          | VProc body :: VSig s :: rest => Ok (scope, VItem (ISigned s body) :: rest)
          | _ => Stuck
          end
      | TBuildStack count =>
          match take_values count values with
          | Some (taken, rest) =>
              match as_sigs (rev taken) with
              | Some cells => Ok (scope, VItem (IStack cells) :: rest)
              | None => Stuck
              end
          | None => Stuck
          end
      | TBuildExpr => Ok (scope, VItem IExpr :: values)
      | TVisit _ _ | TItem _ _ | TEnter _ _ _ => Stuck
      end
  end.

(** The machine, generic in its node schedule and in its `new` expansion, so
    the negative controls can change one of them. *)
Fixpoint mrun_with (node : proc -> option Seed -> list (task Seed))
    (enter : nat -> proc -> option Seed -> outcome (list (task Seed)))
    (fuel : nat) (work : list (task Seed)) (current : state) : outcome state :=
  match fuel with
  | 0 => Stuck
  | S fuel' =>
      match work with
      | [] => Ok current
      | TVisit p seed :: rest => mrun_with node enter fuel' (node p seed ++ rest) current
      | TItem child ts :: rest => mrun_with node enter fuel' (expand_item child ts ++ rest) current
      | TEnter spec body ts :: rest =>
          match enter spec body ts with
          | Ok expansion => mrun_with node enter fuel' (expansion ++ rest) current
          | Err code => Err code
          | Stuck => Stuck
          end
      | pending :: rest =>
          match exec pending current with
          | Ok next => mrun_with node enter fuel' rest next
          | Err code => Err code
          | Stuck => Stuck
          end
      end
  end.

Definition mrun := mrun_with expand_visit expand_enter.

(** The specification of each task: a visit pushes the one-pass resolution of
    its term, an item pushes its resolved item, and a `new` pushes its
    resolved body. *)
Definition den (pending : task Seed) (current : state) : outcome state :=
  match current with
  | (scope, values) =>
      match pending with
      | TVisit p seed => bind (r2 (env_of scope) seed p) (fun p' => Ok (scope, VProc p' :: values))
      | TItem child ts => bind (r2_item (env_of scope) ts child) (fun child' => Ok (scope, VItem child' :: values))
      | TEnter spec body ts =>
          bind (match ts with
                | Some s =>
                    match allocate P spec s with
                    | Some (names, rest_seed) => r2 (bind_names names (env_of scope)) (Some rest_seed) body
                    | None => Err spec
                    end
                | None => r2 (holes (binders P spec) (env_of scope)) None body
                end)
               (fun body' => Ok (scope, VProc body' :: values))
      | _ => exec pending (scope, values)
      end
  end.

Fixpoint den_all (work : list (task Seed)) (current : state) : outcome state :=
  match work with
  | [] => Ok current
  | pending :: rest => bind (den pending current) (den_all rest)
  end.

Lemma den_all_app : forall first_work later_work current,
  den_all (first_work ++ later_work) current = bind (den_all first_work current) (den_all later_work).
Proof.
  induction first_work as [| pending first_work IH]; intros later_work current; simpl.
  - reflexivity.
  - rewrite bind_assoc. apply bind_ext. intro next. apply IH.
Qed.

(** Results of the one-pass resolver keep the shape of their input. *)
Lemma r2_items_count : forall children e seed count index children',
  r2_items e seed count index children = Ok children' -> item_count children' = item_count children.
Proof.
  induction children as [| child rest IH]; intros e seed count index children' resolved.
  - cbn [r2_items] in resolved. inversion resolved. reflexivity.
  - cbn [r2_items] in resolved.
    destruct (r2_item e (term_seed seed index count) child) as [child' | code |];
      cbn [bind] in resolved; try discriminate.
    destruct (r2_items e seed count (S index) rest) as [rest' | code |] eqn:resolved_rest;
      cbn [bind] in resolved; try discriminate.
    inversion resolved. cbn [item_count]. f_equal. eapply IH. exact resolved_rest.
Qed.

Lemma r2_procs_count : forall ps e seed ps',
  r2_procs e seed ps = Ok ps' -> procs_count ps' = procs_count ps.
Proof.
  induction ps as [| q rest IH]; intros e seed ps' resolved.
  - cbn [r2_procs] in resolved. inversion resolved. reflexivity.
  - cbn [r2_procs] in resolved.
    destruct (r2 e seed q) as [q' | code |]; cbn [bind] in resolved; try discriminate.
    destruct (r2_procs e seed rest) as [rest' | code |] eqn:resolved_rest;
      cbn [bind] in resolved; try discriminate.
    inversion resolved. cbn [procs_count]. f_equal. eapply IH. exact resolved_rest.
Qed.

Lemma r2_cases_counts : forall cs e seed cs',
  r2_cases e seed cs = Ok cs' -> case_counts cs' = case_counts cs.
Proof.
  induction cs as [| count body rest IH]; intros e seed cs' resolved.
  - cbn [r2_cases] in resolved. inversion resolved. reflexivity.
  - cbn [r2_cases] in resolved.
    destruct (r2 (holes count e) seed body) as [body' | code |]; cbn [bind] in resolved;
      try discriminate.
    destruct (r2_cases e seed rest) as [rest' | code |] eqn:resolved_rest;
      cbn [bind] in resolved; try discriminate.
    inversion resolved. cbn [case_counts]. f_equal. eapply IH. exact resolved_rest.
Qed.

Lemma length_case_bodies : forall cs, length (case_bodies cs) = length (case_counts cs).
Proof. induction cs as [| count body rest IH]; simpl; congruence. Qed.

(** Running the item tasks of a node pushes the resolved items in order. *)
Lemma den_item_tasks : forall children seed count index scope values,
  den_all (item_tasks seed count index children) (scope, values) =
  bind (r2_items (env_of scope) seed count index children)
    (fun children' => Ok (scope, rev (map VItem (list_of_items children')) ++ values)).
Proof.
  induction children as [| child rest IH]; intros seed count index scope values.
  - reflexivity.
  - cbn [item_tasks den_all den r2_items].
    rewrite !bind_assoc. apply bind_ext. intro child'. cbn [bind].
    rewrite IH. rewrite bind_assoc. apply bind_ext. intro rest'.
    cbn [bind list_of_items map rev].
    rewrite <- app_assoc. reflexivity.
Qed.

Lemma den_visit_procs : forall ps ts scope values,
  den_all (visit_procs ts ps) (scope, values) =
  bind (r2_procs (env_of scope) ts ps)
    (fun ps' => Ok (scope, rev (map VProc (list_of_procs ps')) ++ values)).
Proof.
  induction ps as [| q rest IH]; intros ts scope values.
  - reflexivity.
  - cbn [visit_procs den_all den r2_procs].
    rewrite !bind_assoc. apply bind_ext. intro q'. cbn [bind].
    rewrite IH. rewrite bind_assoc. apply bind_ext. intro rest'.
    cbn [bind list_of_procs map rev].
    rewrite <- app_assoc. reflexivity.
Qed.

Lemma den_visit_cases : forall cs ts scope values,
  den_all (visit_cases ts cs) (scope, values) =
  bind (r2_cases (env_of scope) ts cs)
    (fun cs' => Ok (scope, rev (map VProc (case_bodies cs')) ++ values)).
Proof.
  induction cs as [| count body rest IH]; intros ts scope values.
  - reflexivity.
  - cbn [visit_cases den_all den exec bind r2_cases].
    rewrite (proj1 r2_same body (env_of (repeat None count ++ scope)) (holes count (env_of scope)))
      by apply env_of_holes.
    rewrite !bind_assoc. apply bind_ext. intro body'.
    cbn [bind den_all den exec].
    rewrite skipn_repeat_app.
    rewrite IH. rewrite bind_assoc. apply bind_ext. intro rest'.
    cbn [bind case_bodies map rev].
    rewrite <- app_assoc. reflexivity.
Qed.

Lemma sig_values : forall e sigs,
  map (fun s => VSig (subst_sig e s)) sigs = map VSig (map (subst_sig e) sigs).
Proof. intros e sigs. rewrite map_map. reflexivity. Qed.

(** Expanding a visit keeps the denotation: the item tasks and the rebuild
    push the one-pass resolution of the node. *)
Lemma expand_visit_den : forall p seed scope values,
  den_all (expand_visit p seed) (scope, values) = den (TVisit p seed) (scope, values).
Proof.
  intros [children] seed scope values. cbn [expand_visit den].
  rewrite den_all_app, den_item_tasks. cbn [r2]. rewrite !bind_assoc.
  destruct (r2_items (env_of scope) seed (item_count children) 0 children)
    as [children' | code |] eqn:resolved; cbn [bind]; [| reflexivity | reflexivity].
  apply r2_items_count in resolved.
  cbn [den_all den exec].
  rewrite <- resolved, <- length_list_of_items, take_pushed.
  cbn [bind]. rewrite rev_involutive, as_items_map. cbn [bind].
  rewrite items_round_trip. reflexivity.
Qed.

Lemma expand_item_den : forall child ts scope values,
  den_all (expand_item child ts) (scope, values) = den (TItem child ts) (scope, values).
Proof.
  intros child ts scope values.
  destruct child as [data | sigs count body | spec body | alternatives | bodies | s body | cells |].
  - (* ISend *)
    cbn [expand_item den r2_item].
    rewrite den_all_app, den_visit_procs. rewrite !bind_assoc.
    destruct (r2_procs (env_of scope) None data) as [data' | code |] eqn:resolved;
      cbn [bind]; [| reflexivity | reflexivity].
    apply r2_procs_count in resolved.
    cbn [den_all den exec].
    rewrite <- resolved, <- length_list_of_procs, take_pushed.
    cbn [bind]. rewrite rev_involutive, as_procs_map. cbn [bind].
    rewrite procs_round_trip. reflexivity.
  - (* IReceive *)
    cbn [expand_item den r2_item den_all exec bind].
    rewrite (proj1 r2_same body (env_of (repeat None count ++ scope)) (holes count (env_of scope)))
      by apply env_of_holes.
    rewrite !bind_assoc.
    destruct (r2 (holes count (env_of scope)) (receive_seed ts) body) as [body' | code |];
      cbn [bind den_all den exec]; [| reflexivity | reflexivity].
    rewrite skipn_repeat_app, sig_values.
    replace (length sigs) with (length (map (subst_sig (env_of scope)) sigs)) by apply length_map.
    rewrite take_pushed. cbn [bind]. rewrite rev_involutive, as_sigs_map. reflexivity.
  - (* INew *)
    destruct ts as [s |].
    + destruct (allocate P spec s) as [[names rest_seed] |] eqn:allocated.
      * cbn [expand_item den r2_item den_all exec bind]. rewrite allocated. cbn [bind].
        rewrite !bind_assoc. apply bind_ext. intro body'. reflexivity.
      * cbn [expand_item den r2_item den_all exec bind]. rewrite allocated. reflexivity.
    + cbn [expand_item den r2_item den_all exec bind].
      rewrite !bind_assoc. apply bind_ext. intro body'. reflexivity.
  - (* IMatch *)
    cbn [expand_item den r2_item].
    rewrite den_all_app, den_visit_cases. rewrite !bind_assoc.
    destruct (r2_cases (env_of scope) ts alternatives) as [alternatives' | code |] eqn:resolved;
      cbn [bind]; [| reflexivity | reflexivity].
    apply r2_cases_counts in resolved.
    cbn [den_all den exec].
    rewrite <- resolved, <- length_case_bodies, take_pushed.
    cbn [bind]. rewrite rev_involutive, as_procs_map. cbn [bind].
    rewrite cases_round_trip. reflexivity.
  - (* IBranches *)
    cbn [expand_item den r2_item].
    rewrite den_all_app, den_visit_procs. rewrite !bind_assoc.
    destruct (r2_procs (env_of scope) ts bodies) as [bodies' | code |] eqn:resolved;
      cbn [bind]; [| reflexivity | reflexivity].
    apply r2_procs_count in resolved.
    cbn [den_all den exec].
    rewrite <- resolved, <- length_list_of_procs, take_pushed.
    cbn [bind]. rewrite rev_involutive, as_procs_map. cbn [bind].
    rewrite procs_round_trip. reflexivity.
  - (* ISigned *)
    cbn [expand_item den r2_item den_all exec bind map rev app].
    rewrite !bind_assoc. apply bind_ext. intro body'. reflexivity.
  - (* IStack *)
    cbn [expand_item den r2_item den_all exec bind].
    rewrite sig_values.
    replace (length cells) with (length (map (subst_sig (env_of scope)) cells)) by apply length_map.
    rewrite take_pushed. cbn [bind]. rewrite rev_involutive, as_sigs_map. reflexivity.
  - (* IExpr *)
    reflexivity.
Qed.

Lemma expand_enter_den : forall spec body ts expansion scope values,
  expand_enter spec body ts = Ok expansion ->
  den_all expansion (scope, values) = den (TEnter spec body ts) (scope, values).
Proof.
  intros spec body ts expansion scope values expanded. unfold expand_enter in expanded.
  destruct ts as [s |].
  - destruct (allocate P spec s) as [[names rest_seed] |] eqn:allocated;
      inversion expanded; subst.
    cbn [den den_all exec bind]. rewrite allocated. cbn [bind].
    rewrite (proj1 r2_same body (env_of (map Some names ++ scope)) (bind_names names (env_of scope)))
      by apply env_of_names.
    rewrite !bind_assoc. apply bind_ext. intro body'. cbn [bind den_all den exec].
    rewrite skipn_map_app. reflexivity.
  - inversion expanded; subst. cbn [den den_all exec bind].
    rewrite (proj1 r2_same body (env_of (repeat None (binders P spec) ++ scope))
               (holes (binders P spec) (env_of scope))) by apply env_of_holes.
    rewrite !bind_assoc. apply bind_ext. intro body'. cbn [bind den_all den exec].
    rewrite skipn_repeat_app. reflexivity.
Qed.

Lemma expand_enter_error : forall spec body ts code scope values,
  expand_enter spec body ts = Err code -> den (TEnter spec body ts) (scope, values) = Err code.
Proof.
  intros spec body ts code scope values expanded. unfold expand_enter in expanded.
  destruct ts as [s |]; [| discriminate].
  destruct (allocate P spec s) as [[names rest_seed] |] eqn:allocated;
    inversion expanded; subst.
  cbn [den]. rewrite allocated. reflexivity.
Qed.

Lemma expand_enter_never_stuck : forall spec body ts, expand_enter spec body ts <> Stuck.
Proof.
  intros spec body [s |]; unfold expand_enter; [| discriminate].
  destruct (allocate P spec s) as [[names rest_seed] |]; discriminate.
Qed.

(** The machine's steps: one per expansion and one per primitive task. *)
Fixpoint wsize (p : proc) : nat :=
  match p with
  | Proc children => 2 + wsize_items children
  end
with wsize_items (children : items) : nat :=
  match children with
  | INil => 0
  | ICons child rest => wsize_item child + wsize_items rest
  end
with wsize_item (child : item) : nat :=
  match child with
  | ISend data => 2 + wsize_procs data
  | IReceive _ _ body => 5 + wsize body
  | INew _ body => 5 + wsize body
  | IMatch alternatives => 2 + wsize_cases alternatives
  | IBranches bodies => 2 + wsize_procs bodies
  | ISigned _ body => 3 + wsize body
  | IStack _ => 3
  | IExpr => 2
  end
with wsize_procs (ps : procs) : nat :=
  match ps with
  | PNil => 0
  | PCons q rest => wsize q + wsize_procs rest
  end
with wsize_cases (cs : cases) : nat :=
  match cs with
  | CNil => 0
  | CCons _ body rest => 2 + wsize body + wsize_cases rest
  end.

Definition task_weight (pending : task Seed) : nat :=
  match pending with
  | TVisit p _ => wsize p
  | TItem child _ => wsize_item child
  | TEnter _ body _ => 3 + wsize body
  | _ => 1
  end.

Fixpoint work_weight (work : list (task Seed)) : nat :=
  match work with
  | [] => 0
  | pending :: rest => task_weight pending + work_weight rest
  end.

Lemma work_weight_app : forall first_work later_work,
  work_weight (first_work ++ later_work) = work_weight first_work + work_weight later_work.
Proof.
  induction first_work as [| pending first_work IH]; intros later_work; simpl; [reflexivity |].
  rewrite IH. lia.
Qed.

Lemma weight_item_tasks : forall children seed count index,
  work_weight (item_tasks seed count index children) = wsize_items children.
Proof.
  induction children as [| child rest IH]; intros seed count index; simpl; [reflexivity |].
  rewrite IH. reflexivity.
Qed.

Lemma weight_visit_procs : forall ps ts, work_weight (visit_procs ts ps) = wsize_procs ps.
Proof. induction ps as [| q rest IH]; intros ts; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

Lemma weight_visit_cases : forall cs ts, work_weight (visit_cases ts cs) = wsize_cases cs.
Proof. induction cs as [| count body rest IH]; intros ts; simpl; [reflexivity | rewrite IH; lia]. Qed.

Lemma expand_visit_weight : forall p seed, S (work_weight (expand_visit p seed)) = wsize p.
Proof.
  intros [children] seed. unfold expand_visit. rewrite work_weight_app, weight_item_tasks.
  simpl. lia.
Qed.

Lemma expand_item_weight : forall child ts, S (work_weight (expand_item child ts)) = wsize_item child.
Proof.
  intros child ts.
  destruct child as [data | sigs count body | spec body | alternatives | bodies | s body | cells |];
    simpl expand_item.
  - rewrite work_weight_app, weight_visit_procs. simpl. lia.
  - simpl. lia.
  - simpl. lia.
  - rewrite work_weight_app, weight_visit_cases. simpl. lia.
  - rewrite work_weight_app, weight_visit_procs. simpl. lia.
  - simpl. lia.
  - simpl. lia.
  - reflexivity.
Qed.

Lemma expand_enter_weight : forall spec body ts expansion,
  expand_enter spec body ts = Ok expansion -> S (work_weight expansion) = 3 + wsize body.
Proof.
  intros spec body ts expansion expanded. unfold expand_enter in expanded.
  destruct ts as [s |].
  - destruct (allocate P spec s) as [[names rest_seed] |]; inversion expanded; subst. simpl. lia.
  - inversion expanded; subst. simpl. lia.
Qed.

(** Each step of the machine keeps the denotation of its work stack, and each
    step lowers the weight by one, so with enough fuel the machine computes the
    denotation. *)
Theorem machine_runs_its_denotation : forall fuel work current,
  work_weight work < fuel -> mrun fuel work current = den_all work current.
Proof.
  induction fuel as [| fuel IH]; intros work [scope values] bound.
  - lia.
  - destruct work as [| pending rest]; [reflexivity |].
    destruct pending as [p seed | child ts | spec body ts | sigs | count | names | count
                         | count | count | count signatures | spec | counts | count | | count |];
      cbn [work_weight task_weight] in bound.
    1: {
      change (mrun fuel (expand_visit p seed ++ rest) (scope, values) =
              bind (den (TVisit p seed) (scope, values)) (den_all rest)).
      rewrite IH.
      - rewrite den_all_app, expand_visit_den. reflexivity.
      - rewrite work_weight_app. pose proof (expand_visit_weight p seed). lia.
    }
    1: {
      change (mrun fuel (expand_item child ts ++ rest) (scope, values) =
              bind (den (TItem child ts) (scope, values)) (den_all rest)).
      rewrite IH.
      - rewrite den_all_app, expand_item_den. reflexivity.
      - rewrite work_weight_app. pose proof (expand_item_weight child ts). lia.
    }
    1: {
      change (match expand_enter spec body ts with
              | Ok expansion => mrun fuel (expansion ++ rest) (scope, values)
              | Err code => Err code
              | Stuck => Stuck
              end = bind (den (TEnter spec body ts) (scope, values)) (den_all rest)).
      destruct (expand_enter spec body ts) as [expansion | code |] eqn:expanded.
      - rewrite IH.
        + rewrite den_all_app, (expand_enter_den _ _ _ _ _ _ expanded). reflexivity.
        + rewrite work_weight_app. pose proof (expand_enter_weight _ _ _ _ expanded). lia.
      - rewrite (expand_enter_error _ _ _ _ _ _ expanded). reflexivity.
      - exfalso. exact (expand_enter_never_stuck _ _ _ expanded).
    }
    all: match goal with
         | |- mrun (S ?steps) (?pending :: ?later) ?current = _ =>
             change (match exec pending current with
                     | Ok next => mrun steps later next
                     | Err code => Err code
                     | Stuck => Stuck
                     end = bind (exec pending current) (den_all later));
             destruct (exec pending current) as [next | code |];
             cbn [bind]; [apply IH; lia | reflexivity | reflexivity]
         end.
Qed.

(** The headline refinement: started with a visit of the root, with no
    binder and no value, the machine ends with the one-pass resolution. *)
Theorem machine_refines_one_pass_resolver : forall p seed,
  mrun (S (wsize p)) [TVisit p seed] ([], []) =
  bind (r2 no_names seed p) (fun p' => Ok ([], [VProc p'])).
Proof.
  intros p seed.
  rewrite machine_runs_its_denotation by (cbn [work_weight task_weight]; lia).
  cbn [den_all den].
  rewrite (proj1 r2_same p (env_of []) no_names) by apply env_of_empty.
  rewrite bind_assoc. apply bind_ext. intro p'. reflexivity.
Qed.

(** With receive bodies resolved, as HEAD did, the machine computes HEAD's
    recursive resolution. *)
Corollary machine_refines_recursive_resolver : forall p seed,
  resolve_receive_bodies P = true ->
  mrun (S (wsize p)) [TVisit p (Some seed)] ([], []) =
  bind (r1 (size p) seed p) (fun p' => Ok ([], [VProc p'])).
Proof.
  intros p seed resolving.
  rewrite machine_refines_one_pass_resolver.
  rewrite (one_pass_equals_recursive_resolver p seed resolving). reflexivity.
Qed.

(** * 5. Linear work *)

Theorem resolver_steps_are_linear :
  (forall p, wsize p + 4 <= 6 * size p) /\
  (forall children, wsize_items children <= 6 * size_items children) /\
  (forall child, wsize_item child <= 6 * size_item child) /\
  (forall ps, wsize_procs ps <= 6 * size_procs ps) /\
  (forall cs, wsize_cases cs <= 6 * size_cases cs).
Proof.
  apply resolver_mutind; intros; simpl in *; lia.
Qed.

(** * 6. Negative controls (generic part) *)

(** A `new` expansion that does not pop the binders after the body. *)
Definition leaky_enter (spec : nat) (body : proc) (ts : option Seed) : outcome (list (task Seed)) :=
  match ts with
  | Some s =>
      match allocate P spec s with
      | Some (values, rest_seed) => Ok [TNames values; TVisit body (Some rest_seed)]
      | None => Err spec
      end
  | None => Ok [THoles (binders P spec); TVisit body None]
  end.

(** A node schedule that pushes the item tasks in source order on the LIFO
    stack, so the items run in reverse order. *)
Definition reversed_visit (p : proc) (seed : option Seed) : list (task Seed) :=
  match p with
  | Proc children =>
      rev (item_tasks seed (item_count children) 0 children) ++ [TBuildNode (item_count children)]
  end.

End Resolver.

(** * 6. Negative controls (witnesses) *)

(** A concrete instance: a seed is a number, a split mixes in the index, a
    `new` with a spec below 7 allocates one name, and a spec of 7 or more fails
    with its spec as the error. *)
Definition witness_params : params nat := {|
  split_rand := fun seed index _ => seed * 7 + index + 1;
  allocate := fun spec seed => if 7 <=? spec then None else Some ([seed + 100], seed + 1);
  binders := fun _ => 1;
  resolve_receive_bodies := true
|}.

Lemma witness_alloc_binders : forall spec seed values rest_seed,
  allocate witness_params spec seed = Some (values, rest_seed) ->
  length values = binders witness_params spec.
Proof.
  intros spec seed values rest_seed allocated.
  cbn [allocate witness_params] in allocated.
  destruct (7 <=? spec); [discriminate allocated |].
  injection allocated as values_eq rest_eq. subst values. reflexivity.
Qed.

(** The witness instance satisfies the premise, so the refinement holds for
    it. *)
Corollary witness_machine_refines_recursive_resolver : forall p seed,
  mrun witness_params (S (wsize p)) [TVisit p (Some seed)] ([], []) =
  bind (r1 witness_params (size p) seed p) (fun p' => Ok ([], [VProc p'])).
Proof.
  intros p seed.
  apply (machine_refines_recursive_resolver witness_params witness_alloc_binders).
  reflexivity.
Qed.

(** A `new` followed by a sibling signed term that names bound level 0. At the
    root no binder encloses the sibling, so its signature stays a bound level. *)
Definition leak_witness : proc :=
  Proc (ICons (INew 1 (Proc INil)) (ICons (ISigned (SBound 0) (Proc INil)) INil)).

Theorem skipped_scope_pop_breaks_the_refinement :
  mrun_with witness_params (expand_visit witness_params) (leaky_enter witness_params)
    (S (wsize leak_witness)) [TVisit leak_witness (Some 1)] ([], []) <>
  bind (r2 witness_params no_names (Some 1) leak_witness) (fun p' => Ok ([], [VProc p'])).
Proof.
  vm_compute. intro same. discriminate same.
Qed.

(** Two `new` terms whose allocations both fail: the first one in term order
    decides the error. *)
Definition order_witness : proc :=
  Proc (ICons (INew 7 (Proc INil)) (ICons (INew 8 (Proc INil)) INil)).

Theorem reordered_visit_breaks_the_refinement :
  mrun_with witness_params (reversed_visit witness_params) (expand_enter witness_params)
    (S (wsize order_witness)) [TVisit order_witness (Some 1)] ([], []) <>
  bind (r2 witness_params no_names (Some 1) order_witness) (fun p' => Ok ([], [VProc p'])).
Proof.
  vm_compute. intro same. discriminate same.
Qed.
