use crate::release_version::{is_newer, newest_by_tag};
use crate::ICON_DATA;
use anyhow::anyhow;
use http_req::request::{HttpVersion, Request};
use http_req::uri::Uri;
use onlyterm_config::{configuration, onlyterm_version};
use onlyterm_toast_notification::*;
use serde::*;
use std::convert::TryFrom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use termwiz::cell::{Hyperlink, Underline};
use termwiz::color::AnsiColor;
use termwiz::escape::csi::{Cursor, Sgr};
use termwiz::escape::osc::{ITermDimension, ITermFileData, ITermProprietary};
use termwiz::escape::{OneBased, OperatingSystemCommand, CSI};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Release {
    pub url: String,
    /// GitHub sends `null` for a release without notes.
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub size: usize,
    pub url: String,
    pub browser_download_url: String,
}

/// The list, not `/releases/latest`: that endpoint never returns a
/// pre-release, and every OnlyTerm release is one.
const RELEASES_URL: &str =
    "https://api.github.com/repos/PHPCraftdream/OnlyTerm/releases?per_page=30";

fn get_github_releases(uri: &str) -> anyhow::Result<Vec<Release>> {
    let uri = Uri::try_from(uri)?;

    let mut body = Vec::new();
    let response = Request::new(&uri)
        .version(HttpVersion::Http10)
        .header(
            "User-Agent",
            &format!("OnlyTerm/OnlyTerm-{}", onlyterm_version()),
        )
        .send(&mut body)
        .map_err(|e| anyhow!("failed to query github releases: {}", e))?;

    let status = response.status_code();
    if !status.is_success() {
        anyhow::bail!("github releases request failed with status {}", status);
    }
    parse_releases(&body)
}

/// A release that does not fit [`Release`] is skipped, not fatal: one odd
/// entry must not switch the whole update check off.
fn parse_releases(body: &[u8]) -> anyhow::Result<Vec<Release>> {
    let entries: Vec<serde_json::Value> = serde_json::from_slice(body)?;
    Ok(entries
        .into_iter()
        .filter_map(|entry| {
            serde_json::from_value::<Release>(entry)
                .map_err(|err| log::warn!("skipping a malformed release entry: {}", err))
                .ok()
        })
        .collect())
}

/// The newest published release, by version rather than by publish date or
/// tag text.
fn latest_release(releases: Vec<Release>) -> Option<Release> {
    newest_by_tag(
        releases.into_iter().filter(|release| !release.draft),
        |release| release.tag_name.as_str(),
    )
}

pub fn get_latest_release_info() -> anyhow::Result<Release> {
    latest_release(get_github_releases(RELEASES_URL)?)
        .ok_or_else(|| anyhow!("no published release has a recognizable version tag"))
}

pub fn load_last_release_info_and_set_banner() {
    if !configuration().check_for_updates {
        return;
    }

    let update_file_name = onlyterm_config::DATA_DIR.join("check_update");
    if let Ok(data) = std::fs::read(update_file_name) {
        let latest: Release = match serde_json::from_slice(&data) {
            Ok(d) => d,
            Err(_) => return,
        };

        let current = onlyterm_version();
        let force_ui = std::env::var_os("ONLYTERM_ALWAYS_SHOW_UPDATE_UI").is_some();
        if !is_newer(&latest.tag_name, current) && !force_ui {
            return;
        }

        set_banner_from_release_info(&latest);
    }
}

fn set_banner_from_release_info(latest: &Release) {
    let mux = crate::Mux::get();
    let url = latest.html_url.clone();

    let icon = ITermFileData {
        name: None,
        size: Some(ICON_DATA.len()),
        width: ITermDimension::Automatic,
        height: ITermDimension::Cells(2),
        preserve_aspect_ratio: true,
        inline: true,
        do_not_move_cursor: false,
        data: ICON_DATA.to_vec(),
    };
    let icon = OperatingSystemCommand::ITermProprietary(ITermProprietary::File(Box::new(icon)));
    let top_line_pos = CSI::Cursor(Cursor::CharacterAndLinePosition {
        line: OneBased::new(1),
        col: OneBased::new(6),
    });
    let second_line_pos = CSI::Cursor(Cursor::CharacterAndLinePosition {
        line: OneBased::new(2),
        col: OneBased::new(6),
    });
    let link_on = OperatingSystemCommand::SetHyperlink(Some(Hyperlink::new(url)));
    let underline_color = CSI::Sgr(Sgr::UnderlineColor(AnsiColor::Blue.into()));
    let underline_on = CSI::Sgr(Sgr::Underline(Underline::Single));
    let reset = CSI::Sgr(Sgr::Reset);
    let link_off = OperatingSystemCommand::SetHyperlink(None);
    mux.set_banner(Some(format!(
        "{}{}OnlyTerm Update Available\r\n{}{}{}{}Click to see what's new{}{}\r\n",
        icon,
        top_line_pos,
        second_line_pos,
        link_on,
        underline_color,
        underline_on,
        link_off,
        reset,
    )));
}

fn schedule_set_banner_from_release_info(latest: &Release) {
    if !is_newer(&latest.tag_name, onlyterm_version()) {
        return;
    }
    // This fires at most once per `check_for_updates_interval_seconds`
    // (hours by default) and only sets an informational banner string;
    // it is not something the user is waiting on, unlike pty output
    // becoming visible or input being processed. Route it through the
    // low priority queue so that it never jumps ahead of
    // pending high-priority GUI work (e.g. a backlog of pane output
    // notifications) competing for the same main-thread spawn queue.
    onlyterm_promise::spawn::spawn_into_main_thread_with_low_priority({
        let latest = latest.clone();
        async move {
            set_banner_from_release_info(&latest);
        }
    })
    .detach();
}

