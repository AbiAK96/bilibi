//! The `/workspace` page and its mounted state (Bilimbi
//! `Tiling.Web.WorkspaceLive`).
//!
//! The tree is always in the URL: `?t=` carries the layout after every change,
//! so a reload or a copied address reproduces the screen, and Back and Forward
//! work at the workspace level. `?open=` is how a page is tiled in place: the
//! sidebar's tile control links to `/workspace?t=<that page>&inplace=1&open=<the
//! clicked page>`, the page in `open` takes half of the largest tile, and the
//! address is replaced by the resulting tree, so `open` is never replayed by a
//! reload. `inplace=1` marks a workspace entered from a page: closed down to
//! one tile, it leaves for that page's own address.
//!
//! Each tile shows its page in a same-origin frame at the page's own URL; the
//! tile is judged by that route's access before the frame is drawn, so a tile
//! the account may not open shows the refusal instead of a frame. The page in
//! a frame is its own live session: it mounts, authorizes and refreshes
//! exactly as when opened alone, and nothing here reads its state. The Tiling
//! hook (base/ui `assets/tiling.js`) reports where each frame went and what
//! its page is called; the tree records the path and replaces the address,
//! so Back steps through the workspace's own changes and then leaves it.
//! Back and Forward within the workspace arrive as params (`data-history`),
//! and the tree read back is reconciled with the one on screen, so the frames
//! that stay keep their documents.
//!
//! Tiles talk through the follow channel (base/ui `src/workspace.rs`). A tile
//! showing one record's page can follow selections: the tree keeps that page's
//! route pattern (`/companies/42>/companies/:id`), and when a page in any tile
//! announces a record of the module that owns the pattern, this page fills the
//! id in, records the new path and tells the hook to send the frame there
//! (`tile_navigate`). The kinds the tiles follow are rendered as
//! `data-follows`, which the hook passes to every frame, so a list whose kind
//! is followed selects instead of navigating.
use crate::layout::{At, Direction, Layout, Leaf, Node, Rect, Side, DEFAULT_VIEWPORT};
use askama::Template;
use bilibi_database::{Db, Error, Result};
use bilibi_menu::Menu;
use bilibi_module_registry::contributions::{Access, Method, Route};
use bilibi_tenancy::Scope;
use bilibi_ui::{
    lists::EmptyState,
    page::{
        LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState, MountFuture, Page, PageBody,
        PageFuture, PageRequest,
    },
    Button, ButtonVariant, FlashMap, Html, StatusKind, Tab, NO_FLASH,
};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

const WORKSPACE: &str = "/workspace";

struct Core {
    db: Db,
    menu: Arc<Menu>,
    /// Every discovered route with the module that contributes it.
    routes: Vec<(&'static str, &'static Route)>,
}
pub struct Pages(Arc<Core>);
impl Pages {
    pub fn new(db: Db, menu: Arc<Menu>, routes: Vec<(&'static str, &'static Route)>) -> Self {
        Self(Arc::new(Core { db, menu, routes }))
    }
}

/// What a tile shows, judged by the same route access the page applies.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Reach {
    Open,
    Forbidden,
    /// No framed page of this deployment serves the path.
    Unserved,
}

/// One destination the page picker offers.
struct PickerPage {
    id: String,
    label: String,
    route: String,
    trail: String,
}

/// The actor's standing, read once per render: the tiles and the picker are
/// judged from it.
struct Standing {
    capabilities: BTreeSet<&'static str>,
    operator: bool,
}

impl Core {
    async fn standing(&self, scope: &Scope) -> Result<Standing> {
        let client = self.db.get().await?;
        let capabilities = bilibi_authz::effective_capabilities(&**client, scope).await?;
        let operator = match bilibi_tenancy::prove_operator(&**client, scope).await {
            Ok(_) => true,
            Err(Error::Forbidden) => false,
            Err(error) => return Err(error),
        };
        Ok(Standing {
            capabilities,
            operator,
        })
    }

