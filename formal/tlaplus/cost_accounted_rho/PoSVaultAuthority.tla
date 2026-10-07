------------------------ MODULE PoSVaultAuthority ------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    \* @type: Str;
    PosKey,
    \* @type: Str;
    AttackerKey,
    \* @type: Str;
    PlaceholderKey,
    \* @type: Int;
    InitialPosBalance,
    \* @type: Int;
    InitialTargetBalance,
    \* @type: Bool;
    BindAuthenticatedKey,
    \* @type: Bool;
    HasUnresolvedPlaceholder,
    \* @type: Bool;
    RejectUnresolvedTemplates

ASSUME /\ Cardinality({PosKey, AttackerKey, PlaceholderKey}) = 3
       /\ InitialPosBalance >= 1
       /\ InitialTargetBalance >= 0
       /\ BindAuthenticatedKey \in BOOLEAN
       /\ HasUnresolvedPlaceholder \in BOOLEAN
       /\ RejectUnresolvedTemplates \in BOOLEAN

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Bool;
    compiled,
    \* @type: Bool;
    installed,
    \* @type: Str;
    controlKey,
    \* @type: Int;
    posBalance,
    \* @type: Int;
    targetBalance,
    \* @type: Bool;
    unauthorizedAccepted,
    \* @type: Bool;
    authorizedAccepted

vars == <<phase, compiled, installed, controlKey, posBalance,
          targetBalance, unauthorizedAccepted, authorizedAccepted>>

Init ==
    /\ phase = "compile"
    /\ compiled = FALSE
    /\ installed = FALSE
    /\ controlKey = PlaceholderKey
    /\ posBalance = InitialPosBalance
    /\ targetBalance = InitialTargetBalance
    /\ unauthorizedAccepted = FALSE
    /\ authorizedAccepted = FALSE

Compile ==
    /\ phase = "compile"
    /\ IF HasUnresolvedPlaceholder /\ RejectUnresolvedTemplates
          THEN /\ compiled' = FALSE
               /\ phase' = "rejected"
          ELSE /\ compiled' = TRUE
               /\ phase' = "install"
    /\ UNCHANGED <<installed, controlKey, posBalance, targetBalance,
                    unauthorizedAccepted, authorizedAccepted>>

Install ==
    /\ phase = "install"
    /\ compiled
    /\ installed' = TRUE
    /\ controlKey' = IF BindAuthenticatedKey THEN PosKey ELSE PlaceholderKey
    /\ phase' = "unauthorized"
    /\ UNCHANGED <<compiled, posBalance, targetBalance,
                    unauthorizedAccepted, authorizedAccepted>>

UnauthorizedAttempt ==
    /\ phase = "unauthorized"
    /\ installed
    /\ unauthorizedAccepted' = (AttackerKey = controlKey)
    /\ posBalance' = IF AttackerKey = controlKey THEN posBalance - 1 ELSE posBalance
    /\ targetBalance' = IF AttackerKey = controlKey THEN targetBalance + 1 ELSE targetBalance
    /\ phase' = "authorized"
    /\ UNCHANGED <<compiled, installed, controlKey, authorizedAccepted>>

AuthorizedAttempt ==
    /\ phase = "authorized"
    /\ installed
    /\ authorizedAccepted' = (PosKey = controlKey)
    /\ posBalance' = IF PosKey = controlKey THEN posBalance - 1 ELSE posBalance
    /\ targetBalance' = IF PosKey = controlKey THEN targetBalance + 1 ELSE targetBalance
    /\ phase' = "done"
    /\ UNCHANGED <<compiled, installed, controlKey, unauthorizedAccepted>>

Terminal ==
    /\ phase \in {"done", "rejected"}
    /\ UNCHANGED vars

Next == Compile \/ Install \/ UnauthorizedAttempt \/ AuthorizedAttempt \/ Terminal

Fairness ==
    /\ WF_vars(Compile)
    /\ WF_vars(Install)
    /\ WF_vars(UnauthorizedAttempt)
    /\ WF_vars(AuthorizedAttempt)

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ phase \in {"compile", "install", "unauthorized", "authorized", "done", "rejected"}
    /\ compiled \in BOOLEAN
    /\ installed \in BOOLEAN
    /\ controlKey \in {PosKey, PlaceholderKey}
    /\ posBalance \in Nat
    /\ targetBalance \in Nat
    /\ unauthorizedAccepted \in BOOLEAN
    /\ authorizedAccepted \in BOOLEAN

NoCompiledPlaceholder == ~(compiled /\ HasUnresolvedPlaceholder)

InstalledBindsAuthenticatedKey == installed => controlKey = PosKey

UnauthorizedCannotDebit ==
    /\ ~unauthorizedAccepted
    /\ posBalance = InitialPosBalance - (IF authorizedAccepted THEN 1 ELSE 0)
    /\ targetBalance = InitialTargetBalance + (IF authorizedAccepted THEN 1 ELSE 0)

VaultConservation == posBalance + targetBalance = InitialPosBalance + InitialTargetBalance

InstalledEventuallyAuthorizes == installed ~> authorizedAccepted

=============================================================================