fn update_checker() {
    // Compute how long we should sleep for;
    // if we've never checked, give it a few seconds after the first
    // launch, otherwise compute the interval based on the time of
    // the last check.
    let update_interval = Duration::from_secs(configuration().check_for_updates_interval_seconds);
    let initial_interval = Duration::from_secs(10);

    let force_ui = std::env::var_os("ONLYTERM_ALWAYS_SHOW_UPDATE_UI").is_some();

    let update_file_name = onlyterm_config::DATA_DIR.join("check_update");
    let delay = update_file_name
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map_err(|_| ())
        .and_then(|systime| {
            let elapsed = systime.elapsed().unwrap_or(Duration::new(0, 0));
            update_interval.checked_sub(elapsed).ok_or(())
        })
        .unwrap_or(initial_interval);

    std::thread::sleep(if force_ui { initial_interval } else { delay });

    let my_sock = onlyterm_config::RUNTIME_DIR.join(format!("gui-sock-{}", std::process::id()));

    loop {
        // Figure out which other onlyterm-guis are running.
        // We have a little "consensus protocol" to decide which
        // of us will show the toast notification or show the update
        // window: the one of us that sorts first in the list will
        // own doing that, so that if there are a dozen gui processes
        // running, we don't spam the user with a lot of notifications.
        let socks = onlyterm_client::discovery::discover_gui_socks();

        if configuration().check_for_updates {
            if let Ok(latest) = get_latest_release_info() {
                schedule_set_banner_from_release_info(&latest);
                let current = onlyterm_version();
                if is_newer(&latest.tag_name, current) || force_ui {
                    log::info!(
                        "latest release {} is newer than current build {}",
                        latest.tag_name,
                        current
                    );

                    let url = latest.html_url.clone();

                    if force_ui || socks.is_empty() || socks[0] == my_sock {
                        persistent_toast_notification_with_click_to_open_url(
                            "OnlyTerm Update Available",
                            "Click to see what's new",
                            &url,
                        );
                    }
                }

                onlyterm_config::create_user_owned_dirs(update_file_name.parent().unwrap()).ok();

                // Record the time of this check
                if let Ok(f) = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&update_file_name)
                {
                    serde_json::to_writer_pretty(f, &latest).ok();
                }
            }
        }

        std::thread::sleep(Duration::from_secs(
            configuration().check_for_updates_interval_seconds,
        ));
    }
}

pub fn start_update_checker() {
    static CHECKER_STARTED: AtomicBool = AtomicBool::new(false);
    if let Ok(false) =
        CHECKER_STARTED.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
    {
        std::thread::Builder::new()
            .name("update_checker".into())
            .spawn(update_checker)
            .expect("failed to spawn update checker thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str, draft: bool, body: &str) -> String {
        format!(
            r#"{{"url":"u","body":{},"html_url":"h","tag_name":"{}","draft":{},"assets":[]}}"#,
            body, tag, draft
        )
    }

    fn releases(entries: &[String]) -> Vec<Release> {
        parse_releases(format!("[{}]", entries.join(",")).as_bytes()).unwrap()
    }

    #[test]
    fn a_release_without_notes_still_parses() {
        let parsed = releases(&[release_json("v0.0.44-alpha", false, "null")]);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].body, None);
    }

    #[test]
    fn a_malformed_entry_is_skipped_not_fatal() {
        let parsed = releases(&[
            r#"{"tag_name":"v0.0.1-alpha"}"#.to_string(),
            release_json("v0.0.2-alpha", false, r#""notes""#),
        ]);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].tag_name, "v0.0.2-alpha");
    }

    #[test]
    fn a_body_that_is_not_a_list_is_an_error() {
        assert!(parse_releases(br#"{"message":"Not Found"}"#).is_err());
        assert!(parse_releases(b"").is_err());
    }

    #[test]
    fn the_latest_release_is_the_highest_version_not_the_last_or_longest_tag() {
        let latest = latest_release(releases(&[
            release_json("v0.0.44-alpha", false, "null"),
            release_json("v0.0.100-alpha", false, "null"),
            release_json("v0.0.9-alpha", false, "null"),
        ]));
        assert_eq!(latest.unwrap().tag_name, "v0.0.100-alpha");
    }

    #[test]
    fn drafts_and_unrecognized_tags_are_never_the_latest() {
        let latest = latest_release(releases(&[
            release_json("v9.9.9-alpha", true, "null"),
            release_json("nightly", false, "null"),
            release_json("v0.0.44-alpha", false, "null"),
        ]));
        assert_eq!(latest.unwrap().tag_name, "v0.0.44-alpha");
        assert!(latest_release(releases(&[release_json("nightly", false, "null")])).is_none());
        assert!(latest_release(Vec::new()).is_none());
    }

    #[test]
    fn a_cached_release_from_an_older_build_still_loads() {
        let old =
            r#"{"url":"u","body":"notes","html_url":"h","tag_name":"v0.0.40-alpha","assets":[]}"#;
        let cached: Release = serde_json::from_str(old).unwrap();
        assert!(!cached.draft);
        assert_eq!(cached.body.as_deref(), Some("notes"));
    }
}
