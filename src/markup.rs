//! Turns one block's source into either a styled CSS model or a verdict
//! that Typst must draw it instead. `source` is always a block's *content*
//! slice (`blocks::Block::content()`); every byte range this module hands
//! back is block-relative, on a char boundary, the same coordinate space
//! `caret::Piece::start` uses (see the module doc at the top of
//! `src/caret.rs`).
//!
//! The verdict is read off the `typst-syntax` tree's node kinds alone,
//! never off the source text — an explicit allow-list of markup kinds plus
//! the three calls the app owns (`meta`, `l`, `quote`); anything else falls
//! back to a compiled Typst widget. Once the verdict is CSS, building the
//! spans and a block's structural role *may* read text — that is a
//! rendering decision, not the verdict, and each place it happens says so.

use std::ops::Range;

use typst_syntax::ast;
use typst_syntax::{SyntaxKind, SyntaxNode};

/// The walk's node budget, own to this module (unrelated to
/// `parse::MAX_NODES`): a block is one physical line, so a tree this size
/// is already implausible — exhausting it is treated as a tree too big for
/// CSS to trust, not raised as an error.
const MAX_NODES: usize = 2_000;

/// One inline role a span of markup renders with. `Checkbox` carries
/// whether the box is ticked so the DOM can draw both states from the role
/// alone, with no second text lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Text,
    Strong,
    Emph,
    Raw,
    Link,
    Meta,
    Marker,
    Checkbox { done: bool },
}

/// One byte-range slice of a block's markup, block-relative like every
/// other range in `caret::Piece`. `delimiter` only carries meaning on
/// `Strong`/`Emph`/`Raw`: the run's own `*`/`_`/backtick bytes keep the
/// run's role (so the run's rendered weight — bold, italic, mono — never
/// breaks at the delimiter) but are flagged so a decoration pass can still
/// dim them relative to the run's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub role: Role,
    pub delimiter: bool,
}

/// The structural role of a whole block. `Heading`/`Item` are read off the
/// parse tree; `Quote` is read off the block's own leading text instead,
/// since `>` is not special syntax to Typst — only the *verdict* above is
/// kind-only, a block's structural role is free to look at text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRole {
    Plain,
    Blank,
    Heading(u8),
    Item { indent: usize },
    Quote,
}

/// A block's markup rendered as CSS: its structural role plus the spans
/// that tile its content byte for byte — every byte belongs to exactly one
/// span, in ascending order, no gaps and no overlap, so the rendered DOM
/// text is byte-identical to the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Markup {
    pub block: BlockRole,
    pub spans: Vec<Span>,
}

/// What a block's source asks the editor to draw with: `Css` when its
/// parse tree holds only markup this module knows how to style, `Typst`
/// when it holds anything else and the compiled-SVG fallback must draw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Draw {
    Css(Markup),
    Typst,
}

/// Classifies one block's source and, when CSS can draw it, builds the
/// spans and structural role for it. Parses with `typst_syntax::parse` and
/// walks the tree with an explicit bounded worklist — no recursion, no
/// `while` — copying the idiom of `parse::parse_note`
/// (`src/parse.rs:29-70`).
pub fn model(source: &str) -> Draw {
    let root = typst_syntax::parse(source);
    if !allows_css(&root) {
        return Draw::Typst;
    }
    let block = block_role(source, &root);
    let spans = build_spans(source, &root, block);
    Draw::Css(Markup { block, spans })
}

/// The verdict walk: every node's kind must be on the allow-list below, and
/// every `FuncCall` node's callee must be one of the three calls the app
/// owns — checked on every node the tree holds, not just the top-level
/// children, which is what makes a `#table`/`#figure`/`#image` (or any
/// other unrecognised call) fall back even when it sits nested inside
/// other markup (`src/parse.rs`'s `link_nested_in_other_markup_is_found`
/// test shows nesting like this is real, not a hypothetical). Exhausting
/// `MAX_NODES` before the stack empties answers `false` too: a tree this
/// size is not something CSS should guess at.
fn allows_css(root: &SyntaxNode) -> bool {
    let mut stack = vec![root];
    for _ in 0..MAX_NODES {
        let Some(node) = stack.pop() else { return true };
        if !kind_is_css_safe(node.kind()) {
            return false;
        }
        if let Some(call) = node.cast::<ast::FuncCall>()
            && recognized_role(call).is_none()
        {
            return false;
        }
        stack.extend(node.children().rev());
    }
    false
}

