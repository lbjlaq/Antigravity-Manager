import type { Account } from '../types/account';

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
            account.validation_blocked ||
            account.quota?.is_forbidden ||
            (account.proxy_disabled &&
                (reason.startsWith('forbidden (403)') ||
                    reason.includes('403 forbidden') ||
                    reason.startsWith('banned:')))
    );
}
