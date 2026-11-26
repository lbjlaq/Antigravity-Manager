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

    // 3) 本地文件 (file:// 或普通路径) — 仅限本机直连回环主请求，且须通过大小与容器魔数校验
    let is_file_uri = src.starts_with("file://");
    if !crate::proxy::pipeline::budget::can_read_local_files() {
        if is_file_uri {
            tracing::warn!("[Video] Denied local file URI outside loopback owner context");
            return None;
        }
    } else if is_file_uri || (src.len() < 4096 && Path::new(src).is_file()) {
        let file_path = crate::proxy::mappers::common_utils::strip_file_scheme(src);
        const MAX_VIDEO_BYTES: usize = 20 * 1024 * 1024;
        match crate::proxy::mappers::common_utils::read_bounded_local_file(
            &file_path,
            MAX_VIDEO_BYTES,
        ) {
            Ok(bytes) => {
                if !has_valid_video_magic(&bytes) {
                    tracing::warn!(
                        "[Video] Rejected local file failing video container magic check: {}",
                        file_path
                    );
                    return None;
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

fn has_valid_video_magic(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) || bytes.starts_with(b"FLV\x01") {
        return true;
    }
    if bytes.len() >= 8
        && matches!(
            &bytes[4..8],
            b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free"
        )
    {
        return true;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"AVI " {
        return true;
    }
    if bytes.len() >= 16 && bytes.starts_with(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        return true;
    }
    false
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

    #[tokio::test]
    async fn test_local_video_file_security_guards() {
        let dir = std::env::temp_dir().join(format!("ag_video_sec_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake_mp4 = dir.join("secret.mp4");
        std::fs::write(&fake_mp4, b"root:x:0:0:root:/root:/bin/bash").unwrap();
        let valid_mp4 = dir.join("clip.mp4");
        std::fs::write(&valid_mp4, b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00").unwrap();

        let fake_uri = format!("file://{}", fake_mp4.display());
        let valid_uri = format!("file://{}", valid_mp4.display());

        // 1. Denied outside loopback RequestContext
        assert!(video_part_from_source(&valid_uri, None).is_none());

        // 2. Inside loopback RequestContext: disguised text rejected, real MP4 allowed
        let ctx = crate::proxy::pipeline::budget::RequestContext::new(
            crate::proxy::pipeline::budget::deadline_now(),
        );
        ctx.set_allow_local_files(true);
        crate::proxy::pipeline::budget::scope(ctx, async {
            assert!(video_part_from_source(&fake_uri, None).is_none());
            let part = video_part_from_source(&valid_uri, None).unwrap();
            assert_eq!(part["inlineData"]["mimeType"], "video/mp4");
        })
        .await;

        let _ = std::fs::remove_dir_all(&dir);
    }
}
