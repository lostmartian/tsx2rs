//! The input format: the bytes the generated encoder writes and the engine reads (D-016).
//!
//! Values are written in exactly the order the program reads them, with no field names:
//!
//! | Read by | Encoding |
//! |---|---|
//! | [`Op::Text`](crate::Op::Text), [`Op::RawHtml`](crate::Op::RawHtml) | string: `u32` LE byte length, then UTF-8 bytes |
//! | [`Op::Number`](crate::Op::Number) | `f64` LE |
//! | [`Op::Prop`](crate::Op::Prop) | a tagged primitive: [`tag::UNDEFINED`], [`tag::NULL`], [`tag::FALSE`], [`tag::TRUE`], [`tag::NUMBER`], [`tag::STRING`] or [`tag::BIGINT`], with its value |
//! | [`Op::Attributes`](crate::Op::Attributes) | `u32` LE count, then per prop: its name (string) and a tagged primitive |
//! | [`Op::If`](crate::Op::If) | `u8`: 0 false, 1 true |
//! | [`Op::Loop`](crate::Op::Loop) | `u32` LE count |
//! | [`Op::Island`](crate::Op::Island) | string (React's HTML), then a flags byte ([`island::TEXT_START`], [`island::TEXT_END`]) |
//! | [`Op::Value`](crate::Op::Value) | a tagged value: any [`tag`], arrays as `u32` count + values, objects as `u32` count + (string key, value) pairs |
//! | [`Op::Child`](crate::Op::Child) | a tagged node: [`tag::UNDEFINED`], [`tag::NULL`], [`tag::FALSE`], [`tag::TRUE`], [`tag::NUMBER`], [`tag::STRING`], [`tag::BIGINT`], or [`tag::ARRAY`] with `u32` count + nodes |

/// Tags for optional and generic values.
pub mod tag {
    /// `undefined`
    pub const UNDEFINED: u8 = 0;
    /// `null`
    pub const NULL: u8 = 1;
    /// `false`
    pub const FALSE: u8 = 2;
    /// `true`
    pub const TRUE: u8 = 3;
    /// A number (`f64` LE follows).
    pub const NUMBER: u8 = 4;
    /// A string (`u32` LE length + UTF-8 follows).
    pub const STRING: u8 = 5;
    /// An array (`u32` LE count + values follow).
    pub const ARRAY: u8 = 6;
    /// An object (`u32` LE count + (string key, value) pairs follow).
    pub const OBJECT: u8 = 7;
    /// A bigint (a string follows: its decimal digits, as `String(value)` writes them).
    pub const BIGINT: u8 = 8;
}

/// Flags for [`Op::Island`](crate::Op::Island): the island's HTML begins / ends with text.
pub mod island {
    /// The HTML begins with a non-empty text node.
    pub const TEXT_START: u8 = 1;
    /// The HTML ends with a non-empty text node.
    pub const TEXT_END: u8 = 2;
}

/// Writes input in the engine's format. Used by Rust callers and tests; the generated
/// JavaScript encoder writes the same bytes.
#[derive(Debug, Default)]
pub struct InputWriter {
    bytes: Vec<u8>,
}

impl InputWriter {
    /// An empty input.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes written so far.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    /// A string (for `Text`, `RawHtml`, object keys).
    ///
    /// # Panics
    ///
    /// If the string is longer than `u32::MAX` bytes.
    pub fn string(&mut self, s: &str) -> &mut Self {
        self.length(s.len());
        self.bytes.extend_from_slice(s.as_bytes());
        self
    }

    /// A number (for `Number`).
    pub fn number(&mut self, n: f64) -> &mut Self {
        self.bytes.extend_from_slice(&n.to_le_bytes());
        self
    }

    /// A condition (for `If`).
    pub fn condition(&mut self, b: bool) -> &mut Self {
        self.bytes.push(u8::from(b));
        self
    }

    /// A loop count (for `Loop`).
    ///
    /// # Panics
    ///
    /// If the count is larger than `u32::MAX`.
    pub fn count(&mut self, n: usize) -> &mut Self {
        self.length(n);
        self
    }

    /// A tag byte (for props, children and generic values).
    pub fn tag(&mut self, tag: u8) -> &mut Self {
        self.bytes.push(tag);
        self
    }

    /// An island: React's HTML and its text-edge flags.
    pub fn island(&mut self, html: &str, flags: u8) -> &mut Self {
        self.string(html);
        self.bytes.push(flags);
        self
    }

    #[expect(
        clippy::expect_used,
        reason = "a length over 4 GiB is a caller bug, documented above"
    )]
    fn length(&mut self, n: usize) {
        let n = u32::try_from(n).expect("length fits in u32");
        self.bytes.extend_from_slice(&n.to_le_bytes());
    }
}
