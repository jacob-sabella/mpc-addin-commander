//! A plugin skin (`TUI.json`, see `docs/SKIN.md`) resolved into pages, and each page laid out
//! as a flat draw list and a flat control list from live parameter values.
//!
//! This crate has no network and no GPU dependency: it parses, resolves placed instances of
//! local component definitions, maps handles to parameter indices, applies
//! `IndexedEnabling` visibility, and tells the caller what to draw where and what a click or
//! drag at a point means. Drawing is the caller's job.

#![forbid(unsafe_code)]

pub mod model;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

/// The page area a skin lays out, in device pixels.
pub const PAGE_WIDTH: f32 = 1280.0;
/// The page area a skin lays out, in device pixels.
pub const PAGE_HEIGHT: f32 = 628.0;

/// Why a skin could not be loaded.
#[derive(Debug)]
pub enum Error {
    Json(serde_json::Error),
    /// A tab names a component definition that does not exist.
    MissingDefinition {
        page: String,
        definition: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Json(e) => write!(f, "TUI.json: {e}"),
            Error::MissingDefinition { page, definition } => {
                write!(
                    f,
                    "page {page:?} uses the missing definition {definition:?}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// An axis-aligned rectangle in page pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Parses `"x y w h"`; missing or malformed numbers read as 0.
    pub fn parse(s: &str) -> Self {
        let mut it = s
            .split_whitespace()
            .map(|v| v.parse::<f32>().unwrap_or(0.0));
        let mut next = || it.next().unwrap_or(0.0);
        Self::new(next(), next(), next(), next())
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// An `aarrggbb` colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Colour {
    pub const WHITE: Colour = Colour {
        a: 255,
        r: 255,
        g: 255,
        b: 255,
    };

    /// Parses `aarrggbb` or `rrggbb` hex; anything else is opaque light grey.
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        let hex = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("zz"), 16);
        match s.len() {
            8 => match (hex(0), hex(2), hex(4), hex(6)) {
                (Ok(a), Ok(r), Ok(g), Ok(b)) => Colour { a, r, g, b },
                _ => Colour::fallback(),
            },
            6 => match (hex(0), hex(2), hex(4)) {
                (Ok(r), Ok(g), Ok(b)) => Colour { a: 255, r, g, b },
                _ => Colour::fallback(),
            },
            _ => Colour::fallback(),
        }
    }

    const fn fallback() -> Self {
        Colour {
            a: 255,
            r: 200,
            g: 200,
            b: 200,
        }
    }
}

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    Left,
    #[default]
    Centre,
    Right,
}

/// Vertical text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    Top,
    #[default]
    Centre,
    Bottom,
}

/// A JUCE justification string, reduced to its two axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Justification {
    pub h: HAlign,
    pub v: VAlign,
}

impl Justification {
    /// `left`, `right`, `horizontallyCentred`, `top`, `bottom`, `verticallyCentred` in any order.
    pub fn parse(s: &str) -> Self {
        let mut j = Justification::default();
        for word in s.split_whitespace() {
            match word {
                "left" => j.h = HAlign::Left,
                "right" => j.h = HAlign::Right,
                "top" => j.v = VAlign::Top,
                "bottom" => j.v = VAlign::Bottom,
                _ => {}
            }
        }
        j
    }
}

/// A font request: family name, style and pixel height.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSpec {
    pub name: String,
    pub style: String,
    pub height: f32,
}

/// How a label draws its text.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelStyle {
    pub font: FontSpec,
    pub colour: Colour,
    pub justification: Justification,
    pub uppercase: bool,
}

/// What a label shows: the bound parameter's name, or its display text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    Name,
    Value,
}

/// Live parameter data, looked up by VST index.
pub trait ParamSource {
    /// The value, 0 to 1; 0 for an unknown parameter.
    fn value(&self, param: u32) -> f32;
    /// The parameter's name; empty for an unknown parameter.
    fn name(&self, param: u32) -> String;
    /// The parameter's display text; empty for an unknown parameter.
    fn text(&self, param: u32) -> String;
}

/// One parameter's state, for [`ParamSource`] implementations backed by a map.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParamState {
    pub value: f32,
    pub name: String,
    pub text: String,
}

impl ParamSource for HashMap<u32, ParamState> {
    fn value(&self, param: u32) -> f32 {
        self.get(&param).map_or(0.0, |p| p.value)
    }

    fn name(&self, param: u32) -> String {
        self.get(&param)
            .map_or_else(String::new, |p| p.name.clone())
    }

    fn text(&self, param: u32) -> String {
        self.get(&param)
            .map_or_else(String::new, |p| p.text.clone())
    }
}

/// One thing to draw, in page pixels; the list order is the draw order.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// The PNG `file`, stretched to `rect`.
    Image { rect: Rect, file: String },
    /// `rect` filled with `colour` (a definition's background colour).
    Fill { rect: Rect, colour: Colour },
    /// Frame `frame` of the `frames`-frame vertical filmstrip `file`.
    Knob {
        rect: Rect,
        file: String,
        frames: u32,
        frame: u32,
        param: Option<u32>,
    },
    /// The PNG `file`: the on image or the off image, whichever the value picked.
    Button {
        rect: Rect,
        file: String,
        on: bool,
        param: Option<u32>,
    },
    /// A track-and-thumb slider: the PNG `thumb` at its natural size, slid along `rect` by
    /// `value` (bottom to top when `vertical`, left to right otherwise).
    Slider {
        rect: Rect,
        thumb: String,
        vertical: bool,
        value: f32,
        param: Option<u32>,
    },
    /// A down-pointing triangle filling `rect` (a menu's arrow).
    Arrow { rect: Rect, colour: Colour },
    /// `text` in `style`, inside `rect`.
    Label {
        rect: Rect,
        text: String,
        style: LabelStyle,
        param: Option<u32>,
    },
    /// The generic meter bar for a value.
    Meter {
        rect: Rect,
        value: f32,
        param: Option<u32>,
    },
    /// A component type the renderer does not know: draw the generic widget for its parameter.
    Generic {
        rect: Rect,
        kind: String,
        name: String,
        param: Option<u32>,
    },
}

