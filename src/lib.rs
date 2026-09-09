#![forbid(unsafe_code)]

//! The regular-expression path technology — a technology of `xmip-core-path`.
//!
//! Three things, because a path language is nothing without content to address:
//! [`RegexEngine`], the [`PathEngine`] for the language `regex`;
//! [`TextStructure`], a [`StructureReader`] over a UTF-8 text Stream; and
//! [`TextRewrite`], a [`StructureWriter`] that produces a new Stream with the
//! match replaced, as ADR-0013 asks of anything that changes content. Promote
//! reads through the first two; demote writes through the first and third;
//! route and process read.
//!
//! An expression is a pattern in the `regex` crate's syntax, optionally
//! followed by `#name` to pick a named group: `(?<id>[A-Z]\d+)#id`. A read
//! yields the first match as text — the named group when one is given, else
//! the first capture group when the pattern has one, else the whole match. A
//! write replaces that same span with the value's text, and a pattern that
//! matches nothing is refused: demote names a place, it does not invent one.
//!
//! The cost is a scan. Nothing is parsed and no document is built; the automaton
//! runs once over the text, so a promotion by pattern is cheap by construction
//! and safe against a pattern an operator got wrong.

mod pattern;

pub use pattern::Pattern;

use contract::{
    ContractDescriptor, ContractError, ContractId, StructureReader, StructureWriter,
    StructuredValue,
};
use path::{Path, PathCost, PathEngine};
use stream::Stream;
use xcore::StreamId;

/// The `regex` engine. The reader speaks patterns already, so the engine adds
/// no traversal of its own.
pub struct RegexEngine;

impl PathEngine for RegexEngine {
    fn language(&self) -> &'static str {
        "regex"
    }

    fn read(
        &self,
        reader: &dyn StructureReader,
        path: &Path,
    ) -> Result<Option<StructuredValue>, ContractError> {
        reader.read(&path.expression)
    }

    fn write(
        &self,
        writer: &mut dyn StructureWriter,
        path: &Path,
        value: StructuredValue,
    ) -> Result<(), ContractError> {
        writer.write(&path.expression, value)
    }

    /// One linear pass, no copy: the automaton stops at the first match and
    /// never holds more than its state.
    fn cost(&self, _path: &Path) -> PathCost {
        PathCost::StreamScan
    }
}

fn descriptor() -> ContractDescriptor {
    ContractDescriptor {
        id: ContractId("regex".to_string()),
        version: "1".to_string(),
        representation: "text/plain".to_string(),
    }
}

fn decode(stream: &Stream) -> Result<&str, ContractError> {
    std::str::from_utf8(stream.bytes()).map_err(|error| ContractError {
        message: format!("not UTF-8 text: {error}"),
    })
}

/// A text Stream, read by pattern.
pub struct TextStructure {
    descriptor: ContractDescriptor,
    text: String,
}

impl TextStructure {
    /// Decode `stream` once; every read is a scan after that.
    ///
    /// # Errors
    /// The Stream is not UTF-8 text.
    pub fn parse(stream: &Stream) -> Result<Self, ContractError> {
        Ok(Self {
            descriptor: descriptor(),
            text: decode(stream)?.to_string(),
        })
    }
}

