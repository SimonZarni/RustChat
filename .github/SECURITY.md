# Security Policy

RustChat is in early development and has not had an independent security review. Do not rely on it to protect sensitive communication yet.

## Reporting a vulnerability

Please report suspected vulnerabilities **privately**:

1. Go to the repository's **Security** tab and choose **Report a vulnerability** (GitHub private vulnerability reporting): <https://github.com/SimonZarni/RustChat/security/advisories/new>
2. Include what you found, how to reproduce it, the affected version or commit, and the impact you expect.

Please do not open a public issue or pull request for a vulnerability, and do not disclose it publicly until a fix is available or we have agreed a date together. Do not access, modify or retain other people's data while testing, and do not run tests that degrade a service you do not own.

## What to expect

This is a small project without a funded security team, so there is no guaranteed response time. We aim to acknowledge a report within about 7 days, to keep you updated, and to credit you in the advisory if you wish.

## Scope

The code in this repository. Vulnerabilities in a dependency should also be reported to that dependency's maintainers; tell us if RustChat's use of it makes the impact worse.

The security requirements and threat assumptions are in [docs/SECURITY.md](../docs/SECURITY.md) and [docs/THREAT_MODEL_AUTH.md](../docs/THREAT_MODEL_AUTH.md). Known gaps are listed there; a report about a gap already listed as not yet implemented is still welcome but is unlikely to be treated as urgent.
