------------------ MODULE DivergentFinalizationHistories ------------------
EXTENDS Integers, TLC

CONSTANT
    \* @type: Bool;
    UnsafeImportRemoteLedger

ASSUME UnsafeImportRemoteLedger \in BOOLEAN

Nodes == {"stepped", "direct"}
Blocks == {0, 5, 10}
LedgerDigests == {0, 51, 101, 512}

VARIABLES
    \* @type: Str -> Int;
    head,
    \* @type: Str -> Int;
    revision,
    \* @type: Str -> Int;
    digest,
    \* @type: Str -> Bool;
    importedRemoteLedger

vars == <<head, revision, digest, importedRemoteLedger>>

Init ==
    /\ head = [node \in Nodes |-> 0]
    /\ revision = [node \in Nodes |-> 0]
    /\ digest = [node \in Nodes |-> 0]
    /\ importedRemoteLedger = [node \in Nodes |-> FALSE]

SteppedFinalizeFive ==
    /\ head["stepped"] = 0
    /\ head' = [head EXCEPT !["stepped"] = 5]
    /\ revision' = [revision EXCEPT !["stepped"] = 1]
    /\ digest' = [digest EXCEPT !["stepped"] = 51]
    /\ UNCHANGED importedRemoteLedger

SteppedFinalizeTen ==
    /\ head["stepped"] = 5
    /\ head' = [head EXCEPT !["stepped"] = 10]
    /\ revision' = [revision EXCEPT !["stepped"] = 2]
    /\ digest' = [digest EXCEPT !["stepped"] = 512]
    /\ UNCHANGED importedRemoteLedger

DirectFinalizeTen ==
    /\ head["direct"] = 0
    /\ head' = [head EXCEPT !["direct"] = 10]
    /\ revision' = [revision EXCEPT !["direct"] = 1]
    /\ digest' = [digest EXCEPT !["direct"] = 101]
    /\ UNCHANGED importedRemoteLedger

UnsafeImport(receiver, sender) ==
    /\ UnsafeImportRemoteLedger
    /\ receiver # sender
    /\ head[receiver] = head[sender]
    /\ head[receiver] = 10
    /\ \/ revision[receiver] # revision[sender]
       \/ digest[receiver] # digest[sender]
    /\ importedRemoteLedger' =
         [importedRemoteLedger EXCEPT ![receiver] = TRUE]
    /\ UNCHANGED <<head, revision, digest>>

Next ==
    \/ SteppedFinalizeFive
    \/ SteppedFinalizeTen
    \/ DirectFinalizeTen
    \/ \E receiver, sender \in Nodes : UnsafeImport(receiver, sender)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ head \in [Nodes -> Blocks]
    /\ revision \in [Nodes -> 0..2]
    /\ digest \in [Nodes -> LedgerDigests]
    /\ importedRemoteLedger \in [Nodes -> BOOLEAN]

Inv_GenesisIdentity ==
    \A node \in Nodes : head[node] = 0 => revision[node] = 0 /\ digest[node] = 0

Inv_SameTargetAllowsDifferentLocalLedgerIdentity ==
    head["stepped"] = 10 /\ head["direct"] = 10 =>
      /\ revision["stepped"] # revision["direct"]
      /\ digest["stepped"] # digest["direct"]

Inv_RemoteLedgerNeverInstalled ==
    \A node \in Nodes : ~importedRemoteLedger[node]

Safety ==
    /\ TypeOK
    /\ Inv_GenesisIdentity
    /\ Inv_SameTargetAllowsDifferentLocalLedgerIdentity
    /\ Inv_RemoteLedgerNeverInstalled

=============================================================================
