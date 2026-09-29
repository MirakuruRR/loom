//! `loom lsp`: a language server for an editor, on stdin and stdout. A moment after a source is
//! opened or changed, it compiles the source's project as the editor's text stands and reports
//! what is wrong as diagnostics. It also shows what the project's types say about a name, and
//! goes to where the project's own sources declare one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use loom::project::{self, Project};
use loom::syntax::{self, Item};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{GotoDefinition, HoverRequest, Request as _};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, Location, MarkupContent, MarkupKind, OneOf, Position,
    PositionEncodingKind, PublishDiagnosticsParams, Range, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};
use serde_json::Value;

/// How long a project's sources must go unchanged before it is compiled.
const SETTLE: Duration = Duration::from_millis(300);

/// What the client counts a column in. The compiler counts characters, which is UTF-32.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

impl Encoding {
    /// The first the client offers that the server knows, or UTF-16, which every client knows.
    fn negotiate(params: &InitializeParams) -> Encoding {
        let offered = params.capabilities.general.as_ref().and_then(|g| g.position_encodings.as_ref());
        offered
            .into_iter()
            .flatten()
            .find_map(|kind| match kind.as_str() {
                "utf-8" => Some(Encoding::Utf8),
                "utf-16" => Some(Encoding::Utf16),
                "utf-32" => Some(Encoding::Utf32),
                _ => None,
            })
            .unwrap_or(Encoding::Utf16)
    }

    fn kind(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
            Encoding::Utf32 => PositionEncodingKind::UTF32,
        }
    }

    fn width(self, c: char) -> u32 {
        match self {
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf16 => c.len_utf16() as u32,
            Encoding::Utf32 => 1,
        }
    }
}

/// Line `line` of a text, counted from 0, without its ending.
fn line_of(text: &str, line: usize) -> &str {
    let line = text.split('\n').nth(line).unwrap_or("");
    line.strip_suffix('\r').unwrap_or(line)
}

/// A place as the compiler gives it, a line and a column of characters, both counted from 1, as
/// the client counts it. A column past the end of its line is the line's end.
pub fn to_position(text: &str, line: u32, column: u32, encoding: Encoding) -> Position {
    let line = line.saturating_sub(1);
    let chars = line_of(text, line as usize).chars().take(column.saturating_sub(1) as usize);
    Position { line, character: chars.map(|c| encoding.width(c)).sum() }
}

/// A place the client gives, as the index of a character in its line. A place inside a character
/// is that character's.
pub fn to_char(text: &str, position: Position, encoding: Encoding) -> usize {
    let mut at = 0;
    let mut index = 0;
    for c in line_of(text, position.line as usize).chars() {
        at += encoding.width(c);
        if at > position.character {
            break;
        }
        index += 1;
    }
    index
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The range of what starts at a place: a name or a number, or one character.
fn range_at(text: &str, line: u32, column: u32, encoding: Encoding) -> Range {
    let start = to_position(text, line, column, encoding);
    let rest: Vec<char> = line_of(text, start.line as usize).chars().skip(column.saturating_sub(1) as usize).collect();
    let length = match rest.iter().take_while(|c| is_word(**c)).count() {
        0 => rest.len().min(1),
        n => n,
    };
    let end = to_position(text, line, column + length as u32, encoding);
    Range { start, end }
}

/// The name a character of a line is in, where it starts, and the name before it and a `.`, if
/// there is one: `Class` for `Class.name`.
fn name_at(line: &str, index: usize) -> Option<(String, usize, Option<String>)> {
    let chars: Vec<char> = line.chars().collect();
    let (name, start, from) = match quoted_at(&chars, index) {
        Some((open, close)) => (chars[open + 1..close].iter().collect(), open + 1, open),
        None => {
            if !chars.get(index).is_some_and(|c| is_word(*c)) {
                return None;
            }
            let start = chars[..index].iter().rposition(|c| !is_word(*c)).map_or(0, |i| i + 1);
            let end = chars[index..].iter().position(|c| !is_word(*c)).map_or(chars.len(), |i| index + i);
            if chars[start].is_ascii_digit() {
                return None;
            }
            (chars[start..end].iter().collect::<String>(), start, start)
        }
    };
    let qualifier = (from > 0 && chars[from - 1] == '.').then(|| {
        let before = &chars[..from - 1];
        match before.last() {
            Some('`') => {
                let open = before[..before.len() - 1].iter().rposition(|c| *c == '`')?;
                Some(before[open + 1..before.len() - 1].iter().collect::<String>())
            }
            _ => {
                let from = before.iter().rposition(|c| !is_word(*c)).map_or(0, |i| i + 1);
                Some(before[from..].iter().collect::<String>())
            }
        }
    });
    Some((name, start, qualifier.flatten().filter(|q| !q.is_empty())))
}

/// The backticks around a name in backticks that a character is in or on, as (open, close).
fn quoted_at(chars: &[char], index: usize) -> Option<(usize, usize)> {
    let mut open = None;
    for (i, c) in chars.iter().enumerate() {
        if *c != '`' {
            continue;
        }
        match open.take() {
            None => open = Some(i),
            Some(o) if (o..=i).contains(&index) => return Some((o, i)),
            Some(_) => {}
        }
    }
    None
}

/// Where a whole name is in a line, at or after character `from`.
fn find_name(line: &str, from: usize, name: &str) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    let wanted: Vec<char> = name.chars().collect();
    (from..chars.len()).find(|&i| {
        chars[i..].starts_with(&wanted)
            && (i == 0 || !is_word(chars[i - 1]))
            && chars.get(i + wanted.len()).is_none_or(|c| !is_word(*c))
    })
}

