subtitles:

1. "off" is the empty subtitle entry in the normal selection list; a stored "off" is displayed and persisted like any other stored selection.
2. when a file opens, display the stored per-item selection if it is valid.
3. if there is no valid stored selection, use a bilingual subtitle matching the primary and secondary languages, then the configured primary subtitle language.
4. if the configured primary subtitle language has no track, open with "off".
5. a stale, invalid, or deleted stored selection falls back to the configured primary subtitle language, or "off" if that language has no track.
6. deleting a generated subtitle uses the same fallback.
7. selecting generate shows a temporary "(generating…)" state, keeps the pending option selected, plays the source subtitle, and persists that source selection.
8. if a partial bilingual VTT checkpoint exists, resume translation during playback when no translation thread is already running.
9. if generation fails, change nothing about the selection or partial VTT; on 429, set the wait-for-subtitles message to "encountered 429, try again later" in both the UI and the VTT.
10. the incomplete-generation notice is an SRT/VTT cue starting at the existing last untranslated cue timestamp, ending at 09:00:00.000, and remaining the final line until the SRT is finished; its text matches the generation-failure message.
11. all explicit settings are persisted in the database and mirrored to Plex clients.
12. the primary subtitle setting allows "off"; when set to "off", bilingual is disabled.
13. bilingual: if an immediate 429 is encountered still generate vtt, and switch to bilingual (user can switch back manually)
