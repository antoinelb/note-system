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
            // a well-formed block's content never ends with '\n' — the
            // separator carries it — but a malformed parse (an unterminated
            // raw fence, say) could still hand one back; strip it so the
            // split below never manufactures a phantom trailing row
            let slice = slice.strip_suffix('\n').unwrap_or(slice);
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

    /// The row index holding `offset` — the operators' linewise arithmetic.
    pub fn row_of(&self, offset: usize) -> usize {
        self.index_of(offset)
    }

    /// The row's range, clamped onto the table.
    pub fn row(&self, index: usize) -> Range<usize> {
        self.get(index)
    }

    /// How many visible lines the note has.
    pub fn rows(&self) -> usize {
        self.len()
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

/// A linewise span over rows `first..=last`: whole lines, each trailing
/// newline included only when it is block content — the newline after a
/// block's final line is the separator's, and deleting up to it leaves the
/// bare separator to merge at the next resegmentation
/// (adr/2026-08-editor-splice-cross-block.md).
pub fn linewise_span(
    lines: &Lines,
    first: usize,
    last: usize,
) -> Range<usize> {
    let start = lines.row(first).start;
    let ending = lines.row(last);
    let next = last + 1;
    let end = if next < lines.rows() && lines.row(next).start == ending.end + 1
    {
        ending.end + 1
    } else {
        ending.end
    };
    start..end
}

/// One text object — what an operator's noun names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// iw aw: the run of same-class clusters around the caret.
    Word,
    /// i" a" and the sibling quotes: a same-character pair on the line.
    Quote(char),
    /// i( a) and the sibling pairs: a nested pair inside the block.
    Pair(char, char),
    /// ip ap, the house meaning: the paragraph *is* the block, so the
    /// object comes from the block map, not a scan
    /// (adr/2026-08-one-register-the-clipboard.md).
    Block,
}

/// The span a text object names around `at`, or `None` when nothing does.
/// `around` widens: the word takes its trailing (else leading) blanks, the
/// quotes and pairs take their delimiters, the block takes its separator.
pub fn object(
    text: &str,
    blocks: &[Block],
    at: usize,
    kind: ObjectKind,
    around: bool,
) -> Option<Range<usize>> {
    let block = blocks.get(crate::blocks::block_at(blocks, at))?;
    let content = block.content();
    match kind {
        ObjectKind::Word => {
            word_object(text, &Lines::of(text, blocks).around(at), at, around)
        }
        ObjectKind::Quote(quote) => quote_object(
            text,
            &Lines::of(text, blocks).around(at),
            at,
            quote,
            around,
        ),
        ObjectKind::Pair(open, close) => {
            pair_object(text, &content, at, open, close, around)
        }
        ObjectKind::Block => {
            Some(if around { block.range.clone() } else { content })
        }
    }
}

/// The two delimiters of the innermost pair around `at` — what cs, ds
/// and the surround keys splice, as opposed to the content `object`
/// names. Quote kinds pair left to right along the line (Typst's * and _
/// ride this path), bracket kinds nest within the block.
pub fn surround_spans(
    text: &str,
    blocks: &[Block],
    at: usize,
    kind: ObjectKind,
) -> Option<(Range<usize>, Range<usize>)> {
    match kind {
        ObjectKind::Quote(quote) => {
            let line = Lines::of(text, blocks).around(at);
            let slice = text.get(line.clone()).unwrap_or_default();
            let rel = at.saturating_sub(line.start).min(slice.len());
            let (open, close) = quote_delimiters(slice, rel, quote)?;
            Some((
                line.start + open..line.start + open + quote.len_utf8(),
                line.start + close..line.start + close + quote.len_utf8(),
            ))
        }
        ObjectKind::Pair(open_ch, close_ch) => {
            let block = blocks.get(crate::blocks::block_at(blocks, at))?;
            let content = block.content();
            let slice = text.get(content.clone()).unwrap_or_default();
            let rel = at.saturating_sub(content.start).min(slice.len());
            let (open, close) =
                bracket_delimiters(slice, rel, open_ch, close_ch)?;
            Some((
                content.start + open
                    ..content.start + open + open_ch.len_utf8(),
                content.start + close
                    ..content.start + close + close_ch.len_utf8(),
            ))
        }
        ObjectKind::Word | ObjectKind::Block => None,
    }
}

