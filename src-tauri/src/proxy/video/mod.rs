use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub struct VideoProcessor;

impl VideoProcessor {
    /// 检测视频 MIME 类型
    pub fn detect_mime_type(filename: &str) -> Result<String, String> {
        let ext = Path::new(filename)
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("无法获取文件扩展名")?;

        match ext.to_lowercase().as_str() {
            "mp4" | "m4v" => Ok("video/mp4".to_string()),
            "webm" => Ok("video/webm".to_string()),
            "mov" => Ok("video/quicktime".to_string()),
            "avi" => Ok("video/x-msvideo".to_string()),
            "wmv" => Ok("video/x-ms-wmv".to_string()),
            "flv" => Ok("video/x-flv".to_string()),
            "mkv" => Ok("video/x-matroska".to_string()),
            "3gp" => Ok("video/3gpp".to_string()),
            _ => Err(format!("不支持的视频格式: {}", ext)),
        }
    }

    /// 将视频数据编码为 Base64
    pub fn encode_to_base64(video_data: &[u8]) -> String {
        general_purpose::STANDARD.encode(video_data)
    }

    /// 判断文件是否超过内联大小建议限制 (20MB)
    pub fn exceeds_size_limit(size_bytes: usize) -> bool {
        const MAX_SIZE: usize = 20 * 1024 * 1024; // 20MB
        size_bytes > MAX_SIZE
    }
}

/// 将格式字符串归一化为 Gemini 支持的视频 MIME 类型
pub fn normalize_video_mime(format: &str) -> String {
    let f = format.trim().to_lowercase();
    let bare = f.strip_prefix("video/").unwrap_or(&f);
    match bare {
        "mp4" | "m4v" => "video/mp4".to_string(),
        "webm" => "video/webm".to_string(),
        "mov" | "quicktime" => "video/quicktime".to_string(),
        "avi" | "x-msvideo" => "video/x-msvideo".to_string(),
        "wmv" | "x-ms-wmv" => "video/x-ms-wmv".to_string(),
        "flv" | "x-flv" => "video/x-flv".to_string(),
        "mkv" | "x-matroska" => "video/x-matroska".to_string(),
        "3gp" | "3gpp" => "video/3gpp".to_string(),
        other => format!("video/{}", other),
    }
}

/// 根据文件路径/URL 的扩展名推断视频 MIME，失败时回退到 video/mp4
fn mime_from_path(path: &str) -> String {
    let clean = path.split(['?', '#']).next().unwrap_or(path);
    VideoProcessor::detect_mime_type(clean).unwrap_or_else(|_| "video/mp4".to_string())
}

/// 把 OpenAI 风格的视频引用转换为 Gemini part
/// 支持四种来源：
///   * `data:video/mp4;base64,...`  -> inlineData
///   * `http(s)://...`              -> fileData (fileUri)
///   * `file:///path` 或本地路径     -> 读盘后 inlineData
///   * 裸 base64                     -> inlineData (需要 declared_mime)
pub fn video_part_from_source(src: &str, declared_mime: Option<&str>) -> Option<Value> {
    let declared = declared_mime.map(normalize_video_mime);

    // 1) data: URL
    if src.starts_with("data:") {
        let pos = src.find(',')?;
        let meta = &src[5..pos];
        let mime_part = meta.split(';').next().unwrap_or("");
        let mime = if mime_part.contains('/') {
            normalize_video_mime(mime_part)
        } else {
            declared.clone().unwrap_or_else(|| "video/mp4".to_string())
        };
        let data = &src[pos + 1..];
        warn_if_oversized(data.len(), &mime);
        return Some(json!({ "inlineData": { "mimeType": mime, "data": data } }));
    }

    // 2) 远程 URL：交给 Gemini 侧拉取
    if src.starts_with("http://") || src.starts_with("https://") {
        let mime = declared.unwrap_or_else(|| mime_from_path(src));
        return Some(json!({ "fileData": { "fileUri": src, "mimeType": mime } }));
    }

    // 3) 本地文件 (file:// 或普通路径)
    let looks_like_path =
        src.starts_with("file://") || (src.len() < 4096 && Path::new(src).is_file());
    if looks_like_path {
        let file_path = if let Some(rest) = src.strip_prefix("file://") {
            #[cfg(target_os = "windows")]
            {
                rest.trim_start_matches('/').replace('/', "\\")
            }
            #[cfg(not(target_os = "windows"))]
            {
                rest.to_string()
            }
        } else {
            src.to_string()
        };

        match std::fs::read(&file_path) {
            Ok(bytes) => {
                if VideoProcessor::exceeds_size_limit(bytes.len()) {
                    tracing::warn!(
                        "[Video] 本地视频超过 20MB ({} bytes)，仍会尝试内联上传: {}",
                        bytes.len(),
                        file_path
                    );
                }
                let mime = declared.unwrap_or_else(|| mime_from_path(&file_path));
                let b64 = VideoProcessor::encode_to_base64(&bytes);
                tracing::debug!(
                    "[Video] 已加载本地视频 {} ({} bytes, {})",
                    file_path,
                    bytes.len(),
                    mime
                );
                return Some(json!({ "inlineData": { "mimeType": mime, "data": b64 } }));
            }
            Err(e) => {
                tracing::warn!("[Video] 读取本地视频失败 {}: {}", file_path, e);
                return None;
            }
        }
    }

    // 4) 裸 base64
    if src.is_empty() {
        return None;
    }
    let mime = declared.unwrap_or_else(|| "video/mp4".to_string());
    warn_if_oversized(src.len(), &mime);
    Some(json!({ "inlineData": { "mimeType": mime, "data": src } }))
}

