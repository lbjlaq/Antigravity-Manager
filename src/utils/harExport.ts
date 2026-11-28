import { ProxyRequestLog } from '../components/proxy/RequestDetailModal';

export interface HarHeader {
    name: string;
    value: string;
    comment?: string;
}

export interface HarQueryParam {
    name: string;
    value: string;
    comment?: string;
}

export interface HarPostData {
    mimeType: string;
    text?: string;
    params?: Array<{ name: string; value: string }>;
}

export interface HarRequest {
    method: string;
    url: string;
    httpVersion: string;
    cookies: Array<{ name: string; value: string }>;
    headers: HarHeader[];
    queryString: HarQueryParam[];
    postData?: HarPostData;
    headersSize: number;
    bodySize: number;
}

export interface HarResponse {
    status: number;
    statusText: string;
    httpVersion: string;
    cookies: Array<{ name: string; value: string }>;
    headers: HarHeader[];
    content: {
        size: number;
        mimeType: string;
        text?: string;
    };
    redirectURL: string;
    headersSize: number;
    bodySize: number;
}

export interface HarEntry {
    startedDateTime: string;
    time: number;
    request: HarRequest;
    response: HarResponse;
    _forwardedRequest?: HarRequest;
    _upstream?: {
        model?: string;
        endpoint?: string;
    };
    cache: Record<string, unknown>;
    timings: {
        blocked?: number;
        dns?: number;
        connect?: number;
        send: number;
        wait: number;
        receive: number;
        ssl?: number;
    };
    serverIPAddress?: string;
    comment?: string;
}

export interface HarLog {
    version: string;
    creator: {
        name: string;
        version: string;
        comment?: string;
    };
    pages: unknown[];
    entries: HarEntry[];
}

export interface HarRoot {
    log: HarLog;
}

/**
 * 将 JSON 字符串的 headers 转换为 HAR Header 数组
 */
export function parseHeadersToHarList(headersJson?: string): HarHeader[] {
    if (!headersJson) return [];
    try {
        const parsed = JSON.parse(headersJson);
        if (Array.isArray(parsed)) {
            return parsed
                .filter((item) => item && typeof item.name === 'string')
                .map((item) => ({
                    name: String(item.name),
                    value: String(item.value ?? ''),
                }));
        }
        if (typeof parsed === 'object' && parsed !== null) {
            return Object.entries(parsed).map(([key, val]) => ({
                name: key,
                value: typeof val === 'string' ? val : JSON.stringify(val),
            }));
        }
    } catch {
        // Fallback text format: name: value per line
        const lines = headersJson.split('\n');
        const list: HarHeader[] = [];
        for (const line of lines) {
            const idx = line.indexOf(':');
            if (idx > 0) {
                list.push({
                    name: line.slice(0, idx).trim(),
                    value: line.slice(idx + 1).trim(),
                });
            }
        }
        return list;
    }
    return [];
}

/**
 * 从 URL 中解析 Query 参数列表
 */
export function parseQueryStringFromUrl(url: string): HarQueryParam[] {
    const list: HarQueryParam[] = [];
    try {
        const queryIndex = url.indexOf('?');
        if (queryIndex < 0) return [];
        const search = url.slice(queryIndex + 1);
        const searchParams = new URLSearchParams(search);
        searchParams.forEach((value, name) => {
            list.push({ name, value });
        });
    } catch {
        // ignore url parsing error
    }
    return list;
}

/**
 * 根据 HTTP 状态码获取标准描述文本
 */
export function getStatusText(status: number): string {
    const map: Record<number, string> = {
        101: 'Switching Protocols',
        200: 'OK',
        201: 'Created',
        204: 'No Content',
        400: 'Bad Request',
        401: 'Unauthorized',
        403: 'Forbidden',
        404: 'Not Found',
        429: 'Too Many Requests',
        500: 'Internal Server Error',
        502: 'Bad Gateway',
        503: 'Service Unavailable',
        504: 'Gateway Timeout',
    };
    return map[status] || (status >= 200 && status < 300 ? 'OK' : 'Error');
}

/**
 * 将 ProxyRequestLog 转换为标准 HAR 1.2 规范对象
 */
