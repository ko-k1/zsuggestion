---
name: plan-change
description: Plans a non-trivial repository change by clarifying scope, affected areas, risks, and verification before implementation. Use for multi-file changes, architectural work, migrations, or when the user asks for a plan.
---

# Plan a Change

Produce a concise plan that makes the intended change and its verification clear.

1. Inspect the relevant implementation, documentation, tests, and repository conventions.
2. State the goal, scope, non-goals, assumptions, and unresolved questions.
3. Identify likely files or components, behavior changes, dependencies, and compatibility or security risks.
4. Define observable acceptance conditions and the checks that can provide evidence for them.
5. Present the plan before implementation when the scope is material or a decision is needed. Do not ask for approval again when the user has already authorized the same scope.
6. Update the plan when evidence invalidates an assumption or changes the scope.

Do not turn a plan into a commitment to implement every related improvement.