    /// The discovered GET page serving `path`, literal segments before
    /// parameters, as Web resolves it.
    fn route(&self, path: &str) -> Option<&'static Route> {
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let segments: Vec<_> = path.split('/').collect();
        self.routes
            .iter()
            .map(|(_, route)| *route)
            .filter(|route| route.method == Method::Get)
            .filter_map(|route| {
                let pattern: Vec<_> = route.path.split('/').collect();
                if pattern.len() != segments.len() {
                    return None;
                }
                let mut literal = 0;
                for (pattern, value) in pattern.iter().zip(&segments) {
                    if pattern.starts_with('{') {
                        if value.is_empty() || [".", ".."].contains(value) {
                            return None;
                        }
                    } else if pattern == value {
                        literal += 1;
                    } else {
                        return None;
                    }
                }
                Some((literal, route))
            })
            .max_by_key(|(literal, _)| *literal)
            .map(|(_, route)| route)
    }

    /// The route pattern a tile at `path` may follow: its page's own pattern
    /// when that names one record, such as `/companies/:id`.
    fn follow_pattern(&self, path: &str) -> Option<String> {
        let route = self.route(path).filter(|route| route.framed)?;
        let pattern = pattern(route.path);
        crate::layout::follow_pattern(&pattern).then_some(pattern)
    }

    /// The module that owns the page `pattern` names: the kind of record a
    /// tile following it goes to.
    fn pattern_kind(&self, pattern: &str) -> Option<&'static str> {
        self.routes
            .iter()
            .find(|(_, route)| {
                route.method == Method::Get && route.framed && self::pattern(route.path) == pattern
            })
            .map(|(module, _)| *module)
    }

    /// The kinds the tiles of `layout` follow, each once.
    fn follow_kinds(&self, layout: &Layout) -> Vec<&'static str> {
        let mut kinds = Vec::new();
        for kind in layout
            .follows()
            .into_iter()
            .filter_map(|pattern| self.pattern_kind(pattern))
        {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        kinds
    }

    fn reach(&self, standing: &Standing, path: &str) -> Reach {
        let Some(route) = self.route(path).filter(|route| route.framed) else {
            return Reach::Unserved;
        };
        let allowed = match route.access {
            Access::Public | Access::SignedIn => true,
            Access::Capability(key) => standing.capabilities.contains(key),
            Access::Operator => standing.operator,
            Access::OperatorCapability(key) => {
                standing.operator && standing.capabilities.contains(key)
            }
        };
        if allowed {
            Reach::Open
        } else {
            Reach::Forbidden
        }
    }

    /// Every leaf of the navigation this account sees, minus the workspace.
    fn pages(&self, standing: &Standing) -> Vec<PickerPage> {
        fn walk(nodes: &[bilibi_menu::Node], trail: &[&str], out: &mut Vec<PickerPage>) {
            for node in nodes {
                let item = node.item.item;
                if let Some(route) = item.route.filter(|route| *route != WORKSPACE) {
                    out.push(PickerPage {
                        id: item.id.replace('.', "-"),
                        label: item.label.to_owned(),
                        route: route.to_owned(),
                        trail: trail.join(" › "),
                    });
                }
                let mut deeper = trail.to_vec();
                deeper.push(item.label);
                walk(&node.children, &deeper, out);
            }
        }
        let tree = self
            .menu
            .visible_tree(&|key| standing.capabilities.contains(key));
        let mut out = Vec::new();
        walk(&tree, &[], &mut out);
        out
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Viewport {
        w: f64,
        h: f64,
    },
    /// A frame loaded or its live page changed address; `title` is the name
    /// the page gives itself.
    TileNavigated {
        id: String,
        path: String,
        title: Option<String>,
    },
    /// The narrow-screen tab strip names its value `tab`.
    FocusTile {
        #[serde(alias = "tab")]
        id: String,
    },
    FocusDirection {
        side: String,
    },
    MoveTile {
        id: String,
        side: String,
    },
    /// `with` names the tile a drag dropped this one on; `side` the neighbour
    /// the keyboard mode chose; neither means the nearest neighbour.
    SwapTile {
        id: String,
        with: Option<String>,
        side: Option<String>,
    },
    MakeMaster {
        id: String,
    },
    CloseTile {
        id: String,
    },
    OpenAlone {
        id: String,
    },
    /// The tiling mode's `w`: follow the records other tiles select.
    FollowTile {
        id: String,
    },
    UnfollowTile {
        id: String,
    },
    /// A page in a tile opened or selected a record. The hook checked it came
    /// from one of this workspace's frames; it is checked again here.
    RecordSelected {
        kind: String,
        id: String,
    },
    ToggleMonocle {
        id: Option<String>,
    },
    ToggleSplit {
        id: String,
    },
    ResizeSplit {
        id: String,
        ratio: f64,
    },
    NudgeSplit {
        id: String,
        side: String,
    },
    ResizeStep {
        side: String,
    },
    OpenPicker {
        id: Option<String>,
    },
    ClosePicker,
    AddTile {
        path: String,
    },
    #[serde(rename = "clear-flash")]
    ClearFlash {
        key: String,
    },
}

