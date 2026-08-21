---------------------- MODULE StateBoundValidatorConvergence ----------------------
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS Validators, Events, Schedules, EventCost,
          RootChoices, ContextChoices, DeployOrders,
          CorrectRoot, CorrectContext, CanonicalDeployOrder, CertifiedSchedule,
          IgnoreCertificateContext, UseArrivalOrderWithoutPostCheck

ASSUME /\ Validators # {}
       /\ Events # {}
       /\ Schedules \subseteq Seq(Events)
       /\ Schedules # {}
       /\ EventCost \in [Events -> Nat]
       /\ CertifiedSchedule \in Schedules
       /\ CorrectRoot \in RootChoices
       /\ CorrectContext \in ContextChoices
       /\ CanonicalDeployOrder \in DeployOrders
       /\ IgnoreCertificateContext \in BOOLEAN
       /\ UseArrivalOrderWithoutPostCheck \in BOOLEAN

VARIABLES verdict, localRoot, localContext, arrivalOrder, schedule,
          executedOrder, observedCost, observedPost

vars == <<verdict, localRoot, localContext, arrivalOrder, schedule,
          executedOrder, observedCost, observedPost>>

RECURSIVE ScheduleCost(_)

ScheduleCost(events) ==
  IF Len(events) = 0
  THEN 0
  ELSE EventCost[Head(events)] + ScheduleCost(Tail(events))

RECURSIVE EventSet(_)

EventSet(events) ==
  IF Len(events) = 0
  THEN {}
  ELSE {Head(events)} \cup EventSet(Tail(events))

CertificateCost == ScheduleCost(CertifiedSchedule)
CertificatePost == <<CorrectRoot, CanonicalDeployOrder, CertifiedSchedule>>
NoPost == <<CorrectRoot, <<>>, {}>>
PossiblePostStates ==
  {NoPost} \cup
  {<<root, order, events>> :
    root \in RootChoices, order \in DeployOrders, events \in Schedules}

Init ==
  /\ verdict = [validator \in Validators |-> "Pending"]
  /\ localRoot \in [Validators -> RootChoices]
  /\ localContext \in [Validators -> ContextChoices]
  /\ arrivalOrder \in [Validators -> DeployOrders]
  /\ schedule \in [Validators -> Schedules]
  /\ executedOrder = [validator \in Validators |-> <<>>]
  /\ observedCost = [validator \in Validators |-> 0]
  /\ observedPost = [validator \in Validators |-> NoPost]

ContextMatches(validator) ==
  /\ localRoot[validator] = CorrectRoot
  /\ localContext[validator] = CorrectContext

ChosenOrder(validator) ==
  IF UseArrivalOrderWithoutPostCheck
  THEN arrivalOrder[validator]
  ELSE CanonicalDeployOrder

ComputedCost(validator) ==
  IF UseArrivalOrderWithoutPostCheck
  THEN ScheduleCost(schedule[validator])
  ELSE CertificateCost

ComputedPost(validator) ==
  IF UseArrivalOrderWithoutPostCheck
  THEN <<localRoot[validator], ChosenOrder(validator), schedule[validator]>>
  ELSE CertificatePost

CostMatches(validator) == ComputedCost(validator) = CertificateCost
PostMatches(validator) == ComputedPost(validator) = CertificatePost

Accept(validator) ==
  /\ verdict[validator] = "Pending"
  /\ CostMatches(validator)
  /\ IF IgnoreCertificateContext
        THEN TRUE
        ELSE ContextMatches(validator)
  /\ IF UseArrivalOrderWithoutPostCheck
        THEN TRUE
        ELSE PostMatches(validator)
  /\ verdict' = [verdict EXCEPT ![validator] = "Accepted"]
  /\ executedOrder' = [executedOrder EXCEPT ![validator] = ChosenOrder(validator)]
  /\ observedCost' = [observedCost EXCEPT ![validator] = ComputedCost(validator)]
  /\ observedPost' = [observedPost EXCEPT ![validator] = ComputedPost(validator)]
  /\ UNCHANGED <<localRoot, localContext, arrivalOrder, schedule>>

Reject(validator) ==
  /\ verdict[validator] = "Pending"
  /\ ~(/\ CostMatches(validator)
       /\ IF IgnoreCertificateContext
             THEN TRUE
             ELSE ContextMatches(validator)
       /\ IF UseArrivalOrderWithoutPostCheck
             THEN TRUE
             ELSE PostMatches(validator))
  /\ verdict' = [verdict EXCEPT ![validator] = "Rejected"]
  /\ UNCHANGED <<localRoot, localContext, arrivalOrder, schedule,
                  executedOrder, observedCost, observedPost>>

Decide(validator) == Accept(validator) \/ Reject(validator)

Quiesce ==
  /\ \A validator \in Validators : verdict[validator] # "Pending"
  /\ UNCHANGED vars

Next == (\E validator \in Validators : Decide(validator)) \/ Quiesce

Spec == /\ Init
        /\ [][Next]_vars
        /\ \A validator \in Validators : WF_vars(Decide(validator))

TypeOK ==
  /\ verdict \in [Validators -> {"Pending", "Accepted", "Rejected"}]
  /\ localRoot \in [Validators -> RootChoices]
  /\ localContext \in [Validators -> ContextChoices]
  /\ arrivalOrder \in [Validators -> DeployOrders]
  /\ schedule \in [Validators -> Schedules]
  /\ executedOrder \in [Validators -> (DeployOrders \cup {<<>>})]
  /\ observedCost \in [Validators -> Nat]
  /\ observedPost \in [Validators -> PossiblePostStates]

ScheduleDiversityIsExercised ==
  \E left, right \in Schedules :
    EventSet(left) # EventSet(right) \/ ScheduleCost(left) # ScheduleCost(right)

AcceptedUsesAuthenticatedContext ==
  \A validator \in Validators :
    verdict[validator] = "Accepted" => ContextMatches(validator)

AcceptedUsesCanonicalDeployOrder ==
  \A validator \in Validators :
    verdict[validator] = "Accepted" => executedOrder[validator] = CanonicalDeployOrder

AcceptedReproducesCertificate ==
  \A validator \in Validators :
    verdict[validator] = "Accepted" =>
      /\ observedCost[validator] = CertificateCost
      /\ observedPost[validator] = CertificatePost

AcceptedValidatorsAgree ==
  \A left, right \in Validators :
    verdict[left] = "Accepted" /\ verdict[right] = "Accepted" =>
      /\ observedCost[left] = observedCost[right]
      /\ observedPost[left] = observedPost[right]

EventuallyAllValidatorsDecide ==
  <> (\A validator \in Validators : verdict[validator] # "Pending")

=============================================================================
