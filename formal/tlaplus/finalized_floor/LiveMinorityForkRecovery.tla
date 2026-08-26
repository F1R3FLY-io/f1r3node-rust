---------------------- MODULE LiveMinorityForkRecovery ---------------------
EXTENDS FiniteSets, Integers, TLC

CONSTANT
    \* @type: Int;
    MaxHead,
    \* @type: Bool;
    UnsafeRemoteHeadMutation,
    \* @type: Bool;
    UnsafeAdmissionWithoutDependencies,
    \* @type: Bool;
    UnsafeGlobalProposalPause

ASSUME /\ MaxHead \in Nat \ {0}
       /\ UnsafeRemoteHeadMutation \in BOOLEAN
       /\ UnsafeAdmissionWithoutDependencies \in BOOLEAN
       /\ UnsafeGlobalProposalPause \in BOOLEAN

Nodes == {1, 2, 3}
Blocks == 0..MaxHead

\* @type: (Int) => Set(Int);
Prefix(block) == {dependency \in Blocks : dependency <= block}

VARIABLES
    \* @type: Int -> Set(Int);
    knownBlocks,
    \* @type: Int -> Set(Int);
    advertisedTips,
    \* @type: Int -> Int;
    requestedFinalization,
    \* @type: Int -> Int;
    completedFinalization,
    \* @type: Int -> Int;
    durableHead,
    \* @type: Int -> Int;
    appliedEffectsThrough,
    \* @type: Int -> Bool;
    proposalPaused,
    \* @type: Int -> Bool;
    remoteHeadMutation

vars == <<knownBlocks, advertisedTips, requestedFinalization,
          completedFinalization, durableHead, appliedEffectsThrough,
          proposalPaused, remoteHeadMutation>>

Init ==
    /\ knownBlocks = [node \in Nodes |-> {0}]
    /\ advertisedTips = [node \in Nodes |-> {}]
    /\ requestedFinalization = [node \in Nodes |-> 0]
    /\ completedFinalization = [node \in Nodes |-> 0]
    /\ durableHead = [node \in Nodes |-> 0]
    /\ appliedEffectsThrough = [node \in Nodes |-> 0]
    /\ proposalPaused = [node \in Nodes |-> FALSE]
    /\ remoteHeadMutation = [node \in Nodes |-> FALSE]

ProduceBlock(node) ==
    /\ Cardinality(knownBlocks[node]) - 1 < MaxHead
    /\ LET block == Cardinality(knownBlocks[node]) IN
       knownBlocks' = [knownBlocks EXCEPT ![node] = @ \cup {block}]
    /\ UNCHANGED <<advertisedTips, requestedFinalization,
                    completedFinalization, durableHead,
                    appliedEffectsThrough, proposalPaused,
                    remoteHeadMutation>>

AdvertiseTip(sender, receiver, block) ==
    /\ sender # receiver
    /\ block \in knownBlocks[sender]
    /\ advertisedTips' =
         [advertisedTips EXCEPT ![receiver] = @ \cup {block}]
    /\ UNCHANGED <<knownBlocks, requestedFinalization,
                    completedFinalization, durableHead,
                    appliedEffectsThrough, proposalPaused,
                    remoteHeadMutation>>

AdmitTipWithDependencies(receiver, block) ==
    /\ block \in advertisedTips[receiver]
    /\ knownBlocks' =
         [knownBlocks EXCEPT ![receiver] = @ \cup Prefix(block)]
    /\ UNCHANGED <<advertisedTips, requestedFinalization,
                    completedFinalization, durableHead,
                    appliedEffectsThrough, proposalPaused,
                    remoteHeadMutation>>

RequestLocalFinalization(node) ==
    /\ requestedFinalization[node] < MaxHead
    /\ requestedFinalization' =
         [requestedFinalization EXCEPT ![node] = @ + 1]
    /\ UNCHANGED <<knownBlocks, advertisedTips, completedFinalization,
                    durableHead, appliedEffectsThrough, proposalPaused,
                    remoteHeadMutation>>

