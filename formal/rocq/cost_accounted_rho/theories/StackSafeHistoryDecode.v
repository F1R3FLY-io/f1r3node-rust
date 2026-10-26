(* DR-131 P2b: the explicit-stack decoder of the native history rows refines
   the recursive decoder.

   The host-work removal (DR-131) deletes the 128-level depth stop of the
   typed history decoder. Replay decodes stored rows in its cold reads, and the
   derived bincode decoder recurses once for each nesting level. So P2b gives
   the four roots of the rows (Par, ListParWithRandom, BindPattern and
   TaggedContinuation) an explicit-stack machine, in
   models/src/rust/rholang/record_decoder.rs. This module proves, for a model
   of that machine, that it decodes exactly what the recursive decoder
   decodes, on every input, well-formed or not.

   The model. The input is a list of tokens. One token is one primitive read
   of bincode: a bool, an Option tag, a variant index or a sequence count. A
   tree is a node: an arm, read as a variant index that must be below
   [arm_count], and then the fields of the program of that arm. A field is
   one of:
   - [FBool]: one token, which must be 0 or 1, as bincode's bool;
   - [FTrees]: a count token, then that many trees, as a sequence of Par;
   - [FOption]: a tag token, which must be 0 or 1, then one tree when it is
     1, as an Option of Par.
   The program of every arm is a Section variable, so the theorems hold for
   every schema of this form. The Rust machine has one field program for each
   type of the schema, and each program is a sequence of these reads. The
   model's bool stands for every bounded scalar that a program reads between
   two descents.

   Sections:
   1. The format: fields, trees and outcomes.
   2. The recursive decoder [dec_tree], the derived decoder of a tree. It
      reads in stream order and recurses once for each nested tree.
   3. The machine [run]: a work stack of ops and a value stack, as in the
      Rust code. [OTree] reads an arm, [OFields] is a resume point between
      two fields with the shape of the fields read so far, and [ORep] is the
      counted repeat. When all fields of a node are read, the build step takes
      the trees of the node from the value stack and pushes the node.
   4. Simulation. [tree_simulation] shows that the machine follows the
      recursion on every input: it produces the same tree and the same
      remainder, or it rejects when the recursion rejects. Each lemma bounds
      the steps linearly in the input length.
   5. Main theorems:
      - [machine_refines_recursion]: with 3 * |input| + 1 steps or more, the
        machine returns the machine_result of the recursion, and the recursion never
        runs out of fuel ([decode_rec_total]).
      - [machine_never_reports_invariant]: the build step never finds too few
        values on its stack, for every input and every fuel. This is the
        claim that the Rust MachineInvariant error is unreachable.
      - [machine_is_never_wrong]: with too little fuel the machine stops
        without an answer. It never gives a wrong answer.
   6. Records and the encoding. A row is its root followed by a bounded
      tail, which the derived decoder reads.
      [record_machine_refines_record_rec] shows that the machine prefix,
      followed by the tail decoder, equals the derived decoding of the whole
      row. [decode_rec_round_trip], [machine_round_trip] and
      [record_round_trip] show that a well-formed row decodes to itself and
      leaves any trailing tokens as the remainder: the extent is exact.
   7. Negative controls with concrete witnesses. An Option tag 2 read as
      Some, children taken from the stack without reversal, a range check
      that admits [arm_count], and a field order that reads the bool before
      the trees each break the refinement.

   No axiom is used. [arm_count], [program] and the tail decoder of Section 6
   become ordinary parameters of each theorem when their sections close. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia Bool.
Import ListNotations.

(** * 1. The format *)

Inductive field : Type :=
| FBool : field
| FTrees : field
| FOption : field.

(** A node holds an arm and the values of its fields, in program order: a
    bool for [FBool], and a list of trees for [FTrees] and [FOption]. *)
Inductive tree : Type :=
| Node : nat -> list (bool + list tree) -> tree.

Definition part : Type := (bool + list tree)%type.

Definition bool_of_token (token : nat) : option bool :=
  match token with
  | 0 => Some false
  | 1 => Some true
  | _ => None
  end.

Definition token_of_bool (b : bool) : nat := if b then 1 else 0.

(** A decoder returns a value and the rest of the input, rejects the input,
    or runs out of fuel. *)
Inductive outcome (A : Type) : Type :=
| Done : A -> list nat -> outcome A
| Reject : outcome A
| Stuck : outcome A.

Arguments Done {A}.
Arguments Reject {A}.
Arguments Stuck {A}.

Lemma split_at_length : forall (A : Type) (prefix suffix : list A),
    firstn (length prefix) (prefix ++ suffix) = prefix /\
    skipn (length prefix) (prefix ++ suffix) = suffix.
Proof.
  intros A prefix suffix. induction prefix as [|value prefix IH]; simpl.
  - split; reflexivity.
  - destruct IH as [Hfirst Hskip]. rewrite Hfirst, Hskip. split; reflexivity.
Qed.

Section Decoder.
  Variable arm_count : nat.
  Variable program : nat -> list field.

  (** * 2. The recursive decoder *)

  (** [count] trees in sequence, each read by [sub], stopping at the first
      rejection. *)
  Fixpoint dec_trees (sub : list nat -> outcome tree) (count : nat)
    (input : list nat) : outcome (list tree) :=
    match count with
    | 0 => Done [] input
    | S count' =>
        match sub input with
        | Done t rest =>
            match dec_trees sub count' rest with
            | Done ts rest' => Done (t :: ts) rest'
            | Reject => Reject
            | Stuck => Stuck
            end
        | Reject => Reject
        | Stuck => Stuck
        end
    end.

  (** Puts one part in front of the parts that follow it. *)
  Definition prepend (p : part) (rest : outcome (list part))
    : outcome (list part) :=
    match rest with
    | Done parts remainder => Done (p :: parts) remainder
    | Reject => Reject
    | Stuck => Stuck
    end.

  (** The fields of one program, in order. *)
  Fixpoint dec_fields (sub : list nat -> outcome tree) (fs : list field)
    (input : list nat) : outcome (list part) :=
    match fs with
    | [] => Done [] input
    | f :: fs' =>
        match input with
        | [] => Reject
        | token :: rest =>
            match f with
            | FBool =>
                match bool_of_token token with
                | Some b => prepend (inl b) (dec_fields sub fs' rest)
                | None => Reject
                end
            | FTrees =>
                match dec_trees sub token rest with
                | Done ts after => prepend (inr ts) (dec_fields sub fs' after)
                | Reject => Reject
                | Stuck => Stuck
                end
            | FOption =>
                match token with
                | 0 => prepend (inr []) (dec_fields sub fs' rest)
                | 1 =>
                    match sub rest with
                    | Done t after => prepend (inr [t]) (dec_fields sub fs' after)
                    | Reject => Reject
                    | Stuck => Stuck
                    end
                | _ => Reject
                end
            end
        end
    end.

  (** The recursive decoder of one tree. Each nested tree costs one unit of
      fuel and one level of recursion. *)
  Fixpoint dec_tree (fuel : nat) (input : list nat) : outcome tree :=
    match fuel with
    | 0 => Stuck
    | S fuel' =>
        match input with
        | [] => Reject
        | arm :: rest =>
            if Nat.ltb arm arm_count then
              match dec_fields (dec_tree fuel') (program arm) rest with
              | Done parts rest' => Done (Node arm parts) rest'
              | Reject => Reject
              | Stuck => Stuck
              end
            else Reject
        end
    end.

  (** Each tree reads at least its arm token, so one unit of fuel for each
      token always suffices ([decode_rec_total]). *)
  Definition decode_rec (input : list nat) : outcome tree :=
    dec_tree (S (length input)) input.

  (** * 3. The machine *)

  (** The shape of one field that is already read: its bool, or the number
      of trees that it pushed on the value stack. *)
  Inductive pending : Type :=
  | QBool : bool -> pending
  | QTrees : nat -> pending.

  Inductive op : Type :=
  | OTree : op
  | OFields : nat -> list field -> list pending -> op
  | ORep : nat -> op.

  Inductive machine_result : Type :=
  | MDone : tree -> list nat -> machine_result
  | MReject : machine_result
  | MInvariant : machine_result
  | MStuck : machine_result.

  Fixpoint pending_trees (done : list pending) : nat :=
    match done with
    | [] => 0
    | QBool _ :: rest => pending_trees rest
    | QTrees count :: rest => count + pending_trees rest
    end.

  (** Gives each field that was read its value, taking the trees in stream
      order. *)
  Fixpoint attach (done : list pending) (trees : list tree) : list part :=
    match done with
    | [] => []
    | QBool b :: rest => inl b :: attach rest trees
    | QTrees count :: rest =>
        inr (firstn count trees) :: attach rest (skipn count trees)
    end.

  (** The build step. The trees of the node are the [pending_trees done]
      newest values, newest first, so they are reversed into stream order. A
      stack with fewer values is the failure that the Rust code reports as
      MachineInvariant. *)
  Definition build (arm : nat) (done : list pending) (values : list tree)
    : option (list tree) :=
    let count := pending_trees done in
    if Nat.leb count (length values)
    then Some (Node arm (attach done (rev (firstn count values)))
                 :: skipn count values)
    else None.

  Inductive transition : Type :=
  | Continue : list op -> list tree -> list nat -> transition
  | Halt : machine_result -> transition.

  (** One step of the op on top of the work stack. [ops] is the rest of the
      work stack, and the head of [values] is the newest value. *)
  Definition machine_step (o : op) (ops : list op) (values : list tree)
    (input : list nat) : transition :=
    match o with
    | OTree =>
        match input with
        | [] => Halt MReject
        | arm :: rest =>
            if Nat.ltb arm arm_count
            then Continue (OFields arm (program arm) [] :: ops) values rest
            else Halt MReject
        end
    | OFields arm [] done =>
        match build arm done values with
        | Some values' => Continue ops values' input
        | None => Halt MInvariant
        end
    | OFields arm (f :: fs) done =>
        match input with
        | [] => Halt MReject
        | token :: rest =>
            match f with
            | FBool =>
                match bool_of_token token with
                | Some b =>
                    Continue (OFields arm fs (done ++ [QBool b]) :: ops) values rest
                | None => Halt MReject
                end
            | FTrees =>
                Continue
                  (ORep token :: OFields arm fs (done ++ [QTrees token]) :: ops)
                  values rest
            | FOption =>
                match token with
                | 0 =>
                    Continue (OFields arm fs (done ++ [QTrees 0]) :: ops) values rest
                | 1 =>
                    Continue (OTree :: OFields arm fs (done ++ [QTrees 1]) :: ops)
                      values rest
                | _ => Halt MReject
                end
            end
        end
    | ORep 0 => Continue ops values input
    | ORep (S count) => Continue (OTree :: ORep count :: ops) values input
    end.

  (** The interpreter loop. When the work stack is empty, exactly one value
      must remain: the root. [fuel] bounds the number of steps. The Rust loop
      has no fuel: it ends because every tree reads a token. *)
  Fixpoint run_with
    (step_fn : op -> list op -> list tree -> list nat -> transition)
    (fuel : nat) (ops : list op) (values : list tree) (input : list nat)
    : machine_result :=
    match ops with
    | [] =>
        match values with
        | [t] => MDone t input
        | _ => MInvariant
        end
    | o :: ops' =>
        match fuel with
        | 0 => MStuck
        | S fuel' =>
            match step_fn o ops' values input with
            | Continue ops'' values' input' =>
                run_with step_fn fuel' ops'' values' input'
            | Halt r => r
            end
        end
    end.

  Definition run := run_with machine_step.

  Lemma run_machine_step : forall fuel o ops values input,
      run (S fuel) (o :: ops) values input =
      match machine_step o ops values input with
      | Continue ops' values' input' => run fuel ops' values' input'
      | Halt r => r
      end.
  Proof. reflexivity. Qed.

  (** An empty work stack ends the run for every fuel. [run_with] recurses
      on its fuel, so this needs a case split on the fuel. *)
  Lemma run_nil : forall fuel values input,
      run fuel [] values input =
      match values with
      | [t] => MDone t input
      | _ => MInvariant
      end.
  Proof. intros [|fuel] values input; reflexivity. Qed.

  (** * 4. Simulation *)

  Definition result_of (o : outcome tree) : machine_result :=
    match o with
    | Done t rest => MDone t rest
    | Reject => MReject
    | Stuck => MStuck
    end.

  (** [OTree] simulates the sub decoder [sub]. A tree that leaves [rest]
      takes at most [3 * (|input| - |rest|) - 1] steps, and a rejection at
      most [3 * |input| + 1] steps. *)
  Definition simulates (sub : list nat -> outcome tree) : Prop :=
    forall input,
      (forall t rest, sub input = Done t rest ->
         forall ops values, exists k,
           k + 1 + 3 * length rest <= 3 * length input /\
           forall extra,
             run (k + extra) (OTree :: ops) values input =
             run extra ops (t :: values) rest) /\
      (sub input = Reject ->
         forall ops values, exists k,
           k <= 3 * length input + 1 /\
           forall extra,
             run (k + extra) (OTree :: ops) values input = MReject).

  Lemma dec_trees_length : forall sub count input ts rest,
      dec_trees sub count input = Done ts rest -> length ts = count.
  Proof.
    intros sub count. induction count as [|count IH];
      intros input ts rest Hdec; cbn [dec_trees] in Hdec.
    - inversion Hdec. reflexivity.
    - destruct (sub input) as [t r| |]; try discriminate.
      destruct (dec_trees sub count r) as [ts' r'| |] eqn:Erest; try discriminate.
      inversion Hdec; subst. cbn [length]. f_equal. exact (IH r ts' rest Erest).
  Qed.

  (** The counted repeat runs the trees of a sequence and leaves their values
      on the stack, newest first. *)
  Lemma trees_simulation : forall sub, simulates sub ->
    forall count input ts rest,
      dec_trees sub count input = Done ts rest ->
      forall ops values, exists k,
        k + 3 * length rest <= 3 * length input + 1 /\
        forall extra,
          run (k + extra) (ORep count :: ops) values input =
          run extra ops (rev ts ++ values) rest.
  Proof.
    intros sub Hsub count. induction count as [|count IH];
      intros input ts rest Hdec ops values; cbn [dec_trees] in Hdec.
    - inversion Hdec; subst. exists 1. split; [lia|].
      intros extra. reflexivity.
    - destruct (sub input) as [t r| |] eqn:Esub; try discriminate.
      destruct (dec_trees sub count r) as [ts' r'| |] eqn:Erest; try discriminate.
      inversion Hdec; subst.
      destruct (proj1 (Hsub input) t r Esub (ORep count :: ops) values)
        as [k1 [Hk1 Hrun1]].
      destruct (IH r ts' rest Erest ops (t :: values)) as [k2 [Hk2 Hrun2]].
      exists (S (k1 + k2)). split; [lia|].
      intros extra. rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
      rewrite <- Nat.add_assoc, Hrun1, Hrun2.
      cbn [rev]. rewrite <- app_assoc. reflexivity.
  Qed.

  Lemma trees_rejection : forall sub, simulates sub ->
    forall count input,
      dec_trees sub count input = Reject ->
      forall ops values, exists k,
        k <= 3 * length input + 2 /\
        forall extra, run (k + extra) (ORep count :: ops) values input = MReject.
  Proof.
    intros sub Hsub count. induction count as [|count IH];
      intros input Hdec ops values; cbn [dec_trees] in Hdec.
    - discriminate.
    - destruct (sub input) as [t r| |] eqn:Esub; try discriminate.
      + destruct (dec_trees sub count r) as [ts' r'| |] eqn:Erest;
          try discriminate.
        destruct (proj1 (Hsub input) t r Esub (ORep count :: ops) values)
          as [k1 [Hk1 Hrun1]].
        destruct (IH r Erest ops (t :: values)) as [k2 [Hk2 Hrun2]].
        exists (S (k1 + k2)). split; [lia|].
        intros extra. rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
        rewrite <- Nat.add_assoc, Hrun1. exact (Hrun2 extra).
      + destruct (proj2 (Hsub input) Esub (ORep count :: ops) values)
          as [k1 [Hk1 Hrun1]].
        exists (S k1). split; [lia|].
        intros extra. rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
        exact (Hrun1 extra).
  Qed.

  (** The shape of the parts that are read, and the trees that they hold. *)
  Definition shape (p : part) : pending :=
    match p with
    | inl b => QBool b
    | inr ts => QTrees (length ts)
    end.

  Definition trees_of (p : part) : list tree :=
    match p with
    | inl _ => []
    | inr ts => ts
    end.

  Definition all_trees (parts : list part) : list tree := flat_map trees_of parts.

  Lemma pending_trees_shape : forall parts,
      pending_trees (map shape parts) = length (all_trees parts).
  Proof.
    induction parts as [|[b|ts] parts IH]; cbn [map pending_trees shape].
    - reflexivity.
    - exact IH.
    - unfold all_trees in *. cbn [flat_map trees_of].
      rewrite length_app, IH. reflexivity.
  Qed.

  Lemma attach_shape : forall parts extra,
      attach (map shape parts) (all_trees parts ++ extra) = parts.
  Proof.
    induction parts as [|[b|ts] parts IH]; intros extra;
      unfold all_trees in *; cbn [map attach shape flat_map trees_of app].
    - reflexivity.
    - rewrite IH. reflexivity.
    - rewrite <- app_assoc.
      destruct (split_at_length _ ts (flat_map trees_of parts ++ extra))
        as [Hfirst Hskip].
      rewrite Hfirst, Hskip, IH. reflexivity.
  Qed.

  Lemma build_shape : forall arm parts values,
      build arm (map shape parts) (rev (all_trees parts) ++ values) =
      Some (Node arm parts :: values).
  Proof.
    intros arm parts values. unfold build. cbv zeta.
    rewrite pending_trees_shape.
    destruct (split_at_length _ (rev (all_trees parts)) values) as [Hfirst Hskip].
    rewrite length_rev in Hfirst, Hskip.
    rewrite length_app, length_rev.
    replace (Nat.leb (length (all_trees parts)) (length (all_trees parts) + length values))
      with true by (symmetry; apply Nat.leb_le; lia).
    rewrite Hfirst, Hskip, rev_involutive.
    pose proof (attach_shape parts []) as Hattach.
    rewrite app_nil_r in Hattach. rewrite Hattach. reflexivity.
  Qed.

  Lemma shape_snoc : forall prior p,
      map shape (prior ++ [p]) = map shape prior ++ [shape p].
  Proof. intros prior p. rewrite map_app. reflexivity. Qed.

  Lemma all_trees_snoc : forall prior p,
      all_trees (prior ++ [p]) = all_trees prior ++ trees_of p.
  Proof.
    intros prior p. unfold all_trees.
    rewrite flat_map_app. cbn [flat_map]. rewrite app_nil_r. reflexivity.
  Qed.

  (** The fields of one node. [prior] is the list of parts read before
      [fs]: the resume point holds their shape, and the value stack holds
      their trees, newest first, above [values]. *)
  Lemma fields_simulation : forall sub, simulates sub ->
    forall fs input parts rest,
      dec_fields sub fs input = Done parts rest ->
      forall arm prior ops values, exists k,
        k + 3 * length rest <= 3 * length input + 1 /\
        forall extra,
          run (k + extra) (OFields arm fs (map shape prior) :: ops)
            (rev (all_trees prior) ++ values) input =
          run extra ops (Node arm (prior ++ parts) :: values) rest.
  Proof.
    intros sub Hsub fs. induction fs as [|f fs IH];
      intros input parts rest Hdec arm prior ops values.
    - cbn [dec_fields] in Hdec. inversion Hdec; subst.
      exists 1. split; [lia|]. intros extra.
      change (1 + extra) with (S extra).
      rewrite run_machine_step. cbn [machine_step]. rewrite build_shape, app_nil_r.
      reflexivity.
    - destruct input as [|token input']; [cbn [dec_fields] in Hdec; discriminate|].
      destruct f; cbn [dec_fields] in Hdec.
      + (* FBool *)
        destruct (bool_of_token token) as [b|] eqn:Etoken; [|discriminate].
        destruct (dec_fields sub fs input') as [parts' rest'| |] eqn:Erest;
          unfold prepend in Hdec; try discriminate.
        inversion Hdec; subst.
        destruct (IH input' parts' rest Erest arm (prior ++ [inl b]) ops values)
          as [k [Hk Hrun]].
        exists (S k). split; [cbn [length]; lia|]. intros extra.
        rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step]. rewrite Etoken.
        cbv beta iota.
        rewrite shape_snoc, all_trees_snoc in Hrun. cbn [shape trees_of] in Hrun.
        rewrite app_nil_r in Hrun.
        rewrite Hrun, <- (app_assoc prior). reflexivity.
      + (* FTrees *)
        destruct (dec_trees sub token input') as [ts after| |] eqn:Etrees;
          try discriminate.
        destruct (dec_fields sub fs after) as [parts' rest'| |] eqn:Erest;
          unfold prepend in Hdec; try discriminate.
        inversion Hdec; subst.
        pose proof (dec_trees_length sub token input' ts after Etrees) as Hlength.
        destruct (trees_simulation sub Hsub token input' ts after Etrees
                    (OFields arm fs (map shape prior ++ [QTrees token]) :: ops)
                    (rev (all_trees prior) ++ values)) as [k1 [Hk1 Hrun1]].
        destruct (IH after parts' rest Erest arm (prior ++ [inr ts]) ops values)
          as [k2 [Hk2 Hrun2]].
        exists (S (k1 + k2)). split; [cbn [length]; lia|]. intros extra.
        rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
        rewrite <- Nat.add_assoc, Hrun1.
        rewrite shape_snoc, all_trees_snoc in Hrun2. cbn [shape trees_of] in Hrun2.
        rewrite Hlength, rev_app_distr, <- (app_assoc (rev ts)) in Hrun2.
        rewrite Hrun2, <- (app_assoc prior). reflexivity.
      + (* FOption *)
        destruct token as [|[|token]].
        * destruct (dec_fields sub fs input') as [parts' rest'| |] eqn:Erest;
            unfold prepend in Hdec; try discriminate.
          inversion Hdec; subst.
          destruct (IH input' parts' rest Erest arm (prior ++ [inr []]) ops values)
            as [k [Hk Hrun]].
          exists (S k). split; [cbn [length]; lia|]. intros extra.
          rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
          rewrite shape_snoc, all_trees_snoc in Hrun. cbn [shape trees_of length] in Hrun.
          rewrite app_nil_r in Hrun.
          rewrite Hrun, <- (app_assoc prior). reflexivity.
        * destruct (sub input') as [t after| |] eqn:Esub; try discriminate.
          destruct (dec_fields sub fs after) as [parts' rest'| |] eqn:Erest;
            unfold prepend in Hdec; try discriminate.
          inversion Hdec; subst.
          destruct (proj1 (Hsub input') t after Esub
                      (OFields arm fs (map shape prior ++ [QTrees 1]) :: ops)
                      (rev (all_trees prior) ++ values)) as [k1 [Hk1 Hrun1]].
          destruct (IH after parts' rest Erest arm (prior ++ [inr [t]]) ops values)
            as [k2 [Hk2 Hrun2]].
          exists (S (k1 + k2)). split; [cbn [length]; lia|]. intros extra.
          rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
          rewrite <- Nat.add_assoc, Hrun1.
          rewrite shape_snoc, all_trees_snoc in Hrun2. cbn [shape trees_of length] in Hrun2.
          rewrite rev_app_distr in Hrun2. cbn [rev app] in Hrun2.
          rewrite Hrun2, <- (app_assoc prior). reflexivity.
        * discriminate.
  Qed.

  Lemma fields_rejection : forall sub, simulates sub ->
    forall fs input,
      dec_fields sub fs input = Reject ->
      forall arm prior ops values, exists k,
        k <= 3 * length input + 1 /\
        forall extra,
          run (k + extra) (OFields arm fs (map shape prior) :: ops)
            (rev (all_trees prior) ++ values) input = MReject.
  Proof.
    intros sub Hsub fs. induction fs as [|f fs IH];
      intros input Hdec arm prior ops values.
    - cbn [dec_fields] in Hdec. discriminate.
    - destruct input as [|token input'].
      + exists 1. split; [lia|]. intros extra. reflexivity.
      + destruct f; cbn [dec_fields] in Hdec.
        * (* FBool *)
          destruct (bool_of_token token) as [b|] eqn:Etoken.
          -- destruct (dec_fields sub fs input') as [parts' rest'| |] eqn:Erest;
               unfold prepend in Hdec; try discriminate.
             destruct (IH input' Erest arm (prior ++ [inl b]) ops values)
               as [k [Hk Hrun]].
             exists (S k). split; [cbn [length]; lia|]. intros extra.
             rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step]. rewrite Etoken.
             rewrite shape_snoc, all_trees_snoc in Hrun. cbn [shape trees_of] in Hrun.
             rewrite app_nil_r in Hrun.
             exact (Hrun extra).
          -- exists 1. split; [cbn [length]; lia|]. intros extra.
             change (1 + extra) with (S extra).
             rewrite run_machine_step. cbn [machine_step]. rewrite Etoken. reflexivity.
        * (* FTrees *)
          destruct (dec_trees sub token input') as [ts after| |] eqn:Etrees.
          -- destruct (dec_fields sub fs after) as [parts' rest'| |] eqn:Erest;
               unfold prepend in Hdec; try discriminate.
             pose proof (dec_trees_length sub token input' ts after Etrees)
               as Hlength.
             destruct (trees_simulation sub Hsub token input' ts after Etrees
                         (OFields arm fs (map shape prior ++ [QTrees token]) :: ops)
                         (rev (all_trees prior) ++ values)) as [k1 [Hk1 Hrun1]].
             destruct (IH after Erest arm (prior ++ [inr ts]) ops values)
               as [k2 [Hk2 Hrun2]].
             exists (S (k1 + k2)). split; [cbn [length]; lia|]. intros extra.
             rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
             rewrite <- Nat.add_assoc, Hrun1.
             rewrite shape_snoc, all_trees_snoc in Hrun2.
             cbn [shape trees_of] in Hrun2.
             rewrite Hlength, rev_app_distr, <- (app_assoc (rev ts)) in Hrun2.
             exact (Hrun2 extra).
          -- destruct (trees_rejection sub Hsub token input' Etrees
                         (OFields arm fs (map shape prior ++ [QTrees token]) :: ops)
                         (rev (all_trees prior) ++ values)) as [k [Hk Hrun]].
             exists (S k). split; [cbn [length]; lia|]. intros extra.
             rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step]. exact (Hrun extra).
          -- discriminate.
        * (* FOption *)
          destruct token as [|[|token]].
          -- destruct (dec_fields sub fs input') as [parts' rest'| |] eqn:Erest;
               unfold prepend in Hdec; try discriminate.
             destruct (IH input' Erest arm (prior ++ [inr []]) ops values)
               as [k [Hk Hrun]].
             exists (S k). split; [cbn [length]; lia|]. intros extra.
             rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
             rewrite shape_snoc, all_trees_snoc in Hrun.
             cbn [shape trees_of length] in Hrun. rewrite app_nil_r in Hrun.
             exact (Hrun extra).
          -- destruct (sub input') as [t after| |] eqn:Esub.
             ++ destruct (dec_fields sub fs after) as [parts' rest'| |] eqn:Erest;
                  unfold prepend in Hdec; try discriminate.
                destruct (proj1 (Hsub input') t after Esub
                            (OFields arm fs (map shape prior ++ [QTrees 1]) :: ops)
                            (rev (all_trees prior) ++ values)) as [k1 [Hk1 Hrun1]].
                destruct (IH after Erest arm (prior ++ [inr [t]]) ops values)
                  as [k2 [Hk2 Hrun2]].
                exists (S (k1 + k2)). split; [cbn [length]; lia|]. intros extra.
                rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step].
                rewrite <- Nat.add_assoc, Hrun1.
                rewrite shape_snoc, all_trees_snoc in Hrun2.
                cbn [shape trees_of length] in Hrun2.
                rewrite rev_app_distr in Hrun2. cbn [rev app] in Hrun2.
                exact (Hrun2 extra).
             ++ destruct (proj2 (Hsub input') Esub
                            (OFields arm fs (map shape prior ++ [QTrees 1]) :: ops)
                            (rev (all_trees prior) ++ values)) as [k [Hk Hrun]].
                exists (S k). split; [cbn [length]; lia|]. intros extra.
                rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step]. exact (Hrun extra).
             ++ discriminate.
          -- exists 1. split; [cbn [length]; lia|]. intros extra.
             change (1 + extra) with (S extra).
             rewrite run_machine_step. reflexivity.
  Qed.

  (** The machine follows the recursion for every fuel of the recursion. *)
  Theorem tree_simulation : forall fuel, simulates (dec_tree fuel).
  Proof.
    induction fuel as [|fuel IH]; intros input; split.
    - intros t rest Hdec. cbn [dec_tree] in Hdec. discriminate.
    - intros Hdec. cbn [dec_tree] in Hdec. discriminate.
    - intros t rest Hdec ops values.
      destruct input as [|arm input']; cbn [dec_tree] in Hdec; [discriminate|].
      destruct (Nat.ltb arm arm_count) eqn:Earm; [|discriminate].
      destruct (dec_fields (dec_tree fuel) (program arm) input')
        as [parts rest'| |] eqn:Efields; try discriminate.
      inversion Hdec; subst.
      destruct (fields_simulation (dec_tree fuel) IH (program arm) input' parts rest
                  Efields arm [] ops values) as [k [Hk Hrun]].
      exists (S k). split; [cbn [length]; lia|]. intros extra.
      rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step]. rewrite Earm.
      exact (Hrun extra).
    - intros Hdec ops values.
      destruct input as [|arm input'].
      + exists 1. split; [lia|]. intros extra. reflexivity.
      + cbn [dec_tree] in Hdec.
        destruct (Nat.ltb arm arm_count) eqn:Earm.
        * destruct (dec_fields (dec_tree fuel) (program arm) input')
            as [parts rest'| |] eqn:Efields; try discriminate.
          destruct (fields_rejection (dec_tree fuel) IH (program arm) input'
                      Efields arm [] ops values) as [k [Hk Hrun]].
          exists (S k). split; [cbn [length]; lia|]. intros extra.
          rewrite Nat.add_succ_l, run_machine_step. cbn [machine_step]. rewrite Earm.
          exact (Hrun extra).
        * exists 1. split; [cbn [length]; lia|]. intros extra.
          change (1 + extra) with (S extra).
          rewrite run_machine_step. cbn [machine_step]. rewrite Earm. reflexivity.
  Qed.

  (** * 5. Main theorems *)

  (** Totality of the recursion: it reads a token for each tree, so it never
      needs more fuel than one unit for each token. *)

  Lemma dec_tree_shrinks : forall fuel input t rest,
      dec_tree fuel input = Done t rest -> length rest < length input.
  Proof.
    intros fuel input t rest Hdec.
    destruct (proj1 (tree_simulation fuel input) t rest Hdec [] [])
      as [k [Hk _]].
    lia.
  Qed.

  Lemma dec_trees_shrinks : forall sub,
      (forall x t r, sub x = Done t r -> length r <= length x) ->
      forall count input ts rest,
        dec_trees sub count input = Done ts rest -> length rest <= length input.
  Proof.
    intros sub Hshrink count. induction count as [|count IH];
      intros input ts rest Hdec; cbn [dec_trees] in Hdec.
    - inversion Hdec; subst. lia.
    - destruct (sub input) as [t r| |] eqn:Esub; try discriminate.
      destruct (dec_trees sub count r) as [ts' r'| |] eqn:Erest; try discriminate.
      inversion Hdec; subst.
      pose proof (Hshrink input t r Esub). pose proof (IH r ts' rest Erest). lia.
  Qed.

  Lemma prepend_not_stuck : forall p o, o <> Stuck -> prepend p o <> Stuck.
  Proof. intros p [parts rest| |] Hnot; cbn; congruence. Qed.

  Lemma dec_trees_total : forall sub bound,
      (forall x, length x <= bound -> sub x <> Stuck) ->
      (forall x t r, sub x = Done t r -> length r <= length x) ->
      forall count input, length input <= bound -> dec_trees sub count input <> Stuck.
  Proof.
    intros sub bound Htotal Hshrink count. induction count as [|count IH];
      intros input Hlength; cbn [dec_trees].
    - discriminate.
    - destruct (sub input) as [t r| |] eqn:Esub.
      + pose proof (Hshrink input t r Esub).
        destruct (dec_trees sub count r) as [ts rest| |] eqn:Erest; try discriminate.
        exfalso. apply (IH r); [lia|exact Erest].
      + discriminate.
      + exfalso. exact (Htotal input Hlength Esub).
  Qed.

  Lemma dec_fields_total : forall sub bound,
      (forall x, length x <= bound -> sub x <> Stuck) ->
      (forall x t r, sub x = Done t r -> length r <= length x) ->
      forall fs input, length input <= bound -> dec_fields sub fs input <> Stuck.
  Proof.
    intros sub bound Htotal Hshrink fs. induction fs as [|f fs IH];
      intros input Hlength.
    - cbn [dec_fields]. discriminate.
    - destruct input as [|token input']; cbn [dec_fields]; [discriminate|].
      cbn [length] in Hlength.
      destruct f.
      + destruct (bool_of_token token); [|discriminate].
        apply prepend_not_stuck. apply IH. lia.
      + destruct (dec_trees sub token input') as [ts after| |] eqn:Etrees.
        * apply prepend_not_stuck. apply IH.
          pose proof (dec_trees_shrinks sub Hshrink token input' ts after Etrees).
          lia.
        * discriminate.
        * exfalso. refine (dec_trees_total sub bound Htotal Hshrink token input' _ Etrees).
          lia.
      + destruct token as [|[|token]].
        * apply prepend_not_stuck. apply IH. lia.
        * destruct (sub input') as [t after| |] eqn:Esub.
          -- apply prepend_not_stuck. apply IH.
             pose proof (Hshrink input' t after Esub). lia.
          -- discriminate.
          -- exfalso. refine (Htotal input' _ Esub). lia.
        * discriminate.
  Qed.

  Lemma dec_tree_total : forall fuel input,
      length input < fuel -> dec_tree fuel input <> Stuck.
  Proof.
    induction fuel as [|fuel IH]; intros input Hlength; [lia|].
    destruct input as [|arm input']; cbn [dec_tree]; [discriminate|].
    cbn [length] in Hlength.
    destruct (Nat.ltb arm arm_count); [|discriminate].
    destruct (dec_fields (dec_tree fuel) (program arm) input') eqn:Efields;
      try discriminate.
    exfalso.
    refine (dec_fields_total (dec_tree fuel) (length input') _ _
              (program arm) input' (le_n _) Efields).
    - intros x Hx. apply IH. lia.
    - intros x t r Hx. pose proof (dec_tree_shrinks fuel x t r Hx). lia.
  Qed.

  Theorem decode_rec_total : forall input, decode_rec input <> Stuck.
  Proof. intros input. apply dec_tree_total. lia. Qed.

  (** The refinement: with [3 * |input| + 1] steps or more, the machine
      returns exactly what the recursion returns. The bound does not depend
      on the counts that the input claims, so a hostile count costs no more
      than the tokens that follow it. *)
  Theorem machine_refines_recursion : forall input extra,
      run (3 * length input + 1 + extra) [OTree] [] input =
      result_of (decode_rec input).
  Proof.
    intros input extra. unfold result_of.
    destruct (decode_rec input) as [t rest| |] eqn:Hdec.
    - destruct (proj1 (tree_simulation (S (length input)) input) t rest Hdec [] [])
        as [k [Hk Hrun]].
      replace (3 * length input + 1 + extra)
        with (k + (3 * length input + 1 + extra - k)) by lia.
      rewrite Hrun, run_nil. reflexivity.
    - destruct (proj2 (tree_simulation (S (length input)) input) Hdec [] [])
        as [k [Hk Hrun]].
      replace (3 * length input + 1 + extra)
        with (k + (3 * length input + 1 + extra - k)) by lia.
      apply Hrun.
    - exfalso. exact (decode_rec_total input Hdec).
  Qed.

  Corollary machine_steps_are_linear : forall input,
      run (3 * length input + 1) [OTree] [] input = result_of (decode_rec input).
  Proof.
    intros input. rewrite <- (Nat.add_0_r (3 * length input + 1)).
    apply machine_refines_recursion.
  Qed.

  (** More fuel never changes an answer. *)
  Lemma run_monotone : forall fuel ops values input,
      run fuel ops values input <> MStuck ->
      forall more, run (fuel + more) ops values input = run fuel ops values input.
  Proof.
    induction fuel as [|fuel IH]; intros ops values input Hnot more.
    - destruct ops as [|o ops']; [rewrite !run_nil; reflexivity|].
      exfalso. apply Hnot. reflexivity.
    - destruct ops as [|o ops']; [rewrite !run_nil; reflexivity|].
      rewrite Nat.add_succ_l, !run_machine_step.
      rewrite run_machine_step in Hnot. revert Hnot.
      destruct (machine_step o ops' values input) as [ops'' values' input'|r];
        intros Hnot; [|reflexivity].
      exact (IH ops'' values' input' Hnot more).
  Qed.

  (** The build step never finds too few values: the Rust MachineInvariant
      error is unreachable, for every input and every fuel. *)
  Theorem machine_never_reports_invariant : forall fuel input,
      run fuel [OTree] [] input <> MInvariant.
  Proof.
    intros fuel input Hinvariant.
    assert (Hnot : run fuel [OTree] [] input <> MStuck)
      by (rewrite Hinvariant; discriminate).
    pose proof (run_monotone fuel [OTree] [] input Hnot (3 * length input + 1))
      as Hmore.
    pose proof (machine_refines_recursion input fuel) as Hrefine.
    replace (3 * length input + 1 + fuel) with (fuel + (3 * length input + 1))
      in Hrefine by lia.
    rewrite Hmore, Hinvariant in Hrefine.
    unfold result_of in Hrefine.
    destruct (decode_rec input); discriminate.
  Qed.

  (** With too little fuel the machine stops without an answer. It never
      gives a wrong answer. *)
  Theorem machine_is_never_wrong : forall fuel input,
      run fuel [OTree] [] input = MStuck \/
      run fuel [OTree] [] input = result_of (decode_rec input).
  Proof.
    intros fuel input.
    remember (run fuel [OTree] [] input) as r eqn:Er.
    destruct r as [t rest| | |]; [right|right|right|left; reflexivity].
    all: assert (Hnot : run fuel [OTree] [] input <> MStuck)
           by (rewrite <- Er; discriminate).
    all: pose proof (run_monotone fuel [OTree] [] input Hnot (3 * length input + 1))
           as Hmore.
    all: pose proof (machine_refines_recursion input fuel) as Hrefine.
    all: replace (3 * length input + 1 + fuel) with (fuel + (3 * length input + 1))
           in Hrefine by lia.
    all: rewrite Hmore, <- Er in Hrefine; exact Hrefine.
  Qed.

  (** * 6. Records and the encoding *)

  (** The encoding: a node writes its arm, then each part in order. A bool
      is one token, and a list of trees is its count followed by the trees.
      An [FOption] part writes the same tokens, because its tag is 0 for no
      tree and 1 for one tree. *)
  Fixpoint encode (t : tree) : list nat :=
    match t with
    | Node arm parts =>
        arm :: flat_map (fun p =>
                           match p with
                           | inl b => [token_of_bool b]
                           | inr ts => length ts :: flat_map encode ts
                           end) parts
    end.

  Definition encode_part (p : part) : list nat :=
    match p with
    | inl b => [token_of_bool b]
    | inr ts => length ts :: flat_map encode ts
    end.

  Lemma encode_node : forall arm parts,
      encode (Node arm parts) = arm :: flat_map encode_part parts.
  Proof. reflexivity. Qed.

  (** A well-formed tree: each arm is in range, and its parts follow the
      program of its arm. *)
  Inductive wf_tree : tree -> Prop :=
  | wf_node : forall arm parts,
      arm < arm_count -> wf_parts (program arm) parts -> wf_tree (Node arm parts)
  with wf_parts : list field -> list part -> Prop :=
  | wf_parts_nil : wf_parts [] []
  | wf_parts_bool : forall fs b parts,
      wf_parts fs parts -> wf_parts (FBool :: fs) (inl b :: parts)
  | wf_parts_trees : forall fs ts parts,
      wf_forest ts -> wf_parts fs parts -> wf_parts (FTrees :: fs) (inr ts :: parts)
  | wf_parts_none : forall fs parts,
      wf_parts fs parts -> wf_parts (FOption :: fs) (inr [] :: parts)
  | wf_parts_some : forall fs t parts,
      wf_tree t -> wf_parts fs parts -> wf_parts (FOption :: fs) (inr [t] :: parts)
  with wf_forest : list tree -> Prop :=
  | wf_forest_nil : wf_forest []
  | wf_forest_cons : forall t ts,
      wf_tree t -> wf_forest ts -> wf_forest (t :: ts).

  Scheme wf_tree_mutual := Induction for wf_tree Sort Prop
  with wf_parts_mutual := Induction for wf_parts Sort Prop
  with wf_forest_mutual := Induction for wf_forest Sort Prop.

  Combined Scheme wf_mutual from wf_tree_mutual, wf_parts_mutual, wf_forest_mutual.

  (** Every well-formed tree, its parts and its sequences decode to
      themselves and leave the trailing tokens [s]. *)
  Lemma wf_round_trip :
    (forall t, wf_tree t ->
       forall fuel s, length (encode t) < fuel ->
         dec_tree fuel (encode t ++ s) = Done t s) /\
    (forall fs parts, wf_parts fs parts ->
       forall fuel s, length (flat_map encode_part parts) < fuel ->
         dec_fields (dec_tree fuel) fs (flat_map encode_part parts ++ s) =
         Done parts s) /\
    (forall ts, wf_forest ts ->
       forall fuel s, length (flat_map encode ts) < fuel ->
         dec_trees (dec_tree fuel) (length ts) (flat_map encode ts ++ s) =
         Done ts s).
  Proof.
    apply wf_mutual.
    - (* wf_node *)
      intros arm parts Harm Hparts IHparts fuel s Hfuel.
      rewrite encode_node in *. cbn [length] in Hfuel.
      destruct fuel as [|fuel]; [lia|].
      cbn [app dec_tree].
      replace (Nat.ltb arm arm_count) with true by (symmetry; apply Nat.ltb_lt; exact Harm).
      rewrite (IHparts fuel s) by lia. reflexivity.
    - (* wf_parts_nil *)
      intros fuel s Hfuel. reflexivity.
    - (* wf_parts_bool *)
      intros fs b parts Hparts IHparts fuel s Hfuel.
      cbn [flat_map encode_part app length] in *.
      cbn [dec_fields].
      replace (bool_of_token (token_of_bool b)) with (Some b) by (destruct b; reflexivity).
      rewrite (IHparts fuel s) by lia. reflexivity.
    - (* wf_parts_trees *)
      intros fs ts parts Hforest IHforest Hparts IHparts fuel s Hfuel.
      cbn [flat_map encode_part app length] in *. rewrite length_app in Hfuel.
      rewrite <- (app_assoc (flat_map encode ts)). cbn [dec_fields].
      rewrite (IHforest fuel (flat_map encode_part parts ++ s)) by lia.
      cbn [prepend]. rewrite (IHparts fuel s) by lia. reflexivity.
    - (* wf_parts_none *)
      intros fs parts Hparts IHparts fuel s Hfuel.
      cbn [flat_map encode_part length app] in *.
      cbn [dec_fields prepend].
      rewrite (IHparts fuel s) by lia. reflexivity.
    - (* wf_parts_some *)
      intros fs t parts Ht IHt Hparts IHparts fuel s Hfuel.
      cbn [flat_map encode_part length app] in *.
      rewrite app_nil_r in *. rewrite length_app in Hfuel.
      rewrite <- (app_assoc (encode t)). cbn [dec_fields].
      rewrite (IHt fuel (flat_map encode_part parts ++ s)) by lia.
      cbn [prepend]. rewrite (IHparts fuel s) by lia. reflexivity.
    - (* wf_forest_nil *)
      intros fuel s Hfuel. reflexivity.
    - (* wf_forest_cons *)
      intros t ts Ht IHt Hts IHts fuel s Hfuel.
      cbn [flat_map length] in *. rewrite length_app in Hfuel.
      rewrite <- (app_assoc (encode t)). cbn [dec_trees].
      rewrite (IHt fuel (flat_map encode ts ++ s)) by lia.
      cbv beta iota. rewrite (IHts fuel s) by lia. reflexivity.
  Qed.

  Theorem decode_rec_round_trip : forall t s, wf_tree t ->
      decode_rec (encode t ++ s) = Done t s.
  Proof.
    intros t s Hwf. unfold decode_rec.
    apply (proj1 wf_round_trip t Hwf). rewrite length_app. lia.
  Qed.

  Corollary machine_round_trip : forall t s extra, wf_tree t ->
      run (3 * length (encode t ++ s) + 1 + extra) [OTree] [] (encode t ++ s) =
      MDone t s.
  Proof.
    intros t s extra Hwf.
    rewrite machine_refines_recursion, decode_rec_round_trip by exact Hwf.
    reflexivity.
  Qed.

  Section Record.
    Variable A : Type.
    (** The bounded tail of a row, read by the derived decoder. *)
    Variable read_tail : list nat -> outcome A.

    Definition then_tail (o : outcome tree) : outcome (tree * A) :=
      match o with
      | Done t rest =>
          match read_tail rest with
          | Done a rest' => Done (t, a) rest'
          | Reject => Reject
          | Stuck => Stuck
          end
      | Reject => Reject
      | Stuck => Stuck
      end.

    (** A derived struct decoder reads its fields in order with no framing,
        so the derived decoder of a row reads the root and then the tail. *)
    Definition record_rec (input : list nat) : outcome (tree * A) :=
      then_tail (decode_rec input).

    (** The record functions of the Rust code: the machine reads the root and
        reports where it ended, and the derived decoder reads the tail from
        there. *)
    Definition record_machine (input : list nat) : outcome (tree * A) :=
      match run (3 * length input + 1) [OTree] [] input with
      | MDone t rest => then_tail (Done t rest)
      | MReject => Reject
      | MInvariant | MStuck => Stuck
      end.

    Theorem record_machine_refines_record_rec : forall input,
        record_machine input = record_rec input.
    Proof.
      intros input. unfold record_machine, record_rec.
      rewrite machine_steps_are_linear. unfold result_of.
      destruct (decode_rec input) as [t rest| |]; reflexivity.
    Qed.

    Variable write_tail : A -> list nat.
    Hypothesis tail_round_trip : forall a s, read_tail (write_tail a ++ s) = Done a s.

    (** A row is the concatenation of its root and its tail: it decodes to
        both, and the trailing tokens stay the remainder. *)
    Theorem record_round_trip : forall t a s, wf_tree t ->
        record_machine (encode t ++ write_tail a ++ s) = Done (t, a) s.
    Proof.
      intros t a s Hwf.
      rewrite record_machine_refines_record_rec. unfold record_rec.
      rewrite decode_rec_round_trip by exact Hwf.
      cbn [then_tail]. rewrite tail_round_trip. reflexivity.
    Qed.
  End Record.
End Decoder.

(** * 7. Negative controls *)

(** A step that reads every Option tag above 1 as [Some]. *)
Definition option_program (_ : nat) : list field := [FOption].

Definition step_lenient_tag (o : op) (ops : list op) (values : list tree)
  (input : list nat) : transition :=
  match o, input with
  | OFields arm (FOption :: fs) done, S (S _) :: rest =>
      Continue (OTree :: OFields arm fs (done ++ [QTrees 1]) :: ops) values rest
  | _, _ => machine_step 1 option_program o ops values input
  end.

Example lenient_option_tag_breaks_the_refinement :
  run_with step_lenient_tag 20 [OTree] [] [0; 2; 0; 0] <>
  result_of (decode_rec 1 option_program [0; 2; 0; 0]).
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.

(** A build step that takes the children from the stack without reversing
    them puts them in reverse stream order. *)
Definition trees_program (_ : nat) : list field := [FTrees].

Definition build_unreversed (arm : nat) (done : list pending)
  (values : list tree) : option (list tree) :=
  let count := pending_trees done in
  if Nat.leb count (length values)
  then Some (Node arm (attach done (firstn count values)) :: skipn count values)
  else None.

Definition step_unreversed (o : op) (ops : list op) (values : list tree)
  (input : list nat) : transition :=
  match o with
  | OFields arm [] done =>
      match build_unreversed arm done values with
      | Some values' => Continue ops values' input
      | None => Halt MInvariant
      end
  | _ => machine_step 1 trees_program o ops values input
  end.

(** A root with two different children. *)
Definition two_children_input : list nat :=
  encode (Node 0 [inr [Node 0 [inr []]; Node 0 [inr [Node 0 [inr []]]]]]).

Example unreversed_children_break_the_refinement :
  run_with step_unreversed 50 [OTree] [] two_children_input <>
  result_of (decode_rec 1 trees_program two_children_input).
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.

(** A range check written with [<=] instead of [<] is the machine of
    [arm_count + 1]: it accepts the first invalid arm. *)
Definition empty_program (_ : nat) : list field := [].

Example off_by_one_range_check_breaks_the_refinement :
  run 2 empty_program 10 [OTree] [] [1] <>
  result_of (decode_rec 1 empty_program [1]).
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.

(** A machine whose field program has drifted from the schema: it reads the
    bool before the trees. *)
Definition trees_then_bool (_ : nat) : list field := [FTrees; FBool].
Definition bool_then_trees (_ : nat) : list field := [FBool; FTrees].

(** A root with one child and the bool [true], in the schema
    [trees_then_bool]. *)
Definition drift_input : list nat :=
  encode (Node 0 [inr [Node 0 [inr []; inl false]]; inl true]).

Example field_order_drift_breaks_the_refinement :
  run 1 bool_then_trees 50 [OTree] [] drift_input <>
  result_of (decode_rec 1 trees_then_bool drift_input).
Proof. vm_compute. intros Hequal. inversion Hequal. Qed.
