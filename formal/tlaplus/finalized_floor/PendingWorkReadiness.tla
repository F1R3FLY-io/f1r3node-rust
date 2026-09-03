------------------------ MODULE PendingWorkReadiness ------------------------
EXTENDS Naturals

CONSTANT
  \* @type: Bool;
  HeartbeatReadsRetry

ASSUME HeartbeatReadsRetry \in BOOLEAN

VARIABLES
  \* @type: Bool;
  fresh,
  \* @type: Bool;
  retry,
  \* @type: Bool;
  terminal,
  \* @type: Bool;
  selected,
  \* @type: Bool;
  transferComplete

\* @type: <<Bool, Bool, Bool, Bool, Bool>>;
vars == <<fresh, retry, terminal, selected, transferComplete>>

HeartbeatReady ==
  ~terminal /\ (fresh \/ (HeartbeatReadsRetry /\ retry))

Init ==
  /\ fresh = TRUE
  /\ retry = FALSE
  /\ terminal = FALSE
  /\ selected = FALSE
  /\ transferComplete = FALSE

MoveFreshToRetry ==
  /\ fresh
  /\ ~retry
  /\ ~terminal
  /\ fresh' = FALSE
  /\ retry' = TRUE
  /\ terminal' = terminal
  /\ selected' = selected
  /\ transferComplete' = TRUE

Select ==
  /\ HeartbeatReady
  /\ ~selected
  /\ selected' = TRUE
  /\ UNCHANGED <<fresh, retry, terminal, transferComplete>>

Terminalize ==
  /\ ~terminal
  /\ fresh \/ retry
  /\ terminal' = TRUE
  /\ fresh' = FALSE
  /\ retry' = FALSE
  /\ selected' = selected
  /\ transferComplete' = transferComplete

Next == MoveFreshToRetry \/ Select \/ Terminalize

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Select)

TypeOK ==
  /\ fresh \in BOOLEAN
  /\ retry \in BOOLEAN
  /\ terminal \in BOOLEAN
  /\ selected \in BOOLEAN
  /\ transferComplete \in BOOLEAN

Inv_CustodyExclusive ==
  ~fresh \/ ~retry

Inv_TerminalIsNotReady ==
  terminal => ~HeartbeatReady

Inv_CompletedTransferCannotAppearIdle ==
  transferComplete /\ retry /\ ~terminal => HeartbeatReady

Inv_SelectionRequiresOwnedWork ==
  selected => fresh \/ retry \/ terminal

Live_TransferredWorkEventuallySettles ==
  transferComplete ~> (selected \/ terminal)

=============================================================================