/// An open document.
struct Doc {
    /// The URI the client knows it by.
    uri: Url,
    text: String,
    /// Its project's root, or why it has none.
    project: Result<PathBuf, String>,
}

/// A project's source, as the editor has it.
struct Source {
    uri: Url,
    text: String,
    parsed: Option<syntax::Source>,
}

impl Source {
    /// It parsed, as a Blueprint's source.
    fn file(&self) -> Option<&syntax::File> {
        match &self.parsed {
            Some(syntax::Source::Blueprint(file)) => Some(file),
            _ => None,
        }
    }

    /// It parsed, as a struct's source.
    fn structure(&self) -> Option<&syntax::Struct> {
        match &self.parsed {
            Some(syntax::Source::Struct(s)) => Some(s),
            _ => None,
        }
    }

    /// It parsed, as an enum's source.
    fn enumeration(&self) -> Option<&syntax::Enum> {
        match &self.parsed {
            Some(syntax::Source::Enum(e)) => Some(e),
            _ => None,
        }
    }
}

/// A name a source declares.
struct Declaration<'a> {
    source: &'a Source,
    /// Counted from 0, the column in characters.
    line: usize,
    column: usize,
    /// What declares it, as written.
    text: String,
    doc: Option<String>,
}

struct Server {
    connection: Connection,
    encoding: Encoding,
    /// Open documents, by their path as the project's sources are found by it.
    open: HashMap<PathBuf, Doc>,
    /// Projects to compile, by their root, and when.
    due: HashMap<PathBuf, Instant>,
    /// The documents given diagnostics at each project's last compile.
    published: HashMap<PathBuf, HashSet<Url>>,
    /// Each project's types, as last read, and when the file was written.
    types: HashMap<PathBuf, (SystemTime, Rc<Value>)>,
}

/// A path as the project's sources are found by, so that two ways to one file are one key.
fn key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn run() -> Result<()> {
    let (connection, threads) = Connection::stdio();
    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params).context("reading the client's initialize")?;
    let encoding = Encoding::negotiate(&params);
    let result = InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: Some(encoding.kind()),
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            definition_provider: Some(OneOf::Left(true)),
            ..ServerCapabilities::default()
        },
        server_info: Some(ServerInfo { name: "loom".to_string(), version: Some(env!("CARGO_PKG_VERSION").to_string()) }),
    };
    connection.initialize_finish(id, serde_json::to_value(result)?)?;
    let mut server =
        Server { connection, encoding, open: HashMap::new(), due: HashMap::new(), published: HashMap::new(), types: HashMap::new() };
    server.serve()?;
    drop(server);
    threads.join()?;
    Ok(())
}

