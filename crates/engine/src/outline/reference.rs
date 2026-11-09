//! The first `outline`: it parses the whole value into a tree, then walks it. Kept, for tests
//! only, as the reference the streaming [`super::outline`] is checked against on random values.

#![allow(clippy::pedantic, clippy::nursery)]

use super::{MAX_ROW_SIZE, OutlineError, TEXT_ROW_LENGTH};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::render::Mark;

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
pub(super) fn outline(
    flight: &[u8],
    marks: &[(usize, Mark)],
) -> Result<Vec<Vec<u8>>, OutlineError> {
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
