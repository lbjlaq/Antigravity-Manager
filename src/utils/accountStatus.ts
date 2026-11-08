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
 * 账号是否处于异常状态。
 *
 * 统一账号列表筛选与仪表盘统计口径，避免两处各自实现导致计数漂移。
 * 403 代理锁可能尚未写入 quota.is_forbidden（例如手动禁用或旧数据），
 * 因此同时检查 proxy_disabled_reason。
 */
export function isAccountAbnormal(account: Account): boolean {
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
 * 账号是否处于健康可用状态（非异常、未停用）。
 */
export function isAccountHealthy(account: Account): boolean {
    return !account.disabled && !isAccountAbnormal(account);
}

/** 账号是否有模型级 403 冻结 */
export function hasForbiddenModels(account: Account): boolean {
    return Boolean(account.forbidden_models && account.forbidden_models.length > 0);
}