/// The workspace one browser tab holds. Everything else is derived on render.
struct Workspace {
    layout: Layout,
    focused: Option<String>,
    monocle: bool,
    /// Master mode keeps the first tile as master; a saved layout chooses it.
    master: bool,
    viewport: (f64, f64),
    in_place: bool,
    framed: bool,
    picker_open: bool,
    picker_for: Option<String>,
    flash: Option<(StatusKind, String)>,
    /// A full navigation the next reply asks for instead of a patch.
    leave: Option<String>,
    /// Pushes to the Tiling hook the next reply carries.
    pushes: Vec<(&'static str, serde_json::Value)>,
    /// The names the frames report, by tile id; a tile is named by its path
    /// until its page reports.
    titles: HashMap<String, String>,
}

impl Workspace {
    /// The workspace an address describes. `open` halves the largest tile of
    /// the tree in `t`; every other parameter is ignored.
    fn load(
        core: &Core,
        standing: &Standing,
        query: &HashMap<String, String>,
        framed: bool,
    ) -> Self {
        let mut workspace = Self {
            layout: Layout::empty(),
            focused: None,
            monocle: false,
            master: false,
            viewport: DEFAULT_VIEWPORT,
            in_place: query.get("inplace").is_some_and(|v| v == "1"),
            framed,
            picker_open: false,
            picker_for: None,
            flash: None,
            leave: None,
            pushes: Vec::new(),
            titles: HashMap::new(),
        };
        if framed {
            return workspace;
        }
        match Layout::decode(query.get("t").map_or("", String::as_str)) {
            Some(layout) => workspace.layout = layout,
            None => {
                workspace.flash = Some((
                    StatusKind::Error,
                    "The layout in the address could not be read, so the workspace opened empty."
                        .into(),
                ))
            }
        }
        workspace.keep_focus();
        if let Some(path) = query.get("open") {
            workspace.open_page(core, standing, path);
        }
        workspace
    }

    /// Tiles `path` in place: it takes half of the largest tile. It accepts
    /// what a tile shows, a served page this account may open, except the
    /// workspace itself.
    fn open_page(&mut self, core: &Core, standing: &Standing, path: &str) {
        if crate::layout::page_path(path)
            && !workspace_page(path)
            && core.reach(standing, path) == Reach::Open
        {
            self.tile(path, At::Largest);
        } else {
            self.flash = Some((
                StatusKind::Error,
                format!("{path} cannot be opened in a tile."),
            ));
        }
    }

    fn tile(&mut self, path: &str, at: At<'_>) {
        let id = if self.master {
            self.layout.master_open(path)
        } else {
            self.layout.open(path, at, self.viewport)
        };
        self.picker_open = false;
        self.picker_for = None;
        self.monocle = false;
        self.focused = Some(id);
    }

