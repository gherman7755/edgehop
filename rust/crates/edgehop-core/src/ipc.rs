//! Newline-delimited JSON server (docs/IPC.md). Requests are handled one at a time in arrival order; events
//! can be emitted from any thread through an [`Emitter`].

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{json, Map, Value};

/// Errors a handler can return; they become `{"error": {"code", "message"}}` responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcError {
    InvalidParams(String),
    UnknownMethod(String),
    Busy(String),
    Internal(String),
}

impl IpcError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidParams(_) => "invalid_params",
            Self::UnknownMethod(_) => "unknown_method",
            Self::Busy(_) => "busy",
            Self::Internal(_) => "internal",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidParams(m) | Self::UnknownMethod(m) | Self::Busy(m) | Self::Internal(m) => {
                m
            }
        }
    }
}

/// What the server dispatches requests to. [`crate::service::CoreService`] is the real one.
pub trait Handler {
    fn handle(&self, method: &str, params: &Map<String, Value>) -> Result<Value, IpcError>;
    /// Stop everything and give input back. Called on `core.shutdown` and on end of input.
    fn shutdown(&self);
}

/// Serialised writer shared by responses and events: one JSON object per line, never interleaved.
#[derive(Clone)]
pub struct Emitter {
    out: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Emitter {
    pub fn new(writer: impl Write + Send + 'static) -> Self {
        Self {
            out: Arc::new(Mutex::new(Box::new(writer))),
        }
    }

    pub fn emit(&self, event: &str, data: Value) {
        self.write(&json!({"event": event, "data": data}));
    }

    fn write(&self, value: &Value) {
        // serde_json escapes newlines inside strings, so one value is always exactly one line.
        let Ok(mut line) = serde_json::to_string(value) else {
            return;
        };
        line.push('\n');
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        // A closed pipe (the UI is gone) must not panic: end of input follows and shuts us down.
        let _ = out.write_all(line.as_bytes()).and_then(|()| out.flush());
    }
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

/// Blocks until the input ends or `core.shutdown` is received, then shuts the handler down.
pub fn serve(mut input: impl BufRead, emitter: &Emitter, handler: &dyn Handler) {
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match input.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => break, // end of input
            Ok(_) => {}
        }
        // Invalid UTF-8 becomes replacement characters and then fails as a parse error: never a crash.
        let text = String::from_utf8_lossy(&raw);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        if process(line, emitter, handler) == Flow::Stop {
            break;
        }
    }
    handler.shutdown();
}

fn process(line: &str, emitter: &Emitter, handler: &dyn Handler) -> Flow {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line) else {
        respond_error(
            emitter,
            Value::Null,
            "parse_error",
            "line is not a JSON object",
        );
        return Flow::Continue;
    };
    // Missing, boolean, null, array and object ids are all invalid: only numbers and strings are accepted.
    let id = match object.get("id") {
        Some(id @ (Value::Number(_) | Value::String(_))) => id.clone(),
        _ => {
            respond_error(
                emitter,
                Value::Null,
                "invalid_request",
                "\"id\" must be a number or a string",
            );
            return Flow::Continue;
        }
    };
    let method = match object.get("method") {
        Some(Value::String(m)) if !m.is_empty() => m.as_str(),
        _ => {
            respond_error(
                emitter,
                id,
                "invalid_request",
                "\"method\" must be a non-empty string",
            );
            return Flow::Continue;
        }
    };
    let empty = Map::new();
    let params = match object.get("params") {
        None => &empty,
        Some(Value::Object(params)) => params,
        Some(_) => {
            respond_error(
                emitter,
                id,
                "invalid_params",
                "\"params\" must be an object",
            );
            return Flow::Continue;
        }
    };

    if method == "core.shutdown" {
        emitter.write(&json!({"id": id, "result": {}}));
        return Flow::Stop;
    }
    match handler.handle(method, params) {
        Ok(result) => emitter.write(&json!({"id": id, "result": result})),
        Err(error) => respond_error(emitter, id, error.code(), error.message()),
    }
    Flow::Continue
}

