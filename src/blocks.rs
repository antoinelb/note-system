use std::ops::Range;

use typst_syntax::ast;
use typst_syntax::{SyntaxKind, SyntaxNode};

/// One editable unit of a note: one physical line, except a multi-line
/// construct (a raw fence, a multi-line `#let`/`#table`/equation, a
/// `ContentBlock`) which the parse tree already reports as a single child
/// and so is never split, and a leading run of `#import`/`#show`/`#meta`
/// lines, folded into one preamble block
/// (adr/2026-08-per-line-block-segmentation.md, superseding
/// adr/2026-07-block-segmentation-parbreak-tiling.md). Ranges are in bytes
/// and blocks tile the whole text — every byte belongs to exactly one
/// block, separators trail the block they follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub range: Range<usize>,
    /// Where the trailing separator (parbreak, trailing spacing) begins.
    /// The widget shows and edits only `content()`; the separator stays in
    /// the buffer, invisible, so the textarea carries no phantom blank
    /// lines — and emptying a block's content leaves a bare separator that
    /// merges away at the next resegmentation. The note's *last* block is
    /// the exception: its content runs to the end of the note, so the
    /// trailing empty line is real and the cursor can rest on it
    /// (adr/2026-08-cursor-always-in-the-note.md).
    pub content_end: usize,
    /// The block carries its own template import (the note's preamble), so a
    /// fragment compile must not prepend another one.
    pub standalone: bool,
}

impl Block {
    /// The slice the widget shows and edits: the source without its
    /// trailing separator.
    pub fn content(&self) -> Range<usize> {
        self.range.start..self.content_end
    }
}

/// Splits `text` into one block per physical line: every top-level
/// `Space`/`Parbreak` child's own newlines are the split points, so a
/// multi-line construct the parser reports as one child (a raw fence, a
/// multi-line `#let`/`#table`/equation) is never split — the tree, not
/// character counting, decides what merges (adr/2026-08-per-line-block-segmentation.md).
/// A leading run of 2+ `#import`/`#show`/`#meta` lines then folds into one
/// preamble block. Total: an empty or blank-only note is one block covering
/// it all, and the returned ranges tile `0..text.len()`. A note ending in a
/// newline gains a trailing zero-length block — the empty last line the
/// cursor rests on (adr/2026-08-cursor-always-in-the-note.md).
pub fn segment(text: &str) -> Vec<Block> {
    let root = typst_syntax::parse(text);
    // a note with no content anywhere is one block regardless of how many
    // blank lines it carries — the per-line split below only fires once
    // there is a line worth naming
    let all_blank = root.children().all(|child| {
        matches!(child.kind(), SyntaxKind::Space | SyntaxKind::Parbreak)
    });
    if all_blank {
        return vec![Block {
            range: 0..text.len(),
            content_end: text.len(),
            standalone: false,
        }];
    }

    let mut blocks = Vec::new();
    let mut preamble_shaped = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let mut content_end = 0;
    let mut standalone = false;
    // whether the in-progress line carries an import/show/meta child —
    // reset at every split, folded into the preamble decision afterward
    let mut line_shaped = false;
    for child in root.children() {
        let len = child.len();
        match child.kind() {
            SyntaxKind::Space | SyntaxKind::Parbreak => {
                let slice = text.get(offset..offset + len).unwrap_or("");
                for (rel, ch) in slice.char_indices() {
                    if ch == '\n' {
                        let end = offset + rel + 1;
                        blocks.push(Block {
                            range: start..end,
                            content_end,
                            standalone,
                        });
                        preamble_shaped.push(line_shaped);
                        start = end;
                        content_end = end;
                        standalone = false;
                        line_shaped = false;
                    }
                }
                // a whitespace child with no newline just advances the
                // offset — it must not move content_end, as today
            }
            _ => {
                standalone |= child.kind() == SyntaxKind::ModuleImport;
                line_shaped |= is_preamble_shaped(child);
                content_end = offset + len;
            }
        }
        offset += len;
    }
    blocks.push(Block {
        range: start..text.len(),
        content_end: text.len(),
        standalone,
    });
    preamble_shaped.push(line_shaped);

    fold_preamble(&mut blocks, &preamble_shaped);
    blocks
}

/// Whether `child` is one of the three lines a note's preamble is made of:
/// an import, a show rule, or a call to `meta` — read off the callee
/// identifier itself, never a string prefix on the source (the phase-2
/// never-regex rule).
fn is_preamble_shaped(child: &SyntaxNode) -> bool {
    matches!(
        child.kind(),
        SyntaxKind::ModuleImport | SyntaxKind::ShowRule
    ) || child.cast::<ast::FuncCall>().is_some_and(|call| {
        matches!(
            call.callee(),
            ast::Expr::Ident(name) if name.as_str() == "meta"
        )
    })
}

