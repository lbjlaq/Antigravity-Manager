use serde_json::Value;
use std::collections::HashMap;

/// Choose the first supported OS language when creating a new configuration.
/// Saved configurations keep their explicit language selection.
pub fn default_language() -> String {
    language_from_locales(sys_locale::get_locales()).to_string()
}

fn language_from_locales(locales: impl IntoIterator<Item = impl AsRef<str>>) -> &'static str {
    locales
        .into_iter()
        .find_map(|locale| supported_language(locale.as_ref()))
        .unwrap_or("en")
}

/// Normalize OS locale tags to the identifiers shared by the UI and tray menu.
fn supported_language(locale: &str) -> Option<&'static str> {
    let locale = locale
        .split(['.', '@'])
        .next()?
        .replace('_', "-")
        .to_ascii_lowercase();
    let mut subtags = locale.split('-');
    match subtags.next()? {
        "zh" => Some(match subtags.next() {
            // An explicit script takes precedence over the region (zh-Hans-TW).
            Some("hant" | "tw" | "hk" | "mo") => "zh-TW",
            _ => "zh",
        }),
        "en" => Some("en"),
        "ja" => Some("ja"),
        "tr" => Some("tr"),
        "vi" => Some("vi"),
        "pt" => Some("pt"),
        "ru" => Some("ru"),
        "ko" => Some("ko"),
        "ar" => Some("ar"),
        "es" => Some("es"),
        // The existing Malay translation uses the application's legacy "my" key.
        "ms" | "my" => Some("my"),
        _ => None,
    }
}

/// Normalize OS locale or application configured language tags to the canonical language keys
pub fn normalize_language(locale: &str) -> &'static str {
    supported_language(locale).unwrap_or("en")
}

/// Gateway error diagnostic texts and templates
#[derive(Debug, Clone)]
pub struct GatewayErrorStrings {
    pub label_diagnosis: &'static str,
    pub label_suggestion: &'static str,

    pub outdated_ua_missing_sig_title: &'static str,
    pub outdated_ua_missing_sig_diag: &'static str,
    pub outdated_ua_missing_sig_sug: &'static str,

    pub outdated_ua_low_ver_title: &'static str,
    pub outdated_ua_low_ver_diag: &'static str,
    pub outdated_ua_low_ver_sug: &'static str,

    pub model_pool_suspended_title: &'static str,
    pub model_pool_suspended_diag: &'static str,
    pub model_pool_suspended_sug: &'static str,

    pub all_accounts_limited_title: &'static str,
    pub all_accounts_limited_diag: &'static str,
    pub all_accounts_limited_sug: &'static str,

    pub upstream_network_error_title: &'static str,
    pub upstream_network_error_diag: &'static str,
    pub upstream_network_error_sug: &'static str,

    pub model_not_found_title: &'static str,
    pub model_not_found_diag: &'static str,
    pub model_not_found_sug: &'static str,

    pub rate_limit_title: &'static str,
    pub rate_limit_diag: &'static str,
    pub rate_limit_sug: &'static str,

    pub upstream_error_title: &'static str,
    pub upstream_error_diag: &'static str,
    pub upstream_error_sug: &'static str,
}

pub static GW_ERR_ZH: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "网关诊断:",
    label_suggestion: "建议:",
    outdated_ua_missing_sig_title: "【上游指纹门禁异常】",
    outdated_ua_missing_sig_diag: "上游接口拒绝访问模型 [{model}] (HTTP {status})。网关检测到当前出站 User-Agent [\"{ua}\"] 缺少官方 Hub 签名（格式为旧版 IDE 指纹）。Google Cloud Code 上游对新一代旗舰模型（如 Gemini 4 Argon 等）实施了严格的客户端指纹门禁，非 antigravity/hub/ 标准格式指纹会直接导致上游返回 404 (Requested entity was not found) 并触发网关全池账号保护性熔断。",
    outdated_ua_missing_sig_sug: "建议前往「API代理设置」将自定义 User-Agent 关闭（恢复官方内置默认指纹），或更新为现代 Hub 标准指纹（如 {recommended_ua}）。",
    outdated_ua_low_ver_title: "【上游指纹门禁异常】",
    outdated_ua_low_ver_diag: "上游接口拒绝访问模型 [{model}] (HTTP {status})。网关检测到当前出站 User-Agent [\"{ua}\"] 虽然具备官方 Hub 签名，但版本号 [{user_version}] 低于当前官方推荐默认基准 [{baseline_version}]。Google Cloud Code 上游对新一代旗舰模型（如 Gemini 4 Argon 等）实施了严格的客户端指纹门禁，过低版本指纹会导致上游返回 404 (Requested entity was not found) 并触发网关全池账号保护性熔断。",
    outdated_ua_low_ver_sug: "建议前往「API代理设置」将自定义 User-Agent 关闭（恢复官方内置默认指纹 {recommended_ua}），或升级版本号至官方推荐基准（{baseline_version} 及以上）后重试。",
    model_pool_suspended_title: "【模型全池临时熔断】",
    model_pool_suspended_diag: "网关本地账号池中所有可用账号在尝试调用模型 [{model}] 时均被上游拒绝并触发了临时保护性熔断 (HTTP 503，冷却 15 分钟)。可能原因：上游返回 404/403，账号缺少该模型访问权限（如未订阅对应套餐），或上游服务异常。",
    model_pool_suspended_sug: "请检查账号是否具备该模型的订阅资质与配额，或在「代理设置」中检查 User-Agent 是否正常。若触发熔断，请等待 15 分钟冷却结束后自动恢复。",
    all_accounts_limited_title: "【网关调度受限】",
    all_accounts_limited_diag: "网关本地账号池当前暂无可用账号或全部可用账号处于限流冷却中。调度详情: {error}",
    all_accounts_limited_sug: "请等待冷却结束（参考等待秒数），或在网关中添加更多正常账号。",
    upstream_network_error_title: "【网络请求异常（非服务端故障）】",
    upstream_network_error_diag: "请求上游接口时网络连接失败或未收到响应回报（非网关服务端故障，请排查网络或代理）：{error}",
    upstream_network_error_sug: "请检查系统代理、上游节点连通性或本地网络设置，确保能正常连接上游服务，并在「超时设置」中调整分阶段传输预算。",
    model_not_found_title: "【模型不存在】[{model}]",
    model_not_found_diag: "模型 [{model}] 在上游端点不存在，或当前绑定的账号暂未开通该模型的访问权限。",
    model_not_found_sug: "请核对模型名称，或在网关配置中的「自定义模型映射」将其重定向至可用模型（如 gemini-2.5-flash）。",
    rate_limit_title: "【上游限流 HTTP {status}】",
    rate_limit_diag: "模型 [{model}] 触发上游配额耗尽或频率限制。",
    rate_limit_sug: "请稍候自动恢复，或添加更多账号以分散并发请求。",
    upstream_error_title: "【上游错误 HTTP {status}】",
    upstream_error_diag: "调用上游模型 [{model}] 发生错误 (HTTP {status})。",
    upstream_error_sug: "请参考 upstream_error 中的详细字段排查原因。",
};

