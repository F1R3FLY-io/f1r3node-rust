--------------------- MODULE ParallelStackMaterialization ---------------------
EXTENDS Naturals, Sequences, TLC

CONSTANT
  \* @type: Bool;
  UnsafeSiblingScheduling

ASSUME UnsafeSiblingScheduling \in BOOLEAN

\* @type: Seq(Str);
Certificate == <<"InitialDeclaration", "ParentReduction", "NestedDeclaration">>

VARIABLES
  \* @type: Str;
  phase,
  \* @type: Int;
  payer,
  \* @type: Int;
  initialPurse,
  \* @type: Int;
  nestedPurse,
  \* @type: Int;
  burned,
  \* @type: Bool;
  initialDeclared,
  \* @type: Bool;
  parentCommitted,
  \* @type: Bool;
  nestedDeclared,
  \* @type: Bool;
  rejected,
  \* @type: Seq(Str);
  trace,
  \* @type: Int;
  replayPayer,
  \* @type: Int;
  replayInitialPurse,
  \* @type: Int;
  replayNestedPurse,
  \* @type: Int;
  replayBurned,
  \* @type: Bool;
  replayInitialDeclared,
  \* @type: Bool;
  replayParentCommitted,
  \* @type: Bool;
  replayNestedDeclared,
  \* @type: Seq(Str);
  replayTrace

vars == <<phase,
          payer,
          initialPurse,
          nestedPurse,
          burned,
          initialDeclared,
          parentCommitted,
          nestedDeclared,
          rejected,
          trace,
          replayPayer,
          replayInitialPurse,
          replayNestedPurse,
          replayBurned,
          replayInitialDeclared,
          replayParentCommitted,
          replayNestedDeclared,
          replayTrace>>

Init ==
    /\ phase = "Evaluate"
    /\ payer = 4
    /\ initialPurse = 0
    /\ nestedPurse = 0
    /\ burned = 0
    /\ initialDeclared = FALSE
    /\ parentCommitted = FALSE
    /\ nestedDeclared = FALSE
    /\ rejected = FALSE
    /\ trace = <<>>
    /\ replayPayer = 4
    /\ replayInitialPurse = 0
    /\ replayNestedPurse = 0
    /\ replayBurned = 0
    /\ replayInitialDeclared = FALSE
    /\ replayParentCommitted = FALSE
    /\ replayNestedDeclared = FALSE
    /\ replayTrace = <<>>

MaterializeInitial ==
    /\ phase = "Evaluate"
    /\ ~initialDeclared
    /\ payer >= 2
    /\ payer' = payer - 2
    /\ initialPurse' = initialPurse + 2
    /\ initialDeclared' = TRUE
    /\ trace' = Append(trace, "InitialDeclaration")
    /\ UNCHANGED <<phase, nestedPurse, burned, parentCommitted,
                    nestedDeclared, rejected, replayPayer,
                    replayInitialPurse, replayNestedPurse, replayBurned,
                    replayInitialDeclared, replayParentCommitted,
                    replayNestedDeclared, replayTrace>>

RunParent ==
    /\ phase = "Evaluate"
    /\ initialDeclared
    /\ ~parentCommitted
    /\ payer >= 1
    /\ initialPurse >= 1
    /\ payer' = payer - 1
    /\ initialPurse' = initialPurse - 1
    /\ burned' = burned + 2
    /\ parentCommitted' = TRUE
    /\ trace' = Append(trace, "ParentReduction")
    /\ UNCHANGED <<phase, nestedPurse, initialDeclared, nestedDeclared,
                    rejected, replayPayer, replayInitialPurse,
                    replayNestedPurse, replayBurned, replayInitialDeclared,
                    replayParentCommitted, replayNestedDeclared, replayTrace>>

MaterializeNested ==
    /\ phase = "Evaluate"
    /\ parentCommitted
    /\ ~nestedDeclared
    /\ payer >= 1
    /\ payer' = payer - 1
    /\ nestedPurse' = nestedPurse + 1
    /\ nestedDeclared' = TRUE
    /\ trace' = Append(trace, "NestedDeclaration")
    /\ UNCHANGED <<phase, initialPurse, burned, initialDeclared,
                    parentCommitted, rejected, replayPayer,
                    replayInitialPurse, replayNestedPurse, replayBurned,
                    replayInitialDeclared, replayParentCommitted,
                    replayNestedDeclared, replayTrace>>

BeginReplay ==
    /\ phase = "Evaluate"
    /\ initialDeclared
    /\ parentCommitted
    /\ nestedDeclared
    /\ phase' = "Replay"
    /\ UNCHANGED <<payer, initialPurse, nestedPurse, burned,
                    initialDeclared, parentCommitted, nestedDeclared,
                    rejected, trace, replayPayer, replayInitialPurse,
                    replayNestedPurse, replayBurned, replayInitialDeclared,
                    replayParentCommitted, replayNestedDeclared, replayTrace>>

