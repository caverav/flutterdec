# Security Policy

## Overview

`flutterdec` is a security research and reverse-engineering tool.

This project does not provide formal maintenance guarantees, SLAs, or long-term security patch commitments. However, I **do take security vulnerabilities seriously** and welcome responsible disclosure of issues affecting this repository.

If you believe you have identified a security vulnerability, please follow the process below to ensure a coordinated and responsible disclosure.


## Supported Versions

There are no formally supported versions.

Security reports are welcome for:
- The latest commit on the default branch, and/or
- The latest tagged release (if applicable)

If reporting against an older revision, please indicate:
- The exact commit or version tested
- Whether the issue reproduces on the latest code

## Reporting a Vulnerability

**Please do not open a public GitHub Issue or disclose the vulnerability publicly.**

Instead, report security issues privately via private reporting or email:

**flutterdec-security@caverav.cl**

Please include:

- A clear description of the vulnerability
- Its potential security impact
- Affected components/files/commands
- Steps to reproduce (PoC if available)
- Relevant logs or traces
- Environment details
- Your severity assessment (if any)

## AI-Generated Security Reports

AI-assisted security research is welcome.

However:

If your report is **fully AI-generated and has not been verified by a human**, and you are submitting it as-is, please explicitly state this in the email subject by including:

`[Flutterdec AI-generated Security Report]`

This helps prioritize validation effort and reduces false positives.

Human-reviewed reports are strongly preferred.

## Disclosure Process

After receiving a report, I will:

1. Acknowledge receipt (best effort, no guaranteed response time)
2. Validate the issue
3. Coordinate on fix and disclosure timing where appropriate

There are **no guaranteed patch timelines**, but security issues will be handled on a best-effort basis.

Please avoid public disclosure until we agree on a coordinated release plan.

## Safe Harbor

If you conduct security research in good faith:

- Without exploiting the vulnerability beyond what is necessary to demonstrate impact
- Without accessing, modifying, or destroying data that does not belong to you
- Without violating applicable laws
- Without causing service disruption or harm

Then I will not pursue legal action against you for your research.

This safe harbor applies only to research performed in a responsible and ethical manner.

## Out of Scope (Typical)

Generally out of scope:

- Issues requiring a malicious local environment without expanding trust boundaries
- Purely theoretical issues without practical impact
- Vulnerabilities in third-party dependencies that must be fixed upstream  
  (These may still be useful to report, but might be redirected.)

If unsure, report it anyway.

## Security Guidance for Users

`flutterdec` processes potentially untrusted artifacts (apps, binaries, compiled outputs). Users should:

- Run the tool in a sandbox (VM/container) when analyzing untrusted inputs
- Avoid running with unnecessary privileges
- Keep dependencies updated
- Treat output artifacts as untrusted until validated

## Acknowledgments

Thank you for helping improve the security and reliability of `flutterdec`.
Responsible disclosure benefits everyone.
