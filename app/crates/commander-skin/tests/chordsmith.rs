//! The Chordsmith skin (built by mpc-vst-plugins' own tool) lays out as the device draws it.

use commander_skin::{
    hit, Action, Colour, Gesture, HAlign, Item, ParamState, Skin, VAlign, PAGE_HEIGHT, PAGE_WIDTH,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chordsmith")
}

fn skin() -> Skin {
    let json = std::fs::read_to_string(fixture_dir().join("TUI.json")).unwrap();
    Skin::parse(&json).unwrap()
}

/// Every parameter at `value`, named `P<n>` with text `T<n>`.
fn params(value: f32) -> HashMap<u32, ParamState> {
    (0..128)
        .map(|i| {
            (
                i,
                ParamState {
                    value,
                    name: format!("P{i}"),
                    text: format!("T{i}"),
                },
            )
        })
        .collect()
}

#[test]
fn pages_are_the_f_keys() {
    let s = skin();
    let names: Vec<&str> = s.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["PLAY", "VOICE", "BUTTONS", "PERFORM", "SETUP"]);
    for (i, p) in s.pages().iter().enumerate() {
        assert_eq!(p.fn_key as usize, i);
        assert_eq!(p.sub_index, 0);
        assert_eq!(p.size.w, PAGE_WIDTH);
        assert_eq!(p.size.h, PAGE_HEIGHT);
        assert_eq!(s.page_index(i as u32, 0), Some(i));
    }
    assert_eq!(s.page_index(9, 0), None);
}

#[test]
fn every_page_draws_without_generic_items() {
    let s = skin();
    let p = params(0.0);
    for (i, page) in s.pages().iter().enumerate() {
        let items = s.draw_list(i, &p);
        assert!(items.len() > 10, "{}: {} items", page.name, items.len());
        let generic: Vec<_> = items
            .iter()
            .filter(|it| matches!(it, Item::Generic { .. }))
            .collect();
        assert!(generic.is_empty(), "{}: {generic:?}", page.name);
        // The page's background colour, if it has one, fills first; then its picture.
        let first = items
            .iter()
            .find(|it| !matches!(it, Item::Fill { .. }))
            .unwrap();
        match first {
            Item::Image { rect, file } => {
                assert_eq!(
                    (rect.x, rect.y, rect.w, rect.h),
                    (0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT)
                );
                assert!(file.starts_with("sh_bg_"), "{file}");
            }
            other => panic!("{}: first item {other:?}", page.name),
        }
        for it in &items {
            let r = it.rect();
            assert!(
                r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= PAGE_WIDTH + 0.5,
                "{it:?}"
            );
            assert!(r.y + r.h <= PAGE_HEIGHT + 0.5, "{it:?}");
        }
    }
}

#[test]
fn every_named_image_exists() {
    let s = skin();
    let files = s.image_files();
    assert!(files.len() > 100, "{}", files.len());
    for f in &files {
        assert!(
            fixture_dir().join(f).is_file(),
            "{f} missing from the fixture"
        );
    }
    // Everything the draw lists name is in that set.
    let p = params(1.0);
    for i in 0..s.pages().len() {
        for it in s.draw_list(i, &p) {
            match it {
                Item::Image { file, .. } | Item::Knob { file, .. } | Item::Button { file, .. } => {
                    assert!(files.contains(&file), "{file}");
                }
                _ => {}
            }
        }
    }
}

#[test]
fn indexed_enabling_shows_the_popup_list() {
    let s = skin();
    let popup_buttons = |items: &[Item]| {
        items
            .iter()
            .filter(|it| matches!(it, Item::Button { file, .. } if file.starts_with("sh_popopt_0_key_")))
            .count()
    };
    let panel = |items: &[Item]| {
        items
            .iter()
            .any(|it| matches!(it, Item::Image { file, .. } if file == "sh_pop_0_key.png"))
    };
    // Closed: parameter 73 (the hidden `__open` field) at 0.
    let closed = s.draw_list(0, &params(0.0));
    assert_eq!(popup_buttons(&closed), 0);
    assert!(!panel(&closed));
    // Open: parameter 73 at 1 shows the panel and its 12 keys, drawn after the field.
    let mut p = params(0.0);
    p.get_mut(&73).unwrap().value = 1.0;
    let open = s.draw_list(0, &p);
    assert_eq!(popup_buttons(&open), 12);
    assert!(panel(&open));
    let panel_pos = open
        .iter()
        .position(|it| matches!(it, Item::Image { file, .. } if file == "sh_pop_0_key.png"))
        .unwrap();
    let first_button = open
        .iter()
        .position(
            |it| matches!(it, Item::Button { file, .. } if file.starts_with("sh_popopt_0_key_")),
        )
        .unwrap();
    assert!(
        panel_pos < first_button,
        "the panel is drawn under its options"
    );
    // The scale popup (parameter 74) stays closed.
    assert!(!open
        .iter()
        .any(|it| matches!(it, Item::Image { file, .. } if file == "sh_pop_0_scale.png")));
}

