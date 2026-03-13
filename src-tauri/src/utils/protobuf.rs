/// Protobuf Varint Encoding
pub fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    while value >= 0x80 {
        buf.push((value & 0x7F | 0x80) as u8);
        value >>= 7;
    }
    buf.push(value as u8);
    buf
}

/// Read Protobuf Varint
pub fn read_varint(data: &[u8], offset: usize) -> Result<(u64, usize), String> {
    let mut result = 0u64;
    let mut shift = 0;
    let mut pos = offset;

    loop {
        if pos >= data.len() {
            return Err("incomplete_data".to_string());
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

/// Skip Protobuf Field
pub fn skip_field(data: &[u8], offset: usize, wire_type: u8) -> Result<usize, String> {
    match wire_type {
        0 => {
            // Varint
            let (_, new_offset) = read_varint(data, offset)?;
            Ok(new_offset)
        }
        1 => {
            // 64-bit
            Ok(offset + 8)
        }
        2 => {
            // Length-delimited
            let (length, content_offset) = read_varint(data, offset)?;
            Ok(content_offset + length as usize)
        }
        5 => {
            // 32-bit
            Ok(offset + 4)
        }
        _ => Err(format!("unknown_wire_type: {}", wire_type)),
    }
}

/// Remove specified Protobuf field
pub fn remove_field(data: &[u8], field_num: u32) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        let start_offset = offset;
        let (tag, new_offset) = read_varint(data, offset)?;
        let wire_type = (tag & 7) as u8;
        let current_field = (tag >> 3) as u32;

        if current_field == field_num {
            // Skip this field
            offset = skip_field(data, new_offset, wire_type)?;
        } else {
            // Keep other fields
            let next_offset = skip_field(data, new_offset, wire_type)?;
            result.extend_from_slice(&data[start_offset..next_offset]);
            offset = next_offset;
        }
    }

    Ok(result)
}

/// Find specified Protobuf field content (Length-Delimited only)
pub fn find_field(data: &[u8], target_field: u32) -> Result<Option<Vec<u8>>, String> {
    let mut offset = 0;

    while offset < data.len() {
        let (tag, new_offset) = match read_varint(data, offset) {
            Ok(v) => v,
            Err(_) => break, // Incomplete data, stop
        };

        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;

        if field_num == target_field && wire_type == 2 {
            let (length, content_offset) = read_varint(data, new_offset)?;
            return Ok(Some(
                data[content_offset..content_offset + length as usize].to_vec(),
            ));
        }

        // Skip field
        offset = skip_field(data, new_offset, wire_type)?;
    }

    Ok(None)
}

/// Create OAuthTokenInfo (Field 6)
///
/// Structure:
/// message OAuthTokenInfo {
///     optional string access_token = 1;
///     optional string token_type = 2;
///     optional string refresh_token = 3;
///     optional Timestamp expiry = 4;
/// }
pub fn create_oauth_field(access_token: &str, refresh_token: &str, expiry: i64) -> Vec<u8> {
    // Field 1: access_token (string, wire_type = 2)
    let tag1 = (1 << 3) | 2;
    let field1 = {
        let mut f = encode_varint(tag1);
        f.extend(encode_varint(access_token.len() as u64));
        f.extend(access_token.as_bytes());
        f
    };

    // Field 2: token_type (string, fixed value "Bearer", wire_type = 2)
    let tag2 = (2 << 3) | 2;
    let token_type = "Bearer";
    let field2 = {
        let mut f = encode_varint(tag2);
        f.extend(encode_varint(token_type.len() as u64));
        f.extend(token_type.as_bytes());
        f
    };

    // Field 3: refresh_token (string, wire_type = 2)
    let tag3 = (3 << 3) | 2;
    let field3 = {
        let mut f = encode_varint(tag3);
        f.extend(encode_varint(refresh_token.len() as u64));
        f.extend(refresh_token.as_bytes());
        f
    };

    // Field 4: expiry (Nested Timestamp message, wire_type = 2)
    // Timestamp message contains: Field 1: seconds (int64, wire_type = 0)
    let timestamp_tag = (1 << 3) | 0; // Field 1, varint
    let timestamp_msg = {
        let mut m = encode_varint(timestamp_tag);
        m.extend(encode_varint(expiry as u64));
        m
    };

    let tag4 = (4 << 3) | 2; // Field 4, length-delimited
    let field4 = {
        let mut f = encode_varint(tag4);
        f.extend(encode_varint(timestamp_msg.len() as u64));
        f.extend(timestamp_msg);
        f
    };

    // Merge all fields into OAuthTokenInfo message
    let oauth_info = [field1, field2, field3, field4].concat();

    // Wrap as Field 6 (length-delimited)
    let tag6 = (6 << 3) | 2;
    let mut field6 = encode_varint(tag6);
    field6.extend(encode_varint(oauth_info.len() as u64));
    field6.extend(oauth_info);

    field6
}

/// Create Email (Field 2)
pub fn create_email_field(email: &str) -> Vec<u8> {
    let tag = (2 << 3) | 2;
    let mut f = encode_varint(tag);
    f.extend(encode_varint(email.len() as u64));
    f.extend(email.as_bytes());
    f
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

/// 编码 varint 字段 (wire_type = 0)
pub fn encode_varint_field(field_num: u32, value: u64) -> Vec<u8> {
    let tag = (field_num << 3) | 0;
    let mut f = encode_varint(tag as u64);
    f.extend(encode_varint(value));
    f
}

/// 创建 OAuthTokenInfo 消息（不包含 Field 6 包装，用于新格式）
pub fn create_oauth_info(
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    is_gcp_tos: bool,
) -> Vec<u8> {
    // Field 1: access_token
    let field1 = encode_string_field(1, access_token);

    // Field 2: token_type = "Bearer"
    let field2 = encode_string_field(2, "Bearer");

    // Field 3: refresh_token
    let field3 = encode_string_field(3, refresh_token);

    // Field 4: expiry (嵌套的 Timestamp 消息)
    let timestamp_tag = (1 << 3) | 0;
    let mut timestamp_msg = encode_varint(timestamp_tag);
    timestamp_msg.extend(encode_varint(expiry as u64));
    let field4 = encode_len_delim_field(4, &timestamp_msg);

    // Field 6: is_gcp_tos = true
    let field6 = is_gcp_tos.then(|| encode_varint_field(6, 1));

    // 合并所有字段为 OAuthTokenInfo 消息
    let mut oauth_info = Vec::new();
    oauth_info.extend(field1);
    oauth_info.extend(field2);
    oauth_info.extend(field3);
    oauth_info.extend(field4);
    if let Some(field6) = field6 {
        oauth_info.extend(field6);
    }
    oauth_info
}

fn decode_legacy_base64_payload_if_needed(payload: Vec<u8>) -> Vec<u8> {
    use base64::{engine::general_purpose, Engine as _};

    let looks_like_legacy_base64 = payload.len() % 4 == 0
        && !payload.is_empty()
        && payload
            .iter()
            .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='));

    if !looks_like_legacy_base64 {
        return payload;
    }

    let Ok(decoded) = general_purpose::STANDARD.decode(&payload) else {
        return payload;
    };

    if decoded.is_empty() {
        payload
    } else {
        decoded
    }
}

fn decode_topic_row_payload(topic_blob: &[u8]) -> Result<(String, Vec<u8>), String> {
    use base64::{engine::general_purpose, Engine as _};

    let data_entry = find_field(topic_blob, 1)?.ok_or("Topic data entry not found".to_string())?;
    let sentinel_key = String::from_utf8(
        find_field(&data_entry, 1)?.ok_or("Topic data entry key not found".to_string())?,
    )
    .map_err(|_| "Topic data entry key is not UTF-8".to_string())?;
    let row_blob = find_field(&data_entry, 2)?.ok_or("Topic row not found".to_string())?;
    let encoded_payload = String::from_utf8(
        find_field(&row_blob, 1)?.ok_or("Topic row value not found".to_string())?,
    )
    .map_err(|_| "Topic row value is not UTF-8".to_string())?;
    let payload = general_purpose::STANDARD
        .decode(encoded_payload)
        .map_err(|e| format!("Topic row payload base64 decoding failed: {}", e))?;

    Ok((sentinel_key, payload))
}

fn decode_legacy_unified_state_entry(outer_blob: &[u8]) -> Result<(String, Vec<u8>), String> {
    let inner_blob = find_field(outer_blob, 1)?.ok_or("Outer Field 1 not found".to_string())?;
    let sentinel_key = String::from_utf8(
        find_field(&inner_blob, 1)?.ok_or("Inner Field 1 not found".to_string())?,
    )
    .map_err(|_| "Sentinel key is not UTF-8".to_string())?;
    let payload = find_field(&inner_blob, 2)?.ok_or("Inner Field 2 not found".to_string())?;
    let payload = decode_legacy_base64_payload_if_needed(payload);

    Ok((sentinel_key, payload))
}

/// 创建统一状态同步条目：Topic(Field 1 data map) -> DataEntry(Field 1 key, Field 2 Row) -> Row(Field 1 base64 payload)
pub fn create_unified_state_entry(sentinel_key: &str, payload: &[u8]) -> String {
    use base64::{engine::general_purpose, Engine as _};

    let row = encode_string_field(1, &general_purpose::STANDARD.encode(payload));
    let data_entry = [
        encode_string_field(1, sentinel_key),
        encode_len_delim_field(2, &row),
    ]
    .concat();
    let topic = encode_len_delim_field(1, &data_entry);

    general_purpose::STANDARD.encode(topic)
}

/// 解码统一状态同步条目，返回 sentinel key 和原始 payload。
/// 优先支持官方 Topic/Row 格式，并兼容早期工具写入的错误嵌套格式。
pub fn decode_unified_state_entry(outer_b64: &str) -> Result<(String, Vec<u8>), String> {
    use base64::{engine::general_purpose, Engine as _};

    let outer_blob = general_purpose::STANDARD
        .decode(outer_b64)
        .map_err(|e| format!("Outer Base64 decoding failed: {}", e))?;

    decode_topic_row_payload(&outer_blob).or_else(|_| decode_legacy_unified_state_entry(&outer_blob))
}

/// 查找指定 protobuf varint 字段
pub fn find_varint_field(data: &[u8], target_field: u32) -> Result<Option<u64>, String> {
    let mut offset = 0;

    while offset < data.len() {
        let (tag, new_offset) = read_varint(data, offset)?;
        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;

        if field_num == target_field && wire_type == 0 {
            let (value, _) = read_varint(data, new_offset)?;
            return Ok(Some(value));
        }

        offset = skip_field(data, new_offset, wire_type)?;
    }

    Ok(None)
}

/// 创建 unified-state stringValue payload
pub fn create_string_value_payload(value: &str) -> Vec<u8> {
    // Matches the upstream `fs` message: { value: { case: "stringValue", value } }
    encode_string_field(3, value)
}

/// 创建最小可用的 UserStatus payload。
///
/// Antigravity 的认证链路要求 `uss-userStatus` 里至少存在 sentinel key；
/// 账号展示和会话绑定依赖名字和邮箱，因此这里写入最小身份信息即可。
pub fn create_minimal_user_status_payload(email: &str) -> Vec<u8> {
    [encode_string_field(3, email), encode_string_field(7, email)].concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};

    #[test]
    fn create_unified_state_entry_wraps_payload_in_topic_row_schema() {
        let payload = create_string_value_payload("intense-age-490103-c3");
        let entry = create_unified_state_entry("enterpriseGcpProjectId", &payload);

        let outer = general_purpose::STANDARD
            .decode(entry)
            .expect("entry should be valid base64");
        let data_entry = find_field(&outer, 1)
            .expect("outer protobuf should parse")
            .expect("topic data entry should exist");
        let sentinel = find_field(&data_entry, 1)
            .expect("data entry protobuf should parse")
            .expect("data entry key should exist");
        let row = find_field(&data_entry, 2)
            .expect("data entry protobuf should parse")
            .expect("data entry row should exist");
        let encoded_payload = find_field(&row, 1)
            .expect("row protobuf should parse")
            .expect("row value should exist");

        assert_eq!(sentinel, b"enterpriseGcpProjectId");
        assert_eq!(
            String::from_utf8(encoded_payload).expect("row value should be utf-8"),
            general_purpose::STANDARD.encode(&payload)
        );
    }

    #[test]
    fn decode_unified_state_entry_accepts_legacy_base64_payloads() {
        let payload = create_string_value_payload("graphic-autumn-9sk6k");
        let payload_b64 = general_purpose::STANDARD.encode(&payload);
        let inner = [
            encode_string_field(1, "enterpriseGcpProjectId"),
            encode_string_field(2, &payload_b64),
        ]
        .concat();
        let outer = encode_len_delim_field(1, &inner);
        let entry = general_purpose::STANDARD.encode(outer);

        let (sentinel, decoded_payload) =
            decode_unified_state_entry(&entry).expect("legacy entry should decode");

        assert_eq!(sentinel, "enterpriseGcpProjectId");
        assert_eq!(decoded_payload, payload);
    }
}
