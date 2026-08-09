//! Pure text math and the render model for the owned-caret widget: every
//! offset in and out is a byte position on a grapheme-cluster boundary of
//! the active block's source (adr/2026-08-caret-on-editor-note-bytes.md).
//! The widget draws whatever `layout` answers; nothing here touches the
//! DOM, so all of it tests headlessly.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// One caret movement the phase-0 widget knows — the arrow-key vocabulary,
/// not vim's (that grammar arrives in v2 phases 1–2 above this).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    WordLeft,
    WordRight,
}

/// The caret the widget draws: insert's bar between clusters, or normal
/// mode's box over the cluster after `head` (unused until v2 phase 1, drawn
/// and tested now so the mode flip is one parameter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Bar,
    Box,
}

/// One span of one rendered line. `start` offsets are block-relative bytes
/// carried into the DOM as `data-start`, so the mouse hit probe can answer
/// in a coordinate the editor understands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text {
        start: usize,
        text: String,
    },
    Selected {
        start: usize,
        text: String,
    },
    /// The bar caret: zero width, drawn between clusters.
    Caret,
    /// The box caret: the cluster under it, inverted; a no-break space
    /// stands in at the end of a line, where no cluster follows.
    CaretBox {
        start: usize,
        cluster: String,
    },
    /// The live IME composition ("^" mid–dead-key), previewed at the caret
    /// but absent from the buffer until compositionend commits it. Carries
    /// the caret's offset so a press during composition still maps near it.
    Preview {
        start: usize,
        text: String,
    },
}

impl Piece {
    /// How the widget draws this piece: its css class, `data-start` byte
    /// and text — or `None` for the zero-width bar, which the widget draws
    /// as its own keyed span. One method, so the widget renders every
    /// visible piece through one arm and phase 1's box caret arrives
    /// without touching the rsx.
    pub fn drawn(&self) -> Option<(&'static str, usize, &str)> {
        match self {
            Piece::Text { start, text } => Some(("", *start, text)),
            Piece::Selected { start, text } => Some(("sel", *start, text)),
            Piece::Caret => None,
            Piece::CaretBox { start, cluster } => {
                Some(("caret-box", *start, cluster))
            }
            Piece::Preview { start, text } => Some(("compose", *start, text)),
        }
    }
}

/// One logical line of the active block, ready to render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub pieces: Vec<Piece>,
}

/// The whole render model: the block's source cut into lines and pieces,
/// with the selection highlighted, the caret placed at `head`, and any
/// composition previewed there. Offsets off a char boundary are clamped
/// back — a stale caller draws a caret, never panics.
pub fn layout(
    source: &str,
    anchor: usize,
    head: usize,
    preview: Option<&str>,
    shape: Shape,
) -> Vec<Line> {
    let anchor = clamp_boundary(source, anchor);
    let head = clamp_boundary(source, head);
    let selection = anchor.min(head)..anchor.max(head);
    // the box caret owns the cluster after head, unless a newline or the
    // block's end is there — then the end-of-line stand-in draws instead
    let boxed = shape == Shape::Box
        && preview.is_none()
        && source
            .get(head..)
            .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('\n'));
    let box_span = boxed.then(|| head..next_cluster(source, head));

    let mut lines = Vec::new();
    let mut line_start = 0;
    let breaks: Vec<usize> = source
        .char_indices()
        .filter(|(_, ch)| *ch == '\n')
        .map(|(offset, _)| offset)
        .collect();
    for index in 0..=breaks.len() {
        let line_end = breaks.get(index).copied().unwrap_or(source.len());
        lines.push(build_line(
            source,
            line_start..line_end,
            &selection,
            head,
            box_span.as_ref(),
            preview,
            shape,
        ));
        line_start = line_end + 1;
    }
    lines
}