pub static GW_ERR_ZH_TW: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "網關診斷:",
    label_suggestion: "建議:",
    outdated_ua_missing_sig_title: "【上游指紋門禁異常】",
    outdated_ua_missing_sig_diag: "上游介面拒絕存取模型 [{model}] (HTTP {status})。網關偵測到目前出站 User-Agent [\"{ua}\"] 缺少官方 Hub 簽名（格式為舊版 IDE 指紋）。Google Cloud Code 上游對新一代旗艦模型（如 Gemini 4 Argon 等）實施了嚴格的客戶端指紋門禁，非 antigravity/hub/ 標準格式指紋會直接導致上游傳回 404 (Requested entity was not found) 並觸發網關全池帳號保護性熔斷。",
    outdated_ua_missing_sig_sug: "建議前往「API代理設定」將自訂 User-Agent 關閉（恢復官方內建預設指紋），或更新為現代 Hub 標準指紋（如 {recommended_ua}）。",
    outdated_ua_low_ver_title: "【上游指紋門禁異常】",
    outdated_ua_low_ver_diag: "上游介面拒絕存取模型 [{model}] (HTTP {status})。網關偵測到目前出站 User-Agent [\"{ua}\"] 雖然具備官方 Hub 簽名，但版本號 [{user_version}] 低於目前官方最新基準 [{baseline_version}]。Google Cloud Code 上游對新一代旗艦模型（如 Gemini 4 Argon 等）實施了嚴格的客戶端指紋門禁，過低版本指紋會導致上游傳回 404 (Requested entity was not found) 並觸發網關全池帳號保護性熔斷。",
    outdated_ua_low_ver_sug: "建議前往「API代理設定」將自訂 User-Agent 關閉（恢復官方內建預設指紋 {recommended_ua}），或升級版本號至官方推薦基準（{baseline_version} 及以上）後重試。",
    model_pool_suspended_title: "【模型全池臨時熔斷】",
    model_pool_suspended_diag: "網關本地帳號池中所有可用帳號在嘗試呼叫模型 [{model}] 時均被上游拒絕並觸發了臨時保護性熔斷 (HTTP 503，冷卻 15 分鐘)。可能原因：上游傳回 404/403，帳號缺少該模型存取權限（如未訂閱對應方案），或上游服務異常。",
    model_pool_suspended_sug: "請檢查帳號是否具備該模型的訂閱資質與配額，或在「代理設定」中檢查 User-Agent 是否正常。若觸發熔斷，請等待 15 分鐘冷卻結束後自動恢復。",
    all_accounts_limited_title: "【網關調度受限】",
    all_accounts_limited_diag: "網關本地帳號池目前暫無可用帳號或全部可用帳號處於限流冷卻中。調度詳情: {error}",
    all_accounts_limited_sug: "請等待冷卻結束（參考等待秒數），或在網關中新增更多正常帳號。",
    upstream_network_error_title: "【網路請求異常（非服務端故障）】",
    upstream_network_error_diag: "請求上游介面時網路連線失敗或未收到回應回報（非網關服務端故障，請排查網路或代理）：{error}",
    upstream_network_error_sug: "請檢查系統代理、上游節點連線性或本地網路設定，確保能正常連線上游服務，並在「超時設定」中調整分階段傳輸預算。",
    model_not_found_title: "【模型不存在】[{model}]",
    model_not_found_diag: "模型 [{model}] 在上游端點不存在，或目前綁定的帳號暫未開通該模型的存取權限。",
    model_not_found_sug: "請核對模型名稱，或在網關設定中的「自訂模型對應」將其重新導向至可用模型（如 gemini-2.5-flash）。",
    rate_limit_title: "【上游限流 HTTP {status}】",
    rate_limit_diag: "模型 [{model}] 觸發上游配額耗盡或頻率限制。",
    rate_limit_sug: "請稍候自動恢復，或新增更多帳號以分散並發請求。",
    upstream_error_title: "【上游錯誤 HTTP {status}】",
    upstream_error_diag: "呼叫上游模型 [{model}] 發生錯誤 (HTTP {status})。",
    upstream_error_sug: "請參考 upstream_error 中的詳細欄位排查原因。",
};

pub static GW_ERR_EN: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Gateway Diagnosis:",
    label_suggestion: "Suggestion:",
    outdated_ua_missing_sig_title: "[Upstream Fingerprint Gate Exception]",
    outdated_ua_missing_sig_diag: "Upstream service rejected access to model [{model}] (HTTP {status}). The gateway detected that current egress User-Agent [\"{ua}\"] lacks the official Hub signature (legacy IDE format). Google Cloud Code strictly enforces client fingerprint gating on flagship models like Gemini 4 Argon; non-Hub signatures result in HTTP 404 (Requested entity was not found) and trigger a protective circuit breaker for the entire account pool.",
    outdated_ua_missing_sig_sug: "We recommend going to \"API Proxy Settings\" to turn off custom User-Agent (restoring the official default fingerprint), or update to a modern Hub standard fingerprint (such as {recommended_ua}).",
    outdated_ua_low_ver_title: "[Upstream Fingerprint Gate Exception]",
    outdated_ua_low_ver_diag: "Upstream service rejected access to model [{model}] (HTTP {status}). Current egress User-Agent [\"{ua}\"] has an official Hub signature, but version [{user_version}] is lower than the official baseline [{baseline_version}]. Google Cloud Code strictly enforces client fingerprint gating on flagship models like Gemini 4 Argon; outdated versions trigger HTTP 404 (Requested entity was not found) and account pool circuit breaking.",
    outdated_ua_low_ver_sug: "We recommend turning off custom User-Agent in \"API Proxy Settings\" (restoring official default {recommended_ua}), or upgrading the version to the official baseline ({baseline_version} or higher) and retrying.",
    model_pool_suspended_title: "[Model Account Pool Suspended]",
    model_pool_suspended_diag: "All available accounts in the gateway pool were rejected by upstream when calling model [{model}], triggering a temporary protective circuit break (HTTP 503, 15-minute cooldown). Possible reasons: upstream returned 404/403, accounts lack access to this model, or upstream service error.",
    model_pool_suspended_sug: "Please verify that your accounts have active subscriptions/quotas for this model, or check User-Agent in Proxy Settings. When suspended, please wait for the 15-minute cooldown to auto-recover.",
    all_accounts_limited_title: "[Gateway Scheduling Limited]",
    all_accounts_limited_diag: "No accounts currently available in the local pool, or all accounts are in rate-limit cooldown. Details: {error}",
    all_accounts_limited_sug: "Please wait for cooldown to expire (see retry delay), or add more active accounts to the pool.",
    upstream_network_error_title: "[Network Request Exception (Non-Server Fault)]",
    upstream_network_error_diag: "Network connection failed or no response received when sending request to upstream (not a gateway server fault, please check network or proxy): {error}",
    upstream_network_error_sug: "Please check system proxy, upstream connectivity, or local network settings to ensure proper connection, and adjust staged timeout budgets in Timeout Settings if needed.",
    model_not_found_title: "[Model Not Found] [{model}]",
    model_not_found_diag: "Model [{model}] does not exist on upstream endpoint, or the bound account lacks access permission.",
    model_not_found_sug: "Please verify model name, or redirect it to an available model in Custom Model Mapping (such as gemini-2.5-flash).",
    rate_limit_title: "[Upstream Rate Limit HTTP {status}]",
    rate_limit_diag: "Model [{model}] exceeded upstream quota or request rate limit.",
    rate_limit_sug: "Please wait for automatic recovery, or add more accounts to distribute concurrent requests.",
    upstream_error_title: "[Upstream Error HTTP {status}]",
    upstream_error_diag: "An error occurred while calling upstream model [{model}] (HTTP {status}).",
    upstream_error_sug: "Please inspect details in upstream_error to identify the root cause.",
};

