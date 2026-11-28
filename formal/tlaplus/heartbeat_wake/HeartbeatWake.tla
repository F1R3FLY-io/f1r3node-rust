--------------------------- MODULE HeartbeatWake ---------------------------
EXTENDS Naturals

CONSTANTS Times, MinimumRecheck, WakeOnRejected, BypassCooldown, RecheckDespiteLag
VARIABLES input, check, evaluated

Inputs ==
    [admission : {"accepted", "duplicate", "failed"},
     autopropose : BOOLEAN, proposerAvailable : BOOLEAN, signalReady : BOOLEAN,
     pending : BOOLEAN, lagAllows : BOOLEAN, otherLaneDue : BOOLEAN,
     ageKnown : BOOLEAN, age : Times, cooldown : Times, interval : Times]

Cooling(x) == x.ageKnown /\ x.age < x.cooldown
Ready(x) == x.pending /\ x.lagAllows
ApiProposes(x) == x.autopropose /\ x.proposerAvailable
Smaller(a, b) == IF a < b THEN a ELSE b
Larger(a, b) == IF a > b THEN a ELSE b

Evaluate(x) ==
    LET recheck == x.pending /\ (x.lagAllows \/ RecheckDespiteLag)
                   /\ Cooling(x) /\ ~x.otherLaneDue
        remaining == IF Cooling(x) THEN x.cooldown - x.age ELSE 0
    IN [wake |-> (x.admission = "accepted" \/ WakeOnRejected)
                 /\ ~ApiProposes(x) /\ x.signalReady,
        pendingProposal |-> Ready(x) /\ (~Cooling(x) \/ BypassCooldown),
        recheck |-> recheck,
        sleep |-> IF recheck
                  THEN Smaller(x.interval, Larger(MinimumRecheck, remaining))
                  ELSE x.interval]

Init ==
    /\ input = CHOOSE x \in Inputs : ~x.signalReady /\ ~x.pending
    /\ check = Evaluate(input)
    /\ evaluated = FALSE

Next ==
    IF evaluated THEN UNCHANGED <<input, check, evaluated>>
    ELSE /\ input' \in Inputs
         /\ check' = Evaluate(input')
         /\ evaluated' = TRUE

WakeOnlyOnAdmission ==
    check.wake = (input.admission = "accepted" /\ ~ApiProposes(input) /\ input.signalReady)
CooldownProtectsPending ==
    check.pendingProposal = (Ready(input) /\ ~Cooling(input))
RecheckRequiresPendingCooldown ==
    check.recheck = (Ready(input) /\ Cooling(input) /\ ~input.otherLaneDue)
ShorterSleepRespectsCooldown ==
    check.sleep <= input.interval
    /\ (check.sleep < input.interval =>
         check.recheck /\ check.sleep >= input.cooldown - input.age)
=============================================================================