/// One line's pieces: the line's span cut at every boundary that matters —
/// selection edges, the caret, the box cluster — each slice classified,
/// with the bar caret (and any preview) slotted in at `head`.
#[allow(clippy::too_many_arguments)]
fn build_line(
    source: &str,
    line: Range<usize>,
    selection: &Range<usize>,
    head: usize,
    box_span: Option<&Range<usize>>,
    preview: Option<&str>,
    shape: Shape,
) -> Line {
    let mut cuts = vec![line.start, line.end];
    for offset in [selection.start, selection.end, head] {
        if offset > line.start && offset < line.end {
            cuts.push(offset);
        }
    }
    if let Some(span) = box_span
        && span.start >= line.start
        && span.end <= line.end
    {
        cuts.push(span.start);
        cuts.push(span.end);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let head_here = head >= line.start && head <= line.end;
    let mut pieces = Vec::new();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if head_here && head == start {
            push_caret(&mut pieces, head, box_span.is_some(), preview, shape);
        }
        let text = source.get(start..end).unwrap_or("").to_string();
        if box_span.is_some_and(|span| *span == (start..end)) {
            pieces.push(Piece::CaretBox {
                start,
                cluster: text,
            });
        } else if start >= selection.start && end <= selection.end {
            pieces.push(Piece::Selected { start, text });
        } else {
            pieces.push(Piece::Text { start, text });
        }
    }
    if head_here && head == line.end {
        push_caret(&mut pieces, head, box_span.is_some(), preview, shape);
    }
    Line { pieces }
}

/// The caret pieces at `head`: the preview first when one is composing,
/// then the bar — or, under the box shape with no cluster to sit on (the
/// end of a line, where `box_span` is `None`), the stand-in space box.
fn push_caret(
    pieces: &mut Vec<Piece>,
    head: usize,
    has_box_cluster: bool,
    preview: Option<&str>,
    shape: Shape,
) {
    if let Some(text) = preview {
        pieces.push(Piece::Preview {
            start: head,
            text: text.to_string(),
        });
        pieces.push(Piece::Caret);
        return;
    }
    match shape {
        Shape::Bar => pieces.push(Piece::Caret),
        // the box cluster piece itself draws the caret
        Shape::Box if has_box_cluster => {}
        Shape::Box => pieces.push(Piece::CaretBox {
            start: head,
            cluster: "\u{a0}".to_string(),
        }),
    }
}

