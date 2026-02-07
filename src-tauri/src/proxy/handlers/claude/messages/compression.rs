//! 3-layer progressive context compression.
//!
//! Implements automatic context compression when usage exceeds thresholds:
//! - Layer 1: Tool message trimming
//! - Layer 2: Thinking content compression
//! - Layer 3: Fork conversation + XML summary

use crate::proxy::mappers::claude::models::ClaudeRequest;
use crate::proxy::mappers::context_manager::ContextManager;
use crate::proxy::mappers::estimation_calibrator::get_calibrator;
use crate::proxy::token_manager::TokenManager;
use std::sync::Arc;
use tracing::{error, info};

/// Result of compression attempt.
pub struct CompressionResult {
    pub request: ClaudeRequest,
    pub is_purified: bool,
    pub compression_applied: bool,
    pub estimated_usage: u32,
}

/// Apply 3-layer progressive compression to the request.
pub async fn apply_progressive_compression(
    mut request: ClaudeRequest,
    trace_id: &str,
    mapped_model: &str,
    threshold_l1: f32,
    threshold_l2: f32,
    threshold_l3: f32,
    token_manager: &Arc<TokenManager>,
) -> Result<CompressionResult, String> {
    let context_limit = if mapped_model.contains("flash") {
        1_000_000
    } else {
        2_000_000
    };

    let raw_estimated = ContextManager::estimate_token_usage(&request);
    let calibrator = get_calibrator();
    let mut estimated_usage = calibrator.calibrate(raw_estimated);
    let mut usage_ratio = estimated_usage as f32 / context_limit as f32;

    info!(
        "[{}] [ContextManager] Context pressure: {:.1}% (raw: {}, calibrated: {} / {}), Calibration factor: {:.2}",
        trace_id,
        usage_ratio * 100.0,
        raw_estimated,
        estimated_usage,
        context_limit,
        calibrator.get_factor()
    );

    let mut is_purified = false;
    let mut compression_applied = false;

    // Layer 1: Tool Message Trimming
    if usage_ratio > threshold_l1 && !compression_applied {
        if ContextManager::trim_tool_messages(&mut request.messages, 5) {
            info!(
                "[{}] [Layer-1] Tool trimming triggered (usage: {:.1}%, threshold: {:.1}%)",
                trace_id,
                usage_ratio * 100.0,
                threshold_l1 * 100.0
            );
            compression_applied = true;

            let new_raw = ContextManager::estimate_token_usage(&request);
            let new_usage = calibrator.calibrate(new_raw);
            let new_ratio = new_usage as f32 / context_limit as f32;

            info!(
                "[{}] [Layer-1] Compression result: {:.1}% -> {:.1}% (saved {} tokens)",
                trace_id,
                usage_ratio * 100.0,
                new_ratio * 100.0,
                estimated_usage - new_usage
            );

            if new_ratio < 0.7 {
                estimated_usage = new_usage;
                usage_ratio = new_ratio;
            } else {
                usage_ratio = new_ratio;
                compression_applied = false;
            }
        }
    }

    // Layer 2: Thinking Content Compression
    if usage_ratio > threshold_l2 && !compression_applied {
        info!(
            "[{}] [Layer-2] Thinking compression triggered (usage: {:.1}%, threshold: {:.1}%)",
            trace_id,
            usage_ratio * 100.0,
            threshold_l2 * 100.0
        );

        if ContextManager::compress_thinking_preserve_signature(&mut request.messages, 4) {
            is_purified = true;
            compression_applied = true;

            let new_raw = ContextManager::estimate_token_usage(&request);
            let new_usage = calibrator.calibrate(new_raw);
            let new_ratio = new_usage as f32 / context_limit as f32;

            info!(
                "[{}] [Layer-2] Compression result: {:.1}% -> {:.1}% (saved {} tokens)",
                trace_id,
                usage_ratio * 100.0,
                new_ratio * 100.0,
                estimated_usage - new_usage
            );

            usage_ratio = new_ratio;
        }
    }

    // Layer 3: Fork Conversation + XML Summary
    if usage_ratio > threshold_l3 && !compression_applied {
        info!(
            "[{}] [Layer-3] Context pressure ({:.1}%) exceeded threshold ({:.1}%), attempting Fork+Summary",
            trace_id,
            usage_ratio * 100.0,
            threshold_l3 * 100.0
        );

        match super::super::compression::try_compress_with_summary(&request, trace_id, token_manager).await {
            Ok(forked_request) => {
                info!(
                    "[{}] [Layer-3] Fork successful: {} -> {} messages",
                    trace_id,
                    request.messages.len(),
                    forked_request.messages.len()
                );

                let new_raw = ContextManager::estimate_token_usage(&forked_request);
                let new_usage = calibrator.calibrate(new_raw);
                let new_ratio = new_usage as f32 / context_limit as f32;

                info!(
                    "[{}] [Layer-3] Compression result: {:.1}% -> {:.1}% (saved {} tokens)",
                    trace_id,
                    usage_ratio * 100.0,
                    new_ratio * 100.0,
                    estimated_usage - new_usage
                );

                return Ok(CompressionResult {
                    request: forked_request,
                    is_purified: false,
                    compression_applied: true,
                    estimated_usage: new_usage,
                });
            }
            Err(e) => {
                error!(
                    "[{}] [Layer-3] Fork+Summary failed: {}, falling back to error response",
                    trace_id, e
                );
                return Err(format!(
                    "Context too long and automatic compression failed: {}",
                    e
                ));
            }
        }
    }

    Ok(CompressionResult {
        request,
        is_purified,
        compression_applied,
        estimated_usage,
    })
}