impl Server {
    fn serve(&mut self) -> Result<()> {
        loop {
            let next = self.due.values().min().map(|at| at.saturating_duration_since(Instant::now()));
            let message = match next {
                Some(wait) => match self.connection.receiver.recv_timeout(wait) {
                    Ok(message) => Some(message),
                    Err(e) if e.is_timeout() => None,
                    Err(_) => return Ok(()),
                },
                None => match self.connection.receiver.recv() {
                    Ok(message) => Some(message),
                    Err(_) => return Ok(()),
                },
            };
            match message {
                Some(Message::Request(request)) => {
                    if self.connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    self.request(request)?;
                }
                Some(Message::Notification(notification)) => self.notification(notification)?,
                Some(Message::Response(_)) | None => {}
            }
            let now = Instant::now();
            let ready: Vec<PathBuf> = self.due.iter().filter(|(_, at)| **at <= now).map(|(root, _)| root.clone()).collect();
            for root in ready {
                self.due.remove(&root);
                self.compile(&root)?;
            }
        }
    }

    fn send(&self, message: impl Into<Message>) -> Result<()> {
        self.connection.sender.send(message.into()).context("writing to the client")
    }

    fn request(&mut self, request: Request) -> Result<()> {
        let id: RequestId = request.id.clone();
        let result = match request.method.as_str() {
            HoverRequest::METHOD => {
                let params: HoverParams = serde_json::from_value(request.params)?;
                serde_json::to_value(self.hover(params))?
            }
            GotoDefinition::METHOD => {
                let params: GotoDefinitionParams = serde_json::from_value(request.params)?;
                serde_json::to_value(self.definition(params))?
            }
            method => {
                let code = lsp_server::ErrorCode::MethodNotFound as i32;
                return self.send(Response::new_err(id, code, format!("loom lsp does not do {method}")));
            }
        };
        self.send(Response::new_ok(id, result))
    }