impl Item {
    pub fn rect(&self) -> Rect {
        match self {
            Item::Image { rect, .. }
            | Item::Knob { rect, .. }
            | Item::Button { rect, .. }
            | Item::Slider { rect, .. }
            | Item::Arrow { rect, .. }
            | Item::Fill { rect, .. }
            | Item::Label { rect, .. }
            | Item::Meter { rect, .. }
            | Item::Generic { rect, .. } => *rect,
        }
    }
}

/// What a pointer does on a control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// Drag vertically; `height` page pixels of travel span the value range 0 to 1.
    Drag { height: f32 },
    /// A press sets this value (a button of a group).
    Set { value: f32 },
    /// A press flips the value between 0 and 1.
    Toggle,
    /// A press selects the control on the device and sets nothing.
    Select,
    /// A press does nothing, but is swallowed (a panel covering what is below it).
    Inert,
}

/// A hit-testable region of a page, in draw order (later controls are on top).
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub rect: Rect,
    pub param: Option<u32>,
    pub gesture: Gesture,
    /// The component's name in the skin, for logs and tooltips.
    pub name: String,
}

/// What a press on a control asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Send `set` once.
    Set { param: u32, value: f32 },
    /// Start a vertical drag from `start`, sending `set` as the pointer moves.
    Drag { param: u32, height: f32, start: f32 },
}

impl Control {
    /// The action a press on this control starts, given the current values.
    pub fn press(&self, params: &dyn ParamSource) -> Option<Action> {
        let param = self.param?;
        match self.gesture {
            Gesture::Drag { height } => Some(Action::Drag {
                param,
                height,
                start: params.value(param),
            }),
            Gesture::Set { value } => Some(Action::Set { param, value }),
            Gesture::Toggle => Some(Action::Set {
                param,
                value: if params.value(param) < 0.5 { 1.0 } else { 0.0 },
            }),
            Gesture::Select | Gesture::Inert => None,
        }
    }
}

/// The topmost control at a point.
pub fn hit(controls: &[Control], x: f32, y: f32) -> Option<&Control> {
    controls.iter().rev().find(|c| c.rect.contains(x, y))
}

/// A page's draw list and control list.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub items: Vec<Item>,
    pub controls: Vec<Control>,
}

/// The option an N-option parameter is on: `round(value * (N - 1))`.
pub fn option_index(value: f32, count: u32) -> u32 {
    let last = count.saturating_sub(1);
    let i = (value * last as f32).round();
    if i.is_nan() || i < 0.0 {
        0
    } else {
        (i as u32).min(last)
    }
}

/// `IndexedEnabling/<index>/<count>/Parameter <param>`: shown while the parameter is on option
/// `index` of `count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Condition {
    index: u32,
    count: u32,
    param: u32,
}

impl Condition {
    fn parse(handle: &str) -> Option<Self> {
        let rest = handle.strip_prefix("IndexedEnabling/")?;
        let mut parts = rest.splitn(3, '/');
        let index = parts.next()?.parse().ok()?;
        let count = parts.next()?.parse().ok()?;
        let param = parse_parameter(parts.next()?)?;
        Some(Condition {
            index,
            count,
            param,
        })
    }

    fn holds(&self, params: &dyn ParamSource) -> bool {
        option_index(params.value(self.param), self.count) == self.index
    }
}

/// `Parameter <n>` to `n`.
/// Lower case, letters and digits only.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// `short`'s letters appear in order in `long`, which starts with the same letter.
fn abbreviates(short: &str, long: &str) -> bool {
    if short.chars().next() != long.chars().next() {
        return false;
    }
    let mut rest = long.chars();
    short.chars().all(|c| rest.any(|l| l == c))
}

fn parse_parameter(s: &str) -> Option<u32> {
    s.trim().strip_prefix("Parameter ")?.trim().parse().ok()
}

#[derive(Debug, Clone, PartialEq)]
enum Widget {
    Image {
        file: String,
    },
    Knob {
        file: String,
        frames: u32,
        invert: bool,
        handle: String,
    },
    Button {
        on: String,
        off: String,
        id: u32,
        group: u32,
        handle: String,
        /// An `Indicator`: lit like a button, never pressed.
        lamp: bool,
    },
    Arrow {
        colour: Colour,
    },
    Fill {
        colour: Colour,
    },
    Slider {
        thumb: String,
        vertical: bool,
        handle: String,
    },
    Label {
        style: LabelStyle,
        kind: LabelKind,
        handle: String,
    },
    /// A `Knob` of `knobType: "ValueSlider"`: the value's text, dragged like a knob.
    ValueText {
        style: LabelStyle,
        handle: String,
    },
    Focus,
    Meter {
        handle: String,
    },
    /// A placed instance of the local definition `def`, with its handle map.
    Instance {
        def: String,
        map: Vec<(String, String)>,
    },
}

