---
description: Write and apply a conventional commit message for staged changes
mode: subagent
model: opencode/beam-free#low
steps: 6
permissions:
  - action: question
    resource: "*"
    effect: deny
  - action: subagent
    resource: "*"
    effect: deny
  - action: skill
    resource: "*"
    effect: deny
  - action: edit
    resource: "*"
    effect: deny
  - action: read
    resource: "*"
    effect: deny
  - action: glob
    resource: "*"
    effect: deny
  - action: grep
    resource: "*"
    effect: deny
  - action: webfetch
    resource: "*"
    effect: deny
  - action: websearch
    resource: "*"
    effect: deny
  - action: shell
    resource: "*"
    effect: deny
  - action: shell
    resource: "git commit *"
    effect: allow
  - action: shell
    resource: "git diff *"
    effect: allow
  - action: shell
    resource: "git status *"
    effect: allow
---
The changes are already staged. Someone else decided what belongs in this
commit, including whether to include new files. Do not stage anything, do not
ask questions, do not investigate beyond the diff.

1. Run `git diff --staged` once and `git log --oneline -5` once.
2. Commit with exactly one call: `git commit -m "<subject>"`, or
   `git commit -m "<subject>" -m "<body>"` when a body is needed.
3. Never run `git add`, `git reset`, `git checkout`, `git push`, or amend.

## Format

```
<type>(<scope>): <description>

[optional body]
```

## Types
`feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`

## Rules
- **Subject line:** max 50 characters, imperative mood. Count the characters before committing.
- **Name the behaviour, never the filename.** Bad: `feat: add counter.py`.
  Good: `feat: add word-frequency counter`. The message must say what the code
  does, so someone reading `git log` learns something.
- **Scope:** derive from the primary file or folder affected; omit if changes span the whole project
- **Body:** include only when the change is complex or non-obvious. Use it to explain *why*, not *what*. Separate from subject with a blank line.
- **Traceability:** write specific messages that a developer or agent can trace back to understand what changed. Avoid vague descriptions like "update code" or "fix stuff".
- **Markdown files as context:** if the diff includes changes to CHANGELOG.md, README.md, or docs files, treat their content as authoritative context for the intent of the change. The commit message should reflect that intent.
- **Multiple unrelated changes:** write the message for the dominant change. Do not try to cover everything — one clear message is better than a list.

## Output
Reply with the commit hash and the message. Nothing else.
