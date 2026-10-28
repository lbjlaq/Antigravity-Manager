use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::proxy::token_manager::ProxyToken;

/// 阶梯自愈时间表 (单位: 分钟): 2m -> 5m -> 10m -> 15m -> 20m -> 30m
pub const DEFAULT_QOS_LADDER_MINUTES: [u64; 6] = [2, 5, 10, 15, 20, 30];

/// 账号 QoS 运行时健康状态
#[derive(Debug, Clone, PartialEq)]
pub enum AccountQoSState {
    /// 纯健康：参与基线计算，正常分配新会话
    Healthy,
    /// 瞬态可疑：从同行基线分母中瞬态剔除，调度器开启“只出不进”，暂缓新会话分配
    Suspicious {
        first_strike_at: Instant,
        strike_count: u8,
        consecutive_healthy: u8,
    },
    /// 小黑屋隔离：会话已强制解绑，彻底暂停业务调度，进入阶梯自愈
    Jailed {
        level: usize,
        jailed_at: Instant,     // 本级开始冷却的时间戳 (关键：每次从 0 计时)
        current_wait: Duration, // 本级等待总时长
    },
    /// 探针执行中：后台轻量探针正在发出“你好”测速，防止并发重复探测
    Probing { level: usize, started_at: Instant },
    /// 缓刑观察期：探针通过，单路试探真实请求，连续 2 次真实请求通过才转 Healthy，再次失败从严惩罚
    HalfOpen {
        level: usize,
        consecutive_healthy: u8,
        entered_at: Instant,
    },
}

/// 单次请求的 TTFT 观测记录
#[derive(Debug, Clone)]
pub struct TtftSample {
    pub ttft_ms: f64,
    pub timestamp: Instant,
    pub is_thinking: bool,
    pub is_cold_start: bool,
    /// 未缓存 token 的分桶（桶宽 20K），用于同规模对比，避免拿热缓存小请求当基准
    pub uncached_bucket: u8,
}

/// 冷启动样本分桶宽度（未缓存 token）
const COLD_BUCKET_TOKENS: u32 = 20_000;

/// 判定冷启动是否违规所需的最少同桶基线样本数（低于它只记样本、不判罚）
const MIN_COLD_PEER_SAMPLES: usize = 3;

fn cold_bucket_of(uncached_tokens: u32) -> u8 {
    ((uncached_tokens / COLD_BUCKET_TOKENS).min(15)) as u8
}

/// 裁决动作
#[derive(Debug, Clone, PartialEq)]
pub enum QoSAction {
    None,
    TaggedSuspicious,
    Vindicated,
    TriggerJailedAndUnbind {
        level: usize,
        wait_duration: Duration,
    },
    HalfOpenPassed,
    HalfOpenFailed {
        escalated_level: usize,
        wait_duration: Duration,
    },
}

/// QoS 守护管理器
pub struct QoSGuardian {
    /// 账号状态映射: account_id -> AccountQoSState
    pub states: Arc<DashMap<String, AccountQoSState>>,
    /// 账号最近有效 TTFT 滑动窗口采样: account_id -> Vec<TtftSample> (保留最近 15 分钟)
    pub samples: Arc<DashMap<String, Vec<TtftSample>>>,
    /// 会话上下文水位跟踪: (account_id, session_id) -> (last_input_tokens, last_seen_at)
    pub session_watermarks: Arc<DashMap<(String, String), (u32, Instant)>>,
    /// 惩罚历史记忆: account_id -> (last_level, updated_at)，保留 30 分钟防短时高频复发
    pub penalty_memory: Arc<DashMap<String, (usize, Instant)>>,
    /// 阶梯冷却时间表
    pub ladder_intervals: Vec<Duration>,
}

impl Default for QoSGuardian {
    fn default() -> Self {
        Self::new()
    }
}

impl QoSGuardian {
    pub fn new() -> Self {
        let ladder = DEFAULT_QOS_LADDER_MINUTES
            .iter()
            .map(|&m| Duration::from_secs(m * 60))
            .collect();

        Self {
            states: Arc::new(DashMap::new()),
            samples: Arc::new(DashMap::new()),
            session_watermarks: Arc::new(DashMap::new()),
            penalty_memory: Arc::new(DashMap::new()),
            ladder_intervals: ladder,
        }
    }

    /// 获取账号当前 QoS 状态（未记录则默认 Healthy）
    pub fn get_state(&self, account_id: &str) -> AccountQoSState {
        self.states
            .get(account_id)
            .map(|r| r.value().clone())
            .unwrap_or(AccountQoSState::Healthy)
    }

    /// 判断账号是否处于小黑屋或探针探测中（完全不可用于业务调度）
    pub fn is_jailed_or_probing(&self, account_id: &str) -> bool {
        matches!(
            self.get_state(account_id),
            AccountQoSState::Jailed { .. } | AccountQoSState::Probing { .. }
        )
    }

