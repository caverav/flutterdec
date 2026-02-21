
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
