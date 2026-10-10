(* G1-1 (DR-67): the iterative analyzer walks refine the recursive walks.

   The pre-execution funding check runs the static analyzer of
   delta_sigma.rs on every incoming deploy. Its recursive walks
   (signed_demand_par, collect and has_bound_level in
   static_authority_signatures, demand_by_sig_into) use one stack frame per
   nesting level of the deploy. Merge 29b729551 on branch
   integration/f1r3lang-cost-accounted-rho-20261005 replaced them with
   explicit work stacks and value stacks, and it keeps the enclosing signed
   regions in a parent-linked arena (ScopeLink) instead of a copied list.
   G1-1 backports that code verbatim. This module proves that the backported
   machines compute what the recursions computed.

   Sections:
   1. The analyzer's view of a Par: a node is an ordered list of items. An
      item is a local event on the node's demand, a signed region, a child
      that is entered in one of three modes, a list of match alternatives or
      an if with two optional branches.
   2. The recursion of HEAD 17e07307f ([eval]) and the post-order program
      that the machine runs ([compile_term], [run]). Theorem
      [demand_compile_run_equivalence] is the instance of
      compile_run_equivalence (StackSafePDA.v on feature/mettail) for this
      algebra: running the compiled program on a value stack pushes the value
      of the recursion. It holds for every interpretation of the demand
      algebra, so it covers the lanes, the transfer lanes, the guaranteed
      supply, the introduction flag and the first rejection reason of the
      Rust SignedDemand.
   3. The lazy work stack of the Rust machine ([lazy_run]) expands one task
      at a time. It executes exactly the compiled program
      ([lazy_run_runs_the_flattened_program]), so it refines the recursion
      ([iterative_walk_refines_recursion]). Its step count is linear in the
      size of the term ([iterative_walk_steps_are_linear]).
   4. The parent-linked arena represents the persistent scope chain that each
      task carries in the model ([arena_push_extends_chain],
      [arena_append_preserves_chain], [arena_push_preserves_order]).
   5. Negative controls with a concrete trace algebra: a machine that skips
      the scope pop after a signed region, and a machine that visits the
      items of a node in reverse order, both break the refinement.
   6. The machine reads the scope chain inner to outer, and the recursion
      reads it outer to inner. Lane bumps commute, so the direction does not
      change the demand ([bump_lanes_reverse]).
   7. The worklist of collect and has_bound_level visits the same events in
      the same order as their recursion, so a fold that stops at the first
      error returns the same value or the same first error
      ([collect_worklist_refines_recursion]).

   The two premises of Section 2 are Section hypotheses. They become ordinary
   premises of each theorem when the section closes, so no axiom is used.
   Section 5 instantiates them with a trace algebra, and Section 6 discharges
   the chain-direction premise for a model of the lane counters. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
Import ListNotations.

(** * 1. The analyzer's view of a term *)

(** A lane is the key of a signed region. The option [None] is the unit
    region: it opens a scope but owns no lane. An event is a local update of
    the current node's demand: a stack, a send or receive introduction, a
    rejection or the dequotation scan. *)
Definition lane : Type := nat.
Definition event : Type := nat.

(** How a child is entered.
    - [Same]: the body of a [new], a [match] case, an [if] branch or a
      bundle keeps the scope and the execution flag.
    - [Data]: a send datum starts with no scope, outside execution position.
    - [Body]: a receive body starts with no scope, in execution position. *)
Inductive mode : Type :=
| Same : mode
| Data : mode
| Body : mode.

Inductive term : Type :=
| Node : items -> term
with items : Type :=
| INil : items
| ICons : item -> items -> items
with item : Type :=
| Local : event -> item
| Scoped : option lane -> term -> item
| Child : mode -> term -> item
| Alts : forest -> item
| Cond : branch -> branch -> item
with forest : Type :=
| FNil : forest
| FCons : term -> forest -> forest
with branch : Type :=
| BNone : branch
| BSome : term -> branch.

Scheme term_mut := Induction for term Sort Prop
  with items_mut := Induction for items Sort Prop
  with item_mut := Induction for item Sort Prop
  with forest_mut := Induction for forest Sort Prop
  with branch_mut := Induction for branch Sort Prop.

Combined Scheme walk_mutind from term_mut, items_mut, item_mut, forest_mut, branch_mut.

Definition child_scope (m : mode) (scope : list (option lane)) : list (option lane) :=
  match m with
  | Same => scope
  | Data => []
  | Body => []
  end.

Definition child_exec (m : mode) (exec : bool) : bool :=
  match m with
  | Same => exec
  | Data => false
  | Body => true
  end.

(** * 2. The post-order program of the machine *)

(** The instructions of the value-stack machine. [IPush] is the Rust
    [Visit] (and [Empty]) push of a fresh accumulator. [ILocal] is an
    in-place update of the top accumulator, with the scope chain of its task.
    [ICombine], [IAlternative] and [IFinish] are the Rust [Combine],
    [Alternative] and [SignedFinish]. *)
Inductive instruction : Type :=
| IPush : instruction
| ILocal : list (option lane) -> bool -> event -> instruction
| ICombine : instruction
| IAlternative : instruction
| IFinish : option lane -> instruction.

(** The machine's chain lists the enclosing lanes inner to outer, as the
    arena's parent links do. *)
Fixpoint compile_term (chain : list (option lane)) (exec : bool) (subject : term)
    {struct subject} : list instruction :=
  match subject with
  | Node children => IPush :: compile_items chain exec children
  end
with compile_items (chain : list (option lane)) (exec : bool) (children : items)
    {struct children} : list instruction :=
  match children with
  | INil => []
  | ICons child rest => compile_item chain exec child ++ compile_items chain exec rest
  end
with compile_item (chain : list (option lane)) (exec : bool) (child : item)
    {struct child} : list instruction :=
  match child with
  | Local e => [ILocal chain exec e]
  | Scoped l body => compile_term (l :: chain) true body ++ [IFinish l]
  | Child m body => compile_term (child_scope m chain) (child_exec m exec) body ++ [ICombine]
  | Alts sources =>
      match sources with
      | FNil => []
      | FCons source rest =>
          compile_term chain exec source ++ compile_alts chain exec rest ++ [ICombine]
      end
  | Cond if_true if_false =>
      compile_branch chain exec if_true ++ compile_branch chain exec if_false
        ++ [IAlternative; ICombine]
  end
with compile_alts (chain : list (option lane)) (exec : bool) (sources : forest)
    {struct sources} : list instruction :=
  match sources with
  | FNil => []
  | FCons source rest => compile_term chain exec source ++ IAlternative :: compile_alts chain exec rest
  end
with compile_branch (chain : list (option lane)) (exec : bool) (choice : branch)
    {struct choice} : list instruction :=
  match choice with
  | BNone => [IPush]
  | BSome body => compile_term chain exec body
  end.

(** The tasks of the lazy work stack. A [TVisit] expands into the node's
    accumulator push and its item tasks. A [TItem] expands into the tasks of
    its children and the instruction that folds them in. *)
Inductive task : Type :=
| TVisit : term -> list (option lane) -> bool -> task
| TItem : item -> list (option lane) -> bool -> task
| TInstr : instruction -> task.

Fixpoint item_tasks (chain : list (option lane)) (exec : bool) (children : items) : list task :=
  match children with
  | INil => []
  | ICons child rest => TItem child chain exec :: item_tasks chain exec rest
  end.

Fixpoint alt_tasks (chain : list (option lane)) (exec : bool) (sources : forest) : list task :=
  match sources with
  | FNil => []
  | FCons source rest => TVisit source chain exec :: TInstr IAlternative :: alt_tasks chain exec rest
  end.

Definition branch_task (chain : list (option lane)) (exec : bool) (choice : branch) : task :=
  match choice with
  | BNone => TInstr IPush
  | BSome body => TVisit body chain exec
  end.

Definition expand_visit (subject : term) (chain : list (option lane)) (exec : bool) : list task :=
  match subject with
  | Node children => TInstr IPush :: item_tasks chain exec children
  end.

Definition expand_item (child : item) (chain : list (option lane)) (exec : bool) : list task :=
  match child with
  | Local e => [TInstr (ILocal chain exec e)]
  | Scoped l body => [TVisit body (l :: chain) true; TInstr (IFinish l)]
  | Child m body => [TVisit body (child_scope m chain) (child_exec m exec); TInstr ICombine]
  | Alts FNil => []
  | Alts (FCons source rest) => TVisit source chain exec :: alt_tasks chain exec rest ++ [TInstr ICombine]
  | Cond if_true if_false =>
      [branch_task chain exec if_true; branch_task chain exec if_false;
       TInstr IAlternative; TInstr ICombine]
  end.

Definition flatten (pending : task) : list instruction :=
  match pending with
  | TVisit subject chain exec => compile_term chain exec subject
  | TItem child chain exec => compile_item chain exec child
  | TInstr instr => [instr]
  end.

Fixpoint flatten_all (work : list task) : list instruction :=
  match work with
  | [] => []
  | pending :: rest => flatten pending ++ flatten_all rest
  end.

Lemma flatten_all_app : forall left_work right_work,
  flatten_all (left_work ++ right_work) = flatten_all left_work ++ flatten_all right_work.
Proof.
  induction left_work as [| pending left_work IH]; intros right_work; simpl.
  - reflexivity.
  - rewrite IH, app_assoc. reflexivity.
Qed.

Lemma flatten_item_tasks : forall chain exec children,
  flatten_all (item_tasks chain exec children) = compile_items chain exec children.
Proof.
  intros chain exec children.
  induction children as [| child rest IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma flatten_alt_tasks : forall chain exec sources,
  flatten_all (alt_tasks chain exec sources) = compile_alts chain exec sources.
Proof.
  intros chain exec sources.
  induction sources as [| source rest IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma flatten_expand_visit : forall subject chain exec,
  flatten_all (expand_visit subject chain exec) = compile_term chain exec subject.
Proof.
  intros [children] chain exec. simpl. rewrite flatten_item_tasks. reflexivity.
Qed.

Lemma flatten_expand_item : forall child chain exec,
  flatten_all (expand_item child chain exec) = compile_item chain exec child.
Proof.
  intros child chain exec.
  destruct child as [e | l body | m body | [| source rest] | [| if_true] [| if_false]];
    simpl.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - rewrite flatten_all_app, flatten_alt_tasks. simpl. reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

(** The number of machine steps of each task: one per expansion and one per
    instruction. *)
Fixpoint term_weight (subject : term) : nat :=
  match subject with
  | Node children => 2 + items_weight children
  end
with items_weight (children : items) : nat :=
  match children with
  | INil => 0
  | ICons child rest => item_weight child + items_weight rest
  end
with item_weight (child : item) : nat :=
  match child with
  | Local _ => 2
  | Scoped _ body => 2 + term_weight body
  | Child _ body => 2 + term_weight body
  | Alts sources => 1 + alts_weight sources
  | Cond if_true if_false => 3 + branch_weight if_true + branch_weight if_false
  end
with alts_weight (sources : forest) : nat :=
  match sources with
  | FNil => 0
  | FCons source rest => 1 + term_weight source + alts_weight rest
  end
with branch_weight (choice : branch) : nat :=
  match choice with
  | BNone => 1
  | BSome body => term_weight body
  end.

Definition task_weight (pending : task) : nat :=
  match pending with
  | TVisit subject _ _ => term_weight subject
  | TItem child _ _ => item_weight child
  | TInstr _ => 1
  end.

Fixpoint work_weight (work : list task) : nat :=
  match work with
  | [] => 0
  | pending :: rest => task_weight pending + work_weight rest
  end.

Lemma work_weight_app : forall left_work right_work,
  work_weight (left_work ++ right_work) = work_weight left_work + work_weight right_work.
Proof.
  induction left_work as [| pending left_work IH]; intros right_work; simpl.
  - reflexivity.
  - rewrite IH. lia.
Qed.

Lemma work_weight_item_tasks : forall chain exec children,
  work_weight (item_tasks chain exec children) = items_weight children.
Proof.
  intros chain exec children.
  induction children as [| child rest IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma work_weight_alt_tasks : forall chain exec sources,
  work_weight (alt_tasks chain exec sources) = alts_weight sources.
Proof.
  intros chain exec sources.
  induction sources as [| source rest IH]; simpl.
  - reflexivity.
  - rewrite IH. lia.
Qed.

Lemma task_weight_branch_task : forall chain exec choice,
  task_weight (branch_task chain exec choice) = branch_weight choice.
Proof.
  intros chain exec [| body]; reflexivity.
Qed.

Lemma expand_visit_weight : forall subject chain exec,
  S (work_weight (expand_visit subject chain exec)) = term_weight subject.
Proof.
  intros [children] chain exec. simpl. rewrite work_weight_item_tasks. reflexivity.
Qed.

Lemma expand_item_weight : forall child chain exec,
  S (work_weight (expand_item child chain exec)) = item_weight child.
Proof.
  intros child chain exec.
  destruct child as [e | l body | m body | [| source rest] | if_true if_false]; simpl.
  - reflexivity.
  - lia.
  - lia.
  - reflexivity.
  - rewrite work_weight_app, work_weight_alt_tasks. simpl. lia.
  - rewrite !task_weight_branch_task. lia.
Qed.

(** The number of syntax nodes of a term. *)
Fixpoint term_nodes (subject : term) : nat :=
  match subject with
  | Node children => 1 + items_nodes children
  end
with items_nodes (children : items) : nat :=
  match children with
  | INil => 0
  | ICons child rest => item_nodes child + items_nodes rest
  end
with item_nodes (child : item) : nat :=
  match child with
  | Local _ => 1
  | Scoped _ body => 1 + term_nodes body
  | Child _ body => 1 + term_nodes body
  | Alts sources => 1 + forest_nodes sources
  | Cond if_true if_false => 1 + branch_nodes if_true + branch_nodes if_false
  end
with forest_nodes (sources : forest) : nat :=
  match sources with
  | FNil => 0
  | FCons source rest => term_nodes source + forest_nodes rest
  end
with branch_nodes (choice : branch) : nat :=
  match choice with
  | BNone => 1
  | BSome body => term_nodes body
  end.

(** The machine takes at most three steps per syntax node. *)
Theorem iterative_walk_steps_are_linear :
  (forall subject, 1 + term_weight subject <= 3 * term_nodes subject) /\
  (forall children, items_weight children <= 3 * items_nodes children) /\
  (forall child, item_weight child <= 3 * item_nodes child) /\
  (forall sources, alts_weight sources <= 3 * forest_nodes sources) /\
  (forall choice, branch_weight choice <= 3 * branch_nodes choice).
Proof.
  apply walk_mutind; intros; simpl in *; lia.
Qed.

Section Walk.

(** The demand algebra: the Rust [SignedDemand] with [default], [combine],
    [alternative] and the lane that [SignedFinish] adds when a signed body
    has no introduction. *)
Variable D : Type.
Variable empty : D.
Variable combine : D -> D -> D.
Variable alternative : D -> D -> D.
Variable finish : option lane -> D -> D.
(** A local event as the recursion applies it: it reads the enclosing lanes
    outer to inner and the execution flag. *)
Variable apply : list (option lane) -> bool -> event -> D -> D.
(** The same event as the machine applies it: it follows the parent links,
    so it reads the lanes inner to outer. *)
Variable apply_chain : list (option lane) -> bool -> event -> D -> D.

(** The recursion of HEAD 17e07307f. A signed region appends its lane to the
    scope ([scopes.to_vec()] and [push]). A match folds its alternatives left
    to right and combines the result, which is the empty demand when no case
    has a source. *)
Fixpoint eval (scope : list (option lane)) (exec : bool) (subject : term) {struct subject} : D :=
  match subject with
  | Node children => eval_items scope exec children empty
  end
with eval_items (scope : list (option lane)) (exec : bool) (children : items) (acc : D)
    {struct children} : D :=
  match children with
  | INil => acc
  | ICons child rest => eval_items scope exec rest (eval_item scope exec child acc)
  end
with eval_item (scope : list (option lane)) (exec : bool) (child : item) (acc : D)
    {struct child} : D :=
  match child with
  | Local e => apply scope exec e acc
  | Scoped l body => combine acc (finish l (eval (scope ++ [l]) true body))
  | Child m body => combine acc (eval (child_scope m scope) (child_exec m exec) body)
  | Alts sources =>
      combine acc
        (match sources with
         | FNil => empty
         | FCons source rest => eval_alts scope exec rest (eval scope exec source)
         end)
  | Cond if_true if_false =>
      combine acc (alternative (eval_branch scope exec if_true) (eval_branch scope exec if_false))
  end
with eval_alts (scope : list (option lane)) (exec : bool) (sources : forest) (acc : D)
    {struct sources} : D :=
  match sources with
  | FNil => acc
  | FCons source rest => eval_alts scope exec rest (alternative acc (eval scope exec source))
  end
with eval_branch (scope : list (option lane)) (exec : bool) (choice : branch) {struct choice} : D :=
  match choice with
  | BNone => empty
  | BSome body => eval scope exec body
  end.

(** The value-stack machine. A missing operand is a stuck machine. *)
Fixpoint run (program : list instruction) (values : list D) : option (list D) :=
  match program, values with
  | [], _ => Some values
  | IPush :: rest, _ => run rest (empty :: values)
  | ILocal chain exec e :: rest, value :: others => run rest (apply_chain chain exec e value :: others)
  | ICombine :: rest, child :: parent :: others => run rest (combine parent child :: others)
  | IAlternative :: rest, later :: earlier :: others =>
      run rest (alternative earlier later :: others)
  | IFinish l :: rest, child :: parent :: others => run rest (combine parent (finish l child) :: others)
  | _ :: _, _ => None
  end.

(** IN's [add_scope_demand] and [add_scope_transfer] walk the chain inner to
    outer, and HEAD's walk the slice outer to inner. *)
Hypothesis apply_chain_reverses : forall chain exec e value,
  apply_chain chain exec e value = apply (rev chain) exec e value.

(** HEAD combines an empty demand for a match without sources, and IN skips
    the combination. [SignedDemand::combine] with the default demand returns
    its left operand (delta_sigma.rs: [add_lane_demands] adds nothing,
    [ResourceMultiset::checked_add] of an empty multiset clones, the first
    rejection reason and the introduction flag are kept). *)
Hypothesis combine_empty_right : forall value, combine value empty = value.

Theorem demand_compile_run_equivalence :
  (forall subject chain exec program values,
      run (compile_term chain exec subject ++ program) values =
      run program (eval (rev chain) exec subject :: values)) /\
  (forall children chain exec program acc values,
      run (compile_items chain exec children ++ program) (acc :: values) =
      run program (eval_items (rev chain) exec children acc :: values)) /\
  (forall child chain exec program acc values,
      run (compile_item chain exec child ++ program) (acc :: values) =
      run program (eval_item (rev chain) exec child acc :: values)) /\
  (forall sources,
      (forall chain exec program acc values,
          run (compile_alts chain exec sources ++ program) (acc :: values) =
          run program (eval_alts (rev chain) exec sources acc :: values)) /\
      (forall chain exec program acc values,
          run (compile_item chain exec (Alts sources) ++ program) (acc :: values) =
          run program (eval_item (rev chain) exec (Alts sources) acc :: values))) /\
  (forall choice chain exec program values,
      run (compile_branch chain exec choice ++ program) values =
      run program (eval_branch (rev chain) exec choice :: values)).
Proof.
  apply walk_mutind.
  - (* Node *)
    intros children IH chain exec program values. simpl. apply IH.
  - (* INil *)
    intros chain exec program acc values. reflexivity.
  - (* ICons *)
    intros child IHchild rest IHrest chain exec program acc values. simpl.
    rewrite <- app_assoc. rewrite IHchild. apply IHrest.
  - (* Local *)
    intros e chain exec program acc values. simpl.
    rewrite apply_chain_reverses. reflexivity.
  - (* Scoped *)
    intros l body IH chain exec program acc values. simpl.
    rewrite <- app_assoc. rewrite IH. reflexivity.
  - (* Child *)
    intros m body IH chain exec program acc values.
    destruct m; simpl; rewrite <- app_assoc; rewrite IH; reflexivity.
  - (* Alts *)
    intros sources IH chain exec program acc values. apply (proj2 IH).
  - (* Cond *)
    intros if_true IHtrue if_false IHfalse chain exec program acc values. simpl.
    rewrite <- !app_assoc. rewrite IHtrue. rewrite IHfalse. reflexivity.
  - (* FNil *)
    split.
    + intros chain exec program acc values. reflexivity.
    + intros chain exec program acc values. simpl. rewrite combine_empty_right. reflexivity.
  - (* FCons *)
    intros source IHsource rest IHrest. split.
    + intros chain exec program acc values. simpl.
      rewrite <- app_assoc. rewrite IHsource. simpl. apply (proj1 IHrest).
    + intros chain exec program acc values. simpl.
      rewrite <- !app_assoc. rewrite IHsource. rewrite (proj1 IHrest). reflexivity.
  - (* BNone *)
    intros chain exec program values. reflexivity.
  - (* BSome *)
    intros body IH chain exec program values. simpl. apply IH.
Qed.

Corollary demand_pda_equals_recursion : forall subject,
  run (compile_term [] true subject) [] = Some [eval [] true subject].
Proof.
  intro subject.
  pose proof (proj1 demand_compile_run_equivalence subject [] true [] []) as equivalence.
  rewrite app_nil_r in equivalence. exact equivalence.
Qed.

(** One instruction of the machine, as the lazy work stack executes it. *)
Definition step (instr : instruction) (values : list D) : option (list D) :=
  match instr, values with
  | IPush, _ => Some (empty :: values)
  | ILocal chain exec e, value :: others => Some (apply_chain chain exec e value :: others)
  | ICombine, child :: parent :: others => Some (combine parent child :: others)
  | IAlternative, later :: earlier :: others => Some (alternative earlier later :: others)
  | IFinish l, child :: parent :: others => Some (combine parent (finish l child) :: others)
  | _, _ => None
  end.

Lemma run_cons_step : forall instr rest values,
  run (instr :: rest) values =
  match step instr values with
  | Some next => run rest next
  | None => None
  end.
Proof.
  intros instr rest values.
  destruct instr; destruct values as [| top [| below others]]; reflexivity.
Qed.

(** The Rust machine: it pops one task, expands a visit or an item in place,
    or executes an instruction. The fuel bounds the number of steps. *)
Fixpoint lazy_run (fuel : nat) (work : list task) (values : list D) : option (list D) :=
  match fuel with
  | 0 => None
  | S fuel' =>
      match work with
      | [] => Some values
      | TVisit subject chain exec :: rest =>
          lazy_run fuel' (expand_visit subject chain exec ++ rest) values
      | TItem child chain exec :: rest =>
          lazy_run fuel' (expand_item child chain exec ++ rest) values
      | TInstr instr :: rest =>
          match step instr values with
          | Some next => lazy_run fuel' rest next
          | None => None
          end
      end
  end.

Theorem lazy_run_runs_the_flattened_program : forall fuel work values,
  work_weight work < fuel ->
  lazy_run fuel work values = run (flatten_all work) values.
Proof.
  induction fuel as [| fuel IH]; intros work values bound.
  - lia.
  - destruct work as [| pending rest].
    + reflexivity.
    + destruct pending as [subject chain exec | child chain exec | instr];
        simpl in bound.
      * change (lazy_run fuel (expand_visit subject chain exec ++ rest) values =
                run (compile_term chain exec subject ++ flatten_all rest) values).
        rewrite IH.
        -- rewrite flatten_all_app, flatten_expand_visit. reflexivity.
        -- rewrite work_weight_app.
           pose proof (expand_visit_weight subject chain exec). lia.
      * change (lazy_run fuel (expand_item child chain exec ++ rest) values =
                run (compile_item chain exec child ++ flatten_all rest) values).
        rewrite IH.
        -- rewrite flatten_all_app, flatten_expand_item. reflexivity.
        -- rewrite work_weight_app.
           pose proof (expand_item_weight child chain exec). lia.
      * change (match step instr values with
                | Some next => lazy_run fuel rest next
                | None => None
                end = run (instr :: flatten_all rest) values).
        rewrite run_cons_step.
        destruct (step instr values) as [next |].
        -- apply IH. lia.
        -- reflexivity.
Qed.

(** The headline refinement: the Rust work stack, started with one visit of
    the root in execution position with no scope, ends with the value of the
    recursion and nothing else. *)
Theorem iterative_walk_refines_recursion : forall subject,
  lazy_run (S (term_weight subject)) [TVisit subject [] true] [] =
  Some [eval [] true subject].
Proof.
  intro subject.
  rewrite lazy_run_runs_the_flattened_program.
  - simpl. rewrite app_nil_r. apply demand_pda_equals_recursion.
  - simpl. lia.
Qed.

End Walk.

(** * 4. The scope arena represents persistent scope chains *)

(** A [ScopeLink]: the index of the enclosing link and the lane of the
    region. The machine pushes a link when it enters a signed body and never
    removes one, so a task's index keeps denoting the same chain. *)
Definition link : Type := (option nat * option lane)%type.

Fixpoint chain_walk (fuel : nat) (arena : list link) (scope : option nat) : list (option lane) :=
  match fuel with
  | 0 => []
  | S fuel' =>
      match scope with
      | None => []
      | Some index =>
          match nth_error arena index with
          | Some (parent, l) => l :: chain_walk fuel' arena parent
          | None => []
          end
      end
  end.

Definition chain_of (arena : list link) (scope : option nat) : list (option lane) :=
  chain_walk (length arena) arena scope.

Definition parents_precede (arena : list link) : Prop :=
  forall index parent l, nth_error arena index = Some (Some parent, l) -> parent < index.

Definition scope_within (arena : list link) (scope : option nat) : Prop :=
  match scope with
  | None => True
  | Some index => index < length arena
  end.

Lemma chain_walk_none : forall fuel arena, chain_walk fuel arena None = [].
Proof.
  intros [| fuel] arena; reflexivity.
Qed.

Lemma chain_walk_fuel_irrelevant : forall arena,
  parents_precede arena ->
  forall fuel fuel' index, index < fuel -> index < fuel' ->
  chain_walk fuel arena (Some index) = chain_walk fuel' arena (Some index).
Proof.
  intros arena precede fuel.
  induction fuel as [| fuel IH]; intros fuel' index bound bound'.
  - lia.
  - destruct fuel' as [| fuel']; [lia |]. simpl.
    destruct (nth_error arena index) as [[[parent |] l] |] eqn:entry.
    + pose proof (precede _ _ _ entry) as ordered.
      f_equal. apply IH; lia.
    + rewrite !chain_walk_none. reflexivity.
    + reflexivity.
Qed.

Lemma chain_walk_append : forall arena suffix,
  parents_precede arena ->
  forall fuel index, index < length arena ->
  chain_walk fuel (arena ++ suffix) (Some index) = chain_walk fuel arena (Some index).
Proof.
  intros arena suffix precede fuel.
  induction fuel as [| fuel IH]; intros index bound; [reflexivity |].
  simpl. rewrite nth_error_app1 by exact bound.
  destruct (nth_error arena index) as [[[parent |] l] |] eqn:entry.
  - pose proof (precede _ _ _ entry) as ordered.
    f_equal. apply IH. lia.
  - rewrite !chain_walk_none. reflexivity.
  - reflexivity.
Qed.

Theorem arena_append_preserves_chain : forall arena suffix index,
  parents_precede arena -> index < length arena ->
  chain_of (arena ++ suffix) (Some index) = chain_of arena (Some index).
Proof.
  intros arena suffix index precede bound. unfold chain_of.
  rewrite chain_walk_append by assumption.
  apply chain_walk_fuel_irrelevant; [assumption | rewrite length_app; lia | assumption].
Qed.

Theorem arena_push_extends_chain : forall arena parent l,
  parents_precede arena -> scope_within arena parent ->
  chain_of (arena ++ [(parent, l)]) (Some (length arena)) = l :: chain_of arena parent.
Proof.
  intros arena parent l precede within. unfold chain_of.
  replace (length (arena ++ [(parent, l)])) with (S (length arena))
    by (rewrite length_app; simpl; lia).
  simpl. rewrite nth_error_app2 by lia. rewrite Nat.sub_diag. simpl.
  f_equal.
  destruct parent as [index |].
  - simpl in within. apply chain_walk_append; assumption.
  - rewrite !chain_walk_none. reflexivity.
Qed.

Theorem arena_push_preserves_order : forall arena parent l,
  parents_precede arena -> scope_within arena parent ->
  parents_precede (arena ++ [(parent, l)]).
Proof.
  intros arena parent l precede within index parent' l' entry.
  destruct (Nat.lt_ge_cases index (length arena)) as [inside | outside].
  - rewrite nth_error_app1 in entry by exact inside. exact (precede _ _ _ entry).
  - rewrite nth_error_app2 in entry by exact outside.
    destruct (index - length arena) as [| offset] eqn:position; simpl in entry.
    + inversion entry; subst. simpl in within. lia.
    + destruct offset; simpl in entry; discriminate.
Qed.

(** * 5. Negative controls *)

(** A trace algebra: each local event records the lanes that it read. *)
Definition trace : Type := list (list (option lane) * event).
Definition trace_empty : trace := [].
Definition trace_combine (parent child : trace) : trace := parent ++ child.
Definition trace_alternative (earlier later : trace) : trace := earlier ++ later.
Definition trace_finish (_ : option lane) (child : trace) : trace := child.
Definition trace_apply (scope : list (option lane)) (_ : bool) (e : event) (value : trace)
  : trace := value ++ [(scope, e)].
Definition trace_apply_chain (chain : list (option lane)) (exec : bool) (e : event)
    (value : trace) : trace :=
  trace_apply (rev chain) exec e value.

Lemma trace_apply_chain_reverses : forall chain exec e value,
  trace_apply_chain chain exec e value = trace_apply (rev chain) exec e value.
Proof. reflexivity. Qed.

Lemma trace_combine_empty_right : forall value, trace_combine value trace_empty = value.
Proof. intro value. apply app_nil_r. Qed.

(** The trace algebra satisfies both premises, so the refinement holds for
    it. *)
Corollary trace_machine_refines_recursion : forall subject,
  lazy_run trace trace_empty trace_combine trace_alternative trace_finish trace_apply_chain
    (S (term_weight subject)) [TVisit subject [] true] [] =
  Some [eval trace trace_empty trace_combine trace_alternative trace_finish trace_apply
          [] true subject].
Proof.
  intro subject.
  apply (iterative_walk_refines_recursion trace trace_empty trace_combine trace_alternative
           trace_finish trace_apply trace_apply_chain trace_apply_chain_reverses
           trace_combine_empty_right).
Qed.

(** A machine that pushes a signed region's lane on a scope stack and does not
    pop it after the region: the later siblings still read the inner lane. *)
Fixpoint leaky_items (chain : list (option lane)) (exec : bool) (children : items)
  : list instruction :=
  match children with
  | INil => []
  | ICons (Scoped l body) rest =>
      compile_term (l :: chain) true body ++ IFinish l :: leaky_items (l :: chain) exec rest
  | ICons child rest => compile_item chain exec child ++ leaky_items chain exec rest
  end.

Definition leaky_term (chain : list (option lane)) (exec : bool) (subject : term)
  : list instruction :=
  match subject with
  | Node children => IPush :: leaky_items chain exec children
  end.

(** A signed region with an empty body, followed by an event of the parent. *)
Definition leak_witness : term :=
  Node (ICons (Scoped (Some 1) (Node INil)) (ICons (Local 7) INil)).

Theorem skipped_scope_pop_breaks_the_refinement :
  run trace trace_empty trace_combine trace_alternative trace_finish trace_apply_chain
    (leaky_term [] true leak_witness) [] <>
  Some [eval trace trace_empty trace_combine trace_alternative trace_finish trace_apply
          [] true leak_witness].
Proof.
  vm_compute. intro same. discriminate same.
Qed.

(** A machine that pushes a node's items on the LIFO stack in source order, so
    that they run in reverse order. *)
Fixpoint reversed_items (chain : list (option lane)) (exec : bool) (children : items)
  : list instruction :=
  match children with
  | INil => []
  | ICons child rest => reversed_items chain exec rest ++ compile_item chain exec child
  end.

Definition reversed_term (chain : list (option lane)) (exec : bool) (subject : term)
  : list instruction :=
  match subject with
  | Node children => IPush :: reversed_items chain exec children
  end.

(** Two events of one node, such as a rejection and a later one. *)
Definition order_witness : term := Node (ICons (Local 1) (ICons (Local 2) INil)).

Theorem reordered_visit_breaks_the_refinement :
  run trace trace_empty trace_combine trace_alternative trace_finish trace_apply_chain
    (reversed_term [] true order_witness) [] <>
  Some [eval trace trace_empty trace_combine trace_alternative trace_finish trace_apply
          [] true order_witness].
Proof.
  vm_compute. intro same. discriminate same.
Qed.

(** * 6. Lane bumps commute *)

(** The lanes that a demand counts, as a sorted bag: one copy of a lane for
    each unit of demand. A bump inserts one copy. *)
Fixpoint insert_lane (x : nat) (bag : list nat) : list nat :=
  match bag with
  | [] => [x]
  | y :: rest => if x <=? y then x :: bag else y :: insert_lane x rest
  end.

Ltac split_comparisons :=
  repeat match goal with
         | |- context [?a <=? ?b] => destruct (Nat.leb_spec a b); simpl
         end.

Lemma insert_lane_comm : forall bag x y,
  insert_lane x (insert_lane y bag) = insert_lane y (insert_lane x bag).
Proof.
  induction bag as [| z rest IH]; intros x y.
  - simpl. split_comparisons; try reflexivity; try (exfalso; lia).
    assert (x = y) as same by lia. subst. reflexivity.
  - simpl. split_comparisons; try reflexivity; try (exfalso; lia);
      try (f_equal; apply IH);
      try (assert (x = y) as same by lia; subst; reflexivity).
Qed.

(** [add_scope_demand] bumps each lane of the scope once. The unit region owns
    no lane. *)
Fixpoint bump_lanes (lanes : list (option lane)) (bag : list nat) : list nat :=
  match lanes with
  | [] => bag
  | Some x :: rest => bump_lanes rest (insert_lane x bag)
  | None :: rest => bump_lanes rest bag
  end.

Lemma bump_lanes_insert : forall lanes x bag,
  bump_lanes lanes (insert_lane x bag) = insert_lane x (bump_lanes lanes bag).
Proof.
  induction lanes as [| [y |] lanes IH]; intros x bag; simpl.
  - reflexivity.
  - rewrite insert_lane_comm. apply IH.
  - apply IH.
Qed.

Lemma bump_lanes_app : forall first_lanes later_lanes bag,
  bump_lanes (first_lanes ++ later_lanes) bag = bump_lanes later_lanes (bump_lanes first_lanes bag).
Proof.
  induction first_lanes as [| [x |] first_lanes IH]; intros later_lanes bag; simpl;
    [reflexivity | apply IH | apply IH].
Qed.

(** The demand that IN's chain walk adds (inner to outer) equals the demand
    that HEAD's slice walk adds (outer to inner). This discharges
    [apply_chain_reverses] for the lane counters. *)
Theorem bump_lanes_reverse : forall lanes bag,
  bump_lanes (rev lanes) bag = bump_lanes lanes bag.
Proof.
  induction lanes as [| l lanes IH]; intros bag; simpl; [reflexivity |].
  rewrite bump_lanes_app. destruct l as [x |]; simpl.
  - rewrite IH. symmetry. apply bump_lanes_insert.
  - apply IH.
Qed.

Definition bag_apply (scope : list (option lane)) (_ : bool) (_ : event) (bag : list nat)
  : list nat := bump_lanes scope bag.

Definition bag_apply_chain (chain : list (option lane)) (_ : bool) (_ : event)
    (bag : list nat) : list nat :=
  bump_lanes chain bag.

Corollary scope_demand_reads_the_chain_in_either_direction : forall chain exec e bag,
  bag_apply_chain chain exec e bag = bag_apply (rev chain) exec e bag.
Proof.
  intros chain exec e bag. unfold bag_apply_chain, bag_apply.
  symmetry. apply bump_lanes_reverse.
Qed.

(** * 7. The first-error worklist of collect and has_bound_level *)

(** The events of a term in the order of the recursion: a node's items in
    order, each child's events where the child appears. *)
Fixpoint events_term (subject : term) : list event :=
  match subject with
  | Node children => events_items children
  end
with events_items (children : items) : list event :=
  match children with
  | INil => []
  | ICons child rest => events_item child ++ events_items rest
  end
with events_item (child : item) : list event :=
  match child with
  | Local e => [e]
  | Scoped _ body => events_term body
  | Child _ body => events_term body
  | Alts sources => events_forest sources
  | Cond if_true if_false => events_branch if_true ++ events_branch if_false
  end
with events_forest (sources : forest) : list event :=
  match sources with
  | FNil => []
  | FCons source rest => events_term source ++ events_forest rest
  end
with events_branch (choice : branch) : list event :=
  match choice with
  | BNone => []
  | BSome body => events_term body
  end.

(** The tasks of the IN worklist [CollectTask]: a visit, an item or one
    event ([RequiredSignature], [RequiredBody] or [Stack]). *)
Inductive collect_task : Type :=
| CVisit : term -> collect_task
| CItem : item -> collect_task
| CEmit : event -> collect_task.

Fixpoint collect_items (children : items) : list collect_task :=
  match children with
  | INil => []
  | ICons child rest => CItem child :: collect_items rest
  end.

Fixpoint collect_forest (sources : forest) : list collect_task :=
  match sources with
  | FNil => []
  | FCons source rest => CVisit source :: collect_forest rest
  end.

Definition collect_branch (choice : branch) : list collect_task :=
  match choice with
  | BNone => []
  | BSome body => [CVisit body]
  end.

Definition collect_expand (pending : collect_task) : list collect_task :=
  match pending with
  | CVisit (Node children) => collect_items children
  | CItem (Local e) => [CEmit e]
  | CItem (Scoped _ body) => [CVisit body]
  | CItem (Child _ body) => [CVisit body]
  | CItem (Alts sources) => collect_forest sources
  | CItem (Cond if_true if_false) => collect_branch if_true ++ collect_branch if_false
  | CEmit e => [CEmit e]
  end.

Definition collect_events (pending : collect_task) : list event :=
  match pending with
  | CVisit subject => events_term subject
  | CItem child => events_item child
  | CEmit e => [e]
  end.

Fixpoint collect_all (work : list collect_task) : list event :=
  match work with
  | [] => []
  | pending :: rest => collect_events pending ++ collect_all rest
  end.

Lemma collect_all_app : forall first_work later_work,
  collect_all (first_work ++ later_work) = collect_all first_work ++ collect_all later_work.
Proof.
  induction first_work as [| pending first_work IH]; intros later_work; simpl.
  - reflexivity.
  - rewrite IH, app_assoc. reflexivity.
Qed.

Lemma collect_expand_events : forall pending,
  (match pending with CEmit _ => False | _ => True end) ->
  collect_all (collect_expand pending) = collect_events pending.
Proof.
  intros pending relevant.
  destruct pending as [[children] | child | e]; simpl in relevant |- *;
    [clear relevant | clear relevant | contradiction].
  - induction children as [| child rest IH]; simpl; [reflexivity |].
    rewrite IH. reflexivity.
  - destruct child as [e | l body | m body | sources | if_true if_false]; simpl.
    + reflexivity.
    + rewrite app_nil_r. reflexivity.
    + rewrite app_nil_r. reflexivity.
    + induction sources as [| source rest IH]; simpl; [reflexivity |].
      rewrite IH. reflexivity.
    + rewrite collect_all_app.
      destruct if_true, if_false; simpl; rewrite ?app_nil_r; reflexivity.
Qed.

(** The size of a task, which bounds the steps that it needs. *)
Fixpoint term_size (subject : term) : nat :=
  match subject with
  | Node children => S (items_size children)
  end
with items_size (children : items) : nat :=
  match children with
  | INil => 0
  | ICons child rest => item_size child + items_size rest
  end
with item_size (child : item) : nat :=
  match child with
  | Local _ => 2
  | Scoped _ body => S (term_size body)
  | Child _ body => S (term_size body)
  | Alts sources => S (forest_size sources)
  | Cond if_true if_false => S (branch_size if_true + branch_size if_false)
  end
with forest_size (sources : forest) : nat :=
  match sources with
  | FNil => 0
  | FCons source rest => term_size source + forest_size rest
  end
with branch_size (choice : branch) : nat :=
  match choice with
  | BNone => 0
  | BSome body => term_size body
  end.

Definition collect_size (pending : collect_task) : nat :=
  match pending with
  | CVisit subject => term_size subject
  | CItem child => item_size child
  | CEmit _ => 1
  end.

Fixpoint collect_work_size (work : list collect_task) : nat :=
  match work with
  | [] => 0
  | pending :: rest => collect_size pending + collect_work_size rest
  end.

Lemma collect_work_size_app : forall first_work later_work,
  collect_work_size (first_work ++ later_work) =
  collect_work_size first_work + collect_work_size later_work.
Proof.
  induction first_work as [| pending first_work IH]; intros later_work; simpl.
  - reflexivity.
  - rewrite IH. lia.
Qed.

Lemma collect_expand_size : forall pending,
  (match pending with CEmit _ => False | _ => True end) ->
  S (collect_work_size (collect_expand pending)) = collect_size pending.
Proof.
  intros pending relevant.
  destruct pending as [[children] | child | e]; simpl in relevant |- *;
    [clear relevant | clear relevant | contradiction].
  - induction children as [| child rest IH]; simpl; [reflexivity |].
    lia.
  - destruct child as [e | l body | m body | sources | if_true if_false]; simpl.
    + reflexivity.
    + lia.
    + lia.
    + induction sources as [| source rest IH]; simpl; [reflexivity |].
      lia.
    + rewrite collect_work_size_app.
      destruct if_true, if_false; simpl; lia.
Qed.

Section Collect.

(** The accumulator of [collect] (the signature map) and one event of it,
    which may fail. *)
Variable A : Type.
Variable visit : A -> event -> option A.

(** The recursion stops at the first failing event. *)
Fixpoint fold_events (events : list event) (acc : A) : option A :=
  match events with
  | [] => Some acc
  | e :: rest =>
      match visit acc e with
      | Some next => fold_events rest next
      | None => None
      end
  end.

Lemma fold_events_app : forall first_events later_events acc,
  fold_events (first_events ++ later_events) acc =
  match fold_events first_events acc with
  | Some next => fold_events later_events next
  | None => None
  end.
Proof.
  induction first_events as [| e first_events IH]; intros later_events acc; simpl.
  - reflexivity.
  - destruct (visit acc e) as [next |]; [apply IH | reflexivity].
Qed.

(** The worklist pops one task; an event runs [visit] and stops on failure. *)
Fixpoint collect_run (fuel : nat) (work : list collect_task) (acc : A) : option A :=
  match fuel with
  | 0 => None
  | S fuel' =>
      match work with
      | [] => Some acc
      | CEmit e :: rest =>
          match visit acc e with
          | Some next => collect_run fuel' rest next
          | None => None
          end
      | pending :: rest => collect_run fuel' (collect_expand pending ++ rest) acc
      end
  end.

Theorem collect_run_folds_the_events : forall fuel work acc,
  collect_work_size work < fuel ->
  collect_run fuel work acc = fold_events (collect_all work) acc.
Proof.
  induction fuel as [| fuel IH]; intros work acc bound.
  - lia.
  - destruct work as [| pending rest]; [reflexivity |].
    destruct pending as [subject | child | e]; simpl in bound.
    + change (collect_run fuel (collect_expand (CVisit subject) ++ rest) acc =
              fold_events (events_term subject ++ collect_all rest) acc).
      rewrite IH.
      * rewrite collect_all_app, (collect_expand_events (CVisit subject) I). reflexivity.
      * rewrite collect_work_size_app.
        pose proof (collect_expand_size (CVisit subject) I) as size.
        change (collect_size (CVisit subject)) with (term_size subject) in size. lia.
    + change (collect_run fuel (collect_expand (CItem child) ++ rest) acc =
              fold_events (events_item child ++ collect_all rest) acc).
      rewrite IH.
      * rewrite collect_all_app, (collect_expand_events (CItem child) I). reflexivity.
      * rewrite collect_work_size_app.
        pose proof (collect_expand_size (CItem child) I) as size.
        change (collect_size (CItem child)) with (item_size child) in size. lia.
    + change (match visit acc e with
              | Some next => collect_run fuel rest next
              | None => None
              end = fold_events (e :: collect_all rest) acc).
      simpl. destruct (visit acc e) as [next |]; [apply IH; lia | reflexivity].
Qed.

(** The recursion of HEAD: each item in order, each child where it appears,
    and the first failure ends the walk. *)
Fixpoint collect_term (subject : term) (acc : A) {struct subject} : option A :=
  match subject with
  | Node children => collect_items_rec children acc
  end
with collect_items_rec (children : items) (acc : A) {struct children} : option A :=
  match children with
  | INil => Some acc
  | ICons child rest =>
      match collect_item_rec child acc with
      | Some next => collect_items_rec rest next
      | None => None
      end
  end
with collect_item_rec (child : item) (acc : A) {struct child} : option A :=
  match child with
  | Local e => visit acc e
  | Scoped _ body => collect_term body acc
  | Child _ body => collect_term body acc
  | Alts sources => collect_forest_rec sources acc
  | Cond if_true if_false =>
      match collect_branch_rec if_true acc with
      | Some next => collect_branch_rec if_false next
      | None => None
      end
  end
with collect_forest_rec (sources : forest) (acc : A) {struct sources} : option A :=
  match sources with
  | FNil => Some acc
  | FCons source rest =>
      match collect_term source acc with
      | Some next => collect_forest_rec rest next
      | None => None
      end
  end
with collect_branch_rec (choice : branch) (acc : A) {struct choice} : option A :=
  match choice with
  | BNone => Some acc
  | BSome body => collect_term body acc
  end.

Theorem collect_recursion_folds_the_events :
  (forall subject acc, collect_term subject acc = fold_events (events_term subject) acc) /\
  (forall children acc, collect_items_rec children acc = fold_events (events_items children) acc) /\
  (forall child acc, collect_item_rec child acc = fold_events (events_item child) acc) /\
  (forall sources acc, collect_forest_rec sources acc = fold_events (events_forest sources) acc) /\
  (forall choice acc, collect_branch_rec choice acc = fold_events (events_branch choice) acc).
Proof.
  apply walk_mutind.
  - intros children IH acc. simpl. apply IH.
  - intros acc. reflexivity.
  - intros child IHchild rest IHrest acc. simpl.
    rewrite fold_events_app, IHchild.
    destruct (fold_events (events_item child) acc); [apply IHrest | reflexivity].
  - intros e acc. simpl. destruct (visit acc e); reflexivity.
  - intros l body IH acc. simpl. apply IH.
  - intros m body IH acc. simpl. apply IH.
  - intros sources IH acc. simpl. apply IH.
  - intros if_true IHtrue if_false IHfalse acc. simpl.
    rewrite fold_events_app, IHtrue.
    destruct (fold_events (events_branch if_true) acc); [apply IHfalse | reflexivity].
  - intros acc. reflexivity.
  - intros source IHsource rest IHrest acc. simpl.
    rewrite fold_events_app, IHsource.
    destruct (fold_events (events_term source) acc); [apply IHrest | reflexivity].
  - intros acc. reflexivity.
  - intros body IH acc. simpl. apply IH.
Qed.

(** The worklist returns the value of the recursion, or its first failure. *)
Theorem collect_worklist_refines_recursion : forall subject acc,
  collect_run (S (term_size subject)) [CVisit subject] acc = collect_term subject acc.
Proof.
  intros subject acc.
  rewrite collect_run_folds_the_events by (simpl; lia).
  rewrite (proj1 collect_recursion_folds_the_events). simpl. rewrite app_nil_r.
  reflexivity.
Qed.

End Collect.
