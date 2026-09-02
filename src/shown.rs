//! What an overlay's query input may still be showing when it speaks.
//!
//! The app writes a query from two places the webview cannot see coming:
//! the relay pushes a key typed at the pane before the overlay's focus grab
//! landed (adr/2026-09-overlay-keys-relay-before-focus-lands.md), and a step
//! change clears the field (the creator's type → title). A write reaches the
//! input as a patch over the edit socket while the focus grab travels over a
//! separate eval, so a key typed at the input in between arrives with a
//! snapshot the patch has not reached: an `input` event whose value is the
//! stale field plus the key. Replacing the query with that value loses every
//! letter the patch carried — one `make e2e` run named the note `oncept`,
//! another chose the type a lone `t` matched
//! (adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md).

/// Every value the field may be showing at its next `input` event: the last
/// value it reported, then each value the app wrote since, oldest first.
/// Patches land in order, so the field shows exactly one of these.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    values: Vec<String>,
}

/// One write per relayed key: a burst longer than this outruns any focus
/// grab by a margin no person types at, and the oldest write is the one the
/// field has most surely shown already.
const KEPT: usize = 32;

impl Shown {
    /// A freshly mounted input shows its opener's value and nothing else.
    pub fn opened(initial: &str) -> Self {
        Self {
            values: vec![initial.to_string()],
        }
    }

    /// The app set the query to `value`: the field shows it once the patch
    /// lands, and may show any earlier write until then.
    pub fn wrote(&mut self, value: &str) {
        if self.values.len() >= KEPT {
            self.values.remove(0);
        }
        self.values.push(value.to_string());
    }

    /// The field reported `reported` while the query held `query`: answers
    /// the query as the person meant it. The field's previous value is the
    /// longest remembered value the report extends — what was typed is the
    /// rest, appended to the query — or the one it shortens by a character,
    /// a Backspace, which pops the query instead. A report matching neither
    /// is a deliberate edit elsewhere in the field, and the field is right.
    pub fn typed(&mut self, query: &str, reported: &str) -> String {
        let extended = self
            .values
            .iter()
            .enumerate()
            .filter(|(_, value)| {
                reported.len() > value.len()
                    && reported.starts_with(value.as_str())
            })
            .max_by_key(|(_, value)| value.len());
        let (from, answer) = match extended {
            Some((from, value)) => {
                (from, format!("{query}{}", &reported[value.len()..]))
            }
            None => {
                let shortened = self.values.iter().rposition(|value| {
                    value.starts_with(reported)
                        && value.chars().count()
                            == reported.chars().count() + 1
                });
                match shortened {
                    Some(from) => {
                        let mut popped = query.to_string();
                        popped.pop();
                        (from, popped)
                    }
                    None => (self.values.len(), reported.to_string()),
                }
            }
        };
        // the report proves every patch up to the value it extends landed;
        // the writes after it may still be on their way, and the answer is
        // the next one
        let mut pending =
            self.values.split_off((from + 1).min(self.values.len()));
        self.values = vec![reported.to_string()];
        self.values.append(&mut pending);
        self.wrote(&answer);
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The normal path: the field is up to date and each report extends
    /// the query by the key just typed.
    #[test]
    fn a_report_extending_the_query_is_the_query_plus_the_key() {
        let mut shown = Shown::opened("");
        assert_eq!(shown.typed("", "c"), "c");
        assert_eq!(shown.typed("c", "co"), "co");
        assert_eq!(shown.typed("co", "c"), "c");
    }

    /// The relay wrote four letters the field has not shown yet; the fifth
    /// lands on the stale field and is reported alone.
    #[test]
    fn a_key_on_a_stale_field_joins_the_relayed_letters() {
        let mut shown = Shown::opened("");
        for written in ["c", "co", "con", "conc"] {
            shown.wrote(written);
        }
        assert_eq!(shown.typed("conc", "e"), "conce");
        // the first two patches then land before the next key
        assert_eq!(shown.typed("conce", "cop"), "concep");
        // and every patch has landed by the last one
        assert_eq!(shown.typed("concep", "concept"), "concept");
    }

    /// The creator's step change clears the query while the field still
    /// shows the type name; the title's first letter is reported on top
    /// of it.
    #[test]
    fn a_key_on_a_field_a_clear_has_not_reached_is_the_key_alone() {
        let mut shown = Shown::opened("");
        shown.typed("", "concept");
        shown.wrote("");
        assert_eq!(shown.typed("", "conceptt"), "t");
        assert_eq!(shown.typed("t", "ty"), "ty");
    }

    /// A Backspace on a stale field shortens what the field showed by one
    /// character; the query loses its last one.
    #[test]
    fn a_report_one_character_short_of_a_shown_value_pops_the_query() {
        let mut shown = Shown::opened("");
        shown.wrote("c");
        shown.wrote("co");
        assert_eq!(shown.typed("co", ""), "c");
        let mut shown = Shown::opened("é");
        shown.wrote("éa");
        assert_eq!(shown.typed("éa", ""), "é");
    }

    /// A report matching nothing remembered is an edit the field made on
    /// its own — a selection replaced, a caret moved — and stands as is.
    #[test]
    fn a_report_matching_nothing_shown_is_taken_as_is() {
        let mut shown = Shown::opened("conc");
        assert_eq!(shown.typed("conc", "xonc"), "xonc");
        let mut shown = Shown::default();
        assert_eq!(shown.typed("", "abc"), "abc");
    }

    /// The ex line opens with a prefill the field shows from the start.
    #[test]
    fn an_opener_value_is_what_the_field_shows_first() {
        let mut shown = Shown::opened("'<,'>");
        assert_eq!(shown.typed("'<,'>", "'<,'>s"), "'<,'>s");
    }

    /// The memory is bounded: the oldest write leaves once the cap is
    /// reached, and a report extending only that one is then an edit the
    /// field made on its own.
    #[test]
    fn the_memory_keeps_the_newest_writes_only() {
        let mut shown = Shown::opened("");
        let mut value = String::new();
        for _ in 0..KEPT {
            value.push('a');
            shown.wrote(&value);
        }
        assert_eq!(shown.values.len(), KEPT);
        assert_eq!(shown.values[0], "a");
        assert_eq!(shown.typed(&value, "b"), "b");
    }
}