pub static GW_ERR_JA: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "ゲートウェイ診断:",
    label_suggestion: "提案:",
    outdated_ua_missing_sig_title: "【上流フィンガープリント拒否】",
    outdated_ua_missing_sig_diag: "上流APIがモデル [{model}] へのアクセスを拒否しました (HTTP {status})。現在の送信User-Agent [\"{ua}\"] に公式Hub署名（旧IDE形式）が不足しています。Google Cloud CodeはGemini 4 Argon等の新世代フラッグシップモデルに厳格なフィンガープリント門番を敷いており、非Hub署名ではHTTP 404が返され、全アカウントプールの一時保護的サーキットブレーカーが作動します。",
    outdated_ua_missing_sig_sug: "「APIプロキシ設定」でカスタムUser-Agentを無効化（公式デフォルトに戻す）するか、最新のHub標準フィンガープリント（例: {recommended_ua}）に更新してください。",
    outdated_ua_low_ver_title: "【上流フィンガープリント拒否】",
    outdated_ua_low_ver_diag: "上流APIがモデル [{model}] へのアクセスを拒否しました (HTTP {status})。送信User-Agent [\"{ua}\"] は公式Hub署名を持っていますが、バージョン [{user_version}] が公式最新基準 [{baseline_version}] を下回っています。古いバージョンはHTTP 404と全アカウントプールの保護的遮断を引き起こします。",
    outdated_ua_low_ver_sug: "「APIプロキシ設定」でカスタムUser-Agentを無効化（公式デフォルト {recommended_ua} に戻す）するか、バージョンを公式基準（{baseline_version} 以上）に更新して再試行してください。",
    model_pool_suspended_title: "【アカウントプール一時遮断】",
    model_pool_suspended_diag: "モデル [{model}] のリクエスト時に全アカウントが上流から拒否され、一時的な保護的サーキットブレーカー（HTTP 503、15分間冷却）が発動しました。原因: 上流404/403、アカウントの購読権限不足、または上流障害。",
    model_pool_suspended_sug: "アカウントに対象モデルの購読権限やクォータがあるか、またはUser-Agent設定が適切かご確認ください。遮断時は15分の冷却後に自動復旧します。",
    all_accounts_limited_title: "【ゲートウェイスケジューリング制限】",
    all_accounts_limited_diag: "現在利用可能なアカウントがないか、全アカウントがレート制限の冷却期間中です。詳細: {error}",
    all_accounts_limited_sug: "冷却終了までお待ちいただくか、ゲートウェイに正常なアカウントを追加してください。",
    upstream_network_error_title: "【ネットワーク接続異常（非サーバー障害）】",
    upstream_network_error_diag: "上流APIへの接続に失敗したか、応答を受信できませんでした（ゲートウェイ自体の障害ではありません）: {error}",
    upstream_network_error_sug: "システムプロキシ、上流接続、またはローカルネットワーク設定を確認し、「タイムアウト設定」で各段階の転送予算を調整してください。",
    model_not_found_title: "【モデルが存在しません】[{model}]",
    model_not_found_diag: "モデル [{model}] は上流エンドポイントに存在しないか、紐付けられたアカウントにアクセス権がありません。",
    model_not_found_sug: "モデル名を確認するか、「カスタムモデルマッピング」で利用可能なモデル（gemini-2.5-flashなど）にリダイレクトしてください。",
    rate_limit_title: "【上流レート制限 HTTP {status}】",
    rate_limit_diag: "モデル [{model}] の上流クォータが枯渇したか、レート制限に達しました。",
    rate_limit_sug: "自動復旧をお待ちいただくか、アカウントを追加して負荷を分散してください。",
    upstream_error_title: "【上流エラー HTTP {status}】",
    upstream_error_diag: "上流モデル [{model}] の呼び出し中にエラーが発生しました (HTTP {status})。",
    upstream_error_sug: "upstream_error の詳細情報を確認して原因を特定してください。",
};

pub static GW_ERR_KO: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "게이트웨이 진단:",
    label_suggestion: "제안:",
    outdated_ua_missing_sig_title: "【업스트림 지문 차단 오류】",
    outdated_ua_missing_sig_diag: "업스트림 서비스가 모델 [{model}] 접근을 거부했습니다 (HTTP {status}). 현재 발신 User-Agent [\"{ua}\"] 에 공식 Hub 서명이 누락되었습니다. Google Cloud Code는 플래그십 모델(예: Gemini 4 Argon)에 엄격한 지문 게이트를 적용하며, 비 Hub 서명은 HTTP 404를 반환하고 계정 풀 전체를 일시 보호 차단합니다.",
    outdated_ua_missing_sig_sug: "「API 프록시 설정」에서 사용자 지정 User-Agent를 끄거나 공식 Hub 표준 지문(예: {recommended_ua})으로 업데이트하세요.",
    outdated_ua_low_ver_title: "【업스트림 지문 차단 오류】",
    outdated_ua_low_ver_diag: "업스트림 서비스가 모델 [{model}] 접근을 거부했습니다 (HTTP {status}). 발신 User-Agent [\"{ua}\"] 에 Hub 서명이 있으나 버전 [{user_version}] 이 공식 기준 [{baseline_version}] 보다 낮습니다. 구버전 지문은 HTTP 404 및 계정 풀 차단을 유발합니다.",
    outdated_ua_low_ver_sug: "사용자 지정 User-Agent를 끄거나 버전을 공식 기준({baseline_version} 이상)으로 업그레이드한 후 다시 시도하세요.",
    model_pool_suspended_title: "【모델 계정 풀 일시 차단】",
    model_pool_suspended_diag: "모델 [{model}] 호출 시 모든 계정이 거부되어 일시적 보호 차단(HTTP 503, 15분 대기)이 작동했습니다. 원인: 업스트림 404/403, 모델 구독 권한 부족 또는 업스트림 장애.",
    model_pool_suspended_sug: "계정의 모델 구독 자격 및 쿼터를 확인하거나 User-Agent가 정상인지 확인하세요. 15분 대기 후 자동 복구됩니다.",
    all_accounts_limited_title: "【게이트웨이 스케줄링 제한】",
    all_accounts_limited_diag: "현재 사용 가능한 계정이 없거나 모든 계정이 속도 제한 대기 중입니다. 세부 정보: {error}",
    all_accounts_limited_sug: "대기 시간이 끝날 때까지 기다리거나 활성 계정을 추가하세요.",
    upstream_network_error_title: "【네트워크 연결 오류 (서버 오류 아님)】",
    upstream_network_error_diag: "업스트림 인터페이스 연결에 실패했거나 응답을 받지 못했습니다 (게이트웨이 서버 오류 아님): {error}",
    upstream_network_error_sug: "시스템 프록시 및 로컬 네트워크 연결을 확인하고 필요 시 「타임아웃 설정」에서 전송 예산을 조정하세요.",
    model_not_found_title: "【모델이 존재하지 않음】[{model}]",
    model_not_found_diag: "모델 [{model}] 이 업스트림에 존재하지 않거나 계정에 접근 권한이 없습니다.",
    model_not_found_sug: "모델명을 확인하거나 「사용자 지정 모델 매핑」에서 사용 가능한 모델로 리디렉션하세요.",
    rate_limit_title: "【업스트림 속도 제한 HTTP {status}】",
    rate_limit_diag: "모델 [{model}] 의 쿼터가 소진되었거나 요청 빈도 제한을 초과했습니다.",
    rate_limit_sug: "자동 복구를 기다리거나 계정을 추가하여 요청을 분산하세요.",
    upstream_error_title: "【업스트림 오류 HTTP {status}】",
    upstream_error_diag: "업스트림 모델 [{model}] 호출 중 오류가 발생했습니다 (HTTP {status}).",
    upstream_error_sug: "upstream_error 세부 항목을 확인하여 원인을 파악하세요.",
};

