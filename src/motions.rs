//! The note's motion math, pure over its text and block map: where one vim
//! motion lands, char-wise over French text. Line-scoped motions walk the
//! visible-line table — block separators are bytes no caret may rest on —
//! while word motions scan the raw text, whose separators are whitespace
//! they skip anyway (adr/2026-08-motions-on-visible-lines.md).

use std::ops::Range;

use crate::blocks::Block;
use crate::caret;

/// The note's visible lines in order: every block content's lines as
/// note-global byte ranges. Separator bytes belong to no line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lines(Vec<Range<usize>>);

impl Lines {
    pub fn of(text: &str, blocks: &[Block]) -> Lines {
        let mut lines = Vec::new();
        for block in blocks {
            let content = block.content();
            let slice = text.get(content.clone()).unwrap_or("");
            let mut start = content.start;
            for part in slice.split('\n') {
                lines.push(start..start + part.len());
                start += part.len() + 1;
            }
        }
        if lines.is_empty() {
            lines.push(0..0);
        }
        Lines(lines)
    }

    /// The visible line holding `offset` — what the insert entries and the
    /// escape step measure themselves against.
    pub fn around(&self, offset: usize) -> Range<usize> {
        self.get(self.index_of(offset))
    }

    /// The index of the line holding `offset` — the last line starting at
    /// or before it, so an offset inside a separator answers the line the
    /// separator trails.
    fn index_of(&self, offset: usize) -> usize {
        self.0
            .iter()
            .rposition(|line| line.start <= offset)
            .unwrap_or(0)
    }

    fn get(&self, index: usize) -> Range<usize> {
        self.0
            .get(index.min(self.0.len().saturating_sub(1)))
            .cloned()
            .unwrap_or(0..0)
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// One normal-mode motion. Counts are the caller's; `Find` carries the
/// character the prefix collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Down,
    Up,
    WordForward,
    WordBack,
    WordEnd,
    LineStart,
    FirstNonBlank,
    LineEnd,
    Find(FindKind, char),
    RepeatFind,
    RepeatFindBack,
    FirstLine,
    LastLine,
}

/// The four find flavours: f F t T.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindKind {
    ForwardOn,
    BackwardOn,
    ForwardBefore,
    BackwardBefore,
}

/// Where `motion` lands from `at`, and the goal column a vertical run must
/// remember (`Some` only for Down and Up). `None` is a failed motion — an
/// f with no target on the line — and the caret stays. The landing follows
/// vim's normal-mode convention: the caret rests on a cluster, so a line's
/// last place is its final cluster's start.
pub fn motion(
    text: &str,
    lines: &Lines,
    at: usize,
    motion: Motion,
    count: usize,
    goal: Option<usize>,
    last_find: Option<(FindKind, char)>,
) -> Option<(usize, Option<usize>)> {
    let count = count.max(1);
    let row = lines.index_of(at);
    let line = lines.get(row);
    match motion {
        Motion::Left => {
            let target = (0..count).fold(at, |from, _| {
                caret::prev_cluster(text, from).max(line.start)
            });
            Some((target, None))
        }
        Motion::Right => {
            let stop = last_cluster(text, &line);
            let target = (0..count)
                .fold(at, |from, _| caret::next_cluster(text, from).min(stop));
            Some((target, None))
        }
        Motion::Down => {
            Some(vertical(text, lines, row, at, count, goal, false))
        }
        Motion::Up => Some(vertical(text, lines, row, at, count, goal, true)),
        Motion::WordForward => {
            let target =
                (0..count).fold(at, |from, _| word_forward(text, from));
            Some((settle(text, lines, target), None))
        }
        Motion::WordBack => {
            let target = (0..count).fold(at, |from, _| word_back(text, from));
            Some((target, None))
        }
        Motion::WordEnd => {
            let target = (0..count).fold(at, |from, _| word_end(text, from));
            Some((target, None))
        }
        Motion::LineStart => Some((line.start, None)),
        Motion::FirstNonBlank => Some((first_non_blank(text, &line), None)),
        Motion::LineEnd => {
            // a count runs count-1 lines down first, as vim's $ does
            let target = lines.get((row + count - 1).min(lines.len() - 1));
            Some((last_cluster(text, &target), None))
        }
        Motion::Find(kind, wanted) => {
            find(text, &line, at, kind, wanted, count).map(|hit| (hit, None))
        }
        Motion::RepeatFind => {
            let (kind, wanted) = last_find?;
            find(text, &line, at, kind, wanted, count).map(|hit| (hit, None))
        }
        Motion::RepeatFindBack => {
            let (kind, wanted) = last_find?;
            find(text, &line, at, reverse(kind), wanted, count)
                .map(|hit| (hit, None))
        }
        Motion::FirstLine => {
            // a count goes to line N, vim's [count]gg
            let target = lines.get(count.saturating_sub(1));
            Some((first_non_blank(text, &target), None))
        }
        Motion::LastLine => {
            let target = if count > 1 {
                lines.get(count - 1)
            } else {
                lines.get(lines.len() - 1)
            };
            Some((first_non_blank(text, &target), None))
        }
    }
}

