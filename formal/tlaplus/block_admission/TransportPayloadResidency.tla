-------------------- MODULE TransportPayloadResidency --------------------
EXTENDS Naturals, FiniteSets, Apalache

CONSTANTS
    \* @type: Set(Str);
    Payloads,
    \* @type: Set(Str);
    Peers,
    \* @type: Int;
    MaxDecodedBytes,
    \* @type: Int;
    MaxWireBytes,
    \* @type: Int;
    ByteCap,
    \* @type: Int;
    ItemCap,
    \* @type: Bool;
    ByteBounded,
    \* @type: Bool;
    ChargeCompressedWire,
    \* @type: Bool;
    LazyChunks,
    \* @type: Bool;
    ReportSuccessOnEnqueue

ASSUME Payloads # {} /\ Peers # {}
ASSUME MaxDecodedBytes >= 1 /\ MaxWireBytes >= 1
ASSUME ByteCap >= 1 /\ ItemCap >= 1

Phases == {"absent", "resident", "done", "cancelled", "rejected"}
TerminalPhases == {"done", "cancelled", "rejected"}

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Int;
    decodedBytes,
    \* @type: Str -> Int;
    wireBytes,
    \* @type: Str -> Bool;
    compressed,
    \* @type: Str -> Set(Str);
    pendingPeers,
    \* @type: Str -> Set(Str);
    activePeers,
    \* @type: Int;
    budgetBytes,
    \* @type: Int;
    budgetItems,
    \* @type: Set(Str);
    reportedSuccess

vars == <<phase, decodedBytes, wireBytes, compressed, pendingPeers,
          activePeers, budgetBytes, budgetItems, reportedSuccess>>

Live == {payload \in Payloads : phase[payload] = "resident"}

\* @type: Str => Int;
ChargedCost(payload) ==
    decodedBytes[payload]
        + (IF compressed[payload] /\ ChargeCompressedWire
           THEN wireBytes[payload]
           ELSE 0)

\* @type: Str => Int;
ActualCost(payload) ==
    decodedBytes[payload]
        + (IF compressed[payload] THEN wireBytes[payload] ELSE 0)
        + (IF compressed[payload] /\ ~LazyChunks THEN wireBytes[payload] ELSE 0)

\* @type: (Int, Str) => Int;
AddChargedCost(total, payload) == total + ChargedCost(payload)

\* @type: (Int, Str) => Int;
AddActualCost(total, payload) == total + ActualCost(payload)

ChargedResidency == ApaFoldSet(AddChargedCost, 0, Live)
ActualResidency == ApaFoldSet(AddActualCost, 0, Live)

Init ==
    /\ phase = [payload \in Payloads |-> "absent"]
    /\ decodedBytes = [payload \in Payloads |-> 0]
    /\ wireBytes = [payload \in Payloads |-> 0]
    /\ compressed = [payload \in Payloads |-> FALSE]
    /\ pendingPeers = [payload \in Payloads |-> {}]
    /\ activePeers = [payload \in Payloads |-> {}]
    /\ budgetBytes = 0
    /\ budgetItems = 0
    /\ reportedSuccess = {}

Prepare(payload, decoded, wire, isCompressed, targets) ==
    /\ phase[payload] = "absent"
    /\ decoded \in 1..MaxDecodedBytes
    /\ isCompressed \in BOOLEAN
    /\ IF isCompressed THEN wire \in 1..MaxWireBytes ELSE wire = 0
    /\ targets \in SUBSET Peers
    /\ targets # {}
    /\ LET charged == decoded
                         + (IF isCompressed /\ ChargeCompressedWire THEN wire ELSE 0)
           fits == budgetItems < ItemCap
                    /\ (~ByteBounded \/ budgetBytes + charged <= ByteCap)
       IN /\ phase' = [phase EXCEPT ![payload] = IF fits THEN "resident" ELSE "rejected"]
          /\ decodedBytes' = [decodedBytes EXCEPT ![payload] = decoded]
          /\ wireBytes' = [wireBytes EXCEPT ![payload] = wire]
          /\ compressed' = [compressed EXCEPT ![payload] = isCompressed]
          /\ pendingPeers' = [pendingPeers EXCEPT ![payload] = IF fits THEN targets ELSE {}]
          /\ activePeers' = activePeers
          /\ budgetBytes' = IF fits THEN budgetBytes + charged ELSE budgetBytes
          /\ budgetItems' = IF fits THEN budgetItems + 1 ELSE budgetItems
          /\ reportedSuccess' =
                IF fits /\ ReportSuccessOnEnqueue
                THEN reportedSuccess \cup {payload}
                ELSE reportedSuccess