impl Widget {
    /// Makes the widget's images relative to the skin folder, from the folder of `file`.
    fn rebase(&mut self, file: &str) {
        let fix = |f: &mut String| {
            if !f.is_empty() {
                if let Some(p) = resolve_path(file, f) {
                    *f = p;
                }
            }
        };
        match self {
            Widget::Image { file } | Widget::Knob { file, .. } => fix(file),
            Widget::Slider { thumb, .. } => fix(thumb),
            Widget::Button { on, off, .. } => {
                fix(on);
                fix(off);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Node {
    name: String,
    rect: Rect,
    /// `rect` is in fractions of the parent's size.
    proportional: bool,
    /// `whenVisible: Always`; the focus ring (`WhenFocussed`) is never drawn.
    always: bool,
    conditions: Vec<Condition>,
    widget: Widget,
}

#[derive(Debug, Clone, PartialEq)]
struct Definition {
    toggles: bool,
    qlink: bool,
    children: Vec<Node>,
}

/// One page of a skin.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub name: String,
    /// The F-key the page is on, 0-based.
    pub fn_key: u32,
    /// The nested page under that F-key, 0 for the F-key's own page.
    pub sub_index: u32,
    /// The page area.
    pub size: Rect,
    definition: String,
}

/// A parsed, resolved skin.
#[derive(Debug, Clone)]
pub struct Skin {
    pages: Vec<Page>,
    definitions: HashMap<String, Definition>,
    imports: Vec<String>,
}

/// `rel` relative to the folder of `base`, both relative to the skin folder: `"../../A/x.json"`
/// and `"../B/y.json"` give `"../../B/y.json"`. `None` for an absolute path.
pub fn resolve_path(base: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for c in rel.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(p) if *p != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            c => parts.push(c),
        }
    }
    Some(parts.join("/"))
}

impl Skin {
    /// Parses a `TUI.json` document.
    pub fn parse(json: &str) -> Result<Self, Error> {
        let tui: model::Tui = serde_json::from_str(json)?;
        Self::from_tui(tui)
    }

    /// Resolves an already-deserialized document.
    pub fn from_tui(tui: model::Tui) -> Result<Self, Error> {
        let data = tui.page_data;
        let imports = data.component_definitions.import_files.clone();
        let mut definitions: HashMap<String, Definition> = data
            .component_definitions
            .local_component_definitions
            .into_iter()
            .map(|kv| (kv.key, convert_definition(kv.value)))
            .collect();
        let mut pages = Vec::new();
        for (i, mut tab) in data.tabs.into_iter().enumerate() {
            // An inline definition gets a key no file can use.
            if let Some(def) = tab.component_definition.take() {
                tab.component_name = format!("\0tab {i}");
                definitions.insert(tab.component_name.clone(), convert_definition(def));
            }
            if !definitions.contains_key(&tab.component_name) {
                return Err(Error::MissingDefinition {
                    page: tab.tab_name,
                    definition: tab.component_name,
                });
            }
            let mut size = Rect::parse(&tab.initial_size);
            if size.w <= 0.0 || size.h <= 0.0 {
                size = Rect::new(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT);
            }
            pages.push(Page {
                name: tab.tab_name,
                fn_key: tab.fn_key_index,
                sub_index: tab.fn_key_sub_index,
                size,
                definition: tab.component_name,
            });
        }
        pages.sort_by_key(|p| (p.fn_key, p.sub_index));
        Ok(Skin {
            pages,
            definitions,
            imports: imports
                .iter()
                .filter_map(|i| resolve_path("TUI.json", i))
                .collect(),
        })
    }

    /// The definition files the skin imports, relative to the skin folder (`../../X/y.json`);
    /// absolute paths are left out.
    pub fn imports(&self) -> &[String] {
        &self.imports
    }

    /// Adds the definitions of the imported file at `path` (as [`Skin::imports`] gives it) that
    /// the skin does not define itself; its images become relative to the skin folder. Returns
    /// the files it imports in turn.
    pub fn add_library(&mut self, path: &str, json: &str) -> Result<Vec<String>, Error> {
        let lib: model::Library = serde_json::from_str(json)?;
        let defs = lib.component_definitions;
        for kv in defs.local_component_definitions {
            if self.definitions.contains_key(&kv.key) {
                continue;
            }
            let mut def = convert_definition(kv.value);
            for node in &mut def.children {
                node.widget.rebase(path);
            }
            self.definitions.insert(kv.key, def);
        }
        Ok(defs
            .import_files
            .iter()
            .filter_map(|i| resolve_path(path, i))
            .collect())
    }

    /// The pages, sorted by F-key then sub-index.
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The page on an F-key (its own page, `sub_index` 0, or a nested one).
    pub fn page_index(&self, fn_key: u32, sub_index: u32) -> Option<usize> {
        self.pages
            .iter()
            .position(|p| p.fn_key == fn_key && p.sub_index == sub_index)
    }

    /// The definitions the pages reach, through placed instances.
    fn reachable(&self) -> Vec<&Definition> {
        let mut out = Vec::new();
        let mut todo: Vec<&str> = self.pages.iter().map(|p| p.definition.as_str()).collect();
        let mut seen = HashSet::new();
        while let Some(name) = todo.pop() {
            if !seen.insert(name) {
                continue;
            }
            let Some(def) = self.definitions.get(name) else {
                continue;
            };
            for node in &def.children {
                if let Widget::Instance { def, .. } = &node.widget {
                    todo.push(def);
                }
            }
            out.push(def);
        }
        out
    }