/// j and k: walk the line table keeping the goal column in clusters —
/// clamped to each line's last cluster, remembered through short lines.
fn vertical(
    text: &str,
    lines: &Lines,
    row: usize,
    at: usize,
    count: usize,
    goal: Option<usize>,
    up: bool,
) -> (usize, Option<usize>) {
    let line = lines.get(row);
    let column = goal.unwrap_or_else(|| {
        text.get(line.start..at)
            .map(|before| {
                unicode_segmentation::UnicodeSegmentation::graphemes(
                    before, true,
                )
                .count()
            })
            .unwrap_or(0)
    });
    let target_row = if up {
        row.saturating_sub(count)
    } else {
        (row + count).min(lines.len() - 1)
    };
    let target = lines.get(target_row);
    (at_column(text, &target, column), Some(column))
}

/// The cluster-start `column` clusters into the line, clamped onto its
/// last cluster — the vim landing for a goal column.
fn at_column(text: &str, line: &Range<usize>, column: usize) -> usize {
    let slice = text.get(line.clone()).unwrap_or("");
    let landed = unicode_segmentation::UnicodeSegmentation::grapheme_indices(
        slice, true,
    )
    .nth(column)
    .map(|(offset, _)| line.start + offset);
    landed.unwrap_or_else(|| last_cluster(text, line))
}

/// Where a caret may rest deepest on this line: the start of its final
/// cluster, or the line's start when it is empty.
fn last_cluster(text: &str, line: &Range<usize>) -> usize {
    if line.is_empty() {
        line.start
    } else {
        caret::prev_cluster(text, line.end).max(line.start)
    }
}

/// The line's first non-blank cluster, or its start when all blank —
/// public because `I` measures itself with it too.
pub fn first_non_blank(text: &str, line: &Range<usize>) -> usize {
    text.get(line.clone())
        .and_then(|slice| {
            slice
                .char_indices()
                .find(|(_, ch)| !ch.is_whitespace())
                .map(|(offset, _)| line.start + offset)
        })
        .unwrap_or(line.start)
}

/// A landing that may have run past the last visible cluster (w at the
/// note's end): settle back onto the final line's resting place.
fn settle(text: &str, lines: &Lines, at: usize) -> usize {
    let last = lines.get(lines.len() - 1);
    if last.is_empty() {
        at.min(last.start)
    } else {
        at.min(last_cluster(text, &last))
    }
}

/// vim's three character classes: a word runs over alphanumerics (French
/// letters included) and underscores, punctuation runs are their own
/// words, blanks separate — and the hidden block separators are blanks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Word,
    Punct,
    Blank,
}

