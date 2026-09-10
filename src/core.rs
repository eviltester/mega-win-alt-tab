use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowEntry {
    pub hwnd: isize,
    pub title: String,
    pub app_name: String,
    pub class_name: String,
    pub screen_number: Option<u32>,
    pub desktop_location: DesktopLocation,
    pub minimized: bool,
    pub has_thumbnail: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopLocation {
    Current,
    Other,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabSource {
    Accessibility,
    Extension,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabEntry {
    pub browser: String,
    pub parent_hwnd: Option<isize>,
    pub window_title: Option<String>,
    pub title: String,
    pub active: bool,
    pub extension_window_id: Option<i64>,
    pub extension_tab_id: Option<i64>,
    pub source: TabSource,
    pub last_seen: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppSource {
    UserStartMenu,
    UserAppPath,
    AllUsersStartMenu,
    MachineAppPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    pub name: String,
    pub launch_path: String,
    pub source: AppSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionTabPayload {
    pub browser: String,
    #[serde(rename = "windowId")]
    pub window_id: i64,
    #[serde(rename = "tabId")]
    pub tab_id: i64,
    pub title: String,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionTabSnapshot {
    pub browser: String,
    pub tabs: Vec<ExtensionTabPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionCommand {
    ActivateTab {
        browser: String,
        #[serde(rename = "windowId")]
        window_id: i64,
        #[serde(rename = "tabId")]
        tab_id: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchResultKind {
    Window,
    Tab,
    App,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationTarget {
    Window {
        hwnd: isize,
    },
    Tab {
        parent_hwnd: Option<isize>,
        browser: String,
        title: String,
        extension_window_id: Option<i64>,
        extension_tab_id: Option<i64>,
    },
    App {
        launch_path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub kind: SearchResultKind,
    pub title: String,
    pub subtitle: String,
    pub screen_number: Option<u32>,
    pub rank: i32,
    pub target: ActivationTarget,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildResultOptions {
    pub show_desktop_labels: bool,
}

pub fn extension_payload_to_tab(payload: ExtensionTabPayload, now: Instant) -> TabEntry {
    TabEntry {
        browser: payload.browser,
        parent_hwnd: None,
        window_title: None,
        title: payload.title,
        active: payload.active,
        extension_window_id: Some(payload.window_id),
        extension_tab_id: Some(payload.tab_id),
        source: TabSource::Extension,
        last_seen: now,
    }
}

pub fn prune_stale_tabs(tabs: &mut Vec<TabEntry>, now: Instant, max_age: Duration) {
    tabs.retain(|tab| now.duration_since(tab.last_seen) <= max_age);
}

pub fn merge_tabs(accessibility_tabs: &[TabEntry], extension_tabs: &[TabEntry]) -> Vec<TabEntry> {
    let mut merged = Vec::new();
    let mut extension_by_title: HashMap<(String, String), usize> = HashMap::new();
    let mut seen_extension_ids = HashSet::new();

    for tab in extension_tabs {
        if let (Some(window_id), Some(tab_id)) = (tab.extension_window_id, tab.extension_tab_id) {
            if !seen_extension_ids.insert((window_id, tab_id)) {
                continue;
            }
        }
        let key = (
            normalize_for_match(&tab.browser),
            normalize_for_match(&tab.title),
        );
        extension_by_title.insert(key, merged.len());
        merged.push(tab.clone());
    }

    for tab in accessibility_tabs {
        let key = (
            normalize_for_match(&tab.browser),
            normalize_for_match(&tab.title),
        );
        if let Some(index) = extension_by_title.get(&key).copied() {
            if merged[index].parent_hwnd.is_none() {
                merged[index].parent_hwnd = tab.parent_hwnd;
            }
            if merged[index].window_title.is_none() {
                merged[index].window_title = tab.window_title.clone();
            }
            continue;
        }
        merged.push(tab.clone());
    }

    merged
}

pub fn build_results(
    query: &str,
    windows: &[WindowEntry],
    accessibility_tabs: &[TabEntry],
    extension_tabs: &[TabEntry],
) -> Vec<SearchResult> {
    build_results_with_options(
        query,
        windows,
        accessibility_tabs,
        extension_tabs,
        BuildResultOptions::default(),
    )
}

pub fn build_results_with_options(
    query: &str,
    windows: &[WindowEntry],
    accessibility_tabs: &[TabEntry],
    extension_tabs: &[TabEntry],
    options: BuildResultOptions,
) -> Vec<SearchResult> {
    let query = query.trim();
    let tabs = merge_tabs(accessibility_tabs, extension_tabs);
    let matching_tabs = if query.is_empty() {
        Vec::new()
    } else {
        tabs.iter()
            .filter_map(|tab| match_score(query, &tab.title).map(|score| (tab, score)))
            .collect::<Vec<_>>()
    };
    let mut results = Vec::new();

    for (index, window) in windows.iter().enumerate() {
        if !query.is_empty()
            && matching_tabs
                .iter()
                .any(|(tab, _)| tab_represents_window(tab, window))
        {
            continue;
        }

        let score = if query.is_empty() {
            10_000 - index as i32
        } else if let Some(score) = match_score(query, &window.title) {
            score + 2_000
        } else if let Some(score) = match_score(query, &window.app_name) {
            score + 1_000
        } else {
            continue;
        };

        let subtitle = window_subtitle(window, options.show_desktop_labels);

        results.push(SearchResult {
            kind: SearchResultKind::Window,
            title: window.title.clone(),
            subtitle,
            screen_number: window.screen_number,
            rank: score,
            target: ActivationTarget::Window { hwnd: window.hwnd },
        });
    }

    for (tab, score) in matching_tabs {
        let parent_window = tab
            .parent_hwnd
            .and_then(|parent_hwnd| windows.iter().find(|window| window.hwnd == parent_hwnd));
        let subtitle = tab
            .window_title
            .clone()
            .unwrap_or_else(|| format!("{} tab", display_browser_name(&tab.browser)));
        let subtitle = with_desktop_label(
            subtitle,
            parent_window
                .map(|window| window.desktop_location)
                .unwrap_or(DesktopLocation::Unknown),
            options.show_desktop_labels,
        );
        let screen_number = parent_window.and_then(|window| window.screen_number);

        results.push(SearchResult {
            kind: SearchResultKind::Tab,
            title: tab.title.clone(),
            subtitle,
            screen_number,
            rank: score + 1_500,
            target: ActivationTarget::Tab {
                parent_hwnd: tab.parent_hwnd,
                browser: tab.browser.clone(),
                title: tab.title.clone(),
                extension_window_id: tab.extension_window_id,
                extension_tab_id: tab.extension_tab_id,
            },
        });
    }

    results.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| result_kind_order(&a.kind).cmp(&result_kind_order(&b.kind)))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    results
}

fn window_subtitle(window: &WindowEntry, show_desktop_label: bool) -> String {
    let subtitle = if window.app_name.is_empty() {
        window.class_name.clone()
    } else {
        format!("{} - {}", window.app_name, window.class_name)
    };
    with_desktop_label(subtitle, window.desktop_location, show_desktop_label)
}

fn with_desktop_label(
    subtitle: String,
    desktop_location: DesktopLocation,
    show_desktop_label: bool,
) -> String {
    if !show_desktop_label {
        return subtitle;
    }

    let label = match desktop_location {
        DesktopLocation::Current => "Current desktop",
        DesktopLocation::Other => "Other desktop",
        DesktopLocation::Unknown => "Desktop unknown",
    };

    if subtitle.is_empty() {
        label.to_string()
    } else {
        format!("{subtitle} - {label}")
    }
}

pub fn build_app_results(query: &str, apps: &[AppEntry]) -> Vec<SearchResult> {
    let query = query.trim();
    let apps = dedupe_apps(apps);
    let mut results = Vec::new();

    for (index, app) in apps.iter().enumerate() {
        let score = if query.is_empty() {
            5_000 - index as i32
        } else if let Some(score) = match_score(query, &app.name) {
            score + 3_000
        } else {
            continue;
        };

        results.push(SearchResult {
            kind: SearchResultKind::App,
            title: app.name.clone(),
            subtitle: "Application".to_string(),
            screen_number: None,
            rank: score,
            target: ActivationTarget::App {
                launch_path: app.launch_path.clone(),
            },
        });
    }

    results.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    results
}

pub fn dedupe_apps(apps: &[AppEntry]) -> Vec<AppEntry> {
    let mut apps_by_name: HashMap<String, AppEntry> = HashMap::new();
    for app in apps {
        let key = normalize_for_match(&app.name);
        if key.is_empty() || app.launch_path.trim().is_empty() {
            continue;
        }

        match apps_by_name.get(&key) {
            Some(existing) if !should_replace_app(existing, app) => {}
            _ => {
                apps_by_name.insert(key, app.clone());
            }
        }
    }

    let mut apps = apps_by_name.into_values().collect::<Vec<_>>();
    apps.sort_by(|a, b| {
        normalize_for_match(&a.name)
            .cmp(&normalize_for_match(&b.name))
            .then_with(|| a.launch_path.cmp(&b.launch_path))
    });
    apps
}

fn should_replace_app(existing: &AppEntry, candidate: &AppEntry) -> bool {
    app_source_priority(candidate.source)
        .cmp(&app_source_priority(existing.source))
        .then_with(|| candidate.launch_path.len().cmp(&existing.launch_path.len()))
        .is_lt()
}

fn app_source_priority(source: AppSource) -> i32 {
    match source {
        AppSource::UserStartMenu => 0,
        AppSource::UserAppPath => 1,
        AppSource::AllUsersStartMenu => 2,
        AppSource::MachineAppPath => 3,
    }
}

fn tab_represents_window(tab: &TabEntry, window: &WindowEntry) -> bool {
    if let Some(parent_hwnd) = tab.parent_hwnd {
        return parent_hwnd == window.hwnd;
    }

    tab.active
        && browser_matches_window(&tab.browser, window)
        && browser_window_title_matches_tab(&window.title, &tab.title, &tab.browser)
}

fn browser_matches_window(browser: &str, window: &WindowEntry) -> bool {
    let app_name = normalize_for_match(&window.app_name);
    let class_name = normalize_for_match(&window.class_name);
    let title = normalize_for_match(&window.title);

    match browser {
        "chrome" => {
            app_name == "chrome"
                || class_name == "chrome_widgetwin_1"
                || title.ends_with(" - google chrome")
        }
        "edge" => app_name == "msedge" || title.ends_with(" - microsoft edge"),
        _ => false,
    }
}

fn browser_window_title_matches_tab(window_title: &str, tab_title: &str, browser: &str) -> bool {
    let window_title = normalize_for_match(window_title);
    let tab_title = normalize_for_match(tab_title);
    if tab_title.is_empty() {
        return false;
    }

    let stripped = match browser {
        "chrome" => strip_browser_suffix(&window_title, &[" - google chrome", " - chrome"]),
        "edge" => strip_browser_suffix(&window_title, &[" - microsoft edge", " - edge"]),
        _ => window_title.as_str(),
    };

    stripped == tab_title
        || stripped
            .strip_prefix(&tab_title)
            .is_some_and(|rest| rest.starts_with(" - "))
}

fn strip_browser_suffix<'a>(title: &'a str, suffixes: &[&str]) -> &'a str {
    suffixes
        .iter()
        .find_map(|suffix| title.strip_suffix(suffix))
        .unwrap_or(title)
        .trim()
}

pub fn match_score(query: &str, candidate: &str) -> Option<i32> {
    let query = normalize_for_match(query);
    let candidate = normalize_for_match(candidate);

    if query.is_empty() {
        return Some(0);
    }
    if candidate.is_empty() {
        return None;
    }
    if candidate == query {
        return Some(10_000);
    }
    if candidate.starts_with(&query) {
        return Some(8_000 - candidate.len() as i32);
    }
    if let Some(position) = candidate.find(&query) {
        return Some(6_000 - position as i32 - candidate.len() as i32);
    }

    None
}

pub fn normalize_for_match(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn display_browser_name(browser: &str) -> &'static str {
    match browser {
        "chrome" => "Chrome",
        "edge" => "Edge",
        _ => "Browser",
    }
}

fn result_kind_order(kind: &SearchResultKind) -> i32 {
    match kind {
        SearchResultKind::Window => 0,
        SearchResultKind::Tab => 1,
        SearchResultKind::App => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(hwnd: isize, title: &str, app_name: &str, class_name: &str) -> WindowEntry {
        WindowEntry {
            hwnd,
            title: title.to_string(),
            app_name: app_name.to_string(),
            class_name: class_name.to_string(),
            screen_number: Some(1),
            desktop_location: DesktopLocation::Current,
            minimized: false,
            has_thumbnail: true,
        }
    }

    fn tab(
        title: &str,
        source: TabSource,
        parent_hwnd: Option<isize>,
        ids: Option<(i64, i64)>,
        now: Instant,
    ) -> TabEntry {
        TabEntry {
            browser: "chrome".to_string(),
            parent_hwnd,
            window_title: parent_hwnd.map(|_| "Chrome".to_string()),
            title: title.to_string(),
            active: false,
            extension_window_id: ids.map(|ids| ids.0),
            extension_tab_id: ids.map(|ids| ids.1),
            source,
            last_seen: now,
        }
    }

    fn active_extension_tab(title: &str, ids: (i64, i64), now: Instant) -> TabEntry {
        TabEntry {
            browser: "chrome".to_string(),
            parent_hwnd: None,
            window_title: None,
            title: title.to_string(),
            active: true,
            extension_window_id: Some(ids.0),
            extension_tab_id: Some(ids.1),
            source: TabSource::Extension,
            last_seen: now,
        }
    }

    fn app(name: &str, launch_path: &str, source: AppSource) -> AppEntry {
        AppEntry {
            name: name.to_string(),
            launch_path: launch_path.to_string(),
            source,
        }
    }

    #[test]
    fn match_score_prefers_exact_prefix_then_contains() {
        let exact = match_score("notes", "notes").unwrap();
        let prefix = match_score("notes", "notes from standup").unwrap();
        let contains = match_score("notes", "weekly notes from standup").unwrap();
        assert!(exact > prefix);
        assert!(prefix > contains);
        assert!(match_score("zzz", "notes from standup").is_none());
    }

    #[test]
    fn match_score_requires_contiguous_query_text() {
        assert!(match_score("alt tab", "Mega Win Alt Tab design doc").is_some());
        assert!(match_score("sign", "Signal").is_some());
        assert!(match_score("nfs", "notes from standup").is_none());
        assert!(match_score("sgn", "Signal").is_none());
    }

    #[test]
    fn empty_query_returns_windows_only_in_current_order() {
        let windows = vec![
            window(1, "First", "app", "class"),
            window(2, "Second", "app", "class"),
        ];
        let now = Instant::now();
        let tabs = vec![tab(
            "First tab",
            TabSource::Extension,
            None,
            Some((1, 10)),
            now,
        )];

        let results = build_results("", &windows, &[], &tabs);
        assert_eq!(results.len(), 2);
        assert!(matches!(
            results[0].target,
            ActivationTarget::Window { hwnd: 1 }
        ));
        assert!(results
            .iter()
            .all(|result| result.kind == SearchResultKind::Window));
    }

    #[test]
    fn search_includes_matching_windows_and_tabs() {
        let windows = vec![window(1, "Project README", "notepad", "Notepad")];
        let now = Instant::now();
        let tabs = vec![tab(
            "Mega Win Alt Tab design doc",
            TabSource::Extension,
            None,
            Some((2, 20)),
            now,
        )];

        let results = build_results("alt tab", &windows, &[], &tabs);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Tab);
    }

    #[test]
    fn app_results_match_contiguous_search_text() {
        let apps = vec![
            app(
                "Snipping Tool",
                "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\Snipping Tool.lnk",
                AppSource::AllUsersStartMenu,
            ),
            app(
                "Signal",
                "C:\\Users\\Example\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Signal.lnk",
                AppSource::UserStartMenu,
            ),
        ];

        let results = build_app_results("sign", &apps);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::App);
        assert_eq!(results[0].title, "Signal");
        assert!(matches!(results[0].target, ActivationTarget::App { .. }));

        let non_contiguous = build_app_results("sgn", &apps);
        assert!(non_contiguous.is_empty());
    }

    #[test]
    fn app_results_match_substrings_inside_app_names() {
        let apps = vec![app(
            "Thunderbird",
            "C:\\Program Files\\Mozilla Thunderbird\\thunderbird.exe",
            AppSource::UserAppPath,
        )];

        let results = build_app_results("thunder", &apps);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Thunderbird");
    }

    #[test]
    fn app_dedupe_prefers_current_user_shortcut() {
        let apps = vec![
            app(
                "Signal",
                "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\Signal.lnk",
                AppSource::AllUsersStartMenu,
            ),
            app(
                "Signal",
                "C:\\Users\\Example\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Signal.lnk",
                AppSource::UserStartMenu,
            ),
        ];

        let deduped = dedupe_apps(&apps);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].source, AppSource::UserStartMenu);
        assert!(deduped[0].launch_path.contains("Roaming"));
    }

    #[test]
    fn app_mode_results_do_not_include_window_or_tab_entries() {
        let apps = vec![app(
            "Signal",
            "C:\\Users\\Example\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Signal.lnk",
            AppSource::UserStartMenu,
        )];

        let results = build_app_results("", &apps);
        assert_eq!(results.len(), 1);
        assert!(results
            .iter()
            .all(|result| result.kind == SearchResultKind::App));
    }

    #[test]
    fn all_desktop_mode_labels_window_desktop_location() {
        let mut current = window(1, "Current Project", "notepad", "Notepad");
        current.desktop_location = DesktopLocation::Current;
        let mut other = window(2, "Other Project", "code", "Chrome_WidgetWin_1");
        other.desktop_location = DesktopLocation::Other;

        let results = build_results_with_options(
            "project",
            &[current, other],
            &[],
            &[],
            BuildResultOptions {
                show_desktop_labels: true,
            },
        );

        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .any(|result| result.subtitle.ends_with("Current desktop")));
        assert!(results
            .iter()
            .any(|result| result.subtitle.ends_with("Other desktop")));
    }

    #[test]
    fn default_results_do_not_label_desktop_location() {
        let mut other = window(2, "Other Project", "code", "Chrome_WidgetWin_1");
        other.desktop_location = DesktopLocation::Other;

        let results = build_results("project", &[other], &[], &[]);

        assert_eq!(results.len(), 1);
        assert!(!results[0].subtitle.contains("desktop"));
    }

    #[test]
    fn all_desktop_mode_labels_tab_parent_desktop_location() {
        let mut chrome = window(42, "Chrome", "chrome", "Chrome_WidgetWin_1");
        chrome.desktop_location = DesktopLocation::Other;
        let now = Instant::now();
        let tabs = vec![tab(
            "Planning",
            TabSource::Accessibility,
            Some(42),
            None,
            now,
        )];

        let results = build_results_with_options(
            "planning",
            &[chrome],
            &tabs,
            &[],
            BuildResultOptions {
                show_desktop_labels: true,
            },
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Tab);
        assert!(results[0].subtitle.ends_with("Other desktop"));
    }

    #[test]
    fn search_prefers_tab_over_matching_chrome_window() {
        let windows = vec![window(
            42,
            "Calendar - Google Chrome",
            "chrome",
            "Chrome_WidgetWin_1",
        )];
        let now = Instant::now();
        let tabs = vec![tab(
            "Calendar",
            TabSource::Accessibility,
            Some(42),
            None,
            now,
        )];

        let results = build_results("calendar", &windows, &tabs, &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Tab);
        assert!(matches!(
            results[0].target,
            ActivationTarget::Tab {
                parent_hwnd: Some(42),
                ..
            }
        ));
    }

    #[test]
    fn active_extension_tab_can_suppress_matching_chrome_window_without_parent() {
        let windows = vec![window(
            42,
            "Inbox - Work - Google Chrome",
            "chrome",
            "Chrome_WidgetWin_1",
        )];
        let now = Instant::now();
        let tabs = vec![active_extension_tab("Inbox", (1, 2), now)];

        let results = build_results("inbox", &windows, &[], &tabs);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Tab);
    }

    #[test]
    fn tab_results_inherit_parent_window_screen_number() {
        let windows = vec![window(42, "Chrome", "chrome", "Chrome_WidgetWin_1")];
        let now = Instant::now();
        let tabs = vec![tab(
            "Planning notes",
            TabSource::Accessibility,
            Some(42),
            None,
            now,
        )];

        let results = build_results("planning", &windows, &tabs, &[]);
        assert_eq!(results[0].screen_number, Some(1));
    }

    #[test]
    fn merge_prefers_extension_identity_and_adds_accessibility_parent() {
        let now = Instant::now();
        let accessibility = vec![tab(
            "Calendar - Work",
            TabSource::Accessibility,
            Some(42),
            None,
            now,
        )];
        let extension = vec![tab(
            "Calendar - Work",
            TabSource::Extension,
            None,
            Some((7, 77)),
            now,
        )];

        let merged = merge_tabs(&accessibility, &extension);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].parent_hwnd, Some(42));
        assert_eq!(merged[0].extension_tab_id, Some(77));
        assert_eq!(merged[0].source, TabSource::Extension);
    }

    #[test]
    fn stale_tabs_are_removed() {
        let now = Instant::now();
        let mut tabs = vec![
            tab("Fresh", TabSource::Extension, None, Some((1, 1)), now),
            tab(
                "Old",
                TabSource::Extension,
                None,
                Some((1, 2)),
                now - Duration::from_secs(60),
            ),
        ];

        prune_stale_tabs(&mut tabs, now, Duration::from_secs(30));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Fresh");
    }
}
