# Casper Campaign Reservation Deployment

Preparation is complete for review. The [configuration](casper-campaign-storage.jsonc) contains the bucket request, access policy, principal bindings, and initial record template.

Deployment and live qualification remain pending. The initial record contains unresolved bindings and must not be uploaded.

## Resource and principal review

The target region is `us-sanjose-1`. The namespace is `axd0qezqa9z3`, and the compartment is `ci-runner`.

The bucket is `f1r3node-casper-campaign`. It uses private access, Standard storage, and enabled versioning. The fixed object is `task-017-12-baseline.json`.

The live inventory returned no buckets or policies in this compartment. Its parent is the tenancy, where eight policies were reviewed.

The proposed controller user is the single active member of `ci-runner-launchers`. The configuration records the exact user and group identifiers.

The reviewed policies grant that group compute and network access. They do not grant Object Storage access. The user has no other returned group memberships.

The workflow selects its user through `OCI_USER_OCID`. The inventory does not establish that this secret identifies the proposed user. Activation requires that identity check.

The local operator belongs to `Administrators`. Operator access cannot establish that the runtime policy works. Runtime checks must use the bound service principals.

The supervisor function and its proposed dynamic group do not exist. The existing scheduling function is a different resource.

The new dynamic group must match the exact supervisor function identifier. A compartment-wide function rule would include unrelated functions.

Oracle documents [function-specific dynamic groups](https://docs.oracle.com/en-us/iaas/Content/Functions/Tasks/functionsaccessingociresources.htm). The configuration leaves the matching rule unresolved until the function identifier is available.

## Access policy

The prepared policy contains three statements. The controller and supervisor receive `OBJECT_READ` and `OBJECT_OVERWRITE` for the fixed bucket and object.

The controller grant also requires the exact service user identifier. The supervisor grant applies to the dedicated dynamic group.

The third statement lets the controller read only the policy named `casper-campaign-reservation`. The controller verifies its returned statement digest before launch.

Oracle documents [policy identity conditions](https://docs.oracle.com/en-us/iaas/Content/Identity/Reference/iampolicyreference.htm) and [object-specific permissions](https://docs.oracle.com/en-us/iaas/Content/Identity/Reference/objectstoragepolicyreference.htm).

The policy grants no object creation, deletion, version deletion, restore, or listing. It grants no bucket changes, preauthenticated requests, or policy updates.

The bootstrap operator creates the initial object once. Runtime principals can overwrite that existing object but cannot recreate a missing object under this policy.

The policy does not revoke permissions from other grants. Repeat the inheritance review and principal checks before activation.

## Initial record and controller binding

The record has schema version 2, sequence zero, and five null slots. Its `campaign_id` and `config_digest` must identify the accepted controller configuration.

The five slots are `preflight`, `baseline-dev-amd64`, `baseline-dev-arm64`, `stability-dev-amd64`, and `stability-dev-arm64`.

The controller digest includes the policy identifier, supervisor configuration, candidate identities, source identities, and activation evidence. Those inputs must be complete before initialization.

The trusted digest command is:

```bash
target/debug/casper-campaign-control config-digest --config <accepted-configuration>
```

The policy digest covers the compact JSON statements array followed by one newline. Statement order remains significant. The configuration records the prepared digest.

Initialization uses `If-None-Match: *`. Subsequent updates use `If-Match` with the entity tag from the current object.

Oracle documents these [conditional upload headers](https://docs.oracle.com/en-us/iaas/tools/dotnet/2.0.0/api/Oci.ObjectstorageService.Requests.PutObjectRequest.html). A failed or ambiguous initialization requires readback, not another initialization request.

The operator must compare the exact initial bytes and record the returned entity tag. An existing object must never become an empty replacement budget.

## Deployment sequence

1. Confirm that the workflow credential identifies the proposed controller user.
2. Obtain authorization for the prepared OCI resource changes.
3. Create the private bucket from `bucket_create_request`.
4. Verify its namespace, compartment, storage tier, access type, and versioning.
5. Complete the separate supervisor deployment and record its function identifier.
6. Bind the dynamic group to that function identifier.
7. Create the dynamic group and verify its exact membership rule.
8. Create the policy from `policy_create_request`.
9. Verify the returned policy identifier, active state, statements, and statement digest.
10. Complete the accepted controller configuration and calculate its digest.
11. Bind both unresolved fields in `initial_state_template` to that configuration.
12. Verify all five slots are null and the sequence is zero.
13. Submit the initial object once with `If-None-Match: *`.
14. Verify its exact bytes, entity tag, and version identifier.
15. Complete the runtime qualification below before campaign activation.

These steps describe the required order. They do not provide the missing supervisor deployment or authorize a campaign launch.

## Runtime qualification

Use `task-017-12-storage-qualification.json` for destructive permission checks and competing writes. Do not use the authoritative object or consume campaign slots.

The probe requires a separate temporary policy with the same runtime permissions and its exact object name. Its initial creation belongs to the operator.

1. Verify a successful read with each runtime principal before testing denied operations.
2. Read one entity tag into two independent writers.
3. Submit different probe updates with that same entity tag.
4. Verify that exactly one update succeeds.
5. Verify that the rejected update changes no bytes or version.
6. Simulate a lost response and reconcile the stored record through readback.
7. Verify that runtime creation, deletion, version deletion, and restore fail on probe resources.
8. Verify that both principals lack access to an unrelated object.
9. Verify that the authoritative record and its five slots remain unchanged.
10. Retain request identities, results, entity tags, version identifiers, and policy digests.

A rejection without a successful identity control is incomplete permission evidence. Operator success cannot replace runtime-principal evidence.

## Enforcement limits

IAM checks resource permissions. It does not require `If-Match` or inspect the reservation JSON for valid transitions.

The controller and supervisor must remain trusted writers. Their credentials must not reach workload processes or unrelated automation.

Versioning retains earlier values, but it does not prevent an authorized writer from uploading old bytes again. Oracle describes [versioning behavior](https://docs.oracle.com/en-us/iaas/Content/Object/Tasks/usingversioning.htm).

Privileged changes, alternative writer paths, and configuration replacement invalidate the reviewed binding. Missing or replaced state must block execution.

The preparation does not enable tenancy-wide deny policies or alter administrator permissions. It does not establish live conditional-update behavior or source-bound claim acceptance.

## Evidence

The [preparation report](../casper/cbc-evidence/runs/casper-campaign-storage-preparation-20260922-01/report.json) records source identities, inventory results, request digests, and remaining bindings.

Bulk inventory and extracted request files remain under `target/task-017-12-storage-preparation-20260922-01/`. No OCI resources changed during preparation.