/// Folds a leading run of 2+ preamble-shaped line-blocks into one. A lone
/// import with nothing preamble-shaped after it is left as its own block —
/// the fold only earns its keep once it merges something — and a blank line
/// is not preamble-shaped, so it ends the run before `take_while` reaches
/// it, honestly splitting a preamble that has one inside it.
fn fold_preamble(blocks: &mut Vec<Block>, shaped: &[bool]) {
    let run = shaped.iter().take_while(|&&line| line).count();
    if run < 2 {
        return;
    }
    let first_start = blocks[0].range.start;
    let last = blocks[run - 1].clone();
    let standalone = blocks[..run].iter().any(|block| block.standalone);
    blocks.splice(
        0..run,
        [Block {
            range: first_start..last.range.end,
            content_end: last.content_end,
            standalone,
        }],
    );
}

/// The index of the block owning byte `offset`. Total for any output of
/// `segment`: offsets at or past the end clamp to the last block.
pub fn block_at(blocks: &[Block], offset: usize) -> usize {
    for (index, block) in blocks.iter().enumerate() {
        if offset < block.range.end {
            return index;
        }
    }
    blocks.len().saturating_sub(1)
}

/// How many indent guides each line draws: one per two-space level of its
/// own leading spaces, and — indent-blankline's rule — for a
/// whitespace-only line the smaller of its two nearest non-blank
/// neighbours' depths, so a blank line inside a nested run leaves the
/// run's rules unbroken while one at either end of the note, or beside a
/// top-level line, draws none. Odd leading spaces round down and a tab is
/// never an indent: the editor only ever writes `caret::INDENT`, two
/// spaces (adr/2026-08-tab-indents-in-every-mode.md). A multi-line block
/// (a raw fence, the preamble) counts as the one line it is drawn as and
/// takes its first line's depth.
pub fn guide_depths(lines: &[&str]) -> Vec<usize> {
    let own: Vec<Option<usize>> =
        lines.iter().map(|line| leading_depth(line)).collect();
    // the last non-blank depth strictly before each line, and the first
    // strictly after it — 0 where the note runs out, so a leading or a
    // trailing blank line takes the note's edge as a top-level neighbour
    let mut before = Vec::with_capacity(own.len());
    let mut carried = 0;
    for depth in &own {
        before.push(carried);
        carried = depth.unwrap_or(carried);
    }
    let mut after = Vec::with_capacity(own.len());
    let mut carried = 0;
    for depth in own.iter().rev() {
        after.push(carried);
        carried = depth.unwrap_or(carried);
    }
    after.reverse();
    own.iter()
        .zip(before)
        .zip(after)
        .map(|((own, before), after)| own.unwrap_or(before.min(after)))
        .collect()
}

/// One line's own indent depth, or `None` for a whitespace-only line —
/// which has no indent to speak of and takes its neighbours' instead.
fn leading_depth(line: &str) -> Option<usize> {
    if line.trim().is_empty() {
        return None;
    }
    Some(line.chars().take_while(|&ch| ch == ' ').count() / 2)
}

/// After the active block's content is replaced by `new_len` bytes, its
/// separator and every later block shift by the same delta. An out-of-range
/// `active` is a stale caller and shifts nothing.
pub fn resize(blocks: &mut [Block], active: usize, new_len: usize) {
    let Some(block) = blocks.get(active) else {
        return;
    };
    let old_end = block.content_end;
    let new_end = block.range.start + new_len;
    blocks[active].content_end = new_end;
    // every offset at or past the old content end shifts; subtracting the
    // old end first cannot underflow
    blocks[active].range.end = blocks[active].range.end - old_end + new_end;
    for later in &mut blocks[active + 1..] {
        later.range.start = later.range.start - old_end + new_end;
        later.range.end = later.range.end - old_end + new_end;
        later.content_end = later.content_end - old_end + new_end;
    }
}

/// What a fragment compiles to match the note's styling without repeating
/// its meta line: the template import and show rule, never `#meta` (the
/// template's `meta()` emits the visible meta line where called — only the
/// note's own preamble block should show it). Fragment-friendly margins are
/// the template's own business: its in-app palette columns carry them
/// (adr/2026-07-note-rendering-theme-input.md).
const FRAGMENT_PREAMBLE: &str =
    "#import \"/templates/template.typ\": *\n#show: note\n";

