# Security Policy

## Reporting a Vulnerability

Please report security vulnerabilities **privately** — do not open a public issue, pull
request, or discussion.

Use GitHub's private vulnerability reporting: open the repository's **Security** tab and click
**"Report a vulnerability."** This creates a private advisory visible only to maintainers,
where we can triage and coordinate a fix with you.

Include where possible:

- A description of the vulnerability and its impact
- Steps to reproduce, ideally a proof of concept
- Affected component(s), branch, and commit
- Any suggested remediation

## What to Expect

- We aim to acknowledge your report within **3 business days**.
- We follow **coordinated disclosure**: please give us a reasonable window to ship a fix
  before public disclosure. We will credit you when we publish, if you wish.

## Scope

In scope: the node, consensus, cryptography, networking, and RPC/API surfaces in this
repository. Out of scope: third-party dependencies (report those upstream) and issues
requiring physical access to or a compromised host.

## Supported Versions

F1R3FLY is under active development and has no mainnet version. The project publishes stable
and canary releases. A fix ships in the next release. Report against the latest `dev`, and name
the release if you found the issue in a release.

## Maintainers

The [security advisory process](docs/security-advisory-process.md) tells maintainers how to
triage, fix, and publish an advisory.
