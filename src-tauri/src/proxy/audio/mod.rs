use base64::{engine::general_purpose, Engine as _};
use std::path::Path;

pub struct AudioProcessor;

impl AudioProcessor {
    /// 检测音频 MIME 类型
    pub fn detect_mime_type(filename: &str) -> Result<String, String> {
        let ext = Path::new(filename)
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("无法获取文件扩展名")?;

        match ext.to_lowercase().as_str() {
            "mp3" => Ok("audio/mp3".to_string()),
            "wav" => Ok("audio/wav".to_string()),
            "m4a" => Ok("audio/aac".to_string()),
            "ogg" => Ok("audio/ogg".to_string()),
            "flac" => Ok("audio/flac".to_string()),
            "aiff" | "aif" => Ok("audio/aiff".to_string()),
            _ => Err(format!("不支持的音频格式: {}", ext)),
        }
    }

    /// 将音频数据编码为 Base64
    pub fn encode_to_base64(audio_data: &[u8]) -> String {
        general_purpose::STANDARD.encode(audio_data)
    }

    /// 判断文件是否超过大小限制
    pub fn exceeds_size_limit(size_bytes: usize) -> bool {
        const MAX_SIZE: usize = 15 * 1024 * 1024; // 15MB
        size_bytes > MAX_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_mime_type() {
        assert_eq!(
            AudioProcessor::detect_mime_type("audio.mp3").unwrap(),
            "audio/mp3"
        );
        assert_eq!(
            AudioProcessor::detect_mime_type("audio.wav").unwrap(),
            "audio/wav"
        );
        assert!(AudioProcessor::detect_mime_type("audio.txt").is_err());
    }

    #[test]
    fn test_exceeds_size_limit() {
        assert!(!AudioProcessor::exceeds_size_limit(10 * 1024 * 1024)); // 10MB
        assert!(AudioProcessor::exceeds_size_limit(20 * 1024 * 1024)); // 20MB
        assert!(AudioProcessor::exceeds_size_limit(15 * 1024 * 1024 + 1)); // 刚好超过
        assert!(!AudioProcessor::exceeds_size_limit(15 * 1024 * 1024)); // 刚好等于限制
    }

    #[test]
    fn test_base64_encoding() {
        let data = b"test audio data";
        let encoded = AudioProcessor::encode_to_base64(data);
        assert!(!encoded.is_empty());
    }
}