    /// The parameter each of `names` controls, matched against the names of the placed
    /// instances that bind `Parameter N`. Saved names can be shortened (`Rels` for a `Release`
    /// knob): an exact match (ignoring case and anything but letters and digits) wins, then
    /// one whose letters appear in order in the instance name, starting with the same letter.
    /// Each parameter is given out once.
    pub fn bind_names(&self, names: &[String]) -> Vec<Option<u32>> {
        let mut bound: Vec<(String, u32)> = Vec::new();
        for def in self.reachable() {
            for node in &def.children {
                if let Widget::Instance { map, .. } = &node.widget {
                    let param = map
                        .iter()
                        .find(|(k, _)| k == "Data")
                        .and_then(|(_, v)| parse_parameter(v));
                    if let Some(p) = param {
                        if !bound.iter().any(|(_, q)| *q == p) {
                            bound.push((squash(&node.name), p));
                        }
                    }
                }
            }
        }
        let wanted: Vec<String> = names.iter().map(|n| squash(n)).collect();
        let mut out = vec![None; names.len()];
        let mut taken = HashSet::new();
        let passes: [fn(&str, &str) -> bool; 2] = [|a, b| a == b, abbreviates];
        for pass in passes {
            for (k, w) in wanted.iter().enumerate() {
                if out[k].is_some() || w.is_empty() {
                    continue;
                }
                if let Some((_, p)) = bound.iter().find(|(b, p)| !taken.contains(p) && pass(w, b)) {
                    out[k] = Some(*p);
                    taken.insert(*p);
                }
            }
        }
        out
    }

    /// Every PNG the skin's pages can draw, relative to the skin folder. Definitions no page
    /// reaches (most of an imported library) are left out.
    pub fn image_files(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for def in self.reachable() {
            for node in &def.children {
                match &node.widget {
                    Widget::Image { file }
                    | Widget::Knob { file, .. }
                    | Widget::Slider { thumb: file, .. } => {
                        out.insert(file.clone());
                    }
                    Widget::Button { on, off, .. } => {
                        out.insert(on.clone());
                        out.insert(off.clone());
                    }
                    _ => {}
                }
            }
        }
        out.remove("");
        out
    }

    /// The draw list and control list of page `page` for these parameter values.
    pub fn layout(&self, page: usize, params: &dyn ParamSource) -> Layout {
        let mut out = Layout::default();
        if let Some(p) = self.pages.get(page) {
            let map = HashMap::new();
            self.walk(
                &p.definition,
                Rect::new(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT),
                &map,
                false,
                params,
                0,
                &mut out,
            );
        }
        out
    }

    /// The draw list of page `page`.
    pub fn draw_list(&self, page: usize, params: &dyn ParamSource) -> Vec<Item> {
        self.layout(page, params).items
    }

