use std::ops::Range;

use typst_syntax::SyntaxKind;

/// One editable unit of a note: a maximal run of top-level markup children
/// between paragraph breaks (adr/2026-07-block-segmentation-parbreak-tiling.md).
/// Ranges are in bytes and blocks tile the whole text — every byte belongs to
/// exactly one block, separators trail the block they follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub range: Range<usize>,
    /// Where the trailing separator (parbreak, trailing spacing) begins.
    /// The widget shows and edits only `content()`; the separator stays in
    /// the buffer, invisible, so the textarea carries no phantom blank
    /// lines — and emptying a block's content leaves a bare separator that
    /// merges away at the next resegmentation.
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

/// Splits `text` into blocks at top-level `Parbreak` nodes. Total: an empty
/// or blank-only note is one block covering it all, leading blank lines
/// belong to block 0, and the returned ranges tile `0..text.len()`.
pub fn segment(text: &str) -> Vec<Block> {
    let root = typst_syntax::parse(text);
    let mut blocks = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let mut content_end = 0;
    let mut standalone = false;
    let mut seen_content = false;
    let mut split_pending = false;
    for child in root.children() {
        if child.kind() == SyntaxKind::Parbreak {
            // a break before any content splits nothing: leading blank
            // lines belong to the first block rather than forming their own
            split_pending = seen_content;
        } else {
            if split_pending {
                blocks.push(Block {
                    range: start..offset,
                    content_end,
                    standalone,
                });
                start = offset;
                content_end = offset;
                standalone = false;
                split_pending = false;
            }
            seen_content = true;
            standalone |= child.kind() == SyntaxKind::ModuleImport;
            // spacing never ends the content: a trailing newline stays in
            // the separator, spacing between siblings is swallowed when the
            // next real child advances past it
            if child.kind() != SyntaxKind::Space {
                content_end = offset + child.len();
            }
        }
        offset += child.len();
    }
    blocks.push(Block {
        range: start..text.len(),
        content_end,
        standalone,
    });
    blocks
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

/// The source a block's fragment compiles from: the slice as-is when the
/// block is standalone, otherwise under the synthesized preamble. A stale
/// range yields the preamble alone rather than panicking.
pub fn fragment_source(text: &str, block: &Block) -> String {
    let slice = text.get(block.range.clone()).unwrap_or("");
    if block.standalone {
        slice.to_string()
    } else {
        format!("{FRAGMENT_PREAMBLE}{slice}")
    }
}

/// Where a boundary arrow leaves the active block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slide {
    Prev,
    Next,
}

