# 10. Public-repo hygiene

**Status:** Accepted, 2026-10-25

## Context

The repo is public, and rexboard is built and tested against a real fleet of agents. Fixtures, screenshots and demos come from real panes, which carry hostnames, session labels, paths and conversation text.

## Decision

- Use generic names in code, tests, fixtures, docs, screenshots and demos: hosts like `studio` and `mini`, sessions like `api` and `web`, panes like `seat-a`.
- Never commit tailnet hostnames, real session or tab labels, home paths, transcripts, hook payloads with real text, or people's names.
- Scrub a recorded fixture before it lands (scratch paths become `/work/project`).
- The demo is recorded on a private, throwaway Rex server, and its cast is checked against a deny list before it is written.

## Consequences

- A test or screenshot from a real run needs a pass before it is committed.
- Reviewers check for leaks as they check for bugs.

## Supersedes

None.
