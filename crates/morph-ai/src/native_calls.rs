//! Gemma 4's own tool-call syntax.
//!
//! `weirdnet-model` 0.1 asks for `<tool_call>{json}</tool_call>`, but Gemma 4 often answers in
//! the syntax its chat template taught it:
//! `<|tool_call>call:web.search{query:<|"|>tokio shutdown<|"|>}<tool_call|>`. [`NativeCalls`]
//! wraps a backend, turns those into tool calls and keeps them out of the streamed text.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};
use weirdnet_agent::{AgentEvent, CancelToken, DeltaSink, Message, ModelBackend, ModelError, ModelTurn, ToolCall, ToolSpec};

const OPEN: &str = "<|tool_call>";
const CLOSE: &str = "<tool_call|>";
const QUOTE: &str = "<|\"|>";

/// A backend whose Gemma-native tool calls come back as tool calls.
pub struct NativeCalls {
    inner: Arc<dyn ModelBackend>,
}

impl NativeCalls {
    pub fn new(inner: Arc<dyn ModelBackend>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ModelBackend for NativeCalls {
    fn name(&self) -> String {
        self.inner.name()
    }

    async fn complete(&self, messages: &[Message], tools: &[ToolSpec], deltas: &DeltaSink, cancel: &CancelToken) -> Result<ModelTurn, ModelError> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = DeltaSink::new(Some(tx));
        let mut hold = HoldBack::default();
        let done = {
            let run = self.inner.complete(messages, tools, &sink, cancel);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    biased;
                    Some(ev) = rx.recv() => forward(ev, &mut hold, deltas),
                    r = &mut run => break r,
                }
            }
        };
        drop(sink);
        while let Ok(ev) = rx.try_recv() {
            forward(ev, &mut hold, deltas);
        }
        let rest = hold.finish();
        if !rest.is_empty() {
            deltas.text(rest);
        }
        let mut turn = done?;
        let (text, calls) = split_native_calls(&turn.text);
        if !calls.is_empty() {
            turn.text = text;
            for (name, arguments) in calls {
                let id = format!("call_{}", turn.tool_calls.len());
                turn.tool_calls.push(ToolCall { id, name, arguments });
            }
        }
        for call in &mut turn.tool_calls {
            match resolve_tool_name(&call.name, tools) {
                Some(name) => call.name = name,
                None => eprintln!("morph: the model called a tool that is not offered: {:?}", call.name.chars().take(80).collect::<String>()),
            }
        }
        Ok(turn)
    }
}

/// The offered tool a small model meant: the exact name, or one written with a namespace
/// prefix (`default_api:page.fill`), underscores for dots (`page_fill`), or without its
/// group when only one tool has that last part (`fill`). None when nothing offered fits.
pub fn resolve_tool_name(name: &str, tools: &[ToolSpec]) -> Option<String> {
    let exact = |n: &str| tools.iter().find(|t| t.name == n).map(|t| t.name.clone());
    if let Some(n) = exact(name) {
        return Some(n);
    }
    let bare = name.trim().rsplit(':').next().unwrap_or(name).trim();
    if let Some(n) = exact(bare) {
        return Some(n);
    }
    let flat = |n: &str| n.replace(['_', '-'], ".").to_ascii_lowercase();
    if let Some(t) = tools.iter().find(|t| flat(&t.name) == flat(bare)) {
        return Some(t.name.clone());
    }
    let mut last = tools.iter().filter(|t| t.name.rsplit('.').next() == Some(bare) && t.name.contains('.'));
    match (last.next(), last.next()) {
        (Some(t), None) => Some(t.name.clone()),
        _ => None,
    }
}

fn forward(ev: AgentEvent, hold: &mut HoldBack, deltas: &DeltaSink) {
    if let AgentEvent::TextDelta(t) = ev {
        let out = hold.feed(&t);
        if !out.is_empty() {
            deltas.text(out);
        }
    }
}

/// Streams text through, holding back anything that may be the start of a native call and
/// dropping the calls themselves.
#[derive(Default)]
struct HoldBack {
    buf: String,
    in_call: bool,
}

