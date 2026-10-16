From Stdlib Require Import Lists.List.
Import ListNotations.

(* A field token denotes its complete positional bincode bytes, including the
   collection length. We leave those bytes opaque so the laws quantify over
   every possible cells payload and every possible branch payload. Rust golden
   tests bind the token interpretation to the pre-v7 encoder. Tokens retain
   FIELD BOUNDARIES that raw positional bincode omits: a wrong-epoch raw
   decoder may parse a different value from the same bytes. The authenticated
   epoch, rather than a byte-shape guess, selects the production decoder. *)
Inductive codec_epoch := Legacy | CostV1 | CostV2.
Inductive stack_field := Cells (bytes : list nat) | ParallelTail (bytes : list nat).

Definition old_cost_v1_stack (cells : list nat) : list stack_field :=
  [Cells cells].

(* A standalone CostStack never existed in Legacy. CostV1 cannot silently
   erase a nonempty shared-prefix tree. CostV2 adds precisely one positional
   parallel-tail field, even when its payload encodes an empty vector. *)
Definition encode_stack (epoch : codec_epoch) (cells tail : list nat)
    : option (list stack_field) :=
  match epoch with
  | Legacy => None
  | CostV1 => match tail with
              | [] => Some (old_cost_v1_stack cells)
              | _ :: _ => None
              end
  | CostV2 => Some [Cells cells; ParallelTail tail]
  end.

Definition decode_stack (epoch : codec_epoch) (fields : list stack_field)
    : option (list nat * list nat) :=
  match epoch, fields with
  | CostV1, [Cells cells] => Some (cells, [])
  | CostV2, [Cells cells; ParallelTail tail] => Some (cells, tail)
  | _, _ => None
  end.

Theorem cost_v1_exact_historical_bytes : forall cells,
  encode_stack CostV1 cells [] = Some (old_cost_v1_stack cells).
Proof. reflexivity. Qed.

Theorem cost_v1_refuses_every_branched_tail : forall cells head rest,
  encode_stack CostV1 cells (head :: rest) = None.
Proof. reflexivity. Qed.

Theorem cost_v1_output_iff_linear : forall cells tail fields,
  encode_stack CostV1 cells tail = Some fields <->
  tail = [] /\ fields = old_cost_v1_stack cells.
Proof.
  intros cells tail fields. destruct tail as [|head rest]; simpl.
  - split; intro H; [injection H as <-; auto|].
    destruct H as [_ ->]. reflexivity.
  - split; intro H; [discriminate|].
    destruct H as [H _]. discriminate.
Qed.

Theorem cost_v1_decodes_only_historical_shape : forall cells,
  decode_stack CostV1 (old_cost_v1_stack cells) = Some (cells, []).
Proof. reflexivity. Qed.

Theorem cost_v2_round_trip : forall cells tail,
  match encode_stack CostV2 cells tail with
  | Some fields => decode_stack CostV2 fields = Some (cells, tail)
  | None => False
  end.
Proof. reflexivity. Qed.

(* This is separation of typed field programs, NOT a claim that the two raw
   byte languages are disjoint or that wrong-epoch outer-record decode fails. *)
Theorem typed_v1_v2_field_programs_differ : forall cells tail,
  decode_stack CostV1 [Cells cells; ParallelTail tail] = None /\
  decode_stack CostV2 (old_cost_v1_stack cells) = None.
Proof. intros; split; reflexivity. Qed.

Theorem legacy_refuses_standalone_stack : forall cells tail,
  encode_stack Legacy cells tail = None.
Proof. reflexivity. Qed.

Print Assumptions cost_v1_exact_historical_bytes.
Print Assumptions cost_v1_refuses_every_branched_tail.
Print Assumptions cost_v1_output_iff_linear.
Print Assumptions cost_v1_decodes_only_historical_shape.
Print Assumptions cost_v2_round_trip.
Print Assumptions typed_v1_v2_field_programs_differ.
Print Assumptions legacy_refuses_standalone_stack.