export function buildHarObject(log: ProxyRequestLog): HarRoot {
    const reqHeaders = parseHeadersToHarList(log.request_headers);
    const respHeaders = parseHeadersToHarList(log.response_headers);
    const queryParams = parseQueryStringFromUrl(log.url);

    const startedDateTime = new Date(log.timestamp).toISOString();
    const duration = Math.max(0, log.duration || 0);

    // 解析 content-type
    const reqContentType =
        reqHeaders.find((h) => h.name.toLowerCase() === 'content-type')?.value ||
        'application/json';
    const respContentType =
        respHeaders.find((h) => h.name.toLowerCase() === 'content-type')?.value ||
        'application/json';

    // 解析 postData
    let postData: HarPostData | undefined = undefined;
    if (log.request_body && log.method.toUpperCase() !== 'GET') {
        postData = {
            mimeType: reqContentType,
            text: log.request_body,
        };
    }

    // 粗略估算耗时占比 (TTFT 占 wait，剩余占 receive)
    let waitTime = Math.min(duration, 500);
    const ttftHeader = respHeaders.find(
        (h) => h.name.toLowerCase() === 'x-timing-ttft-ms'
    );
    if (ttftHeader) {
        const parsedTtft = parseFloat(ttftHeader.value);
        if (!isNaN(parsedTtft) && parsedTtft > 0) {
            waitTime = Math.min(duration, Math.round(parsedTtft));
        }
    }
    const receiveTime = Math.max(0, duration - waitTime);

    const reqBodySize = log.request_body ? new TextEncoder().encode(log.request_body).length : 0;
    const respBodySize = log.response_body ? new TextEncoder().encode(log.response_body).length : 0;

    const entry: HarEntry = {
        startedDateTime,
        time: duration,
        request: {
            method: log.method.toUpperCase(),
            url: log.url,
            httpVersion: 'HTTP/1.1',
            cookies: [],
            headers: reqHeaders,
            queryString: queryParams,
            postData,
            headersSize: -1,
            bodySize: reqBodySize,
        },
        response: {
            status: log.status,
            statusText: getStatusText(log.status),
            httpVersion: 'HTTP/1.1',
            cookies: [],
            headers: respHeaders,
            content: {
                size: respBodySize,
                mimeType: respContentType,
                text: log.response_body || '',
            },
            redirectURL: '',
            headersSize: -1,
            bodySize: respBodySize,
        },
        cache: {},
        timings: {
            send: 0,
            wait: waitTime,
            receive: receiveTime,
        },
        comment: `Antigravity Proxy Request ID: ${log.id}`,
    };

    const entries: HarEntry[] = [entry];

    // 无论是显式记录了转出报文，还是直连转发，始终生成完整的网关转出链路 Entry，确保【客户端请求】、【网关转出】、【响应】3 个全都齐全！
    const upHeaders = parseHeadersToHarList(log.upstream_request_headers || log.request_headers);
    const upContentType =
        upHeaders.find((h) => h.name.toLowerCase() === 'content-type')?.value ||
        'application/json';
    const effectiveUpstreamBody = log.upstream_request_body || (log.method.toUpperCase() !== 'GET' ? log.request_body : undefined);
    const upBodySize = effectiveUpstreamBody
        ? new TextEncoder().encode(effectiveUpstreamBody).length
        : 0;

    // 尝试从 x-timing-ttft-attempts 获取真实端点 URL
    let upstreamUrl = 'https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse';
    let endpointName = log.upstream_model || log.mapped_model || log.model || 'Upstream';
    try {
        const attemptsHeader = respHeaders.find((h) => h.name.toLowerCase() === 'x-timing-ttft-attempts');
        if (attemptsHeader && attemptsHeader.value) {
            const decoded = atob(attemptsHeader.value);
            const attempts = JSON.parse(decoded);
            if (Array.isArray(attempts)) {
                const effective = attempts.find((a: any) => a.is_effective) || attempts[0];
                if (effective?.endpoint_url) {
                    upstreamUrl = effective.endpoint_url;
                }
                if (effective?.endpoint_name) {
                    endpointName = `${effective.endpoint_name} (${log.upstream_model || log.mapped_model || 'Model'})`;
                }
            }
        } else if (log.url) {
            upstreamUrl = log.url.startsWith('http')
                ? log.url
                : `https://daily-cloudcode-pa.googleapis.com${log.url.startsWith('/') ? '' : '/'}${log.url}`;
        }
    } catch {
        // fallback
    }

    const forwardedRequestObj: HarRequest = {
        method: log.method.toUpperCase(),
        url: upstreamUrl,
        httpVersion: 'HTTP/1.1',
        cookies: [],
        headers: upHeaders,
        queryString: parseQueryStringFromUrl(upstreamUrl),
        postData: effectiveUpstreamBody
            ? {
                  mimeType: upContentType,
                  text: effectiveUpstreamBody,
              }
            : undefined,
        headersSize: -1,
        bodySize: upBodySize,
    };

    // 1. 在客户端 Entry 上挂载扩展字段 (_forwardedRequest 与 _upstream)
    entry._forwardedRequest = forwardedRequestObj;
    entry._upstream = {
        model: log.upstream_model || log.mapped_model || log.model,
        endpoint: upstreamUrl,
    };
    entry.comment = `[1/2 客户端 -> 网关 | Client -> Gateway] Antigravity Request ID: ${log.id}`;

    // 2. 追加第二个独立的 HAR Entry (网关 -> 上游端点)，使任何 HAR 查看工具都能清晰看到完整的转出报文与上游响应！
    const upstreamEntry: HarEntry = {
        startedDateTime,
        time: waitTime,
        request: forwardedRequestObj,
        response: {
            status: log.status,
            statusText: getStatusText(log.status),
            httpVersion: 'HTTP/1.1',
            cookies: [],
            headers: respHeaders,
            content: {
                size: respBodySize,
                mimeType: respContentType,
                text: log.response_body || '',
            },
            redirectURL: '',
            headersSize: -1,
            bodySize: respBodySize,
        },
        cache: {},
        timings: {
            send: 0,
            wait: waitTime,
            receive: receiveTime,
        },
        comment: `[2/2 网关 -> 上游端点 | Gateway -> Upstream] Target: ${endpointName}`,
    };

    entries.push(upstreamEntry);

    return {
        log: {
            version: '1.2',
            creator: {
                name: 'Antigravity Gateway',
                version: '4.9.8-beta.7',
                comment: 'Exported from Antigravity Manager Traffic Monitor',
            },
            pages: [],
            entries,
        },
    };
}

