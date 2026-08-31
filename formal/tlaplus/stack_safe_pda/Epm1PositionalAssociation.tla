-------------------- MODULE Epm1PositionalAssociation --------------------
EXTENDS Naturals, Sequences, TLC

(***************************************************************************
Finite-state refinement of the EPM1 positional association protocol.

ACTree03 stores one zero marker per logical valued endpoint.  The generated
child PDA and an owned PathMap zipper then advance in lockstep: endpoint i is
paired with protobuf body i.  StorageAddresses deliberately contains a
repeated identity; it is observational data only and therefore cannot collapse
the third logical occurrence.
***************************************************************************)

Endpoints == <<"root", "a", "ab">>
Bodies == <<101, 102, 103>>
StorageAddresses == <<7, 9, 9>>
EndpointCount == Len(Endpoints)

Phases == {"scan", "await-child", "ready", "published", "failed"}
Errors == {"none", "child", "act"}
VisibleImages == {"old", "new"}

VARIABLES phase, cursor, pending, assignments, markers, publications,
          visible, childFailed, deferredActFailed, selectedError

vars == <<phase, cursor, pending, assignments, markers, publications,
          visible, childFailed, deferredActFailed, selectedError>>

Initial(markerImage, initialCursor, initialAssignments) ==
    /\ phase = "scan"
    /\ cursor = initialCursor
    /\ pending = 0
    /\ assignments = initialAssignments
    /\ markers = markerImage
    /\ publications = 0
    /\ visible = "old"
    /\ childFailed = FALSE
    /\ deferredActFailed = FALSE
    /\ selectedError = "none"

Init == Initial(<<0, 0, 0>>, 0, <<>>)

YieldEndpoint ==
    /\ phase = "scan"
    /\ cursor < EndpointCount
    /\ pending' = cursor + 1
    /\ phase' = "await-child"
    /\ UNCHANGED <<cursor, assignments, markers, publications, visible,
                    childFailed, deferredActFailed, selectedError>>

AttachBody ==
    /\ phase = "await-child"
    /\ pending = cursor + 1
    /\ pending <= EndpointCount
    /\ assignments' = Append(assignments,
          [endpoint |-> Endpoints[pending], body |-> Bodies[pending]])
    /\ cursor' = pending
    /\ pending' = 0
    /\ phase' = IF cursor' = EndpointCount THEN "ready" ELSE "scan"
    /\ UNCHANGED <<markers, publications, visible, childFailed,
                    deferredActFailed, selectedError>>

FailChildAndDeferred ==
    /\ phase = "await-child"
    /\ pending = 2
    /\ phase' = "failed"
    /\ pending' = 0
    /\ childFailed' = TRUE
    /\ deferredActFailed' = TRUE
    /\ selectedError' = "child"
    /\ UNCHANGED <<cursor, assignments, markers, publications, visible>>

Publish ==
    /\ phase = "ready"
    /\ cursor = EndpointCount
    /\ Len(assignments) = EndpointCount
    /\ phase' = "published"
    /\ publications' = publications + 1
    /\ visible' = "new"
    /\ UNCHANGED <<cursor, pending, assignments, markers, childFailed,
                    deferredActFailed, selectedError>>

Next == YieldEndpoint \/ AttachBody \/ FailChildAndDeferred \/ Publish
Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ phase \in Phases
    /\ cursor \in 0..EndpointCount
    /\ pending \in 0..EndpointCount
    /\ assignments \in Seq([endpoint: {"root", "a", "ab", "extra"},
                              body: {101, 102, 103, 104}])
    /\ markers \in Seq(Nat)
    /\ publications \in Nat
    /\ visible \in VisibleImages
    /\ childFailed \in BOOLEAN
    /\ deferredActFailed \in BOOLEAN
    /\ selectedError \in Errors

MarkersAreZero ==
    /\ Len(markers) = EndpointCount
    /\ \A index \in 1..Len(markers): markers[index] = 0

LogicalOccurrencesIgnoreStorageIdentity ==
    /\ StorageAddresses[2] = StorageAddresses[3]
    /\ Len(markers) = Len(StorageAddresses)
    /\ EndpointCount = Len(StorageAddresses)

AssignmentPrefixIsExact ==
    /\ Len(assignments) = cursor
    /\ \A index \in 1..Len(assignments):
          assignments[index] =
            [endpoint |-> Endpoints[index], body |-> Bodies[index]]

SuspendedFocusIsExact ==
    IF phase = "await-child"
    THEN pending = cursor + 1
    ELSE pending = 0

ChildErrorPrecedesDeferredAct ==
    childFailed /\ deferredActFailed => selectedError = "child"

FailureNeverPublishes ==
    phase = "failed" => /\ visible = "old" /\ publications = 0

