From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import EPM1 EPathMap EPM1ACTree03.

Import ListNotations.

Definition act_hash_u64_modulus : N := N.pow 2 64.

Definition act_hash_rotl64 (value : N) (amount : N) : N :=
  N.modulo
    (N.lor (N.shiftl value amount)
      (N.shiftr value (64 - amount)))
    act_hash_u64_modulus.

Definition act_fallback_step (state : N * N) (byte : nat) : N * N :=
  let '(lo, hi) := state in
  (act_hash_rotl64
     (N.modulo (lo + N.of_nat byte) act_hash_u64_modulus) 3,
   N.lxor hi (act_hash_rotl64 (N.of_nat byte) 11)).

Definition act_fallback_low_step (lo : N) (byte : nat) : N :=
  act_hash_rotl64
    (N.modulo (lo + N.of_nat byte) act_hash_u64_modulus) 3.

Definition act_fallback_hash_n (bytes : list nat) : N :=
  fst (fold_left act_fallback_step bytes (0%N, 0%N)).

Definition act_fallback_hash (bytes : list nat) : nat :=
  N.to_nat (act_fallback_hash_n bytes).

Theorem act_fallback_hash_n_adapter :
  forall bytes, N.of_nat (act_fallback_hash bytes) = act_fallback_hash_n bytes.
Proof.
  intros bytes. unfold act_fallback_hash. apply N2Nat.id.
Qed.

Lemma act_fallback_low_ignores_high :
  forall bytes lo hi,
    fst (fold_left act_fallback_step bytes (lo, hi)) =
    fold_left act_fallback_low_step bytes lo.
Proof.
  induction bytes as [|byte rest IH]; intros lo hi; simpl.
  - reflexivity.
  - apply IH.
Qed.

Definition act_hash_xor (left right : nat) : nat :=
  N.to_nat (N.lxor (N.of_nat left) (N.of_nat right)).

Definition act_hash_add8 (left right : nat) : nat :=
  (left + right) mod 256.

Definition act_hash_word_le (word : N) : list nat :=
  map (fun index =>
    N.to_nat (N.land (N.shiftr word (N.of_nat (8 * index))) 255))
    (seq 0 4).

Definition act_gx_keys : list N :=
  [0xF2784542%N; 0xB09D3E21%N; 0x89C222E5%N; 0xFC3BC28E%N;
   0x03FCE279%N; 0xCB6B2E9B%N; 0xB361DC58%N; 0x39132BD9%N;
   0xD0012E32%N; 0x689D2B7D%N; 0x5544B1B7%N; 0xC78B122B%N].

Definition act_gx_key (index : nat) : list nat :=
  concat (map act_hash_word_le
    (firstn 4 (skipn (4 * index) act_gx_keys))).