    /// 判断账号是否处于瞬态可疑状态（开启只出不进）
    pub fn is_suspicious(&self, account_id: &str) -> bool {
        matches!(
            self.get_state(account_id),
            AccountQoSState::Suspicious { .. }
        )
    }

    /// 判断账号是否处于缓刑期（HalfOpen）
    pub fn is_half_open(&self, account_id: &str) -> bool {
        matches!(self.get_state(account_id), AccountQoSState::HalfOpen { .. })
    }

    /// 获取账号当前小黑屋冷却剩余时长 (单位: 秒)
    pub fn get_jailed_remaining_seconds(&self, account_id: &str) -> Option<u64> {
        if let Some(state) = self.states.get(account_id) {
            if let AccountQoSState::Jailed {
                jailed_at,
                current_wait,
                ..
            } = state.value()
            {
                let elapsed = jailed_at.elapsed();
                if elapsed < *current_wait {
                    return Some((*current_wait - elapsed).as_secs());
                } else {
                    return Some(0);
                }
            }
        }
        None
    }

    /// 手动解除账号的小黑屋与 QoS 隔离限制，重置为 Healthy
    pub fn release_from_jail(&self, account_id: &str) {
        self.states
            .insert(account_id.to_string(), AccountQoSState::Healthy);
        self.penalty_memory.remove(account_id);
    }

    /// 判断账号是否处于纯健康状态
    pub fn is_healthy(&self, account_id: &str) -> bool {
        matches!(self.get_state(account_id), AccountQoSState::Healthy)
    }

    /// 计算纯净同侪基线均值（Pure Peer Baseline Extraction）
    /// 严格排除：自身、非同级Tier、本地禁用、429限流/零配额、小黑屋(Jailed)、探测中(Probing)、瞬态可疑(Suspicious)、缓刑(HalfOpen)
    pub fn calculate_pure_peer_avg<F>(
        &self,
        target_account_id: &str,
        target_tier: &str,
        all_accounts: &[ProxyToken],
        is_unhealthy_or_limited: F,
    ) -> Option<f64>
    where
        F: Fn(&str) -> bool,
    {
        let now = Instant::now();
        let sample_ttl = Duration::from_secs(15 * 60); // 仅采纳近 15 分钟内的采样
        let norm_target_tier = target_tier.trim().to_uppercase();

        let mut valid_ttfts: Vec<f64> = Vec::new();

        for acc in all_accounts {
            // 过滤 1: 必须是同类账号 (Tier 对齐，如 PRO vs PRO)
            let acc_tier = acc
                .subscription_tier
                .as_deref()
                .unwrap_or("FREE")
                .trim()
                .to_uppercase();
            if acc_tier != norm_target_tier {
                continue;
            }

            // 过滤 2: 严格排除自身，杜绝自相关性
            if acc.account_id == target_account_id {
                continue;
            }

            // 过滤 3: 排除外部已判定为非健康的账号 (本地已禁用 / 429 限流中 / 配额 <= 0%)
            if is_unhealthy_or_limited(&acc.account_id) {
                continue;
            }

            // 过滤 4: 排除任何非纯 Healthy 的账号！杜绝劣币污染基准
            if let Some(state) = self.states.get(&acc.account_id) {
                match state.value() {
                    AccountQoSState::Healthy => {}
                    _ => continue,
                }
            }

            // 提取该纯净同行近 15 分钟内的非思考、非冷启动样本均值
            if let Some(samples) = self.samples.get(&acc.account_id) {
                let fresh_samples: Vec<f64> = samples
                    .iter()
                    .filter(|s| {
                        now.duration_since(s.timestamp) <= sample_ttl
                            && !s.is_thinking
                            && !s.is_cold_start
                    })
                    .map(|s| s.ttft_ms)
                    .collect();

                if !fresh_samples.is_empty() {
                    let avg = fresh_samples.iter().sum::<f64>() / fresh_samples.len() as f64;
                    valid_ttfts.push(avg);
                }
            }
        }

        if valid_ttfts.is_empty() {
            None
        } else {
            Some(valid_ttfts.iter().sum::<f64>() / valid_ttfts.len() as f64)
        }
    }

