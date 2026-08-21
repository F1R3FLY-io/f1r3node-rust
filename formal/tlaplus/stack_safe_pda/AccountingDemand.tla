------------------------ MODULE AccountingDemand ------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************
Finite refinement and mutation-calibration model for the accounting-demand
byte-region PDA.  The corpus pins the observable historical order: root family
order, receive COMM before its body, first bind only, match case order, true
before false, missing-channel counting without a callback, and sticky unknown.
Multiplicity is occurrence-major whole-trace replay.

Rocq proves the same refinement for arbitrary finite recognized trees.  This
model adds exhaustive bounded traces, weak-fair termination, and explicit
counterexamples to the plausible ordering/summary mutants listed below.
***************************************************************************)

CONSTANT MaxMultiplicity

ASSUME MaxMultiplicity \in Nat \ {0, 1}

PresentEvents == {
  "Send", "Receive", "ReceiveBody", "NewBody", "Match1", "Match2",
  "IfTrue", "IfFalse", "Bundle", "A", "B"
}
Events == PresentEvents \union {"Missing", "Unknown"}

FullTrace ==
  <<"Send", "Receive", "Missing", "ReceiveBody", "NewBody", "Match1",
    "Match2", "IfTrue", "IfFalse", "Bundle", "Unknown">>

Corpus == {
  <<>>,
  <<"A", "B">>,
  <<"Missing">>,
  <<"Unknown">>,
  FullTrace
}

RECURSIVE RepeatWhole(_, _), RepeatOne(_, _), RepeatGrouped(_,_),
          CallbackTrace(_), CommCount(_), ContainsUnknown(_)

RepeatWhole(trace, count) ==
  IF count = 0
  THEN <<>>
  ELSE trace \o RepeatWhole(trace, count - 1)

RepeatOne(event, count) ==
  IF count = 0
  THEN <<>>
  ELSE <<event>> \o RepeatOne(event, count - 1)

RepeatGrouped(trace, count) ==
  IF Len(trace) = 0
  THEN <<>>
  ELSE RepeatOne(trace[1], count)
         \o RepeatGrouped(SubSeq(trace, 2, Len(trace)), count)

CallbackTrace(trace) ==
  IF Len(trace) = 0
  THEN <<>>
  ELSE IF trace[1] \in PresentEvents
       THEN <<trace[1]>> \o CallbackTrace(SubSeq(trace, 2, Len(trace)))
       ELSE CallbackTrace(SubSeq(trace, 2, Len(trace)))

CommCount(trace) ==
  IF Len(trace) = 0
  THEN 0
  ELSE (IF trace[1] = "Unknown" THEN 0 ELSE 1)
       + CommCount(SubSeq(trace, 2, Len(trace)))

ContainsUnknown(trace) ==
  IF Len(trace) = 0
  THEN FALSE
  ELSE (trace[1] = "Unknown")
       \/ ContainsUnknown(SubSeq(trace, 2, Len(trace)))

VARIABLES
  trace,
  multiplicity,
  expected,
  work,
  output,
  callbacks,
  commCount,
  unknown,
  stepCount

vars ==
  <<trace, multiplicity, expected, work, output, callbacks, commCount,
    unknown, stepCount>>

Init ==
  /\ trace \in Corpus
  /\ multiplicity \in 1..MaxMultiplicity
  /\ expected = RepeatWhole(trace, multiplicity)
  /\ work = expected
  /\ output = <<>>
  /\ callbacks = <<>>
  /\ commCount = 0
  /\ unknown = FALSE
  /\ stepCount = 0

Advance ==
  /\ Len(work) > 0
  /\ LET event == work[1]
     IN /\ work' = SubSeq(work, 2, Len(work))
        /\ output' = Append(output, event)
        /\ callbacks' =
             IF event \in PresentEvents THEN Append(callbacks, event)
             ELSE callbacks
        /\ commCount' =
             IF event = "Unknown" THEN commCount ELSE commCount + 1
        /\ unknown' = (unknown \/ event = "Unknown")
  /\ stepCount' = stepCount + 1
  /\ UNCHANGED <<trace, multiplicity, expected>>

Next == Advance
Spec == Init /\ [][Next]_vars /\ WF_vars(Advance)
Done == Len(work) = 0

TypeInvariant ==
  /\ trace \in Corpus
  /\ multiplicity \in 1..MaxMultiplicity
  /\ expected \in Seq(Events)
  /\ work \in Seq(Events)
  /\ output \in Seq(Events)
  /\ callbacks \in Seq(PresentEvents)
  /\ commCount \in Nat
  /\ unknown \in BOOLEAN
  /\ stepCount \in Nat

TracePrefixRefinement == output \o work = expected
CallbackPrefixRefinement == callbacks = CallbackTrace(output)
SummaryRefinement ==
  /\ commCount = CommCount(output)
  /\ unknown = ContainsUnknown(output)

MultiplicitySummaryRefinement ==
  /\ CommCount(expected) = multiplicity * CommCount(trace)
  /\ ContainsUnknown(expected) = ContainsUnknown(trace)

CompletedTraceEquivalent ==
  Done =>
    /\ output = expected
    /\ callbacks = CallbackTrace(expected)
    /\ commCount = CommCount(expected)
    /\ unknown = ContainsUnknown(expected)

ActionWitnessesAreNonVacuous ==
  /\ FullTrace \in Corpus
  /\ <<"A", "B">> \in Corpus
  /\ <<"Missing">> \in Corpus
  /\ <<"Unknown">> \in Corpus
  /\ MaxMultiplicity >= 2

GroupedMultiplicityMutationIsDetected ==
  RepeatWhole(<<"A", "B">>, 2) # RepeatGrouped(<<"A", "B">>, 2)

FamilyOrderMutationIsDetected ==
  FullTrace #
    <<"Receive", "Send", "Missing", "ReceiveBody", "NewBody", "Match1",
      "Match2", "IfTrue", "IfFalse", "Bundle", "Unknown">>

BranchOrderMutationIsDetected ==
  FullTrace #
    <<"Send", "Receive", "Missing", "ReceiveBody", "NewBody", "Match1",
      "Match2", "IfFalse", "IfTrue", "Bundle", "Unknown">>

DroppedMissingCommMutationIsDetected == CommCount(<<"Missing">>) # 0
MissingChannelCallbackMutationIsDetected == CallbackTrace(<<"Missing">>) # <<"Missing">>

AllReceiveSourcesMutationIsDetected ==
  FullTrace #
    <<"Send", "Receive", "ReceiveSecondBind", "Missing", "ReceiveBody",
      "NewBody", "Match1", "Match2", "IfTrue", "IfFalse", "Bundle",
      "Unknown">>

DroppedUnknownMutationIsDetected == ContainsUnknown(<<"Unknown">>) # FALSE

EventuallyComplete == <>Done

=============================================================================
