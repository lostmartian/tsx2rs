//! Splitting a rendered Flight value into rows as React's Flight server does (ADR-0025).
//!
//! React serializes a task's model with `JSON.stringify` and a replacer that keeps a running
//! `serializedSize`: the length of every property name and array index it visits, and of every
//! string value (React 19.3, `react-server-dom-*` production). When it reaches an element, or a
//! server component's element, while that size is over 3200, it defers it to a new task: a row of
//! its own, referred to as `"$L<id>"`. An async server component's output is always a row of its
//! own. Each task starts counting from 0, and tasks run in the order they were created.
//!
//! The engine renders a page's Flight as one value. [`outline`] replays React's walk over it, with
//! the server components' positions the program marked ([`tsx2rs_ir::Op::Mark`]), and returns the
//! rows: the first is the page's, the others in React's task order. A row's id is its index here;
//! references to it are written `"$L!<index>"`, for the runtime to replace with the id Next.js gave
//! it (the ids interleave with other rows of Next.js's payload).

use std::collections::{HashMap, VecDeque};

use tsx2rs_ir::MarkKind;

/// React's `serializedSize` limit: past it, an element is deferred to a row of its own.
const MAX_ROW_SIZE: usize = 3200;

/// React writes a string this long (in UTF-16 units) or longer as a text row.
const TEXT_ROW_LENGTH: usize = 1024;

/// Why a value can't be split as React splits it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutlineError {
    /// The value isn't JSON as the engine writes it (a bug).
    #[error("not the engine's JSON at byte {0}")]
    Malformed(usize),
    /// A mark that isn't where a value starts (a bug in the program).
    #[error("a mark at byte {0}, where no value starts")]
    Mark(usize),
    /// A string React writes as a text row (`$T`), which the engine doesn't write yet.
    #[error("a string of {0} characters, which React sends as a text row")]
    LongString(usize),
}

/// A parsed JSON value: its source span and kind.
struct Node {
    start: usize,
    end: usize,
    kind: Kind,
}

enum Kind {
    /// A string value: its length in UTF-16 units as React counts it, or `None` for one React
    /// writes for a non-string (`"$"`, `"$undefined"`, a reference): those aren't counted.
    String(Option<usize>),
    /// A number, `true`, `false` or `null`.
    Literal,
    Array(Vec<usize>),
    /// Each property: its key's span (with quotes), its key's length in UTF-16 units, its value.
    Object(Vec<((usize, usize), usize, usize)>),
}

/// The rows of `flight`, a value the engine rendered, with the marks recorded while rendering it
/// (byte offset where the marked value starts).
///
/// # Errors
///
/// [`OutlineError`] if the value can't be split as React would.
pub fn outline(flight: &[u8], marks: &[(usize, MarkKind)]) -> Result<Vec<Vec<u8>>, OutlineError> {
    let mut parser = Parser {
        src: flight,
        at: 0,
        nodes: Vec::new(),
    };
    let root = parser.value()?;
    if parser.at != flight.len() {
        return Err(OutlineError::Malformed(parser.at));
    }
    let starts: HashMap<usize, usize> = parser
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.start, i))
        .collect();
    let mut marked = HashMap::new();
    for &(at, kind) in marks {
        // Nested components start where their caller's output does: the outermost mark wins
        // (an async component's output is a row whatever is inside it).
        let node = *starts.get(&at).ok_or(OutlineError::Mark(at))?;
        let entry = marked.entry(node).or_insert(kind);
        if kind == MarkKind::Async {
            *entry = MarkKind::Async;
        }
    }
    let mut outliner = Outliner {
        src: flight,
        nodes: &parser.nodes,
        marked,
        rows: vec![Vec::new()],
        queue: VecDeque::new(),
    };
    let mut row = Vec::new();
    outliner.task(root, &mut row)?;
    outliner.rows[0] = row;
    while let Some((node, id)) = outliner.queue.pop_front() {
        let mut row = Vec::new();
        outliner.task(node, &mut row)?;
        outliner.rows[id] = row;
    }
    Ok(outliner.rows)
}

struct Outliner<'a> {
    src: &'a [u8],
    nodes: &'a [Node],
    marked: HashMap<usize, MarkKind>,
    rows: Vec<Vec<u8>>,
    /// Deferred values and their row ids, in creation order.
    queue: VecDeque<(usize, usize)>,
}

