# ADR-0001: Adopt the repository template

- Status: Accepted
- Date: 2026-11-21

## Context

The repository had a README plus architecture and roadmap documents, but no
shared documentation index, vision or status page, changelog, project policies,
or convention for recording decisions. Documentation ownership and contribution
expectations were implicit, and the README duplicated status and planning
content.

## Decision

Adopt the shared project template's documentation and policy layout, adapted to
this project (source: <https://github.com/ko-k1/template>):

- Keep a single documentation index in `docs/INDEX.md`.
- Separate purpose (`VISION.md`), structure (`ARCHITECTURE.md`), present reality
  (`STATUS.md`), and future intent (`ROADMAP.md`).
- Record consequential decisions as ADRs under `docs/adr/`.
- Add `CHANGELOG.md` for user-visible changes, the template's contributor
  policies and editor defaults, and generated agent guidance for the supported
  harnesses.

Code, build configuration, CI behavior, and the project license are unchanged
by this decision.

## Consequences

- Readers have one entry point and a clear split between current state and
  planned work.
- Contributors have explicit commit, verification, support, security, and
  conduct expectations.
- The repository must keep the new documents aligned with implemented behavior
  and replace template placeholders rather than copying them blindly.
- There is no runtime behavior change.

## Alternatives

- Keep the existing ad-hoc documentation. This was rejected because status and
  planning content already duplicated the README and had no shared index.
- Copy the template placeholders verbatim. This was rejected because empty
  placeholders would misrepresent the project and leave policy contacts
  undefined.
