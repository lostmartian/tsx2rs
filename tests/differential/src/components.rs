//! Component fixtures (ADR-0012, section 6), step 5's acceptance test: a small `.tsx` component
//! is compiled (template program, records, data functions), its data functions are run in
//! Node.js and their records encoded by the generated encoder (`compiler::encoder`) and, as a
//! cross-check, by a reference (`oracle/components.ts`), the program is rendered by the
//! engine, and the result is compared with React rendering the original component: HTML byte
//! for byte, Flight payload equal.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tsx2rs_compiler::data;
use tsx2rs_compiler::graph::{self, ModuleGraph, ModuleId};
use tsx2rs_compiler::lower::{self, Body, Slot, Test, ValueKind};
use tsx2rs_engine::{Output, render};
use tsx2rs_ir::Program;

use crate::oracle::Process;
use crate::tree::json_string;

/// A fixture compiled: the entry's program, the plan that says how to read its records, and
/// the module with the data functions appended.
#[derive(Debug)]
pub struct Compiled {
    /// The page's template program.
    pub program: Program,
    /// The records' shapes, as `oracle/components.ts` reads them (JSON).
    pub plan: String,
    /// The fixture's source with its data functions.
    pub data: String,
    /// The generated encoder (ADR-0016).
    pub encoder: String,
    /// The data functions the encoder's `data` array holds, by name, in order.
    pub callees: Vec<String>,
}

