---
name: Progress Checklist
description: Maintain an incremental task checklist during multi-step work, starting unchecked and ticking items off as they complete.
---

# Progress Checklist

Use this skill for any task with more than one step, multiple files, multiple
requirements, or work that will span several tool calls.

## Workflow

1. Before starting work, write a Markdown checklist containing every requested
   item. Every box starts unchecked:

   ```markdown
   - [ ] Item one
   - [ ] Item two
   - [ ] Verify
   ```

2. Add a current-item line immediately after the checklist:

   ```markdown
   Current item: **first item being worked on.**
   ```

3. After an item is completed, send another progress update with that item
   checked off:

   ```markdown
   - [x] Item one
   - [ ] Item two
   - [ ] Verify

   Current item: **second item being worked on.**
   ```

4. Keep the checklist visible in progress responses. Do not replace it with a
   new list, silently reorder it, or pre-check items.

5. Do not claim the task is complete until all requested items are checked and
   verified. The final response must contain the fully checked list and state
   that no work remains.

## Rules

- Never start with checked boxes.
- Tick an item only after its work is actually finished.
- If an item is partially done, leave it unchecked and describe the remaining
  work in the current-item line.
- If scope changes, add new unchecked items instead of hiding them.
- Keep the checklist concise enough to scan, but complete enough to cover every
  user request.
- A simple one-step question does not need a checklist.
