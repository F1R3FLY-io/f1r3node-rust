----------------------- MODULE SoakQualification -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Required, LegacyElapsedOnly
VARIABLES phase, preflight, valid, elapsed, qualified, pending, busy, seen, passed, jobSucceeded

vars == <<phase, preflight, valid, elapsed, qualified, pending, busy, seen, passed, jobSucceeded>>
Cap(n) == IF n >= Required THEN Required ELSE n

Init ==
    /\ phase = "preflight"
    /\ preflight = FALSE
    /\ valid = TRUE
    /\ elapsed = 0
    /\ qualified = 0
    /\ pending = 0
    /\ busy = 0
    /\ seen = {}
    /\ passed = FALSE
    /\ jobSucceeded = FALSE

Start ==
    /\ phase = "preflight"
    /\ phase' = "running"
    /\ preflight' \in BOOLEAN
    /\ UNCHANGED <<valid, elapsed, qualified, pending, busy, seen, passed, jobSucceeded>>

Begin ==
    /\ phase = "running"
    /\ busy = 0
    /\ busy' \in {1, 2}
    /\ pending' = 0
    /\ UNCHANGED <<phase, preflight, valid, elapsed, qualified, seen, passed, jobSucceeded>>

Work ==
    /\ phase = "running"
    /\ busy # 0
    /\ pending' = Cap(pending + 1)
    /\ elapsed' = Cap(elapsed + 1)
    /\ UNCHANGED <<phase, preflight, valid, qualified, busy, seen, passed, jobSucceeded>>

Idle ==
    /\ phase = "running"
    /\ busy = 0
    /\ elapsed' = Cap(elapsed + 1)
    /\ UNCHANGED <<phase, preflight, valid, qualified, pending, busy, seen, passed, jobSucceeded>>

Complete ==
    /\ phase = "running"
    /\ busy # 0
    /\ pending > 0
    /\ qualified' = Cap(qualified + pending)
    /\ seen' = seen \cup {busy}
    /\ busy' = 0
    /\ pending' = 0
    /\ UNCHANGED <<phase, preflight, valid, elapsed, passed, jobSucceeded>>

Invalidate ==
    /\ phase \in {"running", "checking"}
    /\ valid' = FALSE
    /\ UNCHANGED <<phase, preflight, elapsed, qualified, pending, busy, seen, passed, jobSucceeded>>

EndJob ==
    /\ phase = "running"
    /\ phase' = "checking"
    /\ jobSucceeded' \in BOOLEAN
    /\ UNCHANGED <<preflight, valid, elapsed, qualified, pending, busy, seen, passed>>

Contract == preflight /\ valid /\ busy = 0 /\ qualified >= Required /\ seen = {1, 2} /\ jobSucceeded

Seal ==
    /\ phase = "checking"
    /\ phase' = "sealed"
    /\ passed' = IF LegacyElapsedOnly THEN elapsed >= Required ELSE Contract
    /\ UNCHANGED <<preflight, valid, elapsed, qualified, pending, busy, seen, jobSucceeded>>

Next == Start \/ Begin \/ Work \/ Idle \/ Complete \/ Invalidate \/ EndJob \/ Seal
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"preflight", "running", "checking", "sealed"}
    /\ preflight \in BOOLEAN
    /\ valid \in BOOLEAN
    /\ elapsed \in 0..Required
    /\ qualified \in 0..Required
    /\ pending \in 0..Required
    /\ busy \in 0..2
    /\ seen \subseteq {1, 2}
    /\ passed \in BOOLEAN
    /\ jobSucceeded \in BOOLEAN

QualifiedImpliesContract == passed => (phase = "sealed" /\ Contract)
CreditBoundedByElapsed == qualified <= elapsed
FailureIsPermanent == ~valid => ~passed
=============================================================================