    fn known(&self, id: &str) -> Option<String> {
        self.layout.fetch_leaf(id).map(|leaf| leaf.id.clone())
    }

    fn focus(&mut self, id: &str) {
        if let Some(id) = self.known(id) {
            self.focused = Some(id);
        }
    }

    /// After the tree changes under the focus, focus stays where it can.
    fn keep_focus(&mut self) {
        let leaves = self.layout.leaves();
        if !self
            .focused
            .as_deref()
            .is_some_and(|id| leaves.iter().any(|leaf| leaf.id == id))
        {
            self.focused = leaves.first().map(|leaf| leaf.id.clone());
        }
        if leaves.is_empty() {
            self.monocle = false;
        }
    }

    fn event(&mut self, core: &Core, standing: &Standing, event: Event) {
        let side = |side: &str| Side::parse(side);
        match event {
            Event::Viewport { w, h } => {
                if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
                    self.viewport = (w, h);
                }
            }
            // The tree keeps where a tile went, so a reload or a copied
            // address shows the page the tile shows now. A path no framed page
            // of this deployment serves is not recorded.
            Event::TileNavigated { id, path, title } => {
                if self.known(&id).is_some()
                    && path.len() <= 2048
                    && crate::layout::page_path(&path)
                    && core.route(&path).is_some()
                {
                    self.layout.update_path(&id, &path);
                    match title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                        Some(title) => {
                            self.titles.insert(id, title.chars().take(80).collect());
                        }
                        None => {
                            self.titles.remove(&id);
                        }
                    }
                }
            }
            Event::FocusTile { id } => self.focus(&id),
            Event::FocusDirection { side: s } => {
                if let (Some(s), Some(id)) = (side(&s), self.focused.clone()) {
                    if let Some(other) = self.layout.neighbor(&id, s) {
                        self.focused = Some(other);
                    }
                }
            }
            Event::MoveTile { id, side: s } => {
                if let Some(s) = side(&s) {
                    self.layout.move_tile(&id, s);
                }
            }
            Event::SwapTile { id, with, side: s } => {
                let partner = match (with, s.as_deref().and_then(side)) {
                    (Some(other), _) => self.known(&other),
                    (None, Some(s)) => self.layout.neighbor(&id, s),
                    (None, None) => [Side::Right, Side::Down, Side::Left, Side::Up]
                        .into_iter()
                        .find_map(|s| self.layout.neighbor(&id, s)),
                };
                if let Some(other) = partner {
                    self.layout.swap(&id, &other);
                }
            }
            Event::MakeMaster { id } => {
                if self.master {
                    self.layout.promote_master(&id);
                    self.focus(&id);
                } else {
                    self.flash = Some((
                        StatusKind::Info,
                        "This layout has no master tile. The saved layouts dialog switches a saved layout to master."
                            .into(),
                    ));
                }
            }
            // Closing down to one tile leaves a workspace entered by tiling in
            // place for that page's own address: it began from a full page,
            // and it ends as one. A page the account may not open stays in its
            // tile, where the refusal is readable.
            Event::CloseTile { id } => {
                if self.master {
                    self.layout.master_close(&id);
                } else {
                    self.layout.close(&id);
                }
                self.monocle = self.monocle && self.layout.count() > 1;
                self.titles.remove(&id);
                self.keep_focus();
                if self.in_place {
                    if let [only] = self.layout.leaves().as_slice() {
                        if core.reach(standing, &only.path) == Reach::Open {
                            self.leave = Some(only.path.clone());
                        }
                    }
                }
            }
            // The tile's own page with the full shell: a tile is a page that
            // stays reachable alone.
            Event::OpenAlone { id } => {
                if let Some(leaf) = self.layout.fetch_leaf(&id) {
                    if core.reach(standing, &leaf.path) == Reach::Open {
                        self.leave = Some(leaf.path.clone());
                    }
                }
            }
            // The tile follows the pattern of the page it shows now, so the
            // choice is made on a record the person can see. The key is the
            // only way to ask, so a page with nothing to follow says so.
            Event::FollowTile { id } => {
                if let Some(path) = self.layout.fetch_leaf(&id).map(|leaf| leaf.path.clone()) {
                    match core.follow_pattern(&path) {
                        Some(pattern) => self.layout.follow(&id, Some(&pattern)),
                        None => {
                            self.flash = Some((
                                StatusKind::Info,
                                "This tile's page shows no single record, so there is nothing for it to follow. Open a record in the tile first."
                                    .into(),
                            ))
                        }
                    }
                }
            }
            Event::UnfollowTile { id } => self.layout.follow(&id, None),
            // Every tile following a pattern of the module that owns the
            // record goes to that record's page, unless it is there already.
            Event::RecordSelected { kind, id } => {
                if !bilibi_ui::workspace::fact(&kind, &id) {
                    return;
                }
                let moves: Vec<(String, String)> = self
                    .layout
                    .leaves()
                    .into_iter()
                    .filter_map(|leaf| {
                        let pattern = leaf.follow.as_deref()?;
                        if core.pattern_kind(pattern) != Some(kind.as_str()) {
                            return None;
                        }
                        let path = fill_pattern(pattern, &id);
                        let current = leaf.path.split(['?', '#']).next().unwrap_or_default();
                        (current != path && core.route(&path).is_some())
                            .then(|| (leaf.id.clone(), path))
                    })
                    .collect();
                for (tile, path) in moves {
                    self.layout.update_path(&tile, &path);
                    self.titles.remove(&tile);
                    self.pushes.push((
                        "tile_navigate",
                        serde_json::json!({ "id": tile, "path": path }),
                    ));
                }
            }
            Event::ToggleMonocle { id } => {
                if let Some(id) = id {
                    self.focus(&id);
                }
                self.monocle = !self.monocle && self.focused.is_some();
            }
            Event::ToggleSplit { id } => {
                if self.master {
                    self.layout.toggle_master_orientation();
                } else {
                    self.layout.toggle_split(&id);
                }
            }
            Event::ResizeSplit { id, ratio } => self.layout.resize(&id, ratio),
            Event::NudgeSplit { id, side: s } => {
                if let Some(s) = side(&s) {
                    self.layout.nudge(&id, s);
                }
            }
            Event::ResizeStep { side: s } => {
                if let (Some(s), Some(id)) = (side(&s), self.focused.clone()) {
                    self.layout.resize_step(&id, s);
                }
            }
            Event::OpenPicker { id } => {
                self.picker_open = true;
                self.picker_for = id.and_then(|id| self.known(&id));
            }
            Event::ClosePicker => {
                self.picker_open = false;
                self.picker_for = None;
            }
            // Only a page the picker offers: a forged path changes nothing.
            Event::AddTile { path } => {
                if core.pages(standing).iter().any(|page| page.route == path) {
                    let at = self.picker_for.clone().or_else(|| self.focused.clone());
                    match at {
                        Some(at) => self.tile(&path, At::Tile(&at)),
                        None => self.tile(&path, At::Last),
                    }
                }
            }
            Event::ClearFlash { key } => {
                if self
                    .flash
                    .as_ref()
                    .is_some_and(|(kind, _)| kind.name() == key)
                {
                    self.flash = None;
                }
            }
        }
    }

    /// The address that reproduces this workspace.
    fn query(&self) -> HashMap<String, String> {
        let mut query = HashMap::new();
        let encoded = self.layout.encode();
        if !encoded.is_empty() {
            query.insert("t".to_owned(), encoded);
            if self.in_place {
                query.insert("inplace".to_owned(), "1".to_owned());
            }
        }
        query
    }

    fn render(&self, core: &Core, standing: &Standing) -> Result<String> {
        let internal = |_| Error::Internal;
        let mut flash = NO_FLASH;
        if let Some((kind, message)) = &self.flash {
            match kind {
                StatusKind::Success => flash.success = message,
                StatusKind::Error => flash.error = message,
                StatusKind::Warning => flash.warning = message,
                StatusKind::Info => flash.info = message,
            }
        }
        if self.framed {
            let mut state = EmptyState::missing(
                "The workspace cannot open inside a tile",
                "A tile shows one page. Open the workspace on its own to tile pages.",
            );
            state.id = "workspace-framed-state".into();
            // A frame link that must leave the frame: `target="_top"` is the one
            // attribute the shared button does not carry.
            state.action = Html::raw(
                r#"<a id="workspace-framed-open" href="/workspace" target="_top" class="text-sm font-medium text-link underline hover:text-ink">Open the workspace</a>"#,
            );
            return FramedView { flash, state }.render().map_err(internal);
        }

        let rects = self.layout.rects();
        let master_id = self
            .master
            .then(|| rects.leaves.first().map(|(leaf, _)| leaf.id.clone()))
            .flatten();
        let mut tiles: Vec<TileView> = rects
            .leaves
            .iter()
            .map(|(leaf, rect)| {
                let focused = self.focused.as_deref() == Some(leaf.id.as_str());
                let reach = core.reach(standing, &leaf.path);
                TileView {
                    id: leaf.id.clone(),
                    path: leaf.path.clone(),
                    title: self
                        .titles
                        .get(&leaf.id)
                        .cloned()
                        .unwrap_or_else(|| leaf.path.clone()),
                    place: if self.monocle && focused {
                        place(Rect::FULL)
                    } else {
                        place(*rect)
                    },
                    focused,
                    hidden: self.monocle && !focused,
                    master: master_id.as_deref() == Some(leaf.id.as_str()),
                    following: leaf.follow.is_some(),
                    // "on" while the tile follows, which is what the mode's `w`
                    // reads to stop; "off" when its page names a record it
                    // could follow; nothing for any other page.
                    follow: if leaf.follow.is_some() {
                        Some("on")
                    } else {
                        core.follow_pattern(&leaf.path).map(|_| "off")
                    },
                    reach,
                    refusal: match reach {
                        Reach::Open => None,
                        Reach::Forbidden => Some(EmptyState::forbidden(format!("open {}", leaf.path))),
                        Reach::Unserved => Some(EmptyState::missing(
                            "This page is not available",
                            format!(
                                "Nothing in this deployment serves {}. Close the tile or add another page.",
                                leaf.path
                            ),
                        )),
                    },
                }
            })
            .collect();
        // Tiles render in creation order, never tree order: a swap or move that
        // reordered the elements would make the browser move a frame in the
        // DOM, and a moved frame reloads its page.
        tiles.sort_by(|a, b| (a.id.len(), &a.id).cmp(&(b.id.len(), &b.id)));
        let titles: HashMap<&str, &str> = tiles
            .iter()
            .map(|tile| (tile.id.as_str(), tile.title.as_str()))
            .collect();
        let handles = if self.monocle {
            Vec::new()
        } else {
            rects
                .handles
                .iter()
                .map(|(split, rect)| HandleView {
                    id: split.id.clone(),
                    horizontal: split.direction == Direction::H,
                    place: handle_place(split.direction, split.ratio, *rect),
                    rect: [rect.x, rect.y, rect.w, rect.h]
                        .iter()
                        .map(|v| format!("{}", (v * 10_000.0).round() / 10_000.0))
                        .collect::<Vec<_>>()
                        .join(" "),
                    label: format!(
                        "Resize {} and {}",
                        first_title(&split.first, &titles),
                        first_title(&split.second, &titles)
                    ),
                })
                .collect()
        };
        let tabs = tiles
            .iter()
            .map(|tile| Tab::button("focus_tile", &tile.id, &tile.title, tile.focused))
            .collect();
        let empty = if tiles.is_empty() {
            let mut state = EmptyState::missing(
                "No pages open",
                "Add a page to start tiling. Each new page takes half of the focused tile, and every tile stays a page with its own address.",
            );
            state.id = "workspace-empty-state".into();
            state.action = Html::render(&Button {
                variant: ButtonVariant::Primary,
                id: "workspace-empty-add",
                event: "open_picker",
                body: Html::text("Add a page"),
                ..Button::default()
            })
            .map_err(internal)?;
            Some(state)
        } else {
            None
        };
        let pages = if self.picker_open {
            core.pages(standing)
        } else {
            Vec::new()
        };
        let picker_empty = {
            let mut state = EmptyState::missing(
                "No pages to add",
                "No destinations are available for this account. Ask an operator to assign a role.",
            );
            state.id = "workspace-picker-empty".into();
            state
        };
        WorkspaceView {
            flash,
            follows: core.follow_kinds(&self.layout).join(" "),
            focused: self.focused.clone().unwrap_or_default(),
            monocle: self.monocle,
            mode: if self.master { "master" } else { "dwindle" },
            tiles: &tiles,
            handles,
            tabs,
            empty,
            picker_open: self.picker_open,
            picker_description: if self.master {
                "The page joins the master layout's stack. Every tile is a live page, so many tiles cost as much as many tabs."
            } else {
                "The page opens in a new tile beside the focused one. Every tile is a live page, so many tiles cost as much as many tabs."
            },
            pages,
            picker_empty,
        }
        .render()
        .map_err(internal)
    }
}

