//! The saved state of Akai's own plugins, as a project file holds it: JUCE's base64 around an
//! `ACVS` block. Two layouts are known (`docs/PROTOCOL.md`, "The project snapshot"):
//!
//! - **Indexed** (the AIR instruments and effects): `ACVS`, a u32, the engine name in 64 bytes,
//!   a header of varying length, then a u32 count N and N little-endian float32 values in 0..1,
//!   in parameter order: value i is the skin's `Parameter i`.
//! - **Named** (older effects): `ACVS`, a u32, a version byte, a count byte, a header, then
//!   entries of a u64 name length, the name and a float32, sorted by name. Which skin parameter
//!   a name is is not known yet, so these show on the generic panel.
//!
//! Both end with the preset: its name and a NUL, `PRESETNAME`, and a u32 of the name's length
//! with the NUL.
//!
//! - **XML** (XYFX): JUCE's `copyXmlToBinary`, `VC2!`, a u32 length and
//!   `<PluginState><param index="i" value="v"/>...</PluginState>`, no preset. Indexed too.

/// The alphabet of JUCE's `MemoryBlock::toBase64Encoding`.
const ALPHABET: &[u8; 64] = b".ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+";
/// More than any stock plugin has; a count above it is not a count.
const MAX_VALUES: usize = 1024;
const PRESET_TAG: &[u8] = b"PRESETNAME";

/// How a state lists its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// By parameter index, without names.
    Indexed,
    /// By name, in name order.
    Named,
}

/// A decoded state.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub layout: Layout,
    /// The engine name (indexed layout), else empty.
    pub engine: String,
    pub preset: String,
    /// `(name, value)`; the name is empty in the indexed layout.
    pub values: Vec<(String, f32)>,
}

impl State {
    /// Decodes a state string from the project file; `None` when it is not one of the known
    /// layouts.
    pub fn decode(state: &str) -> Option<State> {
        Self::parse(&juce_base64(state)?)
    }

    /// Parses a decoded `ACVS` block.
    pub fn parse(b: &[u8]) -> Option<State> {
        if b.len() >= 8 && &b[..4] == b"VC2!" {
            return xml(&b[8..]);
        }
        if b.len() < 10 || &b[..4] != b"ACVS" {
            return None;
        }
        let (end, preset) = preset(b);
        let b = &b[..end];
        if let Some(values) = indexed(b) {
            let name = &b[8..72];
            let engine = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            return Some(State {
                layout: Layout::Indexed,
                engine: String::from_utf8_lossy(engine).into_owned(),
                preset,
                values: values.into_iter().map(|v| (String::new(), v)).collect(),
            });
        }
        named(b).map(|values| State {
            layout: Layout::Named,
            engine: String::new(),
            preset,
            values,
        })
    }
}

/// The XML layout: every `<param index="i" value="v"/>`, ordered by index; a missing index
/// reads as 0.
fn xml(b: &[u8]) -> Option<State> {
    let text = std::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).ok()?;
    if !text.contains("<PluginState") {
        return None;
    }
    let attr = |tag: &str, name: &str| -> Option<String> {
        let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
        Some(tag[at..].split('"').next()?.to_string())
    };
    let mut values: Vec<(String, f32)> = Vec::new();
    for tag in text.split("<param ").skip(1) {
        let tag = tag.split("/>").next()?;
        let i: usize = attr(tag, "index")?.parse().ok()?;
        let v: f32 = attr(tag, "value")?.parse().ok()?;
        if i >= MAX_VALUES {
            return None;
        }
        if values.len() <= i {
            values.resize(i + 1, (String::new(), 0.0));
        }
        values[i].1 = v;
    }
    (!values.is_empty()).then_some(State {
        layout: Layout::Indexed,
        engine: String::new(),
        preset: String::new(),
        values,
    })
}

