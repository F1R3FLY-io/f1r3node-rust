----------------------- MODULE ProtocolDeployIngress -----------------------
EXTENDS FiniteSets, Integers

CONSTANT
    \* @type: Bool;
    EnforceProtocolIngress

ASSUME EnforceProtocolIngress \in BOOLEAN

Protocols == {5, 6}
Kinds == {"legacy", "envelope"}

VARIABLES
    \* @type: Int;
    protocol,
    \* @type: Set(Int);
    legacyPool,
    \* @type: Set(Int);
    envelopePool,
    \* @type: Int;
    nextId

vars == <<protocol, legacyPool, envelopePool, nextId>>

Init ==
    /\ protocol \in Protocols
    /\ legacyPool = {}
    /\ envelopePool = {}
    /\ nextId = 0

LegacyAuthorized == protocol < 6
EnvelopeAuthorized == protocol >= 6

SubmitLegacy ==
    /\ legacyPool' =
        IF EnforceProtocolIngress /\ ~LegacyAuthorized
        THEN legacyPool
        ELSE legacyPool \union {nextId}
    /\ nextId' = nextId + 1
    /\ UNCHANGED <<protocol, envelopePool>>

SubmitEnvelope ==
    /\ envelopePool' =
        IF EnforceProtocolIngress /\ ~EnvelopeAuthorized
        THEN envelopePool
        ELSE envelopePool \union {nextId}
    /\ nextId' = nextId + 1
    /\ UNCHANGED <<protocol, legacyPool>>

Next == nextId < 2 /\ (SubmitLegacy \/ SubmitEnvelope)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ protocol \in Protocols
    /\ legacyPool \subseteq 0..nextId
    /\ envelopePool \subseteq 0..nextId
    /\ nextId \in Nat

V6HasNoLegacyPool == protocol >= 6 => legacyPool = {}
PreV6HasNoEnvelopePool == protocol < 6 => envelopePool = {}
PoolDomainsAreDisjoint == legacyPool \intersect envelopePool = {}

=============================================================================
