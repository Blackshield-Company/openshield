---
name: git
description: Git workflow and best practices
triggers:
  - git
  - commit
  - branch
  - merge
  - rebase
  - stash
  - pull request
  - pr
  - github
tags:
  - version-control
  - collaboration
---

# Git Best Practices

## Commits
- Write commit messages in imperative mood: "Add feature" not "Added feature"
- First line: 50 chars max, summary
- Blank line, then detailed description if needed
- Reference issues: "Fixes #123" or "Closes #456"

## Branching
- `main` is always deployable
- Feature branches: `feature/description` or `feat/description`
- Bugfix branches: `fix/description`
- Hotfix branches: `hotfix/description`
- Delete merged branches immediately

## Workflow
- Pull before push: `git pull --rebase` or `git pull --ff-only`
- Squash messy WIP commits before merging
- Use `git rebase -i` to clean up history
- Never force-push to shared branches
- `git stash` is for temporary saves, not long-term storage

## Common Commands
- `git log --oneline --graph --all` — visual history
- `git diff --cached` — see staged changes
- `git restore --staged <file>` — unstage
- `git restore <file>` — discard changes
- `git switch -c <branch>` — create and switch (modern, replaces `checkout -b`)