fn warn_if_oversized(base64_len: usize, mime: &str) {
    let raw = (base64_len * 3) / 4;
    if VideoProcessor::exceeds_size_limit(raw) {
        tracing::warn!(
            "[Video] 内联视频 {} 约 {} bytes，超过 20MB 建议上限",
            mime,
            raw
        );
    }
}

// ============================================================================
// Google Veo 3.1 视频生成支持 (Issue #253)
// ============================================================================

/// 判定是否属于 Google Veo 视频生成系列模型
pub fn is_veo_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.starts_with("veo-") || lower == "veo" || lower.contains("veo-3") || lower.contains("veo-2")
}

/// 归一化 Veo 模型名称为官方标准 ID
pub fn normalize_veo_model(model: &str) -> Option<&'static str> {
    let lower = model.trim().to_lowercase();
    match lower.as_str() {
        "veo" | "veo-3" | "veo-3.1" | "veo-3.1-generate" | "veo-3-generate" => Some("veo-3.1"),
        "veo-2" | "veo-2.0" | "veo-2.0-generate-001" => Some("veo-2.0-generate-001"),
        _ if lower.starts_with("veo-3") => Some("veo-3.1"),
        _ if lower.starts_with("veo-2") => Some("veo-2.0-generate-001"),
        _ => None,
    }
}

/// OpenAI 格式兼容的视频生成请求体 (POST /v1/videos/generations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoGenerationRequest {
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
}

/// 视频生成响应项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

/// OpenAI 格式兼容的视频生成响应体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoGenerationResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub data: Vec<VideoData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// 构建 Google Veo 3.1 上游预测 Payload (predictLongRunning / generateVideo 契约)
pub fn build_veo_payload(req: &VideoGenerationRequest) -> Value {
    let aspect_ratio = req.aspect_ratio.as_deref().unwrap_or("16:9");
    let duration = req.duration_seconds.unwrap_or(5).clamp(3, 8);
    let fps = req.fps.unwrap_or(24);

    let mut instance = json!({
        "prompt": req.prompt,
    });

    // 如果携带图片参考（图生视频 Image-to-Video）
    if let Some(ref img_ref) = req.image {
        if let Some(part) = video_part_from_source(img_ref, None) {
            instance["image"] = part;
        }
    }

    json!({
        "instances": [instance],
        "parameters": {
            "aspectRatio": aspect_ratio,
            "durationSeconds": duration,
            "fps": fps,
            "sampleCount": 1
        }
    })
}

