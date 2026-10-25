//! Embedding Flight in an HTML document: Next.js writes each Flight chunk as
//! `<script>self.__next_f.push([1,"<chunk>"])</script>`, where `<chunk>` is
//! `htmlEscapeJsonString(JSON.stringify(chunk))` without the quotes
//! (`next/dist/esm/shared/lib/htmlescape.js`, 16.4.3).

use crate::flight::json_string_body;

/// Appends the contents of the JS string literal for a raw Flight chunk: `JSON.stringify`, then
/// `&` `>` `<` U+2028 U+2029 as `\u0026` `\u003e` `\u003c` `\u2028` `\u2029`, so the script can't
/// close itself or be broken by HTML parsing.
pub fn script_string(out: &mut Vec<u8>, raw: &str) {
    let mut json = Vec::with_capacity(raw.len() + raw.len() / 8);
    json_string_body(&mut json, raw);
    let mut start = 0;
    let mut i = 0;
    while i < json.len() {
        let (replacement, width): (&[u8], usize) = match json[i] {
            b'&' => (b"\\u0026", 1),
            b'>' => (b"\\u003e", 1),
            b'<' => (b"\\u003c", 1),
            // U+2028 / U+2029 are E2 80 A8 / E2 80 A9 in UTF-8.
            0xe2 if json.get(i + 1) == Some(&0x80) && json.get(i + 2) == Some(&0xa8) => {
                (b"\\u2028", 3)
            }
            0xe2 if json.get(i + 1) == Some(&0x80) && json.get(i + 2) == Some(&0xa9) => {
                (b"\\u2029", 3)
            }
            _ => {
                i += 1;
                continue;
            }
        };
        out.extend_from_slice(&json[start..i]);
        out.extend_from_slice(replacement);
        i += width;
        start = i;
    }
    out.extend_from_slice(&json[start..]);
}

#[cfg(test)]
mod tests {
    use super::script_string;

    fn escaped(raw: &str) -> String {
        let mut out = Vec::new();
        script_string(&mut out, raw);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn escapes_like_next_js() {
        assert_eq!(
            escaped("0:[\"$\",\"div\",null,{}]\n"),
            r#"0:[\"$\",\"div\",null,{}]\n"#
        );
        assert_eq!(
            escaped("</script><b>&"),
            r"\u003c/script\u003e\u003cb\u003e\u0026"
        );
        assert_eq!(escaped("a\u{2028}b\u{2029}c"), r"a\u2028b\u2029c");
        assert_eq!(escaped("é…"), "é…"); // other multi-byte characters are kept
    }
}
