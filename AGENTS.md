# harbinger

Project guidance for `harbinger`: the target is a 100% drop-in replacement for Plex's official Media Server. This includes documented PMS APIs and matching behavior in official Plex clients, including every client-visible capability and item menu. This is past the MVP stage; treat full compatibility as the goal, not a single-route or single-action milestone.

## Essential rules

- Follow DRY principles across UI components, markup, styles, route behavior, data transformations, validation, media logic, code architecture, and code-reuse. Home and Library presentation should share implementations where sensible. This way we minimize bugs from having a well-tested code path, not multiple.

- progress_checklist: use the `progress-checklist` skill for any multi-step task. Start with a Markdown checklist where every item is unchecked (`[ ]`), keep it visible as work proceeds, tick each item off (`[x]`) in the next progress update after it is completed, and explicitly state what is being worked on now. Do not pre-check items, and do not only show a fully checked list at the end.
  - Verify: progress responses show the checklist evolving from unchecked to checked; each completed item is ticked in the update after completion; the final response has the fully checked list.

- Never expand scope beyond what the user requested. Make only changes needed to fulfill the request; do not add adjacent features, cleanup, or speculative improvements. If broader work seems necessary, explain why and ask before proceeding.

- Read the named rules for a user-facing feature before changing it. Treat their requirements and `Verify:` checks as contracts; tests and implementation details verify those requirements but do not redefine them. If a proposed implementation conflicts with a rule, preserve the rule and find another implementation. Record regressions as regressions.

- Read relevant code and current Plex request traces before changing behavior.
- Match log sources to the client the user is currently using. If a needed trace requires the user to reproduce an action or switch clients, state the exact action and why before asking; never assume a stale client log represents the current behavior.
- When investigating a regression, inspect Git history before changing behavior. Find the commit that introduced the failure and the last known-good behavior, compare their code and tests with current logs/traces, and preserve the previously working contract. Do not change tests merely to make a regression pass.
- Keep the protocol surface Plex-only. Do not add Jellyfin routes or mix adapters.
- Before implementing or changing any Plex API behavior, consult the official PMS API documentation at https://developer.plex.tv/pms and verify the operation, parameters, response formats, and status codes. Do not rely on request traces or assumptions in place of the documentation. If the documentation does not specify a detail or is unavailable, state that clearly and use current request traces and a reference PMS or compatible implementation where available; do not present an inference as documented behavior. Validate client-visible behavior, including the full item menus, against Plex clients; do not infer parity from route tests alone.
- For Plex webhooks, also consult the official support article at https://support.plex.tv/articles/115002267687-webhooks/ and inspect current Plex client traces before implementation. Webhook destination management is account-level; verify the account API behavior against traces and a compatible implementation when the PMS API reference does not document it. Do not replace existing Plex webhook destinations with an unrelated local URL list.
- Playback is a core requirement: support direct play, direct stream/remux, and transcoding when a Plex client requires it.
- Prefer portable in-process Rust libraries for media work where practical. FFmpeg-backed libraries and bundled `ffmpeg`/`ffprobe` binaries are acceptable for now; Docker and other supported distributions must provide the versions Harbinger uses.
- Keep platform support in mind when selecting media libraries and features. Do not make one operating system's hardware decoder mandatory.
- Preserve original-file serving and seeking when a client can play the source directly. Transcoding must be selected from client capabilities and media metadata, not used as a blanket path.
- Keep shared file handling and media logic reusable. Avoid duplicated protocol responses, route behavior, or UI implementations.
- no_request_storms: one page render, API request, or background pass must not fan out into hundreds of blocking operations or follow-up requests. Large libraries are the normal case. Batch database reads, cache or key per-request lookups instead of canonicalizing or reading files per item, return catalog data without waiting for a cold rebuild on hot paths, keep synchronous filesystem, FFmpeg, DNS, and network work off Tokio workers (`tokio::fs`/`tokio::process` or `spawn_blocking`), and bound concurrency for expensive work. Never make an unrelated endpoint (such as the dashboard metrics feed) wait on library-size work.
  - Verify: open a library with hundreds of items and confirm the page, the metrics feed, and unrelated pages stay responsive; confirm a cold restart stays responsive while catalog and artwork work runs in the background; profile a render to show it does not perform O(items) sidecar reads, per-item `canonicalize` calls, whole-catalog clones, or unbounded child requests.
