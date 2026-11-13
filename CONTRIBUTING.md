# Contributing to Dapple Prompt

Thanks for your interest in improving Dapple Prompt. Bug reports, fixes, documentation and new features are all welcome. This guide explains how to report issues, how to send a pull request, how contributions are licensed, and how to become a maintainer.

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting issues

- **Security vulnerabilities:** don't open a public issue. Follow [SECURITY.md](SECURITY.md).
- **Bugs:** open a GitHub issue with what you did, what you expected, what happened instead, and the relevant logs or error output. Include the commit you're on (`git rev-parse --short HEAD`), your OS, and which service (api, conduit, flow, …) is involved. A minimal guide or request that reproduces the problem helps most.
- **Feature ideas:** open an issue describing the problem you want solved before writing a large change, so we can agree on the approach first. Small fixes can go straight to a pull request.

Search existing issues first; adding detail to an existing one is better than a duplicate.

## Development setup

- [ONBOARDING.md](ONBOARDING.md) sets up the full local stack (microk8s, Postgres, the services). On macOS, microk8s runs in an OrbStack VM; see `docs/books/journey-builder-guide/development/orbstack-setup.md`.
- [README.md](README.md) gives an overview of the workspace, and [docs/books/](docs/books/README.md) has the long-form documentation.
- Every crate has an `AGENTS.md` describing its conventions. [CLAUDE.md](CLAUDE.md) collects the cross-cutting rules. They're written for coding agents, and they're the most precise statement of the project's conventions for humans too.

The tooling runs through Nushell (`nu -c '...'`); the environment variables, kubectl context and helper commands are set up by the Nushell profiles.

### Before you open a pull request

Most crates can be checked without the cluster:

```bash
cargo fmt --all
SQLX_OFFLINE=true cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p dapple-data --locked
cargo test -p dapple-render --locked
```

Integration tests for the API crates need the local Postgres (`DAPPLE_PROMPT_DATABASE_URL`). Run the ones covering the code you changed.

Install the repo's pre-commit hook to run the formatting and SQL checks automatically:

```bash
git config core.hooksPath assets/git-hooks
```

Things reviewers will check:

- **SQL:** edit the `.sqlc` sources in `crates/dapple-data/assets/sqlc/`, never the generated `assets/.sql/`. Afterwards run `dapple sqlc compose`, which also refreshes the `.sqlx/` offline cache, and commit the regenerated `.sql` and `.sqlx/` files together.
- **Migrations** are append-only. Never edit a migration that has already merged; add a new one.
- **Tests:** new behaviour comes with a test. Tests for repair or backfill endpoints must corrupt the database state first and verify the repair with a query (see "Testing Repair Operations" in `crates/dapple-prompt-api/AGENTS.md`).
- **No secrets:** CI scans the full history with gitleaks. Use obviously fake placeholder values in docs and tests.

## Pull requests

1. Fork the repository and create a branch from `main`.
2. Keep each PR focused on one change. Separate refactors from behaviour changes when you can.
3. Write commit messages that say what changed and why.
4. **Sign off every commit** (see below).
5. Open the PR against `main`. Describe the change, why it's needed, and how you tested it, and link the issue it addresses.
6. CI must pass: Rust tests and clippy, the secrets scan, the dependency license check and the DCO check. A maintainer will review; please respond to feedback by pushing new commits rather than force-pushing over review history, until the reviewer asks you to squash.

Maintainers merge PRs once they're approved and green. Workflows on PRs from first-time contributors need a maintainer's approval before they run, so don't worry if CI doesn't start immediately.

## Licensing of contributions

Dapple Prompt is licensed under the [Apache License 2.0](LICENSE). By contributing, you agree that your contribution is licensed under the Apache License 2.0, as described in section 5 of the license ("inbound = outbound"). You keep the copyright to your work; there is no copyright assignment and no Contributor License Agreement.

### Developer Certificate of Origin (DCO)

We use the [Developer Certificate of Origin](https://developercertificate.org/) to record that you have the right to submit your contribution. Every commit must carry a `Signed-off-by` line with your real name and an email address you can be reached at:

```
Signed-off-by: Jane Developer <jane@example.com>
```

`git commit -s` adds it for you. To sign off commits you've already made on your branch:

```bash
git rebase --signoff main
git push --force-with-lease
```

The sign-off means you certify the following:

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

If your employer has rights to code you write, make sure you're allowed to contribute it before signing off.

Code generated with AI assistants is welcome, but you're responsible for it as if you wrote it: review it, test it, and only sign off if you have the right to submit it.

## Trademarks

The license covers the code, not the Dapple names and logos. See [TRADEMARK.md](TRADEMARK.md) before naming a fork or a hosted service.

## Becoming a maintainer

The project is maintained by the DappleAI Co. team, and we want community maintainers too. The path is:

1. **Contributor:** you've had pull requests merged.
2. **Triager:** after several good contributions and helpful participation in issues and reviews, a maintainer may invite you to the triage role. Triagers label, close and reproduce issues, and review pull requests.
3. **Maintainer:** triagers who show sustained, good-judgment reviews in an area of the codebase (for example a crate, the docs, or the Nushell tooling) can be nominated by an existing maintainer. Maintainers can approve and merge PRs. Nominations are discussed among the current maintainers and accepted by consensus.

You can also ask: if you want to take on more responsibility, say so in an issue or to a maintainer. Maintainers who become inactive for an extended period may move to emeritus status, and can return at any time.

Maintainers are expected to follow the Code of Conduct, review promptly and kindly, and keep sensitive matters (security reports, conduct reports) private.

## Questions

Open a GitHub issue labelled `question`. Thanks for contributing!