#[test]
fn knob_frame_follows_the_value() {
    let s = skin();
    let voice = s.page_index(1, 0).unwrap();
    let knob = |value: f32| -> Vec<(u32, u32, Option<u32>)> {
        s.draw_list(voice, &params(value))
            .into_iter()
            .filter_map(|it| match it {
                Item::Knob {
                    frames,
                    frame,
                    param,
                    file,
                    ..
                } => {
                    assert_eq!(file, "sh_knob_r54.png");
                    Some((frames, frame, param))
                }
                _ => None,
            })
            .collect()
    };
    let at0 = knob(0.0);
    assert_eq!(at0.len(), 2);
    for (frames, frame, param) in &at0 {
        assert_eq!(*frames, 127);
        assert_eq!(*frame, 0);
        assert!(param.is_some());
    }
    assert!(knob(0.5).iter().all(|k| k.1 == 63));
    assert!(knob(1.0).iter().all(|k| k.1 == 126));
    // The knob is draggable over its own height.
    let controls = s.controls(voice, &params(0.25));
    let knobs: Vec<_> = controls
        .iter()
        .filter(|c| matches!(c.gesture, Gesture::Drag { .. }))
        .collect();
    assert_eq!(knobs.len(), 2);
    match knobs[0].press(&params(0.25)) {
        Some(Action::Drag { height, start, .. }) => {
            assert_eq!(height, 118.0);
            assert_eq!(start, 0.25);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn inverted_knob_counts_down() {
    let json = r#"{"pageData": {"tabs": [{"tabName": "A", "fnKeyIndex": 0, "fnKeySubIndex": 0,
        "componentName": "A|A", "initialSize": "0 0 1280 628"}],
        "componentDefinitions": {"importFiles": [], "localComponentDefinitions": [
          {"key": "A|A", "value": {"actions": [], "componentsData": [
            {"componentData": {"name": "k", "type": "knob", "data": {"handleName": "Data"}},
             "handle remapping": {"map": [{"key": "Data", "value": "Parameter 5"}]},
             "bounds": {"bounds": "10 20 100 100", "whenVisible": "Always", "additionalInvalidatingHandles": []}}]}},
          {"key": "knob", "value": {"actions": [{"onAction": "Mouse Down", "handler": "Q-Link", "handleName": "Data"}],
            "componentsData": [
            {"componentData": {"name": "Knob", "type": "Knob", "data": {"knobType": "FilmStrip", "filmStrip": "k.png",
               "numFrames": 11, "invert": true, "handleName": "Data"}},
             "bounds": {"bounds": "0 0 100 100", "whenVisible": "Always", "additionalInvalidatingHandles": []}}]}}
        ]}}}"#;
    let s = Skin::parse(json).unwrap();
    let frame = |v: f32| match &s.draw_list(0, &params(v))[0] {
        Item::Knob { frame, rect, .. } => (*frame, rect.x, rect.y),
        other => panic!("{other:?}"),
    };
    assert_eq!(frame(0.0), (10, 10.0, 20.0));
    assert_eq!(frame(0.5), (5, 10.0, 20.0));
    assert_eq!(frame(1.0), (0, 10.0, 20.0));
}

#[test]
fn button_group_lights_the_chosen_option() {
    let s = skin();
    // Parameter 14 drives the three PLAY FROM segments (group of 3).
    let lit = |value: f32| -> Vec<String> {
        let mut p = params(0.0);
        p.get_mut(&14).unwrap().value = value;
        s.draw_list(0, &p)
            .into_iter()
            .filter_map(|it| match it {
                Item::Button {
                    file,
                    on: true,
                    param: Some(14),
                    ..
                } => Some(file),
                _ => None,
            })
            .collect()
    };
    assert_eq!(lit(0.0), ["sh_seg_input_0_on.png"]);
    assert_eq!(lit(0.5), ["sh_seg_input_1_on.png"]);
    assert_eq!(lit(1.0), ["sh_seg_input_2_on.png"]);
    let off = s
        .draw_list(0, &params(0.0))
        .into_iter()
        .filter(|it| matches!(it, Item::Button { file, on: false, param: Some(14), .. } if file.starts_with("sh_seg_input_")))
        .count();
    assert_eq!(off, 2);
    // A single-button group (the toggle pill on parameter 41) is on from 0.5 up.
    let pill = |value: f32| {
        let mut p = params(0.0);
        p.get_mut(&41).unwrap().value = value;
        s.draw_list(0, &p).into_iter().find_map(|it| match it {
            Item::Button {
                file,
                param: Some(41),
                ..
            } => Some(file),
            _ => None,
        })
    };
    assert_eq!(pill(0.0).as_deref(), Some("sh_pill_off.png"));
    assert_eq!(pill(0.49).as_deref(), Some("sh_pill_off.png"));
    assert_eq!(pill(0.5).as_deref(), Some("sh_pill_on.png"));
}

