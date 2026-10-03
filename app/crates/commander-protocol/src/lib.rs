//! Serde types for the Commander protocol (`docs/PROTOCOL.md`): every message the addin
//! pushes over its WebSocket and every message the app sends back, plus the DAW messages
//! (`transport`, `midi_in`, `project`) the addin grows later.
//!
//! Every message is a JSON object whose `t` field names its type. Unknown types parse as
//! [`ServerMessage::Unknown`] and unknown fields are ignored, so an older app keeps working
//! against a newer addin.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// The protocol version this crate speaks (the `protocol` field of `hello`).
pub const PROTOCOL_VERSION: u32 = 1;

/// The addin's default HTTP/WebSocket port.
pub const DEFAULT_PORT: u16 = 6730;

/// A message from the addin to the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The first message after the upgrade.
    Hello(Hello),
    /// Every plugin instance, right after `hello` and in reply to `list`.
    Plugins {
        #[serde(default)]
        plugins: Vec<Plugin>,
    },
    /// MPC created an instance.
    PluginAdded { plugin: Plugin },
    /// MPC closed an instance.
    PluginRemoved { id: u32 },
    /// Parameter values or display texts that changed.
    Values {
        id: u32,
        #[serde(default)]
        v: Vec<ValueChange>,
    },
    /// The reply to `ping`.
    Pong,
    /// The addin rejected a client message.
    Error {
        #[serde(default)]
        msg: String,
    },
    /// Transport state (from MMC, MIDI clock or the project).
    Transport(Transport),
    /// A MIDI event received on the addin's ALSA port.
    MidiIn(MidiIn),
    /// A snapshot of the newest project file.
    Project(Project),
    /// A message type this crate does not know.
    #[serde(other)]
    Unknown,
}

/// A message from the app to the addin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMessage {
    /// `setParameter(i, value)` on instance `id`.
    Set { id: u32, i: u32, value: f32 },
    /// These instances get their text polled at `poll_ms`, the others at `text_ms`.
    Subscribe { ids: Vec<u32> },
    /// Ask for a fresh `plugins` message.
    List,
    /// Ask for a `pong`.
    Ping,
    /// Drive the transport.
    Transport { cmd: TransportCmd },
    /// Send these bytes on the addin's ALSA port.
    Midi { bytes: Vec<u8> },
    /// Ask for a fresh `project` snapshot.
    Project,
    /// Append these bytes to the device's control-surface injector file.
    Surface { bytes: Vec<u8> },
}

/// The `hello` message.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Hello {
    #[serde(default)]
    pub protocol: u32,
    /// The addin's version string.
    #[serde(default)]
    pub addin: String,
    #[serde(default)]
    pub device: Device,
    /// The poll period of subscribed instances, in milliseconds.
    #[serde(default)]
    pub poll_ms: u32,
    /// The text poll period of unsubscribed instances, in milliseconds.
    #[serde(default)]
    pub text_ms: u32,
}

/// The device the addin runs on.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Device {
    /// `/proc/device-tree/model`.
    #[serde(default)]
    pub model: String,
    /// The MPC software version.
    #[serde(default)]
    pub mpc: String,
}

/// A plugin instance as the addin reports it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Plugin {
    /// Unique for the life of the MPC process, never reused.
    pub id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub product: String,
    /// The VST unique id as eight hex digits.
    #[serde(default)]
    pub uid: String,
    /// The path of the plugin's shared object.
    #[serde(default)]
    pub so: String,
    /// Whether `Plugin Skins/TUI.json` exists next to `so`.
    #[serde(default)]
    pub skin: bool,
    /// An instrument (true) or an effect (false).
    #[serde(default)]
    pub synth: bool,
    /// Complete and in VST index order.
    #[serde(default)]
    pub params: Vec<Param>,
}

/// One parameter of a plugin instance.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Param {
    /// The VST parameter index.
    pub i: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub label: String,
    /// 0 to 1.
    #[serde(default)]
    pub value: f32,
    /// What the plugin shows for the value.
    #[serde(default)]
    pub text: String,
}