    /// The control list of page `page`.
    pub fn controls(&self, page: usize, params: &dyn ParamSource) -> Vec<Control> {
        self.layout(page, params).controls
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        def_name: &str,
        parent: Rect,
        map: &HashMap<String, u32>,
        buttons_set: bool,
        params: &dyn ParamSource,
        depth: u32,
        out: &mut Layout,
    ) {
        // Definitions can only nest so deep before the file is a cycle.
        if depth > 16 {
            return;
        }
        let Some(def) = self.definitions.get(def_name) else {
            return;
        };
        for node in &def.children {
            if !node.always || !node.conditions.iter().all(|c| c.holds(params)) {
                continue;
            }
            let rect = if node.proportional {
                let r = node.rect;
                Rect::new(
                    parent.x + r.x * parent.w,
                    parent.y + r.y * parent.h,
                    r.w * parent.w,
                    r.h * parent.h,
                )
            } else {
                node.rect.translate(parent.x, parent.y)
            };
            let bound = |handle: &str| map.get(handle).copied();
            match &node.widget {
                Widget::Image { file } => out.items.push(Item::Image {
                    rect,
                    file: file.clone(),
                }),
                Widget::Knob {
                    file,
                    frames,
                    invert,
                    handle,
                } => {
                    let param = bound(handle);
                    let value = param.map_or(0.0, |p| params.value(p));
                    let mut frame = option_index(value, *frames);
                    if *invert {
                        frame = frames.saturating_sub(1).saturating_sub(frame);
                    }
                    out.items.push(Item::Knob {
                        rect,
                        file: file.clone(),
                        frames: *frames,
                        frame,
                        param,
                    });
                }
                Widget::Arrow { colour } => out.items.push(Item::Arrow {
                    rect,
                    colour: *colour,
                }),
                Widget::Fill { colour } => out.items.push(Item::Fill {
                    rect,
                    colour: *colour,
                }),
                Widget::Button {
                    on,
                    off,
                    id,
                    group,
                    handle,
                    lamp,
                } => {
                    let param = bound(handle);
                    let value = param.map_or(0.0, |p| params.value(p));
                    let lit = if *group > 1 {
                        option_index(value, *group) == *id
                    } else {
                        value >= 0.5
                    };
                    out.items.push(Item::Button {
                        rect,
                        file: if lit { on.clone() } else { off.clone() },
                        on: lit,
                        param,
                    });
                    if buttons_set && param.is_some() && !*lamp {
                        let value = if *group > 1 {
                            *id as f32 / (*group - 1) as f32
                        } else {
                            1.0
                        };
                        out.controls.push(Control {
                            rect,
                            param,
                            gesture: Gesture::Set { value },
                            name: node.name.clone(),
                        });
                    }
                }
                Widget::Slider {
                    thumb,
                    vertical,
                    handle,
                } => {
                    let param = bound(handle);
                    out.items.push(Item::Slider {
                        rect,
                        thumb: thumb.clone(),
                        vertical: *vertical,
                        value: param.map_or(0.0, |p| params.value(p)),
                        param,
                    });
                }
                Widget::Label {
                    style,
                    kind,
                    handle,
                } => {
                    let param = bound(handle);
                    let mut text = match (kind, param) {
                        (LabelKind::Name, Some(p)) => params.name(p),
                        (LabelKind::Value, Some(p)) => params.text(p),
                        (_, None) => String::new(),
                    };
                    if style.uppercase {
                        text = text.to_uppercase();
                    }
                    out.items.push(Item::Label {
                        rect,
                        text,
                        style: style.clone(),
                        param,
                    });
                }
                Widget::ValueText { style, handle } => {
                    let param = bound(handle);
                    out.items.push(Item::Label {
                        rect,
                        text: param.map_or_else(String::new, |p| params.text(p)),
                        style: style.clone(),
                        param,
                    });
                }
                Widget::Focus => {}
                Widget::Meter { handle } => {
                    let param = bound(handle);
                    out.items.push(Item::Meter {
                        rect,
                        value: param.map_or(0.0, |p| params.value(p)),
                        param,
                    });
                }
                Widget::Instance {
                    def: child_def,
                    map: child_map,
                } => {
                    let inner = resolve_map(child_map, map);
                    let Some(cd) = self.definitions.get(child_def) else {
                        out.items.push(Item::Generic {
                            rect,
                            kind: child_def.clone(),
                            name: node.name.clone(),
                            param: inner.get("Data").copied(),
                        });
                        continue;
                    };
                    let (gesture, set_buttons) = instance_gesture(cd, &inner, params);
                    out.controls.push(Control {
                        rect,
                        param: inner.get("Data").copied(),
                        gesture,
                        name: node.name.clone(),
                    });
                    self.walk(child_def, rect, &inner, set_buttons, params, depth + 1, out);
                }
            }
        }
    }
}

/// A placed instance's handle map: `Parameter N` binds directly, any other value names a handle
/// of the enclosing instance.
fn resolve_map(entries: &[(String, String)], outer: &HashMap<String, u32>) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for (key, value) in entries {
        let param = parse_parameter(value).or_else(|| outer.get(value.as_str()).copied());
        if let Some(p) = param {
            map.insert(key.clone(), p);
        }
    }
    map
}

/// What a press on a placed instance does, and whether its button children set values.
///
/// A `Toggle Switch` action (on any trigger) flips the `Data` parameter. Otherwise a `Q-Link`
/// action selects the control: a knob or slider child makes it draggable, button children each set their
/// own value, anything else only selects. No action at all swallows the press.
fn instance_gesture(
    def: &Definition,
    map: &HashMap<String, u32>,
    params: &dyn ParamSource,
) -> (Gesture, bool) {
    if def.toggles {
        return (Gesture::Toggle, false);
    }
    if !def.qlink {
        return (Gesture::Inert, false);
    }
    let visible = |n: &Node| n.always && n.conditions.iter().all(|c| c.holds(params));
    let knob = def.children.iter().find(|n| {
        visible(n)
            && matches!(&n.widget, Widget::Knob { handle, .. } | Widget::Slider { handle, .. }
                | Widget::ValueText { handle, .. } if map.contains_key(handle))
    });
    if let Some(k) = knob {
        return (Gesture::Drag { height: k.rect.h }, false);
    }
    let has_button = def.children.iter().any(|n| {
        visible(n)
            && matches!(&n.widget, Widget::Button { handle, lamp: false, .. } if map.contains_key(handle))
    });
    (Gesture::Select, has_button)
}

fn convert_definition(d: model::Definition) -> Definition {
    let toggles = d.actions.iter().any(|a| a.handler == "Toggle Switch");
    let qlink = d.actions.iter().any(|a| a.handler == "Q-Link");
    // The background becomes two children drawn first, filling the definition's bounds.
    let mut children = Vec::new();
    if let Some(bg) = d.background_data.map(|b| b.unfocussed) {
        let whole = |widget| Node {
            name: String::new(),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            proportional: true,
            always: true,
            conditions: Vec::new(),
            widget,
        };
        // A JUCE colour as hex, `0` being transparent: the shared libraries' components use it.
        let argb = u32::from_str_radix(bg.colour.trim(), 16).unwrap_or(0);
        if argb >> 24 > 0 {
            let [a, r, g, b] = argb.to_be_bytes();
            children.push(whole(Widget::Fill {
                colour: Colour { a, r, g, b },
            }));
        }
        if !bg.image.is_empty() {
            children.push(whole(Widget::Image { file: bg.image }));
        }
    }
    children.extend(d.components_data.into_iter().map(convert_component));
    Definition {
        toggles,
        qlink,
        children,
    }
}