impl HoldBack {
    fn feed(&mut self, t: &str) -> String {
        self.buf.push_str(t);
        let mut out = String::new();
        loop {
            if self.in_call {
                match self.buf.find(CLOSE) {
                    Some(i) => {
                        self.buf.drain(..i + CLOSE.len());
                        self.in_call = false;
                    }
                    None => return out,
                }
            } else if let Some(i) = self.buf.find(OPEN) {
                out.push_str(&self.buf[..i]);
                self.buf.drain(..i + OPEN.len());
                self.in_call = true;
            } else {
                // Keep a tail that could still grow into OPEN.
                let keep = (1..OPEN.len()).rev().find(|&n| self.buf.ends_with(&OPEN[..n])).unwrap_or(0);
                let cut = self.buf.len() - keep;
                out.push_str(&self.buf[..cut]);
                self.buf.drain(..cut);
                return out;
            }
        }
    }

    fn finish(&mut self) -> String {
        if self.in_call { String::new() } else { std::mem::take(&mut self.buf) }
    }
}

/// The text without native calls, and the calls as (name, arguments).
pub fn split_native_calls(text: &str) -> (String, Vec<(String, Value)>) {
    let mut out = String::new();
    let mut calls = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(OPEN) {
        out.push_str(&rest[..i]);
        let body_start = i + OPEN.len();
        let (body, after) = match rest[body_start..].find(CLOSE) {
            Some(j) => (&rest[body_start..body_start + j], &rest[body_start + j + CLOSE.len()..]),
            None => (&rest[body_start..], ""),
        };
        if let Some(call) = parse_call(body) {
            calls.push(call);
        }
        rest = after;
    }
    out.push_str(rest);
    (out.trim().to_string(), calls)
}

/// `call:NAME{args}`.
fn parse_call(body: &str) -> Option<(String, Value)> {
    let body = body.trim();
    let body = body.strip_prefix("call:").unwrap_or(body);
    let (name, args) = match body.find('{') {
        Some(i) => (&body[..i], &body[i..]),
        None => (body, ""),
    };
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let args = if args.is_empty() {
        Value::Object(Map::new())
    } else {
        let mut p = Parser { s: args, i: 0 };
        match p.value() {
            Some(v @ Value::Object(_)) => v,
            _ => Value::Object(Map::new()),
        }
    };
    Some((name.to_string(), args))
}

