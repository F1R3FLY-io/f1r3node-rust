From Stdlib Require Import List NArith String Ascii.
From NodeObservationB11 Require Import Wire.
Import ListNotations.
Open Scope N_scope.

Fixpoint text_bytes (s : string) : wire :=
  match s with EmptyString => [] | String head rest => N.of_nat (nat_of_ascii head) :: text_bytes rest end.

Definition tagged {A} (tag : string) (item : codec A) := prefixed (emit blob (text_bytes tag)) item.

Record metadata := {
  metadata_block_hash : wire;
  metadata_parents : list wire;
  metadata_sender : wire;
  metadata_justifications : list (wire * wire);
  metadata_weights : list (wire * N);
  metadata_block_number : N;
  metadata_sequence_number : N;
  metadata_invalid : bool;
  metadata_directly_finalized : bool;
  metadata_finalized : bool;
  metadata_fault_tolerance_bits : N;
  metadata_merge_base : wire
}.

Definition metadata_tuple_codec :=
  pair_codec (blob)
    (pair_codec (list_codec blob)
    (pair_codec (blob)
    (pair_codec (list_codec
    (pair_codec blob blob))
    (pair_codec (list_codec
    (pair_codec blob (uint 8)))
    (pair_codec (uint 8)
    (pair_codec (uint 4)
    (pair_codec (boolean)
    (pair_codec (boolean)
    (pair_codec (boolean)
    (pair_codec (uint 4) ((blob)))))))))))).

Definition metadata_unpack (value : metadata) :=
  (metadata_block_hash value, (metadata_parents value, (metadata_sender value, (metadata_justifications value, (metadata_weights value, (metadata_block_number value, (metadata_sequence_number value, (metadata_invalid value, (metadata_directly_finalized value, (metadata_finalized value, (metadata_fault_tolerance_bits value, metadata_merge_base value))))))))))).

Definition metadata_pack (value : ((wire) * ((list wire) * ((wire) * ((list (wire * wire)) * ((list (wire * N)) * ((N) * ((N) * ((bool) * ((bool) * ((bool) * ((N) * (wire))))))))))))%type) : metadata :=
  let '(v0, (v1, (v2, (v3, (v4, (v5, (v6, (v7, (v8, (v9, (v10, v11))))))))))) := value in
  {| metadata_block_hash := v0;
     metadata_parents := v1;
     metadata_sender := v2;
     metadata_justifications := v3;
     metadata_weights := v4;
     metadata_block_number := v5;
     metadata_sequence_number := v6;
     metadata_invalid := v7;
     metadata_directly_finalized := v8;
     metadata_finalized := v9;
     metadata_fault_tolerance_bits := v10;
     metadata_merge_base := v11 |}.

Lemma metadata_inverse : forall value, metadata_pack (metadata_unpack value) = value.
Proof. intros []; reflexivity. Qed.

Definition metadata_codec := mapped metadata_tuple_codec metadata_pack metadata_unpack metadata_inverse.

Record snapshot := {
  snapshot_schema_version : N;
  snapshot_scope : wire;
  snapshot_limits : list N * N;
  snapshot_usage : list N;
  snapshot_coverage : N * (bool * N);
  snapshot_generation : N;
  snapshot_transactions : list (wire * (N * (N * option N)));
  snapshot_dag_set : list wire;
  snapshot_child_map : list (wire * list wire);
  snapshot_height_map : list (N * list wire);
  snapshot_block_number_map : list (wire * N);
  snapshot_main_parent_map : list (wire * wire);
  snapshot_self_justification_map : list (wire * wire);
  snapshot_last_finalized_block : option (wire * N);
  snapshot_finalized_block_set : list wire;
  snapshot_latest_messages : list (wire * wire);
  snapshot_invalid_blocks : list (wire * metadata);
  snapshot_blocks : list (wire * (metadata * (option wire * option wire)));
  snapshot_bodies : list (wire * option wire);
  snapshot_work : N
}.