- Add or update tests for behavior changes, including success and failure paths. Keep the Plex endpoint inventory and route tests in sync with the official API documentation.
- Treat `tests/unit/plex_regressions.rs` as an append-only protected suite. Never edit, reformat, rename, or remove an existing test or expectation there unless the user explicitly authorizes it. Add new cases only; ask before changing an existing case.
- Do not commit secrets, generated artifacts, or changes unless explicitly asked.
- When asked to commit, use the commit subject as a concise summary; include a detailed body when useful to explain the change, rationale, and validation. The subject is not a substitute for the body.
- For fixes and regressions, commit messages must include a detailed body that records the observed symptoms/errors, the root cause and relevant history or traces, what changed, regression coverage, and validation performed. Make the body specific enough that a future agent can find and understand the fix from `git log` without relying on chat history.
- Whenever the project version is bumped, create a Git tag for that version after the version bump commit, using the `v` prefix (for example, `v0.1.1`).
- Before work begins on a task refer to https://developer.plex.tv/pms and check offcial API for a solution rather than guessing or checking logs. also consolt implementations from https://github.com/debridmediamanager/rex, https://github.com/braedonsaunders/ether/, and https://github.com/edde746/plezy

## @rules/
Named rules in `rules/*.md` are mandatory requirements, not suggestions.

- Each rule is one bullet formatted `name: requirement`. Rules added or changed from now on must also include an indented `Verify:` line describing the observable evidence that proves them; migrate older rules when touching them. Names are stable `snake_case` identifiers, unique across the whole `rules/` folder.
- Refer to a rule by its bare name inside its own file, and canonically from anywhere else as `@rules.<file>.<name>` using the file stem without `.md`. For example: `@rules.plex.skip_playback_modes`, `@rules.harbinger.spa`, `@rules.audiobookshelf.cover_precedence`.
- Before changing code, read every rule file that applies to the feature you are touching, and all relevant files when a change spans features. Not having read a rule is not an excuse for breaking it.
- Follow every applicable rule. If a rule conflicts with the request or with `AGENTS.md`, stop and ask instead of silently bypassing it.
- Refer to affected rules in plans, progress updates, and handoffs using the canonical form, for example `@rules.harbinger.spa`, `@rules.harbinger.dashboard_has_next`, `@rules.audiobookshelf.directory_structure`, `@rules.audiobookshelf.cover_precedence`. When the user names a rule, treat that name as the requirement and verify it before replying.
- Do not take a shortcut that a rule forbids just because it is simpler, such as forcing a full-page reload when `@rules.harbinger.spa` requires in-page navigation.
- If a rule is unclear, incomplete, or missing, ask before implementing, then add or update it under a `snake_case` name.
- If you find that a rule was violated, say so explicitly, correct the behavior, and add or strengthen a test or rule so it does not regress.
- Add new feature rules to the matching domain file in `rules/` under a `## section` heading, not to `AGENTS.md`. For example, skipping rules belong under `## skipping` in `rules/plex.md`. Do not create a standalone rule file for a single cross-cutting topic. Keep rule wording concrete enough to verify.

## Commands

- Build: `cargo build`
- Test: `cargo test` for the full suite; regular interactive/manual testing uses `just dev`.
- After every code change, run an appropriate build, test, or lint check and inspect its complete output for errors and warnings; fix new diagnostics instead of overlooking them.
- Before every code handoff, run the full `cargo test` suite. If it cannot run or fails, report that clearly and do not claim verification is complete.
- Run: `cargo run`
- Do not send `SIGHUP` or other process signals to the running server.

## Structure

- `src/main.rs`: configuration, media/library state, Plex routes, shared media serving, playback, and the built-in web client.
- `src/plex_tls.rs`: Plex discovery, TLS, and HTTP/WebSocket serving.
- `README.md`: user-facing project overview, setup, connection, playback, and current limitations.
- `ROADMAP.md`: full PMS compatibility checklist and client validation status.
- `CHANGELOG.md`: features and fixes from release to releast. shouldn't mirror a git commit history

## Documentation and UI rules
- Keep `README.md` user-focused. Put detailed implementation notes, technical compatibility status, and technical reflections in `ROADMAP.md`.

- Update `README.md` and `ROADMAP.md` only when preparing a requested commit. In `ROADMAP.md`, record a brief reflection with what changed, why, and the checks run. Do not update `CHANGELOG.md` unless explicitly requested.
- Dialog close controls must stay fixed at the dialog's top-right corner, have an accessible label and tooltip, and close on Escape.
