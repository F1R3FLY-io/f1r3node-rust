# Temporary Session Ownership

## Observed failure

B25 creates an unowned temporary session inside a disposable container. A live fixture writer holds its data file open.

The session directory has a modification time two hours in the past. The writer appends data without changing the directory modification time.

The baseline driver deletes the directory during disk hygiene. The writer still holds the deleted file open when the fixture observes the result.

Directory age does not establish ownership or writer termination. Deleting an open file also does not establish immediate space reclamation.

## Correction

The driver no longer deletes temporary sessions by age. It reports that ownership remains unconfirmed and preserves the session directories.

The disk admission checks remain enabled. When available space remains below the admission threshold, the driver refuses work and records one protection failure.

This correction deliberately retains temporary data. Safe reclamation requires a separate ownership and termination protocol, not a longer age threshold.

## Model correspondence

`CleanupOwnership` represents one unowned session with a live writer. Its directory age is either old or recent.

The negative control enables the age-only sweep. It requires exit 12 on `UnownedSessionPreserved`.

The corrected configuration disables that sweep and has four distinct states. The production fixture covers the old-directory counterexample.

The model assumes that its sweep transition can execute. Its fairness condition does not establish a time bound.

## Limits

The fixture uses real files and a live fixture writer, but no node process. External disk samples and Docker commands remain controlled boundaries.

This cycle does not establish Docker resource ownership, image preservation, complete writer termination, crash durability, or disk-growth bounds.

The correction can cause earlier disk refusal because it preserves more data. D2, D3, hosted checks, maintainer review, and acceptance remain pending.
