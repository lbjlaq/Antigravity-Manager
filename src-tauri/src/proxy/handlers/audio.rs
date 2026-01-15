use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use tracing::{debug, info};
use uuid::Uuid;

use crate::proxy::{
    audio::AudioProcessor,
    server::AppState,
};

/// 处理音频转录请求 (OpenAI Whisper API 兼容)
pub async fn handle_audio_transcription(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut audio_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut model = "gemini-2.0-flash-exp".to_string();
    let mut prompt = "Generate a transcript of the speech.".to_string();

    // 1. 解析 multipart/form-data
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, format!("解析表单失败: {}", e))
    })? {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                audio_data = Some(field.bytes().await.map_err(|e| {
                    (StatusCode::BAD_REQUEST, format!("读取文件失败: {}", e))
                })?.to_vec());
            }
            "model" => {
                model = field.text().await.unwrap_or(model);
            }
            "prompt" => {
                prompt = field.text().await.unwrap_or(prompt);
            }
            _ => {}
        }
    }

    let audio_bytes = audio_data.ok_or((
        StatusCode::BAD_REQUEST,
        "缺少音频文件".to_string(),
    ))?;

    let file_name = filename.ok_or((
        StatusCode::BAD_REQUEST,
        "无法获取文件名".to_string(),
    ))?;

    info!(
        "收到音频转录请求: 文件={}, 大小={} bytes, 模型={}",
        file_name,
        audio_bytes.len(),
        model
    );

    // 2. 检测 MIME 类型
    let mime_type = AudioProcessor::detect_mime_type(&file_name)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // 3. 验证文件大小
    if AudioProcessor::exceeds_size_limit(audio_bytes.len()) {
        let size_mb = audio_bytes.len() as f64 / (1024.0 * 1024.0);
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "音频文件过大 ({:.1} MB)。最大支持 15 MB (约 16 分钟 MP3)。建议: 1) 压缩音频质量 2) 分段上传",
                size_mb
            ),
        ));
    }

    // 4. 使用 Inline Data 方式
    debug!("使用 Inline Data 方式处理");
    let base64_audio = AudioProcessor::encode_to_base64(&audio_bytes);

    // 5. 构建 Gemini 请求
    let gemini_request = json!({
        "contents": [{
            "parts": [
                {"text": prompt},
                {
                    "inlineData": {
                        "mimeType": mime_type,
                        "data": base64_audio
                    }
                }
            ]
        }]
    });

    // 6. 获取 Token 和上游客户端
    let token_manager = state.token_manager;
    let (access_token, project_id, email) = token_manager
        .get_token("text", false, None, &model)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;

    info!("使用账号: {}", email);

    // 7. 包装请求为 v1internal 格式
    let wrapped_body = json!({
        "project": project_id,
        "requestId": format!("audio-{}", Uuid::new_v4()),
        "request": gemini_request,
        "model": model,
        "userAgent": "antigravity",
        "requestType": "text"
    });

    // 8. 发送请求到 Gemini
    let upstream = state.upstream.clone();
    let response = upstream
        .call_v1_internal("generateContent", &access_token, wrapped_body, None)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("上游请求失败: {}", e)))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Gemini API 错误: {}", error_text),
        ));
    }

    let result: Value = response
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("解析响应失败: {}", e)))?;

    // 9. 提取文本响应（解包 v1internal 响应）
    let inner_response = result.get("response").unwrap_or(&result);
    let text = inner_response
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    info!("音频转录完成，返回 {} 字符", text.len());

    // 10. 返回标准格式响应
    Ok((
        StatusCode::OK,
        [("X-Account-Email", email.as_str())],
        Json(json!({
            "text": text
        }))
    ).into_response())
}
