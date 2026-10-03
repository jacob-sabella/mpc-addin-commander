//! A plugin skin (`TUI.json`, see `docs/SKIN.md`) resolved into pages, and each page laid out
//! as a flat draw list and a flat control list from live parameter values.
//!
//! This crate has no network and no GPU dependency: it parses, resolves placed instances of
//! local component definitions, maps handles to parameter indices, applies
//! `IndexedEnabling` visibility, and tells the caller what to draw where and what a click or
//! drag at a point means. Drawing is the caller's job.

#![forbid(unsafe_code)]

pub mod model;

use std::collections::{BTreeSet, HashMap};
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
    },
    Label {
        style: LabelStyle,
        kind: LabelKind,
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

#[derive(Debug, Clone, PartialEq)]
struct Node {
    name: String,
    rect: Rect,
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
        let definitions: HashMap<String, Definition> = data
            .component_definitions
            .local_component_definitions
            .into_iter()
            .map(|kv| (kv.key, convert_definition(kv.value)))
            .collect();
        let mut pages = Vec::new();
        for tab in data.tabs {
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
        Ok(Skin { pages, definitions })
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

    /// Every PNG the skin names, relative to the skin folder.
    pub fn image_files(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for def in self.definitions.values() {
            for node in &def.children {
                match &node.widget {
                    Widget::Image { file } | Widget::Knob { file, .. } => {
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
            self.walk(&p.definition, (0.0, 0.0), &map, false, params, 0, &mut out);
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
        origin: (f32, f32),
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
            let rect = node.rect.translate(origin.0, origin.1);
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
                Widget::Button {
                    on,
                    off,
                    id,
                    group,
                    handle,
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
                    if buttons_set && param.is_some() {
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
                    self.walk(
                        child_def,
                        (rect.x, rect.y),
                        &inner,
                        set_buttons,
                        params,
                        depth + 1,
                        out,
                    );
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
/// action selects the control: a knob child makes it draggable, button children each set their
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
        visible(n) && matches!(&n.widget, Widget::Knob { handle, .. } if map.contains_key(handle))
    });
    if let Some(k) = knob {
        return (Gesture::Drag { height: k.rect.h }, false);
    }
    let has_button = def.children.iter().any(|n| {
        visible(n) && matches!(&n.widget, Widget::Button { handle, .. } if map.contains_key(handle))
    });
    (Gesture::Select, has_button)
}

fn convert_definition(d: model::Definition) -> Definition {
    let toggles = d.actions.iter().any(|a| a.handler == "Toggle Switch");
    let qlink = d.actions.iter().any(|a| a.handler == "Q-Link");
    let children = d
        .components_data
        .into_iter()
        .map(convert_component)
        .collect();
    Definition {
        toggles,
        qlink,
        children,
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
        },
        "Label" => {
            let ts = data.get("textStyle").cloned().unwrap_or_default();
            let font = ts.get("font").cloned().unwrap_or_default();
            let s = |v: &serde_json::Value, k: &str| {
                v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
            };
            Widget::Label {
                style: LabelStyle {
                    font: FontSpec {
                        name: s(&font, "name"),
                        style: s(&font, "style"),
                        height: font.get("height").and_then(|v| v.as_f64()).unwrap_or(20.0) as f32,
                    },
                    colour: Colour::parse(&s(&ts, "colour")),
                    justification: Justification::parse(&s(&ts, "justification")),
                    uppercase: s(&ts, "case") == "Upper Case",
                },
                kind: if str_field("type") == "Name" {
                    LabelKind::Name
                } else {
                    LabelKind::Value
                },
                handle: handle(),
            }
        }
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
