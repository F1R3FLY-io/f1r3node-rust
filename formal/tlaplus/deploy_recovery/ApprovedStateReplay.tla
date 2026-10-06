-------------------------- MODULE ApprovedStateReplay --------------------------
EXTENDS Naturals

CONSTANT BlockBoundReplay

Genesis == 0
Approved == 2
Blocks == 0..Approved
NoContext == 3
NoRoot == 4
CorruptRoot == 5
Contexts == Blocks \union {NoContext}
Roots == 0..CorruptRoot

ConsensusContext(block) == block
DeclaredPostRoot(block) == block + 1

ReplayResult(block, context) ==
    IF context = ConsensusContext(block)
    THEN DeclaredPostRoot(block)
    ELSE CorruptRoot

VARIABLES
    phase,
    nextBlock,
    usedContext,
    installedRoot,
    invalidated

vars == <<phase, nextBlock, usedContext, installedRoot, invalidated>>

Init ==
    /\ phase = "restore"
    /\ nextBlock = Genesis
    /\ usedContext = [block \in Blocks |-> NoContext]
    /\ installedRoot = [block \in Blocks |-> NoRoot]
    /\ invalidated = {}

RestoreApproved ==
    /\ phase = "restore"
    /\ phase' = "replay"
    /\ installedRoot' =
        [installedRoot EXCEPT ![Approved] = DeclaredPostRoot(Approved)]
    /\ UNCHANGED <<nextBlock, usedContext, invalidated>>

SelectedContext(block) ==
    IF BlockBoundReplay
    THEN ConsensusContext(block)
    ELSE ConsensusContext(Approved)

ReplayHistoricalBlock ==
    /\ phase = "replay"
    /\ nextBlock \in Blocks
    /\ LET block == nextBlock
           context == SelectedContext(block)
           result == ReplayResult(block, context)
       IN /\ usedContext' = [usedContext EXCEPT ![block] = context]
          /\ installedRoot' = [installedRoot EXCEPT ![block] = result]
          /\ invalidated' =
                IF result = DeclaredPostRoot(block)
                THEN invalidated
                ELSE invalidated \union {block}
          /\ nextBlock' = block + 1
          /\ phase' =
                IF result # DeclaredPostRoot(block)
                THEN "failed"
                ELSE IF block = Approved THEN "running" ELSE "replay"

Next == RestoreApproved \/ ReplayHistoricalBlock

Spec ==
    Init
    /\ [][Next]_vars
    /\ WF_vars(RestoreApproved)
    /\ WF_vars(ReplayHistoricalBlock)

TypeOK ==
    /\ phase \in {"restore", "replay", "failed", "running"}
    /\ nextBlock \in 0..(Approved + 1)
    /\ usedContext \in [Blocks -> Contexts]
    /\ installedRoot \in [Blocks -> Roots]
    /\ invalidated \subseteq Blocks

Inv_ReplayUsesConsensusContext ==
    \A block \in Blocks :
        usedContext[block] = NoContext
        \/ usedContext[block] = ConsensusContext(block)

Inv_InstalledRootsAreDeclared ==
    \A block \in Blocks :
        installedRoot[block] = NoRoot
        \/ installedRoot[block] = DeclaredPostRoot(block)

Inv_ValidHistoryNeverInvalidated == invalidated = {}

Inv_RunningHasCompleteHistory ==
    phase = "running"
    => \A block \in Blocks :
        installedRoot[block] = DeclaredPostRoot(block)

Live_ReachesRunning == <> (phase = "running")
=============================================================================