pub static GW_ERR_ES: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Diagnóstico del gateway:",
    label_suggestion: "Sugerencia:",
    outdated_ua_missing_sig_title: "【Excepción de huella digital upstream】",
    outdated_ua_missing_sig_diag: "El servicio upstream rechazó el acceso al modelo [{model}] (HTTP {status}). Se detectó que el User-Agent saliente [\"{ua}\"] carece de la firma oficial de Hub. Google Cloud Code aplica un estricto control de huellas digitales en modelos emblemáticos como Gemini 4 Argon; las firmas que no son de Hub devuelven HTTP 404 y suspenden todo el grupo de cuentas.",
    outdated_ua_missing_sig_sug: "Recomendamos desactivar el User-Agent personalizado en la configuración del proxy (restaurando el valor oficial) o actualizarlo al estándar moderno de Hub ({recommended_ua}).",
    outdated_ua_low_ver_title: "【Excepción de huella digital upstream】",
    outdated_ua_low_ver_diag: "El servicio upstream rechazó el acceso al modelo [{model}] (HTTP {status}). El User-Agent [\"{ua}\"] tiene firma de Hub pero la versión [{user_version}] es inferior a la referencia oficial [{baseline_version}]. Esto provoca HTTP 404 y la suspensión del grupo de cuentas.",
    outdated_ua_low_ver_sug: "Desactive el User-Agent personalizado o actualice la versión a la referencia oficial ({baseline_version} o superior) y vuelva a intentarlo.",
    model_pool_suspended_title: "【Grupo de cuentas suspendido temporalmente】",
    model_pool_suspended_diag: "Todas las cuentas disponibles fueron rechazadas por el upstream al invocar el modelo [{model}], activando una suspensión temporal (HTTP 503, 15 min de enfriamiento). Posibles causas: 404/403, falta de suscripción o fallo en el upstream.",
    model_pool_suspended_sug: "Verifique si sus cuentas tienen suscripción y cuota activas para este modelo, o revise el User-Agent. La recuperación es automática tras 15 minutos.",
    all_accounts_limited_title: "【Programación del gateway limitada】",
    all_accounts_limited_diag: "No hay cuentas disponibles en el grupo o todas están en enfriamiento por límite de velocidad: {error}",
    all_accounts_limited_sug: "Espere a que finalice el enfriamiento o agregue más cuentas activas al gateway.",
    upstream_network_error_title: "【Excepción de red (no es fallo del servidor)】",
    upstream_network_error_diag: "Fallo de conexión de red o no se recibió respuesta del upstream (no es fallo del gateway): {error}",
    upstream_network_error_sug: "Revise el proxy del sistema, la conectividad o la red local, y ajuste los presupuestos de tiempo de espera si es necesario.",
    model_not_found_title: "【Modelo no encontrado】[{model}]",
    model_not_found_diag: "El modelo [{model}] no existe en el upstream o la cuenta no tiene permisos asignados.",
    model_not_found_sug: "Verifique el nombre o rediríjalo a un modelo disponible en «Mapeo de modelos personalizados».",
    rate_limit_title: "【Límite de velocidad upstream HTTP {status}】",
    rate_limit_diag: "El modelo [{model}] agotó su cuota o superó el límite de frecuencia.",
    rate_limit_sug: "Espere la recuperación automática o agregue más cuentas para balancear la carga.",
    upstream_error_title: "【Error upstream HTTP {status}】",
    upstream_error_diag: "Ocurrió un error al llamar al modelo upstream [{model}] (HTTP {status}).",
    upstream_error_sug: "Consulte los detalles en upstream_error para diagnosticar la causa.",
};

