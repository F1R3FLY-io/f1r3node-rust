---------------------- MODULE CertifiedSnapshotCapture ----------------------
EXTENDS Naturals, TLC

CONSTANT
  \* @type: Bool;
  UseRevisionCoherentCapture

ASSUME UseRevisionCoherentCapture \in BOOLEAN

Proposers == {"p1", "p2"}
Floors == {"G", "A"}
Certificates == {"CG", "CA"}
Phases == {"Idle", "HeadRead", "DagRead", "Retry", "Completed"}
NoRevision == 2
NoFloor == "NoFloor"
NoCertificate == "NoCertificate"

CertificateBinds(floor, certificate) ==
  \/ /\ floor = "G" /\ certificate = "CG"
  \/ /\ floor = "A" /\ certificate = "CA"

VARIABLES
  \* @type: Int;
  ledgerRevision,
  \* @type: Str;
  ledgerFloor,
  \* @type: Str;
  ledgerCertificate,
  \* @type: Int;
  dagRevision,
  \* @type: Str;
  dagFloor,
  \* @type: Str -> Str;
  phase,
  \* @type: Str -> Int;
  capturedHeadBefore,
  \* @type: Str -> Int;
  capturedDagRevision,
  \* @type: Str -> Str;
  capturedDagFloor,
  \* @type: Str -> Int;
  capturedHeadAfter,
  \* @type: Str -> Str;
  capturedCertificate,
  \* @type: Set(Str);
  signedSnapshots

vars == <<
  ledgerRevision,
  ledgerFloor,
  ledgerCertificate,
  dagRevision,
  dagFloor,
  phase,
  capturedHeadBefore,
  capturedDagRevision,
  capturedDagFloor,
  capturedHeadAfter,
  capturedCertificate,
  signedSnapshots
>>

Init ==
  /\ ledgerRevision = 0
  /\ ledgerFloor = "G"
  /\ ledgerCertificate = "CG"
  /\ dagRevision = 0
  /\ dagFloor = "G"
  /\ phase = [proposer \in Proposers |-> "Idle"]
  /\ capturedHeadBefore = [proposer \in Proposers |-> NoRevision]
  /\ capturedDagRevision = [proposer \in Proposers |-> NoRevision]
  /\ capturedDagFloor = [proposer \in Proposers |-> NoFloor]
  /\ capturedHeadAfter = [proposer \in Proposers |-> NoRevision]
  /\ capturedCertificate = [proposer \in Proposers |-> NoCertificate]
  /\ signedSnapshots = {}

AdvanceDurableHead ==
  /\ ledgerRevision = 0
  /\ ledgerRevision' = 1
  /\ ledgerFloor' = "A"
  /\ ledgerCertificate' = "CA"
  /\ UNCHANGED <<dagRevision, dagFloor, phase, capturedHeadBefore,
       capturedDagRevision, capturedDagFloor, capturedHeadAfter,
       capturedCertificate, signedSnapshots>>

ProjectDurableHeadIntoDag ==
  /\ dagRevision # ledgerRevision
  /\ dagRevision' = ledgerRevision
  /\ dagFloor' = ledgerFloor
  /\ UNCHANGED <<ledgerRevision, ledgerFloor, ledgerCertificate, phase,
       capturedHeadBefore, capturedDagRevision, capturedDagFloor,
       capturedHeadAfter, capturedCertificate, signedSnapshots>>

ReadHeadBefore(proposer) ==
  /\ proposer \in Proposers
  /\ phase[proposer] \in {"Idle", "Retry"}
  /\ phase' = [phase EXCEPT ![proposer] = "HeadRead"]
  /\ capturedHeadBefore' =
       [capturedHeadBefore EXCEPT ![proposer] = ledgerRevision]
  /\ capturedDagRevision' =
       [capturedDagRevision EXCEPT ![proposer] = NoRevision]
  /\ capturedDagFloor' =
       [capturedDagFloor EXCEPT ![proposer] = NoFloor]
  /\ capturedHeadAfter' =
       [capturedHeadAfter EXCEPT ![proposer] = NoRevision]
  /\ capturedCertificate' =
       [capturedCertificate EXCEPT ![proposer] = NoCertificate]
  /\ UNCHANGED <<ledgerRevision, ledgerFloor, ledgerCertificate,
       dagRevision, dagFloor, signedSnapshots>>