Definition snapshot_tuple_codec :=
  pair_codec (tagged "schema" (uint 4))
    (pair_codec (blob)
    (pair_codec (tagged "limits"
    (pair_codec (fixed_rows 12 (uint 8)) (uint 4)))
    (pair_codec (tagged "read usage" (fixed_rows 3 (uint 8)))
    (pair_codec (tagged "coverage"
    (pair_codec (uint 8)
    (pair_codec boolean (uint 8))))
    (pair_codec (tagged "generation" (uint 8))
    (pair_codec (tagged "transactions" (list_codec
    (pair_codec blob
    (pair_codec (uint 8)
    (pair_codec (uint 8) (optional (uint 8)))))))
    (pair_codec (tagged "dag_set" (list_codec blob))
    (pair_codec (tagged "child_map" (list_codec
    (pair_codec blob (list_codec blob))))
    (pair_codec (tagged "height_map" (list_codec
    (pair_codec (uint 8) (list_codec blob))))
    (pair_codec (tagged "block_number_map" (list_codec
    (pair_codec blob (uint 8))))
    (pair_codec (tagged "main_parent_map" (list_codec
    (pair_codec blob blob)))
    (pair_codec (tagged "self_justification_map" (list_codec
    (pair_codec blob blob)))
    (pair_codec (tagged "last_finalized_block" (optional
    (pair_codec blob (uint 8))))
    (pair_codec (tagged "finalized_block_set" (list_codec blob))
    (pair_codec (tagged "latest_messages" (list_codec
    (pair_codec blob blob)))
    (pair_codec (tagged "invalid_blocks" (list_codec
    (pair_codec blob metadata_codec)))
    (pair_codec (tagged "blocks" (list_codec
    (pair_codec blob
    (pair_codec metadata_codec
    (pair_codec (optional blob) (optional blob))))))
    (pair_codec (tagged "bodies" (list_codec
    (pair_codec blob (optional blob)))) ((tagged "work" (uint 8))))))))))))))))))))).

Definition snapshot_unpack (value : snapshot) :=
  (snapshot_schema_version value, (snapshot_scope value, (snapshot_limits value, (snapshot_usage value, (snapshot_coverage value, (snapshot_generation value, (snapshot_transactions value, (snapshot_dag_set value, (snapshot_child_map value, (snapshot_height_map value, (snapshot_block_number_map value, (snapshot_main_parent_map value, (snapshot_self_justification_map value, (snapshot_last_finalized_block value, (snapshot_finalized_block_set value, (snapshot_latest_messages value, (snapshot_invalid_blocks value, (snapshot_blocks value, (snapshot_bodies value, snapshot_work value))))))))))))))))))).

Definition snapshot_pack (value : ((N) * ((wire) * ((list N * N) * ((list N) * ((N * (bool * N)) * ((N) * ((list (wire * (N * (N * option N)))) * ((list wire) * ((list (wire * list wire)) * ((list (N * list wire)) * ((list (wire * N)) * ((list (wire * wire)) * ((list (wire * wire)) * ((option (wire * N)) * ((list wire) * ((list (wire * wire)) * ((list (wire * metadata)) * ((list (wire * (metadata * (option wire * option wire)))) * ((list (wire * option wire)) * (N))))))))))))))))))))%type) : snapshot :=
  let '(v0, (v1, (v2, (v3, (v4, (v5, (v6, (v7, (v8, (v9, (v10, (v11, (v12, (v13, (v14, (v15, (v16, (v17, (v18, v19))))))))))))))))))) := value in
  {| snapshot_schema_version := v0;
     snapshot_scope := v1;
     snapshot_limits := v2;
     snapshot_usage := v3;
     snapshot_coverage := v4;
     snapshot_generation := v5;
     snapshot_transactions := v6;
     snapshot_dag_set := v7;
     snapshot_child_map := v8;
     snapshot_height_map := v9;
     snapshot_block_number_map := v10;
     snapshot_main_parent_map := v11;
     snapshot_self_justification_map := v12;
     snapshot_last_finalized_block := v13;
     snapshot_finalized_block_set := v14;
     snapshot_latest_messages := v15;
     snapshot_invalid_blocks := v16;
     snapshot_blocks := v17;
     snapshot_bodies := v18;
     snapshot_work := v19 |}.

Lemma snapshot_inverse : forall value, snapshot_pack (snapshot_unpack value) = value.
Proof. intros []; reflexivity. Qed.

Definition snapshot_codec := mapped snapshot_tuple_codec snapshot_pack snapshot_unpack snapshot_inverse.
