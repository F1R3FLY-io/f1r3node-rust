# Daily Back-merge Provenance

## Scope

[US-013](../UserStories.md#us-013-trusted-daily-back-merges) and TASK-029-1 cover the review follow-up for PR #792.
The image publication dependencies and the accepted `ca-soaked` label risk do not change.

## Design

A `pull_request_target` workflow checks out only the immutable base commit.
It never checks out or executes pull request code.
It publishes the `CA Daily Provenance` commit status on the event head SHA.
The daily queue requires this status in addition to its existing checks.

The validator resolves the source commit from the branch name.
The source must belong to `ca-release`.
The pull request must come from this repository and the promotion app.
The validator accepts the source itself or a bounded chain of clean base merges.
Each merge has two parents, with the previous head first and an ancestor of the captured base second.
The merge tree must equal the tree that `git merge-tree --write-tree` computes for those parents.
A conflict, altered tree, extra commit, missing object, or chain beyond 128 merges fails the check.

## Verification

Bash tests construct Git objects in private fixtures.
They cover source membership, repeated base merges, altered merge trees, conflict rejection, and invalid metadata.
Workflow checks verify the trusted checkout and the head-bound status.
The existing protected-head tests and workflow invariant checks must still pass.

## Deployment

The validator workflow and its scripts must reach `ca-canary` before the daily queue can pass its new status.
The default branch must receive the updated Mergify configuration before Mergify enforces the new status.
Missing or failed validation blocks the daily queue.
