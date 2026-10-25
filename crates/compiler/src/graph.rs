//! The module graph of a route: every file the route's server components run, starting from the
//! route's entry (e.g. `app/product/[handle]/page.tsx`).
//!
//! Server modules are parsed and their imports followed. A module whose first directive is
//! `"use client"` is a **client boundary**: it is recorded, but its imports run in the browser
//! and are not followed. `"use server"` modules (server actions), npm packages, type-only imports
//! and non-code assets are recorded without being followed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParserReturn};
use oxc_resolver::{
    ResolveOptions, Resolver, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};
use oxc_span::SourceType;
use oxc_syntax::module_record::{
    ExportExportName, ExportImportName, ImportImportName, ModuleRecord,
};

use crate::component::ComponentUse;
use crate::diagnostic::{Code, Diagnostic};

/// Index into [`ModuleGraph::modules`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleId(pub usize);

/// What role a module plays for the server render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    /// Runs on the server during the render (the default). Its imports are followed.
    Server,
    /// Starts with `"use client"`: a client boundary. Its imports aren't followed.
    Client,
    /// Starts with `"use server"`: server actions, called from the client. Not followed.
    ServerActions,
}

/// One module in the graph.
#[derive(Debug)]
pub struct Module {
    /// Path relative to the project root, with `/` separators.
    pub path: String,
    /// The module's role.
    pub kind: ModuleKind,
    /// Its import and re-export statements, in source order (only for parsed server modules).
    pub imports: Vec<Import>,
    /// What it exports (only for parsed server modules).
    pub exports: Exports,
    /// The component elements in its JSX, in source order (only for parsed server modules).
    pub component_uses: Vec<ComponentUse>,
}

/// A module's exports: defined in it, or re-exported from another module.
#[derive(Debug, Default)]
pub struct Exports {
    /// Names defined in the module (`default` for the default export).
    pub local: Vec<String>,
    /// `export { a as b } from "./x"` and `export * as ns from "./x"`.
    pub reexports: Vec<Reexport>,
    /// Specifiers of `export * from "./x"`.
    pub star: Vec<String>,
}

/// A named re-export.
#[derive(Debug)]
pub struct Reexport {
    /// The name this module exports.
    pub exported: String,
    /// What it refers to in the other module (`Namespace` for `export * as ns`).
    pub imported: Imported,
    /// The other module's specifier (an entry of [`Module::imports`] has the same specifier).
    pub specifier: String,
}

/// An import or re-export statement.
#[derive(Debug)]
pub struct Import {
    /// The specifier as written (`"./tile"`, `"components/grid/tile"`, `"next/link"`).
    pub specifier: String,
    /// What it resolved to.
    pub target: Target,
    /// The bindings it introduces (empty for side-effect imports and re-exports).
    pub bindings: Vec<Binding>,
    /// `import type` / all bindings type-only: no runtime effect, not followed.
    pub type_only: bool,
}

/// Where an import leads.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// A source module in the project.
    Module(ModuleId),
    /// An npm package (not analysed; the compiler treats known packages specially).
    Package(String),
    /// A non-code file in the project (CSS, images, JSON), relative path.
    Asset(String),
    /// Didn't resolve (a diagnostic says why), or a type-only import that wasn't resolved.
    None,
}

/// A name an import binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The exported name it refers to.
    pub imported: Imported,
    /// The local name in the importing module.
    pub local: String,
    /// `import { type X }`: type-only.
    pub type_only: bool,
}

/// The exported name an import binding refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Imported {
    /// `import X from`
    Default,
    /// `import { X } from` / `import { X as Y } from`
    Named(String),
    /// `import * as X from`
    Namespace,
}

/// A route's module graph.
#[derive(Debug)]
pub struct ModuleGraph {
    /// The entry module (always `ModuleId(0)`).
    pub entry: ModuleId,
    /// All modules, in the order they were discovered (breadth-first from the entry).
    pub modules: Vec<Module>,
    /// Problems found while building the graph.
    pub diagnostics: Vec<Diagnostic>,
}

/// Why a graph couldn't be built at all.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// The entry file couldn't be read.
    #[error("can't read the entry {path}: {source}")]
    Entry {
        /// The entry path.
        path: PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },
}

/// File extensions parsed as code (in resolution order).
const CODE_EXTENSIONS: [&str; 6] = [".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs"];