    /// 冷启动同桶基线均值（毫秒）
    ///
    /// 只采纳：同 Tier、纯 Healthy、`is_cold_start` 且未缓存桶相差 ±1 的样本。
    /// 样本数不足 [`MIN_COLD_PEER_SAMPLES`] 时返回 None（上层据此“只采样不判罚”）。
    fn calculate_cold_peer_avg<F>(
        &self,
        target_account_id: &str,
        target_tier: &str,
        target_bucket: u8,
        all_accounts: &[ProxyToken],
        is_unhealthy_or_limited: F,
    ) -> Option<f64>
    where
        F: Fn(&str) -> bool,
    {
        let now = Instant::now();
        let sample_ttl = Duration::from_secs(15 * 60);
        let norm_target_tier = target_tier.trim().to_uppercase();
        let mut values: Vec<f64> = Vec::new();

        for acc in all_accounts {
            let acc_tier = acc
                .subscription_tier
                .as_deref()
                .unwrap_or("FREE")
                .trim()
                .to_uppercase();
            if acc_tier != norm_target_tier || acc.account_id == target_account_id {
                continue;
            }
            if is_unhealthy_or_limited(&acc.account_id) {
                continue;
            }
            if let Some(state) = self.states.get(&acc.account_id) {
                if !matches!(state.value(), AccountQoSState::Healthy) {
                    continue;
                }
            }
            if let Some(samples) = self.samples.get(&acc.account_id) {
                for s in samples.iter() {
                    if now.duration_since(s.timestamp) > sample_ttl
                        || !s.is_cold_start
                        || s.uncached_bucket.abs_diff(target_bucket) > 1
                    {
                        continue;
                    }
                    values.push(s.ttft_ms);
                }
            }
        }

        if values.len() < MIN_COLD_PEER_SAMPLES {
            return None;
        }
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }

