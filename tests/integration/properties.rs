//! Three invariants the example tests state one case at a time, held over
//! generated input (adr/2026-09-property-tests-guard-three-invariants.md):
//! the vim grammar survives any key sequence and undo walks back to the
//! opened text, a block's CSS spans tile its source byte for byte, and the
//! note parser survives anything a file can hold.

use dioxus::html::{Key, Modifiers};
use note_system::domain::MetaStatus;
use note_system::editor::Editor;
use note_system::markup::{Draw, model};
use note_system::parse::parse_note;
use note_system::vim::{Act, Outcome, View, Vim};
use proptest::prelude::*;

/// Every key the grammar reads, plus the digits that build counts, the
/// prompts' sigils and the platform keys the webview sends around them.
const KEYS: &[&str] = &[
    "h",
    "j",
    "k",
    "l",
    "w",
    "b",
    "e",
    "W",
    "B",
    "E",
    "0",
    "^",
    "$",
    "g",
    "G",
    "x",
    "X",
    "d",
    "c",
    "y",
    "p",
    "P",
    "o",
    "O",
    "i",
    "I",
    "a",
    "A",
    "s",
    "S",
    "r",
    "R",
    "u",
    "J",
    "~",
    "v",
    "V",
    ".",
    "%",
    "f",
    "F",
    "t",
    "T",
    ";",
    ",",
    "n",
    "N",
    "*",
    "#",
    "/",
    ":",
    "\"",
    "'",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "<",
    ">",
    "z",
    "Z",
    "q",
    "m",
    " ",
    "_",
    "`",
    "-",
    "=",
    "+",
    "1",
    "2",
    "9",
    "é",
    "字",
    "Escape",
    "Enter",
    "Backspace",
    "Tab",
    "ArrowLeft",
    "ArrowRight",
    "Home",
    "End",
    "Shift",
    "Control",
];

fn key(name: &str) -> Key {
    match name {
        "Escape" => Key::Escape,
        "Enter" => Key::Enter,
        "Backspace" => Key::Backspace,
        "Tab" => Key::Tab,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "Home" => Key::Home,
        "End" => Key::End,
        "Shift" => Key::Shift,
        "Control" => Key::Control,
        character => Key::Character(character.to_string()),
    }
}

/// One keystroke through the grammar and its acts through the editor —
/// the executor `src/ui.rs` runs, minus the parts that need a webview or
/// a clipboard (a paste lands a fixed clip; a visual-line walk, a
/// scroll and a prompt are inert here).
fn drive(editor: &mut Editor, vim: &mut Vim, key: &Key) {
    if editor.caret().is_none() {
        editor.reactivate();
    }
    let Some(caret) = editor.caret() else { return };
    let Some((_, text)) = editor.note() else {
        return;
    };
    let text = text.to_string();
    let blocks = editor.blocks().to_vec();
    let view = View {
        text: &text,
        blocks: &blocks,
        head: caret.head,
        anchor: caret.anchor,
    };
    let Outcome::Acts(acts) = vim.handle(key, Modifiers::empty(), &view)
    else {
        return;
    };
    for act in acts {
        match act {
            Act::Place(at) => editor.place_at(at),
            Act::Type(text) => editor.insert_at_caret(&text),
            Act::Deactivate => editor.deactivate(),
            Act::Splice { span, text, caret } => {
                editor.splice(span, &text, caret);
            }
            Act::Paste { before, count } => {
                editor.paste("clip", before, count.min(4));
            }
            Act::Extend(at) => editor.extend_to(at),
            Act::SwapEnds => editor.swap_ends(),
            Act::Checkpoint => editor.checkpoint(),
            Act::Undo => editor.undo(),
            Act::Redo => editor.redo(),
            Act::Save => {
                editor.flush();
            }
            Act::SetClipboard(_)
            | Act::OpenSearch
            | Act::OpenEx { .. }
            | Act::FollowLink
            | Act::Scroll(_)
            | Act::PasteOver { .. }
            | Act::WalkVisual { .. } => {}
        }
    }
}

fn opened(text: &str) -> (tempfile::TempDir, Editor) {
    let dir = tempfile::tempdir().expect("a temp dir");
    let file = dir.path().join("note.typ");
    std::fs::write(&file, text).expect("the note");
    (dir, Editor::open(file))
}

proptest! {
    // 256 cases missed the first defect this file caught; 5000 found it
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn any_key_sequence_leaves_the_editor_sound_and_undo_walks_it_back(
        text in "[\\PC\\n]{0,60}",
        keys in prop::collection::vec(prop::sample::select(KEYS), 0..40),
    ) {
        let (_dir, mut editor) = opened(&text);
        let original = editor
            .note()
            .map(|(_, text)| text.to_string())
            .expect("the note opened");
        let mut vim = Vim::default();
        for name in keys {
            drive(&mut editor, &mut vim, &key(name));
            prop_assert_eq!(
                editor.trouble(),
                None,
                "after {:?} in mode {:?}",
                name,
                vim.mode
            );
        }
        for _ in 0..200 {
            editor.undo();
        }
        let restored = editor.note().map(|(_, text)| text.to_string());
        prop_assert_eq!(restored, Some(original));
    }

    #[test]
    fn css_spans_tile_a_blocks_source_byte_for_byte(
        source in "\\PC{0,80}",
    ) {
        let Draw::Css(markup) = model(&source) else { return Ok(()) };
        let mut at = 0;
        for span in &markup.spans {
            prop_assert_eq!(span.range.start, at, "{:?}", markup);
            prop_assert!(span.range.end >= span.range.start, "{:?}", markup);
            prop_assert!(source.is_char_boundary(span.range.end));
            at = span.range.end;
        }
        prop_assert_eq!(at, source.len(), "{:?}", markup);
    }

    #[test]
    fn the_parser_survives_anything_and_finds_no_meta_where_none_is_spelled(
        source in "[\\PC\\n]{0,200}",
    ) {
        let parsed = parse_note(&source);
        if !source.contains("meta") {
            prop_assert_eq!(parsed.meta, MetaStatus::Missing);
        }
    }
}