fn class(ch: char) -> Class {
    if ch.is_whitespace() {
        Class::Blank
    } else if ch.is_alphanumeric() || ch == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

/// w: the start of the next word — past the current run, over the blanks.
fn word_forward(text: &str, at: usize) -> usize {
    let Some(first) = text.get(at..).and_then(|rest| rest.chars().next())
    else {
        return at;
    };
    let start_class = class(first);
    let run_end = text
        .get(at..)
        .and_then(|rest| {
            rest.char_indices()
                .find(|(_, ch)| class(*ch) != start_class)
                .map(|(offset, _)| at + offset)
        })
        .unwrap_or(text.len());
    if start_class == Class::Blank {
        return run_end;
    }
    text.get(run_end..)
        .and_then(|rest| {
            rest.char_indices()
                .find(|(_, ch)| class(*ch) != Class::Blank)
                .map(|(offset, _)| run_end + offset)
        })
        .unwrap_or(text.len())
}

/// b: the start of the current word, or of the one before when already on
/// a start — one cluster back, over the blanks, then to the run's head.
fn word_back(text: &str, at: usize) -> usize {
    let from = caret::prev_cluster(text, at);
    let scan = text.get(..caret::next_cluster(text, from)).unwrap_or("");
    scan.char_indices()
        .rev()
        .scan(None, |run, (offset, ch)| {
            let current = class(ch);
            match run {
                None if current == Class::Blank => Some(None),
                None => {
                    *run = Some(current);
                    Some(Some(offset))
                }
                Some(class) if *class == current => Some(Some(offset)),
                Some(_) => None,
            }
        })
        .flatten()
        .last()
        .unwrap_or(0)
}

/// e: the last cluster of the current or next word — one cluster forward,
/// over the blanks, then to the run's tail.
fn word_end(text: &str, at: usize) -> usize {
    let from = caret::next_cluster(text, at);
    let start = text
        .get(from..)
        .and_then(|rest| {
            rest.char_indices()
                .find(|(_, ch)| class(*ch) != Class::Blank)
                .map(|(offset, _)| from + offset)
        })
        .unwrap_or(text.len());
    let Some(first) = text.get(start..).and_then(|rest| rest.chars().next())
    else {
        // nothing ahead: the motion fails in place, as vim's e does
        return at;
    };
    let run_class = class(first);
    let run_end = text
        .get(start..)
        .and_then(|rest| {
            rest.char_indices()
                .find(|(_, ch)| class(*ch) != run_class)
                .map(|(offset, _)| start + offset)
        })
        .unwrap_or(text.len());
    caret::prev_cluster(text, run_end)
}

/// f F t T: the count'th occurrence on the caret's line, landed on or
/// beside; `None` when the line has no such target and the caret stays.
fn find(
    text: &str,
    line: &Range<usize>,
    at: usize,
    kind: FindKind,
    wanted: char,
    count: usize,
) -> Option<usize> {
    // the line comes from the table over this same text, and the caret
    // offsets are cluster boundaries, so the slices always answer
    let slice = text.get(line.clone()).unwrap_or_default();
    let forward =
        matches!(kind, FindKind::ForwardOn | FindKind::ForwardBefore);
    let hit = if forward {
        let from =
            (caret::next_cluster(text, at) - line.start).min(slice.len());
        slice
            .get(from..)
            .unwrap_or_default()
            .char_indices()
            .filter(|(_, ch)| *ch == wanted)
            .nth(count - 1)
            .map(|(offset, _)| line.start + from + offset)
    } else {
        let until = at.saturating_sub(line.start).min(slice.len());
        slice
            .get(..until)
            .unwrap_or_default()
            .char_indices()
            .rev()
            .filter(|(_, ch)| *ch == wanted)
            .nth(count - 1)
            .map(|(offset, _)| line.start + offset)
    }?;
    match kind {
        FindKind::ForwardOn | FindKind::BackwardOn => Some(hit),
        FindKind::ForwardBefore => Some(caret::prev_cluster(text, hit)),
        FindKind::BackwardBefore => Some(caret::next_cluster(text, hit)),
    }
}

/// What , repeats: the last find, the other way.
fn reverse(kind: FindKind) -> FindKind {
    match kind {
        FindKind::ForwardOn => FindKind::BackwardOn,
        FindKind::BackwardOn => FindKind::ForwardOn,
        FindKind::ForwardBefore => FindKind::BackwardBefore,
        FindKind::BackwardBefore => FindKind::ForwardBefore,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::blocks;

    /// Three blocks over French text: a heading, a two-line list, prose
    /// with a trailing newline — the last block's empty line is real.
    const NOTE: &str =
        "= l'été\n\n- une idée\n- deux cafés\n\nLa pluie, enfin arrivée.\n";

    fn table(text: &str) -> Lines {
        Lines::of(text, &blocks::segment(text))
    }

    fn go(
        text: &str,
        at: usize,
        m: Motion,
        count: usize,
    ) -> Option<(usize, Option<usize>)> {
        motion(text, &table(text), at, m, count, None, None)
    }

    #[test]
    fn the_line_table_skips_the_separators() {
        let lines = table(NOTE);
        // "= l'été" | "- une idée" | "- deux cafés" | "La pluie…" | ""
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!(lines.get(0), 0..9);
        assert!(NOTE[lines.get(1)].starts_with("- une"));
        assert!(NOTE[lines.get(3)].starts_with("La pluie"));
        assert_eq!(lines.get(4).len(), 0, "the final empty line is real");
        // an offset inside a separator answers the line it trails
        assert_eq!(lines.index_of(9), 0);
    }

    #[test]
    fn an_empty_note_is_one_empty_line() {
        let lines = Lines::of("", &[]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines.get(0), 0..0);
        assert_eq!(lines.index_of(5), 0);
    }

    #[test]
    fn h_and_l_walk_clusters_and_stop_at_the_lines_edges() {
        // "= l'été": = (0) blank l (2) ' ( 3) é(4..6) t(6) é(7..9)
        let (target, _) = go(NOTE, 6, Motion::Left, 1).expect("moves");
        assert_eq!(target, 4, "h steps over é whole");
        let (target, _) = go(NOTE, 6, Motion::Right, 1).expect("moves");
        assert_eq!(target, 7);
        let (target, _) = go(NOTE, 6, Motion::Right, 9).expect("moves");
        assert_eq!(target, 7, "l rests on the last cluster, never past");
        let (target, _) = go(NOTE, 2, Motion::Left, 9).expect("moves");
        assert_eq!(target, 0, "h stops at the line's start");
    }

    #[test]
    fn j_falls_through_short_lines_without_forgetting_the_column() {
        // from column 10 of "- deux cafés" (line 2), k to the shorter
        // "- une idée" clamps, k again to the heading keeps the goal
        let lines = table(NOTE);
        let start = lines.get(2).start + 10; // on the é of cafés
        let (first, goal) =
            motion(NOTE, &lines, start, Motion::Up, 1, None, None)
                .expect("moves");
        assert_eq!(goal, Some(10));
        assert_eq!(
            first,
            lines.get(1).start + 10,
            "clamped onto idée's final e"
        );
        let (second, goal) =
            motion(NOTE, &lines, first, Motion::Up, 1, goal, None)
                .expect("moves");
        assert_eq!(goal, Some(10));
        assert_eq!(second, 7, "the heading's last cluster, still goal 10");
    }

    #[test]
    fn j_crosses_blocks_and_counts_clamp_at_the_ends() {
        let lines = table(NOTE);
        // j from the heading lands in the list block: the slide is just
        // the caret crossing
        let (down, _) = motion(NOTE, &lines, 0, Motion::Down, 1, None, None)
            .expect("moves");
        assert_eq!(down, lines.get(1).start);
        // a count beyond the note clamps to the last line
        let (bottom, _) =
            motion(NOTE, &lines, 0, Motion::Down, 99, None, None)
                .expect("moves");
        assert_eq!(bottom, lines.get(4).start);
        let (top, _) =
            motion(NOTE, &lines, bottom, Motion::Up, 99, None, None)
                .expect("moves");
        assert_eq!(top, 0);
    }

    #[test]
    fn word_motions_cross_punctuation_blanks_and_blocks() {
        // w from the start: "=", "l", "'", "été"… — runs and punctuation
        let (w1, _) = go(NOTE, 0, Motion::WordForward, 1).expect("moves");
        assert_eq!(w1, 2, "past = onto l");
        let (w2, _) = go(NOTE, 2, Motion::WordForward, 1).expect("moves");
        assert_eq!(w2, 3, "the apostrophe is its own word");
        let (w3, _) = go(NOTE, 3, Motion::WordForward, 1).expect("moves");
        assert_eq!(w3, 4, "onto été");
        // w from été crosses the block separator to the list's dash
        let (w4, _) = go(NOTE, 4, Motion::WordForward, 1).expect("moves");
        assert_eq!(w4, 11, "the next block's first cluster");
        // and back
        let (b1, _) = go(NOTE, 11, Motion::WordBack, 1).expect("moves");
        assert_eq!(b1, 4, "b re-crosses the separator onto été's start");
        // e lands on ends
        let (e1, _) = go(NOTE, 0, Motion::WordEnd, 1).expect("moves");
        assert_eq!(e1, 2, "e from = lands on l");
        let (e2, _) = go(NOTE, 4, Motion::WordEnd, 1).expect("moves");
        assert_eq!(&NOTE[e2..e2 + 2], "é", "e lands on été's last cluster");
    }

    #[test]
    fn w_from_a_blank_lands_on_the_next_word() {
        let text = "un  deux";
        let (w, _) = go(text, 2, Motion::WordForward, 1).expect("m");
        assert_eq!(w, 4, "from the blank straight onto deux");
        // and overshooting a note with no trailing newline settles on the
        // last cluster
        let (end, _) = go(text, 4, Motion::WordForward, 9).expect("m");
        assert_eq!(&text[end..end + 1], "x");
    }

    #[test]
    fn the_comma_reverses_every_find_flavour() {
        let text = "un café, un café noir\n";
        let lines = table(text);
        for (kind, from, expected) in [
            (FindKind::ForwardOn, 13, Some(3)),
            (FindKind::BackwardOn, 3, Some(13)),
            (FindKind::ForwardBefore, 13, Some(4)),
            (FindKind::BackwardBefore, 4, Some(12)),
        ] {
            let landed = motion(
                text,
                &lines,
                from,
                Motion::RepeatFindBack,
                1,
                None,
                Some((kind, 'c')),
            )
            .map(|(hit, _)| hit);
            assert_eq!(landed, expected, "{kind:?}");
        }
    }

    #[test]
    fn word_motions_clamp_at_both_ends_of_the_note() {
        // the trailing newline makes the final empty line a real rest
        let (w, _) =
            go(NOTE, NOTE.len() - 2, Motion::WordForward, 9).expect("m");
        assert_eq!(w, NOTE.len(), "w settles on the empty last line");
        let (b, _) = go(NOTE, 0, Motion::WordBack, 9).expect("m");
        assert_eq!(b, 0);
        let (e, _) = go(NOTE, NOTE.len() - 2, Motion::WordEnd, 9).expect("m");
        assert_eq!(&NOTE[e..e + 1], ".", "e fails in place at the end");
    }

    #[test]
    fn line_edges_answer_zero_caret_and_dollar() {
        let lines = table(NOTE);
        let mid = lines.get(3).start + 5;
        let (zero, _) = go(NOTE, mid, Motion::LineStart, 1).expect("m");
        assert_eq!(zero, lines.get(3).start);
        let (dollar, _) = go(NOTE, mid, Motion::LineEnd, 1).expect("m");
        assert_eq!(&NOTE[dollar..dollar + 1], ".", "on the last cluster");
        // 2$ runs a line down first — from the list's first line, its
        // second line's end
        let list_mid = lines.get(1).start;
        let (two, _) = go(NOTE, list_mid, Motion::LineEnd, 2).expect("m");
        assert_eq!(&NOTE[two..two + 1], "s");
    }

    #[test]
    fn caret_finds_the_first_non_blank() {
        let text = "   trois mots\n";
        let (hat, _) = go(text, 12, Motion::FirstNonBlank, 1).expect("m");
        assert_eq!(hat, 3);
        // an all-blank line answers its start
        let blank = "   \nx";
        let lines = table(blank);
        let (hat, _) =
            motion(blank, &lines, 1, Motion::FirstNonBlank, 1, None, None)
                .expect("m");
        assert_eq!(hat, 0);
    }

    #[test]
    fn gg_and_g_land_on_first_and_last_lines() {
        let lines = table(NOTE);
        let (top, _) = go(NOTE, 30, Motion::FirstLine, 1).expect("m");
        assert_eq!(top, 0);
        let (bottom, _) = go(NOTE, 0, Motion::LastLine, 1).expect("m");
        assert_eq!(bottom, lines.get(4).start, "the real empty last line");
        // [count]gg and [count]G go to line N, clamped
        let (second, _) = go(NOTE, 0, Motion::FirstLine, 2).expect("m");
        assert_eq!(second, lines.get(1).start);
        let (third, _) = go(NOTE, 0, Motion::LastLine, 3).expect("m");
        assert_eq!(third, lines.get(2).start);
        let (past, _) = go(NOTE, 0, Motion::FirstLine, 99).expect("m");
        assert_eq!(past, lines.get(4).start, "past the end clamps");
    }

    #[test]
    fn find_lands_on_or_before_and_repeats_both_ways() {
        //          0123456789
        let text = "un café, un café noir\n";
        let lines = table(text);
        let f = |at, kind, ch, count| {
            motion(text, &lines, at, Motion::Find(kind, ch), count, None, None)
                .map(|(hit, _)| hit)
        };
        assert_eq!(f(0, FindKind::ForwardOn, 'c', 1), Some(3));
        assert_eq!(f(0, FindKind::ForwardOn, 'c', 2), Some(13));
        assert_eq!(
            f(0, FindKind::ForwardBefore, 'c', 1),
            Some(2),
            "t stops short"
        );
        assert_eq!(f(12, FindKind::BackwardOn, 'c', 1), Some(3));
        assert_eq!(f(12, FindKind::BackwardBefore, 'c', 1), Some(4));
        assert_eq!(
            f(0, FindKind::ForwardOn, 'z', 1),
            None,
            "no target, no move"
        );
        assert_eq!(f(0, FindKind::ForwardOn, 'c', 9), None, "count too deep");

        // ; repeats, , reverses
        let last = Some((FindKind::ForwardOn, 'c'));
        let again = motion(text, &lines, 3, Motion::RepeatFind, 1, None, last)
            .map(|(hit, _)| hit);
        assert_eq!(again, Some(13));
        let back =
            motion(text, &lines, 12, Motion::RepeatFindBack, 1, None, last)
                .map(|(hit, _)| hit);
        assert_eq!(back, Some(3));
        // with nothing to repeat, ; and , fail quietly
        assert_eq!(
            motion(text, &lines, 0, Motion::RepeatFind, 1, None, None),
            None
        );
        assert_eq!(
            motion(text, &lines, 0, Motion::RepeatFindBack, 1, None, None),
            None
        );
    }

    #[test]
    fn find_works_on_accents_too() {
        let text = "été\n";
        let lines = table(text);
        let hit = motion(
            text,
            &lines,
            0,
            Motion::Find(FindKind::ForwardOn, 'é'),
            1,
            None,
            None,
        );
        assert_eq!(hit.map(|(h, _)| h), Some(3), "the second é, char-wise");
    }
}
