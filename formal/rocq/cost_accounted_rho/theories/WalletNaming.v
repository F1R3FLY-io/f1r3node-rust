(* ═══════════════════════════════════════════════════════════════════════════
   WalletNaming.v — Per-validator wallet @W_v naming: injectivity + domain
                    disjointness (Cost-Accounted Rho Stage A; DR-13)
   ═══════════════════════════════════════════════════════════════════════════

   The validator economic layer realizes the spec's per-validator phlogiston
   DRAW wallet (spec Appendix B eq:38) as a content-addressed Rholang channel

       @W_v  :=  @( *walletTag, validatorPk )

   where [walletTag] is a fresh, genesis-scoped UNFORGEABLE name (minted by
   [new] in PoS.rho, never derivable from a public key or a string). Two
   consensus-critical properties of this naming scheme must hold:

   (1) INJECTIVITY in the validator public key. Distinct validators must get
       DISTINCT wallets, so a mint authorized for one validator can never land
       in another's wallet and one validator's draw can never starve another.
       This is [wallet_name_injective].

   (2) DOMAIN DISJOINTNESS. The validator economic layer derives several
       families of content-addressed names / deterministic seeds from a public
       key, each in its OWN domain:
         - the wallet draw channel       (this file's [Wallet] domain),
         - the quarantine channel        ([Quarantine]; Stage C slashing,
                                          spec Appendix B "Slashing"),
         - the funding-slot seed         ([FundingSlot]; spec §4.7),
       mirrored on the Rust side by the domain-tagged
       [generate_epoch_mint_deploy_random_seed] / quarantine / funding-slot
       seed constructors (system_deploy_util.rs). A name in one domain must
       NEVER collide with a name in another — otherwise a wallet draw could be
       confused with a quarantine move or a funding-slot deposit. These are
       [wallet_quarantine_domain_disjoint],
       [wallet_funding_slot_domain_disjoint], and
       [quarantine_funding_slot_domain_disjoint].

   MODELLING. We model @W_v as a [name] (RhoSyntax.v): [@(...)] is [Quote], and
   the channel is the quotation of a process that injectively encodes the pair
   (domain-tag, public-key). The public key is a [list bool] (matching the
   ground-axis carrier [SGround : list bool -> sig], CostAccountedSyntax.v).
   The unforgeable [walletTag] is realized as a fixed closed marker process
   shared by all wallet names of a shard; because injectivity and disjointness
   are properties of the (tag, pk) encoding — NOT of tag secrecy — we do not
   need to model the GPrivate unforgeable namespace here (unforgeability is the
   substrate guarantee discharged at the Rust/runtime layer; see
   supply-realization-c-d-handoff.md Decision 1). The encoder is built
   CONCRETELY from the native [proc]/[name] constructors, so every theorem in
   this file is proved unconditionally (no axioms, no Section hypotheses):
   [Print Assumptions] of each headline theorem reports "Closed under the
   global context".

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                          │ Property
   ───────────────────────────────────────┼─────────────────────────────────
   wallet_name_injective                  │ distinct pk ⇒ distinct @W_v
                                          │   (spec App. B eq:38; DR-13)
   wallet_quarantine_domain_disjoint      │ wallet ≠ quarantine name
   wallet_funding_slot_domain_disjoint    │ wallet ≠ funding-slot name
   quarantine_funding_slot_domain_disjoint│ quarantine ≠ funding-slot name
   domain_name_injective                  │ per-domain pk-injectivity
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.x stdlib, RhoSyntax (this project).
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Public keys and seed domains
   ═══════════════════════════════════════════════════════════════════════════

   A public key is a list of bits — the same ground-axis carrier the
   cost-accounted signature syntax uses ([SGround : list bool -> sig]). The
   three seed domains are the distinct name families the validator economic
   layer derives from a public key.                                            *)

Definition pubkey : Type := list bool.

Inductive seed_domain : Type :=
  | Wallet      : seed_domain   (* the validator draw wallet @W_v (spec App. B eq:38) *)
  | Quarantine  : seed_domain   (* slashed-stake quarantine channel (Stage C) *)
  | FundingSlot : seed_domain.  (* funding-slot seed (spec §4.7) *)

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: An injective bit-list encoder into [proc]
   ═══════════════════════════════════════════════════════════════════════════

   We encode a [list bool] as a [proc] using two structurally-distinct unit
   markers, one per bit value, terminated by [PNil]:

       []          ↦  PNil
       false :: bs ↦  PPar  zero_marker      (encode_bits bs)
       true  :: bs ↦  PPar  one_marker       (encode_bits bs)

   with [zero_marker := PNil] and [one_marker := PDeref (Quote PNil)] — two
   processes that are never equal (one is [PNil], the other a [PDeref]). The
   left component of each [PPar] records the bit, the right the tail, so the
   encoding is a structural list and is INJECTIVE.                            *)