/// The markup-kind allow-list the verdict is decided from. `Star` and
/// `Underscore` join it alongside `Strong`/`Emph` for the same reason
/// `RawDelim` joins `Raw`: they are that construct's own delimiter and
/// never appear anywhere else in the tree.
fn kind_is_css_safe(kind: SyntaxKind) -> bool {
    // Deliberate exception, not part of the allow-list below: a half-typed
    // `#l(` under the caret parses with an `Error` node, and rendering it
    // as plain CSS text is what keeps the block from flashing a fallback
    // to a compiled-Typst error on every keystroke while it is incomplete.
    if kind == SyntaxKind::Error {
        return true;
    }
    matches!(
        kind,
        SyntaxKind::Markup
            | SyntaxKind::Text
            | SyntaxKind::Space
            | SyntaxKind::Parbreak
            | SyntaxKind::Linebreak
            | SyntaxKind::Escape
            | SyntaxKind::Shorthand
            | SyntaxKind::SmartQuote
            | SyntaxKind::Strong
            | SyntaxKind::Star
            | SyntaxKind::Emph
            | SyntaxKind::Underscore
            | SyntaxKind::Raw
            | SyntaxKind::RawLang
            | SyntaxKind::RawDelim
            | SyntaxKind::RawTrimmed
            | SyntaxKind::Link
            | SyntaxKind::Label
            | SyntaxKind::Heading
            | SyntaxKind::HeadingMarker
            | SyntaxKind::ListItem
            | SyntaxKind::ListMarker
            | SyntaxKind::EnumItem
            | SyntaxKind::EnumMarker
            | SyntaxKind::TermItem
            | SyntaxKind::TermMarker
            | SyntaxKind::Hash
            | SyntaxKind::Ident
            | SyntaxKind::FuncCall
            | SyntaxKind::Args
            | SyntaxKind::LeftParen
            | SyntaxKind::RightParen
            | SyntaxKind::Comma
            | SyntaxKind::Colon
            | SyntaxKind::Str
            | SyntaxKind::Named
            | SyntaxKind::ContentBlock
            // a content block's own delimiters, and nothing else's: the
            // `[body]` of a `#link` (adr/2026-09-link-is-for-resources.md)
            | SyntaxKind::LeftBracket
            | SyntaxKind::RightBracket
            | SyntaxKind::End
    )
}

/// The role a recognised call's whole invocation renders with, or `None`
/// for anything else — the callee-name half of the verdict, and (once the
/// verdict is CSS) what `Role` a matched `#l`/`#link`/`#meta`/`#quote`
/// gets. Typst's own `#link` wears the same role as `#l`: it is the link
/// form for what is not a note (adr/2026-09-link-is-for-resources.md).
fn recognized_role(call: ast::FuncCall) -> Option<Role> {
    let ast::Expr::Ident(name) = call.callee() else {
        return None;
    };
    match name.as_str() {
        "l" | "link" => Some(Role::Link),
        "meta" | "quote" => Some(Role::Meta),
        _ => None,
    }
}

/// A block's structural role. Text-based checks (`Quote`) only run once
/// the verdict above already accepted the tree — see the module doc.
fn block_role(source: &str, root: &SyntaxNode) -> BlockRole {
    if source.trim().is_empty() {
        return BlockRole::Blank;
    }
    for child in root.children() {
        if let Some(heading) = child.cast::<ast::Heading>() {
            let depth = u8::try_from(heading.depth().get()).unwrap_or(u8::MAX);
            return BlockRole::Heading(depth);
        }
        if child.cast::<ast::ListItem>().is_some()
            || child.cast::<ast::EnumItem>().is_some()
            || child.cast::<ast::TermItem>().is_some()
        {
            return BlockRole::Item {
                indent: leading_spaces(source) / crate::caret::INDENT.len(),
            };
        }
    }
    let leading = leading_spaces(source);
    if source
        .get(leading..)
        .is_some_and(|rest| rest.starts_with("> "))
    {
        return BlockRole::Quote;
    }
    BlockRole::Plain
}

/// The block's own leading run of ASCII spaces, in bytes (one byte per
/// space, so this is also the char count).
fn leading_spaces(source: &str) -> usize {
    source.chars().take_while(|&ch| ch == ' ').count()
}

/// Builds the spans tiling `source`, then layers on the two roles no
/// syntax node names: `Quote`'s leading `"> "` and a checklist item's
/// leading `[ ]`/`[x]`, both applied as byte-range overrides after the
/// tree walk so the walk itself never has to special-case them.
fn build_spans(
    source: &str,
    root: &SyntaxNode,
    block: BlockRole,
) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut checkbox_hits: Vec<(Range<usize>, bool)> = Vec::new();
    let mut stack: Vec<(usize, &SyntaxNode, Role)> =
        vec![(0, root, Role::Text)];
    for _ in 0..MAX_NODES {
        let Some((offset, node, role)) = stack.pop() else {
            break;
        };
        if node.children().next().is_none() {
            let (leaf, delimiter) = leaf_role(node.kind(), role);
            spans.push(Span {
                range: offset..offset + node.len(),
                role: leaf,
                delimiter,
            });
            continue;
        }
        push_children(
            node,
            offset,
            children_role(node.kind(), role),
            source,
            &mut spans,
            &mut checkbox_hits,
            &mut stack,
        );
    }

    // the walk is depth-first but a `Hash`+call merge (above) is emitted
    // the instant `push_children` reaches it, ahead of sibling frames still
    // sitting on the stack — sorting once here is simpler than threading
    // emission order through the walk, and the result tiles regardless of
    // which order the spans were produced in
    spans.sort_by_key(|span| span.range.start);
    if matches!(block, BlockRole::Quote) {
        let leading = leading_spaces(source);
        spans = override_role(spans, leading..leading + 2, Role::Marker);
    }
    for (range, done) in checkbox_hits {
        spans = override_role(spans, range, Role::Checkbox { done });
    }
    marker_owns_its_gap(spans, source)
}

/// A marker owns the run of spaces after it — `"= "`, `"- "` — the way the
/// quote's `"> "` already does, so an inactive block can hide the whole
/// prefix by hiding one span; two adjacent marker spans (the quote
/// override cutting a wider space leaf) fold into one for the same reason
/// (adr/2026-09-inactive-blocks-hide-their-syntax.md).
fn marker_owns_its_gap(spans: Vec<Span>, source: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        let gap = span.role == Role::Marker
            || span.role == Role::Text
                && source
                    .get(span.range.clone())
                    .is_some_and(|text| text.chars().all(|ch| ch == ' '));
        match out.last_mut() {
            Some(last) if gap && last.role == Role::Marker => {
                last.range.end = span.range.end;
            }
            _ => out.push(span),
        }
    }
    out
}