/// iw / aw: the same-class run under the caret, widened by its trailing
/// (else leading) blanks for `around`. Line-scoped, as vim's word is.
fn word_object(
    text: &str,
    line: &Range<usize>,
    at: usize,
    around: bool,
) -> Option<Range<usize>> {
    let slice = text.get(line.clone()).unwrap_or_default();
    if slice.is_empty() {
        return None;
    }
    let rel = at.saturating_sub(line.start).min(slice.len());
    let anchor = if rel >= slice.len() {
        caret::prev_cluster(slice, slice.len())
    } else {
        rel
    };
    // the slice is non-empty and the anchor rests on a cluster, so a
    // char is always there
    let wanted = slice[anchor..]
        .chars()
        .next()
        .map(class)
        .unwrap_or(Class::Blank);
    let start = slice[..anchor]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| class(*ch) == wanted)
        .last()
        .map(|(offset, _)| offset)
        .unwrap_or(anchor);
    let end = slice[anchor..]
        .char_indices()
        .find(|(_, ch)| class(*ch) != wanted)
        .map(|(offset, _)| anchor + offset)
        .unwrap_or(slice.len());
    let mut span = start..end;
    if around && wanted != Class::Blank {
        let trailed = slice[end..]
            .char_indices()
            .find(|(_, ch)| class(*ch) != Class::Blank)
            .map(|(offset, _)| end + offset)
            .unwrap_or(slice.len());
        if trailed > end {
            span.end = trailed;
        } else {
            let led = slice[..start]
                .char_indices()
                .rev()
                .take_while(|(_, ch)| class(*ch) == Class::Blank)
                .last()
                .map(|(offset, _)| offset)
                .unwrap_or(start);
            span.start = led;
        }
    }
    Some(line.start + span.start..line.start + span.end)
}

/// i" / a": the quoted span the caret stands in or before, paired left to
/// right along the line, as vim pairs them.
fn quote_object(
    text: &str,
    line: &Range<usize>,
    at: usize,
    quote: char,
    around: bool,
) -> Option<Range<usize>> {
    let slice = text.get(line.clone()).unwrap_or_default();
    let rel = at.saturating_sub(line.start).min(slice.len());
    let (open, close) = quote_delimiters(slice, rel, quote)?;
    let span = if around {
        open..close + quote.len_utf8()
    } else {
        open + quote.len_utf8()..close
    };
    Some(line.start + span.start..line.start + span.end)
}

/// The chunked left-to-right quote pair the caret at `rel` stands in or
/// before, as offsets into `slice` — what `quote_object` and
/// `surround_spans` both build their span from.
fn quote_delimiters(
    slice: &str,
    rel: usize,
    quote: char,
) -> Option<(usize, usize)> {
    let marks: Vec<usize> = slice
        .char_indices()
        .filter(|(_, ch)| *ch == quote)
        .map(|(offset, _)| offset)
        .collect();
    marks.chunks(2).find_map(|pair| match pair {
        [open, close] if rel <= *close => Some((*open, *close)),
        _ => None,
    })
}

/// i( / a): the innermost pair around the caret, nesting respected,
/// scanned within the block's content — vim semantics are textual, and a
/// pair straddling blocks is not a real sentence.
fn pair_object(
    text: &str,
    content: &Range<usize>,
    at: usize,
    open: char,
    close: char,
    around: bool,
) -> Option<Range<usize>> {
    let slice = text.get(content.clone()).unwrap_or_default();
    let rel = at.saturating_sub(content.start).min(slice.len());
    let (opened, closed) = bracket_delimiters(slice, rel, open, close)?;
    let span = if around {
        opened..closed + close.len_utf8()
    } else {
        opened + open.len_utf8()..closed
    };
    Some(content.start + span.start..content.start + span.end)
}

/// The innermost nesting-aware bracket pair around `rel`, as offsets into
/// `slice` — what `pair_object` and `surround_spans` both build their span
/// from.
fn bracket_delimiters(
    slice: &str,
    rel: usize,
    open: char,
    close: char,
) -> Option<(usize, usize)> {
    let opened = open_before(slice, rel, open, close)?;
    let closed = close_after(slice, rel, open, close)?;
    Some((opened, closed))
}

/// The unmatched opener at or before `rel`, depth counted right to left.
fn open_before(
    slice: &str,
    rel: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0i32;
    let scan = slice.get(..rel.min(slice.len())).unwrap_or_default();
    if slice.get(rel..).unwrap_or_default().starts_with(open) {
        return Some(rel);
    }
    for (offset, ch) in scan.char_indices().rev() {
        if ch == close {
            depth += 1;
        } else if ch == open {
            if depth == 0 {
                return Some(offset);
            }
            depth -= 1;
        }
    }
    None
}

