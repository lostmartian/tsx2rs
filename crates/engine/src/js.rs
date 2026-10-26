//! The few JavaScript semantics React DOM's attribute rules depend on.

use crate::input::Primitive;

/// JavaScript's `"" + value` for a primitive, unescaped.
pub(crate) fn to_string(out: &mut Vec<u8>, value: Primitive<'_>) {
    match value {
        Primitive::Undefined => out.extend_from_slice(b"undefined"),
        Primitive::Null => out.extend_from_slice(b"null"),
        Primitive::Bool(b) => out.extend_from_slice(if b { b"true" } else { b"false" }),
        Primitive::Number(n) => crate::html::number(out, n),
        Primitive::String(s) | Primitive::BigInt(s) => out.extend_from_slice(s.as_bytes()),
    }
}

/// JavaScript truthiness.
pub(crate) fn is_truthy(value: Primitive<'_>) -> bool {
    match value {
        Primitive::Bool(b) => b,
        Primitive::Number(n) => n != 0.0 && !n.is_nan(),
        Primitive::String(s) => !s.is_empty(),
        Primitive::BigInt(digits) => digits != "0",
        Primitive::Undefined | Primitive::Null => false,
    }
}

/// `null` or `undefined` (`value == null`).
pub(crate) const fn is_nullish(value: Primitive<'_>) -> bool {
    matches!(value, Primitive::Undefined | Primitive::Null)
}

/// JavaScript's `Number(s)` for a string (ECMAScript `StringToNumber`): surrounding whitespace is
/// ignored, `""` is 0, decimal literals (with `Infinity`, signs, fractions and exponents),
/// `0x`/`0o`/`0b` literals (unsigned), and `NaN` for anything else.
///
/// Decimal literals are converted exactly (correctly rounded, like JavaScript). Non-decimal
/// literals beyond 2^53 may differ in their last bits; React only uses the result for `isNaN` and
/// `1 <= value`, which they never change.
pub(crate) fn string_to_number(s: &str) -> f64 {
    let s = trim(s);
    if s.is_empty() {
        return 0.0;
    }
    let bytes = s.as_bytes();
    if let [b'0', prefix, digits @ ..] = bytes {
        let radix = match prefix {
            b'x' | b'X' => Some(16),
            b'o' | b'O' => Some(8),
            b'b' | b'B' => Some(2),
            _ => None,
        };
        if let Some(radix) = radix {
            return non_decimal(digits, radix);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned == "Infinity" {
        return if s.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    if is_decimal_literal(unsigned.as_bytes()) {
        // The grammar is checked above, so Rust's parser (which also accepts `inf`, `nan` and
        // the like) only sees JavaScript decimal literals, and rounds them as JavaScript does.
        s.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn non_decimal(digits: &[u8], radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut value = 0.0_f64;
    for &d in digits {
        match char::from(d).to_digit(radix) {
            Some(d) => value = value * f64::from(radix) + f64::from(d),
            None => return f64::NAN,
        }
    }
    value
}

/// `StrUnsignedDecimalLiteral` without `Infinity`: `1`, `1.`, `.5`, `1.5e-3`, `2E+10`.
fn is_decimal_literal(s: &[u8]) -> bool {
    let digits = |s: &[u8]| s.iter().take_while(|b| b.is_ascii_digit()).count();
    let int = digits(s);
    let mut rest = &s[int..];
    let mut fraction = 0;
    if let [b'.', after @ ..] = rest {
        fraction = digits(after);
        rest = &after[fraction..];
    }
    if int == 0 && fraction == 0 {
        return false;
    }
    if let [b'e' | b'E', after @ ..] = rest {
        let after = after
            .strip_prefix(b"+")
            .or_else(|| after.strip_prefix(b"-"))
            .unwrap_or(after);
        let exponent = digits(after);
        return exponent > 0 && exponent == after.len();
    }
    rest.is_empty()
}

/// `String.prototype.trim`.
pub(crate) fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// JavaScript's `WhiteSpace` and `LineTerminator` (what `String.prototype.trim` removes).
const fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// React's `isJavaScriptProtocol`:
/// `/^[\u0000-\u001F ]*j[\r\n\t]*a[\r\n\t]*v[\r\n\t]*a[\r\n\t]*s[\r\n\t]*c[\r\n\t]*r[\r\n\t]*i[\r\n\t]*p[\r\n\t]*t[\r\n\t]*:/i`.
/// Without the `u` flag, `/i` folds only ASCII letters to ASCII letters.
pub(crate) fn is_javascript_url(url: &str) -> bool {
    let mut bytes = url
        .as_bytes()
        .iter()
        .skip_while(|&&b| b <= 0x1f || b == b' ')
        .peekable();
    for expected in b"javascript:" {
        if expected != &b'j' {
            while bytes
                .next_if(|&&b| matches!(b, b'\r' | b'\n' | b'\t'))
                .is_some()
            {}
        }
        match bytes.next() {
            Some(b) if b.eq_ignore_ascii_case(expected) => {}
            _ => return false,
        }
    }
    true
}

/// What React writes instead of a `javascript:` URL (`sanitizeURL`).
pub(crate) const BLOCKED_URL: &str =
    "javascript:throw new Error('React has blocked a javascript: URL as a security precaution.')";

#[cfg(test)]
mod tests {
    use super::{is_javascript_url, string_to_number};

    #[test]
    fn converts_strings_like_javascript_number() {
        let cases: &[(&str, f64)] = &[
            ("", 0.0),
            ("  \t\n", 0.0),
            ("12", 12.0),
            (" 12 ", 12.0),
            ("\u{FEFF}\u{3000}7\u{2028}", 7.0),
            ("+3", 3.0),
            ("-3", -3.0),
            ("1.", 1.0),
            (".5", 0.5),
            ("1.5e3", 1500.0),
            ("1E-2", 0.01),
            ("2e+1", 20.0),
            ("0x1F", 31.0),
            ("0XfF", 255.0),
            ("0o17", 15.0),
            ("0b101", 5.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("+Infinity", f64::INFINITY),
            ("0.99999999999999999", 1.0),
            ("007", 7.0),
        ];
        for &(s, expected) in cases {
            assert_eq!(string_to_number(s).to_bits(), expected.to_bits(), "{s:?}");
        }
        assert!(string_to_number("-0").is_sign_negative());
        for nan in [
            "abc",
            "1a",
            "1 2",
            ".",
            "e5",
            "1e",
            "1e+",
            "-0x10",
            "0x",
            "0xg",
            "0b2",
            "0o8",
            "inf",
            "infinity",
            "NaN",
            "nan",
            "1_000",
            "--1",
            "+-1",
            "Infinityx",
            "١",
            "1,5",
        ] {
            assert!(string_to_number(nan).is_nan(), "{nan:?}");
        }
    }

    #[test]
    fn detects_javascript_urls_like_react() {
        for url in [
            "javascript:alert(1)",
            "JavaScript:x",
            "  \u{1}\u{1f}javascript:x",
            "j\na\rv\ta\nscript:",
            "java\n\n\tscript:x",
            "javascript\t:",
        ] {
            assert!(is_javascript_url(url), "{url:?}");
        }
        for url in [
            "",
            "javascript",
            "https://example.com",
            "\u{7f}javascript:x",
            "\u{A0}javascript:x",
            "java script:x",
            "java\u{17F}cript:", // `ſ` doesn't fold to `s` without the `u` flag
            "\njavascrip:",
            "xjavascript:",
        ] {
            assert!(!is_javascript_url(url), "{url:?}");
        }
    }
}
