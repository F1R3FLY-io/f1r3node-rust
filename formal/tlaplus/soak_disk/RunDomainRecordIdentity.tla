------------------------ MODULE RunDomainRecordIdentity ------------------------
CONSTANT BindIdentity
VARIABLES phase, checkedTrusted, openedTrusted, recordComplete, admitted

vars == <<phase, checkedTrusted, openedTrusted, recordComplete, admitted>>

Init ==
    /\ phase = "probe"
    /\ checkedTrusted \in BOOLEAN
    /\ openedTrusted \in BOOLEAN
    /\ recordComplete \in BOOLEAN
    /\ admitted = FALSE

Trusted == IF BindIdentity THEN openedTrusted /\ recordComplete ELSE checkedTrusted

Admission ==
    /\ phase = "probe"
    /\ phase' = IF Trusted THEN "admitted" ELSE "refused"
    /\ admitted' = Trusted
    /\ UNCHANGED <<checkedTrusted, openedTrusted, recordComplete>>

Next == Admission
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"probe", "admitted", "refused"}
    /\ checkedTrusted \in BOOLEAN
    /\ openedTrusted \in BOOLEAN
    /\ recordComplete \in BOOLEAN
    /\ admitted \in BOOLEAN

AdmissionRequiresOpenedRecordTrust == admitted => (openedTrusted /\ recordComplete)
TrustedRecordAdmits == phase = "refused" => ~(openedTrusted /\ recordComplete)
Completes == <>(phase /= "probe")
=============================================================================