    /// 评估单次请求的 TTFT 并流转状态机
    pub fn record_and_evaluate<F>(
        &self,
        account_id: &str,
        session_id: Option<&str>,
        tier: &str,
        ttft_ms: f64,
        is_deep_thinking: bool,
        input_tokens: Option<u32>,
        all_accounts: &[ProxyToken],
        is_unhealthy_or_limited: F,
    ) -> QoSAction
    where
        F: Fn(&str) -> bool,
    {
        let ttft_secs = ttft_ms / 1000.0;
        let now = Instant::now();

        // 1. 未缓存 Token 规模计算 (Uncached Tokens)
        // 区分大 Token 冷启动 (Cold Start Prefill) 与 热缓存交互 (Hot Cache Turn)
        let uncached_tokens = if let Some(sid) = session_id.filter(|s| !s.is_empty()) {
            let key = (account_id.to_string(), sid.to_string());
            let uncached = match self.session_watermarks.get(&key) {
                Some(prev) => {
                    let (prev_toks, prev_time) = *prev.value();
                    // 若距离上次请求超过 5 分钟，上游 KV Cache 大概率已失效淘汰，按全量冷启动计算
                    if now.duration_since(prev_time) > Duration::from_secs(5 * 60) {
                        input_tokens.unwrap_or(0)
                    } else {
                        input_tokens.unwrap_or(0).saturating_sub(prev_toks)
                    }
                }
                None => {
                    // 该会话在该账号上的首次请求 (冷启动 Prefill)
                    input_tokens.unwrap_or(0)
                }
            };
            if let Some(tokens) = input_tokens {
                self.session_watermarks.insert(key, (tokens, now));
            }
            uncached
        } else {
            input_tokens.unwrap_or(0)
        };

        if self.session_watermarks.len() > 2000 {
            self.session_watermarks
                .retain(|_, (_, ts)| now.duration_since(*ts).as_secs() < 15 * 60);
        }

        // 2. 动态预算补偿算法：
        // - 每 100K 未缓存 Token 额外补偿 +15.0s 物理 Prefill 耗时
        //   （实测 150K 冷启动首字 36~44s，原来的 7.0s/100K 明显低估，会把正常 prefill 判成限速）
        // - 基础阈值：非深度思考为 20.0s，深度思考上浮至 35.0s
        let uncached_bonus_secs = (uncached_tokens as f64 / 100_000.0) * 15.0;
        let base_threshold_secs = if is_deep_thinking { 35.0 } else { 20.0 };
        let threshold_secs = base_threshold_secs + uncached_bonus_secs;

        // 宽限平反阈值：基础为 18.5s，同比例支持未缓存增量补偿
        let vindicate_threshold_secs = 18.5 + uncached_bonus_secs;

        // 标记是否为大 Token 冷启动 (未缓存 >= 60K)
        let is_cold_start = uncached_tokens >= 60_000;
        let bucket = cold_bucket_of(uncached_tokens);

        // 冷启动不搞“一刀切豁免”（真被限速也测不出来），而是**只和同桶冷启动样本比**：
        // - 有 ≥3 个同桶基线 → 正常参与 strike 判定；
        // - 没有基线 → 只记样本、不判罚（首次遇到该规模时绝不误杀）。
        let cold_peer_avg = if is_cold_start {
            self.calculate_cold_peer_avg(
                account_id,
                tier,
                bucket,
                all_accounts,
                &is_unhealthy_or_limited,
            )
        } else {
            None
        };

        if is_cold_start && cold_peer_avg.is_none() {
            tracing::debug!(
                "[QoS] cold-start without same-bucket baseline | account={} ttft={:.2}s uncached={} bucket={} strike_threshold={:.1}s",
                account_id,
                ttft_secs,
                uncached_tokens,
                bucket,
                threshold_secs
            );
            self.push_sample_with_bucket(account_id, ttft_ms, is_deep_thinking, true, bucket);
            return QoSAction::None;
        }

        // 3. 提取同侪均值 (秒)：冷启动对比同桶冷启动，常规对比热缓存样本
        let peer_avg_secs = if is_cold_start {
            cold_peer_avg
        } else {
            self.calculate_pure_peer_avg(account_id, tier, all_accounts, &is_unhealthy_or_limited)
        }
        .map(|ms| ms / 1000.0);

        // 3.1 冷启动的相对阈值：有同桶基线时，拿“同桶均值的 1.5 倍”与绝对阈值取大。
        // 否则 150K 冷启动的 42.5s 绝对线会卡在实测 36~44s 的抖动区间，依旧可能误杀。
        let threshold_secs = match (is_cold_start, peer_avg_secs) {
            (true, Some(peer_avg)) => threshold_secs.max(peer_avg * 1.5),
            _ => threshold_secs,
        };

        // 4. 检查全局雪崩防护：若纯净同行均值本身也 >= 20s，说明上游机房大面积拥堵，刹车不触发单号惩罚
        if let Some(peer_avg) = peer_avg_secs {
            if peer_avg >= threshold_secs {
                tracing::warn!(
                    "🌐 [QoS Guardian] Global upstream congestion detected (peer_avg={:.2}s >= {:.2}s). Bypassing strike for account {}",
                    peer_avg, threshold_secs, account_id
                );
                // 记录采样后退出
                self.push_sample_with_bucket(
                    account_id,
                    ttft_ms,
                    is_deep_thinking,
                    is_cold_start,
                    bucket,
                );
                return QoSAction::None;
            }
        }

        // 5. 判定是否构成一次卡顿打击 (Strike)
        let is_strike = match peer_avg_secs {
            // 同行健康 (<20s)，唯独当前账号超标
            Some(_) => ttft_secs > threshold_secs,
            // 纯净同行不足时，退化为绝对阈值防护 (额外 +10.0s 兜底)
            None => ttft_secs > (threshold_secs + 10.0),
        };

        let mut current_state = self
            .states
            .entry(account_id.to_string())
            .or_insert(AccountQoSState::Healthy);

        let action = match current_state.value().clone() {
            AccountQoSState::Healthy => {
                if is_strike {
                    // 第 1 次超标：打上瞬态可疑标记！
                    tracing::warn!(
                        "⚠️ [QoS Guardian] Account {} tagged as SUSPICIOUS (TTFT={:.2}s > {:.2}s, uncached_tokens={}, bonus=+{:.2}s)",
                        account_id,
                        ttft_secs,
                        threshold_secs,
                        uncached_tokens,
                        uncached_bonus_secs
                    );
                    *current_state = AccountQoSState::Suspicious {
                        first_strike_at: now,
                        strike_count: 1,
                        consecutive_healthy: 0,
                    };
                    QoSAction::TaggedSuspicious
                } else {
                    QoSAction::None
                }
            }
            AccountQoSState::Suspicious {
                first_strike_at,
                mut strike_count,
                mut consecutive_healthy,
            } => {
                // 检查第 1 次 Strike 是否在 5 分钟滑动窗口内
                let window_valid =
                    now.duration_since(first_strike_at) <= Duration::from_secs(5 * 60);

                if is_strike {
                    if window_valid {
                        strike_count += 1;
                    } else {
                        strike_count = 1;
                    }

                    if strike_count >= 2 {
                        // 5 分钟内累计 2 次超标：坐实被上游降速！关入小黑屋并触发会话解绑
                        let level = self.resolve_initial_jail_level(account_id);
                        let wait_duration = self.ladder_intervals[level];

                        tracing::error!(
                            "🚨 [QoS Guardian] Account {} CONFIRMED THROTTLED (2 strikes in window). Jailing Level {} ({:?}) & unbinding session!",
                            account_id, level, wait_duration
                        );

                        // 记录惩罚历史（保留 30 分钟记忆）
                        self.penalty_memory
                            .insert(account_id.to_string(), (level, now));

                        *current_state = AccountQoSState::Jailed {
                            level,
                            jailed_at: now, // 从 0 开始重新计时
                            current_wait: wait_duration,
                        };

                        QoSAction::TriggerJailedAndUnbind {
                            level,
                            wait_duration,
                        }
                    } else {
                        *current_state = AccountQoSState::Suspicious {
                            first_strike_at: now,
                            strike_count,
                            consecutive_healthy: 0,
                        };
                        QoSAction::TaggedSuspicious
                    }
                } else if ttft_secs < vindicate_threshold_secs {
                    // 表现良好 (TTFT < 宽限线)：双重确认平反
                    consecutive_healthy += 1;
                    if consecutive_healthy >= 2 || !window_valid {
                        tracing::info!(
                            "✅ [QoS Guardian] Account {} vindicated (consecutive healthy passes), restored to HEALTHY",
                            account_id
                        );
                        *current_state = AccountQoSState::Healthy;
                        QoSAction::Vindicated
                    } else {
                        *current_state = AccountQoSState::Suspicious {
                            first_strike_at,
                            strike_count,
                            consecutive_healthy,
                        };
                        QoSAction::None
                    }
                } else {
                    // vindicate_threshold_secs ~ threshold_secs 处于迟滞死区 (Deadband)：不惩罚也不平反，保持原状态
                    if !window_valid {
                        // 超过 5 分钟无再次违规，TTL 衰减平反
                        tracing::info!(
                            "🕒 [QoS Guardian] Account {} suspicious flag expired via TTL, restored to HEALTHY",
                            account_id
                        );
                        *current_state = AccountQoSState::Healthy;
                        QoSAction::Vindicated
                    } else {
                        QoSAction::None
                    }
                }
            }
            AccountQoSState::HalfOpen {
                level,
                mut consecutive_healthy,
                ..
            } => {
                if is_strike {
                    // 缓刑期间真实业务请求再次卡顿：判定假康复，从严惩罚！
                    let next_level = (level + 1).min(self.ladder_intervals.len() - 1);
                    let next_wait = self.ladder_intervals[next_level];

                    tracing::error!(
                        "💥 [QoS Guardian] Account {} FAILED Half-Open probation (TTFT={:.2}s). Escalating to Level {} ({:?})!",
                        account_id, ttft_secs, next_level, next_wait
                    );

                    self.penalty_memory
                        .insert(account_id.to_string(), (next_level, now));

                    *current_state = AccountQoSState::Jailed {
                        level: next_level,
                        jailed_at: now, // 从 0 重新计时
                        current_wait: next_wait,
                    };

                    QoSAction::HalfOpenFailed {
                        escalated_level: next_level,
                        wait_duration: next_wait,
                    }
                } else if ttft_secs < vindicate_threshold_secs {
                    // 缓刑期间真实请求达标 (< vindicate_threshold_secs)
                    consecutive_healthy += 1;
                    if consecutive_healthy >= 2 {
                        tracing::info!(
                            "🎉 [QoS Guardian] Account {} fully PASSED Half-Open probation (2 consecutive healthy real turns). Restored to HEALTHY!",
                            account_id
                        );
                        *current_state = AccountQoSState::Healthy;
                        QoSAction::HalfOpenPassed
                    } else {
                        *current_state = AccountQoSState::HalfOpen {
                            level,
                            consecutive_healthy,
                            entered_at: now,
                        };
                        QoSAction::None
                    }
                } else {
                    // 死区：不记通过，也不算失败
                    QoSAction::None
                }
            }
            AccountQoSState::Jailed { .. } | AccountQoSState::Probing { .. } => {
                // 小黑屋与探测中的账号通常不承接生产流量
                QoSAction::None
            }
        };

        // 记录采样 (冷启动样本同时记入其规模桶，供后续同桶对比)
        self.push_sample_with_bucket(account_id, ttft_ms, is_deep_thinking, is_cold_start, bucket);

        action
    }

