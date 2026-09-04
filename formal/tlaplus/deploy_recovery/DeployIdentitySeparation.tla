----------------------- MODULE DeployIdentitySeparation -----------------------
EXTENDS FiniteSets

CONSTANT
    \* @type: Bool;
    TaggedIdentity

Domains == {"legacy", "v6"}
Payload == 0
Identities == {<<domain, Payload>> : domain \in Domains}
\* @type: Str => <<Str, Int>>;
Id(domain) == <<domain, Payload>>
\* @type: <<Str, Int>> => <<Str, Int>>;
Key(identity) == <<IF TaggedIdentity THEN identity[1] ELSE "raw", identity[2]>>

VARIABLES
    \* @type: Str;
    rejectedDomain,
    \* @type: Set(<<Str, Int>>);
    tombstoneKeys

vars == <<rejectedDomain, tombstoneKeys>>

Init ==
    /\ rejectedDomain = "none"
    /\ tombstoneKeys = {}

Reject(domain) ==
    /\ domain \in Domains
    /\ rejectedDomain = "none"
    /\ rejectedDomain' = domain
    /\ tombstoneKeys' = {Key(Id(domain))}

Next == \E domain \in Domains : Reject(domain)

Spec == Init /\ [][Next]_vars

Active(domain) == Key(Id(domain)) \notin tombstoneKeys

TypeOK ==
    /\ rejectedDomain \in Domains \union {"none"}
    /\ tombstoneKeys \subseteq {Key(identity) : identity \in Identities}

Inv_DomainKeysAreDisjoint == Key(Id("legacy")) /= Key(Id("v6"))

Inv_CrossDomainRejectionIsolation ==
    \A rejected \in Domains, survivor \in Domains :
        rejected /= survivor /\ rejectedDomain = rejected => Active(survivor)
=============================================================================
