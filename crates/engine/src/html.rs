//! HTML output rules, as React DOM's server renderer (Fizz) applies them.

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
