//! Embedding Flight in an HTML document: Next.js writes each Flight chunk as
//! `<script>self.__next_f.push([1,"<chunk>"])</script>`, where `<chunk>` is
//! `htmlEscapeJsonString(JSON.stringify(chunk))` without the quotes
//! (`next/dist/esm/shared/lib/htmlescape.js`, 16.4.5).

/// Appends the contents of the JS string literal for a raw Flight chunk: `JSON.stringify`, then
/// `&` `>` `<` U+2028 U+2029 as `\u0026` `\u003e` `\u003c` `\u2028` `\u2029`, so the script can't
/// close itself or be broken by HTML parsing.
pub fn script_string(out: &mut Vec<u8>, raw: &str) {
    escape(out, raw.as_bytes(), None);
}

/// [`script_string`] for one of a page's rows ([`crate::outline()`]), with the ids Next.js gave
/// the rows put in: `"$L!<n>"` becomes `"$L<ids[n]>"` and `"$!<n>:` becomes `"$<ids[n]>:`
/// (ADR-0025, ADR-0026), in one pass (ADR-0031). A row number with no id gets an empty one.
pub fn flight_script(out: &mut Vec<u8>, row: &str, ids: &[&str]) {
    escape(out, row.as_bytes(), Some(ids));
}

/// Which bytes [`escape`] looks at: everything else is copied as it is.
const SPECIAL: [bool; 256] = {
    let mut table = [false; 256];
    let mut b = 0;
    while b < 0x20 {
        table[b] = true;
        b += 1;
    }
    table[b'"' as usize] = true;
    table[b'\\' as usize] = true;
    table[b'&' as usize] = true;
    table[b'<' as usize] = true;
    table[b'>' as usize] = true;
    table[0xe2] = true; // U+2028 / U+2029 are E2 80 A8 / E2 80 A9 in UTF-8
    table
};

const HEX: &[u8; 16] = b"0123456789abcdef";

/// One pass over valid UTF-8 `s`: `JSON.stringify`'s escapes and Next.js's, and the row ids when
/// `ids` is given.
fn escape(out: &mut Vec<u8>, s: &[u8], ids: Option<&[&str]>) {
    out.reserve(s.len() + s.len() / 8);
    let mut start = 0;
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if !SPECIAL[usize::from(b)] {
            i += 1;
            continue;
        }
        let (replacement, width): (&[u8], usize) = match b {
            b'"' => {
                if let Some(ids) = ids
                    && let Some((id, end, row)) = placeholder(s, i, ids)
                {
                    out.extend_from_slice(&s[start..i]);
                    out.extend_from_slice(if row { b"\\\"$L" } else { b"\\\"$" });
                    escape(out, id.as_bytes(), None);
                    out.extend_from_slice(if row { b"\\\"" } else { b":" });
                    i = end;
                    start = end;
                    continue;
                }
                (b"\\\"", 1)
            }
            b'\\' => (b"\\\\", 1),
            0x08 => (b"\\b", 1),
            0x0c => (b"\\f", 1),
            b'\n' => (b"\\n", 1),
            b'\r' => (b"\\r", 1),
            b'\t' => (b"\\t", 1),
            0..0x20 => {
                out.extend_from_slice(&s[start..i]);
                out.extend_from_slice(b"\\u00");
                out.push(HEX[usize::from(b >> 4)]);
                out.push(HEX[usize::from(b & 0xf)]);
                i += 1;
                start = i;
                continue;
            }
            b'&' => (b"\\u0026", 1),
            b'>' => (b"\\u003e", 1),
            b'<' => (b"\\u003c", 1),
            _ => match (s.get(i + 1), s.get(i + 2)) {
                (Some(0x80), Some(0xa8)) => (b"\\u2028", 3),
                (Some(0x80), Some(0xa9)) => (b"\\u2029", 3),
                _ => {
                    i += 1;
                    continue;
                }
            },
        };
        out.extend_from_slice(&s[start..i]);
        out.extend_from_slice(replacement);
        i += width;
        start = i;
    }
    out.extend_from_slice(&s[start..]);
}