/// Builds the module graph of the route whose entry is `entry` (relative to `root`).
///
/// `root` is the project root: the folder with `package.json` and, if present, `tsconfig.json`,
/// whose `baseUrl` and `paths` are used to resolve imports.
///
/// # Errors
///
/// If the entry can't be read. Every other problem is a [`Diagnostic`] in the result.
pub fn build(root: &Path, entry: &Path) -> Result<ModuleGraph, GraphError> {
    // The resolver returns real paths, so compare them with the real root (e.g. macOS's /var is
    // a symlink to /private/var).
    let given = root.join(entry);
    let entry_error = |source| GraphError::Entry {
        path: given.clone(),
        source,
    };
    let root = std::fs::canonicalize(root).map_err(entry_error)?;
    let entry_path = std::fs::canonicalize(&given).map_err(entry_error)?;
    let source = std::fs::read_to_string(&entry_path).map_err(entry_error)?;
    let mut builder = Builder::new(&root);
    let entry_id = builder.add(&entry_path);
    builder.sources.insert(entry_id.0, source);
    builder.run();
    Ok(ModuleGraph {
        entry: entry_id,
        modules: builder.modules,
        diagnostics: builder.diagnostics,
    })
}

/// An import or re-export statement before resolution.
struct Statement {
    start: u32,
    specifier: String,
    type_only: bool,
    bindings: Vec<Binding>,
    span: (u32, u32),
}

struct Builder {
    root: PathBuf,
    resolver: Resolver,
    modules: Vec<Module>,
    /// Absolute path of each module (same index as `modules`).
    paths: Vec<PathBuf>,
    by_path: HashMap<PathBuf, ModuleId>,
    /// Source of modules read ahead of parsing (the entry).
    sources: HashMap<usize, String>,
    diagnostics: Vec<Diagnostic>,
}

