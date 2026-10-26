//! The React oracle (`oracle/oracle.ts`): two long-lived Node.js processes, one running React's
//! Flight server (under the `react-server` condition) and one running React DOM's server and the
//! comparison.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::tree::{Tree, json_string};

/// What React and the engine disagreed on, as the oracle reports it (a JSON object).
#[derive(Debug)]
pub struct Mismatch(pub String);

/// Both oracle processes.
#[derive(Debug)]
pub struct Oracle {
    flight: Process,
    html: Process,
}

#[derive(Debug)]
struct Process {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Process {
    fn spawn(mode: &str) -> std::io::Result<Self> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut command = Command::new("node");
        command.arg("--no-warnings");
        if mode == "flight" {
            command.arg("--conditions=react-server");
        }
        let mut child = command
            .arg("oracle/oracle.ts")
            .arg(mode)
            .current_dir(dir)
            .env("NODE_ENV", "production")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        Ok(Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn ask(&mut self, request: &str) -> std::io::Result<String> {
        self.stdin.write_all(request.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Err(std::io::Error::other("the oracle exited"));
        }
        Ok(line.trim_end().to_owned())
    }
}

impl Oracle {
    /// Starts both processes (run from this crate's folder, so `node_modules` resolves).
    ///
    /// # Errors
    ///
    /// If `node` can't be started.
    pub fn start() -> std::io::Result<Self> {
        Ok(Self {
            flight: Process::spawn("flight")?,
            html: Process::spawn("html")?,
        })
    }

    /// Renders `tree` with React and compares React's HTML and Flight with the engine's. The
    /// HTML is a whole document: `tree` is the body of `<html><head></head><body>`, so the
    /// engine's `engine_html` has its hoisted `<head>` content too.
    ///
    /// # Errors
    ///
    /// An I/O error talking to the oracle (`Err(Err(_))`), or what React and the engine
    /// disagree on (`Err(Ok(_))`).
    pub fn compare(
        &mut self,
        tree: &Tree,
        engine_html: &str,
        engine_flight: &str,
    ) -> Result<(), Result<Mismatch, std::io::Error>> {
        let tree_json = tree.to_json();
        let flight = self
            .flight
            .ask(&format!(r#"{{"tree":{tree_json}}}"#))
            .map_err(Err)?;
        let payload = flight
            .strip_prefix(r#"{"payload":"#)
            .and_then(|rest| rest.strip_suffix('}'))
            .ok_or_else(|| Ok(Mismatch(flight.clone())))?;
        let mut request = format!(r#"{{"tree":{tree_json},"engineHtml":"#);
        json_string(&mut request, engine_html);
        request.push_str(r#","engineFlight":"#);
        json_string(&mut request, engine_flight);
        let _ = write!(request, r#","reactPayload":{payload},"document":true}}"#);
        let verdict = self.html.ask(&request).map_err(Err)?;
        if verdict == r#"{"ok":true}"# {
            Ok(())
        } else {
            Err(Ok(Mismatch(verdict)))
        }
    }

    /// React's Flight payload for `tree`.
    ///
    /// # Errors
    ///
    /// An I/O error talking to the oracle, or a response it doesn't understand.
    pub fn react_flight(&mut self, tree: &Tree) -> std::io::Result<String> {
        let response = self
            .flight
            .ask(&format!(r#"{{"tree":{}}}"#, tree.to_json()))?;
        response
            .strip_prefix(r#"{"payload":"#)
            .and_then(|rest| rest.strip_suffix('}'))
            .map(str::to_owned)
            .ok_or_else(|| std::io::Error::other(response))
    }

    /// What React DOM's server does with `tree`, for a tree the engine returned an error for:
    /// `Ok` with its HTML (as a JSON string literal), or `Err` with the message it threw. With
    /// `document`, the tree is rendered as the body of a document, so what React hoists shows
    /// up in its `<head>`.
    ///
    /// # Errors
    ///
    /// An I/O error talking to the oracle, or a response it doesn't understand.
    pub fn react_html(
        &mut self,
        tree: &Tree,
        document: bool,
    ) -> std::io::Result<Result<String, String>> {
        let request = format!(r#"{{"tree":{},"document":{document}}}"#, tree.to_json());
        let response = self.html.ask(&request)?;
        let field = |name: &str| {
            response
                .strip_prefix(&format!(r#"{{"{name}":"#))
                .and_then(|rest| rest.strip_suffix('}'))
                .map(str::to_owned)
        };
        match (field("reactHtml"), field("reactThrew")) {
            (Some(html), _) => Ok(Ok(html)),
            (None, Some(message)) => Ok(Err(message)),
            (None, None) => Err(std::io::Error::other(response)),
        }
    }
}