/// A route path in the tree's pattern form: `/companies/{id}` is
/// `/companies/:id`, as Bilimbi writes it in a saved layout.
fn pattern(route: &str) -> String {
    route
        .split('/')
        .map(
            |segment| match segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(name) => format!(":{name}"),
                None => segment.to_owned(),
            },
        )
        .collect::<Vec<_>>()
        .join("/")
}

/// `pattern` with its one parameter filled by `id`, percent-encoded.
fn fill_pattern(pattern: &str, id: &str) -> String {
    let segment: String = id
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    pattern
        .split('/')
        .map(|part| {
            if part.starts_with(':') {
                segment.as_str()
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn workspace_page(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    path == WORKSPACE || path.starts_with("/workspace/")
}

fn pct(fraction: f64) -> String {
    format!("{}%", (fraction * 100_000.0).round() / 1000.0)
}

fn place(rect: Rect) -> String {
    format!(
        "left: {}; top: {}; width: {}; height: {}",
        pct(rect.x),
        pct(rect.y),
        pct(rect.w),
        pct(rect.h)
    )
}

fn handle_place(direction: Direction, ratio: f64, rect: Rect) -> String {
    match direction {
        Direction::H => format!(
            "left: {}; top: {}; height: {}",
            pct(rect.x + rect.w * ratio),
            pct(rect.y),
            pct(rect.h)
        ),
        Direction::V => format!(
            "left: {}; top: {}; width: {}",
            pct(rect.x),
            pct(rect.y + rect.h * ratio),
            pct(rect.w)
        ),
    }
}

fn first_title<'a>(node: &'a Node, titles: &HashMap<&str, &'a str>) -> &'a str {
    match node {
        Node::Leaf(Leaf { id, .. }) => titles.get(id.as_str()).copied().unwrap_or(id),
        Node::Split(split) => first_title(&split.first, titles),
    }
}

struct TileView {
    id: String,
    path: String,
    title: String,
    place: String,
    focused: bool,
    hidden: bool,
    master: bool,
    following: bool,
    follow: Option<&'static str>,
    reach: Reach,
    refusal: Option<EmptyState>,
}
impl TileView {
    fn open(&self) -> bool {
        self.reach == Reach::Open
    }
    fn forbidden(&self) -> bool {
        self.reach == Reach::Forbidden
    }
}

struct HandleView {
    id: String,
    horizontal: bool,
    place: String,
    rect: String,
    label: String,
}

#[derive(Template)]
#[template(path = "workspace.html")]
struct WorkspaceView<'a> {
    flash: FlashMap<'a>,
    /// The kinds some tile follows, space separated, for the hook to pass to
    /// every frame.
    follows: String,
    focused: String,
    monocle: bool,
    mode: &'static str,
    tiles: &'a [TileView],
    handles: Vec<HandleView>,
    tabs: Vec<Tab<'a>>,
    empty: Option<EmptyState>,
    picker_open: bool,
    picker_description: &'static str,
    pages: Vec<PickerPage>,
    picker_empty: EmptyState,
}

#[derive(Template)]
#[template(path = "workspace_framed.html")]
struct FramedView<'a> {
    flash: FlashMap<'a>,
    state: EmptyState,
}

struct Mounted {
    core: Arc<Core>,
    workspace: Workspace,
}

impl Page<Scope, Error> for Pages {
    fn render<'a>(&'a self, request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async move {
            if request.operation != "index" {
                return Err(Error::NotFound);
            }
            let scope = request.scope.ok_or(Error::Unauthorized)?;
            let standing = self.0.standing(scope).await?;
            let workspace = Workspace::load(&self.0, &standing, request.query, request.framed);
            Ok(PageBody {
                html: workspace.render(&self.0, &standing)?,
                live: true,
            })
        })
    }
    fn mount<'a>(&'a self, request: PageRequest<'a, Scope>) -> MountFuture<'a, Scope, Error> {
        Box::pin(async move {
            if request.operation != "index" {
                return Err(Error::NotFound);
            }
            let scope = request.scope.ok_or(Error::Unauthorized)?;
            let standing = self.0.standing(scope).await?;
            let state: Box<dyn LiveState<Scope, Error>> = Box::new(Mounted {
                core: self.0.clone(),
                workspace: Workspace::load(&self.0, &standing, request.query, request.framed),
            });
            Ok(Some(state))
        })
    }
}