ReadDagProjection(proposer) ==
  /\ phase[proposer] = "HeadRead"
  /\ phase' = [phase EXCEPT ![proposer] = "DagRead"]
  /\ capturedDagRevision' =
       [capturedDagRevision EXCEPT ![proposer] = dagRevision]
  /\ capturedDagFloor' =
       [capturedDagFloor EXCEPT ![proposer] = dagFloor]
  /\ UNCHANGED <<ledgerRevision, ledgerFloor, ledgerCertificate,
       dagRevision, dagFloor, capturedHeadBefore, capturedHeadAfter,
       capturedCertificate, signedSnapshots>>

CapturedTupleIsCoherent(proposer) ==
  /\ capturedHeadBefore[proposer] = ledgerRevision
  /\ capturedDagRevision[proposer] = ledgerRevision
  /\ capturedDagFloor[proposer] = ledgerFloor
  /\ CertificateBinds(capturedDagFloor[proposer], ledgerCertificate)

ReadHeadAfter(proposer) ==
  /\ phase[proposer] = "DagRead"
  /\ capturedHeadAfter' =
       [capturedHeadAfter EXCEPT ![proposer] = ledgerRevision]
  /\ IF UseRevisionCoherentCapture /\ ~CapturedTupleIsCoherent(proposer)
     THEN
       /\ phase' = [phase EXCEPT ![proposer] = "Retry"]
       /\ capturedCertificate' =
            [capturedCertificate EXCEPT ![proposer] = NoCertificate]
       /\ UNCHANGED signedSnapshots
     ELSE
       /\ phase' = [phase EXCEPT ![proposer] = "Completed"]
       /\ capturedCertificate' =
            [capturedCertificate EXCEPT ![proposer] = ledgerCertificate]
       /\ signedSnapshots' = signedSnapshots \union {proposer}
  /\ UNCHANGED <<ledgerRevision, ledgerFloor, ledgerCertificate,
       dagRevision, dagFloor, capturedHeadBefore, capturedDagRevision,
       capturedDagFloor>>

Quiescent ==
  /\ ledgerRevision = 1
  /\ dagRevision = 1
  /\ \A proposer \in Proposers : phase[proposer] = "Completed"
  /\ UNCHANGED vars

Next ==
  \/ AdvanceDurableHead
  \/ ProjectDurableHeadIntoDag
  \/ \E proposer \in Proposers : ReadHeadBefore(proposer)
  \/ \E proposer \in Proposers : ReadDagProjection(proposer)
  \/ \E proposer \in Proposers : ReadHeadAfter(proposer)
  \/ Quiescent

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(AdvanceDurableHead)
  /\ WF_vars(ProjectDurableHeadIntoDag)
  /\ \A proposer \in Proposers :
       /\ WF_vars(ReadHeadBefore(proposer))
       /\ WF_vars(ReadDagProjection(proposer))
       /\ WF_vars(ReadHeadAfter(proposer))

TypeOK ==
  /\ ledgerRevision \in 0..1
  /\ ledgerFloor \in Floors
  /\ ledgerCertificate \in Certificates
  /\ dagRevision \in 0..1
  /\ dagFloor \in Floors
  /\ phase \in [Proposers -> Phases]
  /\ capturedHeadBefore \in [Proposers -> 0..NoRevision]
  /\ capturedDagRevision \in [Proposers -> 0..NoRevision]
  /\ capturedDagFloor \in [Proposers -> Floors \union {NoFloor}]
  /\ capturedHeadAfter \in [Proposers -> 0..NoRevision]
  /\ capturedCertificate \in
       [Proposers -> Certificates \union {NoCertificate}]
  /\ signedSnapshots \subseteq Proposers

CompletedSnapshotsBindOneRevision ==
  \A proposer \in signedSnapshots :
    /\ phase[proposer] = "Completed"
    /\ capturedHeadBefore[proposer] = capturedDagRevision[proposer]
    /\ capturedDagRevision[proposer] = capturedHeadAfter[proposer]
    /\ CertificateBinds(
         capturedDagFloor[proposer],
         capturedCertificate[proposer])

RetriesNeverSign ==
  \A proposer \in Proposers :
    phase[proposer] = "Retry" => proposer \notin signedSnapshots

CompletedSnapshotIsOldOrNew ==
  \A proposer \in signedSnapshots :
    \/ /\ capturedHeadAfter[proposer] = 0
          /\ capturedDagFloor[proposer] = "G"
          /\ capturedCertificate[proposer] = "CG"
    \/ /\ capturedHeadAfter[proposer] = 1
          /\ capturedDagFloor[proposer] = "A"
          /\ capturedCertificate[proposer] = "CA"

AllCapturesEventuallyComplete ==
  <> (\A proposer \in Proposers : phase[proposer] = "Completed")

=============================================================================
