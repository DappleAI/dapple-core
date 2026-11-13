# Security Policy

## Reporting a vulnerability

**Please don't report security problems in public issues, pull requests or discussions.**

Report them privately through GitHub:

1. Go to this repository's **Security** tab.
2. Click **Report a vulnerability**.
3. Describe the issue, the affected component (crate, endpoint or script), how to reproduce it, and the impact you expect.

Only the maintainers can see the report. We'll acknowledge it within **3 business days**, keep you updated while we investigate, and let you know when a fix is released. If you'd like credit in the advisory, say so in the report.

Please give us a reasonable chance to fix the problem before disclosing it publicly. We're happy to agree a disclosure date with you.

## Supported versions

The project hasn't cut versioned releases yet. Security fixes land on the `main` branch; please check that the problem still exists on the latest `main` before reporting.

## Scope

In scope:

- The Rust crates in this repository (`crates/`), including the internal and external API surfaces, authentication and service tokens, tenant isolation between companies, the Nushell task sandbox in `dapple-prompt-flow`, and guide/tarball ingest.
- The deployment manifests and scripts in `assets/`, where a default would make a deployment insecure.

Out of scope:

- The local development cluster defaults (`*.kube.dappleai.dev`, `dapple:ai` database credentials, self-signed local CA). They're intentionally development-only and must never be used in production.
- Vulnerabilities in third-party dependencies that don't affect how this project uses them. Please report those upstream. If our usage makes one exploitable, report it here.
- Findings from automated scanners without a demonstrated impact.

## Handling secrets

If you find a credential committed to this repository, report it the same way rather than opening an issue. CI scans the full git history for secrets with gitleaks (`.gitleaks.toml`).
