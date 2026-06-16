//! Minimal signature IR for cost-accounted surface recognition.
//!
//! [`Sig`] is the desugared, resolution-ready form of a surface
//! [`Signature`](rholang_parser::ast::Signature): the lollipop (`Transfer`)
//! sugar has been eliminated (it is rewritten at the *term* level by
//! [`super::desugar`]) and compounds are flattened + key-sorted, so two
//! structurally-equal signatures share one `Sig` — and therefore (via the
//! native bridge in [`super::sig`]) one supply channel `Σ⟦s⟧`.
//!
//! `Sig` deliberately mirrors the *shape* of f1r3node's native
//! `accounting::Sig` / `ResourceSignature`: it carries the extra `Bound`
//! ring-fence axis the native enum lacks, plus the `key()` content-hash used
//! for the compound sort. The bridge to the native funding algebra lives in
//! [`super::sig::signature_to_native_sig`] (native `from_sig` derives the
//! consensus channel — surface forms DECORATE, they never re-emit metered ops).
//!
//! ## Content-addressing invariant
//!
//! `key(s)` is derived from the same atom hashes the native channel is, so two
//! signatures share a `key` iff they share a channel:
//!
//! * atom `Ground(b)` / `Quote(b)` ⇒ `key = Blake2b256(DOMAIN ‖ b)`;
//! * `Compound([s₁..sₙ])` (components key-sorted) ⇒
//!   `key = Blake2b256(DOMAIN_COMPOUND ‖ key(s₁) ‖ … ‖ key(sₙ))`, injective in
//!   the sorted component list.

use crypto::rust::hash::blake2b256::Blake2b256;

/// Domain separator for the ground axis `Σ⟦g⟧`. Distinct from
/// [`DOMAIN_QUOTE`] so a ground principal and a quote principal can never
/// alias even if their canonical bytes coincide, and distinct from any other
/// protocol hash over the same bytes.
///
/// NOTE: native `SignatureChannel::from_sig` applies NO domain separator at the
/// channel layer (ground vs quote are byte-identical there; DR-1). This IR
/// separates the axes only in the structural `key()` digest used for the
/// compound sort — the native channel derivation is authoritative for consensus.
pub const DOMAIN_GROUND: &[u8] = b"f1r3fly.cost.sig.ground.v1";

/// Domain separator for the quote axis `Σ⟦#P⟧`. See [`DOMAIN_GROUND`].
pub const DOMAIN_QUOTE: &[u8] = b"f1r3fly.cost.sig.quote.v1";

/// Domain separator for the compound key digest. Keeps a compound's structural
/// key from colliding with a raw atom hash over the same bytes.
pub const DOMAIN_COMPOUND: &[u8] = b"f1r3fly.cost.sig.compound.v1";

/// Domain separator for a `new`-bound (ring-fenced) ground signature. A ground
/// sig that resolves to a `new`-binder keys its channel on the binder's stable
/// identity (its source span) under THIS domain, so a fresh `new`-bound sig is
/// unforgeable/ring-fenced and never aliases a free (content-by-spelling) sig of
/// the same identifier — that is how ring-fencing is realised (cost-accounted-rho
/// "signatures are names": a `new`-bound name is an unforgeable capability).
pub const DOMAIN_BOUND: &[u8] = b"f1r3fly.cost.sig.bound.v1";

/// A signature's local resource-logic contract, mirroring native
/// `accounting::ResourceSignature` by *shape* (no hard dependency). Anchors
/// the funding pool: two values share a pool iff they share a [`key`].
///
/// [`key`]: ResourceSignature::key
pub trait ResourceSignature {
    /// Canonical, collision-resistant per-signature key (content hash).
    fn key(&self) -> [u8; 32];

    /// Component decompositions a combined-cell token can be split into / a
    /// split set can be joined from. Empty for an atom (no decomposition);
    /// the key-sorted component list for a compound. Agrees with the native
    /// funding pool structure (`split_join_decompositions`).
    fn split_join_decompositions(&self) -> Vec<Self>
    where Self: Sized;
}

/// Minimal, resolution-ready signature IR. Lollipop is desugared away before a
/// `Signature` becomes a `Sig`, so there is no `Transfer`/`Lolly` variant.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Sig {
    /// Ground principal `g` (a *free* identifier): `content` = canonical encoding
    /// of the surface identifier (see [`super::sig::canon_ground`]); shared by
    /// spelling across deploys/parties (the §9 rendezvous).
    Ground(Vec<u8>),
    /// A `new`-bound (ring-fenced) ground principal: `content` = the binder's
    /// stable identity (its source span). Unique per `new`-binder, so the fuel is
    /// ring-fenced to that scope and never aliases a free sig of the same name.
    Bound(Vec<u8>),
    /// Quote principal `#P`: `content` = canonical, binder-depth-independent
    /// encoding of `𝒫⟦P⟧` (see [`super::sig::canon_quote`]).
    Quote(Vec<u8>),
    /// Compound `s₁ * … * sₙ` (n ≥ 2), components flattened + key-sorted.
    /// Always constructed via [`Sig::compound`] so the invariant holds.
    Compound(Vec<Sig>),
}

