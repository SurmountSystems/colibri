//! Minimal chat template rendering for native hosts (no Python).
//!
//! Ports the text-only subsets of `c/openai_server.py`:
//! `render_chat` (GLM), `render_chat_kimi`, `render_chat_v4`, `render_chat_inkling`.
//!
//! Tool-calling and Inkling audio are **not** fully ported; text multi-turn is
//! enough for a GPUI / embed desktop chat path. Prefer this over OpenAI HTTP
//! when the host already owns the conversation UI.

use crate::error::{Error, Result};
use crate::model::ModelFamily;

/// Message role for chat templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

impl ChatRole {
    /// Parse common role strings (`system`, `user`, …).
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "system" => Ok(Self::System),
            "developer" => Ok(Self::Developer),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            other => Err(Error::invalid(format!("unsupported message role: {other}"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Developer => "developer",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// One chat turn for template rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    /// Plain text content (multipart OpenAI content is host-side flattened).
    pub content: String,
    /// Assistant reasoning / thinking text when the family uses it.
    pub reasoning_content: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
            reasoning_content: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
            reasoning_content: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            reasoning_content: None,
        }
    }

    pub fn assistant_with_reasoning(
        content: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            reasoning_content: Some(reasoning.into()),
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            content: content.into(),
            reasoning_content: None,
        }
    }
}

/// Options that affect template generation prompts.
#[derive(Debug, Clone, Default)]
pub struct ChatRenderOptions {
    /// Open a thinking / reasoning prefix for the new assistant turn.
    pub enable_thinking: bool,
    /// Effort hint: family-specific (`high` / `max` for GLM, none|minimal|… for Inkling).
    pub reasoning_effort: Option<String>,
}

/// Render messages for `family` into an engine SUBMIT prompt string.
///
/// Dispatches to the same family templates as Python `openai_server` (text-only).
pub fn render_chat(
    messages: &[ChatMessage],
    family: ModelFamily,
    opts: &ChatRenderOptions,
) -> Result<String> {
    if messages.is_empty() {
        return Err(Error::invalid("`messages` must be a non-empty array"));
    }
    match family {
        ModelFamily::Glm | ModelFamily::Olmoe => render_chat_glm(messages, opts),
        ModelFamily::Kimi => render_chat_kimi(messages, opts),
        ModelFamily::DeepseekV4 => render_chat_v4(messages, opts),
        ModelFamily::Inkling => render_chat_inkling(messages, opts),
    }
}

/// Convenience: render with default options (no thinking).
pub fn render_chat_simple(messages: &[ChatMessage], family: ModelFamily) -> Result<String> {
    render_chat(messages, family, &ChatRenderOptions::default())
}

// ---- GLM-5.2 (and OLMoE research path on the same binary) --------------------

fn render_chat_glm(messages: &[ChatMessage], opts: &ChatRenderOptions) -> Result<String> {
    let mut prompt = String::from("[gMASK]<sop>");
    if opts.enable_thinking {
        let effort = match opts.reasoning_effort.as_deref() {
            Some("high") => "High",
            _ => "Max",
        };
        prompt.push_str(&format!("<|system|>Reasoning Effort: {effort}"));
    }
    let mut prev_tool = false;
    for message in messages {
        match message.role {
            ChatRole::System | ChatRole::Developer => {
                prompt.push_str("<|system|>");
                prompt.push_str(&message.content);
            }
            ChatRole::User => {
                prompt.push_str("<|user|>");
                prompt.push_str(&message.content);
            }
            ChatRole::Assistant => {
                let reasoning = message.reasoning_content.as_deref().unwrap_or("");
                prompt.push_str("<|assistant|><think>");
                prompt.push_str(reasoning);
                prompt.push_str("</think>");
                prompt.push_str(message.content.trim());
            }
            ChatRole::Tool => {
                if !prev_tool {
                    prompt.push_str("<|observation|>");
                }
                prompt.push_str("<tool_response>");
                prompt.push_str(&message.content);
                prompt.push_str("</tool_response>");
            }
        }
        prev_tool = message.role == ChatRole::Tool;
    }
    if opts.enable_thinking {
        prompt.push_str("<|assistant|><think>");
    } else {
        prompt.push_str("<|assistant|><think></think>");
    }
    Ok(prompt)
}

// ---- Kimi K3 (length-framed private payload) ---------------------------------

