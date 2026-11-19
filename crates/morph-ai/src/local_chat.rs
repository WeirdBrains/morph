//! The local model as the agent's backend: weirdnet-model's `LocalModel` with a call parser that
//! keeps what the model wrote. `LocalModel` reads a `<tool_call>` body only as strict JSON with
//! `name` and `arguments`, and a small model often writes something close to it (its own
//! `call:NAME{...}` syntax inside the tags, or `parameters` for `arguments`); that came back as a
//! call with no name, so the tool was never run. A body with only the arguments goes to the one
//! offered tool those arguments fit; the call still goes through the policy and its card.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use weirdnet_agent::{CancelToken, DeltaSink, Message, ModelBackend, ModelError, ModelTurn, ToolCall, ToolSpec};
use weirdnet_model::agent::turns;
use weirdnet_model::prompt::{TOOL_CLOSE, TOOL_OPEN};
use weirdnet_model::worker::{Chunk, GenerateRequest, StopCause, Worker};

pub struct LocalChat {
    name: String,
    worker: Arc<Worker>,
}

impl LocalChat {
    /// `name` shows in traces, for example `gemma-4-e4b`.
    pub fn new(name: impl Into<String>, worker: Arc<Worker>) -> Self {
        Self { name: name.into(), worker }
    }
}

#[async_trait]
impl ModelBackend for LocalChat {
    fn name(&self) -> String {
        self.name.clone()
    }

    async fn complete(&self, messages: &[Message], tools: &[ToolSpec], deltas: &DeltaSink, cancel: &CancelToken) -> Result<ModelTurn, ModelError> {
        let mut generation = self.worker.generate(GenerateRequest::new(turns(messages, tools)));
        let canceller = generation.canceller();
        let mut tags = Tags::default();
        let mut turn = ModelTurn::default();
        let take = |pieces: Vec<Piece>, turn: &mut ModelTurn| {
            for p in pieces {
                match p {
                    Piece::Text(t) => {
                        deltas.text(t.clone());
                        turn.text.push_str(&t);
                    }
                    Piece::Call(body) => {
                        let (name, arguments) = parse_body(&body, tools).unwrap_or_else(|| {
                            eprintln!("morph: the model's tool call could not be read: {:?}", body.chars().take(200).collect::<String>());
                            (String::new(), Value::Object(Default::default()))
                        });
                        let id = format!("call_{}", turn.tool_calls.len());
                        turn.tool_calls.push(ToolCall { id, name, arguments });
                    }
                }
            }
        };
        loop {
            let chunk = tokio::select! {
                c = generation.recv() => c,
                () = cancel.cancelled() => {
                    canceller.cancel();
                    return Err(ModelError::Cancelled);
                }
            };
            match chunk {
                Some(Chunk::Text(t)) => take(tags.feed(&t), &mut turn),
                Some(Chunk::Done(stats)) => {
                    if stats.stop == StopCause::Cancelled {
                        return Err(ModelError::Cancelled);
                    }
                    take(tags.finish(), &mut turn);
                    return Ok(turn);
                }
                Some(Chunk::Failed(e)) => return Err(ModelError::Failed(e)),
                None => return Err(ModelError::Failed("the model worker stopped".into())),
            }
        }
    }
}

