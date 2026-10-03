//! What the app knows: the connection, the instances and their values, the skins, the DAW
//! state, and the log. The network task writes it, the UI reads it once per frame.

use commander_protocol::{Hello, MidiIn, Param, Plugin, Project, ServerMessage, Transport};
use commander_skin::{ParamSource, Skin};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub type Shared = Arc<Mutex<Model>>;

/// The lines the log pane keeps.
pub const LOG_LINES: usize = 200;
/// The MIDI events the MIDI pane keeps.
pub const MIDI_LINES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    Disconnected,
    Connecting {
        host: String,
        port: u16,
        attempt: u32,
    },
    Connected {
        host: String,
        port: u16,
    },
}

impl ConnState {
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnState::Connected { .. })
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, ConnState::Disconnected)
    }
}

/// A decoded PNG.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A skin with its images decoded.
#[derive(Debug)]
pub struct SkinBundle {
    pub skin: Skin,
    pub images: HashMap<String, Arc<Image>>,
}

#[derive(Debug, Clone)]
pub enum SkinState {
    Loading { done: usize, total: usize },
    Ready(Arc<SkinBundle>),
    Failed(String),
}

/// A plugin instance, plus what the app learnt about its option parameters.
#[derive(Debug, Clone)]
pub struct Instance {
    pub plugin: Plugin,
    /// The display texts seen per parameter, with the value that showed each: the generic
    /// panel's segmented controls.
    pub options: HashMap<u32, BTreeMap<String, f32>>,
}

impl Instance {
    fn new(plugin: Plugin) -> Self {
        let mut inst = Instance {
            plugin,
            options: HashMap::new(),
        };
        let params = inst.plugin.params.clone();
        for p in &params {
            inst.learn(p.i, p.value, &p.text);
        }
        inst
    }

    pub fn param(&self, i: u32) -> Option<&Param> {
        match self.plugin.params.get(i as usize) {
            Some(p) if p.i == i => Some(p),
            _ => self.plugin.params.iter().find(|p| p.i == i),
        }
    }

    pub fn param_mut(&mut self, i: u32) -> Option<&mut Param> {
        let direct = matches!(self.plugin.params.get(i as usize), Some(p) if p.i == i);
        if direct {
            self.plugin.params.get_mut(i as usize)
        } else {
            self.plugin.params.iter_mut().find(|p| p.i == i)
        }
    }

    /// Remembers a non-numeric display text and the value that showed it.
    fn learn(&mut self, i: u32, value: f32, text: &str) {
        if text.is_empty() || is_numeric(text) {
            return;
        }
        let seen = self.options.entry(i).or_default();
        if seen.len() < 64 || seen.contains_key(text) {
            seen.insert(text.to_string(), value);
        }
    }
}

/// A display text that reads as a number, with or without a unit.
pub fn is_numeric(text: &str) -> bool {
    let t = text.trim();
    let digits: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ','))
        .collect();
    !digits.is_empty() && digits.replace(',', ".").parse::<f64>().is_ok()
}

impl ParamSource for Instance {
    fn value(&self, param: u32) -> f32 {
        self.param(param).map_or(0.0, |p| p.value)
    }

    fn name(&self, param: u32) -> String {
        self.param(param)
            .map_or_else(String::new, |p| p.name.clone())
    }

    fn text(&self, param: u32) -> String {
        self.param(param)
            .map_or_else(String::new, |p| p.text.clone())
    }
}

pub struct Model {
    pub conn: ConnState,
    pub hello: Option<Hello>,
    pub latency_ms: Option<f32>,
    pub instances: Vec<Instance>,
    /// Skins by plugin uid.
    pub skins: HashMap<String, SkinState>,
    /// Instances whose skin has not been requested yet: `(id, uid)`, drained by the network task.
    pub skins_wanted: Vec<(u32, String)>,
    pub transport: Option<Transport>,
    pub project: Option<Project>,
    pub midi_in: VecDeque<MidiIn>,
    pub log: VecDeque<String>,
    started: Instant,
}

