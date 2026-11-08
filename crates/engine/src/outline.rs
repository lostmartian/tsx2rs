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
//!
//! React also writes an object it has written before (the same object, by identity) as a
//! reference to where it first wrote it (ADR-0026): `"$<row>:<path>"`, the path from the row's
//! value through each property, index, or an element's `type`, `key` or `props`. Each object is
//! registered with its path as React's walk reaches it, if its parent was (a key with a `:` breaks
//! the chain). The encoder tags the objects that may repeat with an identity
//! (`tsx2rs_ir::input::tag::IDENTITY`); the first occurrence React's walk reaches is written, and
//! later ones as `"$!<index>:<path>"`.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::render::Mark;

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

/// One step of a path React registers: an array index, an element's `type`/`key`/`props`, or an
/// object key (its span in the source, as written).
#[derive(Clone, Copy)]
enum Segment {
    Index(usize),
    Name(&'static str),
    Key(usize, usize),
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
pub fn outline(flight: &[u8], marks: &[(usize, Mark)]) -> Result<Vec<Vec<u8>>, OutlineError> {
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
    let mut identities = HashMap::new();
    let mut once = HashSet::new();
    let mut repeated = HashSet::new();
    for &(at, mark) in marks {
        let node = *starts.get(&at).ok_or(OutlineError::Mark(at))?;
        match mark {
            Mark::Identity(identity) => {
                identities.insert(node, identity);
                if !once.insert(identity) {
                    repeated.insert(identity);
                }
            }
            // Nested components start where their caller's output does; an async component's
            // output is a row whatever is inside it.
            Mark::Async => {
                marked.insert(node, Mark::Async);
            }
            Mark::Component => {
                marked.entry(node).or_insert(Mark::Component);
            }
        }
    }
    let mut outliner = Outliner {
        src: flight,
        nodes: &parser.nodes,
        marked,
        repeated,
        identities,
        written: HashMap::new(),
        rows: vec![Vec::new()],
        queue: VecDeque::new(),
    };
    let mut row = Vec::new();
    outliner.task(root, 0, &mut row)?;
    outliner.rows[0] = row;
    while let Some((node, id)) = outliner.queue.pop_front() {
        let mut row = Vec::new();
        outliner.task(node, id, &mut row)?;
        outliner.rows[id] = row;
    }
    Ok(outliner.rows)
}

struct Outliner<'a> {
    src: &'a [u8],
    nodes: &'a [Node],
    marked: HashMap<usize, Mark>,
    /// Identified objects (node → identity).
    identities: HashMap<usize, u32>,
    /// The identities that occur more than once: only those are ever referred to.
    repeated: HashSet<u32>,
    /// Where each repeated identity was first written: its reference.
    written: HashMap<u32, String>,
    rows: Vec<Vec<u8>>,
    /// Deferred values and their row ids, in creation order.
    queue: VecDeque<(usize, usize)>,
}

impl Outliner<'_> {
    /// A task: `node` as its model, counting from 0, its path the row's (`$!<row>`).
    fn task(&mut self, node: usize, row: usize, out: &mut Vec<u8>) -> Result<(), OutlineError> {
        let mut size = 0;
        // Paths are only kept while some object repeats: nothing refers to them otherwise.
        let mut path = Vec::new();
        let path = (!self.repeated.is_empty()).then_some(&mut path);
        self.write(node, 0, &mut size, out, path, row, true)
    }