/// A `<tool_call>` body as (name, arguments): strict JSON as asked, JSON with another key for
/// the name or the arguments, the arguments alone when exactly one offered tool fits them, or
/// the model's native `call:NAME{...}`.
pub fn parse_body(body: &str, tools: &[ToolSpec]) -> Option<(String, Value)> {
    let body = body.trim();
    if let Ok(Value::Object(v)) = serde_json::from_str::<Value>(body) {
        let Some(name) = ["name", "tool", "tool_name", "function"].iter().find_map(|k| v.get(*k).and_then(Value::as_str)) else {
            return fitting_tool(&v, tools).map(|name| (name, Value::Object(v)));
        };
        let args = ["arguments", "parameters", "args", "input"].iter().find_map(|k| v.get(*k)).cloned();
        let args = match args {
            Some(Value::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            Some(a @ Value::Object(_)) => a,
            _ => Value::Object(Default::default()),
        };
        return (!name.trim().is_empty()).then(|| (name.trim().to_string(), args));
    }
    let (text, calls) = crate::native_calls::split_native_calls(&format!("<|tool_call>{body}<tool_call|>"));
    match (text.trim().is_empty(), calls.into_iter().next()) {
        (true, Some(call)) if !call.0.contains(['"', ' ', '\n']) => Some(call),
        _ => None,
    }
}

/// The one offered tool whose required arguments are all in `args` and that knows every key.
fn fitting_tool(args: &serde_json::Map<String, Value>, tools: &[ToolSpec]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let fits = |t: &&ToolSpec| {
        let props = t.input_schema.get("properties").and_then(Value::as_object);
        let required = t.input_schema.get("required").and_then(Value::as_array);
        props.is_some_and(|p| args.keys().all(|k| p.contains_key(k)))
            && required.is_none_or(|r| r.iter().filter_map(Value::as_str).all(|k| args.contains_key(k)))
    };
    let mut fit = tools.iter().filter(fits);
    match (fit.next(), fit.next()) {
        (Some(t), None) => Some(t.name.clone()),
        _ => None,
    }
}

enum Piece {
    Text(String),
    Call(String),
}

/// Splits streamed text into text and `<tool_call>` bodies, holding back a partial tag.
#[derive(Default)]
struct Tags {
    buf: String,
    in_call: bool,
}

impl Tags {
    fn feed(&mut self, piece: &str) -> Vec<Piece> {
        let mut out = Vec::new();
        self.buf.push_str(piece);
        loop {
            if self.in_call {
                let Some(end) = self.buf.find(TOOL_CLOSE) else { return out };
                out.push(Piece::Call(self.buf[..end].to_string()));
                self.buf.drain(..end + TOOL_CLOSE.len());
                self.in_call = false;
            } else if let Some(i) = self.buf.find(TOOL_OPEN) {
                if i > 0 {
                    out.push(Piece::Text(self.buf[..i].to_string()));
                }
                self.buf.drain(..i + TOOL_OPEN.len());
                self.in_call = true;
            } else {
                let keep = (1..TOOL_OPEN.len()).rev().find(|&k| self.buf.ends_with(&TOOL_OPEN[..k])).unwrap_or(0);
                let cut = self.buf.len() - keep;
                if cut > 0 {
                    out.push(Piece::Text(self.buf[..cut].to_string()));
                    self.buf.drain(..cut);
                }
                return out;
            }
        }
    }

    /// End of stream: held text, or an unterminated call read as it is.
    fn finish(&mut self) -> Vec<Piece> {
        let rest = std::mem::take(&mut self.buf);
        match (self.in_call, rest.is_empty()) {
            (true, _) => vec![Piece::Call(rest)],
            (false, true) => Vec::new(),
            (false, false) => vec![Piece::Text(rest)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_call_written_close_to_the_asked_format_is_still_read() {
        let tools = vec![
            ToolSpec::new("page.fill", "")
                .schema(json!({"type": "object", "properties": {"target": {}, "role": {}, "value": {}}, "required": ["target", "value"]})),
            ToolSpec::new("page.click", "").schema(json!({"type": "object", "properties": {"target": {}, "role": {}}, "required": ["target"]})),
            ToolSpec::new("browser.find", "").schema(json!({"type": "object", "properties": {"query": {}}, "required": ["query"]})),
        ];
        let p = |b: &str| parse_body(b, &tools);
        assert_eq!(
            p(r#"{"name": "page.fill", "arguments": {"target": "Email", "text": "ada@example.com"}}"#),
            Some(("page.fill".into(), json!({"target": "Email", "text": "ada@example.com"})))
        );
        assert_eq!(p(r#"{"tool": "page.fill", "parameters": {"target": "Email"}}"#), Some(("page.fill".into(), json!({"target": "Email"}))));
        assert_eq!(p(r#"{"name": "page.fill", "arguments": "{\"target\": \"Name\"}"}"#), Some(("page.fill".into(), json!({"target": "Name"}))));
        assert_eq!(
            p(r#"call:page.fill{target:<|"|>Email<|"|>,text:<|"|>ada@example.com<|"|>}"#),
            Some(("page.fill".into(), json!({"target": "Email", "text": "ada@example.com"})))
        );
        assert_eq!(p("page.click{target: 'Subscribe'}").map(|c| c.0), Some("page.click".into()));
        assert_eq!(p(r#"{"arguments": {}}"#), None);
        // Seen from Gemma 4 E4B: page.fill's arguments with no name.
        let fill = r#"{"target":"email","role":"textbox","value":"ada@example.com"}"#;
        assert_eq!(p(fill), Some(("page.fill".into(), json!({"target": "email", "role": "textbox", "value": "ada@example.com"}))));
        assert_eq!(p(r#"{"target":"Subscribe"}"#).map(|c| c.0), Some("page.click".into()));
        assert_eq!(p(r#"{"target":"x","url":"https://a.example/"}"#), None, "no tool knows every key");
        assert_eq!(p(r#"{"role":"button"}"#), None, "two tools fit, or none has its required ones");
        assert_eq!(p("I would fill the email field"), None);
    }

    #[test]
    fn tags_split_streamed_text_and_keep_the_call_body() {
        let mut t = Tags::default();
        let mut got = Vec::new();
        for piece in ["Sure. <tool", "_call>call:page.fill{target:", "<|\"|>Name<|\"|>}</tool_call> done"] {
            got.extend(t.feed(piece));
        }
        got.extend(t.finish());
        let shown: Vec<String> = got
            .iter()
            .map(|p| match p {
                Piece::Text(s) => format!("T:{s}"),
                Piece::Call(s) => format!("C:{s}"),
            })
            .collect();
        assert_eq!(shown, vec!["T:Sure. ", "C:call:page.fill{target:<|\"|>Name<|\"|>}", "T: done"]);
    }
}
