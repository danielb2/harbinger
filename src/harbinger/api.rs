use super::*;
use axum::routing::{any, get, post};

const FAVICON: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/favicon.ico"));
const SIDEBAR_MOBILE_CSS: &str = r#"
.sidebar-mobile-toggle { display: none; }
.sidebar-panel { min-width: 0; }
.sidebar-chevron { margin-left: auto; color: #7f8ea3; transform: rotate(-90deg); transition: transform .15s ease; }
.sidebar-settings-group[open] > summary .sidebar-chevron { transform: rotate(0deg); }
.sidebar-subnav { display: grid; gap: .15rem; margin: .2rem 0 .35rem 1.15rem; padding-left: .65rem; border-left: 1px solid #263244; }
.sidebar-sublink { display: flex; align-items: center; min-height: 2rem; padding: .4rem .55rem; border-radius: .4rem; color: #94a3b8; text-decoration: none; font-size: .86rem; }
.sidebar-sublink:hover, .sidebar-sublink.active { color: #fff; background: #17243a; }
@media (max-width: 700px) {
  .sidebar-subnav { margin-left: 0; padding-left: 0; border-left: 0; }
  html.nav-js .sidebar { position: sticky; top: 0; z-index: 60; }
  html.nav-js .sidebar-mobile-toggle { display: grid; place-items: center; width: 2.75rem; height: 2.75rem; margin-left: auto; border: 1px solid #263244; border-radius: .6rem; color: #e5e7eb; background: #111c2e; font-size: 0; cursor: pointer; }
  html.nav-js .sidebar-mobile-toggle::before { content: "☰"; font-size: 1.25rem; }
  html.nav-js.nav-open .sidebar-mobile-toggle::before { content: "×"; }
  html.nav-js .sidebar-panel { position: fixed; top: 4.5rem; right: 0; bottom: 0; z-index: 50; width: min(20rem, 85vw); max-height: calc(100vh - 4.5rem); overflow-y: auto; box-sizing: border-box; padding: .85rem; border-top: 1px solid #263244; border-left: 1px solid #263244; background: #0d1421; box-shadow: -18px 0 40px rgba(0,0,0,.45); transform: translateX(102%); transition: transform .2s ease; overscroll-behavior: contain; -webkit-overflow-scrolling: touch; }
  html.nav-js.nav-open .sidebar-panel { transform: translateX(0); }
  html.nav-js.nav-open body { overflow: hidden; }
  html.nav-js.nav-open .app-shell::before { content: ""; position: fixed; inset: 4.5rem 0 0; z-index: 45; background: rgba(2,6,23,.62); }
  html.nav-js .sidebar-label { display: block; }
  html.nav-js .sidebar-nav { display: grid; gap: .25rem; padding-bottom: 0; }
  html.nav-js .sidebar-link { white-space: normal; }
}
"#;
const SIDEBAR_MOBILE_SCRIPT: &str = r#"<script>
(() => {
  const root = document.documentElement;
  const toggle = document.querySelector(".sidebar-mobile-toggle");
  const panel = document.querySelector(".sidebar-panel");
  if (!toggle || !panel) return;
  root.classList.add("nav-js");
  const setOpen = open => {
    root.classList.toggle("nav-open", open);
    toggle.setAttribute("aria-expanded", String(open));
    toggle.setAttribute("aria-label", open ? "Close navigation" : "Open navigation");
    toggle.title = open ? "Close navigation" : "Open navigation";
  };
  toggle.addEventListener("click", event => {
    event.stopPropagation();
    setOpen(!root.classList.contains("nav-open"));
  });
  panel.querySelectorAll("a").forEach(link => link.addEventListener("click", () => setOpen(false)));
  document.addEventListener("click", event => {
    if (!root.classList.contains("nav-open")) return;
    if (panel.contains(event.target) || toggle.contains(event.target)) return;
    setOpen(false);
  });
  document.addEventListener("keydown", event => {
    if (event.key === "Escape") setOpen(false);
  });
  window.addEventListener("resize", () => {
    if (window.matchMedia("(min-width: 701px)").matches) setOpen(false);
  });
})();
</script>"#;

/// Routes owned by the Harbinger web UI and its 4826 listener surface.
pub(crate) fn web_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(root_handler))
        .route("/favicon.ico", get(favicon_handler))
        .route("/auth", get(auth_page))
        .route("/auth/setup", post(auth_setup))
        .route("/auth/login", post(auth_login))
        .route("/auth/logout", post(auth_logout))
        .route("/web/dashboard/stats", get(dashboard_stats))
        .route("/web/ping", get(web_ping))
        .route("/web/search", get(web_search))
        .route("/web/stream", any(web_stream))
        .route("/web/index.html", get(web_index_redirect))
        .route("/web/", get(web_index_redirect))
        .route("/web/progress", get(web_progress).post(update_web_progress))
        .route("/web/translation-stop", post(stop_web_translation))
        .route("/web/watch-action", post(web_watch_action))
        .route(
            "/web/audiobook-cover",
            post(set_audiobook_cover).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route("/web/audiobook-cover/options", get(audiobook_cover_options))
        .route("/web/audiobook-cover/image", get(audiobook_cover_image))
        .route("/web/audiobook-cover/select", post(select_audiobook_cover))
        .route("/web/audiobook-cover/url", post(set_audiobook_cover_url))
        .route("/web/audio-selection", post(update_web_audio_selection))
        .route(
            "/web/subtitle-selection",
            get(web_subtitle_selection).post(update_web_subtitle_selection),
        )
        .route(
            "/web/subtitle-selection/socket",
            get(web_subtitle_selection_socket),
        )
        .route("/web/subtitle", get(web_subtitle))
        .route("/web/translated-subtitle", post(web_translated_subtitle))
        .route("/web/translation-progress", post(web_translation_progress))
        .route("/web/artwork", get(web_artwork))
        .route("/web/artwork/media/:id/:kind", get(web_media_artwork))
        .route("/web/episode-artwork/:id", get(web_episode_artwork))
        .route("/web/info", get(web_info))
        .route("/web/tv-match/search", get(tv_match_search))
        .route("/web/tv-match/select", post(tv_match_select))
        .route("/web/book-match/search", get(book_match_search))
        .route("/web/book-match/select", post(book_match_select))
        .route(
            "/settings",
            get(settings_page).post(save_translation_settings),
        )
        .route(
            "/settings/translation",
            get(translation_settings_page).post(save_translation_settings),
        )
        .route("/settings/skipping", post(save_skip_settings))
        .route("/settings/logs/socket", get(settings_logs_socket))
        .route("/settings/logs/download", get(download_logs))
        .route("/settings/logs/level", post(update_log_level))
        .route("/settings/logs/clear", post(clear_logs))
        .route("/settings/libraries", post(create_library))
        .route("/settings/libraries/:id/rename", post(rename_library))
        .route("/settings/libraries/:id/save", post(save_library_settings))
        .route("/settings/libraries/:id/delete", post(delete_library))
        .route("/settings/folders", post(add_library_folder))
        .route("/settings/folders/remove", post(remove_library_folder))
        .route("/settings/libraries/display", post(update_library_display))
        .route(
            "/settings/libraries/scan-settings",
            post(update_library_scan_settings),
        )
        .route("/settings/scan", post(scan_all_libraries))
        .route("/settings/preferences", post(update_preferences))
        .route("/settings/server-name", post(update_server_name))
        .route(
            "/settings/users",
            get(users_settings_page).post(create_user),
        )
        .route("/settings/users/:id/update", post(update_user))
        .route("/settings/users/:id/password", post(update_user_password))
        .route("/settings/plex/start", get(start_plex_auth))
        .route("/settings/plex/status", get(plex_auth_status))
        .route("/settings/plex/callback", get(plex_auth_callback))
        .route("/settings/plex/disconnect", post(disconnect_plex))
        .route("/settings/libraries/:id/scan", post(scan_library_metadata))
        .route(
            "/settings/libraries/:id/scan-files",
            post(scan_library_inventory_handler),
        )
        .route("/settings/media/scan", post(scan_media_metadata))
        .route("/settings/browse", get(settings_browse))
}

pub(crate) async fn users_settings_page() -> Response {
    Redirect::to("/settings?tab=users").into_response()
}

fn users_settings_markup(state: &AppState, query: &SettingsQuery) -> String {
    let rows = state
        .db
        .lock()
        .ok()
        .map(|db| {
            db.prepare(
                "SELECT id, username, user_type, is_active FROM users
                 ORDER BY user_type = 'admin' DESC, username COLLATE NOCASE",
            )
            .ok()
            .map(|mut stmt| {
                stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .ok()
                .map(|rows| rows.flatten().collect::<Vec<_>>())
                .unwrap_or_default()
            })
            .unwrap_or_default()
        })
        .unwrap_or_default();
    let user_count = rows.len();
    let count_label = if user_count == 1 { "user" } else { "users" };
    let mut body = String::from(
        r#"<style>
.users-panel .user-count { display:inline-flex; align-items:center; justify-content:center; min-width:1.8rem; padding:.2rem .55rem; border:1px solid #354a68; border-radius:999px; color:#c6d8ef; background:#152238; font-size:.75rem; font-weight:700; }
.user-list { display:grid; gap:.8rem; margin-top:1.15rem; }
.user-card { overflow:hidden; border:1px solid #2c3a50; border-radius:.85rem; background:rgba(10,17,30,.46); transition:border-color .15s, background .15s; }
.user-card:hover { border-color:#3a4d69; }
.user-card[open] { border-color:#3d5577; background:rgba(13,22,38,.72); }
.user-summary { display:grid; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:.9rem; width:100%; padding:.95rem 1rem; border:0; border-radius:0; color:inherit; background:transparent; cursor:pointer; list-style:none; }
.user-summary::before { content:none !important; }
.user-summary::-webkit-details-marker { display:none; }
.user-avatar { display:grid; place-items:center; width:2.7rem; height:2.7rem; border:1px solid #3d5577; border-radius:.8rem; color:#dbeafe; background:linear-gradient(145deg, #244a80, #16243a); font-size:1.05rem; font-weight:800; text-transform:uppercase; }
.user-identity { display:grid; gap:.28rem; min-width:0; }
.user-name-line { display:flex; flex-wrap:wrap; align-items:center; gap:.5rem; }
.user-name-line strong { color:#f1f5fb; overflow-wrap:anywhere; }
.user-meta { color:#91a0b6; font-size:.8rem; }
.user-summary-action { display:flex; align-items:center; gap:.55rem; color:#9db7d8; font-size:.82rem; font-weight:650; white-space:nowrap; }
.user-summary-action::after { content:"⌄"; color:#8fa9c9; font-size:1rem; transition:transform .15s; }
.user-card[open] .user-summary-action::after { transform:rotate(180deg); }
.user-editor { display:grid; gap:1.15rem; padding:0 1rem 1rem; }
.user-editor .settings-form { padding-top:1rem; border-top:1px solid #26354a; }
.user-editor h3 { margin:0; color:#e8eef8; font-size:.95rem; }
.user-editor .action-row { margin-top:0; }
.user-create { margin-top:1rem; }
.type-pill.user-role-admin { border-color:#4f6f9d; color:#d5e6ff; background:#1a3152; }
.type-pill.user-role-guest { border-color:#3f536e; color:#c3d2e6; background:#17243a; }
.type-pill.user-state-disabled { border-color:#6b4a52; color:#f3c7ce; background:#35202a; }
.users-panel .empty-note { margin:1.15rem 0 0; padding:1rem; border:1px dashed #33445c; border-radius:.75rem; color:#98a6bb; text-align:center; }
@media (max-width: 560px) { .user-summary { grid-template-columns:auto minmax(0,1fr); } .user-summary-action { grid-column:2; justify-self:start; } }
</style>"#,
    );
    body.push_str(&format!(
        r#"<section class="settings-card users-panel" aria-labelledby="users-heading"><div class="section-heading"><div><h2 id="users-heading">Users</h2><p class="section-description">People who can sign in to Harbinger and compatible apps. Change names, roles, access, and passwords below.</p></div><span class="user-count" aria-label="{} {}">{}</span></div>"#,
        user_count, count_label, user_count,
    ));
    if let Some(error) = query.user_error.as_deref() {
        body.push_str(&format!(
            "<div class=\"flash-error\" role=\"alert\">{}</div>",
            html(error)
        ));
    }
    if let Some(status) = query.user_status.as_deref() {
        body.push_str(&format!(
            "<div class=\"flash-notice\" role=\"status\">{}</div>",
            html(status)
        ));
    }
    if rows.is_empty() {
        body.push_str(r#"<p class="empty-note">No users yet. Add the first account below.</p>"#);
    } else {
        body.push_str("<div class=\"user-list\">");
        for (id, username, user_type, is_active) in rows {
            let display_name = username.unwrap_or_else(|| id.clone());
            let initial = display_name
                .chars()
                .next()
                .map(|character| character.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_owned());
            let role_label = if user_type == "admin" {
                "Administrator"
            } else {
                "Guest"
            };
            let state_label = if is_active != 0 { "Active" } else { "Disabled" };
            let state_pill = if is_active != 0 {
                ""
            } else {
                "<span class=\"type-pill user-state-disabled\">Disabled</span>"
            };
            let admin_selected = if user_type == "admin" {
                " selected"
            } else {
                ""
            };
            let guest_selected = if user_type == "admin" {
                ""
            } else {
                " selected"
            };
            let active_checked = if is_active != 0 { " checked" } else { "" };
            let id_path = url_encode(&id);
            let name = html(&display_name);
            body.push_str(&format!(
                r#"<details class="user-card" data-user-id="{}"><summary class="user-summary"><span class="user-avatar" aria-hidden="true">{}</span><span class="user-identity"><span class="user-name-line"><strong>{}</strong><span class="type-pill user-role-{}">{}</span>{}</span><span class="user-meta">{} user</span></span><span class="user-summary-action">Manage</span></summary><div class="user-editor"><form method="post" action="/settings/users/{}/update" class="settings-form"><h3>Profile and access</h3><div class="form-grid"><label class="field"><span>Username</span><input name="username" type="text" value="{}" maxlength="64" required autocomplete="username"></label><label class="field"><span>Role</span><select name="user_type"><option value="admin"{}>Administrator</option><option value="guest"{}>Guest</option></select></label></div><label class="settings-toggle"><input type="checkbox" name="is_active" value="true"{}> Account is active</label><div class="action-row"><button type="submit">Save changes</button></div></form><form method="post" action="/settings/users/{}/password" class="settings-form"><h3>Change password</h3><div class="form-grid"><label class="field"><span>New password</span><input name="password" type="password" minlength="8" required autocomplete="new-password"></label><label class="field"><span>Confirm new password</span><input name="confirm_password" type="password" minlength="8" required autocomplete="new-password"></label></div><small class="note">Use at least 8 characters. Changing the administrator password also updates the web sign-in password.</small><div class="action-row"><button type="submit" class="button-secondary">Update password</button></div></form></div></details>"#,
                html(&id),
                html(&initial),
                name,
                if user_type == "admin" { "admin" } else { "guest" },
                role_label,
                state_pill,
                state_label.to_lowercase(),
                id_path,
                name,
                admin_selected,
                guest_selected,
                active_checked,
                id_path,
            ));
        }
        body.push_str("</div>");
    }
    body.push_str(
        r#"<details class="add-library user-create"><summary>Add user</summary><form method="post" action="/settings/users" class="settings-form"><div class="form-grid"><label class="field"><span>Username</span><input name="username" type="text" maxlength="64" required autocomplete="off"></label><label class="field"><span>Password</span><input name="password" type="password" minlength="8" required autocomplete="new-password"></label></div><label class="field"><span>Role</span><select name="user_type"><option value="guest">Guest</option><option value="admin">Administrator</option></select></label><small class="note">Guests can use the account in compatible apps. Administrators can also manage Harbinger's settings.</small><div class="action-row"><button type="submit">Create user</button></div></form></details></section>"#,
    );
    body
}

fn users_redirect(status: Option<&str>, error: Option<&str>) -> Response {
    let mut url = String::from("/settings?tab=users");
    if let Some(status) = status {
        url.push_str("&user_status=");
        url.push_str(&url_encode(status));
    }
    if let Some(error) = error {
        url.push_str("&user_error=");
        url.push_str(&url_encode(error));
    }
    Redirect::to(&url).into_response()
}

fn normalize_user_type(value: &str) -> Option<&'static str> {
    match value {
        "admin" => Some("admin"),
        "guest" => Some("guest"),
        _ => None,
    }
}

pub(crate) async fn create_user(
    State(state): State<AppState>,
    Form(form): Form<UserCreateForm>,
) -> Response {
    let username = form.username.trim();
    if username.is_empty() || username.chars().count() > 64 {
        return users_redirect(None, Some("Enter a username between 1 and 64 characters."));
    }
    if form.password.len() < 8 {
        return users_redirect(None, Some("Password must be at least 8 characters."));
    }
    let Some(user_type) = normalize_user_type(&form.user_type) else {
        return users_redirect(None, Some("Choose a valid role."));
    };
    let salt = SaltString::generate(&mut OsRng);
    let Ok(hash) = Argon2::default().hash_password(form.password.as_bytes(), &salt) else {
        return users_redirect(None, Some("Could not create the user. Try again."));
    };
    let Ok(db) = state.db.lock() else {
        return users_redirect(None, Some("Could not access the user database. Try again."));
    };
    let exists = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 OR username = ?2)",
            params![username, username],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if exists {
        return users_redirect(None, Some("That username is already taken."));
    }
    if db
        .execute(
            "INSERT INTO users (id, name, username, password_hash, user_type) VALUES (?1, ?2, ?2, ?3, ?4)",
            params![username, username, hash.to_string(), user_type],
        )
        .is_err()
    {
        return users_redirect(None, Some("Could not create the user. Try again."));
    }
    users_redirect(Some("User created."), None)
}

pub(crate) async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<UserUpdateForm>,
) -> Response {
    let username = form.username.trim();
    if username.is_empty() || username.chars().count() > 64 {
        return users_redirect(None, Some("Enter a username between 1 and 64 characters."));
    }
    let Some(user_type) = normalize_user_type(&form.user_type) else {
        return users_redirect(None, Some("Choose a valid role."));
    };
    let Ok(db) = state.db.lock() else {
        return users_redirect(None, Some("Could not access the user database. Try again."));
    };
    let current = db.query_row(
        "SELECT user_type, is_active FROM users WHERE id = ?1",
        [&id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    );
    let Ok((current_type, current_active)) = current else {
        return users_redirect(None, Some("That user no longer exists."));
    };
    let taken = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1 AND id <> ?2)",
            params![username, id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if taken {
        return users_redirect(None, Some("That username is already taken."));
    }
    let remains_admin = current_type == "admin" && current_active != 0;
    if remains_admin && (user_type != "admin" || !form.is_active) {
        let other_admins = db
            .query_row(
                "SELECT COUNT(*) FROM users WHERE user_type = 'admin' AND is_active = 1 AND id <> ?1",
                [&id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        if other_admins == 0 {
            return users_redirect(
                None,
                Some("Keep at least one active administrator before changing this account."),
            );
        }
    }
    if db
        .execute(
            "UPDATE users SET name = ?1, username = ?1, user_type = ?2, is_active = ?3 WHERE id = ?4",
            params![username, user_type, i64::from(form.is_active), id],
        )
        .is_err()
    {
        return users_redirect(None, Some("Could not save the user. Try again."));
    }
    users_redirect(Some("User updated."), None)
}

pub(crate) async fn update_user_password(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<UserPasswordForm>,
) -> Response {
    if form.password.len() < 8 {
        return users_redirect(None, Some("Password must be at least 8 characters."));
    }
    if form.password != form.confirm_password {
        return users_redirect(None, Some("The new passwords do not match."));
    }
    let salt = SaltString::generate(&mut OsRng);
    let Ok(hash) = Argon2::default().hash_password(form.password.as_bytes(), &salt) else {
        return users_redirect(None, Some("Could not update the password. Try again."));
    };
    let Ok(db) = state.db.lock() else {
        return users_redirect(None, Some("Could not access the user database. Try again."));
    };
    match db.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
        params![hash.to_string(), id],
    ) {
        Ok(0) => return users_redirect(None, Some("That user no longer exists.")),
        Ok(_) => {}
        Err(_) => return users_redirect(None, Some("Could not update the password. Try again.")),
    }
    if id == "admin"
        && db
            .execute(
                "INSERT INTO web_admin (id, password_hash) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET password_hash = excluded.password_hash",
                [hash.to_string()],
            )
            .is_err()
    {
        return users_redirect(
            Some("User password updated, but the web sign-in password could not be synchronized."),
            None,
        );
    }
    users_redirect(Some("Password updated."), None)
}

#[derive(serde::Deserialize)]
pub(crate) struct UserCreateForm {
    username: String,
    password: String,
    #[serde(default = "default_guest")]
    user_type: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct UserUpdateForm {
    username: String,
    #[serde(default = "default_guest")]
    user_type: String,
    #[serde(default)]
    is_active: bool,
}

#[derive(serde::Deserialize)]
pub(crate) struct UserPasswordForm {
    password: String,
    #[serde(default)]
    confirm_password: String,
}

fn default_guest() -> String {
    "guest".to_owned()
}

pub(crate) async fn favicon_handler() -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/x-icon"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        FAVICON,
    )
        .into_response()
}

pub(crate) async fn download_logs() -> Response {
    let mut body = recent_log_history().join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let filename = format!(
        "harbinger-logs-{}.log",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"));
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

#[derive(Deserialize)]
pub(crate) struct LogLevelForm {
    level: String,
}

pub(crate) async fn update_log_level(
    State(state): State<AppState>,
    Form(form): Form<LogLevelForm>,
) -> Response {
    let level = match form.level.trim() {
        "debug" => "debug",
        "info" => "info",
        _ => return settings_error("Unknown log level"),
    };
    if let Err(error) = set_log_level(level) {
        return settings_error(&error);
    }
    store_explicit_setting(&state, "log_level", level);
    let mut response = Redirect::to("/settings?tab=logs").into_response();
    response.extensions_mut().insert(SuppressHttpResponseLog);
    response
}

pub(crate) async fn clear_logs() -> Response {
    clear_log_lines();
    let mut response = Redirect::to("/settings?tab=logs").into_response();
    response.extensions_mut().insert(SuppressHttpResponseLog);
    response
}

pub(crate) async fn settings_logs_socket(upgrade: WebSocketUpgrade) -> Response {
    let mut response = upgrade
        .on_upgrade(|mut socket| async move {
            let mut updates = log_broadcast().subscribe();
            let (mut last_sequence, lines) = recent_log_snapshot();
            let snapshot = serde_json::json!({
                "type": "snapshot",
                "sequence": last_sequence,
                "lines": lines,
            });
            if socket.send(Message::Text(snapshot.to_string().into())).await.is_err() {
                return;
            }
            loop {
                tokio::select! {
                    message = socket.recv() => match message {
                        Some(Ok(Message::Ping(payload))) => {
                            if socket.send(Message::Pong(payload)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(Message::Text(_) | Message::Binary(_) | Message::Pong(_))) => {}
                    },
                    update = updates.recv() => match update {
                        Ok(LogEvent::Entry(sequence, line)) if sequence > last_sequence => {
                            let event = serde_json::json!({
                                "type": "entry",
                                "sequence": sequence,
                                "line": line,
                            });
                            if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                                break;
                            }
                            last_sequence = sequence;
                        }
                        Ok(LogEvent::Cleared(sequence)) => {
                            let event = serde_json::json!({
                                "type": "cleared",
                                "sequence": sequence,
                            });
                            if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                                break;
                            }
                            last_sequence = sequence;
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let (sequence, lines) = recent_log_snapshot();
                            let snapshot = serde_json::json!({
                                "type": "snapshot",
                                "sequence": sequence,
                                "lines": lines,
                            });
                            if socket.send(Message::Text(snapshot.to_string().into())).await.is_err() {
                                break;
                            }
                            last_sequence = sequence;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        })
        .into_response();
    response.extensions_mut().insert(SuppressHttpResponseLog);
    response
}

pub(crate) async fn web_translation_progress(
    State(state): State<AppState>,
    Json(request): Json<WebTranslationProgressRequest>,
) -> Response {
    if !state
        .config
        .lock()
        .is_ok_and(|config| config.bilingual_subtitles_enabled)
    {
        return (
            StatusCode::FORBIDDEN,
            "Bilingual subtitles are disabled in Settings",
        )
            .into_response();
    }
    if request.first_language == request.second_language
        || isolang::Language::from_639_1(&request.first_language).is_none()
        || isolang::Language::from_639_1(&request.second_language).is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            "Choose two distinct supported languages",
        )
            .into_response();
    }
    let Some(relative) = clean_relative_path(&request.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, request.library, request.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let total = match request.total_cues {
        Some(total) if total <= 20_000 => total,
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Subtitle track contains too many cues",
            )
                .into_response()
        }
        None => {
            let (contents, _, format) =
                match read_subtitle_source(&state, &media, request.stream_id).await {
                    Ok(source) => source,
                    Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
                };
            match subtitles::parse_subtitles(&subtitle_to_vtt(&contents, &format)) {
                Ok(cues) => cues.len(),
                Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
            }
        }
    };
    let final_path = configured_bilingual_subtitle_path(
        &state,
        &media,
        &request.first_language,
        &request.second_language,
        "srt",
    );
    let checkpoint_path = configured_bilingual_subtitle_path(
        &state,
        &media,
        &request.first_language,
        &request.second_language,
        "vtt",
    );
    let checkpoint_path = PathBuf::from(format!("{}.part", checkpoint_path.to_string_lossy()));
    let finished =
        if subtitle_marker_source_matches(&final_path, request.source_key.as_deref()).await {
            tokio::fs::read_to_string(&final_path).await.ok()
        } else {
            None
        };
    let staged = if let Some(contents) = finished.as_ref() {
        contents.clone()
    } else if subtitle_marker_source_matches(&checkpoint_path, request.source_key.as_deref()).await
    {
        tokio::fs::read_to_string(&checkpoint_path)
            .await
            .unwrap_or_default()
    } else {
        String::new()
    };
    let completed = subtitles::parse_subtitles(&staged)
        .map(|cues| cues.len())
        .unwrap_or(0)
        .min(total);
    let percent = if total == 0 {
        0
    } else {
        (completed * 100 / total) as u8
    };
    Json(serde_json::json!({
        "completed": completed,
        "total": total,
        "percent": if finished.is_some() { 100 } else { percent },
        "complete": finished.is_some(),
    }))
    .into_response()
}

pub(crate) async fn web_translated_subtitle(
    State(state): State<AppState>,
    Json(request): Json<WebTranslatedSubtitleRequest>,
) -> Response {
    info!(
        path = %request.path,
        prepared = request.prepared,
        source_stream_id = request.stream_id,
        first_language = %request.first_language,
        second_language = %request.second_language,
        "bilingual subtitle translation request received"
    );

    if !state
        .config
        .lock()
        .is_ok_and(|config| config.bilingual_subtitles_enabled)
    {
        return (
            StatusCode::FORBIDDEN,
            "Bilingual subtitles are disabled in Settings",
        )
            .into_response();
    }
    if !request.prepared {
        return (
            StatusCode::FORBIDDEN,
            "Use the Generate bilingual subtitle button to start generation",
        )
            .into_response();
    }
    if request.first_language == request.second_language
        || isolang::Language::from_639_1(&request.first_language).is_none()
        || isolang::Language::from_639_1(&request.second_language).is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            "Choose two distinct supported languages",
        )
            .into_response();
    }
    let Some(relative) = clean_relative_path(&request.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, request.library, request.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (contents, source_language, format) = if request.source_cues.is_empty() {
        match read_subtitle_source(&state, &media, request.stream_id).await {
            Ok(source) => source,
            Err(error) => {
                warn!(media = %media.title, source_stream_id = request.stream_id, error = %error, "bilingual subtitle generation could not start");
                return (StatusCode::BAD_REQUEST, error).into_response();
            }
        }
    } else {
        match browser_cues_to_srt(&request.source_cues) {
            Ok(contents) => (
                contents,
                request
                    .source_language
                    .as_deref()
                    .unwrap_or("und")
                    .to_owned(),
                "srt".to_owned(),
            ),
            Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
        }
    };
    let (contents, format) = if request.prepared {
        (
            vtt_to_srt(&subtitle_to_vtt(&contents, &format)),
            "srt".to_owned(),
        )
    } else {
        (contents, format)
    };
    let generation_started = std::time::Instant::now();
    info!(
        media = %media.title,
        prepared = request.prepared,
        source_stream_id = request.stream_id,
        first_language = %request.first_language,
        second_language = %request.second_language,
        client_source_cues = request.source_cues.len(),
        "bilingual subtitle generation started"
    );
    let started = spawn_bilingual_generation(
        &state,
        &media,
        &request.first_language,
        &request.second_language,
        contents,
        source_language,
        format,
        true,
    )
    .await;
    let checkpoint = staged_bilingual_vtt(
        &state,
        &media,
        &request.first_language,
        &request.second_language,
    )
    .await;
    if let Some(vtt) = checkpoint {
        info!(
            media = %media.title,
            prepared = request.prepared,
            started,
            elapsed_ms = generation_started.elapsed().as_millis(),
            "bilingual subtitle generation checkpoint ready"
        );
        return (
            [
                (header::CONTENT_TYPE, "text/vtt; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            vtt,
        )
            .into_response();
    }
    warn!(media = %media.title, "bilingual subtitle generation could not create a checkpoint");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Bilingual subtitle generation could not be started",
    )
        .into_response()
}

async fn audiobook_media_key_for_web_path(
    state: &AppState,
    library: Option<usize>,
    folder: Option<usize>,
    relative: &std::path::Path,
) -> Option<String> {
    let is_audiobook = library
        .and_then(|index| {
            state
                .config
                .lock()
                .ok()
                .and_then(|config| config.libraries.get(index).cloned())
        })
        .is_some_and(|config| is_audiobook_library(&config.kind));
    if !is_audiobook {
        return None;
    }
    let root = selected_library_folder_root(state, library, folder);
    let path = web_path(state, &root, relative).await.ok()?;
    let catalog = crate::audiobookshelf::catalog::Catalog::load_async(state).await;
    catalog
        .find_item_path(library.unwrap_or_default(), relative, &root, &path)
        .map(|item| item.id.clone())
}

pub(crate) async fn web_progress(
    State(state): State<AppState>,
    Query(query): Query<ProgressQuery>,
) -> Response {
    let Some(path) = clean_relative_path(&query.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let root = selected_library_folder_root(&state, query.library, query.folder);
    if web_path(&state, &root, &path).await.is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let media_key = audiobook_media_key_for_web_path(&state, query.library, query.folder, &path)
        .await
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let progress = connection.query_row(
        "SELECT position_ms, duration_ms, watched FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
        (USER_ID, media_key.as_str()),
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, bool>(2)?)),
    );
    match progress {
        Ok((position_ms, duration_ms, watched)) => Json(serde_json::json!({
            "positionMs": position_ms,
            "durationMs": duration_ms,
            "watched": watched
        }))
        .into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Json(serde_json::json!({"positionMs": 0, "durationMs": null, "watched": false}))
                .into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn stop_web_translation(
    State(state): State<AppState>,
    Json(request): Json<WebTranslationStopRequest>,
) -> Response {
    let Some(relative) = clean_relative_path(&request.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, request.library, request.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    cancel_streaming_translation_jobs(&media.id);
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn update_web_progress(
    State(state): State<AppState>,
    Json(update): Json<ProgressUpdate>,
) -> Response {
    let Some(path) = clean_relative_path(&update.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let root = selected_library_folder_root(&state, update.library, update.folder);
    if web_path(&state, &root, &path).await.is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let media_key = audiobook_media_key_for_web_path(&state, update.library, update.folder, &path)
        .await
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    if connection
        .execute(
            "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, CASE WHEN ?5 THEN ?6 ELSE NULL END) ON CONFLICT(user_id, media_key) DO UPDATE SET position_ms = excluded.position_ms, duration_ms = excluded.duration_ms, watched = excluded.watched, hidden_from_continue = 0, updated_at = excluded.updated_at, finished_at = CASE WHEN excluded.watched = 1 THEN COALESCE(watch_progress.finished_at, excluded.updated_at) ELSE NULL END",
            (USER_ID, media_key.as_str(), update.position_ms.max(0), update.duration_ms, update.watched, updated_at),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn web_watch_action(
    State(state): State<AppState>,
    Json(update): Json<WebWatchActionUpdate>,
) -> Response {
    let Some(relative) = clean_relative_path(&update.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, update.library, update.folder);
    let audiobook_media_key =
        audiobook_media_key_for_web_path(&state, update.library, update.folder, &relative).await;
    let media_keys = if let Some(media_key) = audiobook_media_key {
        vec![media_key]
    } else {
        let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
            warn!(path = %relative.display(), library = ?update.library, action = %update.action, "Web watch action could not find media");
            return StatusCode::NOT_FOUND.into_response();
        };
        let Ok(media_key) = media.path.strip_prefix(&root) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let media_key = media_key.to_string_lossy().into_owned();
        if update.scope.as_deref() == Some("show") {
            let library_config = update.library.and_then(|index| {
                state
                    .config
                    .lock()
                    .ok()
                    .and_then(|config| config.libraries.get(index).cloned())
            });
            if let Some(library_config) = library_config.filter(|library| library.kind == "tv") {
                let show_folder = relative
                    .components()
                    .next()
                    .map(|component| PathBuf::from(component.as_os_str()));
                show_folder
                    .map(|show_folder| {
                        media_snapshot(&state)
                            .iter()
                            .filter(|item| media_in_library(item, &library_config))
                            .filter_map(|item| {
                                let relative = item.path.strip_prefix(&root).ok()?;
                                relative
                                    .starts_with(&show_folder)
                                    .then(|| relative.to_string_lossy().into_owned())
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|keys| !keys.is_empty())
                    .unwrap_or_else(|| vec![media_key.clone()])
            } else {
                vec![media_key]
            }
        } else {
            vec![media_key]
        }
    };
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let sql = match update.action.as_str() {
        "watched" => "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at, finished_at) VALUES (?1, ?2, 0, NULL, 1, 0, ?3, ?3) ON CONFLICT(user_id, media_key) DO UPDATE SET position_ms = 0, watched = 1, hidden_from_continue = 0, updated_at = excluded.updated_at, finished_at = COALESCE(watch_progress.finished_at, excluded.updated_at)",
        "unwatched" => "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at, finished_at) VALUES (?1, ?2, 0, NULL, 0, 0, ?3, NULL) ON CONFLICT(user_id, media_key) DO UPDATE SET position_ms = 0, watched = 0, hidden_from_continue = 0, updated_at = excluded.updated_at, finished_at = NULL",
        "remove_from_continue" => "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at) VALUES (?1, ?2, 0, NULL, 0, 1, ?3) ON CONFLICT(user_id, media_key) DO UPDATE SET hidden_from_continue = 1, updated_at = excluded.updated_at",
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    for media_key in &media_keys {
        if let Err(error) = connection.execute(sql, (USER_ID, media_key, updated_at)) {
            warn!(action = %update.action, error = %error, "Web watch action database update failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    info!(
        action = %update.action,
        scope = ?update.scope,
        library = ?update.library,
        media_keys = ?media_keys,
        "Web watch action saved"
    );
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn set_audiobook_cover(
    State(state): State<AppState>,
    Query(query): Query<WebPath>,
    body: axum::body::Bytes,
) -> Response {
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(library_index) = query.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if body.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(extension) = crate::audiobookshelf::scanner::image_extension(&body) else {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    };
    let root = selected_library_folder_root(&state, query.library, query.folder);
    let requested_path = match web_path(&state, &root, &relative).await {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    let catalog = crate::audiobookshelf::catalog::Catalog::load_async(&state).await;
    let Some(item) = catalog.find_item_path(library_index, &relative, &root, &requested_path)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let store = crate::audiobookshelf::catalog::item_sidecar_dir(&state, library_index, item);
    let Some(cover) = crate::audiobookshelf::catalog::write_cover_file(&store, &body, extension)
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    crate::audiobookshelf::catalog::store_cover_reference(&store, &cover);
    crate::audiobookshelf::catalog::invalidate_cache();
    info!(
        path = %relative.display(),
        library = library_index,
        "Audiobook cover image saved"
    );
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub(crate) struct AudiobookCoverQuery {
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) path: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct AudiobookCoverSelection {
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) file: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct AudiobookCoverUrl {
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) path: String,
    pub(crate) url: String,
}

#[derive(Deserialize)]
pub(crate) struct AudiobookCoverImageQuery {
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) path: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) file: Option<String>,
}

pub(crate) async fn audiobook_cover_options(
    State(state): State<AppState>,
    Query(query): Query<AudiobookCoverQuery>,
) -> Response {
    let Some(library_index) = query.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(item) = audiobook_item_for_match(&state, library_index, query.folder, &relative).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let sidecar = crate::audiobookshelf::catalog::item_sidecar_dir(&state, library_index, &item);
    let folder = item
        .path
        .parent()
        .filter(|_| item.is_file)
        .unwrap_or(&item.path);
    let stored = crate::audiobookshelf::scanner::images_in_dir(&sidecar)
        .into_iter()
        .filter_map(|path| {
            let file = path.file_name()?.to_str()?.to_owned();
            Some(serde_json::json!({
                "file": file,
                "label": format!("Stored: {file}"),
                "url": audiobook_cover_image_url(library_index, query.folder, &relative, "stored", Some(&file))
            }))
        })
        .collect::<Vec<_>>();
    let mut folder_images = crate::audiobookshelf::scanner::images_in_dir(folder)
        .into_iter()
        .filter(|path| {
            !item.is_file
                || path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| {
                        stem.eq_ignore_ascii_case("cover")
                            || stem.eq_ignore_ascii_case(&crate::audiobookshelf::scanner::file_stem(
                                &item.path,
                            ))
                    })
        })
        .filter_map(|path| {
            let file = path.file_name()?.to_str()?.to_owned();
            Some(serde_json::json!({
                "file": file,
                "label": file,
                "url": audiobook_cover_image_url(library_index, query.folder, &relative, "folder", Some(&file))
            }))
        })
        .collect::<Vec<_>>();
    folder_images.sort_by(|left, right| left["file"].as_str().cmp(&right["file"].as_str()));
    let embedded = item
        .audio_files
        .first()
        .and_then(|file| crate::audiobookshelf::scanner::embedded_cover(&file.path))
        .is_some();
    Json(serde_json::json!({
        "stored": stored,
        "folder": folder_images,
        "embedded": embedded,
        "embeddedUrl": embedded.then(|| audiobook_cover_image_url(library_index, query.folder, &relative, "embedded", None))
    }))
    .into_response()
}

fn audiobook_cover_image_url(
    library: usize,
    folder: Option<usize>,
    path: &std::path::Path,
    source: &str,
    file: Option<&str>,
) -> String {
    let mut url = format!(
        "/web/audiobook-cover/image?library={library}&folder={}&path={}&source={source}",
        folder.unwrap_or_default(),
        url_encode(&path.to_string_lossy())
    );
    if let Some(file) = file {
        url.push_str("&file=");
        url.push_str(&url_encode(file));
    }
    url
}

pub(crate) async fn audiobook_cover_image(
    State(state): State<AppState>,
    Query(query): Query<AudiobookCoverImageQuery>,
) -> Response {
    let Some(library_index) = query.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(item) = audiobook_item_for_match(&state, library_index, query.folder, &relative).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let source = query.source.as_deref().unwrap_or_default();
    let selected = match source {
        "embedded" => item
            .audio_files
            .first()
            .and_then(|file| crate::audiobookshelf::scanner::embedded_cover(&file.path))
            .map(|(bytes, extension)| {
                (
                    bytes,
                    crate::audiobookshelf::catalog::image_content_type(std::path::Path::new(
                        &format!("cover.{extension}"),
                    )),
                )
            }),
        "stored" => query.file.as_deref().and_then(|file| {
            safe_cover_file(file).and_then(|file| {
                let path =
                    crate::audiobookshelf::catalog::item_sidecar_dir(&state, library_index, &item)
                        .join(file);
                std::fs::read(&path).ok().map(|bytes| {
                    (
                        bytes,
                        crate::audiobookshelf::catalog::image_content_type(&path),
                    )
                })
            })
        }),
        "folder" => query.file.as_deref().and_then(|file| {
            safe_cover_file(file).and_then(|file| {
                let directory = item
                    .path
                    .parent()
                    .filter(|_| item.is_file)
                    .unwrap_or(&item.path);
                let path = directory.join(file);
                std::fs::read(&path).ok().map(|bytes| {
                    (
                        bytes,
                        crate::audiobookshelf::catalog::image_content_type(&path),
                    )
                })
            })
        }),
        _ => None,
    };
    let Some((bytes, content_type)) = selected else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response()
}

pub(crate) async fn select_audiobook_cover(
    State(state): State<AppState>,
    Json(selection): Json<AudiobookCoverSelection>,
) -> Response {
    let Some(library_index) = selection.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(relative) = clean_relative_path(&selection.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(item) =
        audiobook_item_for_match(&state, library_index, selection.folder, &relative).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let bytes = match selection.source.as_str() {
        "embedded" => item
            .audio_files
            .first()
            .and_then(|file| crate::audiobookshelf::scanner::embedded_cover(&file.path))
            .map(|(bytes, _)| bytes),
        "stored" => selection.file.as_deref().and_then(|file| {
            safe_cover_file(file).and_then(|file| {
                let path =
                    crate::audiobookshelf::catalog::item_sidecar_dir(&state, library_index, &item)
                        .join(file);
                std::fs::read(&path).ok()
            })
        }),
        "folder" => selection.file.as_deref().and_then(|file| {
            safe_cover_file(file).and_then(|file| {
                let directory = item
                    .path
                    .parent()
                    .filter(|_| item.is_file)
                    .unwrap_or(&item.path);
                std::fs::read(directory.join(file)).ok()
            })
        }),
        _ => None,
    };
    let Some(bytes) = bytes else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(extension) = crate::audiobookshelf::scanner::image_extension(&bytes) else {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    };
    let store = crate::audiobookshelf::catalog::item_sidecar_dir(&state, library_index, &item);
    let Some(cover) = crate::audiobookshelf::catalog::write_cover_file(&store, &bytes, extension)
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    crate::audiobookshelf::catalog::store_cover_reference(&store, &cover);
    crate::audiobookshelf::catalog::invalidate_cache();
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn set_audiobook_cover_url(
    State(state): State<AppState>,
    Json(request): Json<AudiobookCoverUrl>,
) -> Response {
    let Some(library_index) = request.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(relative) = clean_relative_path(&request.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let parsed = match reqwest::Url::parse(request.url.trim()) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let Some(item) =
        audiobook_item_for_match(&state, library_index, request.folder, &relative).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("harbinger/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let response = match client.get(parsed).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "Could not download that cover image." })),
            )
                .into_response()
        }
    };
    let bytes = match response.bytes().await {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= 10 * 1024 * 1024 => bytes.to_vec(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "The cover image is empty or too large." })),
            )
                .into_response()
        }
    };
    let Some(extension) = crate::audiobookshelf::scanner::image_extension(&bytes) else {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    };
    let store = crate::audiobookshelf::catalog::item_sidecar_dir(&state, library_index, &item);
    let Some(cover) = crate::audiobookshelf::catalog::write_cover_file(&store, &bytes, extension)
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    crate::audiobookshelf::catalog::store_cover_reference(&store, &cover);
    crate::audiobookshelf::catalog::invalidate_cache();
    StatusCode::NO_CONTENT.into_response()
}

fn safe_cover_file(name: &str) -> Option<&str> {
    let path = std::path::Path::new(name);
    (path.components().count() == 1 && !name.starts_with('.')).then_some(name)
}

pub(crate) async fn web_subtitle_selection(
    State(state): State<AppState>,
    Query(query): Query<WebSubtitleSelectionQuery>,
) -> Response {
    let Some(relative) = clean_relative_path(&query.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, query.library, query.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let subtitles = subtitle_tracks(&state, &media, &relative, query.library, query.folder).await;
    let stream_id = selected_web_subtitle_stream_id(&state, &media, &subtitles);
    Json(serde_json::json!({ "streamId": stream_id })).into_response()
}

pub(crate) async fn web_subtitle_selection_socket(
    State(state): State<AppState>,
    Query(query): Query<WebSubtitleSelectionQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(relative) = clean_relative_path(&query.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, query.library, query.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let part_id = plex_part_id(&media);
    let mut updates = web_subtitle_selection_broadcast().subscribe();
    let subtitles = subtitle_tracks(&state, &media, &relative, query.library, query.folder).await;
    let stream_id = selected_web_subtitle_stream_id(&state, &media, &subtitles);
    upgrade
        .on_upgrade(move |mut socket| async move {
            let snapshot = serde_json::json!({ "streamId": stream_id });
            if socket
                .send(Message::Text(snapshot.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
            let mut heartbeat = tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(30),
                Duration::from_secs(30),
            );
            loop {
                tokio::select! {
                    message = socket.recv() => match message {
                        Some(Ok(Message::Ping(payload))) => {
                            if socket.send(Message::Pong(payload)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(Message::Text(_) | Message::Binary(_) | Message::Pong(_))) => {}
                    },
                    update = updates.recv() => match update {
                        Ok((updated_part_id, stream_id)) if updated_part_id == part_id => {
                            let event = serde_json::json!({ "streamId": stream_id });
                            if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let stream_id =
                                selected_web_subtitle_stream_id(&state, &media, &subtitles);
                            let snapshot = serde_json::json!({ "streamId": stream_id });
                            if socket.send(Message::Text(snapshot.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                    _ = heartbeat.tick() => {
                        if socket.send(Message::Ping(vec![b'?'].into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .into_response()
}

pub(crate) async fn update_web_audio_selection(
    State(state): State<AppState>,
    Json(update): Json<WebAudioSelectionUpdate>,
) -> Response {
    let Some(relative) = clean_relative_path(&update.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, update.library, update.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if update.stream_id != 0
        && !web_audio_tracks(&state, &media)
            .await
            .iter()
            .any(|track| track.stream_id == update.stream_id)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let current = stored_plex_stream_selection(&state, &media).unwrap_or_default();
    save_plex_stream_selection(
        &state,
        &media,
        PlexStreamSelection {
            audio_stream_id: (update.stream_id != 0).then_some(update.stream_id),
            ..current
        },
    );
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn update_web_subtitle_selection(
    State(state): State<AppState>,
    Json(update): Json<WebSubtitleSelectionUpdate>,
) -> Response {
    let Some(relative) = clean_relative_path(&update.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&state, update.library, update.folder);
    let Some(media) = web_media_for_relative(&state, &root, &relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let external = external_subtitle_files(&media.path).await;
    let info = ensure_media_info(&state, &media)
        .await
        .unwrap_or_else(|| serde_json::json!({"media": {"track": []}}));
    if update.stream_id != 0 {
        let is_external = external
            .iter()
            .any(|path| plex_external_subtitle_id(&media, path) == update.stream_id);
        let is_translated = state.config.lock().ok().is_some_and(|config| {
            update.stream_id
                == plex_translated_subtitle_id(
                    &media,
                    &config.subtitle_language_primary,
                    &config.subtitle_language_secondary,
                )
        });
        let is_embedded = technical_tracks(&info).iter().any(|track| {
            technical_text(track, "@type") == Some("Text")
                && technical_track_id(track).saturating_add(1) == update.stream_id
        });
        if !is_external && !is_embedded && !is_translated {
            return StatusCode::BAD_REQUEST.into_response();
        }
    }
    let selection = stored_plex_stream_selection(&state, &media).unwrap_or_default();
    save_plex_stream_selection(
        &state,
        &media,
        PlexStreamSelection {
            subtitle_stream_id: Some(update.stream_id),
            ..selection
        },
    );
    let _ = web_subtitle_selection_broadcast().send((plex_part_id(&media), update.stream_id));
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn web_media_for_relative(
    state: &AppState,
    root: &std::path::Path,
    relative: &std::path::Path,
) -> Option<Media> {
    let path = web_path(state, root, relative).await.ok()?;
    if !path.is_file() {
        return None;
    }
    let media_index = media_snapshot(state);
    for media in media_index.iter() {
        if tokio::fs::canonicalize(&media.path)
            .await
            .is_ok_and(|candidate| candidate == path)
        {
            return Some(media.clone());
        }
    }
    None
}

pub(crate) async fn start_plex_auth(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(_) => return settings_error("Could not initialize Plex authentication"),
    };
    let configured_server_name = server_name(&state);
    let auth_product = plex_auth_product(&state);
    let auth_client_identifier = plex_auth_client_identifier(&state);
    let Ok((signing_key, kid)) = plex_device_key(&state) else {
        return settings_error("Could not initialize Plex device authentication");
    };
    let response = match client
        .post("https://clients.plex.tv/api/v2/pins")
        .json(&serde_json::json!({
            "jwk": plex_device_jwk(&signing_key, &kid),
            "strong": true
        }))
        .header("Accept", "application/json")
        .header("X-Plex-Product", &auth_product)
        .header("X-Plex-Client-Identifier", &auth_client_identifier)
        .send()
        .await
    {
        Ok(response) => match response.error_for_status() {
            Ok(response) => response,
            Err(error) => {
                warn!(%error, "Plex PIN request was rejected");
                return settings_error("Plex.tv could not start sign-in");
            }
        },
        Err(error) => {
            warn!(%error, "Plex PIN request failed");
            return settings_error("Plex.tv could not be reached");
        }
    };
    let Ok(pin) = response.json::<PlexPinResponse>().await else {
        return settings_error("Plex.tv returned an invalid authentication response");
    };
    info!(client_identifier = %auth_client_identifier, product = "harbinger", server_name = %configured_server_name, "Created Plex sign-in PIN");
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default();
    if connection
        .execute(
            "DELETE FROM plex_auth WHERE created_at < ?1",
            [created_at.saturating_sub(30 * 60 * 1000)],
        )
        .is_err()
        || connection
            .execute(
                "INSERT OR REPLACE INTO plex_auth (id, code, created_at) VALUES (?1, ?2, ?3)",
                (pin.id, &pin.code, created_at),
            )
            .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1:32400");
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let callback = format!("{proto}://{host}/settings/plex/callback?pin_id={}", pin.id);
    let link = plex_auth_url(
        &auth_client_identifier,
        &pin.code,
        &configured_server_name,
        &callback,
    );
    let mut response = Redirect::to(&link).into_response();
    refresh_web_session_cookie(&headers, &mut response);
    response
}

pub(crate) async fn disconnect_plex(State(state): State<AppState>) -> Response {
    let credentials = state.db.lock().ok().and_then(|connection| {
        connection
            .query_row(
                "SELECT plex_account_token FROM server_identity WHERE id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
            .map(|token| (token, state.machine_identifier.to_string()))
    });
    let Some((token, machine_identifier)) = credentials else {
        return settings_error("harbinger is not connected to Plex.tv");
    };
    let client = reqwest::Client::new();
    let devices = match client
        .get("https://plex.tv/devices.xml")
        .header("Accept", "application/xml")
        .header("X-Plex-Product", "harbinger")
        .header("X-Plex-Client-Identifier", &machine_identifier)
        .header("X-Plex-Token", &token)
        .send()
        .await
    {
        Ok(response) => match response.error_for_status() {
            Ok(response) => response.text().await.ok(),
            Err(_) => None,
        },
        Err(_) => None,
    };
    if let Some(body) = devices {
        if let Some(device_id) = find_plex_device_id(&body, &machine_identifier) {
            let result = client
                .delete(format!("https://plex.tv/devices/{device_id}.xml"))
                .header("Accept", "application/xml")
                .header("X-Plex-Product", "harbinger")
                .header("X-Plex-Client-Identifier", &machine_identifier)
                .header("X-Plex-Token", &token)
                .send()
                .await;
            if !matches!(
                result.and_then(|response| response.error_for_status()),
                Ok(_)
            ) {
                return settings_error("Plex.tv refused to revoke harbinger");
            }
        }
    }
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection
        .execute(
            "UPDATE server_identity SET plex_account_token = NULL, claimed = 0, plex_last_error = NULL WHERE id = 1",
            [],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to("/settings?metadata_status=Disconnected%20and%20revoked%20from%20Plex.tv")
        .into_response()
}

pub(crate) async fn translation_settings_page() -> Response {
    Redirect::to("/settings#language-settings").into_response()
}

pub(crate) async fn save_translation_settings(
    State(state): State<AppState>,
    Form(form): Form<TranslationSettingsForm>,
) -> Response {
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let audio_language_primary = form
        .audio_language_primary
        .unwrap_or_else(|| config.audio_language_primary.clone())
        .trim()
        .to_ascii_lowercase();
    let audio_language_secondary = form
        .audio_language_secondary
        .unwrap_or_else(|| config.audio_language_secondary.clone().unwrap_or_default())
        .trim()
        .to_ascii_lowercase();
    let audio_language_secondary = (!audio_language_secondary.is_empty()
        && audio_language_secondary != "none")
        .then_some(audio_language_secondary);
    let subtitle_primary = form.language_primary.trim().to_ascii_lowercase();
    let subtitle_secondary = form.language_secondary.trim().to_ascii_lowercase();
    let supported = |language: &str| isolang::Language::from_639_1(language).is_some();
    let bilingual_enabled = subtitle_primary != "off" && form.bilingual_enabled;
    if !supported(&audio_language_primary)
        || audio_language_secondary
            .as_deref()
            .is_some_and(|language| !supported(language))
        || audio_language_secondary.as_deref() == Some(audio_language_primary.as_str())
        || (subtitle_primary != "off" && !supported(&subtitle_primary))
        || !supported(&subtitle_secondary)
        || (subtitle_primary != "off" && subtitle_primary == subtitle_secondary)
    {
        return (
            StatusCode::BAD_REQUEST,
            "Choose distinct supported languages for voice and subtitles",
        )
            .into_response();
    }
    config.audio_language_primary = audio_language_primary.clone();
    config.audio_language_secondary = audio_language_secondary;
    config.subtitle_language_primary = subtitle_primary.clone();
    config.subtitle_language_secondary = subtitle_secondary;
    config.bilingual_subtitles_enabled = bilingual_enabled;
    config
        .plex_account_preferences
        .insert("defaultAudioLanguage".to_owned(), audio_language_primary);
    config.plex_account_preferences.insert(
        "defaultSubtitleLanguage".to_owned(),
        subtitle_primary.clone(),
    );
    config
        .plex_account_preferences
        .insert("autoSelectAudio".to_owned(), "1".to_owned());
    config.plex_account_preferences.insert(
        "subtitleMode".to_owned(),
        if subtitle_primary == "off" { "0" } else { "2" }.to_owned(),
    );
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    store_config_settings(&state, &config);
    if !config.bilingual_subtitles_enabled {
        cancel_all_translation_jobs();
    }
    Redirect::to("/settings#language-settings").into_response()
}

pub(crate) async fn save_skip_settings(
    State(state): State<AppState>,
    Form(form): Form<SkipSettingsForm>,
) -> Response {
    let (Some(skip_intro_mode), Some(skip_credits_mode)) = (
        parse_skip_mode(&form.skip_intro_mode),
        parse_skip_mode(&form.skip_credits_mode),
    ) else {
        return (
            StatusCode::BAD_REQUEST,
            "Choose Automatic, Manual, or Off for both skip settings",
        )
            .into_response();
    };
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    config.skip_intro_mode = skip_intro_mode;
    config.skip_credits_mode = skip_credits_mode;
    config.marker_detection_enabled = !form.marker_detection_disabled;
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to("/settings#skip-settings").into_response()
}

pub(crate) async fn settings_page(
    State(state): State<AppState>,
    Query(query): Query<SettingsQuery>,
) -> Response {
    let (
        libraries,
        tmdb_token_saved,
        bilingual_enabled,
        audio_language_primary,
        audio_language_secondary,
        subtitle_language_primary,
        subtitle_language_secondary,
        skip_intro_mode,
        skip_credits_mode,
        marker_detection_enabled,
    ) = {
        let Ok(config) = state.config.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let libraries = config
            .libraries
            .iter()
            .enumerate()
            .map(|(id, library)| {
                (
                    id as i64,
                    library.name.clone(),
                    library.kind.clone(),
                    library
                        .folders
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                    library.show_recently_added,
                    library.show_continue_watching,
                    library.scan_enabled,
                    library.scan_interval_value,
                    library.scan_interval_unit.clone(),
                )
            })
            .collect::<Vec<_>>();
        let token_saved = config
            .tmdb_bearer_token
            .as_deref()
            .is_some_and(|token| !token.trim().is_empty());
        (
            libraries,
            token_saved,
            config.bilingual_subtitles_enabled,
            config.audio_language_primary.clone(),
            config.audio_language_secondary.clone(),
            config.subtitle_language_primary.clone(),
            config.subtitle_language_secondary.clone(),
            config.skip_intro_mode,
            config.skip_credits_mode,
            config.marker_detection_enabled,
        )
    };
    let list = libraries
        .iter()
        .map(
            |(
                id,
                name,
                kind,
                folders,
                show_recently_added,
                show_continue_watching,
                scan_enabled,
                interval_value,
                interval_unit,
            )| {
                let unit_options = ["seconds", "minutes", "hours"]
                    .into_iter()
                    .map(|unit| {
                        format!(
                            "<option value=\"{unit}\" {}>{unit}</option>",
                            if unit == interval_unit {
                                "selected"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect::<String>();
                let library_settings = format!(
                    concat!(
                        "<form class=\"library-settings\" id=\"library-settings-{0}\" method=\"post\" action=\"/settings/libraries/{0}/save\" aria-label=\"Library settings\"></form><div class=\"library-rename\" style=\"display:flex;flex-wrap:nowrap;align-items:center;gap:.55rem\"><label for=\"library-name-{0}\" style=\"flex:0 0 auto\">Library name</label><input id=\"library-name-{0}\" name=\"name\" type=\"text\" form=\"library-settings-{0}\" value=\"{1}\" maxlength=\"64\" required style=\"flex:1 1 auto;width:auto;min-width:0\"></div>",
                        "<div class=\"library-scan-settings\"><strong>Automatic scan</strong><label><input type=\"checkbox\" name=\"scan_enabled\" value=\"true\" form=\"library-settings-{0}\" {2}> Enabled</label><label>Every <input type=\"number\" name=\"scan_interval_value\" min=\"1\" value=\"{3}\" required form=\"library-settings-{0}\"><select name=\"scan_interval_unit\" form=\"library-settings-{0}\">{4}</select></label>",
                        "<div class=\"library-scan-actions\" style=\"display:flex;flex:0 0 auto;align-items:center;gap:.4rem;white-space:nowrap\"><button type=\"submit\" form=\"library-settings-{0}\" formmethod=\"post\" formaction=\"/settings/libraries/{0}/scan\" formnovalidate>Scan metadata</button><button type=\"submit\" form=\"library-settings-{0}\" formmethod=\"post\" formaction=\"/settings/libraries/{0}/scan-files\" formnovalidate>Scan files</button><button class=\"button-secondary\" type=\"submit\" form=\"library-settings-{0}\" formmethod=\"post\" formaction=\"/settings/libraries/{0}/scan-files?force=true\" formnovalidate>Force scan &amp; detect</button><button class=\"button-secondary\" type=\"submit\" form=\"library-settings-{0}\" formmethod=\"post\" formaction=\"/settings/libraries/{0}/scan?force=true\" formnovalidate>Force metadata</button></div></div>",
                        "<div class=\"library-display\" style=\"margin-top:.8rem\"><fieldset><legend>Show on Home</legend><label><input type=\"checkbox\" name=\"show_recently_added\" value=\"true\" form=\"library-settings-{0}\" {5}> Recently Added</label><label><input type=\"checkbox\" name=\"show_continue_watching\" value=\"true\" form=\"library-settings-{0}\" {6}> Continue Watching</label></fieldset></div>"
                    ),

                    id,
                    html(name),
                    if *scan_enabled { "checked" } else { "" },
                    interval_value,
                    unit_options,
                    if *show_recently_added { "checked" } else { "" },
                    if *show_continue_watching { "checked" } else { "" },
                );
                let add_folder_control = format!(
                    "<details class=\"library-add-folder\" style=\"margin:0\"><summary style=\"min-height:2.65rem;padding:.65rem .95rem;font-size:.87rem\">Add folder</summary><form method=\"post\" action=\"/settings/folders\" class=\"inline-form\"><input type=\"hidden\" name=\"library_id\" value=\"{id}\"><label class=\"sr-only\" for=\"folder-{id}\">Folder path</label><input id=\"folder-{id}\" name=\"path\" type=\"text\" placeholder=\"/another/folder\" required><button type=\"submit\">Add folder</button><button class=\"button-secondary\" type=\"button\" onclick=\"this.form.reset();this.closest('details').open=false\">Cancel</button></form></details>"
                );
                let folder_rows = folders
                    .iter()
                    .enumerate()
                    .map(|(folder_index, path)| {
                        let folder_path = html(path);
                        format!(
                            "<li class=\"library-folder-row\" style=\"display:flex;align-items:center;justify-content:space-between;gap:.55rem\"><code style=\"flex:1 1 auto;min-width:0\">{folder_path}</code><form method=\"post\" action=\"/settings/folders/remove\" class=\"inline-form\" data-settings-delete=\"folder\" style=\"margin:0\" onsubmit=\"return confirm('Remove this folder from the library? Media files will not be deleted.')\"><input type=\"hidden\" name=\"library_id\" value=\"{id}\"><input type=\"hidden\" name=\"folder_index\" value=\"{folder_index}\"><input type=\"hidden\" name=\"path\" value=\"{folder_path}\"><button class=\"button-secondary\" type=\"submit\" style=\"min-height:2.2rem;padding:.45rem .65rem;font-size:.8rem\">Remove folder</button></form></li>"
                        )
                    })
                    .collect::<String>();
                format!(
                    "<details class=\"library-card\" data-library-id=\"{id}\"><summary class=\"library-summary\"><span class=\"library-summary-title\"><strong>{}</strong><span class=\"type-pill\">{}</span></span><span class=\"folder-count\">{} folders</span></summary><div class=\"library-content\">{}<section class=\"library-folders\" aria-label=\"Folders\"><div style=\"display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:.55rem;margin-bottom:.6rem\"><h4 style=\"margin:0\">Folders</h4>{}</div><ul>{}</ul></section><div class=\"library-footer\" style=\"display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:1rem;margin-top:.9rem\"><form method=\"post\" action=\"/settings/libraries/{}/delete\" class=\"library-delete\" data-settings-delete=\"library\" style=\"display:flex;justify-content:flex-start;margin:0\" onsubmit=\"return confirm('Delete this library from Harbinger? Media files will not be deleted.')\"><button class=\"button-danger\" type=\"submit\">Delete library</button></form><button class=\"button-secondary\" type=\"submit\" form=\"library-settings-{id}\" style=\"margin-left:auto\">Save changes</button></div></div></details>",
                    html(name),
                    html(library_kind_label(kind)),
                    folders.len(),
                    library_settings,
                    add_folder_control,
                    folder_rows,
                    id,
                )
            },
        )
        .collect::<String>();
    let mut list = list;
    list.insert_str(
        0,
        r#"<form class="library-card" method="post" action="/settings/scan"><div style="display:flex;flex-wrap:wrap;gap:.4rem"><button type="submit">Scan all libraries</button><button class="button-secondary" type="submit" formaction="/settings/scan?force=true">Force scan all &amp; detect</button></div></form>"#,
    );
    let subtitle_settings = format!(
        r#"<details class="settings-card settings-section" id="language-settings"><summary class="settings-section-summary"><span><strong>Playback languages</strong><small>Choose preferred voice and subtitle languages, including the bilingual subtitle pair.</small></span></summary><form method="post" action="/settings" class="settings-form settings-section-content"><p class="note">Primary and secondary voice languages set the preferred audio tracks shown to Plex clients and the built-in player. Bilingual subtitles combine the selected subtitle languages. No API key is required. Subtitle text is sent to Google for translation. Generated subtitle files use the selected metadata location.</p><div class="form-grid"><label class="field"><span>Primary voice</span><select name="audio_language_primary">{}</select></label><label class="field"><span>Secondary voice</span><select name="audio_language_secondary">{}</select></label><label class="field"><span>Primary subtitles</span><select name="language_primary">{}</select></label><label class="field"><span>Secondary subtitles</span><select name="language_secondary">{}</select></label></div><label class="settings-toggle"><input type="checkbox" name="bilingual_enabled" value="true"{}> Enable bilingual subtitles</label><div><button type="submit">Save language settings</button></div></form></details>"#,
        translation_language_options(&audio_language_primary),
        voice_language_options(audio_language_secondary.as_deref()),
        subtitle_language_options(&subtitle_language_primary),
        translation_language_options(&subtitle_language_secondary),
        if bilingual_enabled { " checked" } else { "" },
    );
    let skip_settings = format!(
        r#"<details class="settings-card settings-section" id="skip-settings"><summary class="settings-section-summary"><span><strong>Intro and credits skipping</strong><small>Choose how Harbinger's player reacts to detected markers.</small></span></summary><form method="post" action="/settings/skipping" class="settings-form settings-section-content"><p class="note">Automatic seeks to two seconds before a marker ends. Manual shows a Skip Intro or Skip Credits button while playback is inside the marker. Off hides skip controls and disables automatic skipping.</p><div class="form-grid"><label class="field"><span>Intro skipping</span><select name="skip_intro_mode">{}</select></label><label class="field"><span>Credits skipping</span><select name="skip_credits_mode">{}</select></label></div><label class="settings-toggle"><input type="checkbox" name="marker_detection_disabled" value="true"{}> Disable automatic intro/credit detection</label><p class="note">Automatic detection decodes episode audio after library scans. Turning it off keeps existing markers and manual detection available.</p><div><button type="submit">Save skipping settings</button></div></form></details>"#,
        skip_mode_options(skip_intro_mode),
        skip_mode_options(skip_credits_mode),
        if marker_detection_enabled {
            ""
        } else {
            " checked"
        },
    );
    let error = query
        .error
        .as_deref()
        .map(|message| {
            format!(
                "<div class=\"flash-error\" role=\"alert\">{}</div>",
                html(message)
            )
        })
        .unwrap_or_default();
    let error = if error.is_empty() && query.inventory_error.unwrap_or(false) {
        "<div class=\"flash-error\" role=\"alert\">One or more libraries could not be scanned. Check the server log for details.</div>".to_owned()
    } else {
        error
    };
    let notice = query
        .metadata_matched
        .map(|count| {
            format!(
                "<div class=\"flash-notice\" role=\"status\">Matched {} metadata records.</div>",
                count
            )
        })
        .or_else(|| {
            query.metadata_status.as_deref().map(|status| {
                format!(
                    "<div class=\"flash-notice\" role=\"status\">{}</div>",
                    html(status)
                )
            })
        })
        .or_else(|| {
            query.server_name_saved.map(|_| {
                "<div class=\"flash-notice\" role=\"status\">Server name saved.</div>".to_owned()
            })
        })
        .or_else(|| {
            query.inventory_scan.map(|count| {
                let mode = if query.force.unwrap_or(false) {
                    "Forced scan"
                } else {
                    "Scanned"
                };
                format!(
                    "<div class=\"flash-notice\" role=\"status\">{mode} all libraries; found {count} media files.</div>"
                )
            })
        })
        .or_else(|| {
            query.library_created.filter(|created| *created).map(|_| {
                "<div class=\"flash-notice\" role=\"status\">Library created and added to Plex discovery.</div>".to_owned()
            })
        })
        .or_else(|| {
            query.library_renamed.filter(|saved| *saved).map(|_| {
                "<div class=\"flash-notice\" role=\"status\">Library name saved.</div>".to_owned()
            })
        })
        .or_else(|| {
            query.library_saved.filter(|saved| *saved).map(|_| {
                "<div class=\"flash-notice\" role=\"status\">Library settings saved.</div>".to_owned()
            })
        })
        .unwrap_or_default();
    let logs_active = query.tab.as_deref() == Some("logs");
    let dashboard_active = query.tab.as_deref() == Some("dashboard");
    let users_active = query.tab.as_deref() == Some("users");
    let settings_stack_style = if logs_active || dashboard_active || users_active {
        " style=\"display:none\""
    } else {
        ""
    };
    let log_lines = recent_log_lines();
    let logs_content = if logs_active {
        let output = format!(
            "<pre class=\"settings-logs\" id=\"settings-logs-output\">{}</pre>",
            html(&log_lines.join("\n"))
        );
        let empty_notice = if log_lines.is_empty() {
            "<p id=\"settings-logs-empty\" class=\"note\">No log entries yet.</p>"
        } else {
            "<p id=\"settings-logs-empty\" class=\"note\" hidden>No log entries yet.</p>"
        };
        let (next_level, level_label, level_title) = if debug_logging_enabled() {
            ("info", "Disable debug", "Return logging to the info level")
        } else {
            ("debug", "Enable debug", "Show debug-level diagnostic logs")
        };
        let log_level_control = format!(
            "<form method=\"post\" action=\"/settings/logs/level\" style=\"margin:0\"><input type=\"hidden\" name=\"level\" value=\"{next_level}\"><button class=\"button-secondary\" type=\"submit\" title=\"{level_title}\">{level_label}</button></form>"
        );
        format!(
            "<section class=\"settings-card\" aria-labelledby=\"logs-heading\"><div class=\"section-heading\"><div><h2 id=\"logs-heading\">Logs</h2><p class=\"section-description\">Recent server and request events. The live view keeps up to 1000 entries; downloads include up to 20000.</p></div><span id=\"settings-logs-live\" role=\"status\">Live</span></div><div class=\"log-filters\"><label for=\"settings-logs-search\">Search log entries <input id=\"settings-logs-search\" type=\"search\" placeholder=\"Search log entries…\" autocomplete=\"off\"></label><label><input id=\"settings-logs-reverse\" type=\"checkbox\"> Reverse filter (show non-matching)</label><span id=\"settings-logs-count\" role=\"status\" aria-live=\"polite\"></span><button class=\"button-secondary\" id=\"settings-logs-copy\" type=\"button\" title=\"Copy the visible filtered log entries\">Copy visible</button><span id=\"settings-logs-copy-status\" role=\"status\" aria-live=\"polite\"></span><a class=\"button-link button-secondary\" id=\"settings-logs-download\" href=\"/settings/logs/download\" download title=\"Download the recent log history as a text file\">Download log</a>{log_level_control}<form method=\"post\" action=\"/settings/logs/clear\" style=\"margin:0\"><button class=\"button-secondary\" id=\"settings-logs-clear\" type=\"submit\" title=\"Clear the in-memory log buffer\">Clear log</button></form><button class=\"button-secondary log-follow\" id=\"settings-logs-follow\" type=\"button\" aria-label=\"Scroll to latest log entries\" title=\"Scroll to latest log entries\" hidden>↓ Latest</button></div>{empty_notice}{output}</section>"
        )
    } else {
        String::new()
    };
    let dashboard_content_html = if dashboard_active {
        dashboard_markup()
    } else {
        ""
    };
    let users_content_html = if users_active {
        users_settings_markup(&state, &query)
    } else {
        String::new()
    };
    let notice = format!(
        "{}<dialog id=\"folder-picker\" aria-labelledby=\"folder-picker-title\" style=\"position:relative;width:min(48rem,calc(100vw - 2rem));height:min(38rem,calc(100vh - 2rem));padding:3.5rem 0 0;border:1px solid #394a63;border-radius:1rem;color:#e5eaf4;background:#101a2b\"><header style=\"position:absolute;top:.65rem;left:1rem;right:1rem;display:flex;align-items:center;justify-content:space-between\"><h2 id=\"folder-picker-title\">Choose a media folder</h2><button id=\"folder-picker-close\" type=\"button\" aria-label=\"Close folder picker\" title=\"Close folder picker\">×</button></header><iframe id=\"folder-picker-frame\" title=\"Media folder browser\" src=\"/settings/browse\" style=\"width:100%;height:calc(100% - 1rem);border:0;border-radius:0 0 1rem 1rem\"></iframe></dialog><script>document.addEventListener('DOMContentLoaded',()=>{{const params=new URLSearchParams(location.search);if(params.get('add_library')==='true')document.querySelector('.add-library')?.setAttribute('open','');const openLibrary=Number(params.get('library_open'));if(Number.isInteger(openLibrary)&&openLibrary>=0)document.querySelectorAll('.library-list details.library-card')[openLibrary]?.setAttribute('open','');}});</script>",
        notice
    );
    let current_server_name = html(&server_name(&state));
    let folder_picker_script = r#"<style>#folder-picker[open]{position:fixed!important;inset:0!important;margin:auto!important}#folder-picker::backdrop{background:rgba(4,8,16,.78)}#folder-picker-close{position:absolute;top:.3rem;right:0}</style><script>
const folderPicker = document.querySelector('#folder-picker');
const folderPickerFrame = document.querySelector('#folder-picker-frame');
document.addEventListener('DOMContentLoaded', () => {
  document.querySelectorAll('.library-add-folder .inline-form').forEach(form => {
    const input = form.querySelector('input[name="path"]');
    if (!input) return;
    const browse = document.createElement('a');
    browse.href = '/settings/browse';
    browse.className = 'button-link button-secondary';
    browse.textContent = 'Browse';
    input.insertAdjacentElement('afterend', browse);
  });
  if (!new URLSearchParams(location.search).has('library_open')) {
    document.querySelectorAll('.library-list details.library-card').forEach(card => card.removeAttribute('open'));
  }
});
document.addEventListener('click', event => {
  const browse = event.target.closest?.('a[href="/settings/browse"]');
  if (browse) {
    event.preventDefault();
    const input = browse.previousElementSibling;
    if (!input || !folderPicker) return;
    folderPicker.dataset.target = input.id;
    const url = new URL(browse.href, location.origin);
    if (input.value.trim()) url.searchParams.set('path', input.value.trim());
    folderPickerFrame.src = url.pathname + url.search;
    folderPicker.showModal();
    return;
  }
  if (event.target.closest?.('#folder-picker-close')) folderPicker?.close();
});
window.addEventListener('message', event => {
  if (event.origin !== location.origin || event.data?.type !== 'mplex-folder-selected') return;
  const input = document.getElementById(folderPicker?.dataset.target || '');
  if (input) input.value = event.data.path;
  folderPicker?.close();
});
</script>"#;

    let settings_delete_script = r#"<style>.settings-delete-status{flex-basis:100%;margin:0;color:#ffd6dc;font-size:.8rem}</style><script>
if (!window.__mplexSettingsDeleteBound) {
  window.__mplexSettingsDeleteBound = true;
  document.addEventListener("submit", async event => {
    const form = event.target.closest("form[data-settings-delete]");
    if (!form || event.defaultPrevented || form.dataset.pending === "true") return;
    event.preventDefault();
    form.dataset.pending = "true";
    form.setAttribute("aria-busy", "true");
    const button = form.querySelector('button[type="submit"]');
    const originalLabel = button?.textContent || "Delete";
    if (button) {
      button.disabled = true;
      button.textContent = form.dataset.settingsDelete === "folder" ? "Removing…" : "Deleting…";
    }
    form.querySelector(".settings-delete-status")?.remove();
    const showError = message => {
      const status = document.createElement("p");
      status.className = "settings-delete-status";
      status.setAttribute("role", "alert");
      status.textContent = message || "Could not complete this action. Try again.";
      form.append(status);
    };
    try {
      const response = await fetch(form.action, {
        method: "POST",
        headers: {
          "content-type": "application/x-www-form-urlencoded;charset=UTF-8",
          "X-Mplex-Async": "1"
        },
        body: new URLSearchParams(new FormData(form))
      });
      if (!response.ok) throw new Error((await response.text()).trim());
      if (form.dataset.settingsDelete === "folder") {
        const card = form.closest("details.library-card");
        form.closest("li.library-folder-row")?.remove();
        const folderCount = card?.querySelectorAll(".library-folders ul > li").length || 0;
        const countLabel = card?.querySelector(".folder-count");
        if (countLabel) countLabel.textContent = `${folderCount} folder${folderCount === 1 ? "" : "s"}`;
        return;
      }
      const deletedId = Number(form.closest("details.library-card")?.dataset.libraryId);
      const scrollTop = window.scrollY;
      const openLibraryIds = new Set(
        [...document.querySelectorAll(".library-list details.library-card[open]")]
          .map(card => Number(card.dataset.libraryId))
          .filter(id => Number.isInteger(id) && id !== deletedId)
          .map(id => id > deletedId ? id - 1 : id)
      );
      const page = await fetch("/settings", { cache: "no-store" });
      if (!page.ok) throw new Error("Library deleted. Reload Settings to refresh the library list.");
      const parsed = new DOMParser().parseFromString(await page.text(), "text/html");
      const currentList = document.querySelector(".library-list");
      const nextList = parsed.querySelector(".library-list");
      const currentNav = document.querySelector(".sidebar-nav");
      const nextNav = parsed.querySelector(".sidebar-nav");
      if (!currentList || !nextList || !currentNav || !nextNav) {
        throw new Error("Library deleted. Reload Settings to refresh the library list.");
      }
      currentList.replaceWith(nextList);
      currentNav.replaceWith(nextNav);
      nextList.querySelectorAll("details.library-card[data-library-id]").forEach(card => {
        if (openLibraryIds.has(Number(card.dataset.libraryId))) card.open = true;
      });
      window.scrollTo(0, scrollTop);
    } catch (error) {
      form.dataset.pending = "false";
      form.removeAttribute("aria-busy");
      if (button) {
        button.disabled = false;
        button.textContent = originalLabel;
      }
      showError(error instanceof Error ? error.message : "Could not complete this action. Try again.");
    }
  });
}
</script>"#;
    let notice = format!("{notice}{folder_picker_script}{settings_delete_script}");
    let tmdb_token_status = if tmdb_token_saved {
        "<p class=\"credential-status is-saved\"><span class=\"status-dot\"></span>TMDB token is saved. Leave the field blank to keep it, or enter a new token to replace it.</p>"
    } else {
        "<p class=\"credential-status\">No TMDB token is saved. Add one to scan libraries for metadata.</p>"
    };
    let config_selected = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    let plex_status = state
        .db
        .lock()
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT plex_account_token IS NOT NULL, claimed, plex_last_error FROM server_identity WHERE id = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, bool>(0)?,
                            row.get::<_, bool>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .ok()
        })
        .unwrap_or((false, false, None));
    let dashboard_styles_html = if dashboard_active {
        dashboard_styles()
    } else {
        ""
    };
    let dashboard_script_html = if dashboard_active {
        dashboard_script()
    } else {
        ""
    };
    let plex_auth = if plex_status.1 {
        if let Some(error) = plex_status.2.as_deref() {
            format!(
                "<p class=\"flash-error\" role=\"status\">Signed in, but secure Plex client publication failed: {}. HTTP access remains published.</p><div class=\"action-row\"><form method=\"get\" action=\"/settings/plex/start\"><button type=\"submit\">Reconnect to Plex.tv</button></form><form method=\"post\" action=\"/settings/plex/disconnect\"><button class=\"button-secondary\" type=\"submit\">Disconnect and revoke</button></form></div>",
                html(error)
            )
        } else {
            r#"<p class="status-line"><span class="status-dot"></span>Connected to Plex.tv and published for Plex clients.</p><div class="action-row"><form method="get" action="/settings/plex/start"><button type="submit">Reconnect to Plex.tv</button></form><form method="post" action="/settings/plex/disconnect" onsubmit="return confirm('Disconnect and revoke harbinger from Plex.tv?')"><button class="button-secondary" type="submit">Disconnect and revoke</button></form></div>"#.to_owned()
        }
    } else if plex_status.0 {
        match plex_status.2.as_deref() {
            Some(error) => format!(
                "<p class=\"flash-error\" role=\"status\">harbinger is signed in to Plex.tv, but server claiming or resource publication failed: {}.</p><div class=\"action-row\"><form method=\"get\" action=\"/settings/plex/start\"><button type=\"submit\">Connect to Plex.tv again</button></form><form method=\"post\" action=\"/settings/plex/disconnect\"><button class=\"button-secondary\" type=\"submit\">Disconnect and revoke</button></form></div>",
                html(error)
            ),
            None => r#"<p class="flash-error" role="status">harbinger has an older Plex.tv account token but has not completed server claiming or resource publication.</p><div class="action-row"><form method="get" action="/settings/plex/start"><button type="submit">Connect to Plex.tv again</button></form><form method="post" action="/settings/plex/disconnect"><button class="button-secondary" type="submit">Disconnect and revoke</button></form></div>"#.to_owned(),
        }
    } else {
        r#"<p class="note">Connect harbinger to your Plex account so Plex clients can discover and authenticate with this server.</p><div class="action-row"><form method="get" action="/settings/plex/start"><button type="submit">Connect to Plex.tv</button></form></div>"#.to_owned()
    };
    Html(add_build_info_footer(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="icon" href="/favicon.ico" sizes="any">
<title>harbinger settings</title><style>
:root {{ color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #0b1020; color: #e5eaf4; font-synthesis: none; }}
* {{ box-sizing: border-box; }} body {{ margin: 0; min-height: 100vh; background: radial-gradient(ellipse at 75% -20%, #1b3151 0, transparent 44rem), #0b1020; }}
.app-shell {{ display: grid; grid-template-columns: 240px minmax(0, 1fr); min-height: 100vh; transition: grid-template-columns .2s ease; }} .app-shell.sidebar-collapsed {{ grid-template-columns: 68px minmax(0, 1fr); }}
.sidebar {{ position: sticky; top: 0; height: 100vh; box-sizing: border-box; padding: 1.25rem .9rem; border-right: 1px solid #202a3c; background: rgba(10, 16, 29, .9); }} .sidebar-brand {{ display: block; padding: .5rem .7rem 1.5rem; color: #f8fafc; font-size: 1.25rem; font-weight: 750; text-decoration: none; }} .sidebar-top {{ display: flex; align-items: center; justify-content: space-between; padding-bottom: 1rem; }} .sidebar-top .sidebar-brand {{ padding-bottom: 0; }}
.sidebar-toggle {{ width: 2rem; height: 2rem; border: 0; border-radius: .5rem; color: #cbd5e1; background: transparent; cursor: pointer; font-size: 1.15rem; }} .sidebar-collapsed .sidebar-top {{ justify-content: center; }} .sidebar-collapsed .sidebar-brand, .sidebar-collapsed .sidebar-label, .sidebar-collapsed .sidebar-link {{ font-size: 0; }} .sidebar-collapsed .sidebar-link {{ justify-content: center; }} .sidebar-collapsed .sidebar-icon {{ font-size: 1.1rem; }} .sidebar-label {{ padding: 0 .7rem .45rem; color: #77839a; font-size: .68rem; font-weight: 700; letter-spacing: .12em; text-transform: uppercase; }}
.sidebar-nav {{ display: grid; gap: .3rem; }} .sidebar-link {{ display: flex; align-items: center; gap: .7rem; padding: .68rem .7rem; border-radius: .55rem; color: #aeb9cc; text-decoration: none; }} .sidebar-link:hover {{ background: #17243a; color: #fff; }} .sidebar-link.active {{ background: #1d4ed8; color: #fff; }} .sidebar-icon {{ width: 1.3rem; text-align: center; font-size: 1.1rem; }}
main {{ min-width: 0; width: min(100%, 1020px); margin: 0 auto; padding: clamp(1.5rem, 4vw, 3.25rem); }} a {{ color: #9bc5ff; }} .page-heading {{ margin: 0 0 1.8rem; }} .eyebrow {{ margin: 0 0 .45rem; color: #82a9e5; font-size: .74rem; font-weight: 750; letter-spacing: .14em; text-transform: uppercase; }} h1 {{ margin: 0; color: #f8fafc; font-size: clamp(2rem, 4vw, 2.65rem); letter-spacing: -.04em; }} .page-subtitle {{ max-width: 48rem; margin: .6rem 0 0; color: #9aa8bf; line-height: 1.65; }}
.settings-stack {{ display: grid; gap: 1.15rem; }} .settings-card {{ min-width: 0; padding: clamp(1.1rem, 2.8vw, 1.65rem); border: 1px solid #29364b; border-radius: 1rem; background: linear-gradient(145deg, rgba(22, 32, 50, .94), rgba(15, 23, 39, .97)); box-shadow: 0 18px 48px rgba(0, 0, 0, .14); }} .section-heading {{ display: flex; align-items: flex-start; justify-content: space-between; gap: 1rem; margin-bottom: 1.15rem; }} h2 {{ margin: 0; color: #f1f5fb; font-size: 1.18rem; letter-spacing: -.015em; }} h3 {{ margin: 0; color: #f1f5fb; font-size: 1.05rem; }} h4 {{ margin: 0 0 .6rem; color: #aab8cc; font-size: .77rem; letter-spacing: .07em; text-transform: uppercase; }} .note, .section-description {{ color: #98a6bb; line-height: 1.6; }} .section-description {{ max-width: 48rem; margin: .45rem 0 0; font-size: .91rem; }}
.flash-error, .flash-notice {{ margin: 0 0 1rem; padding: .85rem 1rem; border-radius: .7rem; line-height: 1.5; }} .flash-error {{ border: 1px solid #793f50; color: #ffd6dc; background: #381d2a; }} .flash-notice {{ border: 1px solid #286a52; color: #c2f5d8; background: #123426; }} .status-line {{ display: flex; align-items: center; gap: .65rem; margin: 0; color: #c4d3e8; }} .status-dot {{ width: .58rem; height: .58rem; border-radius: 50%; background: #4ade80; box-shadow: 0 0 0 .24rem rgba(74, 222, 128, .13); }} .credential-status {{ display: flex; align-items: center; gap: .65rem; margin: -.35rem 0 0; color: #9aa8bf; font-size: .82rem; line-height: 1.55; }} .credential-status.is-saved {{ color: #bdebd1; }} .action-row {{ display: flex; flex-wrap: wrap; gap: .65rem; margin-top: 1rem; }}
.settings-form, .inline-form, .library-display {{ margin: 0; }} .settings-form {{ display: grid; gap: 1rem; }} .server-name-form {{ margin-top: 1.15rem; padding-top: 1.05rem; border-top: 1px solid #26354a; }} .server-name-form .field {{ max-width: 34rem; }} .field small {{ font-size: .78rem; font-weight: 400; }} .form-grid {{ display: grid; grid-template-columns: minmax(0, 1fr) minmax(180px, .6fr); gap: 1rem; }} .field {{ display: grid; min-width: 0; gap: .45rem; }} .field > span, .field > label {{ color: #c9d4e4; font-size: .84rem; font-weight: 650; }} input:not([type="checkbox"]):not([type="radio"]), select {{ width: 100%; min-width: 0; min-height: 2.75rem; padding: .68rem .78rem; border: 1px solid #3a4961; border-radius: .6rem; outline: none; color: #f3f6fb; background: #0e1727; font: inherit; }} input::placeholder {{ color: #738198; }} input:focus, select:focus {{ border-color: #60a5fa; box-shadow: 0 0 0 3px rgba(96, 165, 250, .17); }}
button, .button-link {{ display: inline-flex; min-height: 2.65rem; align-items: center; justify-content: center; gap: .4rem; padding: .65rem .95rem; border: 1px solid transparent; border-radius: .6rem; color: #fff; background: #2563eb; font: inherit; font-size: .87rem; font-weight: 700; text-decoration: none; cursor: pointer; transition: background .15s, border-color .15s, transform .15s; }} button:hover, .button-link:hover {{ background: #3476f6; }} button:active, .button-link:active {{ transform: translateY(1px); }} button:focus-visible, a:focus-visible, summary:focus-visible {{ outline: 2px solid #93c5fd; outline-offset: 2px; }} .button-secondary {{ border-color: #3a4961; color: #d4deec; background: #182337; }} .button-secondary:hover {{ border-color: #53647e; background: #22314a; }} .button-quiet {{ min-width: 2.65rem; padding-inline: .72rem; border-color: #3a4961; color: #c7d1df; background: #182337; }}
.metadata-options {{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: .8rem; }} .metadata-option {{ display: flex; min-height: 9rem; align-items: flex-start; gap: .8rem; padding: 1rem; border: 1px solid #35445b; border-radius: .75rem; background: rgba(12, 21, 36, .68); cursor: pointer; transition: border-color .15s, background .15s; }} .metadata-option:has(input:checked) {{ border-color: #4e8cec; background: rgba(28, 65, 120, .22); }} .metadata-option input {{ flex: 0 0 auto; width: 1.05rem; height: 1.05rem; margin: .14rem 0 0; accent-color: #4389f5; }} .option-copy {{ display: grid; gap: .55rem; }} .option-copy strong {{ color: #e7edf7; }} .option-copy .note {{ margin: 0; font-size: .84rem; }} code {{ color: #c5dcff; font: .88em ui-monospace, SFMono-Regular, Menlo, monospace; }}
.library-list {{ display: grid; gap: .85rem; margin: 1.2rem 0 0; padding: 0; list-style: none; }} .library-card {{ min-width: 0; padding: 1.05rem; border: 1px solid #2c3a50; border-radius: .8rem; background: rgba(10, 17, 30, .46); }} .library-heading {{ display: flex; align-items: center; justify-content: space-between; gap: 1rem; }} .library-heading > div {{ display: flex; flex-wrap: wrap; align-items: center; gap: .65rem; }} .type-pill {{ padding: .28rem .55rem; border: 1px solid #354a68; border-radius: 999px; color: #b9cee9; background: #17243a; font-size: .72rem; font-weight: 700; }} .library-folders {{ margin-top: 1.15rem; }} .library-folders ul {{ display: grid; gap: .45rem; margin: 0; padding: 0; list-style: none; }} .library-folders li {{ min-width: 0; margin: 0; padding: .6rem .7rem; overflow-wrap: anywhere; border-radius: .48rem; background: #0d1727; }} .library-folders code {{ overflow-wrap: anywhere; }} .library-add-folder, .add-library {{ margin-top: .8rem; }} .add-library {{ margin: 0; }} .add-library .settings-form {{ margin-top: 1rem; }} summary {{ display: inline-flex; align-items: center; gap: .5rem; padding: .48rem .68rem; border: 1px solid #394a63; border-radius: .55rem; color: #bfcee2; background: #172337; font-size: .82rem; font-weight: 650; cursor: pointer; list-style: none; }} summary::before {{ content: "+"; color: #83b4ff; font-size: 1rem; }} details[open] summary::before {{ content: "−"; }} summary::-webkit-details-marker {{ display: none; }} .inline-form {{ display: flex; flex-wrap: wrap; align-items: end; gap: .55rem; margin-top: .75rem; }} .inline-form input:not([type="hidden"]) {{ flex: 1 1 18rem; }} .path-list {{ display: grid; gap: .55rem; }} .path-row {{ display: flex; flex-wrap: wrap; align-items: center; gap: .5rem; }} .path-row input {{ flex: 1 1 18rem; }} .path-row .button-link {{ flex: 0 0 auto; }} .library-tools {{ display: grid; grid-template-columns: minmax(0, auto) minmax(0, 1fr); align-items: start; gap: 1rem; margin-top: 1.1rem; padding-top: 1rem; border-top: 1px solid #26354a; }} .library-tools > .inline-form {{ margin: 0; }} .library-display fieldset {{ display: flex; flex-wrap: wrap; align-items: center; gap: .6rem 1rem; min-width: 0; margin: 0; padding: 0; border: 0; }} .library-display legend {{ width: 100%; margin-bottom: .55rem; color: #91a0b6; font-size: .72rem; font-weight: 750; letter-spacing: .09em; text-transform: uppercase; }} .library-display label {{ display: inline-flex; align-items: center; gap: .4rem; color: #c6d0df; font-size: .82rem; }} input[type="checkbox"] {{ width: 1rem; height: 1rem; margin: 0; accent-color: #4389f5; }} .library-display button {{ min-height: 2.3rem; padding: .48rem .7rem; font-size: .78rem; }} .empty-state {{ margin: 0; padding: 1rem; border: 1px dashed #35445b; border-radius: .7rem; color: #9aa8bf; text-align: center; }} .sr-only {{ position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }}
.library-card {{ padding: 0; overflow: hidden; }} .library-summary {{ display: flex; width: 100%; align-items: center; justify-content: space-between; gap: .8rem; padding: 1rem 1.05rem; border: 0; border-radius: 0; color: #e8eef8; background: transparent; font-size: .95rem; text-align: left; }} .library-summary::before {{ content: none !important; }} .library-summary::after {{ content: "⌄"; margin-left: auto; color: #91a8c7; transition: transform .15s; }} .library-card[open] > .library-summary::after {{ transform: rotate(180deg); }} .library-summary-title {{ display: flex; flex-wrap: wrap; align-items: center; gap: .65rem; }} .folder-count {{ flex: 0 0 auto; color: #91a0b6; font-size: .78rem; font-weight: 500; }} .library-content {{ padding: 0 1.05rem 1.05rem; }} .library-tools {{ grid-template-columns: minmax(0, auto) minmax(0, 1fr) auto; }} .library-delete {{ display: flex; justify-content: flex-end; }} .button-danger {{ border-color: #713841; color: #ffd3d7; background: #3a202a; }} .button-danger:hover {{ border-color: #9d4b55; background: #512630; }}
@media (max-width: 820px) {{ .library-tools {{ grid-template-columns: 1fr; }} .library-delete {{ justify-content: flex-start; }} }} @media (max-width: 700px) {{ .app-shell {{ display: block; }} .sidebar {{ position: static; height: auto; padding: .7rem; border-right: 0; border-bottom: 1px solid #263244; }} .sidebar-top {{ justify-content: flex-start; padding-bottom: .35rem; }} .sidebar-brand {{ padding: .35rem .55rem; }} .sidebar-toggle {{ display: none; }} .sidebar-label {{ display: none; }} .sidebar-collapsed .sidebar-top {{ justify-content: flex-start; }} .sidebar-collapsed .sidebar-brand {{ font-size: 1.25rem; }} .sidebar-collapsed .sidebar-link {{ justify-content: flex-start; padding-left: .7rem; padding-right: .7rem; font-size: 1rem; }} .sidebar-collapsed .sidebar-icon {{ font-size: 1.1rem; }} .sidebar-nav {{ display: flex; flex-wrap: wrap; gap: .35rem; padding-bottom: .2rem; }} .sidebar-link {{ min-height: 2.75rem; white-space: nowrap; }} .sidebar-collapsed .sidebar-settings-group[open] .sidebar-subnav {{ display: grid; }} main {{ padding: 1.35rem 1rem max(2.5rem, calc(1.35rem + env(safe-area-inset-bottom, 0px))); }} input, select, textarea {{ font-size: 16px; }} }} @media (max-width: 560px) {{ .metadata-options, .form-grid {{ grid-template-columns: 1fr; }} .metadata-option {{ min-height: auto; }} .section-heading {{ display: block; }} .library-tools .inline-form > * {{ flex: 1 1 auto; }} .library-display fieldset {{ align-items: flex-start; }} .action-row > form, .action-row button {{ width: 100%; }} .action-row > form {{ flex: 1 1 100%; }} }}
.library-scan-settings {{ display: flex; grid-column: 1 / -1; flex-wrap: wrap; align-items: center; gap: .4rem; padding-top: .9rem; border-top: 1px solid #26354a; }} .library-scan-settings > strong {{ margin-right: .25rem; }} .library-scan-settings label {{ display: flex; align-items: center; gap: .4rem; color: #c9d4e4; font-size: .84rem; }} .library-scan-settings input[type="number"] {{ width: 5.5rem; }} .library-scan-settings select {{ width: auto; }}

.library-add-folder[open] > summary::before {{ content: "+"; }}
#settings-logs-live {{ color: #8fd6a7; font-size: .82rem; }} .log-follow[hidden] {{ display: none; }}
.settings-logs .log-timestamp {{ color: #4ade80; }} .settings-logs .log-tag-source {{ color: #7dd3fc; }} .settings-logs .log-level-info {{ color: #facc15; font-weight: 700; }} .settings-logs .log-level-warn {{ color: #fb923c; font-weight: 700; }} .settings-logs .log-level-error {{ color: #f87171; font-weight: 700; }} .settings-logs .log-level-debug, .settings-logs .log-level-trace {{ color: #94a3b8; }} .settings-logs .log-key {{ color: #7dd3fc; }} .settings-logs .log-value {{ color: #c4b5fd; }}
.log-filters {{ display: flex; flex-wrap: wrap; align-items: end; gap: 1rem; margin-bottom: 1rem; }} .log-filters > label:first-child {{ display: grid; flex: 1 1 16rem; gap: .45rem; color: #c9d4e4; font-size: .84rem; font-weight: 650; }} .log-filters > label:first-child input {{ margin-top: 0; }} .log-filters > label:nth-child(2) {{ display: flex; align-items: center; gap: .45rem; min-height: 2.75rem; color: #c9d4e4; font-size: .84rem; }} #settings-logs-count {{ margin-left: auto; color: #98a6bb; font-size: .82rem; }}
.sidebar-settings-group {{ margin: 0; }} .sidebar-settings-group > summary {{ list-style: none; cursor: pointer; }} .sidebar-settings-group > summary::-webkit-details-marker {{ display: none; }} .sidebar-settings-group > summary::marker {{ content: ""; }} .sidebar-collapsed .sidebar-settings-group[open] .sidebar-subnav {{ display: none; }} .sidebar-collapsed .sidebar-chevron {{ display: none; }} .settings-logs {{ max-height: 70vh; margin: 0; padding: 1rem; overflow: auto; border: 1px solid #29364b; border-radius: .65rem; color: #d4deec; background: #0a111e; font: .8rem/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }}
{dashboard_styles_html}
.settings-section {{ padding: 0; overflow: hidden; }} .settings-section-summary {{ display: flex; width: 100%; align-items: center; justify-content: space-between; padding: 1.05rem; border: 0; border-radius: 0; background: transparent; text-align: left; }} .settings-section-summary span {{ display: grid; gap: .3rem; }} .settings-section-summary strong {{ color: #e8eef8; font-size: 1rem; }} .settings-section-summary small {{ color: #9aa8bf; font-size: .83rem; font-weight: 400; }} .settings-section-content {{ padding: 0 1.05rem 1.05rem; }} .settings-toggle {{ display: flex; align-items: center; gap: .55rem; color: #d4deec; }}
{SIDEBAR_MOBILE_CSS}</style></head><body><div class="app-shell">{}
<main class="with-global-topbar">{}{}{}
<div class="settings-stack"{}>
<section class="settings-card" aria-labelledby="plex-heading"><div class="section-heading"><div><h2 id="plex-heading">Plex account</h2><p class="section-description">Connect harbinger to Plex so compatible clients can discover and authenticate with this server.</p></div></div>{}<form method="post" action="/settings/server-name" class="settings-form server-name-form"><label class="field"><span>Server name</span><input name="server_name" type="text" value="{}" maxlength="64" required><small class="note">This is the name Plex clients see. It defaults to harbinger.</small></label><div><button class="button-secondary" type="submit">Save server name</button></div></form></section>
<section class="settings-card" aria-labelledby="metadata-heading"><div class="section-heading"><div><h2 id="metadata-heading">Metadata storage</h2><p class="section-description">Both options use the same .hbgr sidecar format. Reads prefer the selected location and fall back to the other. New metadata, artwork, and skip markers are written only to the selected location. Config mode never writes to media folders.</p></div></div><form method="post" action="/settings/preferences" class="settings-form" id="metadata-storage-form"><div class="metadata-options"><label class="metadata-option"><input type="radio" name="metadata_storage" value="config" {}><span class="option-copy"><strong>Store metadata in config folder {}</strong><span class="note">Files are stored under the server config folder.</span></span></label><label class="metadata-option"><input type="radio" name="metadata_storage" value="media" {}><span class="option-copy"><strong>Store metadata with media {}</strong><span class="note">Files are stored in each media folder’s <code>.hbgr</code> directory.</span></span></label></div><label class="field"><span>TMDB bearer token</span><input name="tmdb_bearer_token" type="password" autocomplete="new-password" placeholder="Enter a token to save or replace it"></label><input type="hidden" name="copy_existing" id="copy-existing" value="false">{}<div><button type="submit">Save metadata settings</button></div></form><dialog id="metadata-storage-dialog" aria-labelledby="metadata-storage-dialog-title"><form method="dialog"><button type="submit" aria-label="Close storage dialog" title="Close">×</button></form><h2 id="metadata-storage-dialog-title">Change metadata storage?</h2><p>Existing metadata and artwork stay in their current location unless you choose to copy them. Copying adds files to the new location and does not delete the originals or overwrite files already there.</p><div class="action-row"><button type="button" class="button-secondary" id="storage-switch-only">Switch without copying</button><button type="button" id="storage-switch-copy">Switch and copy existing data</button></div></dialog></section>
{}{}
<section class="settings-card" aria-labelledby="libraries-heading"><div class="section-heading"><div><h2 id="libraries-heading">Libraries</h2><p class="section-description">A library groups media of one type from one or more monitored folders. Add a folder to an existing library when the same collection spans multiple locations.</p></div></div><details class="add-library"><summary>Add a library</summary><form method="post" action="/settings/libraries" class="settings-form"><div class="form-grid"><label class="field"><span>Library name</span><input name="name" type="text" placeholder="e.g. Films" required></label><label class="field"><span>Content type</span><select name="kind"><option value="movie">Movies</option><option value="tv">TV Shows</option><option value="audiobooks">Audiobooks</option><option value="other">Other</option></select></label></div><div class="field"><span>Media folders</span><div class="path-list" id="new-library-paths"><div class="path-row"><label class="sr-only" for="new-path-0">Folder path</label><input id="new-path-0" name="path[]" type="text" placeholder="/path/to/folder" value="{}" required><a class="button-link button-secondary" href="/settings/browse">Browse</a><button type="button" class="button-quiet remove-path" aria-label="Remove folder path" title="Remove folder path" disabled>−</button></div></div></div><div class="action-row path-actions"><button type="button" class="button-secondary" id="add-path" aria-label="Add another folder path" title="Add another folder path">＋ Add folder</button><button type="submit">Create library</button></div></form></details>
<div class="library-list">{}</div></section>
</div>{}{dashboard_content_html}
{users_content_html} </main></div><script>
const shell = document.querySelector('.app-shell'); const toggle = document.querySelector('.sidebar-toggle');
if (localStorage.getItem('mplex-sidebar-collapsed') === 'true') shell.classList.add('sidebar-collapsed');
toggle?.addEventListener('click', () => {{ const collapsed = shell.classList.toggle('sidebar-collapsed'); localStorage.setItem('mplex-sidebar-collapsed', collapsed ? 'true' : 'false'); }});
const pathList = document.querySelector('#new-library-paths'); const updatePathButtons = () => {{ const rows = [...pathList.querySelectorAll('.path-row')]; rows.forEach((row, index) => {{ const button = row.querySelector('.remove-path'); button.disabled = rows.length === 1; button.setAttribute('aria-label', `Remove folder path ${{index + 1}}`); button.title = `Remove folder path ${{index + 1}}`; }}); }};
document.querySelector('#add-path')?.addEventListener('click', () => {{ const index = pathList.querySelectorAll('.path-row').length; const row = document.createElement('div'); row.className = 'path-row'; row.innerHTML = `<label class="sr-only" for="new-path-${{index}}">Folder path</label><input id="new-path-${{index}}" name="path[]" type="text" placeholder="/path/to/folder" required><a class="button-link button-secondary" href="/settings/browse">Browse</a><button type="button" class="button-quiet remove-path">−</button>`; row.querySelector('.remove-path').addEventListener('click', () => {{ row.remove(); updatePathButtons(); }}); pathList.appendChild(row); updatePathButtons(); }});
const logOutput = document.querySelector('#settings-logs-output');
const storageForm = document.querySelector('#metadata-storage-form'); const storageDialog = document.querySelector('#metadata-storage-dialog'); const copyExisting = document.querySelector('#copy-existing'); let storageConfirmed = false;
storageDialog.style.cssText = 'position:fixed;inset:0;width:min(34rem,calc(100vw - 2rem));max-height:calc(100vh - 2rem);margin:auto;padding:1.5rem;color:#f1f5fb;border:1px solid #394a63;border-radius:.85rem;background:#101a2b'; storageDialog.querySelector('form').style.cssText = 'position:absolute;top:.55rem;right:.55rem;margin:0'; storageDialog.querySelector('form button').style.cssText = 'min-height:2.2rem;padding:.3rem .55rem';
storageForm?.addEventListener('submit', event => {{ const selected = storageForm.querySelector('input[name="metadata_storage"]:checked')?.value; const current = storageForm.querySelector('input[name="metadata_storage"][value="config"]')?.parentElement?.querySelector('strong')?.textContent?.includes('(current)') ? 'config' : storageForm.querySelector('input[name="metadata_storage"][value="media"]')?.parentElement?.querySelector('strong')?.textContent?.includes('(current)') ? 'media' : null; if (!storageConfirmed && selected && current && selected !== current) {{ event.preventDefault(); storageDialog.showModal(); }} }});
const confirmStorageChange = copy => {{ copyExisting.value = copy ? 'true' : 'false'; storageConfirmed = true; storageDialog.close(); storageForm.requestSubmit(); }};
document.querySelector('#storage-switch-only')?.addEventListener('click', () => confirmStorageChange(false)); document.querySelector('#storage-switch-copy')?.addEventListener('click', () => confirmStorageChange(true));
const logSearch = document.querySelector('#settings-logs-search');
const logReverse = document.querySelector('#settings-logs-reverse');
const logCount = document.querySelector('#settings-logs-count');
const logEmpty = document.querySelector('#settings-logs-empty');
const logFollowButton = document.querySelector('#settings-logs-follow');
const logCopyButton = document.querySelector('#settings-logs-copy');
const logCopyStatus = document.querySelector('#settings-logs-copy-status');
const logLiveStatus = document.querySelector('#settings-logs-live');
if (logOutput && logSearch && logReverse && logCount && logEmpty && logFollowButton && logCopyButton && logCopyStatus && logLiveStatus) {{
  let logLines = logOutput.textContent ? logOutput.textContent.split("\n") : [];
  let visibleLogLines = [];
  let followLogs = true;
  let lastSequence = 0;
  const isAtBottom = () => logOutput.scrollHeight - logOutput.scrollTop - logOutput.clientHeight <= 4;
  const scrollToLatest = () => {{ logOutput.scrollTop = logOutput.scrollHeight; }};
  const updateFollowButton = () => {{ logFollowButton.hidden = isAtBottom(); }};
  const logTimestampPattern = /^(ts="[^"]*")(\s+)level=(TRACE|DEBUG|INFO|WARN|ERROR)(\s+)tag=([^\s]+)(\s*)(.*)$/i;
  const appendLogSpan = (parent, className, text) => {{
    const span = document.createElement('span');
    span.className = className;
    span.textContent = text;
    parent.append(span);
  }};
  const appendLogMessage = (parent, message) => {{
    const keyValuePattern = /(^|\s)([A-Za-z_][A-Za-z0-9_.-]*)=("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|[^\s]+)/g;
    let offset = 0;
    let match;
    while ((match = keyValuePattern.exec(message)) !== null) {{
      const keyStart = match.index + match[1].length;
      parent.append(document.createTextNode(message.slice(offset, keyStart)));
      appendLogSpan(parent, 'log-key', `${{match[2]}}=`);
      appendLogSpan(parent, 'log-value', match[3]);
      offset = keyValuePattern.lastIndex;
    }}
    parent.append(document.createTextNode(message.slice(offset)));
  }};
  const renderLogLines = lines => {{
    logOutput.replaceChildren();
    lines.forEach((line, index) => {{
      if (index > 0) logOutput.append(document.createTextNode('\n'));
      const parts = line.match(logTimestampPattern);
      if (!parts) {{
        logOutput.append(document.createTextNode(line));
        return;
      }}
      const styledLine = document.createElement('span');
      appendLogSpan(styledLine, 'log-timestamp', parts[1]);
      styledLine.append(document.createTextNode(parts[2]));
      appendLogSpan(styledLine, 'log-key', 'level=');
      appendLogSpan(styledLine, 'log-level-' + parts[3].toLowerCase(), parts[3]);
      styledLine.append(document.createTextNode(parts[4]));
      appendLogSpan(styledLine, 'log-key', 'tag=');
      appendLogSpan(styledLine, 'log-tag-source', parts[5]);
      if (parts[6]) styledLine.append(document.createTextNode(parts[6]));
      else if (parts[7]) styledLine.append(document.createTextNode(' '));
      appendLogMessage(styledLine, parts[7]);
      logOutput.append(styledLine);
    }});
  }};
  const filterLogs = () => {{
    const query = logSearch.value.trim().toLocaleLowerCase();
    const reverse = logReverse.checked && query !== "";
    visibleLogLines = query === "" ? logLines : logLines.filter(line => line.toLocaleLowerCase().includes(query) !== reverse);
    renderLogLines(visibleLogLines);
    logEmpty.hidden = logLines.length !== 0;
    logCount.textContent = `${{visibleLogLines.length}} of ${{logLines.length}} lines`;
    if (followLogs) scrollToLatest();
    updateFollowButton();
  }};
  logOutput.addEventListener('scroll', () => {{
    followLogs = isAtBottom();
    updateFollowButton();
  }});
  logFollowButton.addEventListener('click', () => {{
    followLogs = true;
    scrollToLatest();
    updateFollowButton();
  }});
  logSearch.addEventListener('input', filterLogs);
  logReverse.addEventListener('change', filterLogs);
  const copyLogText = async text => {{
    if (navigator.clipboard?.writeText) {{
      try {{
        await navigator.clipboard.writeText(text);
        return;
      }} catch {{}}
    }}
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.setAttribute('readonly', '');
    textarea.style.position = 'fixed';
    textarea.style.opacity = '0';
    document.body.append(textarea);
    textarea.select();
    let copied = false;
    try {{ copied = document.execCommand('copy'); }} finally {{ textarea.remove(); }}
    if (!copied) throw new Error('Clipboard copy failed');
  }};
  logCopyButton.addEventListener('click', async () => {{
    const lines = visibleLogLines.slice();
    logCopyButton.disabled = true;
    try {{
      await copyLogText(lines.join("\n"));
      logCopyStatus.textContent = `Copied ${{lines.length}} visible log lines.`;
    }} catch {{
      logCopyStatus.textContent = 'Could not copy the visible log lines.';
    }} finally {{
      logCopyButton.disabled = false;
    }}
  }});

  const connectLogs = () => {{
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const socket = new WebSocket(`${{protocol}}//${{location.host}}/settings/logs/socket`);
    socket.addEventListener('open', () => {{ logLiveStatus.textContent = 'Live'; }});
    socket.addEventListener('message', event => {{
      let update;
      try {{ update = JSON.parse(event.data); }} catch {{ return; }}
      if (update.type === 'snapshot' && Array.isArray(update.lines)) {{
        logLines = update.lines;
        lastSequence = update.sequence;
      }} else if (update.type === 'cleared') {{
        logLines = [];
        lastSequence = update.sequence;
      }} else if (update.type === 'entry' && update.sequence > lastSequence) {{
        logLines.push(update.line);
        if (logLines.length > 1000) logLines.shift();
        lastSequence = update.sequence;
      }} else {{
        return;
      }}
      filterLogs();
    }});
    socket.addEventListener('close', () => {{
      logLiveStatus.textContent = 'Reconnecting…';
      window.setTimeout(connectLogs, 1500);
    }});
    socket.addEventListener('error', () => socket.close());
  }};
  filterLogs();
  connectLogs();
}}
</script>{dashboard_script_html}</body></html>"#,
        sidebar_nav_for_settings(&state, query.tab.as_deref()),
        format!("{}{}", global_search_markup(), global_search_top_style()),
        error,
        notice,
        settings_stack_style,
        format!("<div class=\"section-content\">{}</div>", plex_auth),
        current_server_name,
        if config_selected { "checked" } else { "" },
        if config_selected { "(current)" } else { "" },
        if config_selected { "" } else { "checked" },
        if config_selected { "" } else { "(current)" },
        tmdb_token_status,
        subtitle_settings,
        skip_settings,
        html(query.folder.as_deref().unwrap_or("")),
        if list.is_empty() { "<p class=\"empty-state\">No libraries yet. Add a library to start organizing your media.</p>".to_owned() } else { list },
        logs_content
    )))
    .into_response()
}

pub(crate) async fn update_preferences(
    State(state): State<AppState>,
    Form(form): Form<PreferencesForm>,
) -> Response {
    let requested_token = form
        .tmdb_bearer_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    if let Some(token) = requested_token.as_deref() {
        let Ok(response) = reqwest::Client::new()
            .get("https://api.themoviedb.org/3/configuration")
            .bearer_auth(token)
            .send()
            .await
        else {
            return settings_error("TMDB could not be reached; token was not saved");
        };
        if !response.status().is_success() {
            return settings_error("TMDB rejected this bearer token; token was not saved");
        }
    }
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let token = requested_token.or_else(|| config.tmdb_bearer_token.clone());
    let previous_storage = config.metadata_storage.clone();
    let requested_storage = match form.metadata_storage.as_deref() {
        Some("media") => Some("media"),
        Some("config") => Some("config"),
        _ => None,
    };
    if let Some(storage) = requested_storage {
        config.metadata_storage = storage.to_owned();
    }
    config.tmdb_bearer_token = token;
    let new_storage = config.metadata_storage.clone();
    let token_saved = config.tmdb_bearer_token.is_some();
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    drop(config);
    let copied = if previous_storage != new_storage && form.copy_existing == Some(true) {
        match copy_metadata_storage(&state, &previous_storage, &new_storage) {
            Ok(count) => Some(count),
            Err(error) => {
                warn!(%error, "metadata storage copy failed");
                return settings_error(
                    "Storage changed, but some existing metadata could not be copied",
                );
            }
        }
    } else {
        None
    };
    let location = match copied {
        Some(count) => format!("/settings?metadata_status=Copied%20{count}%20metadata%20items"),
        None if new_storage != previous_storage => {
            "/settings?metadata_status=Storage%20location%20changed".to_owned()
        }
        None if token_saved => "/settings?metadata_status=TMDB%20token%20accepted".to_owned(),
        _ => "/settings?metadata_status=TMDB%20token%20cleared".to_owned(),
    };
    Redirect::to(&location).into_response()
}

pub(crate) async fn update_server_name(
    State(state): State<AppState>,
    Form(form): Form<ServerNameForm>,
) -> Response {
    let name = form.server_name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return settings_error(
            "Server name must be 1–64 characters and contain no control characters",
        );
    }
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection
        .execute(
            "UPDATE server_identity SET server_name = ?1 WHERE id = 1",
            [name],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Ok(mut current_name) = state.server_name.write() {
        *current_name = name.to_owned();
    }
    Redirect::to("/settings?server_name_saved=true").into_response()
}

#[derive(Deserialize)]
pub(crate) struct BookMatchQuery {
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) path: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) provider: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BookMatchSelection {
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) path: String,
    pub(crate) provider: Option<String>,
    pub(crate) id: Option<String>,
}

async fn audiobook_item_for_match(
    state: &AppState,
    library_index: usize,
    folder: Option<usize>,
    relative: &std::path::Path,
) -> Option<crate::audiobookshelf::catalog::Item> {
    let root = selected_library_folder_root(state, Some(library_index), folder);
    let requested = web_path(state, &root, relative).await.ok()?;
    crate::audiobookshelf::catalog::Catalog::load_async(state)
        .await
        .find_item_path(library_index, relative, &root, &requested)
        .cloned()
}

pub(crate) async fn book_match_search(
    State(state): State<AppState>,
    Query(query): Query<BookMatchQuery>,
) -> Response {
    let Some(library_index) = query.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(item) = audiobook_item_for_match(&state, library_index, query.folder, &relative).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let title = query
        .query
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(item.title.as_str())
        .to_owned();
    let stored_author = item.metadata.authors.join(", ");
    let mut author = query
        .author
        .as_deref()
        .map(str::trim)
        .filter(|author| !author.is_empty() && !author.eq_ignore_ascii_case("unknown"))
        .map(str::to_owned)
        .unwrap_or_else(|| stored_author.clone());
    if title != item.title
        && !stored_author.is_empty()
        && (author == stored_author || author.eq_ignore_ascii_case("unknown"))
    {
        author.clear();
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("harbinger/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let provider = query
        .provider
        .as_deref()
        .and_then(crate::audiobookshelf::providers::Provider::from_key)
        .unwrap_or(crate::audiobookshelf::providers::Provider::Audible);
    let results =
        crate::audiobookshelf::providers::search(provider, &client, &title, &author).await;
    info!(
        provider = provider.key(),
        title = %title,
        author = %author,
        result_count = results.len(),
        "Book match search finished"
    );
    Json(serde_json::json!({ "results": results })).into_response()
}

pub(crate) async fn book_match_select(
    State(state): State<AppState>,
    Json(selection): Json<BookMatchSelection>,
) -> Response {
    let Some(library_index) = selection.library else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(relative) = clean_relative_path(&selection.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let provider = selection
        .provider
        .as_deref()
        .and_then(crate::audiobookshelf::providers::Provider::from_key)
        .unwrap_or(crate::audiobookshelf::providers::Provider::Audible);
    let Some(provider_id) = selection
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "A provider result id is required." })),
        )
            .into_response();
    };
    if provider == crate::audiobookshelf::providers::Provider::Audible
        && (provider_id.len() != 10
            || !provider_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric()))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "A valid Audible ASIN is required." })),
        )
            .into_response();
    }
    let Some(item) =
        audiobook_item_for_match(&state, library_index, selection.folder, &relative).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("harbinger/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(book) = crate::audiobookshelf::providers::lookup(provider, &client, provider_id).await
    else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "The provider could not load that book." })),
        )
            .into_response();
    };
    if !crate::audiobookshelf::matching::persist_match(
        &client,
        &state,
        library_index,
        &item,
        &book,
        true,
    )
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Could not save the matched metadata." })),
        )
            .into_response();
    }
    crate::audiobookshelf::catalog::invalidate_cache();
    info!(
        library = library_index,
        title = %book.title,
        provider = provider.key(),
        provider_id = %book.provider_id,
        "Audiobook match selected"
    );
    Json(serde_json::json!({
        "success": true,
        "title": book.title,
        "provider": provider.key(),
        "id": book.provider_id
    }))
    .into_response()
}

pub(crate) async fn tv_match_search(
    State(state): State<AppState>,
    Query(query): Query<TvMatchSearchQuery>,
) -> Response {
    let (library, _, _, title, _) = match tv_match_target(&state, query.library, &query.path) {
        Ok(target) => target,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": error})),
            )
                .into_response()
        }
    };
    let search = query.query.trim();
    if search.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Enter a search title"})),
        )
            .into_response();
    }
    let token = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.tmdb_bearer_token.clone())
        .filter(|token| !token.trim().is_empty());
    let Some(token) = token else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Add a TMDB bearer token in Settings first"})),
        )
            .into_response();
    };
    let media_kind = if library.kind == "tv" { "tv" } else { "movie" };
    info!(library = %library.name, title = %title, query = search, "metadata fix-match search started");
    let response = match reqwest::Client::new()
        .get(format!("https://api.themoviedb.org/3/search/{media_kind}"))
        .bearer_auth(token)
        .query(&[("query", search), ("language", "en-US")])
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            warn!(library = %library.name, show = %title, error = %error, "TV fix-match search request failed");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "TMDB search request failed"})),
            )
                .into_response();
        }
    };
    let status = response.status();
    if !status.is_success() {
        warn!(library = %library.name, show = %title, status = status.as_u16(), "TV fix-match search returned an error status");
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("TMDB returned {status}")})),
        )
            .into_response();
    }
    let payload = match response.json::<serde_json::Value>().await {
        Ok(payload) => payload,
        Err(error) => {
            warn!(library = %library.name, show = %title, error = %error, "TV fix-match search response was invalid");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "TMDB search response was invalid"})),
            )
                .into_response();
        }
    };
    let results = payload
        .get("results")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let id = result.get("id")?.as_i64()?;
            Some(serde_json::json!({
                "id": id,
                "title": result.get(if media_kind == "tv" { "name" } else { "title" }).and_then(serde_json::Value::as_str).unwrap_or("Untitled"),
                "first_air_date": result.get(if media_kind == "tv" { "first_air_date" } else { "release_date" }).and_then(serde_json::Value::as_str).unwrap_or(""),
                "overview": result.get("overview").and_then(serde_json::Value::as_str).unwrap_or(""),
                "poster_path": result.get("poster_path").and_then(serde_json::Value::as_str),
            }))
        })
        .take(20)
        .collect::<Vec<_>>();
    info!(library = %library.name, show = %title, result_count = results.len(), "TV fix-match search finished");
    Json(serde_json::json!({"results": results})).into_response()
}

pub(crate) async fn tv_match_select(
    State(state): State<AppState>,
    Json(selection): Json<TvMatchSelection>,
) -> Response {
    let (library, root, media_directory, title, media_items) =
        match tv_match_target(&state, selection.library, &selection.path) {
            Ok(target) => target,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": error})),
                )
                    .into_response();
            }
        };
    if selection.tmdb_id <= 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid TMDB result"})),
        )
            .into_response();
    }
    let config = match state.config.lock() {
        Ok(config) => config.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(token) = config
        .tmdb_bearer_token
        .filter(|token| !token.trim().is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Add a TMDB bearer token in Settings first"})),
        )
            .into_response();
    };
    let media_kind = if library.kind == "tv" { "tv" } else { "movie" };
    info!(library = %library.name, title = %title, tmdb_id = selection.tmdb_id, "metadata fix-match selection started");
    let client = reqwest::Client::new();
    let response = match client
        .get(format!(
            "https://api.themoviedb.org/3/{media_kind}/{}",
            selection.tmdb_id
        ))
        .bearer_auth(&token)
        .query(&[("language", "en-US")])
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            warn!(library = %library.name, show = %title, error = %error, "TV fix-match details request failed");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "TMDB details request failed"})),
            )
                .into_response();
        }
    };
    let status = response.status();
    if !status.is_success() {
        warn!(library = %library.name, show = %title, status = status.as_u16(), "TV fix-match details returned an error status");
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("TMDB returned {status}")})),
        )
            .into_response();
    }
    let metadata = match response.json::<serde_json::Value>().await {
        Ok(metadata)
            if metadata.get("id").and_then(serde_json::Value::as_i64)
                == Some(selection.tmdb_id) =>
        {
            metadata
        }
        Ok(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "TMDB returned a different show"})),
            )
                .into_response()
        }
        Err(error) => {
            warn!(library = %library.name, show = %title, error = %error, "TV fix-match details response was invalid");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "TMDB details response was invalid"})),
            )
                .into_response();
        }
    };
    if config.metadata_storage == "media" {
        if let Err(error) =
            store_metadata_sidecar(&root, &media_directory, &title, &media_items[0], &metadata)
                .await
        {
            warn!(library = %library.name, title = %title, error = %error, "Fix-match metadata could not be saved");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    } else {
        for media in &media_items {
            let media_key = database_media_key(selection.library + 1, &media.id);
            let item_metadata = if media_kind == "tv" {
                tv_metadata_for_media(&metadata, &root, &media.path, "tv")
            } else {
                metadata.clone()
            };
            if !write_config_metadata(&state, &media_key, &item_metadata) {
                warn!(library = %library.name, title = %title, media = %media.path.display(), "Fix-match metadata could not be saved");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }
    if let Err(error) = download_tmdb_artwork(
        &client,
        &token,
        &root,
        &media_directory,
        &database_media_key(selection.library + 1, &media_items[0].id),
        &metadata,
        &state,
        true,
    )
    .await
    {
        warn!(library = %library.name, title = %title, error = %error, "Fix-match artwork download failed");
    }
    if media_kind == "tv" {
        if let Err(error) = ensure_tv_season_metadata(
            &client,
            &token,
            &root,
            &media_items,
            selection.library + 1,
            &metadata,
            config.metadata_storage == "media",
            &state,
            true,
        )
        .await
        {
            warn!(library = %library.name, title = %title, error = %error, "TV season metadata update failed");
        }
    }
    info!(library = %library.name, title = %title, tmdb_id = selection.tmdb_id, "metadata fix-match selection saved");
    Json(serde_json::json!({"ok": true})).into_response()
}

pub(crate) async fn scan_media_metadata(
    State(state): State<AppState>,
    Form(form): Form<MediaScanForm>,
) -> Response {
    let library_id = form.library.unwrap_or(0);
    let Ok(config) = state.config.lock().map(|config| config.clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get(library_id) else {
        return settings_error("Library does not exist");
    };
    if !is_metadata_library(&library.kind) {
        return settings_error("TMDB matching is not available for this library type");
    }
    let sidecar_mode = config.metadata_storage == "media";
    let Some(token) = config
        .tmdb_bearer_token
        .as_deref()
        .filter(|token| !token.is_empty())
    else {
        return settings_error("Add a TMDB bearer token in Settings before matching media");
    };
    let Some(relative) = clean_relative_path(form.path.as_str()) else {
        return settings_error("Media path is invalid");
    };
    let Some(root) = library.folders.first() else {
        return settings_error("Library has no folders");
    };
    let path = match web_path(&state, root, &relative).await {
        Ok(path) if path.is_file() && media_type(&path).is_some() => path,
        _ => return settings_error("Media file does not exist"),
    };
    let Ok(files) = scan_media(root).await else {
        return settings_error("Library could not be scanned");
    };
    let Some(media) = files.into_iter().find(|media| media.path == path) else {
        return settings_error("Media file is not in the selected library");
    };
    let kind = if library.kind == "tv" { "tv" } else { "movie" };
    let title = metadata_search_title(form.title.trim());
    if title.is_empty() {
        return settings_error("A match title is required");
    }
    let client = reqwest::Client::new();
    let Ok(response) = client
        .get(format!("https://api.themoviedb.org/3/search/{kind}"))
        .bearer_auth(token)
        .query(&[
            ("query", title.as_str()),
            ("include_adult", "false"),
            ("language", "en-US"),
        ])
        .send()
        .await
    else {
        return settings_error("TMDB could not be reached");
    };
    let Ok(payload) = response.json::<serde_json::Value>().await else {
        return settings_error("TMDB returned an invalid response");
    };
    let Some(result) = payload
        .get("results")
        .and_then(|results| results.as_array())
        .and_then(|results| results.first())
    else {
        return settings_error("No metadata match was found");
    };
    let media_key = database_media_key(library_id + 1, &media.id);
    let group_path = metadata_group_path(root, &media.path, kind);
    if !sidecar_mode && !write_config_metadata(&state, &media_key, result) {
        return settings_error("Could not write matched metadata to the config-folder sidecar");
    }
    if sidecar_mode
        && store_metadata_sidecar(root, &group_path, &title, &media, result)
            .await
            .is_err()
    {
        return settings_error("Could not write matched metadata to the media-folder sidecar");
    }
    let _ = download_tmdb_artwork(
        &client,
        token,
        root,
        &group_path,
        &media_key,
        result,
        &state,
        true,
    )
    .await;
    let parent = relative.parent().unwrap_or(std::path::Path::new(""));
    Redirect::to(&format!(
        "/?library={library_id}&path={}",
        url_encode(&parent.to_string_lossy())
    ))
    .into_response()
}

pub(crate) fn settings_error(message: &str) -> Response {
    let location = format!("/settings?error={}", url_encode(message));
    Redirect::to(&location).into_response()
}

pub(crate) fn is_async_settings_action(headers: &HeaderMap) -> bool {
    headers
        .get("X-Mplex-Async")
        .and_then(|value| value.to_str().ok())
        == Some("1")
}

pub(crate) fn settings_action_error(
    headers: &HeaderMap,
    status: StatusCode,
    message: &str,
) -> Response {
    if is_async_settings_action(headers) {
        (status, message.to_owned()).into_response()
    } else {
        settings_error(message)
    }
}

pub(crate) fn settings_action_response(headers: &HeaderMap, redirect: &str) -> Response {
    if is_async_settings_action(headers) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        Redirect::to(redirect).into_response()
    }
}

pub(crate) fn library_form_error(message: &str) -> Response {
    Redirect::to(&format!(
        "/settings?add_library=true&error={}",
        url_encode(message)
    ))
    .into_response()
}

pub(crate) async fn settings_browse(Query(query): Query<BrowseQuery>) -> Response {
    let at_root = query.path.is_none();
    let directory = query
        .path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
    let Ok(metadata) = tokio::fs::metadata(&directory).await else {
        return (StatusCode::NOT_FOUND, "Directory does not exist").into_response();
    };
    if !metadata.is_dir() {
        return (StatusCode::BAD_REQUEST, "Path is not a directory").into_response();
    }
    let mut entries = match tokio::fs::read_dir(&directory).await {
        Ok(entries) => entries,
        Err(_) => return StatusCode::FORBIDDEN.into_response(),
    };
    let mut folders = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if entry.file_type().await.is_ok_and(|kind| kind.is_dir()) {
            folders.push((name, path));
        }
    }
    folders.sort_by_key(|(name, _)| name.to_ascii_lowercase());
    let parent = directory.parent().unwrap_or(&directory);
    let items = folders
        .iter()
        .map(|(name, path)| {
            let encoded = url_encode(&path.to_string_lossy());
            format!(
                "<li class=\"folder\"><span class=\"folder-name\">📁 {}</span><span><a class=\"open\" href=\"/settings/browse?path={}\">Open</a><a class=\"choose\" href=\"/settings?folder={}\">Choose</a></span></li>",
                html(name), encoded, encoded
            )
        })
        .collect::<String>();
    let shortcuts = if at_root {
        let candidates = [
            std::env::current_dir().ok(),
            Some(PathBuf::from("/Users")),
            Some(PathBuf::from("/Volumes")),
            Some(PathBuf::from("/home")),
            Some(PathBuf::from("/mnt")),
            Some(PathBuf::from("/media")),
        ];
        candidates
            .into_iter()
            .flatten()
            .filter(|path| path.is_dir())
            .map(|path| {
                format!(
                    "<a class=\"shortcut\" href=\"/settings/browse?path={}\">📂 {}</a>",
                    url_encode(&path.to_string_lossy()),
                    html(&path.to_string_lossy())
                )
            })
            .collect::<String>()
    } else {
        String::new()
    };
    let page = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="icon" href="/favicon.ico" sizes="any"><title>Choose folder · harbinger</title><style>body {{ margin: 0; padding: 2rem; color: #f9fafb; background: #111827; font-family: system-ui, sans-serif; }} main {{ max-width: 820px; margin: auto; }} a {{ color: #93c5fd; }} .toolbar {{ display: flex; flex-wrap: wrap; gap: .6rem; margin: 1.2rem 0; }} .shortcut, .action {{ display: inline-block; padding: .6rem .8rem; border: 1px solid #374151; border-radius: .5rem; text-decoration: none; background: #1f2937; }} .folder-list {{ padding: 0; list-style: none; border: 1px solid #374151; border-radius: .6rem; overflow: hidden; }} .folder {{ display: flex; justify-content: space-between; gap: 1rem; padding: .8rem 1rem; border-bottom: 1px solid #374151; }} .folder:last-child {{ border-bottom: 0; }} .open {{ margin-right: 1rem; }} code, .note {{ color: #9ca3af; overflow-wrap: anywhere; }} </style></head><body><main><p><a href="/settings">← Back to Settings</a></p><h1>Choose a media folder</h1><p class="note">Pick a folder on the harbinger server. You can add more folders to the same library after this.</p><p><strong>Current folder:</strong> <code>{}</code></p><div class="toolbar"><a class="action" href="/settings/browse?path={}">⬆ Parent</a><a class="action" href="/settings?folder={}">Choose this folder</a></div>{}<ul class="folder-list">{}</ul></main></body></html>"#,
        html(&directory.to_string_lossy()),
        url_encode(&parent.to_string_lossy()),
        url_encode(&directory.to_string_lossy()),
        shortcuts,
        items
    );
    let page = page.replace(
        "</body>",
        "<script>document.addEventListener('click',event=>{const link=event.target.closest?.('a.choose, .toolbar a.action[href*=\\\"/settings?folder=\\\"]');if(!link||window.parent===window)return;event.preventDefault();const path=new URL(link.href,location.origin).searchParams.get('folder');window.parent.postMessage({type:'mplex-folder-selected',path},location.origin);});</script></body>",
    );
    Html(add_build_info_footer(page)).into_response()
}

pub(crate) async fn create_library(
    State(state): State<AppState>,
    RawForm(raw): RawForm,
) -> Response {
    let fields = match serde_urlencoded::from_bytes::<Vec<(String, String)>>(&raw) {
        Ok(fields) => fields,
        Err(_) => return library_form_error("Invalid library form"),
    };
    let mut name = String::new();
    let mut kind = String::new();
    let mut paths = Vec::new();
    for (key, value) in fields {
        match key.as_str() {
            "name" => name = value,
            "kind" => kind = value,
            "path" | "path[]" => paths.push(value),
            _ => {}
        }
    }
    let kind = match kind.as_str() {
        "movie" | "tv" | "audiobooks" | "other" => kind,
        _ => return library_form_error("Invalid library type"),
    };
    let name = name.trim();
    if name.is_empty() {
        return library_form_error("Library name is required");
    }
    if paths.is_empty() {
        return library_form_error("At least one folder is required");
    }
    let mut canonical_paths = Vec::with_capacity(paths.len());
    for value in paths
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        let Ok(path_metadata) = tokio::fs::metadata(value).await else {
            return library_form_error(&format!("Directory does not exist: {}", value));
        };
        if !path_metadata.is_dir() {
            return library_form_error(&format!("Not a directory: {}", value));
        }
        let Ok(path) = tokio::fs::canonicalize(value).await else {
            return library_form_error(&format!("Directory could not be resolved: {}", value));
        };
        if !canonical_paths.contains(&path) {
            canonical_paths.push(path);
        }
    }
    if canonical_paths.is_empty() {
        return library_form_error("At least one folder is required");
    }
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if config.libraries.iter().any(|library| library.name == name) {
        return library_form_error("A library with that name already exists");
    }
    config.libraries.push(ConfigLibrary {
        name: name.to_owned(),
        kind,
        folders: canonical_paths,
        show_recently_added: true,
        show_continue_watching: true,
        scan_enabled: true,
        scan_interval_value: default_scan_interval_value(),
        scan_interval_unit: default_scan_interval_unit(),
        plex_agent: None,
        plex_scanner: None,
        plex_language: None,
        metadata_agent_provider_group_id: None,
        plex_preferences: None,
    });
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if sync_config_libraries(&connection, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to(&format!(
        "/settings?library_created=true&add_library=true&library_open={}",
        config.libraries.len() - 1
    ))
    .into_response()
}

pub(crate) async fn add_library_folder(
    State(state): State<AppState>,
    Form(form): Form<LibraryFolderForm>,
) -> Response {
    let requested = PathBuf::from(form.path.trim());
    let Ok(metadata) = tokio::fs::metadata(&requested).await else {
        return settings_error("Directory does not exist");
    };
    if !metadata.is_dir() {
        return settings_error("Path is not a directory");
    }
    let Ok(path) = tokio::fs::canonicalize(requested).await else {
        return settings_error("Directory could not be resolved");
    };
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get_mut(form.library_id as usize) else {
        return settings_error("Library does not exist");
    };
    if library.folders.iter().any(|folder| folder == &path) {
        return Redirect::to(&format!("/settings?library_open={}", form.library_id))
            .into_response();
    }
    library.folders.push(path);
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if sync_config_libraries(&connection, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to(&format!("/settings?library_open={}", form.library_id)).into_response()
}

pub(crate) async fn remove_library_folder(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RemoveLibraryFolderForm>,
) -> Response {
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get(form.library_id) else {
        return settings_action_error(&headers, StatusCode::NOT_FOUND, "Library does not exist");
    };
    let Some(folder) = library.folders.get(form.folder_index) else {
        return settings_action_error(&headers, StatusCode::NOT_FOUND, "Folder does not exist");
    };
    let path = folder.to_string_lossy().into_owned();
    if path != form.path {
        return settings_action_error(
            &headers,
            StatusCode::CONFLICT,
            "Folder list changed. Reload and try again",
        );
    }
    let library_name = library.name.clone();
    let mut updated_config = config.clone();
    updated_config.libraries[form.library_id]
        .folders
        .remove(form.folder_index);

    let Ok(mut connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(transaction) = connection.transaction() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if transaction
        .execute(
            "DELETE FROM library_folders WHERE library_id = (SELECT id FROM library_groups WHERE name = ?1) AND path = ?2",
            (&library_name, &path),
        )
        .is_err()
        || transaction
            .execute(
                "UPDATE library_groups SET updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?1",
                [&library_name],
            )
            .is_err()
        || update_library_catalog_timestamp(&transaction).is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if save_config(&state.config_path, &updated_config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if transaction.commit().is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    *config = updated_config;
    settings_action_response(
        &headers,
        &format!("/settings?library_open={}", form.library_id),
    )
}

pub(crate) async fn rename_library(
    State(state): State<AppState>,
    Path(library_id): Path<usize>,
    RawForm(raw): RawForm,
) -> Response {
    let form = match serde_urlencoded::from_bytes::<RenameLibraryForm>(&raw) {
        Ok(form) => form,
        Err(_) => return settings_error("Invalid library name form"),
    };
    let name = form.name.trim();
    if name.is_empty() {
        return settings_error("Library name is required");
    }
    if name.chars().count() > 64 {
        return settings_error("Library names must be 64 characters or fewer");
    }
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if library_id >= config.libraries.len() {
        return settings_error("Library does not exist");
    }
    let old_name = config.libraries[library_id].name.clone();
    if config
        .libraries
        .iter()
        .enumerate()
        .any(|(index, library)| index != library_id && library.name.eq_ignore_ascii_case(name))
    {
        return settings_error("A library with that name already exists");
    }
    config.libraries[library_id].name = name.to_owned();
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let updated = connection.execute(
        "UPDATE library_groups SET name = ?1, updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?2",
        (&name, &old_name),
    );
    if updated.is_err() || update_library_catalog_timestamp(&connection).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to("/settings?library_renamed=true").into_response()
}

pub(crate) async fn save_library_settings(
    State(state): State<AppState>,
    Path(library_id): Path<usize>,
    Form(form): Form<LibrarySettingsForm>,
) -> Response {
    let name = form.name.trim();
    if name.is_empty() {
        return settings_error("Library name is required");
    }
    if name.chars().count() > 64 {
        return settings_error("Library names must be 64 characters or fewer");
    }
    if form.scan_interval_value == 0
        || !matches!(
            form.scan_interval_unit.as_str(),
            "seconds" | "minutes" | "hours"
        )
    {
        return settings_error("Scan intervals need a positive integer and a valid time unit");
    }
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if library_id >= config.libraries.len() {
        return settings_error("Library does not exist");
    }
    let old_name = config.libraries[library_id].name.clone();
    if config
        .libraries
        .iter()
        .enumerate()
        .any(|(index, library)| index != library_id && library.name.eq_ignore_ascii_case(name))
    {
        return settings_error("A library with that name already exists");
    }
    let library = &mut config.libraries[library_id];
    library.name = name.to_owned();
    library.scan_enabled = form.scan_enabled.is_some();
    library.scan_interval_value = form.scan_interval_value;
    library.scan_interval_unit = form.scan_interval_unit;
    library.show_recently_added = form.show_recently_added.is_some();
    library.show_continue_watching = form.show_continue_watching.is_some();
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if old_name != name {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let updated = connection.execute(
            "UPDATE library_groups SET name = ?1, updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?2",
            (name, &old_name),
        );
        if updated.is_err() || update_library_catalog_timestamp(&connection).is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    Redirect::to(&format!(
        "/settings?library_saved=true&library_open={library_id}"
    ))
    .into_response()
}

pub(crate) async fn update_library_display(
    State(state): State<AppState>,
    Form(form): Form<LibraryDisplayForm>,
) -> Response {
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get_mut(form.library_id) else {
        return settings_error("Library does not exist");
    };
    library.show_recently_added = form.show_recently_added.is_some();
    library.show_continue_watching = form.show_continue_watching.is_some();
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to("/settings").into_response()
}

pub(crate) async fn update_library_scan_settings(
    State(state): State<AppState>,
    Form(form): Form<LibraryScanSettingsForm>,
) -> Response {
    if form.scan_interval_value == 0
        || !matches!(
            form.scan_interval_unit.as_str(),
            "seconds" | "minutes" | "hours"
        )
    {
        return settings_error("Scan intervals need a positive integer and a valid time unit");
    }
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get_mut(form.library_id) else {
        return settings_error("Library does not exist");
    };
    library.scan_enabled = form.scan_enabled.is_some();
    library.scan_interval_value = form.scan_interval_value;
    library.scan_interval_unit = form.scan_interval_unit;
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to("/settings").into_response()
}

pub(crate) async fn scan_all_libraries(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let force = query_bool(&query, "force");
    let library_count = match state.config.lock() {
        Ok(config) => config.libraries.len(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let mut media_count = 0;
    let mut failed = false;
    for library_id in 0..library_count {
        match scan_library_inventory(&state, library_id, true, force, true).await {
            Ok(count) => media_count += count,
            Err(error) => {
                failed = true;
                warn!(library_id, %error, "manual library scan failed");
            }
        }
    }
    Redirect::to(&format!(
        "/settings?inventory_scan={media_count}&inventory_error={failed}&force={force}"
    ))
    .into_response()
}

pub(crate) async fn scan_library_inventory_handler(
    State(state): State<AppState>,
    Path(library_id): Path<usize>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let force = query_bool(&query, "force");
    match scan_library_inventory(&state, library_id, true, force, true).await {
        Ok(count) => Redirect::to(&format!("/settings?inventory_scan={count}")).into_response(),
        Err(error) => {
            warn!(library_id, %error, "manual library scan failed");
            Redirect::to("/settings?inventory_error=true").into_response()
        }
    }
}

pub(crate) async fn delete_library(
    State(state): State<AppState>,
    Path(library_id): Path<usize>,
    headers: HeaderMap,
) -> Response {
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get(library_id) else {
        return settings_action_error(&headers, StatusCode::NOT_FOUND, "Library does not exist");
    };
    let library_name = library.name.clone();
    let mut updated_config = config.clone();
    updated_config.libraries.remove(library_id);

    let Ok(mut connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(transaction) = connection.transaction() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if transaction
        .execute(
            "DELETE FROM library_folders WHERE library_id = (SELECT id FROM library_groups WHERE name = ?1)",
            [&library_name],
        )
        .and_then(|_| {
            transaction.execute("DELETE FROM library_groups WHERE name = ?1", [&library_name])
        })
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if update_library_catalog_timestamp(&transaction).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if save_config(&state.config_path, &updated_config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if transaction.commit().is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    *config = updated_config;
    settings_action_response(&headers, "/settings")
}

pub(crate) fn selected_library_folder_root(
    state: &AppState,
    library: Option<usize>,
    folder: Option<usize>,
) -> PathBuf {
    let Ok(config) = state.config.lock() else {
        return state.root.as_ref().clone();
    };
    library
        .and_then(|index| config.libraries.get(index))
        .and_then(|library| library.folders.get(folder.unwrap_or_default()))
        .cloned()
        .unwrap_or_else(|| state.root.as_ref().clone())
}

pub(crate) async fn home_client(State(state): State<AppState>, search: Option<String>) -> Response {
    let Ok(config) = state.config.lock().map(|config| config.clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let media_index = media_snapshot(&state);
    let metadata_by_library = config
        .libraries
        .iter()
        .enumerate()
        .map(|(index, _)| plex_section_metadata(&state, index + 1))
        .collect::<Vec<_>>();
    let mut libraries = String::new();
    for (index, library) in config.libraries.iter().enumerate() {
        let artwork = library_artwork(&state, library, index).await;
        let artwork_markup = artwork
            .map(|(cover, banner)| {
                format!(
                    r#"<span class="library-art" style="display:grid;place-items:center;min-height:120px;background-size:cover;background-position:center;background-image:linear-gradient(90deg,rgba(17,24,39,.2),rgba(17,24,39,.8)),url('{}')">{}</span>"#,
                    banner,
                    cover_artwork_markup(&cover, &library.name)
                )
            })
            .unwrap_or_else(|| {
                format!(
                    r#"<span class="library-art library-art-fallback" style="display:grid;place-items:center;min-height:120px;font-size:2.5rem;background:linear-gradient(135deg,#1e3a8a,#312e81)">{}</span>"#,
                    library_kind_emoji(&library.kind)
                )
            });
        libraries.push_str(&format!(
            "<a class=\"library-card\" data-spa-link style=\"overflow:hidden;padding:0\" href=\"/?library={}\">{}<span class=\"library-card-info\" style=\"display:flex;flex-direction:column;gap:.25rem;padding:.85rem 1rem 1rem\"><strong>{}</strong><span class=\"note\">{}</span></span></a>",
            index,
            artwork_markup,
            html(&library.name),
            format!(
                "{} · {} folders",
                library_kind_label(&library.kind),
                library.folders.len()
            )
        ));
    }
    let (continue_watching, watched_paths, hidden_paths, last_activity) = state
        .db
        .lock()
        .ok()
        .and_then(|connection| {
            let mut statement = connection
                .prepare("SELECT media_key, watched, hidden_from_continue, updated_at FROM watch_progress WHERE user_id = ?1")
                .ok()?;
            let rows = statement
                .query_map([USER_ID], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .ok()?;
            let mut continue_watching = Vec::new();
            let mut watched_paths = std::collections::HashSet::new();
            let mut hidden_paths = std::collections::HashSet::new();
            let mut last_activity = std::collections::HashMap::new();
            for row in rows.filter_map(Result::ok) {
                let (path, watched, hidden, updated_at) = row;
                if watched {
                    watched_paths.insert(path.clone());
                }
                if hidden {
                    hidden_paths.insert(path);
                    continue;
                }
                last_activity.insert(path.clone(), updated_at);
                if !watched {
                    continue_watching.push((path, updated_at));
                }
            }
            Some((
                continue_watching,
                watched_paths,
                hidden_paths,
                last_activity,
            ))
        })
        .unwrap_or_default();
    let mut tv_episode_paths = std::collections::HashSet::new();
    for library in config
        .libraries
        .iter()
        .filter(|library| library.kind == "tv")
    {
        for root in &library.folders {
            for media in media_index.iter() {
                let Ok(relative) = media.path.strip_prefix(root) else {
                    continue;
                };
                if tv_episode_info(root, &media.path).is_some() {
                    tv_episode_paths.insert(relative.to_string_lossy().into_owned());
                }
            }
        }
    }
    let mut continue_candidates = next_tv_episode_paths(
        &config.libraries,
        media_index.as_ref(),
        &watched_paths,
        &hidden_paths,
        &last_activity,
    );
    for (path, updated_at) in continue_watching {
        if !tv_episode_paths.contains(&path) {
            continue_candidates.push((path, updated_at));
        }
    }
    let continue_paths = ordered_continue_paths(continue_candidates);
    let mut continue_cards = String::new();
    for path in continue_paths {
        let Some((library_index, root, media)) =
            config
                .libraries
                .iter()
                .enumerate()
                .find_map(|(index, library)| {
                    if !library.show_continue_watching {
                        return None;
                    }
                    library.folders.iter().find_map(|root| {
                        media_index
                            .iter()
                            .find(|media| {
                                media
                                    .path
                                    .strip_prefix(root)
                                    .ok()
                                    .is_some_and(|relative| relative.to_string_lossy() == path)
                            })
                            .map(|media| (index, root, media))
                    })
                })
        else {
            continue;
        };
        let Some(folder) = media.path.parent() else {
            continue;
        };
        let Ok(relative_folder) = folder.strip_prefix(root) else {
            continue;
        };
        let Ok(relative_media) = media.path.strip_prefix(root) else {
            continue;
        };
        let metadata = metadata_by_library
            .get(library_index)
            .and_then(|items| items.get(&media.id));
        continue_cards.push_str(&media_card(
            media,
            &config.libraries[library_index],
            metadata,
            root,
            relative_folder,
            relative_media,
            library_index,
            true,
        ));
    }
    let continue_watching = if continue_cards.is_empty() {
        "<p class=\"note empty-state\">Nothing to resume yet. Start watching something and it will appear here.</p>".to_owned()
    } else {
        continue_cards
    };
    let mut recent = String::new();
    let mut recent_count = 0;
    for media in media_index.iter() {
        if recent_count >= 12 {
            break;
        }
        let Some((library_index, root)) =
            config
                .libraries
                .iter()
                .enumerate()
                .find_map(|(index, library)| {
                    if !library.show_recently_added {
                        return None;
                    }
                    library
                        .folders
                        .iter()
                        .find(|folder| media.path.starts_with(folder))
                        .map(|folder| (index, folder))
                })
        else {
            continue;
        };
        let Some(folder) = media.path.parent() else {
            continue;
        };
        let Ok(relative_folder) = folder.strip_prefix(root) else {
            continue;
        };
        let Ok(relative_media) = media.path.strip_prefix(root) else {
            continue;
        };
        recent.push_str(&media_card(
            media,
            &config.libraries[library_index],
            metadata_by_library
                .get(library_index)
                .and_then(|items| items.get(&media.id)),
            root,
            relative_folder,
            relative_media,
            library_index,
            false,
        ));
        recent_count += 1;
    }
    if recent.is_empty() {
        recent = "<p class=\"note empty-state\">No recently added media yet.</p>".to_owned();
    }
    let search_term = search.as_deref().unwrap_or_default().trim();
    let search_section = if search_term.is_empty() {
        String::new()
    } else {
        format!(
            "<section class=\"section\"><h2>Search results</h2>{}</section>",
            web_search_results_markup(
                search_term,
                &config.libraries,
                &metadata_by_library,
                &media_index,
            )
        )
    };
    let sidebar_markup = sidebar_nav_for(&state, std::path::Path::new(""), None);
    let search_markup = format!("{}{}", global_search_markup(), global_search_top_style()).replace(
        "name=\"search\" type=\"search\" value=\"\"",
        &format!(
            "name=\"search\" type=\"search\" value=\"{}\"",
            html(search_term)
        ),
    );
    let mut home_page = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="icon" href="/favicon.ico" sizes="any"><title>harbinger</title><style>body {{ margin: 0; color: #f9fafb; background: #111827; font-family: system-ui, sans-serif; }} .app-shell {{ display: grid; grid-template-columns: 240px minmax(0, 1fr); min-height: 100vh; }} .sidebar {{ position: sticky; top: 0; height: 100vh; box-sizing: border-box; padding: 1.25rem .9rem; border-right: 1px solid #263244; background: #0d1421; }} .sidebar-brand {{ display: block; padding: .5rem .7rem 1.5rem; color: #f9fafb; font-size: 1.25rem; font-weight: 700; text-decoration: none; }} .sidebar-top {{ display: flex; justify-content: space-between; align-items: center; }} .sidebar-toggle {{ background: none; border: 0; color: #cbd5e1; font-size: 1.3rem; cursor: pointer; }} .sidebar-label {{ padding: 1rem .7rem .4rem; color: #6b7280; font-size: .7rem; text-transform: uppercase; }} .sidebar-nav {{ display: grid; gap: .25rem; }} .sidebar-link {{ display: flex; gap: .7rem; padding: .65rem .7rem; border-radius: .5rem; color: #cbd5e1; text-decoration: none; }} .sidebar-link.active, .sidebar-link:hover {{ background: #1d4ed8; color: #fff; }} .sidebar-icon {{ width: 1.3rem; text-align: center; }} .sidebar-settings-group {{ margin: 0; }} .sidebar-settings-group > summary {{ list-style: none; cursor: pointer; }} .sidebar-settings-group > summary::-webkit-details-marker {{ display: none; }} .sidebar-settings-group > summary::marker {{ content: ""; }} .sidebar-collapsed .sidebar-settings-group[open] .sidebar-subnav {{ display: none; }} .sidebar-collapsed .sidebar-chevron {{ display: none; }} main {{ max-width: 1100px; width: 100%; box-sizing: border-box; margin: auto; padding: 2rem 1rem; }} a {{ color: #93c5fd; text-decoration: none; }} h1 {{ margin-top: 0; }} .section {{ margin: 2rem 0; }} .library-grid, .recent-grid, .continue-grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax({}px, 1fr)); gap: 1rem; }} .library-card {{ display: flex; flex-direction: column; gap: .4rem; padding: 1rem; border: 1px solid #374151; border-radius: .7rem; background: #1f2937; }} .media-card {{ position: relative; min-width: 0; padding: .7rem; border: 1px solid transparent; border-radius: .6rem; background: #1f2937; transition: border-color .15s, transform .15s; }} .media-card:hover {{ border-color: #374151; transform: translateY(-2px); }} .media-card-artwork {{ position: relative; }} .media-card-info {{ display: grid; gap: .12rem; padding: .55rem 0 1.6rem; color: inherit; text-align: center; text-decoration: none; }} .media-card-title {{ display: block; height: 2.4em; overflow: hidden; margin: .25rem 0 .15rem; font-size: .8rem; font-weight: 500; line-height: 1.2; }} .media-card-tv .media-card-title {{ height: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }} .media-card-subtitle, .media-card-details {{ overflow: hidden; color: #9ca3af; font-size: .75rem; line-height: 1.35; text-overflow: ellipsis; white-space: nowrap; }} .media-card-progress-row {{ display: flex; align-items: center; gap: .45rem; margin: .3rem 0 .4rem; }} .media-card-progress {{ flex: 1 1 auto; min-width: 0; height: .3rem; border-radius: 999px; background: #374151; overflow: hidden; }} .media-card-progress span {{ display: block; height: 100%; border-radius: inherit; background: linear-gradient(90deg, #2563eb, #22c55e); }} .media-card-percent {{ flex: 0 0 auto; color: #93c5fd; font-size: .68rem; line-height: 1; }} .media-card-menu {{ position: absolute; right: .45rem; bottom: .45rem; z-index: 4; }} .media-card-menu summary {{ display: grid; width: 1.65rem; height: 1.65rem; place-items: center; border: 1px solid rgba(255,255,255,.25); border-radius: 50%; color: #f9fafb; background: rgba(17,24,39,.58); backdrop-filter: blur(3px); font-size: 1rem; font-weight: 800; line-height: 1; list-style: none; cursor: pointer; }} .media-card-menu summary::-webkit-details-marker {{ display: none; }} .media-card-menu-items {{ position: absolute; right: 0; bottom: calc(100% + .35rem); display: grid; min-width: 12.5rem; gap: .2rem; padding: .35rem; border: 1px solid #374151; border-radius: .5rem; background: #111827; box-shadow: 0 8px 24px #0008; }} .media-card-menu-items button {{ padding: .55rem .65rem; border: 0; border-radius: .3rem; color: #e5e7eb; background: transparent; font: inherit; font-size: .78rem; text-align: left; cursor: pointer; }} .media-card-menu-items button:hover {{ background: #263244; }} .library-icon {{ font-size: 2rem; }} .note {{ color: #9ca3af; }} .empty-state {{ padding: 1.5rem; border: 1px dashed #4b5563; border-radius: .7rem; }} .continue-grid > .empty-state {{ grid-column: 1 / -1; }} @media (max-width: 700px) {{ .app-shell {{ display: block; }} .sidebar {{ position: static; height: auto; width: 100%; padding: .7rem; border-right: 0; border-bottom: 1px solid #263244; }} .sidebar-top {{ justify-content: flex-start; padding-bottom: .35rem; }} .sidebar-brand {{ display: inline-block; padding: .35rem .55rem; }} .sidebar-toggle {{ display: none; }} .sidebar-label {{ display: none; }} .sidebar-collapsed .sidebar-top {{ justify-content: flex-start; }} .sidebar-collapsed .sidebar-brand {{ font-size: 1.25rem; }} .sidebar-collapsed .sidebar-link {{ justify-content: flex-start; padding-left: .7rem; padding-right: .7rem; font-size: 1rem; }} .sidebar-collapsed .sidebar-icon {{ font-size: 1.1rem; }} .sidebar-nav {{ display: flex; flex-wrap: wrap; gap: .35rem; padding-bottom: .2rem; }} .sidebar-link {{ min-height: 2.75rem; white-space: nowrap; }} .sidebar-collapsed .sidebar-settings-group[open] .sidebar-subnav {{ display: grid; }} main {{ padding: 1rem .9rem max(2rem, calc(1.25rem + env(safe-area-inset-bottom, 0px))); }} h1 {{ font-size: clamp(1.65rem, 7vw, 2rem); line-height: 1.15; overflow-wrap: anywhere; }} .library-grid, .recent-grid, .continue-grid {{ gap: .7rem; }} .media-card {{ padding: .55rem; }} .media-card-menu summary {{ min-width: 2.5rem; min-height: 2.5rem; padding: .45rem; }} .media-card-menu-items {{ right: 0; min-width: 9rem; max-width: calc(100vw - 2rem); }} .media-card-menu-items button {{ min-height: 2.5rem; }} input, select, textarea {{ font-size: 16px; }} }}{SIDEBAR_MOBILE_CSS}</style></head><body><div class="app-shell">{}<main><form class="section" method="get" action="/" role="search"><label for="media-search">Search all media</label><div style="display:flex;gap:.5rem;margin-top:.5rem"><input id="media-search" name="search" type="search" value="{}" style="flex:1;min-width:0;padding:.7rem;border:1px solid #374151;border-radius:.45rem;color:#fff;background:#0d1421" autocomplete="off"><button type="submit">Search</button></div></form>{}<section class="section"><h2>Continue watching</h2><div class="continue-grid">{}</div></section><section class="section"><h2>Recently added</h2><div class="recent-grid">{}</div></section><section class="section"><h2>Your libraries</h2><div class="library-grid">{}</div></section></main></div>{}{}{}<script>const shell=document.querySelector('.app-shell'), toggle=document.querySelector('.sidebar-toggle'); if(localStorage.getItem('mplex-sidebar-collapsed')==='true') shell.classList.add('sidebar-collapsed'); toggle?.addEventListener('click',()=>{{shell.classList.toggle('sidebar-collapsed');localStorage.setItem('mplex-sidebar-collapsed',shell.classList.contains('sidebar-collapsed')?'true':'false')}});</script></body></html>"#,
        POSTER_CARD_MIN_WIDTH,
        sidebar_markup,
        html(search_term),
        search_section,
        continue_watching,
        recent,
        libraries,
        format!(
            "{}{}",
            fix_match_dialog_markup(),
            book_cover_dialog_markup()
        ),
        media_card_action_script(),
        spa_navigation_script()
    );
    if let Some(start) =
        home_page.find("<form class=\"section\" method=\"get\" action=\"/\" role=\"search\">")
    {
        if let Some(end) = home_page[start..].find("</form>") {
            home_page.replace_range(start..start + end + "</form>".len(), "");
        }
    }
    home_page = home_page.replace(
        "<main>",
        &format!("<main class=\"with-global-topbar\">{}", search_markup),
    );
    Html(add_build_info_footer(home_page)).into_response()
}

pub(crate) fn ordered_continue_paths(mut candidates: Vec<(String, i64)>) -> Vec<String> {
    candidates.sort_by(|(left_path, left_time), (right_path, right_time)| {
        right_time
            .cmp(left_time)
            .then_with(|| left_path.cmp(right_path))
    });
    let mut seen = std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter_map(|(path, _)| seen.insert(path.clone()).then_some(path))
        .take(12)
        .collect()
}

pub(crate) fn web_title_sort_key(title: &str) -> String {
    let title = title.trim();
    let sortable = title
        .get(..4)
        .filter(|prefix| prefix.eq_ignore_ascii_case("the "))
        .map(|_| &title[4..])
        .unwrap_or(title);
    sortable.to_lowercase()
}

fn global_search_top_style() -> &'static str {
    r#"<style>.with-global-topbar{padding-top:0!important}.app-shell:has(.global-topbar) main{padding-top:0!important}.global-topbar{position:sticky;top:0;z-index:30;width:100%;min-height:0;margin:0 0 .65rem;padding:.2rem 0;background:transparent;border-bottom:0}</style>"#
}

pub(crate) async fn web_search(
    State(state): State<AppState>,
    Query(query): Query<WebSearchQuery>,
) -> Response {
    let Ok(config) = state.config.lock().map(|config| config.clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let metadata = config
        .libraries
        .iter()
        .enumerate()
        .map(|(index, _)| plex_section_metadata(&state, index + 1))
        .collect::<Vec<_>>();
    let media = media_snapshot(&state);
    Html(web_search_results_markup(
        query.query.as_deref().unwrap_or_default().trim(),
        &config.libraries,
        &metadata,
        &media,
    ))
    .into_response()
}

fn web_search_results_markup(
    query: &str,
    libraries: &[ConfigLibrary],
    metadata_by_library: &[HashMap<String, serde_json::Value>],
    media_index: &[Media],
) -> String {
    if query.is_empty() {
        return String::new();
    }
    let needle = query.to_lowercase();
    let mut cards = String::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut seen_episodes = std::collections::HashSet::new();
    let mut searchable_media = media_index.iter().collect::<Vec<_>>();
    searchable_media.sort_by_key(|media| web_title_sort_key(&media.title));
    for media in searchable_media {
        let Some((library_index, library, root)) =
            libraries.iter().enumerate().find_map(|(index, library)| {
                let root = library
                    .folders
                    .iter()
                    .find(|root| media.path.starts_with(*root))?;
                media_in_library(media, library).then_some((index, library, root))
            })
        else {
            continue;
        };
        let canonical_path =
            std::fs::canonicalize(&media.path).unwrap_or_else(|_| media.path.clone());
        if !seen_paths.insert(canonical_path) {
            continue;
        }
        let metadata = metadata_by_library
            .get(library_index)
            .and_then(|items| items.get(&media.id));
        let title = metadata
            .and_then(|item| item.get("title").or_else(|| item.get("name")))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&media.title);
        let original_title_matches = metadata
            .and_then(|item| item.get("originalTitle"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|title| title.to_lowercase().contains(&needle));
        if !title.to_lowercase().contains(&needle) && !original_title_matches {
            continue;
        }
        if library.kind == "tv" {
            if let Some(episode) = tv_episode_info(root, &media.path) {
                let show = media
                    .path
                    .strip_prefix(root)
                    .ok()
                    .and_then(|path| path.components().next())
                    .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !seen_episodes.insert((library_index, show, episode.season, episode.episode)) {
                    continue;
                }
            }
        }
        let Some(relative_media) = media.path.strip_prefix(root).ok() else {
            continue;
        };
        let relative_folder = relative_media.parent().unwrap_or(std::path::Path::new(""));
        cards.push_str(&media_card(
            media,
            library,
            metadata,
            root,
            relative_folder,
            relative_media,
            library_index,
            false,
        ));
    }
    if cards.is_empty() {
        format!(
            "<p class=\"note empty-state\" role=\"status\">No matching media found for “{}”.</p>",
            html(query)
        )
    } else {
        format!("<div class=\"global-search-results-list\" style=\"display:grid;grid-template-columns:repeat(auto-fill,minmax(8rem,1fr));gap:.5rem\">{cards}</div>")
    }
}

pub(crate) async fn library_artwork(
    state: &AppState,
    library: &ConfigLibrary,
    index: usize,
) -> Option<(String, String)> {
    let root = library.folders.first()?;
    let config_mode = state
        .config
        .lock()
        .ok()
        .is_some_and(|config| config.metadata_storage == "config");
    for media in media_snapshot(state)
        .iter()
        .filter(|media| media_in_library(media, library))
    {
        let group_path = metadata_group_path(root, &media.path, &library.kind);
        let parent = if group_path.is_file() {
            group_path.parent().unwrap_or(root)
        } else {
            group_path.as_path()
        };
        let relative = parent.strip_prefix(root).ok()?;
        let key = database_media_key(index + 1, &media.id);
        let config_dir = config_metadata_sidecar_path(state, &key)
            .parent()
            .map(|path| path.to_path_buf())?;
        let media_dir = parent.join(".hbgr");
        let mut paths = [config_dir.join("cover.jpg"), media_dir.join("cover.jpg")];
        if !config_mode {
            paths.reverse();
        }
        let Some(cover_path) = paths.iter().find(|path| path.is_file()) else {
            continue;
        };
        let cover = artwork_url(relative, Some(index), "cover");
        let banner_path = cover_path.parent()?.join("banner.jpg");
        let banner = if banner_path.is_file() {
            artwork_url(relative, Some(index), "banner")
        } else {
            cover.clone()
        };
        return Some((cover, banner));
    }
    None
}

pub(crate) fn cover_artwork_markup(url: &str, label: &str) -> String {
    format!(
        r#"<img class="cover-art" style="width:30%;max-height:110px;object-fit:cover;border-radius:.25rem;box-shadow:0 3px 12px #0008" src="{}" alt="Cover for {}">"#,
        html(url),
        html(label)
    )
}

fn library_page_bounds(total: usize, offset: usize) -> (std::ops::Range<usize>, bool) {
    let start = offset.min(total);
    let end = (start + WEB_LIBRARY_PAGE_SIZE).min(total);
    (start..end, end < total)
}

pub(crate) async fn web_client(
    State(s): State<AppState>,
    Query(mut query): Query<WebPath>,
) -> Response {
    let library_offset = query.offset.unwrap_or(0);
    let mut library_has_more = false;
    let (_, audio_language_primary, audio_language_secondary, _, _) =
        effective_plex_language_settings(&s);
    let (bilingual_enabled, language_primary, language_secondary) =
        configured_bilingual_languages(&s);
    let (skip_intro_mode, skip_credits_mode) = s
        .config
        .lock()
        .ok()
        .map(|config| {
            (
                config.skip_intro_mode.as_str().to_owned(),
                config.skip_credits_mode.as_str().to_owned(),
            )
        })
        .unwrap_or((
            default_skip_mode().as_str().to_owned(),
            default_skip_mode().as_str().to_owned(),
        ));
    if query.library.is_none() {
        query.library = s
            .config
            .lock()
            .ok()
            .and_then(|config| (!config.libraries.is_empty()).then_some(0));
    }
    let media_index = media_snapshot(&s);
    let relative = match clean_relative_path(query.path.as_deref().unwrap_or("")) {
        Some(path) => path,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let library_root = selected_library_folder_root(&s, query.library, query.folder);
    let library_kind = query.library.and_then(|index| {
        s.config.lock().ok().and_then(|config| {
            config
                .libraries
                .get(index)
                .map(|library| library.kind.clone())
        })
    });
    let sidecar_mode = s
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "media");
    let library_config = query.library.and_then(|index| {
        s.config
            .lock()
            .ok()
            .and_then(|config| config.libraries.get(index).cloned())
    });
    let library_top_level =
        relative.as_os_str().is_empty() && query.play.is_none() && query.episode.is_none();
    let requested_path = match web_path(&s, &library_root, &relative).await {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    let audiobook_request = query.play.is_none()
        && query.episode.is_none()
        && library_kind.as_deref().is_some_and(is_audiobook_library);
    let directory = if requested_path.is_dir() {
        requested_path.clone()
    } else if requested_path.is_file() && audiobook_request {
        requested_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| library_root.clone())
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let audiobook_detail_item = if audiobook_request {
        let catalog = crate::audiobookshelf::catalog::Catalog::load_async(&s).await;
        catalog
            .find_item_path(
                query.library.unwrap_or_default(),
                &relative,
                &library_root,
                &requested_path,
            )
            .cloned()
    } else {
        None
    };
    let selected_relative = query
        .episode
        .as_deref()
        .or(query.play.as_deref())
        .and_then(clean_relative_path);
    if let Some(selected_relative) = selected_relative.as_deref() {
        match web_path(&s, &library_root, selected_relative).await {
            Ok(path) if path.is_file() && web_media_type(&path).is_some() => {}
            _ => return StatusCode::NOT_FOUND.into_response(),
        }
    }
    let episode_banner_directory = selected_relative.as_deref().and_then(|selected| {
        let selected_path = library_root.join(selected);
        tv_episode_show_directory(&library_root, &selected_path)
    });
    let banner_directory = episode_banner_directory.unwrap_or_else(|| directory.clone());
    let banner_relative = banner_directory
        .strip_prefix(&library_root)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|_| relative.clone());
    let config_banner_exists = query.library.is_some_and(|library_index| {
        media_index
            .iter()
            .filter(|media| {
                library_config
                    .as_ref()
                    .is_some_and(|library| media_in_library(media, library))
            })
            .any(|media| {
                let group_directory = tv_episode_show_directory(&library_root, &media.path)
                    .unwrap_or_else(|| {
                        let group_path = metadata_group_path(
                            &library_root,
                            &media.path,
                            library_kind.as_deref().unwrap_or("movie"),
                        );
                        if group_path.is_file() {
                            group_path.parent().unwrap_or(&group_path).to_path_buf()
                        } else {
                            group_path
                        }
                    });
                let group_matches_banner = group_directory == banner_directory
                    || std::fs::canonicalize(&group_directory)
                        .is_ok_and(|canonical| canonical == banner_directory);
                if !group_matches_banner {
                    return false;
                }
                let key = database_media_key(library_index + 1, &media.id);
                config_metadata_sidecar_path(&s, &key)
                    .parent()
                    .is_some_and(|path| path.join("banner.jpg").is_file())
            })
    });
    let pane_banner = if banner_directory.join(".hbgr/banner.jpg").is_file() || config_banner_exists
    {
        format!(
            r#" style="background-size:cover;background-position:center;background-attachment:fixed;background-image:linear-gradient(180deg,rgba(17,24,39,.72),rgba(17,24,39,.96)),url('{}')""#,
            artwork_url(&banner_relative, query.library, "banner")
        )
    } else {
        String::new()
    };
    let tv_season_view = library_kind.as_deref() == Some("tv")
        && (query.season.is_some()
            || relative
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_tv_season_directory)
                .is_some());
    let tv_show_view = library_kind.as_deref() == Some("tv")
        && relative.components().count() == 1
        && !tv_season_view;

    let info_panel = if audiobook_detail_item.is_some() {
        String::new()
    } else if let Some(selected_relative) = selected_relative.as_deref() {
        if tv_episode_info(&library_root, &library_root.join(selected_relative)).is_some() {
            episode_info_panel(&s, &library_root, selected_relative).await
        } else {
            let selected_path = library_root.join(selected_relative);
            let selected_metadata = media_index
                .iter()
                .find(|media| media.path == selected_path)
                .and_then(|media| {
                    let (library_id, library) = media_library(&s, media)?;
                    let root = library
                        .folders
                        .iter()
                        .find(|folder| media.path.starts_with(folder))?;
                    let media_key = database_media_key(library_id, &media.id);
                    let group_path = metadata_group_path(root, &media.path, &library.kind);
                    read_metadata_sidecar(&s, &media_key, &group_path)
                });
            let cover_artwork = media_index
                .iter()
                .find(|media| media.path == selected_path)
                .map(|media| media_cover_url(&media.id));
            media_info_panel(
                selected_path.parent().unwrap_or(&library_root),
                selected_metadata.as_ref(),
                !sidecar_mode,
                false,
                cover_artwork.as_deref(),
            )
            .await
        }
    } else if sidecar_mode || tv_season_view || tv_show_view {
        let show_metadata_directory = library_root.join(&relative);
        let show_metadata = (tv_show_view || query.season.is_some())
            .then(|| matched_tv_show_metadata(&s, &media_index, &show_metadata_directory))
            .flatten();
        let selected_metadata = tv_show_view.then_some(show_metadata.as_ref()).flatten();
        let cover_url = if tv_show_view {
            let config_cover = media_index
                .iter()
                .find(|media| media.path.starts_with(&show_metadata_directory))
                .and_then(|media| {
                    let (library_id, _) = media_library(&s, media)?;
                    let media_key = database_media_key(library_id, &media.id);
                    config_metadata_sidecar_path(&s, &media_key)
                        .parent()
                        .map(|directory| directory.join("cover.jpg"))
                });
            let media_cover_exists =
                tokio::fs::metadata(show_metadata_directory.join(".hbgr/cover.jpg"))
                    .await
                    .is_ok();
            let config_cover_exists = if let Some(path) = config_cover {
                tokio::fs::metadata(path).await.is_ok()
            } else {
                false
            };
            (media_cover_exists || config_cover_exists)
                .then(|| artwork_url(&relative, query.library, "cover"))
        } else if !tv_season_view {
            media_index
                .iter()
                .filter(|media| {
                    library_config
                        .as_ref()
                        .is_some_and(|library| media_in_library(media, library))
                })
                .find(|media| {
                    let group_path = metadata_group_path(
                        &library_root,
                        &media.path,
                        library_kind.as_deref().unwrap_or("movie"),
                    );
                    let group_directory = if group_path.is_file() {
                        group_path.parent().unwrap_or(&library_root)
                    } else {
                        group_path.as_path()
                    };
                    group_directory == directory
                        || std::fs::canonicalize(group_directory)
                            .is_ok_and(|canonical| canonical == directory)
                })
                .map(|media| media_cover_url(&media.id))
        } else {
            None
        };
        let panel_directory = query
            .season
            .map(|season| tv_season_directory(&directory, season, &directory))
            .unwrap_or_else(|| directory.clone());
        let mut panel = media_info_panel(
            &panel_directory,
            selected_metadata,
            !sidecar_mode,
            tv_show_view,
            cover_url.as_deref(),
        )
        .await;
        if tv_season_view && !panel.contains("class=\"metadata-description\"") {
            let season = query.season.or_else(|| {
                relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(parse_tv_season_directory)
            });
            let show_directory = if query.season.is_some() {
                Some(directory.as_path())
            } else {
                directory.parent()
            };
            if let (Some(season), Some(show_directory)) = (season, show_directory) {
                let show_metadata_directory = library_root.join(if query.season.is_some() {
                    relative.as_path()
                } else {
                    relative.parent().unwrap_or(std::path::Path::new(""))
                });
                let show_metadata =
                    matched_tv_show_metadata(&s, &media_index, &show_metadata_directory);
                let representative_path = media_index
                    .iter()
                    .find(|media| {
                        media
                            .path
                            .strip_prefix(&library_root)
                            .is_ok_and(|path| path.starts_with(&relative))
                            && tv_season_number(&library_root, &media.path) == Some(season)
                    })
                    .map(|media| media.path.clone())
                    .unwrap_or_else(|| show_directory.to_path_buf());
                let season_directory =
                    tv_season_directory(show_directory, season, &representative_path);
                let season_metadata = if sidecar_mode {
                    read_sidecar_info(&season_directory)
                        .await
                        .and_then(|info| info.get("metadata_json").cloned())
                } else {
                    media_index
                        .iter()
                        .filter(|media| {
                            media
                                .path
                                .strip_prefix(&library_root)
                                .is_ok_and(|path| path.starts_with(&relative))
                                && tv_season_number(&library_root, &media.path) == Some(season)
                        })
                        .find_map(|media| {
                            let (library_id, library) = media_library(&s, media)?;
                            let root = library
                                .folders
                                .iter()
                                .find(|folder| media.path.starts_with(folder))?;
                            let key = database_media_key(library_id, &media.id);
                            let group_path = metadata_group_path(root, &media.path, &library.kind);
                            read_metadata_sidecar(&s, &key, &group_path)
                        })
                        .and_then(|metadata| {
                            metadata
                                .get("mplex_season_description")
                                .cloned()
                                .map(|overview| {
                                    serde_json::json!({
                                        "name": format!("Season {season}"),
                                        "overview": overview
                                    })
                                })
                        })
                }
                .unwrap_or_else(|| serde_json::json!({"name": format!("Season {season}")}));
                panel = render_season_description_panel(
                    &season_metadata,
                    season,
                    show_metadata.as_ref(),
                );
            }
        }
        panel
    } else {
        String::new()
    };
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let virtual_tv_view =
        library_kind.as_deref() == Some("tv") && (tv_show_view || query.season.is_some());
    if audiobook_detail_item.is_none() && !library_top_level && !virtual_tv_view {
        let mut entries = match tokio::fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let child_relative = relative.join(&name);
            if file_type.is_dir() {
                directories.push((name, child_relative));
            } else if file_type.is_file()
                && web_media_type(&path).is_some()
                && selected_relative
                    .as_deref()
                    .is_none_or(|selected| selected == child_relative)
            {
                files.push((name, child_relative, path));
            }
        }
    }
    directories.sort_by_key(|(name, _)| name.to_ascii_lowercase());
    files.sort_by_key(|(name, _, _)| name.to_ascii_lowercase());

    let mut items = String::new();
    if audiobook_detail_item.is_none() {
        items.push_str(&breadcrumb_nav(
            &relative,
            &query,
            selected_relative.as_deref(),
            library_kind.as_deref(),
        ));
    }
    let sidebar_library = query.library.or_else(|| {
        s.config
            .lock()
            .ok()
            .and_then(|config| (!config.libraries.is_empty()).then_some(0))
    });
    let sidebar = sidebar_nav_for(&s, &relative, sidebar_library);
    let page_heading = audiobook_detail_item
        .as_ref()
        .map(|item| {
            let title = if item.metadata.title.is_some() {
                if item.author.trim().is_empty() {
                    html(&item.title)
                } else {
                    format!("{} - {}", html(&item.author), html(&item.title))
                }
            } else {
                relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(html)
                    .unwrap_or_else(|| html(&item.title))
            };
            format!("<h1>{title}</h1>")
        })
        .unwrap_or_default();
    let mut folder_cards = String::new();
    let mut season_sections = String::new();
    let mut has_content = false;
    if let Some(item) = audiobook_detail_item.as_ref() {
        items.push_str(&audiobook_detail(
            &s,
            item,
            &relative,
            query.library.unwrap_or_default(),
            query.folder,
        ));
        has_content = true;
    }
    let section_metadata = query
        .library
        .map(|index| plex_section_metadata(&s, index + 1))
        .unwrap_or_default();
    let mut virtual_seasons = BTreeMap::<u32, Vec<&Media>>::new();
    let folder_query = query
        .folder
        .map(|folder| format!("&folder={folder}"))
        .unwrap_or_default();
    if virtual_tv_view {
        let mut seen_episodes = std::collections::HashSet::new();
        for media in media_index.iter() {
            if !media
                .path
                .strip_prefix(&library_root)
                .is_ok_and(|path| path.starts_with(&relative))
            {
                continue;
            }
            if let Some(season) = tv_season_number(&library_root, &media.path) {
                let episode_key = tv_episode_info(&library_root, &media.path)
                    .map(|episode| format!("{season}:{}", episode.episode))
                    .unwrap_or_else(|| format!("{season}:{}", media.id));
                if !seen_episodes.insert(episode_key) {
                    continue;
                }
                virtual_seasons.entry(season).or_default().push(media);
            }
        }
        for episodes in virtual_seasons.values_mut() {
            episodes.sort_by_key(|media| {
                tv_episode_info(&library_root, &media.path)
                    .map(|episode| episode.episode)
                    .unwrap_or(u32::MAX)
            });
        }
    }
    if tv_show_view {
        let show_path = relative.to_string_lossy();
        for (season, episodes) in &virtual_seasons {
            let Some(representative) = episodes.first() else {
                continue;
            };
            let href = format!(
                "/?library={}&path={}{}&season={season}",
                query.library.unwrap_or_default(),
                url_encode(&show_path),
                folder_query
            );
            let representative_path = representative
                .path
                .strip_prefix(&library_root)
                .unwrap_or(std::path::Path::new(""));
            folder_cards.push_str(&media_cover_card(
                Some(&representative.id),
                &href,
                &format!("Season {season:02}"),
                Some(&format!("{} episodes", episodes.len())),
                None,
                average_media_progress(episodes, &section_metadata),
                None,
                representative_path,
                query.library.unwrap_or_default(),
                false,
                false,
                "item",
                false,
            ));
        }
        has_content = !folder_cards.is_empty();
    }
    if query.season.is_some() && selected_relative.is_none() {
        for episode in virtual_seasons
            .get(&query.season.unwrap_or_default())
            .into_iter()
            .flatten()
        {
            let Some(episode_path) = episode.path.strip_prefix(&library_root).ok() else {
                continue;
            };
            let item_metadata = section_metadata.get(&episode.id);
            let (episode_title, episode_number) =
                tv_episode_card_labels(&library_root, &episode.path, item_metadata);
            let title = episode_title.unwrap_or_else(|| episode.title.clone());
            let href = format!(
                "/?library={}&path={}{}&season={}&episode={}",
                query.library.unwrap_or_default(),
                url_encode(&relative.to_string_lossy()),
                folder_query,
                query.season.unwrap_or_default(),
                url_encode(&episode_path.to_string_lossy())
            );
            items.push_str(&media_cover_card(
                Some(&episode.id),
                &href,
                &title,
                episode_number.as_deref(),
                relative.file_name().and_then(|name| name.to_str()),
                item_metadata.map(media_card_progress).unwrap_or_default(),
                None,
                episode_path,
                query.library.unwrap_or_default(),
                true,
                true,
                "item",
                false,
            ));
        }
        has_content = virtual_seasons.contains_key(&query.season.unwrap_or_default());
    } else if query.season.is_some() {
        if let Some(selected_relative) = selected_relative.as_ref() {
            if let Some(media) = media_index
                .iter()
                .find(|media| media.path == library_root.join(selected_relative))
            {
                if let (Some(name), Some(path)) = (media.path.file_name(), Some(media.path.clone()))
                {
                    files.push((
                        name.to_string_lossy().into_owned(),
                        selected_relative.clone(),
                        path,
                    ));
                }
            }
        }
    }
    if library_top_level {
        if let (Some(library_index), Some(library)) = (query.library, library_config.as_ref()) {
            let mut media = media_index
                .iter()
                .filter(|item| media_in_library(item, library))
                .collect::<Vec<_>>();
            media.sort_by(|left, right| {
                web_title_sort_key(&left.title).cmp(&web_title_sort_key(&right.title))
            });
            let mut seen_paths = std::collections::HashSet::new();
            media.retain(|item| seen_paths.insert(item.path.clone()));
            if is_audiobook_library(&library.kind) {
                let catalog = match crate::audiobookshelf::catalog::Catalog::cached(&s) {
                    Some(catalog) => Some(catalog),
                    None => tokio::time::timeout(
                        Duration::from_millis(250),
                        crate::audiobookshelf::catalog::Catalog::load_async(&s),
                    )
                    .await
                    .ok(),
                };
                if let Some(catalog) = catalog {
                    if let Some(abs_library) = catalog
                        .libraries
                        .iter()
                        .find(|entry| entry.source_index == library_index)
                    {
                        let (range, more) =
                            library_page_bounds(abs_library.items.len(), library_offset);
                        library_has_more = more;
                        let saved_progress = audiobook_progress_map(&s);
                        for item in &abs_library.items[range] {
                            let Some((folder_index, item_relative)) =
                                abs_library.folders.iter().enumerate().find_map(
                                    |(index, folder)| {
                                        item.path
                                            .strip_prefix(std::path::Path::new(&folder.full_path))
                                            .ok()
                                            .map(|path| (index, path.to_path_buf()))
                                    },
                                )
                            else {
                                continue;
                            };
                            let progress = saved_progress
                                .get(&item.id)
                                .copied()
                                .unwrap_or((0, None, false));
                            folder_cards.push_str(&audiobook_media_card(
                                item,
                                &item_relative,
                                library_index,
                                (folder_index > 0).then_some(folder_index),
                                progress,
                            ));
                        }
                    }
                } else {
                    crate::audiobookshelf::catalog::refresh_catalog_in_background(s.clone());
                    folder_cards.push_str(
                        "<p class=\"note\">Preparing the audiobook catalog. This page will refresh automatically.</p>",
                    );
                }
            } else if library.kind == "tv" {
                let mut shows =
                    plex_tv_show_groups(library_index + 1, &library.folders, media.iter().copied());
                shows.sort_by(|left, right| {
                    web_title_sort_key(&left.title).cmp(&web_title_sort_key(&right.title))
                });
                let (range, more) = library_page_bounds(shows.len(), library_offset);
                library_has_more = more;
                for show in &shows[range] {
                    let Some(representative) = show.episodes.iter().max_by_key(|episode| {
                        section_metadata
                            .get(&episode.id)
                            .and_then(|metadata| metadata.get("mplex_last_viewed_at"))
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or_default()
                    }) else {
                        continue;
                    };
                    let Some((folder_index, source_root)) = library
                        .folders
                        .iter()
                        .enumerate()
                        .find(|(_, root)| representative.path.starts_with(*root))
                    else {
                        continue;
                    };
                    let show_path = representative
                        .path
                        .strip_prefix(source_root)
                        .ok()
                        .and_then(|path| path.components().next())
                        .map(|component| PathBuf::from(component.as_os_str()));
                    let Some(show_path) = show_path else {
                        continue;
                    };

                    let progress = average_media_progress(&show.episodes, &section_metadata);
                    let unwatched = unwatched_episode_count(
                        &show.episodes,
                        &library.folders,
                        &section_metadata,
                    );
                    let season_count = show
                        .episodes
                        .iter()
                        .filter_map(|episode| {
                            library
                                .folders
                                .iter()
                                .find(|root| episode.path.starts_with(*root))
                                .and_then(|root| tv_season_number(root, &episode.path))
                        })
                        .collect::<BTreeSet<_>>()
                        .len();
                    let season_label = format!(
                        "{season_count} season{}",
                        if season_count == 1 { "" } else { "s" }
                    );

                    let Some(representative_path) =
                        representative.path.strip_prefix(source_root).ok()
                    else {
                        continue;
                    };
                    let folder_query = (folder_index > 0)
                        .then(|| format!("&folder={folder_index}"))
                        .unwrap_or_default();
                    let href = format!(
                        "/?library={library_index}{folder_query}&path={}",
                        url_encode(&show_path.to_string_lossy())
                    );
                    folder_cards.push_str(&media_cover_card(
                        Some(&representative.id),
                        &href,
                        &show.title,
                        Some(&season_label),
                        None,
                        progress,
                        Some(unwatched),
                        representative_path,
                        library_index,
                        true,
                        true,
                        "show",
                        false,
                    ));
                }
            } else {
                let (range, more) = library_page_bounds(media.len(), library_offset);
                library_has_more = more;
                for media_item in &media[range] {
                    let Some(source_root) = library
                        .folders
                        .iter()
                        .find(|root| media_item.path.starts_with(*root))
                    else {
                        continue;
                    };
                    let Some(media_path) = media_item.path.strip_prefix(source_root).ok() else {
                        continue;
                    };
                    let folder = media_path.parent().unwrap_or(std::path::Path::new(""));
                    let item_metadata = section_metadata.get(&media_item.id);
                    folder_cards.push_str(&media_card(
                        media_item,
                        library,
                        item_metadata,
                        source_root,
                        folder,
                        media_path,
                        library_index,
                        false,
                    ));
                }
            }
            has_content = !folder_cards.is_empty();
        }
    }
    for (name, child_relative) in directories {
        has_content = true;
        let directory = directory.join(&name);
        if library_kind.as_deref() == Some("tv") && parse_tv_season_directory(&name).is_some() {
            let season_number = parse_tv_season_directory(&name).unwrap_or_default();
            let mut season_entries = match tokio::fs::read_dir(&directory).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            let mut episode_count = 0;
            while let Ok(Some(entry)) = season_entries.next_entry().await {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.')
                    || !entry.file_type().await.is_ok_and(|kind| kind.is_file())
                    || media_type(&path).is_none()
                {
                    continue;
                }
                episode_count += 1;
            }
            if episode_count > 0 {
                let season_path = directory.clone();
                let season_media = media_index
                    .iter()
                    .filter(|media| media.path.starts_with(&season_path))
                    .collect::<Vec<_>>();
                let progress = average_media_progress(&season_media, &section_metadata);
                let season_card = season_media
                    .first()
                    .map(|representative| {
                        media_cover_card(
                            Some(&representative.id),
                            &format!(
                                "/?library={}&path={}",
                                query.library.unwrap_or_default(),
                                url_encode(&child_relative.to_string_lossy())
                            ),
                            &format!("Season {season_number:02}"),
                            Some(&format!("{episode_count} episodes")),
                            None,
                            progress,
                            None,
                            representative
                                .path
                                .strip_prefix(&library_root)
                                .unwrap_or(std::path::Path::new("")),
                            query.library.unwrap_or_default(),
                            false,
                            false,
                            "item",
                            false,
                        )
                    })
                    .unwrap_or_default();
                season_sections.push_str(&format!(
                    r#"<section class="tv-season"><div class="tv-season-card">{season_card}</div></section>"#
                ));
                continue;
            }
        }
        let include_audio = library_kind.as_deref() == Some("audiobooks");
        let media_count = media_file_count(&directory, include_audio).await;
        if media_count > 0 {
            let preview = first_media_file(&directory, include_audio)
                .await
                .and_then(|path| path.strip_prefix(&library_root).ok().map(PathBuf::from));
            let cover = sidecar_mode
                && tokio::fs::metadata(directory.join(".hbgr/cover.jpg"))
                    .await
                    .is_ok();
            let banner = sidecar_mode
                && tokio::fs::metadata(directory.join(".hbgr/banner.jpg"))
                    .await
                    .is_ok();
            folder_cards.push_str(&folder_card(
                &name,
                &child_relative,
                query.library,
                query.folder,
                preview.as_deref(),
                cover,
                banner,
                None,
            ));
        }
    }
    for (name, child_relative, path) in files {
        has_content = true;
        if let Some((_, mime)) = web_media_type(&path) {
            let matched_media = web_media_for_relative(&s, &library_root, &child_relative).await;
            if tv_season_view && selected_relative.is_none() {
                let item_metadata = matched_media
                    .as_ref()
                    .and_then(|media| section_metadata.get(&media.id));
                let media_path = library_root.join(&child_relative);
                let (episode_title, episode_number) =
                    tv_episode_card_labels(&library_root, &media_path, item_metadata);
                let title = episode_title.unwrap_or_else(|| tv_episode_display_name(&name));
                let show_title = child_relative
                    .components()
                    .next()
                    .map(|component| component.as_os_str().to_string_lossy().into_owned());
                let folder = child_relative.parent().unwrap_or(std::path::Path::new(""));
                let library = query.library.unwrap_or_default();
                let href = format!(
                    "/?library={library}&path={}&episode={}",
                    url_encode(&folder.to_string_lossy()),
                    url_encode(&child_relative.to_string_lossy())
                );
                let progress = item_metadata.map(media_card_progress).unwrap_or_default();
                items.push_str(&media_cover_card(
                    matched_media.as_ref().map(|media| media.id.as_str()),
                    &href,
                    &title,
                    episode_number.as_deref(),
                    show_title.as_deref(),
                    progress,
                    None,
                    &child_relative,
                    library,
                    true,
                    true,
                    "item",
                    false,
                ));
                continue;
            }
            let artwork = matched_media
                .as_ref()
                .map(|media| media_cover_url(&media.id));
            let artwork = if artwork.is_some() {
                artwork
            } else if sidecar_mode
                && tokio::fs::metadata(
                    path.parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join(".hbgr/cover.jpg"),
                )
                .await
                .is_ok()
            {
                path.parent()
                    .and_then(|parent| parent.strip_prefix(&library_root).ok())
                    .map(|parent| artwork_url(parent, query.library, "cover"))
            } else {
                None
            };
            let subtitles = if let Some(media) = matched_media.as_ref() {
                subtitle_tracks(&s, media, &child_relative, query.library, query.folder).await
            } else {
                Vec::new()
            };
            let audio_tracks = if let Some(media) = matched_media.as_ref() {
                web_audio_tracks(&s, media).await
            } else {
                Vec::new()
            };
            let stream_selection = matched_media.as_ref().and_then(|media| {
                s.plex_stream_selections
                    .lock()
                    .ok()
                    .and_then(|selections| selections.get(&plex_part_id(media)).copied())
            });
            let selected_audio = stream_selection.and_then(|selection| selection.audio_stream_id);
            let selected_subtitle = matched_media
                .as_ref()
                .map(|media| selected_web_subtitle_stream_id(&s, media, &subtitles));
            let bilingual_stream_id = matched_media
                .as_ref()
                .filter(|_| bilingual_enabled)
                .map(|media| {
                    plex_translated_subtitle_id(media, &language_primary, &language_secondary)
                })
                .unwrap_or_default();
            let player_title = matched_media
                .as_ref()
                .and_then(|media| section_metadata.get(&media.id))
                .and_then(|metadata| metadata.get("mplex_episode_metadata"))
                .and_then(|metadata| metadata.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&name);
            let marker_json = matched_media
                .as_ref()
                .and_then(|media| {
                    s.db.lock()
                        .ok()
                        .and_then(|db| markers::markers_for_media(&db, &media.id).ok())
                })
                .and_then(|markers| serde_json::to_string(&markers).ok())
                .unwrap_or_else(|| "[]".to_owned());
            items.push_str(&web_client_item(
                player_title,
                &child_relative.to_string_lossy(),
                mime,
                query.library,
                query.folder,
                &subtitles,
                &audio_tracks,
                artwork,
                selected_audio,
                selected_subtitle,
                bilingual_enabled,
                bilingual_stream_id,
                &audio_language_primary,
                audio_language_secondary.as_deref(),
                &language_primary,
                &language_secondary,
                &marker_json,
                &skip_intro_mode,
                &skip_credits_mode,
            ));
        }
    }
    items.push_str(&folder_cards);
    if library_has_more {
        items.push_str(&format!(
            "<div id=\"library-sentinel\" data-next-offset=\"{}\" data-page-size=\"{}\" style=\"grid-column:1/-1;height:1px\"></div>",
            library_offset + WEB_LIBRARY_PAGE_SIZE,
            WEB_LIBRARY_PAGE_SIZE
        ));
    }
    items.push_str(&season_sections);
    if !has_content {
        items.push_str(r#"<p class="note">No supported media or subfolders here.</p>"#);
    }
    (
        [(header::CACHE_CONTROL, "no-store")],
        Html(add_build_info_footer(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="icon" href="/favicon.ico" sizes="any">
  <title>harbinger</title>
  <style>
    :root {{ color-scheme: dark; font-family: system-ui, sans-serif; }}
    html {{ -webkit-text-size-adjust: 100%; text-size-adjust: 100%; }}
    body {{ margin: 0; background: #111827; color: #f9fafb; }}
    a, button, summary, select {{ touch-action: manipulation; }}
    img, video {{ max-width: 100%; }}
    .app-shell {{ display: grid; grid-template-columns: 240px minmax(0, 1fr); min-height: 100vh; transition: grid-template-columns .2s ease; }}
    .app-shell.sidebar-collapsed {{ grid-template-columns: 68px minmax(0, 1fr); }}
    .sidebar {{ position: sticky; top: 0; height: 100vh; box-sizing: border-box; padding: 1.25rem .9rem; border-right: 1px solid #263244; background: #0d1421; }}
    .sidebar-brand {{ display: block; padding: .5rem .7rem 1.5rem; color: #f9fafb; font-size: 1.25rem; font-weight: 700; text-decoration: none; letter-spacing: .02em; }}
    .sidebar-top {{ display: flex; align-items: center; justify-content: space-between; gap: .4rem; padding-bottom: 1rem; }}
    .sidebar-top .sidebar-brand {{ padding-bottom: 0; }}
    .sidebar-toggle {{ display: grid; place-items: center; width: 2rem; height: 2rem; border: 0; border-radius: .4rem; color: #cbd5e1; background: transparent; cursor: pointer; font-size: 1.15rem; }}
    .sidebar-toggle:hover {{ background: #17243a; color: #fff; }}
    .sidebar-label {{ padding: 0 .7rem .45rem; color: #6b7280; font-size: .7rem; font-weight: 700; letter-spacing: .1em; text-transform: uppercase; }}
    .sidebar-nav {{ display: grid; gap: .25rem; }}
    .sidebar-link {{ display: flex; align-items: center; gap: .7rem; padding: .65rem .7rem; border-radius: .5rem; color: #cbd5e1; text-decoration: none; }}
    .sidebar-link:hover {{ background: #17243a; color: #fff; }}
    .sidebar-link.active {{ background: #1d4ed8; color: #fff; }}
    .sidebar-icon {{ width: 1.3rem; text-align: center; font-size: 1.1rem; }}
    .sidebar-library {{ overflow-wrap: anywhere; }}
    .sidebar-collapsed .sidebar-top {{ justify-content: center; }}
    .sidebar-collapsed .sidebar-brand, .sidebar-collapsed .sidebar-label, .sidebar-collapsed .sidebar-link {{ font-size: 0; }}
    .sidebar-collapsed .sidebar-toggle {{ transform: rotate(180deg); }}
    .sidebar-collapsed .sidebar-link {{ justify-content: center; padding-left: 0; padding-right: 0; }}
    .sidebar-collapsed .sidebar-icon {{ font-size: 1.1rem; }}
    .sidebar-settings-group {{ margin: 0; }}
    .sidebar-settings-group > summary {{ list-style: none; cursor: pointer; }}
    .sidebar-settings-group > summary::-webkit-details-marker {{ display: none; }}
    .sidebar-settings-group > summary::marker {{ content: ""; }}
    .sidebar-collapsed .sidebar-settings-group[open] .sidebar-subnav {{ display: none; }}
    .sidebar-collapsed .sidebar-chevron {{ display: none; }}
    main {{ min-width: 0; max-width: 1100px; width: 100%; box-sizing: border-box; margin: 0 auto; padding: 2rem 1rem; }}
    h1 {{ margin-top: 0; overflow-wrap: anywhere; }}
    .brand {{ color: inherit; text-decoration: none; }}
    .library {{ display: grid; grid-template-columns: repeat(auto-fill, minmax({}px, 1fr)); align-items: start; gap: 1rem; }}
    .media-card {{ position: relative; min-width: 0; padding: .7rem; border: 1px solid transparent; border-radius: .6rem; background: #1f2937; transition: border-color .15s, transform .15s; }}
    .media-card:hover {{ border-color: #374151; transform: translateY(-2px); }}
    .media-card-artwork {{ position: relative; }}
    .media-card-episode .media-card-cover, .media-card-episode .media-card-cover img {{ aspect-ratio: 16 / 9; }}
        .media-card-info {{ display: grid; gap: .12rem; padding: .55rem 0 0; color: inherit; text-align: center; text-decoration: none; }}
    .media-card-title {{ display: block; height: 2.4em; overflow: hidden; margin: .25rem 0 .15rem; font-size: .8rem; font-weight: 500; line-height: 1.2; }}
    .media-card-tv .media-card-title {{ height: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    .media-card-subtitle, .media-card-details {{ overflow: hidden; color: #9ca3af; font-size: .75rem; line-height: 1.35; text-overflow: ellipsis; white-space: nowrap; }}
    .media-card-progress-row {{ display: flex; align-items: center; gap: .45rem; margin: .3rem 0 0; }}
    .media-card-progress {{ flex: 1 1 auto; min-width: 0; height: .3rem; border-radius: 999px; background: #374151; overflow: hidden; }}
    .media-card-progress span {{ display: block; height: 100%; border-radius: inherit; background: linear-gradient(90deg, #2563eb, #22c55e); }}
    .media-card-percent {{ flex: 0 0 auto; color: #93c5fd; font-size: .68rem; line-height: 1; }}
    .media-card-menu {{ position: absolute; right: 0; bottom: 0; z-index: 4; }}
    .media-card-menu summary {{ display: grid; min-width: 1.5rem; padding: .2rem .4rem; place-items: center; border: 0; border-radius: .35rem 0 0 0; color: #fff; background: rgba(17,24,39,.8); font-size: .85rem; font-weight: 700; line-height: 1; list-style: none; cursor: pointer; }}
    .media-card-menu summary::-webkit-details-marker {{ display: none; }}
    .media-card-menu-items {{ position: absolute; right: 0; bottom: calc(100% + .35rem); display: grid; min-width: 12.5rem; gap: .2rem; padding: .35rem; border: 1px solid #374151; border-radius: .5rem; background: #111827; box-shadow: 0 8px 24px #0008; }}
    .media-card-menu-items button {{ padding: .55rem .65rem; border: 0; border-radius: .3rem; color: #e5e7eb; background: transparent; font: inherit; font-size: .78rem; text-align: left; cursor: pointer; }}
    .media-card-menu-items button:hover {{ background: #263244; }}
    .tv-season {{ min-width: 0; }}
    .tv-season h2 {{ margin: .75rem 0 .6rem; }}
    .tv-season-card {{ width: 100%; max-width: 100%; }}
    .breadcrumb {{ grid-column: 1 / -1; }}
    article, .folder-card {{ width: 100%; box-sizing: border-box; }}
    article {{ grid-column: span 4; min-width: 0; }}
    article {{ overflow: hidden; border: 1px solid #374151; border-radius: .75rem; background: #1f2937; }}
    .player-stage {{ position: relative; }}
    video {{ display: block; width: 100%; aspect-ratio: 16 / 9; margin: 0 auto; background: #000; }}
    .skip-controls {{ position: absolute; right: .9rem; bottom: 4.2rem; z-index: 3; display: flex; gap: .5rem; }}
    .skip-controls button[hidden] {{ display: none; }}
    .skip-controls button {{ padding: .55rem .8rem; border: 1px solid rgba(255,255,255,.3); border-radius: .45rem; color: #f9fafb; background: rgba(17,24,39,.88); font: inherit; font-weight: 700; cursor: pointer; }}
    .skip-controls button:hover, .skip-controls button:focus-visible {{ border-color: #93c5fd; background: #1d4ed8; }}
    body.theatre-mode .theatre-card .skip-controls {{ bottom: 5rem; }}
    .details {{ padding: .9rem 1rem 1rem; }}
    h2 {{ font-size: 1rem; margin: 0 0 .4rem; overflow-wrap: anywhere; }}
    .play-actions {{ display: flex; flex-wrap: wrap; align-items: center; gap: .5rem; margin: .7rem 0; }}
    .play-actions button, .subtitle-picker select, .audio-picker select {{ padding: .45rem .65rem; }}
    .match-form {{ display: flex; flex-wrap: wrap; gap: .4rem; margin-top: .6rem; }} .match-form input {{ min-width: 0; flex: 1 1 12rem; padding: .45rem .6rem; border: 1px solid #4b5563; border-radius: .35rem; color: #fff; background: #111827; }} .match-form button {{ padding: .45rem .65rem; }}
    .info-button {{ background: #374151; }}
    dialog {{ position: relative; max-width: min(760px, calc(100vw - 2rem)); width: 100%; color: #f9fafb; border: 1px solid #4b5563; border-radius: .75rem; background: #111827; }} dialog::backdrop {{ background: rgba(0,0,0,.7); }} dialog form {{ position: absolute; top: .75rem; right: .75rem; margin: 0; }} dialog form button {{ padding: .35rem .6rem; }} .info-grid {{ display: grid; gap: .45rem; max-height: 65vh; overflow: auto; }} .info-row {{ display: grid; grid-template-columns: minmax(140px, .4fr) 1fr; gap: .8rem; padding: .45rem 0; border-bottom: 1px solid #263244; }} .info-key {{ color: #9ca3af; }} .info-value {{ overflow-wrap: anywhere; }}
    .theatre-button {{ background: #4b5563; }}
    body.theatre-mode main {{ max-width: none; padding: 0; }}
    body.theatre-mode .library {{ display: block; }}
    body.theatre-mode article:not(.theatre-card), body.theatre-mode .breadcrumb, body.theatre-mode .folder-card {{ display: none; }}
    body.theatre-mode .theatre-card {{ width: 100%; min-height: 100vh; border: 0; border-radius: 0; }}
    body.theatre-mode .theatre-card video {{ width: 100%; max-height: calc(100vh - 8rem); aspect-ratio: 16 / 9; object-fit: contain; }}
    .subtitle-picker, .audio-picker {{ display: inline-flex; align-items: center; gap: .35rem; color: #cbd5e1; font-size: .9rem; }}
    .subtitle-picker select, .audio-picker select {{ border: 1px solid #4b5563; border-radius: .35rem; color: #fff; background: #111827; }}
    .subtitle-note, .audio-note {{ color: #9ca3af; font-size: .8rem; }}
    .subtitle-translation {{ display: flex; flex-wrap: wrap; align-items: end; gap: .45rem; margin-top: .55rem; }}
    .subtitle-translation label {{ display: grid; gap: .2rem; color: #cbd5e1; font-size: .75rem; }}
    .subtitle-translation select, .subtitle-translation button {{ padding: .4rem .55rem; border: 1px solid #4b5563; border-radius: .35rem; color: #fff; background: #111827; }}
    .translation-status {{ flex-basis: 100%; min-height: 1.2rem; color: #9ca3af; font-size: .8rem; }}
    .translation-status[role="alert"] {{ color: #fca5a5; font-weight: 600; }}
    .translation-source-label {{ flex-basis: 100%; color: #9ca3af; font-size: .75rem; }}
    .translation-progress {{ flex-basis: 100%; width: 100%; height: .65rem; accent-color: #22c55e; }}
    .translation-progress-label {{ flex-basis: 100%; color: #9ca3af; font-size: .75rem; }}
    .breadcrumb {{ display: flex; flex-wrap: wrap; align-items: center; gap: .35rem; box-sizing: border-box; color: #9ca3af; }}
    .breadcrumb a {{ padding: .35rem .55rem; border-radius: .35rem; text-decoration: none; }}
    .breadcrumb a:hover {{ background: #24344d; }}
    .breadcrumb-separator {{ color: #6b7280; }}
    .metadata-panel {{ grid-column: 1 / -1; min-width: 0; padding: 1.15rem 1.3rem; border: 1px solid rgba(55,65,81,.9); border-radius: .85rem; background: linear-gradient(145deg, rgba(31,41,55,.92), rgba(17,24,39,.9)); box-shadow: 0 16px 38px rgba(0,0,0,.28); backdrop-filter: blur(10px); }}
    .metadata-panel h2 {{ margin: 0 0 .55rem; font-size: clamp(1.15rem, 2vw, 1.45rem); letter-spacing: -.02em; }}
    .metadata-description {{ max-width: 75ch; color: #d7dde7; line-height: 1.6; }} .metadata-details {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: .7rem 1.2rem; }} .metadata-details:not(:empty) {{ margin-top: .9rem; padding-top: .85rem; border-top: 1px solid rgba(55,65,81,.65); }} .metadata-detail strong {{ display: block; margin-bottom: .15rem; color: #9ca3af; font-size: .72rem; letter-spacing: .04em; text-transform: uppercase; }}
    .metadata-panel-layout {{ grid-column: 1 / -1; display: grid; grid-template-columns: minmax(120px, 170px) minmax(0, 1fr); align-items: start; gap: 1.2rem; }}
    .metadata-panel-layout > .metadata-panel {{ grid-column: 2; }}
    .metadata-panel-layout.metadata-panel-layout-no-cover {{ grid-template-columns: minmax(0, 1fr); }}
    .metadata-panel-layout.metadata-panel-layout-no-cover > .metadata-panel {{ grid-column: 1; }}
    .metadata-panel-cover {{ display: block; width: 100%; aspect-ratio: 2 / 3; object-fit: cover; border: 1px solid rgba(148,163,184,.28); border-radius: .8rem; background: linear-gradient(135deg, #1e3a8a, #312e81); box-shadow: 0 18px 36px rgba(2,6,23,.55); }}
    .audiobook-detail {{ grid-column: 1 / -1; width: 100%; max-width: 100%; overflow: visible; border: 0; background: transparent; box-shadow: none; }}
    .audiobook-player-shell {{ display: flex; flex-direction: column; align-items: center; box-sizing: border-box; width: min(100%, 34rem); margin: 1.25rem auto 0; padding: 1.5rem; border: 1px solid rgba(148,163,184,.18); border-radius: 1rem; background: radial-gradient(circle at 50% 0%, rgba(59,130,246,.18), transparent 55%), #131c2b; }}
    .audiobook-player-cover {{ position: relative; width: min(100%, 18rem); }}
    .audiobook-player-cover img, .audiobook-cover-placeholder {{ display: block; width: 100%; aspect-ratio: 1; object-fit: cover; border: 1px solid rgba(148,163,184,.25); border-radius: .9rem; background: linear-gradient(135deg, #1e3a8a, #312e81); box-shadow: 0 18px 36px rgba(2,6,23,.55); }}
    .audiobook-cover-placeholder {{ display: grid; place-items: center; padding: .75rem; color: #bfdbfe; font-size: .8rem; text-align: center; }}
    .audiobook-player-cover .audiobook-toggle-button {{ position: absolute; top: 50%; left: 50%; z-index: 2; display: grid; place-items: center; width: 3.75rem; height: 3.75rem; padding: 0; transform: translate(-50%, -50%); border: 1px solid rgba(255,255,255,.65); border-radius: 999px; color: #fff; background: rgba(37,99,235,.94); box-shadow: 0 10px 24px rgba(2,6,23,.65); font: inherit; font-size: 1.25rem; cursor: pointer; }}
    .audiobook-player-cover .audiobook-toggle-button:hover {{ background: #2563eb; }}
    .audiobook-player-main {{ width: 100%; min-width: 0; margin-top: 1.1rem; }}
    .audiobook-player-heading {{ margin-bottom: .9rem; text-align: center; }}
    .audiobook-player-heading h3 {{ margin: .2rem 0 0; font-size: 1.25rem; line-height: 1.25; }}
    .audiobook-player-position {{ color: #93c5fd; font-size: .75rem; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }}
    .audiobook-transport {{ display: flex; flex-wrap: nowrap; justify-content: center; align-items: center; gap: .5rem; margin: 0 0 1rem; }}
    .audiobook-transport button {{ display: grid; place-items: center; min-width: 2.5rem; height: 2.5rem; padding: 0 .5rem; border: 1px solid rgba(148,163,184,.25); border-radius: 999px; color: #e5e7eb; background: #253247; font: inherit; font-weight: 700; cursor: pointer; }}
    .audiobook-transport button:disabled {{ opacity: .4; cursor: default; }}
    .audiobook-seek {{ width: 100%; accent-color: #60a5fa; }}
    .audiobook-times {{ display: flex; justify-content: space-between; margin-top: .25rem; color: #9ca3af; font-size: .75rem; font-variant-numeric: tabular-nums; }}
    .audiobook-player-options {{ display: flex; flex-wrap: nowrap; align-items: center; justify-content: center; gap: .75rem; margin-top: 1rem; color: #9ca3af; font-size: .8rem; }}
    .audiobook-player-options label {{ display: inline-flex; align-items: center; gap: .3rem; min-width: 0; white-space: nowrap; }}
    .audiobook-player-options select {{ min-width: 0; max-width: 8rem; padding: .3rem .45rem; border: 1px solid #4b5563; border-radius: .35rem; color: #fff; background: #111827; }}
    .audiobook-chapter-select {{ max-width: 10rem; }}
    .audiobook-volume {{ width: 5rem; accent-color: #60a5fa; }}
    .audiobook-status {{ min-height: 1rem; margin: .75rem 0 0; color: #fca5a5; font-size: .8rem; text-align: center; }}
    .audiobook-audio {{ display: none; }}
    .audiobook-info {{ margin-top: 1.25rem; padding: 1.25rem 1.5rem; border: 1px solid rgba(148,163,184,.18); border-radius: 1rem; background: #1f2937; }}
    .audiobook-info h3 {{ margin: 0 0 .5rem; }}
    .audiobook-actions {{ display: flex; flex-wrap: wrap; align-items: center; gap: .6rem; margin-top: 1rem; }}
    .audiobook-match {{ margin-top: 0; }}
    .audiobook-matched {{ flex: 0 1 auto; }}
    .audiobook-matched[open] {{ flex-basis: 100%; }}
    .audiobook-matched summary {{ display: inline-flex; align-items: center; gap: .35rem; padding: .4rem .6rem; border: 1px solid rgba(34,197,94,.45); border-radius: 999px; color: #86efac; background: rgba(22,101,52,.25); cursor: pointer; list-style: none; font-size: .8rem; }}
    .audiobook-matched summary::-webkit-details-marker {{ display: none; }}
    .audiobook-matched-check {{ color: #4ade80; font-weight: 700; }}
    .audiobook-match-detail {{ margin-top: .6rem; padding: .75rem .9rem; border: 1px solid rgba(34,197,94,.3); border-radius: .6rem; background: rgba(22,101,52,.12); }}
    .audiobook-match-detail dl {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: .5rem .9rem; margin: 0; }}
    .audiobook-match-detail dt {{ color: #9ca3af; font-size: .7rem; letter-spacing: .06em; text-transform: uppercase; }}
    .audiobook-match-detail dd {{ margin: .15rem 0 0; overflow-wrap: anywhere; }}
    .audiobook-description {{ max-width: 85ch; margin: 0; color: #d7dde7; line-height: 1.6; overflow-wrap: anywhere; white-space: pre-wrap; }}
    .audiobook-facts {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: .75rem 1rem; margin: 1rem 0 0; padding-top: .85rem; border-top: 1px solid rgba(148,163,184,.18); }}
    .audiobook-fact dt {{ color: #9ca3af; font-size: .7rem; letter-spacing: .06em; text-transform: uppercase; }}
    .audiobook-fact dd {{ margin: .15rem 0 0; overflow-wrap: anywhere; }}
    .audiobook-track-fact {{ grid-column: 1 / -1; }}
    .audiobook-track-list summary {{ color: #93c5fd; cursor: pointer; }}
    .audiobook-track-list ul {{ display: grid; gap: .3rem; margin: .5rem 0 0; padding: 0; list-style: none; }}
    .audiobook-track-list li {{ display: flex; justify-content: space-between; gap: .75rem; padding: .35rem .5rem; border-radius: .35rem; background: rgba(15,23,42,.55); font-size: .8rem; }}
    .audiobook-track-name {{ overflow-wrap: anywhere; }}
    .audiobook-track-meta {{ color: #9ca3af; white-space: nowrap; font-variant-numeric: tabular-nums; }}
    @media (max-width: 640px) {{ .metadata-panel-layout {{ grid-template-columns: minmax(84px, 26vw) minmax(0, 1fr); gap: .8rem; }} .metadata-panel {{ padding: 1rem; }} }}
    @media (max-width: 600px) {{
      .audiobook-player-shell {{ padding: 1rem; }}
      .audiobook-player-options {{ flex-wrap: wrap; gap: .5rem; font-size: .75rem; }}
      .audiobook-player-options select {{ max-width: 6.5rem; }}
      .audiobook-chapter-control {{ flex-basis: 100%; }}
      .audiobook-chapter-control select {{ width: 100%; max-width: none; }}
      .audiobook-volume {{ width: 4rem; }}
      .audiobook-transport {{ gap: .4rem; }}
      .audiobook-transport button {{ min-width: 2.75rem; height: 2.75rem; padding: 0 .45rem; }}
      .audiobook-seek {{ height: 2rem; }}
      .audiobook-player-heading h3 {{ font-size: 1.1rem; }}
      .audiobook-info {{ padding: 1rem; }}
    }}
    @media (max-width: 480px) {{ .metadata-panel-layout {{ grid-template-columns: 72px minmax(0, 1fr); gap: .65rem; }} .metadata-panel {{ padding: .85rem; }} }}
    .folder-card {{ display: block; width: 100%; overflow: hidden; color: inherit; text-decoration: none; border: 1px solid #374151; border-radius: .75rem; background: #1f2937; transition: border-color .15s, transform .15s; }}
    .folder-card:hover {{ border-color: #60a5fa; transform: translateY(-2px); }}
    .folder-preview {{ position: relative; display: grid; place-items: center; overflow: hidden; aspect-ratio: 2 / 3; color: #bfdbfe; font-size: 3rem; background: linear-gradient(135deg, #1e3a8a, #312e81 55%, #111827); }}
    .folder-preview-video {{ width: 100%; height: 100%; object-fit: cover; opacity: .85; }}
    .folder-preview-icon {{ position: absolute; inset: 0; display: grid; place-items: center; text-shadow: 0 2px 8px #000; pointer-events: none; }}
    .folder-cover {{ position: absolute; inset: 0; z-index: 1; width: 100%; height: 100%; object-fit: cover; }}
    .folder-info {{ padding: .9rem 1rem 1rem; }}
    .folder-info h2 {{ margin: 0 0 .25rem; }}
    a {{ color: #93c5fd; }}
    .note {{ color: #9ca3af; }}
    @media (max-width: 700px) {{
      .app-shell {{ display: block; }}
      .sidebar {{ position: static; height: auto; padding: .7rem; border-right: 0; border-bottom: 1px solid #263244; }}
      .sidebar-top {{ justify-content: flex-start; padding-bottom: .35rem; }}
      .sidebar-brand {{ display: inline-block; padding: .35rem .55rem; }}
      .sidebar-toggle {{ display: none; }}
      .sidebar-label {{ display: none; }}
      .sidebar-collapsed .sidebar-top {{ justify-content: flex-start; }}
      .sidebar-collapsed .sidebar-brand {{ font-size: 1.25rem; }}
      .sidebar-collapsed .sidebar-link {{ justify-content: flex-start; padding-left: .7rem; padding-right: .7rem; font-size: 1rem; }}
      .sidebar-collapsed .sidebar-icon {{ font-size: 1.1rem; }}
      .sidebar-nav {{ display: flex; flex-wrap: wrap; gap: .35rem; padding-bottom: .2rem; }}
      .sidebar-link {{ min-height: 2.75rem; white-space: nowrap; }}
      .sidebar-collapsed .sidebar-settings-group[open] .sidebar-subnav {{ display: grid; }}
      main {{ padding: 1rem .9rem max(2rem, calc(1.25rem + env(safe-area-inset-bottom, 0px))); }}
      h1 {{ font-size: clamp(1.65rem, 7vw, 2rem); line-height: 1.15; }}
      .library {{ gap: .7rem; }}
      .breadcrumb a {{ display: inline-flex; min-height: 2.5rem; align-items: center; padding: .45rem .6rem; }}
      .media-card {{ padding: .55rem; }}
      .media-card-menu summary {{ min-width: 2.5rem; min-height: 2.5rem; padding: .45rem; border-radius: .45rem 0 0 0; }}
      .media-card-menu-items {{ right: 0; min-width: 9rem; max-width: calc(100vw - 2rem); }}
      .media-card-menu-items button {{ min-height: 2.5rem; }}
      .info-row {{ grid-template-columns: minmax(0, 1fr); gap: .15rem; }}
      dialog {{ max-height: calc(100vh - 2rem); overflow: auto; }}
      dialog form button {{ min-width: 2.75rem; min-height: 2.75rem; }}
      .play-actions button, .subtitle-picker select, .audio-picker select, .match-form button, .skip-controls button {{ min-height: 2.75rem; }}
      input, select, textarea {{ font-size: 16px; }}
      article {{ grid-column: 1 / -1; }}
    }}
{SIDEBAR_MOBILE_CSS}
  </style>
</head>
<body>
  <div class="app-shell">
    {}
    <main{}>{}
    {}
    {}
    <section class="library">{}</section>
    <dialog id="info-modal"><form method="dialog"><button type="submit" aria-label="Close dialog" title="Close">×</button></form><h2>Technical media information</h2><div id="info-content" class="info-grid">Loading…</div></dialog>
    {}
    </main>
  </div>
  <script>
    const shell = window.document.querySelector(".app-shell");
    const sidebarToggle = window.document.querySelector(".sidebar-toggle");
    if (window.localStorage.getItem("mplex-sidebar-collapsed") === "true") shell.classList.add("sidebar-collapsed");
    sidebarToggle?.addEventListener("click", () => {{
      const collapsed = shell.classList.toggle("sidebar-collapsed");
      window.localStorage.setItem("mplex-sidebar-collapsed", collapsed ? "true" : "false");
      sidebarToggle.setAttribute("aria-expanded", String(!collapsed));
    }});
    const infoModal = document.querySelector('#info-modal');
    const infoContent = document.querySelector('#info-content');
    if (!window.__mplexInfoEscapeBound) {{
      window.__mplexInfoEscapeBound = true;
      document.addEventListener("keydown", event => {{
        if (event.key === "Escape") {{
          document.querySelector('#info-modal')?.close();
          document.querySelector('#tv-fix-match-dialog')?.close();
        }}
      }});
    }}
    function addInfoRow(parent, key, item) {{
      if (item && typeof item === "object") {{
        if (Array.isArray(item)) item.forEach((entry, index) => addInfoRow(parent, `${{key}} ${{index + 1}}`, entry));
        else Object.entries(item).forEach(([childKey, childValue]) => addInfoRow(parent, key ? `${{key}} / ${{childKey}}` : childKey, childValue));
        return;
      }}
      const row = document.createElement("div");
      row.className = "info-row";
      const label = document.createElement("span");
      label.className = "info-key";
      label.textContent = key.replaceAll("_", " ");
      const content = document.createElement("span");
      content.className = "info-value";
      content.textContent = String(item ?? "");
      row.append(label, content);
      parent.append(row);
      return row;
    }}
    function renderTechnicalInfo(value, append = false) {{
      if (!append) infoContent.replaceChildren();
      addInfoRow(infoContent, "", value);
    }}
    function formatMarkerTime(value) {{
      const seconds = Math.max(0, Math.round(Number(value) / 1000));
      return `${{Math.floor(seconds / 60)}}:${{String(seconds % 60).padStart(2, "0")}}`;
    }}
    function renderMarkerInfo(button) {{
      let markers = [];
      try {{ markers = JSON.parse(button.dataset.skipMarkers || "[]"); }} catch (_) {{ markers = []; }}
      if (!Array.isArray(markers) || markers.length === 0) {{
        addInfoRow(infoContent, "Markers", "None detected");
        return;
      }}
      markers.forEach(marker => {{
        const kind = marker && (marker.type === "credits" || marker.type === "credit") ? "Credits" : "Introduction";
        addInfoRow(infoContent, kind, `${{formatMarkerTime(marker.startTimeOffset)}}–${{formatMarkerTime(marker.endTimeOffset)}}`);
      }});
    }}
    document.querySelectorAll(".info-button").forEach(button => {{
      button.addEventListener("click", async () => {{
        infoContent.replaceChildren();
        renderMarkerInfo(button);
        const loadingRow = addInfoRow(infoContent, "Technical details", "Loading…");
        infoModal.showModal();
        try {{
          const response = await fetch(button.dataset.infoUrl, {{ cache: "no-store" }});
          if (response.ok) {{
            loadingRow.remove();
            renderTechnicalInfo(await response.json(), true);
          }} else {{
            loadingRow.querySelector(".info-value").textContent = "No cached technical analysis found. Run Scan metadata or Force rescan in Settings.";
          }}
        }} catch (_) {{
          loadingRow.querySelector(".info-value").textContent = "Media analysis could not be loaded.";
        }}
      }});
    }});
    document.querySelectorAll("video").forEach(video => {{
      video.volume = 1;
      video.muted = !video.controls;
      const capturePoster = () => {{
        if (video.poster || video.videoWidth === 0 || video.videoHeight === 0) return;
        const canvas = document.createElement("canvas");
        canvas.width = video.videoWidth;
        canvas.height = video.videoHeight;
        const context = canvas.getContext("2d");
        if (!context) return;
        context.drawImage(video, 0, 0, canvas.width, canvas.height);
        try {{ video.poster = canvas.toDataURL("image/jpeg", 0.78); }} catch (_) {{}}
      }};
      if (video.controls) {{
        video.addEventListener("loadeddata", capturePoster, {{ once: true }});
      }} else {{
        video.addEventListener("loadedmetadata", () => {{
          if (Number.isFinite(video.duration) && video.duration > 0) {{
            video.currentTime = video.duration * 0.1;
            video.addEventListener("seeked", capturePoster, {{ once: true }});
          }}
        }}, {{ once: true }});
      }}
      const path = video.dataset.progressPath;
      const library = video.dataset.library;
      const folder = video.dataset.folder;
      let skipMarkers = [];
      try {{ skipMarkers = JSON.parse(video.dataset.skipMarkers || "[]"); }} catch (_) {{ skipMarkers = []; }}
      if (!Array.isArray(skipMarkers)) skipMarkers = [];
      const skipModes = {{ intro: video.dataset.skipIntroMode || "manual", credits: video.dataset.skipCreditsMode || "manual" }};
      const skipHandled = new Set();
      const skipButtons = [...(video.closest("article")?.querySelectorAll(".skip-controls button") || [])];
      const markerKind = marker => marker && (marker.type === "credits" || marker.type === "credit") ? "credits" : "intro";
      const markerTarget = marker => Math.max(0, (Number(marker.endTimeOffset) / 1000) - 2);
      const setSkipButton = (kind, visible) => {{
        const button = skipButtons.find(candidate => candidate.dataset.skipKind === kind);
        if (button) button.hidden = !visible;
      }};
      const syncSkipMarkers = (allowAutomatic = !video.paused) => {{
        const current = Number.isFinite(video.currentTime) ? video.currentTime : 0;
        let visibleIntro = false;
        let visibleCredits = false;
        skipMarkers.forEach((marker, index) => {{
          const kind = markerKind(marker);
          const start = Number(marker.startTimeOffset) / 1000;
          const end = Number(marker.endTimeOffset) / 1000;
          if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) return;
          const key = String(marker.id ?? index);
          const inside = current >= start && current < end;
          const mode = skipModes[kind];
          if (inside && mode === "automatic" && allowAutomatic && !skipHandled.has(key)) {{
            skipHandled.add(key);
            const target = Math.max(start, markerTarget(marker));
            if (Math.abs(video.currentTime - target) > 0.1) video.currentTime = target;
            return;
          }}
          if (inside && mode === "manual" && !skipHandled.has(key)) {{
            if (kind === "intro") visibleIntro = true;
            else visibleCredits = true;
          }}
        }});
        setSkipButton("intro", visibleIntro);
        setSkipButton("credits", visibleCredits);
      }};
      skipButtons.forEach(button => button.addEventListener("click", () => {{
        const kind = button.dataset.skipKind;
        const marker = skipMarkers.find(candidate => markerKind(candidate) === kind
          && Number(candidate.startTimeOffset) / 1000 <= video.currentTime
          && Number(candidate.endTimeOffset) / 1000 >= video.currentTime);
        if (!marker) return;
        skipHandled.add(String(marker.id ?? skipMarkers.indexOf(marker)));
        video.currentTime = Math.max(Number(marker.startTimeOffset) / 1000, markerTarget(marker));
        button.hidden = true;
      }}));
      video.addEventListener("loadedmetadata", () => syncSkipMarkers(false));
      video.addEventListener("play", () => syncSkipMarkers(true));
      const audioSelect = video.closest("article")?.querySelector(".audio-select");
      const audioNote = video.closest("article")?.querySelector(".audio-note");
      const applyAudioTrack = trackIndex => {{
        if (!video.audioTracks) {{
          if (audioNote) audioNote.textContent = "This browser cannot switch embedded voice tracks.";
          return;
        }}
        [...video.audioTracks].forEach((track, index) => {{
          track.enabled = index === trackIndex;
        }});
        if (audioNote) audioNote.textContent = "";
      }};
      if (audioSelect) {{
        const applyServerAudio = streamId => {{
          const option = [...audioSelect.options].find(candidate => Number(candidate.dataset.streamId || 0) === streamId);
          if (!option) return;
          audioSelect.value = option.value;
          audioSelect.dataset.selectedStreamId = String(streamId);
          applyAudioTrack(Number(option.dataset.trackIndex ?? -1));
        }};
        applyServerAudio(Number(audioSelect.dataset.selectedStreamId || 0));
        audioSelect.addEventListener("change", event => {{
          const select = event.currentTarget;
          const option = select.selectedOptions[0];
          const streamId = Number(option?.dataset.streamId || 0);
          select.dataset.selectedStreamId = String(streamId);
          applyAudioTrack(Number(option?.dataset.trackIndex ?? -1));
          fetch("/web/audio-selection", {{
            method: "POST", headers: {{ "content-type": "application/json" }},
            body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null, stream_id: streamId }})
          }}).catch(() => {{
            if (audioNote) audioNote.textContent = "Could not save the voice selection.";
          }});
        }});
        video.addEventListener("loadedmetadata", () => applyAudioTrack(Number(audioSelect.selectedOptions[0]?.dataset.trackIndex ?? -1)), {{ once: true }});
      }}
      const subtitleSelect = video.closest("article")?.querySelector(".subtitle-select");
      let pendingBilingualNoticeVisible = false;
      let lastSubtitleValue = subtitleSelect?.value ?? "-1";
      let startPendingBilingual = null;
      let activeStreamingSourceKey = null;
      let activeStreamingSourceName = null;
      let updateTranslationSourceLabel = () => {{}};
      const applySubtitle = selected => {{
        [...video.textTracks].forEach((track, index) => {{
          track.mode = index === selected ? "showing" : "disabled";
        }});
        if (subtitleSelect) subtitleSelect.value = String(selected);
      }};
      const subtitleOptionForStream = streamId => [...(subtitleSelect?.options || [])]
        .find(option => Number(option.dataset.streamId || 0) === streamId);
      const applyServerSubtitle = streamId => {{
        const option = subtitleOptionForStream(streamId);
        if (!option) {{
          applySubtitle(-1);
          subtitleSelect.dataset.selectedStreamId = String(streamId);
          return;
        }}
        applySubtitle(Number(option.value));
        subtitleSelect.dataset.selectedStreamId = String(streamId);
      }};
      if (subtitleSelect) {{
        applyServerSubtitle(Number(subtitleSelect.dataset.selectedStreamId || 0));
        lastSubtitleValue = subtitleSelect.value;
        const subtitleQuery = new URLSearchParams({{ path }});
        if (library) subtitleQuery.set("library", library);
        if (folder) subtitleQuery.set("folder", folder);
        subtitleSelect.addEventListener("change", event => {{
          const select = event.currentTarget;
          const option = select.selectedOptions[0];
          if (option?.dataset.pendingBilingual === "true") {{
            if (startPendingBilingual) startPendingBilingual(select, option);
            return;
          }}
          lastSubtitleValue = String(select.value);
          const streamId = Number(option?.dataset.streamId || 0);
          applySubtitle(Number(select.value));
          select.dataset.selectedStreamId = String(streamId);
          fetch("/web/subtitle-selection", {{
            method: "POST", headers: {{ "content-type": "application/json" }},
            body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null, stream_id: streamId }})
          }}).catch(() => {{}});
        }});
        const subtitleSocketUrl = new URL("/web/subtitle-selection/socket", window.location.href);
        subtitleSocketUrl.protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        subtitleSocketUrl.search = subtitleQuery.toString();
        let subtitleSelectionSocket = null;
        let subtitleSelectionReconnectTimer = null;
        let subtitleSelectionSocketActive = true;
        const connectSubtitleSelectionSocket = () => {{
          if (!subtitleSelectionSocketActive || !subtitleSelect.isConnected) return;
          const socket = new WebSocket(subtitleSocketUrl);
          subtitleSelectionSocket = socket;
          socket.addEventListener("message", event => {{
            try {{
              const selection = JSON.parse(event.data);
              const streamId = Number(selection.streamId || 0);
              if (streamId !== Number(subtitleSelect.dataset.selectedStreamId || 0)) {{
                applyServerSubtitle(streamId);
              }}
            }} catch (_) {{}}
          }});
          socket.addEventListener("close", () => {{
            if (!subtitleSelectionSocketActive || subtitleSelectionSocket !== socket || !subtitleSelect.isConnected) return;
            subtitleSelectionReconnectTimer = window.setTimeout(connectSubtitleSelectionSocket, 3000);
          }});
        }};
        const stopSubtitleSelectionSocket = () => {{
          subtitleSelectionSocketActive = false;
          if (subtitleSelectionReconnectTimer) clearTimeout(subtitleSelectionReconnectTimer);
          subtitleSelectionSocket?.close();
          subtitleSelectionObserver.disconnect();
        }};
        const subtitleSelectionObserver = new MutationObserver(() => {{
          if (!subtitleSelect.isConnected) stopSubtitleSelectionSocket();
        }});
        subtitleSelectionObserver.observe(document.body, {{ childList: true, subtree: true }});
        connectSubtitleSelectionSocket();
        window.addEventListener("pagehide", stopSubtitleSelectionSocket, {{ once: true }});
        const addBrowserEmbeddedTracks = () => {{
          const servedTracks = new Set([...video.querySelectorAll("track")].map(element => element.track).filter(Boolean));
          [...video.textTracks].forEach((track, index) => {{
            if (servedTracks.has(track) || !["subtitles", "captions"].includes(track.kind)) return;
            if (subtitleSelect.querySelector(`option[data-browser-track="${{index}}"]`)) return;
            const option = document.createElement("option");
            option.value = String(index); option.dataset.streamId = "0"; option.dataset.browserTrack = String(index);
            option.textContent = `Embedded: ${{track.label || track.language || `Track ${{index + 1}}`}}`;
            subtitleSelect.append(option);
          }});
        }};
        video.addEventListener("loadedmetadata", addBrowserEmbeddedTracks, {{ once: true }});
        setTimeout(addBrowserEmbeddedTracks, 0);
      }}
      const translationControls = video.closest("article")?.querySelector(".subtitle-translation");
      if (translationControls) {{
        let translatedTrackUrl = null;
        let translatedTrack = null;
        let translatedOption = null;
        let translatedCueKeys = new Set();
        let translationPollTimer = null;
        let generationProgressTimer = null;
        let generationProgressPending = false;
        let latestTranslationRequest = null;
        const sourceLabel = translationControls.querySelector(".translation-source-label");
        const status = translationControls.querySelector(".translation-status");
        const clearPendingBilingualNotice = () => {{
          if (!pendingBilingualNoticeVisible || !status) return;
          pendingBilingualNoticeVisible = false;
          status.textContent = "";
          status.removeAttribute("role");
          status.setAttribute("aria-live", "polite");
        }};
        updateTranslationSourceLabel = () => {{
          const option = subtitleSelect?.selectedOptions[0];
          const selectedName = option?.textContent.trim() || "No subtitle selected";
          const selectedKey = option?.dataset.browserTrack !== undefined
            ? `embedded:${{option.dataset.browserTrack}}`
            : `stream:${{Number(option?.dataset.streamId || 0)}}`;
          if (activeStreamingSourceKey && activeStreamingSourceKey !== selectedKey) {{
            sourceLabel.textContent = `Selected: ${{selectedName}} · current translation still uses ${{activeStreamingSourceName}}`;
          }} else if (activeStreamingSourceKey) {{
            sourceLabel.textContent = `Live translation source: ${{activeStreamingSourceName}}`;
          }} else {{
            sourceLabel.textContent = `Selected translation source: ${{selectedName}}`;
          }}
        }};
        subtitleSelect?.addEventListener("change", clearPendingBilingualNotice);
        subtitleSelect?.addEventListener("change", updateTranslationSourceLabel);
        updateTranslationSourceLabel();
        const addTranslatedCues = (contents, track) => {{
          if (!track?.track) return;
          for (const block of contents.split(/\n\s*\n/)) {{
            const lines = block.split("\n").map(line => line.trim()).filter(Boolean);
            const timingIndex = lines.findIndex(line => line.includes("-->"));
            if (timingIndex < 0) continue;
            const [startText, endText] = lines[timingIndex].split("-->").map(value => value.trim().split(/\\s+/)[0]);
            const toSeconds = value => {{
              const parts = value.replace(",", ".").split(":");
              const seconds = Number(parts.pop());
              const minutes = Number(parts.pop() || 0);
              const hours = Number(parts.pop() || 0);
              return hours * 3600 + minutes * 60 + seconds;
            }};
            const text = lines.slice(timingIndex + 1).join("\n");
            const key = `${{startText}}|${{endText}}|${{text}}`;
            if (!text || translatedCueKeys.has(key)) continue;
            try {{
              track.track.addCue(new VTTCue(toSeconds(startText), toSeconds(endText), text));
              translatedCueKeys.add(key);
            }} catch (_) {{}}
          }}
        }};
        let pendingResumeStarted = false;
        const translate = async (prepared, sourceOptionOverride = null, waitingForBilingual = false, pendingOption = null, silentConflict = false) => {{
          const pendingOriginalText = pendingOption?.textContent;
          let ready = false;
          let bilingualReady = false;
          const status = translationControls.querySelector(".translation-status");
          const showTranslationError = message => {{
            status.setAttribute("role", "alert");
            status.setAttribute("aria-live", "assertive");
            status.textContent = message;
          }};
          if (waitingForBilingual) pendingBilingualNoticeVisible = true;
          status.setAttribute("role", "status");
          status.setAttribute("aria-live", "polite");
          const generationProgress = translationControls.querySelector(".translation-progress");
          const generationProgressLabel = translationControls.querySelector(".translation-progress-label");
          const selectedOption = sourceOptionOverride || subtitleSelect?.selectedOptions[0];
          if (selectedOption?.dataset.bilingual === "true") {{
            showTranslationError("This subtitle is already bilingual.");
            return;
          }}
          const sourceId = Number(selectedOption?.dataset.streamId || 0);
          const browserTrackIndex = selectedOption?.dataset.browserTrack;
          const sourceKey = browserTrackIndex !== undefined ? `embedded:${{browserTrackIndex}}` : `stream:${{sourceId}}`;
          const selectedSourceName = selectedOption?.textContent.trim() || "selected subtitle";
          let browserCues = [];
          let sourceLanguage = "und";
          if (browserTrackIndex !== undefined) {{
            const embeddedTrack = video.textTracks[Number(browserTrackIndex)];
            if (!embeddedTrack?.cues || embeddedTrack.cues.length === 0) {{
              showTranslationError("This browser has not loaded subtitle cues yet. Start playback or choose a listed server subtitle track, then try again.");
              return;
            }}
            sourceLanguage = embeddedTrack.language || "und";
            browserCues = [...embeddedTrack.cues].map(cue => ({{ start_ms: cue.startTime * 1000, end_ms: cue.endTime * 1000, text: cue.text }}));
          }} else if (!sourceId) {{ showTranslationError("Select a text subtitle track first."); return; }}
          if (activeStreamingSourceKey && (prepared || activeStreamingSourceKey !== sourceKey)) {{
            await fetch("/web/translation-stop", {{ method: "POST", headers: {{ "content-type": "application/json" }},
              body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null }}) }}).catch(() => {{}});
            activeStreamingSourceKey = null;
            activeStreamingSourceName = null;
          }}
          if (!prepared) {{
            activeStreamingSourceKey = sourceKey;
            activeStreamingSourceName = selectedSourceName;
          }} else if (subtitleSelect) {{
            subtitleSelect.disabled = true;
          }}
          updateTranslationSourceLabel();
          status.textContent = waitingForBilingual
            ? `Generating bilingual subtitles… showing ${{selectedSourceName}} until ready. Reload the bilingual subtitles if generation was interrupted.`
            : prepared ? `Generating from ${{selectedSourceName}}…` : `Translating from ${{selectedSourceName}}…`;
          translationControls.querySelectorAll("button").forEach(button => button.disabled = true);
          try {{
            latestTranslationRequest = {{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null,
                stream_id: sourceId, first_language: translationControls.querySelector(".translation-first").value,
                second_language: translationControls.querySelector(".translation-second").value,
                source_language: sourceLanguage, source_cues: browserCues, source_key: sourceKey, prepared }};
            if (prepared) {{
              generationProgress.hidden = false;
              generationProgress.value = 0;
              generationProgressLabel.textContent = "Starting bilingual subtitle generation…";
              const progressRequest = {{ path, library: latestTranslationRequest.library, folder: latestTranslationRequest.folder, stream_id: sourceId,
                first_language: latestTranslationRequest.first_language, second_language: latestTranslationRequest.second_language,
                source_key: sourceKey, total_cues: browserCues.length || null }};
              const refreshProgress = async () => {{
                if (generationProgressPending) return;
                generationProgressPending = true;
                try {{
                  const progressResponse = await fetch("/web/translation-progress", {{ method: "POST", headers: {{ "content-type": "application/json" }},
                    body: JSON.stringify(progressRequest), cache: "no-store" }});
                  if (!progressResponse.ok) return;
                  const progress = await progressResponse.json();
                  generationProgress.value = progress.percent;
                  generationProgressLabel.textContent = `${{progress.completed}} / ${{progress.total}} cues (${{progress.percent}}%)`;
                }} catch (_) {{}} finally {{ generationProgressPending = false; }}
              }};
              refreshProgress();
              generationProgressTimer = setInterval(refreshProgress, 750);
            }}
            const response = await fetch("/web/translated-subtitle", {{ method: "POST", headers: {{ "content-type": "application/json" }},
              body: JSON.stringify(latestTranslationRequest) }});
            if (!response.ok) {{
              const message = await response.text();
              const error = new Error(message || `Request failed with HTTP ${{response.status}}`);
              error.status = response.status;
              throw error;
            }}
            const initialContents = await response.text();
            if (translatedOption) translatedOption.remove();
            if (translatedTrack) translatedTrack.remove();
            if (translatedTrackUrl) URL.revokeObjectURL(translatedTrackUrl);
            translatedCueKeys = new Set();
            translatedTrackUrl = URL.createObjectURL(new Blob([prepared ? initialContents : "WEBVTT\n\n"], {{ type: "text/vtt" }}));
            const track = document.createElement("track"); track.kind = "subtitles";
            track.label = `${{translationControls.querySelector(".translation-first").selectedOptions[0].text}} + ${{translationControls.querySelector(".translation-second").selectedOptions[0].text}}`;
            track.src = translatedTrackUrl; track.default = true; video.append(track);
            translatedTrack = track;
            if (subtitleSelect) {{
              translatedOption = document.createElement("option");
              translatedOption.value = String(video.textTracks.length - 1);
              translatedOption.dataset.streamId = String(Number(subtitleSelect?.dataset.bilingualStreamId || 0));
              translatedOption.dataset.bilingual = "true";
              translatedOption.textContent = `${{track.label}} (Translated)`;
              subtitleSelect.append(translatedOption);
              subtitleSelect.value = translatedOption.value;
              subtitleSelect.dataset.selectedStreamId = translatedOption.dataset.streamId;
              lastSubtitleValue = translatedOption.value;
              if (waitingForBilingual) {{
                fetch("/web/subtitle-selection", {{
                  method: "POST", headers: {{ "content-type": "application/json" }},
                  body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null, stream_id: Number(translatedOption.dataset.streamId) }})
                }}).catch(() => {{}});
              }}
            }}
            track.addEventListener("load", () => {{
              const wasPlaying = !video.paused && !video.ended;
              const resumeAt = video.currentTime;
              track.track.mode = "showing";
              if (!prepared) {{
                addTranslatedCues(initialContents, track);
                if (translationPollTimer) clearInterval(translationPollTimer);
                translationPollTimer = setInterval(async () => {{
                  if (video.paused || video.ended || !latestTranslationRequest) return;
                  try {{
                    const update = await fetch("/web/translated-subtitle", {{ method: "POST", headers: {{ "content-type": "application/json" }}, body: JSON.stringify(latestTranslationRequest) }});
                    if (update.ok) addTranslatedCues(await update.text(), track);
                  }} catch (_) {{}}
                }}, 2500);
              }}
              if (prepared) {{
                video.pause();
                video.currentTime = resumeAt;
                if (wasPlaying) video.play().catch(() => {{}});
              }}
            }}, {{ once: true }});
            ready = true;
            if (waitingForBilingual) bilingualReady = true;
            status.textContent = waitingForBilingual
              ? "Bilingual subtitle ready and selected."
              : prepared ? "Bilingual subtitle generated and saved as SRT." : "Bilingual subtitles ready.";
            if (prepared) {{ generationProgress.value = 100; generationProgressLabel.textContent = "Complete · saved as .srt"; }}
          }} catch (error) {{
            if (error.status === 429) showTranslationError("Google Translate rate limit reached. Wait a while and try again.");
            else if (!(silentConflict && error.status === 409)) {{
              showTranslationError(`Translation unavailable: ${{error.message || "request failed"}}`);
            }}
            if (prepared) generationProgressLabel.textContent = "Generation stopped; completed cues remain staged for resume. Reload the bilingual subtitles to try again.";
          }} finally {{
            if (generationProgressTimer) clearInterval(generationProgressTimer);
            generationProgressTimer = null;
            generationProgressPending = false;
            if (pendingOption && pendingOriginalText) {{
              pendingOption.textContent = pendingOriginalText;
            }}
            if (!ready) pendingResumeStarted = false;
            if (ready) pendingBilingualNoticeVisible = false;
            if (prepared && subtitleSelect) subtitleSelect.disabled = false;
            translationControls.querySelectorAll("button").forEach(button => button.disabled = false);
          }}
        }};
        const resumePartialTranslation = async () => {{
          if (!subtitleSelect || pendingResumeStarted) return;
          const pendingOption = [...subtitleSelect.options].find(option => option.dataset.pendingBilingual === "true");
          const sourceOption = subtitleSelect.selectedOptions[0];
          if (!pendingOption || !sourceOption || sourceOption === pendingOption) return;
          if (sourceOption.dataset.bilingual === "true" || Number(sourceOption.dataset.streamId || 0) === 0) return;
          const sourceId = Number(sourceOption.dataset.streamId || 0);
          const sourceKey = sourceOption.dataset.browserTrack !== undefined
            ? `embedded:${{sourceOption.dataset.browserTrack}}`
            : `stream:${{sourceId}}`;
          const firstLanguage = translationControls.querySelector(".translation-first").value;
          const secondLanguage = translationControls.querySelector(".translation-second").value;
          const progressResponse = await fetch("/web/translation-progress", {{
            method: "POST", headers: {{ "content-type": "application/json" }}, cache: "no-store",
            body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null,
              stream_id: sourceId, first_language: firstLanguage, second_language: secondLanguage, source_key: sourceKey }})
          }}).catch(() => null);
          if (!progressResponse?.ok) return;
          const progress = await progressResponse.json().catch(() => null);
          if (!progress || progress.complete || progress.completed === 0 || progress.completed >= progress.total) return;
          pendingResumeStarted = true;
          await translate(true, sourceOption, true, pendingOption, true);
        }};
        video.addEventListener("play", resumePartialTranslation);
        startPendingBilingual = (select, pendingOption) => {{
          const status = translationControls.querySelector(".translation-status");
          const sourceOption = (() => {{
            const previousIndex = Number(lastSubtitleValue);
            const previous = previousIndex >= 0 ? select.options[previousIndex] : null;
            if (previous && previous !== pendingOption && previous.dataset.pendingBilingual !== "true") return previous;
            return [...select.options].find(option =>
              option !== pendingOption &&
              option.dataset.pendingBilingual !== "true" &&
              option.dataset.bilingual !== "true" &&
              Number(option.dataset.streamId || 0) !== 0
            );
          }})();
          if (!sourceOption) {{
            status.setAttribute("role", "alert");
            status.textContent = "Select a single-language subtitle first, then choose the bilingual option.";
            select.value = lastSubtitleValue;
            return;
          }}
          if (!pendingOption.dataset.originalLabel) pendingOption.dataset.originalLabel = pendingOption.textContent;
          pendingOption.textContent = pendingOption.dataset.originalLabel.replace(/\s*\(generate\)\s*$/, "") + " (generating…)";
          select.value = String(pendingOption.value);
          applySubtitle(Number(pendingOption.value));
          const bilingualTrack = video.querySelector(`track[src*="bilingual="]`);
          if (bilingualTrack) {{
            const src = new URL(bilingualTrack.getAttribute("src"), window.location.href);
            src.searchParams.set("_ts", Date.now().toString());
            bilingualTrack.src = src.pathname + src.search;
            bilingualTrack.track.mode = "showing";
          }}
          select.dataset.selectedStreamId = pendingOption.dataset.streamId || "0";
          lastSubtitleValue = String(pendingOption.value);
          fetch("/web/subtitle-selection", {{
            method: "POST", headers: {{ "content-type": "application/json" }},
            body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null, stream_id: Number(pendingOption.dataset.streamId || 0) }})
          }}).catch(() => {{}});
          status.setAttribute("role", "status");
          status.setAttribute("aria-live", "polite");
          status.textContent = "Generating bilingual subtitles… Reload subtitles for updates.";
          const sourceId = Number(sourceOption.dataset.streamId || 0);
          const browserTrackIndex = sourceOption.dataset.browserTrack;
          const sourceKey = browserTrackIndex !== undefined ? `embedded:${{browserTrackIndex}}` : `stream:${{sourceId}}`;
          let browserCues = [];
          let sourceLanguage = "und";
          if (browserTrackIndex !== undefined) {{
            const embeddedTrack = video.textTracks[Number(browserTrackIndex)];
            sourceLanguage = embeddedTrack?.language || "und";
            browserCues = embeddedTrack?.cues ? [...embeddedTrack.cues].map(cue => ({{ start_ms: cue.startTime * 1000, end_ms: cue.endTime * 1000, text: cue.text }})) : [];
          }}
          const showTranslationError = message => {{
            status.setAttribute("role", "alert");
            status.setAttribute("aria-live", "assertive");
            status.textContent = message;
          }};
          fetch("/web/translated-subtitle", {{
            method: "POST", headers: {{ "content-type": "application/json" }},
            body: JSON.stringify({{
              path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null,
              stream_id: sourceId, first_language: translationControls.querySelector(".translation-first").value,
              second_language: translationControls.querySelector(".translation-second").value,
              source_language: sourceLanguage, source_cues: browserCues, source_key: sourceKey, prepared: true
            }})
          }}).then(async response => {{
            if (response.ok) {{
              status.textContent = "Generating bilingual subtitles…";
              return;
            }}
            const message = await response.text();
            if (response.status === 429) showTranslationError("Google Translate rate limit reached. Wait a while and try again.");
            else showTranslationError(message || "Translation unavailable.");
          }}).catch(() => showTranslationError("Translation unavailable."));
        }};
        translationControls.querySelector(".translate-prepare").addEventListener("click", () => translate(true));
      }}
      if (!path) return;
      const article = video.closest("article");
      const resumeButton = article?.querySelector(".resume-button");
      const startButton = article?.querySelector(".start-button");
      const theatreButton = article?.querySelector(".theatre-button");
      theatreButton?.addEventListener("click", () => {{
        const active = document.body.classList.toggle("theatre-mode");
        article.classList.toggle("theatre-card", active);
        theatreButton.textContent = active ? "▣ Exit theatre" : "▣ Theatre";
        theatreButton.setAttribute("aria-label", active ? "Exit theatre mode" : "Theatre mode");
      }});
      const playAt = position => {{
        const start = () => {{
          video.currentTime = position;
          video.play().catch(() => {{}});
        }};
        if (video.readyState >= 1) start();
        else video.addEventListener("loadedmetadata", start, {{ once: true }});
      }};
      startButton?.addEventListener("click", () => playAt(0));
      resumeButton?.addEventListener("click", () => playAt(Number(resumeButton.dataset.position || 0)));
      const progressQuery = new URLSearchParams({{ path }});
      if (library) progressQuery.set("library", library);
      if (folder) progressQuery.set("folder", folder);
      fetch(`/web/progress?${{progressQuery.toString()}}`, {{ cache: "no-store" }})
        .then(response => response.ok ? response.json() : null)
        .then(progress => {{
          if (!progress || progress.watched || !Number.isFinite(progress.positionMs) || progress.positionMs <= 2) return;
          const position = progress.positionMs / 1000;
          if (resumeButton) {{
            resumeButton.hidden = false;
            resumeButton.dataset.position = String(position);
          }}
          const resume = () => {{ if (position < video.duration - 5) video.currentTime = position; }};
          if (video.readyState >= 1) resume();
          else video.addEventListener("loadedmetadata", resume, {{ once: true }});
        }}).catch(() => {{}});
      let saveTimer = null;
      let lastProgressSave = 0;
      const sendProgress = watched => {{
        lastProgressSave = Date.now();
        fetch("/web/progress", {{
          method: "POST", headers: {{ "content-type": "application/json" }},
          body: JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null, position_ms: Math.round(video.currentTime * 1000), duration_ms: Number.isFinite(video.duration) ? Math.round(video.duration * 1000) : null, watched }})
        }}).catch(() => {{}});
        if (resumeButton && video.currentTime > 2) {{
          resumeButton.hidden = false;
          resumeButton.dataset.position = String(video.currentTime);
        }}
      }};
      const save = watched => {{
        if (watched || video.paused) {{
          clearTimeout(saveTimer);
          saveTimer = null;
          sendProgress(watched);
          return;
        }}
        if (saveTimer !== null) return;
        const delay = Math.max(0, 5000 - (Date.now() - lastProgressSave));
        saveTimer = setTimeout(() => {{
          saveTimer = null;
          if (!video.paused) sendProgress(false);
        }}, delay);
      }};
      video.addEventListener("timeupdate", () => {{ syncSkipMarkers(); if (!video.paused) save(false); }});
      video.addEventListener("pause", () => save(false));
      const translationStopRequest = JSON.stringify({{ path, library: library ? Number(library) : null, folder: folder ? Number(folder) : null }});
      const cancelStreamingTranslation = () => fetch("/web/translation-stop", {{
        method: "POST", headers: {{ "content-type": "application/json" }}, body: translationStopRequest
      }}).catch(() => {{}});
      video.addEventListener("ended", () => {{ save(true); if (translationPollTimer) clearInterval(translationPollTimer); activeStreamingSourceKey = null; activeStreamingSourceName = null; updateTranslationSourceLabel(); cancelStreamingTranslation(); }});
      window.addEventListener("pagehide", () => {{
        if (translationPollTimer) clearInterval(translationPollTimer);
        activeStreamingSourceKey = null;
        activeStreamingSourceName = null;
        if (video.currentTime > 0 && navigator.sendBeacon) {{
          navigator.sendBeacon("/web/translation-stop", new Blob([
            translationStopRequest
          ], {{ type: "application/json" }}));
        }}
      }}, {{ once: true }});
    }});
    document.querySelectorAll("[data-audiobook-player]").forEach(player => {{
      const audio = player.querySelector(".audiobook-audio");
      const seek = player.querySelector(".audiobook-seek");
      const status = player.querySelector(".audiobook-status");
      const currentLabel = player.querySelector(".audiobook-current-time");
      const totalLabel = player.querySelector(".audiobook-total-time");
      const titleLabel = player.querySelector(".audiobook-current-title");
      const positionLabel = player.querySelector(".audiobook-player-position");
      const toggleButtons = [...player.querySelectorAll(".audiobook-toggle-button")];
      const chapterSelect = player.querySelector(".audiobook-chapter-select");
      const previousChapter = player.querySelector(".audiobook-previous-chapter");
      const nextChapter = player.querySelector(".audiobook-next-chapter");
      if (!audio || !seek) return;
      let tracks = [];
      let chapters = [];
      try {{ tracks = JSON.parse(player.dataset.tracks || "[]"); }} catch (_) {{ tracks = []; }}
      try {{ chapters = JSON.parse(player.dataset.chapters || "[]"); }} catch (_) {{ chapters = []; }}
      if (!tracks.length) {{
        if (status) status.textContent = "No playable audio files were found for this book.";
        return;
      }}
      const durations = tracks.map(track => Math.max(0, Number(track.duration) || 0));
      let duration = Math.max(0, Number(player.dataset.duration) || 0);
      if (duration <= 0) duration = durations.reduce((sum, value) => sum + value, 0);
      let trackIndex = 0;
      let pendingSeek = null;
      let resumed = false;
      let hasStarted = false;
      let pageClosing = false;
      let saveTimer = null;
      let lastSave = 0;
      const trackStart = index => durations.slice(0, index).reduce((sum, value) => sum + value, 0);
      const absoluteTime = () => trackStart(trackIndex) + (Number.isFinite(audio.currentTime) ? audio.currentTime : 0);
      const formatTime = seconds => {{
        const total = Math.max(0, Math.round(Number(seconds) || 0));
        const hours = Math.floor(total / 3600);
        const minutes = Math.floor((total % 3600) / 60);
        const remainder = total % 60;
        return hours > 0
          ? `${{hours}}:${{String(minutes).padStart(2, "0")}}:${{String(remainder).padStart(2, "0")}}`
          : `${{minutes}}:${{String(remainder).padStart(2, "0")}}`;
      }};
      const chapterAt = time => chapters.find(chapter => time >= Number(chapter.start) && time < Number(chapter.end));
      const update = () => {{
        const time = absoluteTime();
        if (duration <= 0 && Number.isFinite(audio.duration) && audio.duration > 0) {{
          duration = trackStart(trackIndex) + audio.duration;
        }}
        seek.max = String(Math.max(0, duration));
        seek.value = String(Math.max(0, Math.min(time, duration)));
        if (currentLabel) currentLabel.textContent = formatTime(time);
        if (totalLabel) totalLabel.textContent = formatTime(duration);
        const chapter = chapterAt(time);
        if (titleLabel) titleLabel.textContent = chapter ? chapter.title : (tracks[trackIndex].title || player.dataset.title || "Audiobook");
        if (positionLabel) positionLabel.textContent = chapters.length
          ? `Chapter ${{chapters.indexOf(chapter) + 1}} of ${{chapters.length}}`
          : `Track ${{trackIndex + 1}} of ${{tracks.length}}`;
        if (chapterSelect) chapterSelect.value = String(chapter ? chapters.indexOf(chapter) : 0);
        if (previousChapter) previousChapter.disabled = !chapters.length || (chapter && chapters.indexOf(chapter) === 0 && time < chapter.start + 3);
        if (nextChapter) nextChapter.disabled = !chapters.length || !chapter || chapters.indexOf(chapter) === chapters.length - 1;
        toggleButtons.forEach(button => {{ button.textContent = audio.paused ? "▶" : "❚❚"; }});
      }};
      const setTrack = (index, autoplay) => {{
        trackIndex = Math.max(0, Math.min(index, tracks.length - 1));
        audio.src = tracks[trackIndex].url;
        audio.load();
        if (autoplay) audio.play().catch(error => {{
          if (status) status.textContent = error && error.message ? error.message : "Playback could not start.";
        }});
        update();
      }};
      const seekTo = target => {{
        const wanted = Math.max(0, Math.min(duration || Number(seek.max) || 0, Number(target) || 0));
        let remaining = wanted;
        for (let index = 0; index < tracks.length; index += 1) {{
          const trackDuration = durations[index];
          if (remaining < trackDuration || index === tracks.length - 1 || trackDuration <= 0) {{
            if (index !== trackIndex) setTrack(index, false);
            pendingSeek = remaining;
            if (audio.readyState >= 1) {{
              audio.currentTime = Math.min(remaining, audio.duration || remaining);
              pendingSeek = null;
            }} else {{
              audio.addEventListener("loadedmetadata", () => {{
                if (pendingSeek !== null) {{
                  audio.currentTime = Math.min(pendingSeek, audio.duration || pendingSeek);
                  pendingSeek = null;
                }}
              }}, {{ once: true }});
            }}
            update();
            return;
          }}
          remaining -= trackDuration;
        }}
      }};
      const sendProgress = watched => {{
        const positionMs = Math.round(Math.min(absoluteTime(), duration || absoluteTime()) * 1000);
        fetch("/web/progress", {{
          method: "POST", headers: {{ "content-type": "application/json" }}, keepalive: true,
          body: JSON.stringify({{
            path: player.dataset.path,
            library: Number(player.dataset.library),
            folder: Number(player.dataset.folder),
            position_ms: watched ? 0 : positionMs,
            duration_ms: duration > 0 ? Math.round(duration * 1000) : null,
            watched
          }})
        }}).catch(() => {{}});
        lastSave = Date.now();
      }};
      const save = watched => {{
        if (watched || audio.paused) {{
          clearTimeout(saveTimer);
          saveTimer = null;
          sendProgress(watched);
          return;
        }}
        if (saveTimer !== null) return;
        const delay = Math.max(0, 5000 - (Date.now() - lastSave));
        saveTimer = setTimeout(() => {{ saveTimer = null; if (!audio.paused) sendProgress(false); }}, delay);
      }};
      const flushProgress = () => {{
        if (pageClosing || !hasStarted) return;
        pageClosing = true;
        sendProgress(false);
      }};
      window.addEventListener("pagehide", flushProgress);
      document.addEventListener("visibilitychange", () => {{
        if (document.visibilityState === "hidden") flushProgress();
        else pageClosing = false;
      }});
      const togglePlayback = () => {{
        if (!audio.paused) {{ audio.pause(); return; }}
        if (status) status.textContent = "";
        audio.play().catch(error => {{
          if (status) status.textContent = error && error.message ? error.message : "Playback could not start.";
        }});
      }};
      const moveChapter = step => {{
        const time = absoluteTime();
        const index = chapters.findIndex(chapter => time >= chapter.start && time < chapter.end);
        if (chapters.length === 0) return;
        const target = chapters[Math.max(0, Math.min(chapters.length - 1, (index < 0 ? 0 : index) + step))];
        if (target) seekTo(Number(target.start));
      }};
      seek.addEventListener("input", () => seekTo(seek.value));
      toggleButtons.forEach(button => button.addEventListener("click", togglePlayback));
      player.querySelector(".audiobook-skip-back")?.addEventListener("click", () => seekTo(absoluteTime() - 15));
      player.querySelector(".audiobook-skip-forward")?.addEventListener("click", () => seekTo(absoluteTime() + 30));
      previousChapter?.addEventListener("click", () => moveChapter(-1));
      nextChapter?.addEventListener("click", () => moveChapter(1));
      chapterSelect?.addEventListener("change", event => {{
        const option = event.target.selectedOptions[0];
        if (option) seekTo(Number(option.dataset.audiobookSeek || 0));
      }});
      player.querySelector(".audiobook-speed")?.addEventListener("change", event => {{
        audio.playbackRate = Number(event.target.value) || 1;
      }});
      player.querySelector(".audiobook-volume")?.addEventListener("input", event => {{
        audio.volume = Math.max(0, Math.min(1, Number(event.target.value)));
      }});
      audio.addEventListener("loadedmetadata", () => {{
        if (pendingSeek !== null) {{
          audio.currentTime = Math.min(pendingSeek, audio.duration || pendingSeek);
          pendingSeek = null;
        }}
        if (!resumed && Number(player.dataset.start || 0) > 2) {{
          resumed = true;
          seekTo(Number(player.dataset.start));
        }}
        update();
      }});
      audio.addEventListener("timeupdate", () => {{ update(); if (!audio.paused) save(false); }});
      audio.addEventListener("play", () => {{ hasStarted = true; update(); }});
      audio.addEventListener("pause", () => {{ update(); if (hasStarted && !pageClosing) save(false); }});
      audio.addEventListener("error", () => {{
        if (status) status.textContent = "This audio file could not be played. Try another track or check the server log.";
      }});
      audio.addEventListener("ended", () => {{
        if (trackIndex + 1 < tracks.length) {{ setTrack(trackIndex + 1, true); return; }}
        save(true);
        update();
      }});
      update();
    }});
    if (!window.__mplexMediaHotkeysBound) {{
      window.__mplexMediaHotkeysBound = true;
      window.__mplexSelectedVideo = null;
      document.addEventListener("pointerover", event => {{
        if (event.target instanceof HTMLVideoElement) window.__mplexSelectedVideo = event.target;
      }});
      document.addEventListener("focusin", event => {{
        if (event.target instanceof HTMLVideoElement) window.__mplexSelectedVideo = event.target;
      }});
      document.addEventListener("keydown", event => {{
      if (event.key === "Escape" && window.document.fullscreenElement) {{
        event.preventDefault();
        window.document.exitFullscreen().catch(() => {{}});
        return;
      }}
      if (event.target instanceof Element && event.target.closest("input, textarea, select, [contenteditable='true']")) return;
      const fullscreen = event.key.toLowerCase() === "f";
      const theatre = event.key.toLowerCase() === "t";
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight" && event.code !== "Space" && !fullscreen && !theatre) return;
      const video = window.__mplexSelectedVideo || window.document.querySelector("video");
      if (!video) return;
      event.preventDefault();
      if (theatre) {{
        video.closest("article")?.querySelector(".theatre-button")?.click();
        return;
      }}
      if (fullscreen) {{
        if (window.document.fullscreenElement) window.document.exitFullscreen();
        else video.requestFullscreen().catch(() => {{}});
        return;
      }}
      if (event.code === "Space") {{
        if (video.paused) video.play().catch(() => {{}});
        else video.pause();
        return;
      }}
      if (!Number.isFinite(video.duration)) return;
      const offset = event.key === "ArrowLeft" ? -5 : 5;
      video.currentTime = Math.max(0, Math.min(video.duration, video.currentTime + offset));
      }});
    }}
    window.__mplexInfiniteLibrary = Boolean(window.document.querySelector('#library-sentinel'));
    let refreshing = false;
    async function refreshLibrary() {{
      if (refreshing) return;
      if (window.__mplexInfiniteLibrary) return;
      if (window.document.visibilityState !== "visible") return;
      refreshing = true;
      try {{
        const response = await fetch(window.location.href, {{ cache: "no-store" }});
        if (!response.ok) {{
          window.__mplexShowConnected?.();
          if (response.status === 404) {{
            clearInterval(window.__mplexLibraryRefreshTimer);
            window.__mplexLibraryRefreshTimer = null;
          }}
          return;
        }}
        window.__mplexShowConnected?.();
        const document = new DOMParser().parseFromString(await response.text(), "text/html");
        const current = window.document.querySelector(".library");
        const next = document.querySelector(".library");
        const hasPlaybackState = [...window.document.querySelectorAll("video")]
          .some(video => !video.paused && !video.ended || video.currentTime > 0);
        const hasPlayer = Boolean(current?.querySelector("video[data-progress-path], [data-audiobook-player]"));
        const hasAudiobookPlayback = [...window.document.querySelectorAll(".audiobook-audio")]
          .some(audio => !audio.paused || audio.currentTime > 0);
        if (!hasPlaybackState && !hasPlayer && !hasAudiobookPlayback && current && next && current.innerHTML !== next.innerHTML) {{
          const currentCards = [...current.children];
          const nextCards = [...next.children];
          if (currentCards.length !== nextCards.length) {{
            current.replaceWith(next);
          }} else {{
            nextCards.forEach((nextCard, index) => {{
              const currentCard = currentCards[index];
              if (currentCard && currentCard.outerHTML !== nextCard.outerHTML) {{
                currentCard.replaceWith(nextCard);
              }}
            }});
          }}
          window.__mplexSelectedVideo = null;
        }}
      }} catch (_) {{
        window.__mplexShowConnectionLost?.("Lost connection to Harbinger.");
      }} finally {{
        refreshing = false;
      }}
    }}
    if (window.__mplexLibraryRefreshTimer) clearInterval(window.__mplexLibraryRefreshTimer);
    window.__mplexLibraryRefreshTimer = setInterval(refreshLibrary, 5000);
    const infiniteSentinel = window.document.querySelector('#library-sentinel');
    if (infiniteSentinel) {{
      let infiniteLoading = false;
      const loadMoreLibrary = async () => {{
        if (infiniteLoading) return;
        const nextOffset = infiniteSentinel.dataset.nextOffset;
        if (!nextOffset) return;
        infiniteLoading = true;
        try {{
          const url = new URL(window.location.href);
          url.searchParams.set("offset", nextOffset);
          const response = await fetch(url, {{ cache: "no-store" }});
          if (!response.ok) {{
            window.__mplexShowConnectionLost?.("Could not load more items.");
            return;
          }}
          const document = new DOMParser().parseFromString(await response.text(), "text/html");
          const nextLibrary = document.querySelector(".library");
          const nextSentinel = document.querySelector('#library-sentinel');
          const currentLibrary = window.document.querySelector(".library");
          if (!nextLibrary || !currentLibrary) return;
          const cards = [...nextLibrary.children].filter(element =>
            element.id !== "library-sentinel" && !element.matches("nav.breadcrumb"));
          cards.forEach(card => currentLibrary.insertBefore(card, infiniteSentinel));
          if (nextSentinel && nextSentinel.dataset.nextOffset) {{
            infiniteSentinel.dataset.nextOffset = nextSentinel.dataset.nextOffset;
          }} else {{
            infiniteSentinel.remove();
            observer.disconnect();
          }}
        }} catch (_) {{
          window.__mplexShowConnectionLost?.("Could not load more items.");
        }} finally {{
          infiniteLoading = false;
        }}
      }};
      const observer = new IntersectionObserver(entries => {{
        if (entries.some(entry => entry.isIntersecting)) loadMoreLibrary();
      }}, {{ rootMargin: "800px 0px" }});
      observer.observe(infiniteSentinel);
    }}
  </script>
  {}{}
</body>
</html>"#,
        POSTER_CARD_MIN_WIDTH,
        sidebar,
        pane_banner,
        format!("{}{}", global_search_markup(), global_search_top_style()),
        page_heading,
        info_panel,
        items,
        format!("{}{}", fix_match_dialog_markup(), book_cover_dialog_markup()),
        media_card_action_script(),
        spa_navigation_script()
        ))),
    )
        .into_response()
}

pub(crate) async fn web_subtitle(
    State(s): State<AppState>,
    Query(query): Query<WebPath>,
) -> Response {
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&s, query.library, query.folder);
    let path = match web_path(&s, &root, &relative).await {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    if query.bilingual.unwrap_or(false) {
        let Some(media) = web_media_for_relative(&s, &root, &relative).await else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let languages = s.config.lock().ok().and_then(|config| {
            config.bilingual_subtitles_enabled.then(|| {
                (
                    config.subtitle_language_primary.clone(),
                    config.subtitle_language_secondary.clone(),
                )
            })
        });
        let Some((first, second)) = languages else {
            return StatusCode::NOT_FOUND.into_response();
        };
        return translated_subtitle_stream_response(&s, &media, &first, &second, "vtt").await;
    }
    if let Some(stream) = query.stream {
        let stream_id = stream as u64;
        let Some(media) = media_snapshot(&s)
            .iter()
            .find(|media| media.path == path)
            .cloned()
        else {
            return StatusCode::NOT_FOUND.into_response();
        };
        return match embedded_subtitle_source(&s, &media, stream_id).await {
            Ok((srt, _language)) => (
                [(header::CONTENT_TYPE, "text/vtt; charset=utf-8")],
                srt_to_vtt(&srt),
            )
                .into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        };
    }
    let Ok(contents) = tokio::fs::read_to_string(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let vtt = match extension.as_str() {
        "vtt" => contents,
        "srt" => srt_to_vtt(&contents),
        "ass" => ass_to_vtt(&contents),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, "text/vtt; charset=utf-8")], vtt).into_response()
}

pub(crate) async fn media_info_panel(
    directory: &std::path::Path,
    stored_metadata: Option<&serde_json::Value>,
    prefer_config_metadata: bool,
    hide_episode_fields: bool,
    cover_artwork: Option<&str>,
) -> String {
    if prefer_config_metadata {
        if let Some(metadata) = stored_metadata {
            return metadata_panel_with_cover(
                render_metadata_info_panel(metadata, None),
                cover_artwork,
            );
        }
    }
    if let Some(value) = read_sidecar_info(directory).await {
        let title = value
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Media information");
        let description = value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                value
                    .get("metadata_json")
                    .and_then(|metadata| metadata.get("overview"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
            })
            .map(|value| format!(r#"<p class="metadata-description">{}</p>"#, html(value)))
            .unwrap_or_default();
        let mut details = render_sidecar_details(&value, hide_episode_fields);
        if hide_episode_fields {
            if let Some(metadata) = value.get("metadata_json") {
                details.push_str(&render_tv_show_metadata_details(metadata));
            }
        }
        return metadata_panel_with_cover(
            format!(
                r#"<section class="metadata-panel"><h2>{}</h2>{}<div class="metadata-details">{}</div></section>"#,
                html(title),
                description,
                details
            ),
            cover_artwork,
        );
    }
    stored_metadata
        .map(|metadata| {
            metadata_panel_with_cover(render_metadata_info_panel(metadata, None), cover_artwork)
        })
        .unwrap_or_default()
}

pub(crate) fn metadata_panel_with_cover(panel: String, cover_artwork: Option<&str>) -> String {
    let Some(cover_artwork) = cover_artwork.filter(|_| !panel.is_empty()) else {
        return panel;
    };
    format!(
        r#"<div class="metadata-panel-layout"><img class="metadata-panel-cover" src="{}" alt="Cover artwork" onerror="this.parentElement.classList.add('metadata-panel-layout-no-cover');this.remove()">{}</div>"#,
        html(cover_artwork),
        panel
    )
}

pub(crate) fn render_tv_show_metadata_details(metadata: &serde_json::Value) -> String {
    let mut details = Vec::new();
    if let Some(genres) = metadata
        .get("genres")
        .and_then(serde_json::Value::as_array)
        .map(|genres| {
            genres
                .iter()
                .filter_map(|genre| {
                    genre
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| genre.as_str())
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|genres| !genres.is_empty())
    {
        details.push(("genres", genres));
    }
    let content_rating = metadata
        .get("content_ratings")
        .and_then(|ratings| ratings.get("results"))
        .and_then(serde_json::Value::as_array)
        .and_then(|ratings| {
            ratings
                .iter()
                .find(|rating| {
                    rating.get("iso_3166_1").and_then(serde_json::Value::as_str) == Some("US")
                })
                .or_else(|| ratings.first())
        })
        .and_then(|rating| rating.get("rating"))
        .and_then(serde_json::Value::as_str)
        .filter(|rating| !rating.is_empty());
    if let Some(rating) = content_rating {
        details.push(("rating", rating.to_owned()));
    }
    details
        .into_iter()
        .map(|(key, value)| {
            format!(
                r#"<div class="metadata-detail"><strong>{}</strong><span>{}</span></div>"#,
                html(key),
                html(&value)
            )
        })
        .collect()
}

pub(crate) fn render_metadata_info_panel(
    metadata: &serde_json::Value,
    episode: Option<&TvEpisodeInfo>,
) -> String {
    let episode_metadata = episode.and_then(|episode| {
        metadata.get("mplex_episode_metadata").or_else(|| {
            metadata
                .get("episodes")
                .and_then(serde_json::Value::as_array)
                .and_then(|episodes| {
                    episodes.iter().find(|item| {
                        item.get("episode_number")
                            .and_then(serde_json::Value::as_u64)
                            == Some(episode.episode as u64)
                    })
                })
        })
    });
    let title = episode_metadata
        .and_then(|value| value.get("name").and_then(serde_json::Value::as_str))
        .or_else(|| {
            episode.and_then(|_| {
                metadata
                    .get("mplex_episode_title")
                    .and_then(serde_json::Value::as_str)
            })
        })
        .or_else(|| metadata.get("title").and_then(serde_json::Value::as_str))
        .or_else(|| metadata.get("name").and_then(serde_json::Value::as_str))
        .unwrap_or("Media information");
    let episode_description = episode_metadata
        .and_then(|value| value.get("overview").and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty());
    let season_description = metadata
        .get("mplex_season_description")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty());
    let show_description = metadata
        .get("mplex_show_description")
        .or_else(|| metadata.get("overview"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty());
    let description = if episode.is_some() {
        episode_description
            .or(season_description)
            .or(show_description)
    } else {
        show_description
    };
    let description = description
        .map(|value| format!(r#"<p class="metadata-description">{}</p>"#, html(value)))
        .unwrap_or_default();
    let mut details = Vec::new();
    if let Some(episode) = episode {
        details.push(("season", episode.season.to_string()));
        details.push(("episode", episode.episode.to_string()));
        if let Some(end) = episode.episode_end {
            details.push(("episode end", end.to_string()));
        }
    }
    for (key, label) in [
        ("release_date", "release date"),
        ("first_air_date", "first air date"),
        ("air_date", "air date"),
        ("runtime", "runtime"),
        ("vote_average", "rating"),
    ] {
        let value = episode_metadata
            .and_then(|episode| episode.get(key))
            .or_else(|| metadata.get(key));
        if let Some(value) = value {
            let rendered = match value {
                serde_json::Value::String(value) => value.clone(),
                serde_json::Value::Number(value) => value.to_string(),
                _ => String::new(),
            };
            if !rendered.is_empty() {
                details.push((label, rendered));
            }
        }
    }
    let details = details
        .into_iter()
        .map(|(key, value)| {
            format!(
                r#"<div class="metadata-detail"><strong>{}</strong><span>{}</span></div>"#,
                html(key),
                html(&value)
            )
        })
        .collect::<String>();
    format!(
        r#"<section class="metadata-panel"><h2>{}</h2>{}<div class="metadata-details">{}</div></section>"#,
        html(title),
        description,
        details
    )
}

pub(crate) fn render_season_description_panel(
    metadata: &serde_json::Value,
    season: u32,
    show_metadata: Option<&serde_json::Value>,
) -> String {
    let season_description = metadata
        .get("overview")
        .and_then(serde_json::Value::as_str)
        .filter(|description| !description.is_empty());
    let show_description = show_metadata
        .and_then(|metadata| metadata.get("overview"))
        .and_then(serde_json::Value::as_str)
        .filter(|description| !description.is_empty());
    let Some(description) = season_description.or(show_description) else {
        return String::new();
    };
    let title = metadata
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Season {season}"));
    let details = format!(
        r#"<div class="metadata-detail"><strong>season</strong><span>{season}</span></div>"#
    );
    format!(
        r#"<section class="metadata-panel"><h2>{}</h2><p class="metadata-description">{}</p><div class="metadata-details">{}</div></section>"#,
        html(&title),
        html(description),
        details
    )
}

pub(crate) async fn episode_info_panel(
    state: &AppState,
    root: &std::path::Path,
    relative: &std::path::Path,
) -> String {
    let path = root.join(relative);
    let Some(episode_info) = tv_episode_info(&root.to_path_buf(), &path) else {
        return String::new();
    };
    let media_index = media_snapshot(state);
    let show_directory = relative
        .components()
        .next()
        .map(|component| root.join(component.as_os_str()));
    let show_metadata = show_directory
        .as_deref()
        .and_then(|directory| matched_tv_show_metadata(state, &media_index, directory));
    let sidecar_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "media");
    let selected_metadata = media_index
        .iter()
        .find(|media| media.path == path)
        .and_then(|media| {
            let (library_id, library) = media_library(state, media)?;
            let root = library
                .folders
                .iter()
                .find(|folder| media.path.starts_with(folder))?;
            let media_key = database_media_key(library_id, &media.id);
            let group_path = metadata_group_path(root, &media.path, &library.kind);
            read_metadata_sidecar(state, &media_key, &group_path)
        });
    let season_directory = show_directory
        .as_deref()
        .map(|directory| tv_season_directory(directory, episode_info.season, &path))
        .unwrap_or_else(|| path.parent().unwrap_or(root).to_path_buf());
    let season_metadata = if sidecar_mode {
        read_sidecar_info(&season_directory)
            .await
            .and_then(|info| info.get("metadata_json").cloned())
    } else {
        None
    };
    let stored_metadata = season_metadata.as_ref().or(selected_metadata.as_ref());
    let episode_details = sidecar_mode
        .then(|| {
            media_index
                .iter()
                .find(|media| media.path == path)
                .and_then(|media| read_tv_episode_sidecar_metadata(root, media))
        })
        .flatten();
    let episode_metadata = episode_details.or_else(|| {
        stored_metadata.and_then(|metadata| {
            metadata.get("mplex_episode_metadata").cloned().or_else(|| {
                metadata
                    .get("episodes")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|episodes| {
                        episodes.iter().find(|item| {
                            item.get("episode_number")
                                .and_then(serde_json::Value::as_u64)
                                == Some(episode_info.episode as u64)
                        })
                    })
                    .cloned()
            })
        })
    });
    let season_description = season_metadata
        .as_ref()
        .and_then(|metadata| metadata.get("overview"))
        .cloned()
        .or_else(|| {
            selected_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mplex_season_description"))
                .cloned()
        });
    let mut panel_metadata = show_metadata.unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = panel_metadata.as_object_mut() {
        if let Some(description) = &season_description {
            object.insert("mplex_season_description".to_owned(), description.clone());
        }
        if let Some(episode_metadata) = episode_metadata {
            object.insert("mplex_episode_metadata".to_owned(), episode_metadata);
        }
    }
    render_metadata_info_panel(&panel_metadata, Some(&episode_info))
}

pub(crate) fn render_sidecar_details(
    value: &serde_json::Value,
    hide_episode_fields: bool,
) -> String {
    let mut display_value = value.clone();
    if let Some(object) = display_value.as_object_mut() {
        object.remove("technical");
    }
    toml::Value::try_from(display_value)
        .ok()
        .and_then(|value| {
            value
                .as_table()
                .map(|table| render_toml_details(table, hide_episode_fields))
        })
        .unwrap_or_default()
}

pub(crate) fn render_toml_details(
    table: &toml::map::Map<String, toml::Value>,
    hide_episode_fields: bool,
) -> String {
    table
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "metadata_json" | "description" | "technical" | "path" | "media_path"
            ) && !(hide_episode_fields
                && matches!(
                    key.as_str(),
                    "season" | "episode" | "episode_end" | "episode_title"
                ))
        })
        .map(|(key, value)| {
            let rendered = match value {
                toml::Value::Table(table) => render_toml_details(table, hide_episode_fields),
                toml::Value::Array(values) => values
                    .iter()
                    .map(render_toml_value)
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => render_toml_value(value),
            };
            format!(
                r#"<div class="metadata-detail"><strong>{}</strong><span>{}</span></div>"#,
                html(&key.replace('_', " ")),
                rendered
            )
        })
        .collect()
}

pub(crate) fn render_toml_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => html(value),
        toml::Value::Integer(value) => value.to_string(),
        toml::Value::Float(value) => value.to_string(),
        toml::Value::Boolean(value) => value.to_string(),
        toml::Value::Datetime(value) => html(&value.to_string()),
        toml::Value::Array(_) | toml::Value::Table(_) => html(&value.to_string()),
    }
}

pub(crate) async fn web_artwork(
    State(s): State<AppState>,
    Query(query): Query<WebPath>,
) -> Response {
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(kind) = query
        .kind
        .as_deref()
        .filter(|kind| matches!(*kind, "cover" | "banner"))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&s, query.library, query.folder);
    let requested_path = match web_path(&s, &root, &relative).await {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let library_index = query.library.unwrap_or_default();
    let library = s
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(library_index).cloned());
    let Some(library) = library else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if is_audiobook_library(&library.kind) && kind == "cover" {
        let catalog = crate::audiobookshelf::catalog::Catalog::load_async(&s).await;
        let item = catalog.find_item_path(library_index, &relative, &root, &requested_path);
        let Some(item) = item else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let cover = match item.cover_path.clone().filter(|path| path.is_file()) {
            Some(cover) => Some(cover),
            None => {
                crate::audiobookshelf::catalog::spawn_embedded_cover_extraction(
                    s.clone(),
                    library_index,
                    item.clone(),
                );
                None
            }
        };
        if let Some(cover) = cover {
            if let Ok(bytes) = tokio::fs::read(&cover).await {
                let content_type = crate::audiobookshelf::catalog::image_content_type(&cover);
                let cache_seconds = if query.version.is_some() {
                    31_536_000
                } else {
                    300
                };
                return image_response(content_type, bytes, cache_seconds);
            }
        }
        return StatusCode::NOT_FOUND.into_response();
    }
    let directory = if requested_path.is_dir() {
        requested_path
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let media_index = media_snapshot(&s);
    let group_media = media_index
        .iter()
        .filter(|media| media_in_library(media, &library))
        .filter(|media| {
            let group_directory =
                tv_episode_show_directory(&root, &media.path).unwrap_or_else(|| {
                    let group_path = metadata_group_path(&root, &media.path, &library.kind);
                    if group_path.is_file() {
                        group_path.parent().unwrap_or(&group_path).to_path_buf()
                    } else {
                        group_path
                    }
                });
            group_directory == directory
        })
        .collect::<Vec<_>>();
    let config_mode = s
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    let mut paths = Vec::new();
    if let Some(media) = group_media.first() {
        let key = database_media_key(library_index + 1, &media.id);
        paths.push(
            config_metadata_sidecar_path(&s, &key)
                .parent()
                .unwrap_or(s.subtitle_checkpoints_dir.as_path())
                .join(format!("{kind}.jpg")),
        );
    }
    paths.push(directory.join(".hbgr").join(format!("{kind}.jpg")));
    if !config_mode {
        paths.reverse();
    }
    for path in paths {
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let content_type = crate::audiobookshelf::catalog::image_content_type(&path);
            return ([(header::CONTENT_TYPE, content_type)], bytes).into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

pub(crate) async fn web_media_artwork(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
) -> Response {
    let filename = match kind.as_str() {
        "cover" => "cover.jpg",
        "banner" => "banner.jpg",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    plex_artwork(&state, &id, filename).await
}

pub(crate) async fn web_episode_artwork(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(media) = find_media(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((library_key, library)) = media_library(&state, &media) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(root) = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let sidecar_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "media");
    if let Some(path) = tv_episode_artwork_path(&state, library_key, root, &media, sidecar_mode) {
        if let Ok(bytes) = tokio::fs::read(path).await {
            return image_response("image/jpeg", bytes, 300);
        }
    }
    plex_artwork(&state, &id, "cover.jpg").await
}

pub(crate) async fn web_info(State(s): State<AppState>, Query(query): Query<WebPath>) -> Response {
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = selected_library_folder_root(&s, query.library, query.folder);
    let path = match web_path(&s, &root, &relative).await {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    let directory = if path.is_dir() {
        path
    } else {
        path.parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf()
    };
    let Some(info) = read_sidecar_info(&directory).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(technical) = info.get("technical") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(technical).into_response()
}

pub(crate) fn artwork_url(
    relative: &std::path::Path,
    library: Option<usize>,
    kind: &str,
) -> String {
    artwork_url_with_folder(relative, library, None, kind)
}

fn artwork_url_with_version(
    relative: &std::path::Path,
    library: Option<usize>,
    folder: Option<usize>,
    kind: &str,
    version: Option<i64>,
) -> String {
    let mut url = artwork_url_with_folder(relative, library, folder, kind);
    if let Some(version) = version {
        url.push_str(&format!("&v={version}"));
    }
    url
}

fn artwork_url_with_folder(
    relative: &std::path::Path,
    library: Option<usize>,
    folder: Option<usize>,
    kind: &str,
) -> String {
    let location_query = format!(
        "{}{}",
        library
            .map(|index| format!("library={index}&"))
            .unwrap_or_default(),
        folder
            .map(|index| format!("folder={index}&"))
            .unwrap_or_default()
    );
    format!(
        "/web/artwork?{}path={}&kind={}",
        location_query,
        url_encode(&relative.to_string_lossy()),
        kind
    )
}

pub(crate) fn web_client_item(
    name: &str,
    relative: &str,
    mime: &str,
    library: Option<usize>,
    folder: Option<usize>,
    subtitles: &[SubtitleTrack],
    audio_tracks: &[AudioTrack],
    artwork: Option<String>,
    selected_audio_stream_id: Option<u64>,
    selected_subtitle_stream_id: Option<u64>,
    bilingual_enabled: bool,
    bilingual_stream_id: u64,
    audio_language_primary: &str,
    audio_language_secondary: Option<&str>,
    language_primary: &str,
    language_secondary: &str,
    marker_json: &str,
    skip_intro_mode: &str,
    skip_credits_mode: &str,
) -> String {
    let stream_path = url_encode(relative);
    let library_query = format!(
        "{}{}",
        library
            .map(|index| format!("library={index}&"))
            .unwrap_or_default(),
        folder
            .map(|index| format!("folder={index}&"))
            .unwrap_or_default()
    );
    if mime.starts_with("audio/") {
        let stream_url = format!("/web/stream?{}path={}", library_query, stream_path);
        return format!(
            r#"<article>
  <div class="player-stage"><audio controls preload="auto" data-library="{}" data-folder="{}" data-progress-path="{}" src="{}">Your browser does not support HTML audio. <a href="{}">Open the stream</a>.</audio></div>
  <div class="details">
    <h2>{}</h2>
    <div class="play-actions"><a href="{}">Open stream</a></div>
  </div>
</article>"#,
            library.map(|index| index.to_string()).unwrap_or_default(),
            folder.map(|index| index.to_string()).unwrap_or_default(),
            html(relative),
            html(&stream_url),
            html(&stream_url),
            html(name),
            html(&stream_url),
        );
    }
    let tracks = subtitles
        .iter()
        .map(|track| {
            format!(
                "<track kind=\"subtitles\" label=\"{}\" src=\"{}\" />",
                html(&track.label),
                html(&track.url)
            )
        })
        .collect::<String>();
    let selected_audio_stream_id = selected_audio_stream_id.unwrap_or_default();
    let selected_subtitle_stream_id = selected_subtitle_stream_id.unwrap_or_default();
    let preferred_audio_stream_id = [Some(audio_language_primary), audio_language_secondary]
        .into_iter()
        .flatten()
        .find_map(|language| {
            audio_tracks
                .iter()
                .find(|track| track.language == language)
                .map(|track| track.stream_id)
        });
    let effective_audio_stream_id = if selected_audio_stream_id != 0 {
        selected_audio_stream_id
    } else {
        preferred_audio_stream_id
            .or_else(|| audio_tracks.first().map(|track| track.stream_id))
            .unwrap_or_default()
    };
    let audio_options = audio_tracks
        .iter()
        .map(|track| {
            format!(
                r#"<option value="{}" data-stream-id="{}" data-track-index="{}" data-language="{}"{}>{}</option>"#,
                track.index,
                track.stream_id,
                track.index,
                html(&track.language),
                if track.stream_id == effective_audio_stream_id {
                    " selected"
                } else {
                    ""
                },
                html(&track.label)
            )
        })
        .collect::<String>();
    let audio_selector = if audio_tracks.is_empty() {
        r#"<span class="audio-note">No embedded audio tracks detected.</span>"#.to_owned()
    } else {
        format!(
            r#"<label class="audio-picker">Voice <select class="audio-select" data-progress-path="{}" data-library="{}" data-selected-stream-id="{}" aria-label="Voice track"><option value="-1" data-stream-id="0" data-track-index="-1"{}>Default</option>{}</select></label><span class="audio-note" role="status" aria-live="polite"></span>"#,
            html(relative),
            library.map(|index| index.to_string()).unwrap_or_default(),
            effective_audio_stream_id,
            if effective_audio_stream_id == 0 {
                " selected"
            } else {
                ""
            },
            audio_options
        )
    };
    let subtitle_options = subtitles
        .iter()
        .enumerate()
        .map(|(index, track)| {
            format!(
                r#"<option value="{}" data-stream-id="{}" data-bilingual="{}"{} data-pending-bilingual="{}">{}</option>"#,
                index,
                track.stream_id,
                track.bilingual,
                if track.stream_id == selected_subtitle_stream_id {
                    " selected"
                } else {
                    ""
                },
                track.pending_bilingual,
                html(&track.label)
            )
        })
        .collect::<String>();
    let subtitle_picker = if subtitles.is_empty() {
        r#"<span class="subtitle-note">No external or embedded subtitles detected.</span>"#
            .to_owned()
    } else {
        format!(
            r#"<label class="subtitle-picker">Subtitles <select class="subtitle-select" data-progress-path="{}" data-library="{}" data-selected-stream-id="{}" data-bilingual-stream-id="{}"><option value="-1" data-stream-id="0"{}>Off</option>{}</select></label><span class="subtitle-note">External subtitle selection syncs with Plex clients.</span>"#,
            html(relative),
            library.map(|index| index.to_string()).unwrap_or_default(),
            selected_subtitle_stream_id,
            bilingual_stream_id,
            if selected_subtitle_stream_id == 0 {
                " selected"
            } else {
                ""
            },
            subtitle_options
        )
    };
    let track_selectors = format!("{}{}", audio_selector, subtitle_picker);
    let translation_controls = if bilingual_enabled {
        format!(
            r#"<div class="subtitle-translation"><label>First language <select class="translation-first">{}</select></label><label>Second language <select class="translation-second">{}</select></label><button type="button" class="translate-prepare">Generate bilingual subtitle</button><span class="translation-source-label" role="status" aria-live="polite"></span><progress class="translation-progress" max="100" value="0" hidden aria-label="Bilingual subtitle generation progress"></progress><span class="translation-progress-label" role="status" aria-live="polite"></span><span class="translation-status" role="status" aria-live="polite"></span></div>"#,
            translation_language_options(language_primary),
            translation_language_options(language_secondary)
        )
    } else {
        String::new()
    };
    let relative_path = PathBuf::from(relative);
    let info_path = relative_path.parent().unwrap_or(std::path::Path::new(""));
    let info_url = format!(
        "/web/info?{}path={}",
        library_query,
        url_encode(&info_path.to_string_lossy())
    );
    let poster = artwork
        .map(|url| format!(r#" poster="{}""#, html(&url)))
        .unwrap_or_default();
    format!(
        r#"<article>
  <div class="player-stage">
    <video controls preload="auto" data-library="{}" data-folder="{}" data-progress-path="{}" data-skip-markers="{}" data-skip-intro-mode="{}" data-skip-credits-mode="{}"{} src="/web/stream?{}path={}" type="{}">
      {}
      Your browser does not support HTML video. <a href="/web/stream?{}path={}">Open the stream</a>.
    </video>
    <div class="skip-controls" role="group" aria-label="Skip controls"><button type="button" class="skip-intro-button" data-skip-kind="intro" title="Skip to two seconds before the intro ends" aria-label="Skip intro" hidden>Skip Intro</button><button type="button" class="skip-credits-button" data-skip-kind="credits" title="Skip to two seconds before the credits end" aria-label="Skip credits" hidden>Skip Credits</button></div>
  </div>
  <div class="details">
    <h2>{}</h2>
    <div class="play-actions"><button type="button" class="resume-button" title="Resume from saved position" aria-label="Resume from saved position" hidden>↪ Resume</button><button type="button" class="start-button" title="Play from beginning" aria-label="Play from beginning">▶ Play from beginning</button><button type="button" class="theatre-button" title="Theatre mode" aria-label="Theatre mode">▣ Theatre</button><button type="button" class="info-button" data-info-url="{}" data-skip-markers="{}" title="Show media information" aria-label="Show media information">ⓘ Info</button>{}</div>{}
    <form class="match-form" method="post" action="/settings/media/scan"><input type="hidden" name="path" value="{}"><input type="hidden" name="library" value="{}"><input name="title" value="{}" aria-label="Metadata match title"><button type="submit">↻ Match metadata</button></form>
    <a href="/web/stream?{}path={}">Open stream</a>
  </div>
</article>"#,
        library.map(|index| index.to_string()).unwrap_or_default(),
        folder.map(|index| index.to_string()).unwrap_or_default(),
        html(relative),
        html(marker_json),
        html(skip_intro_mode),
        html(skip_credits_mode),
        poster,
        library_query,
        stream_path,
        mime,
        tracks,
        library_query,
        url_encode(relative),
        html(name),
        html(&info_url),
        html(&marker_json),
        track_selectors,
        translation_controls,
        html(relative),
        library.map(|index| index.to_string()).unwrap_or_default(),
        html(name),
        library_query,
        url_encode(relative)
    )
}

pub(crate) fn translation_language_options(selected: &str) -> String {
    [
        ("ar", "Arabic"),
        ("zh", "Chinese"),
        ("cs", "Czech"),
        ("da", "Danish"),
        ("nl", "Dutch"),
        ("en", "English"),
        ("fi", "Finnish"),
        ("fr", "French"),
        ("de", "German"),
        ("el", "Greek"),
        ("he", "Hebrew"),
        ("hi", "Hindi"),
        ("hu", "Hungarian"),
        ("id", "Indonesian"),
        ("it", "Italian"),
        ("ja", "Japanese"),
        ("ko", "Korean"),
        ("no", "Norwegian"),
        ("pl", "Polish"),
        ("pt", "Portuguese"),
        ("ro", "Romanian"),
        ("ru", "Russian"),
        ("es", "Spanish"),
        ("sv", "Swedish"),
        ("th", "Thai"),
        ("tr", "Turkish"),
        ("uk", "Ukrainian"),
        ("vi", "Vietnamese"),
    ]
    .iter()
    .map(|(code, name)| {
        format!(
            "<option value=\"{code}\" {}>{name}</option>",
            if *code == selected { "selected" } else { "" }
        )
    })
    .collect()
}

pub(crate) fn subtitle_language_options(selected: &str) -> String {
    let mut options = format!(
        "<option value=\"off\"{}>Off</option>",
        if selected == "off" { " selected" } else { "" }
    );
    options.push_str(&translation_language_options(selected));
    options
}

fn voice_language_options(selected: Option<&str>) -> String {
    let mut options = format!(
        "<option value=\"none\"{}>None</option>",
        if selected.is_none() { " selected" } else { "" }
    );
    options.push_str(&translation_language_options(selected.unwrap_or("")));
    options
}

pub(crate) fn sidebar_nav_for(
    state: &AppState,
    relative: &std::path::Path,
    selected_library: Option<usize>,
) -> String {
    let library_links = sidebar_library_links(state, selected_library);
    sidebar_nav_with_library_links(
        selected_library.is_none() && relative.as_os_str().is_empty(),
        false,
        &library_links,
    )
}

pub(crate) fn sidebar_nav_for_settings(state: &AppState, settings_tab: Option<&str>) -> String {
    let library_links = sidebar_library_links(state, None);
    sidebar_nav_markup(false, true, &library_links, settings_tab)
}

pub(crate) fn sidebar_library_links(state: &AppState, selected_library: Option<usize>) -> String {
    state
        .config
        .lock()
        .ok()
        .map(|config| {
            config
                .libraries
                .iter()
                .enumerate()
                .map(|(index, library)| {
                    format!(
                        "<a class=\"sidebar-link sidebar-library {}\" data-spa-link href=\"/?library={}\"><span class=\"sidebar-icon\">{}</span>{}</a>",
                        if selected_library == Some(index) {
                            "active"
                        } else {
                            ""
                        },
                        index,
                        library_kind_glyph(&library.kind),
                        html(&library.name)
                    )
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn sidebar_nav_with_settings(settings_active: bool) -> String {
    sidebar_nav_with_library_links(!settings_active, settings_active, "")
}

pub(crate) fn sidebar_nav_with_library_links(
    home_active: bool,
    settings_active: bool,
    library_links: &str,
) -> String {
    sidebar_nav_markup(home_active, settings_active, library_links, None)
}

fn sidebar_nav_markup(
    home_active: bool,
    settings_active: bool,
    library_links: &str,
    settings_tab: Option<&str>,
) -> String {
    let settings_tab = settings_tab.unwrap_or("libraries");
    let settings_group = format!(
        r#"<details class="sidebar-settings-group"{}>
  <summary class="sidebar-link sidebar-settings {}" aria-label="Settings" title="Settings"><span class="sidebar-icon">⚙</span><span class="sidebar-settings-label">Settings</span><span class="sidebar-chevron" aria-hidden="true">⌄</span></summary>
  <div class="sidebar-subnav">
    <a class="sidebar-sublink{}" href="/settings">Libraries</a>
    <a class="sidebar-sublink{}" href="/settings#plex-heading">Plex &amp; server</a>
    <a class="sidebar-sublink{}" href="/settings#metadata-heading">Metadata</a>
    <a class="sidebar-sublink{}" href="/settings?tab=dashboard">Dashboard</a>
    <a class="sidebar-sublink{}" href="/settings?tab=users">Users</a>
    <a class="sidebar-sublink{}" href="/settings?tab=logs">Logs</a>
  </div>
</details>"#,
        if settings_active { " open" } else { "" },
        if settings_active { "active" } else { "" },
        if settings_tab == "libraries" {
            " active"
        } else {
            ""
        },
        if settings_tab == "plex" {
            " active"
        } else {
            ""
        },
        if settings_tab == "metadata" {
            " active"
        } else {
            ""
        },
        if settings_tab == "dashboard" {
            " active"
        } else {
            ""
        },
        if settings_tab == "users" {
            " active"
        } else {
            ""
        },
        if settings_tab == "logs" {
            " active"
        } else {
            ""
        },
    );
    let markup = format!(
        r#"<aside class="sidebar">
  <div class="sidebar-top">
    <a class="sidebar-brand" data-spa-link href="/">harbinger</a>
    <button class="sidebar-mobile-toggle" type="button" aria-label="Open navigation" aria-expanded="false" aria-controls="app-sidebar-panel" title="Open navigation"></button>
    <button class="sidebar-toggle" type="button" aria-label="Collapse sidebar" aria-expanded="true" title="Collapse sidebar">‹</button>
  </div>
  <div class="sidebar-panel" id="app-sidebar-panel">
    <div class="sidebar-label">Browse</div>
    <nav class="sidebar-nav" aria-label="Main navigation">
      <a class="sidebar-link {}" data-spa-link href="/"><span class="sidebar-icon">⌂</span>Home</a>
      {}
      {}
    </nav>
    <form method="post" action="/auth/logout" style="margin-top:1rem"><button type="submit" class="sidebar-link" style="width:100%;border:0;background:transparent;text-align:left;cursor:pointer"><span class="sidebar-icon">↪</span>Sign out</button></form>
  </div>
</aside>{script}"#,
        if home_active && !settings_active {
            "active"
        } else {
            ""
        },
        library_links,
        settings_group,
        script = SIDEBAR_MOBILE_SCRIPT,
    );
    markup
}

fn global_search_markup() -> &'static str {
    r##"<style>.global-topbar{position:relative;display:flex;align-items:center;justify-content:flex-start;width:100%;min-height:0;margin:0 0 .65rem;padding:.2rem 0;border-bottom:0;background:transparent}.global-topbar-form{position:relative;width:100%}.global-topbar-form input{box-sizing:border-box;width:100%;min-height:2.15rem;padding:.35rem .65rem;border:1px solid #374151;border-radius:.5rem;color:#fff;background:rgba(17,24,39,.8)}@media(max-width:700px){.global-topbar{margin-bottom:.5rem}.global-topbar-form input{min-height:2.75rem;font-size:16px}}</style><header class="global-topbar"><form class="global-topbar-form" id="global-media-search-form" role="search" action="/" method="get"><input id="global-media-search" name="search" type="search" value="" placeholder="Search…" aria-label="Search all media" autocomplete="off"><div id="global-media-search-results" role="status" aria-live="polite" hidden style="position:absolute;top:calc(100% + .4rem);left:0;right:0;z-index:60;max-height:65vh;overflow:auto;padding:.55rem;border:1px solid #374151;border-radius:.55rem;background:#0b1020;box-shadow:0 12px 28px #0009"></div></form></header><script>
if (!window.__mplexGlobalSearchBound) {
  window.__mplexGlobalSearchBound = true;
  let searchTimer;
  let searchRequest = 0;
  document.addEventListener("input", event => {
    const input = event.target.closest?.("#global-media-search");
    if (!input) return;
    const results = document.querySelector("#global-media-search-results");
    if (!results) return;
    const query = input.value.trim();
    clearTimeout(searchTimer);
    if (!query) { results.replaceChildren(); results.hidden = true; return; }
    searchTimer = setTimeout(async () => {
      const request = ++searchRequest;
      results.hidden = false;
      results.textContent = "Searching…";
      try {
        const response = await fetch(`/web/search?query=${encodeURIComponent(query)}`, { cache: "no-store" });
        if (!response.ok) throw new Error("Search failed.");
        const markup = await response.text();
        if (request !== searchRequest) return;
        results.innerHTML = markup || '<p class="note">No matching media found.</p>';
      } catch (_) {
        if (request === searchRequest) results.textContent = "Search failed. Try again.";
      }
    }, 180);
  });
  document.addEventListener("focusout", event => {
    if (event.target.closest?.("#global-media-search-form")) return;
    document.querySelector("#global-media-search-results")?.setAttribute("hidden", "");
  });
  document.addEventListener("keydown", event => {
    if (event.key === "Escape") document.querySelector("#global-media-search-results")?.setAttribute("hidden", "");
  });
}
</script>"##
}

pub(crate) fn breadcrumb_nav(
    relative: &std::path::Path,
    query: &WebPath,
    selected_relative: Option<&std::path::Path>,
    library_kind: Option<&str>,
) -> String {
    if relative.as_os_str().is_empty() && query.season.is_none() && selected_relative.is_none() {
        return String::new();
    }
    let mut prefix = String::new();
    if let Some(library) = query.library {
        prefix.push_str(&format!("library={library}&"));
    }
    if let Some(folder) = query.folder {
        prefix.push_str(&format!("folder={folder}&"));
    }
    let href = |path: &std::path::Path, extra: &str| {
        format!(
            "/?{}path={}{}",
            prefix,
            url_encode(&path.to_string_lossy()),
            extra
        )
    };
    let mut crumbs = Vec::new();
    let mut current = PathBuf::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        crumbs.push((name.to_string_lossy().into_owned(), href(&current, "")));
    }

    if library_kind == Some("tv") {
        let season_from_episode = selected_relative.and_then(|selected| {
            selected
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .find_map(parse_tv_season_directory)
                .or_else(|| {
                    selected
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .and_then(|stem| tv_episode_marker(stem))
                        .and_then(|(season, _, _)| season)
                })
        });
        let season = query.season.or(season_from_episode);
        let directory_is_season = relative
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_tv_season_directory)
            .is_some();
        if let (Some(season), false) = (season, directory_is_season) {
            crumbs.push((
                format!("Season {season:02}"),
                href(relative, &format!("&season={season}")),
            ));
        }
    }

    let mut nav = String::from(r#"<nav class="breadcrumb" aria-label="Breadcrumb">"#);
    for (index, (label, link)) in crumbs.iter().enumerate() {
        if index > 0 {
            nav.push_str(r#"<span class="breadcrumb-separator">/</span>"#);
        }
        nav.push_str(&format!(
            r#"<a data-spa-link href="{}">{}</a>"#,
            link,
            html(label)
        ));
    }
    nav.push_str("</nav>");
    nav
}

pub(crate) fn folder_card(
    name: &str,
    relative: &std::path::Path,
    library: Option<usize>,
    folder: Option<usize>,
    preview: Option<&std::path::Path>,
    cover: bool,
    banner: bool,
    progress: Option<f64>,
) -> String {
    folder_card_with_badge(
        name, relative, library, folder, preview, cover, banner, progress, None, None,
    )
}

pub(crate) fn media_cover_url(media_id: &str) -> String {
    format!("/web/artwork/media/{}/cover", url_encode(media_id))
}

/// Serve image bytes with a short private cache. Versioned cover URLs may be
/// cached much longer because replacing the file changes the URL.
pub(crate) fn image_response(
    content_type: &'static str,
    bytes: Vec<u8>,
    cache_seconds: u32,
) -> Response {
    let value = if cache_seconds >= 86_400 {
        // Versioned URLs never need revalidation: a changed cover gets a new
        // URL, so the browser can keep this image until its cache evicts it.
        format!("private, max-age={cache_seconds}, immutable")
    } else {
        format!("private, max-age={cache_seconds}")
    };
    let cache = HeaderValue::from_str(&value)
        .unwrap_or_else(|_| HeaderValue::from_static("private, max-age=60"));
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (header::CACHE_CONTROL, cache),
        ],
        bytes,
    )
        .into_response()
}

pub(crate) fn media_card(
    media: &Media,
    library_config: &ConfigLibrary,
    metadata: Option<&serde_json::Value>,
    root: &std::path::Path,
    folder: &std::path::Path,
    media_path: &std::path::Path,
    library: usize,
    show_remove_from_continue: bool,
) -> String {
    let folder_index = library_config
        .folders
        .iter()
        .position(|folder| folder == root)
        .unwrap_or_default();
    let folder_query = (folder_index > 0)
        .then(|| format!("&folder={folder_index}"))
        .unwrap_or_default();
    let href = format!(
        "/?library={library}{folder_query}&path={}&play={}",
        url_encode(&folder.to_string_lossy()),
        url_encode(&media_path.to_string_lossy())
    );
    let tv = library_config.kind == "tv";
    let title = if tv {
        metadata
            .and_then(|item| item.get("name"))
            .and_then(serde_json::Value::as_str)
            .filter(|title| !title.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                media
                    .path
                    .strip_prefix(root)
                    .ok()?
                    .components()
                    .next()
                    .map(|component| component.as_os_str().to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| media.title.clone())
    } else {
        metadata
            .and_then(|item| item.get("title"))
            .and_then(serde_json::Value::as_str)
            .filter(|title| !title.is_empty())
            .unwrap_or(&media.title)
            .to_owned()
    };
    let (subtitle, details) = if tv {
        tv_episode_card_labels(root, &media.path, metadata)
    } else {
        metadata
            .and_then(|item| item.get("release_date"))
            .and_then(serde_json::Value::as_str)
            .and_then(release_year)
            .or_else(|| title_year(&media.title))
            .map(|year| year.to_string())
            .map(|year| (Some(year), None))
            .unwrap_or_default()
    };
    let progress = metadata.map(media_card_progress).unwrap_or_default();
    media_cover_card(
        Some(&media.id),
        &href,
        &title,
        subtitle.as_deref(),
        details.as_deref(),
        progress,
        None,
        media_path,
        library,
        tv,
        is_metadata_library(&library_config.kind),
        "item",
        show_remove_from_continue,
    )
}

pub(crate) fn tv_episode_card_labels(
    root: &std::path::Path,
    media_path: &std::path::Path,
    metadata: Option<&serde_json::Value>,
) -> (Option<String>, Option<String>) {
    let episode = tv_episode_info(&root.to_path_buf(), media_path);
    let tmdb_title = metadata
        .and_then(|item| item.get("mplex_episode_metadata"))
        .and_then(|details| details.get("name"))
        .and_then(serde_json::Value::as_str)
        .filter(|title| !title.is_empty())
        .map(str::to_owned);
    let episode_title = tmdb_title
        .or_else(|| {
            episode
                .as_ref()
                .and_then(|episode| episode.episode_title.clone())
        })
        .or_else(|| {
            let metadata_matches = episode.as_ref().is_some_and(|episode| {
                metadata.is_some_and(|item| {
                    item.get("mplex_season").and_then(serde_json::Value::as_u64)
                        == Some(episode.season as u64)
                        && item
                            .get("mplex_episode")
                            .and_then(serde_json::Value::as_u64)
                            == Some(episode.episode as u64)
                })
            });
            metadata_matches
                .then(|| {
                    metadata
                        .and_then(|item| item.get("mplex_episode_title"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|title| !title.is_empty())
                        .map(str::to_owned)
                })
                .flatten()
        });
    let episode_number =
        episode.map(|episode| format!("S{} · E{}", episode.season, episode.episode));
    (episode_title, episode_number)
}

pub(crate) fn release_year(date: &str) -> Option<u32> {
    let year = date.get(..4)?.parse::<u32>().ok()?;
    (1888..=2100).contains(&year).then_some(year)
}

pub(crate) fn media_progress_markup(progress: f64) -> String {
    let progress = progress.clamp(0.0, 1.0);
    let percent = (progress * 100.0).round() as u32;
    format!(
        r#"<div class="media-card-progress-row" style="display:flex;align-items:center;gap:.45rem;grid-row:4;height:1rem"><div class="media-card-progress" role="progressbar" aria-label="Playback progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow="{percent}" style="flex:1;height:5px;margin:0"><span style="width:{:.2}%"></span></div><span class="media-card-percent" style="flex:0 0 auto;color:#9ca3af;font-size:.68rem;line-height:1">{percent}%</span></div>"#,
        progress * 100.0
    )
}

pub(crate) fn media_card_progress(metadata: &serde_json::Value) -> f64 {
    if metadata
        .get("mplex_view_count")
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|count| count > 0)
    {
        return 1.0;
    }
    let offset = metadata
        .get("mplex_view_offset")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let duration = metadata
        .get("mplex_duration")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    if duration > 0 {
        (offset as f64 / duration as f64).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(crate) fn fix_match_dialog_markup() -> &'static str {
    r#"<style>
#tv-fix-match-dialog { position: fixed; inset: 0; width: min(760px, calc(100vw - 2rem)); max-height: calc(100vh - 2rem); margin: auto; padding: 1.25rem; overflow: auto; color: #f9fafb; border: 1px solid #4b5563; border-radius: .75rem; background: #111827; }
#tv-fix-match-dialog::backdrop { background: rgba(0,0,0,.7); }
#tv-fix-match-dialog h2 { padding-right: 2.5rem; }
.fix-match-close { position: absolute; top: .65rem; right: .65rem; min-width: 2.4rem; padding: .3rem .55rem; }
.fix-match-search-row { display: flex; flex-wrap: wrap; gap: .5rem; margin: .9rem 0; }
.fix-match-search-row input { flex: 1 1 16rem; min-width: 0; padding: .6rem .7rem; border: 1px solid #4b5563; border-radius: .4rem; color: #fff; background: #0b1220; font: inherit; }
.fix-match-provider { padding: .6rem .7rem; border: 1px solid #4b5563; border-radius: .4rem; color: #fff; background: #0b1220; font: inherit; }
.fix-match-results { display: grid; gap: .65rem; }
.fix-match-result { display: flex; align-items: flex-start; gap: .9rem; padding: .75rem; border: 1px solid #374151; border-radius: .5rem; background: #1f2937; }
.fix-match-cover { flex: 0 0 76px; width: 76px; height: 114px; object-fit: cover; border-radius: .3rem; background: #111827; }
.fix-match-result-content { min-width: 0; }
.fix-match-result h3 { margin: 0; }
.fix-match-result p { margin: .35rem 0 .65rem; color: #cbd5e1; font-size: .85rem; line-height: 1.45; }
.fix-match-status { min-height: 1.5rem; color: #cbd5e1; }
@media (max-width: 420px) { .fix-match-cover { flex-basis: 60px; width: 60px; height: 90px; } .fix-match-result { gap: .6rem; } }
</style><dialog id="tv-fix-match-dialog" aria-labelledby="tv-fix-match-heading"><button class="fix-match-close" type="button" aria-label="Close Fix Match dialog" title="Close">×</button><h2 id="tv-fix-match-heading">Fix TV Show Match</h2><p id="tv-fix-match-show" class="note"></p><div class="fix-match-search-row"><select id="tv-fix-match-provider" class="fix-match-provider" aria-label="Metadata provider" hidden><option value="audible">Audible</option><option value="itunes">iTunes</option><option value="google">Google Books</option><option value="openlibrary">OpenLibrary</option><option value="fantlab">FantLab</option></select><input id="tv-fix-match-query" type="search" aria-label="Search TV shows" autocomplete="off"><input id="tv-fix-match-author" type="search" aria-label="Author (optional)" placeholder="Author (optional)" autocomplete="off" hidden><button id="tv-fix-match-search" type="button">Search</button></div><p id="tv-fix-match-status" class="fix-match-status" role="status" aria-live="polite"></p><div id="tv-fix-match-results" class="fix-match-results"></div></dialog>"#
}

pub(crate) fn book_cover_dialog_markup() -> &'static str {
    r#"<style>
#book-cover-dialog { position: fixed; inset: 0; width: min(760px, calc(100vw - 2rem)); max-height: calc(100vh - 2rem); margin: auto; padding: 1.25rem; overflow: auto; color: #f9fafb; border: 1px solid #4b5563; border-radius: .75rem; background: #111827; }
#book-cover-dialog::backdrop { background: rgba(0,0,0,.7); }
.book-cover-close { position: absolute; top: .65rem; right: .65rem; min-width: 2.4rem; padding: .3rem .55rem; }
.book-cover-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(110px, 1fr)); gap: .7rem; margin-bottom: 1rem; }
.book-cover-choice { display: grid; gap: .35rem; padding: .4rem; border: 1px solid #374151; border-radius: .5rem; color: #e5e7eb; background: #1f2937; font: inherit; font-size: .72rem; text-align: left; cursor: pointer; overflow-wrap: anywhere; }
.book-cover-choice:hover { border-color: #60a5fa; }
.book-cover-choice img { display: block; width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: .35rem; background: #0b1220; }
.book-cover-url-row { display: flex; flex-wrap: wrap; gap: .5rem; margin: 1rem 0; }
.book-cover-url-row input { flex: 1 1 18rem; min-width: 0; padding: .6rem .7rem; border: 1px solid #4b5563; border-radius: .4rem; color: #fff; background: #0b1220; font: inherit; }
#book-cover-url-use, #book-cover-upload { padding: .6rem .8rem; }
.book-cover-status { min-height: 1.4rem; color: #cbd5e1; }
</style><dialog id="book-cover-dialog" aria-labelledby="book-cover-heading"><button class="book-cover-close" type="button" aria-label="Close cover dialog" title="Close">×</button><h2 id="book-cover-heading">Choose cover</h2><p id="book-cover-status" class="book-cover-status" role="status" aria-live="polite"></p><h3>Stored in .hbgr</h3><div id="book-cover-stored" class="book-cover-grid"></div><h3>Embedded in audiobook</h3><div id="book-cover-embedded" class="book-cover-grid"></div><h3>Images in book folder</h3><div id="book-cover-folder" class="book-cover-grid"></div><div class="book-cover-url-row"><input id="book-cover-url" type="url" placeholder="https://example.com/cover.jpg" aria-label="Cover image URL"><button id="book-cover-url-use" type="button">Use URL</button><button id="book-cover-upload" type="button">Upload image</button></div><input id="book-cover-upload-input" type="file" accept="image/png,image/jpeg,image/webp" hidden></dialog>"#
}

pub(crate) fn media_card_action_script() -> &'static str {
    r##"<script>
window.__mplexCardActionsCleanup?.();
const cardActionListeners = [];
const bindCardAction = (element, type, handler, options) => {
  if (!element) return;
  element.addEventListener(type, handler, options);
  cardActionListeners.push(() => element.removeEventListener(type, handler, options));
};
const replaceBrokenCoverImage = image => {
  if (!(image instanceof HTMLImageElement) || !image.matches(".media-card-cover img")) return;
  const placeholder = document.createElement("div");
  const title = image.alt.replace(/^Cover for /, "");
  placeholder.className = "media-cover-placeholder";
  placeholder.setAttribute("role", "img");
  placeholder.setAttribute("aria-label", `Cover unavailable for ${title}`);
  placeholder.textContent = "No cover available";
  image.replaceWith(placeholder);
};
bindCardAction(document, "error", event => replaceBrokenCoverImage(event.target), true);
document.querySelectorAll(".media-card-cover img").forEach(image => {
  if (image.complete && image.naturalWidth === 0) replaceBrokenCoverImage(image);
});
const fixMatchDialog = document.querySelector("#tv-fix-match-dialog");
const fixMatchProvider = document.querySelector("#tv-fix-match-provider");
const fixMatchQuery = document.querySelector("#tv-fix-match-query");
const fixMatchAuthor = document.querySelector("#tv-fix-match-author");
const bookCoverDialog = document.querySelector("#book-cover-dialog");
const bookCoverStatus = document.querySelector("#book-cover-status");
const bookCoverStored = document.querySelector("#book-cover-stored");
const bookCoverEmbedded = document.querySelector("#book-cover-embedded");
const bookCoverFolder = document.querySelector("#book-cover-folder");
const bookCoverUrl = document.querySelector("#book-cover-url");
const bookCoverUrlUse = document.querySelector("#book-cover-url-use");
const bookCoverUpload = document.querySelector("#book-cover-upload");
const bookCoverUploadInput = document.querySelector("#book-cover-upload-input");
const fixMatchSearchButton = document.querySelector("#tv-fix-match-search");
const fixMatchStatus = document.querySelector("#tv-fix-match-status");
const fixMatchResults = document.querySelector("#tv-fix-match-results");
let matchSearchPending = false;
window.__mplexShowConnectionLost = message => {
  clearTimeout(window.__mplexConnectionNoticeTimer);
  let notice = document.querySelector("#harbinger-connection-status");
  if (!notice) {
    notice = document.createElement("div");
    notice.id = "harbinger-connection-status";
    document.body.append(notice);
  }
  notice.setAttribute("role", "alert");
  notice.textContent = message || "Lost connection to Harbinger.";
  notice.style.cssText = "position:fixed;top:0;left:0;right:0;z-index:9999;display:flex;justify-content:center;padding:.8rem 1rem;border:1px solid #ef4444;color:#fff;background:#7f1d1d;box-shadow:0 8px 24px #0008";
};
window.__mplexShowConnected = () => {
  const notice = document.querySelector("#harbinger-connection-status");
  if (!notice) return;
  notice.setAttribute("role", "status");
  notice.textContent = "Connected";
  notice.style.cssText = "position:fixed;top:0;left:0;right:0;z-index:9999;display:flex;justify-content:center;padding:.8rem 1rem;border:1px solid #22c55e;color:#dcfce7;background:#166534;box-shadow:0 8px 24px #0008";
  window.__mplexConnectionNoticeTimer = window.setTimeout(() => notice.remove(), 2500);
};
const searchTvMatches = async () => {
  if (matchSearchPending) return;
  fixMatchResults.replaceChildren();
  const query = fixMatchQuery.value.trim();
  const isBook = fixMatchDialog.dataset.kind === "book";
  if (!query) {
    fixMatchStatus.textContent = isBook ? "Enter a book title to search." : "Enter a show title to search.";
    return;
  }
  matchSearchPending = true;
  fixMatchStatus.textContent = isBook ? "Searching Audible…" : "Searching TMDB…";
  fixMatchSearchButton.disabled = true;
  try {
    const params = new URLSearchParams({
      library: fixMatchDialog.dataset.library,
      folder: fixMatchDialog.dataset.folder || "0",
      path: fixMatchDialog.dataset.path,
      query
    });
    if (isBook && fixMatchProvider) params.set("provider", fixMatchProvider.value);
    if (isBook && fixMatchAuthor?.value.trim()) params.set("author", fixMatchAuthor.value.trim());
    const endpoint = isBook ? "/web/book-match/search" : "/web/tv-match/search";
    const response = await fetch(`${endpoint}?${params}`, { cache: "no-store" });
    const payload = await response.json().catch(() => ({}));
    if (response.status === 404) {
      throw new Error("The match search endpoint is unavailable (404).");
    }
    if (!response.ok) throw new Error(payload.error || `Match search failed (HTTP ${response.status}).`);
    if (!payload.results.length) {
      fixMatchStatus.textContent = "No matches found. Try another search.";
      return;
    }
    fixMatchStatus.textContent = `${payload.results.length} results. Select the correct match.`;
    payload.results.forEach(result => {
      const item = document.createElement("div");
      item.className = "fix-match-result";
      const cover = document.createElement("img");
      cover.className = "fix-match-cover";
      cover.alt = "";
      cover.loading = "lazy";
      const content = document.createElement("div");
      content.className = "fix-match-result-content";
      const heading = document.createElement("h3");
      const select = document.createElement("button");
      select.type = "button";
      select.textContent = "Use this match";
      if (isBook) {
        if (result.cover_url) cover.src = result.cover_url;
        const year = result.published_year ? ` (${result.published_year})` : "";
        heading.textContent = `${result.title}${year}`;
        const overview = document.createElement("p");
        const details = [];
        if (result.authors?.length) details.push(result.authors.join(", "));
        if (result.narrators?.length) details.push(`Narrated by ${result.narrators.join(", ")}`);
        if (result.duration_minutes) details.push(`${Math.round(result.duration_minutes / 60)}h ${result.duration_minutes % 60}m`);
        overview.textContent = details.join(" · ") || "No details available.";
        select.dataset.matchBookId = result.provider_id;
        content.append(heading, overview, select);
      } else {
        if (result.poster_path) cover.src = `https://image.tmdb.org/t/p/w185${result.poster_path}`;
        const year = result.first_air_date ? ` (${result.first_air_date.slice(0, 4)})` : "";
        heading.textContent = `${result.title}${year}`;
        const overview = document.createElement("p");
        overview.textContent = result.overview || "No summary available.";
        select.dataset.matchId = String(result.id);
        content.append(heading, overview, select);
      }
      item.append(cover, content);
      fixMatchResults.append(item);
    });
  } catch (error) {
    if (error instanceof TypeError) {
      window.__mplexShowConnectionLost("Lost connection to Harbinger.");
      fixMatchStatus.textContent = "";
    } else {
      fixMatchStatus.textContent = error.message || "Match search failed.";
    }
  } finally {
    matchSearchPending = false;
    fixMatchSearchButton.disabled = false;
  }
};
bindCardAction(fixMatchSearchButton, "click", searchTvMatches);
bindCardAction(fixMatchQuery, "input", () => {
  if (!fixMatchAuthor || fixMatchDialog.dataset.kind !== "book") return;
  const defaultAuthor = (fixMatchDialog.dataset.defaultAuthor || "").trim();
  const defaultTitle = (fixMatchDialog.dataset.defaultTitle || "").trim();
  if (fixMatchAuthor.value.trim() === defaultAuthor && fixMatchQuery.value.trim() !== defaultTitle) {
    fixMatchAuthor.value = "";
  }
});
bindCardAction(fixMatchQuery, "keydown", event => {
  if (event.key === "Enter") {
    event.preventDefault();
    searchTvMatches();
  }
});
bindCardAction(fixMatchResults, "click", async event => {
  const isBook = fixMatchDialog.dataset.kind === "book";
  const select = event.target.closest(isBook ? "[data-match-book-id]" : "[data-match-id]");
  if (!select || select.disabled) return;
  select.disabled = true;
  fixMatchStatus.textContent = "Saving match and artwork…";
  try {
    const endpoint = isBook ? "/web/book-match/select" : "/web/tv-match/select";
    const body = isBook
      ? {
          library: Number(fixMatchDialog.dataset.library),
          folder: Number(fixMatchDialog.dataset.folder || 0),
          path: fixMatchDialog.dataset.path,
          provider: fixMatchProvider?.value || "audible",
          id: select.dataset.matchBookId
        }
      : {
          library: Number(fixMatchDialog.dataset.library),
          path: fixMatchDialog.dataset.path,
          tmdb_id: Number(select.dataset.matchId)
        };
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body)
    });
    const payload = await response.json().catch(() => ({}));
    if (response.status === 404) {
      throw new Error("The match save endpoint is unavailable (404).");
    }
    if (!response.ok) throw new Error(payload.error || "Could not save this match.");
    fixMatchDialog.close();
    if (window.__mplexRefresh) window.__mplexRefresh();
    else window.location.reload();
  } catch (error) {
    select.disabled = false;
    if (error instanceof TypeError) {
      window.__mplexShowConnectionLost("Lost connection to Harbinger.");
      fixMatchStatus.textContent = "";
    } else {
      fixMatchStatus.textContent = error.message || "Could not save this match.";
    }
  }
});
bindCardAction(document.querySelector(".fix-match-close"), "click", () => fixMatchDialog.close());
bindCardAction(fixMatchDialog, "click", event => {
  if (event.target === fixMatchDialog) fixMatchDialog.close();
});
const refreshPage = () => {
  if (window.__mplexRefresh) window.__mplexRefresh();
  else window.location.reload();
};
const chooseBookCover = async (source, file) => {
  const response = await fetch("/web/audiobook-cover/select", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      library: Number(bookCoverDialog.dataset.library),
      folder: Number(bookCoverDialog.dataset.folder || 0),
      path: bookCoverDialog.dataset.path,
      source,
      file
    })
  });
  if (!response.ok) throw new Error("Could not save that cover.");
  bookCoverDialog.close();
  refreshPage();
};
const addCoverChoices = (container, choices, source) => {
  choices.forEach(choice => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "book-cover-choice";
    const image = document.createElement("img");
    image.alt = "";
    image.loading = "lazy";
    image.src = choice.url;
    const label = document.createElement("span");
    label.textContent = choice.label || choice.file || "Cover";
    button.append(image, label);
    button.addEventListener("click", async () => {
      button.disabled = true;
      try {
        await chooseBookCover(source, choice.file);
      } catch (error) {
        button.disabled = false;
        bookCoverStatus.textContent = error.message || "Could not save that cover.";
      }
    });
    container.append(button);
  });
};
const openBookCoverDialog = async trigger => {
  if (!bookCoverDialog) return;
  const scope = trigger.closest(".media-card") || trigger.closest(".audiobook-detail");
  const library = scope?.dataset.library;
  const path = scope?.dataset.mediaPath || scope?.dataset.path;
  if (!library || !path) return;
  bookCoverDialog.dataset.library = library;
  bookCoverDialog.dataset.folder = scope?.dataset.folder || "0";
  bookCoverDialog.dataset.path = path;
  bookCoverStored.replaceChildren();
  bookCoverEmbedded.replaceChildren();
  bookCoverFolder.replaceChildren();
  bookCoverUrl.value = "";
  bookCoverStatus.textContent = "Loading cover choices…";
  bookCoverDialog.showModal();
  try {
    const params = new URLSearchParams({ library, folder: bookCoverDialog.dataset.folder, path });
    const response = await fetch(`/web/audiobook-cover/options?${params}`, { cache: "no-store" });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(payload.error || "Could not load cover choices.");
    addCoverChoices(bookCoverStored, payload.stored || [], "stored");
    if (payload.embedded) {
      addCoverChoices(bookCoverEmbedded, [{ url: payload.embeddedUrl, label: "Embedded cover" }], "embedded");
    }
    addCoverChoices(bookCoverFolder, payload.folder || [], "folder");
    const hasChoices = bookCoverStored.children.length
      || bookCoverEmbedded.children.length
      || bookCoverFolder.children.length;
    bookCoverStatus.textContent = hasChoices
      ? "Choose a cover source, load a URL, or upload an image."
      : "No images found. Load a URL or upload an image.";
  } catch (error) {
    bookCoverStatus.textContent = error.message || "Could not load cover choices.";
  }
};
const saveBookCoverUrl = async () => {
  const url = bookCoverUrl.value.trim();
  if (!url) {
    bookCoverStatus.textContent = "Enter an image URL.";
    return;
  }
  bookCoverUrlUse.disabled = true;
  bookCoverStatus.textContent = "Downloading cover…";
  try {
    const response = await fetch("/web/audiobook-cover/url", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        library: Number(bookCoverDialog.dataset.library),
        folder: Number(bookCoverDialog.dataset.folder || 0),
        path: bookCoverDialog.dataset.path,
        url
      })
    });
    if (!response.ok) throw new Error("Could not download that cover.");
    bookCoverDialog.close();
    refreshPage();
  } catch (error) {
    bookCoverStatus.textContent = error.message || "Could not download that cover.";
  } finally {
    bookCoverUrlUse.disabled = false;
  }
};
const uploadBookCover = async file => {
  if (!file) return;
  bookCoverStatus.textContent = "Saving cover…";
  try {
    const response = await fetch(
      `/web/audiobook-cover?library=${encodeURIComponent(bookCoverDialog.dataset.library)}&folder=${encodeURIComponent(bookCoverDialog.dataset.folder || "0")}&path=${encodeURIComponent(bookCoverDialog.dataset.path)}`,
      { method: "POST", headers: { "content-type": file.type || "application/octet-stream" }, body: await file.arrayBuffer() }
    );
    if (!response.ok) throw new Error("Could not save that cover.");
    bookCoverDialog.close();
    refreshPage();
  } catch (error) {
    bookCoverStatus.textContent = error.message || "Could not save that cover.";
  }
};
bindCardAction(bookCoverUrlUse, "click", saveBookCoverUrl);
bindCardAction(bookCoverUrl, "keydown", event => {
  if (event.key === "Enter") {
    event.preventDefault();
    saveBookCoverUrl();
  }
});
bindCardAction(bookCoverUpload, "click", () => bookCoverUploadInput?.click());
bindCardAction(bookCoverUploadInput, "change", () => uploadBookCover(bookCoverUploadInput.files?.[0]));
bindCardAction(document.querySelector(".book-cover-close"), "click", () => bookCoverDialog.close());
bindCardAction(bookCoverDialog, "click", event => {
  if (event.target === bookCoverDialog) bookCoverDialog.close();
});

bindCardAction(document, "click", async event => {
  const menu = event.target.closest(".media-card-menu");
  if (menu && event.target.closest("summary")) {
    document.querySelectorAll(".media-card-menu[open]").forEach(openMenu => {
      if (openMenu !== menu) openMenu.removeAttribute("open");
    });
  } else if (!menu) {
    document.querySelectorAll(".media-card-menu[open]").forEach(openMenu => openMenu.removeAttribute("open"));
  }
  const button = event.target.closest("[data-watch-action]");
  const setCoverButton = event.target.closest("[data-set-cover]");
  const bookMatchButton = event.target.closest("[data-book-match]");
  const fixMatchButton = event.target.closest("[data-fix-match]");
  const refreshMetadataButton = event.target.closest("[data-refresh-metadata]");
  if (setCoverButton) {
    event.preventDefault();
    document.querySelectorAll(".media-card-menu[open]").forEach(openMenu => openMenu.removeAttribute("open"));
    openBookCoverDialog(setCoverButton);
    return;
  }
  if (refreshMetadataButton) {
    event.preventDefault();
    const originalLabel = refreshMetadataButton.textContent;
    refreshMetadataButton.disabled = true;
    refreshMetadataButton.textContent = "Refreshing…";
    try {
      const response = await fetch(
        `/settings/libraries/${refreshMetadataButton.dataset.library}/scan?path=${encodeURIComponent(refreshMetadataButton.dataset.path)}&refresh=true`,
        { method: "POST", redirect: "follow" }
      );
      const resultUrl = new URL(response.url);
      const error = resultUrl.searchParams.get("error");
      if (!response.ok || error) throw new Error(error || "Could not refresh metadata.");
      if (window.__mplexRefresh) window.__mplexRefresh();
      else window.location.reload();
    } catch (error) {
      refreshMetadataButton.disabled = false;
      refreshMetadataButton.textContent = originalLabel;
      let status = refreshMetadataButton.closest(".media-card-menu-items")?.querySelector(".media-card-menu-status");
      if (!status) {
        status = document.createElement("span");
        status.className = "media-card-menu-status";
        status.setAttribute("role", "status");
        status.style.cssText = "padding:.45rem .65rem;color:#fca5a5;font-size:.72rem";
        refreshMetadataButton.closest(".media-card-menu-items")?.append(status);
      }
      status.textContent = error.message || "Could not refresh metadata. Try again.";
    }
    return;
  }

  if (bookMatchButton || fixMatchButton) {
    event.preventDefault();
    const fixMatchHeading = document.querySelector("#tv-fix-match-heading");
    const fixMatchDescription = document.querySelector("#tv-fix-match-show");
    if (!fixMatchDialog || !fixMatchQuery || !fixMatchSearchButton || !fixMatchStatus || !fixMatchResults || !fixMatchHeading || !fixMatchDescription) {
      return;
    }
    document.querySelectorAll(".media-card-menu[open]").forEach(openMenu => openMenu.removeAttribute("open"));
    const trigger = bookMatchButton || fixMatchButton;
    const card = trigger.closest(".media-card");
    const detail = trigger.closest(".audiobook-detail");
    const scope = card || detail;
    const title = card?.querySelector(".media-card-title")?.textContent.trim()
      || document.querySelector("main h1")?.textContent.trim()
      || "Media item";
    const isBook = Boolean(bookMatchButton);
    fixMatchDialog.dataset.kind = isBook ? "book" : "tv";
    if (fixMatchProvider) {
      fixMatchProvider.hidden = !isBook;
      fixMatchProvider.value = "audible";
    }
    if (fixMatchAuthor) {
      fixMatchAuthor.hidden = !isBook;
      const cardAuthor = card?.querySelector(".media-card-subtitle")?.textContent.trim() || "";
      fixMatchAuthor.value = isBook && cardAuthor.toLowerCase() !== "unknown" ? cardAuthor : "";
    }
    fixMatchHeading.textContent = isBook ? "Match Audiobook" : "Fix Match";
    fixMatchQuery.setAttribute("aria-label", isBook ? "Search Audible audiobooks" : "Search movies and TV shows");
    fixMatchDialog.dataset.library = scope?.dataset.library || "";
    fixMatchDialog.dataset.folder = scope?.dataset.folder || "0";
    fixMatchDialog.dataset.path = card?.dataset.mediaPath || detail?.dataset.path || "";
    fixMatchDescription.textContent = isBook
      ? `Search Audible for ${title}.`
      : `Search for the correct match for ${title}.`;
    fixMatchQuery.value = title;
    fixMatchDialog.dataset.defaultTitle = title;
    fixMatchDialog.dataset.defaultAuthor = fixMatchAuthor?.value.trim() || "";
    fixMatchStatus.textContent = "";
    fixMatchResults.replaceChildren();
    fixMatchDialog.showModal();
    fixMatchStatus.textContent = "Review the title, then click Search to look for a match.";
    return;
  }
  if (!button || button.disabled) return;
  event.preventDefault();
  button.disabled = true;
  const card = button.closest(".media-card");
  const { watchAction: action } = button.dataset;
  const path = button.dataset.path || card?.dataset.mediaPath;
  const library = button.dataset.library || card?.dataset.library;
  const watchScope = button.dataset.watchScope || card?.dataset.watchScope;
  const watchKey = button.dataset.watchKey || card?.dataset.watchKey;
  const showFailure = () => {
    let status = button.closest(".media-card-menu")?.querySelector(".media-card-menu-status");
    if (!status) {
      status = document.createElement("span");
      status.className = "media-card-menu-status";
      status.setAttribute("role", "status");
      status.style.cssText = "padding:.45rem .65rem;color:#fca5a5;font-size:.72rem";
      button.closest(".media-card-menu-items")?.append(status);
    }
    status.textContent = "Could not update watch status. Try again.";
  };
  try {
    const response = await fetch("/web/watch-action", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action, path, library: Number(library), folder: Number(card?.dataset.folder || 0), scope: watchScope })
    });
    if (!response.ok) {
      button.disabled = false;
      showFailure();
      return;
    }
    button.closest(".media-card-menu")?.querySelector(".media-card-menu-status")?.remove();
    button.closest("details")?.removeAttribute("open");
    const matchesItem = item => item.dataset.library === library && (
      watchScope === "show"
        ? item.dataset.watchKey === watchKey
        : item.dataset.mediaPath === path
    );
    if (action === "unwatched" || action === "watched") {
      const percentValue = action === "watched" ? "100" : "0";
      document.querySelectorAll(".media-card").forEach(item => {
        if (!matchesItem(item)) return;
        const progress = item.querySelector('[role="progressbar"]');
        const fill = progress?.querySelector("span");
        const percent = item.querySelector(".media-card-percent");
        progress?.setAttribute("aria-valuenow", percentValue);
        if (fill) fill.style.width = `${percentValue}%`;
        if (percent) percent.textContent = `${percentValue}%`;
        const badge = item.querySelector(".media-card-unwatched");
        if (item.classList.contains("media-card-book")) {
          if (action === "watched") {
            if (badge) {
              badge.textContent = "✓";
              badge.setAttribute("aria-label", "Read");
            } else {
              const created = document.createElement("span");
              created.className = "media-card-unwatched";
              created.setAttribute("aria-label", "Read");
              created.style.cssText = "position:absolute;right:0;top:0;z-index:2;min-width:1.5rem;padding:.2rem .4rem;border-radius:0 0 0 .35rem;color:#fff;background:rgba(17,24,39,.8);font-size:.85rem;font-weight:700;line-height:1;text-align:center";
              created.textContent = "✓";
              item.querySelector(".media-card-artwork")?.append(created);
            }
          } else {
            badge?.remove();
          }
        }
        if (action === "watched" && item.closest(".continue-grid")) item.remove();
      });
    } else if (action === "remove_from_continue") {
      document.querySelectorAll(".continue-grid .media-card").forEach(item => {
        if (matchesItem(item)) item.remove();
      });
    }
    const continueGrid = document.querySelector(".continue-grid");
    if (continueGrid && !continueGrid.querySelector(".media-card")) {
      continueGrid.innerHTML = '<p class="note empty-state">Nothing to resume yet. Start watching something and it will appear here.</p>';
    }
    if (action === "watched" && continueGrid) window.location.reload();
  } catch (_) {
    button.disabled = false;
    showFailure();
  }
});
window.__mplexCardActionsCleanup = () => cardActionListeners.splice(0).forEach(remove => remove());
let checkingConnection = false;
if (window.__mplexConnectionCheckTimer) clearInterval(window.__mplexConnectionCheckTimer);
window.__mplexConnectionCheckTimer = window.setInterval(async () => {
  if (checkingConnection) return;
  checkingConnection = true;
  try {
    const response = await fetch("/web/ping", { cache: "no-store" });
    if (response.status === 404) {
      clearInterval(window.__mplexConnectionCheckTimer);
      window.__mplexConnectionCheckTimer = null;
    }
    await response.body?.cancel();
    window.__mplexShowConnected();
  } catch (_) {
    window.__mplexShowConnectionLost("Lost connection to Harbinger.");
  } finally {
    checkingConnection = false;
  }
}, 5000);
</script>"##
}

pub(crate) fn spa_navigation_script() -> &'static str {
    r#"<style>body { background: radial-gradient(ellipse at 75% -20%, #1b3151 0, transparent 44rem), #0b1020; }</style><script>
if (!window.__mplexSpaBound) {
window.__mplexSpaBound = true;
let navigationSequence = 0;
async function navigateMplex(url, addHistory) {
  const sequence = ++navigationSequence;
  const response = await fetch(url, { headers: { "X-Mplex-Web-Navigation": "1" }, cache: "no-store" });
  if (!response.ok) { window.location.assign(url); return; }
  const parsed = new DOMParser().parseFromString(await response.text(), "text/html");
  if (sequence !== navigationSequence) return;
  if (!parsed.querySelector(".library") && window.__mplexLibraryRefreshTimer) {
    clearInterval(window.__mplexLibraryRefreshTimer);
    window.__mplexLibraryRefreshTimer = null;
  }
  const scripts = [...parsed.body.querySelectorAll("script")].map(script => script.textContent || "");
  parsed.body.querySelectorAll("script").forEach(script => script.remove());
  document.title = parsed.title;
  document.head.innerHTML = parsed.head.innerHTML;
  document.body.innerHTML = parsed.body.innerHTML;
  if (addHistory) history.pushState({}, "", url);
  scripts.forEach(source => {
    const script = document.createElement("script");
    script.textContent = `(() => { ${source}\n })()`;
    document.body.append(script);
  });
  window.scrollTo(0, 0);
}
window.__mplexRefresh = () => navigateMplex(location.pathname + location.search + location.hash, false);
document.addEventListener("click", event => {
  const link = event.target.closest("a[data-spa-link]");
  if (!link || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  const url = new URL(link.href);
  if (url.origin !== location.origin || url.pathname !== "/") return;
  event.preventDefault();
  navigateMplex(url.pathname + url.search + url.hash, true).catch(() => window.location.assign(url.href));
});
window.addEventListener("popstate", () => navigateMplex(location.pathname + location.search + location.hash, false));
}
</script>"#
}

pub(crate) fn average_media_progress(
    media: &[&Media],
    metadata: &HashMap<String, serde_json::Value>,
) -> f64 {
    if media.is_empty() {
        return 0.0;
    }
    media
        .iter()
        .map(|item| {
            metadata
                .get(&item.id)
                .map(media_card_progress)
                .unwrap_or_default()
        })
        .sum::<f64>()
        / media.len() as f64
}

pub(crate) fn unwatched_episode_count(
    episodes: &[&Media],
    roots: &[PathBuf],
    metadata: &HashMap<String, serde_json::Value>,
) -> usize {
    let mut groups = BTreeMap::<(u32, u32, String), bool>::new();
    for media in episodes {
        let key = roots
            .iter()
            .find(|root| media.path.starts_with(*root))
            .and_then(|root| tv_episode_info(root, &media.path))
            .map(|episode| (episode.season, episode.episode, String::new()))
            .unwrap_or_else(|| (1, u32::MAX, media.id.clone()));
        let watched = metadata
            .get(&media.id)
            .and_then(|item| item.get("mplex_view_count"))
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|count| count > 0);
        *groups.entry(key).or_insert(false) |= watched;
    }
    groups.values().filter(|watched| !**watched).count()
}

pub(crate) fn folder_card_with_badge(
    name: &str,
    relative: &std::path::Path,
    library: Option<usize>,
    folder: Option<usize>,
    preview: Option<&std::path::Path>,
    cover: bool,
    banner: bool,
    progress: Option<f64>,
    badge: Option<&str>,
    subtitle: Option<&str>,
) -> String {
    let location_query = format!(
        "{}{}",
        library
            .map(|index| format!("library={index}&"))
            .unwrap_or_default(),
        folder
            .map(|index| format!("folder={index}&"))
            .unwrap_or_default()
    );
    let preview_markup = if cover { None } else { preview }
        .map(|path| {
            format!(
                r#"<video class="folder-preview-video" muted playsinline preload="auto" src="/web/stream?{}path={}" aria-label="Preview of {}"></video>"#,
                location_query,
                url_encode(&path.to_string_lossy()),
                html(name)
            )
        })
        .unwrap_or_default();
    let artwork_markup = if cover {
        format!(
            r#"<img class="folder-cover" src="{}" alt="Cover for {}">"#,
            artwork_url_with_folder(relative, library, folder, "cover"),
            html(name)
        )
    } else {
        String::new()
    };
    let fallback_icon = if cover || preview.is_some() {
        String::new()
    } else {
        r#"<span class="folder-preview-icon" aria-hidden="true">📁</span>"#.to_owned()
    };
    let banner_style = if banner {
        format!(
            r#" style="background-image: linear-gradient(90deg, rgba(17,24,39,.45), rgba(17,24,39,.8)), url('{}')""#,
            artwork_url_with_folder(relative, library, folder, "banner")
        )
    } else {
        String::new()
    };
    let progress_markup = progress.map(media_progress_markup).unwrap_or_default();
    let badge_markup = badge
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!(
                r#"<span style="position:absolute;top:.55rem;right:.55rem;z-index:2;min-width:1.7rem;padding:.25rem .45rem;border-radius:.35rem;color:#fff;background:rgba(17,24,39,.9);font-weight:700;text-align:center">{}</span>"#,
                html(value)
            )
        })
        .unwrap_or_default();
    let subtitle_markup = subtitle
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!(
                r#"<p style="margin:.25rem 0 0;color:#9ca3af;font-size:.8rem">{}</p>"#,
                html(value)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<a class="folder-card" data-spa-link style="display:block;overflow:hidden;padding:0;color:inherit;text-decoration:none;border:1px solid #374151;border-radius:.75rem;background:#1f2937" href="/?{}path={}">
  <div class="folder-preview" style="position:relative;display:grid;place-items:center;overflow:hidden;aspect-ratio:2/3;background:linear-gradient(135deg,#1e3a8a,#312e81 55%,#111827)"{}>{}{}{}{}</div>
  <div class="folder-info" style="padding:.9rem 1rem 1rem"><h2 style="font-size:1rem;line-height:1.25;margin:0;overflow-wrap:anywhere">{}</h2>{}{}</div>
</a>"#,
        location_query,
        url_encode(&relative.to_string_lossy()),
        banner_style,
        preview_markup,
        badge_markup,
        fallback_icon,
        artwork_markup,
        html(name),
        subtitle_markup,
        progress_markup,
    )
}

fn audiobook_saved_progress(state: &AppState, item_id: &str) -> (i64, Option<i64>, bool) {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT position_ms, duration_ms, watched FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
                params![USER_ID, item_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                },
            )
            .ok()
        })
        .unwrap_or((0, None, false))
}

/// Load all saved progress for a library page in one query. Rendering a
/// 700-book grid must not issue 700 individual database queries.
fn audiobook_progress_map(state: &AppState) -> HashMap<String, (i64, Option<i64>, bool)> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT media_key, position_ms, duration_ms, watched
                       FROM watch_progress
                      WHERE user_id = ?1",
                )
                .ok()?;
            let rows = statement
                .query_map([USER_ID], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        (
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, bool>(3)?,
                        ),
                    ))
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

fn series_display_label(value: &str) -> String {
    let (name, sequence) = crate::audiobookshelf::scanner::split_series_label(value);
    match sequence {
        Some(sequence) => format!("#{sequence} · {name}"),
        None => name.to_owned(),
    }
}

fn audiobook_media_card(
    item: &crate::audiobookshelf::catalog::Item,
    relative: &std::path::Path,
    library: usize,
    folder: Option<usize>,
    saved_progress: (i64, Option<i64>, bool),
) -> String {
    let folder_query = folder
        .map(|index| format!("folder={index}&"))
        .unwrap_or_default();
    let href = format!(
        "/?library={library}&{folder_query}path={}",
        url_encode(&relative.to_string_lossy())
    );
    let mut details_parts = Vec::new();
    if !item.metadata.series.is_empty() {
        details_parts.push(
            item.metadata
                .series
                .iter()
                .map(|series| series_display_label(series))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !item.metadata.narrators.is_empty() {
        details_parts.push(format!(
            "Narrated by {}",
            item.metadata.narrators.join(", ")
        ));
    }
    let details = details_parts.join(" · ");
    let cover_url = Some(artwork_url_with_version(
        relative,
        Some(library),
        folder,
        "cover",
        item.cover_version,
    ));
    let (position, duration, watched) = saved_progress;
    let progress = if watched {
        1.0
    } else {
        match duration {
            Some(duration) if duration > 0 => (position as f64 / duration as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    };
    media_card_with_options(
        None,
        &href,
        &item.title,
        Some(&item.author),
        (!details.is_empty()).then_some(details.as_str()),
        progress,
        watched.then_some(0),
        relative,
        library,
        false,
        false,
        "audiobook",
        false,
        cover_url.as_deref(),
        Some(&item.id),
    )
}

fn audiobook_description_html(value: &str) -> String {
    let mut sanitizer = ammonia::Builder::new();
    sanitizer
        .tags(std::collections::HashSet::from([
            "a",
            "b",
            "blockquote",
            "br",
            "div",
            "em",
            "h3",
            "h4",
            "h5",
            "h6",
            "i",
            "li",
            "ol",
            "p",
            "s",
            "span",
            "strong",
            "u",
            "ul",
        ]))
        .tag_attributes(std::collections::HashMap::from([(
            "a",
            std::collections::HashSet::from(["href", "title"]),
        )]))
        .generic_attributes(std::collections::HashSet::new())
        .url_schemes(std::collections::HashSet::from(["http", "https", "mailto"]))
        .link_rel(Some("noopener noreferrer"))
        .clean_content_tags(std::collections::HashSet::from([
            "audio", "base", "button", "embed", "form", "iframe", "img", "input", "link", "math",
            "meta", "noscript", "object", "script", "select", "style", "svg", "template",
            "textarea", "video",
        ]))
        .strip_comments(true);
    sanitizer.clean(value).to_string()
}

fn audiobook_detail(
    state: &AppState,
    item: &crate::audiobookshelf::catalog::Item,
    relative: &std::path::Path,
    library: usize,
    folder: Option<usize>,
) -> String {
    let book = &item.metadata;
    let folder_index = folder.unwrap_or_default();
    let cover_url = Some(artwork_url_with_version(
        relative,
        Some(library),
        folder,
        "cover",
        item.cover_version,
    ));
    let cover = match cover_url.as_deref() {
        Some(url) => format!(
            r#"<img src="{}" alt="Cover for {}">"#,
            html(url),
            html(&item.title)
        ),
        None => format!(
            r#"<div class="audiobook-cover-placeholder" role="img" aria-label="Cover unavailable for {}">No cover available</div>"#,
            html(&item.title)
        ),
    };
    let chapters = item.chapters();
    let chapter_end = chapters
        .last()
        .map(|chapter| chapter.end)
        .unwrap_or_default();
    let duration = item.duration.max(chapter_end).max(0.0);
    let (saved_position, saved_duration, watched) = audiobook_saved_progress(state, &item.id);
    let saved_seconds = if watched {
        duration
    } else {
        let reported = if saved_duration.unwrap_or_default() > 0 {
            saved_position as f64 / saved_duration.unwrap() as f64 * duration
        } else {
            saved_position as f64 / 1000.0
        };
        reported.clamp(0.0, duration)
    };
    let location_query = format!("library={library}&folder={folder_index}&");
    let total_size = item.audio_files.iter().map(|file| file.size).sum::<u64>();
    let tracks = item
        .audio_files
        .iter()
        .map(|file| {
            let estimated_duration = if file.duration > 0.0 {
                file.duration
            } else if total_size > 0 {
                duration * file.size as f64 / total_size as f64
            } else if !item.audio_files.is_empty() {
                duration / item.audio_files.len() as f64
            } else {
                0.0
            };
            serde_json::json!({
                "title": file.title,
                "url": format!("/web/stream?{}path={}", location_query, url_encode(&file.rel_path)),
                "duration": estimated_duration,
            })
        })
        .collect::<Vec<_>>();
    let track_json = serde_json::to_string(&tracks).unwrap_or_else(|_| "[]".to_owned());
    let chapter_json = serde_json::to_string(
        &chapters
            .iter()
            .map(|chapter| {
                serde_json::json!({
                    "id": chapter.id,
                    "title": chapter.title,
                    "start": chapter.start,
                    "end": chapter.end,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_owned());
    let chapter_options = if chapters.is_empty() {
        r#"<option value="0">No chapters</option>"#.to_owned()
    } else {
        chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| {
                format!(
                    r#"<option value="{}" data-audiobook-seek="{}">{} · {}</option>"#,
                    index,
                    chapter.start,
                    html(&chapter.title),
                    html(&clock_duration(chapter.end - chapter.start)),
                )
            })
            .collect::<String>()
    };
    let mut facts = Vec::new();
    facts.push(("Title", item.title.clone()));
    if !book.subtitle.trim().is_empty() {
        facts.push(("Subtitle", book.subtitle.clone()));
    }
    if !item.author.is_empty() {
        facts.push(("Author", item.author.clone()));
    }
    if !book.narrators.is_empty() {
        facts.push(("Narrator", book.narrators.join(", ")));
    }
    if !book.series.is_empty() {
        facts.push(("Series", book.series.join(", ")));
    }
    if !book.publisher.is_empty() {
        facts.push(("Publisher", book.publisher.clone()));
    }
    let published = if book.published_year.is_empty() {
        book.published_date.as_str()
    } else {
        book.published_year.as_str()
    };
    if !published.is_empty() {
        facts.push(("Published", published.to_owned()));
    }
    if !book.genres.is_empty() {
        facts.push(("Genres", book.genres.join(", ")));
    }
    if !book.language.is_empty() {
        facts.push(("Language", book.language.clone()));
    }
    if book.abridged {
        facts.push(("Edition", "Abridged".to_owned()));
    }
    if book.explicit {
        facts.push(("Rating", "Explicit".to_owned()));
    }
    if !book.isbn.is_empty() {
        facts.push(("ISBN", book.isbn.clone()));
    }
    if !book.asin.is_empty() {
        facts.push(("ASIN", book.asin.clone()));
    }
    facts.push(("Length", clock_duration(duration)));
    let track_count = item.audio_files.len();
    let track_items = item
        .audio_files
        .iter()
        .map(|file| {
            let name = std::path::Path::new(&file.rel_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&file.title);
            let mut meta = vec![clock_duration(file.duration)];
            if file.size > 0 {
                meta.push(format_file_size(file.size));
            }
            format!(
                r#"<li><span class="audiobook-track-name">{}</span><span class="audiobook-track-meta">{}</span></li>"#,
                html(name),
                html(&meta.join(" · "))
            )
        })
        .collect::<String>();
    let track_fact = format!(
        r#"<div class="audiobook-fact audiobook-track-fact"><dt>Tracks</dt><dd><details class="audiobook-track-list"><summary>{} track{}</summary><ul>{}</ul></details></dd></div>"#,
        track_count,
        if track_count == 1 { "" } else { "s" },
        track_items
    );
    let fact_markup = format!(
        "{}{}",
        facts
            .into_iter()
            .map(|(label, value)| {
                format!(
                    r#"<div class="audiobook-fact"><dt>{}</dt><dd>{}</dd></div>"#,
                    html(label),
                    html(&value)
                )
            })
            .collect::<String>(),
        track_fact
    );
    let matched_markup = book
        .match_provider
        .as_deref()
        .filter(|provider| !provider.trim().is_empty())
        .map(|provider| {
            let provider_name = crate::audiobookshelf::providers::Provider::from_key(provider)
                .map(|provider| provider.display_name())
                .unwrap_or(provider);
            let provider_id = book
                .match_provider_id
                .as_deref()
                .filter(|provider_id| !provider_id.trim().is_empty())
                .map(|provider_id| {
                    format!(
                        r#"<div><dt>Provider ID</dt><dd>{}</dd></div>"#,
                        html(provider_id)
                    )
                })
                .unwrap_or_default();
            format!(
                r#"<details class="audiobook-matched"><summary title="Matched from {}"><span class="audiobook-matched-check" aria-hidden="true">✓</span><span>Matched from {}</span></summary><div class="audiobook-match-detail"><dl><div><dt>Provider</dt><dd>{}</dd></div>{}<div><dt>Matched title</dt><dd>{}</dd></div></dl></div></details>"#,
                html(provider_name),
                html(provider_name),
                html(provider_name),
                provider_id,
                html(book.title.as_deref().unwrap_or(&item.title))
            )
        })
        .unwrap_or_default();
    let description_source = if item.description.trim().is_empty() {
        "No book description is available in metadata.json."
    } else {
        &item.description
    };
    let description = audiobook_description_html(description_source);
    let first_track = tracks
        .first()
        .and_then(|track| track["url"].as_str())
        .unwrap_or_default();
    let initial_title = chapters
        .first()
        .map(|chapter| chapter.title.as_str())
        .unwrap_or(&item.title);

    format!(
        r#"<article class="audiobook-detail" data-audiobook-player data-library="{library}" data-folder="{folder_index}" data-path="{path}" data-duration="{duration}" data-start="{saved_seconds}" data-tracks="{tracks}" data-chapters="{chapters}">
  <section class="audiobook-player-shell">
    <div class="audiobook-player-cover">
      {cover}
      <button type="button" class="audiobook-toggle-button" title="Play or pause" aria-label="Play or pause">▶</button>
    </div>
    <div class="audiobook-player-main">
      <div class="audiobook-player-heading"><span class="audiobook-player-position">Track 1 of {track_count}</span><h3 class="audiobook-current-title">{initial_title}</h3></div>
      <div class="audiobook-transport">
        <button type="button" class="audiobook-previous-chapter" title="Previous chapter" aria-label="Previous chapter">⏮</button>
        <button type="button" class="audiobook-skip-back" title="Back 15 seconds" aria-label="Back 15 seconds">↺ 15</button>
        <button type="button" class="audiobook-skip-forward" title="Forward 30 seconds" aria-label="Forward 30 seconds">↻ 30</button>
        <button type="button" class="audiobook-next-chapter" title="Next chapter" aria-label="Next chapter">⏭</button>
      </div>
      <input class="audiobook-seek" type="range" min="0" max="{duration}" value="0" step="0.1" aria-label="Seek through audiobook">
      <div class="audiobook-times"><span class="audiobook-current-time">0:00</span><span class="audiobook-total-time">{duration_label}</span></div>
      <div class="audiobook-player-options">
        <label class="audiobook-speed-control">Speed <select class="audiobook-speed" aria-label="Playback speed"><option value="0.5">0.5×</option><option value="0.75">0.75×</option><option value="1" selected>1×</option><option value="1.1">1.1×</option><option value="1.25">1.25×</option><option value="1.5">1.5×</option><option value="1.75">1.75×</option><option value="2">2×</option><option value="2.5">2.5×</option><option value="3">3×</option></select></label>
        <label class="audiobook-volume-control">Volume <input class="audiobook-volume" type="range" min="0" max="1" step="0.05" value="1" aria-label="Volume"></label>
        <label class="audiobook-chapter-control">Chapter <select class="audiobook-chapter-select" aria-label="Select chapter">{chapter_options}</select></label>
      </div>
      <p class="audiobook-status" role="status" aria-live="polite"></p>
      <audio class="audiobook-audio" preload="metadata" src="{first_track}">Your browser does not support HTML audio.</audio>
    </div>
  </section>
  <section class="audiobook-info">
    <h3>About this book</h3>
    <div class="audiobook-description">{description}</div>
    <dl class="audiobook-facts">{fact_markup}</dl>
    <div class="audiobook-actions">
      <button type="button" class="button-secondary audiobook-match" data-book-match>Match audiobook</button>
      {matched_markup}
      <button type="button" class="button-secondary audiobook-match" data-set-cover>Set cover image</button>
    </div>
  </section>
</article>"#,
        library = library,
        folder_index = folder_index,
        path = html(&relative.to_string_lossy()),
        duration = duration,
        saved_seconds = saved_seconds,
        tracks = html(&track_json),
        chapters = html(&chapter_json),
        cover = cover,
        track_count = item.audio_files.len().max(1),
        initial_title = html(initial_title),
        duration_label = html(&clock_duration(duration)),
        first_track = html(first_track),
        chapter_options = chapter_options,
        description = description,
        fact_markup = fact_markup,
        matched_markup = matched_markup,
    )
}

fn clock_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let remainder = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{remainder:02}")
    } else {
        format!("{minutes}:{remainder:02}")
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn web_media_type(path: &std::path::Path) -> Option<(&'static str, &'static str)> {
    media_type(path)
        .or_else(|| crate::audiobookshelf::catalog::audio_mime(path).map(|mime| ("audio", mime)))
}

pub(crate) async fn first_media_file(
    directory: &std::path::Path,
    include_audio: bool,
) -> Option<PathBuf> {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = tokio::fs::read_dir(directory).await.ok()?;
        let mut children = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let file_type = entry.file_type().await.ok()?;
            if file_type.is_file()
                && (media_type(&path).is_some()
                    || (include_audio
                        && crate::audiobookshelf::catalog::audio_mime(&path).is_some()))
            {
                return Some(path);
            }
            if file_type.is_dir() {
                children.push(path);
            }
        }
        children.sort();
        pending.extend(children.into_iter().rev());
    }
    None
}

pub(crate) async fn media_file_count(directory: &std::path::Path, include_audio: bool) -> usize {
    let mut pending = vec![directory.to_path_buf()];
    let mut count = 0;
    while let Some(directory) = pending.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && (media_type(&path).is_some()
                    || (include_audio
                        && crate::audiobookshelf::catalog::audio_mime(&path).is_some()))
            {
                count += 1;
            }
        }
    }
    count
}

pub(crate) async fn web_stream(
    State(s): State<AppState>,
    Query(query): Query<WebPath>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let Some(relative) = query.path.as_deref().and_then(clean_relative_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if contains_hidden_component(&relative) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let library_root = selected_library_folder_root(&s, query.library, query.folder);
    let path = match web_path(&s, &library_root, &relative).await {
        Ok(path) => path,
        Err(status) => return status.into_response(),
    };
    let Ok(metadata) = tokio::fs::metadata(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((container, mime)) = web_media_type(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !metadata.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let media = Media {
        id: relative.to_string_lossy().into_owned(),
        title: path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Video")
            .to_owned(),
        path,
        size: metadata.len(),
        container: container.to_owned(),
        mime: mime.to_owned(),
        metadata_revision: 0,
    };
    stream_file(&media, method, headers, false).await
}

pub(crate) async fn web_path(
    _state: &AppState,
    root: &std::path::Path,
    relative: &std::path::Path,
) -> Result<PathBuf, StatusCode> {
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let path = tokio::fs::canonicalize(root.join(relative))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

pub(crate) fn clean_relative_path(value: &str) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in PathBuf::from(value).components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(result)
}

pub(crate) fn is_hidden_path(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

pub(crate) fn contains_hidden_component(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::Normal(name) if name.to_str().is_some_and(|name| name.starts_with('.')))
    })
}

pub(crate) async fn web_ping() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub(crate) async fn dashboard_stats(State(state): State<AppState>) -> Response {
    let snapshot = state.metrics.snapshot();
    let current = snapshot.latest.clone().unwrap_or_default();
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let config = state
        .config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_else(|poison| poison.into_inner().clone());
    let library_count = config.libraries.len();
    let media = media_snapshot(&state);
    let media_count = media.len();
    let movies = config
        .libraries
        .iter()
        .filter(|library| library.kind == "movie")
        .map(|library| {
            media
                .iter()
                .filter(|item| {
                    library
                        .folders
                        .iter()
                        .any(|root| item.path.starts_with(root))
                })
                .count()
        })
        .sum::<usize>();
    let mut series = std::collections::BTreeSet::new();
    for (index, library) in config
        .libraries
        .iter()
        .enumerate()
        .filter(|(_, library)| library.kind == "tv")
    {
        for root in &library.folders {
            for item in media.iter().filter(|item| item.path.starts_with(root)) {
                series.insert((index, tv_show_title(root, &item.path)));
            }
        }
    }
    let audiobooks = crate::audiobookshelf::catalog::Catalog::cached(&state)
        .map(|catalog| {
            catalog
                .libraries
                .iter()
                .map(|library| library.items.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    let (markers_total, markers_last_24h) = state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let total = db
                .query_row("SELECT COUNT(*) FROM media_markers", [], |row| {
                    row.get::<_, i64>(0)
                })
                .ok()?;
            let recent = db
                .query_row(
                    "SELECT COUNT(*) FROM media_markers WHERE created_at >= ?1",
                    [generated_at - 86_400],
                    |row| row.get::<_, i64>(0),
                )
                .ok()?;
            Some((total, recent))
        })
        .unwrap_or((0, 0));

    Json(serde_json::json!({
        "generated_at": generated_at,
        "current": current,
        "series": snapshot.series,
        "activities": snapshot.activities,
        "library": {
            "libraries": library_count,
            "media_items": media_count,
            "audiobooks": audiobooks,
            "movies": movies,
            "series": series.len(),
            "markers_total": markers_total,
            "markers_last_24h": markers_last_24h,
        },
        "disks": snapshot.disks,
    }))
    .into_response()
}

fn dashboard_styles() -> &'static str {
    r##"
#dashboard-status { display: inline-flex; align-items: center; gap: .65rem; margin: 0 0 1rem; padding: .5rem .8rem; border: 1px solid #26354a; border-radius: 999px; color: #bdebd1; background: rgba(18, 52, 38, .45); font-size: .82rem; }
#dashboard-status.is-error { color: #ffd6dc; background: rgba(56, 29, 42, .55); }
#dashboard-status.is-error .status-dot { background: #f87171; box-shadow: 0 0 0 .22rem rgba(248, 113, 113, .14); }
.dashboard-stack { display: grid; gap: 1.1rem; } .chart-grid, .dashboard-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1.1rem; } .dashboard-grid { grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr); }
.metric-value { flex: 0 0 auto; color: #dbeafe; font-size: .88rem; font-weight: 700; text-align: right; }
.dashboard-stack canvas { display: block; width: 100%; height: 170px; border: 1px solid #243248; border-radius: .6rem; background: #0a111e; }
.dashboard-stack .legend { display: flex; flex-wrap: wrap; gap: 1rem; margin-top: .65rem; color: #93a4bd; font-size: .78rem; } .dashboard-stack .legend span::before { display: inline-block; width: .6rem; height: .6rem; margin-right: .4rem; border-radius: 50%; background: var(--legend-color); content: ""; }
.dashboard-stack .stat-list { display: grid; gap: .75rem; margin: 0; } .dashboard-stack .stat-row { display: flex; align-items: baseline; justify-content: space-between; gap: 1rem; padding-bottom: .6rem; border-bottom: 1px solid #243248; } .dashboard-stack .stat-row:last-child { border-bottom: 0; padding-bottom: 0; } .dashboard-stack .stat-row dt { color: #98a6bb; font-size: .84rem; } .dashboard-stack .stat-row dd { margin: 0; color: #e8eef8; font-size: .95rem; font-weight: 700; text-align: right; }
.dashboard-stack table { width: 100%; border-collapse: collapse; font-size: .84rem; } .dashboard-stack th, .dashboard-stack td { padding: .6rem .5rem; border-bottom: 1px solid #243248; text-align: left; vertical-align: top; } .dashboard-stack th { color: #93a4bd; font-size: .72rem; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; } .dashboard-stack td { color: #d7e0ee; } .dashboard-stack tbody tr:last-child td { border-bottom: 0; } .dashboard-stack .empty-note { margin: .5rem 0 0; color: #8fa0b8; font-size: .85rem; }
@media (max-width: 900px) { .chart-grid, .dashboard-grid { grid-template-columns: 1fr; } }
"##
}

fn dashboard_markup() -> &'static str {
    r##"<div class="dashboard-stack">
<section class="settings-card" aria-labelledby="dashboard-heading"><div class="section-heading"><div><h2 id="dashboard-heading">Server dashboard</h2><p class="section-description">Live resource use and background work on this server. Samples are collected once a second and retained for two minutes.</p></div></div><p id="dashboard-status" class="status-line" role="status" aria-live="polite"><span class="status-dot"></span><span id="dashboard-status-text">Connecting to the metrics feed…</span></p></section>
<div class="chart-grid">
<section class="settings-card" aria-labelledby="cpu-heading"><div class="section-heading"><div><h2 id="cpu-heading">Processor</h2><p class="section-description">Whole host versus the Harbinger process.</p></div><span id="dashboard-cpu-summary" class="metric-value">—</span></div><canvas id="dashboard-cpu-chart" role="img" aria-label="CPU usage over time"></canvas><div class="legend"><span style="--legend-color:#60a5fa">Host CPU</span><span style="--legend-color:#f472b6">Harbinger CPU</span></div></section>
<section class="settings-card" aria-labelledby="memory-heading"><div class="section-heading"><div><h2 id="memory-heading">Memory</h2><p class="section-description">Host utilization and resident process memory.</p></div><span id="dashboard-memory-summary" class="metric-value">—</span></div><canvas id="dashboard-memory-chart" role="img" aria-label="Memory usage over time"></canvas><div class="legend"><span style="--legend-color:#60a5fa">Host memory</span><span style="--legend-color:#f472b6">Harbinger memory</span></div></section>
<section class="settings-card" aria-labelledby="network-heading"><div class="section-heading"><div><h2 id="network-heading">Network traffic</h2><p class="section-description">Aggregate non-loopback receive and transmit rates.</p></div><span id="dashboard-network-summary" class="metric-value">—</span></div><canvas id="dashboard-network-chart" role="img" aria-label="Network traffic over time"></canvas><div class="legend"><span style="--legend-color:#4ade80">Received</span><span style="--legend-color:#fbbf24">Transmitted</span></div></section>
<section class="settings-card" aria-labelledby="disk-heading"><div class="section-heading"><div><h2 id="disk-heading">Disk activity</h2><p class="section-description">Filesystem reads and writes, plus Harbinger's own disk I/O.</p></div><span id="dashboard-disk-summary" class="metric-value">—</span></div><canvas id="dashboard-disk-chart" role="img" aria-label="Disk activity over time"></canvas><div class="legend"><span style="--legend-color:#4ade80">Read</span><span style="--legend-color:#fbbf24">Write</span><span style="--legend-color:#f472b6">Harbinger read</span></div></section>
</div>
<div class="dashboard-grid">
<section class="settings-card" aria-labelledby="activity-heading"><div class="section-heading"><div><h2 id="activity-heading">Active work</h2><p class="section-description">Library scans and intro/credit detection currently running.</p></div></div><table id="dashboard-activities-table"><thead><tr><th>Type</th><th>Item</th><th>Location</th><th>Elapsed</th></tr></thead><tbody id="dashboard-activities-body"></tbody></table><p id="dashboard-activities-empty" class="empty-note">No active scans or detection tasks.</p></section>
<section class="settings-card" aria-labelledby="library-heading"><div class="section-heading"><div><h2 id="library-heading">Library</h2><p class="section-description">Configured content and stored skip markers.</p></div></div><dl class="stat-list"><div class="stat-row"><dt>Libraries</dt><dd id="dashboard-library-count">—</dd></div><div class="stat-row"><dt>Audiobooks</dt><dd id="dashboard-audiobook-count">—</dd></div><div class="stat-row"><dt>Movies</dt><dd id="dashboard-movie-count">—</dd></div><div class="stat-row"><dt>TV series</dt><dd id="dashboard-series-count">—</dd></div><div class="stat-row"><dt>Media items</dt><dd id="dashboard-media-count">—</dd></div><div class="stat-row"><dt>Stored markers</dt><dd id="dashboard-marker-count">—</dd></div><div class="stat-row"><dt>Markers added in 24h</dt><dd id="dashboard-marker-recent">—</dd></div><div class="stat-row"><dt>Last sample</dt><dd id="dashboard-updated">—</dd></div></dl></section>
</div>
<section class="settings-card" aria-labelledby="disks-heading"><div class="section-heading"><div><h2 id="disks-heading">Filesystems</h2><p class="section-description">Capacity of mounted disks visible to Harbinger.</p></div></div><table id="dashboard-disks-table"><thead><tr><th>Disk</th><th>Mount</th><th>Used</th><th>Capacity</th></tr></thead><tbody id="dashboard-disks-body"></tbody></table><p id="dashboard-disks-empty" class="empty-note">No filesystem information is available.</p></section>
</div>
"##
}

fn dashboard_script() -> &'static str {
    r##"<script>
(() => {
  const status = document.querySelector("#dashboard-status");
  const statusText = document.querySelector("#dashboard-status-text");
  let timer = null;
  let latest = null;

  const formatBytes = value => {
    const number = Number(value) || 0;
    if (number < 1024) return `${Math.round(number)} B`;
    const units = ["KiB", "MiB", "GiB", "TiB"];
    let scaled = number / 1024;
    let unit = 0;
    while (scaled >= 1024 && unit < units.length - 1) { scaled /= 1024; unit += 1; }
    return `${scaled.toFixed(scaled >= 10 ? 0 : 1)} ${units[unit]}`;
  };
  const formatRate = value => `${formatBytes(value)}/s`;
  const formatPercent = value => `${(Number(value) || 0).toFixed(1)}%`;
  const setStatus = (text, live) => {
    if (!statusText || !status) return;
    statusText.textContent = text;
    status.classList.toggle("is-error", !live);
  };

  const charts = [
    { id: "dashboard-cpu-chart", max: 100, lines: [{ key: "host_cpu_percent", color: "#60a5fa" }, { key: "process_cpu_percent", color: "#f472b6" }] },
    { id: "dashboard-memory-chart", max: 100, lines: [{ key: "host_memory_percent", color: "#60a5fa" }, { key: "process_memory_bytes", color: "#f472b6", scale: "memory" }] },
    { id: "dashboard-network-chart", max: "auto", lines: [{ key: "network_received_bytes_per_sec", color: "#4ade80" }, { key: "network_transmitted_bytes_per_sec", color: "#fbbf24" }] },
    { id: "dashboard-disk-chart", max: "auto", lines: [{ key: "disk_read_bytes_per_sec", color: "#4ade80" }, { key: "disk_write_bytes_per_sec", color: "#fbbf24" }, { key: "process_disk_read_bytes_per_sec", color: "#f472b6" }] },
  ];

  const valueFor = (sample, line) => {
    const value = Number(sample[line.key]) || 0;
    return line.scale === "memory" && sample.host_memory_total_bytes > 0
      ? (value / sample.host_memory_total_bytes) * 100
      : value;
  };

  function drawCharts() {
    if (!latest) return;
    const series = latest.series || [];
    for (const chart of charts) {
      const canvas = document.querySelector(`#${chart.id}`);
      if (!canvas) continue;
      const context = canvas.getContext("2d");
      if (!context) continue;
      const ratio = window.devicePixelRatio || 1;
      const width = canvas.clientWidth || 800;
      const height = canvas.clientHeight || 170;
      const pixelWidth = Math.round(width * ratio);
      const pixelHeight = Math.round(height * ratio);
      if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
        canvas.width = pixelWidth;
        canvas.height = pixelHeight;
      }
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      context.strokeStyle = "#243248";
      context.lineWidth = 1;
      for (let index = 0; index <= 4; index += 1) {
        const y = Math.round((height - 1) * index / 4) + 0.5;
        context.beginPath();
        context.moveTo(0, y);
        context.lineTo(width, y);
        context.stroke();
      }
      if (series.length < 2) {
        context.fillStyle = "#8091aa";
        context.font = "13px system-ui, sans-serif";
        context.fillText("Waiting for samples…", 12, height / 2);
        continue;
      }
      let max = chart.max;
      if (max === "auto") {
        max = 1;
        for (const sample of series) for (const line of chart.lines) max = Math.max(max, valueFor(sample, line));
        max *= 1.15;
      }
      chart.lines.forEach(line => {
        context.beginPath();
        context.strokeStyle = line.color;
        context.lineWidth = 2;
        series.forEach((sample, index) => {
          const x = (index / (series.length - 1)) * (width - 2) + 1;
          const y = height - 1 - Math.min(valueFor(sample, line) / max, 1) * (height - 2);
          if (index === 0) context.moveTo(x, y); else context.lineTo(x, y);
        });
        context.stroke();
      });
    }
  }

  function fillRows(tableBody, empty, rows) {
    if (!tableBody || !empty) return;
    tableBody.replaceChildren();
    for (const values of rows) {
      const row = document.createElement("tr");
      for (const value of values) {
        const cell = document.createElement("td");
        cell.textContent = value;
        row.append(cell);
      }
      tableBody.append(row);
    }
    empty.hidden = rows.length > 0;
  }

  function render() {
    if (!latest) return;
    const current = latest.current || {};
    const library = latest.library || {};
    const setText = (selector, value) => { const element = document.querySelector(selector); if (element) element.textContent = value; };
    setText("#dashboard-cpu-summary", `Host ${formatPercent(current.host_cpu_percent)} · App ${formatPercent(current.process_cpu_percent)}`);
    setText("#dashboard-memory-summary", `Host ${formatPercent(current.host_memory_percent)} · ${formatBytes(current.process_memory_bytes)}`);
    setText("#dashboard-network-summary", `↓ ${formatRate(current.network_received_bytes_per_sec)} · ↑ ${formatRate(current.network_transmitted_bytes_per_sec)}`);
    setText("#dashboard-disk-summary", `↓ ${formatRate(current.disk_read_bytes_per_sec)} · ↑ ${formatRate(current.disk_write_bytes_per_sec)}`);
    setText("#dashboard-library-count", String(library.libraries ?? 0));
    setText("#dashboard-audiobook-count", String(library.audiobooks ?? 0));
    setText("#dashboard-movie-count", String(library.movies ?? 0));
    setText("#dashboard-series-count", String(library.series ?? 0));
    setText("#dashboard-media-count", String(library.media_items ?? 0));
    setText("#dashboard-marker-count", String(library.markers_total ?? 0));
    setText("#dashboard-marker-recent", String(library.markers_last_24h ?? 0));
    setText("#dashboard-updated", latest.generated_at ? new Date(latest.generated_at * 1000).toLocaleTimeString() : "—");

    fillRows(
      document.querySelector("#dashboard-activities-body"),
      document.querySelector("#dashboard-activities-empty"),
      (latest.activities || []).map(activity => [activity.kind, activity.title, activity.detail, `${activity.elapsed_seconds}s`]),
    );
    fillRows(
      document.querySelector("#dashboard-disks-body"),
      document.querySelector("#dashboard-disks-empty"),
      (latest.disks || []).map(disk => [
        disk.name || "Disk",
        disk.mount_point || "—",
        `${formatPercent(disk.used_percent)} used · ${formatBytes(disk.available_bytes)} free`,
        formatBytes(disk.total_bytes),
      ]),
    );
    drawCharts();
  }

  async function refresh() {
    try {
      const response = await fetch("/web/dashboard/stats", { cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      latest = await response.json();
      render();
      setStatus(`Live · updated ${new Date().toLocaleTimeString()}`, true);
    } catch (error) {
      setStatus("Connection lost · retrying", false);
    }
  }

  window.__harbingerDashboardCleanup?.();
  window.__harbingerDashboardCleanup = () => {
    if (timer !== null) clearInterval(timer);
    window.removeEventListener("resize", drawCharts);
  };
  window.addEventListener("resize", drawCharts);
  refresh();
  timer = setInterval(refresh, 2000);
})();
</script>"##
}

pub(crate) async fn web_index_redirect(
    headers: HeaderMap,
    surface: Option<axum::Extension<ListenerSurface>>,
) -> Response {
    if matches!(surface, Some(axum::Extension(ListenerSurface::Web))) {
        return Redirect::to("/").into_response();
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .map(|authority| authority.host().to_owned())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let host = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(_)) => format!("[{host}]"),
        _ => host,
    };
    Redirect::temporary(&format!("http://{host}:{WEB_UI_PORT}/")).into_response()
}

async fn timed_web_client(state: AppState, query: WebPath, request_path: String) -> Response {
    let started = Instant::now();
    let response = web_client(State(state), Query(query)).await;
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms > 250 {
        info!(tag = "hb", path = %request_path, elapsed_ms, "web client request completed");
    }
    response
}

pub(crate) async fn root_handler(
    State(s): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<WebPath>,
    surface: Option<axum::Extension<ListenerSurface>>,
) -> Response {
    if matches!(surface, Some(axum::Extension(ListenerSurface::Plex))) {
        return plex_root(&s).await;
    }
    if matches!(surface, Some(axum::Extension(ListenerSurface::Web))) {
        return if query.library.is_some()
            || query.path.is_some()
            || query.play.is_some()
            || query.episode.is_some()
        {
            timed_web_client(s, query, uri.to_string()).await
        } else {
            home_client(State(s), query.search).await
        };
    }
    let is_plex_client = headers.contains_key("X-Plex-Product")
        || headers.contains_key("X-Plex-Client-Identifier")
        || headers.contains_key("X-Plex-Token");
    let is_plex_query = plex_query_value(&uri, "X-Plex-Product").is_some()
        || plex_query_value(&uri, "X-Plex-Client-Identifier").is_some()
        || plex_query_value(&uri, "X-Plex-Token").is_some();
    if is_plex_client || is_plex_query {
        plex_root(&s).await
    } else if query.library.is_some()
        || query.path.is_some()
        || query.play.is_some()
        || query.episode.is_some()
    {
        timed_web_client(s, query, uri.to_string()).await
    } else {
        home_client(State(s), query.search).await
    }
}

#[allow(dead_code)]
pub(crate) fn web_admin_exists(state: &AppState) -> bool {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM web_admin WHERE id = 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .ok()
        })
        .unwrap_or(false)
}

pub(crate) fn users_exist(state: &AppState) -> bool {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE password_hash IS NOT NULL)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .ok()
        })
        .unwrap_or(false)
}

pub(crate) fn web_session_valid(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(token) = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie| {
            cookie
                .split(';')
                .find_map(|part| part.trim().strip_prefix("harbinger_session="))
        })
    else {
        return false;
    };
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
        "SELECT EXISTS(SELECT 1 FROM web_sessions WHERE token_hash = ?1 AND expires_at > ?2)",
        (&token_hash, now), |row| row.get::<_, bool>(0)
    ).ok()
        })
        .unwrap_or(false)
}

pub(crate) async fn web_auth_gate(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if matches!(
        request.extensions().get::<ListenerSurface>(),
        Some(ListenerSurface::Plex)
    ) {
        return next.run(request).await;
    }
    let path = request.uri().path();
    let plex_client = request.headers().contains_key("X-Plex-Product")
        || request.headers().contains_key("X-Plex-Client-Identifier")
        || request.headers().contains_key("X-Plex-Token")
        || plex_query_value(request.uri(), "X-Plex-Product").is_some()
        || plex_query_value(request.uri(), "X-Plex-Client-Identifier").is_some()
        || plex_query_value(request.uri(), "X-Plex-Token").is_some();
    let web_page = path == "/"
        || path == "/web"
        || path.starts_with("/web/")
        || path == "/settings"
        || path.starts_with("/settings/");
    if !web_page || (path == "/" && plex_client) || path.starts_with("/auth") {
        return next.run(request).await;
    }
    if users_exist(&state) && !web_session_valid(&state, request.headers()) {
        return Redirect::to("/auth").into_response();
    }
    next.run(request).await
}

pub(crate) async fn auth_page(State(state): State<AppState>) -> Response {
    let setup = !users_exist(&state);
    let (title, description, button) = if setup {
        (
            "Create administrator login",
            "Set a password for the Harbinger web interface.",
            "Create login",
        )
    } else {
        (
            "Sign in",
            "Sign in to manage your Harbinger server.",
            "Sign in",
        )
    };
    let action = if setup { "/auth/setup" } else { "/auth/login" };
    let confirm = if setup {
        r#"<label>Confirm password<input name="confirm_password" type="password" autocomplete="new-password" required></label>"#
    } else {
        ""
    };
    let body = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="icon" href="/favicon.ico" sizes="any"><title>{title} · harbinger</title><style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#0b1020;color:#f9fafb;font:16px system-ui}}main{{box-sizing:border-box;width:min(92vw,410px);padding:2rem;border:1px solid #374151;border-radius:1rem;background:#111827}}h1{{margin-top:0}}p{{color:#9ca3af}}form,label{{display:grid;gap:.7rem}}form{{gap:1rem}}input,button{{box-sizing:border-box;width:100%;padding:.8rem;border-radius:.5rem;font:inherit}}input{{border:1px solid #4b5563;background:#0b1020;color:white}}button{{border:0;background:#2563eb;color:white;font-weight:700;cursor:pointer}}</style></head><main><h1>{title}</h1><p>{description}</p><form method="post" action="{action}"><label>Administrator password<input name="password" type="password" minlength="8" autocomplete="{autocomplete}" required></label>{confirm}<button>{button}</button></form></main></html>"#,
        autocomplete = if setup {
            "new-password"
        } else {
            "current-password"
        }
    );
    Html(add_build_info_footer(body)).into_response()
}

pub(crate) async fn auth_setup(
    State(state): State<AppState>,
    Form(form): Form<AdminLoginForm>,
) -> Response {
    if form.password.len() < 8 || form.password != form.confirm_password || users_exist(&state) {
        return (
            StatusCode::BAD_REQUEST,
            "Setup is unavailable, passwords do not match, or the password is too short",
        )
            .into_response();
    }
    let salt = SaltString::generate(&mut OsRng);
    let Ok(hash) = Argon2::default().hash_password(form.password.as_bytes(), &salt) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "INSERT OR IGNORE INTO web_admin (id, password_hash) VALUES (1, ?1)",
        [hash.to_string()],
    );
    match db.execute(
        "INSERT OR IGNORE INTO users (id, name, username, password_hash, user_type) VALUES ('admin', 'Administrator', 'admin', ?1, 'admin')",
        [hash.to_string()],
    ) {
        Ok(1) => {}
        Ok(_) => {
            return (
                StatusCode::CONFLICT,
                "Administrator setup has already completed",
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    drop(db);
    issue_web_session(&state)
}

pub(crate) async fn auth_login(
    State(state): State<AppState>,
    Form(form): Form<AdminLoginForm>,
) -> Response {
    let hash = state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT password_hash FROM web_admin WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
    });
    let valid = hash
        .and_then(|hash| {
            PasswordHash::new(&hash).ok().map(|parsed| {
                Argon2::default()
                    .verify_password(form.password.as_bytes(), &parsed)
                    .is_ok()
            })
        })
        .unwrap_or(false);
    if !valid {
        return (
            StatusCode::UNAUTHORIZED,
            Html("<p>Incorrect password. <a href=\"/auth\">Try again</a></p>"),
        )
            .into_response();
    }
    issue_web_session(&state)
}

pub(crate) async fn auth_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie| {
            cookie
                .split(';')
                .find_map(|part| part.trim().strip_prefix("harbinger_session="))
        })
    {
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        if let Ok(db) = state.db.lock() {
            let _ = db.execute(
                "DELETE FROM web_sessions WHERE token_hash = ?1",
                [&token_hash],
            );
        }
    }
    let mut response = Redirect::to("/auth").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("harbinger_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
    );
    response
}

pub(crate) async fn stream_for_id(
    s: AppState,
    id: String,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let Some(media) = find_media(&s, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    stream_file(&media, method, headers, false).await
}