/// ArrowUp with the caret on the first line slides to the previous block,
/// ArrowDown on the last line to the next; anywhere else the arrow is the
/// browser's ordinary caret movement. `caret` is a byte offset; one that
/// lands off a char boundary is a stale probe and slides nowhere.
pub fn boundary_slide(
    block_text: &str,
    caret: usize,
    up: bool,
) -> Option<Slide> {
    let (before, after) = block_text.split_at_checked(caret)?;
    if up && !before.contains('\n') {
        Some(Slide::Prev)
    } else if !up && !after.contains('\n') {
        Some(Slide::Next)
    } else {
        None
    }
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
                        Read about the #l(\"zettelkasten\").\n";

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
    fn a_real_note_splits_into_preamble_heading_and_prose() {
        let blocks = segment(NOTE);
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_tiles(&blocks, NOTE.len());
        assert!(blocks[0].standalone, "the preamble carries the import");
        assert!(!blocks[1].standalone);
        assert!(!blocks[2].standalone);
        assert!(NOTE[blocks[1].range.clone()].starts_with("= 2026-07-21"));
        assert!(NOTE[blocks[2].range.clone()].starts_with("Read about"));
    }

    #[test]
    fn separators_trail_the_block_but_stay_out_of_its_content() {
        let blocks = segment(NOTE);
        assert!(NOTE[blocks[0].range.clone()].ends_with(")\n\n"));
        assert!(NOTE[blocks[1].range.clone()].ends_with("21\n\n"));
        // the widget shows content only: no phantom blank lines at the end
        assert!(NOTE[blocks[0].content()].ends_with(")"));
        assert!(NOTE[blocks[1].content()].ends_with("21"));
        assert_eq!(
            &NOTE[blocks[2].content()],
            "Read about the #l(\"zettelkasten\").",
            "the note's final newline is separator too"
        );
    }

    #[test]
    fn a_multi_line_list_run_is_one_block() {
        let text = "- one\n- two\n- three\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_tiles(&blocks, text.len());
    }

    #[test]
    fn a_raw_fence_with_internal_blank_lines_stays_whole() {
        let text = "```\na\n\nb\n```\n\nafter\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert!(text[blocks[0].range.clone()].contains("```"));
        assert!(text[blocks[1].range.clone()].starts_with("after"));
    }

    #[test]
    fn a_let_binding_after_a_parbreak_is_its_own_block() {
        let text = "= title\n\n#let x = 3\n\nuses #x\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert!(text[blocks[1].range.clone()].starts_with("#let"));
    }

    #[test]
    fn leading_blank_lines_belong_to_the_first_block() {
        let text = "\n\nfirst\n\nsecond\n";
        let blocks = segment(text);
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_tiles(&blocks, text.len());
        assert!(text[blocks[0].range.clone()].contains("first"));
    }

    #[test]
    fn blank_only_and_empty_notes_are_one_block() {
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
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert!(blocks[0].standalone, "the import half keeps the marker");
        assert!(
            !blocks[1].standalone,
            "the show half compiles as a fragment"
        );
    }

    #[test]
    fn block_at_is_total_over_the_note() {
        let blocks = segment(NOTE);
        assert_eq!(block_at(&blocks, 0), 0);
        assert_eq!(block_at(&blocks, blocks[1].range.start), 1);
        assert_eq!(block_at(&blocks, blocks[1].range.end - 1), 1);
        assert_eq!(block_at(&blocks, NOTE.len()), 2, "end clamps to last");
        assert_eq!(block_at(&blocks, NOTE.len() + 40), 2, "past end clamps");
        assert_eq!(block_at(&[], 5), 0, "an empty slice cannot panic");
    }

    #[test]
    fn resize_shifts_the_separator_and_every_later_block() {
        let mut blocks = segment(NOTE);
        let sep = blocks[1].range.len() - blocks[1].content().len();
        let grown = blocks[1].content().len() + 7;
        let starts: Vec<usize> =
            blocks.iter().map(|b| b.range.start).collect();
        resize(&mut blocks, 1, grown);
        assert_eq!(blocks[1].content().len(), grown);
        assert_eq!(blocks[1].range.len(), grown + sep, "the separator rides");
        assert_eq!(blocks[2].range.start, starts[2] + 7);
        assert_eq!(blocks[0].range.start, starts[0], "earlier blocks hold");

        let shrunk = blocks[1].content().len() - 10;
        resize(&mut blocks, 1, shrunk);
        assert_eq!(blocks[2].range.start, starts[2] - 3);
        assert_tiles(&blocks, blocks[2].range.end);
    }

    #[test]
    fn resize_of_the_last_block_moves_nothing_else() {
        let mut blocks = segment(NOTE);
        let starts: Vec<usize> =
            blocks.iter().map(|b| b.range.start).collect();
        let sep = blocks[2].range.len() - blocks[2].content().len();
        resize(&mut blocks, 2, 3);
        assert_eq!(blocks[2].content_end, starts[2] + 3);
        assert_eq!(blocks[2].range.end, starts[2] + 3 + sep);
        assert_eq!(blocks[1].range.start, starts[1]);
    }

    #[test]
    fn resize_with_a_stale_index_is_dropped() {
        let mut blocks = segment(NOTE);
        let before = blocks.clone();
        resize(&mut blocks, 9, 100);
        assert_eq!(blocks, before);
    }

    #[test]
    fn fragments_get_the_preamble_and_standalone_blocks_do_not() {
        let blocks = segment(NOTE);
        let preamble = fragment_source(NOTE, &blocks[0]);
        assert_eq!(preamble, &NOTE[blocks[0].range.clone()]);

        let heading = fragment_source(NOTE, &blocks[1]);
        assert!(heading.starts_with(FRAGMENT_PREAMBLE));
        assert!(heading.ends_with(&NOTE[blocks[1].range.clone()]));
        assert!(
            !FRAGMENT_PREAMBLE.contains("meta("),
            "a second #meta would repeat the visible meta line"
        );
    }

    #[test]
    fn a_stale_fragment_range_yields_the_preamble_alone() {
        let block = Block {
            range: 5..NOTE.len() + 9,
            content_end: NOTE.len() + 9,
            standalone: false,
        };
        assert_eq!(fragment_source(NOTE, &block), FRAGMENT_PREAMBLE);
    }

    #[test]
    fn boundary_slides_only_fire_on_the_edge_lines() {
        let text = "first line\nlast line";
        assert_eq!(boundary_slide(text, 5, true), Some(Slide::Prev));
        assert_eq!(boundary_slide(text, 5, false), None, "newline follows");
        assert_eq!(boundary_slide(text, 15, false), Some(Slide::Next));
        assert_eq!(boundary_slide(text, 15, true), None, "newline precedes");
    }

    #[test]
    fn a_single_line_block_slides_both_ways() {
        assert_eq!(boundary_slide("only", 2, true), Some(Slide::Prev));
        assert_eq!(boundary_slide("only", 2, false), Some(Slide::Next));
    }

    #[test]
    fn a_caret_off_a_char_boundary_slides_nowhere() {
        assert_eq!(boundary_slide("été", 1, true), None);
        assert_eq!(boundary_slide("été", 1, false), None);
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