    fn notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(notification.params)?;
                self.opened(params.text_document.uri, params.text_document.text)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: lsp_types::DidChangeTextDocumentParams = serde_json::from_value(notification.params)?;
                // Changes are whole documents, as the server asked, so the last is the text.
                if let Some(change) = params.content_changes.into_iter().last() {
                    self.opened(params.text_document.uri, change.text)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(notification.params)?;
                let Ok(path) = params.text_document.uri.to_file_path() else { return Ok(()) };
                if let Some(Doc { project: Ok(root), .. }) = self.open.remove(&key(&path)) {
                    self.due.insert(root, Instant::now() + SETTLE);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A document's text, as opened or changed.
    fn opened(&mut self, uri: Url, text: String) -> Result<()> {
        let Ok(opened) = uri.to_file_path() else { return Ok(()) };
        let path = key(&opened);
        // The project is found from the path the document was opened by, not the one its symlinks
        // lead to: a project's Content can link to sources kept outside it.
        let project = match self.open.get(&path) {
            Some(doc) => doc.project.clone(),
            None => Project::find(opened.parent().unwrap_or(&opened)).map(|p| p.root).map_err(|e| format!("{e:#}")),
        };
        match &project {
            Ok(root) => {
                self.due.insert(root.clone(), Instant::now() + SETTLE);
            }
            Err(error) => {
                let diagnostic = Diagnostic { message: error.clone(), ..diagnostic(Range::default(), "") };
                self.publish(uri.clone(), vec![diagnostic])?;
            }
        }
        self.open.insert(path, Doc { uri, text, project });
        Ok(())
    }

    fn publish(&self, uri: Url, diagnostics: Vec<Diagnostic>) -> Result<()> {
        let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
        self.send(Notification::new(PublishDiagnostics::METHOD.to_string(), params))
    }

    /// Compiles a project with its open documents' text, and publishes what is wrong, clearing
    /// what was wrong before and no longer is.
    fn compile(&mut self, root: &Path) -> Result<()> {
        let project = Project { root: root.to_path_buf() };
        let mut unsaved = HashMap::new();
        let mut found: HashMap<Url, (String, Vec<Diagnostic>)> = HashMap::new();
        let check = project.sources().and_then(|sources| {
            for source in &sources {
                if let Some(doc) = self.open.get(&key(&source.path)) {
                    unsaved.insert(source.path.clone(), doc.text.clone());
                }
            }
            project.check_with(&unsaved)
        });
        let in_project = || self.open.values().filter(|doc| doc.project.as_ref().is_ok_and(|r| r == root));
        match check {
            Ok(check) => {
                for error in check.errors {
                    if error.file.is_empty() {
                        for doc in in_project() {
                            found.entry(doc.uri.clone()).or_insert_with(|| (doc.text.clone(), Vec::new())).1.push(whole(&error.message));
                        }
                        continue;
                    }
                    let path = root.join(&error.file);
                    let (uri, text) = match self.open.get(&key(&path)) {
                        Some(doc) => (doc.uri.clone(), doc.text.clone()),
                        None => match Url::from_file_path(&path) {
                            Ok(uri) => (uri, std::fs::read_to_string(&path).unwrap_or_default()),
                            Err(()) => continue,
                        },
                    };
                    let range = match (error.line, error.column) {
                        (Some(line), Some(column)) => range_at(&text, line, column, self.encoding),
                        (Some(line), None) => range_at(&text, line, 1, self.encoding),
                        _ => Range::default(),
                    };
                    found.entry(uri).or_insert_with(|| (text, Vec::new())).1.push(diagnostic(range, &error.message));
                }
            }
            // The project could not be compiled at all, as when the plugin has not yet saved its
            // types: each of its open documents says why.
            Err(error) => {
                let message = format!("{error:#}");
                for doc in in_project() {
                    found.insert(doc.uri.clone(), (doc.text.clone(), vec![whole(&message)]));
                }
            }
        }

        let before = self.published.remove(root).unwrap_or_default();
        for uri in before.iter().filter(|uri| !found.contains_key(*uri)) {
            self.publish(uri.clone(), Vec::new())?;
        }
        let now: HashSet<Url> = found.keys().cloned().collect();
        for (uri, (_, diagnostics)) in found {
            self.publish(uri, diagnostics)?;
        }
        self.published.insert(root.to_path_buf(), now);
        Ok(())
    }

    /// Every source of a project, as the editor has it, parsed where it parses.
    fn sources(&self, root: &Path) -> Vec<Source> {
        let project = Project { root: root.to_path_buf() };
        let Ok(sources) = project.sources() else { return Vec::new() };
        sources
            .into_iter()
            .filter_map(|source| {
                let (uri, text) = match self.open.get(&key(&source.path)) {
                    Some(doc) => (doc.uri.clone(), doc.text.clone()),
                    None => (Url::from_file_path(&source.path).ok()?, std::fs::read_to_string(&source.path).ok()?),
                };
                let parsed = syntax::parse_text(&text, &source.path).ok();
                Some(Source { uri, text, parsed })
            })
            .collect()
    }

    /// The open document at a place, its project, and the name there.
    fn name_at(&self, uri: &Url, position: Position) -> Option<(PathBuf, PathBuf, String, Option<String>)> {
        let path = key(&uri.to_file_path().ok()?);
        let doc = self.open.get(&path)?;
        let root = doc.project.clone().ok()?;
        let index = to_char(&doc.text, position, self.encoding);
        let (name, _, qualifier) = name_at(line_of(&doc.text, position.line as usize), index)?;
        // A contextual word may be a name, which looking it up finds or does not.
        if syntax::is_reserved(&name) {
            return None;
        }
        Some((path, root, name, qualifier))
    }

    fn types(&mut self, root: &Path) -> Option<Rc<Value>> {
        let project = Project { root: root.to_path_buf() };
        let written = std::fs::metadata(project.types_path()).and_then(|m| m.modified()).ok()?;
        if let Some((at, dump)) = self.types.get(root)
            && *at == written
        {
            return Some(dump.clone());
        }
        let dump = Rc::new(project.types_dump().ok()?);
        self.types.insert(root.to_path_buf(), (written, dump.clone()));
        Some(dump)
    }

    fn hover(&mut self, params: HoverParams) -> Option<Hover> {
        let at = params.text_document_position_params;
        let (path, root, name, qualifier) = self.name_at(&at.text_document.uri, at.position)?;
        let sources = self.sources(&root);
        let here = sources.iter().find(|s| key_of(s) == Some(path.clone()));
        let found = declarations(&sources, here, &name, qualifier.as_deref());
        let text = if !found.is_empty() {
            found
                .iter()
                .map(|d| {
                    let doc = d.doc.as_ref().map(|doc| format!("\n\n{doc}")).unwrap_or_default();
                    format!("```loom\n{}\n```{doc}", d.text)
                })
                .collect::<Vec<_>>()
                .join("\n\n---\n\n")
        } else {
            let dump = self.types(&root)?;
            let file = here.and_then(Source::file);
            let mut names = Vec::new();
            if let Some(qualifier) = qualifier.as_deref().filter(|q| *q != "self") {
                if let Some(ty) = file.and_then(|f| var_type(f, qualifier)) {
                    names.push(format!("{ty}.{name}"));
                }
                names.push(format!("{qualifier}.{name}"));
            }
            names.push(name.clone());
            if qualifier.as_deref().is_none_or(|q| q == "self")
                && let Some(file) = file
            {
                names.push(format!("{}.{name}", file.parent));
            }
            let found = names.iter().find_map(|name| project::lookup(&dump, name).ok())?;
            format!("```\n{}```", found)
        };
        Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: text }), range: None })
    }

    fn definition(&mut self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let at = params.text_document_position_params;
        let (path, root, name, qualifier) = self.name_at(&at.text_document.uri, at.position)?;
        let sources = self.sources(&root);
        let here = sources.iter().find(|s| key_of(s) == Some(path.clone()));
        let locations: Vec<Location> = declarations(&sources, here, &name, qualifier.as_deref())
            .iter()
            .map(|d| {
                let start = to_position(&d.source.text, d.line as u32 + 1, d.column as u32 + 1, self.encoding);
                let end = to_position(&d.source.text, d.line as u32 + 1, (d.column + name.chars().count()) as u32 + 1, self.encoding);
                Location { uri: d.source.uri.clone(), range: Range { start, end } }
            })
            .collect();
        (!locations.is_empty()).then_some(GotoDefinitionResponse::Array(locations))
    }
}