/// The matching closer after `rel`, depth counted left to right.
fn close_after(
    slice: &str,
    rel: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0i32;
    let from = rel.min(slice.len());
    for (offset, ch) in slice.get(from..).unwrap_or_default().char_indices() {
        if ch == open && offset > 0 {
            depth += 1;
        } else if ch == close {
            if depth == 0 {
                return Some(from + offset);
            }
            depth -= 1;
        }
    }
    None
}

/// What p and P splice once the executor has read the clipboard: the
/// insertion span, its text and the caret landing — pure, so the paste
/// grammar tests headlessly. A clip ending in a newline is linewise (vim's
/// register kind cannot ride through the OS, so the trailing newline is
/// the heuristic — adr/2026-08-one-register-the-clipboard.md).
pub fn paste_spec(
    text: &str,
    blocks: &[Block],
    head: usize,
    clip: &str,
    before: bool,
    count: usize,
) -> (Range<usize>, String, usize) {
    let lines = Lines::of(text, blocks);
    let line = lines.around(head);
    let body = clip.repeat(count.max(1));
    if clip.ends_with('\n') {
        if before {
            let caret = line.start + blank_prefix(&body);
            return (line.start..line.start, body, caret);
        }
        let row = lines.row_of(head);
        let next = row + 1;
        if next < lines.rows() && lines.row(next).start == line.end + 1 {
            let at = line.end + 1;
            let caret = at + blank_prefix(&body);
            (at..at, body, caret)
        } else {
            // the line's newline is the separator's (or the note's end):
            // open the line with the break the body carried
            let opened =
                format!("\n{}", body.strip_suffix('\n').unwrap_or(&body));
            let caret = line.end + 1 + blank_prefix(&opened[1..]);
            (line.end..line.end, opened, caret)
        }
    } else {
        let at = if before {
            head
        } else {
            caret::next_cluster(text, head).min(line.end)
        };
        // vim lands on the last pasted cluster
        let caret = at + caret::prev_cluster(&body, body.len());
        (at..at, body, caret)
    }
}

/// How far into its first line the pasted body's first non-blank sits —
/// the linewise paste's caret landing, and the linewise delete's.
pub fn blank_prefix(body: &str) -> usize {
    let first = body.split('\n').next().unwrap_or_default();
    first
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| offset)
        .unwrap_or(0)
}

/// Where / lands and n and N walk: the pattern's occurrences over the
/// visible lines — a match cannot straddle blocks or hide in a separator —
/// with wrap-around, smartcase (an all-lowercase pattern matches any case,
/// a capital anywhere makes it exact), and accents exact
/// (adr/2026-08-search-lands-through-place.md).
pub fn search(
    text: &str,
    lines: &Lines,
    from: usize,
    pattern: &str,
    forward: bool,
) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }
    let smart = pattern.chars().all(|ch| !ch.is_uppercase());
    let hits: Vec<usize> = (0..lines.rows())
        .flat_map(|row| {
            let line = lines.row(row);
            let slice = text.get(line.clone()).unwrap_or_default();
            slice
                .char_indices()
                .filter(|(offset, _)| {
                    matches_at(slice, *offset, pattern, smart)
                })
                .map(|(offset, _)| line.start + offset)
                .collect::<Vec<usize>>()
        })
        .collect();
    if forward {
        hits.iter()
            .find(|&&hit| hit > from)
            .or_else(|| hits.first())
            .copied()
    } else {
        hits.iter()
            .rev()
            .find(|&&hit| hit < from)
            .or_else(|| hits.last())
            .copied()
    }
}

/// Whether the pattern matches at this offset, char by char — folding the
/// candidate's case when the pattern asked for smartcase.
fn matches_at(slice: &str, offset: usize, pattern: &str, smart: bool) -> bool {
    let candidate = slice.get(offset..).unwrap_or_default();
    let mut wanted = pattern.chars();
    let mut have = candidate.chars();
    for expected in wanted.by_ref() {
        let Some(found) = have.next() else {
            return false;
        };
        let matched = if smart {
            found.to_lowercase().eq(expected.to_lowercase())
        } else {
            found == expected
        };
        if !matched {
            return false;
        }
    }
    true
}

