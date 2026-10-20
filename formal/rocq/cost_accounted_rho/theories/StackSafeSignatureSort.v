(* DR-131 P1: the work-stack flatten and the work-stack substitution of cost
   signatures refine their recursions.

   The host-work removal (DR-131) deletes the 256-level depth stop of the
   metered sorter. Two recursions of this branch had no other protection:
   collect_compound in models/src/rust/rholang/sorter/cost_accounting_sorter.rs
   and substitute_cost_signature in rholang/src/rust/interpreter/substitute.rs.
   Each recursed once for each compound level. P1 replaced both with explicit
   stacks. This module proves that the stacks compute what the recursions
   computed.

   Binding on F1R3Lang (changed by the merge of feature/cost-accounted-rho,
   finding R3-19). The production sort of F1R3Lang flattens a compound in
   descend_cost_signature (models/src/rust/rholang/sorter/sort_drive.rs).
   collect_compound stays only as a test oracle. substitute_cost_signature
   runs the work-stack machine sub_drive, whose descend_signature step is in
   rholang/src/rust/interpreter/substitute_drive.rs. Both machines push the
   elements of a compound in reverse source order, so they visit the
   elements in source order, as [flatten_machine] and [subst_machine] do.

   Sections:
   1. A cost signature is a leaf or a compound of signatures. The leaf
      payload stands for every non-compound value: ground, unit, bound level,
      quote, name and the missing value. [sig_induction] gives an induction
      hypothesis for every element of a compound.
   2. The flatten. [flatten_rec] is the recursion of HEAD 5662dfce3.
      [flatten_machine] is the work stack of the Rust code: it takes the next
      pending signature, emits a leaf, or puts the elements of a compound in
      front of the pending signatures in source order. Theorem
      [flatten_machine_refines_recursion] shows that the machine emits the
      leaves of the recursion in the same order. Corollary
      [flatten_machine_steps_are_linear] shows that one step for each node
      suffices. On the branch that this module comes from, source order
      mattered because its sort kept ties in input order. The sort of
      F1R3Lang is a total order (score_tree.rs), so its result does not
      depend on the order of the leaves. (Changed by the merge of
      feature/cost-accounted-rho, finding R3-19.)
   3. The substitution. [subst_rec] is the recursion of HEAD: it substitutes
      every element of a compound from left to right, stops at the first
      error, and closes the compound with one function [close] (build and
      sort). [subst_machine] is the Rust machine: a work stack of [Visit] and
      [Close] steps and a value stack whose head is the newest value. Theorem
      [subst_machine_refines_recursion] shows that for every leaf map and
      every close function the machine returns the value of the recursion,
      and that it fails exactly when the recursion fails. Lemma
      [subst_cost_is_linear] bounds its steps by twice the number of nodes.
   4. Negative controls with concrete witnesses: a flatten that puts the
      elements in reverse order, a close that reads the value stack without
      reversing it, and a machine that skips the close of each level all
      break the refinement.

   The leaf map and the close function of Section 3 are Section variables.
   They become ordinary parameters of each theorem when the section closes,
   so no axiom is used. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
Import ListNotations.

(** * 1. Cost signatures *)

Inductive sig : Type :=
| SLeaf : nat -> sig
| SCompound : list sig -> sig.

(** The induction principle that Rocq generates for a nested inductive gives
    no hypothesis for the elements of a compound. This one gives one for each
    element. *)
Section SigInduction.
  Variable P : sig -> Prop.
  Hypothesis leaf_case : forall leaf, P (SLeaf leaf).
  Hypothesis compound_case :
    forall elements, Forall P elements -> P (SCompound elements).

  Fixpoint sig_induction (signature : sig) : P signature :=
    match signature with
    | SLeaf leaf => leaf_case leaf
    | SCompound elements =>
        compound_case elements
          ((fix all (elements : list sig) : Forall P elements :=
              match elements with
              | [] => Forall_nil P
              | element :: rest =>
                  Forall_cons element (sig_induction element) (all rest)
              end) elements)
    end.
End SigInduction.

(** The number of nodes: one for each leaf and one for each compound. *)
Fixpoint sig_size (signature : sig) : nat :=
  match signature with
  | SLeaf _ => 1
  | SCompound elements => S (list_sum (map sig_size elements))
  end.

Definition forest_size (signatures : list sig) : nat :=
  list_sum (map sig_size signatures).

(** * 2. The flatten *)

(** The recursion of HEAD: a leaf yields itself, and a compound yields the
    leaves of its elements in source order. *)
Fixpoint flatten_rec (signature : sig) : list nat :=
  match signature with
  | SLeaf leaf => [leaf]
  | SCompound elements => flat_map flatten_rec elements
  end.

Definition flatten_forest (signatures : list sig) : list nat :=
  flat_map flatten_rec signatures.

(** The Rust machine. The head of [work] is the next pending signature. A leaf
    goes to the end of [output]. A compound puts its elements in front of the
    pending signatures, in source order. [fuel] bounds the number of steps. *)
Fixpoint flatten_machine (fuel : nat) (work : list sig) (output : list nat)
  : option (list nat) :=
  match work with
  | [] => Some output
  | pending :: rest =>
      match fuel with
      | 0 => None
      | S fuel' =>
          match pending with
          | SLeaf leaf => flatten_machine fuel' rest (output ++ [leaf])
          | SCompound elements => flatten_machine fuel' (elements ++ rest) output
          end
      end
  end.

Theorem flatten_machine_refines_recursion :
  forall fuel work output,
    forest_size work <= fuel ->
    flatten_machine fuel work output = Some (output ++ flatten_forest work).
Proof.
  induction fuel as [|fuel IH]; intros work output Hfuel.
  - destruct work as [|pending rest].
    + simpl. rewrite app_nil_r. reflexivity.
    + unfold forest_size in Hfuel. destruct pending; simpl in Hfuel; lia.
  - destruct work as [|pending rest].
    + simpl. rewrite app_nil_r. reflexivity.
    + destruct pending as [leaf|elements]; simpl.
      * rewrite IH.
        -- rewrite <- app_assoc. reflexivity.
        -- unfold forest_size in *. simpl in Hfuel. lia.
      * rewrite IH.
        -- unfold flatten_forest. rewrite flat_map_app. reflexivity.
        -- unfold forest_size in *. simpl in Hfuel.
           rewrite map_app, list_sum_app. lia.
Qed.

Corollary flatten_machine_steps_are_linear :
  forall work,
    flatten_machine (forest_size work) work [] = Some (flatten_forest work).
Proof.
  intros work.
  rewrite flatten_machine_refines_recursion by lia.
  reflexivity.
Qed.

(** * 3. The substitution *)

Section Substitution.
  Variable R : Type.
  (** [leaf] substitutes and sorts one leaf. [None] is an error, for example
      a missing value. *)
  Variable leaf : nat -> option R.
  (** [close] builds a compound from its substituted elements in source
      order and sorts it. *)
  Variable close : list R -> R.

  (** Left to right, stopping at the first error, as [collect] over
      [Result] does in the Rust code. *)
  Fixpoint traverse (f : sig -> option R) (elements : list sig)
    : option (list R) :=
    match elements with
    | [] => Some []
    | element :: rest =>
        match f element with
        | None => None
        | Some value =>
            match traverse f rest with
            | None => None
            | Some values => Some (value :: values)
            end
        end
    end.

  (** The recursion of HEAD. *)
  Fixpoint subst_rec (signature : sig) : option R :=
    match signature with
    | SLeaf payload => leaf payload
    | SCompound elements =>
        match traverse subst_rec elements with
        | None => None
        | Some values => Some (close values)
        end
    end.

  Inductive step : Type :=
  | Visit : sig -> step
  | Close : nat -> step.

  (** The Rust machine. The head of [work] is the next step, and the head of
      [values] is the newest value. [Close n] takes the [n] newest values,
      which are the elements in reverse source order, and closes them in
      source order. *)
  Fixpoint subst_machine (fuel : nat) (work : list step) (values : list R)
    : option (list R) :=
    match work with
    | [] => Some values
    | pending :: rest =>
        match fuel with
        | 0 => None
        | S fuel' =>
            match pending with
            | Visit (SLeaf payload) =>
                match leaf payload with
                | None => None
                | Some value => subst_machine fuel' rest (value :: values)
                end
            | Visit (SCompound elements) =>
                subst_machine fuel'
                  (map Visit elements ++ Close (length elements) :: rest) values
            | Close count =>
                subst_machine fuel' rest
                  (close (rev (firstn count values)) :: skipn count values)
            end
        end
    end.

  (** The steps of one signature: one [Visit] for each node and one [Close]
      for each compound. *)
  Fixpoint subst_cost (signature : sig) : nat :=
    match signature with
    | SLeaf _ => 1
    | SCompound elements => S (S (list_sum (map subst_cost elements)))
    end.

  Lemma traverse_length : forall f elements values,
      traverse f elements = Some values -> length values = length elements.
  Proof.
    intros f elements. induction elements as [|element rest IH];
      intros values Htraverse; simpl in Htraverse.
    - inversion Htraverse. reflexivity.
    - destruct (f element) as [value|]; [|discriminate].
      destruct (traverse f rest) as [tail|] eqn:Hrest; [|discriminate].
      inversion Htraverse. simpl. rewrite (IH tail eq_refl). reflexivity.
  Qed.

  Lemma prefix_split : forall (prefix suffix : list R),
      firstn (length prefix) (prefix ++ suffix) = prefix /\
      skipn (length prefix) (prefix ++ suffix) = suffix.
  Proof.
    intros prefix suffix. induction prefix as [|value prefix IH]; simpl.
    - split; reflexivity.
    - destruct IH as [Hfirst Hskip]. rewrite Hfirst, Hskip. split; reflexivity.
  Qed.

  (** One step of each kind. The proofs below rewrite with these equations
      instead of [simpl], which would also unfold the next step. *)
  Lemma subst_machine_step_compound : forall fuel elements rest values,
      subst_machine (S fuel) (Visit (SCompound elements) :: rest) values =
      subst_machine fuel
        (map Visit elements ++ Close (length elements) :: rest) values.
  Proof. reflexivity. Qed.

  Lemma subst_machine_step_close : forall fuel count rest values,
      subst_machine (S fuel) (Close count :: rest) values =
      subst_machine fuel rest
        (close (rev (firstn count values)) :: skipn count values).
  Proof. reflexivity. Qed.

  (** Running the visits of a list of elements pushes their values, newest
      first, or fails at the first error. *)
  Lemma subst_machine_visits : forall elements extra rest values,
      Forall (fun signature => forall extra rest values,
                  subst_machine (subst_cost signature + extra)
                    (Visit signature :: rest) values =
                  match subst_rec signature with
                  | None => None
                  | Some value => subst_machine extra rest (value :: values)
                  end) elements ->
      subst_machine (list_sum (map subst_cost elements) + extra)
        (map Visit elements ++ rest) values =
      match traverse subst_rec elements with
      | None => None
      | Some results => subst_machine extra rest (rev results ++ values)
      end.
  Proof.
    induction elements as [|element tail IH]; intros extra rest values Hall.
    - reflexivity.
    - inversion Hall as [|? ? Hhead Htail]; subst.
      change (map Visit (element :: tail) ++ rest)
        with (Visit element :: (map Visit tail ++ rest)).
      change (list_sum (map subst_cost (element :: tail)))
        with (subst_cost element + list_sum (map subst_cost tail)).
      rewrite <- Nat.add_assoc. rewrite Hhead.
      cbn [traverse].
      destruct (subst_rec element) as [value|]; [|reflexivity].
      rewrite (IH extra rest (value :: values) Htail).
      destruct (traverse subst_rec tail) as [results|]; [|reflexivity].
      cbn [rev]. rewrite <- app_assoc. reflexivity.
  Qed.

  Lemma subst_machine_visit : forall signature extra rest values,
      subst_machine (subst_cost signature + extra)
        (Visit signature :: rest) values =
      match subst_rec signature with
      | None => None
      | Some value => subst_machine extra rest (value :: values)
      end.
  Proof.
    intros signature. induction signature as [payload|elements Hall]
      using sig_induction; intros extra rest values.
    - reflexivity.
    - change (subst_cost (SCompound elements) + extra)
        with (S (S (list_sum (map subst_cost elements) + extra))).
      rewrite subst_machine_step_compound.
      rewrite <- Nat.add_succ_r.
      rewrite (subst_machine_visits elements (S extra)
                 (Close (length elements) :: rest) values Hall).
      cbn [subst_rec].
      destruct (traverse subst_rec elements) as [results|] eqn:Htraverse;
        [|reflexivity].
      rewrite subst_machine_step_close.
      pose proof (traverse_length subst_rec elements results Htraverse)
        as Hlength.
      pose proof (prefix_split (rev results) values) as [Hfirst Hskip].
      rewrite length_rev, Hlength in Hfirst, Hskip.
      rewrite Hfirst, Hskip, rev_involutive. reflexivity.
  Qed.

  Theorem subst_machine_refines_recursion : forall signature,
      subst_machine (subst_cost signature) [Visit signature] [] =
      match subst_rec signature with
      | None => None
      | Some value => Some [value]
      end.
  Proof.
    intros signature.
    rewrite <- (Nat.add_0_r (subst_cost signature)).
    rewrite subst_machine_visit.
    destruct (subst_rec signature); reflexivity.
  Qed.

  Lemma subst_cost_list_is_linear : forall elements,
      Forall (fun signature => subst_cost signature <= 2 * sig_size signature)
        elements ->
      list_sum (map subst_cost elements) <=
      2 * list_sum (map sig_size elements).
  Proof.
    induction elements as [|element tail IH]; intros Hall; simpl.
    - lia.
    - inversion Hall as [|? ? Hhead Htail]; subst.
      specialize (IH Htail). lia.
  Qed.

  Lemma subst_cost_is_linear : forall signature,
      subst_cost signature <= 2 * sig_size signature.
  Proof.
    intros signature. induction signature as [payload|elements Hall]
      using sig_induction; simpl.
    - lia.
    - pose proof (subst_cost_list_is_linear elements Hall). lia.
  Qed.
End Substitution.

(** * 4. Negative controls *)

(** A flatten that puts the elements of a compound in reverse order emits the
    leaves in another order. *)
Fixpoint flatten_machine_reversed (fuel : nat) (work : list sig)
  (output : list nat) : option (list nat) :=
  match work with
  | [] => Some output
  | pending :: rest =>
      match fuel with
      | 0 => None
      | S fuel' =>
          match pending with
          | SLeaf leaf => flatten_machine_reversed fuel' rest (output ++ [leaf])
          | SCompound elements =>
              flatten_machine_reversed fuel' (rev elements ++ rest) output
          end
      end
  end.

Example reversed_push_breaks_the_refinement :
  flatten_machine_reversed 3 [SCompound [SLeaf 1; SLeaf 2]] [] <>
  Some (flatten_forest [SCompound [SLeaf 1; SLeaf 2]]).
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.

(** A close that reads the value stack without reversing it closes the
    elements in reverse source order. The witness closes by concatenation. *)
Fixpoint subst_machine_unreversed (fuel : nat) (work : list step)
  (values : list (list nat)) : option (list (list nat)) :=
  match work with
  | [] => Some values
  | pending :: rest =>
      match fuel with
      | 0 => None
      | S fuel' =>
          match pending with
          | Visit (SLeaf payload) =>
              subst_machine_unreversed fuel' rest ([payload] :: values)
          | Visit (SCompound elements) =>
              subst_machine_unreversed fuel'
                (map Visit elements
                   ++ Close (length elements) :: rest) values
          | Close count =>
              subst_machine_unreversed fuel' rest
                (concat (firstn count values) :: skipn count values)
          end
      end
  end.

Example unreversed_close_breaks_the_refinement :
  subst_machine_unreversed 4
    [Visit (SCompound [SLeaf 1; SLeaf 2])] [] <>
  match subst_rec (list nat) (fun payload => Some [payload]) (@concat nat)
          (SCompound [SLeaf 1; SLeaf 2]) with
  | None => None
  | Some value => Some [value]
  end.
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.

(** A machine that skips the close of each level leaves the element values
    on the stack instead of one closed value. The witness closes by
    concatenation. *)
Fixpoint subst_machine_without_close (fuel : nat) (work : list sig)
  (values : list (list nat)) : option (list (list nat)) :=
  match work with
  | [] => Some values
  | pending :: rest =>
      match fuel with
      | 0 => None
      | S fuel' =>
          match pending with
          | SLeaf payload =>
              subst_machine_without_close fuel' rest ([payload] :: values)
          | SCompound elements =>
              subst_machine_without_close fuel' (elements ++ rest) values
          end
      end
  end.

Example skipped_close_breaks_the_refinement :
  subst_machine_without_close 3 [SCompound [SLeaf 1; SLeaf 2]] [] <>
  match subst_rec (list nat) (fun payload => Some [payload]) (@concat nat)
          (SCompound [SLeaf 1; SLeaf 2]) with
  | None => None
  | Some value => Some [value]
  end.
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.
