//! Shared authenticated chrome. Owners supply already-authorized navigation and
//! account metadata; no identity or capability decision is made by a template.
//! Theme, time display and pins are the account's own (Core User); K17
//! supplies affiliation labels.
use crate::Html;
use askama::Template;

#[derive(Clone, Debug)]
pub struct DisplayPreferences {
    pub theme: String,
    pub mode: String,
    pub timezone: String,
}
impl Default for DisplayPreferences {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            mode: "company".into(),
            timezone: "UTC".into(),
        }
    }
}
impl DisplayPreferences {
    /// The theme toggles as (value, label, pressed), in Bilimbi's order.
    pub fn themes(&self) -> [(&'static str, &'static str, bool); 3] {
        [("light", "Light"), ("dark", "Dark"), ("system", "System")]
            .map(|(value, label)| (value, label, self.theme == value))
    }
    /// The time-display choices as (value, label, detail, pressed), in
    /// Bilimbi's order; company time names its zone.
    pub fn modes(&self) -> [(&'static str, &'static str, &str, bool); 3] {
        [
            ("company", "Company time", self.timezone.as_str()),
            ("local", "This device's local time", ""),
            ("utc", "Stored UTC", ""),
        ]
        .map(|(value, label, detail)| (value, label, detail, self.mode == value))
    }
    pub fn mode_label(&self) -> &str {
        match self.mode.as_str() {
            "utc" => "UTC",
            "local" => "Local",
            _ => "Company",
        }
    }
}

/// A visible menu node. Build it from Menu::visible_tree, never the full catalog.
#[derive(Clone, Debug)]
pub struct NavNode {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub route: Option<String>,
    pub active: bool,
    pub children: Vec<NavNode>,
}
impl NavNode {
    fn contains_active(&self) -> bool {
        self.active || self.children.iter().any(Self::contains_active)
    }
    fn render(&self, tileable: bool, pinnable: bool) -> askama::Result<Html> {
        let children = navigation_options(&self.children, tileable, pinnable)?;
        Html::render(&NavBranch {
            node: self,
            dom_id: self.id.replace('.', "-"),
            expanded: self.contains_active(),
            children,
            pinnable,
            tileable: tileable && self.route.as_deref() != Some("/workspace"),
        })
    }
}
pub fn navigation(nodes: &[NavNode], tileable: bool) -> askama::Result<Html> {
    navigation_options(nodes, tileable, true)
}
/// The shell's own rows without sidebar-only pin/tile controls, for specimens.
pub fn navigation_unpinned(nodes: &[NavNode]) -> askama::Result<Html> {
    navigation_options(nodes, false, false)
}
fn navigation_options(nodes: &[NavNode], tileable: bool, pinnable: bool) -> askama::Result<Html> {
    nodes
        .iter()
        .map(|node| node.render(tileable, pinnable))
        .collect()
}
#[derive(Template)]
#[template(path = "navigation.html")]
struct NavBranch<'a> {
    node: &'a NavNode,
    dom_id: String,
    expanded: bool,
    children: Html,
    tileable: bool,
    pinnable: bool,
}

/// Display-only scope. The host must obtain operator standing from an explicit
/// proof; a tenant ID or administrator role never implies it.
pub struct ShellContext<'a> {
    pub user: &'a str,
    pub email: &'a str,
    pub tenant: &'a str,
    pub tenant_id: i64,
    pub user_id: i64,
    pub company: Option<&'a str>,
    pub platform_operator: bool,
    pub csrf: &'a str,
    pub preferences: DisplayPreferences,
    pub navigation: Html,
    /// Core owns the bell. This slot permits its later embedded live panel.
    pub notifications: Html,
    pub home: &'a str,
    pub served_routes: String,
    /// The account's pins as JSON (`[{id,label,url,icon,sort_order}]`), already
    /// limited to pages this deployment serves.
    pub pins: String,
    pub development: bool,
    /// The host the development server listens on, shown after "dev" in the
    /// status bar as Bilimbi's `listen_address`; `None` outside development.
    pub listen_address: Option<&'a str>,
    pub version: &'a str,
    /// The open page's controls in the top bar, left of the bell (Bilimbi's
    /// `topbar_actions` slot), from `Page::topbar_actions`.
    pub topbar_actions: Html,
}
impl ShellContext<'_> {
    pub fn initials(&self) -> String {
        self.user
            .split_whitespace()
            .take(2)
            .filter_map(|part| part.chars().next())
            .collect::<String>()
            .to_uppercase()
    }
}

#[derive(Template)]
#[template(path = "shell.html")]
pub struct AppShell<'a> {
    pub context: &'a ShellContext<'a>,
    pub live: bool,
    pub body: &'a str,
}

#[derive(Template)]
#[template(path = "framed.html")]
pub struct Framed<'a> {
    pub body: &'a str,
    pub live: bool,
    pub tenant_id: i64,
    pub user_id: i64,
    pub preferences: &'a DisplayPreferences,
}
