import type { BackendAccount, QuotaSummary } from '../api';

export type AccountTier = 'ULTRA' | 'ENTERPRISE' | 'PRO' | 'FREE';

/**
 * 复用 Antigravity Tools (`src/types/account.ts#getAccountTier`) 的权威订阅等级判定逻辑：
 * 1. 先检查 enterprise / gcp_ / oauth_client_key (antigravity_gcp / gcp / vertex)
 * 2. 再检查 ultra / helium -> free / starter -> pro / premium / advanced
 * 3. 无有效标识时兜底返回 FREE（绝不按模型列表或用户标签猜测）
 */
export function getAccountTier(account: {
  quota?: QuotaSummary | null;
  token?: { is_gcp_tos?: boolean; oauth_client_key?: string } | null;
}): AccountTier {
  const rawTier = account.quota?.subscription_tier?.trim().toLowerCase();
  const clientKey = account.token?.oauth_client_key?.trim().toLowerCase();
  const isEnterpriseClient = Boolean(
    clientKey &&
      (clientKey === 'antigravity_gcp' ||
        clientKey.includes('gcp') ||
        clientKey.includes('vertex'))
  );

  if (rawTier?.includes('enterprise') || rawTier?.startsWith('gcp_') || isEnterpriseClient) {
    return 'ENTERPRISE';
  }

  if (!rawTier) return 'FREE';

  if (rawTier.includes('ultra') || rawTier.includes('helium')) return 'ULTRA';
  if (rawTier.includes('free') || rawTier.includes('starter')) return 'FREE';
  if (
    rawTier.includes('pro') ||
    rawTier.includes('premium') ||
    rawTier.includes('advanced')
  ) {
    return 'PRO';
  }

  return 'FREE';
}

export function tierSortWeight(tier: string): number {
  const t = tier.toUpperCase();
  if (t === 'ENTERPRISE') return 4;
  if (t === 'ULTRA') return 3;
  if (t === 'PRO') return 2;
  return 1;
}

/** 验证锁是否仍在有效窗口内；0 / 缺失表示无明确截止时间。 */
export function isValidationBlockActive(
  account: Pick<BackendAccount, 'validation_blocked' | 'validation_blocked_until'>,
  nowSeconds: number = Math.floor(Date.now() / 1000)
): boolean {
  if (!account.validation_blocked) return false;
  const until = account.validation_blocked_until;
  return until === undefined || until === null || until === 0 || until > nowSeconds;
}

/**
 * 账号是否触发上游风控/封禁/403 受限（用于仪表盘区分「封禁」与「禁用」）。
 */
export function isAccountRestricted(
  account: Pick<
    BackendAccount,
    | 'banned'
    | 'validation_blocked'
    | 'validation_blocked_until'
    | 'quota'
    | 'proxy_disabled'
    | 'proxy_disabled_reason'
  >
): boolean {
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
 * 账号在账号管理页是否属于「异常」筛选范畴：
 * 涵盖 disabled（如 invalid_grant 刷新失效/账号已禁用）、banned、有效窗口内的 validation_blocked、
 * quota.is_forbidden，以及 403/banned 导致的 proxy_disabled；普通手动禁用反代开关不计入异常。
 */
export function isAccountAbnormal(
  account: Pick<
    BackendAccount,
    | 'disabled'
    | 'banned'
    | 'validation_blocked'
    | 'validation_blocked_until'
    | 'quota'
    | 'proxy_disabled'
    | 'proxy_disabled_reason'
  >
): boolean {
  return Boolean(account.disabled || isAccountRestricted(account));
}

/** 账号是否处于健康可用状态（非异常、未停用）。 */
export function isAccountHealthy(account: BackendAccount): boolean {
  return !isAccountAbnormal(account);
}

const RATE_LIMIT_HINTS = [
  'resource_exhausted',
  'too many requests',
  '429',
  'rate limit',
  'rate-limited',
  'risk control',
  'risk-controlled',
  '风控',
  '冻结',
  'frozen',
];

const OAUTH_HINTS = [
  'unauthorized_client',
  'invalid_client',
  'invalid_grant',
  'oauth client',
  'not authorized',
];

export function getValidationBlockedLabelKey(
  reason?: string | null
): 'status_risk_controlled' | 'status_oauth_reauth_required' | 'status_validation_required' {
  const normalized = (reason || '').toLowerCase();
  if (RATE_LIMIT_HINTS.some((hint) => normalized.includes(hint))) {
    return 'status_risk_controlled';
  }
  if (OAUTH_HINTS.some((hint) => normalized.includes(hint))) {
    return 'status_oauth_reauth_required';
  }
  return 'status_validation_required';
}

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
