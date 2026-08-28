---------------- MODULE RSpaceStreamingCodecPDA ----------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS ScratchCapacity, MaxInputBytes, Mutation

ASSUME ScratchCapacity \in Nat \ {0}
ASSUME MaxInputBytes \in Nat \ {0}
ASSUME ScratchCapacity < MaxInputBytes

NoError == "none"
Codecs == {
  "datum", "continuation", "join", "mutation", "dag", "blob",
  "semantic", "accelerator", "retention"
}
Phases == {"Header", "Admission", "Grammar", "Order", "Finish", "Done"}

Scenario(name, codec, fault) == [name |-> name, codec |-> codec, fault |-> fault]

Scenarios == {
  Scenario("datum-success", "datum", NoError),
  Scenario("datum-dangling", "datum", "dangling-escape"),
  Scenario("datum-bad-escape", "datum", "bad-escape"),
  Scenario("datum-group-inside", "datum", "group-inside-frame"),
  Scenario("datum-suffix", "datum", "datum-suffix"),
  Scenario("continuation-success", "continuation", NoError),
  Scenario("continuation-empty", "continuation", "empty-group"),
  Scenario("continuation-trailing", "continuation", "continuation-trailing"),
  Scenario("join-success", "join", NoError),
  Scenario("join-suffix", "join", "join-suffix"),
  Scenario("mutation-success", "mutation", NoError),
  Scenario("mutation-resource", "mutation", "resource"),
  Scenario("mutation-aggregate-resource", "mutation", "aggregate-resource"),
  Scenario("mutation-key", "mutation", "invalid-key"),
  Scenario("mutation-equal", "mutation", "equal-key"),
  Scenario("mutation-prefix", "mutation", "prefix-order"),
  Scenario("mutation-noop", "mutation", "noop"),
  Scenario("mutation-order-noop", "mutation", "order-and-noop"),
  Scenario("mutation-reread", "mutation", "unstable-reread"),
  Scenario("mutation-empty-trailing", "mutation", "empty-and-trailing"),
  Scenario("dag-success", "dag", NoError),
  Scenario("blob-success", "blob", NoError),
  Scenario("semantic-success", "semantic", NoError),
  Scenario("accelerator-success", "accelerator", NoError),
  Scenario("retention-success", "retention", NoError),
  Scenario("object-envelope", "dag", "envelope"),
  Scenario("object-aggregate", "accelerator", "aggregate"),
  Scenario("object-field", "retention", "field")
}

IsPath(codec) == codec \in {"datum", "continuation", "join"}
IsObject(codec) == codec \in {"dag", "blob", "semantic", "accelerator", "retention"}

PathOracle(s) ==
  CASE s.fault = NoError -> "success"
    [] s.fault = "dangling-escape" -> "truncated-frame"
    [] s.fault = "bad-escape" -> "invalid-escape"
    [] s.fault = "group-inside-frame" -> "invalid-escape"
    [] s.fault = "empty-group" -> "empty-group"
    [] s.fault = "continuation-trailing" -> "trailing"
    [] s.fault = "datum-suffix" -> "success"
    [] s.fault = "join-suffix" -> "success"
    [] OTHER -> "invalid-path"

MutationOracle(s) ==
  CASE s.fault = NoError -> "success"
    [] s.fault \in {"resource", "aggregate-resource"} -> "resource"
    [] s.fault = "invalid-key" -> "invalid-key"
    [] s.fault \in {"equal-key", "prefix-order", "order-and-noop"} -> "key-order"
    [] s.fault = "noop" -> "noop"
    [] s.fault = "unstable-reread" -> "unstable-source"
    [] s.fault = "empty-and-trailing" -> "trailing"
    [] OTHER -> "invalid-mutation"

ObjectOracle(s) ==
  CASE s.fault = NoError -> "success"
    [] s.fault = "envelope" -> "envelope"
    [] s.fault = "aggregate" -> "aggregate"
    [] s.fault = "field" -> "field"
    [] OTHER -> "invalid-object"