/// What , repeats: the last find, the other way — public because the
/// operator grammar normalizes , into the find it reverses.
pub fn reverse(kind: FindKind) -> FindKind {
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
    fn the_line_table_includes_blank_lines_as_real_lines() {
        // per-line blocks make a blank line a real, navigable row —
        // adr/2026-08-per-line-block-segmentation.md
        let lines = table(NOTE);
        // "= l'été" | "" | "- une idée" | "- deux cafés" | "" | "La pluie…" | ""
        assert_eq!(lines.len(), 7, "{lines:?}");
        assert_eq!(lines.get(0), 0..9);
        assert_eq!(lines.get(1).len(), 0, "between the heading and the list");
        assert!(NOTE[lines.get(2)].starts_with("- une"));
        assert!(NOTE[lines.get(3)].starts_with("- deux"));
        assert_eq!(lines.get(4).len(), 0, "between the list and the prose");
        assert!(NOTE[lines.get(5)].starts_with("La pluie"));
        assert_eq!(lines.get(6).len(), 0, "the final empty line is real");
        // an offset inside a separator answers the line it trails
        assert_eq!(lines.index_of(9), 0);
    }

    #[test]
    fn a_block_whose_content_ends_with_a_newline_gets_no_phantom_row() {
        // defensive: a well-formed block's content never ends with '\n',
        // but Lines::of must not manufacture an extra empty row from the
        // trailing split if one ever did (adr/2026-08-per-line-block-segmentation.md)
        let text = "abc\n";
        let blocks = [Block {
            range: 0..4,
            content_end: 4,
            standalone: false,
        }];
        let lines = Lines::of(text, &blocks);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines.get(0), 0..3);
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
    fn j_falls_through_short_lines_and_the_blank_row_without_forgetting_the_column()
     {
        // from column 10 of "- deux cafés", k to the shorter "- une idée"
        // clamps, k again lands on the blank row between the list and the
        // heading — a real, navigable line now — and k once more reaches
        // the heading, still clamped, the goal column held throughout
        let lines = table(NOTE);
        let start = lines.get(3).start + 10; // on the é of cafés
        let (first, goal) =
            motion(NOTE, &lines, start, Motion::Up, 1, None, None)
                .expect("moves");
        assert_eq!(goal, Some(10));
        assert_eq!(
            first,
            lines.get(2).start + 10,
            "clamped onto idée's final e"
        );
        let (second, goal) =
            motion(NOTE, &lines, first, Motion::Up, 1, goal, None)
                .expect("moves");
        assert_eq!(goal, Some(10));
        assert_eq!(second, lines.get(1).start, "the blank row, goal held");
        let (third, goal) =
            motion(NOTE, &lines, second, Motion::Up, 1, goal, None)
                .expect("moves");
        assert_eq!(goal, Some(10));
        assert_eq!(third, 7, "the heading's last cluster, still goal 10");
    }

    #[test]
    fn j_crosses_blocks_and_counts_clamp_at_the_ends() {
        let lines = table(NOTE);
        // j from the heading lands on the blank row after it: the slide is
        // just the caret crossing
        let (down, _) = motion(NOTE, &lines, 0, Motion::Down, 1, None, None)
            .expect("moves");
        assert_eq!(down, lines.get(1).start);
        // a count beyond the note clamps to the last line
        let (bottom, _) =
            motion(NOTE, &lines, 0, Motion::Down, 99, None, None)
                .expect("moves");
        assert_eq!(bottom, lines.get(lines.rows() - 1).start);
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
        let mid = lines.get(5).start + 5;
        let (zero, _) = go(NOTE, mid, Motion::LineStart, 1).expect("m");
        assert_eq!(zero, lines.get(5).start);
        let (dollar, _) = go(NOTE, mid, Motion::LineEnd, 1).expect("m");
        assert_eq!(&NOTE[dollar..dollar + 1], ".", "on the last cluster");
        // 2$ runs a line down first — from the list's first line, its
        // second line's end
        let list_mid = lines.get(2).start;
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
        assert_eq!(
            bottom,
            lines.get(lines.rows() - 1).start,
            "the real empty last line"
        );
        // [count]gg and [count]G go to line N, clamped
        let (second, _) = go(NOTE, 0, Motion::FirstLine, 2).expect("m");
        assert_eq!(second, lines.get(1).start);
        let (third, _) = go(NOTE, 0, Motion::LastLine, 3).expect("m");
        assert_eq!(third, lines.get(2).start);
        let (past, _) = go(NOTE, 0, Motion::FirstLine, 99).expect("m");
        assert_eq!(
            past,
            lines.get(lines.rows() - 1).start,
            "past the end clamps"
        );
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
    fn word_objects_take_runs_blanks_and_edges() {
        let text = "un  mot final\n";
        let parsed = blocks::segment(text);
        // iw on the blank run is the blanks themselves
        assert_eq!(
            object(text, &parsed, 2, ObjectKind::Word, false),
            Some(2..4),
        );
        // aw on the last word has no trailing blanks: the leading ones come
        assert_eq!(
            object(text, &parsed, 8, ObjectKind::Word, true),
            Some(7..13),
        );
        // a caret resting past the line's last cluster still names it
        assert_eq!(
            object(text, &parsed, 13, ObjectKind::Word, false),
            Some(8..13),
        );
        // an empty line names nothing
        assert_eq!(object(text, &parsed, 14, ObjectKind::Word, false), None);
    }

    #[test]
    fn pair_objects_work_from_their_delimiters_and_respect_nesting() {
        let text = "a (b (c) d) e\n";
        let parsed = blocks::segment(text);
        let pair = ObjectKind::Pair('(', ')');
        // from inside the inner pair, the inner pair answers
        assert_eq!(object(text, &parsed, 6, pair, false), Some(6..7));
        // from between the pairs, the outer one answers
        assert_eq!(object(text, &parsed, 9, pair, true), Some(2..11));
        // standing on the opener names its own pair
        assert_eq!(object(text, &parsed, 5, pair, false), Some(6..7));
        // standing on the closer too
        assert_eq!(object(text, &parsed, 7, pair, false), Some(6..7));
        // from before the inner pair, the closer walk nests through it
        assert_eq!(object(text, &parsed, 3, pair, false), Some(3..10));
        // an unclosed pair names nothing
        let broken = "a (b\n";
        let parsed = blocks::segment(broken);
        assert_eq!(object(broken, &parsed, 3, pair, false), None);
        let unopened = "a b) c\n";
        let parsed = blocks::segment(unopened);
        assert_eq!(object(unopened, &parsed, 1, pair, false), None);
    }

    #[test]
    fn block_objects_read_the_map_not_a_scan() {
        // ip/ap now name a line, not a paragraph
        // (adr/2026-08-per-line-block-segmentation.md): ip on "- deux
        // cafés" is that one line, ap rides its own separator
        let parsed = blocks::segment(NOTE);
        assert_eq!(
            object(NOTE, &parsed, 25, ObjectKind::Block, false),
            Some(23..36),
        );
        assert_eq!(
            object(NOTE, &parsed, 25, ObjectKind::Block, true),
            Some(23..37),
        );
        // the prose line's around still runs to the note's end here, since
        // it directly precedes the trailing empty line
        assert_eq!(
            object(NOTE, &parsed, 40, ObjectKind::Block, true),
            Some(38..NOTE.len()),
        );
    }

    #[test]
    fn objects_over_nothing_answer_none_and_other_quotes_answer() {
        assert_eq!(object("", &[], 0, ObjectKind::Word, false), None);
        let text = "l'idée et `du code`\n";
        let parsed = blocks::segment(text);
        assert_eq!(
            object(text, &parsed, 3, ObjectKind::Quote('\''), false),
            None,
            "a lone apostrophe pairs with nothing"
        );
        assert_eq!(
            object(text, &parsed, 12, ObjectKind::Quote('`'), false),
            Some(12..19),
        );
    }

    #[test]
    fn surround_spans_finds_the_innermost_bracket_nesting() {
        let text = "a (b (c) d) e\n";
        let parsed = blocks::segment(text);
        let pair = ObjectKind::Pair('(', ')');
        let (open, close) =
            surround_spans(text, &parsed, 6, pair).expect("found");
        assert_eq!(open, 5..6, "the inner opener");
        assert_eq!(close, 7..8, "the inner closer");
    }

    #[test]
    fn surround_spans_answers_from_the_delimiter_itself() {
        let text = "a (b (c) d) e\n";
        let parsed = blocks::segment(text);
        let pair = ObjectKind::Pair('(', ')');
        let (open, close) =
            surround_spans(text, &parsed, 5, pair).expect("on the opener");
        assert_eq!((open, close), (5..6, 7..8));
        let (open, close) =
            surround_spans(text, &parsed, 7, pair).expect("on the closer");
        assert_eq!((open, close), (5..6, 7..8));
    }

    #[test]
    fn surround_spans_pairs_quotes_and_stars_left_to_right() {
        let text = "l'idée et \"du *code*\"\n";
        let parsed = blocks::segment(text);
        let open_quote = text.find('"').expect("an opening quote");
        let close_quote = text.rfind('"').expect("a closing quote");
        let open_star = text.find('*').expect("an opening star");
        let close_star = text.rfind('*').expect("a closing star");
        let inside_quotes = open_quote + 2; // "d[u]…" — inside the quotes
        let (open, close) = surround_spans(
            text,
            &parsed,
            inside_quotes,
            ObjectKind::Quote('"'),
        )
        .expect("found");
        assert_eq!(open, open_quote..open_quote + 1);
        assert_eq!(close, close_quote..close_quote + 1);
        let (open, close) =
            surround_spans(text, &parsed, open_star, ObjectKind::Quote('*'))
                .expect("the emphasis pair, standing on its opener");
        assert_eq!(open, open_star..open_star + 1);
        assert_eq!(close, close_star..close_star + 1);
    }

    #[test]
    fn surround_spans_answers_the_pair_ahead_when_two_stand_on_the_line() {
        // two pairs, the caret between them: the chunked left-to-right
        // pairing must answer the second, where a nearest-mark scan would
        // answer the first
        let text = "un \"mot\" et \"deux\"\n";
        let parsed = blocks::segment(text);
        let between = 9; // the e of "et", past the first pair's closer
        let (open, close) =
            surround_spans(text, &parsed, between, ObjectKind::Quote('"'))
                .expect("the pair ahead");
        assert_eq!(open, 12..13, "the second pair's opener");
        assert_eq!(close, 17..18, "and its closer");
    }

    #[test]
    fn surround_spans_quotes_answer_none_off_their_line() {
        let text = "\"un\"\nmot\n";
        let parsed = blocks::segment(text);
        let at = text.find("mot").expect("mot is in the text");
        assert_eq!(
            surround_spans(text, &parsed, at, ObjectKind::Quote('"')),
            None,
        );
    }

    #[test]
    fn surround_spans_answers_none_for_an_unmatched_opener() {
        let text = "a (b\n";
        let parsed = blocks::segment(text);
        assert_eq!(
            surround_spans(text, &parsed, 3, ObjectKind::Pair('(', ')')),
            None,
        );
    }

    #[test]
    fn surround_spans_brackets_do_not_straddle_a_block_boundary() {
        let text = "a (b\n\nc)\n";
        let parsed = blocks::segment(text);
        assert_eq!(parsed.len(), 4, "one block per line, including the blank");
        assert_eq!(
            surround_spans(text, &parsed, 3, ObjectKind::Pair('(', ')')),
            None,
            "the closer lives in the next block, out of reach"
        );
    }

    #[test]
    fn surround_spans_brackets_answer_none_with_no_blocks_at_all() {
        assert_eq!(
            surround_spans("a (b) c", &[], 2, ObjectKind::Pair('(', ')')),
            None,
            "an empty block map names no block to scope into"
        );
    }

    #[test]
    fn surround_spans_answers_none_for_word_and_block_kinds() {
        let text = "un mot\n";
        let parsed = blocks::segment(text);
        assert_eq!(surround_spans(text, &parsed, 3, ObjectKind::Word), None);
        assert_eq!(surround_spans(text, &parsed, 3, ObjectKind::Block), None);
    }

    #[test]
    fn search_walks_matches_with_wrap_and_smartcase() {
        let text = "Un café.\n\nEncore un Café noir.\n";
        let lines = table(text);
        // smartcase: a lowercase pattern matches both cafés
        let first = search(text, &lines, 0, "café", true);
        assert_eq!(first, Some(3));
        let second = search(text, &lines, 3, "café", true);
        assert_eq!(second, Some(21));
        // wrap-around forward and back
        assert_eq!(search(text, &lines, 21, "café", true), Some(3));
        assert_eq!(search(text, &lines, 3, "café", false), Some(21));
        // a capital makes it exact
        assert_eq!(search(text, &lines, 0, "Café", true), Some(21));
        // no match, empty pattern: nothing
        assert_eq!(search(text, &lines, 0, "thé", true), None);
        assert_eq!(search(text, &lines, 0, "", true), None);
        // a pattern longer than the line's tail cannot match past it
        assert_eq!(search(text, &lines, 0, "noir.x", true), None);
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