/// JUCE's `MemoryBlock::fromBase64Encoding`: `<bytes>.<chars>`, six bits per character, least
/// significant bit first.
pub fn juce_base64(s: &str) -> Option<Vec<u8>> {
    let (size, body) = s.split_once('.')?;
    let size: usize = size.parse().ok()?;
    if size > 1 << 24 || body.len() * 6 < size * 8 {
        return None;
    }
    let mut out = vec![0u8; size];
    let mut bit = 0usize;
    for c in body.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c)? as u32;
        for b in 0..6 {
            if bit < size * 8 && v >> b & 1 == 1 {
                out[bit / 8] |= 1 << (bit % 8);
            }
            bit += 1;
        }
    }
    Some(out)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn f32_at(b: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn unit(v: f32) -> bool {
    (-0.001..=1.001).contains(&v)
}

/// Where the values end, and the preset name; a block without the trailer has no preset.
fn preset(b: &[u8]) -> (usize, String) {
    let n = b.len();
    let tagged = n >= 4 + PRESET_TAG.len() && &b[n - 4 - PRESET_TAG.len()..n - 4] == PRESET_TAG;
    let len = if tagged { u32_at(b, n - 4) } else { None };
    match len.map(|l| l as usize) {
        Some(l) if l <= n - 4 - PRESET_TAG.len() => {
            let start = n - 4 - PRESET_TAG.len() - l;
            let name = &b[start..start + l];
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            (start, String::from_utf8_lossy(name).into_owned())
        }
        _ => (n, String::new()),
    }
}

/// The indexed layout: a count N after the 72-byte head, followed by exactly N unit floats up to
/// the end.
fn indexed(b: &[u8]) -> Option<Vec<f32>> {
    let end = b.len();
    (72..end.saturating_sub(4)).find_map(|at| {
        let n = u32_at(b, at)? as usize;
        if n == 0 || n > MAX_VALUES || at + 4 + 4 * n != end {
            return None;
        }
        let v: Vec<f32> = (0..n).filter_map(|i| f32_at(b, at + 4 + 4 * i)).collect();
        v.iter().all(|&x| unit(x)).then_some(v)
    })
}

/// The named layout: entries back to back up to the end, starting wherever the header ends. The
/// count byte at 9 is one more than the named entries (the header holds an unnamed one), so it
/// only bounds them.
fn named(b: &[u8]) -> Option<Vec<(String, f32)>> {
    let count = *b.get(9)? as usize;
    (10..b.len()).find_map(|start| {
        let mut at = start;
        let mut out = Vec::new();
        while at < b.len() && out.len() < count {
            let len = u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?) as usize;
            if len == 0 || len > 64 {
                return None;
            }
            let name = b.get(at + 8..at + 8 + len)?;
            if !name.iter().all(|c| (0x20..0x7f).contains(c)) {
                return None;
            }
            let v = f32_at(b, at + 8 + len)?;
            if !v.is_finite() {
                return None;
            }
            out.push((String::from_utf8_lossy(name).into_owned(), v));
            at += 8 + len + 4;
        }
        (at == b.len() && !out.is_empty()).then_some(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JUCE's `MemoryBlock::toBase64Encoding`, for building test states.
    fn encode(b: &[u8]) -> String {
        let bits = b.len() * 8;
        let mut out = format!("{}.", b.len());
        let mut bit = 0;
        while bit < bits {
            let mut v = 0usize;
            for i in 0..6 {
                if bit + i < bits && b[(bit + i) / 8] >> ((bit + i) % 8) & 1 == 1 {
                    v |= 1 << i;
                }
            }
            out.push(ALPHABET[v] as char);
            bit += 6;
        }
        out
    }

    fn trailer(out: &mut Vec<u8>, preset: &str) {
        out.extend_from_slice(preset.as_bytes());
        out.push(0);
        out.extend_from_slice(PRESET_TAG);
        out.extend_from_slice(&(preset.len() as u32 + 1).to_le_bytes());
    }

    fn indexed_block(engine: &str, header: usize, values: &[f32], preset: &str) -> Vec<u8> {
        let mut b = b"ACVS".to_vec();
        b.extend_from_slice(&0u32.to_le_bytes());
        let mut name = engine.as_bytes().to_vec();
        name.resize(64, 0);
        b.extend_from_slice(&name);
        // A header of junk the parser has to skip, as some engines leave.
        b.extend((0..header).map(|i| (i * 37 % 251) as u8));
        b.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for v in values {
            b.extend_from_slice(&v.to_le_bytes());
        }
        trailer(&mut b, preset);
        b
    }

    #[test]
    fn base64_round_trip() {
        let data: Vec<u8> = (0..=255u8).collect();
        assert_eq!(juce_base64(&encode(&data)).unwrap(), data);
        assert_eq!(juce_base64("3.").as_deref(), None);
        assert_eq!(juce_base64("1.!!"), None);
        assert_eq!(juce_base64("no dot"), None);
        assert_eq!(juce_base64("0.").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn indexed_layout() {
        let values = [0.5, 0.0, 1.0, 0.25, 0.75];
        for header in [4, 68] {
            let s = encode(&indexed_block("Test Engine", header, &values, "Warm Pad"));
            let st = State::decode(&s).unwrap();
            assert_eq!(st.layout, Layout::Indexed);
            assert_eq!(st.engine, "Test Engine");
            assert_eq!(st.preset, "Warm Pad");
            let got: Vec<f32> = st.values.iter().map(|v| v.1).collect();
            assert_eq!(got, values, "header {header}");
        }
    }

    #[test]
    fn named_layout() {
        let mut b = b"ACVS".to_vec();
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[1, 4]);
        b.extend_from_slice(&[0u8; 19]);
        for (name, v) in [("Amount", 0.5f32), ("Gain", 1.0), ("Time", 0.125)] {
            b.extend_from_slice(&(name.len() as u64).to_le_bytes());
            b.extend_from_slice(name.as_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        trailer(&mut b, "Init");
        let st = State::decode(&encode(&b)).unwrap();
        assert_eq!(st.layout, Layout::Named);
        assert_eq!(st.preset, "Init");
        assert_eq!(
            st.values,
            vec![
                ("Amount".to_string(), 0.5),
                ("Gain".to_string(), 1.0),
                ("Time".to_string(), 0.125)
            ]
        );
    }

    #[test]
    fn xml_layout() {
        let body = br#"<?xml version="1.0" encoding="UTF-8"?> <PluginState><param index="1" value="0.25"/><param index="0" value="1.0"/><param index="3" value="0.5"/></PluginState>"#;
        let mut b = b"VC2!".to_vec();
        b.extend_from_slice(&(body.len() as u32 + 1).to_le_bytes());
        b.extend_from_slice(body);
        b.push(0);
        let st = State::decode(&encode(&b)).unwrap();
        assert_eq!(st.layout, Layout::Indexed);
        let got: Vec<f32> = st.values.iter().map(|v| v.1).collect();
        assert_eq!(got, [1.0, 0.25, 0.0, 0.5]);
        assert_eq!(State::parse(b"VC2!\x05\0\0\0<a/>\0"), None);
    }

    #[test]
    fn unknown_blocks() {
        assert_eq!(State::parse(b"RIFF0000"), None);
        assert_eq!(State::parse(b"ACVS"), None);
        // An indexed block whose values leave 0..1 is not indexed.
        let b = indexed_block("X", 4, &[0.5, 7.0], "P");
        assert_eq!(State::parse(&b), None);
        // No trailer: no preset, values to the end.
        let mut b = indexed_block("X", 4, &[0.5], "P");
        b.truncate(b.len() - 2 - PRESET_TAG.len() - 4);
        let st = State::parse(&b).unwrap();
        assert_eq!((st.preset.as_str(), st.values.len()), ("", 1));
    }
}