impl StructureReader for TextStructure {
    fn contract(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    fn read(&self, path: &str) -> Result<Option<StructuredValue>, ContractError> {
        let span = Pattern::parse(path)?.find(&self.text);
        Ok(span.map(|range| StructuredValue::Text(self.text[range].to_string())))
    }
}

/// A text Stream being rewritten into a new one.
pub struct TextRewrite {
    descriptor: ContractDescriptor,
    id: StreamId,
    text: String,
}

impl TextRewrite {
    /// Start from `stream`; the Stream `finish` produces carries `id`.
    ///
    /// # Errors
    /// The Stream is not UTF-8 text.
    pub fn of(stream: &Stream, id: StreamId) -> Result<Self, ContractError> {
        Ok(Self {
            descriptor: descriptor(),
            id,
            text: decode(stream)?.to_string(),
        })
    }
}

impl StructureWriter for TextRewrite {
    fn contract(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    /// Replace the span `path` names in the current text with `value`'s text.
    /// Later writes see earlier ones.
    fn write(&mut self, path: &str, value: StructuredValue) -> Result<(), ContractError> {
        let replacement = text(value)?;
        let range = Pattern::parse(path)?
            .find(&self.text)
            .ok_or_else(|| ContractError {
                message: format!("{path:?} matches nothing to write over"),
            })?;
        self.text.replace_range(range, &replacement);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Stream, ContractError> {
        Ok(Stream::new(
            self.id,
            self.text.into_bytes(),
            Some(self.descriptor.representation),
        ))
    }
}

/// The text a value writes as. Null and binary have no textual form and are
/// refused rather than guessed at.
fn text(value: StructuredValue) -> Result<String, ContractError> {
    Ok(match value {
        StructuredValue::Bool(flag) => flag.to_string(),
        StructuredValue::Integer(integer) => integer.to_string(),
        StructuredValue::Decimal(decimal) => decimal.to_string(),
        StructuredValue::Text(text) => text,
        StructuredValue::Null | StructuredValue::Binary(_) => {
            return Err(ContractError {
                message: "the value has no text form".to_string(),
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(text: &str) -> Stream {
        Stream::new(StreamId::new(1), text.as_bytes().to_vec(), None)
    }

    const RECORD: &str = "MSH|^~\\&|LAB|ORDER=A1;STATUS=open;QTY=2\r\n";

    #[test]
    fn reads_the_first_match_a_group_or_a_named_group() {
        let structure = TextStructure::parse(&stream(RECORD)).expect("decodes");
        let engine = RegexEngine;
        let read = |p: &str| engine.read(&structure, &Path::new("regex", p));
        assert_eq!(
            read(r"[A-Z]+\|").expect("reads"),
            Some(StructuredValue::Text("MSH|".into()))
        );
        assert_eq!(
            read(r"ORDER=(\w+);").expect("reads"),
            Some(StructuredValue::Text("A1".into()))
        );
        assert_eq!(
            read(r"(?<key>STATUS)=(?<state>\w+)#state").expect("reads"),
            Some(StructuredValue::Text("open".into()))
        );
        assert_eq!(read(r"PRICE=(\d+)").expect("reads"), None);
        assert!(read(r"(broken").is_err());
        assert!(read(r"(?<a>x)#nope").is_err());
        assert_eq!(
            engine.cost(&Path::new("regex", r"\w+")),
            PathCost::StreamScan
        );
    }

    #[test]
    fn rewrites_the_span_into_a_new_stream_with_the_given_id() {
        let mut rewrite = TextRewrite::of(&stream(RECORD), StreamId::new(2)).expect("decodes");
        let engine = RegexEngine;
        engine
            .write(
                &mut rewrite,
                &Path::new("regex", r"STATUS=(\w+)"),
                StructuredValue::Text("closed".into()),
            )
            .expect("writes");
        engine
            .write(
                &mut rewrite,
                &Path::new("regex", r"QTY=(?<n>\d+)#n"),
                StructuredValue::Integer(30),
            )
            .expect("writes");
        engine
            .write(
                &mut rewrite,
                &Path::new("regex", r"\|LAB\|"),
                StructuredValue::Bool(true),
            )
            .expect("writes");
        assert!(
            rewrite
                .write(r"PRICE=(\d+)", StructuredValue::Integer(1))
                .is_err()
        );
        assert!(rewrite.write(r"QTY=(\d+)", StructuredValue::Null).is_err());
        assert!(
            rewrite
                .write(r"QTY=(\d+)", StructuredValue::Binary(vec![1]))
                .is_err()
        );
        let out = Box::new(rewrite).finish().expect("finishes");
        assert_eq!(out.id(), StreamId::new(2));
        assert_eq!(out.media_type(), Some("text/plain"));
        assert_eq!(
            out.bytes(),
            b"MSH|^~\\&trueORDER=A1;STATUS=closed;QTY=30\r\n"
        );
    }

    #[test]
    fn a_stream_that_is_not_text_is_refused_up_front() {
        let bytes = Stream::new(StreamId::new(1), vec![0xff, 0xfe, b'a'], None);
        assert!(TextStructure::parse(&bytes).is_err());
        assert!(TextRewrite::of(&bytes, StreamId::new(1)).is_err());
    }
}