/// 解析上游返回的视频生成报文为 OpenAI 标准格式
pub fn parse_veo_response(resp: &Value, model_name: &str) -> VideoGenerationResponse {
    let now = chrono::Utc::now().timestamp();
    let task_id = format!("video-{}", uuid::Uuid::new_v4().simple());

    let mut data = Vec::new();

    // 检查标准 predictions 数组结构
    if let Some(predictions) = resp.get("predictions").and_then(|p| p.as_array()) {
        for pred in predictions {
            let mut item = VideoData {
                url: None,
                b64_json: None,
                revised_prompt: None,
            };

            if let Some(bytes_b64) = pred.get("bytesBase64Encoded").and_then(|b| b.as_str()) {
                item.b64_json = Some(bytes_b64.to_string());
            } else if let Some(uri) = pred.get("gcsUri").or_else(|| pred.get("uri")).and_then(|u| u.as_str()) {
                item.url = Some(uri.to_string());
            } else if let Some(video_obj) = pred.get("video") {
                if let Some(uri) = video_obj.get("uri").and_then(|u| u.as_str()) {
                    item.url = Some(uri.to_string());
                } else if let Some(b64) = video_obj.get("bytesBase64Encoded").and_then(|b| b.as_str()) {
                    item.b64_json = Some(b64.to_string());
                }
            }

            if let Some(prompt) = pred.get("revisedPrompt").and_then(|p| p.as_str()) {
                item.revised_prompt = Some(prompt.to_string());
            }

            data.push(item);
        }
    }

    // 检查 Gemini candidates 格式回退
    if data.is_empty() {
        if let Some(candidates) = resp.get("candidates").and_then(|c| c.as_array()) {
            for cand in candidates {
                if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(inline) = part.get("inlineData") {
                            if let Some(b64) = inline.get("data").and_then(|d| d.as_str()) {
                                data.push(VideoData {
                                    url: None,
                                    b64_json: Some(b64.to_string()),
                                    revised_prompt: None,
                                });
                            }
                        } else if let Some(file_data) = part.get("fileData") {
                            if let Some(uri) = file_data.get("fileUri").and_then(|u| u.as_str()) {
                                data.push(VideoData {
                                    url: Some(uri.to_string()),
                                    b64_json: None,
                                    revised_prompt: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // 若依然为空且有 response 包装
    if data.is_empty() {
        if let Some(inner) = resp.get("response") {
            return parse_veo_response(inner, model_name);
        }
    }

    VideoGenerationResponse {
        id: task_id,
        object: "video.generation".to_string(),
        created: now,
        model: model_name.to_string(),
        data,
        status: Some("completed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_url_to_inline_data() {
        let part = video_part_from_source("data:video/mp4;base64,AAAA", None).unwrap();
        assert_eq!(part["inlineData"]["mimeType"], "video/mp4");
        assert_eq!(part["inlineData"]["data"], "AAAA");
    }

    #[test]
    fn test_http_url_to_file_data() {
        let part = video_part_from_source("https://example.com/demo.webm", None).unwrap();
        assert_eq!(part["fileData"]["fileUri"], "https://example.com/demo.webm");
        assert_eq!(part["fileData"]["mimeType"], "video/webm");
    }

    #[test]
    fn test_is_veo_model_and_normalize() {
        assert!(is_veo_model("veo-3.1"));
        assert!(is_veo_model("veo-3"));
        assert!(is_veo_model("veo"));
        assert!(is_veo_model("veo-2.0-generate-001"));
        assert!(!is_veo_model("gemini-3.1-flash-image"));
        assert!(!is_veo_model("claude-sonnet-4-6"));

        assert_eq!(normalize_veo_model("veo"), Some("veo-3.1"));
        assert_eq!(normalize_veo_model("veo-3"), Some("veo-3.1"));
        assert_eq!(normalize_veo_model("veo-3.1"), Some("veo-3.1"));
        assert_eq!(normalize_veo_model("veo-2"), Some("veo-2.0-generate-001"));
        assert_eq!(normalize_veo_model("gpt-4o"), None);
    }

    #[test]
    fn test_build_veo_payload_and_parse_response() {
        let req = VideoGenerationRequest {
            prompt: "A neon cyberpunk car driving in rain".to_string(),
            model: Some("veo-3.1".to_string()),
            aspect_ratio: Some("16:9".to_string()),
            duration_seconds: Some(5),
            fps: Some(24),
            image: None,
            response_format: Some("url".to_string()),
        };

        let payload = build_veo_payload(&req);
        assert_eq!(payload["instances"][0]["prompt"], "A neon cyberpunk car driving in rain");
        assert_eq!(payload["parameters"]["aspectRatio"], "16:9");
        assert_eq!(payload["parameters"]["durationSeconds"], 5);

        let upstream_resp = json!({
            "predictions": [
                {
                    "gcsUri": "gs://bucket/sample_video.mp4",
                    "revisedPrompt": "A neon cyberpunk car driving in heavy rain at night"
                }
            ]
        });

        let parsed = parse_veo_response(&upstream_resp, "veo-3.1");
        assert_eq!(parsed.model, "veo-3.1");
        assert_eq!(parsed.object, "video.generation");
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].url.as_deref(), Some("gs://bucket/sample_video.mp4"));
        assert_eq!(
            parsed.data[0].revised_prompt.as_deref(),
            Some("A neon cyberpunk car driving in heavy rain at night")
        );
    }
}