fn render_chat_kimi(messages: &[ChatMessage], opts: &ChatRenderOptions) -> Result<String> {
    let mut parts = String::from("K3CHAT1\n");
    for message in messages {
        match message.role {
            ChatRole::System | ChatRole::Developer | ChatRole::User => {
                let role = message.role.as_str();
                let role = if message.role == ChatRole::Developer {
                    "developer"
                } else {
                    role
                };
                let text = &message.content;
                let nbytes = text.len();
                parts.push_str(&format!("M {role} {nbytes}\n{text}"));
            }
            ChatRole::Assistant => {
                let text = &message.content;
                if opts.enable_thinking {
                    let reasoning = message.reasoning_content.as_deref().unwrap_or("");
                    let rn = reasoning.len();
                    let tn = text.len();
                    parts.push_str(&format!("A {rn} {tn}\n{reasoning}{text}"));
                } else {
                    let nbytes = text.len();
                    parts.push_str(&format!("M assistant {nbytes}\n{text}"));
                }
            }
            ChatRole::Tool => {
                return Err(Error::invalid(
                    "Tool role is not wired up for the Kimi K3 engine yet",
                ));
            }
        }
    }
    parts.push_str(&format!("G {}\n", if opts.enable_thinking { 1 } else { 0 }));
    Ok(parts)
}

// ---- DeepSeek V4 -------------------------------------------------------------

fn render_chat_v4(messages: &[ChatMessage], opts: &ChatRenderOptions) -> Result<String> {
    // Native markers use fullwidth vertical bar (U+FF5C) and lower-one-eighth
    // block (U+2581) as in openai_server.render_chat_v4.
    let bos = "<\u{ff5c}begin\u{2581}of\u{2581}sentence\u{ff5c}>";
    let user = "<\u{ff5c}User\u{ff5c}>";
    let assistant = "<\u{ff5c}Assistant\u{ff5c}>";
    let eos = "<\u{ff5c}end\u{2581}of\u{2581}sentence\u{ff5c}>";

    let mut parts = String::from(bos);
    for message in messages {
        match message.role {
            ChatRole::System | ChatRole::Developer => {
                parts.push_str(&message.content);
            }
            ChatRole::User => {
                parts.push_str(user);
                parts.push_str(&message.content);
            }
            ChatRole::Assistant => {
                parts.push_str(assistant);
                if let Some(reasoning) = message.reasoning_content.as_deref() {
                    if !reasoning.is_empty() {
                        parts.push_str("<think>");
                        parts.push_str(reasoning);
                        parts.push_str("</think>");
                    } else {
                        parts.push_str("</think>");
                    }
                } else {
                    parts.push_str("</think>");
                }
                parts.push_str(&message.content);
                parts.push_str(eos);
            }
            ChatRole::Tool => {
                return Err(Error::invalid(
                    "Tool role is not wired up for DeepSeek V4 yet",
                ));
            }
        }
    }
    parts.push_str(assistant);
    if opts.enable_thinking {
        parts.push_str("<think>");
    } else {
        parts.push_str("</think>");
    }
    Ok(parts)
}

// ---- Inkling (text-only) -----------------------------------------------------

fn render_chat_inkling(messages: &[ChatMessage], opts: &ChatRenderOptions) -> Result<String> {
    let effort = inkling_effort(opts);
    let effort_str = if effort == 0.0 {
        "<|message_system|><|content_text|>Thinking effort level: 0<|end_message|>".to_string()
    } else {
        format!("<|message_system|><|content_text|>Thinking effort level: {effort}<|end_message|>")
    };

    let mut prompt = String::new();
    let mut effort_emitted = false;
    for message in messages {
        let rtok = match message.role {
            ChatRole::User => "<|message_user|>",
            ChatRole::System | ChatRole::Developer => "<|message_system|>",
            ChatRole::Assistant => "<|message_model|>",
            ChatRole::Tool => "<|message_tool|>",
        };
        if !effort_emitted && !matches!(message.role, ChatRole::System | ChatRole::Developer) {
            prompt.push_str(&effort_str);
            effort_emitted = true;
        }
        prompt.push_str(rtok);
        prompt.push_str("<|content_text|>");
        prompt.push_str(&message.content);
        prompt.push_str("<|end_message|>");
        if message.role == ChatRole::Assistant {
            prompt.push_str("<|content_model_end_sampling|>");
        }
    }
    if !effort_emitted {
        prompt.push_str(&effort_str);
    }
    prompt.push_str("<|message_model|>");
    if effort == 0.0 {
        prompt.push_str("<|content_text|>");
    }
    Ok(prompt)
}

