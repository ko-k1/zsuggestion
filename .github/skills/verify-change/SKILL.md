---
name: verify-change
description: Verifies a code, configuration, or documentation change against its intended behavior and reports evidence and remaining uncertainty. Use before declaring meaningful work complete.
---

# Verify a Change

1. Identify the changed behavior or claim and its observable success and failure conditions.
2. Find the checks documented by the repository. Do not invent commands.
3. Run the narrowest relevant check, then broader checks when the change warrants them.
4. Exercise the real boundary when unit or build checks cannot establish runtime behavior.
5. Inspect the diff and generated artifacts.
6. Report exact checks and outcomes, what was not run, and behavior that remains unverified.

A passing check is evidence only for the behavior it actually exercises.
