#![forbid(unsafe_code)]

//! The regular-expression path technology — a technology of `xmip-core-path`.
//!
//! [`RegexLanguage`] is the [`PathLanguage`] `regex`: it compiles a
//! [`Pattern`] once, and the compiled pattern reads from a UTF-8 text Stream
//! and writes into a rewrite of it (ADR-0013). Promote reads through it;
//! demote writes through it; route and process read, and the `regex` route
//! technology extracts from a context value's text through the same
//! [`Pattern`].
//!
//! An expression is a pattern in the `regex` crate's syntax, optionally
//! followed by `#name` to pick a named group: `(?<id>[A-Z]\d+)#id`. A read
//! yields the first match as text — the named group when one is given, else
//! the first capture group when the pattern has one, else the whole match. A
//! write replaces that same span with the value's text, and a pattern that
//! matches nothing is refused: demote names a place, it does not invent one.
//!
//! The cost is a scan. Nothing is parsed and no document is built; the text is
//! the Stream's own, decoded once and borrowed, and the automaton runs once
//! over it, so a promotion by pattern is cheap by construction and safe
//! against a pattern an operator got wrong.

mod pattern;

pub use pattern::Pattern;

use contract::ContractError;
use path::{CompiledExpression, Content, PathLanguage, Rewriting};
use xcore::ScalarValue;

/// The language `regex`.
pub struct RegexLanguage;

impl PathLanguage for RegexLanguage {
    fn language(&self) -> &'static str {
        "regex"
    }

    fn compile(&self, expression: &str) -> Result<Box<dyn CompiledExpression>, ContractError> {
        Ok(Box::new(Compiled {
            pattern: Pattern::parse(expression)?,
            expression: expression.to_string(),
        }))
    }
}

/// A pattern compiled, with the text a refusal names.
struct Compiled {
    pattern: Pattern,
    expression: String,
}

impl CompiledExpression for Compiled {
    fn read(&self, content: &Content<'_>) -> Result<Option<ScalarValue>, ContractError> {
        let text = content.text()?;
        Ok(self
            .pattern
            .find(text)
            .map(|range| ScalarValue::Text(text[range].to_string())))
    }

    /// Replace the span the pattern names in the current text with the
    /// value's text. Later writes see earlier ones. Null and binary have no
    /// text and are refused rather than guessed at.
    fn write(&self, rewriting: &mut Rewriting, value: ScalarValue) -> Result<(), ContractError> {
        let replacement = value
            .text()
            .ok_or_else(|| ContractError::new("the value has no text form"))?;
        let text = rewriting.form_mut::<String>()?;
        let range = self.pattern.find(text).ok_or_else(|| {
            ContractError::new(format!(
                "{:?} matches nothing to write over",
                self.expression
            ))
        })?;
        text.replace_range(range, &replacement);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contract::fixture::stream;
    use stream::Stream;
    use xcore::StreamId;

    const RECORD: &str = "MSH|^~\\&|LAB|ORDER=A1;STATUS=open;QTY=2\r\n";

    fn compiled(pattern: &str) -> Result<Box<dyn CompiledExpression>, ContractError> {
        RegexLanguage.compile(pattern)
    }

    #[test]
    fn reads_the_first_match_a_group_or_a_named_group() {
        let record = stream(RECORD);
        let content = Content::of(&record);
        let read = |p: &str| compiled(p)?.read(&content);
        assert_eq!(
            read(r"[A-Z]+\|").expect("reads"),
            Some(ScalarValue::Text("MSH|".into()))
        );
        assert_eq!(
            read(r"ORDER=(\w+);").expect("reads"),
            Some(ScalarValue::Text("A1".into()))
        );
        assert_eq!(
            read(r"(?<key>STATUS)=(?<state>\w+)#state").expect("reads"),
            Some(ScalarValue::Text("open".into()))
        );
        assert_eq!(read(r"PRICE=(\d+)").expect("reads"), None);
        assert!(read(r"(broken").is_err());
        assert!(read(r"(?<a>x)#nope").is_err());
    }

    #[test]
    fn rewrites_the_span_into_a_new_stream_with_the_given_id() {
        let mut rewriting = Rewriting::of(&stream(RECORD), StreamId::new(2));
        let mut write =
            |p: &str, v: ScalarValue| compiled(p).and_then(|c| c.write(&mut rewriting, v));
        write(r"STATUS=(\w+)", ScalarValue::Text("closed".into())).expect("writes");
        write(r"QTY=(?<n>\d+)#n", ScalarValue::Integer(30)).expect("writes");
        write(r"\|LAB\|", ScalarValue::Bool(true)).expect("writes");
        assert!(write(r"PRICE=(\d+)", ScalarValue::Integer(1)).is_err());
        assert!(write(r"QTY=(\d+)", ScalarValue::Null).is_err());
        assert!(write(r"QTY=(\d+)", ScalarValue::Binary(vec![1])).is_err());
        let out = rewriting.finish().expect("finishes");
        assert_eq!(out.id(), StreamId::new(2));
        assert_eq!(out.media_type(), Some("text/plain"));
        assert_eq!(
            out.bytes(),
            b"MSH|^~\\&trueORDER=A1;STATUS=closed;QTY=30\r\n"
        );
    }

    #[test]
    fn a_stream_that_is_not_text_is_refused() {
        let bytes = Stream::new(StreamId::new(1), vec![0xff, 0xfe, b'a'], None);
        let any = compiled(".").expect("compiles");
        assert!(any.read(&Content::of(&bytes)).is_err());
        assert!(
            any.write(
                &mut Rewriting::of(&bytes, StreamId::new(1)),
                ScalarValue::Null
            )
            .is_err()
        );
        assert!(
            any.write(
                &mut Rewriting::of(&bytes, StreamId::new(1)),
                ScalarValue::Text("x".into())
            )
            .is_err()
        );
    }
}
