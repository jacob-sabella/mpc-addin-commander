//! Every JSON fixture parses as the message its file name says, survives a serialize/parse
//! round trip unchanged, and carries the values the fixture spells out.

use commander_protocol::{ClientMessage, ServerMessage, TransportCmd, TransportSource};
use std::fs;
use std::path::PathBuf;

fn fixtures() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    assert!(out.len() >= 18, "fixtures missing: {}", out.len());
    out
}

#[test]
fn server_fixtures_roundtrip() {
    for (name, json) in fixtures().iter().filter(|(n, _)| n.starts_with("server_")) {
        let m: ServerMessage = serde_json::from_str(json).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_ne!(m, ServerMessage::Unknown, "{name} parsed as Unknown");
        let expected_tag = name.trim_start_matches("server_").trim_end_matches(".json");
        let again = serde_json::to_string(&m).unwrap();
        let v: serde_json::Value = serde_json::from_str(&again).unwrap();
        assert_eq!(v["t"], expected_tag, "{name}");
        let back: ServerMessage = serde_json::from_str(&again).unwrap();
        assert_eq!(back, m, "{name}");
    }
}

#[test]
fn client_fixtures_roundtrip() {
    for (name, json) in fixtures().iter().filter(|(n, _)| n.starts_with("client_")) {
        let m: ClientMessage = serde_json::from_str(json).unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected_tag = name.trim_start_matches("client_").trim_end_matches(".json");
        let again = serde_json::to_string(&m).unwrap();
        let v: serde_json::Value = serde_json::from_str(&again).unwrap();
        assert_eq!(v["t"], expected_tag, "{name}");
        let back: ClientMessage = serde_json::from_str(&again).unwrap();
        assert_eq!(back, m, "{name}");
    }
}

#[test]
fn fixture_values() {
    let get = |n: &str| {
        fixtures()
            .into_iter()
            .find(|(name, _)| name == n)
            .map(|(_, j)| j)
            .unwrap()
    };
    match serde_json::from_str(&get("server_hello.json")).unwrap() {
        ServerMessage::Hello(h) => {
            assert_eq!(h.protocol, 1);
            assert_eq!(h.addin, "0.1.0");
            assert_eq!(h.device.mpc, "3.4.0");
            assert_eq!((h.poll_ms, h.text_ms), (20, 200));
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_str(&get("server_plugins.json")).unwrap() {
        ServerMessage::Plugins { plugins } => {
            assert_eq!(plugins.len(), 2);
            assert!(plugins[0].skin && plugins[0].synth);
            assert!(!plugins[1].skin && !plugins[1].synth);
            assert_eq!(plugins[0].params[1].text, "Minor");
            assert_eq!(plugins[1].params[0].label, "ms");
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_str(&get("server_values.json")).unwrap() {
        ServerMessage::Values { id, v } => {
            assert_eq!(id, 3);
            assert_eq!(v.len(), 3);
            assert_eq!(v[0].index(), 0);
            assert_eq!(v[0].text(), Some("Db"));
            assert_eq!(v[1].text(), None);
            assert_eq!(v[2].value(), 1.0);
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_str(&get("server_transport.json")).unwrap() {
        ServerMessage::Transport(t) => {
            assert!(t.playing && !t.recording);
            assert_eq!(t.tempo, Some(120.0));
            assert_eq!((t.bar, t.beat, t.tick), (Some(3), Some(2), Some(48)));
            assert_eq!(t.source, TransportSource::Clock);
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_str(&get("server_project.json")).unwrap() {
        ServerMessage::Project(p) => {
            assert_eq!(p.tracks.len(), 2);
            assert_eq!(p.tracks[1].type_name, "plugin");
            assert_eq!(p.tracks[1].kind, Some(3));
            let plugin = p.tracks[1].plugin.as_ref().unwrap();
            assert_eq!(
                (plugin.name.as_str(), plugin.preset.as_str()),
                ("Chordsmith", "Lydian Pad")
            );
            assert!(p.tracks[1].record_arm && p.tracks[1].mute);
            assert_eq!(p.tracks[0].plugin, None);
            assert_eq!(p.current_track, Some(1));
            let seq = p.sequence.unwrap();
            assert_eq!((seq.bars, seq.loop_start, seq.loop_end), (8, 4, 8));
            assert_eq!(seq.beats_per_bar, Some(4));
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_str(&get("server_midi_in.json")).unwrap() {
        ServerMessage::MidiIn(m) => {
            assert_eq!((m.bytes.as_slice(), m.ms), (&[144u8, 60, 100][..], 123456))
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_str(&get("client_transport.json")).unwrap() {
        ClientMessage::Transport { cmd } => assert_eq!(cmd, TransportCmd::Play),
        other => panic!("{other:?}"),
    }
}
