/// 官方标准 Antigravity Hub 出站指纹常量与 UA 生成器
/// 对齐原生 /Applications/Antigravity.app (v2.33.0, CL: 1023160026)

pub const OFFICIAL_HUB_VERSION: &str = "2.33.0";
pub const OFFICIAL_HUB_CL: &str = "1023160026";
pub const HUB_FINGERPRINT_OS: &str = "windows";
pub const HUB_FINGERPRINT_ARCH: &str = "amd64";

/// 生成官方出站标准 User-Agent:
/// `antigravity/hub/2.33.0 (aidev_client; os_type=windows; arch=amd64; cl=1023160026)`
pub fn get_official_user_agent() -> String {
    format!(
        "antigravity/hub/{} (aidev_client; os_type={}; arch={}; cl={})",
        OFFICIAL_HUB_VERSION, HUB_FINGERPRINT_OS, HUB_FINGERPRINT_ARCH, OFFICIAL_HUB_CL
    )
}