impl Builder {
    fn new(root: &Path) -> Self {
        let tsconfig = root.join("tsconfig.json");
        let options = ResolveOptions {
            extensions: CODE_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect(),
            condition_names: vec![
                "node".into(),
                "import".into(),
                "require".into(),
                "default".into(),
            ],
            tsconfig: tsconfig
                .is_file()
                .then_some(TsconfigDiscovery::Manual(TsconfigOptions {
                    config_file: tsconfig,
                    references: TsconfigReferences::Auto,
                })),
            ..ResolveOptions::default()
        };
        Self {
            root: root.to_path_buf(),
            resolver: Resolver::new(options),
            modules: Vec::new(),
            paths: Vec::new(),
            by_path: HashMap::new(),
            sources: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn relative(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Registers a module (kind decided when parsed).
    fn add(&mut self, path: &Path) -> ModuleId {
        if let Some(&id) = self.by_path.get(path) {
            return id;
        }
        let id = ModuleId(self.modules.len());
        self.modules.push(Module {
            path: self.relative(path),
            kind: ModuleKind::Server,
            imports: Vec::new(),
            exports: Exports::default(),
            component_uses: Vec::new(),
        });
        self.paths.push(path.to_path_buf());
        self.by_path.insert(path.to_path_buf(), id);
        id
    }

    /// Parses modules breadth-first until every reachable server module is done.
    fn run(&mut self) {
        let mut next = 0;
        while next < self.modules.len() {
            self.visit(ModuleId(next));
            next += 1;
        }
    }

    fn visit(&mut self, id: ModuleId) {
        let path = self.paths[id.0].clone();
        let file = self.modules[id.0].path.clone();
        let source = match self.sources.remove(&id.0) {
            Some(source) => source,
            None => match std::fs::read_to_string(&path) {
                Ok(source) => source,
                Err(e) => {
                    self.diagnostic(
                        Code::UnresolvedImport,
                        format!("can't read {file}: {e}"),
                        &file,
                        (0, 0),
                    );
                    return;
                }
            },
        };
        let allocator = Allocator::default();
        let source_type = SourceType::from_path(&path)
            .unwrap_or_default()
            .with_jsx(true);
        let ParserReturn {
            program,
            module_record,
            diagnostics,
            ..
        } = Parser::new(&allocator, &source, source_type).parse();
        if let Some(error) = diagnostics.iter().next() {
            let span = error.labels.first().map_or((0, 0), |l| {
                let start = l.offset();
                (start, start + l.len())
            });
            self.diagnostic(Code::ParseError, error.message.to_string(), &file, span);
            return;
        }

        let kind = match program.directives.first().map(|d| d.directive.as_str()) {
            Some("use client") => ModuleKind::Client,
            Some("use server") => ModuleKind::ServerActions,
            _ => ModuleKind::Server,
        };
        self.modules[id.0].kind = kind;
        if kind != ModuleKind::Server {
            return; // a boundary: what it imports isn't part of the server render
        }

        for dynamic in &module_record.dynamic_imports {
            let span = (dynamic.span.start, dynamic.span.end);
            self.diagnostic(
                Code::DynamicImport,
                "dynamic import() in a server module".into(),
                &file,
                span,
            );
        }

        self.modules[id.0].exports = exports(&module_record);
        self.modules[id.0].component_uses = crate::component::collect(&program);
        let statements = statements(&module_record);

        let dir = path
            .parent()
            .map_or_else(|| self.root.clone(), Path::to_path_buf);
        for Statement {
            specifier,
            type_only,
            bindings,
            span,
            ..
        } in statements
        {
            let target = if type_only {
                Target::None
            } else {
                self.resolve(&dir, &specifier, &file, span)
            };
            self.modules[id.0].imports.push(Import {
                specifier,
                target,
                bindings,
                type_only,
            });
        }
    }

    fn resolve(&mut self, dir: &Path, specifier: &str, file: &str, span: (u32, u32)) -> Target {
        let bare = !(specifier.starts_with('.') || specifier.starts_with('/'));
        match self.resolver.resolve(dir, specifier) {
            Ok(resolution) => {
                let path = resolution.path().to_path_buf();
                let in_node_modules = path.components().any(|c| c.as_os_str() == "node_modules");
                if in_node_modules || !path.starts_with(&self.root) {
                    Target::Package(specifier.to_owned())
                } else if CODE_EXTENSIONS
                    .iter()
                    .any(|e| path.to_string_lossy().ends_with(e))
                {
                    Target::Module(self.add(&path))
                } else {
                    Target::Asset(self.relative(&path))
                }
            }
            // A bare specifier that isn't a project alias is a package, installed or not.
            Err(_) if bare => Target::Package(specifier.to_owned()),
            Err(e) => {
                self.diagnostic(
                    Code::UnresolvedImport,
                    format!("can't resolve \"{specifier}\": {e}"),
                    file,
                    span,
                );
                Target::None
            }
        }
    }

    fn diagnostic(&mut self, code: Code, message: String, file: &str, span: (u32, u32)) {
        self.diagnostics.push(Diagnostic {
            code,
            message,
            file: PathBuf::from(file),
            span,
        });
    }
}

/// The module's import and re-export statements in source order, with their bindings.
/// `requested_modules` covers side-effect imports and re-exports too.
fn statements(module_record: &ModuleRecord<'_>) -> Vec<Statement> {
    let mut statements: Vec<Statement> = Vec::new();
    for (specifier, requests) in &module_record.requested_modules {
        for request in requests {
            let bindings = module_record
                .import_entries
                .iter()
                .filter(|e| e.statement_span == request.statement_span)
                .map(|e| Binding {
                    imported: match &e.import_name {
                        ImportImportName::Name(n) => Imported::Named(n.name.as_str().to_owned()),
                        ImportImportName::NamespaceObject => Imported::Namespace,
                        ImportImportName::Default(_) => Imported::Default,
                    },
                    local: e.local_name.name.as_str().to_owned(),
                    type_only: e.is_type,
                })
                .collect::<Vec<_>>();
            let type_only =
                request.is_type || (!bindings.is_empty() && bindings.iter().all(|b| b.type_only));
            statements.push(Statement {
                start: request.statement_span.start,
                specifier: specifier.as_str().to_owned(),
                type_only,
                bindings,
                span: (request.span.start, request.span.end),
            });
        }
    }
    statements.sort_by_key(|s| s.start);
    statements
}

/// The module's exports, from its module record.
fn exports(module_record: &ModuleRecord<'_>) -> Exports {
    let name = |n: &ExportExportName<'_>| match n {
        ExportExportName::Name(n) => Some(n.name.as_str().to_owned()),
        ExportExportName::Default(_) => Some("default".to_owned()),
        ExportExportName::Null => None,
    };
    let mut exports = Exports::default();
    for entry in module_record
        .local_export_entries
        .iter()
        .filter(|e| !e.is_type)
    {
        exports.local.extend(name(&entry.export_name));
    }
    for entry in module_record
        .indirect_export_entries
        .iter()
        .filter(|e| !e.is_type)
    {
        let (Some(exported), Some(request)) = (name(&entry.export_name), &entry.module_request)
        else {
            continue;
        };
        let imported = match &entry.import_name {
            ExportImportName::Name(n) if n.name.as_str() == "default" => Imported::Default,
            ExportImportName::Name(n) => Imported::Named(n.name.as_str().to_owned()),
            ExportImportName::All | ExportImportName::AllButDefault | ExportImportName::Null => {
                Imported::Namespace
            }
        };
        exports.reexports.push(Reexport {
            exported,
            imported,
            specifier: request.name.as_str().to_owned(),
        });
    }
    for entry in module_record
        .star_export_entries
        .iter()
        .filter(|e| !e.is_type)
    {
        if let Some(request) = &entry.module_request {
            exports.star.push(request.name.as_str().to_owned());
        }
    }
    exports
}