/// The role (and `Strong`/`Emph`/`Raw` delimiter flag) a leaf node's whole
/// byte range takes. A marker kind is always `Role::Marker` regardless of
/// what it is nested under; a `Star`/`Underscore`/`RawDelim` only ever
/// appears inside the `Strong`/`Emph`/`Raw` node whose delimiter it is, so
/// `inherited` is already that run's role by construction when one is hit.
fn leaf_role(kind: SyntaxKind, inherited: Role) -> (Role, bool) {
    match kind {
        SyntaxKind::HeadingMarker
        | SyntaxKind::ListMarker
        | SyntaxKind::EnumMarker
        | SyntaxKind::TermMarker => (Role::Marker, false),
        SyntaxKind::Star | SyntaxKind::Underscore | SyntaxKind::RawDelim => {
            (inherited, true)
        }
        SyntaxKind::Error => (Role::Text, false),
        _ => (inherited, false),
    }
}

/// The role a container node's children inherit: `Strong`/`Emph`/`Raw`
/// start a new run, everything else (`Markup`, `Heading`, `ListItem`,
/// `ContentBlock`…) just carries the role it was already pushed with.
fn children_role(kind: SyntaxKind, inherited: Role) -> Role {
    match kind {
        SyntaxKind::Strong => Role::Strong,
        SyntaxKind::Emph => Role::Emph,
        SyntaxKind::Raw => Role::Raw,
        _ => inherited,
    }
}

/// Pushes one container node's children onto the walk's stack, with two
/// exceptions handled inline because they change what gets pushed rather
/// than just a leaf's role: a `Hash` immediately followed by a recognised
/// `FuncCall` (`#l(...)`, `#meta(...)`, `#quote(...)`) becomes one whole
/// span over both and its children are never pushed — the call's own
/// arguments are not rendered as separate spans; and a bullet `ListItem`
/// whose body opens with `[ ]`/`[x]` records that range in `checkbox_hits`
/// for `build_spans` to relabel afterward.
fn push_children<'a>(
    node: &'a SyntaxNode,
    offset: usize,
    role: Role,
    source: &str,
    spans: &mut Vec<Span>,
    checkbox_hits: &mut Vec<(Range<usize>, bool)>,
    stack: &mut Vec<(usize, &'a SyntaxNode, Role)>,
) {
    let mut child_data: Vec<(usize, &SyntaxNode)> = Vec::new();
    let mut running = offset;
    for child in node.children() {
        child_data.push((running, child));
        running += child.len();
    }

    if node.kind() == SyntaxKind::ListItem
        && let Some(&(body_offset, _)) = child_data.last()
        && let Some(done) = checkbox_done(source, body_offset)
    {
        checkbox_hits.push((body_offset..body_offset + 3, done));
    }

    let mut frames = Vec::new();
    let mut skip_next = false;
    for i in 0..child_data.len() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let (child_offset, child) = child_data[i];
        if child.kind() == SyntaxKind::Hash
            && let Some(&(_, next)) = child_data.get(i + 1)
            && let Some(call) = next.cast::<ast::FuncCall>()
            && let Some(call_role) = recognized_role(call)
        {
            spans.push(Span {
                range: child_offset..child_offset + child.len() + next.len(),
                role: call_role,
                delimiter: false,
            });
            skip_next = true;
            continue;
        }
        frames.push((child_offset, child, role));
    }
    stack.extend(frames.into_iter().rev());
}

/// Whether `source` at `offset` opens with a checklist box, and its state.
fn checkbox_done(source: &str, offset: usize) -> Option<bool> {
    match source.get(offset..offset + 3) {
        Some("[ ]") => Some(false),
        Some("[x]") => Some(true),
        _ => None,
    }
}

/// Relabels the portion of `spans` inside `range` to `role`, splitting any
/// span the range only partly covers so the result still tiles exactly.
/// Used for the two roles no syntax node names (`> ` and `[ ]`/`[x]`): a
/// plain-text run can carry either sitting anywhere inside it, not aligned
/// to a leaf's own boundary, so relabelling has to be a byte-range
/// operation rather than a per-node one.
fn override_role(
    spans: Vec<Span>,
    range: Range<usize>,
    role: Role,
) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len() + 2);
    for span in spans {
        let overlap_start = span.range.start.max(range.start);
        let overlap_end = span.range.end.min(range.end);
        if overlap_start >= overlap_end {
            out.push(span);
            continue;
        }
        if span.range.start < overlap_start {
            out.push(Span {
                range: span.range.start..overlap_start,
                role: span.role,
                delimiter: span.delimiter,
            });
        }
        out.push(Span {
            range: overlap_start..overlap_end,
            role,
            delimiter: false,
        });
        if overlap_end < span.range.end {
            out.push(Span {
                range: overlap_end..span.range.end,
                role: span.role,
                delimiter: span.delimiter,
            });
        }
    }
    out
}

