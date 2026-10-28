//! 全双工模型思考与响应统一流水线架构
//!
//! 1. 统一中间领域对象（Canonical IR）：谷歌 Gemini 标准报文 (`contents` + `generationConfig`)
//! 2. 协议进站策略适配器 (`InboundThinkingPipeline`, `UpstreamClassification`)
//! 3. 统一用量与缓存核心计算收拢与散开 (`CanonicalUsage`)
//!
//! 架构设计规范：出站发散由各协议 mapper 适配器独立实现（Gemini -> 各协议线缆格式）。
//! 出站不设统一流水线，保持协议发散的灵活性与流式稳定性。

pub mod auto_heal;
pub mod budget;
pub mod estimator;
pub mod image;
pub mod inbound;
#[cfg(test)]
mod official_alignment_tests;
pub mod policy;
pub mod stream_lifecycle;
pub mod usage;

pub use auto_heal::{wrap_stream_with_empty_thinking_auto_heal, ThinkingAutoHealContext};
pub use budget::{
    clamp_budget_secs, current_deadline, deadline_now, estimated_tokens, exhausted,
    gateway_request_budget_secs, register_upstream_call, remaining_secs, set_estimated_tokens,
    RequestContext, DEFAULT_GATEWAY_REQUEST_BUDGET_SECS, MAX_LARGE_BODY_UPSTREAM_CALLS,
    REPLAY_GUARD_BODY_BYTES,
};
pub use estimator::{estimate_tokens, PipelineTokenEstimator};
pub use image::{
    extract_image_generation_diagnostic, response_has_inline_image_data,
    stream_chunk_has_image_data, text_has_nonempty_image_data_url,
    value_has_nonempty_image_data_url, ImageDiagnostic,
};
pub use inbound::{extract_client_thinking_switch, InboundThinkingPipeline};
pub use policy::UpstreamClassification;
pub use stream_lifecycle::{run_stream_lifecycle, ProtocolStreamHandler, StreamLifecycleConfig};
pub use usage::CanonicalUsage;
