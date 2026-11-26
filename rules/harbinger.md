# harbinger rules

- dashboard_has_next: when I've watched an episode, the dashboard in plex etc must show the next episode to watch.
  - Verify: finish an episode and confirm the dashboard and Plex next-up surface show the following episode, not the finished one.

## harbinger web
- web_admin_auth: Harbinger's browser UI requires an admin login so an unauthenticated visitor cannot manage or delete media. First run creates the admin password, and the operator can reset it with the SQLite command documented in `README.md`. Plex clients keep Plex authentication and must not be redirected through the web-admin login.
  - Verify: first-run setup creates a usable admin session; protected UI routes reject unauthenticated requests; destructive UI actions require the session; Plex API routes stay available to Plex clients; the documented reset restores access.
- ui_failure_feedback: when Harbinger or a requested service is unavailable, the UI must clearly report the lost connection or failed operation. Do not leave silent actions, uncaught browser-console errors, or repeated requests that only produce 404s. Actions such as Fix Match must complete and show success or a useful failure.
  - Verify: disconnect the server during an action and confirm a clear message and bounded retries; exercise Fix Match success and failure in a browser; tests cover missing elements and rejected requests.
- no_the: don't sort using "the" when showing a library.
  - Verify: a title such as "The Matrix" sorts under M, not T, in every library listing.
- search_media: the navbar search is omnipresent and searches all media. Search results must display as the user types.
  - Verify: the search control is present on every web page, typing updates results without submitting, and results include every library.
- no_doubles: don't show double episodes.
  - Verify: an episode represented by multiple files or metadata records appears only once in cards and episode lists.
- spa: the webpage must be a single-page app driven by JavaScript to prevent load time. In-page actions must not trigger full-page reloads or full document navigation.
  - Verify: exercising library navigation, playback actions, cover upload, watch-state changes, and dialogs does not call `window.location.reload()` or navigate the full document; history/back-forward behavior still works.
- unwatched_badge: tv-series covers show the number of unwatched episodes in the top-right corner in white font on an 80% opaque background, layered on the cover; when there are no unwatched episodes show a checkmark instead of a 0.
  - Verify: a series with unwatched episodes renders the count badge, and a fully watched series renders `✓`; both are positioned over the cover with the specified background.

## storage

- media_sidecars_portable: with `metadata_storage = "media"`, media-folder sidecars are the portable metadata record for the media beside them, not a disposable cache. Copying a media directory to a fresh Harbinger host must restore the same match, posters, covers, and per-episode skip markers without refetching artwork or repeating expensive work such as intro/credit detection. Detected and user-edited skip markers are metadata per episode and must round-trip through the sidecar. SQLite may be a local index or cache, but it must not be the only durable home for sidecar-owned metadata.
  - Verify: copy a populated media directory to a fresh host and confirm artwork, match metadata, and per-episode markers appear without external fetches or re-running detection solely because the local database is empty.
- config_storage_read_only: with `metadata_storage = "config"`, Harbinger must not create, modify, or delete any file in a media folder, including `.hbgr` sidecars, skip markers, artwork, and metadata. Sidecar-format files belong under the config directory only. Existing media-folder sidecars may be read as a fallback, but must never be written in config mode. Do not describe marker persistence as complete when it survives only in a host-local database unless config mode was deliberately chosen.
  - Verify: scans, detection, metadata refresh, and cover uploads leave the media directory byte-for-byte unchanged; markers and metadata persist under the config directory.