/// A copy of what the UI draws in a frame, taken under the lock and released before drawing.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub conn: ConnState,
    pub hello: Option<Hello>,
    pub latency_ms: Option<f32>,
    pub instances: Vec<Instance>,
    pub skins: HashMap<String, SkinState>,
    pub transport: Option<Transport>,
    pub project: Option<Project>,
    pub midi_in: Vec<MidiIn>,
    pub log: Vec<String>,
}

impl Snapshot {
    pub fn instance(&self, id: u32) -> Option<&Instance> {
        self.instances.iter().find(|i| i.plugin.id == id)
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Model {
            conn: ConnState::Disconnected,
            hello: None,
            latency_ms: None,
            instances: Vec::new(),
            skins: HashMap::new(),
            skins_wanted: Vec::new(),
            transport: None,
            project: None,
            midi_in: VecDeque::new(),
            log: VecDeque::new(),
            started: Instant::now(),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            conn: self.conn.clone(),
            hello: self.hello.clone(),
            latency_ms: self.latency_ms,
            instances: self.instances.clone(),
            skins: self.skins.clone(),
            transport: self.transport.clone(),
            project: self.project.clone(),
            midi_in: self.midi_in.iter().cloned().collect(),
            log: self.log.iter().cloned().collect(),
        }
    }

