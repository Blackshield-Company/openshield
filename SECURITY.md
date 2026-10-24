# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 1.1.x   | Yes       |
| < 1.1   | No        |

## Reporting a vulnerability

Please do **not** open a public issue for security problems.

Report privately through GitHub's
[private vulnerability reporting](https://github.com/Blackshield-Company/openshield/security/advisories/new)
("Security" tab → "Report a vulnerability"). Include:

- the OpenShield version (`openshield --version`) and OS,
- steps to reproduce or a proof of concept,
- the impact you observed (e.g. tool approval bypass, sandbox escape, secret leakage).

You should get an acknowledgement within 7 days. Once a fix is ready we will
publish a patched release and a GitHub Security Advisory, crediting you unless
you prefer otherwise.

## Scope

OpenShield runs model-chosen tools (shell, filesystem, git) on your machine.
Issues of particular interest:

- bypasses of the tool approval gate, sudo/sensitive-path checks or the working-directory sandbox,
- secrets or PII leaking into model requests, logs or the memory database,
- prompt-injection paths that lead to unapproved tool execution,
- integrity problems in `install.sh` or release artifacts.

Behaviour you explicitly enabled (for example YOLO/autonomous mode approving
risky commands) is working as designed and is out of scope.