impl Outliner<'_> {
    /// A task: `node` as its model, counting from 0.
    fn task(&mut self, node: usize, out: &mut Vec<u8>) -> Result<(), OutlineError> {
        let mut size = 0;
        self.write(node, 0, &mut size, out, true)
    }

    fn write(
        &mut self,
        node: usize,
        key_length: usize,
        size: &mut usize,
        out: &mut Vec<u8>,
        root: bool,
    ) -> Result<(), OutlineError> {
        *size += key_length;
        let n = &self.nodes[node];
        if !root {
            let element = self.is_element(node);
            let deferred = match self.marked.get(&node) {
                Some(MarkKind::Async) => true,
                Some(MarkKind::Component) => *size > MAX_ROW_SIZE,
                None => element && *size > MAX_ROW_SIZE,
            };
            if deferred {
                let id = self.rows.len();
                self.rows.push(Vec::new());
                self.queue.push_back((node, id));
                out.extend_from_slice(format!("\"$L!{id}\"").as_bytes());
                return Ok(());
            }
        }
        match &n.kind {
            Kind::String(counted) => {
                if let Some(length) = *counted {
                    if length >= TEXT_ROW_LENGTH {
                        return Err(OutlineError::LongString(length));
                    }
                    *size += length;
                }
                out.extend_from_slice(&self.src[n.start..n.end]);
            }
            Kind::Literal => out.extend_from_slice(&self.src[n.start..n.end]),
            Kind::Array(items) => {
                let items = items.clone();
                out.push(b'[');
                for (i, item) in items.into_iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    self.write(item, digits(i), size, out, false)?;
                }
                out.push(b']');
            }
            Kind::Object(props) => {
                let props = props.clone();
                out.push(b'{');
                for (i, ((key_start, key_end), key_length, value)) in props.into_iter().enumerate()
                {
                    if i > 0 {
                        out.push(b',');
                    }
                    out.extend_from_slice(&self.src[key_start..key_end]);
                    out.push(b':');
                    self.write(value, key_length, size, out, false)?;
                }
                out.push(b'}');
            }
        }
        Ok(())
    }

    /// An element: `["$", type, key, props]`, as React writes one.
    fn is_element(&self, node: usize) -> bool {
        let Kind::Array(items) = &self.nodes[node].kind else {
            return false;
        };
        items
            .first()
            .map(|&first| &self.nodes[first])
            .is_some_and(|first| &self.src[first.start..first.end] == br#""$""#)
    }
}

/// The length of an array index's decimal string.
fn digits(i: usize) -> usize {
    i.checked_ilog10().map_or(1, |d| d as usize + 1)
}

/// A JSON parser for the engine's own Flight output (no whitespace).
struct Parser<'a> {
    src: &'a [u8],
    at: usize,
    nodes: Vec<Node>,
}

impl Parser<'_> {
    fn value(&mut self) -> Result<usize, OutlineError> {
        let start = self.at;
        let kind = match self.src.get(self.at) {
            Some(b'"') => {
                // React writes `$`-strings for non-strings (`"$"`, `"$undefined"`, references),
                // which it doesn't count, and escapes a string starting with `$` as `$$…`.
                let (length, first) = self.string()?;
                Kind::String(match first {
                    [Some(b'$'), Some(b'$')] => Some(length - 1),
                    [Some(b'$'), _] => None,
                    _ => Some(length),
                })
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                if self.src.get(self.at) == Some(&b']') {
                    self.at += 1;
                } else {
                    loop {
                        items.push(self.value()?);
                        match self.src.get(self.at) {
                            Some(b',') => self.at += 1,
                            Some(b']') => {
                                self.at += 1;
                                break;
                            }
                            _ => return Err(OutlineError::Malformed(self.at)),
                        }
                    }
                }
                Kind::Array(items)
            }
            Some(b'{') => {
                self.at += 1;
                let mut props = Vec::new();
                if self.src.get(self.at) == Some(&b'}') {
                    self.at += 1;
                } else {
                    loop {
                        let key_start = self.at;
                        if self.src.get(self.at) != Some(&b'"') {
                            return Err(OutlineError::Malformed(self.at));
                        }
                        // Keys are written as they are: their whole length counts.
                        let (key_length, _) = self.string()?;
                        let key_end = self.at;
                        if self.src.get(self.at) != Some(&b':') {
                            return Err(OutlineError::Malformed(self.at));
                        }
                        self.at += 1;
                        let value = self.value()?;
                        props.push(((key_start, key_end), key_length, value));
                        match self.src.get(self.at) {
                            Some(b',') => self.at += 1,
                            Some(b'}') => {
                                self.at += 1;
                                break;
                            }
                            _ => return Err(OutlineError::Malformed(self.at)),
                        }
                    }
                }
                Kind::Object(props)
            }
            Some(b't' | b'f' | b'n' | b'-' | b'0'..=b'9') => {
                while self
                    .src
                    .get(self.at)
                    .is_some_and(|b| !matches!(b, b',' | b']' | b'}'))
                {
                    self.at += 1;
                }
                Kind::Literal
            }
            _ => return Err(OutlineError::Malformed(self.at)),
        };
        self.nodes.push(Node {
            start,
            end: self.at,
            kind,
        });
        Ok(self.nodes.len() - 1)
    }

    /// A JSON string at `self.at`: its length in UTF-16 units, and its first two characters when
    /// they're ASCII (enough to tell React's `$` strings apart).
    fn string(&mut self) -> Result<(usize, [Option<u8>; 2]), OutlineError> {
        self.at += 1; // the opening quote
        let mut length = 0;
        let mut first = [None, None];
        loop {
            let byte = *self
                .src
                .get(self.at)
                .ok_or(OutlineError::Malformed(self.at))?;
            let (width, units, ascii) = match byte {
                b'"' => {
                    self.at += 1;
                    break;
                }
                // An escape is one UTF-16 unit (a surrogate pair is written as two `\u` escapes).
                b'\\' => match self.src.get(self.at + 1) {
                    Some(b'u') => (6, 1, None),
                    Some(&c) => (2, 1, Some(c)),
                    None => return Err(OutlineError::Malformed(self.at)),
                },
                0x00..=0x7f => (1, 1, Some(byte)),
                0xc0..=0xdf => (2, 1, None),
                0xe0..=0xef => (3, 1, None),
                _ => (4, 2, None),
            };
            if length < 2 {
                first[length] = ascii;
            }
            self.at += width;
            length += units;
        }
        Ok((length, first))
    }
}

