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

F1R3FLY is under active development with no released or mainnet versions yet. Report against
the latest `staging` / `dev`.