/// The CSS class (or class list) a span's role and delimiter flag render
/// with. A `Strong`/`Emph`/`Raw` delimiter byte gets a second class so a
/// decoration pass can dim it while the run around it keeps its full
/// weight.
pub fn class(role: Role, delimiter: bool) -> &'static str {
    match role {
        Role::Text => "mk-text",
        Role::Strong if delimiter => "mk-strong mk-delim",
        Role::Strong => "mk-strong",
        Role::Emph if delimiter => "mk-emph mk-delim",
        Role::Emph => "mk-emph",
        Role::Raw if delimiter => "mk-raw mk-delim",
        Role::Raw => "mk-raw",
        Role::Link => "mk-link",
        Role::Meta => "mk-meta",
        Role::Marker => "mk-marker",
        Role::Checkbox { done: true } => "mk-checkbox mk-checkbox-done",
        Role::Checkbox { done: false } => "mk-checkbox",
    }
}

/// The CSS class a whole block's structural role renders with.
pub fn block_class(block: BlockRole) -> String {
    match block {
        BlockRole::Plain => "mk-line".to_string(),
        BlockRole::Blank => "mk-blank".to_string(),
        BlockRole::Heading(level) => format!("mk-h{level}"),
        BlockRole::Item { .. } => "mk-item".to_string(),
        BlockRole::Quote => "mk-quote".to_string(),
    }
}

/// The inline `--mk-indent` custom property a nested `Item` block renders
/// with, so `.mk-item`'s `padding-left` (`assets/theme.css`) steps out per
/// nesting level instead of `block_class` flattening every depth to the
/// same `mk-item` class — depth is unbounded like `Heading`'s, so a class
/// per level is not an option (see `[class*="mk-h"]`'s own comment in
/// `assets/theme.css`), and a custom property is. `None` on every other
/// role, and on a top-level item (`indent: 0`), where `.mk-item`'s base
/// `padding-left: 16px` is already correct with no property set.
pub fn item_indent_style(block: BlockRole) -> Option<String> {
    match block {
        BlockRole::Item { indent } if indent > 0 => {
            Some(format!("--mk-indent: {indent}"))
        }
        _ => None,
    }
}

/// Subdivides `pieces` at every span boundary that falls strictly inside a
/// piece's own byte range, so a decoration pass can draw the caret,
/// selection and IME preview at the same time as the markup roles instead
/// of in two disagreeing layers. `Piece::Caret` carries no position of its
/// own (it is zero-width): its role is read off the position the pieces
/// around it already establish — the end of whichever concrete piece came
/// right before it, or, if it opens the line, the start of the one right
/// after. A caret with no concrete piece anywhere in `pieces` (a wholly
/// empty line) has no byte to look up and renders as `Role::Text`, the
/// same fallback a stale span list's gap would get.
pub fn tint(
    spans: &[Span],
    pieces: Vec<crate::caret::Piece>,
) -> Vec<(Role, bool, crate::caret::Piece)> {
    let mut out = Vec::new();
    let mut known_pos: Option<usize> = None;
    for (index, piece) in pieces.iter().enumerate() {
        match piece {
            crate::caret::Piece::Caret => {
                let pos = known_pos
                    .or_else(|| lookahead_start(&pieces[index + 1..]));
                let (role, delimiter) =
                    pos.map_or((Role::Text, false), |p| role_at(spans, p));
                out.push((role, delimiter, crate::caret::Piece::Caret));
            }
            crate::caret::Piece::Text { start, text } => {
                known_pos = Some(start + text.len());
                push_split(spans, *start, text, &mut out, |s, t| {
                    crate::caret::Piece::Text { start: s, text: t }
                });
            }
            crate::caret::Piece::Selected { start, text } => {
                known_pos = Some(start + text.len());
                push_split(spans, *start, text, &mut out, |s, t| {
                    crate::caret::Piece::Selected { start: s, text: t }
                });
            }
            crate::caret::Piece::Preview { start, text } => {
                known_pos = Some(start + text.len());
                push_split(spans, *start, text, &mut out, |s, t| {
                    crate::caret::Piece::Preview { start: s, text: t }
                });
            }
            crate::caret::Piece::CaretBox { start, cluster } => {
                known_pos = Some(start + cluster.len());
                push_split(spans, *start, cluster, &mut out, |s, t| {
                    crate::caret::Piece::CaretBox {
                        start: s,
                        cluster: t,
                    }
                });
            }
        }
    }
    out
}

/// The first concrete piece's `start` in `rest`, for a `Caret` that opens
/// `pieces` with nothing processed yet to read a position off.
fn lookahead_start(rest: &[crate::caret::Piece]) -> Option<usize> {
    rest.iter().find_map(|piece| match piece {
        crate::caret::Piece::Text { start, .. }
        | crate::caret::Piece::Selected { start, .. }
        | crate::caret::Piece::Preview { start, .. }
        | crate::caret::Piece::CaretBox { start, .. } => Some(*start),
        crate::caret::Piece::Caret => None,
    })
}

