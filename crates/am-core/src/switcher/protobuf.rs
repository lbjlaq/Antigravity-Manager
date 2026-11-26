//! Protobuf 协议编解码实用工具
//!
//! 用于 Antigravity IDE (VS Code 架构) `state.vscdb` 中的 `antigravityUnifiedStateSync` 字段编解码，
//! 严格还原 Protobuf 序列化结构，保障 IDE 前端 Webview 与语言服务即时同步账号状态。

use base64::{engine::general_purpose, Engine as _};

/// 编码 Protobuf Varint
pub fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    while value >= 0x80 {
        buf.push((value & 0x7F | 0x80) as u8);
        value >>= 7;
    }
    buf.push(value as u8);
    buf
}

/// 读取 Protobuf Varint（带 64 位溢出保护）
pub fn read_varint(data: &[u8], offset: usize) -> Result<(u64, usize), String> {
    let mut result = 0u64;
    let mut shift = 0u32;
    let mut pos = offset;

    loop {
        if pos >= data.len() {
            return Err("incomplete_data".to_string());
        }
        if shift >= 64 {
            return Err("varint_overflow".to_string());
        }
        let byte = data[pos];
        result |= ((byte & 0x7F) as u64) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }

    Ok((result, pos))
}

/// 跳过 Protobuf 字段（带边界与整数溢出检查）
pub fn skip_field(data: &[u8], offset: usize, wire_type: u8) -> Result<usize, String> {
    let end = match wire_type {
        0 => {
            let (_, new_offset) = read_varint(data, offset)?;
            new_offset
        }
        1 => offset
            .checked_add(8)
            .ok_or_else(|| "offset_overflow".to_string())?,
        2 => {
            let (length, content_offset) = read_varint(data, offset)?;
            let len_usize = usize::try_from(length).map_err(|_| "length_overflow".to_string())?;
            content_offset
                .checked_add(len_usize)
                .ok_or_else(|| "offset_overflow".to_string())?
        }
        5 => offset
            .checked_add(4)
            .ok_or_else(|| "offset_overflow".to_string())?,
        _ => return Err(format!("unknown_wire_type: {}", wire_type)),
    };
    if end > data.len() {
        return Err("incomplete_data".to_string());
    }
    Ok(end)
}

/// 编码长度分隔字段 (wire_type = 2)
pub fn encode_len_delim_field(field_num: u32, data: &[u8]) -> Vec<u8> {
    let tag = (field_num << 3) | 2;
    let mut f = encode_varint(tag as u64);
    f.extend(encode_varint(data.len() as u64));
    f.extend_from_slice(data);
    f
}

/// 编码字符串字段 (wire_type = 2)
pub fn encode_string_field(field_num: u32, value: &str) -> Vec<u8> {
    encode_len_delim_field(field_num, value.as_bytes())
}

/// 编码 Varint 字段 (wire_type = 0)
pub fn encode_varint_field(field_num: u32, value: u64) -> Vec<u8> {
    let tag = (field_num << 3) | 0;
    let mut f = encode_varint(tag as u64);
    f.extend(encode_varint(value));
    f
}

/// 创建 OAuthTokenInfo 消息
pub fn create_oauth_info(
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    mut is_gcp_tos: bool,
    id_token: Option<&str>,
    email: Option<&str>,
) -> Vec<u8> {
    // 自动纠正个人账号 (@gmail.com 等) 的 GCP 标志位以确保 IDE 兼容性
    if let Some(email_str) = email {
        let lower = email_str.to_lowercase();
        let is_personal = lower.ends_with("@gmail.com")
            || lower.ends_with("@outlook.com")
            || lower.ends_with("@hotmail.com")
            || lower.ends_with("@qq.com")
            || lower.ends_with("@163.com");

        if is_personal && is_gcp_tos {
            is_gcp_tos = false;
        }
    }

    // Field 1: access_token
    let field1 = encode_string_field(1, access_token);
    // Field 2: token_type = "Bearer"
    let field2 = encode_string_field(2, "Bearer");
    // Field 3: refresh_token
    let field3 = encode_string_field(3, refresh_token);

    // Field 4: expiry (Timestamp: seconds = 1, nanos = 2)
    let seconds_tag = (1 << 3) | 0;
    let mut timestamp_msg = encode_varint(seconds_tag);
    timestamp_msg.extend(encode_varint(expiry as u64));
    let nanos_tag = (2 << 3) | 0;
    timestamp_msg.extend(encode_varint(nanos_tag));
    timestamp_msg.extend(encode_varint(0));
    let field4 = encode_len_delim_field(4, &timestamp_msg);

    // Field 5: id_token
    let field5 = id_token.map(|it| encode_string_field(5, it));
    // Field 6: is_gcp_tos
    let field6 = is_gcp_tos.then(|| encode_varint_field(6, 1));

    let mut oauth_info = Vec::new();
    oauth_info.extend(field1);
    oauth_info.extend(field2);
    oauth_info.extend(field3);
    oauth_info.extend(field4);
    if let Some(f5) = field5 {
        oauth_info.extend(f5);
    }
    if let Some(f6) = field6 {
        oauth_info.extend(f6);
    }
    oauth_info
}