    /// Appends a line to the log pane (and the process log).
    pub fn log(&mut self, line: impl Into<String>) {
        let line = line.into();
        log::info!("{line}");
        let t = self.started.elapsed().as_secs_f64();
        self.log.push_back(format!("{t:9.3}  {line}"));
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    pub fn instance_mut(&mut self, id: u32) -> Option<&mut Instance> {
        self.instances.iter_mut().find(|i| i.plugin.id == id)
    }

    /// Forgets everything that belongs to a connection.
    pub fn reset_session(&mut self) {
        self.hello = None;
        self.latency_ms = None;
        self.instances.clear();
        self.skins_wanted.clear();
        self.transport = None;
        self.project = None;
    }

    /// The value the app just sent, shown until the addin's next poll confirms it.
    pub fn set_local(&mut self, id: u32, i: u32, value: f32) {
        if let Some(p) = self.instance_mut(id).and_then(|inst| inst.param_mut(i)) {
            p.value = value;
        }
    }

    fn add_instance(&mut self, plugin: Plugin) {
        let id = plugin.id;
        let uid = plugin.uid.clone();
        if plugin.skin && !uid.is_empty() && !self.skins.contains_key(&uid) {
            self.skins
                .insert(uid.clone(), SkinState::Loading { done: 0, total: 0 });
            self.skins_wanted.push((id, uid));
        }
        match self.instances.iter_mut().find(|i| i.plugin.id == id) {
            Some(existing) => *existing = Instance::new(plugin),
            None => self.instances.push(Instance::new(plugin)),
        }
        self.instances.sort_by_key(|i| i.plugin.id);
    }

    /// Applies a message from the addin.
    pub fn apply(&mut self, msg: ServerMessage) {
        match msg {
            ServerMessage::Hello(h) => {
                self.log(format!(
                    "hello: addin {} protocol {} on {} (software {}), poll {} ms, text {} ms",
                    h.addin, h.protocol, h.device.model, h.device.mpc, h.poll_ms, h.text_ms
                ));
                if h.protocol != commander_protocol::PROTOCOL_VERSION {
                    self.log(format!(
                        "protocol {} differs from the app's {}; unknown messages are ignored",
                        h.protocol,
                        commander_protocol::PROTOCOL_VERSION
                    ));
                }
                self.hello = Some(h);
            }
            ServerMessage::Plugins { plugins } => {
                self.log(format!("{} plugin instance(s)", plugins.len()));
                let keep: Vec<u32> = plugins.iter().map(|p| p.id).collect();
                self.instances.retain(|i| keep.contains(&i.plugin.id));
                for p in plugins {
                    self.add_instance(p);
                }
            }
            ServerMessage::PluginAdded { plugin } => {
                self.log(format!(
                    "added #{} {} ({})",
                    plugin.id, plugin.name, plugin.vendor
                ));
                self.add_instance(plugin);
            }
            ServerMessage::PluginRemoved { id } => {
                self.log(format!("removed #{id}"));
                self.instances.retain(|i| i.plugin.id != id);
            }
            ServerMessage::Values { id, v } => {
                if let Some(inst) = self.instance_mut(id) {
                    for change in v {
                        let (i, value, text) = (change.0, change.1, change.2);
                        if let Some(p) = inst.param_mut(i) {
                            p.value = value;
                            if let Some(t) = &text {
                                p.text = t.clone();
                            }
                        }
                        if let Some(t) = text {
                            inst.learn(i, value, &t);
                        }
                    }
                }
            }
            ServerMessage::Pong => {}
            ServerMessage::Error { msg } => self.log(format!("addin error: {msg}")),
            ServerMessage::Transport(t) => self.transport = Some(t),
            ServerMessage::MidiIn(m) => {
                self.midi_in.push_back(m);
                while self.midi_in.len() > MIDI_LINES {
                    self.midi_in.pop_front();
                }
            }
            ServerMessage::Project(p) => {
                self.log(format!("project {:?}: {} track(s)", p.name, p.tracks.len()));
                self.project = Some(p);
            }
            ServerMessage::Unknown => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_protocol::ValueChange;

    fn plugin(id: u32) -> Plugin {
        Plugin {
            id,
            name: "Synth".into(),
            uid: "abcd0001".into(),
            skin: true,
            params: vec![
                Param {
                    i: 0,
                    name: "Key".into(),
                    text: "C".into(),
                    ..Default::default()
                },
                Param {
                    i: 1,
                    name: "Cutoff".into(),
                    value: 0.5,
                    text: "1200 Hz".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn plugins_and_values() {
        let mut m = Model::new();
        m.apply(ServerMessage::Plugins {
            plugins: vec![plugin(3)],
        });
        assert_eq!(m.instances.len(), 1);
        assert_eq!(m.skins_wanted, vec![(3, "abcd0001".to_string())]);
        m.apply(ServerMessage::Values {
            id: 3,
            v: vec![
                ValueChange(0, 0.5, Some("F#".into())),
                ValueChange(1, 0.25, None),
            ],
        });
        let inst = m.instances.iter().find(|i| i.plugin.id == 3).unwrap();
        assert_eq!(inst.param(0).unwrap().text, "F#");
        assert_eq!(inst.param(1).unwrap().value, 0.25);
        assert_eq!(inst.param(1).unwrap().text, "1200 Hz");
        // Option texts are learnt, numeric ones are not.
        assert_eq!(inst.options[&0].len(), 2);
        assert!(!inst.options.contains_key(&1));
        assert_eq!(inst.text(0), "F#");
        m.apply(ServerMessage::PluginAdded { plugin: plugin(4) });
        assert_eq!(m.instances.len(), 2);
        // The same uid is fetched once.
        assert_eq!(m.skins_wanted.len(), 1);
        m.apply(ServerMessage::PluginRemoved { id: 3 });
        assert_eq!(m.instances.len(), 1);
        assert!(m.log.iter().any(|l| l.contains("removed #3")));
    }

    #[test]
    fn numeric_texts() {
        assert!(is_numeric("1200 Hz"));
        assert!(is_numeric("-3.5 dB"));
        assert!(is_numeric("50%"));
        assert!(is_numeric("0"));
        assert!(!is_numeric("Minor"));
        assert!(!is_numeric("C"));
        assert!(!is_numeric(""));
    }
}