Definition act_aes_sbox : list nat :=
  [0x63; 0x7c; 0x77; 0x7b; 0xf2; 0x6b; 0x6f; 0xc5; 0x30; 0x01; 0x67; 0x2b; 0xfe; 0xd7; 0xab; 0x76;
   0xca; 0x82; 0xc9; 0x7d; 0xfa; 0x59; 0x47; 0xf0; 0xad; 0xd4; 0xa2; 0xaf; 0x9c; 0xa4; 0x72; 0xc0;
   0xb7; 0xfd; 0x93; 0x26; 0x36; 0x3f; 0xf7; 0xcc; 0x34; 0xa5; 0xe5; 0xf1; 0x71; 0xd8; 0x31; 0x15;
   0x04; 0xc7; 0x23; 0xc3; 0x18; 0x96; 0x05; 0x9a; 0x07; 0x12; 0x80; 0xe2; 0xeb; 0x27; 0xb2; 0x75;
   0x09; 0x83; 0x2c; 0x1a; 0x1b; 0x6e; 0x5a; 0xa0; 0x52; 0x3b; 0xd6; 0xb3; 0x29; 0xe3; 0x2f; 0x84;
   0x53; 0xd1; 0x00; 0xed; 0x20; 0xfc; 0xb1; 0x5b; 0x6a; 0xcb; 0xbe; 0x39; 0x4a; 0x4c; 0x58; 0xcf;
   0xd0; 0xef; 0xaa; 0xfb; 0x43; 0x4d; 0x33; 0x85; 0x45; 0xf9; 0x02; 0x7f; 0x50; 0x3c; 0x9f; 0xa8;
   0x51; 0xa3; 0x40; 0x8f; 0x92; 0x9d; 0x38; 0xf5; 0xbc; 0xb6; 0xda; 0x21; 0x10; 0xff; 0xf3; 0xd2;
   0xcd; 0x0c; 0x13; 0xec; 0x5f; 0x97; 0x44; 0x17; 0xc4; 0xa7; 0x7e; 0x3d; 0x64; 0x5d; 0x19; 0x73;
   0x60; 0x81; 0x4f; 0xdc; 0x22; 0x2a; 0x90; 0x88; 0x46; 0xee; 0xb8; 0x14; 0xde; 0x5e; 0x0b; 0xdb;
   0xe0; 0x32; 0x3a; 0x0a; 0x49; 0x06; 0x24; 0x5c; 0xc2; 0xd3; 0xac; 0x62; 0x91; 0x95; 0xe4; 0x79;
   0xe7; 0xc8; 0x37; 0x6d; 0x8d; 0xd5; 0x4e; 0xa9; 0x6c; 0x56; 0xf4; 0xea; 0x65; 0x7a; 0xae; 0x08;
   0xba; 0x78; 0x25; 0x2e; 0x1c; 0xa6; 0xb4; 0xc6; 0xe8; 0xdd; 0x74; 0x1f; 0x4b; 0xbd; 0x8b; 0x8a;
   0x70; 0x3e; 0xb5; 0x66; 0x48; 0x03; 0xf6; 0x0e; 0x61; 0x35; 0x57; 0xb9; 0x86; 0xc1; 0x1d; 0x9e;
   0xe1; 0xf8; 0x98; 0x11; 0x69; 0xd9; 0x8e; 0x94; 0x9b; 0x1e; 0x87; 0xe9; 0xce; 0x55; 0x28; 0xdf;
   0x8c; 0xa1; 0x89; 0x0d; 0xbf; 0xe6; 0x42; 0x68; 0x41; 0x99; 0x2d; 0x0f; 0xb0; 0x54; 0xbb; 0x16].

Definition act_aes_shifted_byte (state : list nat) (index : nat) : nat :=
  let row := index mod 4 in
  let column := index / 4 in
  nth (nth (((column + row) mod 4) * 4 + row) state 0)
    act_aes_sbox 0.

Definition act_aes_xtime (byte : nat) : nat :=
  act_hash_xor ((2 * byte) mod 256)
    (if 128 <=? byte then 27 else 0).

Definition act_aes_mixed_byte (state : list nat) (index : nat) : nat :=
  let base := 4 * (index / 4) in
  let row := index mod 4 in
  let first := act_aes_shifted_byte state (base + row) in
  let next := act_aes_shifted_byte state (base + (row + 1) mod 4) in
  let all := fold_left act_hash_xor
    (map (fun r => act_aes_shifted_byte state (base + r)) (seq 0 4)) 0 in
  act_hash_xor (act_hash_xor first all)
    (act_aes_xtime (act_hash_xor first next)).

Definition act_aesenc (state key : list nat) : list nat :=
  map (fun index =>
    act_hash_xor (act_aes_mixed_byte state index) (nth index key 0))
    (seq 0 16).

Definition act_aesenclast (state key : list nat) : list nat :=
  map (fun index =>
    act_hash_xor (act_aes_shifted_byte state index) (nth index key 0))
    (seq 0 16).

Definition act_gx_add_vector (left right : list nat) : list nat :=
  map (fun index =>
    act_hash_add8 (nth index left 0) (nth index right 0))
    (seq 0 16).

Definition act_gx_block (bytes : list nat) (start : nat) : list nat :=
  firstn 16 (skipn start bytes).

