# OCI merge-recovery soak scheduler

This OCI Function replaces GitHub Actions `schedule` delivery for the merge-recovery soak. OCI Resource Scheduler invokes the Function at both UTC times that can represent 19:30 Pacific. A Bash handler in a custom Docker image dispatches GitHub only when the resolved slot is Monday through Friday at exactly 19:30 Pacific.

```mermaid
flowchart LR
    RS[OCI Resource Scheduler<br/>02:30 and 03:30 UTC] --> FN[OCI Function]
    FN --> V[OCI Vault<br/>GitHub App private key]
    FN --> API[GitHub workflow dispatch API]
    API --> WF[Merge Recovery Soak]
```

## GitHub App

Create or update a GitHub App installed only on `F1R3FLY-io/f1r3node-rust` with:

- Actions: read and write
- Metadata: read

Store its PEM private key in an OCI Vault secret. Record the App ID, installation ID, secret OCID, function compartment OCID, and subnet OCID. The deployment script never accepts the PEM contents directly.

## Deploy

The selected subnet must provide outbound HTTPS access to `api.github.com`. Authenticate Docker to the regional OCIR endpoint before deploying.

```bash
export OCI_COMPARTMENT_OCID='<function-and-scheduler-compartment-ocid>'
export OCI_SUBNET_OCID='<outbound-enabled-subnet-ocid>'
export GITHUB_APP_ID='<github-app-id>'
export GITHUB_APP_INSTALLATION_ID='<github-app-installation-id>'
export GITHUB_APP_PRIVATE_KEY_SECRET_OCID='<vault-secret-ocid>'

scripts/oci/deploy-soak-scheduler.sh
```

The script builds and pushes the Bash/HotWrap Function image, creates or updates the Function, creates the 02:30 and 03:30 UTC schedules, and creates least-scope dynamic groups and IAM policy statements for Function invocation and Vault access. The image pins the OCI CLI and HotWrap base images by digest and contains the Bash, jq, curl, OpenSSL, and timezone tools used by the handler.

Deploy only after the workflow change reaches `master`, because GitHub resolves `workflow_dispatch` from the configured `master` ref.

## Verify

Run the Bash tests from the repository root:

```bash
bash oci/soak-scheduler/test-handler.sh
```

The tests cover PDT and PST selection, daily and weekend routing, weekend-day suppression, late invocation rejection, duplicate suppression, and the GitHub dispatch payloads.

For the live smoke test, invoke the currently eligible payload within 15 minutes of its UTC slot and confirm:

1. Exactly one run appears with `Merge Recovery Soak [scheduled:<epoch>]` as its title.
2. The schedule gate reports the same slot epoch and Pacific 19:30.
3. Invoking the same payload again reports `duplicate` and creates no second run.
4. The other UTC payload reports `ineligible`.

## Timing and failure behavior

- OCI schedules are UTC-only, so both UTC slots remain necessary across Pacific DST changes.
- Invocations more than 15 minutes late fail instead of launching an hours-late soak.
- Saturday and Sunday Pacific slots do not dispatch GitHub.
- The Function checks recent workflow titles before dispatching to suppress Resource Scheduler retries.
- The workflow independently preserves the intended slot epoch for duration, checkpoint, and restart calculations.
