//! Component fixtures (ADR-0012 section 6, ADR-0015), step 5's acceptance test: components are
//! compiled (template program, records, data functions, the generated encoder), their data
//! functions run in Node.js and their records encoded by the generated encoder (ADR-0016) and, as
//! a cross-check, by a reference (`oracle/components.ts`), the program is rendered by the
//! engine, and the result is compared with React rendering the original component: HTML byte
//! for byte, Flight payload equal.
//!
//! A fixture is one module (`components/*.tsx`); a component of a whole app (Commerce) works the
//! same way, with its module graph.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tsx2rs_compiler::data;
use tsx2rs_compiler::graph::{self, ModuleGraph, ModuleId, ModuleKind, Target};
use tsx2rs_compiler::lower::{self, Body, Slot, Test, ValueKind};
use tsx2rs_engine::{Output, render};
use tsx2rs_ir::Program;

use crate::oracle::Process;
use crate::tree::json_string;

/// A component compiled: its program, the plan that says how to read its records, and its
/// module graph twice, as written and with the data functions appended (JSON for the oracle).
#[derive(Debug)]
pub struct Compiled {
    /// The component's template program.
    pub program: Program,
    /// The records' shapes, as `oracle/components.ts` reads them (JSON).
    pub plan: String,
    /// The server modules, as written (JSON: path → source and resolved imports).
    pub modules: String,
    /// The same modules with their data functions appended.
    pub data: String,
    /// The generated encoder (ADR-0016).
    pub encoder: String,
    /// The data functions the encoder's `data` array holds, in order (JSON:
    /// `[{module, export}]`).
    pub callees: String,
}

/// Compiles a one-module fixture: `source` is the page's module (`app/page.tsx`), its default
/// export the page.
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
    let entry = graph.modules[0]
        .components
        .iter()
        .position(|c| c.default_export)
        .ok_or("the fixture has no default export")?;
    compile_component(&graph, &dir.0, ModuleId(0), entry)
}

/// Compiles the `index`th component of `module`, in a graph built from the project at `root`.
///
/// # Errors
///
/// What failed: a source that can't be read, or a component that doesn't lower.
pub fn compile_component(
    graph: &ModuleGraph,
    root: &Path,
    module: ModuleId,
    index: usize,
) -> Result<Compiled, String> {
    let lowered = lower::linked(graph, module, index).map_err(|e| e.to_string())?;
    let (mut modules, mut data_modules) = (String::from("{"), String::from("{"));
    for (m, server) in graph
        .modules
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == ModuleKind::Server)
    {
        let source = std::fs::read_to_string(root.join(&server.path))
            .map_err(|e| format!("{}: {e}", server.path))?;
        // Every component's data function, from its own record.
        let own: Vec<_> = (0..server.components.len())
            .map(|i| lower::linked(graph, ModuleId(m), i).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let pairs: Vec<_> = server
            .components
            .iter()
            .zip(&own)
            .map(|(c, l)| (c, l.record.as_slice()))
            .collect();
        let with_data = format!("{source}{}", data::functions(&source, &pairs));
        let imports = imports(graph, ModuleId(m));
        for (out, text) in [(&mut modules, &source), (&mut data_modules, &with_data)] {
            if out.len() > 1 {
                out.push(',');
            }
            json_string(out, &server.path);
            out.push_str(r#":{"source":"#);
            json_string(out, text);
            let _ = write!(out, r#","imports":{imports}}}"#);
        }
    }
    modules.push('}');
    data_modules.push('}');
    let plan = Plan { graph };
    let mut json = String::from(r#"{"module":"#);
    json_string(&mut json, &graph.modules[module.0].path);
    json.push_str(r#","entry":"#);
    json_string(
        &mut json,
        &data::name(&graph.modules[module.0].components[index]),
    );
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
    let mut callees = String::from("[");
    for (n, (m, i)) in encoder.callees.iter().enumerate() {
        if n > 0 {
            callees.push(',');
        }
        callees.push_str(r#"{"module":"#);
        json_string(&mut callees, &graph.modules[m.0].path);
        callees.push_str(r#","export":"#);
        json_string(
            &mut callees,
            &data::name(&graph.modules[m.0].components[*i]),
        );
        callees.push('}');
    }
    callees.push(']');
    Ok(Compiled {
        program: lowered.program,
        plan: json,
        modules,
        data: data_modules,
        encoder: encoder.source,
        callees,
    })
}

/// Where a module's imports of other server modules lead (JSON: specifier → path).
fn imports(graph: &ModuleGraph, module: ModuleId) -> String {
    let mut out = String::from("{");
    for import in &graph.modules[module.0].imports {
        if let Target::Module(target) = import.target
            && graph.modules[target.0].kind == ModuleKind::Server
        {
            if out.len() > 1 {
                out.push(',');
            }
            json_string(&mut out, &import.specifier);
            out.push(':');
            json_string(&mut out, &graph.modules[target.0].path);
        }
    }
    out.push('}');
    out
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
                out.push_str(r#"{"m":"#);
                json_string(out, &self.graph.modules[module.0].path);
                out.push_str(r#","c":"#);
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
            Slot::Island { .. } => out.push_str(r#"{"i":1}"#),
            Slot::Suspense { .. } => out.push_str(r#"{"su":1}"#),
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

    /// Compiles the one-module fixture `source` and checks every one of its cases (its
    /// `export const cases`) against React; returns how many there were.
    ///
    /// # Errors
    ///
    /// What failed, for the first case that did.
    pub fn check(&mut self, source: &str) -> Result<usize, String> {
        let compiled = compile(source)?;
        let page = r#"{"module":"app/page.tsx","export":"default"}"#;
        let cases = r#"{"module":"app/page.tsx","export":"cases"}"#;
        self.check_compiled(&compiled, page, cases)
    }

    /// Checks a compiled component against React for every case: `page` names it for React
    /// (`{"module", "export"}`, JSON), and `cases` where its props come from (`{"module" |
    /// "file", "export"}`). Returns how many cases there were.
    ///
    /// # Errors
    ///
    /// What failed, for the first case that did.
    pub fn check_compiled(
        &mut self,
        compiled: &Compiled,
        page: &str,
        cases: &str,
    ) -> Result<usize, String> {
        let request = format!(
            r#"{{"op":"cases","modules":{},"cases":{cases}}}"#,
            compiled.modules
        );
        let response = self.ask_html(&request)?;
        let n: usize = field(&response, "cases")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| format!("no cases: {response}"))?;
        let subject = format!(
            r#""modules":{},"page":{page},"cases":{cases}"#,
            compiled.modules
        );
        for case in 0..n {
            self.case(compiled, &subject, case)
                .map_err(|e| format!("case {case}: {e}"))?;
        }
        Ok(n)
    }

    fn case(&mut self, compiled: &Compiled, subject: &str, case: usize) -> Result<(), String> {
        let mut request = format!(
            r#"{{"op":"input",{subject},"data":{},"encoder":"#,
            compiled.data
        );
        json_string(&mut request, &compiled.encoder);
        let _ = write!(
            request,
            r#","callees":{},"plan":{},"case":{case}}}"#,
            compiled.callees, compiled.plan
        );
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
        let request = format!(r#"{{"op":"flight",{subject},"case":{case}}}"#);
        let response = self
            .flight
            .ask(&request)
            .map_err(|e| format!("oracle I/O: {e}"))?;
        let payload =
            field(&response, "payload").ok_or_else(|| format!("React's Flight: {response}"))?;
        let mut request = format!(r#"{{"op":"judge",{subject},"case":{case},"engineHtml":"#);
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
