import type { Account } from '../types/account';

/** 验证锁是否仍在有效窗口内；0 / 缺失表示无明确截止时间。 */
export function isValidationBlockActive(
    account: Account,
    nowSeconds: number = Math.floor(Date.now() / 1000)
): boolean {
    if (!account.validation_blocked) return false;
    const until = account.validation_blocked_until;
    return until === undefined || until === null || until === 0 || until > nowSeconds;
}

/**
 * 账号是否受到上游风控、封禁、验证阻断或 403 锁定。
 *
 * 用于仪表盘风控细分统计（区分“手动/凭据停用”与“上游风控异常”）。
 * 403 代理锁可能尚未写入 quota.is_forbidden（例如旧数据或仅写入 proxy_disabled_reason），
 * 因此同时检查 proxy_disabled_reason。
 */
export function isAccountRestricted(account: Account): boolean {
    const reason = (account.proxy_disabled_reason ?? '').toLowerCase();
    return Boolean(
        account.banned ||
            isValidationBlockActive(account) ||
            account.quota?.is_forbidden ||
            (account.proxy_disabled &&
                (reason.startsWith('forbidden (403)') ||
                    reason.includes('403 forbidden') ||
                    reason.startsWith('banned:')))
    );
}

/**
 * 账号是否处于异常状态（凭据停用或受上游风控/403 限制）。
 *
 * 用于账号页「异常」筛选胶囊与全局异常状态判定；纯手动停用反代 (`proxy_disabled`)
 * 或配额保护不计入异常。
 */
export function isAccountAbnormal(account: Account): boolean {
    return Boolean(account.disabled || isAccountRestricted(account));
}

/**
 * 账号是否处于健康可用状态（未停用且无上游风控限制）。
 */
export function isAccountHealthy(account: Account): boolean {
    return !isAccountAbnormal(account);
}

/** 账号是否有模型级 403 冻结 */
export function hasForbiddenModels(account: Account): boolean {
    return Boolean(account.forbidden_models && account.forbidden_models.length > 0);
}

/**
 * 判定文本是否属于上游地区不支持（假 403 / 代理出口 IP 地区限制错误），
 * 与后端 `models::account::is_location_unsupported_error` 保持一致。
 */
export function isLocationUnsupportedReason(reason?: string | null): boolean {
    const lower = (reason ?? '').toLowerCase();
    return (
        lower.includes('user location is not supported') ||
        lower.includes('location not supported') ||
        lower.includes('location is not supported') ||
        lower.includes('location not allowed') ||
        lower.includes('unsupported_location')
    );
}

/**
 * 判定 `proxy_disabled_reason` 是否由系统 403 / 验证 / 地区限制锁定引起
 * （与后端 `classify_lock_reason` 的 HardForbidden / Validation 及地区限制一致），
 * 排除用户手动停代、批量禁用、配额保护与非 403 隔离原因。
 */
export function isClearableProxyLockReason(reason?: string | null): boolean {
    const lower = (reason ?? '').trim().toLowerCase();
    if (!lower) return false;
    if (lower.startsWith('banned:') || lower === 'quota_protection') {
        return false;
    }
    return (
        lower.startsWith('forbidden (403)') ||
        lower.includes('403 forbidden') ||
        lower.includes('validation_required') ||
        lower.includes('verify your account') ||
        lower.includes('validation_url') ||
        lower.includes('further action is required') ||
        isLocationUnsupportedReason(lower)
    );
}

/**
 * 判定账号是否允许执行「一键清除 403 标记并重新测活」。
 *
 * 严格排除：
 * - 上游确认永久封禁 (`banned`)
 * - 凭据/OAuth 停用 (`disabled`，如 `invalid_grant`，清 403 无法恢复)
 * - 纯手动停代 (`manual_proxy_disabled`) 或非 403/验证原因的 `proxy_disabled`
 */
export function canClearForbiddenLock(account: Account): boolean {
    if (account.banned || account.disabled) {
        return false;
    }

    if (
        account.quota?.is_forbidden ||
        isValidationBlockActive(account) ||
        Boolean(account.validation_blocked) ||
        hasForbiddenModels(account)
    ) {
        return true;
    }

    const rawReason =
        account.validation_blocked_reason ||
        account.quota?.forbidden_reason ||
        account.proxy_disabled_reason ||
        '';
    if (isLocationUnsupportedReason(rawReason)) {
        return true;
    }

    return Boolean(
        account.proxy_disabled &&
            !account.manual_proxy_disabled &&
            isClearableProxyLockReason(account.proxy_disabled_reason)
    );
}

/**
 * 提取测活/清理失败的真实错误摘要用于前端 Toast 展示，避免吞掉错误原因后一律提示检查代理。
 */
export function formatWarmUpErrorDetail(error: unknown, maxLength: number = 180): string {
    let raw = '';
    if (typeof error === 'string') {
        raw = error;
    } else if (error instanceof Error) {
        raw = error.message;
    } else if (error && typeof error === 'object') {
        const record = error as Record<string, unknown>;
        if (typeof record.error === 'string') {
            raw = record.error;
        } else if (typeof record.message === 'string') {
            raw = record.message;
        } else {
            try {
                raw = JSON.stringify(error);
            } catch {
                raw = String(error);
            }
        }
    } else if (error !== null && error !== undefined) {
        raw = String(error);
    }

    const trimmed = raw.trim();
    if (!trimmed) return '';

    // 若为 JSON 包装错误，优先提取内部 message
    let extracted = trimmed;
    const jsonStart = trimmed.indexOf('{');
    const jsonEnd = trimmed.lastIndexOf('}');
    if (jsonStart >= 0 && jsonEnd > jsonStart) {
        try {
            const parsed = JSON.parse(trimmed.slice(jsonStart, jsonEnd + 1));
            const candidate =
                parsed?.error?.message ||
                (typeof parsed?.error === 'string' ? parsed.error : null) ||
                parsed?.message;
            if (typeof candidate === 'string' && candidate.trim()) {
                extracted = candidate.trim();
            }
        } catch {
            // 保留原始文本
        }
    }

    const compact = extracted.replace(/\s+/g, ' ').trim();
    if (compact.length <= maxLength) {
        return compact;
    }
    return `${compact.slice(0, Math.max(0, maxLength - 3)).trimEnd()}...`;
}
