//! HTML output rules, as React DOM's server renderer (Fizz) applies them.

use crate::input::Primitive;
use crate::js::{BLOCKED_URL, is_javascript_url, string_to_number};
use tsx2rs_ir::attribute::Kind;

/// React's text separator between adjacent text nodes (`textSeparator` in
/// `react-dom-server`, 19.3.1).
pub(crate) const TEXT_SEPARATOR: &[u8] = b"<!-- -->";

/// Escapes text or an attribute value like React DOM's `escapeTextForBrowser` (19.3.1):
/// `"` `&` `'` `<` `>` become `&quot;` `&amp;` `&#x27;` `&lt;` `&gt;`.
pub(crate) fn escape(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let replacement: &[u8] = match b {
            b'"' => b"&quot;",
            b'&' => b"&amp;",
            b'\'' => b"&#x27;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            _ => continue,
        };
        out.extend_from_slice(&bytes[start..i]);
        out.extend_from_slice(replacement);
        start = i + 1;
    }
    out.extend_from_slice(&bytes[start..]);
}

/// Writes a number as JavaScript's `String(n)` does, which is how React renders a number child.
pub(crate) fn number(out: &mut Vec<u8>, n: f64) {
    if n.is_nan() {
        out.extend_from_slice(b"NaN");
    } else if n.is_infinite() {
        out.extend_from_slice(if n > 0.0 { b"Infinity" } else { b"-Infinity" });
    } else if n == 0.0 {
        out.push(b'0'); // String(-0) is "0"
    } else {
        let mut buffer = ryu_js::Buffer::new();
        out.extend_from_slice(buffer.format_finite(n).as_bytes());
    }
}

/// React would throw on this value (see [`attribute`]).
#[derive(Debug)]
pub(crate) struct Rejected;

/// Writes a prop as the HTML attribute ` name="value"` by React DOM's `pushAttribute` rules
/// (react-dom-server 19.3.1) for its [`Kind`], or writes nothing.
///
/// # Errors
///
/// [`Rejected`] for a bigint where React converts the value to a number (`isNaN` throws).
pub(crate) fn attribute(
    out: &mut Vec<u8>,
    name: &[u8],
    kind: Kind,
    value: Primitive<'_>,
) -> Result<(), Rejected> {
    if matches!(value, Primitive::Undefined | Primitive::Null) {
        return Ok(()); // the caller's `null != propValue`
    }
    let write = match kind {
        Kind::String => !matches!(value, Primitive::Bool(_)),
        Kind::Any => true,
        Kind::Boolean => {
            if is_truthy(value) {
                empty_attribute(out, name);
            }
            false
        }
        Kind::OverloadedBoolean => match value {
            Primitive::Bool(true) => {
                empty_attribute(out, name);
                false
            }
            Primitive::Bool(false) => false,
            _ => true,
        },
        Kind::PositiveNumber => {
            let n = to_number(value)?;
            !n.is_nan() && n >= 1.0
        }
        Kind::Number => !to_number(value)?.is_nan(),
        Kind::Url | Kind::NonEmptyUrl => {
            if let Primitive::String(url) = value {
                if !(kind == Kind::NonEmptyUrl && url.is_empty()) {
                    let url = if is_javascript_url(url) {
                        BLOCKED_URL
                    } else {
                        url
                    };
                    attribute_start(out, name);
                    escape(out, url);
                    out.push(b'"');
                }
                return Ok(());
            }
            !matches!(value, Primitive::Bool(_)) // numbers and bigints can't be `javascript:`
        }
    };
    if write {
        attribute_start(out, name);
        value_text(out, value);
        out.push(b'"');
    }
    Ok(())
}

fn attribute_start(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b' ');
    out.extend_from_slice(name);
    out.extend_from_slice(b"=\"");
}

fn empty_attribute(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b' ');
    out.extend_from_slice(name);
    out.extend_from_slice(b"=\"\"");
}

/// A value as `escapeTextForBrowser` writes it: `"" + value`, escaped.
fn value_text(out: &mut Vec<u8>, value: Primitive<'_>) {
    match value {
        Primitive::String(s) => escape(out, s),
        Primitive::Number(n) => number(out, n),
        Primitive::BigInt(digits) => out.extend_from_slice(digits.as_bytes()),
        Primitive::Bool(b) => out.extend_from_slice(if b { b"true" } else { b"false" }),
        Primitive::Undefined | Primitive::Null => {}
    }
}

/// JavaScript truthiness.
fn is_truthy(value: Primitive<'_>) -> bool {
    match value {
        Primitive::Bool(b) => b,
        Primitive::Number(n) => n != 0.0 && !n.is_nan(),
        Primitive::String(s) => !s.is_empty(),
        Primitive::BigInt(digits) => digits != "0",
        Primitive::Undefined | Primitive::Null => false,
    }
}

/// JavaScript's `Number(value)`, as `isNaN` and `1 <= value` convert; a bigint throws.
fn to_number(value: Primitive<'_>) -> Result<f64, Rejected> {
    Ok(match value {
        Primitive::Bool(b) => f64::from(u8::from(b)),
        Primitive::Number(n) => n,
        Primitive::String(s) => string_to_number(s),
        Primitive::Null => 0.0,
        Primitive::Undefined => f64::NAN,
        Primitive::BigInt(_) => return Err(Rejected),
    })
}

#[cfg(test)]
mod tests {
    use super::{escape, number};

    fn escaped(s: &str) -> String {
        let mut out = Vec::new();
        escape(&mut out, s);
        String::from_utf8(out).unwrap()
    }

    fn formatted(n: f64) -> String {
        let mut out = Vec::new();
        number(&mut out, n);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn escapes_the_five_characters_react_escapes() {
        assert_eq!(
            escaped(r#"a "b" & 'c' <d> é"#),
            "a &quot;b&quot; &amp; &#x27;c&#x27; &lt;d&gt; é"
        );
        assert_eq!(escaped(""), "");
        assert_eq!(escaped("plain"), "plain");
        assert_eq!(escaped("<<"), "&lt;&lt;");
    }

    #[test]
    fn formats_numbers_like_javascript() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (20.0, "20"),
            (-3.25, "-3.25"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e-7, "1e-7"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for (n, expected) in cases {
            assert_eq!(formatted(n), expected, "{n:?}");
        }
    }
}
