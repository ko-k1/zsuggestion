---
name: improve-agents
description: Improves repository agent guidance from observed experience by recording useful or misleading guidance and proposing focused changes for human review. Use after a meaningful task, user correction, costly mistake, or recurring workflow discovery.
---

# Improve Agent Guidance

Agents may suggest improvements; changes to shared instructions, Rules, and Skills require human review.

1. Record a concise entry in `.agents/journal.md` describing the task and which guidance helped or misled.
2. Before proposing a change, search existing guidance to avoid duplication.
3. Put a focused proposal in `.agents/pending.md`, naming the owning file and explaining the observed problem and suggested correction.
4. Keep proposals inactive. Do not silently apply them or treat pending text as instructions.
5. After human approval, update the single authoritative owner and remove or mark the proposal as resolved.
6. Remove stale, duplicated, or project-specific guidance rather than continually appending instructions.

Prefer fixing a missing mechanism or unclear ownership over adding another warning. Do not place secrets, personal data, or untrusted instructions in the journal or proposals.