    /// Writes `node`, reached by a key `key_length` long, at `path` (its segments from the row's
    /// value; `None` once React stops registering paths, or when nothing repeats).
    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        node: usize,
        key_length: usize,
        size: &mut usize,
        out: &mut Vec<u8>,
        mut path: Option<&mut Vec<Segment>>,
        row: usize,
        root: bool,
    ) -> Result<(), OutlineError> {
        *size += key_length;
        let nodes = self.nodes;
        let n = &nodes[node];
        if let Some(&identity) = self.identities.get(&node)
            && self.repeated.contains(&identity)
        {
            if let Some(reference) = self.written.get(&identity) {
                // Written before: a reference to it (React doesn't count it).
                out.push(b'"');
                out.extend_from_slice(reference.as_bytes());
                out.push(b'"');
                return Ok(());
            }
            if let Some(path) = path.as_deref() {
                let reference = self.reference(row, path);
                self.written.insert(identity, reference);
            }
        }
        if !root {
            let element = self.is_element(node);
            let deferred = match self.marked.get(&node) {
                Some(Mark::Async) => true,
                Some(Mark::Component) => *size > MAX_ROW_SIZE,
                _ => element && *size > MAX_ROW_SIZE,
            };
            if deferred {
                let id = self.rows.len();
                self.rows.push(Vec::new());
                self.queue.push_back((node, id));
                out.extend_from_slice(b"\"$L!");
                out.extend_from_slice(id.to_string().as_bytes());
                out.push(b'"');
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
                let element = self.is_element(node);
                out.push(b'[');
                for (i, &item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    // An element's type, key and props are reached by name.
                    let segment = match (element, i) {
                        (true, 1) => Segment::Name("type"),
                        (true, 2) => Segment::Name("key"),
                        (true, 3) => Segment::Name("props"),
                        _ => Segment::Index(i),
                    };
                    let child = self.child(path.as_deref_mut(), item, segment);
                    let pushed = child.is_some();
                    self.write(item, digits(i), size, out, child, row, false)?;
                    if pushed && let Some(path) = path.as_deref_mut() {
                        path.pop();
                    }
                }
                out.push(b']');
            }
            Kind::Object(props) => {
                out.push(b'{');
                for (i, &((key_start, key_end), key_length, value)) in props.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    out.extend_from_slice(&self.src[key_start..key_end]);
                    out.push(b':');
                    // The key as written (JSON-escaped), which is how it goes into a reference.
                    let segment = Segment::Key(key_start + 1, key_end - 1);
                    let child = self.child(path.as_deref_mut(), value, segment);
                    let pushed = child.is_some();
                    self.write(value, key_length, size, out, child, row, false)?;
                    if pushed && let Some(path) = path.as_deref_mut() {
                        path.pop();
                    }
                }
                out.push(b'}');
            }
        }
        Ok(())
    }

    /// The path React registers for `node` under its parent's path and `segment` (pushed onto
    /// it): only objects and arrays, only while every key on the way is free of `:`.
    fn child<'p>(
        &self,
        path: Option<&'p mut Vec<Segment>>,
        node: usize,
        segment: Segment,
    ) -> Option<&'p mut Vec<Segment>> {
        let path = path?;
        let object = matches!(self.nodes[node].kind, Kind::Array(_) | Kind::Object(_));
        let colon =
            matches!(segment, Segment::Key(start, end) if self.src[start..end].contains(&b':'));
        if !object || colon {
            return None;
        }
        path.push(segment);
        Some(path)
    }

    /// A reference to what's at `path` in row `row`: `$!<row>:<segment>:…`.
    fn reference(&self, row: usize, path: &[Segment]) -> String {
        let mut out = format!("$!{row}");
        for segment in path {
            out.push(':');
            match *segment {
                Segment::Index(i) => out.push_str(&i.to_string()),
                Segment::Name(name) => out.push_str(name),
                Segment::Key(start, end) => {
                    out.push_str(&String::from_utf8_lossy(&self.src[start..end]));
                }
            }
        }
        out
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
    use crate::render::Mark;

    fn rows(flight: &str, marks: &[(usize, Mark)]) -> Vec<String> {
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
            rows(flight, &[(at, Mark::Async), (at, Mark::Component)]),
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
    fn writes_an_object_written_before_as_a_reference_to_its_path() {
        // The same object (identity 7) as `a` in an element's props, then again under `b`, then
        // under a key with a `:`, where React stops registering paths.
        let flight = r#"[["$","p",null,{"a":{"x":1}}],{"b":{"x":1},"c:d":{"y":[]}},{"y":[]}]"#;
        let first = flight.find(r#"{"x":1}"#).unwrap();
        let second = flight.rfind(r#"{"x":1}"#).unwrap();
        let third = flight.find(r#"{"y":[]}"#).unwrap();
        let fourth = flight.rfind(r#"{"y":[]}"#).unwrap();
        let marks = [
            (first, Mark::Identity(7)),
            (second, Mark::Identity(7)),
            (third, Mark::Identity(8)),
            (fourth, Mark::Identity(8)),
        ];
        assert_eq!(
            rows(flight, &marks),
            [concat!(
                r#"[["$","p",null,{"a":{"x":1}}],{"b":"$!0:0:props:a","c:d":{"y":[]}},"#,
                r#"{"y":[]}]"#
            )]
        );
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
            outline(b"[1]", &[(2, Mark::Component)]),
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