    /// 确定入狱初始阶梯：30 分钟内二次复发直接继承并升级阶梯
    fn resolve_initial_jail_level(&self, account_id: &str) -> usize {
        let memory_ttl = Duration::from_secs(30 * 60);
        if let Some(mem) = self.penalty_memory.get(account_id) {
            let (last_level, recorded_at) = *mem.value();
            if recorded_at.elapsed() <= memory_ttl {
                let escalated = (last_level + 1).min(self.ladder_intervals.len() - 1);
                tracing::info!(
                    "🧠 [QoS Guardian] Account {} has recent penalty memory (Lv {}), escalating to Lv {}",
                    account_id, last_level, escalated
                );
                return escalated;
            }
        }
        0 // 初犯从 Level 0 (2分钟) 开始
    }

    /// 记录历史滑动采样 (保留最近 15 分钟)
    pub fn push_sample(
        &self,
        account_id: &str,
        ttft_ms: f64,
        is_thinking: bool,
        is_cold_start: bool,
    ) {
        self.push_sample_with_bucket(account_id, ttft_ms, is_thinking, is_cold_start, 0);
    }

    /// 带分桶的采样入口
    fn push_sample_with_bucket(
        &self,
        account_id: &str,
        ttft_ms: f64,
        is_thinking: bool,
        is_cold_start: bool,
        uncached_bucket: u8,
    ) {
        let mut entry = self.samples.entry(account_id.to_string()).or_default();
        entry.push(TtftSample {
            ttft_ms,
            timestamp: Instant::now(),
            is_thinking,
            is_cold_start,
            uncached_bucket,
        });
        let ttl = Duration::from_secs(15 * 60);
        entry.retain(|s| s.timestamp.elapsed() <= ttl);
    }

    /// 手动或探针将账号置为 HalfOpen
    pub fn mark_probe_passed(&self, account_id: &str, level: usize) {
        self.states.insert(
            account_id.to_string(),
            AccountQoSState::HalfOpen {
                level,
                consecutive_healthy: 0,
                entered_at: Instant::now(),
            },
        );
    }