/// One entry of a `values` message: `[index, value, text]`, `text` null when it was not read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueChange(pub u32, pub f32, pub Option<String>);

impl ValueChange {
    pub fn index(&self) -> u32 {
        self.0
    }

    pub fn value(&self) -> f32 {
        self.1
    }

    pub fn text(&self) -> Option<&str> {
        self.2.as_deref()
    }
}

/// Where the transport state comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportSource {
    /// MIDI Machine Control.
    Mmc,
    /// MIDI clock.
    Clock,
    /// The project file.
    Project,
    #[default]
    #[serde(other)]
    Unknown,
}

/// The `transport` message.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Transport {
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub recording: bool,
    #[serde(default)]
    pub tempo: Option<f32>,
    #[serde(default)]
    pub bar: Option<u32>,
    #[serde(default)]
    pub beat: Option<u32>,
    #[serde(default)]
    pub tick: Option<u32>,
    #[serde(default)]
    pub source: TransportSource,
}

/// A transport command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportCmd {
    Play,
    Stop,
    Record,
    Continue,
}

/// The `midi_in` message.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MidiIn {
    #[serde(default)]
    pub bytes: Vec<u8>,
    /// The addin's millisecond clock when the event arrived.
    #[serde(default)]
    pub ms: u64,
}

/// The `project` message: a snapshot parsed from the newest project file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    /// The file's modification time, Unix seconds.
    #[serde(default)]
    pub mtime: u64,
    #[serde(default)]
    pub key: String,
    /// The master tempo when that is enabled, else the current sequence's.
    #[serde(default)]
    pub tempo: Option<f32>,
    #[serde(default)]
    pub master_tempo: Option<f32>,
    #[serde(default)]
    pub master_tempo_enabled: bool,
    /// The selected track's `index`.
    #[serde(default)]
    pub current_track: Option<u32>,
    #[serde(default)]
    pub sequence: Option<Sequence>,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

/// The current sequence of a project snapshot.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sequence {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tempo: Option<f32>,
    #[serde(default)]
    pub tempo_enabled: bool,
    /// The length in bars.
    #[serde(default)]
    pub bars: u32,
    #[serde(default)]
    pub r#loop: bool,
    /// The loop's first bar, 0-based.
    #[serde(default)]
    pub loop_start: u32,
    /// The bar after the loop, 0-based.
    #[serde(default)]
    pub loop_end: u32,
    #[serde(default)]
    pub beats_per_bar: Option<u32>,
    /// Ticks per beat.
    #[serde(default)]
    pub beat_length: Option<u32>,
}

/// One track of a project snapshot.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub name: String,
    /// The program type number in the file.
    #[serde(default)]
    pub kind: Option<u32>,
    /// The program type's name (`drum`, `plugin`, `audio`, ..., `other`).
    #[serde(rename = "type", default)]
    pub type_name: String,
    /// `0xRRGGBB`.
    #[serde(default)]
    pub colour: Option<u32>,
    /// The sequencer track's mute.
    #[serde(default)]
    pub track_mute: bool,
    #[serde(default)]
    pub record_arm: bool,
    /// The mixer's mute.
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub volume: f32,
    #[serde(default)]
    pub pan: f32,
    /// The plugin on the track, if any.
    #[serde(default)]
    pub plugin: Option<TrackPlugin>,
}