#[test]
fn labels_carry_text_font_and_colour() {
    let s = skin();
    let p = params(0.0);
    let items = s.draw_list(0, &p);
    // The toggle's Name label shows parameter 41's name.
    let name = items
        .iter()
        .find_map(|it| match it {
            Item::Label {
                text,
                style,
                param: Some(41),
                rect,
            } => Some((text.clone(), style.clone(), *rect)),
            _ => None,
        })
        .expect("name label");
    assert_eq!(name.0, "P41");
    assert_eq!(name.1.font.name, "Titillium Web");
    assert_eq!(name.1.font.style, "SemiBold");
    assert!((name.1.font.height - 27.3).abs() < 1e-3);
    assert_eq!(
        name.1.colour,
        Colour {
            a: 255,
            r: 0xf3,
            g: 0xec,
            b: 0xff
        }
    );
    assert_eq!(name.1.justification.h, HAlign::Centre);
    assert_eq!(name.1.justification.v, VAlign::Centre);
    assert!(!name.1.uppercase);
    assert_eq!((name.2.x, name.2.y), (530.0, 82.0 + 34.0));
    // The popup field's Value label reads the Text handle (parameter 0), not Data (73).
    let value = items
        .iter()
        .find_map(|it| match it {
            Item::Label {
                text,
                param: Some(0),
                style,
                ..
            } => Some((text.clone(), style.colour)),
            _ => None,
        })
        .expect("value label");
    assert_eq!(value.0, "T0");
    assert_eq!(value.1, Colour::parse("ff2ad4c0"));
    // Upper Case labels upper-case the text; the left-justified row label keeps its alignment.
    let voice = s.draw_list(1, &p);
    assert!(voice
        .iter()
        .any(|it| matches!(it, Item::Label { text, style, .. } if style.uppercase && text.starts_with("T"))));
    assert!(items.iter().any(
        |it| matches!(it, Item::Label { style, .. } if style.justification.h == HAlign::Left)
    ));
}

#[test]
fn hits_set_toggle_and_respect_visibility() {
    let s = skin();
    let p = params(0.0);
    let controls = s.controls(0, &p);
    // The middle PLAY FROM segment (225 104 150 33) sets parameter 14 to 1/2.
    let c = hit(&controls, 300.0, 120.0).expect("segment");
    assert_eq!(
        c.press(&p),
        Some(Action::Set {
            param: 14,
            value: 0.5
        })
    );
    // The toggle (530 82 221 74) flips parameter 41.
    let c = hit(&controls, 640.0, 119.0).expect("toggle");
    assert_eq!(c.gesture, Gesture::Toggle);
    assert_eq!(
        c.press(&p),
        Some(Action::Set {
            param: 41,
            value: 1.0
        })
    );
    let mut on = params(0.0);
    on.get_mut(&41).unwrap().value = 1.0;
    assert_eq!(
        c.press(&on),
        Some(Action::Set {
            param: 41,
            value: 0.0
        })
    );
    // The popup field (750 96 200 48) is a toggle on the hidden parameter 73.
    let c = hit(&controls, 850.0, 120.0).expect("field");
    assert_eq!(
        c.press(&p),
        Some(Action::Set {
            param: 73,
            value: 1.0
        })
    );
    // Its options are not there while it is closed.
    let under = hit(&controls, 856.0, 258.0);
    assert!(
        !matches!(
            under.and_then(|c| c.press(&p)),
            Some(Action::Set { param: 0, .. })
        ),
        "{under:?}"
    );
    // Open, the third option (756 238 200 40) sets parameter 0 to 2/11.
    let mut open = params(0.0);
    open.get_mut(&73).unwrap().value = 1.0;
    let controls = s.controls(0, &open);
    let c = hit(&controls, 856.0, 258.0).expect("option");
    match c.press(&open) {
        Some(Action::Set { param: 0, value }) => assert!((value - 2.0 / 11.0).abs() < 1e-6),
        other => panic!("{other:?}"),
    }
    // Empty background: nothing.
    assert!(hit(&controls, 5.0, 600.0).is_none());
}
