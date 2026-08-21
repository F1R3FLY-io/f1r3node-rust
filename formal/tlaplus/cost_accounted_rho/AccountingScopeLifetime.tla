---------------------- MODULE AccountingScopeLifetime ----------------------
EXTENDS Integers, FiniteSets

CONSTANTS Scopes, UnsafeBoolean

ASSUME Scopes # {}
ASSUME UnsafeBoolean \in BOOLEAN

VARIABLES owners, count, active

vars == <<owners, count, active>>

Init ==
    /\ owners = {}
    /\ count = 0
    /\ active = FALSE

Enter(scope) ==
    /\ scope \in Scopes \ owners
    /\ owners' = owners \cup {scope}
    /\ count' = count + 1
    /\ active' = TRUE

Exit(scope) ==
    /\ scope \in owners
    /\ owners' = owners \ {scope}
    /\ count' = count - 1
    /\ active' = IF UnsafeBoolean THEN FALSE ELSE count' > 0

Next ==
    \/ \E scope \in Scopes : Enter(scope)
    \/ \E scope \in Scopes : Exit(scope)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ owners \subseteq Scopes
    /\ count \in Nat
    /\ active \in BOOLEAN

ScopeCountMatchesOwners == count = Cardinality(owners)

AccountingScopeReflectsOwners == active = (owners # {})

=============================================================================