fn inkling_effort(opts: &ChatRenderOptions) -> f64 {
    if let Some(ref effort) = opts.reasoning_effort {
        match effort.as_str() {
            "none" => 0.0,
            "minimal" => 0.1,
            "low" => 0.2,
            "medium" => 0.7,
            "high" => 0.9,
            "max" => 0.99,
            _ => {
                if opts.enable_thinking {
                    0.9
                } else {
                    0.0
                }
            }
        }
    } else if opts.enable_thinking {
        0.9
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glm_multi_turn_golden() {
        let messages = vec![
            ChatMessage::system("You are helpful."),
            ChatMessage::user("Hi"),
            ChatMessage::assistant("Hello!"),
            ChatMessage::user("How are you?"),
        ];
        let out = render_chat_simple(&messages, ModelFamily::Glm).unwrap();
        assert_eq!(
            out,
            "[gMASK]<sop><|system|>You are helpful.<|user|>Hi\
             <|assistant|><think></think>Hello!\
             <|user|>How are you?<|assistant|><think></think>"
        );
    }

    #[test]
    fn glm_thinking_opens_think() {
        let messages = vec![ChatMessage::user("why?")];
        let out = render_chat(
            &messages,
            ModelFamily::Glm,
            &ChatRenderOptions {
                enable_thinking: true,
                reasoning_effort: Some("high".into()),
            },
        )
        .unwrap();
        assert!(out.starts_with("[gMASK]<sop><|system|>Reasoning Effort: High"));
        assert!(out.ends_with("<|assistant|><think>"));
        assert!(!out.ends_with("<|assistant|><think></think>"));
    }

    #[test]
    fn kimi_multi_turn_golden() {
        let messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi"),
            ChatMessage::user("again"),
        ];
        let out = render_chat_simple(&messages, ModelFamily::Kimi).unwrap();
        let expected = "K3CHAT1\n\
            M system 3\nsys\
            M user 5\nhello\
            M assistant 2\nhi\
            M user 5\nagain\
            G 0\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn v4_multi_turn_golden() {
        let messages = vec![
            ChatMessage::user("Hi"),
            ChatMessage::assistant("Yo"),
            ChatMessage::user("Next"),
        ];
        let out = render_chat_simple(&messages, ModelFamily::DeepseekV4).unwrap();
        let bos = "<\u{ff5c}begin\u{2581}of\u{2581}sentence\u{ff5c}>";
        let user = "<\u{ff5c}User\u{ff5c}>";
        let assistant = "<\u{ff5c}Assistant\u{ff5c}>";
        let eos = "<\u{ff5c}end\u{2581}of\u{2581}sentence\u{ff5c}>";
        let expected =
            format!("{bos}{user}Hi{assistant}</think>Yo{eos}{user}Next{assistant}</think>");
        assert_eq!(out, expected);
    }

    #[test]
    fn inkling_multi_turn_golden() {
        let messages = vec![
            ChatMessage::system("Be brief."),
            ChatMessage::user("Hi"),
            ChatMessage::assistant("Hello"),
            ChatMessage::user("Bye"),
        ];
        let out = render_chat_simple(&messages, ModelFamily::Inkling).unwrap();
        let expected = "\
            <|message_system|><|content_text|>Be brief.<|end_message|>\
            <|message_system|><|content_text|>Thinking effort level: 0<|end_message|>\
            <|message_user|><|content_text|>Hi<|end_message|>\
            <|message_model|><|content_text|>Hello<|end_message|><|content_model_end_sampling|>\
            <|message_user|><|content_text|>Bye<|end_message|>\
            <|message_model|><|content_text|>";
        assert_eq!(out, expected);
    }

    #[test]
    fn empty_messages_err() {
        let err = render_chat_simple(&[], ModelFamily::Glm).unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn olmoe_uses_glm_template() {
        let messages = vec![ChatMessage::user("x")];
        let a = render_chat_simple(&messages, ModelFamily::Glm).unwrap();
        let b = render_chat_simple(&messages, ModelFamily::Olmoe).unwrap();
        assert_eq!(a, b);
    }
}
