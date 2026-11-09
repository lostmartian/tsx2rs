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
//!
//! [`outline`] reads the value once, without building a tree: a row is its source span with the
//! deferred values and references put in, so most of it is copied in bulk. A deferred value is
//! skipped where it is and read when its row's turn comes (ADR-0030).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::render::Mark;

#[cfg(test)]
mod reference;

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

/// The rows of `flight`, a value the engine rendered, with the marks recorded while rendering it
/// (byte offset where the marked value starts, in the order the render recorded them).
///
/// # Errors
///
/// [`OutlineError`] if the value can't be split as React would.
pub fn outline(flight: &[u8], marks: &[(usize, Mark)]) -> Result<Vec<Vec<u8>>, OutlineError> {
    // The render records marks in order; anything else is put in order (stably: of two identities
    // at one place, the later counts).
    let sorted;
    let marks = if marks.is_sorted_by_key(|&(at, _)| at) {
        marks
    } else {
        let mut copy = marks.to_vec();
        copy.sort_by_key(|&(at, _)| at);
        sorted = copy;
        &sorted
    };
    let mut once = HashSet::new();
    let mut repeated = HashSet::new();
    for &(_, mark) in marks {
        if let Mark::Identity(identity) = mark
            && !once.insert(identity)
        {
            repeated.insert(identity);
        }
    }
    let mut outliner = Outliner {
        src: flight,
        marks,
        repeated,
        written: HashMap::new(),
        rows: vec![Vec::new()],
        queue: VecDeque::new(),
        at: 0,
        mark: 0,
        size: 0,
        flushed: 0,
        out: Vec::new(),
        row: 0,
    };
    outliner.task(0, flight.len(), 0)?;
    while let Some((start, end, row)) = outliner.queue.pop_front() {
        outliner.task(start, end, row)?;
    }
    Ok(outliner.rows)
}

struct Outliner<'a> {
    src: &'a [u8],
    marks: &'a [(usize, Mark)],
    /// The identities that occur more than once: only those are ever referred to.
    repeated: HashSet<u32>,
    /// Where each repeated identity was first written: its reference.
    written: HashMap<u32, String>,
    rows: Vec<Vec<u8>>,
    /// Deferred values (their spans) and their row ids, in creation order.
    queue: VecDeque<(usize, usize, usize)>,
    // The task being written:
    /// Where reading is.
    at: usize,
    /// The next mark not yet reached.
    mark: usize,
    /// React's `serializedSize` so far.
    size: usize,
    /// The source up to here is in `out` already (or replaced).
    flushed: usize,
    out: Vec<u8>,
    row: usize,
}

