---------------- MODULE PhysicalSettlementWorklist ----------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS Deployments, EventCount, NativeStackLimit, UseRecursive

ASSUME /\ Deployments # {}
       /\ EventCount \in Nat \ {0}
       /\ NativeStackLimit \in Nat
       /\ UseRecursive \in BOOLEAN

VARIABLES worklists, results, completed, nativeDepth, recursivePaths

vars == <<worklists, results, completed, nativeDepth, recursivePaths>>

WinningPath == [index \in 1..EventCount |-> 1]

SearchNode(depth, path) == [depth |-> depth, path |-> path]

Init ==
    /\ worklists =
         [deployment \in Deployments |-> <<SearchNode(0, <<>>)>>]
    /\ results = [deployment \in Deployments |-> <<>>]
    /\ completed = [deployment \in Deployments |-> FALSE]
    /\ nativeDepth = [deployment \in Deployments |-> 0]
    /\ recursivePaths = [deployment \in Deployments |-> <<>>]

WorklistStep(deployment) ==
    /\ ~completed[deployment]
    /\ Len(worklists[deployment]) > 0
    /\ LET queue == worklists[deployment]
           node == Head(queue)
       IN CASE node.depth = EventCount /\ node.path = WinningPath ->
                 /\ worklists' =
                      [worklists EXCEPT ![deployment] = Tail(queue)]
                 /\ results' =
                      [results EXCEPT ![deployment] = node.path]
                 /\ completed' =
                      [completed EXCEPT ![deployment] = TRUE]
          [] node.depth = EventCount ->
                 /\ worklists' =
                      [worklists EXCEPT ![deployment] = Tail(queue)]
                 /\ UNCHANGED <<results, completed>>
          [] OTHER ->
                 /\ worklists' =
                      [worklists EXCEPT ![deployment] =
                         <<SearchNode(node.depth + 1, Append(node.path, 0)),
                           SearchNode(node.depth + 1, Append(node.path, 1))>>
                         \o Tail(queue)]
                 /\ UNCHANGED <<results, completed>>
    /\ UNCHANGED <<nativeDepth, recursivePaths>>

RecursiveStep(deployment) ==
    /\ ~completed[deployment]
    /\ LET nextPath == Append(recursivePaths[deployment], 1)
       IN /\ recursivePaths' =
                [recursivePaths EXCEPT ![deployment] = nextPath]
          /\ nativeDepth' =
                [nativeDepth EXCEPT ![deployment] = @ + 1]
          /\ completed' =
                [completed EXCEPT
                   ![deployment] = Len(nextPath) = EventCount]
          /\ results' =
                [results EXCEPT
                   ![deployment] =
                     IF Len(nextPath) = EventCount THEN nextPath ELSE @]
    /\ UNCHANGED worklists

Progress(deployment) ==
    IF UseRecursive
    THEN RecursiveStep(deployment)
    ELSE WorklistStep(deployment)

Next == \E deployment \in Deployments : Progress(deployment)

Spec == /\ Init
        /\ [][Next]_vars
        /\ \A deployment \in Deployments : WF_vars(Progress(deployment))

TypeOK ==
    /\ worklists \in
         [Deployments ->
           Seq([depth : 0..EventCount, path : Seq({0, 1})])]
    /\ results \in [Deployments -> Seq({0, 1})]
    /\ completed \in [Deployments -> BOOLEAN]
    /\ nativeDepth \in [Deployments -> Nat]
    /\ recursivePaths \in [Deployments -> Seq({0, 1})]

NativeStackBound ==
    \A deployment \in Deployments :
      nativeDepth[deployment] <= NativeStackLimit

WorklistUsesNoNativeRecursion ==
    ~UseRecursive =>
      \A deployment \in Deployments : nativeDepth[deployment] = 0

CompletedResultMatchesReference ==
    \A deployment \in Deployments :
      completed[deployment] => results[deployment] = WinningPath

SearchNodesStayWithinTheFiniteTree ==
    \A deployment \in Deployments :
      \A index \in DOMAIN worklists[deployment] :
        LET node == worklists[deployment][index]
        IN /\ node.depth = Len(node.path)
           /\ node.depth <= EventCount

EventuallyAllDeploymentsComplete ==
    <> (\A deployment \in Deployments : completed[deployment])

=============================================================================
