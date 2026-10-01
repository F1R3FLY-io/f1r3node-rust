---------------- MODULE RSpaceWindowsDurableAuthority ----------------
EXTENDS Naturals, TLC

(***************************************************************************
This finite model is the contract for a future native Windows adapter at the
existing durable/io.rs seam. The environment varies filesystem qualification,
directory and child identity/security checks, name identity, and lock
availability. A publication is acknowledged only after the installed name is
durable. Failed pre-install operations preserve the old authority; a failed
post-install sync remains indeterminate until restart. The three switches are
single-defect negative controls and must be FALSE in the base configuration.
***************************************************************************)

CONSTANTS SkipDirectoryCheck, SkipNameIdentity, SkipDirectorySync

Filesystems == {"ntfs", "other"}
Stages == {"new", "directory", "leased", "opened", "sealed",
           "installed", "indeterminate", "durable", "acknowledged",
           "refused", "restarted"}
Authorities == {"old", "new"}
Observations == {"none", "old", "new", "reject"}

VARIABLES filesystem, directoryOwner, directoryAcl, directoryReparse,
          childOwner, childAcl, childReparse, childOrdinary, childSingleLink,
          nameMatches, lockAvailable, stage, directoryPinned, lockHeld,
          childPinned, childSynced, official, durable, acknowledged, observed

vars == <<filesystem, directoryOwner, directoryAcl, directoryReparse,
          childOwner, childAcl, childReparse, childOrdinary, childSingleLink,
          nameMatches, lockAvailable, stage, directoryPinned, lockHeld,
          childPinned, childSynced, official, durable, acknowledged, observed>>

environment == <<filesystem, directoryOwner, directoryAcl, directoryReparse,
                 childOwner, childAcl, childReparse, childOrdinary,
                 childSingleLink, nameMatches, lockAvailable>>

DirectorySafe == directoryOwner /\ directoryAcl /\ ~directoryReparse
ChildSafe == childOwner /\ childAcl /\ ~childReparse /\
             childOrdinary /\ childSingleLink
DirectoryAccepted == filesystem = "ntfs" /\
                     (DirectorySafe \/ SkipDirectoryCheck)
ChildAccepted == ChildSafe /\ (nameMatches \/ SkipNameIdentity)

Init ==
    /\ filesystem \in Filesystems
    /\ directoryOwner \in BOOLEAN
    /\ directoryAcl \in BOOLEAN
    /\ directoryReparse \in BOOLEAN
    /\ childOwner \in BOOLEAN
    /\ childAcl \in BOOLEAN
    /\ childReparse \in BOOLEAN
    /\ childOrdinary \in BOOLEAN
    /\ childSingleLink \in BOOLEAN
    /\ nameMatches \in BOOLEAN
    /\ lockAvailable \in BOOLEAN
    /\ stage = "new"
    /\ directoryPinned = FALSE
    /\ lockHeld = FALSE
    /\ childPinned = FALSE
    /\ childSynced = FALSE
    /\ official = "old"
    /\ durable = "old"
    /\ acknowledged = FALSE
    /\ observed = "none"

OpenDirectory ==
    /\ stage = "new"
    /\ DirectoryAccepted
    /\ stage' = "directory"
    /\ directoryPinned' = TRUE
    /\ UNCHANGED <<lockHeld, childPinned, childSynced, official,
                    durable, acknowledged, observed>>

RefuseDirectory ==
    /\ stage = "new"
    /\ ~DirectoryAccepted
    /\ stage' = "refused"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged, observed>>

AcquireLease ==
    /\ stage = "directory"
    /\ lockAvailable
    /\ stage' = "leased"
    /\ lockHeld' = TRUE
    /\ UNCHANGED <<directoryPinned, childPinned, childSynced, official,
                    durable, acknowledged, observed>>

RefuseLease ==
    /\ stage = "directory"
    /\ ~lockAvailable
    /\ stage' = "refused"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged, observed>>

OpenBoundChild ==
    /\ stage = "leased"
    /\ ChildAccepted
    /\ stage' = "opened"
    /\ childPinned' = TRUE
    /\ UNCHANGED <<directoryPinned, lockHeld, childSynced, official,
                    durable, acknowledged, observed>>

RefuseChild ==
    /\ stage = "leased"
    /\ ~ChildAccepted
    /\ stage' = "refused"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged, observed>>

SealChild ==
    /\ stage = "opened"
    /\ stage' = "sealed"
    /\ childSynced' = TRUE
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, official,
                    durable, acknowledged, observed>>

RefuseChildSync ==
    /\ stage = "opened"
    /\ stage' = "refused"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged, observed>>

InstallBoundChild ==
    /\ stage = "sealed"
    /\ stage' = "installed"
    /\ official' = "new"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    durable, acknowledged, observed>>

RefuseReplace ==
    /\ stage = "sealed"
    /\ stage' = "refused"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged, observed>>

SyncDirectory ==
    /\ stage = "installed"
    /\ stage' = "durable"
    /\ durable' = "new"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, acknowledged, observed>>

DirectorySyncFailed ==
    /\ stage = "installed"
    /\ stage' = "indeterminate"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged, observed>>

Acknowledge ==
    /\ stage = "durable" \/ (SkipDirectorySync /\ stage = "installed")
    /\ stage' = "acknowledged"
    /\ acknowledged' = TRUE
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, observed>>

RestartUncertain ==
    /\ stage \in {"installed", "indeterminate"}
    /\ observed' \in {"old", "new", "reject"}
    /\ stage' = "restarted"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged>>

RestartDurable ==
    /\ stage \in {"durable", "acknowledged"}
    /\ stage' = "restarted"
    /\ observed' = "new"
    /\ UNCHANGED <<directoryPinned, lockHeld, childPinned, childSynced,
                    official, durable, acknowledged>>

Next == UNCHANGED environment /\
        (OpenDirectory \/ RefuseDirectory \/ AcquireLease \/ RefuseLease \/
         OpenBoundChild \/ RefuseChild \/ SealChild \/ RefuseChildSync \/
         InstallBoundChild \/ RefuseReplace \/ SyncDirectory \/
         DirectorySyncFailed \/ Acknowledge \/ RestartUncertain \/
         RestartDurable)

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ filesystem \in Filesystems
    /\ stage \in Stages
    /\ official \in Authorities
    /\ durable \in Authorities
    /\ observed \in Observations

PinnedDirectoryIsTrusted ==
    directoryPinned => filesystem = "ntfs" /\ DirectorySafe

LeasePrecedesChildAuthority ==
    childPinned => directoryPinned /\ lockHeld /\ ChildSafe /\ nameMatches

PublishedNameIsSealedAndBound ==
    official = "new" => childPinned /\ childSynced /\
                        lockHeld /\ nameMatches /\ DirectorySafe

RefusalPreservesAuthority ==
    stage = "refused" => official = "old" /\ durable = "old" /\
                         ~acknowledged

AcknowledgmentRequiresDurability ==
    acknowledged => official = "new" /\ durable = "new" /\ childSynced

RestartNeverSelectsThirdAuthority ==
    stage = "restarted" =>
        /\ observed \in {"old", "new", "reject"}
        /\ (durable = "new" => observed = "new")

============================================================