/// 创建最小化 UserStatus Payload (Field 3: email, Field 7: email)
pub fn create_minimal_user_status_payload(email: &str) -> Vec<u8> {
    [encode_string_field(3, email), encode_string_field(7, email)].concat()
}

/// 创建 unified-state Topic.data entry (Row format)
pub fn create_unified_topic_entry(sentinel_key: &str, payload: &[u8]) -> Vec<u8> {
    let row = encode_string_field(1, &general_purpose::STANDARD.encode(payload));
    let entry = [
        encode_string_field(1, sentinel_key),
        encode_len_delim_field(2, &row),
    ]
    .concat();
    encode_len_delim_field(1, &entry)
}

/// 创建 unified-state Topic.data raw entry (Row format 直接包含原始字节，用于 authStateWithContextSentinelKey)
pub fn create_unified_raw_topic_entry(sentinel_key: &str, raw_payload: &[u8]) -> Vec<u8> {
    let row = encode_len_delim_field(1, raw_payload);
    let entry = [
        encode_string_field(1, sentinel_key),
        encode_len_delim_field(2, &row),
    ]
    .concat();
    encode_len_delim_field(1, &entry)
}

/// 创建单一 unified state entry base64 字符串
pub fn create_unified_state_entry(sentinel_key: &str, payload: &[u8]) -> String {
    let row = encode_string_field(1, &general_purpose::STANDARD.encode(payload));
    let data_entry = [
        encode_string_field(1, sentinel_key),
        encode_len_delim_field(2, &row),
    ]
    .concat();
    let topic = encode_len_delim_field(1, &data_entry);
    general_purpose::STANDARD.encode(topic)
}

fn unified_topic_entry_key(entry_data: &[u8]) -> Option<String> {
    let mut offset = 0;
    while offset < entry_data.len() {
        let (tag, new_offset) = read_varint(entry_data, offset).ok()?;
        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;

        if field_num == 1 && wire_type == 2 {
            let (length, content_offset) = read_varint(entry_data, new_offset).ok()?;
            let length = length as usize;
            if content_offset + length <= entry_data.len() {
                return String::from_utf8(
                    entry_data[content_offset..content_offset + length].to_vec(),
                )
                .ok();
            }
        }
        offset = skip_field(entry_data, new_offset, wire_type).ok()?;
    }
    None
}

/// 从 Topic.data 中移除指定 map entry，保留同 topic 下其他 sentinel row
pub fn remove_unified_topic_entry(data: &[u8], target_key: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        let start_offset = offset;
        let (tag, new_offset) = read_varint(data, offset)?;
        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;
        let next_offset = skip_field(data, new_offset, wire_type)?;

        let should_remove = if field_num == 1 && wire_type == 2 {
            let (length, content_offset) = read_varint(data, new_offset)?;
            let length = length as usize;
            if content_offset + length > data.len() {
                return Err("Topic.data entry 数据不完整".to_string());
            }
            let entry = &data[content_offset..content_offset + length];
            unified_topic_entry_key(entry).as_deref() == Some(target_key)
        } else {
            false
        };

        if !should_remove {
            result.extend_from_slice(&data[start_offset..next_offset]);
        }
        offset = next_offset;
    }

    Ok(result)
}

/// 查找指定 Length-Delimited (wire_type = 2) 字段的首个原始字节内容
pub fn find_field(data: &[u8], target_field: u32) -> Result<Option<Vec<u8>>, String> {
    let mut offset = 0;
    while offset < data.len() {
        let (tag, new_offset) = match read_varint(data, offset) {
            Ok(v) => v,
            Err(_) => break,
        };
        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;

        if field_num == target_field && wire_type == 2 {
            let (length, content_offset) = read_varint(data, new_offset)?;
            let len_usize = usize::try_from(length).map_err(|_| "length_overflow".to_string())?;
            let end = content_offset
                .checked_add(len_usize)
                .ok_or_else(|| "offset_overflow".to_string())?;
            if end > data.len() {
                return Err("incomplete_data".to_string());
            }
            return Ok(Some(data[content_offset..end].to_vec()));
        }

        offset = skip_field(data, new_offset, wire_type)?;
    }
    Ok(None)
}

