-------------------- MODULE FeeCursorReadBoundary --------------------
EXTENDS Integers, FiniteSets

CONSTANT RejectExtraContent
ASSUME RejectExtraContent \in BOOLEAN
VARIABLES responseCount, fieldCount, typed, present, revision, position,
          payerCount, extraContent, accepted
vars == <<responseCount, fieldCount, typed, present, revision, position,
          payerCount, extraContent, accepted>>

MaximumRevision == 3
Fields == {"response", "presence", "revision", "position"}
NumericValid ==
    IF present
    THEN /\ revision \in 0..MaximumRevision /\ position \in 0..(payerCount - 1)
    ELSE /\ revision = 0 /\ position = 0
CanonicalReply ==
    /\ responseCount = 1 /\ fieldCount = 3 /\ typed
    /\ extraContent = {} /\ NumericValid
Decode ==
    /\ responseCount = 1 /\ fieldCount = 3 /\ typed
    /\ (~RejectExtraContent \/ extraContent = {})
    /\ NumericValid

Init ==
    /\ responseCount \in 0..2
    /\ fieldCount \in 2..4
    /\ typed \in BOOLEAN /\ present \in BOOLEAN
    /\ revision \in -1..4 /\ position \in -1..3
    /\ payerCount \in 1..3
    /\ extraContent \in SUBSET Fields
    /\ accepted = Decode
Next == UNCHANGED vars
Spec == Init /\ [][Next]_vars

AcceptedContainsExactlyOneTypedReply == accepted => CanonicalReply
ValidCanonicalReplyIsAccepted == CanonicalReply => accepted
AbsenceHasZeroPayload == accepted /\ ~present => revision = 0 /\ position = 0
PresentCursorWithinBounds ==
    accepted /\ present => revision \in 0..MaximumRevision /\ position \in 0..(payerCount - 1)
=============================================================================
