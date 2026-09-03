---------------- MODULE MergeableEvidenceAuthentication ----------------
EXTENDS Sequences

CONSTANT
    \* @type: Str;
    Defect

ASSUME Defect \in {"None", "LegacyKeyAliasing", "TrustPeerEvidence", "VacuousLatestRetirement", "MainSpineOnlyRetirement"}

Validators == {"V1", "V2"}
Blocks == {"A", "B"}
Keys == {"Legacy", "KeyA", "KeyB"}
EvidenceValues == {"None", "EvidenceA", "EvidenceB", "Forged"}
BlockOrders == {<<>>, <<"A">>, <<"B">>, <<"A", "B">>, <<"B", "A">>}

CanonicalKey(block) == IF block = "A" THEN "KeyA" ELSE "KeyB"
Key(block) == IF Defect = "LegacyKeyAliasing" THEN "Legacy" ELSE CanonicalKey(block)
CanonicalEvidence(block) == IF block = "A" THEN "EvidenceA" ELSE "EvidenceB"
InsertEvidence(store, block) == [store EXCEPT ![Key(block)] = CanonicalEvidence(block)]
DeleteEvidence(store, block) == [store EXCEPT ![Key(block)] = "None"]
RetirementEligible(finalized, beyondHorizon, childrenKnown, children, latestMessages, advancedMessages) ==
    /\ finalized
    /\ beyondHorizon
    /\ childrenKnown
    /\ children # {}
    /\ IF Defect = "VacuousLatestRetirement"
       THEN latestMessages \subseteq advancedMessages
       ELSE /\ latestMessages # {}
            /\ latestMessages \subseteq advancedMessages

VARIABLES
    \* @type: (Str -> Set(Str));
    replayed,
    \* @type: (Str -> Seq(Str));
    replayOrder,
    \* @type: Set(<<Str, Str>>);
    peerReceived,
    \* @type: (Str -> (Str -> Str));
    stores

vars == <<replayed, replayOrder, peerReceived, stores>>

EmptyStore == [key \in Keys |-> "None"]

Init ==
    /\ replayed = [validator \in Validators |-> {}]
    /\ replayOrder = [validator \in Validators |-> <<>>]
    /\ peerReceived = {}
    /\ stores = [validator \in Validators |-> EmptyStore]

Replay(validator, block) ==
    /\ validator \in Validators
    /\ block \in Blocks
    /\ block \notin replayed[validator]
    /\ replayed' = [replayed EXCEPT ![validator] = @ \cup {block}]
    /\ replayOrder' = [replayOrder EXCEPT ![validator] = Append(@, block)]
    /\ stores' = [stores EXCEPT ![validator] = InsertEvidence(@, block)]
    /\ UNCHANGED peerReceived

ReceivePeer(validator, block) ==
    /\ validator \in Validators
    /\ block \in Blocks
    /\ <<validator, block>> \notin peerReceived
    /\ peerReceived' = peerReceived \cup {<<validator, block>>}
    /\ IF Defect = "TrustPeerEvidence"
       THEN stores' = [stores EXCEPT ![validator][Key(block)] = "Forged"]
       ELSE UNCHANGED stores
    /\ UNCHANGED <<replayed, replayOrder>>

Complete ==
    /\ \A validator \in Validators : replayed[validator] = Blocks
    /\ peerReceived = Validators \X Blocks

TerminalStutter ==
    /\ Complete
    /\ UNCHANGED vars

Next ==
    \/ \E validator \in Validators, block \in Blocks : Replay(validator, block)
    \/ \E validator \in Validators, block \in Blocks : ReceivePeer(validator, block)
    \/ TerminalStutter

TypeOK ==
    /\ replayed \in [Validators -> SUBSET Blocks]
    /\ replayOrder \in [Validators -> BlockOrders]
    /\ peerReceived \subseteq Validators \X Blocks
    /\ stores \in [Validators -> [Keys -> EvidenceValues]]

CompleteKeySeparatesEquivocations ==
    Key("A") # Key("B")

LocallyDerivedEvidenceOnly ==
    \A validator \in Validators, key \in Keys :
        stores[validator][key] # "None" =>
            \E block \in replayed[validator] :
                /\ Key(block) = key
                /\ CanonicalEvidence(block) = stores[validator][key]

ExactLocalReplayLookup ==
    \A validator \in Validators :
        \A block \in replayed[validator] :
            stores[validator][Key(block)] = CanonicalEvidence(block)

OppositeArrivalOrdersConverge ==
    /\ Len(replayOrder["V1"]) = 2
    /\ Len(replayOrder["V2"]) = 2
    /\ replayOrder["V1"] # replayOrder["V2"]
    => \A block \in Blocks :
        /\ stores["V1"][Key(block)] = CanonicalEvidence(block)
        /\ stores["V2"][Key(block)] = CanonicalEvidence(block)
        /\ stores["V1"][Key(block)] = stores["V2"][Key(block)]

PeerInputCannotOverwriteReplay ==
    \A validator \in Validators :
        \A block \in replayed[validator] :
            stores[validator][Key(block)] = CanonicalEvidence(block)

DeletionRemovesExactExecution ==
    \A validator \in Validators :
        DeleteEvidence(stores[validator], "A")[Key("A")] = "None"

DeletionPreservesDistinctExecution ==
    \A validator \in Validators :
        stores[validator][Key("B")] = CanonicalEvidence("B") =>
            DeleteEvidence(stores[validator], "A")[Key("B")] = CanonicalEvidence("B")

DeletionIsIdempotent ==
    \A validator \in Validators :
        DeleteEvidence(DeleteEvidence(stores[validator], "A"), "A") =
            DeleteEvidence(stores[validator], "A")

DeletionCommutesWithDistinctReplay ==
    \A validator \in Validators :
        DeleteEvidence(InsertEvidence(stores[validator], "B"), "A") =
            InsertEvidence(DeleteEvidence(stores[validator], "A"), "B")

RetirementRequiresEverySafetyGuard ==
    \A finalized \in BOOLEAN,
       beyondHorizon \in BOOLEAN,
       childrenKnown \in BOOLEAN,
       children \in SUBSET Blocks,
       latestMessages \in SUBSET Blocks,
       advancedMessages \in SUBSET Blocks :
        RetirementEligible(
            finalized,
            beyondHorizon,
            childrenKnown,
            children,
            latestMessages,
            advancedMessages
        ) =>
            /\ finalized
            /\ beyondHorizon
            /\ childrenKnown
            /\ children # {}
            /\ latestMessages # {}
            /\ latestMessages \subseteq advancedMessages

SecondaryParentMessages == {"B"}

RecognizedSecondaryParentAdvancement ==
    IF Defect = "MainSpineOnlyRetirement" THEN {} ELSE SecondaryParentMessages

SecondaryParentRetirementComplete ==
    RetirementEligible(
        TRUE,
        TRUE,
        TRUE,
        {"A"},
        SecondaryParentMessages,
        RecognizedSecondaryParentAdvancement
    )

=============================================================================