OracleOutcome(s) ==
  IF IsPath(s.codec) THEN PathOracle(s)
  ELSE IF s.codec = "mutation" THEN MutationOracle(s)
  ELSE ObjectOracle(s)

VARIABLES scenario, phase, cursor, scratchUsed, retainedInputBytes,
          variableReads, admitted, frameChecked, keyChecked, orderChecked,
          rereadChecked, aggregateChecked, fieldChecked, trailingChecked,
          outcome

vars == <<scenario, phase, cursor, scratchUsed, retainedInputBytes,
          variableReads, admitted, frameChecked, keyChecked, orderChecked,
          rereadChecked, aggregateChecked, fieldChecked, trailingChecked,
          outcome>>

Init ==
  /\ scenario \in Scenarios
  /\ phase = "Header"
  /\ cursor = 0
  /\ scratchUsed = 0
  /\ retainedInputBytes = 0
  /\ variableReads = 0
  /\ admitted = FALSE
  /\ frameChecked = FALSE
  /\ keyChecked = FALSE
  /\ orderChecked = FALSE
  /\ rereadChecked = FALSE
  /\ aggregateChecked = FALSE
  /\ fieldChecked = FALSE
  /\ trailingChecked = FALSE
  /\ outcome = NoError

Header ==
  /\ phase = "Header"
  /\ IF scenario.fault = "envelope"
        THEN /\ phase' = "Done"
             /\ outcome' = "envelope"
        ELSE /\ phase' = "Admission"
             /\ outcome' = outcome
  /\ cursor' = cursor + 1
  /\ UNCHANGED <<scenario, scratchUsed, retainedInputBytes, variableReads,
                  admitted, frameChecked, keyChecked, orderChecked,
                  rereadChecked, aggregateChecked, fieldChecked, trailingChecked>>