/// The source one block compiles from when its markup needs the Typst
/// fallback: `FRAGMENT_PREAMBLE` (unless the block is `standalone`, the
/// note's own preamble, which already carries the import) plus the block's
/// own `range` slice verbatim — content plus its trailing separator, so a
/// multi-line construct compiles exactly as it sits in the note. A block
/// whose range no longer resolves against `text` (a stale caller,
/// mirroring `resize_with_a_stale_index_is_dropped`) contributes no
/// content, only the preamble.
pub fn block_source(text: &str, block: &Block) -> String {
    let mut source = if block.standalone {
        String::new()
    } else {
        FRAGMENT_PREAMBLE.to_string()
    };
    if let Some(slice) = text.get(block.range.clone()) {
        source.push_str(slice);
    }
    source
}

/// JS `selectionStart` counts UTF-16 code units; block ranges count UTF-8
/// bytes. Clamps to the text's end, and to the character's start when the
/// probe lands mid-surrogate-pair.
pub fn byte_offset_of_utf16(text: &str, units: usize) -> usize {
    let mut remaining = units;
    for (offset, ch) in text.char_indices() {
        let width = ch.len_utf16();
        if remaining < width {
            return offset;
        }
        remaining -= width;
    }
    text.len()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const NOTE: &str = "#import \"/templates/template.typ\": *\n\
                        #show: note\n\
                        #meta(\n  id: \"2026-07-21\",\n  type: \"daily\",\n)\n\
                        \n\
                        = 2026-07-21\n\
                        \n\
                        Read about the [[zettelkasten]].\n";

    fn assert_tiles(blocks: &[Block], len: usize) {
        assert_eq!(blocks[0].range.start, 0);
        for pair in blocks.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start, "{blocks:?}");
        }
        let Some(last) = blocks.last() else {
            panic!("segment never returns an empty vec");
        };
        assert_eq!(last.range.end, len);
    }

    #[test]
    fn a_real_note_merges_its_preamble_and_splits_the_rest_per_line() {
        let blocks = segment(NOTE);
        assert_eq!(blocks.len(), 6, "{blocks:?}");
        assert_tiles(&blocks, NOTE.len());
        assert!(blocks[0].standalone, "the preamble carries the import");
        assert!(!blocks[1].standalone);
        assert!(!blocks[2].standalone);
        assert!(!blocks[3].standalone);
        assert!(!blocks[4].standalone);
        assert!(!blocks[5].standalone);
        // import, show and meta merge into one preamble block
        assert!(NOTE[blocks[0].content()].starts_with("#import"));
        assert!(NOTE[blocks[0].content()].contains("#show: note"));
        assert!(NOTE[blocks[0].content()].ends_with(')'));
        // the blank line, heading, blank line and prose each get their own
        assert!(blocks[1].content().is_empty(), "the blank line");
        assert!(NOTE[blocks[2].content()].starts_with("= 2026-07-21"));
        assert!(blocks[3].content().is_empty(), "the blank line");
        assert!(NOTE[blocks[4].content()].starts_with("Read about"));
        // the note ends in a newline: a genuinely empty last block, where
        // the cursor rests (adr/2026-08-cursor-always-in-the-note.md)
        assert!(blocks[5].content().is_empty());
    }

    #[test]
    fn separators_trail_the_block_but_stay_out_of_its_content() {
        let blocks = segment(NOTE);
        assert!(NOTE[blocks[0].range.clone()].ends_with(")\n"));
        assert!(NOTE[blocks[2].range.clone()].ends_with("21\n"));
        // the widget shows content only: no phantom blank lines between
        // blocks…
        assert!(NOTE[blocks[0].content()].ends_with(')'));
        assert!(NOTE[blocks[2].content()].ends_with("21"));
        // …but the note's final newline is real: it opens the genuinely
        // empty last block where the cursor rests, while the prose line
        // that carries it keeps only its own text
        // (adr/2026-08-cursor-always-in-the-note.md)
        assert_eq!(
            &NOTE[blocks[4].content()],
            "Read about the [[zettelkasten]].",
        );
        assert_eq!(blocks[5].content(), blocks[5].range);
        assert!(blocks[5].content().is_empty());
    }

    #[test]
    fn a_multi_line_list_run_is_one_block_per_item() {
        let text = "- one\n- two\n- three\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 4, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert_eq!(&text[blocks[0].content()], "- one");
        assert_eq!(&text[blocks[1].content()], "- two");
        assert_eq!(&text[blocks[2].content()], "- three");
        assert!(blocks[3].content().is_empty(), "the trailing empty line");
    }

    #[test]
    fn a_raw_fence_with_internal_blank_lines_stays_whole() {
        // the blank line *inside* the fence never splits it — only the
        // blank line after it, between the fence and "after", is its own
        // block
        let text = "```\na\n\nb\n```\n\nafter\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 4, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert_eq!(&text[blocks[0].content()], "```\na\n\nb\n```");
        assert!(blocks[1].content().is_empty(), "the blank line after it");
        assert_eq!(&text[blocks[2].content()], "after");
    }

    #[test]
    fn a_multi_line_let_binding_stays_whole() {
        let text = "= title\n\n#let x = (\n 1,\n)\n\nuses #x\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 6, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert_eq!(&text[blocks[2].content()], "#let x = (\n 1,\n)");
        assert_eq!(&text[blocks[4].content()], "uses #x");
    }

    #[test]
    fn a_multi_line_table_call_stays_whole() {
        let text = "#table(\n [a], [b],\n)\nafter\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert_eq!(&text[blocks[0].content()], "#table(\n [a], [b],\n)");
        assert_eq!(&text[blocks[1].content()], "after");
    }

    #[test]
    fn multi_line_display_math_stays_whole() {
        let text = "$ a \n+ b $\nafter\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert_eq!(&text[blocks[0].content()], "$ a \n+ b $");
        assert_eq!(&text[blocks[1].content()], "after");
    }

    #[test]
    fn leading_blank_lines_get_one_block_each() {
        let text = "\n\nfirst\n\nsecond\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 6, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert!(blocks[0].content().is_empty());
        assert!(blocks[1].content().is_empty());
        assert_eq!(&text[blocks[2].content()], "first");
        assert!(blocks[3].content().is_empty());
        assert_eq!(&text[blocks[4].content()], "second");
        assert!(blocks[5].content().is_empty());
    }

    #[test]
    fn blank_only_and_empty_notes_are_one_block() {
        // a note with no content anywhere never earns a per-line split —
        // only a line worth naming does
        for text in ["", "\n\n\n"] {
            let blocks = segment(text);
            assert_eq!(blocks.len(), 1, "{text:?} -> {blocks:?}");
            assert_tiles(&blocks, text.len());
            assert!(!blocks[0].standalone);
        }
    }

    #[test]
    fn a_blank_line_inside_the_preamble_splits_it_honestly() {
        let text = "#import \"/templates/template.typ\": *\n\n#show: note\n";
        let blocks = segment(text);
        // the blank line breaks the run before it reaches two lines, so
        // nothing folds: import, blank, show, and the note's own trailing
        // empty line
        assert_eq!(blocks.len(), 4, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert!(blocks[0].standalone, "the import half keeps the marker");
        assert!(blocks[1].content().is_empty());
        assert!(
            !blocks[2].standalone,
            "the show half compiles as a fragment"
        );
        assert!(blocks[3].content().is_empty(), "the note's own final line");
    }

    #[test]
    fn block_at_is_total_over_the_note() {
        let blocks = segment(NOTE);
        assert_eq!(block_at(&blocks, 0), 0);
        assert_eq!(block_at(&blocks, blocks[2].range.start), 2);
        assert_eq!(block_at(&blocks, blocks[2].range.end - 1), 2);
        assert_eq!(block_at(&blocks, NOTE.len()), 5, "end clamps to last");
        assert_eq!(block_at(&blocks, NOTE.len() + 40), 5, "past end clamps");
        assert_eq!(block_at(&[], 5), 0, "an empty slice cannot panic");
    }

    #[test]
    fn resize_shifts_the_separator_and_every_later_block() {
        let mut blocks = segment(NOTE);
        let sep = blocks[2].range.len() - blocks[2].content().len();
        let grown = blocks[2].content().len() + 7;
        let starts: Vec<usize> =
            blocks.iter().map(|b| b.range.start).collect();
        resize(&mut blocks, 2, grown);
        assert_eq!(blocks[2].content().len(), grown);
        assert_eq!(blocks[2].range.len(), grown + sep, "the separator rides");
        assert_eq!(blocks[3].range.start, starts[3] + 7);
        assert_eq!(blocks[0].range.start, starts[0], "earlier blocks hold");

        let shrunk = blocks[2].content().len() - 10;
        resize(&mut blocks, 2, shrunk);
        assert_eq!(blocks[3].range.start, starts[3] - 3);
        assert_tiles(&blocks, blocks.last().expect("a block").range.end);
    }

    #[test]
    fn resize_of_the_last_block_moves_nothing_else() {
        let mut blocks = segment(NOTE);
        let starts: Vec<usize> =
            blocks.iter().map(|b| b.range.start).collect();
        let last = blocks.len() - 1;
        resize(&mut blocks, last, 3);
        assert_eq!(blocks[last].content_end, starts[last] + 3);
        assert_eq!(blocks[last].range.end, starts[last] + 3, "no separator");
        assert_eq!(blocks[last - 1].range.start, starts[last - 1]);
    }

    #[test]
    fn resize_with_a_stale_index_is_dropped() {
        let mut blocks = segment(NOTE);
        let before = blocks.clone();
        resize(&mut blocks, 9, 100);
        assert_eq!(blocks, before);
    }

    #[test]
    fn a_standalone_block_gets_no_synthesized_preamble() {
        let blocks = segment(NOTE);
        // blocks[0] is the note's own preamble: it already carries the
        // import, so nothing is prepended
        let source = block_source(NOTE, &blocks[0]);
        assert!(source.starts_with("#import"), "{source}");
        assert_eq!(
            source.matches("#import").count(),
            1,
            "the synthesized preamble never doubles the real one: {source}"
        );
    }

    #[test]
    fn a_non_standalone_block_gets_one_synthesized_preamble() {
        let blocks = segment(NOTE);
        // blocks[2] is the heading, which carries no import of its own
        let source = block_source(NOTE, &blocks[2]);
        assert!(source.starts_with(FRAGMENT_PREAMBLE), "{source}");
        assert_eq!(
            source.matches(FRAGMENT_PREAMBLE).count(),
            1,
            "exactly one synthesized preamble: {source}"
        );
        assert!(
            !FRAGMENT_PREAMBLE.contains("meta("),
            "a second #meta would repeat the visible meta line"
        );
        assert!(source.contains("= 2026-07-21"));
    }

    #[test]
    fn a_blocks_own_range_carries_content_and_separator_verbatim() {
        let blocks = segment(NOTE);
        let source = block_source(NOTE, &blocks[4]);
        assert!(
            source.ends_with("Read about the [[zettelkasten]].\n"),
            "{source}"
        );
    }

    #[test]
    fn a_stale_block_contributes_no_content_only_the_preamble() {
        // a block whose range no longer resolves against `text` (a stale
        // caller, mirroring `resize_with_a_stale_index_is_dropped`) is
        // skipped rather than panicking or corrupting the source
        let text = "= heading\n";
        let stale = Block {
            range: text.len() + 5..text.len() + 9,
            content_end: text.len() + 9,
            standalone: false,
        };
        let source = block_source(text, &stale);
        assert_eq!(source, FRAGMENT_PREAMBLE, "{source}");
    }

    // -- guide_depths -----------------------------------------------------

    #[test]
    fn a_nested_run_draws_one_guide_per_two_space_level() {
        let lines = ["- a", "  - b", "    - c", "  - d", "e"];
        assert_eq!(guide_depths(&lines), vec![0, 1, 2, 1, 0]);
    }

    #[test]
    fn a_blank_line_takes_the_shallower_of_its_two_neighbours() {
        // inside a nested run the rules carry through; before a dedent the
        // shallower side wins, and a top-level neighbour ends them
        let lines = ["    a", "", "    b", "", "  c", "", "d"];
        assert_eq!(guide_depths(&lines), vec![2, 2, 2, 1, 1, 0, 0]);
    }

    #[test]
    fn a_blank_line_at_either_end_of_the_note_draws_nothing() {
        let lines = ["", "  a", "    b", "   ", ""];
        assert_eq!(guide_depths(&lines), vec![0, 1, 2, 0, 0]);
    }

    #[test]
    fn a_tab_is_never_an_indent_and_an_odd_space_rounds_down() {
        // the editor only ever writes two-space levels
        // (adr/2026-08-tab-indents-in-every-mode.md), so a tabbed line is
        // read as top level rather than guessing a width for it
        let lines = ["\t\ta", "   b", "     c"];
        assert_eq!(guide_depths(&lines), vec![0, 1, 2]);
    }

    #[test]
    fn a_note_with_no_lines_draws_no_guides() {
        assert_eq!(guide_depths(&[]), Vec::<usize>::new());
    }

    #[test]
    fn utf16_offsets_convert_to_bytes() {
        assert_eq!(byte_offset_of_utf16("abc", 2), 2, "ascii is identity");
        assert_eq!(byte_offset_of_utf16("été", 2), 3, "two-byte chars");
        assert_eq!(byte_offset_of_utf16("a🙂b", 3), 5, "surrogate pair");
        assert_eq!(byte_offset_of_utf16("a🙂b", 2), 1, "mid-pair clamps back");
        assert_eq!(byte_offset_of_utf16("abc", 9), 3, "past end clamps");
    }
}