impl LiveState<Scope, Error> for Mounted {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            let standing = self.core.standing(request.scope).await?;
            let before = self.workspace.flash.clone();
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(values) => {
                    // `open` arrives from the sidebar's tile control as a patch
                    // of this address; it is applied to the reconciled tree, so
                    // the tiles on screen keep their ids and frames.
                    let mut values = values.clone();
                    let open = values.remove("open");
                    let framed = self.workspace.framed;
                    let previous = std::mem::replace(
                        &mut self.workspace,
                        Workspace::load(&self.core, &standing, &values, framed),
                    );
                    // A tree read back from the address keeps the frames on screen.
                    self.workspace.layout =
                        previous.layout.reconcile(self.workspace.layout.clone());
                    self.workspace.viewport = previous.viewport;
                    self.workspace.focused = previous.focused.clone();
                    self.workspace.titles = previous.titles;
                    self.workspace
                        .titles
                        .retain(|id, _| self.workspace.layout.fetch_leaf(id).is_some());
                    self.workspace.keep_focus();
                    self.workspace.monocle = previous.monocle
                        && self.workspace.focused == previous.focused
                        && self.workspace.layout.count() > 1;
                    if let Some(path) = open.filter(|_| !framed) {
                        self.workspace.open_page(&self.core, &standing, &path);
                    }
                }
                LiveInput::Event(value) => {
                    let event: Event =
                        serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?;
                    self.workspace.event(&self.core, &standing, event);
                }
            }
            let redirect = self.workspace.leave.take();
            let events = std::mem::take(&mut self.workspace.pushes);
            let notice = match &self.workspace.flash {
                Some((_, message)) if self.workspace.flash != before => message.clone(),
                _ => String::new(),
            };
            Ok(LiveBody {
                html: self.workspace.render(&self.core, &standing)?,
                query: self.workspace.query(),
                notice,
                rows: self.workspace.layout.count(),
                redirect,
                events,
            })
        })
    }
}