impl Sig {
    /// Smart constructor for a compound. Flattens nested compounds
    /// (associativity) and key-sorts the components (commutativity), so
    /// `(a*b)*c`, `a*(b*c)` and `c*b*a` all yield the *same* `Sig`. A
    /// single-component input collapses to that component (an atom is never
    /// wrapped in a spurious `Compound`).
    pub fn compound(components: Vec<Sig>) -> Sig {
        let mut flat: Vec<Sig> = Vec::with_capacity(components.len());
        for component in components {
            match component {
                Sig::Compound(inner) => flat.extend(inner),
                atom => flat.push(atom),
            }
        }
        flat.sort_by(|a, b| a.key().cmp(&b.key()));
        if flat.len() == 1 {
            flat.into_iter().next().expect("len checked to be 1")
        } else {
            Sig::Compound(flat)
        }
    }

    /// The atoms this signature funds with: a compound is funded by its
    /// component atoms, an atom by itself.
    pub fn atoms(&self) -> Vec<Sig> {
        match self {
            Sig::Compound(components) => components.clone(),
            atom => vec![atom.clone()],
        }
    }

    /// Bridge the surface IR to the native funding algebra
    /// [`accounting::Sig`](crate::rust::interpreter::accounting::Sig) — the
    /// CONSENSUS signature whose `from_sig` derives the supply channel `Σ⟦s⟧`.
    /// Native `from_sig` is authoritative (design §3.1: native wins; surface forms
    /// DECORATE, they never re-emit metered ops), so this maps the IR onto it
    /// rather than deriving a separate channel:
    ///
    /// * `Ground(c)` / `Quote(c)` → the native atom of the SAME content bytes
    ///   (native applies no channel-layer domain separator, DR-1; the ground/quote
    ///   distinction lives in the wire `AtomKind`, not the channel);
    /// * `Bound(span)` → `Ground(DOMAIN_BOUND ‖ span)` — the ring-fence is folded
    ///   INTO the ground content (design §3.2: no native enum change). It cannot
    ///   alias a free `Ground(canon_ground(name))`: `DOMAIN_BOUND` begins `0x66`
    ///   (`'f'`) while a `canon_ground` encoding begins `0x2a` (the `Par.exprs`
    ///   field-5 protobuf tag) — disjoint by leading byte (MINOR-6);
    /// * `Compound([s₁..sₙ])` (key-sorted) → the left-assoc `And`-fold; native
    ///   `from_sig`'s `And` arm sort-matches, so the channel is permutation-
    ///   invariant (commutative `∘`, P8), matching `envelope_sig_compound`.
    pub fn to_native(&self) -> crate::rust::interpreter::accounting::Sig {
        use crate::rust::interpreter::accounting::Sig as NativeSig;
        match self {
            Sig::Ground(content) => NativeSig::Ground(content.clone()),
            Sig::Quote(content) => NativeSig::Quote(content.clone()),
            Sig::Bound(content) => {
                let mut bytes = Vec::with_capacity(DOMAIN_BOUND.len() + content.len());
                bytes.extend_from_slice(DOMAIN_BOUND);
                bytes.extend_from_slice(content);
                NativeSig::Ground(bytes)
            }
            Sig::Compound(components) => {
                let mut iter = components.iter();
                let first = iter
                    .next()
                    .expect("Sig::Compound invariant: ≥ 2 components")
                    .to_native();
                iter.fold(first, |acc, component| {
                    NativeSig::And(Box::new(acc), Box::new(component.to_native()))
                })
            }
        }
    }
}

impl ResourceSignature for Sig {
    fn key(&self) -> [u8; 32] {
        let preimage = match self {
            Sig::Ground(content) => {
                let mut bytes = Vec::with_capacity(DOMAIN_GROUND.len() + content.len());
                bytes.extend_from_slice(DOMAIN_GROUND);
                bytes.extend_from_slice(content);
                bytes
            }
            Sig::Bound(content) => {
                let mut bytes = Vec::with_capacity(DOMAIN_BOUND.len() + content.len());
                bytes.extend_from_slice(DOMAIN_BOUND);
                bytes.extend_from_slice(content);
                bytes
            }
            Sig::Quote(content) => {
                let mut bytes = Vec::with_capacity(DOMAIN_QUOTE.len() + content.len());
                bytes.extend_from_slice(DOMAIN_QUOTE);
                bytes.extend_from_slice(content);
                bytes
            }
            Sig::Compound(components) => {
                let mut bytes = Vec::with_capacity(DOMAIN_COMPOUND.len() + components.len() * 32);
                bytes.extend_from_slice(DOMAIN_COMPOUND);
                for component in components {
                    bytes.extend_from_slice(&component.key());
                }
                bytes
            }
        };
        let hash = Blake2b256::hash(preimage);
        let mut key = [0_u8; 32];
        key.copy_from_slice(&hash[..32]);
        key
    }

    fn split_join_decompositions(&self) -> Vec<Sig> {
        match self {
            Sig::Compound(components) => components.clone(),
            _ => Vec::new(),
        }
    }
}