StartDelivery(payload, peer) ==
    /\ phase[payload] = "resident"
    /\ peer \in pendingPeers[payload]
    /\ pendingPeers' = [pendingPeers EXCEPT ![payload] = @ \ {peer}]
    /\ activePeers' = [activePeers EXCEPT ![payload] = @ \cup {peer}]
    /\ UNCHANGED <<phase, decodedBytes, wireBytes, compressed,
                    budgetBytes, budgetItems, reportedSuccess>>

FinishDelivery(payload, peer) ==
    /\ phase[payload] = "resident"
    /\ peer \in activePeers[payload]
    /\ LET remaining == activePeers[payload] \ {peer}
           finished == remaining = {} /\ pendingPeers[payload] = {}
       IN /\ activePeers' = [activePeers EXCEPT ![payload] = remaining]
          /\ phase' = [phase EXCEPT ![payload] = IF finished THEN "done" ELSE @]
          /\ budgetBytes' = IF finished
                            THEN budgetBytes - ChargedCost(payload)
                            ELSE budgetBytes
          /\ budgetItems' = IF finished THEN budgetItems - 1 ELSE budgetItems
          /\ reportedSuccess' = IF finished
                                 THEN reportedSuccess \cup {payload}
                                 ELSE reportedSuccess
    /\ UNCHANGED <<decodedBytes, wireBytes, compressed, pendingPeers>>

Cancel(payload) ==
    /\ phase[payload] = "resident"
    /\ phase' = [phase EXCEPT ![payload] = "cancelled"]
    /\ pendingPeers' = [pendingPeers EXCEPT ![payload] = {}]
    /\ activePeers' = [activePeers EXCEPT ![payload] = {}]
    /\ budgetBytes' = budgetBytes - ChargedCost(payload)
    /\ budgetItems' = budgetItems - 1
    /\ UNCHANGED <<decodedBytes, wireBytes, compressed, reportedSuccess>>

Quiescent ==
    /\ \A payload \in Payloads : phase[payload] \in TerminalPhases
    /\ UNCHANGED vars

Next ==
    \/ \E payload \in Payloads,
          decoded \in 1..MaxDecodedBytes,
          wire \in 0..MaxWireBytes,
          isCompressed \in BOOLEAN,
          targets \in SUBSET Peers :
             Prepare(payload, decoded, wire, isCompressed, targets)
    \/ \E payload \in Payloads, peer \in Peers : StartDelivery(payload, peer)
    \/ \E payload \in Payloads, peer \in Peers : FinishDelivery(payload, peer)
    \/ \E payload \in Payloads : Cancel(payload)
    \/ Quiescent

Fairness ==
    /\ \A payload \in Payloads, peer \in Peers : WF_vars(StartDelivery(payload, peer))
    /\ \A payload \in Payloads, peer \in Peers : WF_vars(FinishDelivery(payload, peer))

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ phase \in [Payloads -> Phases]
    /\ decodedBytes \in [Payloads -> 0..MaxDecodedBytes]
    /\ wireBytes \in [Payloads -> 0..MaxWireBytes]
    /\ compressed \in [Payloads -> BOOLEAN]
    /\ pendingPeers \in [Payloads -> SUBSET Peers]
    /\ activePeers \in [Payloads -> SUBSET Peers]
    /\ budgetBytes \in Nat
    /\ budgetItems \in Nat
    /\ reportedSuccess \subseteq Payloads
    /\ \A payload \in Payloads :
          /\ pendingPeers[payload] \cap activePeers[payload] = {}
          /\ phase[payload] # "resident" =>
                pendingPeers[payload] = {} /\ activePeers[payload] = {}
          /\ ~compressed[payload] => wireBytes[payload] = 0

Inv_ByteBudgetExact == budgetBytes = ChargedResidency
Inv_ItemBudgetExact == budgetItems = Cardinality(Live)
Inv_ByteCapacity == budgetBytes <= ByteCap
Inv_ItemCapacity == budgetItems <= ItemCap
Inv_ReservationCoversActual == ActualResidency <= budgetBytes
Inv_ActualResidencyBounded == ActualResidency <= ByteCap
Inv_FanoutSharesOneReservation == budgetItems = Cardinality(Live)
Inv_TerminalReleased ==
    \A payload \in Payloads :
        phase[payload] \in TerminalPhases => payload \notin Live
Inv_SuccessRequiresRemoteCompletion ==
    \A payload \in reportedSuccess : phase[payload] = "done"

Safety ==
    /\ TypeOK
    /\ Inv_ByteBudgetExact
    /\ Inv_ItemBudgetExact
    /\ Inv_ByteCapacity
    /\ Inv_ItemCapacity
    /\ Inv_ReservationCoversActual
    /\ Inv_ActualResidencyBounded
    /\ Inv_FanoutSharesOneReservation
    /\ Inv_TerminalReleased
    /\ Inv_SuccessRequiresRemoteCompletion

Live_EveryReservedPayloadReleases ==
    \A payload \in Payloads :
        (payload \in Live) ~> (phase[payload] \in {"done", "cancelled"})

=============================================================================
