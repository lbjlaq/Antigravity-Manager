use std::sync::Arc;
use serde::{Deserialize, Serialize};
// use serde_json::Value;

// ===== OpenAI 格式定义 =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Array(Vec<ContentPart>),
}

impl MessageContent {
    /// 获取文本内容的预览
    pub fn preview(&self) -> String {
        match self {
            MessageContent::Text(s) => if s.chars().count() > 200 { format!("{}...", s.chars().take(200).collect::<String>()) } else { s.clone() },
            MessageContent::Array(parts) => {
                let mut s = String::new();
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        s.push_str(text);
                    }
                }
                if s.chars().count() > 200 { format!("{}...", s.chars().take(200).collect::<String>()) } else { s }
            }
        }
    }

    /// 获取完整文本内容
    pub fn text(&self) -> String {
         match self {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Array(parts) => {
                let mut s = String::new();
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        s.push_str(text);
                    }
                }
                s
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(flatten)]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
}

// ===== Anthropic 格式定义 =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String, signature: Option<String> },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String, // "image/jpeg", "image/png"
    pub data: String, // base64 string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    // Anthropic content is always a list of blocks, but incoming JSON might process single string? 
    // Officially it can be string or array of blocks.
    #[serde(deserialize_with = "deserialize_anthropic_content")]
    pub content: Vec<AnthropicContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>, // 新增：用于存放思维链签名
}

// Custom deserializer to handle content being either string or array
fn deserialize_anthropic_content<'de, D>(deserializer: D) -> Result<Vec<AnthropicContent>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ContentVisitor;

    impl<'de> serde::de::Visitor<'de> for ContentVisitor {
        type Value = Vec<AnthropicContent>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or list of content blocks")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<AnthropicContent>, E>
        where
            E: serde::de::Error,
        {
            Ok(vec![AnthropicContent::Text { text: value.to_string() }])
        }

        fn visit_seq<V>(self, mut visitor: V) -> Result<Vec<AnthropicContent>, V::Error>
        where
            V: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = visitor.next_element()? {
                vec.push(elem);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(ContentVisitor)
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicChatRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(default, deserialize_with = "deserialize_anthropic_system")]
    pub system: Option<String>, // System prompt is top-level, supports string or array
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
}

// Custom deserializer for system field (supports both string and array formats)
fn deserialize_anthropic_system<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Deserialize;
    
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SystemField {
        String(String),
        Array(Vec<SystemBlock>),
    }
    
    #[derive(Deserialize)]
    struct SystemBlock {
        #[serde(rename = "type")]
        block_type: String,
        text: String,
    }
    
    let value = Option::<SystemField>::deserialize(deserializer)?;
    Ok(value.map(|v| match v {
        SystemField::String(s) => s,
        SystemField::Array(blocks) => {
            blocks.into_iter()
                .filter(|b| b.block_type == "text")
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join("\n")
        }
    }))
}

// ===== Gemini 格式定义 =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "inlineData")]
    pub inline_data: Option<GeminiInlineData>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "thoughtSignature")]
    pub thought_signature: Option<String>, // 新增
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}



// ===== 格式转换 =====