pub static GW_ERR_RU: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Диагностика шлюза:",
    label_suggestion: "Рекомендация:",
    outdated_ua_missing_sig_title: "【Ошибка проверки отпечатка шлюза】",
    outdated_ua_missing_sig_diag: "Вышестоящий сервис отклонил доступ к модели [{model}] (HTTP {status}). Обнаружено, что исходящий User-Agent [\"{ua}\"] не содержит официальной подписи Hub. Google Cloud Code строго контролирует отпечатки клиентов для флагманских моделей (Gemini 4 Argon), возвращая HTTP 404 и блокируя весь пул аккаунтов.",
    outdated_ua_missing_sig_sug: "Рекомендуется отключить пользовательский User-Agent в настройках прокси (вернув стандартный отпечаток) или обновить его до стандарта Hub ({recommended_ua}).",
    outdated_ua_low_ver_title: "【Ошибка проверки отпечатка шлюза】",
    outdated_ua_low_ver_diag: "Вышестоящий сервис отклонил доступ к модели [{model}] (HTTP {status}). Исходящий User-Agent [\"{ua}\"] имеет подпись Hub, но версия [{user_version}] ниже официальной базовой [{baseline_version}]. Устаревший отпечаток вызывает HTTP 404 и блокировку пула аккаунтов.",
    outdated_ua_low_ver_sug: "Отключите пользовательский User-Agent или обновите версию до официальной ({baseline_version} или выше) и повторите попытку.",
    model_pool_suspended_title: "【Пул аккаунтов временно заблокирован】",
    model_pool_suspended_diag: "Все доступные аккаунты были отклонены вышестоящим сервисом при вызове модели [{model}] (HTTP 503, таймаут защиты 15 минут). Возможные причины: 404/403, отсутствие подписки или сбой сервиса.",
    model_pool_suspended_sug: "Проверьте подписку и квоты для этой модели, либо корректность User-Agent. Восстановление произойдет автоматически через 15 минут.",
    all_accounts_limited_title: "【Диспетчеризация шлюза ограничена】",
    all_accounts_limited_diag: "В локальном пуле нет доступных аккаунтов или все они находятся на охлаждении: {error}",
    all_accounts_limited_sug: "Дождитесь окончания охлаждения или добавьте дополнительные активные аккаунты.",
    upstream_network_error_title: "【Сетевая ошибка (не является сбоем сервера)】",
    upstream_network_error_diag: "Сбой сетевого подключения к вышестоящему сервису или ответ не получен (шлюз исправен): {error}",
    upstream_network_error_sug: "Проверьте системный прокси и локальное подключение к сети, при необходимости настройте таймауты.",
    model_not_found_title: "【Модель не найдена】[{model}]",
    model_not_found_diag: "Модель [{model}] не найдена в вышестоящей службе или у аккаунта нет прав доступа к ней.",
    model_not_found_sug: "Проверьте имя модели или перенаправьте её на доступную в «Пользовательском сопоставлении моделей».",
    rate_limit_title: "【Ограничение частоты запросов HTTP {status}】",
    rate_limit_diag: "Для модели [{model}] исчерпана квота или превышен лимит частоты вызовов.",
    rate_limit_sug: "Дождитесь автоматического восстановления или добавьте аккаунты для распределения нагрузки.",
    upstream_error_title: "【Ошибка вышестоящего сервиса HTTP {status}】",
    upstream_error_diag: "Произошла ошибка при вызове вышестоящей модели [{model}] (HTTP {status}).",
    upstream_error_sug: "Ознакомьтесь с подробностями в поле upstream_error для выявления причины.",
};

pub static GW_ERR_PT: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Diagnóstico do gateway:",
    label_suggestion: "Sugestão:",
    outdated_ua_missing_sig_title: "【Exceção de fingerprint upstream】",
    outdated_ua_missing_sig_diag: "O serviço upstream recusou o acesso ao modelo [{model}] (HTTP {status}). O User-Agent de saída [\"{ua}\"] não possui a assinatura oficial do Hub. O Google Cloud Code restringe o acesso aos modelos principais como Gemini 4 Argon, retornando HTTP 404 e suspendendo o pool de contas.",
    outdated_ua_missing_sig_sug: "Recomendamos desativar o User-Agent personalizado nas configurações do proxy ou atualizar para o fingerprint padrão do Hub ({recommended_ua}).",
    outdated_ua_low_ver_title: "【Exceção de fingerprint upstream】",
    outdated_ua_low_ver_diag: "O serviço upstream recusou o acesso ao modelo [{model}] (HTTP {status}). O User-Agent [\"{ua}\"] possui assinatura do Hub, mas a versão [{user_version}] é inferior à linha de base oficial [{baseline_version}], provocando HTTP 404.",
    outdated_ua_low_ver_sug: "Desative o User-Agent personalizado ou atualize para a versão oficial recomendada ({baseline_version} ou superior).",
    model_pool_suspended_title: "【Pool de contas suspenso temporariamente】",
    model_pool_suspended_diag: "Todas as contas disponíveis foram rejeitadas pelo upstream ao chamar o modelo [{model}] (HTTP 503, espera de 15 minutos). Causas: 404/403, falta de assinatura ou erro no upstream.",
    model_pool_suspended_sug: "Verifique se suas contas possuem assinatura e cota para este modelo, ou verifique o User-Agent. O pool se recupera após 15 minutos.",
    all_accounts_limited_title: "【Agendamento do gateway limitado】",
    all_accounts_limited_diag: "Não há contas disponíveis ou todas estão em período de espera por limite de taxa: {error}",
    all_accounts_limited_sug: "Aguarde o término do resfriamento ou adicione mais contas ativas ao gateway.",
    upstream_network_error_title: "【Exceção de rede (não é falha do servidor)】",
    upstream_network_error_diag: "Falha na conexão de rede com o serviço upstream ou nenhuma resposta foi recebida: {error}",
    upstream_network_error_sug: "Verifique o proxy do sistema ou as configurações de rede local e ajuste os limites de tempo limite.",
    model_not_found_title: "【Modelo não encontrado】[{model}]",
    model_not_found_diag: "O modelo [{model}] não existe no upstream ou a conta vinculada não possui permissão de acesso.",
    model_not_found_sug: "Verifique o nome do modelo ou redirecione-o para um modelo disponível no mapeamento personalizado.",
    rate_limit_title: "【Limite de taxa upstream HTTP {status}】",
    rate_limit_diag: "O modelo [{model}] esgotou a cota ou excedeu o limite de requisições.",
    rate_limit_sug: "Aguarde a recuperação automática ou adicione mais contas para distribuir a carga.",
    upstream_error_title: "【Erro upstream HTTP {status}】",
    upstream_error_diag: "Ocorreu um erro ao chamar o modelo upstream [{model}] (HTTP {status}).",
    upstream_error_sug: "Consulte os detalhes em upstream_error para diagnosticar a causa.",
};