RunLocalFinalizer(node) ==
    /\ completedFinalization[node] < requestedFinalization[node]
    /\ LET target == CHOOSE block \in knownBlocks[node] :
                       \A other \in knownBlocks[node] : other <= block
       IN
       /\ durableHead' = [durableHead EXCEPT ![node] = target]
       /\ appliedEffectsThrough' =
            [appliedEffectsThrough EXCEPT ![node] = target]
    /\ completedFinalization' =
         [completedFinalization EXCEPT ![node] = requestedFinalization[node]]
    /\ UNCHANGED <<knownBlocks, advertisedTips, requestedFinalization,
                    proposalPaused, remoteHeadMutation>>

UnsafeRemoteInstall(sender, receiver) ==
    /\ UnsafeRemoteHeadMutation
    /\ sender # receiver
    /\ durableHead[sender] > durableHead[receiver]
    /\ durableHead' =
         [durableHead EXCEPT ![receiver] = durableHead[sender]]
    /\ remoteHeadMutation' = [remoteHeadMutation EXCEPT ![receiver] = TRUE]
    /\ UNCHANGED <<knownBlocks, advertisedTips, requestedFinalization,
                    completedFinalization, appliedEffectsThrough,
                    proposalPaused>>

UnsafeAdmitWithoutDependencies(receiver, block) ==
    /\ UnsafeAdmissionWithoutDependencies
    /\ block \in advertisedTips[receiver]
    /\ block > 0
    /\ knownBlocks' = [knownBlocks EXCEPT ![receiver] = {block}]
    /\ UNCHANGED <<advertisedTips, requestedFinalization,
                    completedFinalization, durableHead,
                    appliedEffectsThrough, proposalPaused,
                    remoteHeadMutation>>

UnsafePauseAll(node) ==
    /\ UnsafeGlobalProposalPause
    /\ proposalPaused' = [other \in Nodes |-> TRUE]
    /\ UNCHANGED <<knownBlocks, advertisedTips, requestedFinalization,
                    completedFinalization, durableHead,
                    appliedEffectsThrough, remoteHeadMutation>>

Next ==
    \/ \E node \in Nodes : ProduceBlock(node)
    \/ \E sender, receiver \in Nodes, block \in Blocks :
         AdvertiseTip(sender, receiver, block)
    \/ \E receiver \in Nodes, block \in Blocks :
         AdmitTipWithDependencies(receiver, block)
    \/ \E node \in Nodes : RequestLocalFinalization(node)
    \/ \E node \in Nodes : RunLocalFinalizer(node)
    \/ \E sender, receiver \in Nodes : UnsafeRemoteInstall(sender, receiver)
    \/ \E receiver \in Nodes, block \in Blocks :
         UnsafeAdmitWithoutDependencies(receiver, block)
    \/ \E node \in Nodes : UnsafePauseAll(node)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ knownBlocks \in [Nodes -> SUBSET Blocks]
    /\ advertisedTips \in [Nodes -> SUBSET Blocks]
    /\ requestedFinalization \in [Nodes -> 0..MaxHead]
    /\ completedFinalization \in [Nodes -> 0..MaxHead]
    /\ durableHead \in [Nodes -> Blocks]
    /\ appliedEffectsThrough \in [Nodes -> Blocks]
    /\ proposalPaused \in [Nodes -> BOOLEAN]
    /\ remoteHeadMutation \in [Nodes -> BOOLEAN]

Inv_GenesisRetained == \A node \in Nodes : 0 \in knownBlocks[node]
Inv_DependencyClosure ==
    \A node \in Nodes :
      \A block \in knownBlocks[node] : Prefix(block) \subseteq knownBlocks[node]
Inv_LocalHeadKnown ==
    \A node \in Nodes : durableHead[node] \in knownBlocks[node]
Inv_LocalEffectsAtomic == durableHead = appliedEffectsThrough
Inv_RemoteAdviceCannotPublish ==
    \A node \in Nodes : ~remoteHeadMutation[node]
Inv_RecoveryDoesNotGloballyPause ==
    \A node \in Nodes : ~proposalPaused[node]
Inv_FinalizerCoverageMonotone ==
    \A node \in Nodes : completedFinalization[node] <= requestedFinalization[node]

Safety ==
    /\ TypeOK
    /\ Inv_GenesisRetained
    /\ Inv_DependencyClosure
    /\ Inv_LocalHeadKnown
    /\ Inv_LocalEffectsAtomic
    /\ Inv_RemoteAdviceCannotPublish
    /\ Inv_RecoveryDoesNotGloballyPause
    /\ Inv_FinalizerCoverageMonotone

=============================================================================