PublicationRequiresCompleteLockstep ==
    visible = "new" =>
      /\ phase = "published"
      /\ cursor = EndpointCount
      /\ Len(assignments) = EndpointCount

PublicationIsExactlyOnce ==
    /\ publications \in 0..1
    /\ publications = 1 <=> visible = "new"
    /\ publications = 0 <=> visible = "old"

(***************************************************************************
Causal negative controls.  Each mutant is isolated by a configuration in the
proof harness so the reported invariant identifies the broken obligation.
***************************************************************************)

SequentialMarkerInit == Initial(<<0, 1, 2>>, 0, <<>>)
SequentialMarkerSpec == SequentialMarkerInit /\ [][Next]_vars

NonzeroMarkerInit == Initial(<<0, 0, 7>>, 0, <<>>)
NonzeroMarkerSpec == NonzeroMarkerInit /\ [][Next]_vars

RootOmissionInit == Initial(<<0, 0, 0>>, 1, <<>>)
RootOmissionSpec == RootOmissionInit /\ [][Next]_vars

CollapseSharedOccurrence ==
    /\ phase = "scan"
    /\ cursor = 2
    /\ StorageAddresses[2] = StorageAddresses[3]
    /\ cursor' = 3
    /\ phase' = "ready"
    /\ UNCHANGED <<pending, assignments, markers, publications, visible,
                    childFailed, deferredActFailed, selectedError>>
AddressCollapseSpec == Init /\ [][Next \/ CollapseSharedOccurrence]_vars

SkipEndpoint ==
    /\ phase = "await-child"
    /\ pending = 2
    /\ cursor' = 2
    /\ pending' = 0
    /\ phase' = "scan"
    /\ UNCHANGED <<assignments, markers, publications, visible, childFailed,
                    deferredActFailed, selectedError>>
SkipEndpointSpec == Init /\ [][Next \/ SkipEndpoint]_vars

WrongEndpointAttach ==
    /\ phase = "await-child"
    /\ pending = 2
    /\ assignments' = Append(assignments,
          [endpoint |-> Endpoints[3], body |-> Bodies[2]])
    /\ cursor' = 2
    /\ pending' = 0
    /\ phase' = "scan"
    /\ UNCHANGED <<markers, publications, visible, childFailed,
                    deferredActFailed, selectedError>>
WrongEndpointSpec == Init /\ [][Next \/ WrongEndpointAttach]_vars

DuplicateEndpointAttach ==
    /\ phase = "await-child"
    /\ pending = 2
    /\ assignments' = Append(assignments,
          [endpoint |-> Endpoints[1], body |-> Bodies[2]])
    /\ cursor' = 2
    /\ pending' = 0
    /\ phase' = "scan"
    /\ UNCHANGED <<markers, publications, visible, childFailed,
                    deferredActFailed, selectedError>>
DuplicateEndpointSpec == Init /\ [][Next \/ DuplicateEndpointAttach]_vars

MissingBodyAccept ==
    /\ phase = "await-child"
    /\ pending = 3
    /\ cursor' = 3
    /\ pending' = 0
    /\ phase' = "ready"
    /\ UNCHANGED <<assignments, markers, publications, visible, childFailed,
                    deferredActFailed, selectedError>>
MissingBodySpec == Init /\ [][Next \/ MissingBodyAccept]_vars

ExtraBodyAttach ==
    /\ phase = "ready"
    /\ assignments' = Append(assignments,
          [endpoint |-> "extra", body |-> 104])
    /\ UNCHANGED <<phase, cursor, pending, markers, publications, visible,
                    childFailed, deferredActFailed, selectedError>>
ExtraBodySpec == Init /\ [][Next \/ ExtraBodyAttach]_vars

EarlyPublish ==
    /\ phase = "scan"
    /\ cursor < EndpointCount
    /\ phase' = "published"
    /\ visible' = "new"
    /\ publications' = 1
    /\ UNCHANGED <<cursor, pending, assignments, markers, childFailed,
                    deferredActFailed, selectedError>>
EarlyPublishSpec == Init /\ [][Next \/ EarlyPublish]_vars

DoublePublish ==
    /\ phase = "published"
    /\ publications' = publications + 1
    /\ UNCHANGED <<phase, cursor, pending, assignments, markers, visible,
                    childFailed, deferredActFailed, selectedError>>
DoublePublishSpec == Init /\ [][Next \/ DoublePublish]_vars

ReverseErrorPrecedence ==
    /\ phase = "await-child"
    /\ pending = 2
    /\ phase' = "failed"
    /\ pending' = 0
    /\ childFailed' = TRUE
    /\ deferredActFailed' = TRUE
    /\ selectedError' = "act"
    /\ UNCHANGED <<cursor, assignments, markers, publications, visible>>
PrecedenceSpec == Init /\ [][Next \/ ReverseErrorPrecedence]_vars

=============================================================================