/// 将 OpenAI messages 转换为 Gemini contents 数组（用于 Antigravity API）
pub fn convert_openai_to_gemini_contents(messages: &Vec<OpenAIMessage>) -> Vec<GeminiContent> {
    let mut contents = Vec::new();
    // 预编译正则，提取 markdown 图片：![alt](data:image/png;base64,.....)
    // 捕获组1: mime type, 捕获组2: base64 data (允许空格/换行)
    let re = regex::Regex::new(r"!\[.*?\]\(data:\s*(image/[a-zA-Z+-]+)\s*;\s*base64\s*,\s*([a-zA-Z0-9+/=\s]+)\)").unwrap();
    
    // 正则用于从 data URL 中提取 base64
    let re_data_url = regex::Regex::new(r"data:\s*(image/[a-zA-Z+-]+)\s*;\s*base64\s*,\s*([a-zA-Z0-9+/=\s]+)").unwrap();

    let mut pending_images: Vec<GeminiInlineData> = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        // Debug: 查看消息内容预览
        let preview = msg.content.preview();
        tracing::info!("Msg[{}][{}] content={:?}", i, msg.role, preview);

        // 角色映射
        let role = match msg.role.as_str() {
            "assistant" => "model",
            "system" => "user",
            _ => &msg.role,
        };
        
        let mut parts = Vec::new();
        
        // 1. 处理 Pending Images (Assist 历史图片注入到 User)
        if role == "user" && !pending_images.is_empty() {
             let count = pending_images.len();
             tracing::info!("向 User 消息注入 {} 张待处理图片 (上下文携带)", count);
             for img in pending_images.drain(..) {
                parts.push(GeminiPart {
                    text: None,
                    inline_data: Some(img),
                    thought_signature: None,
                });
            }
        }

        // 2. 解析当前消息内容 (支持 String 和 Array)
        match &msg.content {
            MessageContent::Text(text) => {
                // 处理 String 格式 (解析 Markdown 图片)
                let mut last_end = 0;
                for caps in re.captures_iter(text) {
                    let match_start = caps.get(0).unwrap().start();
                    let match_end = caps.get(0).unwrap().end();
                    
                    if match_start > last_end {
                        let text_part = &text[last_end..match_start];
                        if !text_part.is_empty() {
                            parts.push(GeminiPart { text: Some(text_part.to_string()), inline_data: None, thought_signature: None });
                        }
                    }
                    
                    let mime_type = caps.get(1).unwrap().as_str().to_string();
                    let data = caps.get(2).unwrap().as_str().replace(|c: char| c.is_whitespace(), "");
                    let inline_data = GeminiInlineData { mime_type, data };

                    if role == "model" {
                        pending_images.push(inline_data);
                    } else {
                        parts.push(GeminiPart { text: None, inline_data: Some(inline_data), thought_signature: None });
                    }
                    last_end = match_end;
                }
                if last_end < text.len() {
                    let text_part = &text[last_end..];
                    if !text_part.is_empty() {
                        parts.push(GeminiPart { text: Some(text_part.to_string()), inline_data: None, thought_signature: None });
                    }
                }
            },
            MessageContent::Array(content_parts) => {
                // 处理 Array 格式 (多模态)
                for part in content_parts {
                    match part {
                        ContentPart::Text { text } => {
                            parts.push(GeminiPart { text: Some(text.clone()), inline_data: None, thought_signature: None });
                        },
                        ContentPart::ImageUrl { image_url } => {
                            let url = &image_url.url;
                            if let Some(caps) = re_data_url.captures(url) {
                                let mime_type = caps.get(1).unwrap().as_str().to_string();
                                let data = caps.get(2).unwrap().as_str().replace(|c: char| c.is_whitespace(), "");
                                let inline_data = GeminiInlineData { mime_type: mime_type.clone(), data };
                                
                                if role == "model" {
                                    // 理论上 Model 消息不应该发这里，但防以后
                                    pending_images.push(inline_data);
                                } else {
                                    tracing::info!("解析到 Multimodal 图片数据 (Mime: {})", mime_type);
                                    parts.push(GeminiPart { text: None, inline_data: Some(inline_data), thought_signature: None });
                                }
                            } else {
                                tracing::warn!("忽略不支持的图片 URL 格式: {}", url);
                            }
                        }
                    }
                }
            }
        }
        
        // 3. 补全与清理
        if role == "model" && parts.is_empty() && !pending_images.is_empty() {
            parts.push(GeminiPart {
                text: Some("[Image Generated]".to_string()), 
                inline_data: None,
                thought_signature: None,
            });
        }

        if parts.is_empty() {
            parts.push(GeminiPart {
                text: Some("".to_string()),
                inline_data: None,
                thought_signature: None,
            });
        }
        
        contents.push(GeminiContent {
            role: role.to_string(),
            parts,
        });
    }
    
    // 合并连续 User 消息
    let mut i = 1;
    while i < contents.len() {
        if contents[i].role == "user" && contents[i-1].role == "user" {
            let mut parts_to_append = contents[i].parts.clone();
            
            let need_separator = if let Some(last_part) = contents[i-1].parts.last() {
                if let Some(first_part) = parts_to_append.first() {
                    last_part.text.is_some() && first_part.text.is_some()
                } else {
                    false
                }
            } else {
                false
            };
            
            if need_separator {
                contents[i-1].parts.push(GeminiPart {
                    text: Some("\n\n".to_string()),
                    inline_data: None,
                    thought_signature: None,
                });
            }
            
            contents[i-1].parts.append(&mut parts_to_append);
            contents.remove(i);
        } else {
            i += 1;
        }
    }
    
    contents
}

