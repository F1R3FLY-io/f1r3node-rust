(* ═══════════════════════════════════════════════════════════════════════════
   Namespace.v — the 0xE0 namespace of the Reified RSpaces ids (P4)

   This module models the namespace of the native spaces (plan decision 7,
   P4 design B.1). Every Reified name is a GPrivate whose id starts with the
   tag 0xE0. The id layouts are these:

     E0‖op                2 bytes       a system channel, a marker or AGENT_TAG
     E0‖C‖sid             34 bytes      the config channel of the space sid
     E0‖S‖sid             34 bytes      the space counter
     E0‖S‖sid‖key         >34 bytes     the counter of the collection key
     E0‖D‖sid‖key         >34 bytes     the data channel of the collection key
     E0‖I‖sid‖prefix      >=34 bytes    a path index (P5)
     E0‖A‖sid             34 bytes      the allocator (P5)
     E0‖H‖sid             34 bytes      the handle this of the space
     E0‖N‖sid‖tail        66 or 42      a name of the space
     E0‖X‖state           >=34 bytes    a matcher (P8)
     E0‖B‖tag             34 bytes      a body (P8)
     E0‖M‖..., E0‖T‖...   >=34 bytes    reserved

   The ephemeral form of a KIND is its lowercase letter, kind | 0x20 (I-4).
   M and T have no ephemeral form. A collection key is the bincode of the
   sorted inner channel (A5), canonical_inner_channel_bytes.

   Sources
   - Design: the P4 design report B.1 (constants, constructors, the
     decoder, the collection key) and E.1 (the headlines of this module).
   - Plan: decisions 7 (tag 0xE0 and the KIND table), 18 (the syntax
     markers 0xD0 to 0xD4), 32 (A5: the key is the bincode of the sorted
     Par), I-3 (self-describing names) and I-4 (ephemeral KINDs).
   - Code: the P4 commit of this repository. The line numbers below are the
     line numbers of that commit.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   reified_tag              models/src/rust/reified/encoding.rs:29
                            (REIFIED_TAG = 0xE0)
   sid_len, ephemeral_bit   spaces/namespace.rs:35 (SID_LEN), 50
                            (EPHEMERAL_BIT)
   kind, kind_letter        spaces/namespace.rs:82-96 (enum Kind, the
                            discriminant is the uppercase letter)
   has_ephemeral_form       spaces/namespace.rs:115
   kind_byte                spaces/namespace.rs:119-125 (Kind::byte)
   letter_kind, kind_of_byte
                            spaces/namespace.rs:128-148 (Kind::from_byte:
                            byte & !EPHEMERAL_BIT, then the bit 0x20)
   durability, native_space spaces/namespace.rs:154-157, 161-164
   name_tail, tail_bytes    spaces/namespace.rs:168-171 (NameTail)
   scoped_kind_byte         spaces/namespace.rs:216-221 (fn kind_byte)
   space_scoped_id          spaces/namespace.rs:223-230
   config_channel, space_counter, collection_counter, data_channel,
   handle_channel, name_id  spaces/namespace.rs:233, 236, 239-241, 244-246,
                            249 and 270-275 (the constructors)
   op_channel               spaces/namespace.rs:278
   op ids                   spaces/namespace.rs:53-77 (FACTORY_LAST to
                            AGENT_TAG), models/src/rust/reified/encoding.rs:
                            34-45 (Marker 0xD0 to 0xD4)
   reified_id               spaces/namespace.rs:282-297 (ReifiedId)
   decode_name_tail         spaces/namespace.rs:299-307 (fn name_tail)
   decode_reified_id        spaces/namespace.rs:312-342
   canon, key_of            spaces/namespace.rs:200-206, 180-191
                            (canonical_inner_channel_bytes, then
                            CollectionKey::of_channel and from_bytes)
   byte names               rholang/src/rust/interpreter/system_processes.rs:
                            89-93 (byte_name: a 1-byte id)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A byte list models a GPrivate id. A byte of the code is a u8, so the
     decoder theorems take the premise that every byte is below 256.
   - The model represents the index tail of an Array or Vector name by its
     eight big-endian bytes. u64::to_be_bytes and u64::from_be_bytes are
     mutually inverse bijections between u64 and [u8; 8] (Rust core), so
     this representation loses nothing, and it keeps the u64 range out of
     the unary arithmetic of nat.
   - The decoder returns a native_space whose sid is the first 32 bytes of
     the payload, as the code does with split_at_checked(SID_LEN).
   - canon_respects_par_eq abstracts the Par type: a Rust Par is a shape and
     its locally_free bit set. Par equality compares the shapes
     (models/src/lib.rs:210-225 ignores locally_free), serde writes
     locally_free as empty bytes (models/build.rs), and bincode is
     injective on shapes. The section premise ser_injective is discharged by
     the instance canon_respects_par_eq_instance.
   - A program mints a new name with rand.next(), which returns 32 bytes
     (crypto/src/rust/hash/blake2b512_random.rs:169-178). A byte name has
     1 byte. A MeTTaIL id has 34 bytes and a first byte in 0xF0-0xF9 (the
     MeTTaIL band of plan decision 7).

   Headline theorems (each one has a Print Assumptions line at the end)
    1. kind_byte_injective          one KIND byte for each kind and
                                    durability, and the decoder inverts it
    2. storage_id_injective         the data and counter ids are injective
                                    and pairwise disjoint
    3. handle_id_injective          the handle id is injective and disjoint
                                    from every other constructor
    4. ephemeral_durable_disjoint   an ephemeral id never equals a durable id
    5. reified_ids_disjoint_from_new_ids  a Reified id never has 32 bytes
    6. reified_ids_disjoint_from_byte_names_and_mettail
    7. op_ids_pairwise_distinct     the op ids are pairwise distinct
    8. decode_encode_reified_id     decode inverts encode, and decode is sound
    9. space_and_collection_counters_distinct
   10. name_id_routes_to_sid        a name id decodes to its space
   11. canon_respects_par_eq        the key depends on the sorted shape only

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. The file
   imports only the Rocq standard library.

   Failed strategies (kept so that they are not tried again)
   - A u64 index as a nat with a byte-wise big-endian codec makes Rocq
     compute 256^7 in unary arithmetic. The model keeps the 8 bytes.
   - A direct proof of kind_of_byte_inv by bit lemmas (Nat.land, Nat.lor)
     is long. The proof checks all 256 bytes with one boolean scan instead.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
Import ListNotations.

(* ── 1. Bytes ──────────────────────────────────────────────────────────── *)

Definition bytes : Type := list nat.

Definition bytes_ok (b : bytes) : Prop := Forall (fun x => x < 256) b.

Definition reified_tag : nat := 224.
Definition sid_len : nat := 32.
Definition ephemeral_bit : nat := 32.

Lemma firstn_app_length :
  forall (l1 l2 : bytes), firstn (length l1) (l1 ++ l2) = l1.
Proof.
  induction l1 as [| x l1 IH]; intros l2; simpl; [reflexivity |].
  rewrite IH. reflexivity.
Qed.

Lemma skipn_app_length :
  forall (l1 l2 : bytes), skipn (length l1) (l1 ++ l2) = l2.
Proof. induction l1 as [| x l1 IH]; intros l2; simpl; [reflexivity | apply IH]. Qed.

Lemma app_inj_length :
  forall (a b c d : bytes), length a = length c -> a ++ b = c ++ d -> a = c /\ b = d.
Proof.
  induction a as [| x a IH]; intros b [| y c] d Hlen Heq; simpl in *; try discriminate.
  - split; [reflexivity | exact Heq].
  - injection Heq as -> Heq'. injection Hlen as Hlen.
    destruct (IH b c d Hlen Heq') as [-> ->]. split; reflexivity.
Qed.

(* ── 2. KINDs and durability ───────────────────────────────────────────── *)

Inductive kind : Type :=
| KData | KConfig | KPathIndex | KAllocator | KSequence | KHandle
| KName | KMatcher | KBody | KEngine | KTheory.

Definition all_kinds : list kind :=
  [KData; KConfig; KPathIndex; KAllocator; KSequence; KHandle; KName; KMatcher; KBody;
   KEngine; KTheory].

(* The discriminant of enum Kind is its uppercase ASCII letter. *)
Definition kind_letter (k : kind) : nat :=
  match k with
  | KData => 68 | KConfig => 67 | KPathIndex => 73 | KAllocator => 65 | KSequence => 83
  | KHandle => 72 | KName => 78 | KMatcher => 88 | KBody => 66 | KEngine => 77
  | KTheory => 84
  end.

Inductive durability : Type := Durable | Ephemeral.

Definition durability_eqb (a b : durability) : bool :=
  match a, b with
  | Durable, Durable | Ephemeral, Ephemeral => true
  | _, _ => false
  end.

Definition has_ephemeral_form (k : kind) : bool :=
  match k with KEngine | KTheory => false | _ => true end.

(* Kind::byte: the uppercase letter, or the letter | 0x20 for an ephemeral
   kind that has an ephemeral form. *)
Definition kind_byte (k : kind) (d : durability) : option nat :=
  match d with
  | Durable => Some (kind_letter k)
  | Ephemeral => if has_ephemeral_form k then Some (Nat.lor (kind_letter k) ephemeral_bit) else None
  end.

(* fn kind_byte of the constructors. No constructor makes an ephemeral M or
   T id. *)
Definition scoped_kind_byte (k : kind) (d : durability) : nat :=
  match d with
  | Durable => kind_letter k
  | Ephemeral => Nat.lor (kind_letter k) ephemeral_bit
  end.

Definition letter_kind (b : nat) : option kind :=
  if b =? 68 then Some KData
  else if b =? 67 then Some KConfig
  else if b =? 73 then Some KPathIndex
  else if b =? 65 then Some KAllocator
  else if b =? 83 then Some KSequence
  else if b =? 72 then Some KHandle
  else if b =? 78 then Some KName
  else if b =? 88 then Some KMatcher
  else if b =? 66 then Some KBody
  else if b =? 77 then Some KEngine
  else if b =? 84 then Some KTheory
  else None.

(* Kind::from_byte. For a u8, byte & !EPHEMERAL_BIT is Nat.land b 223. *)
Definition kind_of_byte (b : nat) : option (kind * durability) :=
  match letter_kind (Nat.land b 223) with
  | None => None
  | Some k =>
      if Nat.land b ephemeral_bit =? 0 then Some (k, Durable)
      else if has_ephemeral_form k then Some (k, Ephemeral) else None
  end.

Definition kind_eqb (a b : kind) : bool := kind_letter a =? kind_letter b.

Lemma kind_letter_injective : forall a b, kind_letter a = kind_letter b -> a = b.
Proof. intros [] [] H; simpl in H; try discriminate; reflexivity. Qed.

Lemma kind_eqb_iff : forall a b, kind_eqb a b = true <-> a = b.
Proof.
  intros a b. unfold kind_eqb. rewrite Nat.eqb_eq. split.
  - apply kind_letter_injective.
  - intros ->. reflexivity.
Qed.

Lemma scoped_kind_byte_spec :
  forall k d, d = Durable \/ has_ephemeral_form k = true ->
    kind_byte k d = Some (scoped_kind_byte k d).
Proof. intros [] [] [H | H]; try discriminate; reflexivity. Qed.

Lemma kind_of_byte_scoped :
  forall k d, d = Durable \/ has_ephemeral_form k = true ->
    kind_of_byte (scoped_kind_byte k d) = Some (k, d).
Proof. intros [] [] [H | H]; try discriminate; reflexivity. Qed.

Lemma scoped_kind_byte_range : forall k d, 65 <= scoped_kind_byte k d < 256.
Proof. intros [] []; cbv; lia. Qed.

(* The boolean scan behind kind_of_byte_inv: every byte below 256 that
   decodes is the byte of its kind and durability. *)
Definition byte_scan (b : nat) : bool :=
  match kind_of_byte b with
  | Some (k, d) =>
      (b =? scoped_kind_byte k d)
      && (durability_eqb d Durable || has_ephemeral_form k)
  | None => true
  end.

Lemma byte_scan_all : forallb byte_scan (seq 0 256) = true.
Proof. vm_compute. reflexivity. Qed.

Lemma kind_of_byte_inv :
  forall b k d, b < 256 -> kind_of_byte b = Some (k, d) ->
    b = scoped_kind_byte k d /\ (d = Durable \/ has_ephemeral_form k = true).
Proof.
  intros b k d Hb Hdec.
  pose proof byte_scan_all as Hall. rewrite forallb_forall in Hall.
  assert (Hin : In b (seq 0 256)) by (apply in_seq; lia).
  specialize (Hall b Hin). unfold byte_scan in Hall. rewrite Hdec in Hall.
  apply andb_true_iff in Hall as [Heq Hd]. apply Nat.eqb_eq in Heq.
  split; [exact Heq |].
  destruct d; [left; reflexivity | right].
  simpl in Hd. exact Hd.
Qed.

(* ── 3. Spaces, tails and the constructors ─────────────────────────────── *)

Record native_space : Type := mkSpace { sp_sid : bytes; sp_dur : durability }.

Definition space_ok (s : native_space) : Prop := length (sp_sid s) = sid_len /\ bytes_ok (sp_sid s).

(* NameTail: 32 random bytes, or the 8 big-endian bytes of a u64 index. *)
Inductive name_tail : Type :=
| TailRandom : bytes -> name_tail
| TailIndex : bytes -> name_tail.

Definition tail_bytes (t : name_tail) : bytes :=
  match t with TailRandom b => b | TailIndex b => b end.

Definition tail_ok (t : name_tail) : Prop :=
  match t with
  | TailRandom b => length b = 32 /\ bytes_ok b
  | TailIndex b => length b = 8 /\ bytes_ok b
  end.

Definition space_scoped_id (k : kind) (s : native_space) (tail : bytes) : bytes :=
  reified_tag :: scoped_kind_byte k (sp_dur s) :: sp_sid s ++ tail.

Definition config_channel (s : native_space) : bytes := space_scoped_id KConfig s [].
Definition space_counter (s : native_space) : bytes := space_scoped_id KSequence s [].
Definition collection_counter (s : native_space) (key : bytes) : bytes :=
  space_scoped_id KSequence s key.
Definition data_channel (s : native_space) (key : bytes) : bytes := space_scoped_id KData s key.
Definition handle_channel (s : native_space) : bytes := space_scoped_id KHandle s [].
Definition name_id (s : native_space) (t : name_tail) : bytes :=
  space_scoped_id KName s (tail_bytes t).
Definition op_channel (op : nat) : bytes := [reified_tag; op].

(* ── 4. The decoder ────────────────────────────────────────────────────── *)

Inductive reified_id : Type :=
| ROp : nat -> reified_id
| RConfig : native_space -> reified_id
| RSpaceCounter : native_space -> reified_id
| RCollectionCounter : native_space -> bytes -> reified_id
| RData : native_space -> bytes -> reified_id
| RPathIndex : native_space -> bytes -> reified_id
| RAllocator : native_space -> reified_id
| RHandle : native_space -> reified_id
| RName : native_space -> name_tail -> reified_id
| RMatcher : durability -> bytes -> reified_id
| RBody : durability -> bytes -> reified_id
| RReserved : kind -> bytes -> reified_id.

Definition decode_name_tail (tail : bytes) : option name_tail :=
  if length tail =? 32 then Some (TailRandom tail)
  else if length tail =? 8 then Some (TailIndex tail)
  else None.

(* The step of decode_reified_id after the KIND byte and the length check:
   the sid is the first 32 bytes of the payload, and the tail is the rest. *)
Definition decode_payload (k : kind) (d : durability) (payload : bytes) : option reified_id :=
  match k, skipn sid_len payload with
  | KConfig, [] => Some (RConfig (mkSpace (firstn sid_len payload) d))
  | KHandle, [] => Some (RHandle (mkSpace (firstn sid_len payload) d))
  | KAllocator, [] => Some (RAllocator (mkSpace (firstn sid_len payload) d))
  | KBody, [] => Some (RBody d (firstn sid_len payload))
  | KSequence, [] => Some (RSpaceCounter (mkSpace (firstn sid_len payload) d))
  | KSequence, _ :: _ =>
      Some (RCollectionCounter (mkSpace (firstn sid_len payload) d) (skipn sid_len payload))
  | KData, _ :: _ => Some (RData (mkSpace (firstn sid_len payload) d) (skipn sid_len payload))
  | KPathIndex, _ =>
      Some (RPathIndex (mkSpace (firstn sid_len payload) d) (skipn sid_len payload))
  | KName, _ =>
      match decode_name_tail (skipn sid_len payload) with
      | Some t => Some (RName (mkSpace (firstn sid_len payload) d) t)
      | None => None
      end
  | KMatcher, _ => Some (RMatcher d payload)
  | KEngine, _ | KTheory, _ => Some (RReserved k payload)
  | KConfig, _ :: _ | KHandle, _ :: _ | KAllocator, _ :: _ | KBody, _ :: _ | KData, [] => None
  end.

(* decode_reified_id: total, None for every id that no constructor makes. *)
Definition decode_reified_id (id : bytes) : option reified_id :=
  match id with
  | [] => None
  | tag :: rest =>
      if negb (tag =? reified_tag) then None else
      match rest with
      | [] => None
      | kb :: payload =>
          match payload with
          | [] => Some (ROp kb)
          | _ :: _ =>
              match kind_of_byte kb with
              | None => None
              | Some (k, d) =>
                  if length payload <? sid_len then None else decode_payload k d payload
              end
          end
      end
  end.

Definition encode_reified_id (x : reified_id) : bytes :=
  match x with
  | ROp b => op_channel b
  | RConfig s => config_channel s
  | RSpaceCounter s => space_counter s
  | RCollectionCounter s key => collection_counter s key
  | RData s key => data_channel s key
  | RPathIndex s tail => space_scoped_id KPathIndex s tail
  | RAllocator s => space_scoped_id KAllocator s []
  | RHandle s => handle_channel s
  | RName s t => name_id s t
  | RMatcher d payload => reified_tag :: scoped_kind_byte KMatcher d :: payload
  | RBody d payload => reified_tag :: scoped_kind_byte KBody d :: payload
  | RReserved k payload => reified_tag :: kind_letter k :: payload
  end.

(* The ids that the constructors and the decoder agree on. *)
Definition reified_id_ok (x : reified_id) : Prop :=
  match x with
  | ROp b => b < 256
  | RConfig s | RSpaceCounter s | RAllocator s | RHandle s => space_ok s
  | RCollectionCounter s key | RData s key => space_ok s /\ key <> [] /\ bytes_ok key
  | RPathIndex s tail => space_ok s /\ bytes_ok tail
  | RName s t => space_ok s /\ tail_ok t
  | RMatcher _ payload => sid_len <= length payload /\ bytes_ok payload
  | RBody _ payload => length payload = sid_len /\ bytes_ok payload
  | RReserved k payload =>
      has_ephemeral_form k = false /\ sid_len <= length payload /\ bytes_ok payload
  end.

(* The durability of an id, or None for a 2-byte op id. *)
Definition id_durability (x : reified_id) : option durability :=
  match x with
  | ROp _ => None
  | RConfig s | RSpaceCounter s | RAllocator s | RHandle s
  | RCollectionCounter s _ | RData s _ | RPathIndex s _ | RName s _ => Some (sp_dur s)
  | RMatcher d _ | RBody d _ => Some d
  | RReserved _ _ => Some Durable
  end.

Lemma decode_tag_cons :
  forall kb payload, payload <> [] ->
    decode_reified_id (reified_tag :: kb :: payload) =
    match kind_of_byte kb with
    | None => None
    | Some (k, d) => if length payload <? sid_len then None else decode_payload k d payload
    end.
Proof. intros kb [| y p] H; [contradiction | reflexivity]. Qed.

Lemma decode_tag_scoped :
  forall k d payload,
    payload <> [] -> (d = Durable \/ has_ephemeral_form k = true) -> sid_len <= length payload ->
    decode_reified_id (reified_tag :: scoped_kind_byte k d :: payload) = decode_payload k d payload.
Proof.
  intros k d payload Hne Hform Hlen.
  rewrite (decode_tag_cons _ _ Hne), (kind_of_byte_scoped k d Hform).
  replace (length payload <? sid_len) with false by (symmetry; apply Nat.ltb_ge; exact Hlen).
  reflexivity.
Qed.

Lemma decode_payload_split :
  forall k d sid tail, length sid = sid_len ->
    decode_payload k d (sid ++ tail) =
    match k, tail with
    | KConfig, [] => Some (RConfig (mkSpace sid d))
    | KHandle, [] => Some (RHandle (mkSpace sid d))
    | KAllocator, [] => Some (RAllocator (mkSpace sid d))
    | KBody, [] => Some (RBody d sid)
    | KSequence, [] => Some (RSpaceCounter (mkSpace sid d))
    | KSequence, _ :: _ => Some (RCollectionCounter (mkSpace sid d) tail)
    | KData, _ :: _ => Some (RData (mkSpace sid d) tail)
    | KPathIndex, _ => Some (RPathIndex (mkSpace sid d) tail)
    | KName, _ =>
        match decode_name_tail tail with Some t => Some (RName (mkSpace sid d) t) | None => None end
    | KMatcher, _ => Some (RMatcher d (sid ++ tail))
    | KEngine, _ | KTheory, _ => Some (RReserved k (sid ++ tail))
    | KConfig, _ :: _ | KHandle, _ :: _ | KAllocator, _ :: _ | KBody, _ :: _ | KData, [] => None
    end.
Proof.
  intros k d sid tail Hlen. unfold decode_payload.
  rewrite <- Hlen, firstn_app_length, skipn_app_length. reflexivity.
Qed.

Lemma space_payload_nonempty :
  forall (sid tail : bytes), length sid = sid_len -> sid ++ tail <> [].
Proof. intros [| x sid] tail H; [unfold sid_len in H; discriminate | discriminate]. Qed.

Lemma decode_space_scoped :
  forall k s tail,
    space_ok s -> (sp_dur s = Durable \/ has_ephemeral_form k = true) ->
    decode_reified_id (space_scoped_id k s tail) =
    decode_payload k (sp_dur s) (sp_sid s ++ tail).
Proof.
  intros k [sid d] tail [Hlen _] Hform. unfold space_scoped_id. cbn [sp_sid sp_dur] in *.
  apply decode_tag_scoped; [apply space_payload_nonempty; exact Hlen | exact Hform |].
  rewrite length_app. lia.
Qed.

(* ── 5. Headline 8: the decoder inverts the constructors and is sound ──── *)

Lemma decode_name_tail_ok : forall t, tail_ok t -> decode_name_tail (tail_bytes t) = Some t.
Proof.
  intros [b | b] [Hlen _]; unfold decode_name_tail, tail_bytes; rewrite Hlen; reflexivity.
Qed.

Lemma decode_encode_ok : forall x, reified_id_ok x -> decode_reified_id (encode_reified_id x) = Some x.
Proof.
  intros [b | s | s | s key | s key | s tail | s | s | s t | d p | d p | k p] Hok.
  - reflexivity.
  - unfold encode_reified_id, config_channel.
    rewrite (decode_space_scoped KConfig s [] Hok (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hok)). destruct s. reflexivity.
  - unfold encode_reified_id, space_counter.
    rewrite (decode_space_scoped KSequence s [] Hok (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hok)). destruct s. reflexivity.
  - destruct Hok as [Hs [Hne _]]. unfold encode_reified_id, collection_counter.
    rewrite (decode_space_scoped KSequence s key Hs (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hs)).
    destruct key; [contradiction |]. destruct s. reflexivity.
  - destruct Hok as [Hs [Hne _]]. unfold encode_reified_id, data_channel.
    rewrite (decode_space_scoped KData s key Hs (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hs)).
    destruct key; [contradiction |]. destruct s. reflexivity.
  - destruct Hok as [Hs _]. unfold encode_reified_id.
    rewrite (decode_space_scoped KPathIndex s tail Hs (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hs)).
    destruct s. destruct tail; reflexivity.
  - unfold encode_reified_id.
    rewrite (decode_space_scoped KAllocator s [] Hok (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hok)). destruct s. reflexivity.
  - unfold encode_reified_id, handle_channel.
    rewrite (decode_space_scoped KHandle s [] Hok (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hok)). destruct s. reflexivity.
  - destruct Hok as [Hs Ht]. unfold encode_reified_id, name_id.
    rewrite (decode_space_scoped KName s (tail_bytes t) Hs (or_intror eq_refl)).
    rewrite (decode_payload_split _ _ _ _ (proj1 Hs)).
    rewrite (decode_name_tail_ok t Ht). destruct s. destruct (tail_bytes t); reflexivity.
  - destruct Hok as [Hlen _]. unfold encode_reified_id.
    rewrite (decode_tag_scoped KMatcher d p) by
      (try (destruct p; [simpl in Hlen; unfold sid_len in Hlen; lia | discriminate]);
       try (right; reflexivity); exact Hlen).
    unfold decode_payload. destruct (skipn sid_len p); reflexivity.
  - destruct Hok as [Hlen _]. unfold encode_reified_id.
    rewrite (decode_tag_scoped KBody d p) by
      (try (destruct p; [simpl in Hlen; unfold sid_len in Hlen; lia | discriminate]);
       try (right; reflexivity); lia).
    replace (decode_payload KBody d p) with (decode_payload KBody d (p ++ []))
      by (rewrite app_nil_r; reflexivity).
    rewrite (decode_payload_split _ _ _ _ Hlen). reflexivity.
  - destruct Hok as [Hk [Hlen _]]. unfold encode_reified_id.
    change (kind_letter k) with (scoped_kind_byte k Durable).
    rewrite (decode_tag_scoped k Durable p) by
      (try (destruct p; [simpl in Hlen; unfold sid_len in Hlen; lia | discriminate]);
       try (left; reflexivity); exact Hlen).
    unfold decode_payload.
    destruct k; try discriminate; destruct (skipn sid_len p); reflexivity.
Qed.

Lemma bytes_ok_app : forall a b, bytes_ok (a ++ b) <-> bytes_ok a /\ bytes_ok b.
Proof. intros a b. unfold bytes_ok. apply Forall_app. Qed.

Lemma bytes_ok_firstn : forall n b, bytes_ok b -> bytes_ok (firstn n b).
Proof.
  intros n b H. unfold bytes_ok in *. rewrite <- (firstn_skipn n b) in H.
  apply Forall_app in H as [H _]. exact H.
Qed.

Lemma bytes_ok_skipn : forall n b, bytes_ok b -> bytes_ok (skipn n b).
Proof.
  intros n b H. unfold bytes_ok in *. rewrite <- (firstn_skipn n b) in H.
  apply Forall_app in H as [_ H]. exact H.
Qed.

Lemma decode_payload_sound :
  forall k d payload x,
    bytes_ok payload -> sid_len <= length payload ->
    (d = Durable \/ has_ephemeral_form k = true) ->
    decode_payload k d payload = Some x ->
    reified_tag :: scoped_kind_byte k d :: payload = encode_reified_id x /\ reified_id_ok x.
Proof.
  intros k d payload x Hbytes Hlen Hform Hdec.
  remember (firstn sid_len payload) as sid eqn:Esid.
  remember (skipn sid_len payload) as tail eqn:Etail.
  assert (Hsplit : payload = sid ++ tail) by (subst; symmetry; apply firstn_skipn).
  assert (Hsidlen : length sid = sid_len) by (subst sid; rewrite length_firstn; lia).
  assert (Hsidok : bytes_ok sid) by (subst sid; apply bytes_ok_firstn; exact Hbytes).
  assert (Htailok : bytes_ok tail) by (subst tail; apply bytes_ok_skipn; exact Hbytes).
  assert (Hsok : forall d', space_ok (mkSpace sid d')) by (intros d'; split; assumption).
  clear Esid Etail. subst payload.
  rewrite (decode_payload_split k d sid tail Hsidlen) in Hdec.
  assert (Hspace : forall d', reified_id_ok (RConfig (mkSpace sid d'))) by exact Hsok.
  assert (Hlong : sid_len <= length (sid ++ tail) /\ bytes_ok (sid ++ tail))
    by (split; [rewrite length_app; lia | apply bytes_ok_app; split; assumption]).
  destruct k; destruct tail as [| z tail']; try discriminate.
  - (* KData, a key *)
    injection Hdec as <-. split; [reflexivity |].
    split; [exact (Hsok d) | split; [discriminate | exact Htailok]].
  - (* KConfig *)
    injection Hdec as <-. split; [reflexivity | exact (Hsok d)].
  - (* KPathIndex, the empty prefix *)
    injection Hdec as <-. split; [reflexivity | split; [exact (Hsok d) | exact Htailok]].
  - (* KPathIndex, a prefix *)
    injection Hdec as <-. split; [reflexivity | split; [exact (Hsok d) | exact Htailok]].
  - (* KAllocator *)
    injection Hdec as <-. split; [reflexivity | exact (Hsok d)].
  - (* KSequence, the space counter *)
    injection Hdec as <-. split; [reflexivity | exact (Hsok d)].
  - (* KSequence, a collection counter *)
    injection Hdec as <-. split; [reflexivity |].
    split; [exact (Hsok d) | split; [discriminate | exact Htailok]].
  - (* KHandle *)
    injection Hdec as <-. split; [reflexivity | exact (Hsok d)].
  - (* KName with a tail *)
    destruct (decode_name_tail (z :: tail')) as [t |] eqn:Et; [| discriminate].
    injection Hdec as <-. unfold decode_name_tail in Et.
    destruct (length (z :: tail') =? 32) eqn:E32.
    + injection Et as <-. apply Nat.eqb_eq in E32.
      split; [reflexivity | split; [exact (Hsok d) | split; [exact E32 | exact Htailok]]].
    + destruct (length (z :: tail') =? 8) eqn:E8; [| discriminate].
      injection Et as <-. apply Nat.eqb_eq in E8.
      split; [reflexivity | split; [exact (Hsok d) | split; [exact E8 | exact Htailok]]].
  - (* KMatcher, a 32-byte state *)
    injection Hdec as <-. split; [reflexivity | exact Hlong].
  - (* KMatcher, a longer state *)
    injection Hdec as <-. split; [reflexivity | exact Hlong].
  - (* KBody *)
    injection Hdec as <-. split; [rewrite app_nil_r; reflexivity | split; [exact Hsidlen | exact Hsidok]].
  - (* KEngine *)
    destruct Hform as [-> | Hf]; [| discriminate]. injection Hdec as <-.
    split; [reflexivity | split; [reflexivity | exact Hlong]].
  - destruct Hform as [-> | Hf]; [| discriminate]. injection Hdec as <-.
    split; [reflexivity | split; [reflexivity | exact Hlong]].
  - (* KTheory *)
    destruct Hform as [-> | Hf]; [| discriminate]. injection Hdec as <-.
    split; [reflexivity | split; [reflexivity | exact Hlong]].
  - destruct Hform as [-> | Hf]; [| discriminate]. injection Hdec as <-.
    split; [reflexivity | split; [reflexivity | exact Hlong]].
Qed.

Lemma decode_sound_ok :
  forall id x, bytes_ok id -> decode_reified_id id = Some x ->
    id = encode_reified_id x /\ reified_id_ok x.
Proof.
  intros id x Hbytes Hdec.
  destruct id as [| tag rest]; [discriminate |].
  destruct (tag =? reified_tag) eqn:Etag.
  2: { unfold decode_reified_id in Hdec. rewrite Etag in Hdec. discriminate. }
  apply Nat.eqb_eq in Etag. subst tag.
  destruct rest as [| kb payload]; [discriminate |].
  inversion Hbytes as [| ? ? _ Hrest]; subst.
  inversion Hrest as [| ? ? Hkb Hpayload]; subst.
  destruct payload as [| y payload'].
  - unfold decode_reified_id in Hdec. rewrite Nat.eqb_refl in Hdec. cbn in Hdec.
    injection Hdec as <-. split; [reflexivity | exact Hkb].
  - rewrite (decode_tag_cons kb (y :: payload') ltac:(discriminate)) in Hdec.
    destruct (kind_of_byte kb) as [[k d] |] eqn:Ekb; [| discriminate].
    destruct (kind_of_byte_inv kb k d Hkb Ekb) as [-> Hform].
    destruct (length (y :: payload') <? sid_len) eqn:Elen; [discriminate |].
    apply Nat.ltb_ge in Elen.
    exact (decode_payload_sound k d (y :: payload') x Hpayload Elen Hform Hdec).
Qed.

(* Headline 8. decode_reified_id inverts encode_reified_id on every id that
   the constructors make, and it accepts only those ids. *)
Theorem decode_encode_reified_id :
  (forall x, reified_id_ok x -> decode_reified_id (encode_reified_id x) = Some x) /\
  (forall id x, bytes_ok id -> decode_reified_id id = Some x ->
     id = encode_reified_id x /\ reified_id_ok x).
Proof. split; [exact decode_encode_ok | exact decode_sound_ok]. Qed.

Lemma encode_injective_ok :
  forall x y, reified_id_ok x -> reified_id_ok y ->
    encode_reified_id x = encode_reified_id y -> x = y.
Proof.
  intros x y Hx Hy Heq.
  pose proof (decode_encode_ok x Hx) as H1. pose proof (decode_encode_ok y Hy) as H2.
  rewrite Heq, H2 in H1. injection H1 as ->. reflexivity.
Qed.

(* ── 6. Headline 1: the KIND byte ──────────────────────────────────────── *)

(* Headline 1. Each kind and durability has at most one KIND byte, and no
   two kinds or durabilities share one. The ephemeral form exists exactly
   for the nine kinds other than M and T, and Kind::from_byte inverts
   Kind::byte. *)
Lemma kind_byte_some :
  forall k d b, kind_byte k d = Some b ->
    b = scoped_kind_byte k d /\ (d = Durable \/ has_ephemeral_form k = true).
Proof.
  intros k d b H. destruct d.
  - injection H as <-. split; [reflexivity | left; reflexivity].
  - unfold kind_byte in H. destruct (has_ephemeral_form k) eqn:E; [| discriminate].
    injection H as <-. split; [reflexivity | right; reflexivity].
Qed.

Theorem kind_byte_injective :
  (forall k1 d1 k2 d2 b,
      kind_byte k1 d1 = Some b -> kind_byte k2 d2 = Some b -> k1 = k2 /\ d1 = d2) /\
  (forall k d b, kind_byte k d = Some b -> kind_of_byte b = Some (k, d)) /\
  (forall k, kind_byte k Ephemeral = None <-> (k = KEngine \/ k = KTheory)) /\
  (forall b k d, b < 256 -> kind_of_byte b = Some (k, d) -> kind_byte k d = Some b).
Proof.
  split; [| split; [| split]].
  - intros k1 d1 k2 d2 b H1 H2.
    destruct (kind_byte_some _ _ _ H1) as [E1 F1]. destruct (kind_byte_some _ _ _ H2) as [E2 F2].
    pose proof (kind_of_byte_scoped k1 d1 F1) as K1. pose proof (kind_of_byte_scoped k2 d2 F2) as K2.
    rewrite <- E1 in K1. rewrite <- E2 in K2. rewrite K1 in K2. injection K2 as -> ->.
    split; reflexivity.
  - intros k d b H. destruct (kind_byte_some _ _ _ H) as [-> F]. apply kind_of_byte_scoped. exact F.
  - intros k. destruct k; cbv [kind_byte has_ephemeral_form]; split; intro H;
      try discriminate; try (destruct H as [H | H]; discriminate);
      try (left; reflexivity); try (right; reflexivity); try reflexivity.
  - intros b k d Hb Hdec. destruct (kind_of_byte_inv b k d Hb Hdec) as [-> Hform].
    apply scoped_kind_byte_spec. exact Hform.
Qed.

(* ── 7. Headlines 2, 3 and 9: the constructors ─────────────────────────── *)

Lemma space_scoped_id_injective :
  forall k1 s1 t1 k2 s2 t2,
    space_ok s1 -> space_ok s2 ->
    space_scoped_id k1 s1 t1 = space_scoped_id k2 s2 t2 ->
    scoped_kind_byte k1 (sp_dur s1) = scoped_kind_byte k2 (sp_dur s2) /\
    sp_sid s1 = sp_sid s2 /\ t1 = t2.
Proof.
  intros k1 [sid1 d1] t1 k2 [sid2 d2] t2 [H1 _] [H2 _] Heq.
  unfold space_scoped_id in Heq. simpl in *. injection Heq as Hk Hrest.
  assert (Hl : length sid1 = length sid2) by lia.
  destruct (app_inj_length sid1 t1 sid2 t2 Hl Hrest) as [-> ->].
  split; [exact Hk | split; reflexivity].
Qed.

Lemma scoped_kind_byte_injective :
  forall k1 d1 k2 d2, scoped_kind_byte k1 d1 = scoped_kind_byte k2 d2 -> k1 = k2 /\ d1 = d2.
Proof.
  intros k1 d1 k2 d2 H.
  destruct k1, d1, k2, d2; vm_compute in H; try discriminate; split; reflexivity.
Qed.

Lemma space_eq : forall s1 s2, sp_sid s1 = sp_sid s2 -> sp_dur s1 = sp_dur s2 -> s1 = s2.
Proof. intros [a b] [c d] H1 H2. simpl in *. subst. reflexivity. Qed.

Lemma space_scoped_id_inj_full :
  forall k1 s1 t1 k2 s2 t2,
    space_ok s1 -> space_ok s2 ->
    space_scoped_id k1 s1 t1 = space_scoped_id k2 s2 t2 ->
    k1 = k2 /\ s1 = s2 /\ t1 = t2.
Proof.
  intros k1 s1 t1 k2 s2 t2 H1 H2 Heq.
  destruct (space_scoped_id_injective k1 s1 t1 k2 s2 t2 H1 H2 Heq) as [Hk [Hs Ht]].
  destruct (scoped_kind_byte_injective _ _ _ _ Hk) as [Hkk Hd].
  split; [exact Hkk | split; [apply space_eq; assumption | exact Ht]].
Qed.

(* Headline 2. The storage ids D(sid,key), S(sid,key) and S(sid) are
   injective in the space and the key, and they are pairwise disjoint: a
   data channel is never a counter, and the space counter is never a
   collection counter. *)
Theorem storage_id_injective :
  forall s1 s2 k1 k2,
    space_ok s1 -> space_ok s2 ->
    (data_channel s1 k1 = data_channel s2 k2 -> s1 = s2 /\ k1 = k2) /\
    (collection_counter s1 k1 = collection_counter s2 k2 -> s1 = s2 /\ k1 = k2) /\
    (space_counter s1 = space_counter s2 -> s1 = s2) /\
    data_channel s1 k1 <> collection_counter s2 k2 /\
    data_channel s1 k1 <> space_counter s2 /\
    (k2 <> [] -> space_counter s1 <> collection_counter s2 k2).
Proof.
  intros s1 s2 k1 k2 H1 H2. split; [| split; [| split; [| split; [| split]]]].
  - intros H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [_ [-> ->]].
    split; reflexivity.
  - intros H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [_ [-> ->]].
    split; reflexivity.
  - intros H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [_ [-> _]].
    reflexivity.
  - intros H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [Hk _]. discriminate.
  - intros H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [Hk _]. discriminate.
  - intros Hne H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [_ [_ Ht]].
    apply Hne. symmetry. exact Ht.
Qed.

(* Headline 3. The handle id E0‖H‖sid is injective in the space and differs
   from every other id that a constructor makes. *)
Theorem handle_id_injective :
  forall s1 s2,
    space_ok s1 -> space_ok s2 ->
    (handle_channel s1 = handle_channel s2 -> s1 = s2) /\
    handle_channel s1 <> config_channel s2 /\
    handle_channel s1 <> space_counter s2 /\
    (forall key, handle_channel s1 <> collection_counter s2 key) /\
    (forall key, handle_channel s1 <> data_channel s2 key) /\
    (forall t, handle_channel s1 <> name_id s2 t) /\
    (forall op, handle_channel s1 <> op_channel op) /\
    decode_reified_id (handle_channel s1) = Some (RHandle s1).
Proof.
  intros s1 s2 H1 H2.
  assert (Hkind : forall k t, k <> KHandle -> handle_channel s1 <> space_scoped_id k s2 t).
  { intros k t Hk H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [Hkk _].
    apply Hk. symmetry. exact Hkk. }
  split; [| split; [| split; [| split; [| split; [| split; [| split]]]]]].
  - intros H. destruct (space_scoped_id_inj_full _ _ _ _ _ _ H1 H2 H) as [_ [-> _]].
    reflexivity.
  - apply Hkind. discriminate.
  - apply Hkind. discriminate.
  - intros key. apply Hkind. discriminate.
  - intros key. apply Hkind. discriminate.
  - intros t. apply Hkind. discriminate.
  - intros op H. unfold handle_channel, space_scoped_id, op_channel in H.
    destruct H1 as [Hlen _]. injection H as _ H.
    destruct (sp_sid s1) as [| x rest]; [simpl in Hlen; discriminate | discriminate].
  - exact (decode_encode_ok (RHandle s1) H1).
Qed.

(* Headline 9. The space counter S(sid) and a collection counter S(sid,key)
   never share an id, and the decoder tells them apart. *)
Theorem space_and_collection_counters_distinct :
  forall s1 s2 key,
    space_ok s1 -> space_ok s2 -> key <> [] -> bytes_ok key ->
    space_counter s1 <> collection_counter s2 key /\
    length (space_counter s1) = 2 + sid_len /\
    2 + sid_len < length (collection_counter s2 key) /\
    decode_reified_id (space_counter s1) = Some (RSpaceCounter s1) /\
    decode_reified_id (collection_counter s2 key) = Some (RCollectionCounter s2 key).
Proof.
  intros s1 s2 key H1 H2 Hne Hkey. split; [| split; [| split; [| split]]].
  - exact (proj2 (proj2 (proj2 (proj2 (proj2 (storage_id_injective s1 s2 key key H1 H2)))))
             Hne).
  - destruct H1 as [Hlen _]. unfold space_counter, space_scoped_id. simpl.
    rewrite length_app, Hlen. simpl. reflexivity.
  - destruct H2 as [Hlen _]. unfold collection_counter, space_scoped_id. simpl.
    rewrite length_app, Hlen.
    destruct key; [contradiction | simpl; unfold sid_len; lia].
  - exact (decode_encode_ok (RSpaceCounter s1) H1).
  - exact (decode_encode_ok (RCollectionCounter s2 key) (conj H2 (conj Hne Hkey))).
Qed.

(* ── 8. Headlines 4, 5 and 6: disjointness ────────────────────────────── *)

Lemma encode_length :
  forall x, reified_id_ok x ->
    length (encode_reified_id x) = 2 \/ 2 + sid_len <= length (encode_reified_id x).
Proof.
  intros x Hok.
  destruct x as [b | s | s | s key | s key | s tail | s | s | s t | d p | d p | k p];
    simpl in Hok; [left; reflexivity | | | | | | | | | | |]; right;
    unfold encode_reified_id, config_channel, space_counter, collection_counter,
      data_channel, handle_channel, name_id, space_scoped_id; simpl; rewrite ?length_app.
  - destruct Hok as [Hlen _]. lia.
  - destruct Hok as [Hlen _]. lia.
  - destruct Hok as [[Hlen _] _]. lia.
  - destruct Hok as [[Hlen _] _]. lia.
  - destruct Hok as [[Hlen _] _]. lia.
  - destruct Hok as [Hlen _]. lia.
  - destruct Hok as [Hlen _]. lia.
  - destruct Hok as [[Hlen _] _]. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in *. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in *. lia.
  - destruct Hok as [_ [Hlen _]]. unfold sid_len in *. lia.
Qed.

Lemma encode_length_spaced :
  forall x, reified_id_ok x -> id_durability x <> None ->
    2 + sid_len <= length (encode_reified_id x).
Proof.
  intros x Hok Hd. destruct (encode_length x Hok) as [H | H]; [| exact H].
  destruct x; simpl in Hd; [contradiction Hd; reflexivity | ..];
    exfalso; simpl in Hok;
    unfold encode_reified_id, config_channel, space_counter, collection_counter,
      data_channel, handle_channel, name_id, space_scoped_id in H; simpl in H;
    rewrite ?length_app in H.
  - destruct Hok as [Hlen _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [[Hlen _] _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [[Hlen _] _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [[Hlen _] _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [[Hlen _] _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [Hlen _]. unfold sid_len in Hlen. lia.
  - destruct Hok as [_ [Hlen _]]. unfold sid_len in Hlen. lia.
Qed.

Lemma encode_head : forall x, exists rest, encode_reified_id x = reified_tag :: rest.
Proof.
  intros [b | s | s | s key | s key | s tail | s | s | s t | d p | d p | k p];
    eexists; reflexivity.
Qed.

(* Headline 4. An id of an ephemeral space never equals an id of a durable
   space. The KIND byte, the second byte of every id with a space, carries
   the durability: the bit 0x20 is clear for a durable id and set for an
   ephemeral id. *)
Theorem ephemeral_durable_disjoint :
  (forall x y,
      reified_id_ok x -> reified_id_ok y ->
      id_durability x = Some Ephemeral -> id_durability y = Some Durable ->
      encode_reified_id x <> encode_reified_id y) /\
  (forall x d, id_durability x = Some d ->
     exists k, nth_error (encode_reified_id x) 1 = Some (scoped_kind_byte k d)) /\
  (forall k, Nat.land (scoped_kind_byte k Durable) ephemeral_bit = 0) /\
  (forall k, Nat.land (scoped_kind_byte k Ephemeral) ephemeral_bit = ephemeral_bit).
Proof.
  split; [| split; [| split]].
  - intros x y Hx Hy Hdx Hdy Heq.
    pose proof (encode_injective_ok x y Hx Hy Heq) as ->. rewrite Hdx in Hdy. discriminate.
  - intros x d Hd.
    destruct x as [b | s | s | s key | s key | s tail | s | s | s t | d' p | d' p | k p];
      simpl in Hd; try discriminate; injection Hd as <-.
    + exists KConfig. reflexivity.
    + exists KSequence. reflexivity.
    + exists KSequence. reflexivity.
    + exists KData. reflexivity.
    + exists KPathIndex. reflexivity.
    + exists KAllocator. reflexivity.
    + exists KHandle. reflexivity.
    + exists KName. reflexivity.
    + exists KMatcher. reflexivity.
    + exists KBody. reflexivity.
    + exists k. reflexivity.
  - intros []; vm_compute; reflexivity.
  - intros []; vm_compute; reflexivity.
Qed.

(* Headline 5. A program mints a new name with 32 random bytes. No Reified
   id has 32 bytes: an id has 2 bytes or at least 34. *)
Theorem reified_ids_disjoint_from_new_ids :
  forall x, reified_id_ok x ->
    (length (encode_reified_id x) = 2 \/ 34 <= length (encode_reified_id x)) /\
    (forall new_id, length new_id = 32 -> encode_reified_id x <> new_id).
Proof.
  intros x Hok. pose proof (encode_length x Hok) as Hl. unfold sid_len in Hl.
  split; [exact Hl |].
  intros new_id Hlen Heq. rewrite <- Heq in Hlen. lia.
Qed.

(* The MeTTaIL band 0xF0-0xF9 (plan decision 7). *)
Definition mettail_tag (b : nat) : Prop := 240 <= b <= 249.

(* Headline 6. A Reified id is never a byte name (1 byte) and never a
   MeTTaIL id (first byte 0xF0-0xF9), and its first byte is 0xE0. *)
Theorem reified_ids_disjoint_from_byte_names_and_mettail :
  forall x, reified_id_ok x ->
    (forall b, encode_reified_id x <> [b]) /\
    (forall t rest, mettail_tag t -> encode_reified_id x <> t :: rest) /\
    hd_error (encode_reified_id x) = Some reified_tag.
Proof.
  intros x Hok. destruct (encode_head x) as [rest Hhead].
  split; [| split].
  - intros b Heq. pose proof (encode_length x Hok) as Hl. rewrite Heq in Hl. simpl in Hl.
    unfold sid_len in Hl. lia.
  - intros t rest' Ht Heq. rewrite Hhead in Heq. injection Heq as Heq _.
    unfold mettail_tag, reified_tag in *. lia.
  - rewrite Hhead. reflexivity.
Qed.


(* ── 9. Headline 7: the op ids ────────────────────────────────────────── *)

Definition factory_ops : list nat := seq 0 32.
Definition native_engine_factory : nat := 32.
Definition get_space_agent : nat := 33.
Definition assert_equal : nat := 34.
Definition vector_op : nat := 35.
Definition handle_op : nat := 48.
Definition matcher_op : nat := 49.
Definition sink_op : nat := 50.
Definition marker_ops : list nat := [208; 209; 210; 211; 212].
Definition agent_tag : nat := 255.

(* Every op index that the plan names: the 32 factories, the engine
   factory, getSpaceAgent, assertEqual, vector, the three internal ops, the
   five syntax markers and AGENT_TAG. *)
Definition named_ops : list nat :=
  factory_ops ++ [native_engine_factory; get_space_agent; assert_equal; vector_op;
                  handle_op; matcher_op; sink_op] ++ marker_ops ++ [agent_tag].

Fixpoint nodupb (l : list nat) : bool :=
  match l with
  | [] => true
  | x :: rest => negb (existsb (Nat.eqb x) rest) && nodupb rest
  end.

Lemma nodupb_spec : forall l, nodupb l = true -> NoDup l.
Proof.
  induction l as [| x rest IH]; intros H; [constructor |].
  simpl in H. apply andb_true_iff in H as [Hx Hrest].
  constructor; [| apply IH; exact Hrest].
  intros Hin. apply negb_true_iff in Hx.
  assert (Hex : existsb (Nat.eqb x) rest = true)
    by (apply existsb_exists; exists x; split; [exact Hin | apply Nat.eqb_refl]).
  rewrite Hex in Hx. discriminate.
Qed.

(* Headline 7. The named op indices are pairwise distinct and below 256,
   the op channel is injective, an op id differs from every id with a
   space, and the decoder reads an op id as ROp. *)
Theorem op_ids_pairwise_distinct :
  NoDup named_ops /\
  Forall (fun op => op < 256) named_ops /\
  length named_ops = 45 /\
  (forall a b, op_channel a = op_channel b -> a = b) /\
  (forall op x, reified_id_ok x -> id_durability x <> None ->
     op_channel op <> encode_reified_id x) /\
  (forall op, decode_reified_id (op_channel op) = Some (ROp op)).
Proof.
  split; [| split; [| split; [| split; [| split]]]].
  - apply nodupb_spec. vm_compute. reflexivity.
  - apply Forall_forall. intros op Hin.
    unfold named_ops, factory_ops, marker_ops in Hin.
    repeat rewrite in_app_iff in Hin. rewrite in_seq in Hin. simpl in Hin.
    unfold native_engine_factory, get_space_agent, assert_equal, vector_op, handle_op,
      matcher_op, sink_op, agent_tag in Hin. lia.
  - reflexivity.
  - intros a b H. injection H as H. exact H.
  - intros op x Hok Hd Heq.
    pose proof (encode_length_spaced x Hok Hd) as Hl. rewrite <- Heq in Hl. simpl in Hl.
    unfold sid_len in Hl. lia.
  - intros op. reflexivity.
Qed.

(* ── 10. Headline 10: names route to their space ───────────────────────── *)

(* Headline 10. A name id decodes to its space and its tail, so routing
   finds the space from the name alone (I-3, rule R3). A random tail gives a
   66-byte id and an index tail a 42-byte id. *)
Theorem name_id_routes_to_sid :
  forall s t, space_ok s -> tail_ok t ->
    decode_reified_id (name_id s t) = Some (RName s t) /\
    (forall s' t', decode_reified_id (name_id s t) = Some (RName s' t') -> s' = s /\ t' = t) /\
    length (name_id s t) = match t with TailRandom _ => 66 | TailIndex _ => 42 end.
Proof.
  intros s t Hs Ht.
  assert (Hdec : decode_reified_id (name_id s t) = Some (RName s t))
    by exact (decode_encode_ok (RName s t) (conj Hs Ht)).
  split; [exact Hdec | split].
  - intros s' t' H. rewrite Hdec in H. injection H as -> ->. split; reflexivity.
  - destruct Hs as [Hlen _]. unfold name_id, space_scoped_id. simpl.
    rewrite length_app, Hlen.
    destruct t as [b | b]; destruct Ht as [Hb _]; simpl; rewrite Hb; reflexivity.
Qed.

(* ── 11. Headline 11: the collection key ──────────────────────────────── *)

Section Canon.
  (* A Rust Par is a shape and its locally_free bit set. Par equality
     compares the shapes. sort models ParSortMatcher::sort_match on the
     shape, and ser models bincode::serialize, which writes locally_free as
     empty bytes. *)
  Variable Shape : Type.
  Variable sort : Shape -> Shape.
  Variable ser : Shape -> bytes.
  Hypothesis ser_injective : forall a b, ser a = ser b -> a = b.
  Hypothesis ser_nonempty : forall a, ser a <> [].

  Record rust_par : Type := mkPar { par_shape : Shape; par_locally_free : bytes }.

  Definition par_eq (p q : rust_par) : Prop := par_shape p = par_shape q.

  (* canonical_inner_channel_bytes *)
  Definition canon (p : rust_par) : bytes := ser (sort (par_shape p)).

  (* Headline 11. The key of a channel is a function of its sorted shape:
     Par-equal channels share one key whatever their locally_free bits are,
     two channels share a key exactly when they sort to one shape, and no key
     is empty, so a collection id is longer than a space id. *)
  Theorem canon_respects_par_eq :
    (forall p q, par_eq p q -> canon p = canon q) /\
    (forall p q, canon p = canon q <-> sort (par_shape p) = sort (par_shape q)) /\
    (forall p, canon p <> []) /\
    (forall s p q, space_ok s ->
       data_channel s (canon p) = data_channel s (canon q) <->
       sort (par_shape p) = sort (par_shape q)) /\
    (forall s p, space_ok s -> 2 + sid_len < length (data_channel s (canon p))).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros p q Heq. unfold canon. rewrite Heq. reflexivity.
    - intros p q. unfold canon. split; [apply ser_injective | intros ->; reflexivity].
    - intros p. apply ser_nonempty.
    - intros s p q Hs. split.
      + intros H. unfold data_channel, space_scoped_id in H. injection H as H.
        apply app_inv_head in H. apply ser_injective. exact H.
      + intros H. unfold canon. rewrite H. reflexivity.
    - intros s p [Hlen _]. unfold data_channel, space_scoped_id. simpl.
      rewrite length_app, Hlen. pose proof (ser_nonempty (sort (par_shape p))) as Hne.
      unfold canon. destruct (ser (sort (par_shape p))); [contradiction |].
      simpl. unfold sid_len in *. lia.
  Qed.
End Canon.

(* The premises of headline 11 hold for a concrete shape type and a
   self-delimiting serializer: the shape is a list of numbers, the sort is
   the identity on the shape, and the serializer writes the length first,
   as bincode writes the length of a Vec. So the premises are consistent. *)
Definition length_prefixed (l : list nat) : bytes := length l :: l.

Corollary canon_respects_par_eq_instance :
  (forall p q : rust_par (list nat),
      par_eq (list nat) p q ->
      canon (list nat) (fun x => x) length_prefixed p =
      canon (list nat) (fun x => x) length_prefixed q) /\
  (forall l1 l2 lf1 lf2,
      canon (list nat) (fun x => x) length_prefixed (mkPar (list nat) l1 lf1) =
      canon (list nat) (fun x => x) length_prefixed (mkPar (list nat) l2 lf2) <-> l1 = l2).
Proof.
  assert (Hinj : forall a b, length_prefixed a = length_prefixed b -> a = b)
    by (intros a b H; injection H as _ H; exact H).
  assert (Hne : forall a, length_prefixed a <> []) by (intros a H; discriminate).
  destruct (canon_respects_par_eq (list nat) (fun x => x) length_prefixed Hinj Hne)
    as [H1 [H2 _]].
  split; [exact H1 |].
  intros l1 l2 lf1 lf2. exact (H2 (mkPar _ l1 lf1) (mkPar _ l2 lf2)).
Qed.

(* ── 12. Worked examples ──────────────────────────────────────────────── *)

Definition example_sid : bytes := repeat 7 32.
Definition example_space : native_space := mkSpace example_sid Durable.
Definition example_temp_space : native_space := mkSpace example_sid Ephemeral.

Example example_handle_bytes :
  firstn 3 (handle_channel example_space) = [224; 72; 7] /\
  firstn 3 (handle_channel example_temp_space) = [224; 104; 7].
Proof. split; reflexivity. Qed.

Example example_decoder_rejects_ephemeral_engine :
  decode_reified_id (224 :: 109 :: example_sid) = None.
Proof. reflexivity. Qed.

Example example_decoder_rejects_short_payload :
  decode_reified_id [224; 72; 1; 2; 3] = None.
Proof. reflexivity. Qed.

Example example_decoder_rejects_other_tag :
  decode_reified_id (225 :: 72 :: example_sid) = None.
Proof. reflexivity. Qed.

Example example_decoder_rejects_config_with_tail :
  decode_reified_id (224 :: 67 :: example_sid ++ [1]) = None.
Proof. reflexivity. Qed.

Example example_decoder_rejects_bad_name_tail :
  decode_reified_id (224 :: 78 :: example_sid ++ [1; 2; 3]) = None.
Proof. reflexivity. Qed.

(* ── 13. Assumption audit ──────────────────────────────────────────────── *)

Print Assumptions kind_byte_injective.
Print Assumptions storage_id_injective.
Print Assumptions handle_id_injective.
Print Assumptions ephemeral_durable_disjoint.
Print Assumptions reified_ids_disjoint_from_new_ids.
Print Assumptions reified_ids_disjoint_from_byte_names_and_mettail.
Print Assumptions op_ids_pairwise_distinct.
Print Assumptions decode_encode_reified_id.
Print Assumptions space_and_collection_counters_distinct.
Print Assumptions name_id_routes_to_sid.
Print Assumptions canon_respects_par_eq.