pub static GW_ERR_TR: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Ağ Geçidi Teşhisi:",
    label_suggestion: "Öneri:",
    outdated_ua_missing_sig_title: "【Yukarı Akış Parmak İzi Doğrulama Hatası】",
    outdated_ua_missing_sig_diag: "Yukarı akış servisi [{model}] modeline erişimi reddetti (HTTP {status}). Giden User-Agent [\"{ua}\"] dizesinde resmi Hub imzası eksik. Google Cloud Code, Gemini 4 Argon gibi modellerde sıkı parmak izi denetimi uygular; Hub dışı imzalar HTTP 404 döndürür ve tüm hesap havuzunu askıya alır.",
    outdated_ua_missing_sig_sug: "Özel User-Agent ayarını kapatmanızı (varsayılan resmi parmak izine dönmenizi) veya modern Hub standardına ({recommended_ua}) güncellemenizi öneririz.",
    outdated_ua_low_ver_title: "【Yukarı Akış Parmak İzi Doğrulama Hatası】",
    outdated_ua_low_ver_diag: "Yukarı akış servisi [{model}] modeline erişimi reddetti (HTTP {status}). User-Agent [\"{ua}\"] Hub imzasına sahip ancak [{user_version}] sürümü resmi temel sürümden [{baseline_version}] düşüktür.",
    outdated_ua_low_ver_sug: "Özel User-Agent'ı kapatın veya sürümü resmi temele ({baseline_version} veya üzeri) yükseltip tekrar deneyin.",
    model_pool_suspended_title: "【Hesap Havuzu Geçici Olarak Askıya Alındı】",
    model_pool_suspended_diag: "[{model}] modeli çağrılırken tüm hesaplar reddedildi ve koruyucu devre kesici tetiklendi (HTTP 503, 15 dakika bekleme). Olası nedenler: 404/403, abonelik eksikliği veya yukarı akış arızası.",
    model_pool_suspended_sug: "Hesaplarınızın bu model için kota ve aboneliğe sahip olduğunu doğrulayın. Askıya alma 15 dakika sonra otomatik olarak kalkar.",
    all_accounts_limited_title: "【Ağ Geçidi Zamanlaması Sınırlı】",
    all_accounts_limited_diag: "Yerel havuzda kullanılabilir hesap yok veya tüm hesaplar istek sınırı bekleme süresinde: {error}",
    all_accounts_limited_sug: "Bekleme süresinin bitmesini bekleyin veya ağ geçidine daha fazla etkin hesap ekleyin.",
    upstream_network_error_title: "【Ağ İsteği İstisnası (Sunucu Hatası Değil)】",
    upstream_network_error_diag: "Yukarı akış servisine bağlanılamadı veya yanıt alınamadı (ağ geçidi sunucu arızası değildir): {error}",
    upstream_network_error_sug: "Sistem proxy'sini ve yerel ağ bağlantınızı kontrol edin, gerekirse zaman aşımı sürelerini düzenleyin.",
    model_not_found_title: "【Model Bulunamadı】[{model}]",
    model_not_found_diag: "[{model}] modeli yukarı akış uç noktasında mevcut değil veya bağlı hesabın erişim izni yok.",
    model_not_found_sug: "Model adını doğrulayın veya Özel Model Eşleme üzerinden kullanılabilir bir modele yönlendirin.",
    rate_limit_title: "【Yukarı Akış İstek Sınırı HTTP {status}】",
    rate_limit_diag: "[{model}] modelinin kotası tükendi veya istek sıklık sınırı aşıldı.",
    rate_limit_sug: "Otomatik olarak düzelmesini bekleyin veya istekleri dengelemek için daha fazla hesap ekleyin.",
    upstream_error_title: "【Yukarı Akış Hatası HTTP {status}】",
    upstream_error_diag: "Yukarı akış modeli [{model}] çağrılırken hata oluştu (HTTP {status}).",
    upstream_error_sug: "Nedeni tespit etmek için upstream_error alanındaki ayrıntıları inceleyin.",
};

pub static GW_ERR_VI: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Chẩn đoán cổng:",
    label_suggestion: "Đề xuất:",
    outdated_ua_missing_sig_title: "【Lỗi kiểm duyệt dấu vân tay upstream】",
    outdated_ua_missing_sig_diag: "Dịch vụ upstream từ chối truy cập mô hình [{model}] (HTTP {status}). User-Agent [\"{ua}\"] thiếu chữ ký Hub chính thức. Google Cloud Code áp dụng kiểm duyệt dấu vân tay nghiêm ngặt cho các mô hình như Gemini 4 Argon; chữ ký không phải Hub sẽ nhận HTTP 404 và tạm ngưng toàn bộ nhóm tài khoản.",
    outdated_ua_missing_sig_sug: "Nên tắt User-Agent tùy chỉnh trong Cài đặt Proxy API (khôi phục mặc định chính thức) hoặc cập nhật lên chữ ký Hub chuẩn ({recommended_ua}).",
    outdated_ua_low_ver_title: "【Lỗi kiểm duyệt dấu vân tay upstream】",
    outdated_ua_low_ver_diag: "Dịch vụ upstream từ chối truy cập mô hình [{model}] (HTTP {status}). User-Agent [\"{ua}\"] có chữ ký Hub nhưng phiên bản [{user_version}] thấp hơn phiên bản chuẩn chính thức [{baseline_version}].",
    outdated_ua_low_ver_sug: "Tắt User-Agent tùy chỉnh hoặc nâng cấp phiên bản lên chuẩn chính thức ({baseline_version} trở lên) và thử lại.",
    model_pool_suspended_title: "【Nhóm tài khoản bị tạm ngưng】",
    model_pool_suspended_diag: "Tất cả tài khoản bị từ chối khi gọi mô hình [{model}], kích hoạt ngắt mạch bảo vệ (HTTP 503, làm mát 15 phút). Nguyên nhân: 404/403, thiếu gói đăng ký hoặc lỗi upstream.",
    model_pool_suspended_sug: "Vui lòng kiểm tra gói cước và hạn ngạch của tài khoản cho mô hình này. Hệ thống sẽ tự khôi phục sau 15 phút.",
    all_accounts_limited_title: "【Điều phối cổng bị giới hạn】",
    all_accounts_limited_diag: "Hiện không có tài khoản khả dụng hoặc tất cả tài khoản đang trong thời gian làm mát giới hạn tần suất: {error}",
    all_accounts_limited_sug: "Vui lòng đợi hết thời gian làm mát hoặc thêm tài khoản hoạt động vào cổng.",
    upstream_network_error_title: "【Lỗi yêu cầu mạng (Không phải lỗi máy chủ)】",
    upstream_network_error_diag: "Kết nối mạng đến dịch vụ upstream thất bại hoặc không nhận được phản hồi (không phải lỗi máy chủ cổng): {error}",
    upstream_network_error_sug: "Vui lòng kiểm tra proxy hệ thống và mạng cục bộ, điều chỉnh ngân sách thời gian chờ nếu cần.",
    model_not_found_title: "【Mô hình không tồn tại】[{model}]",
    model_not_found_diag: "Mô hình [{model}] không tồn tại trên upstream hoặc tài khoản chưa được cấp quyền truy cập.",
    model_not_found_sug: "Kiểm tra tên mô hình hoặc chuyển hướng sang mô hình có sẵn trong Ánh xạ Mô hình Tùy chỉnh.",
    rate_limit_title: "【Giới hạn tần suất upstream HTTP {status}】",
    rate_limit_diag: "Mô hình [{model}] đã hết hạn ngạch hoặc vượt quá giới hạn tần suất.",
    rate_limit_sug: "Vui lòng đợi tự động khôi phục hoặc thêm tài khoản để phân bổ tải.",
    upstream_error_title: "【Lỗi upstream HTTP {status}】",
    upstream_error_diag: "Đã xảy ra lỗi khi gọi mô hình upstream [{model}] (HTTP {status}).",
    upstream_error_sug: "Vui lòng kiểm tra chi tiết trong upstream_error để xác định nguyên nhân.",
};