Definition zero_marker : proc := PNil.
Definition one_marker  : proc := PDeref (Quote PNil).

Lemma markers_distinct : zero_marker <> one_marker.
Proof. unfold zero_marker, one_marker. discriminate. Qed.

Fixpoint encode_bits (bs : pubkey) : proc :=
  match bs with
  | []          => PNil
  | false :: bs' => PPar zero_marker (encode_bits bs')
  | true  :: bs' => PPar one_marker  (encode_bits bs')
  end.

(* The empty bit-list is the ONLY one encoded as [PNil]; every non-empty list
   is encoded as a [PPar]. This lets us discriminate [[]] from any cons. *)
Lemma encode_bits_nil_iff : forall bs,
  encode_bits bs = PNil <-> bs = [].
Proof.
  intros bs. split.
  - destruct bs as [| b bs']; [reflexivity |].
    destruct b; simpl; discriminate.
  - intros ->. reflexivity.
Qed.

(* The encoder is injective: equal encodings come from equal bit-lists.
   By induction on the first list, case-splitting on the head bit and using
   [markers_distinct] to separate the [false]/[true] heads. *)
Lemma encode_bits_injective : forall bs1 bs2,
  encode_bits bs1 = encode_bits bs2 -> bs1 = bs2.
Proof.
  induction bs1 as [| b1 bs1' IH]; intros bs2 Heq.
  - (* bs1 = [] : then encode_bits bs2 = PNil, so bs2 = []. *)
    simpl in Heq. symmetry in Heq.
    apply encode_bits_nil_iff in Heq. exact (eq_sym Heq).
  - (* bs1 = b1 :: bs1' *)
    destruct bs2 as [| b2 bs2'].
    + (* bs2 = [] : encode_bits (b1::bs1') is a PPar, cannot equal PNil. *)
      exfalso. simpl in Heq.
      destruct b1; simpl in Heq; discriminate.
    + (* bs2 = b2 :: bs2' : compare heads, then recurse on tails.
         In the same-head cases the [PPar] marker components are syntactically
         identical, so [injection] discharges that equation and yields ONLY the
         tail equation; in the mismatched-head cases the markers differ and
         [injection] yields the contradictory marker equation. We unfold the
         markers first so each mismatched head reduces to distinct top-level
         constructors ([PNil] vs [PDeref]); then [inversion] handles the
         varying equation count uniformly. *)
      unfold zero_marker, one_marker in Heq.
      destruct b1; destruct b2; simpl in Heq.
      * (* true, true *)
        inversion Heq as [Htail]. f_equal. apply IH. exact Htail.
      * (* true, false : PDeref (Quote PNil) = PNil — impossible *)
        inversion Heq.
      * (* false, true : PNil = PDeref (Quote PNil) — impossible *)
        inversion Heq.
      * (* false, false *)
        inversion Heq as [Htail]. f_equal. apply IH. exact Htail.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Domain markers and the content-addressed name
   ═══════════════════════════════════════════════════════════════════════════

   Each domain is given a STRUCTURALLY DISTINCT closed marker process. We use
   replication-nesting depth as the discriminator: 1, 2, and 3 wraps of
   [PReplicate] around [PNil]. Distinct depths give never-equal processes
   (a single [PReplicate] is not a double [PReplicate], etc.).                 *)

Definition domain_marker (d : seed_domain) : proc :=
  match d with
  | Wallet      => PReplicate PNil
  | Quarantine  => PReplicate (PReplicate PNil)
  | FundingSlot => PReplicate (PReplicate (PReplicate PNil))
  end.

(* The domain markers are pairwise distinct (distinct [PReplicate] nesting
   depths). Stated as the three needed inequalities. *)
Lemma domain_marker_wallet_quarantine :
  domain_marker Wallet <> domain_marker Quarantine.
Proof. simpl. discriminate. Qed.

Lemma domain_marker_wallet_funding_slot :
  domain_marker Wallet <> domain_marker FundingSlot.
Proof. simpl. discriminate. Qed.

Lemma domain_marker_quarantine_funding_slot :
  domain_marker Quarantine <> domain_marker FundingSlot.
Proof. simpl. discriminate. Qed.

(* The content-addressed name for (domain, pubkey). This models the Rholang
   channel @( *tag_d, pk ): a quotation of a process pairing the domain marker
   (which encodes BOTH the unforgeable [walletTag]-style tag AND the domain
   discriminator) with the injective bit-encoding of the public key. *)
Definition domain_name (d : seed_domain) (pk : pubkey) : name :=
  Quote (PPar (domain_marker d) (encode_bits pk)).

(* The validator draw wallet @W_v := @( *walletTag, validatorPk ) is the
   [Wallet]-domain name. *)
Definition wallet_name (pk : pubkey) : name :=
  domain_name Wallet pk.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Per-domain injectivity
   ═══════════════════════════════════════════════════════════════════════════

   Within a fixed domain the name is injective in the public key: the domain
   marker is the same on both sides, so equality of names forces equality of
   the bit-encodings, hence (by [encode_bits_injective]) equality of the keys. *)

Lemma domain_name_injective : forall d pk1 pk2,
  domain_name d pk1 = domain_name d pk2 -> pk1 = pk2.
Proof.
  intros d pk1 pk2 Heq.
  unfold domain_name in Heq.
  (* [Quote] and [PPar] are injective; the left (marker) [PPar] components are
     syntactically identical (same [d]), so [injection] descends through the
     [Quote] and the identical marker in one step, leaving exactly the
     bit-encoding equality [encode_bits pk1 = encode_bits pk2]. *)
  injection Heq as Hbits.
  apply encode_bits_injective. exact Hbits.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: Headline theorem — wallet-name injectivity (DR-13)
   ═══════════════════════════════════════════════════════════════════════════

   Distinct validator public keys yield distinct draw wallets @W_v. This is
   the consensus-critical property that a mint authorized for validator [pk1]
   cannot land in validator [pk2]'s wallet, and that independent validators'
   wallet Produces have distinct content-addressed identities (so the
   multi-parent merge engine never conflates them). The contrapositive of
   [domain_name_injective] at the [Wallet] domain.                            *)

Theorem wallet_name_injective : forall pk1 pk2,
  wallet_name pk1 = wallet_name pk2 -> pk1 = pk2.
Proof.
  intros pk1 pk2 Heq.
  unfold wallet_name in Heq.
  apply (domain_name_injective Wallet). exact Heq.
Qed.

(* Equivalent inequality form: distinct keys ⇒ distinct wallets. *)
Corollary wallet_name_distinct : forall pk1 pk2,
  pk1 <> pk2 -> wallet_name pk1 <> wallet_name pk2.
Proof.
  intros pk1 pk2 Hne Heq.
  apply Hne. apply wallet_name_injective. exact Heq.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Headline theorems — domain disjointness
   ═══════════════════════════════════════════════════════════════════════════

   Names in different domains are NEVER equal, for ANY public keys. Because the
   domain marker sits in the left component of the [PPar] under the [Quote],
   equality of two cross-domain names would force equality of their domain
   markers, which the [domain_marker_*] lemmas refute. This guarantees a wallet
   draw can never be confused with a quarantine move or a funding-slot deposit
   — the disjointness mirrored on the Rust side by the domain-tagged seed
   constructors (system_deploy_util.rs).                                       *)

Lemma domain_name_disjoint : forall d1 d2 pk1 pk2,
  domain_marker d1 <> domain_marker d2 ->
  domain_name d1 pk1 <> domain_name d2 pk2.
Proof.
  intros d1 d2 pk1 pk2 Hmark Heq.
  apply Hmark.
  unfold domain_name in Heq.
  (* [injection] descends through the [Quote] and [PPar], yielding the marker
     equality (left component) and the bit-encoding equality (right). We need
     only the marker equality, which contradicts [Hmark]. *)
  injection Heq as Hm _.
  exact Hm.
Qed.

Theorem wallet_quarantine_domain_disjoint : forall pk1 pk2,
  domain_name Wallet pk1 <> domain_name Quarantine pk2.
Proof.
  intros pk1 pk2.
  apply domain_name_disjoint.
  apply domain_marker_wallet_quarantine.
Qed.

Theorem wallet_funding_slot_domain_disjoint : forall pk1 pk2,
  domain_name Wallet pk1 <> domain_name FundingSlot pk2.
Proof.
  intros pk1 pk2.
  apply domain_name_disjoint.
  apply domain_marker_wallet_funding_slot.
Qed.

Theorem quarantine_funding_slot_domain_disjoint : forall pk1 pk2,
  domain_name Quarantine pk1 <> domain_name FundingSlot pk2.
Proof.
  intros pk1 pk2.
  apply domain_name_disjoint.
  apply domain_marker_quarantine_funding_slot.
Qed.

(* In particular the wallet domain is disjoint from BOTH other domains for any
   keys — the wallet's unforgeable, injective, domain-separated identity is
   complete. *)
Corollary wallet_domain_separated : forall pk1 pk2,
  domain_name Wallet pk1 <> domain_name Quarantine pk2 /\
  domain_name Wallet pk1 <> domain_name FundingSlot pk2.
Proof.
  intros pk1 pk2. split.
  - apply wallet_quarantine_domain_disjoint.
  - apply wallet_funding_slot_domain_disjoint.
Qed.