/// The plugin on a project track.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TrackPlugin {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    /// `VST`, or `MPC` for Akai's own instruments.
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub preset: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_server(m: &ServerMessage) {
        let s = serde_json::to_string(m).unwrap();
        let back: ServerMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(&back, m, "{s}");
    }

    fn roundtrip_client(m: &ClientMessage) {
        let s = serde_json::to_string(m).unwrap();
        let back: ClientMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(&back, m, "{s}");
    }

    #[test]
    fn server_messages_roundtrip() {
        roundtrip_server(&ServerMessage::Hello(Hello {
            protocol: 1,
            addin: "0.1.0".into(),
            device: Device {
                model: "model".into(),
                mpc: "3.0".into(),
            },
            poll_ms: 20,
            text_ms: 200,
        }));
        roundtrip_server(&ServerMessage::Plugins {
            plugins: vec![Plugin {
                id: 3,
                name: "Synth".into(),
                params: vec![Param {
                    i: 0,
                    name: "Key".into(),
                    text: "C".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        });
        roundtrip_server(&ServerMessage::PluginRemoved { id: 3 });
        roundtrip_server(&ServerMessage::Values {
            id: 3,
            v: vec![
                ValueChange(0, 0.5, None),
                ValueChange(1, 1.0, Some("D".into())),
            ],
        });
        roundtrip_server(&ServerMessage::Pong);
        roundtrip_server(&ServerMessage::Error { msg: "bad".into() });
        roundtrip_server(&ServerMessage::Transport(Transport {
            playing: true,
            tempo: Some(120.0),
            bar: Some(1),
            source: TransportSource::Clock,
            ..Default::default()
        }));
        roundtrip_server(&ServerMessage::MidiIn(MidiIn {
            bytes: vec![0x90, 60, 100],
            ms: 12,
        }));
        roundtrip_server(&ServerMessage::Project(Project {
            name: "Song".into(),
            tracks: vec![Track {
                index: 0,
                name: "Drums".into(),
                type_name: "drum".into(),
                plugin: None,
                ..Default::default()
            }],
            ..Default::default()
        }));
    }

    #[test]
    fn client_messages_roundtrip() {
        roundtrip_client(&ClientMessage::Set {
            id: 3,
            i: 2,
            value: 0.25,
        });
        roundtrip_client(&ClientMessage::Subscribe { ids: vec![3, 4] });
        roundtrip_client(&ClientMessage::List);
        roundtrip_client(&ClientMessage::Ping);
        roundtrip_client(&ClientMessage::Transport {
            cmd: TransportCmd::Continue,
        });
        roundtrip_client(&ClientMessage::Midi { bytes: vec![0xf8] });
        roundtrip_client(&ClientMessage::Project);
        roundtrip_client(&ClientMessage::Surface { bytes: vec![1, 2] });
    }

    #[test]
    fn values_are_tuples() {
        let s = serde_json::to_string(&ServerMessage::Values {
            id: 1,
            v: vec![
                ValueChange(4, 0.5, None),
                ValueChange(5, 1.0, Some("x".into())),
            ],
        })
        .unwrap();
        assert_eq!(s, r#"{"t":"values","id":1,"v":[[4,0.5,null],[5,1.0,"x"]]}"#);
    }

    #[test]
    fn tags_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&ClientMessage::List).unwrap(),
            r#"{"t":"list"}"#
        );
        assert_eq!(
            serde_json::to_string(&ServerMessage::PluginRemoved { id: 7 }).unwrap(),
            r#"{"t":"plugin_removed","id":7}"#
        );
        assert_eq!(
            serde_json::to_string(&ClientMessage::Transport {
                cmd: TransportCmd::Play
            })
            .unwrap(),
            r#"{"t":"transport","cmd":"play"}"#
        );
    }

    #[test]
    fn unknown_types_and_fields_are_ignored() {
        let m: ServerMessage = serde_json::from_str(r#"{"t":"later","x":1}"#).unwrap();
        assert_eq!(m, ServerMessage::Unknown);
        let m: ServerMessage =
            serde_json::from_str(r#"{"t":"pong","extra":{"deep":[1,2]}}"#).unwrap();
        assert_eq!(m, ServerMessage::Pong);
        let m: ServerMessage = serde_json::from_str(
            r#"{"t":"transport","playing":true,"source":"telepathy","bars":3}"#,
        )
        .unwrap();
        assert_eq!(
            m,
            ServerMessage::Transport(Transport {
                playing: true,
                source: TransportSource::Unknown,
                ..Default::default()
            })
        );
    }
}
