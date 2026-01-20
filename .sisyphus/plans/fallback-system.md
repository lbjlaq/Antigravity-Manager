# 失败兜底配置功能实施计划

## 📋 需求总结

基于现有的 **z.ai External Provider** 和 **模型路由中心** 功能，实现两个核心兜底能力：

1. **通用外部提供商兜底**（复用 z.ai 架构）
   - 解锁 base_url 配置，支持任意 OpenAI 兼容服务
   - 保留 Fallback/Pooled/Exclusive 调度模式
   - 支持主服务恢复后自动回切

2. **模型映射兜底**（扩展模型路由中心）
   - 新增开关："当模型不可用时启用映射"
   - 复用现有 custom_mapping 配置
   - 集成配额保护检查逻辑

## 🎯 技术方案（最小化改动）

### Phase 1: 通用外部提供商配置（复用 ZaiConfig）

#### 1.1 配置结构扩展
**文件**: `src-tauri/src/proxy/config.rs`

```rust
// 重命名 ZaiConfig -> ExternalProviderConfig
// 保持向后兼容：zai 字段保留，新增 fallback_provider 字段

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalProviderConfig {
    #[serde(default)]
    pub enabled: bool,
    
    #[serde(default = "default_provider_base_url")]
    pub base_url: String,  // 不再锁定为 z.ai
    
    #[serde(default)]
    pub api_key: String,
    
    #[serde(default)]
    pub dispatch_mode: ProviderDispatchMode,  // 复用现有枚举
    
    #[serde(default)]
    pub model_mapping: HashMap<String, String>,
    
    #[serde(default)]
    pub auto_switch_back: bool,  // 新增：主服务恢复后自动回切
    
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u64,  // 新增：健康检查间隔
}

fn default_provider_base_url() -> String {
    "https://api.openai.com".to_string()  // 默认 OpenAI
}

fn default_health_check_interval() -> u64 {
    60  // 60秒检查一次
}

// ProxyConfig 中添加
pub struct ProxyConfig {
    // ... 现有字段
    
    #[serde(default)]
    pub zai: ExternalProviderConfig,  // 保持字段名兼容
    
    #[serde(default)]
    pub fallback_provider: ExternalProviderConfig,  // 新增通用兜底
}
```

#### 1.2 请求处理集成
**文件**: `src-tauri/src/proxy/handlers/claude.rs` (已有 z.ai 逻辑)

```rust
// 在现有 use_zai 判断后添加通用兜底逻辑
let use_fallback_provider = if !use_zai && fallback_enabled {
    match fallback.dispatch_mode {
        ProviderDispatchMode::Fallback => {
            // 复用现有的 has_available_account 检查
            !state.token_manager.has_available_account("claude", &normalized_model).await
        },
        ProviderDispatchMode::Exclusive => true,
        ProviderDispatchMode::Pooled => {
            // 复用现有轮询逻辑
            let total = google_accounts.saturating_add(1).max(1);
            let slot = state.fallback_rr.fetch_add(1, Ordering::Relaxed) % total;
            slot == 0
        },
        _ => false,
    }
} else {
    false
};

if use_fallback_provider {
    return forward_to_external_provider(
        &state,
        &fallback,
        "/v1/chat/completions",  // OpenAI 协议
        &headers,
        &request,
    ).await;
}
```

#### 1.3 健康检查模块（新增）
**文件**: `src-tauri/src/proxy/health_checker.rs`

```rust
// 轻量级健康检查器
pub struct HealthChecker {
    last_check: Arc<RwLock<Instant>>,
    is_healthy: Arc<AtomicBool>,
}

impl HealthChecker {
    pub async fn check_google_health(&self, token_manager: &TokenManager) -> bool {
        // 检查是否有可用账号
        token_manager.has_any_available_account().await
    }
    
    pub fn should_switch_back(&self, config: &ExternalProviderConfig) -> bool {
        config.auto_switch_back && self.is_healthy.load(Ordering::Relaxed)
    }
}
```

### Phase 2: 模型映射兜底开关（扩展模型路由中心）

#### 2.1 配置扩展
**文件**: `src-tauri/src/proxy/config.rs`