fn key_of(source: &Source) -> Option<PathBuf> {
    source.uri.to_file_path().ok().map(|p| key(&p))
}

fn diagnostic(range: Range, message: &str) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("loom".to_string()),
        message: message.to_string(),
        ..Diagnostic::default()
    }
}

/// A diagnostic of a whole document, which goes at its start.
fn whole(message: &str) -> Diagnostic {
    diagnostic(Range::default(), message)
}

/// The type a Blueprint declares a variable or component of, by its name.
fn var_type<'a>(file: &'a syntax::File, name: &str) -> Option<&'a str> {
    file.items.iter().find_map(|item| match item {
        Item::Var { name: n, ty, .. } | Item::Component { name: n, class: ty, .. } if n == name => Some(ty.name.as_str()),
        _ => None,
    })
}

/// The name an item declares, and where it starts.
fn declared(item: &Item) -> Option<(&str, syntax::Span)> {
    match item {
        Item::Var { name, span, .. }
        | Item::Dispatcher { name, span, .. }
        | Item::Fn { name, span, .. }
        | Item::Event { name, span, .. }
        | Item::Component { name, span, .. }
        | Item::Widget { name, span, .. } => Some((name, *span)),
        _ => None,
    }
}

/// Where the project's sources declare a name: a Blueprint, an enum or a struct, a member, an enum's
/// value or a struct's field. A member is looked for in the Blueprint the qualifier names, or whose
/// type it has, and without one in this source; failing those, in every source. A value is looked
/// for in the enum the qualifier names, and a field in the struct it names or whose type it has.
fn declarations<'a>(sources: &'a [Source], here: Option<&'a Source>, name: &str, qualifier: Option<&str>) -> Vec<Declaration<'a>> {
    let parsed = || sources.iter().filter_map(|s| s.file().map(|f| (s, f)));
    let mut found = Vec::new();

    if qualifier.is_none() {
        // The header is the first line that says what the source declares, since only comments
        // come before it.
        for (source, keyword) in sources.iter().filter_map(|s| match &s.parsed {
            Some(syntax::Source::Blueprint(f)) if f.name == name => Some((s, "blueprint")),
            Some(syntax::Source::Enum(e)) if e.name == name => Some((s, "enum")),
            Some(syntax::Source::Struct(st)) if st.name == name => Some((s, "struct")),
            Some(syntax::Source::Interface(i)) if i.name == name => Some((s, "interface")),
            _ => None,
        }) {
            let header = (0..source.text.split('\n').count())
                .find_map(|line| Some((line, find_name(line_of(&source.text, line), 0, keyword)?)));
            if let Some((line, at)) = header {
                let text = line_of(&source.text, line);
                let Some(column) = find_name(text, at, name) else { continue };
                found.push(Declaration { source, line, column, text: text.trim().to_string(), doc: None });
            }
        }
    }

    let members = |source: &'a Source, file: &'a syntax::File| {
        file.items.iter().filter_map(move |item| {
            let (declared, span) = declared(item)?;
            if declared != name {
                return None;
            }
            let line = span.line.saturating_sub(1) as usize;
            let text = line_of(&source.text, line);
            let column = find_name(text, span.col.saturating_sub(1) as usize, name)?;
            let doc = match item {
                Item::Var { doc, .. } | Item::Dispatcher { doc, .. } | Item::Fn { doc, .. } | Item::Event { doc, .. } => doc.clone(),
                _ => None,
            };
            let text = text.trim().trim_end_matches('{').trim_end().to_string();
            Some(Declaration { source, line, column, text, doc })
        })
    };
    if let Some(qualifier) = qualifier {
        for (source, e) in sources.iter().filter_map(|s| s.enumeration().map(|e| (s, e))).filter(|(_, e)| e.name == qualifier) {
            for value in e.values.iter().filter(|v| v.name == name) {
                let line = value.span.line.saturating_sub(1) as usize;
                let text = line_of(&source.text, line);
                let Some(column) = find_name(text, value.span.col.saturating_sub(1) as usize, name) else { continue };
                found.push(Declaration { source, line, column, text: text.trim().to_string(), doc: value.doc.clone() });
            }
        }
        // A field of a struct, named by the struct or by a variable of it.
        let owner = here.and_then(Source::file).and_then(|f| var_type(f, qualifier)).unwrap_or(qualifier);
        for (source, st) in sources.iter().filter_map(|s| s.structure().map(|st| (s, st))).filter(|(_, st)| st.name == owner) {
            for field in st.fields.iter().filter(|f| f.name == name) {
                let line = field.span.line.saturating_sub(1) as usize;
                let text = line_of(&source.text, line);
                let Some(column) = find_name(text, field.span.col.saturating_sub(1) as usize, name) else { continue };
                found.push(Declaration { source, line, column, text: text.trim().to_string(), doc: field.doc.clone() });
            }
        }
        if !found.is_empty() {
            return found;
        }
    }

    let here_file = here.and_then(|s| s.file().map(|f| (s, f)));
    let owner = match qualifier {
        None | Some("self") => here_file,
        Some(q) => {
            let class = here_file.and_then(|(_, f)| var_type(f, q)).unwrap_or(q);
            parsed().find(|(_, f)| f.name == class)
        }
    };
    if let Some((source, file)) = owner {
        found.extend(members(source, file));
    }
    if found.is_empty() && !matches!(qualifier, None | Some("self")) {
        for (source, file) in parsed() {
            found.extend(members(source, file));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_a_column_of_characters_to_each_encoding() {
        // "é" is two bytes and one UTF-16 unit; "😀" is four bytes and two UTF-16 units.
        let text = "first\r\nlet s = \"é😀\" + x\n";
        let x = 16;
        assert_eq!(line_of(text, 1).chars().nth(x - 1), Some('x'));
        assert_eq!(to_position(text, 2, x as u32, Encoding::Utf32), Position { line: 1, character: 15 });
        assert_eq!(to_position(text, 2, x as u32, Encoding::Utf16), Position { line: 1, character: 16 });
        assert_eq!(to_position(text, 2, x as u32, Encoding::Utf8), Position { line: 1, character: 19 });
        for encoding in [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32] {
            assert_eq!(to_char(text, to_position(text, 2, x as u32, encoding), encoding), x - 1, "{encoding:?}");
        }
    }

    #[test]
    fn a_place_past_the_end_of_its_line_is_its_end() {
        assert_eq!(to_position("ab\ncd", 1, 9, Encoding::Utf16), Position { line: 0, character: 2 });
        assert_eq!(to_position("ab", 4, 1, Encoding::Utf16), Position { line: 3, character: 0 });
    }

    #[test]
    fn a_place_inside_a_character_is_that_character() {
        // The second UTF-16 unit of "😀", and its third byte.
        assert_eq!(to_char("😀x", Position { line: 0, character: 1 }, Encoding::Utf16), 0);
        assert_eq!(to_char("😀x", Position { line: 0, character: 2 }, Encoding::Utf16), 1);
        assert_eq!(to_char("😀x", Position { line: 0, character: 3 }, Encoding::Utf8), 0);
    }

    #[test]
    fn a_diagnostic_covers_the_name_it_is_at() {
        let range = range_at("var Count int\n", 1, 11, Encoding::Utf16);
        assert_eq!((range.start.character, range.end.character), (10, 13));
        let range = range_at("x = (\n", 1, 5, Encoding::Utf16);
        assert_eq!((range.start.character, range.end.character), (4, 5));
    }

    #[test]
    fn finds_the_name_at_a_place_and_what_qualifies_it() {
        assert_eq!(name_at("  self.Health = 1", 9), Some(("Health".to_string(), 7, Some("self".to_string()))));
        assert_eq!(name_at("var Other: BP_A", 12), Some(("BP_A".to_string(), 11, None)));
        assert_eq!(name_at("x = 12", 4), None);
        assert_eq!(name_at("a + b", 1), None);
        assert_eq!(name_at("    `Scale Sample Count`(2)", 10), Some(("Scale Sample Count".to_string(), 5, None)));
        assert_eq!(name_at("x = `My Actor`.`Move It`()", 17), Some(("Move It".to_string(), 16, Some("My Actor".to_string()))));
    }

    /// A project's sources, each at its own path, as the server reads them.
    fn sources(texts: &[&str]) -> Vec<Source> {
        texts
            .iter()
            .enumerate()
            .map(|(i, text)| Source {
                uri: Url::from_file_path(format!("/project/Content/{i}.lm")).unwrap(),
                text: text.to_string(),
                parsed: syntax::parse_source(text).ok(),
            })
            .collect()
    }

    /// Where each declaration found is: its source's index, line and column.
    fn places(sources: &[Source], found: &[Declaration]) -> Vec<(usize, usize, usize)> {
        found.iter().map(|d| (sources.iter().position(|s| std::ptr::eq(s, d.source)).unwrap(), d.line, d.column)).collect()
    }

    #[test]
    fn finds_an_enum_struct_or_interface_by_its_header() {
        let sources = sources(&[
            "// What a worker holds.\nenum E_Tool at /Game/E_Tool {\n    Hammer\n}\n",
            "struct S_Cell at /Game/S_Cell {\n    X: int\n}\n",
            "interface BPI_Usable at /Game/BPI_Usable {\n    fn Use(by: Actor)\n}\n",
        ]);
        assert_eq!(places(&sources, &declarations(&sources, None, "E_Tool", None)), [(0, 1, 5)]);
        assert_eq!(places(&sources, &declarations(&sources, None, "S_Cell", None)), [(1, 0, 7)]);
        assert_eq!(places(&sources, &declarations(&sources, None, "BPI_Usable", None)), [(2, 0, 10)]);
    }

    #[test]
    fn finds_an_enums_value_and_a_structs_field() {
        let sources = sources(&[
            "enum E_Tool at /Game/E_Tool {\n    Hammer\n    // Cuts wood.\n    Saw \"Hand Saw\"\n}\n",
            "struct S_Cell at /Game/S_Cell {\n    X: int\n    Y: int = 1\n}\n",
            "blueprint BP_Grid : Actor at /Game/BP_Grid\n\nvar Cell: S_Cell\n",
        ]);
        let saw = declarations(&sources, None, "Saw", Some("E_Tool"));
        assert_eq!(places(&sources, &saw), [(0, 3, 4)]);
        assert_eq!(saw[0].doc.as_deref(), Some("Cuts wood."));
        assert_eq!(places(&sources, &declarations(&sources, None, "Y", Some("S_Cell"))), [(1, 2, 4)]);
        // A field named through a variable of the struct.
        assert_eq!(places(&sources, &declarations(&sources, Some(&sources[2]), "Y", Some("Cell"))), [(1, 2, 4)]);
    }

    #[test]
    fn finds_a_whole_name() {
        assert_eq!(find_name("var Count: Counter", 0, "Count"), Some(4));
        assert_eq!(find_name("var Counter: Count", 0, "Count"), Some(13));
    }

    #[test]
    fn prefers_what_the_client_offers_first() {
        let params = |offered: &[PositionEncodingKind]| InitializeParams {
            capabilities: lsp_types::ClientCapabilities {
                general: Some(lsp_types::GeneralClientCapabilities { position_encodings: Some(offered.to_vec()), ..Default::default() }),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(Encoding::negotiate(&InitializeParams::default()), Encoding::Utf16);
        assert_eq!(Encoding::negotiate(&params(&[PositionEncodingKind::UTF8, PositionEncodingKind::UTF16])), Encoding::Utf8);
        assert_eq!(Encoding::negotiate(&params(&[PositionEncodingKind::new("utf-7"), PositionEncodingKind::UTF16])), Encoding::Utf16);
    }
}