Definition act_gx_partial (bytes : list nat) (start count : nat)
  : list nat :=
  map (fun byte => act_hash_add8 byte count)
    (firstn 16 (firstn count (skipn start bytes) ++ repeat 0 16)).

Definition act_gx_len_vector (len : nat) : list nat :=
  concat (repeat (act_hash_word_le (N.of_nat len)) 4).

Fixpoint act_gx_fold_blocks (fuel : nat) (bytes : list nat)
  (start : nat) (state : list nat) : list nat :=
  match fuel with
  | 0 => state
  | S remaining =>
      act_gx_fold_blocks remaining bytes (start + 16)
        (act_aesenc state (act_gx_block bytes start))
  end.

Fixpoint act_gx_fold_eight (fuel : nat) (bytes : list nat)
  (start len : nat) (t1 t2 lane1 lane2 : list nat) : list nat :=
  match fuel with
  | 0 =>
      let length_vector := act_gx_len_vector len in
      act_aesenc (act_gx_add_vector lane1 length_vector)
        (act_gx_add_vector lane2 length_vector)
  | S remaining =>
      let b0 := act_gx_block bytes start in
      let b1 := act_gx_block bytes (start + 16) in
      let b2 := act_gx_block bytes (start + 32) in
      let b3 := act_gx_block bytes (start + 48) in
      let b4 := act_gx_block bytes (start + 64) in
      let b5 := act_gx_block bytes (start + 80) in
      let b6 := act_gx_block bytes (start + 96) in
      let b7 := act_gx_block bytes (start + 112) in
      let tmp1 := act_aesenc (act_aesenc (act_aesenc b0 b2) b4) b6 in
      let tmp2 := act_aesenc (act_aesenc (act_aesenc b1 b3) b5) b7 in
      let next_t1 := act_gx_add_vector t1 (act_gx_key 0) in
      let next_t2 := act_gx_add_vector t2 (act_gx_key 1) in
      act_gx_fold_eight remaining bytes (start + 128) len next_t1 next_t2
        (act_aesenclast (act_aesenc tmp1 next_t1) lane1)
        (act_aesenclast (act_aesenc tmp2 next_t2) lane2)
  end.

Definition act_gx_compress_all (bytes : list nat) : list nat :=
  let len := length bytes in
  if len =? 0 then repeat 0 16
  else if len <=? 16 then act_gx_partial bytes 0 len
  else
    let extra := len mod 16 in
    let state := if extra =? 0 then act_gx_block bytes 0
                 else act_gx_partial bytes 0 extra in
    let position := if extra =? 0 then 16 else extra in
    let v0 := act_gx_block bytes position in
    let after_v0 := position + 16 in
    let v0 := if 32 <? len then
                act_aesenc v0 (act_gx_block bytes after_v0)
              else v0 in
    let after_v1 := after_v0 + 16 in
    let v0 := if 48 <? len then
                act_aesenc v0 (act_gx_block bytes after_v1)
              else v0 in
    let after_v2 := after_v1 + 16 in
    let state := if 64 <? len then
      let remaining := len - after_v2 in
      let leading := remaining / 16 - 8 * (remaining / 128) in
      let folded := act_gx_fold_blocks leading bytes after_v2 state in
      act_gx_fold_eight (remaining / 128) bytes
        (after_v2 + 16 * leading) len
        (repeat 0 16) (repeat 0 16) folded folded
      else state in
    act_aesenclast state
      (act_aesenc (act_aesenc v0 (act_gx_key 0)) (act_gx_key 1)).

Definition act_gx_final_vector (bytes : list nat) : list nat :=
  let initial_key := act_aesenc (repeat 0 16) (act_gx_key 0) in
  let state := act_aesenclast (act_gx_compress_all bytes) initial_key in
  let state := act_aesenc state (act_gx_key 0) in
  let state := act_aesenc state (act_gx_key 1) in
  act_aesenclast state (act_gx_key 2).

Definition act_hash_little_endian_n (digits : list nat) : N :=
  fold_right (fun digit value =>
    (N.of_nat digit + 256 * value)%N) 0%N digits.

