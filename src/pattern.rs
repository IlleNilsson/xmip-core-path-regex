//! The expression of the `regex` language: a pattern, optionally followed by
//! `#name` picking one named group out of it.

use contract::ContractError;
use regex::Regex;
use std::ops::Range;

/// A compiled expression: the pattern and which part of a match it names.
pub struct Pattern {
    regex: Regex,
    group: Option<String>,
}

impl Pattern {
    /// Compile `expression`. The part after the last unescaped `#` outside a
    /// character class is a group name; `\#` and `[#]` stay in the pattern.
    ///
    /// # Errors
    /// The pattern is not valid `regex` syntax, or the named group is not
    /// defined in it.
    pub fn parse(expression: &str) -> Result<Self, ContractError> {
        let (pattern, group) = match split_group(expression) {
            Some(at) => (&expression[..at], Some(expression[at + 1..].to_string())),
            None => (expression, None),
        };
        let regex = Regex::new(pattern).map_err(|error| ContractError {
            message: format!("pattern refused: {error}"),
        })?;
        if let Some(name) = &group {
            let defined = regex.capture_names().flatten().any(|known| known == name);
            if !defined {
                return Err(ContractError {
                    message: format!("{expression:?} names no group {name:?}"),
                });
            }
        }
        Ok(Self { regex, group })
    }

    /// The span in `text` the expression names: the named group when one was
    /// given, else the first capture group when the pattern has one, else the
    /// whole first match. `None` when nothing matches, or the group the
    /// expression names took no part in the match.
    #[must_use]
    pub fn find(&self, text: &str) -> Option<Range<usize>> {
        let captures = self.regex.captures(text)?;
        let found = match &self.group {
            Some(name) => captures.name(name),
            None if self.regex.captures_len() > 1 => captures.get(1),
            None => captures.get(0),
        };
        found.map(|matched| matched.range())
    }
}

/// The byte offset of the last `#` that is neither escaped nor inside a
/// character class; `None` when the expression is all pattern.
fn split_group(expression: &str) -> Option<usize> {
    let mut last = None;
    let mut escaped = false;
    let mut in_class = false;
    for (at, symbol) in expression.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match symbol {
            '\\' => escaped = true,
            '[' => in_class = true,
            ']' => in_class = false,
            '#' if !in_class => last = Some(at),
            _ => {}
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_pattern_names_its_whole_first_match() {
        let pattern = Pattern::parse(r"\d+").expect("compiles");
        assert_eq!(pattern.find("order 42 of 7"), Some(6..8));
        assert_eq!(pattern.find("none"), None);
    }

    #[test]
    fn the_first_capture_group_narrows_the_match() {
        let pattern = Pattern::parse(r"id=(\w+);").expect("compiles");
        assert_eq!(pattern.find("x id=A1; y"), Some(5..7));
    }

    #[test]
    fn a_named_group_is_picked_after_the_hash() {
        let pattern = Pattern::parse(r"(?<key>\w+)=(?<value>\w+)#value").expect("compiles");
        assert_eq!(pattern.find("ref=R7"), Some(4..6));
        let optional = Pattern::parse(r"(?<a>x)?(?<b>y)#a").expect("compiles");
        assert_eq!(optional.find("y"), None);
    }

    #[test]
    fn escaped_and_classed_hashes_stay_in_the_pattern() {
        assert_eq!(split_group(r"a\#b"), None);
        assert_eq!(split_group(r"a[#]b"), None);
        assert_eq!(split_group(r"a\\#b"), Some(3));
        assert_eq!(split_group(r"(?<n>\w)#n"), Some(8));
        let literal = Pattern::parse(r"item\#(\d+)").expect("compiles");
        assert_eq!(literal.find("item#12"), Some(5..7));
    }

    #[test]
    fn bad_syntax_and_unknown_groups_are_refused() {
        assert!(Pattern::parse(r"(unclosed").is_err());
        assert!(Pattern::parse(r"(?<a>x)#b").is_err());
    }
}