impl Outliner<'_> {
    /// A task: the value at `start..end` as its model, counting from 0, its path the row's.
    fn task(&mut self, start: usize, end: usize, row: usize) -> Result<(), OutlineError> {
        self.at = start;
        self.mark = self.marks.partition_point(|&(at, _)| at < start);
        self.size = 0;
        self.flushed = start;
        self.row = row;
        self.out = Vec::with_capacity(end - start);
        // Paths are only kept while some object repeats: nothing refers to them otherwise.
        let mut path = Vec::new();
        let path = (!self.repeated.is_empty()).then_some(&mut path);
        self.value(0, path, true)?;
        if self.at != end {
            return Err(OutlineError::Malformed(self.at));
        }
        if let Some(&(at, _)) = self.marks.get(self.mark)
            && at < end
        {
            return Err(OutlineError::Mark(at));
        }
        self.out.extend_from_slice(&self.src[self.flushed..end]);
        self.rows[row] = std::mem::take(&mut self.out);
        Ok(())
    }

    /// The marks on the value starting here: its component kind, and its identity.
    fn marks_here(&mut self) -> Result<(Option<Mark>, Option<u32>), OutlineError> {
        let mut component = None;
        let mut identity = None;
        while let Some(&(at, mark)) = self.marks.get(self.mark) {
            if at > self.at {
                break;
            }
            if at < self.at {
                return Err(OutlineError::Mark(at));
            }
            match mark {
                Mark::Identity(i) => identity = Some(i),
                // Nested components start where their caller's output does; an async
                // component's output is a row whatever is inside it.
                Mark::Async => component = Some(Mark::Async),
                Mark::Component => {
                    component.get_or_insert(Mark::Component);
                }
            }
            self.mark += 1;
        }
        Ok((component, identity))
    }

    /// Puts `with` in place of the value at `self.at` (skipped, along with its marks).
    fn replace(&mut self, with: &[u8]) -> Result<(), OutlineError> {
        let start = self.at;
        let end = self.skip(start)?;
        self.out.extend_from_slice(&self.src[self.flushed..start]);
        self.out.extend_from_slice(with);
        self.flushed = end;
        self.at = end;
        self.mark += self.marks[self.mark..].partition_point(|&(at, _)| at < end);
        Ok(())
    }

    /// Reads the value at `self.at`, reached by a key `key_length` long, at `path` (its segments
    /// from the row's value; `None` once React stops registering paths, or when nothing repeats).
    fn value(
        &mut self,
        key_length: usize,
        path: Option<&mut Vec<Segment>>,
        root: bool,
    ) -> Result<(), OutlineError> {
        self.size += key_length;
        let start = self.at;
        let (component, identity) = self.marks_here()?;
        if let Some(identity) = identity
            && self.repeated.contains(&identity)
        {
            if let Some(reference) = self.written.get(&identity) {
                // Written before: a reference to it (React doesn't count it).
                let quoted = format!("\"{reference}\"");
                return self.replace(quoted.as_bytes());
            }
            if let Some(path) = path.as_deref() {
                let reference = self.reference(path);
                self.written.insert(identity, reference);
            }
        }
        if !root {
            let deferred = match component {
                Some(Mark::Async) => true,
                Some(_) => self.size > MAX_ROW_SIZE,
                None => self.size > MAX_ROW_SIZE && self.is_element(start),
            };
            if deferred {
                let id = self.rows.len();
                self.rows.push(Vec::new());
                let end = self.skip(start)?;
                self.queue.push_back((start, end, id));
                return self.replace(format!("\"$L!{id}\"").as_bytes());
            }
        }
        match self.src.get(start) {
            Some(b'"') => {
                // React writes `$`-strings for non-strings (`"$"`, `"$undefined"`, references),
                // which it doesn't count, and escapes a string starting with `$` as `$$…`.
                let (length, first) = self.string(self.size <= MAX_ROW_SIZE)?;
                let counted = match first {
                    [Some(b'$'), Some(b'$')] => Some(length - 1),
                    [Some(b'$'), _] => None,
                    _ => Some(length),
                };
                if let Some(length) = counted {
                    if length >= TEXT_ROW_LENGTH {
                        return Err(OutlineError::LongString(length));
                    }
                    self.size += length;
                }
            }
            Some(b'[') => self.array(start, path)?,
            Some(b'{') => self.object(path)?,
            Some(b't' | b'f' | b'n' | b'-' | b'0'..=b'9') => self.at = self.literal_end(start),
            _ => return Err(OutlineError::Malformed(self.at)),
        }
        Ok(())
    }

    /// Reads the array at `start` (`self.at`): its items, at `path`.
    fn array(
        &mut self,
        start: usize,
        mut path: Option<&mut Vec<Segment>>,
    ) -> Result<(), OutlineError> {
        let element = self.is_element(start);
        self.at += 1;
        if self.src.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(());
        }
        let mut i = 0;
        loop {
            // An element's type, key and props are reached by name.
            let segment = match (element, i) {
                (true, 1) => Segment::Name("type"),
                (true, 2) => Segment::Name("key"),
                (true, 3) => Segment::Name("props"),
                _ => Segment::Index(i),
            };
            let child = self.child(path.as_deref_mut(), segment);
            let pushed = child.is_some();
            self.value(digits(i), child, false)?;
            if pushed && let Some(path) = path.as_deref_mut() {
                path.pop();
            }
            match self.src.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(OutlineError::Malformed(self.at)),
            }
            i += 1;
        }
        Ok(())
    }

    /// Reads the object at `self.at`: its properties, at `path`.
    fn object(&mut self, mut path: Option<&mut Vec<Segment>>) -> Result<(), OutlineError> {
        self.at += 1;
        if self.src.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(());
        }
        loop {
            let key_start = self.at;
            if self.src.get(self.at) != Some(&b'"') {
                return Err(OutlineError::Malformed(self.at));
            }
            // Keys are written as they are: their whole length counts.
            let (key_length, _) = self.string(self.size <= MAX_ROW_SIZE)?;
            let key_end = self.at;
            if self.src.get(self.at) != Some(&b':') {
                return Err(OutlineError::Malformed(self.at));
            }
            self.at += 1;
            // The key as written (JSON-escaped), which is how it goes into a reference.
            let segment = Segment::Key(key_start + 1, key_end - 1);
            let child = self.child(path.as_deref_mut(), segment);
            let pushed = child.is_some();
            self.value(key_length, child, false)?;
            if pushed && let Some(path) = path.as_deref_mut() {
                path.pop();
            }
            match self.src.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(OutlineError::Malformed(self.at)),
            }
        }
        Ok(())
    }

    /// The path React registers for the value at `self.at` under its parent's path and `segment`
    /// (pushed onto it): only objects and arrays, only while every key on the way is free of `:`.
    fn child<'p>(
        &self,
        path: Option<&'p mut Vec<Segment>>,
        segment: Segment,
    ) -> Option<&'p mut Vec<Segment>> {
        let path = path?;
        let object = matches!(self.src.get(self.at), Some(b'[' | b'{'));
        let colon =
            matches!(segment, Segment::Key(start, end) if self.src[start..end].contains(&b':'));
        if !object || colon {
            return None;
        }
        path.push(segment);
        Some(path)
    }

    /// A reference to what's at `path` in this row: `$!<row>:<segment>:…`.
    fn reference(&self, path: &[Segment]) -> String {
        let mut out = format!("$!{}", self.row);
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

    /// Whether an element starts at `at`: `["$", type, key, props]`, as React writes one.
    fn is_element(&self, at: usize) -> bool {
        self.src
            .get(at..at + 5)
            .is_some_and(|s| s.starts_with(br#"["$""#) && matches!(s[4], b',' | b']'))
    }

    /// Where the literal (a number, `true`, `false` or `null`) at `start` ends.
    fn literal_end(&self, start: usize) -> usize {
        self.src[start..]
            .iter()
            .position(|b| matches!(b, b',' | b']' | b'}'))
            .map_or(self.src.len(), |n| start + n)
    }

    /// Where the value at `start` ends, without reading it (it's read when its row is written).
    fn skip(&self, start: usize) -> Result<usize, OutlineError> {
        let src = self.src;
        let mut depth = 0usize;
        let mut at = start;
        loop {
            match src.get(at) {
                Some(b'"') => {
                    at = string_end(src, at)?;
                    if depth == 0 {
                        return Ok(at);
                    }
                }
                Some(b'[' | b'{') => {
                    depth += 1;
                    at += 1;
                }
                Some(b']' | b'}') => {
                    depth = depth.checked_sub(1).ok_or(OutlineError::Malformed(at))?;
                    at += 1;
                    if depth == 0 {
                        return Ok(at);
                    }
                }
                Some(_) if depth == 0 => return Ok(self.literal_end(at)),
                Some(_) => at += 1,
                None => return Err(OutlineError::Malformed(at)),
            }
        }
    }

    /// The JSON string at `self.at`: its length in UTF-16 units, and its first two characters when
    /// they're ASCII (enough to tell React's `$` strings apart). Unless `count`, a length below
    /// [`TEXT_ROW_LENGTH`] may be any length up to the exact one's (past React's limit, only
    /// whether a string is a text row still matters).
    fn string(&mut self, count: bool) -> Result<(usize, [Option<u8>; 2]), OutlineError> {
        let src = self.src;
        let start = self.at;
        self.at += 1; // the opening quote
        let mut length = 0;
        let mut first = [None, None];
        // The first two characters one by one, then the rest in runs between escapes.
        while length < 2 {
            let byte = *src.get(self.at).ok_or(OutlineError::Malformed(self.at))?;
            let (width, units, ascii) = match byte {
                b'"' => {
                    self.at += 1;
                    return Ok((length, first));
                }
                // An escape is one UTF-16 unit (a surrogate pair is written as two `\u` escapes).
                b'\\' => match src.get(self.at + 1) {
                    Some(b'u') => (6, 1, None),
                    Some(&c) => (2, 1, Some(c)),
                    None => return Err(OutlineError::Malformed(self.at)),
                },
                0x00..=0x7f => (1, 1, Some(byte)),
                0xc0..=0xdf => (2, 1, None),
                0xe0..=0xef => (3, 1, None),
                _ => (4, 2, None),
            };
            first[length] = ascii;
            self.at += width;
            length += units;
        }
        loop {
            let rest = src
                .get(self.at..)
                .ok_or(OutlineError::Malformed(src.len()))?;
            let run =
                memchr::memchr2(b'"', b'\\', rest).ok_or(OutlineError::Malformed(src.len()))?;
            // Bytes are UTF-16 units or more: counting bytes is exact enough below a text row's
            // length, and is counted again if it reaches it.
            length += if count {
                utf16_units(&rest[..run])
            } else {
                run
            };
            self.at += run;
            if src[self.at] == b'"' {
                self.at += 1;
                if !count && length >= TEXT_ROW_LENGTH {
                    self.at = start;
                    return self.string(true);
                }
                return Ok((length, first));
            }
            self.at += match src.get(self.at + 1) {
                Some(b'u') => 6,
                Some(_) => 2,
                None => return Err(OutlineError::Malformed(self.at)),
            };
            length += 1;
        }
    }
}

/// Where the JSON string starting at `start` ends (after its closing quote).
fn string_end(src: &[u8], start: usize) -> Result<usize, OutlineError> {
    let mut at = start + 1;
    loop {
        let rest = src.get(at..).ok_or(OutlineError::Malformed(src.len()))?;
        let run = memchr::memchr2(b'"', b'\\', rest).ok_or(OutlineError::Malformed(src.len()))?;
        at += run;
        if src[at] == b'"' {
            return Ok(at + 1);
        }
        at += 2; // an escape: `\"` and `\\` are the ones that matter here
    }
}

/// The length in UTF-16 units of valid UTF-8 text: one unit per character, two for one past the
/// Basic Multilingual Plane (a 4-byte sequence).
fn utf16_units(text: &[u8]) -> usize {
    text.iter()
        .map(|&b| usize::from(b & 0xc0 != 0x80) + usize::from(b >= 0xf0))
        .sum()
}

/// The length of an array index's decimal string.
fn digits(i: usize) -> usize {
    i.checked_ilog10().map_or(1, |d| d as usize + 1)
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

    #[test]
    fn splits_commerces_product_page_as_the_reference_does() {
        // The real page the instruction counts use (benches/cases/commerce).
        let program =
            tsx2rs_ir::Program::decode(include_bytes!("../benches/cases/commerce/program.bin"))
                .unwrap();
        let mut out = crate::Output::new();
        crate::render(
            &program,
            include_bytes!("../benches/cases/commerce/input.bin"),
            &mut out,
        )
        .unwrap();
        let rows = outline(&out.flight, &out.marks).unwrap();
        assert!(rows.len() > 1, "the page has deferred rows");
        assert_eq!(
            rows,
            super::reference::outline(&out.flight, &out.marks).unwrap()
        );
    }

    mod against_the_reference {
        use proptest::prelude::*;

        use super::super::{outline, reference};
        use crate::flight;
        use crate::render::Mark;

        /// A value as the engine writes Flight: strings, literals and React's own `$` strings,
        /// arrays, objects and elements.
        #[derive(Debug, Clone)]
        enum Value {
            String(String),
            Literal(&'static str),
            Array(Vec<Value>),
            Object(Vec<(String, Value)>),
            Element(String, Vec<(String, Value)>),
        }

        fn value() -> impl Strategy<Value = Value> {
            let leaf = prop_oneof![
                "[a-z$é😀\"\\\\\n:]{0,6}".prop_map(Value::String),
                // Long enough that a few of them pass React's 3200 limit.
                "[a-zé]{200,700}".prop_map(Value::String),
                // Under a text row's 1024 characters, but over it in bytes.
                "[é😀\n]{500,700}".prop_map(Value::String),
                prop_oneof![
                    Just("null"),
                    Just("12"),
                    Just("-1.5"),
                    Just("true"),
                    Just("\"$undefined\""),
                    Just("\"$L1f\""),
                    Just("\"$\""),
                ]
                .prop_map(Value::Literal),
            ];
            let key = "[a-z$:é]{0,4}";
            leaf.prop_recursive(6, 120, 6, move |inner| {
                prop_oneof![
                    prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
                    prop::collection::vec((key, inner.clone()), 0..5).prop_map(Value::Object),
                    ("[a-z]{1,3}", prop::collection::vec((key, inner), 0..4))
                        .prop_map(|(tag, props)| Value::Element(tag, props)),
                ]
            })
        }

        /// Writes `value`, recording where each value starts.
        fn write(value: &Value, out: &mut Vec<u8>, starts: &mut Vec<usize>) {
            starts.push(out.len());
            let object = |props: &[(String, Value)], out: &mut Vec<u8>, starts: &mut Vec<usize>| {
                starts.push(out.len());
                out.push(b'{');
                for (i, (key, value)) in props.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    flight::key(out, key);
                    out.push(b':');
                    write(value, out, starts);
                }
                out.push(b'}');
            };
            match value {
                Value::String(s) => flight::string(out, s),
                Value::Literal(l) => out.extend_from_slice(l.as_bytes()),
                Value::Array(items) => {
                    out.push(b'[');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(b',');
                        }
                        write(item, out, starts);
                    }
                    out.push(b']');
                }
                Value::Object(props) => {
                    starts.pop();
                    object(props, out, starts);
                }
                Value::Element(tag, props) => {
                    out.extend_from_slice(br#"["$","#);
                    flight::string(out, tag);
                    out.extend_from_slice(b",null,");
                    object(props, out, starts);
                    out.push(b']');
                }
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(2000))]
            #[test]
            fn splits_as_the_reference_does(
                value in value(),
                picks in prop::collection::vec((any::<prop::sample::Index>(), 0u8..6), 0..12),
            ) {
                let mut flight = Vec::new();
                let mut starts = Vec::new();
                write(&value, &mut flight, &mut starts);
                // Marks at value starts, in order, as the render records them; few identities,
                // so that they repeat.
                let mut marks: Vec<(usize, Mark)> = picks
                    .iter()
                    .map(|(at, kind)| {
                        let mark = match kind {
                            0 => Mark::Async,
                            1 | 2 => Mark::Component,
                            k => Mark::Identity(u32::from(*k)),
                        };
                        (starts[at.index(starts.len())], mark)
                    })
                    .collect();
                marks.sort_by_key(|&(at, _)| at);
                prop_assert_eq!(outline(&flight, &marks), reference::outline(&flight, &marks));
            }
        }
    }
}