pub static GW_ERR_AR: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "تشخيص البوابة:",
    label_suggestion: "اقتراح:",
    outdated_ua_missing_sig_title: "【استثناء بوابة بصمة المنبع】",
    outdated_ua_missing_sig_diag: "رفضت خدمة المنبع الوصول إلى النموذج [{model}] (HTTP {status}). تفتقر بصمة User-Agent [\"{ua}\"] إلى توقيع Hub الرسمي. يفرض Google Cloud Code فحصاً صارماً لبصمات العملاء لنماذج مثل Gemini 4 Argon؛ التوقيعات غير المعتمدة تُرجع خطأ 404 وتوقف مجموعة الحسابات مؤقتاً.",
    outdated_ua_missing_sig_sug: "نوصي بتعطيل User-Agent المخصص في إعدادات الوكيل (استعادة البصمة الرسمية) أو التحديث إلى بصمة Hub المعتمدة ({recommended_ua}).",
    outdated_ua_low_ver_title: "【استثناء بوابة بصمة المنبع】",
    outdated_ua_low_ver_diag: "رفضت خدمة المنبع الوصول إلى النموذج [{model}] (HTTP {status}). يمتلك User-Agent [\"{ua}\"] توقيع Hub ولكن الإصدار [{user_version}] أقل من الإصدار المرجعي الرسمي [{baseline_version}].",
    outdated_ua_low_ver_sug: "قم بتعطيل User-Agent المخصص أو ترقية الإصدار إلى المرجع الرسمي ({baseline_version} فما فوق) ثم أعد المحاولة.",
    model_pool_suspended_title: "【تم تعليق مجموعة الحسابات مؤقتاً】",
    model_pool_suspended_diag: "تم رفض جميع الحسابات المتاحة عند استدعاء النموذج [{model}]، مما أدى إلى تفعيل قاطع الدائرة الواقي (HTTP 503، فترة تبريد 15 دقيقة). الأسباب المحتملة: 404/403، عدم وجود اشتراك صالح، أو عطل في المنبع.",
    model_pool_suspended_sug: "يرجى التحقق من توفر اشتراك وحصص للنموذج أو مراجعة User-Agent. سيتم الاسترداد تلقائياً بعد 15 دقيقة.",
    all_accounts_limited_title: "【جدولة البوابة مقيدة】",
    all_accounts_limited_diag: "لا توجد حسابات متاحة حالياً أو أن جميع الحسابات في فترة تهدئة حدود الطلبات: {error}",
    all_accounts_limited_sug: "يرجى الانتظار حتى انتهاء فترة التبريد أو إضافة المزيد من الحسابات النشطة.",
    upstream_network_error_title: "【استثناء في طلب الشبكة (ليس خطأ في الخادم)】",
    upstream_network_error_diag: "فشل الاتصال بالشبكة مع خدمة المنبع أو لم يتم استلام أي رد (البوابة تعمل بشكل طبيعي): {error}",
    upstream_network_error_sug: "يرجى فحص وكيل النظام والشبكة المحلية وضبط مهلات الانتظار في إعدادات المهلة.",
    model_not_found_title: "【النموذج غير موجود】[{model}]",
    model_not_found_diag: "النموذج [{model}] غير موجود في نقطة نهاية المنبع أو أن الحساب لا يمتلك إذن الوصول.",
    model_not_found_sug: "تحقق من اسم النموذج أو أعد توجيهه إلى نموذج متاح في «تعيين النماذج المخصصة».",
    rate_limit_title: "【حد معدل المنبع HTTP {status}】",
    rate_limit_diag: "استنفد النموذج [{model}] الحصة المتاحة أو تجاوز حد معدل الطلبات.",
    rate_limit_sug: "يرجى الانتظار للاسترداد التلقائي أو إضافة المزيد من الحسابات لتوزيع الطلبات.",
    upstream_error_title: "【خطأ في المنبع HTTP {status}】",
    upstream_error_diag: "حدث خطأ أثناء استدعاء نموذج المنبع [{model}] (HTTP {status}).",
    upstream_error_sug: "يرجى فحص التفاصيل في حقل upstream_error لمعرفة السبب الجذري.",
};

pub static GW_ERR_MY: GatewayErrorStrings = GatewayErrorStrings {
    label_diagnosis: "Diagnosis Gerbang:",
    label_suggestion: "Cadangan:",
    outdated_ua_missing_sig_title: "【Ralat Gerbang Cap Jari Upstream】",
    outdated_ua_missing_sig_diag: "Perkhidmatan upstream menolak akses ke model [{model}] (HTTP {status}). User-Agent keluar [\"{ua}\"] tidak mempunyai tandatangan rasmi Hub. Google Cloud Code menguatkuasakan pengesahan cap jari yang ketat untuk model seperti Gemini 4 Argon; tandatangan bukan Hub mengembalikan HTTP 404 dan menggantung seluruh kolam akaun.",
    outdated_ua_missing_sig_sug: "Disyorkan untuk mematikan User-Agent tersuai dalam Tetapan Proksi API (memulihkan cap jari lalai rasmi) atau kemas kini ke cap jari standard Hub ({recommended_ua}).",
    outdated_ua_low_ver_title: "【Ralat Gerbang Cap Jari Upstream】",
    outdated_ua_low_ver_diag: "Perkhidmatan upstream menolak akses ke model [{model}] (HTTP {status}). User-Agent [\"{ua}\"] mempunyai tandatangan Hub tetapi versi [{user_version}] lebih rendah daripada garis dasar rasmi [{baseline_version}].",
    outdated_ua_low_ver_sug: "Matikan User-Agent tersuai atau tingkatkan versi ke garis dasar rasmi ({baseline_version} atau lebih tinggi) dan cuba lagi.",
    model_pool_suspended_title: "【Kolam Akaun Digantung Sementara】",
    model_pool_suspended_diag: "Semua akaun yang tersedia ditolak oleh upstream semasa memanggil model [{model}] (HTTP 503, penyejukan 15 minit). Sebab yang mungkin: 404/403, tiada langganan atau ralat upstream.",
    model_pool_suspended_sug: "Sila sahkan akaun anda mempunyai langganan dan kuota untuk model ini, atau semak User-Agent. Pemulihan berlaku secara automatik selepas 15 minit.",
    all_accounts_limited_title: "【Penjadualan Gerbang Terhad】",
    all_accounts_limited_diag: "Tiada akaun yang tersedia dalam kolam atau semua akaun sedang dalam penyejukan had kadar: {error}",
    all_accounts_limited_sug: "Sila tunggu penyejukan tamat atau tambah akaun aktif ke dalam gerbang.",
    upstream_network_error_title: "【Pengecualian Permintaan Rangkaian (Bukan Ralat Pelayan)】",
    upstream_network_error_diag: "Sambungan rangkaian ke perkhidmatan upstream gagal atau tiada respons diterima (bukan kesalahan pelayan gerbang): {error}",
    upstream_network_error_sug: "Sila periksa proksi sistem dan sambungan rangkaian tempatan, serta laras belanjawan tamat masa jika perlu.",
    model_not_found_title: "【Model Tidak Ditemui】[{model}]",
    model_not_found_diag: "Model [{model}] tidak wujud pada titik akhir upstream atau akaun tidak mempunyai kebenaran akses.",
    model_not_found_sug: "Sahkan nama model atau halakan semula ke model yang tersedia dalam Pemetaan Model Tersuai.",
    rate_limit_title: "【Had Kadar Upstream HTTP {status}】",
    rate_limit_diag: "Model [{model}] telah kehabisan kuota atau melebihi had kekerapan permintaan.",
    rate_limit_sug: "Sila tunggu pemulihan automatik atau tambah lebih banyak akaun untuk mengimbangi beban.",
    upstream_error_title: "【Ralat Upstream HTTP {status}】",
    upstream_error_diag: "Ralat berlaku semasa memanggil model upstream [{model}] (HTTP {status}).",
    upstream_error_sug: "Sila semak butiran dalam upstream_error untuk mengenal pasti punca masalah.",
};

