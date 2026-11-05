//! Flight output rules, as React's Flight server (`react-server-dom-*`, 19.3.1) serializes
//! values: `JSON.stringify` plus React's own encodings for strings starting with `$`,
//! `undefined` and special numbers.

use crate::input::Primitive;

/// Writes a Flight reference to another row (`$L15`, `$3`, `$@a`) as it is, without React's `$`
/// escape. Returns `false`, writing nothing, for anything else, so input can't inject Flight
/// syntax through it.
pub(crate) fn reference(out: &mut Vec<u8>, s: &str) -> bool {
    let hex = |d: &str| {
        !d.is_empty()
            && d.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    // `$`, an optional letter or `@`, then hex digits: `$a` is the hex reference `a`.
    let valid = s.strip_prefix('$').is_some_and(|rest| {
        hex(rest)
            || rest
                .strip_prefix(|c: char| c.is_ascii_alphabetic() || c == '@')
                .is_some_and(hex)
    });
    if valid {
        out.push(b'"');
        out.extend_from_slice(s.as_bytes());
        out.push(b'"');
    }
    valid
}

/// Writes a string value: React's `escapeStringValue` (a leading `$` becomes `$$`), then
/// `JSON.stringify`.
pub(crate) fn string(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    if s.starts_with('$') {
        out.push(b'$');
    }
    json_string_body(out, s);
    out.push(b'"');
}

/// Writes an object key: `JSON.stringify` only (React doesn't escape keys).
pub(crate) fn key(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    json_string_body(out, s);
    out.push(b'"');
}

/// The contents of a JSON string literal, as `JSON.stringify` writes them: `"` and `\` are
/// escaped, control characters become `\b \f \n \r \t` or `\u00xx`; everything else is kept.
pub(crate) fn json_string_body(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let short: &[u8] = match b {
            b'"' => b"\\\"",
            b'\\' => b"\\\\",
            0x08 => b"\\b",
            0x0c => b"\\f",
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            b'\t' => b"\\t",
            0..0x20 => {
                out.extend_from_slice(&bytes[start..i]);
                out.extend_from_slice(b"\\u00");
                out.push(HEX[usize::from(b >> 4)]);
                out.push(HEX[usize::from(b & 0xf)]);
                start = i + 1;
                continue;
            }
            _ => continue,
        };
        out.extend_from_slice(&bytes[start..i]);
        out.extend_from_slice(short);
        start = i + 1;
    }
    out.extend_from_slice(&bytes[start..]);
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Writes a number: a JSON number, or React's `"$NaN"`, `"$Infinity"`, `"$-Infinity"`, `"$-0"`.
pub(crate) fn number(out: &mut Vec<u8>, n: f64) {
    if n.is_nan() {
        out.extend_from_slice(b"\"$NaN\"");
    } else if n.is_infinite() {
        out.extend_from_slice(if n > 0.0 {
            b"\"$Infinity\""
        } else {
            b"\"$-Infinity\""
        });
    } else if n == 0.0 && n.is_sign_negative() {
        out.extend_from_slice(b"\"$-0\"");
    } else {
        crate::html::number(out, n); // JSON numbers use JavaScript's number formatting
    }
}

/// Writes a bigint from its decimal digits: React's `"$n<digits>"`.
pub(crate) fn bigint(out: &mut Vec<u8>, digits: &str) {
    out.extend_from_slice(b"\"$n");
    out.extend_from_slice(digits.as_bytes()); // digits and `-` need no JSON escaping
    out.push(b'"');
}

/// Writes a primitive value as React's Flight server serializes it.
pub(crate) fn primitive(out: &mut Vec<u8>, value: Primitive<'_>) {
    match value {
        Primitive::Undefined => out.extend_from_slice(UNDEFINED),
        Primitive::Null => out.extend_from_slice(b"null"),
        Primitive::Bool(b) => out.extend_from_slice(if b { b"true" } else { b"false" }),
        Primitive::Number(n) => number(out, n),
        Primitive::String(s) => string(out, s),
        Primitive::BigInt(digits) => bigint(out, digits),
    }
}

/// `undefined`, as React encodes it.
pub(crate) const UNDEFINED: &[u8] = b"\"$undefined\"";

#[cfg(test)]
mod tests {
    use super::{bigint, key, number, reference, string};

    fn written(f: impl Fn(&mut Vec<u8>)) -> String {
        let mut out = Vec::new();
        f(&mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn writes_references_as_they_are_and_only_references() {
        for r in ["$L15", "$3", "$@a", "$Q1f", "$a"] {
            let mut out = Vec::new();
            assert!(reference(&mut out, r), "{r}");
            assert_eq!(String::from_utf8(out).unwrap(), format!("\"{r}\""));
        }
        for bad in [
            "", "$", "$L", "L15", "$L1G", "$L1\"]", "$Lff,\"x", "$LL1", "$-1",
        ] {
            let mut out = Vec::new();
            assert!(!reference(&mut out, bad), "{bad}");
            assert_eq!(out, b"");
        }
    }

    #[test]
    fn escapes_strings_like_json_stringify_plus_react() {
        assert_eq!(written(|o| string(o, "plain")), r#""plain""#);
        assert_eq!(written(|o| string(o, "$L1")), r#""$$L1""#);
        assert_eq!(written(|o| string(o, "a$")), r#""a$""#);
        assert_eq!(written(|o| string(o, "q\"b\\")), r#""q\"b\\""#);
        assert_eq!(
            written(|o| string(o, "\u{8}\u{c}\n\r\t\u{1}\u{1f}")),
            r#""\b\f\n\r\t\u0001\u001f""#
        );
        assert_eq!(
            written(|o| string(o, "é ✓ \u{2028} <>&")),
            "\"é ✓ \u{2028} <>&\""
        );
    }

    #[test]
    fn encodes_bigints_like_react() {
        assert_eq!(written(|o| bigint(o, "0")), r#""$n0""#);
        assert_eq!(
            written(|o| bigint(o, "-123456789012345678901234567890")),
            r#""$n-123456789012345678901234567890""#
        );
    }

    #[test]
    fn does_not_dollar_escape_keys() {
        assert_eq!(written(|o| key(o, "$k")), r#""$k""#);
    }

    #[test]
    fn encodes_numbers_like_react() {
        let cases = [
            (1.5, "1.5"),
            (0.0, "0"),
            (-0.0, r#""$-0""#),
            (f64::NAN, r#""$NaN""#),
            (f64::INFINITY, r#""$Infinity""#),
            (f64::NEG_INFINITY, r#""$-Infinity""#),
            (1e21, "1e+21"),
        ];
        for (n, expected) in cases {
            assert_eq!(written(|o| number(o, n)), expected, "{n:?}");
        }
    }
}
