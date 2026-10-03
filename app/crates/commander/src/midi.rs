//! MIDI messages the app sends through the addin's port: MMC locate and notes.

/// An MMC locate (`F0 7F 7F 06 44 06 01 hh mm ss ff sf F7`, 30 fps time code) to the start of
/// `bar` (0-based) at `tempo` bpm with `beats_per_bar` beats a bar. This is the message MPC
/// sends on Play, and the one the addin turns back into bars.
pub fn locate(bar: u32, beats_per_bar: u32, tempo: f32) -> Vec<u8> {
    let tempo = if tempo > 0.0 { f64::from(tempo) } else { 120.0 };
    let secs = f64::from(bar) * f64::from(beats_per_bar.max(1)) * 60.0 / tempo;
    let total_sub = (secs * 30.0 * 100.0).round() as u64; // hundredths of a frame
    let sf = (total_sub % 100) as u8;
    let total_frames = total_sub / 100;
    let ff = (total_frames % 30) as u8;
    let total_secs = total_frames / 30;
    let ss = (total_secs % 60) as u8;
    let mm = ((total_secs / 60) % 60) as u8;
    let hh = ((total_secs / 3600) % 24) as u8;
    vec![
        0xF0,
        0x7F,
        0x7F,
        0x06,
        0x44,
        0x06,
        0x01,
        0x60 | hh,
        mm,
        ss,
        ff,
        sf,
        0xF7,
    ]
}

/// Note on, `channel` 0..15.
pub fn note_on(channel: u8, note: u8, velocity: u8) -> Vec<u8> {
    vec![0x90 | (channel & 15), note & 127, velocity.clamp(1, 127)]
}

/// Note off, `channel` 0..15.
pub fn note_off(channel: u8, note: u8) -> Vec<u8> {
    vec![0x80 | (channel & 15), note & 127, 0]
}

/// The note's name, `C4` = 60.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(note % 12)],
        i32::from(note / 12) - 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes a locate like the addin's `locate_clocks`, into beats.
    fn beats(m: &[u8], tempo: f64) -> f64 {
        let fps = [24.0, 25.0, 29.97, 30.0][usize::from((m[7] >> 5) & 3)];
        let sec = f64::from(m[7] & 31) * 3600.0
            + f64::from(m[8] & 63) * 60.0
            + f64::from(m[9] & 63)
            + (f64::from(m[10] & 31) + f64::from(m[11] & 127) / 100.0) / fps;
        sec * tempo / 60.0
    }

    #[test]
    fn locate_round_trips_through_the_addins_decoding() {
        for (bar, tempo) in [
            (0, 120.0),
            (1, 120.0),
            (7, 94.192),
            (19, 94.192),
            (200, 61.3),
        ] {
            let m = locate(bar, 4, tempo);
            assert_eq!(m.len(), 13);
            assert_eq!((m[0], m[12]), (0xF0, 0xF7));
            assert!(m[1..12].iter().all(|b| *b < 0x80), "{m:02X?}");
            let got = beats(&m, f64::from(tempo));
            assert!(
                (got - f64::from(bar * 4)).abs() < 0.01,
                "bar {bar}: {got} beats"
            );
        }
        assert_eq!(locate(0, 4, 120.0)[7..12], [0x60, 0, 0, 0, 0]);
        // Two bars of 4/4 at 120 bpm are 4 seconds.
        assert_eq!(locate(2, 4, 120.0)[7..12], [0x60, 0, 4, 0, 0]);
        // 3/4: one bar at 60 bpm is 3 seconds.
        assert_eq!(locate(1, 3, 60.0)[7..12], [0x60, 0, 3, 0, 0]);
    }

    #[test]
    fn notes() {
        assert_eq!(note_on(0, 60, 100), [0x90, 60, 100]);
        assert_eq!(note_on(9, 36, 0), [0x99, 36, 1]);
        assert_eq!(note_off(15, 200), [0x8F, 72, 0]);
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(0), "C-1");
    }
}
