use base64::{engine::general_purpose, Engine as _};
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
}