fn respond_error(emitter: &Emitter, id: Value, code: &str, message: &str) {
    emitter.write(&json!({"id": id, "error": {"code": code, "message": message}}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Cursor;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FakeHandler {
        calls: RefCell<Vec<String>>,
        shutdowns: RefCell<u32>,
        emitter: Emitter,
    }

    impl Handler for FakeHandler {
        fn handle(&self, method: &str, params: &Map<String, Value>) -> Result<Value, IpcError> {
            self.calls.borrow_mut().push(method.to_string());
            match method {
                "echo" => Ok(json!({"params": params})),
                "bad" => Err(IpcError::InvalidParams("nope".into())),
                "busy" => Err(IpcError::Busy("already running".into())),
                "boom" => Err(IpcError::Internal("kaput".into())),
                "emit" => {
                    self.emitter.emit("log", json!({"line": "hello"}));
                    Ok(json!({}))
                }
                other => Err(IpcError::UnknownMethod(format!("unknown method {other}"))),
            }
        }

        fn shutdown(&self) {
            *self.shutdowns.borrow_mut() += 1;
        }
    }

    /// Runs the server over `input` and returns the parsed output lines and the handler.
    fn session(input: &str) -> (Vec<Value>, FakeHandler) {
        let sink = Sink::default();
        let emitter = Emitter::new(sink.clone());
        let handler = FakeHandler {
            calls: RefCell::default(),
            shutdowns: RefCell::default(),
            emitter: emitter.clone(),
        };
        serve(Cursor::new(input.as_bytes().to_vec()), &emitter, &handler);
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("every output line is JSON"))
            .collect();
        (lines, handler)
    }

    fn code(message: &Value) -> Option<&str> {
        message["error"]["code"].as_str()
    }

    #[test]
    fn answers_each_request_with_its_id() {
        let (out, handler) = session("{\"id\":1,\"method\":\"echo\",\"params\":{\"a\":1}}\n{\"id\":\"abc\",\"method\":\"echo\"}\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["id"], json!(1));
        assert_eq!(out[0]["result"]["params"]["a"], json!(1));
        assert_eq!(out[1]["id"], json!("abc"));
        assert_eq!(*handler.calls.borrow(), vec!["echo", "echo"]);
        assert_eq!(*handler.shutdowns.borrow(), 1); // end of input stops everything
    }

    #[test]
    fn maps_handler_errors_to_codes() {
        let (out, _) = session(
            "{\"id\":1,\"method\":\"bad\"}\n{\"id\":2,\"method\":\"busy\"}\n{\"id\":3,\"method\":\"nothing\"}\n{\"id\":4,\"method\":\"boom\"}\n",
        );
        let codes: Vec<_> = out.iter().map(|m| code(m).unwrap()).collect();
        assert_eq!(
            codes,
            vec!["invalid_params", "busy", "unknown_method", "internal"]
        );
        assert_eq!(out[3]["error"]["message"], json!("kaput"));
    }

    #[test]
    fn survives_malformed_input() {
        let (out, handler) = session(
            "not json at all\n[1,2,3]\n{\"id\":1}\n{\"id\":true,\"method\":\"echo\"}\n{\"id\":null,\"method\":\"echo\"}\n{\"method\":\"echo\"}\n{\"id\":2,\"method\":\"echo\",\"params\":[1]}\n\n   \n{\"id\":3,\"method\":\"echo\"}\n",
        );
        let codes: Vec<_> = out.iter().map(|m| code(m)).collect();
        assert_eq!(
            codes,
            vec![
                Some("parse_error"),
                Some("parse_error"),
                Some("invalid_request"),
                Some("invalid_request"), // a JSON boolean is not a valid id
                Some("invalid_request"),
                Some("invalid_request"),
                Some("invalid_params"),
                None,
            ]
        );
        assert_eq!(out[0]["id"], Value::Null);
        assert_eq!(out[7]["id"], json!(3)); // still serving after the garbage
        assert_eq!(handler.calls.borrow().len(), 1);
    }

    #[test]
    fn survives_invalid_utf8() {
        let sink = Sink::default();
        let emitter = Emitter::new(sink.clone());
        let handler = FakeHandler {
            calls: RefCell::default(),
            shutdowns: RefCell::default(),
            emitter: emitter.clone(),
        };
        let mut input = vec![0xff, 0xfe, b'\n'];
        input.extend_from_slice(b"{\"id\":1,\"method\":\"echo\"}\n");
        serve(Cursor::new(input), &emitter, &handler);
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap().contains("parse_error"));
    }

    #[test]
    fn shutdown_responds_then_stops() {
        let (out, handler) =
            session("{\"id\":9,\"method\":\"core.shutdown\"}\n{\"id\":10,\"method\":\"echo\"}\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], json!(9));
        assert!(out[0].get("result").is_some());
        assert_eq!(*handler.shutdowns.borrow(), 1);
        assert!(handler.calls.borrow().is_empty());
    }

    #[test]
    fn emits_events_as_lines_without_an_id() {
        let (out, _) = session("{\"id\":1,\"method\":\"emit\"}\n");
        let event = out.iter().find(|m| m.get("event").is_some()).unwrap();
        assert_eq!(event["event"], json!("log"));
        assert_eq!(event["data"]["line"], json!("hello"));
        assert!(event.get("id").is_none());
        assert!(out.iter().any(|m| m["id"] == json!(1)));
    }

    #[test]
    fn every_output_line_is_one_json_object() {
        let (out, _) =
            session("{\"id\":1,\"method\":\"echo\",\"params\":{\"s\":\"a\\nb \\\"q\\\" /x\"}}\n");
        assert_eq!(out.len(), 1); // an embedded newline stays escaped inside the string
        assert_eq!(out[0]["result"]["params"]["s"], json!("a\nb \"q\" /x"));
    }

    #[test]
    fn handles_a_final_line_without_a_newline() {
        let (out, _) = session("{\"id\":1,\"method\":\"echo\"}");
        assert_eq!(out.len(), 1);
    }
}
