---
name: review-change
description: Reviews a diff for correctness, regressions, security, compatibility, and missing verification without rewriting the implementation. Use when asked to review changes or before a consequential merge.
---

# Review a Change

1. Read the diff and the surrounding implementation; identify the intended behavior.
2. Trace affected callers, data flows, trust boundaries, and compatibility surfaces.
3. Look for concrete defects, edge cases, failure paths, and unintended scope expansion.
4. Check whether tests exercise the changed behavior and meaningful failure cases.
5. Report findings first, ordered by severity, with file and line references where possible.
6. Distinguish confirmed defects from questions or risks; do not report style preferences as correctness bugs.

Do not modify files unless the user separately requests implementation.