/// The byte before `at` where the previous grapheme cluster starts — the
/// backspace and ArrowLeft step. Already at the start answers the start.
pub fn prev_cluster(text: &str, at: usize) -> usize {
    let at = clamp_boundary(text, at);
    text.get(..at)
        .map(|before| {
            before
                .grapheme_indices(true)
                .next_back()
                .map(|(offset, _)| offset)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// The byte after `at` where the next grapheme cluster starts — the delete
/// and ArrowRight step. Already at the end answers the end.
pub fn next_cluster(text: &str, at: usize) -> usize {
    let at = clamp_boundary(text, at);
    text.get(at..)
        .and_then(|rest| rest.graphemes(true).next())
        .map(|cluster| at + cluster.len())
        .unwrap_or(text.len())
}

/// Where the line holding `at` begins — the byte after the previous
/// newline, or the text's start.
pub fn line_start(text: &str, at: usize) -> usize {
    let at = clamp_boundary(text, at);
    text.get(..at)
        .and_then(|before| before.rfind('\n'))
        .map(|newline| newline + 1)
        .unwrap_or(0)
}

/// Where the line holding `at` ends — the next newline's byte, or the
/// text's end.
pub fn line_end(text: &str, at: usize) -> usize {
    let at = clamp_boundary(text, at);
    text.get(at..)
        .and_then(|rest| rest.find('\n'))
        .map(|newline| at + newline)
        .unwrap_or(text.len())
}

/// One line up or down, keeping the goal column: the column is `goal` when
/// one is remembered (a run of vertical moves through short lines must not
/// forget where it started), otherwise the caret's own. Answers the new
/// offset and the column to remember; `None` means the note's edge in that
/// direction — the widget's cue to slide blocks or clamp.
pub fn vertical(
    text: &str,
    at: usize,
    up: bool,
    goal: Option<usize>,
) -> Option<(usize, usize)> {
    let at = clamp_boundary(text, at);
    let start = line_start(text, at);
    let column = goal.unwrap_or_else(|| {
        text.get(start..at)
            .map(|before| before.graphemes(true).count())
            .unwrap_or(0)
    });
    let target = if up {
        if start == 0 {
            return None;
        }
        let previous = line_start(text, start - 1);
        previous..start - 1
    } else {
        let end = line_end(text, at);
        if end >= text.len() {
            return None;
        }
        (end + 1)..line_end(text, end + 1)
    };
    let line = text.get(target.clone()).unwrap_or("");
    let offset = line
        .grapheme_indices(true)
        .nth(column)
        .map(|(offset, _)| offset)
        .unwrap_or(line.len());
    Some((target.start + offset, column))
}

/// The start of the word before `at`: back over any non-word clusters, then
/// back over the word — Ctrl+ArrowLeft and Ctrl+Backspace, the textarea's
/// word step kept.
pub fn word_left(text: &str, at: usize) -> usize {
    let at = clamp_boundary(text, at);
    // the clamp guarantees a boundary, so the slice always answers
    let before = text.get(..at).unwrap_or_default();
    let word_start = before
        .char_indices()
        .rev()
        .scan(true, |skipping, (offset, ch)| {
            if *skipping && !ch.is_alphanumeric() {
                Some(None)
            } else if ch.is_alphanumeric() {
                *skipping = false;
                Some(Some(offset))
            } else {
                None
            }
        })
        .flatten()
        .last();
    word_start.unwrap_or(0)
}

/// The end of the word after `at`: forward over any non-word clusters, then
/// forward over the word — Ctrl+ArrowRight.
pub fn word_right(text: &str, at: usize) -> usize {
    let at = clamp_boundary(text, at);
    // the clamp guarantees a boundary, so the slice always answers
    let rest = text.get(at..).unwrap_or_default();
    let word_end = rest
        .char_indices()
        .scan(true, |skipping, (offset, ch)| {
            if *skipping && !ch.is_alphanumeric() {
                Some(None)
            } else if ch.is_alphanumeric() {
                *skipping = false;
                Some(Some(offset + ch.len_utf8()))
            } else {
                None
            }
        })
        .flatten()
        .last();
    word_end.map(|end| at + end).unwrap_or(text.len())
}

/// `at` clamped into the text and back onto a char boundary — every public
/// function's first step, so a stale offset degrades instead of panicking.
fn clamp_boundary(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    (0..=at)
        .rev()
        .find(|&offset| text.is_char_boundary(offset))
        .unwrap_or(0)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    // -- cluster steps -------------------------------------------------------

    #[test]
    fn cluster_steps_walk_french_text_char_wise() {
        let text = "été";
        assert_eq!(next_cluster(text, 0), 2, "é is two bytes");
        assert_eq!(next_cluster(text, 2), 3);
        assert_eq!(next_cluster(text, 3), 5);
        assert_eq!(next_cluster(text, 5), 5, "the end holds");
        assert_eq!(prev_cluster(text, 5), 3);
        assert_eq!(prev_cluster(text, 3), 2);
        assert_eq!(prev_cluster(text, 2), 0);
        assert_eq!(prev_cluster(text, 0), 0, "the start holds");
    }

    #[test]
    fn cluster_steps_keep_combining_marks_whole() {
        // e + combining acute is one cluster, two chars
        let text = "e\u{301}x";
        assert_eq!(next_cluster(text, 0), 3);
        assert_eq!(prev_cluster(text, 3), 0);
    }

    #[test]
    fn cluster_steps_clamp_stale_offsets() {
        assert_eq!(next_cluster("été", 1), 2, "mid-char floors to the char");
        assert_eq!(prev_cluster("été", 1), 0);
        assert_eq!(next_cluster("abc", 9), 3, "past the end clamps");
        assert_eq!(prev_cluster("abc", 9), 2);
        assert_eq!(prev_cluster("", 0), 0, "empty text holds");
        assert_eq!(next_cluster("", 4), 0);
    }

    // -- line edges ----------------------------------------------------------

    #[test]
    fn line_edges_find_the_surrounding_newlines() {
        let text = "un\ndeux\ntrois";
        assert_eq!(line_start(text, 0), 0);
        assert_eq!(line_end(text, 0), 2);
        assert_eq!(line_start(text, 5), 3);
        assert_eq!(line_end(text, 5), 7);
        assert_eq!(line_start(text, 13), 8);
        assert_eq!(line_end(text, 13), 13, "the last line ends at the end");
        assert_eq!(line_start(text, 2), 0, "a caret on the newline stays");
        assert_eq!(line_end(text, 2), 2);
    }

    #[test]
    fn line_edges_survive_stale_offsets() {
        assert_eq!(line_start("été", 1), 0);
        assert_eq!(line_end("été", 99), 5);
    }

    // -- vertical movement and the goal column -------------------------------

    #[test]
    fn vertical_moves_between_lines_keeping_the_column() {
        let text = "premier\ndeux\ntroisième";
        // from column 6 of line 0, down: line 1 is short, clamp to its end
        let (down, goal) = vertical(text, 6, false, None).expect("moves");
        assert_eq!(down, 12, "the end of deux, before its newline");
        assert_eq!(goal, 6);
        // down again with the goal: line 2 is long enough, column 6 returns
        let (again, goal) =
            vertical(text, down, false, Some(goal)).expect("moves");
        assert_eq!(goal, 6);
        assert_eq!(&text[again..again + 2], "è");
    }

    #[test]
    fn vertical_counts_columns_in_clusters_not_bytes() {
        let text = "été\nabc";
        // caret after "été" (byte 5) is column 3; down lands on byte 3 of
        // the ascii line, not byte 5
        let (down, goal) = vertical(text, 5, false, None).expect("moves");
        assert_eq!(goal, 3);
        assert_eq!(down, 6 + 3);
    }

    #[test]
    fn vertical_answers_none_at_the_edges() {
        let text = "un\ndeux";
        assert_eq!(vertical(text, 1, true, None), None, "first line, up");
        assert_eq!(vertical(text, 4, false, None), None, "last line, down");
    }

    #[test]
    fn vertical_up_mirrors_down() {
        let text = "premier\ndeux";
        let (up, goal) = vertical(text, 8 + 4, true, None).expect("moves");
        assert_eq!(goal, 4);
        assert_eq!(up, 4);
    }

    // -- word steps ----------------------------------------------------------

    #[test]
    fn word_steps_cross_punctuation_and_accents() {
        let text = "l'idée est là";
        assert_eq!(word_left(text, 7), 2, "back to the start of idée");
        assert_eq!(word_left(text, 2), 0, "back over the apostrophe to l");
        assert_eq!(word_left(text, 0), 0, "the start holds");
        assert_eq!(word_right(text, 0), 1, "to the end of l");
        assert_eq!(
            word_right(text, 1),
            7,
            "over the apostrophe to idée's end"
        );
        assert_eq!(&text[7..8], " ");
        assert_eq!(word_right(text, 8), 11);
    }

    #[test]
    fn word_steps_clamp_at_the_ends() {
        let text = "mot  ";
        assert_eq!(word_right(text, 3), 5, "only spaces left: the end");
        assert_eq!(word_left("  mot", 2), 0, "only spaces before: the start");
        assert_eq!(word_left("", 0), 0);
        assert_eq!(word_right("", 0), 0);
    }

    // -- layout: the render model --------------------------------------------

    fn texts(line: &Line) -> Vec<(&'static str, String)> {
        line.pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text { text, .. } => ("text", text.clone()),
                Piece::Selected { text, .. } => ("sel", text.clone()),
                Piece::Caret => ("caret", String::new()),
                Piece::CaretBox { cluster, .. } => ("box", cluster.clone()),
                Piece::Preview { text, .. } => ("preview", text.clone()),
            })
            .collect()
    }

    #[test]
    fn a_bar_caret_splits_its_line() {
        let lines = layout("abc", 1, 1, None, Shape::Bar);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            texts(&lines[0]),
            [
                ("text", "a".to_string()),
                ("caret", String::new()),
                ("text", "bc".to_string()),
            ]
        );
    }

    #[test]
    fn caret_at_the_edges_needs_no_split() {
        let start = layout("ab", 0, 0, None, Shape::Bar);
        assert_eq!(
            texts(&start[0]),
            [("caret", String::new()), ("text", "ab".to_string())]
        );
        let end = layout("ab", 2, 2, None, Shape::Bar);
        assert_eq!(
            texts(&end[0]),
            [("text", "ab".to_string()), ("caret", String::new())]
        );
    }

    #[test]
    fn the_caret_lands_on_its_own_line() {
        // a trailing newline is a real empty last line
        // (adr/2026-08-cursor-always-in-the-note.md)
        let lines = layout("ab\n", 3, 3, None, Shape::Bar);
        assert_eq!(lines.len(), 2);
        assert_eq!(texts(&lines[0]), [("text", "ab".to_string())]);
        assert_eq!(texts(&lines[1]), [("caret", String::new())]);
    }

    #[test]
    fn a_selection_highlights_across_lines() {
        // anchor after "a", head at "d": the highlight spans the newline
        let lines = layout("ab\ncd", 1, 4, None, Shape::Bar);
        assert_eq!(
            texts(&lines[0]),
            [("text", "a".to_string()), ("sel", "b".to_string())]
        );
        assert_eq!(
            texts(&lines[1]),
            [
                ("sel", "c".to_string()),
                ("caret", String::new()),
                ("text", "d".to_string()),
            ]
        );
    }

    #[test]
    fn a_backward_selection_draws_the_caret_at_its_head() {
        // anchor after "d", head after "a": same highlight, caret left
        let lines = layout("ab\ncd", 4, 1, None, Shape::Bar);
        assert_eq!(
            texts(&lines[0]),
            [
                ("text", "a".to_string()),
                ("caret", String::new()),
                ("sel", "b".to_string()),
            ]
        );
        assert_eq!(
            texts(&lines[1]),
            [("sel", "c".to_string()), ("text", "d".to_string())]
        );
    }

    #[test]
    fn the_box_caret_wears_the_next_cluster() {
        let lines = layout("été", 2, 2, None, Shape::Box);
        assert_eq!(
            texts(&lines[0]),
            [
                ("text", "é".to_string()),
                ("box", "t".to_string()),
                ("text", "é".to_string()),
            ]
        );
    }

    #[test]
    fn the_box_caret_at_a_line_end_is_a_stand_in_space() {
        for (source, head) in [("ab\ncd", 2), ("ab", 2), ("", 0)] {
            let lines = layout(source, head, head, None, Shape::Box);
            let boxed = lines[0].pieces.iter().find_map(|piece| match piece {
                Piece::CaretBox { cluster, .. } => Some(cluster.clone()),
                _ => None,
            });
            assert_eq!(
                boxed,
                Some("\u{a0}".to_string()),
                "{source:?} at {head}"
            );
        }
    }

    #[test]
    fn a_composition_previews_at_the_caret() {
        let lines = layout("ab", 1, 1, Some("^"), Shape::Bar);
        assert_eq!(
            texts(&lines[0]),
            [
                ("text", "a".to_string()),
                ("preview", "^".to_string()),
                ("caret", String::new()),
                ("text", "b".to_string()),
            ]
        );
    }

    #[test]
    fn a_composition_suspends_the_box_caret() {
        // composing happens in insert mode; a box would fight the preview
        let lines = layout("ab", 1, 1, Some("¨"), Shape::Box);
        assert_eq!(
            texts(&lines[0]),
            [
                ("text", "a".to_string()),
                ("preview", "¨".to_string()),
                ("caret", String::new()),
                ("text", "b".to_string()),
            ]
        );
    }

    #[test]
    fn stale_offsets_clamp_instead_of_panicking() {
        let lines = layout("été", 99, 1, None, Shape::Bar);
        // 99 clamps to the end, 1 floors to é's start: "é" is selected
        assert_eq!(
            texts(&lines[0]),
            [("caret", String::new()), ("sel", "été".to_string()),]
        );
    }

    #[test]
    fn every_piece_knows_how_it_is_drawn() {
        let cases = [
            (
                Piece::Text {
                    start: 0,
                    text: "un".into(),
                },
                Some(("", 0, "un")),
            ),
            (
                Piece::Selected {
                    start: 2,
                    text: "deux".into(),
                },
                Some(("sel", 2, "deux")),
            ),
            (Piece::Caret, None),
            (
                Piece::CaretBox {
                    start: 4,
                    cluster: "é".into(),
                },
                Some(("caret-box", 4, "é")),
            ),
            (
                Piece::Preview {
                    start: 6,
                    text: "^".into(),
                },
                Some(("compose", 6, "^")),
            ),
        ];
        for (piece, expected) in cases {
            assert_eq!(piece.drawn(), expected, "{piece:?}");
        }
    }

    #[test]
    fn piece_starts_are_block_relative_bytes() {
        let lines = layout("un\ndeux", 4, 6, None, Shape::Bar);
        let starts: Vec<usize> = lines
            .iter()
            .flat_map(|line| &line.pieces)
            .filter_map(|piece| match piece {
                Piece::Text { start, .. } | Piece::Selected { start, .. } => {
                    Some(*start)
                }
                _ => None,
            })
            .collect();
        assert_eq!(starts, [0, 3, 4, 6]);
    }
}
