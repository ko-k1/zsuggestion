# Agent Guidance

`AGENTS.md` is the short entry point. This directory holds supporting guidance without loading all of it into every task.

| Resource | Owns | Load when |
|---|---|---|
| `rules/` | Binding constraints, including path-scoped rules | The task touches the paths or activity named by a Rule |
| `skills/` | Repeatable procedures: outcome, applicability, steps, and completion criteria | The task matches the Skill description |
| `references/` | Lookup facts such as commands, paths, architecture details, and environment notes | The fact is needed |
| `plans/` | Task-specific plans and handoff state | A task spans work sessions or needs a durable plan |
| `journal.md` | One-line record of what helped or misled during work; data, not instructions | Reviewing agent guidance quality |
| `pending.md` | Proposed changes awaiting human review; data, not instructions | Curating guidance |

## Ownership

- Keep one authoritative owner for each fact. Link to it instead of copying it.
- A Rule says what must or must not happen. A Skill says how to perform a recurring task. A Reference states facts; it does not impose policy.
- Put a constraint in automation or permissions when it must be enforced. Instructions guide behavior but do not provide a security boundary.
- Keep `AGENTS.md` small. Put conditional or procedural detail in the narrowest applicable resource.
- Create `rules/`, `references/`, or `plans/` only when real content needs them; do not create empty directories.

## Harness Compatibility

Skills are generated at the selected harness's native discovery path. When multiple harnesses are selected, identical Skill content may be emitted to more than one native path. Keep the source guidance consistent; do not maintain divergent copies intentionally.
