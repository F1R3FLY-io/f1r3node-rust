----------------------- MODULE ProtocolDeployIngress -----------------------
EXTENDS FiniteSets, Integers

CONSTANT
    \* @type: Bool;
    EnforceProtocolIngress,
    \* @type: Bool;
    EnforceBlockWindow

ASSUME /\ EnforceProtocolIngress \in BOOLEAN
       /\ EnforceBlockWindow \in BOOLEAN

Protocols == {5, 6}
Kinds == {"legacy", "envelope"}
Ids == 0..1
MaxTip == 2
Lifespan == 1

ValidAfter(id) == 0

WindowOpen(id, observedTip) ==
    ValidAfter(id) > observedTip - Lifespan

VARIABLES
    \* @type: Int;
    protocol,
    \* @type: Set(Int);
    legacyPool,
    \* @type: Set(Int);
    envelopePool,
    \* @type: Int;
    nextId,
    \* @type: Int;
    tip,
    \* @type: (Int -> Int);
    admissionTip

vars == <<protocol, legacyPool, envelopePool, nextId, tip, admissionTip>>

Init ==
    /\ protocol \in Protocols
    /\ legacyPool = {}
    /\ envelopePool = {}
    /\ nextId = 0
    /\ tip = 0
    /\ admissionTip = [id \in Ids |-> -1]

LegacyAuthorized == protocol < 6
EnvelopeAuthorized == protocol >= 6

SubmitLegacy ==
    /\ nextId \in Ids
    /\ LET accepted ==
            (~EnforceProtocolIngress \/ LegacyAuthorized)
            /\ (~EnforceBlockWindow \/ WindowOpen(nextId, tip))
       IN /\ legacyPool' = IF accepted THEN legacyPool \union {nextId} ELSE legacyPool
          /\ admissionTip' = IF accepted
                              THEN [admissionTip EXCEPT ![nextId] = tip]
                              ELSE admissionTip
    /\ nextId' = nextId + 1
    /\ UNCHANGED <<protocol, envelopePool, tip>>

SubmitEnvelope ==
    /\ nextId \in Ids
    /\ LET accepted ==
            (~EnforceProtocolIngress \/ EnvelopeAuthorized)
            /\ (~EnforceBlockWindow \/ WindowOpen(nextId, tip))
       IN /\ envelopePool' = IF accepted THEN envelopePool \union {nextId} ELSE envelopePool
          /\ admissionTip' = IF accepted
                              THEN [admissionTip EXCEPT ![nextId] = tip]
                              ELSE admissionTip
    /\ nextId' = nextId + 1
    /\ UNCHANGED <<protocol, legacyPool, tip>>

AdvanceTip ==
    /\ tip < MaxTip
    /\ tip' = tip + 1
    /\ UNCHANGED <<protocol, legacyPool, envelopePool, nextId, admissionTip>>

Next == SubmitLegacy \/ SubmitEnvelope \/ AdvanceTip

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ protocol \in Protocols
    /\ legacyPool \subseteq Ids
    /\ envelopePool \subseteq Ids
    /\ nextId \in 0..Cardinality(Ids)
    /\ tip \in 0..MaxTip
    /\ admissionTip \in [Ids -> -1..MaxTip]

V6HasNoLegacyPool == protocol >= 6 => legacyPool = {}
PreV6HasNoEnvelopePool == protocol < 6 => envelopePool = {}
PoolDomainsAreDisjoint == legacyPool \intersect envelopePool = {}
IngressWindowSound ==
    \A id \in legacyPool \union envelopePool : WindowOpen(id, admissionTip[id])

=============================================================================
