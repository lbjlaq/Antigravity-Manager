/**
 * HTTP status classification helpers for proxy monitor, request details, and connectivity.
 * Treats HTTP 101 (Switching Protocols, used by WebSocket upgrades) as successful.
 */
export function isProxySuccessStatus(status?: number | null): boolean {
    if (typeof status !== 'number') return false;
    return status === 101 || (status >= 200 && status < 400);
}

export function isStrictProxySuccessStatus(status?: number | null): boolean {
    if (typeof status !== 'number') return false;
    return status === 101 || (status >= 200 && status < 300);
}