/// 将 Anthropic request 转换为 Gemini contents 数组
pub fn _convert_anthropic_to_gemini_contents(request: &AnthropicChatRequest) -> Vec<GeminiContent> {
    let mut contents = Vec::new();
    
    // 1. 处理 System Prompt
    // Gemini 将 System Prompt 视为 user 消息的一部分，或者放到 systemInstruction 中 (client.rs 处理 systemInstruction)
    // 这里我们仅处理 messages 部分。System prompt 将在 client.rs 中通过 systemInstruction 处理，
    // 或者如果需要兼容性，也可以在这里转为 User message。
    // ANTIGRAVITY 策略: System prompt 尽可能放到 systemInstruction。
    // 但是，Client 端的 convert 方法只接受 messages 向量，因此需要在 Client 中显式地把 request.system 拿出来。
    // converter 的这个函数只负责转换 messages 列表。

    for msg in &request.messages {
        let role = match msg.role.as_str() {
            "assistant" => "model",
            "user" => "user",
            _ => "user", // Default fallback
        };

        let mut parts = Vec::new();

        for content in &msg.content {
            match content {
                AnthropicContent::Text { text } => {
                    parts.push(GeminiPart {
                        text: Some(text.clone()),
                        inline_data: None,
                        thought_signature: None,
                    });
                },
                AnthropicContent::Image { source } => {
                    // source_type: "base64", media_type: "image/jpeg", data: "..."
                    if source.source_type == "base64" {
                        parts.push(GeminiPart {
                            text: None,
                            inline_data: Some(GeminiInlineData {
                                mime_type: source.media_type.clone(),
                                data: source.data.clone(),
                            }),
                            thought_signature: None,
                        });
                    }
                },
                AnthropicContent::Thinking { .. } => {
                    // Gemini 目前不支持直接接收 thinking 类型的输入块，忽略之以避免 400 错误
                    // 或者可以考虑将其转换为 text，但可能会干扰模型
                    tracing::debug!("Ignoring thinking block in input message");
                }
            }
        }

        contents.push(GeminiContent {
            role: role.to_string(),
            parts,
        });
    }

    // 合并连续 User 消息 (Gemini 不允许 consecutive user messages without model response)
    let mut i = 1;
    while i < contents.len() {
        if contents[i].role == contents[i-1].role {
             let mut parts_to_append = contents[i].parts.clone();
             contents[i-1].parts.append(&mut parts_to_append);
             contents.remove(i);
        } else {
            i += 1;
        }
    }

    contents
}

/// 支持思维链签名的 Anthropic -> Gemini 转换
pub async fn convert_anthropic_to_gemini_contents_ext(
    request: &AnthropicChatRequest,
    signature_map: Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>
) -> Vec<GeminiContent> {
    let mut contents = Vec::new();
    let map = signature_map.lock().await;

    for msg in &request.messages {
        let role = match msg.role.as_str() {
            "assistant" => "model",
            "user" => "user",
            _ => "user",
        };

        let mut parts = Vec::new();

        for content in &msg.content {
            match content {
                AnthropicContent::Text { text } => {
                    parts.push(GeminiPart {
                        text: Some(text.clone()),
                        inline_data: None,
                        thought_signature: None,
                    });
                },
                AnthropicContent::Image { source } => {
                    if source.source_type == "base64" {
                        parts.push(GeminiPart {
                            text: None,
                            inline_data: Some(GeminiInlineData {
                                mime_type: source.media_type.clone(),
                                data: source.data.clone(),
                            }),
                            thought_signature: None,
                        });
                    }
                },
                AnthropicContent::Thinking { .. } => {
                    // 同样忽略 Thinking 输入
                    tracing::debug!("Ignoring thinking block in input message (ext)");
                }
            }
        }
        
        // 尝试回传 thoughtSignature
        // 如果是最后一轮的 assistant 消息，且我们暂存了签名，则尝试回填
        if role == "model" {
            let sig = map.get("latest").cloned();
            if let Some(s) = sig {
                parts.push(GeminiPart {
                    text: None,
                    inline_data: None,
                    thought_signature: Some(s),
                });
            }
        }
        
        contents.push(GeminiContent {
            role: role.to_string(),
            parts,
        });
    }
    
    // 合并
    let mut i = 1;
    while i < contents.len() {
        if contents[i].role == contents[i-1].role {
             let mut parts_to_append = contents[i].parts.clone();
             contents[i-1].parts.append(&mut parts_to_append);
             contents.remove(i);
        } else {
            i += 1;
        }
    }

    contents
}

// 移除文件末尾多余的 Arc 引用
