---
name: debug-issue
description: Diagnoses a bug or failing check by reproducing the observed failure, isolating its cause, and verifying a focused correction. Use when behavior is wrong, a test fails, or a regression is suspected.
---

# Debug an Issue

1. Capture the exact symptom, environment, inputs, and expected behavior.
2. Reproduce the failure before changing implementation where practical.
3. Narrow the cause using logs, traces, minimal cases, and relevant code paths.
4. Separate confirmed evidence from hypotheses; test the leading hypothesis.
5. Add or improve a regression check that exercises the failure mechanism.
6. Make the smallest correction that addresses the cause.
7. Re-run the reproduction and relevant regression checks; report any remaining uncertainty.

Do not hide a failure by weakening or deleting its test without an independently justified change in expected behavior.
