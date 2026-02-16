# Argus Sprint Template
# This file is the canonical sprint prompt. Each new sprint session reads this.
# Update this file as features ship and new ones are identified.

## What Is Argus
AI code review CLI tool in Rust. One binary, 8 crates. Repo: ~/argus, GitHub: Meru143/argus.

## Research
Always read before implementing:
- ~/.openclaw/workspace/research/argus/competitive-deep-dive.md
- ~/.openclaw/workspace/research/argus/claude-code-best-practices-report.md (MUST READ before CC)
- ~/.openclaw/workspace/research/argus/PROJECT-PLAN-REVIEW.md
- ~/argus/SPRINT-LOG.md (what's already shipped)

## Feature Backlog (update as features ship)
1. PR description generation — ~80% done in dirty tree
2. Incremental review — only review NEW changes after force-push
3. Hotspot-aware review prioritization — USE gitpulse data
4. Learning from feedback — thumbs up/down → persistent rules
5. More tree-sitter languages — PHP, Kotlin, Swift
6. [When backlog empty: research competitors, identify new features, never stop]

## Rules
- Individual commits per feature (NO squash merge)
- cargo test + clippy + fmt before committing
- gh run list after each push
- Update ~/argus/SPRINT-LOG.md after each feature
- Disk: run cargo clean if needed (~13GB free)
- When backlog empty: research new features autonomously

## CC Orchestration
- READ the CC best practices report BEFORE starting CC
- tmux skill for CC control
- One task per CC session, /clear between
- Proxy on port 8080, settings at ~/.claude/settings.json
- Rate limits: wait and retry
- Stuck/degraded: kill and restart
- Write state to files before /clear

## Sprint Chain
When done:
1. Check ~/argus/SPRINT-CONTROL — if PAUSED=true or missing, stop
2. If not paused, schedule wake event in 5 min: "SPRINT_CHAIN: Start next argus sprint"
3. Report what shipped
