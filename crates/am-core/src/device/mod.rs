use crate::types::DeviceProfile;
use rand::Rng;

const AUTH0_CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// 动态生成符合 Google Cloud Code / Antigravity 官方遥测规范的随机设备配置
pub fn generate_device_profile() -> DeviceProfile {
    let mut rng = rand::thread_rng();

    // 1. 生成标准的 32 位小写字母数字序列: auth0|user_<32 hex>
    let machine_rand: String = (0..32)
        .map(|_| {
            let idx = rng.gen_range(0..AUTH0_CHARSET.len());
            AUTH0_CHARSET[idx] as char
        })
        .collect();
    let machine_id = format!("auth0|user_{}", machine_rand);

    // 2. 生成标准的 UUIDv4
    let mac_machine_id = uuid::Uuid::new_v4().to_string();
    let dev_device_id = uuid::Uuid::new_v4().to_string();

    // 3. 生成大写带花括号的 Windows/MS SQM GUID: {XXXXXXXX-XXXX-4XXX-YXXX-XXXXXXXXXXXX}
    let sqm_id = format!("{{{}}}", uuid::Uuid::new_v4().to_string().to_uppercase());

    DeviceProfile {
        machine_id,
        mac_machine_id,
        dev_device_id,
        sqm_id,
    }
}