/// Compiles a fixture: `source` is the page's module (`app/page.tsx`), its default export the
/// page.
///
/// # Errors
///
/// What failed: a diagnostic, or a component that doesn't lower.
pub fn compile(source: &str) -> Result<Compiled, String> {
    let dir = Scratch::new(source)?;
    let graph = graph::build(&dir.0, Path::new("app/page.tsx")).map_err(|e| e.to_string())?;
    if let Some(d) = graph.diagnostics.first() {
        return Err(format!("{}: {}", d.code, d.message));
    }
    let module = &graph.modules[0];
    let entry = module
        .components
        .iter()
        .position(|c| c.default_export)
        .ok_or("the fixture has no default export")?;
    let lowered = lower::linked(&graph, ModuleId(0), entry).map_err(|e| e.to_string())?;
    // Every component's data function, from its own record.
    let own: Vec<_> = (0..module.components.len())
        .map(|i| lower::linked(&graph, ModuleId(0), i).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let pairs: Vec<_> = module
        .components
        .iter()
        .zip(&own)
        .map(|(c, l)| (c, l.record.as_slice()))
        .collect();
    let data = format!("{source}{}", data::functions(source, &pairs));
    let plan = Plan { graph: &graph };
    let mut json = String::from(r#"{"entry":"#);
    json_string(&mut json, &data::name(&module.components[entry]));
    json.push_str(r#","record":"#);
    plan.record(&mut json, &lowered.record, true);
    json.push_str(r#","templates":["#);
    for (i, record) in lowered.templates.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        plan.record(&mut json, record, true);
    }
    json.push_str("]}");
    let encoder = tsx2rs_compiler::encoder::generate(&lowered);
    let callees = encoder
        .callees
        .iter()
        .map(|(m, i)| data::name(&graph.modules[m.0].components[*i]))
        .collect();
    Ok(Compiled {
        program: lowered.program,
        plan: json,
        data,
        encoder: encoder.source,
        callees,
    })
}

/// Writes record shapes as `oracle/components.ts`'s `Slot` type.
struct Plan<'g> {
    graph: &'g ModuleGraph,
}

impl Plan<'_> {
    /// `root`: the record of a template's root, where a component element or a conditional is
    /// the component's own output (React passes its key path on).
    fn record(&self, out: &mut String, record: &[Slot], root: bool) {
        out.push('[');
        let own_output = root && matches!(record, [Slot::Call { .. } | Slot::Branch { .. }]);
        for (i, slot) in record.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            self.slot(out, slot, own_output);
        }
        out.push(']');
    }

    fn slot(&self, out: &mut String, slot: &Slot, root: bool) {
        match slot {
            Slot::Value { kind, .. } => {
                let kind = match kind {
                    ValueKind::Prop => "prop",
                    ValueKind::Style => "style",
                    ValueKind::FlightOnly => "flight",
                    ValueKind::Child => "child",
                };
                let _ = write!(out, r#"{{"v":"{kind}"}}"#);
            }
            Slot::Props { .. } => out.push_str(r#"{"p":1}"#),
            Slot::Branch {
                test,
                then,
                otherwise,
                ..
            } => {
                let test = match test {
                    Test::Truthy => "truthy",
                    Test::NotNullish => "notnull",
                    Test::Present => "present",
                };
                let _ = write!(out, r#"{{"b":"{test}","root":{root},"then":"#);
                self.record(out, then, root);
                out.push_str(r#","else":"#);
                self.record(out, otherwise, root);
                out.push('}');
            }
            Slot::List { item, .. } => {
                out.push_str(r#"{"l":"#);
                self.slot(out, item, false);
                out.push('}');
            }
            Slot::Returns(returns) => {
                out.push_str(r#"{"r":["#);
                for (i, (_, record)) in returns.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    self.record(out, record, true);
                }
                out.push_str("]}");
            }
            Slot::Keyed {
                root: own, record, ..
            } => {
                let _ = write!(out, r#"{{"k":{own},"body":"#);
                self.record(out, record, false);
                out.push('}');
            }
            Slot::RootWrap => out.push_str(r#"{"rw":1}"#),
            Slot::Call {
                module,
                component,
                slots,
                body,
                ..
            } => {
                let callee = &self.graph.modules[module.0].components[*component];
                out.push_str(r#"{"c":"#);
                json_string(out, &data::name(callee));
                let _ = write!(out, r#","root":{root},"#);
                match body {
                    Body::Inline(record) => {
                        out.push_str(r#""body":"#);
                        self.record(out, record, true);
                    }
                    Body::Template(t) => {
                        let _ = write!(out, r#""t":{t}"#);
                    }
                }
                out.push_str(r#","slots":{"#);
                for (i, (prop, record)) in slots.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    json_string(out, prop);
                    out.push(':');
                    self.record(out, record, false);
                }
                out.push_str("}}");
            }
            Slot::Placed { record, .. } => {
                out.push_str(r#"{"pl":"#);
                self.record(out, record, false);
                out.push('}');
            }
        }
    }
}

/// A fresh folder holding a fixture as `app/page.tsx`, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(source: &str) -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tsx2rs-fixture-{}-{n}", std::process::id()));
        let io = |e: std::io::Error| e.to_string();
        std::fs::create_dir_all(dir.join("app")).map_err(io)?;
        std::fs::write(dir.join("app/page.tsx"), source).map_err(io)?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The two Node.js processes of `oracle/components.ts`.
#[derive(Debug)]
pub struct Fixtures {
    html: Process,
    flight: Process,
}

impl Fixtures {
    /// Starts both processes.
    ///
    /// # Errors
    ///
    /// If `node` can't be started.
    pub fn start() -> std::io::Result<Self> {
        Ok(Self {
            html: Process::spawn_script("oracle/components.ts", "html")?,
            flight: Process::spawn_script("oracle/components.ts", "flight")?,
        })
    }

    /// Compiles the fixture `source` and checks every one of its cases against React; returns
    /// how many there were.
    ///
    /// # Errors
    ///
    /// What failed, for the first case that did.
    pub fn check(&mut self, source: &str) -> Result<usize, String> {
        let compiled = compile(source)?;
        let mut request = String::from(r#"{"op":"cases","source":"#);
        json_string(&mut request, source);
        request.push('}');
        let cases = self.ask_html(&request)?;
        let cases: usize = field(&cases, "cases")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| format!("no cases: {cases}"))?;
        for case in 0..cases {
            self.case(source, &compiled, case)
                .map_err(|e| format!("case {case}: {e}"))?;
        }
        Ok(cases)
    }

    fn case(&mut self, source: &str, compiled: &Compiled, case: usize) -> Result<(), String> {
        let mut request = String::from(r#"{"op":"input","source":"#);
        json_string(&mut request, source);
        request.push_str(r#","data":"#);
        json_string(&mut request, &compiled.data);
        request.push_str(r#","encoder":"#);
        json_string(&mut request, &compiled.encoder);
        request.push_str(r#","callees":["#);
        for (i, name) in compiled.callees.iter().enumerate() {
            if i > 0 {
                request.push(',');
            }
            json_string(&mut request, name);
        }
        let _ = write!(request, r#"],"plan":{},"case":{case}}}"#, compiled.plan);
        let response = self.ask_html(&request)?;
        let hex = field(&response, "input")
            .map(|h| h.trim_matches('"').to_owned())
            .ok_or_else(|| format!("running the data functions: {response}"))?;
        let input = decode_hex(&hex).ok_or("bad hex")?;
        let mut output = Output::new();
        render(&compiled.program, &input, &mut output)
            .map_err(|e| format!("the engine failed: {e:?}"))?;
        if !output.head.is_empty() {
            return Err("the page hoists into <head>, which fixtures don't compare yet".into());
        }
        let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string());
        let (html, flight, hints) = (
            text(&output.html)?,
            text(&output.flight)?,
            text(output.hints.rows())?,
        );
        let mut request = String::from(r#"{"op":"flight","source":"#);
        json_string(&mut request, source);
        let _ = write!(request, r#","case":{case}}}"#);
        let response = self
            .flight
            .ask(&request)
            .map_err(|e| format!("oracle I/O: {e}"))?;
        let payload =
            field(&response, "payload").ok_or_else(|| format!("React's Flight: {response}"))?;
        let mut request = String::from(r#"{"op":"judge","source":"#);
        json_string(&mut request, source);
        let _ = write!(request, r#","case":{case},"engineHtml":"#);
        json_string(&mut request, &html);
        request.push_str(r#","engineFlight":"#);
        json_string(&mut request, &flight);
        request.push_str(r#","engineHints":"#);
        json_string(&mut request, &hints);
        let _ = write!(request, r#","reactPayload":{payload}}}"#);
        let verdict = self.ask_html(&request)?;
        if verdict == r#"{"ok":true}"# {
            Ok(())
        } else {
            Err(verdict)
        }
    }

    fn ask_html(&mut self, request: &str) -> Result<String, String> {
        self.html
            .ask(request)
            .map_err(|e| format!("oracle I/O: {e}"))
    }
}

/// The raw JSON value of a one-field response `{"name":value}`, or `None` (an error response).
fn field<'r>(response: &'r str, name: &str) -> Option<&'r str> {
    response
        .strip_prefix(&format!(r#"{{"{name}":"#))
        .and_then(|rest| rest.strip_suffix('}'))
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}