Definition act_gx_hash_n (bytes : list nat) : N :=
  act_hash_little_endian_n (firstn 8 (act_gx_final_vector bytes)).

Definition act_gx_hash (bytes : list nat) : nat :=
  N.to_nat (act_gx_hash_n bytes).

Theorem act_gx_hash_n_adapter :
  forall bytes, N.of_nat (act_gx_hash bytes) = act_gx_hash_n bytes.
Proof.
  intros bytes. unfold act_gx_hash. apply N2Nat.id.
Qed.

Definition act_arch_hash (fallback : bool) : list nat -> nat :=
  if fallback then act_fallback_hash else act_gx_hash.

Definition act_hash_boundary_lengths : list nat :=
  [0; 1; 15; 16; 17; 31; 32; 33; 48; 49;
   64; 65; 127; 128; 129; 191; 192; 255; 256; 257].

Definition act_hash_vector (variant len : nat) : list nat :=
  map (fun index =>
    if variant =? 0 then (17 * index + 29) mod 256
    else 255 - ((37 * index + 11) mod 256))
    (seq 0 len).

Definition act_hash_corpus : list (nat * nat) :=
  flat_map (fun len => [(0, len); (1, len)]) act_hash_boundary_lengths.

Definition act_gx_corpus_results : list N :=
  map (fun '(variant, len) => act_gx_hash_n (act_hash_vector variant len))
    act_hash_corpus.

Definition act_fallback_corpus_results : list N :=
  map (fun '(variant, len) =>
    act_fallback_hash_n (act_hash_vector variant len)) act_hash_corpus.

Theorem act_fallback_replay_iff_canonical_step :
  forall cache position line_start line_end suffix next next_cache,
    act_replay_line act_fallback_hash cache position line_start line_end
      suffix = Some (next, next_cache) <->
    act_canonical_line_step act_fallback_hash cache position line_start
      line_end suffix next next_cache.
Proof.
  intros. apply act_replay_line_iff_canonical_line_step.
Qed.

Theorem act_gx_replay_iff_canonical_step :
  forall cache position line_start line_end suffix next next_cache,
    act_replay_line act_gx_hash cache position line_start line_end
      suffix = Some (next, next_cache) <->
    act_canonical_line_step act_gx_hash cache position line_start
      line_end suffix next next_cache.
Proof.
  intros. apply act_replay_line_iff_canonical_line_step.
Qed.

Theorem act_arch_replay_iff_canonical_step :
  forall fallback cache position line_start line_end suffix next next_cache,
    act_replay_line (act_arch_hash fallback) cache position line_start
      line_end suffix = Some (next, next_cache) <->
    act_canonical_line_step (act_arch_hash fallback) cache position line_start
      line_end suffix next next_cache.
Proof.
  intros. apply act_replay_line_iff_canonical_line_step.
Qed.

Theorem act_arch_decode_refines_epm1 :
  forall fallback encoded snapshot trie,
    decode_epm1_actree03 (act_arch_hash fallback) encoded =
      Some (snapshot, trie) ->
    EPM1.decode_epm1 encoded = Some snapshot.
Proof.
  intros. eapply decode_epm1_actree03_refines_epm1; eauto.
Qed.

Theorem act_arch_decoded_map_has_exact_zero_markers :
  forall fallback encoded snapshot trie,
    decode_epm1_actree03 (act_arch_hash fallback) encoded =
      Some (snapshot, Some trie) ->
    EPM1.snapshot_mode snapshot = MapMode ->
    Forall (fun marker => marker = 0%N) (act_value_markers trie) /\
    length (act_value_markers trie) =
      length (EPM1.value_bodies snapshot).
Proof.
  intros. eapply decoded_map_has_exact_zero_markers; eauto.
Qed.

Print Assumptions act_fallback_low_ignores_high.
Print Assumptions act_fallback_replay_iff_canonical_step.
Print Assumptions act_gx_replay_iff_canonical_step.
Print Assumptions act_arch_replay_iff_canonical_step.
Print Assumptions act_arch_decode_refines_epm1.
Print Assumptions act_arch_decoded_map_has_exact_zero_markers.