/// 查找指定 Length-Delimited (wire_type = 2) 字段的所有出现项
pub fn find_all_fields(data: &[u8], target_field: u32) -> Result<Vec<Vec<u8>>, String> {
    let mut results = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let (tag, new_offset) = match read_varint(data, offset) {
            Ok(v) => v,
            Err(_) => break,
        };
        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;

        if field_num == target_field && wire_type == 2 {
            let (length, content_offset) = read_varint(data, new_offset)?;
            let len_usize = usize::try_from(length).map_err(|_| "length_overflow".to_string())?;
            let end = content_offset
                .checked_add(len_usize)
                .ok_or_else(|| "offset_overflow".to_string())?;
            if end > data.len() {
                return Err("incomplete_data".to_string());
            }
            results.push(data[content_offset..end].to_vec());
            offset = end;
        } else {
            offset = skip_field(data, new_offset, wire_type)?;
        }
    }
    Ok(results)
}

/// 解码 `antigravityUnifiedStateSync` Topic/Row 结构并返回指定 sentinel key 的内层 payload
pub fn decode_unified_state_entry_with_target(
    outer_b64: &str,
    preferred_key: Option<&str>,
) -> Result<(String, Vec<u8>), String> {
    let outer_blob = general_purpose::STANDARD
        .decode(outer_b64.trim())
        .map_err(|e| format!("Outer Base64 decode failed: {}", e))?;

    decode_topic_row_payload_with_target(&outer_blob, preferred_key)
        .or_else(|_| decode_legacy_unified_state_entry(&outer_blob))
}

fn decode_topic_row_payload_with_target(
    topic_blob: &[u8],
    preferred_key: Option<&str>,
) -> Result<(String, Vec<u8>), String> {
    let data_entries = find_all_fields(topic_blob, 1)?;
    if data_entries.is_empty() {
        return Err("Topic data entry not found".to_string());
    }

    let mut first_valid = None;
    for data_entry in data_entries {
        let Some(sentinel_key_bytes) = find_field(&data_entry, 1)? else {
            continue;
        };
        let Ok(sentinel_key) = String::from_utf8(sentinel_key_bytes) else {
            continue;
        };
        let Some(row_blob) = find_field(&data_entry, 2)? else {
            continue;
        };
        let Some(encoded_payload_bytes) = find_field(&row_blob, 1)? else {
            continue;
        };
        let payload = if let Ok(encoded_str) = String::from_utf8(encoded_payload_bytes.clone()) {
            general_purpose::STANDARD
                .decode(&encoded_str)
                .unwrap_or_else(|_| encoded_str.into_bytes())
        } else {
            encoded_payload_bytes
        };

        if let Some(pref) = preferred_key {
            if sentinel_key == pref {
                return Ok((sentinel_key, payload));
            }
        }
        if first_valid.is_none() {
            first_valid = Some((sentinel_key, payload));
        }
    }

    first_valid.ok_or_else(|| "No valid topic row payload decoded".to_string())
}

fn decode_legacy_unified_state_entry(outer_blob: &[u8]) -> Result<(String, Vec<u8>), String> {
    let inner_blob =
        find_field(outer_blob, 1)?.ok_or_else(|| "Outer Field 1 not found".to_string())?;
    let sentinel_key = String::from_utf8(
        find_field(&inner_blob, 1)?.ok_or_else(|| "Inner Field 1 not found".to_string())?,
    )
    .map_err(|_| "Sentinel key is not UTF-8".to_string())?;
    let raw_payload =
        find_field(&inner_blob, 2)?.ok_or_else(|| "Inner Field 2 not found".to_string())?;
    let looks_b64 = !raw_payload.is_empty()
        && raw_payload.len() % 4 == 0
        && raw_payload
            .iter()
            .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='));
    let payload = if looks_b64 {
        general_purpose::STANDARD
            .decode(&raw_payload)
            .ok()
            .filter(|d| !d.is_empty())
            .unwrap_or(raw_payload)
    } else {
        raw_payload
    };
    Ok((sentinel_key, payload))
}
