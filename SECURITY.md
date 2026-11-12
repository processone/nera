# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in nera, please report it
responsibly. **Do not open a public GitHub issue.**

Instead, please email: **security@process-one.net**

Include:

- A description of the vulnerability
- Steps to reproduce (if applicable)
- The potential impact
- Any suggested fix (if you have one)

## Response Timeline

We aim to:

- Acknowledge your report within **48 hours**
- Provide an initial assessment within **1 week**
- Release a fix for confirmed vulnerabilities as soon as practical

## Scope

nera is an ahead-of-time compiler that produces standalone native binaries.
Security-relevant areas include:

- **Compiler**: Malicious `.erl` input should not compromise the build machine
  (e.g., path traversal in `-include`, unbounded memory in parsing)
- **Runtime**: The generated binary's runtime (process isolation, memory safety
  in the GC, FFI boundary correctness)
- **Dependency fetching**: `nera deps get` downloads from hex.pm and git —
  transport security and tarball handling matter

Out of scope:

- Bugs in user-written Erlang programs (application logic)
- Denial-of-service via intentionally large inputs (compiler resource limits
  are not yet implemented)

## Supported Versions

Only the latest release on `main` is actively supported with security fixes.