/**
 * 导出格式化后的 HAR JSON 字符串
 */
export function exportToHarString(log: ProxyRequestLog): string {
    const har = buildHarObject(log);
    return JSON.stringify(har, null, 2);
}

/**
 * 触发本地下载 .har 文件
 */
export function downloadHarFile(log: ProxyRequestLog): void {
    const jsonStr = exportToHarString(log);
    const blob = new Blob([jsonStr], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    const safeMethod = log.method.toLowerCase();
    const dateStr = new Date(log.timestamp)
        .toISOString()
        .replace(/[:.]/g, '-')
        .slice(0, 19);
    a.href = url;
    a.download = `request-${safeMethod}-${dateStr}.har`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
}

/**
 * 将请求转换为终端可直接执行的标准 cURL 命令
 */
export function exportToCurl(log: ProxyRequestLog): string {
    const headers = parseHeadersToHarList(log.request_headers);
    const parts: string[] = ['curl'];

    // Method
    const method = log.method.toUpperCase();
    if (method !== 'GET') {
        parts.push(`-X ${method}`);
    }

    // URL
    const targetUrl = log.url.startsWith('http')
        ? log.url
        : `http://127.0.0.1:8045${log.url.startsWith('/') ? '' : '/'}${log.url}`;
    parts.push(`'${targetUrl}'`);

    // Headers
    for (const h of headers) {
        // 过滤掉伪 Header 或部分不需要手动指定的 hop-by-hop 头
        const nameLower = h.name.toLowerCase();
        if (
            nameLower === 'host' ||
            nameLower === 'content-length' ||
            nameLower.startsWith(':')
        ) {
            continue;
        }
        parts.push(`-H '${h.name}: ${h.value.replace(/'/g, "'\\''")}'`);
    }

    // Body
    if (log.request_body && method !== 'GET') {
        parts.push(`--data-raw '${log.request_body.replace(/'/g, "'\\''")}'`);
    }

    return parts.join(' \\\n  ');
}
