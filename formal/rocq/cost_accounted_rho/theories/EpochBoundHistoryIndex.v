(* An abstract, collision-free *preimage* model of the channel-key and
   block-index boundaries. Digest collision resistance is not proved here.

   Rust correspondence:
   - channel_hash_for_epoch routes ordinary and cost-bearing channels;
   - EpochHistoryReader typed and by-hash methods share one bound decoder;
   - create_event_log_index validates an event in the adopted epoch before
     using its already-hashed channel key;
   - BlockIndex.event_epoch guards cache reuse.

   The model deliberately does not require CostV1 and CostV2 cost-bearing
   keys to differ: their canonical byte encodings may coincide for a value.
   Both are disjoint from ordinary key *preimages* by the cost domain tag. *)

From Stdlib Require Import Arith.PeanoNat Bool.Bool.

Inductive epoch := Legacy | CostV1 | CostV2.
Inductive channel_key := OrdinaryKey (channel : nat) | CostKey (channel : nat).

Definition route (version : epoch) (cost_bearing : bool) (channel : nat)
  : option channel_key :=
  if cost_bearing then
    match version with
    | Legacy => None
    | CostV1 | CostV2 => Some (CostKey channel)
    end
  else Some (OrdinaryKey channel).

Theorem ordinary_key_is_epoch_independent :
  forall first second channel,
    route first false channel = route second false channel.
Proof. reflexivity. Qed.

Theorem legacy_rejects_cost_bearing_channel :
  forall channel, route Legacy true channel = None.
Proof. reflexivity. Qed.

Theorem cost_key_never_aliases_ordinary_preimage :
  forall version cost_channel ordinary_channel,
    route version true cost_channel <> Some (OrdinaryKey ordinary_channel).
Proof.
  destruct version; intros; simpl; discriminate.
Qed.

Definition history := epoch -> channel_key -> option nat.

Definition read_by_hash (store : history) (version : epoch)
  (key : channel_key) : option nat := store version key.

Definition read_typed (store : history) (version : epoch)
  (cost_bearing : bool) (channel : nat) : option nat :=
  match route version cost_bearing channel with
  | None => None
  | Some key => read_by_hash store version key
  end.

Theorem typed_and_authenticated_by_hash_read_agree :
  forall store version cost_bearing channel key,
    route version cost_bearing channel = Some key ->
    read_typed store version cost_bearing channel =
      read_by_hash store version key.
Proof.
  intros store version cost_bearing channel key routed.
  unfold read_typed. rewrite routed. reflexivity.
Qed.

Definition epoch_equal (left right : epoch) : bool :=
  match left, right with
  | Legacy, Legacy | CostV1, CostV1 | CostV2, CostV2 => true
  | _, _ => false
  end.

Record cached_index := {
  index_epoch : epoch;
  index_value : nat
}.

Definition cached_lookup (request : epoch) (entry : cached_index)
  : option nat :=
  if epoch_equal request (index_epoch entry)
  then Some (index_value entry)
  else None.

Theorem cache_hit_requires_the_same_epoch :
  forall request entry value,
    cached_lookup request entry = Some value ->
    request = index_epoch entry.
Proof.
  intros request [stored_epoch stored_value] value hit.
  destruct request, stored_epoch; simpl in hit;
    try discriminate; reflexivity.
Qed.

Theorem cache_miss_on_epoch_mismatch :
  forall request entry,
    request <> index_epoch entry -> cached_lookup request entry = None.
Proof.
  intros request [stored_epoch stored_value] different.
  destruct request, stored_epoch; simpl in *;
    try reflexivity; contradiction different; reflexivity.
Qed.