/// Get gateway error localization texts based on language code
pub fn get_gateway_error_strings(lang: &str) -> &'static GatewayErrorStrings {
    match normalize_language(lang) {
        "zh" => &GW_ERR_ZH,
        "zh-TW" => &GW_ERR_ZH_TW,
        "ja" => &GW_ERR_JA,
        "ko" => &GW_ERR_KO,
        "es" => &GW_ERR_ES,
        "ru" => &GW_ERR_RU,
        "pt" => &GW_ERR_PT,
        "tr" => &GW_ERR_TR,
        "vi" => &GW_ERR_VI,
        "ar" => &GW_ERR_AR,
        "my" => &GW_ERR_MY,
        _ => &GW_ERR_EN,
    }
}

/// Tray text structure
#[derive(Debug, Clone)]
pub struct TrayTexts {
    pub current: String,
    pub quota: String,
    pub switch_next: String,
    pub refresh_current: String,
    pub show_window: String,
    pub mini_view: String,
    pub quit: String,
    pub no_account: String,
    pub unknown_quota: String,
    pub forbidden: String,
    pub lightweight_mode: String,
}

/// Load translations from JSON
fn load_translations(lang: &str) -> HashMap<String, String> {
    // Map every language the frontend supports (see src/i18n.ts / navbar/constants.ts)
    // so the tray menu follows the in-app language switch. Unknown codes fall back to
    // English, matching the frontend's fallbackLng.
    let json_content = match lang {
        "zh" | "zh-CN" | "zh-Hans" => include_str!("../../../src/locales/zh.json"),
        "zh-TW" | "zh-Hant" => include_str!("../../../src/locales/zh-TW.json"),
        "ja" | "ja-JP" => include_str!("../../../src/locales/ja.json"),
        "tr" | "tr-TR" => include_str!("../../../src/locales/tr.json"),
        "vi" | "vi-VN" => include_str!("../../../src/locales/vi.json"),
        "pt" | "pt-BR" | "pt-PT" => include_str!("../../../src/locales/pt.json"),
        "ru" | "ru-RU" => include_str!("../../../src/locales/ru.json"),
        "ko" | "ko-KR" => include_str!("../../../src/locales/ko.json"),
        "ar" | "ar-SA" => include_str!("../../../src/locales/ar.json"),
        "es" | "es-ES" | "es-MX" => include_str!("../../../src/locales/es.json"),
        "my" | "ms" | "ms-MY" => include_str!("../../../src/locales/my.json"),
        "en" | "en-US" => include_str!("../../../src/locales/en.json"),
        _ => include_str!("../../../src/locales/en.json"),
    };

    let v: Value = serde_json::from_str(json_content).unwrap_or_else(|_| serde_json::json!({}));

    let mut map = HashMap::new();

    if let Some(tray) = v.get("tray").and_then(|t| t.as_object()) {
        for (key, value) in tray {
            if let Some(s) = value.as_str() {
                map.insert(key.clone(), s.to_string());
            }
        }
    }

    map
}

/// Get tray texts (based on language)
pub fn get_tray_texts(lang: &str) -> TrayTexts {
    let t = load_translations(lang);

    TrayTexts {
        current: t
            .get("current")
            .cloned()
            .unwrap_or_else(|| "Current".to_string()),
        quota: t
            .get("quota")
            .cloned()
            .unwrap_or_else(|| "Quota".to_string()),
        switch_next: t
            .get("switch_next")
            .cloned()
            .unwrap_or_else(|| "Switch to Next Account".to_string()),
        refresh_current: t
            .get("refresh_current")
            .cloned()
            .unwrap_or_else(|| "Refresh Current Quota".to_string()),
        show_window: t
            .get("show_window")
            .cloned()
            .unwrap_or_else(|| "Show Main Window".to_string()),
        mini_view: t
            .get("mini_view")
            .cloned()
            .unwrap_or_else(|| "Mini View".to_string()),
        quit: t
            .get("quit")
            .cloned()
            .unwrap_or_else(|| "Quit Application".to_string()),
        no_account: t
            .get("no_account")
            .cloned()
            .unwrap_or_else(|| "No Account".to_string()),
        unknown_quota: t
            .get("unknown_quota")
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string()),
        forbidden: t
            .get("forbidden")
            .cloned()
            .unwrap_or_else(|| "Account Forbidden".to_string()),
        lightweight_mode: t
            .get("lightweight_mode")
            .cloned()
            .unwrap_or_else(|| "Lightweight Mode".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{get_tray_texts, language_from_locales};

    #[test]
    fn detects_supported_languages_from_os_locale_tags() {
        for (locale, expected) in [
            ("en-US", "en"),
            ("en-GB", "en"),
            ("ru-RU", "ru"),
            ("ja-JP", "ja"),
            ("tr-TR", "tr"),
            ("vi-VN", "vi"),
            ("pt-BR", "pt"),
            ("pt-PT", "pt"),
            ("ko-KR", "ko"),
            ("ar-SA", "ar"),
            ("es-MX", "es"),
            ("ms-MY", "my"),
            ("RU_ru.UTF-8", "ru"),
            ("es_ES@euro", "es"),
        ] {
            assert_eq!(language_from_locales([locale]), expected, "{locale}");
        }
    }

    #[test]
    fn distinguishes_chinese_scripts_and_regions() {
        for (locale, expected) in [
            ("zh", "zh"),
            ("zh-CN", "zh"),
            ("zh-SG", "zh"),
            ("zh-Hans", "zh"),
            ("zh-Hans-TW", "zh"),
            ("zh-TW", "zh-TW"),
            ("zh-HK", "zh-TW"),
            ("zh-MO", "zh-TW"),
            ("zh-Hant", "zh-TW"),
            ("zh-Hant-CN", "zh-TW"),
        ] {
            assert_eq!(language_from_locales([locale]), expected, "{locale}");
        }
    }

    #[test]
    fn honors_preference_order_and_skips_unsupported_languages() {
        assert_eq!(language_from_locales(["de-DE", "ru-RU", "en-US"]), "ru");
        assert_eq!(language_from_locales(["en-GB", "zh-CN"]), "en");
        assert_eq!(language_from_locales(["zh-TW", "en-US"]), "zh-TW");
    }

    #[test]
    fn falls_back_to_english_without_a_supported_locale() {
        assert_eq!(language_from_locales(Vec::<String>::new()), "en");
        for locale in ["", "C", "POSIX", "C.UTF-8", "de-DE", "my-MM"] {
            assert_eq!(language_from_locales([locale]), "en", "{locale}");
        }
    }

    #[test]
    fn tray_uses_the_detected_language() {
        let language = language_from_locales(["ru-RU"]);
        let texts = get_tray_texts(language);
        let russian: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/locales/ru.json")).unwrap();
        assert_eq!(texts.quit, russian["tray"]["quit"].as_str().unwrap());
    }
}
