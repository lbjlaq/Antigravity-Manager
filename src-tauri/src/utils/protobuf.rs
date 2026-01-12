/// Protobuf Varint 编码
pub fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    while value >= 0x80 {
        buf.push((value & 0x7F | 0x80) as u8);
        value >>= 7;
    }
    buf.push(value as u8);
    buf
}

/// 读取 Protobuf Varint
pub fn read_varint(data: &[u8], offset: usize) -> Result<(u64, usize), String> {
    let mut result = 0u64;
    let mut shift = 0;
    let mut pos = offset;

    loop {
        if pos >= data.len() {
            return Err("数据不完整".to_string());
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

/// 跳过 Protobuf 字段
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
        _ => Err(format!("未知 wire_type: {}", wire_type)),
    }
}

/// 移除指定的 Protobuf 字段
pub fn remove_field(data: &[u8], field_num: u32) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        let start_offset = offset;
        let (tag, new_offset) = read_varint(data, offset)?;
        let wire_type = (tag & 7) as u8;
        let current_field = (tag >> 3) as u32;

        if current_field == field_num {
            // 跳过此字段
            offset = skip_field(data, new_offset, wire_type)?;
        } else {
            // 保留其他字段
            let next_offset = skip_field(data, new_offset, wire_type)?;
            result.extend_from_slice(&data[start_offset..next_offset]);
            offset = next_offset;
        }
    }

    Ok(result)
}

/// 查找指定的 Protobuf 字段内容 (Length-Delimited only)
pub fn find_field(data: &[u8], target_field: u32) -> Result<Option<Vec<u8>>, String> {
    let mut offset = 0;

    while offset < data.len() {
        let (tag, new_offset) = match read_varint(data, offset) {
            Ok(v) => v,
            Err(_) => break, // 数据不完整，停止
        };

        let wire_type = (tag & 7) as u8;
        let field_num = (tag >> 3) as u32;

        if field_num == target_field && wire_type == 2 {
            let (length, content_offset) = read_varint(data, new_offset)?;
            return Ok(Some(
                data[content_offset..content_offset + length as usize].to_vec(),
            ));
        }

        // 跳过字段
        offset = skip_field(data, new_offset, wire_type)?;
    }

    Ok(None)
}

/// 创建 OAuthTokenInfo (Field 6)
///
/// 结构：
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

    // Field 4: expiry (嵌套的 Timestamp 消息, wire_type = 2)
    // Timestamp 消息包含: Field 1: seconds (int64, wire_type = 0)
    let timestamp_tag = 1 << 3; // Field 1, varint
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

    // 合并所有字段为 OAuthTokenInfo 消息
    let oauth_info = [field1, field2, field3, field4].concat();

    // 包装为 Field 6 (length-delimited)
    let tag6 = (6 << 3) | 2;
    let mut field6 = encode_varint(tag6);
    field6.extend(encode_varint(oauth_info.len() as u64));
    field6.extend(oauth_info);

    field6
}
