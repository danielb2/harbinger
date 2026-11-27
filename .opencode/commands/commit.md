---
description: Commit the current changes
---
Prepare a commit for the current repository. Staging is your job; the
`committer` subagent only writes the message and commits.

Tracked changes:
!`git status --short -uno`

Untracked files:
!`git ls-files --others --exclude-standard`

1. If any untracked files are listed, use the question tool to ask the user
   whether to include them. Offer: "Include all", "Skip them", and let them
   name specific files in their own answer. Wait for the answer before doing
   anything else. If the list is empty, do not ask.
2. Stage the tracked changes with `git add -u`. Stage any untracked files the
   user approved, by name. Never `git add -A` or `git add .`.
3. If nothing is staged, tell the user and stop.
4. Use the `committer` subagent with the prompt "Commit the staged changes."
   It writes the message and makes the commit. Do not write a commit message
   yourself and do not commit yourself.

$ARGUMENTS