/// A row placeholder at `at` (a `"`): `"$L!<n>"` (a row, `true`) or `"$!<n>:` (a path in one),
/// with the row's id and where the placeholder ends.
fn placeholder<'a>(s: &[u8], at: usize, ids: &[&'a str]) -> Option<(&'a str, usize, bool)> {
    let rest = &s[at + 1..];
    let (row, digits_at, end) = if rest.starts_with(b"$L!") {
        (true, at + 4, b'"')
    } else if rest.starts_with(b"$!") {
        (false, at + 3, b':')
    } else {
        return None;
    };
    let digits = s[digits_at..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 || s.get(digits_at + digits) != Some(&end) {
        return None;
    }
    // A number too large for any row has no id, as in JavaScript (`ids[n] ?? ""`).
    let n = std::str::from_utf8(&s[digits_at..digits_at + digits])
        .ok()
        .and_then(|d| d.parse::<usize>().ok());
    let id = n.and_then(|n| ids.get(n)).copied().unwrap_or("");
    Some((id, digits_at + digits + 1, row))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{flight_script, script_string};
    use crate::flight::json_string_body;

    fn escaped(raw: &str) -> String {
        let mut out = Vec::new();
        script_string(&mut out, raw);
        String::from_utf8(out).unwrap()
    }

    fn scripted(row: &str, ids: &[&str]) -> String {
        let mut out = Vec::new();
        flight_script(&mut out, row, ids);
        String::from_utf8(out).unwrap()
    }

    /// The rules written the long way: `JSON.stringify`'s body, then Next.js's replacements.
    fn reference(raw: &str) -> String {
        let mut json = Vec::new();
        json_string_body(&mut json, raw);
        String::from_utf8(json)
            .unwrap()
            .replace('&', "\\u0026")
            .replace('>', "\\u003e")
            .replace('<', "\\u003c")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    }

    /// The runtime's `withIds`, written the long way.
    fn with_ids(row: &str, ids: &[&str]) -> String {
        let mut out = String::new();
        let mut rest = row;
        while let Some(at) = rest.find('"') {
            out.push_str(&rest[..at]);
            rest = &rest[at..];
            let replaced = [("\"$L!", '"', "\"$L", "\""), ("\"$!", ':', "\"$", ":")]
                .into_iter()
                .find_map(|(prefix, end, open, close)| {
                    let after = rest.strip_prefix(prefix)?;
                    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
                    (digits > 0 && after[digits..].starts_with(end)).then(|| {
                        let id = after[..digits]
                            .parse::<usize>()
                            .ok()
                            .and_then(|n| ids.get(n))
                            .copied()
                            .unwrap_or("");
                        (format!("{open}{id}{close}"), prefix.len() + digits + 1)
                    })
                });
            if let Some((text, length)) = replaced {
                out.push_str(&text);
                rest = &rest[length..];
            } else {
                out.push('"');
                rest = &rest[1..];
            }
        }
        out.push_str(rest);
        out
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
        assert_eq!(escaped("\u{1}\u{1f}\t"), r"\u0001\u001f\t");
    }

    #[test]
    fn puts_in_the_rows_ids() {
        let row = r#"["$","div",null,{"children":"$L!2","a":"$!1:props:x","b":"$$L!1"}]"#;
        assert_eq!(
            scripted(row, &["", "7", "1f"]),
            r#"[\"$\",\"div\",null,{\"children\":\"$L1f\",\"a\":\"$7:props:x\",\"b\":\"$$L!1\"}]"#
        );
        // Not placeholders: no digits, no closing quote or colon; a row with no id gets none.
        assert_eq!(
            scripted(r#"["$L!","$L!1x","$!2","$L!9"]"#, &["a"]),
            r#"[\"$L!\",\"$L!1x\",\"$!2\",\"$L\"]"#
        );
        // Without ids, placeholders stay as they are.
        assert_eq!(escaped(r#""$L!1""#), r#"\"$L!1\""#);
    }

    proptest! {
        #[test]
        fn matches_the_rules_written_the_long_way(raw in "[a-z\"\\\\$L!:0-9<>&é\u{2028}\u{2029}\u{1}\n😀]{0,40}") {
            prop_assert_eq!(escaped(&raw), reference(&raw));
            let ids = ["", "a", "1f", "<&>"];
            prop_assert_eq!(scripted(&raw, &ids), reference(&with_ids(&raw, &ids)));
        }
    }
}
