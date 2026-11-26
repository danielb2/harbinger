# plex rules

- plex_check_references: check references for their implementations to do it right.
  - Verify: cite the official PMS documentation or a reference implementation for every Plex API behavior change, and state when a detail is inferred from traces instead of documented.
  - References:
    - https://developer.plex.tv/pms/
    - https://support.plex.tv/articles/naming-and-organizing-your-tv-show-files/
    - https://github.com/edde746/plezy - a plex client
    - https://github.com/braedonsaunders/ether/ - a plex server for non-stored content
    - https://github.com/debridmediamanager/rex - like ether but in rust
    - https://github.com/Silo-Server
- server_identity: the configured Plex server name is Harbinger's user-visible server identity. Preserve it in the Auth App device context, server registration, resource publication, Plex descriptors, and LAN discovery. The default name is `harbinger`, and a configured name must not silently revert to it. Keep Harbinger's stable client/product identifier separate from the display name; never replace the configured name with the product identifier.
  - Verify: changing the configured name updates every listed surface; traces or tests assert the configured name in each surface; Plex shows the configured name in its resource list after registration.
- plex_registration: complete hosted Plex sign-in, persist the account token, claim/register the server, and report success only when Plex's resource list shows the configured server name with usable local and remote connections, including the local HTTPS `plex.direct` URI. A PIN 404 or a resource list missing the HTTPS connection is not successful registration. Reconnect and disconnect must work.
  - Verify: complete hosted sign-in against Plex, confirm the resource entry under the configured name and its connections, change the name and reconfirm, then disconnect and reconnect successfully.

## skipping

- skip_detection: detect intro and credits ranges from episode audio per season using bundled FFmpeg-backed decoding in supported distributions. Detection must fail closed on unsupported audio rather than inventing ranges.
  - Verify: with at least two episodes in a season, common intro and credits segments are detected; unsupported audio produces no fabricated marker ranges.
- skip_marker_persistence: detected and user-edited intro/credits ranges are written to the selected sidecar location and survive restarts. In `media` mode they survive copying the media directory to a fresh host without re-detection; in `config` mode the media directory remains unmodified. Markers can be created, edited, and deleted through the documented PMS routes.
  - Verify: markers survive a restart and a fresh-host media copy in media mode; config mode leaves the media directory unchanged; the documented create/edit/delete routes persist their changes.
- skip_playback_modes: intro and credits settings are independent and each offers Automatic, Manual, and Off. Automatic seeks to two seconds before the marker ends. Manual shows an accessible Skip Intro or Skip Credits button while playback is inside the marker. Off suppresses both detection-driven playback behavior and skip buttons. Plex clients receive the same marker ranges through the documented marker/metadata surface.
  - Verify: the web player follows all three modes independently for intros and credits; an official Plex client shows the skip affordance and receives the same ranges. Endpoint tests alone do not establish client-visible parity.
- skip_detection_opt_out: automatic detection can be disabled in settings to stop background audio analysis after library scans. Disabling it must not remove existing sidecar markers, player skip controls, or the manual detection routes.
  - Verify: after disabling detection and rescanning, no new analysis runs, while existing markers, skip controls, and manual detection routes continue to work.
