use crate::types::DeviceProfile;
use rand::Rng;
use uuid::Uuid;

/// 生成符合 VSCode / Cursor 官方规范的独立设备指纹特征
pub fn generate_device_profile() -> DeviceProfile {
    let mut rng = rand::thread_rng();

    // 1. machine_id: auth0|user_<32位随机小写十六进制>
    let random_hex: String = (0..32)
        .map(|_| format!("{:x}", rng.gen_range(0..16)))
        .collect();
    let machine_id = format!("auth0|user_{}", random_hex);

    // 2. mac_machine_id: 经典 UUID 硬件哈希格式 (xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx)
    let mac_machine_id = new_standard_machine_id();

    // 3. dev_device_id: 标准 UUID v4 (必须与 state.vscdb 中的 storage.serviceMachineId 保持同步)
    let dev_device_id = Uuid::new_v4().to_string();

    // 4. sqm_id: 微软遥测格式 {UUID大写}
    let sqm_id = format!("{{{}}}", Uuid::new_v4().to_string().to_uppercase());

    DeviceProfile {
        machine_id,
        mac_machine_id,
        dev_device_id,
        sqm_id,
    }
}

fn new_standard_machine_id() -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(36);
    for ch in "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".chars() {
        if ch == '-' || ch == '4' {
            id.push(ch);
        } else if ch == 'x' {
            id.push_str(&format!("{:x}", rng.gen_range(0..16)));
        } else if ch == 'y' {
            id.push_str(&format!("{:x}", rng.gen_range(8..12)));
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_profile_format() {
        let p = generate_device_profile();
        assert!(p.machine_id.starts_with("auth0|user_"));
        assert_eq!(p.machine_id.len(), 43); // "auth0|user_" (11) + 32 = 43
        assert_eq!(p.mac_machine_id.len(), 36);
        assert_eq!(p.dev_device_id.len(), 36);
        assert!(p.sqm_id.starts_with('{') && p.sqm_id.ends_with('}'));
    }
}