```rust
pub struct ProxyConfig {
    // ... 现有字段
    
    #[serde(default)]
    pub custom_mapping: HashMap<String, String>,  // 已有
    
    #[serde(default)]
    pub enable_fallback_mapping: bool,  // 新增：仅在模型不可用时启用映射
}
```

#### 2.2 路由逻辑修改
**文件**: `src-tauri/src/proxy/common/model_mapping.rs`

```rust
// 在 resolve_model_route 中添加条件判断
pub fn resolve_model_route(
    model: &str,
    custom_mapping: &HashMap<String, String>,
    enable_fallback_mapping: bool,
    token_manager: &TokenManager,
) -> String {
    // 1. 优先检查直通模型
    if is_passthrough_model(model) {
        return model.to_string();
    }
    
    // 2. 如果启用兜底映射，检查模型是否可用
    if enable_fallback_mapping {
        let is_available = token_manager
            .has_available_account_for_model(model)
            .await;
        
        if !is_available {
            // 模型不可用，应用映射
            if let Some(fallback_model) = custom_mapping.get(model) {
                tracing::info!(
                    "Model {} unavailable, using fallback mapping: {}",
                    model,
                    fallback_model
                );
                return fallback_model.clone();
            }
        }
    } else {
        // 原有逻辑：始终应用映射
        if let Some(mapped) = custom_mapping.get(model) {
            return mapped.clone();
        }
    }
    
    // 3. 返回原始模型
    model.to_string()
}
```

### Phase 3: 前端UI集成

#### 3.1 通用外部提供商配置
**文件**: `src/pages/ApiProxy.tsx`

在现有 z.ai 配置卡片后添加：

```tsx
{/* 通用兜底提供商配置 */}
<div className="card bg-base-100 shadow-sm border border-base-200">
    <div className="card-body p-4">
        <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-semibold">
                {t('proxy.fallback_provider.title')}
            </h3>
            <input
                type="checkbox"
                className="toggle toggle-sm"
                checked={appConfig.proxy.fallback_provider?.enabled}
                onChange={(e) => updateFallbackProvider({ enabled: e.target.checked })}
            />
        </div>
        
        {appConfig.proxy.fallback_provider?.enabled && (
            <div className="space-y-3">
                <div>
                    <label className="text-xs">{t('proxy.fallback_provider.base_url')}</label>
                    <input
                        type="text"
                        className="input input-sm w-full"
                        value={appConfig.proxy.fallback_provider.base_url}
                        onChange={(e) => updateFallbackProvider({ base_url: e.target.value })}
                        placeholder="https://api.openai.com"
                    />
                </div>
                
                <div>
                    <label className="text-xs">{t('proxy.fallback_provider.api_key')}</label>
                    <input
                        type="password"
                        className="input input-sm w-full"
                        value={appConfig.proxy.fallback_provider.api_key}
                        onChange={(e) => updateFallbackProvider({ api_key: e.target.value })}
                    />
                </div>
                
                <div>
                    <label className="text-xs">{t('proxy.fallback_provider.dispatch_mode')}</label>
                    <select
                        className="select select-sm w-full"
                        value={appConfig.proxy.fallback_provider.dispatch_mode}
                        onChange={(e) => updateFallbackProvider({ dispatch_mode: e.target.value })}
                    >
                        <option value="off">{t('proxy.fallback_provider.mode.off')}</option>
                        <option value="fallback">{t('proxy.fallback_provider.mode.fallback')}</option>
                        <option value="pooled">{t('proxy.fallback_provider.mode.pooled')}</option>
                        <option value="exclusive">{t('proxy.fallback_provider.mode.exclusive')}</option>
                    </select>
                </div>
                
                <div className="flex items-center gap-2">
                    <input
                        type="checkbox"
                        className="checkbox checkbox-sm"
                        checked={appConfig.proxy.fallback_provider.auto_switch_back}
                        onChange={(e) => updateFallbackProvider({ auto_switch_back: e.target.checked })}
                    />
                    <label className="text-xs">{t('proxy.fallback_provider.auto_switch_back')}</label>
                </div>
            </div>
        )}
    </div>
</div>
```

#### 3.2 模型映射兜底开关
**文件**: `src/pages/ModelMapping.tsx`

在模型路由中心页面顶部添加：

