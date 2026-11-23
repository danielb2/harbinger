# subtitle rules
Rules are flat list with snake_case names. Rules refer to each other by name, so nothing needs numbering.

* off: "off" is a valid selection meaning no subtitle is displayed during playback. It is stored and restored like any other selection.
* default_fallback: the bilingual subtitle matching the primary and secondary languages if one exists, otherwise the configured primary subtitle language, otherwise off (when that language has no track or the primary setting is off).
* open_episode: show the stored per-episode selection if valid, including a stored off. If it is missing, stale, invalid, or deleted, use default_fallback.
* no_save_on_open: opening an episode never saves subtitle options. Save only when the selection differs from the default.
* delete_generated: deleting a generated subtitle uses default_fallback.
* settings_persistence: all explicit settings are stored in the database and mirrored to Plex clients.
* primary_off: the primary subtitle setting allows off. When it is off, bilingual is disabled.
- generate_option: when the bilingual subtitle file for an episode does not exist, the subtitle selection list must include an entry labeled “<primary> + <secondary> (generate)”, in both the Plex client and the web UI. For English and Vietnamese the label is exactly “English + Vietnamese (generate)”. Selecting it triggers start_generation.
* start_generation: selecting generate creates the `.vtt.part` checkpoint immediately, before the first translation request. The pending option is shown as "(generating…)" and stays selected, and the source selection is persisted as a fallback.
* serve_during_generation: while generation is in progress, the bilingual subtitle stream serves the `.vtt.part` file. The "(generating…)" state is shown by serving this file. The English source file is never substituted for it.
* resume_generation: if a partial bilingual VTT checkpoint exists and no translation thread is running, resume translation during playback.
* generation_failure: if generation fails, the selection and partial VTT stay unchanged except for incomplete_notice.
* rate_limit: a 429 is Google's Translate API reporting that the translation rate limit was reached. The UI and the VTT both show "Google Translate rate limit reached. Wait a while and try again."
* immediate_rate_limit: a 429 on the first request behaves exactly like a mid-stream 429. The checkpoint already exists from start_generation and carries incomplete_notice. No special response, no second VTT body, no extra UI state.
* incomplete_notice: a cue starting at the timestamp of the last untranslated cue and ending at 09:00:00.000. It remains the final line of the file until the SRT is finished, and its text matches the failure message in use (for a 429, the rate_limit message).