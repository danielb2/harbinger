# User-requested features and product contracts

This file preserves what Harbinger should do and why. It records product intent and acceptance conditions; it is not an implementation checklist. Use `ROADMAP.md` for Plex API coverage, `CHANGELOG.md` for dated implementation history, and `README.md` for operator instructions.

## Status meanings

- **Verified**: the user-visible behavior was checked in the intended client or workflow.
- **Implemented, unverified**: code and tests exist, but the actual user-facing behavior has not been confirmed.
- **Broken / in progress**: recent evidence shows the feature does not currently meet its contract.

Passing tests alone do not make a feature Verified. Tests must preserve the contracts below; changing an expectation to match a proposed implementation does not authorize changing the feature.

## Web UI administration

**Status: Implemented, unverified in live use.**

Harbinger's own browser UI must require an admin login so an unauthenticated visitor cannot manage or delete media. First run must let the operator create the admin password. The operator must be able to reset it with the SQLite command documented in `README.md`. Plex clients continue to use Plex authentication and must not be redirected through the Harbinger web-admin login.

Acceptance: first-run setup creates a usable admin session; protected UI routes reject unauthenticated requests; destructive UI actions require that session; Plex API routes remain available to Plex clients; the documented SQLite reset permits password recovery.

## Clear web UI failures and working actions

**Status: Implemented, live behavior needs continued verification.**

When Harbinger or a requested service is unavailable, the UI must tell the user that the connection was lost or the operation failed. It must not leave the user with a silent action, an uncaught browser-console error, or a loop of repeated requests that only produce 404s. Actions such as **Fix Match** must complete and show success or a useful failure in the UI.

Acceptance: stop or disconnect the server during an action and verify a clear UI message and bounded retry behavior; exercise Fix Match in the browser and verify both success and failure feedback. Tests should cover missing elements and rejected requests, while browser behavior remains the final check.

## Plex sign-in, server registration, and configured server name

**Status: Broken / in progress.** The latest trace shows Plex returned HTTP 201 for `servers.xml` and accepted the device announcement, but its resource list contained only HTTP connections. The local TLS certificate is available, but the server announcement sent only HTTP URLs and relied on Plex to derive the local HTTPS `plex.direct` URI; Plex did not publish it. The announcement now includes the explicit certificate-backed local HTTPS URI as well as HTTP. Verification still requires Plex to return the local HTTPS URI before Harbinger records registration as complete. Earlier diagnosis found stale PIN rows could also prevent the claim step: each callback is now bound to its own PIN ID, and expired rows are pruned when sign-in starts.

The server name configured by the user is Harbinger's Plex-visible server identity. Changing it must preserve the configured name in the hosted Plex sign-in context, server/device registration, resource publication, Plex descriptors, and LAN discovery. The default name is `harbinger`; a configured name must not silently revert to that default.

Harbinger's stable client/product identifier and its configured server display name are different concepts. A stable `X-Plex-Product` value may be required for the client protocol, but it must not replace the configured server name in fields that Plex displays as the server's name. Keep the client identifier stable across PIN creation and polling. Do not change these mappings based on a test passing or a guessed protocol interpretation; compare the last working commit, official documentation, and current traces first.

Acceptance: complete the hosted sign-in; receive and persist the account token; claim/register the server; verify it appears in Plex's resource list under the configured name with usable local and remote connections, including the local HTTPS `plex.direct` URI; reconnect and disconnect successfully; confirm the displayed name after changing it. A PIN 404 or missing HTTPS resource must not be reported as successful registration.

## JavaScript and UI technology

The user prefers JavaScript and does not want TypeScript added. Keep the current UI implementation unless the user asks for a framework change. A framework proposal does not replace the requirement that every action work and report errors clearly.

## Portable media sidecars

**Status: Implemented / pending fresh-host validation.** With `metadata_storage = "media"`, skip markers are mirrored into media-folder sidecars and imported back into SQLite on startup and scans, so copied media directories no longer lose markers merely because the local database is empty.

When `metadata_storage = "media"`, media-folder sidecars are the portable metadata record for the media beside them, not a disposable cache. Their purpose is to let the operator copy a media directory from one host to another and have Harbinger restore the same match, posters, covers, and per-episode skip markers without downloading artwork again or repeating expensive work such as intro/credit audio analysis.

Skip markers are metadata per episode. Detected and user-edited marker ranges must round-trip through the episode's sidecar in media mode. SQLite may be used as a local index or cache, but losing or replacing the database must not lose marker data when the sidecars are present.

`metadata_storage = "config"` is an explicit opt-out: Harbinger must not write any file into a media folder, including marker sidecars, artwork, or metadata. Sidecar-format files are written under the config directory instead. Existing media-folder sidecars may be read as a fallback, but must never be created or updated in config mode.

Acceptance (media mode): copy a populated media directory to a fresh Harbinger host; verify artwork and match metadata appear without external fetches, intro/credit markers appear on the correct episodes, and no marker detection runs solely because the local database is empty.

Acceptance (config mode): scans, detection, and metadata refresh leave the media directory byte-for-byte unchanged; markers and metadata persist under the config directory.

## Intro and credits skipping

**Status: Broken / in progress.**

Harbinger must detect intro and credits ranges from episode audio per season, using bundled FFmpeg-backed decoding in supported distributions, persist those ranges in the selected sidecar location (media folder for `media` mode, config folder for `config` mode), expose them through Plex-compatible marker APIs and metadata, and honor separate intro/credits settings in Harbinger's web player. Each setting must offer Automatic, Manual, and Off: Automatic seeks to two seconds before the marker ends; Manual shows an accessible Skip Intro or Skip Credits button while playback is inside the marker; Off suppresses both detection-driven playback behavior and skip buttons. Plex clients must receive the same marker ranges through the documented marker/metadata surface; client-visible parity requires checking an official client, not endpoint tests alone. Automatic detection can be disabled in settings to stop background audio analysis after library scans; that opt-out must not remove existing sidecar markers, player skip controls, or the manual detection routes.

Acceptance: with at least two episodes in a season, common intro and credits segments are detected using bundled FFmpeg-backed decoding in supported distributions; markers survive restarts and can be created, edited, and deleted through the documented PMS routes; detected and edited markers are written to the selected sidecar location and, in `media` mode, survive copying the media directory to a fresh host without re-detection; in `config` mode the media directory remains unmodified; Harbinger's player follows all three modes independently for intros and credits; official Plex clients show the skip affordance when given the markers. Detection must fail closed on unsupported audio rather than inventing ranges.

## Working with these contracts

Before changing a covered feature, read its entry here and relevant history in `CHANGELOG.md`. Preserve the stated user goal even when implementation details change. If a protocol constraint appears to conflict with a user-visible contract, keep the user-visible contract and find a compatible implementation; do not redefine the feature to fit the first implementation idea. Update status only with evidence, and record regressions as regressions rather than describing them as new feature work.
