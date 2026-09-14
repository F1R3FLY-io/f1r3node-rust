# C5 unreadable-history conflict resolution

## Purpose and scope

This document resolves the C5 conflict that gate F3 must close before the carrier-index claim discharges.
It is a proposal for the Casper maintainer, not a claim decision or a code change.
It refines the claim to match the sound behavior and does not weaken it.
The maintainer adopts the refined statement and keeps both existing tests.

This document reads the committed claim, model, and tests.
It does not run the workload and does not change consensus behavior.

## The conflict

The [carrier-index claim](../claims/repeat-deploy-carrier-index-equivalence.md) states C5 as a blanket equality.

```text
indexed_verdict = reference_scan_verdict
```

C5 states this equality for every generated directed acyclic graph, candidate block, expiration window, and storage-availability pattern, including missing ancestors.

The test `repeat_deploy_certified_index_engagement_skips_the_scan` contradicts the blanket form.
It makes the candidate's ancestry unreadable, so a live scan of that ancestry fails.
Without a certified index, the control scan verdict is a failure, which the test asserts.
With a certified index watermark, the same candidate returns Valid, which the test states is unreachable through the scan path.

So the indexed verdict is Valid while the reference scan verdict is a failure on the same unreadable ancestry.
The blanket equality in C5 does not hold for this case.
This divergence is the intended value of the carrier index, not a defect.

## Why the two verdicts differ soundly

The reference scan reads ancestry live at verdict time.
It fails when the ancestry is unreadable, because it has no other source.
The model represents this with the `readable` variable and the `FailRead` action.

The certified index consults a durable watermark instead of a live read.
The test certifies the watermark with `set_watermark_if_absent` while the ancestry is still readable.
The watermark records the proven absence below its point at that earlier time.
A later read failure does not erase the recorded watermark, so the certified verdict remains Valid.

The two paths therefore consult different information.
The scan sees only current storage, which is unreadable.
The index sees a durable proof recorded when storage was readable.
Equality cannot hold between a live read and a durable proof that carries strictly more information.

## The model already separates the two cases

The model comment records the intended distinction.
`FallbackOnReadFailure distinguishes refusal from false absence`.
The model requires that an uncertified read failure produces a refusal, not a false absence.
It does not require the certified path to refuse when the durable proof already holds.

So the model supports the certified-skip behavior and the uncertified-refusal behavior at the same time.
The blanket C5 statement is narrower than the model and the tests together express.
The fix is to state C5 in the two cases that the model already separates.

## Proposed refinement

Replace the single blanket equality with two precise sub-claims.

C5a, uncertified equivalence.
Without a certified watermark that covers the deploy, the indexed verdict equals the reference scan verdict for every storage-availability pattern.
A read failure in this case produces the same refusal on both paths, and neither path reports a false absence.

C5b, certified soundness.
With a certified watermark that covers the deploy, the indexed verdict is Valid because the durable proof holds.
This verdict is sound, and it may differ from a live scan that lost read access after the certification.
The certified verdict never accepts a deploy that the watermark does not actually cover.

The property comparison establishes C5a on identical generated graphs with no certified watermark.
The `repeat_deploy_certified_index_engagement_skips_the_scan` test establishes C5b.
Neither sub-claim is weaker than the sound behavior, and together they cover every case the blanket form intended.

## Why this does not weaken the claim

The blanket C5 was unsatisfiable as written, because it denied the certified-skip optimization the index exists to provide.
The refinement does not remove any guarantee that the sound system provides.
C5a keeps the full equality wherever the two paths share the same information.
C5b adds the certified soundness condition, which the blanket form silently omitted.

The refinement also makes the absence-path work bound meaningful.
A certified skip is the case that avoids the expensive ancestry scan.
That skip is part of the finalization optimization that gates F1 and F2 measure.
A claim that forbids the skip would forbid the optimization.

## What the maintainer must do

The maintainer adopts C5a and C5b in the claim and records the reason.
The maintainer confirms that the certified watermark is durable across the crash and restart boundaries that gate F3 exercises.
The maintainer confirms that an uncertified read failure still refuses, so the fallback stays a refusal and never a false absence.
The maintainer keeps both tests and adds the model-to-code map that gate F2 requires.

This resolution unblocks C5 for the carrier-index claim.
It does not discharge the claim, which still needs the differential, crash-boundary, and fuzz evidence.
The finalization work bound and its repair remain gates F1 and F2.
D2, D3, and claim discharge remain pending.