#[cfg(test)]
mod tests {
    use super::{OutlineError, outline};
    use tsx2rs_ir::MarkKind;

    fn rows(flight: &str, marks: &[(usize, MarkKind)]) -> Vec<String> {
        outline(flight.as_bytes(), marks)
            .unwrap()
            .into_iter()
            .map(|r| String::from_utf8(r).unwrap())
            .collect()
    }

    #[test]
    fn keeps_a_small_value_as_one_row() {
        let flight = r#"["$","p",null,{"children":["a",1,true,null,"$undefined"]}]"#;
        assert_eq!(rows(flight, &[]), [flight]);
    }

    #[test]
    fn makes_an_async_components_output_a_row_whatever_its_size() {
        let flight = r#"["$","main",null,{"children":["$","p",null,{}]}]"#;
        let at = flight.find(r#"["$","p""#).unwrap();
        assert_eq!(
            rows(flight, &[(at, MarkKind::Async), (at, MarkKind::Component)]),
            [
                r#"["$","main",null,{"children":"$L!1"}]"#,
                r#"["$","p",null,{}]"#
            ]
        );
    }

    #[test]
    fn counts_strings_as_react_had_them_and_not_its_own_markers() {
        // Past 3200 only if `$$x` counts one unit less than written, and `$L5` not at all.
        let long = "x".repeat(1000);
        let flight = format!(
            r#"[["$","b",null,{{}}],"{long}","{long}","{long}","$$ab","$L5",["$","i",null,{{}}]]"#
        );
        let out = rows(&flight, &[]);
        // 6 index characters + 3,000 + 3 (`$ab`) + the `b` element's 1+1+0+0 = 3,011: under.
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn rejects_what_it_cant_split() {
        let long = "x".repeat(1024);
        assert_eq!(
            outline(format!(r#"["{long}"]"#).as_bytes(), &[]),
            Err(OutlineError::LongString(1024))
        );
        assert_eq!(outline(b"[1,", &[]), Err(OutlineError::Malformed(3)));
        assert_eq!(outline(b"[1] ", &[]), Err(OutlineError::Malformed(3)));
        assert_eq!(
            outline(b"[1]", &[(2, MarkKind::Component)]),
            Err(OutlineError::Mark(2))
        );
        assert_eq!(outline(b"{\"a\" 1}", &[]), Err(OutlineError::Malformed(4)));
        assert_eq!(outline(b"x", &[]), Err(OutlineError::Malformed(0)));
    }

    #[test]
    fn reads_escapes_and_wide_characters() {
        // `\n` and `é` are one unit each; `😀` (4 UTF-8 bytes) is two; keys count whole.
        let flight = r#"{"$$k":"a\né😀","ok":{}}"#;
        assert_eq!(rows(flight, &[]), [flight]);
    }
}