Admission ==
  /\ phase = "Admission"
  /\ scratchUsed' = IF Mutation = "WholeInputBuffer" THEN MaxInputBytes
                     ELSE ScratchCapacity
  /\ retainedInputBytes' = IF Mutation = "WholeFieldBuffer" THEN MaxInputBytes ELSE 0
  /\ variableReads' = IF Mutation = "LimitAfterRead" THEN variableReads + 1 ELSE variableReads
  /\ IF scenario.fault = "resource" \/
          (scenario.fault = "aggregate-resource" /\ Mutation # "PerSectionLimit")
        THEN /\ phase' = "Done"
             /\ outcome' = "resource"
             /\ admitted' = FALSE
        ELSE /\ phase' = "Grammar"
             /\ outcome' = outcome
             /\ admitted' = TRUE
  /\ cursor' = cursor + 1
  /\ UNCHANGED <<scenario, frameChecked, keyChecked, orderChecked,
                  rereadChecked, aggregateChecked, fieldChecked, trailingChecked>>

PathGrammarResult(s) ==
  IF s.fault = "dangling-escape" /\ Mutation # "AcceptDanglingEscape"
  THEN "truncated-frame"
  ELSE IF s.fault = "bad-escape" /\ Mutation # "AnyEscapeTerminates"
  THEN "invalid-escape"
  ELSE IF s.fault = "group-inside-frame" /\ Mutation # "GroupInsideFrame"
  THEN "invalid-escape"
  ELSE IF s.fault = "empty-group" /\ Mutation # "AcceptEmptyContinuation"
  THEN "empty-group"
  ELSE IF s.fault = "continuation-trailing" /\ Mutation # "AcceptContinuationTrailing"
  THEN "trailing"
  ELSE IF s.fault = "datum-suffix" /\ Mutation = "RequireFramedDatumSuffix"
  THEN "truncated-frame"
  ELSE IF s.fault = "join-suffix" /\ Mutation = "RequireFramedJoinSuffix"
  THEN "truncated-frame"
  ELSE "success"

Grammar ==
  /\ phase = "Grammar"
  /\ cursor' = cursor + 1
  /\ variableReads' = variableReads + 1
  /\ IF IsPath(scenario.codec)
        THEN /\ frameChecked' = TRUE
             /\ LET result == PathGrammarResult(scenario) IN
                  /\ phase' = "Done"
                  /\ outcome' = result
             /\ UNCHANGED <<keyChecked, orderChecked, rereadChecked,
                             aggregateChecked, fieldChecked, trailingChecked>>
        ELSE IF scenario.codec = "mutation"
        THEN /\ keyChecked' = (scenario.fault # "invalid-key")
             /\ frameChecked' = TRUE
             /\ IF scenario.fault = "invalid-key"
                   THEN /\ phase' = "Done"
                        /\ outcome' = IF Mutation = "OrderBeforeKeyValidation"
                                      THEN "key-order" ELSE "invalid-key"
                        /\ orderChecked' = (Mutation = "OrderBeforeKeyValidation")
                   ELSE /\ phase' = "Order"
                        /\ outcome' = outcome
                        /\ orderChecked' = FALSE
             /\ UNCHANGED <<rereadChecked, aggregateChecked, fieldChecked,
                             trailingChecked>>
        ELSE /\ frameChecked' = frameChecked
             /\ keyChecked' = keyChecked
             /\ orderChecked' = orderChecked
             /\ rereadChecked' = rereadChecked
             /\ aggregateChecked' = TRUE
             /\ fieldChecked' =
                  IF scenario.fault = "aggregate" THEN Mutation = "AggregateAfterFields"
                  ELSE TRUE
             /\ trailingChecked' = trailingChecked
             /\ IF scenario.fault = "aggregate"
                   THEN /\ phase' = "Done"
                        /\ outcome' = "aggregate"
                   ELSE IF scenario.fault = "field"
                   THEN /\ phase' = "Done"
                        /\ outcome' = IF Mutation = "SkipObjectGrammar" THEN "success" ELSE "field"
                   ELSE /\ phase' = "Finish"
                        /\ outcome' = outcome
  /\ UNCHANGED <<scenario, scratchUsed, retainedInputBytes, admitted>>

Order ==
  /\ phase = "Order"
  /\ orderChecked' = TRUE
  /\ rereadChecked' = TRUE
  /\ cursor' = cursor + 1
  /\ IF scenario.fault = "unstable-reread"
        THEN /\ phase' = "Done"
             /\ outcome' = IF Mutation = "TrustReread" THEN "success" ELSE "unstable-source"
        ELSE IF scenario.fault = "equal-key"
        THEN /\ phase' = "Done"
             /\ outcome' = IF Mutation = "AcceptEqualKeys" THEN "success" ELSE "key-order"
        ELSE IF scenario.fault = "prefix-order"
        THEN /\ phase' = "Done"
             /\ outcome' = IF Mutation = "CompareFirstChunkOnly" THEN "success" ELSE "key-order"
        ELSE IF scenario.fault = "order-and-noop"
        THEN /\ phase' = "Done"
             /\ outcome' = IF Mutation = "NoopBeforeOrder" THEN "noop" ELSE "key-order"
        ELSE IF scenario.fault = "noop"
        THEN /\ phase' = "Done"
             /\ outcome' = "noop"
        ELSE /\ phase' = "Finish"
             /\ outcome' = outcome
  /\ UNCHANGED <<scenario, scratchUsed, retainedInputBytes, variableReads,
                  admitted, frameChecked, keyChecked, aggregateChecked,
                  fieldChecked, trailingChecked>>

Finish ==
  /\ phase = "Finish"
  /\ trailingChecked' = TRUE
  /\ cursor' = cursor + 1
  /\ phase' = "Done"
  /\ outcome' =
       IF scenario.fault = "empty-and-trailing"
       THEN IF Mutation = "EmptyBeforeTrailing" THEN "empty" ELSE "trailing"
       ELSE "success"
  /\ UNCHANGED <<scenario, scratchUsed, retainedInputBytes, variableReads,
                  admitted, frameChecked, keyChecked, orderChecked,
                  rereadChecked, aggregateChecked, fieldChecked>>

Done ==
  /\ phase = "Done"
  /\ UNCHANGED vars

Next == Header \/ Admission \/ Grammar \/ Order \/ Finish \/ Done

TypeInvariant ==
  /\ scenario \in Scenarios
  /\ phase \in Phases
  /\ cursor \in Nat
  /\ scratchUsed \in Nat
  /\ retainedInputBytes \in Nat
  /\ variableReads \in Nat
  /\ admitted \in BOOLEAN
  /\ frameChecked \in BOOLEAN
  /\ keyChecked \in BOOLEAN
  /\ orderChecked \in BOOLEAN
  /\ rereadChecked \in BOOLEAN
  /\ aggregateChecked \in BOOLEAN
  /\ fieldChecked \in BOOLEAN
  /\ trailingChecked \in BOOLEAN

ScratchIsBounded == scratchUsed <= ScratchCapacity
NoWholeFieldRetention == retainedInputBytes = 0
VariableReadsFollowAdmission == variableReads > 0 => admitted
OrderingFollowsKeyValidation == orderChecked => keyChecked
RereadStabilityPrecedesOrderAuthority ==
  phase = "Done" /\ scenario.fault = "unstable-reread" => outcome = "unstable-source"
AggregatePrecedesFieldValidation ==
  scenario.fault = "aggregate" => ~fieldChecked
EveryObjectKindIsDispatched ==
  phase = "Done" /\ IsObject(scenario.codec) /\ scenario.fault = NoError => fieldChecked
OutcomeRefinesCodecOracle == phase = "Done" => outcome = OracleOutcome(scenario)
CursorIsFiniteAndMonotone == cursor <= 5

NeverDatumSuccess == ~(phase = "Done" /\ scenario.name = "datum-success" /\ outcome = "success")
NeverContinuationSuccess ==
  ~(phase = "Done" /\ scenario.name = "continuation-success" /\ outcome = "success")
NeverJoinSuccess == ~(phase = "Done" /\ scenario.name = "join-success" /\ outcome = "success")
NeverMutationSuccess ==
  ~(phase = "Done" /\ scenario.name = "mutation-success" /\ outcome = "success")
NeverDagSuccess == ~(phase = "Done" /\ scenario.name = "dag-success" /\ outcome = "success")
NeverBlobSuccess == ~(phase = "Done" /\ scenario.name = "blob-success" /\ outcome = "success")
NeverSemanticSuccess ==
  ~(phase = "Done" /\ scenario.name = "semantic-success" /\ outcome = "success")
NeverAcceleratorSuccess ==
  ~(phase = "Done" /\ scenario.name = "accelerator-success" /\ outcome = "success")
NeverRetentionSuccess ==
  ~(phase = "Done" /\ scenario.name = "retention-success" /\ outcome = "success")
NeverFrameRejection ==
  ~(phase = "Done" /\ scenario.name = "datum-dangling" /\ outcome = "truncated-frame")
NeverGroupRejection ==
  ~(phase = "Done" /\ scenario.name = "continuation-empty" /\ outcome = "empty-group")
NeverResourceRejection ==
  ~(phase = "Done" /\ scenario.name = "mutation-aggregate-resource" /\ outcome = "resource")
NeverKeyRejection ==
  ~(phase = "Done" /\ scenario.name = "mutation-key" /\ outcome = "invalid-key")
NeverOrderRejection ==
  ~(phase = "Done" /\ scenario.name = "mutation-order-noop" /\ outcome = "key-order")
NeverSourceRejection ==
  ~(phase = "Done" /\ scenario.name = "mutation-reread" /\ outcome = "unstable-source")
NeverAggregateRejection ==
  ~(phase = "Done" /\ scenario.name = "object-aggregate" /\ outcome = "aggregate")
NeverFieldRejection ==
  ~(phase = "Done" /\ scenario.name = "object-field" /\ outcome = "field")

Spec == Init /\ [][Next]_vars

=============================================================================
