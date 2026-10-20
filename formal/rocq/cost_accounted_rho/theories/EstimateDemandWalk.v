(* G1-4a (DR-122): the vector pass of the linear cost estimate refines its
   recursion.

   Pass 2 of delta_sigma/estimate.rs folds the term into the raw-count vector
   V(s,k) of design section 5.5: parallel parts add, a branch point takes the
   componentwise maximum, and a send or a receive adds its counts once for
   each region that it carries. Pass 1 emits the post-order program of that
   fold, the program compile_term of StackSafeDemandWalk.v, and pass 2 runs it
   on a value stack with run.

   This module instantiates Section Walk of StackSafeDemandWalk.v with the
   vector algebra. It covers the vector pass only: the walk term has no
   binders, because pass 1 resolves the binders before it emits the program.

   1. Vectors are lists of naturals indexed by the key of a lane and a class,
      with zeros beyond the end. Addition and maximum extend the shorter list
      with zeros, so the empty list is the zero vector.
   2. A participant adds its per-region counts once for each static lane of
      its chain, and once for the deploy lane outside every signed scope. A
      unit region owns no lane.
   3. [vector_chain_reverses] discharges the chain-direction premise of the
      section: the machine reads the chain inner to outer, and the sum does
      not depend on the order. [add_vec_nil_r] discharges the empty-combine
      premise.
   4. [vector_program_equals_recursion] and [vector_walk_refines_recursion]:
      the emitted program and the lazy work stack give the recursive fold.
   5. [vector_merge_order_irrelevant] and [swapped_vector_run_equals_run]:
      addition and maximum commute, so the Rust machine may move the smaller
      map into the larger one.
   6. [vector_program_length_is_linear]: the program has at most three
      instructions per syntax node.
   7. Negative control [swapped_trace_run_breaks_the_refinement]: with a
      combine that does not commute, the trace algebra of
      StackSafeDemandWalk.v, a machine that swaps the operands of a combine
      no longer computes the recursion. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
Import ListNotations.
From CostAccountedRho Require Import StackSafeDemandWalk.

(** * 1. The vector algebra *)

Fixpoint add_vec (left right : list nat) : list nat :=
  match left, right with
  | [], _ => right
  | _, [] => left
  | x :: xs, y :: ys => (x + y) :: add_vec xs ys
  end.

Fixpoint max_vec (left right : list nat) : list nat :=
  match left, right with
  | [], _ => right
  | _, [] => left
  | x :: xs, y :: ys => Nat.max x y :: max_vec xs ys
  end.

Lemma add_vec_nil_r : forall v, add_vec v [] = v.
Proof. intros [| x xs]; reflexivity. Qed.

Lemma max_vec_nil_r : forall v, max_vec v [] = v.
Proof. intros [| x xs]; reflexivity. Qed.

Lemma add_vec_comm : forall left right, add_vec left right = add_vec right left.
Proof.
  induction left as [| x xs IH]; intros [| y ys]; simpl; try reflexivity.
  rewrite IH. f_equal. lia.
Qed.

Lemma max_vec_comm : forall left right, max_vec left right = max_vec right left.
Proof.
  induction left as [| x xs IH]; intros [| y ys]; simpl; try reflexivity.
  rewrite IH. f_equal. lia.
Qed.

Lemma add_vec_assoc : forall a b c, add_vec a (add_vec b c) = add_vec (add_vec a b) c.
Proof.
  induction a as [| x xs IH]; intros [| y ys] [| z zs]; simpl; try reflexivity.
  rewrite IH. f_equal. lia.
Qed.

(** The vector at index [i]: zero beyond the end. *)
Definition at_key (v : list nat) (i : nat) : nat := nth i v 0.

Lemma at_key_add_vec : forall left right i,
  at_key (add_vec left right) i = at_key left i + at_key right i.
Proof.
  unfold at_key.
  induction left as [| x xs IH]; intros [| y ys] [| i]; simpl; try reflexivity; try lia.
  apply IH.
Qed.

Lemma at_key_max_vec : forall left right i,
  at_key (max_vec left right) i = Nat.max (at_key left i) (at_key right i).
Proof.
  unfold at_key.
  induction left as [| x xs IH]; intros [| y ys] [| i]; simpl; try reflexivity; try lia.
  apply IH.
Qed.

(** The sum of a list of vectors. *)
Definition sum_vecs (vs : list (list nat)) : list nat := fold_right add_vec [] vs.

Lemma sum_vecs_app : forall left right,
  sum_vecs (left ++ right) = add_vec (sum_vecs left) (sum_vecs right).
Proof.
  unfold sum_vecs.
  induction left as [| v vs IH]; intros right; simpl.
  - reflexivity.
  - rewrite IH. apply add_vec_assoc.
Qed.

Lemma sum_vecs_rev : forall vs, sum_vecs (rev vs) = sum_vecs vs.
Proof.
  induction vs as [| v vs IH]; simpl; [reflexivity |].
  rewrite sum_vecs_app, IH. unfold sum_vecs at 2. simpl.
  rewrite add_vec_nil_r. apply add_vec_comm.
Qed.

(** * 2. The contribution of a participant *)

Section Contributions.

(** The lane of the deploy region, which a participant outside every signed
    scope opens. *)
Variable deploy : lane.
(** The key width of a lane: one key for each resource class. *)
Variable width : nat.
(** The per-region counts of a participant, one entry per class. *)
Variable counts : event -> list nat.

(** The counts of one region, placed at the keys of its lane. *)
Definition place (l : lane) (q : list nat) : list nat := repeat 0 (width * l) ++ q.

Definition static_lanes (scope : list (option lane)) : list lane :=
  flat_map (fun region => match region with Some l => [l] | None => [] end) scope.

(** The lanes of the regions that a participant carries: the enclosing
    regions, or one deploy region when no signed scope encloses it. *)
Definition region_lanes (scope : list (option lane)) : list lane :=
  match scope with
  | [] => [deploy]
  | _ :: _ => static_lanes scope
  end.

Definition contribution (scope : list (option lane)) (e : event) : list nat :=
  sum_vecs (map (fun l => place l (counts e)) (region_lanes scope)).

Definition vec_apply (scope : list (option lane)) (_ : bool) (e : event) (value : list nat)
  : list nat := add_vec value (contribution scope e).

(** The machine reads the chain inner to outer, as the parent links of the
    scope arena give it. *)
Definition vec_apply_chain (chain : list (option lane)) (_ : bool) (e : event)
    (value : list nat) : list nat := add_vec value (contribution chain e).

Definition vec_finish (_ : option lane) (value : list nat) : list nat := value.

Lemma static_lanes_rev : forall scope, static_lanes (rev scope) = rev (static_lanes scope).
Proof.
  unfold static_lanes.
  induction scope as [| [l |] scope IH]; simpl; [reflexivity | |].
  - rewrite flat_map_app, IH. simpl. reflexivity.
  - rewrite flat_map_app, IH. simpl. rewrite app_nil_r. reflexivity.
Qed.

Lemma region_lanes_cons : forall scope, scope <> [] -> region_lanes scope = static_lanes scope.
Proof. intros [| region scope] nonempty; [contradiction | reflexivity]. Qed.

Lemma contribution_rev : forall scope e, contribution (rev scope) e = contribution scope e.
Proof.
  intros [| region scope] e; [reflexivity |].
  unfold contribution.
  assert (nonempty : rev (region :: scope) <> []).
  { intro empty. apply (f_equal (@length (option lane))) in empty.
    rewrite length_rev in empty. simpl in empty. discriminate. }
  rewrite (region_lanes_cons _ nonempty).
  rewrite (region_lanes_cons (region :: scope)) by discriminate.
  rewrite static_lanes_rev, map_rev. apply sum_vecs_rev.
Qed.

(** The chain-direction premise of Section Walk. *)
Theorem vector_chain_reverses : forall chain exec e value,
  vec_apply_chain chain exec e value = vec_apply (rev chain) exec e value.
Proof.
  intros chain exec e value. unfold vec_apply_chain, vec_apply.
  rewrite contribution_rev. reflexivity.
Qed.

(** * 4. The program and the work stack give the recursive fold *)

Theorem vector_program_equals_recursion : forall subject,
  run (list nat) [] add_vec max_vec vec_finish vec_apply_chain (compile_term [] true subject) [] =
  Some [eval (list nat) [] add_vec max_vec vec_finish vec_apply [] true subject].
Proof.
  intro subject.
  apply (demand_pda_equals_recursion (list nat) [] add_vec max_vec vec_finish vec_apply
           vec_apply_chain vector_chain_reverses add_vec_nil_r).
Qed.

Theorem vector_walk_refines_recursion : forall subject,
  lazy_run (list nat) [] add_vec max_vec vec_finish vec_apply_chain
    (S (term_weight subject)) [TVisit subject [] true] [] =
  Some [eval (list nat) [] add_vec max_vec vec_finish vec_apply [] true subject].
Proof.
  intro subject.
  apply (iterative_walk_refines_recursion (list nat) [] add_vec max_vec vec_finish vec_apply
           vec_apply_chain vector_chain_reverses add_vec_nil_r).
Qed.

End Contributions.

(** * 5. The order of a merge does not matter *)

Theorem vector_merge_order_irrelevant : forall left right,
  add_vec left right = add_vec right left /\ max_vec left right = max_vec right left.
Proof.
  intros left right. split; [apply add_vec_comm | apply max_vec_comm].
Qed.

(** A machine that swaps the operands of every combine and alternative, as a
    merge that moves the smaller map into the larger one may do. *)
Fixpoint run_swapped (D : Type) (empty : D) (combine alternative : D -> D -> D)
    (finish : option lane -> D -> D) (apply_chain : list (option lane) -> bool -> event -> D -> D)
    (program : list instruction) (values : list D) : option (list D) :=
  match program, values with
  | [], _ => Some values
  | IPush :: rest, _ => run_swapped D empty combine alternative finish apply_chain rest (empty :: values)
  | ILocal chain exec e :: rest, value :: others =>
      run_swapped D empty combine alternative finish apply_chain rest
        (apply_chain chain exec e value :: others)
  | ICombine :: rest, child :: parent :: others =>
      run_swapped D empty combine alternative finish apply_chain rest
        (combine child parent :: others)
  | IAlternative :: rest, later :: earlier :: others =>
      run_swapped D empty combine alternative finish apply_chain rest
        (alternative later earlier :: others)
  | IFinish l :: rest, child :: parent :: others =>
      run_swapped D empty combine alternative finish apply_chain rest
        (combine (finish l child) parent :: others)
  | _ :: _, _ => None
  end.

Theorem swapped_vector_run_equals_run : forall apply_chain program values,
  run_swapped (list nat) [] add_vec max_vec vec_finish apply_chain program values =
  run (list nat) [] add_vec max_vec vec_finish apply_chain program values.
Proof.
  intros apply_chain program.
  induction program as [| instr rest IH]; intros values; [reflexivity |].
  destruct instr as [| chain exec e | | | l].
  - simpl. apply IH.
  - destruct values as [| value others]; simpl; [reflexivity | apply IH].
  - destruct values as [| child [| parent others]]; simpl; try reflexivity.
    rewrite (add_vec_comm child parent). apply IH.
  - destruct values as [| later [| earlier others]]; simpl; try reflexivity.
    rewrite (max_vec_comm later earlier). apply IH.
  - destruct values as [| child [| parent others]]; simpl; try reflexivity.
    try unfold vec_finish. rewrite (add_vec_comm child parent). apply IH.
Qed.

(** * 6. The program is linear in the term *)

Theorem compile_length_bounded_by_weight :
  (forall subject chain exec, length (compile_term chain exec subject) <= term_weight subject) /\
  (forall children chain exec, length (compile_items chain exec children) <= items_weight children) /\
  (forall child chain exec, length (compile_item chain exec child) <= item_weight child) /\
  (forall sources chain exec, length (compile_alts chain exec sources) <= alts_weight sources) /\
  (forall choice chain exec, length (compile_branch chain exec choice) <= branch_weight choice).
Proof.
  apply walk_mutind.
  - intros children IH chain exec. simpl. specialize (IH chain exec). lia.
  - intros chain exec. simpl. lia.
  - intros child IHchild rest IHrest chain exec. simpl. rewrite length_app.
    specialize (IHchild chain exec). specialize (IHrest chain exec). lia.
  - intros e chain exec. simpl. lia.
  - intros l body IH chain exec. simpl. rewrite length_app. simpl.
    specialize (IH (l :: chain) true). lia.
  - intros m body IH chain exec. simpl. rewrite length_app. simpl.
    specialize (IH (child_scope m chain) (child_exec m exec)). lia.
  - intros sources IH chain exec. destruct sources as [| source rest]; simpl.
    + lia.
    + rewrite !length_app. simpl. specialize (IH chain exec). simpl in IH.
      rewrite length_app in IH. simpl in IH. lia.
  - intros if_true IHtrue if_false IHfalse chain exec. simpl. rewrite !length_app. simpl.
    specialize (IHtrue chain exec). specialize (IHfalse chain exec). lia.
  - intros chain exec. simpl. lia.
  - intros source IHsource rest IHrest chain exec. simpl. rewrite length_app. simpl.
    specialize (IHsource chain exec). specialize (IHrest chain exec). lia.
  - intros chain exec. simpl. lia.
  - intros body IH chain exec. simpl. apply IH.
Qed.

Corollary vector_program_length_is_linear : forall subject,
  length (compile_term [] true subject) <= 3 * term_nodes subject.
Proof.
  intro subject.
  pose proof (proj1 compile_length_bounded_by_weight subject [] true) as bounded.
  pose proof (proj1 iterative_walk_steps_are_linear subject) as linear.
  lia.
Qed.

(** * 7. Negative control: the swap needs a commuting combine *)

(** Two events of one node. The trace algebra records their order. *)
Definition swap_witness : term := Node (ICons (Local 1) (ICons (Child Same (Node (ICons (Local 2) INil))) INil)).

Theorem swapped_trace_run_breaks_the_refinement :
  run_swapped trace trace_empty trace_combine trace_alternative trace_finish trace_apply_chain
    (compile_term [] true swap_witness) [] <>
  Some [eval trace trace_empty trace_combine trace_alternative trace_finish trace_apply
          [] true swap_witness].
Proof.
  vm_compute. intro same. discriminate same.
Qed.

(** * 8. The fold of a concrete vector *)

(** One send outside every scope and a branch whose taken side holds a
    second send: the deploy lane 0 counts the first send and the larger
    branch, componentwise. *)
Definition fold_witness : term :=
  Node (ICons (Local 0) (ICons (Cond (BSome (Node (ICons (Local 1) INil))) BNone) INil)).

Definition witness_counts (e : event) : list nat :=
  match e with
  | 0 => [1; 10; 4; 128]
  | _ => [1; 20; 0; 128]
  end.

Theorem vector_fold_witness :
  eval (list nat) [] add_vec max_vec vec_finish (vec_apply 0 4 witness_counts) [] true fold_witness =
  [2; 30; 4; 256].
Proof. vm_compute. reflexivity. Qed.

Print Assumptions vector_chain_reverses.
Print Assumptions vector_program_equals_recursion.
Print Assumptions vector_walk_refines_recursion.
Print Assumptions vector_merge_order_irrelevant.
Print Assumptions swapped_vector_run_equals_run.
Print Assumptions compile_length_bounded_by_weight.
Print Assumptions vector_program_length_is_linear.
Print Assumptions swapped_trace_run_breaks_the_refinement.
Print Assumptions vector_fold_witness.