    /// 探针失败升级阶梯 (关键：从 0 开始重新计时)
    pub fn mark_probe_failed(&self, account_id: &str, current_level: usize) -> (usize, Duration) {
        let next_level = (current_level + 1).min(self.ladder_intervals.len() - 1);
        let next_wait = self.ladder_intervals[next_level];
        let now = Instant::now();

        self.penalty_memory
            .insert(account_id.to_string(), (next_level, now));

        self.states.insert(
            account_id.to_string(),
            AccountQoSState::Jailed {
                level: next_level,
                jailed_at: now, // 从 0 开始重新计时
                current_wait: next_wait,
            },
        );

        (next_level, next_wait)
    }

    /// 标记账号开始探测
    pub fn mark_probing(&self, account_id: &str, level: usize) {
        self.states.insert(
            account_id.to_string(),
            AccountQoSState::Probing {
                level,
                started_at: Instant::now(),
            },
        );
    }

    /// 收集已到期的小黑屋账号并统一原子标记为探测中，返回 (account_id, level) 列表。
    ///
    /// 【防止 DashMap 读写自死锁 (Fixes #3749)】
    /// 必须先收集快照并彻底结束 `states.iter()` 释放分片读锁，再对状态进行更新。
    /// 在 DashMap 迭代器持有分片读锁时，若在循环内调用 `insert()` 或 `get_mut()` 写入同一分片，
    /// 会引发同线程自死锁，导致后台巡检线程永久挂死并逐步卡死整个 HTTP 网关。
    pub fn take_expired_jails_for_probing(&self) -> Vec<(String, usize)> {
        // 阶段 1：收集到期账号，结束迭代器并释放所有分片读锁
        let candidates: Vec<(String, usize)> = self
            .states
            .iter()
            .filter_map(|entry| match *entry.value() {
                AccountQoSState::Jailed {
                    level,
                    jailed_at,
                    current_wait,
                } if jailed_at.elapsed() >= current_wait => Some((entry.key().clone(), level)),
                _ => None,
            })
            .collect();

        // 阶段 2：读锁已完全释放，逐个安全原子更新为 Probing 状态
        let mut expired = Vec::with_capacity(candidates.len());
        for (account_id, level) in candidates {
            if let Some(mut entry) = self.states.get_mut(&account_id) {
                if let AccountQoSState::Jailed { .. } = *entry.value() {
                    *entry.value_mut() = AccountQoSState::Probing {
                        level,
                        started_at: Instant::now(),
                    };
                    expired.push((account_id, level));
                }
            }
        }
        expired
    }

    /// 强制恢复健康 (如在 UI 上手动重置)
    pub fn reset_account(&self, account_id: &str) {
        self.states
            .insert(account_id.to_string(), AccountQoSState::Healthy);
        self.samples.remove(account_id);
        self.penalty_memory.remove(account_id);
        self.session_watermarks
            .retain(|(acc, _), _| acc != account_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn create_test_token(account_id: &str, tier: &str) -> ProxyToken {
        ProxyToken {
            account_id: account_id.to_string(),
            priority: 50,
            access_token: "token".to_string(),
            refresh_token: "refresh".to_string(),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: format!("{}@test.com", account_id),
            account_path: PathBuf::from("/tmp/test"),
            project_id: None,
            subscription_tier: Some(tier.to_string()),
            remaining_quota: Some(100),
            protected_models: HashSet::new(),
            health_score: 1.0,
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: HashMap::new(),
            model_limits: HashMap::new(),
            available_models: HashSet::new(),
            model_metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_pure_peer_baseline_filtering() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let acc_c = create_test_token("acc_c", "PRO");
        let acc_ultra = create_test_token("acc_ultra", "ULTRA");
        let all_accounts = vec![
            acc_a.clone(),
            acc_b.clone(),
            acc_c.clone(),
            acc_ultra.clone(),
        ];

        // 注入采样数据
        guardian.push_sample("acc_b", 4000.0, false, false); // 4.0s
        guardian.push_sample("acc_c", 6000.0, false, false); // 6.0s
        guardian.push_sample("acc_ultra", 2000.0, false, false); // ULTRA 不计入 PRO

        // 正常情况下，计算 acc_a 的同类 (PRO) 纯净基线应为 (4000 + 6000) / 2 = 5000ms
        let avg = guardian.calculate_pure_peer_avg("acc_a", "PRO", &all_accounts, |_| false);
        assert_eq!(avg, Some(5000.0));

        // 若 acc_b 被标记为 Suspicious，acc_b 必须被瞬态排除
        guardian.states.insert(
            "acc_b".to_string(),
            AccountQoSState::Suspicious {
                first_strike_at: Instant::now(),
                strike_count: 1,
                consecutive_healthy: 0,
            },
        );
        let avg_after_b_suspicious =
            guardian.calculate_pure_peer_avg("acc_a", "PRO", &all_accounts, |_| false);
        assert_eq!(avg_after_b_suspicious, Some(6000.0)); // 只有 acc_c 的 6000

        // 若 acc_c 被放入小黑屋 Jailed，同行无纯净健康可用账号，返回 None
        guardian.states.insert(
            "acc_c".to_string(),
            AccountQoSState::Jailed {
                level: 0,
                jailed_at: Instant::now(),
                current_wait: Duration::from_secs(120),
            },
        );
        let avg_none = guardian.calculate_pure_peer_avg("acc_a", "PRO", &all_accounts, |_| false);
        assert_eq!(avg_none, None);
    }

    #[test]
    fn test_thinking_model_dynamic_compensation() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];

        // 注入同行基线 4.0s
        guardian.push_sample("acc_b", 4000.0, false, false);

        // 深度思考模型，TTFT = 25s (25000ms)，由于阈值上浮至 35s，不触发 Strike
        let action = guardian.record_and_evaluate(
            "acc_a",
            None,
            "PRO",
            25000.0,
            true, // is_deep_thinking = true
            None,
            &all,
            |_| false,
        );
        assert_eq!(action, QoSAction::None);
        assert_eq!(guardian.get_state("acc_a"), AccountQoSState::Healthy);

        // 深度思考模型，TTFT = 40s (40000ms) > 35s，此时触发 Strike
        let action_strike =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 40000.0, true, None, &all, |_| false);
        assert_eq!(action_strike, QoSAction::TaggedSuspicious);
        assert!(guardian.is_suspicious("acc_a"));
    }