/// A `textStyle` block.
fn label_style(data: &serde_json::Value) -> LabelStyle {
    let ts = data.get("textStyle").cloned().unwrap_or_default();
    let font = ts.get("font").cloned().unwrap_or_default();
    let s = |v: &serde_json::Value, k: &str| {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    LabelStyle {
        font: FontSpec {
            name: s(&font, "name"),
            style: s(&font, "style"),
            height: font.get("height").and_then(|v| v.as_f64()).unwrap_or(20.0) as f32,
        },
        colour: Colour::parse(&s(&ts, "colour")),
        justification: Justification::parse(&s(&ts, "justification")),
        uppercase: s(&ts, "case") == "Upper Case",
    }
}

fn convert_component(c: model::Component) -> Node {
    let data = &c.component_data.data;
    let str_field = |k: &str| {
        data.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let num_field = |k: &str, default: f64| data.get(k).and_then(|v| v.as_f64()).unwrap_or(default);
    let handle = || {
        let h = str_field("handleName");
        if h.is_empty() {
            "Data".to_string()
        } else {
            h
        }
    };
    let kind = c.component_data.kind.as_str();
    let widget = match kind {
        "Image" => Widget::Image {
            file: str_field("image"),
        },
        "Knob" if str_field("knobType") == "ValueSlider" => Widget::ValueText {
            style: label_style(data),
            handle: handle(),
        },
        "Knob" => Widget::Knob {
            file: str_field("filmStrip"),
            frames: num_field("numFrames", 1.0).max(1.0) as u32,
            invert: data
                .get("invert")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            handle: handle(),
        },
        "Button" => Widget::Button {
            on: str_field("onImage"),
            off: str_field("offImage"),
            id: num_field("buttonId", 0.0).max(0.0) as u32,
            group: num_field("numButtonsInGroup", 1.0).max(1.0) as u32,
            handle: handle(),
            lamp: false,
        },
        "Indicator" => Widget::Button {
            on: str_field("onImage"),
            off: str_field("offImage"),
            id: num_field("indicatorId", 0.0).max(0.0) as u32,
            group: num_field("numIndicatorsInGroup", 1.0).max(1.0) as u32,
            handle: handle(),
            lamp: true,
        },
        // Only the down arrow has been seen; other shapes draw nothing rather than a placeholder.
        "Decorator" => match str_field("type").as_str() {
            "Down Arrow" => Widget::Arrow {
                colour: Colour::parse(&str_field("foregroundColour")),
            },
            _ => Widget::Focus,
        },
        "Label" => Widget::Label {
            style: label_style(data),
            kind: if str_field("type") == "Name" {
                LabelKind::Name
            } else {
                LabelKind::Value
            },
            handle: handle(),
        },
        "Slider" => Widget::Slider {
            thumb: str_field("thumbImage"),
            vertical: str_field("direction") != "Horizontal",
            handle: handle(),
        },
        "Focus" => Widget::Focus,
        "Meter" => Widget::Meter { handle: handle() },
        // Anything else names a local definition; one that does not exist draws as `Generic`.
        _ => Widget::Instance {
            def: kind.to_string(),
            map: c
                .handle_remapping
                .map
                .into_iter()
                .map(|m| (m.key, m.value))
                .collect(),
        },
    };
    Node {
        name: c.component_data.name,
        rect: Rect::parse(&c.bounds.bounds),
        proportional: c.bounds.bounds_type == "Proportional",
        always: c.bounds.when_visible != "WhenFocussed",
        conditions: c
            .bounds
            .additional_invalidating_handles
            .iter()
            .filter_map(|h| Condition::parse(h))
            .collect(),
        widget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_parse() {
        assert_eq!(Rect::parse("1 2 3 4"), Rect::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(Rect::parse("1 2"), Rect::new(1.0, 2.0, 0.0, 0.0));
        assert!(Rect::new(0.0, 0.0, 10.0, 10.0).contains(9.9, 0.0));
        assert!(!Rect::new(0.0, 0.0, 10.0, 10.0).contains(10.0, 0.0));
    }

    #[test]
    fn colour_parse() {
        assert_eq!(
            Colour::parse("ff2ad4c0"),
            Colour {
                a: 255,
                r: 0x2a,
                g: 0xd4,
                b: 0xc0
            }
        );
        assert_eq!(Colour::parse("102030").a, 255);
        assert_eq!(Colour::parse("0"), Colour::fallback());
    }

    #[test]
    fn justification_parse() {
        let j = Justification::parse("left verticallyCentred");
        assert_eq!((j.h, j.v), (HAlign::Left, VAlign::Centre));
        let j = Justification::parse("horizontallyCentred bottom");
        assert_eq!((j.h, j.v), (HAlign::Centre, VAlign::Bottom));
    }

    #[test]
    fn condition_parse() {
        let c = Condition::parse("IndexedEnabling/1/2/Parameter 73").unwrap();
        assert_eq!((c.index, c.count, c.param), (1, 2, 73));
        assert!(Condition::parse("Something/1/2/Parameter 3").is_none());
    }

    #[test]
    fn slider_draws_its_thumb_and_drags() {
        let json = r#"{"pageData": {
          "tabs": [{"tabName": "Main", "fnKeyIndex": 0, "componentName": "page",
                    "initialSize": "0 0 400 300"}],
          "componentDefinitions": {"localComponentDefinitions": [
            {"key": "fader", "value": {
              "actions": [{"onAction": "Touched", "handler": "Q-Link", "handleName": "Data"}],
              "componentsData": [
                {"componentData": {"name": "Thumb", "type": "Slider",
                   "data": {"sliderType": "TrackAndThumb", "direction": "Vertical",
                            "thumbImage": "thumb.png", "handleName": "Data"}},
                 "bounds": {"bounds": "0 0 40 200", "whenVisible": "Always"}}]}},
            {"key": "page", "value": {"componentsData": [
                {"componentData": {"name": "Level", "type": "fader"},
                 "handle remapping": {"map": [{"key": "Data", "value": "Parameter 2"}]},
                 "bounds": {"bounds": "10 20 40 200", "whenVisible": "Always"}}]}}]}}}"#;
        let skin = Skin::parse(json).unwrap();
        assert!(skin.image_files().contains("thumb.png"));
        let mut params = HashMap::new();
        params.insert(
            2,
            ParamState {
                value: 0.75,
                ..Default::default()
            },
        );
        let layout = skin.layout(0, &params);
        assert_eq!(
            layout.items,
            vec![Item::Slider {
                rect: Rect::new(10.0, 20.0, 40.0, 200.0),
                thumb: "thumb.png".into(),
                vertical: true,
                value: 0.75,
                param: Some(2),
            }]
        );
        let c = hit(&layout.controls, 20.0, 100.0).unwrap();
        assert_eq!(c.gesture, Gesture::Drag { height: 200.0 });
    }

    #[test]
    fn a_tab_can_hold_its_definition_inline() {
        let json = r#"{"pageData": {
          "tabs": [{"tabName": "Main", "fnKeyIndex": 0, "componentDefinition": {
              "componentsData": [
                {"componentData": {"name": "Bg", "type": "Image", "data": {"image": "bg.png"}},
                 "bounds": {"bounds": "0 0 1280 628", "whenVisible": "Always"}}]}}],
          "componentDefinitions": {"localComponentDefinitions": []}}}"#;
        let skin = Skin::parse(json).unwrap();
        assert_eq!(
            skin.pages()[0].size,
            Rect::new(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT)
        );
        assert_eq!(
            skin.draw_list(0, &HashMap::new()),
            vec![Item::Image {
                rect: Rect::new(0.0, 0.0, 1280.0, 628.0),
                file: "bg.png".into()
            }]
        );
    }

    #[test]
    fn paths_resolve_from_the_importing_file() {
        let r = |b, p| resolve_path(b, p);
        assert_eq!(
            r("TUI.json", "../../AKAI Components/Lib.json").as_deref(),
            Some("../../AKAI Components/Lib.json")
        );
        assert_eq!(
            r("../../AKAI Components/Lib.json", "../Generic/G.json").as_deref(),
            Some("../../Generic/G.json")
        );
        assert_eq!(
            r("../../AKAI Components/Lib.json", "knob.png").as_deref(),
            Some("../../AKAI Components/knob.png")
        );
        assert_eq!(r("TUI.json", "/usr/share/x.json"), None);
    }

    #[test]
    fn imported_definitions_fill_in_with_their_own_images() {
        let json = r#"{"pageData": {
          "tabs": [{"tabName": "Main", "componentName": "page"}],
          "componentDefinitions": {
            "importFiles": ["../../AKAI Components/Lib.json", "/usr/share/Akai/x.json"],
            "localComponentDefinitions": [
            {"key": "page", "value": {"componentsData": [
                {"componentData": {"name": "K", "type": "knobBlack"},
                 "handle remapping": {"map": [{"key": "Data", "value": "Parameter 0"}]},
                 "bounds": {"bounds": "10 10 50 50", "whenVisible": "Always"}}]}}]}}}"#;
        let mut skin = Skin::parse(json).unwrap();
        assert_eq!(skin.imports(), ["../../AKAI Components/Lib.json"]);
        let params: HashMap<u32, ParamState> = HashMap::new();
        assert!(matches!(
            skin.draw_list(0, &params)[0],
            Item::Generic { .. }
        ));
        let lib = r#"{"componentDefinitions": {"importFiles": ["../Generic/G.json"],
          "localComponentDefinitions": [
            {"key": "knobBlack", "value": {"componentsData": [
              {"componentData": {"name": "Knob", "type": "Knob",
                 "data": {"filmStrip": "knob_black.png", "numFrames": 3}},
               "bounds": {"bounds": "0 0 50 50", "whenVisible": "Always"}}]}},
            {"key": "page", "value": {"componentsData": []}}]}}"#;
        let next = skin
            .add_library("../../AKAI Components/Lib.json", lib)
            .unwrap();
        assert_eq!(next, ["../../Generic/G.json"]);
        let items = skin.draw_list(0, &params);
        assert!(
            matches!(&items[0], Item::Knob { file, .. } if file == "../../AKAI Components/knob_black.png"),
            "{items:?}"
        );
        // The skin's own `page` is kept over the library's empty one.
        assert_eq!(items.len(), 1);
        assert!(skin
            .image_files()
            .contains("../../AKAI Components/knob_black.png"));
    }

    #[test]
    fn indicator_lights_and_arrow_draws() {
        let json = r#"{"pageData": {
          "tabs": [{"tabName": "Main", "componentName": "page"}],
          "componentDefinitions": {"localComponentDefinitions": [
            {"key": "page", "value": {"componentsData": [
              {"componentData": {"name": "Lamp", "type": "Indicator", "data": {"onImage": "on.png",
                 "offImage": "off.png", "indicatorId": 1, "numIndicatorsInGroup": 1, "handleName": "Data"}},
               "bounds": {"bounds": "0 0 10 10", "whenVisible": "Always"}},
              {"componentData": {"name": "Arrow", "type": "Decorator", "data": {"type": "Down Arrow",
                 "foregroundColour": "ff00e2ff"}},
               "bounds": {"bounds": "20 0 8 6", "whenVisible": "Always"}}]}}]}}}"#;
        let skin = Skin::parse(json).unwrap();
        let items = skin.draw_list(0, &HashMap::new());
        assert!(matches!(&items[0], Item::Button { file, on: false, .. } if file == "off.png"));
        assert!(
            matches!(&items[1], Item::Arrow { colour, .. } if colour.b == 0xff && colour.r == 0)
        );
        assert!(skin.controls(0, &HashMap::new()).is_empty());
    }

    #[test]
    fn proportional_bounds_value_text_and_used_images() {
        let json = r#"{"pageData": {
          "tabs": [{"tabName": "Main", "componentName": "page"}],
          "componentDefinitions": {"localComponentDefinitions": [
            {"key": "page", "value": {"componentsData": [
              {"componentData": {"name": "Mix", "type": "slider", "data": {}},
               "handle remapping": {"map": [{"key": "Data", "value": "Parameter 0"}]},
               "bounds": {"bounds": "100 50 200 100", "whenVisible": "Always"}}]}},
            {"key": "slider", "value": {
              "actions": [{"onAction": "Touched", "handler": "Q-Link", "handleName": "Data"}],
              "componentsData": [
              {"componentData": {"name": "Track", "type": "Knob", "data": {"knobType": "FilmStrip",
                 "filmStrip": "track.png", "numFrames": 2, "handleName": "Data"}},
               "bounds": {"bounds": "180 10 300 300", "whenVisible": "Always"}},
              {"componentData": {"name": "Text", "type": "Knob", "data": {"knobType": "ValueSlider",
                 "textStyle": {"font": {"name": "Titillium Web", "height": 55.0}}, "handleName": "Data"}},
               "bounds": {"bounds": "0.1 0.25 0.5 0.5", "boundsType": "Proportional", "whenVisible": "Always"}}]}},
            {"key": "spare", "value": {"componentsData": [
              {"componentData": {"name": "Spare", "type": "Image", "data": {"image": "spare.png"}},
               "bounds": {"bounds": "0 0 1 1", "whenVisible": "Always"}}]}}]}}}"#;
        let skin = Skin::parse(json).unwrap();
        let mut params = HashMap::new();
        params.insert(
            0,
            ParamState {
                value: 0.5,
                text: "-3.0 dB".into(),
                ..Default::default()
            },
        );
        let items = skin.draw_list(0, &params);
        assert!(matches!(&items[1], Item::Label { rect, text, .. }
            if *rect == Rect::new(120.0, 75.0, 100.0, 50.0) && text == "-3.0 dB"));
        assert!(matches!(
            skin.controls(0, &params)[0].gesture,
            Gesture::Drag { .. }
        ));
        let files: Vec<_> = skin.image_files().into_iter().collect();
        assert_eq!(files, ["track.png"]);
    }

    #[test]
    fn saved_names_bind_to_the_skin_by_name() {
        let json = r#"{"pageData": {
          "tabs": [{"tabName": "Main", "componentName": "page"}],
          "componentDefinitions": {"localComponentDefinitions": [
            {"key": "page", "value": {
              "backgroundData": {"unfocussed": {"colour": "ff1e1d1e", "image": "bg.jpg"}},
              "componentsData": [
              {"componentData": {"name": "Release", "type": "knob", "data": {}},
               "handle remapping": {"map": [{"key": "Data", "value": "Parameter 4"}]},
               "bounds": {"bounds": "0 0 10 10", "whenVisible": "Always"}},
              {"componentData": {"name": "Dry / Wet", "type": "knob", "data": {}},
               "handle remapping": {"map": [{"key": "Data", "value": "Parameter 12"}]},
               "bounds": {"bounds": "0 0 10 10", "whenVisible": "Always"}}]}},
            {"key": "knob", "value": {
              "backgroundData": {"unfocussed": {"colour": "0", "image": ""}},
              "componentsData": []}}]}}}"#;
        let skin = Skin::parse(json).unwrap();
        let names = ["DryWet", "Rels", "Ratio"].map(String::from);
        assert_eq!(skin.bind_names(&names), [Some(12), Some(4), None]);
        let items = skin.draw_list(0, &HashMap::new());
        assert!(matches!(&items[0], Item::Fill { rect, colour }
            if rect.w == PAGE_WIDTH && colour.a == 0xff && colour.r == 0x1e));
        assert!(matches!(&items[1], Item::Image { file, .. } if file == "bg.jpg"));
        assert_eq!(items.len(), 2, "a 0 background is transparent: {items:?}");
    }

    #[test]
    fn option_rounding() {
        assert_eq!(option_index(0.0, 3), 0);
        assert_eq!(option_index(0.5, 3), 1);
        assert_eq!(option_index(1.0, 3), 2);
        assert_eq!(option_index(0.49, 2), 0);
        assert_eq!(option_index(0.5, 2), 1);
        assert_eq!(option_index(1.0, 1), 0);
        assert_eq!(option_index(2.0, 4), 3);
    }
}