```tsx
<div className="alert alert-info mb-4">
    <div className="flex items-center justify-between w-full">
        <div>
            <h4 className="font-semibold">{t('model_mapping.fallback_mode.title')}</h4>
            <p className="text-xs">{t('model_mapping.fallback_mode.description')}</p>
        </div>
        <input
            type="checkbox"
            className="toggle toggle-primary"
            checked={appConfig.proxy.enable_fallback_mapping}
            onChange={(e) => updateProxyConfig({ enable_fallback_mapping: e.target.checked })}
        />
    </div>
</div>
```

### Phase 4: 国际化翻译

#### 4.1 中文翻译
**文件**: `src/locales/zh.json`

```json
{
  "proxy": {
    "fallback_provider": {
      "title": "通用兜底提供商",
      "base_url": "服务地址",
      "api_key": "API 密钥",
      "dispatch_mode": "调度模式",
      "auto_switch_back": "主服务恢复后自动回切",
      "mode": {
        "off": "关闭",
        "fallback": "仅兜底",
        "pooled": "池化",
        "exclusive": "专属"
      }
    }
  },
  "model_mapping": {
    "fallback_mode": {
      "title": "智能兜底模式",
      "description": "仅在模型不可用时应用映射，否则使用原始模型"
    }
  }
}
```

#### 4.2 英文翻译
**文件**: `src/locales/en.json`

```json
{
  "proxy": {
    "fallback_provider": {
      "title": "Generic Fallback Provider",
      "base_url": "Base URL",
      "api_key": "API Key",
      "dispatch_mode": "Dispatch Mode",
      "auto_switch_back": "Auto switch back when primary service recovers",
      "mode": {
        "off": "Off",
        "fallback": "Fallback Only",
        "pooled": "Pooled",
        "exclusive": "Exclusive"
      }
    }
  },
  "model_mapping": {
    "fallback_mode": {
      "title": "Smart Fallback Mode",
      "description": "Apply mapping only when model is unavailable, otherwise use original model"
    }
  }
}
```

## 📊 实施优先级

### 高优先级（核心功能）
1. ✅ 配置结构扩展（复用 ZaiConfig）
2. ✅ 模型映射兜底开关
3. ✅ 请求处理集成（claude.rs, openai.rs）

### 中优先级（用户体验）
4. ✅ 前端UI配置界面
5. ✅ 国际化翻译（中英文）

### 低优先级（增强功能）
6. ⚠️ 健康检查与自动回切（可选）
7. ⚠️ 监控日志增强

## 🔧 关键技术点

### 1. 复用现有架构
- **ZaiConfig** → 通用化为 ExternalProviderConfig
- **ZaiDispatchMode** → 保持不变，直接复用
- **has_available_account** → 已有配额检查逻辑

### 2. 最小化改动
- 不新增复杂模块，直接在现有 handlers 中添加判断
- 复用 z.ai 的 forward 逻辑，仅修改 URL 和协议
- 模型映射仅添加一个布尔开关

### 3. 向后兼容
- `zai` 字段保留，新增 `fallback_provider` 字段
- 默认禁用所有兜底功能
- 不影响现有用户配置

## ⚠️ 注意事项

1. **协议兼容性**: 兜底服务必须兼容 OpenAI API 协议
2. **认证方式**: 统一使用 Bearer Token 认证
3. **错误处理**: 兜底服务失败时，返回明确错误信息
4. **日志记录**: 所有兜底切换都记录详细日志

## 📝 测试计划

### 单元测试
- [ ] 配置序列化/反序列化
- [ ] 模型映射条件判断
- [ ] 健康检查逻辑

### 集成测试
- [ ] 主服务不可用时切换到兜底
- [ ] 模型配额耗尽时应用映射
- [ ] 主服务恢复后自动回切

### 手动测试
- [ ] UI配置保存与加载
- [ ] 实际请求转发验证
- [ ] 多语言界面检查

## 🚀 实施时间估算

- **Phase 1**: 后端配置与逻辑 - 2小时
- **Phase 2**: 模型映射扩展 - 1小时
- **Phase 3**: 前端UI开发 - 2小时
- **Phase 4**: 国际化与测试 - 1小时

**总计**: 约 6 小时

---

**计划创建时间**: 2026-01-20
**计划状态**: 待审核
