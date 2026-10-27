//! Resource hints in the Flight payload (ADR-0011): the `:H` rows React's Flight server emits
//! while it serializes `<img>` and `<link>` elements (`getChildFormatContext`, `preload` and
//! `preloadModule` in react-server 19.3.1), deduplicated per request.

use std::collections::HashSet;

use crate::input::Primitive;
use crate::{flight, html};

/// The hint rows a render emits, in order, each `:H<code><JSON>` and a newline. They go before
/// the payload's other rows of the same flush. Accumulates across renders until [`Hints::clear`],
/// like React's `request.hints`.
#[derive(Debug, Default)]
pub struct Hints {
    rows: Vec<u8>,
    keys: HashSet<String>,
}

/// A hint's JSON value: `JSON.stringify` of a primitive.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Json<'a>(pub(crate) Primitive<'a>);

impl Hints {
    /// The rows, in order.
    #[must_use]
    pub fn rows(&self) -> &[u8] {
        &self.rows
    }

    /// Whether no hint has been emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Forgets everything, keeping capacity: the start of a new request.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.keys.clear();
    }

    /// A preload (`:HL[href, as]` or `:HL[href, as, options]`), unless one with its key was
    /// emitted. `options` are `(name, value)` pairs in React's order; `null` and `undefined`
    /// ones are left out (`trimOptions`).
    pub(crate) fn preload(&mut self, href: &str, as_: Json<'_>, options: &[(&str, Json<'_>)]) {
        let key = preload_key(href, as_, options);
        if !self.keys.insert(key) {
            return;
        }
        let out = &mut self.rows;
        out.extend_from_slice(b":HL[");
        json_string(out, href);
        out.push(b',');
        as_.write(out);
        write_options(out, options, true);
        out.extend_from_slice(b"]\n");
    }

    /// A module preload (`:Hm"href"` or `:Hm[href, options]`), unless one for `href` was emitted.
    pub(crate) fn preload_module(&mut self, href: &str, options: &[(&str, Json<'_>)]) {
        if !self.keys.insert(format!("m|{href}")) {
            return;
        }
        let out = &mut self.rows;
        out.extend_from_slice(b":Hm");
        if options.iter().any(|(_, value)| value.is_present()) {
            out.push(b'[');
            json_string(out, href);
            write_options(out, options, true);
            out.push(b']');
        } else {
            json_string(out, href);
        }
        out.push(b'\n');
    }
}

/// React's dedupe key for a preload: `L[image][srcSet][sizes]` (or `L[image][][]href`) for
/// images, `L[as]href` otherwise.
fn preload_key(href: &str, as_: Json<'_>, options: &[(&str, Json<'_>)]) -> String {
    let option = |name| {
        options
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, value)| match value.0 {
                Primitive::String(s) => Some(s),
                _ => None,
            })
    };
    if matches!(as_.0, Primitive::String("image")) {
        match option("imageSrcSet").filter(|s| !s.is_empty()) {
            Some(set) => match option("imageSizes") {
                Some(sizes) => format!("L[image][{set}][{sizes}]"),
                None => format!("L[image][{set}]"),
            },
            None => format!("L[image][][]{href}"),
        }
    } else {
        let mut as_text = Vec::new();
        crate::js::to_string(&mut as_text, as_.0);
        format!("L[{}]{href}", String::from_utf8_lossy(&as_text))
    }
}

/// `,{"name":value,…}`, or nothing when every option is `null` or `undefined`.
fn write_options(out: &mut Vec<u8>, options: &[(&str, Json<'_>)], comma: bool) {
    let mut first = true;
    for (name, value) in options {
        if !value.is_present() {
            continue;
        }
        if first {
            if comma {
                out.push(b',');
            }
            out.push(b'{');
            first = false;
        } else {
            out.push(b',');
        }
        json_string(out, name);
        out.push(b':');
        value.write(out);
    }
    if !first {
        out.push(b'}');
    }
}

fn json_string(out: &mut Vec<u8>, s: &str) {
    flight::key(out, s); // plain `JSON.stringify`: no `$` escaping in hints
}

impl Json<'_> {
    /// Not `null` or `undefined` (`trimOptions` keeps it).
    const fn is_present(self) -> bool {
        !crate::js::is_nullish(self.0)
    }

    /// `JSON.stringify(value)` as an array item or object value: `undefined` and non-finite
    /// numbers become `null`. A bigint never gets here (React's `JSON.stringify` throws, and the
    /// renderer rejects the element first).
    fn write(self, out: &mut Vec<u8>) {
        match self.0 {
            Primitive::String(s) => json_string(out, s),
            Primitive::Number(n) if n.is_finite() => html::number(out, n),
            Primitive::Bool(b) => out.extend_from_slice(if b { b"true" } else { b"false" }),
            Primitive::Number(_)
            | Primitive::Null
            | Primitive::Undefined
            | Primitive::BigInt(_) => {
                out.extend_from_slice(b"null");
            }
        }
    }
}
