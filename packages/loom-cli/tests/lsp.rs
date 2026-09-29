//! `loom lsp` driven over its protocol, as an editor drives it, on a project on disk.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use lsp_server::{Message, Notification, Request, RequestId};
use lsp_types::Url;
use serde_json::{json, Value};

const BP_A: &str = "blueprint BP_A : Actor at /Game/X/BP_A\n\n// How many times it was bumped.\nvar Count: int\n\nfn Bump() {\n    Count = Count + 1\n}\n";
const BP_B: &str = "blueprint BP_B : Actor at /Game/X/BP_B\nvar Other: BP_A\n\nfn Poke() {\n    Other.Bump()\n}\n";

/// A project on disk with these sources, by their paths under Content, and the test types when
/// `types` is set.
fn project(name: &str, types: bool, sources: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("loom-lsp-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Test.uproject"), "{}").unwrap();
    if types {
        std::fs::create_dir_all(root.join("Intermediate/Loom")).unwrap();
        let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("../loom/tests/types/small.json");
        std::fs::copy(types, root.join("Intermediate/Loom/types.json")).unwrap();
    }
    for (path, text) in sources {
        let file = root.join("Content").join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    root.canonicalize().unwrap()
}

struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: mpsc::Receiver<Message>,
    next_id: i32,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_loom"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (send, messages) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(message)) = Message::read(&mut stdout) {
                if send.send(message).is_err() {
                    break;
                }
            }
        });
        let mut client = Client { child, stdin, messages, next_id: 0 };
        let result = client.request("initialize", json!({ "capabilities": {} }));
        assert_eq!(result["capabilities"]["positionEncoding"], "utf-16", "{result}");
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, message: impl Into<Message>) {
        message.into().write(&mut self.stdin).unwrap();
    }

    fn next(&self) -> Message {
        self.messages.recv_timeout(Duration::from_secs(20)).expect("the server said nothing")
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.send(Request::new(id.clone(), method.to_string(), params));
        loop {
            if let Message::Response(response) = self.next()
                && response.id == id
            {
                return response.response_result.unwrap_or_else(|e| panic!("{method}: {e:?}"));
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(Notification::new(method.to_string(), params));
    }

    /// The next diagnostics published for a document.
    fn diagnostics(&self, uri: &Url) -> Vec<Value> {
        loop {
            if let Message::Notification(n) = self.next()
                && n.method == "textDocument/publishDiagnostics"
                && n.params["uri"] == uri.as_str()
            {
                return n.params["diagnostics"].as_array().unwrap().clone();
            }
        }
    }

    fn open(&mut self, uri: &Url, text: &str) {
        let document = json!({ "uri": uri, "languageId": "loom", "version": 1, "text": text });
        self.notify("textDocument/didOpen", json!({ "textDocument": document }));
    }

    fn at(&mut self, method: &str, uri: &Url, line: u32, character: u32) -> Value {
        let params = json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });
        self.request(method, params)
    }

    fn stop(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{status}");
    }
}

#[test]
fn reports_errors_as_typed_and_finds_names() {
    let root = project("names", true, &[("X/BP_A.lm", BP_A), ("X/BP_B.lm", BP_B)]);
    let a = Url::from_file_path(root.join("Content/X/BP_A.lm")).unwrap();
    let b = Url::from_file_path(root.join("Content/X/BP_B.lm")).unwrap();
    let mut client = Client::start();

    // The editor's text, not the file's, is compiled.
    client.open(&b, &BP_B.replace("var Other: BP_A", "var Other BP_A"));
    let diagnostics = client.diagnostics(&b);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["range"]["start"], json!({ "line": 1, "character": 10 }), "{diagnostics:?}");
    assert!(diagnostics[0]["message"].as_str().unwrap().contains("expected ':'"), "{diagnostics:?}");

    // Fixed, the error is cleared.
    let change = json!({ "textDocument": { "uri": b, "version": 2 }, "contentChanges": [{ "text": BP_B }] });
    client.notify("textDocument/didChange", change);
    assert_eq!(client.diagnostics(&b), Vec::<Value>::new());

    // A member of another Blueprint, through a variable of its type.
    let hover = client.at("textDocument/hover", &b, 4, 11);
    assert!(hover["contents"]["value"].as_str().unwrap().contains("fn Bump()"), "{hover}");
    let definition = client.at("textDocument/definition", &b, 4, 11);
    assert_eq!(definition[0]["uri"], a.as_str(), "{definition}");
    assert_eq!(definition[0]["range"], json!({ "start": { "line": 5, "character": 3 }, "end": { "line": 5, "character": 7 } }));

    // Another Blueprint by its name.
    let definition = client.at("textDocument/definition", &b, 1, 12);
    assert_eq!(definition[0]["uri"], a.as_str(), "{definition}");
    assert_eq!(definition[0]["range"]["start"], json!({ "line": 0, "character": 10 }), "{definition}");

    // An engine class has the project's types to show, and nowhere to go.
    let hover = client.at("textDocument/hover", &b, 0, 19);
    assert!(hover["contents"]["value"].as_str().unwrap().contains("class /Script/Engine.Actor"), "{hover}");
    assert_eq!(client.at("textDocument/definition", &b, 0, 19), Value::Null);

    // A variable of this source, with its doc comment.
    client.open(&a, BP_A);
    let hover = client.at("textDocument/hover", &a, 6, 5);
    let text = hover["contents"]["value"].as_str().unwrap();
    assert!(text.contains("var Count: int") && text.contains("How many times it was bumped."), "{hover}");

    client.stop();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_project_without_types_says_to_build_it_first() {
    let root = project("untyped", false, &[("X/BP_A.lm", BP_A)]);
    let a = Url::from_file_path(root.join("Content/X/BP_A.lm")).unwrap();
    let mut client = Client::start();
    client.open(&a, BP_A);
    let diagnostics = client.diagnostics(&a);
    client.stop();
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["range"]["start"], json!({ "line": 0, "character": 0 }), "{diagnostics:?}");
    let message = diagnostics[0]["message"].as_str().unwrap();
    assert!(message.starts_with("no types yet") && message.contains("-run=LoomBuild"), "{message}");
}