/// Gemma's argument syntax: `{key:value,...}`, `[a,b]`, `<|"|>string<|"|>`, bare numbers,
/// booleans and null.
struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn skip_ws(&mut self) {
        let r = self.rest();
        self.i += r.len() - r.trim_start().len();
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.rest().starts_with(c) {
            self.i += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.skip_ws();
        let r = self.rest();
        if r.starts_with('{') {
            self.i += 1;
            let mut map = Map::new();
            loop {
                if self.eat('}') {
                    return Some(Value::Object(map));
                }
                let key = self.key()?;
                if !self.eat(':') {
                    return None;
                }
                let v = self.value()?;
                map.insert(key, v);
                if !self.eat(',') {
                    return self.eat('}').then_some(Value::Object(map));
                }
            }
        }
        if r.starts_with('[') {
            self.i += 1;
            let mut items = Vec::new();
            loop {
                if self.eat(']') {
                    return Some(Value::Array(items));
                }
                items.push(self.value()?);
                if !self.eat(',') {
                    return self.eat(']').then_some(Value::Array(items));
                }
            }
        }
        if let Some(q) = r.strip_prefix(QUOTE) {
            let end = q.find(QUOTE)?;
            self.i += QUOTE.len() + end + QUOTE.len();
            return Some(Value::String(q[..end].to_string()));
        }
        if let Some(q) = r.strip_prefix('"') {
            let end = q.find('"')?;
            self.i += 1 + end + 1;
            return Some(Value::String(q[..end].to_string()));
        }
        let end = r.find([',', '}', ']']).unwrap_or(r.len());
        let word = r[..end].trim();
        self.i += end;
        Some(match word {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" => Value::Null,
            w => serde_json::from_str::<serde_json::Number>(w).map(Value::Number).unwrap_or_else(|_| Value::String(w.to_string())),
        })
    }

    fn key(&mut self) -> Option<String> {
        self.skip_ws();
        let r = self.rest();
        if let Some(q) = r.strip_prefix(QUOTE) {
            let end = q.find(QUOTE)?;
            self.i += QUOTE.len() + end + QUOTE.len();
            return Some(q[..end].to_string());
        }
        let end = r.find(':')?;
        self.i += end;
        Some(r[..end].trim().trim_matches('"').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_tool_name_written_loosely_resolves_to_the_offered_tool() {
        let tools: Vec<ToolSpec> = ["page.fill", "page.click", "tabs.list", "web.search"].iter().map(|n| ToolSpec::new(*n, "")).collect();
        let r = |n: &str| resolve_tool_name(n, &tools);
        assert_eq!(r("page.fill").as_deref(), Some("page.fill"));
        assert_eq!(r("default_api:page.fill").as_deref(), Some("page.fill"));
        assert_eq!(r("page_fill").as_deref(), Some("page.fill"));
        assert_eq!(r("Page_Click").as_deref(), Some("page.click"));
        assert_eq!(r("fill").as_deref(), Some("page.fill"));
        assert_eq!(r("search").as_deref(), Some("web.search"));
        assert_eq!(r("page.type"), None);
        assert_eq!(r("list.tabs"), None);
    }

    #[test]
    fn a_gemma_call_becomes_a_tool_call() {
        let said = "<|tool_call>call:web.search{query:<|\"|>how to gracefully shut down a tokio application<|\"|>}<tool_call|>";
        let (text, calls) = split_native_calls(said);
        assert_eq!(text, "");
        assert_eq!(calls, vec![("web.search".to_string(), json!({"query": "how to gracefully shut down a tokio application"}))]);
    }

    #[test]
    fn nested_values_numbers_and_text_around_calls() {
        let said =
            "Let me look. <|tool_call>call:page.scroll{to:<|\"|>bottom<|\"|>,by:3,smooth:true,tags:[<|\"|>a, b<|\"|>,2],o:{k:null}}<tool_call|> ok";
        let (text, calls) = split_native_calls(said);
        assert_eq!(text, "Let me look.  ok");
        assert_eq!(calls[0].0, "page.scroll");
        assert_eq!(calls[0].1, json!({"to": "bottom", "by": 3, "smooth": true, "tags": ["a, b", 2], "o": {"k": null}}));
        let (_, none) = split_native_calls("<|tool_call>call:tabs.list{}<tool_call|>");
        assert_eq!(none, vec![("tabs.list".to_string(), json!({}))]);
        assert_eq!(split_native_calls("plain answer [1]").0, "plain answer [1]");
    }

    #[test]
    fn streamed_text_holds_back_calls_split_across_chunks() {
        let mut h = HoldBack::default();
        let mut out = String::new();
        for piece in ["Sure", " <|tool", "_call>call:web.search{query:", "<|\"|>x<|\"|>}<tool", "_call|> done", " <|"] {
            out.push_str(&h.feed(piece));
        }
        out.push_str(&h.finish());
        assert_eq!(out, "Sure  done <|");
    }

    struct Says(&'static str);

    #[async_trait]
    impl ModelBackend for Says {
        fn name(&self) -> String {
            "says".into()
        }
        async fn complete(&self, _: &[Message], _: &[ToolSpec], deltas: &DeltaSink, _: &CancelToken) -> Result<ModelTurn, ModelError> {
            for chunk in self.0.split_inclusive(' ') {
                deltas.text(chunk);
            }
            Ok(ModelTurn { text: self.0.into(), tool_calls: vec![] })
        }
    }

    #[test]
    fn the_wrapper_streams_clean_text_and_returns_the_calls() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let m = NativeCalls::new(Arc::new(Says("Searching <|tool_call>call:web.search{query:<|\"|>tokio shutdown<|\"|>}<tool_call|>")));
        let turn = rt.block_on(m.complete(&[], &[], &DeltaSink::new(Some(tx)), &CancelToken::new())).unwrap();
        assert_eq!(turn.text, "Searching");
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].arguments, json!({"query": "tokio shutdown"}));
        let mut streamed = String::new();
        while let Ok(AgentEvent::TextDelta(t)) = rx.try_recv() {
            streamed.push_str(&t);
        }
        assert_eq!(streamed, "Searching ");
        assert_eq!(m.name(), "says");
    }
}
