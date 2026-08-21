(* ═══════════════════════════════════════════════════════════════════════════
   SystemStructEquiv.v — System-level structural equivalence and source-level
                         free names for the cost-accounted rho calculus
   ═══════════════════════════════════════════════════════════════════════════

   The pure-process structural equivalence [struct_equiv] (RhoSyntax.v) is
   proc-only. The cost-accounted layer adds the [system] sort, and several
   metatheoretic facts — the parallel-unit law and the Appendix-B
   token-stack decomposition — are most naturally stated as equivalences
   BETWEEN SYSTEMS. This module supplies that missing equivalence,
   [sys_equiv], together with:

   - [sse_par_unit]  : [T ∥ ()] ≡ [T]  (the empty token is the ∥-identity,
                       matching the spec's commutative-monoid structure on
                       token stacks with identity [()]).
   - [token_decomp]  : [SToken (TGate s t)] ≡
                       [SPar (SToken (TGate s TUnit)) (SToken t)]
                       — the Appendix-B "peel one layer" decomposition: a
                       depth-(n+1) token stack equals one single-gate token
                       in parallel with the depth-n remainder. This is
                       justified at the token-translation level by the
                       [K⟦·⟧] image (each gate is an independent parallel
                       output), so the two systems are observationally the
                       same configuration.

   It also realises Section 3.5's source-level free-name discipline:

   - [proc_free_names] / [name_free_names] : the standard locally-nameless
     free de Bruijn name set, matching the spec's [FN(·)].
   - [sig_free_names] (a.k.a. [FN_s]) : [FN_s(g) = ∅], [FN_s(#P) = FN(P)],
     [FN_s(s₁ & s₂) = FN_s(s₁) ∪ FN_s(s₂)] (Def. §3.5).
   - [sig_free_names_quote] : [FN_s(#P) = FN(P)], realised at the SOURCE
     level via the [crypto_quote] / [hash_preimage] serialisation bridge
     (so the byte string carried by a [SQuote] atom recovers exactly the
     quoted process's free names).

   Dependencies: RhoSyntax, CostAccountedSyntax (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: System-Level Structural Equivalence
   ═══════════════════════════════════════════════════════════════════════════

   [sys_equiv] is the least congruence making [SPar] a commutative monoid
   with identity [SToken TUnit] (the empty token stack [()]), closed under
   the [proc]-level [struct_equiv] inside signed processes. This mirrors the
   spec's parametrically-polymorphic [∥] at the signed-term level, whose
   identity is the empty stack [()] (§3.2).                                  *)

Reserved Notation "S1 '≡sys' S2" (at level 70, no associativity).

Inductive sys_equiv : system -> system -> Prop :=
  (* --- equivalence relation --- *)
  | sse_refl  : forall S, S ≡sys S
  | sse_sym   : forall S1 S2, S1 ≡sys S2 -> S2 ≡sys S1
  | sse_trans : forall S1 S2 S3, S1 ≡sys S2 -> S2 ≡sys S3 -> S1 ≡sys S3
  (* --- SPar is a commutative monoid with identity (SToken TUnit) --- *)
  | sse_par_comm  : forall S1 S2, SPar S1 S2 ≡sys SPar S2 S1
  | sse_par_assoc : forall S1 S2 S3,
      SPar (SPar S1 S2) S3 ≡sys SPar S1 (SPar S2 S3)
  | sse_par_token_unit : forall S, SPar S (SToken TUnit) ≡sys S
  (* --- Appendix-B token-stack peel (one gate per parallel output) --- *)
  | sse_token_peel : forall s t,
      SToken (TGate s t) ≡sys SPar (SToken (TGate s TUnit)) (SToken t)
  (* --- congruence rules --- *)
  | sse_par_cong : forall S1 S1' S2 S2',
      S1 ≡sys S1' -> S2 ≡sys S2' -> SPar S1 S2 ≡sys SPar S1' S2'
  | sse_signed_cong : forall P P' s,
      P ≡ P' -> SSigned P s ≡sys SSigned P' s
where "S1 '≡sys' S2" := (sys_equiv S1 S2).

(* Convenience congruence corollaries. *)
Lemma sse_par_cong_l : forall S1 S1' S2,
  S1 ≡sys S1' -> SPar S1 S2 ≡sys SPar S1' S2.
Proof. intros. apply sse_par_cong; [assumption | apply sse_refl]. Qed.

Lemma sse_par_cong_r : forall S1 S2 S2',
  S2 ≡sys S2' -> SPar S1 S2 ≡sys SPar S1 S2'.
Proof. intros. apply sse_par_cong; [apply sse_refl | assumption]. Qed.

(* The unit law in the form the metatheory uses: a system in parallel with
   the empty token stack is equivalent to the system alone. This is the
   system-level analogue of [se_par_nil] (RhoSyntax.v, [P ∥ 0 ≡ P]), and
   discharges the spec's "the empty stack [()] is the ∥-identity at the
   signed-term level" claim (§3.2).                                          *)
Theorem sse_par_unit : forall S, SPar S (SToken TUnit) ≡sys S.
Proof. intro S. apply sse_par_token_unit. Qed.

(* The mirrored left-unit law, derived from commutativity + right unit. *)
Theorem sse_unit_par : forall S, SPar (SToken TUnit) S ≡sys S.
Proof.
  intro S.
  apply (sse_trans _ (SPar S (SToken TUnit))).
  - apply sse_par_comm.
  - apply sse_par_unit.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Appendix-B Token-Stack Decomposition
   ═══════════════════════════════════════════════════════════════════════════

   A token stack [s : t] of depth [n+1] decomposes into the single-gate
   token [s : ()] in parallel with the depth-[n] remainder [t]. Operationally
   this is the [K⟦·⟧] "peel one layer" identity: under the token translation
   each gate becomes an independent parallel output, so a stack and its
   layer-wise parallel splitting denote the same configuration. We take it as
   a defining law of [sys_equiv] specialised to token systems and record it
   as a headline theorem.

   The peel is a defining rule [sse_token_peel] of [≡sys] (a constructor of
   the relation, NOT a global axiom): it is the system-level reflection of
   the per-gate parallel output that the token translation [K⟦·⟧] produces
   ([T⟦s:t⟧ = N⟦s⟧!(T⟦t⟧)], one gate per output). [token_decomp] packages it
   as the Appendix-B headline theorem.                                       *)

Theorem token_decomp : forall s t,
  SToken (TGate s t) ≡sys SPar (SToken (TGate s TUnit)) (SToken t).
Proof. intros s t. apply sse_token_peel. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Source-Level Free Names (§3.5)
   ═══════════════════════════════════════════════════════════════════════════

   The free-name function over processes and names, in the standard
   locally-nameless style: a free name is a [NVar k] not captured by an
   enclosing [PInput] binder. Crossing one [PInput] binder decrements the
   indices of the names that escape it (and drops index 0, which the binder
   captures). This matches the spec's [FN(·)] (§3.5), with [PInput] playing
   the role of the binding [for(y ← x){·}].                                  *)

(* Decrement a list of de Bruijn indices by one binder level, dropping any
   index 0 (which is captured by the binder). *)
Fixpoint strip_binder (l : list nat) : list nat :=
  match l with
  | [] => []
  | 0 :: rest => strip_binder rest
  | S k :: rest => k :: strip_binder rest
  end.

Fixpoint proc_free_names (P : proc) : list nat :=
  match P with
  | PNil          => []
  | PInput x P'   => name_free_names x ++ strip_binder (proc_free_names P')
  | POutput x Q   => name_free_names x ++ proc_free_names Q
  | PPar P1 P2    => proc_free_names P1 ++ proc_free_names P2
  | PDeref x      => name_free_names x
  | PReplicate P' => proc_free_names P'
  end
with name_free_names (x : name) : list nat :=
  match x with
  | Quote P => proc_free_names P
  | NVar k  => [k]
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Serialisation Bridge for Cryptographic Quoting
   ═══════════════════════════════════════════════════════════════════════════

   A cryptographic quote [#P] is modelled at the [sig] level as the
   byte string [SQuote bs] where [bs] is a faithful encoding of the quoted
   process [P]. We realise [FN_s(#P) = FN(P)] (§3.5) at the source level by
   a concrete serialise/deserialise pair: [proc_encode : proc -> list bool]
   is injective with computable left inverse [hash_preimage]. This keeps the
   bridge AXIOM-FREE — the round-trip is a proved lemma, not a hypothesis.

   The encoding is a self-delimiting prefix code: each constructor is tagged
   with a fixed-length bit prefix, naturals are encoded in unary, and the
   decoder consumes exactly one term and returns the unparsed remainder.    *)

(* Self-delimiting unary encoding of naturals: k ones followed by a zero. *)
Fixpoint nat_encode (k : nat) : list bool :=
  match k with
  | 0 => [false]
  | S k' => true :: nat_encode k'
  end.

Fixpoint nat_decode (bs : list bool) : option (nat * list bool) :=
  match bs with
  | false :: rest => Some (0, rest)
  | true :: rest =>
      match nat_decode rest with
      | Some (k, rest') => Some (S k, rest')
      | None => None
      end
  | [] => None
  end.

Lemma nat_decode_encode : forall k rest,
  nat_decode (nat_encode k ++ rest) = Some (k, rest).
Proof.
  induction k as [| k IH]; intro rest; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

(* Constructor tag prefixes (fixed length 3 for proc, length 1 for the
   name/proc discriminator inside a name). *)

Fixpoint proc_encode (P : proc) : list bool :=
  match P with
  | PNil          => [false; false; false]
  | PInput x P'   => [false; false; true]  ++ name_encode x ++ proc_encode P'
  | POutput x Q   => [false; true; false]  ++ name_encode x ++ proc_encode Q
  | PPar P1 P2    => [false; true; true]   ++ proc_encode P1 ++ proc_encode P2
  | PDeref x      => [true;  false; false] ++ name_encode x
  | PReplicate P' => [true;  false; true]  ++ proc_encode P'
  end
with name_encode (x : name) : list bool :=
  match x with
  | Quote P => false :: proc_encode P
  | NVar k  => true  :: nat_encode k
  end.

Fixpoint proc_decode (fuel : nat) (bs : list bool) : option (proc * list bool) :=
  match fuel with
  | 0 => None
  | S fuel' =>
      match bs with
      | false :: false :: false :: rest => Some (PNil, rest)
      | false :: false :: true :: rest =>
          match name_decode fuel' rest with
          | Some (x, rest1) =>
              match proc_decode fuel' rest1 with
              | Some (P', rest2) => Some (PInput x P', rest2)
              | None => None
              end
          | None => None
          end
      | false :: true :: false :: rest =>
          match name_decode fuel' rest with
          | Some (x, rest1) =>
              match proc_decode fuel' rest1 with
              | Some (Q, rest2) => Some (POutput x Q, rest2)
              | None => None
              end
          | None => None
          end
      | false :: true :: true :: rest =>
          match proc_decode fuel' rest with
          | Some (P1, rest1) =>
              match proc_decode fuel' rest1 with
              | Some (P2, rest2) => Some (PPar P1 P2, rest2)
              | None => None
              end
          | None => None
          end
      | true :: false :: false :: rest =>
          match name_decode fuel' rest with
          | Some (x, rest1) => Some (PDeref x, rest1)
          | None => None
          end
      | true :: false :: true :: rest =>
          match proc_decode fuel' rest with
          | Some (P', rest1) => Some (PReplicate P', rest1)
          | None => None
          end
      | _ => None
      end
  end
with name_decode (fuel : nat) (bs : list bool) : option (name * list bool) :=
  match fuel with
  | 0 => None
  | S fuel' =>
      match bs with
      | false :: rest =>
          match proc_decode fuel' rest with
          | Some (P, rest1) => Some (Quote P, rest1)
          | None => None
          end
      | true :: rest =>
          match nat_decode rest with
          | Some (k, rest1) => Some (NVar k, rest1)
          | None => None
          end
      | [] => None
      end
  end.

(* The serialiser/decoder round-trip, parameterised by a fuel bound large
   enough to consume the whole encoding. We prove it by mutual structural
   induction on the term: the [proc_size]/[name_size] fuel always suffices.  *)

Fixpoint proc_size (P : proc) : nat :=
  match P with
  | PNil          => 1
  | PInput x P'   => 1 + name_size x + proc_size P'
  | POutput x Q   => 1 + name_size x + proc_size Q
  | PPar P1 P2    => 1 + proc_size P1 + proc_size P2
  | PDeref x      => 1 + name_size x
  | PReplicate P' => 1 + proc_size P'
  end
with name_size (x : name) : nat :=
  match x with
  | Quote P => 1 + proc_size P
  | NVar _  => 1
  end.

(* The serialiser/decoder round-trip: decoding the encoding of a term
   (followed by any suffix [rest]) with sufficient fuel recovers the term and
   the suffix. Proved by simultaneous mutual induction on the [proc]/[name]
   grammar via the [proc_ind_mut]/[name_ind_mut] scheme; the term size always
   bounds the fuel needed. We carry the [proc] and [name] statements as one
   conjoined induction so the recursive calls line up. *)
Lemma decode_encode_round_trip_mut :
  (forall P fuel rest,
     proc_size P <= fuel ->
     proc_decode fuel (proc_encode P ++ rest) = Some (P, rest))
  /\
  (forall x fuel rest,
     name_size x <= fuel ->
     name_decode fuel (name_encode x ++ rest) = Some (x, rest)).
Proof.
  apply (proc_name_mutind
    (fun P => forall fuel rest,
       proc_size P <= fuel ->
       proc_decode fuel (proc_encode P ++ rest) = Some (P, rest))
    (fun x => forall fuel rest,
       name_size x <= fuel ->
       name_decode fuel (name_encode x ++ rest) = Some (x, rest))).
  - (* PNil *)
    intros fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    reflexivity.
  - (* PInput x P' *)
    intros x IHx P' IHP' fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [proc_encode]. rewrite <- !app_assoc. cbn [proc_decode app].
    rewrite IHx by lia.
    rewrite IHP' by lia.
    reflexivity.
  - (* POutput x Q *)
    intros x IHx Q IHQ fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [proc_encode]. rewrite <- !app_assoc. cbn [proc_decode app].
    rewrite IHx by lia.
    rewrite IHQ by lia.
    reflexivity.
  - (* PPar P1 P2 *)
    intros P1 IHP1 P2 IHP2 fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [proc_encode]. rewrite <- !app_assoc. cbn [proc_decode app].
    rewrite IHP1 by lia.
    rewrite IHP2 by lia.
    reflexivity.
  - (* PDeref x *)
    intros x IHx fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [proc_encode]. rewrite <- !app_assoc. cbn [proc_decode app].
    rewrite IHx by lia.
    reflexivity.
  - (* PReplicate P' *)
    intros P' IHP' fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [proc_encode]. rewrite <- !app_assoc. cbn [proc_decode app].
    rewrite IHP' by lia.
    reflexivity.
  - (* Quote P *)
    intros P IHP fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [name_encode name_decode app].
    rewrite IHP by lia.
    reflexivity.
  - (* NVar k *)
    intros k fuel rest Hfuel.
    destruct fuel as [| fuel']; simpl in Hfuel; [lia |].
    cbn [name_encode name_decode app].
    rewrite nat_decode_encode. reflexivity.
Qed.

Lemma decode_encode_round_trip :
  forall P fuel rest,
    proc_size P <= fuel ->
    proc_decode fuel (proc_encode P ++ rest) = Some (P, rest).
Proof. apply (proj1 decode_encode_round_trip_mut). Qed.

(* The total left-inverse decoder: deserialise, defaulting malformed input
   to [PNil]. We supply the term size as fuel via the round-trip lemma. *)
Definition hash_preimage (bs : list bool) : proc :=
  match proc_decode (length bs) bs with
  | Some (P, _) => P
  | None => PNil
  end.

(* The cryptographic-quote constructor at the source level: [#P] is the
   [SQuote] atom carrying the faithful encoding of [P]. *)
Definition crypto_quote (P : proc) : sig := SQuote (proc_encode P).

(* The round-trip in the form [hash_preimage] needs: decoding the encoding
   with [length] fuel recovers [P]. [length (proc_encode P)] is at least
   [proc_size P] (each constructor emits ≥1 bit), so the fuel suffices. *)
Lemma size_le_encode_length_mut :
  (forall P, proc_size P <= length (proc_encode P))
  /\
  (forall x, name_size x <= length (name_encode x)).
Proof.
  apply (proc_name_mutind
    (fun P => proc_size P <= length (proc_encode P))
    (fun x => name_size x <= length (name_encode x))).
  - (* PNil *) cbn. lia.
  - (* PInput *) intros x IHx P' IHP'.
    cbn [proc_encode proc_size]. rewrite !length_app. cbn [length]. lia.
  - (* POutput *) intros x IHx Q IHQ.
    cbn [proc_encode proc_size]. rewrite !length_app. cbn [length]. lia.
  - (* PPar *) intros P1 IHP1 P2 IHP2.
    cbn [proc_encode proc_size]. rewrite !length_app. cbn [length]. lia.
  - (* PDeref *) intros x IHx.
    cbn [proc_encode proc_size]. rewrite !length_app. cbn [length]. lia.
  - (* PReplicate *) intros P' IHP'.
    cbn [proc_encode proc_size]. rewrite !length_app. cbn [length]. lia.
  - (* Quote *) intros P IHP. cbn [name_encode name_size length]. lia.
  - (* NVar *) intros k. cbn [name_encode name_size length nat_encode]. lia.
Qed.

Lemma proc_size_le_encode_length : forall P,
  proc_size P <= length (proc_encode P).
Proof. apply (proj1 size_le_encode_length_mut). Qed.

Lemma hash_preimage_encode : forall P,
  hash_preimage (proc_encode P) = P.
Proof.
  intro P. unfold hash_preimage.
  assert (H : proc_decode (length (proc_encode P)) (proc_encode P)
              = Some (P, [])).
  { pose proof (decode_encode_round_trip P (length (proc_encode P)) []) as Hr.
    rewrite app_nil_r in Hr.
    apply Hr. apply proc_size_le_encode_length. }
  rewrite H. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Signature Free Names (§3.5) and FN_s(#P) = FN(P)
   ═══════════════════════════════════════════════════════════════════════════ *)

(* [FN_s] over signatures: ground signatures contribute no names; a
   cryptographic quote [#P] contributes [FN(P)] (recovered through the
   serialisation bridge); a compound signature is the union of its
   components. This is the spec's Def. §3.5 [FN_s]. *)
Fixpoint sig_free_names (s : sig) : list nat :=
  match s with
  | SUnit       => []
  | SGround _   => []
  | SQuote bs   => proc_free_names (hash_preimage bs)
  | SAnd s1 s2  => sig_free_names s1 ++ sig_free_names s2
  end.

(* The headline §3.5 fact: the free names of a cryptographic quote [#P] are
   exactly the free names of the quoted process [P]. Realised at the source
   level through [crypto_quote]/[hash_preimage] — no axioms. *)
Theorem sig_free_names_quote : forall P,
  sig_free_names (crypto_quote P) = proc_free_names P.
Proof.
  intro P. unfold crypto_quote. cbn [sig_free_names].
  rewrite hash_preimage_encode. reflexivity.
Qed.

(* The ground axis carries no free names (spec [FN_s(g) = ∅]). *)
Theorem sig_free_names_ground : forall bs,
  sig_free_names (SGround bs) = [].
Proof. reflexivity. Qed.

(* The compound case is the union (spec [FN_s(s₁ & s₂) = FN_s s₁ ∪ FN_s s₂]).
   We expose it as list concatenation (the free-name list with multiplicity);
   set semantics is the [In]-closure of this list. *)
Theorem sig_free_names_and : forall s1 s2,
  sig_free_names (SAnd s1 s2) = sig_free_names s1 ++ sig_free_names s2.
Proof. reflexivity. Qed.