ReplayInitial ==
    /\ phase = "Replay"
    /\ ~replayInitialDeclared
    /\ replayPayer >= 2
    /\ replayPayer' = replayPayer - 2
    /\ replayInitialPurse' = replayInitialPurse + 2
    /\ replayInitialDeclared' = TRUE
    /\ replayTrace' = Append(replayTrace, "InitialDeclaration")
    /\ UNCHANGED <<phase, payer, initialPurse, nestedPurse, burned,
                    initialDeclared, parentCommitted, nestedDeclared,
                    rejected, trace, replayNestedPurse, replayBurned,
                    replayParentCommitted, replayNestedDeclared>>

ReplayParent ==
    /\ phase = "Replay"
    /\ replayInitialDeclared
    /\ ~replayParentCommitted
    /\ replayPayer >= 1
    /\ replayInitialPurse >= 1
    /\ replayPayer' = replayPayer - 1
    /\ replayInitialPurse' = replayInitialPurse - 1
    /\ replayBurned' = replayBurned + 2
    /\ replayParentCommitted' = TRUE
    /\ replayTrace' = Append(replayTrace, "ParentReduction")
    /\ UNCHANGED <<phase, payer, initialPurse, nestedPurse, burned,
                    initialDeclared, parentCommitted, nestedDeclared,
                    rejected, trace, replayNestedPurse,
                    replayInitialDeclared, replayNestedDeclared>>

ReplayNested ==
    /\ phase = "Replay"
    /\ replayParentCommitted
    /\ ~replayNestedDeclared
    /\ replayPayer >= 1
    /\ replayPayer' = replayPayer - 1
    /\ replayNestedPurse' = replayNestedPurse + 1
    /\ replayNestedDeclared' = TRUE
    /\ replayTrace' = Append(replayTrace, "NestedDeclaration")
    /\ phase' = "Done"
    /\ UNCHANGED <<payer, initialPurse, nestedPurse, burned,
                    initialDeclared, parentCommitted, nestedDeclared,
                    rejected, trace, replayInitialPurse, replayBurned,
                    replayInitialDeclared, replayParentCommitted>>

RejectPrematureSibling ==
    /\ phase = "Evaluate"
    /\ UnsafeSiblingScheduling
    /\ ~initialDeclared
    /\ ~parentCommitted
    /\ rejected' = TRUE
    /\ phase' = "Done"
    /\ UNCHANGED <<payer, initialPurse, nestedPurse, burned,
                    initialDeclared, parentCommitted, nestedDeclared, trace,
                    replayPayer, replayInitialPurse, replayNestedPurse,
                    replayBurned, replayInitialDeclared, replayParentCommitted,
                    replayNestedDeclared, replayTrace>>

Next ==
    \/ MaterializeInitial
    \/ RunParent
    \/ MaterializeNested
    \/ BeginReplay
    \/ ReplayInitial
    \/ ReplayParent
    \/ ReplayNested
    \/ RejectPrematureSibling

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"Evaluate", "Replay", "Done"}
    /\ payer \in 0..4
    /\ initialPurse \in 0..2
    /\ nestedPurse \in 0..1
    /\ burned \in 0..2
    /\ initialDeclared \in BOOLEAN
    /\ parentCommitted \in BOOLEAN
    /\ nestedDeclared \in BOOLEAN
    /\ rejected \in BOOLEAN
    /\ replayPayer \in 0..4
    /\ replayInitialPurse \in 0..2
    /\ replayNestedPurse \in 0..1
    /\ replayBurned \in 0..2
    /\ replayInitialDeclared \in BOOLEAN
    /\ replayParentCommitted \in BOOLEAN
    /\ replayNestedDeclared \in BOOLEAN
    /\ trace \in {<<>>, <<"InitialDeclaration">>,
                    <<"InitialDeclaration", "ParentReduction">>, Certificate}
    /\ replayTrace \in {<<>>, <<"InitialDeclaration">>,
                          <<"InitialDeclaration", "ParentReduction">>, Certificate}

MaterializationBarrier == parentCommitted => initialDeclared

NestedDeclarationFollowsParent == nestedDeclared => parentCommitted

ReplayMaterializationBarrier == replayParentCommitted => replayInitialDeclared

ReplayNestedDeclarationFollowsParent == replayNestedDeclared => replayParentCommitted

Conservation ==
    /\ payer + initialPurse + nestedPurse + burned = 4
    /\ replayPayer + replayInitialPurse + replayNestedPurse + replayBurned = 4

CausallyFundedProgramIsAccepted == ~rejected

ReplayAgreement ==
    phase = "Done" /\ ~rejected =>
        /\ trace = Certificate
        /\ replayTrace = trace
        /\ replayPayer = payer
        /\ replayInitialPurse = initialPurse
        /\ replayNestedPurse = nestedPurse
        /\ replayBurned = burned

Progress == <>(phase = "Done")

=============================================================================