    #[test]
    fn test_global_outage_brake() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];

        // 同行均值已经达到 25s (25000ms >= 20s)，说明上游全局拥堵
        guardian.push_sample("acc_b", 25000.0, false, false);

        // 此时 acc_a 即使耗时 26s，也触发全局制动刹车，不误判 Strike
        let action =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 26000.0, false, None, &all, |_| false);
        assert_eq!(action, QoSAction::None);
        assert_eq!(guardian.get_state("acc_a"), AccountQoSState::Healthy);
    }

    #[test]
    fn test_hysteresis_deadband() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];
        guardian.push_sample("acc_b", 4000.0, false, false);

        // 第 1 次超标打成 Suspicious
        guardian.record_and_evaluate("acc_a", None, "PRO", 22000.0, false, None, &all, |_| false);
        assert!(guardian.is_suspicious("acc_a"));

        // 下一次耗时 19s（处于 18.5s ~ 20s 迟滞死区），保持状态不变，不加重也不平反
        let action =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 19000.0, false, None, &all, |_| false);
        assert_eq!(action, QoSAction::None);
        assert!(guardian.is_suspicious("acc_a"));
    }

    #[test]
    fn test_sliding_window_strike_and_jail_handover() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];
        guardian.push_sample("acc_b", 4000.0, false, false);

        // 第一次超标 -> TaggedSuspicious
        let action1 =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 22000.0, false, None, &all, |_| false);
        assert_eq!(action1, QoSAction::TaggedSuspicious);
        assert!(guardian.is_suspicious("acc_a"));

        // 第二次超标 -> TriggerJailedAndUnbind
        let action2 =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 25000.0, false, None, &all, |_| false);
        assert_eq!(
            action2,
            QoSAction::TriggerJailedAndUnbind {
                level: 0,
                wait_duration: Duration::from_secs(120),
            }
        );
        assert!(guardian.is_jailed_or_probing("acc_a"));
    }

    #[test]
    fn test_ladder_probing_escalation_and_probation() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];
        guardian.push_sample("acc_b", 4000.0, false, false);

        // 触发入狱 Level 0 (2m)
        guardian.record_and_evaluate("acc_a", None, "PRO", 22000.0, false, None, &all, |_| false);
        guardian.record_and_evaluate("acc_a", None, "PRO", 22000.0, false, None, &all, |_| false);
        assert!(guardian.is_jailed_or_probing("acc_a"));

        // 探针失败 -> 升级为 Level 1 (5m)，时间从 0 重新计时
        let (next_lv, wait) = guardian.mark_probe_failed("acc_a", 0);
        assert_eq!(next_lv, 1);
        assert_eq!(wait, Duration::from_secs(5 * 60));

        // 下一次探针成功 (<8s) -> 转移到 HalfOpen 缓刑期
        guardian.mark_probe_passed("acc_a", 1);
        assert!(guardian.is_half_open("acc_a"));

        // 缓刑期间真实业务请求连续 2 次通过 (<8s) -> 正式满血出狱
        let act1 =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 5000.0, false, None, &all, |_| false);
        assert_eq!(act1, QoSAction::None);
        assert!(guardian.is_half_open("acc_a"));

        let act2 =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 4000.0, false, None, &all, |_| false);
        assert_eq!(act2, QoSAction::HalfOpenPassed);
        assert_eq!(guardian.get_state("acc_a"), AccountQoSState::Healthy);
    }

    #[test]
    fn test_penalty_memory_retention() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];
        guardian.push_sample("acc_b", 4000.0, false, false);

        // 模拟前次已被关押到 Level 2 (10分钟) 并刚康复
        guardian
            .penalty_memory
            .insert("acc_a".to_string(), (2, Instant::now()));

        // 短期内再次复发违规 2 次
        guardian.record_and_evaluate("acc_a", None, "PRO", 22000.0, false, None, &all, |_| false);
        let action =
            guardian
                .record_and_evaluate("acc_a", None, "PRO", 22000.0, false, None, &all, |_| false);

        // 验证：直接继承并升级至 Level 3 (15分钟)，而不是又从 Level 0 重新来
        assert_eq!(
            action,
            QoSAction::TriggerJailedAndUnbind {
                level: 3,
                wait_duration: Duration::from_secs(15 * 60),
            }
        );
    }

    #[test]
    fn test_cold_start_uncached_tokens_bonus_and_isolation() {
        let guardian = QoSGuardian::new();
        let acc_a = create_test_token("acc_a", "PRO");
        let acc_b = create_test_token("acc_b", "PRO");
        let all = vec![acc_a.clone(), acc_b.clone()];

        // 注入同行基线 4.0s
        guardian.push_sample("acc_b", 4000.0, false, false);

        // 模拟场景：346K 零缓存冷启动 (大 Token Prefill)
        // 按照用户规则：每 100K 未缓存 Token 补偿 +7.0s
        // 346K tokens 补偿 = 3.46 * 7.0s = 24.22s
        // 深度思考基础阈值 35.0s -> 提升至 35.0 + 24.22 = 59.22s
        // 若实际首字为 55.0s (55000ms)，未超过 59.22s，应当豁免，不触发 Strike
        let act = guardian.record_and_evaluate(
            "acc_a",
            Some("session_large_1"),
            "PRO",
            55000.0,
            true, // is_deep_thinking
            Some(346_000),
            &all,
            |_| false,
        );
        assert_eq!(act, QoSAction::None);
        assert_eq!(guardian.get_state("acc_a"), AccountQoSState::Healthy);

        // 验证冷启动样本隔离：acc_a 的该 55s 冷启动样本不应该污染同行基准计算
        // 只有 acc_b 的 4.0s 参与计算，基线仍然是 4.0s (4000ms)
        let peer_avg = guardian.calculate_pure_peer_avg("acc_b", "PRO", &all, |_| false);
        assert_eq!(peer_avg, None); // acc_a 只有一个冷启动样本，被过滤后无有效热样本

        // 紧接着第二轮连续交互 (热缓存)：总 Token 350K，上一轮 346K
        // 未缓存增量仅为 4K tokens，补偿 = 0.04 * 7s = 0.28s
        // 阈值恢复为 35.28s。如果此时再次耗时 40s (超标)，则判定为真卡顿，打上 Suspicious
        let act2 = guardian.record_and_evaluate(
            "acc_a",
            Some("session_large_1"),
            "PRO",
            40000.0,
            true,
            Some(350_000),
            &all,
            |_| false,
        );
        assert_eq!(act2, QoSAction::TaggedSuspicious);
        assert!(guardian.is_suspicious("acc_a"));
    }

    #[test]
    fn test_take_expired_jails_for_probing_does_not_deadlock() {
        let guardian = Arc::new(QoSGuardian::new());

        // 设置到期账号 (50ms 前入狱，刑期 10ms)
        let expired_at = Instant::now() - Duration::from_millis(50);
        guardian.states.insert(
            "acc_expired".to_string(),
            AccountQoSState::Jailed {
                level: 0,
                jailed_at: expired_at,
                current_wait: Duration::from_millis(10),
            },
        );

        // 设置未到期账号 (刑期 1 小时)
        guardian.states.insert(
            "acc_waiting".to_string(),
            AccountQoSState::Jailed {
                level: 1,
                jailed_at: Instant::now(),
                current_wait: Duration::from_secs(3600),
            },
        );

        // 设置普通健康账号
        guardian
            .states
            .insert("acc_healthy".to_string(), AccountQoSState::Healthy);

        // 在单独线程中运行，配合 5 秒超时保护检测死锁
        let g = guardian.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let expired = g.take_expired_jails_for_probing();
            let _ = tx.send(expired);
        });

        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("take_expired_jails_for_probing deadlocked or timed out!");

        // 断言：仅已到期账号被取出
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "acc_expired");
        assert_eq!(result[0].1, 0);

        // 断言：到期账号状态已原子转换为 Probing
        match guardian.get_state("acc_expired") {
            AccountQoSState::Probing { level, .. } => assert_eq!(level, 0),
            other => panic!("Expected Probing state, got {:?}", other),
        }

        // 断言：未到期账号依然保持 Jailed
        match guardian.get_state("acc_waiting") {
            AccountQoSState::Jailed { level, .. } => assert_eq!(level, 1),
            other => panic!("Expected Jailed state, got {:?}", other),
        }

        // 断言：再次调用不会重复取出（已在 Probing 中）
        let second_run = guardian.take_expired_jails_for_probing();
        assert!(second_run.is_empty());
    }
}