/// Splits one piece's `text` at every span boundary strictly inside its
/// range, pushing one `(role, delimiter, piece)` per resulting slice.
/// `make` rebuilds the same `Piece` variant `text` came from, at the
/// slice's own start. A cut is only ever taken at a char boundary of
/// `text` — the same UTF-8 buffer a span's byte offsets were computed
/// against, so every boundary from `spans` already lands on one; the check
/// is defensive, not expected to ever reject one.
fn push_split<F>(
    spans: &[Span],
    start: usize,
    text: &str,
    out: &mut Vec<(Role, bool, crate::caret::Piece)>,
    make: F,
) where
    F: Fn(usize, String) -> crate::caret::Piece,
{
    let end = start + text.len();
    let mut cuts = vec![0, text.len()];
    for span in spans {
        for boundary in [span.range.start, span.range.end] {
            if boundary > start && boundary < end {
                let local = boundary - start;
                if text.is_char_boundary(local) {
                    cuts.push(local);
                }
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    for pair in cuts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let slice = text.get(a..b).unwrap_or("").to_string();
        let (role, delimiter) = role_at(spans, start + a);
        out.push((role, delimiter, make(start + a, slice)));
    }
}

/// The role and delimiter flag of whichever span covers `pos`, or plain
/// text when none does — a stale caller past the spans' own tiled range,
/// defensively, since `spans` is trusted to tile `0..source.len()` for any
/// output `model` actually returned.
fn role_at(spans: &[Span], pos: usize) -> (Role, bool) {
    spans
        .iter()
        .find(|span| span.range.start <= pos && pos < span.range.end)
        .map_or((Role::Text, false), |span| (span.role, span.delimiter))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::caret::Piece;

    fn css(source: &str) -> Markup {
        match model(source) {
            Draw::Css(markup) => markup,
            Draw::Typst => panic!("expected Draw::Css for {source:?}"),
        }
    }

    fn assert_typst(source: &str) {
        assert_eq!(model(source), Draw::Typst, "{source:?}");
    }

    /// Every byte of `source` belongs to exactly one span, in ascending
    /// order, no gaps and no overlap.
    fn assert_tiles(spans: &[Span], len: usize) {
        let Some(first) = spans.first() else {
            assert_eq!(
                len, 0,
                "an empty span list only tiles an empty source"
            );
            return;
        };
        assert_eq!(first.range.start, 0, "{spans:?}");
        for pair in spans.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start, "{spans:?}");
        }
        assert_eq!(spans.last().map(|s| s.range.end), Some(len), "{spans:?}");
    }

    // -- verdict: CSS ---------------------------------------------------

    #[test]
    fn plain_prose_is_css() {
        let markup = css("just some prose\n");
        assert_eq!(markup.block, BlockRole::Plain);
        assert_tiles(&markup.spans, "just some prose\n".len());
    }

    #[test]
    fn a_heading_is_css() {
        let markup = css("= Title");
        assert_eq!(markup.block, BlockRole::Heading(1));
        assert_tiles(&markup.spans, "= Title".len());
        assert_eq!(markup.spans[0].role, Role::Marker);
    }

    #[test]
    fn a_list_item_is_css() {
        let source = "- one item";
        let markup = css(source);
        assert_eq!(markup.block, BlockRole::Item { indent: 0 });
        assert_tiles(&markup.spans, source.len());
        assert_eq!(markup.spans[0].role, Role::Marker);
    }

    /// The marker span swallows the spaces after it, and only spaces: a
    /// tab, a non-space leaf, or nothing at all leaves the marker alone.
    #[test]
    fn a_marker_owns_the_gap_after_it() {
        for (source, marker) in [
            ("= Title", "= "),
            ("-   item", "-   "),
            ("-\titem", "-"),
            ("- ", "- "),
            (">  quoted", ">  "),
        ] {
            let markup = css(source);
            let first = &markup.spans[0];
            assert_eq!(first.role, Role::Marker, "{source:?}");
            assert_eq!(&source[first.range.clone()], marker, "{source:?}");
            assert_tiles(&markup.spans, source.len());
        }
    }

    #[test]
    fn a_nested_checklist_is_css_and_both_boxes_are_tagged() {
        let source = "- [ ] parent\n  - [x] child\n";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
        let parent_box = source.find("[ ]").expect("parent checkbox");
        let child_box = source.find("[x]").expect("child checkbox");
        let role_at_byte = |byte: usize| {
            markup
                .spans
                .iter()
                .find(|s| s.range.start <= byte && byte < s.range.end)
                .map(|s| s.role)
        };
        assert_eq!(
            role_at_byte(parent_box),
            Some(Role::Checkbox { done: false })
        );
        assert_eq!(
            role_at_byte(child_box),
            Some(Role::Checkbox { done: true })
        );
    }

    #[test]
    fn an_inline_link_is_css_and_spans_the_hash() {
        let source = "See #l(\"target\") there";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
        let hash = source.find('#').expect("a hash");
        let link_span = markup
            .spans
            .iter()
            .find(|s| s.range.start == hash)
            .expect("a span starting at the hash");
        assert_eq!(link_span.role, Role::Link);
        assert_eq!(&source[link_span.range.clone()], "#l(\"target\")");
    }

    #[test]
    fn a_typst_link_to_a_resource_is_css_and_wears_the_link_role() {
        let source = "the #link(\"/assets/a.pdf\")[slides] here";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
        let hash = source.find('#').expect("a hash");
        let link_span = markup
            .spans
            .iter()
            .find(|s| s.range.start == hash)
            .expect("a span starting at the hash");
        assert_eq!(link_span.role, Role::Link);
        assert_eq!(
            &source[link_span.range.clone()],
            "#link(\"/assets/a.pdf\")[slides]"
        );
    }

    #[test]
    fn a_quote_call_is_css_meta() {
        let source = "#quote(\"a citation\")";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
        assert_eq!(markup.spans.len(), 1);
        assert_eq!(markup.spans[0].role, Role::Meta);
        assert_eq!(markup.spans[0].range, 0..source.len());
    }

    #[test]
    fn a_bare_meta_call_is_css_meta() {
        // exercises the "meta" arm of recognised_role on its own, not just
        // riding along with the "quote" case above — the preamble's
        // `#meta(...)` never reaches this arm because ModuleImport/ShowRule
        // fall the whole block back to Draw::Typst first (see
        // a_preamble_block_falls_back_so_the_compiled_meta_line_is_unchanged)
        let source = "#meta(id: \"x\")";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
        assert_eq!(markup.spans.len(), 1);
        assert_eq!(markup.spans[0].role, Role::Meta);
        assert_eq!(markup.spans[0].range, 0..source.len());
    }

    #[test]
    fn emph_takes_its_role_over_the_whole_run_including_delimiters() {
        let markup = css("_em_");
        assert_tiles(&markup.spans, "_em_".len());
        assert_eq!(
            markup.spans,
            vec![
                Span {
                    range: 0..1,
                    role: Role::Emph,
                    delimiter: true
                },
                Span {
                    range: 1..3,
                    role: Role::Emph,
                    delimiter: false
                },
                Span {
                    range: 3..4,
                    role: Role::Emph,
                    delimiter: true
                },
            ]
        );
    }

    #[test]
    fn raw_takes_its_role_over_the_whole_run_including_delimiters() {
        let markup = css("`raw`");
        assert_tiles(&markup.spans, "`raw`".len());
        assert_eq!(
            markup.spans,
            vec![
                Span {
                    range: 0..1,
                    role: Role::Raw,
                    delimiter: true
                },
                Span {
                    range: 1..4,
                    role: Role::Raw,
                    delimiter: false
                },
                Span {
                    range: 4..5,
                    role: Role::Raw,
                    delimiter: true
                },
            ]
        );
    }

    #[test]
    fn a_greater_than_line_is_css_quote() {
        let source = "> quoted line";
        let markup = css(source);
        assert_eq!(markup.block, BlockRole::Quote);
        assert_tiles(&markup.spans, source.len());
        assert_eq!(markup.spans[0].role, Role::Marker);
        assert_eq!(markup.spans[0].range, 0..2);
    }

    // -- verdict: Typst ---------------------------------------------------

    #[test]
    fn an_equation_falls_back() {
        assert_typst("$x$");
    }

    #[test]
    fn a_table_call_falls_back() {
        assert_typst("#table(columns: 2)");
    }

    #[test]
    fn a_figure_call_falls_back() {
        assert_typst("#figure(image(\"a.png\"))");
    }

    #[test]
    fn an_image_call_falls_back() {
        assert_typst("#image(\"a.png\")");
    }

    #[test]
    fn an_unknown_call_falls_back() {
        assert_typst("#unknown(1)");
    }

    #[test]
    fn a_let_binding_falls_back() {
        assert_typst("#let x = 1");
    }

    #[test]
    fn an_unrecognised_call_nested_in_markup_falls_back() {
        // the outer call is `emph`, not the app's own `l`/`meta`/`quote` —
        // rejected the moment the walk reaches it, wherever it sits
        assert_typst("_#table(columns: 1)_");
    }

    #[test]
    fn a_call_with_a_non_ident_callee_falls_back() {
        // `foo.bar` is a field access, not a plain identifier — never
        // `meta`/`l`/`quote` no matter what it is named
        assert_typst("#foo.bar()");
    }

    #[test]
    fn a_preamble_block_falls_back_so_the_compiled_meta_line_is_unchanged() {
        // import + show fold into one block (blocks::fold_preamble); their
        // node kinds are not on the allow-list, and that is intentional —
        // the note's meta line keeps compiling exactly as it does today
        let source = "#import \"/templates/template.typ\": *\n#show: note\n#meta(id: \"x\")\n";
        assert_typst(source);
    }

    // -- the Error exception ---------------------------------------------

    #[test]
    fn a_half_typed_call_stays_css() {
        let source = "before #l( after";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
    }

    #[test]
    fn a_stray_bracket_is_an_error_node_that_stays_css_plain_text() {
        // an unmatched `]` outside any recognised call is where the Error
        // exception actually earns its keep: nothing here short-circuits
        // into one merged span the way `#l(` does, so the walk visits the
        // Error leaf itself
        let source = "before ]after";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
        let bracket = source.find(']').expect("the stray bracket");
        let span = markup
            .spans
            .iter()
            .find(|s| s.range.start == bracket)
            .expect("a span at the bracket");
        assert_eq!(span.role, Role::Text);
    }

    // -- tiling on a multi-line block --------------------------------------

    #[test]
    fn a_multi_line_block_still_tiles_exactly() {
        let source = "- one\n  - two\n  - three\n";
        let markup = css(source);
        assert_tiles(&markup.spans, source.len());
    }

    // -- every BlockRole variant -------------------------------------------

    #[test]
    fn blank_blocks_are_blank() {
        for source in ["", "   ", "\n"] {
            assert_eq!(css(source).block, BlockRole::Blank, "{source:?}");
        }
    }

    #[test]
    fn heading_depth_is_read_from_the_marker() {
        assert_eq!(css("== Two").block, BlockRole::Heading(2));
        assert_eq!(css("=== Three").block, BlockRole::Heading(3));
    }

    #[test]
    fn item_indent_is_leading_spaces_over_indent_width() {
        assert_eq!(css("- top").block, BlockRole::Item { indent: 0 });
        assert_eq!(css("  - one deep").block, BlockRole::Item { indent: 1 });
        assert_eq!(css("    - two deep").block, BlockRole::Item { indent: 2 });
        assert_eq!(css("+ enum").block, BlockRole::Item { indent: 0 });
        assert_eq!(css("/ Term: text").block, BlockRole::Item { indent: 0 });
    }

    #[test]
    fn quote_block_role_is_read_from_leading_text() {
        assert_eq!(css("> quoted").block, BlockRole::Quote);
        assert_eq!(css("not a quote").block, BlockRole::Plain);
    }

    #[test]
    fn plain_prose_is_plain() {
        assert_eq!(css("prose line").block, BlockRole::Plain);
    }

    // -- class / block_class -------------------------------------------------

    #[test]
    fn class_names_cover_every_role() {
        assert_eq!(class(Role::Text, false), "mk-text");
        assert_eq!(class(Role::Strong, false), "mk-strong");
        assert_eq!(class(Role::Strong, true), "mk-strong mk-delim");
        assert_eq!(class(Role::Emph, false), "mk-emph");
        assert_eq!(class(Role::Emph, true), "mk-emph mk-delim");
        assert_eq!(class(Role::Raw, false), "mk-raw");
        assert_eq!(class(Role::Raw, true), "mk-raw mk-delim");
        assert_eq!(class(Role::Link, false), "mk-link");
        assert_eq!(class(Role::Meta, false), "mk-meta");
        assert_eq!(class(Role::Marker, false), "mk-marker");
        assert_eq!(
            class(Role::Checkbox { done: false }, false),
            "mk-checkbox"
        );
        assert_eq!(
            class(Role::Checkbox { done: true }, false),
            "mk-checkbox mk-checkbox-done"
        );
    }

    #[test]
    fn block_class_names_cover_every_variant() {
        assert_eq!(block_class(BlockRole::Plain), "mk-line");
        assert_eq!(block_class(BlockRole::Blank), "mk-blank");
        assert_eq!(block_class(BlockRole::Heading(1)), "mk-h1");
        assert_eq!(block_class(BlockRole::Heading(3)), "mk-h3");
        assert_eq!(block_class(BlockRole::Item { indent: 2 }), "mk-item");
        assert_eq!(block_class(BlockRole::Quote), "mk-quote");
    }

    #[test]
    fn item_indent_style_steps_out_past_the_top_level() {
        assert_eq!(item_indent_style(BlockRole::Item { indent: 0 }), None);
        assert_eq!(
            item_indent_style(BlockRole::Item { indent: 1 }),
            Some("--mk-indent: 1".to_string())
        );
        assert_eq!(
            item_indent_style(BlockRole::Item { indent: 2 }),
            Some("--mk-indent: 2".to_string())
        );
        assert_eq!(item_indent_style(BlockRole::Plain), None);
        assert_eq!(item_indent_style(BlockRole::Blank), None);
        assert_eq!(item_indent_style(BlockRole::Heading(1)), None);
        assert_eq!(item_indent_style(BlockRole::Quote), None);
    }

    // -- tint -------------------------------------------------------------

    #[test]
    fn tint_splits_a_selection_edge_mid_strong_run() {
        // "*bold* rest": delimiters at 0..1 and 5..6, interior 1..5,
        // trailing text 6..11
        let markup = css("*bold* rest");
        let piece = Piece::Selected {
            start: 3,
            text: "ld* r".to_string(), // 3..8: crosses the 5 and 6 boundaries
        };
        let tinted = tint(&markup.spans, vec![piece]);
        // the trailing " r" still splits once more, at byte 7: the source
        // tree itself carries a Space/Text boundary there (" " and "rest"
        // are separate leaves), even though both sides share Role::Text —
        // tiling never requires merging same-role neighbours
        assert_eq!(
            tinted,
            vec![
                (
                    Role::Strong,
                    false,
                    Piece::Selected {
                        start: 3,
                        text: "ld".to_string()
                    }
                ),
                (
                    Role::Strong,
                    true,
                    Piece::Selected {
                        start: 5,
                        text: "*".to_string()
                    }
                ),
                (
                    Role::Text,
                    false,
                    Piece::Selected {
                        start: 6,
                        text: " ".to_string()
                    }
                ),
                (
                    Role::Text,
                    false,
                    Piece::Selected {
                        start: 7,
                        text: "r".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn tint_places_a_caret_inside_a_delimiter() {
        let markup = css("*bold* rest");
        let pieces = vec![
            Piece::Caret,
            Piece::Text {
                start: 0,
                text: "*bold* rest".to_string(),
            },
        ];
        let tinted = tint(&markup.spans, pieces);
        assert_eq!(tinted[0].0, Role::Strong);
        assert!(tinted[0].1, "the caret sits inside the leading delimiter");
        assert_eq!(tinted[0].2, Piece::Caret);
    }

    #[test]
    fn tint_carries_a_preview_piece_through() {
        let markup = css("*bold* rest");
        let piece = Piece::Preview {
            start: 2,
            text: "^".to_string(),
        };
        let tinted = tint(&markup.spans, vec![piece]);
        assert_eq!(
            tinted,
            vec![(
                Role::Strong,
                false,
                Piece::Preview {
                    start: 2,
                    text: "^".to_string()
                }
            )]
        );
    }

    #[test]
    fn tint_defaults_a_wholly_isolated_caret_to_text() {
        let markup = css("plain");
        let tinted = tint(&markup.spans, vec![Piece::Caret]);
        assert_eq!(tinted, vec![(Role::Text, false, Piece::Caret)]);
    }

    #[test]
    fn tint_on_an_empty_piece_list_is_empty() {
        let markup = css("plain");
        assert_eq!(tint(&markup.spans, vec![]), vec![]);
    }

    #[test]
    fn tint_splits_and_carries_a_caret_box_piece() {
        let markup = css("*bold* rest");
        // two leading carets exercise `lookahead_start`'s own skip over a
        // `Caret` entry (it carries no position) before it reaches the
        // `CaretBox` piece that actually answers one
        let pieces = vec![
            Piece::Caret,
            Piece::Caret,
            Piece::CaretBox {
                start: 5,
                cluster: "*".to_string(),
            },
        ];
        let tinted = tint(&markup.spans, pieces);
        assert_eq!(tinted[0].0, Role::Strong, "found via the CaretBox ahead");
        assert_eq!(tinted[0].2, Piece::Caret);
        assert_eq!(tinted[1], tinted[0]);
        assert_eq!(
            tinted[2],
            (
                Role::Strong,
                true,
                Piece::CaretBox {
                    start: 5,
                    cluster: "*".to_string()
                }
            )
        );
    }

    #[test]
    fn tint_caret_lookahead_finds_a_selected_or_preview_piece() {
        // `lookahead_start` matches each concrete `Piece` variant on its own
        // line; a caret ahead of only a `CaretBox` (tested above) leaves the
        // `Selected`/`Preview` arms unexercised
        let markup = css("*bold* rest");
        let selected = tint(
            &markup.spans,
            vec![
                Piece::Caret,
                Piece::Selected {
                    start: 5,
                    text: "*".to_string(),
                },
            ],
        );
        assert_eq!(selected[0].0, Role::Strong);
        let preview = tint(
            &markup.spans,
            vec![
                Piece::Caret,
                Piece::Preview {
                    start: 5,
                    text: "*".to_string(),
                },
            ],
        );
        assert_eq!(preview[0].0, Role::Strong);
    }

    #[test]
    fn tint_splits_a_preview_piece_at_a_span_boundary() {
        let markup = css("*bold* rest");
        // "d*" runs 4..6, straddling the closing delimiter's boundary at 5
        let piece = Piece::Preview {
            start: 4,
            text: "d*".to_string(),
        };
        let tinted = tint(&markup.spans, vec![piece]);
        assert_eq!(
            tinted,
            vec![
                (
                    Role::Strong,
                    false,
                    Piece::Preview {
                        start: 4,
                        text: "d".to_string()
                    }
                ),
                (
                    Role::Strong,
                    true,
                    Piece::Preview {
                        start: 5,
                        text: "*".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn tint_splits_a_caret_box_piece_at_a_span_boundary() {
        let markup = css("*bold* rest");
        // "*b" runs 0..2, straddling the opening delimiter's boundary at 1
        let piece = Piece::CaretBox {
            start: 0,
            cluster: "*b".to_string(),
        };
        let tinted = tint(&markup.spans, vec![piece]);
        assert_eq!(
            tinted,
            vec![
                (
                    Role::Strong,
                    true,
                    Piece::CaretBox {
                        start: 0,
                        cluster: "*".to_string()
                    }
                ),
                (
                    Role::Strong,
                    false,
                    Piece::CaretBox {
                        start: 1,
                        cluster: "b".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn tint_never_cuts_a_preview_piece_off_its_own_char_boundary() {
        // IME composition text is not a substring of the source: at the
        // same relative offset a span boundary from `spans` (always a char
        // boundary of *source*) can land mid-character of the *preview*
        // text instead. `push_split`'s boundary check is defensive for
        // exactly this — the cut is silently skipped rather than panicking
        // or slicing off a byte.
        let markup = css("*bold* rest");
        // span boundary at byte 1 (the opening delimiter's end) would fall
        // inside "é" (a 2-byte character) in this composition text, which
        // is not a char boundary of the text itself
        let piece = Piece::Preview {
            start: 0,
            text: "éx".to_string(),
        };
        let tinted = tint(&markup.spans, vec![piece.clone()]);
        assert_eq!(
            tinted,
            vec![(Role::Strong, true, piece)],
            "the un-cuttable boundary is skipped, not split on"
        );
    }

    // -- override_role ------------------------------------------------------

    #[test]
    fn override_role_splits_a_span_on_both_sides_of_the_overridden_range() {
        // the range 3..6 sits strictly inside the one span 0..10, so both
        // the left remainder (0..3) and the right remainder (6..10) must
        // survive as their own spans — the real callers (quote's leading
        // `"> "`, a checklist's `[ ]`/`[x]`) never start mid-leaf, so this
        // exercises the general case directly
        let spans = vec![Span {
            range: 0..10,
            role: Role::Text,
            delimiter: false,
        }];
        let out = override_role(spans, 3..6, Role::Marker);
        assert_eq!(
            out,
            vec![
                Span {
                    range: 0..3,
                    role: Role::Text,
                    delimiter: false
                },
                Span {
                    range: 3..6,
                    role: Role::Marker,
                    delimiter: false
                },
                Span {
                    range: 6..10,
                    role: Role::Text,
                    delimiter: false
                },
            ]
        );
    }

    // -- MAX_NODES exhaustion ------------------------------------------------

    #[test]
    fn a_tree_past_the_node_cap_falls_back_to_typst() {
        // MAX_NODES/2 short list items comfortably exceeds the cap: each
        // item alone is ListItem + ListMarker + Space + Markup + Text
        let source = "- a\n".repeat(MAX_NODES / 2);
        assert_typst(&source);
        // and a small tree of the same shape stays well under the cap
        assert!(matches!(model("- a\n- b\n- c\n"), Draw::Css(_)));
    }
}
