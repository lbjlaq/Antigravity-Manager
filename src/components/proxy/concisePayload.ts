/**
 * 流量日志「简要模式」。
 *
 * 只做两件事：
 * 1. 省略图片 / base64 等大体积叶子，其余字段原样保留（不增删、不改名、不抬升层级）。
 * 2. 在报文根节点，以及 Antigravity 信封的 `request` 节点，把核心段落提前：
 *    标识 → 模型 → generationConfig（思考参数仍在其内部）→ 系统提示词 → 对话 → 工具。
 *    `generationConfig`、`systemInstruction`、`labels`、单条 content 的内部键序保持转出原文。
 */

export interface ConcisePayloadLog {
    input_tokens?: number;
    output_tokens?: number;
    cached_tokens?: number | null;
}

const ID_KEYS = [
    'requestId',
    'id',
    '_session_thinking_id',
    'sessionId',
    'session_id',
    'conversation_id',
] as const;

/** 这些键只在「自己所在的那一层」提前，绝不会从子对象里抽出来。 */
const SECTION_KEYS = [
    'model',
    'generationConfig',
    'generation_config',
    'thinking',
    'reasoning_effort',
    'reasoning',
    'reasoning_content',
    'thinkingConfig',
    'thinking_config',
    'thinking_signature',
    'thought_signature',
    'signature',
    'thoughtSignature',
    'system',
    'systemInstruction',
    'instructions',
    'contents',
    'messages',
    'input',
    'prompt',
    'content',
    'output',
    'tool_calls',
    'tools',
    'request',
    'choices',
    'candidates',
    'usage',
    'usageMetadata',
    '_timing',
] as const;

function orderKeys(obj: Record<string, unknown>, priority: readonly string[]): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    const used = new Set<string>();
    for (const key of priority) {
        if (Object.prototype.hasOwnProperty.call(obj, key)) {
            out[key] = obj[key];
            used.add(key);
        }
    }
    for (const key of Object.keys(obj)) {
        if (!used.has(key)) out[key] = obj[key];
    }
    return out;
}

function slimValue(val: unknown): unknown {
    if (typeof val === 'string' || val == null || typeof val !== 'object') return val;
    if (Array.isArray(val)) return val.map(slimValue);

    const record = val as Record<string, unknown>;
    const mime = record.mimeType ?? record.mime_type;
    const isInlineBlob = typeof mime === 'string' && typeof record.data === 'string';
    const isBase64Source = record.type === 'base64' && typeof record.data === 'string';

    const out: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(record)) {
        if ((isInlineBlob || isBase64Source) && key === 'data' && typeof child === 'string') {
            out[key] = `[base64 image: ${child.length} bytes]`;
            continue;
        }
        if (key === 'url' && typeof child === 'string' && child.startsWith('data:')) {
            const comma = child.indexOf(',');
            const header = comma >= 0 ? child.slice(0, comma) : child.slice(0, 64);
            out[key] = `${header},[base64 image: ${child.length} bytes]`;
            continue;
        }
        out[key] = slimValue(child);
    }
    return out;
}

function presentPayload(slimmed: unknown): unknown {
    if (!slimmed || typeof slimmed !== 'object' || Array.isArray(slimmed)) return slimmed;
    const record = slimmed as Record<string, unknown>;
    const withOrderedRequest: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(record)) {
        if (key === 'request' && child && typeof child === 'object' && !Array.isArray(child)) {
            // 信封内层只提前思考配置 / 系统提示词 / 对话 / 工具，sessionId、labels 留在原文相对位置。
            withOrderedRequest[key] = orderKeys(child as Record<string, unknown>, SECTION_KEYS);
        } else {
            withOrderedRequest[key] = child;
        }
    }
    return orderKeys(withOrderedRequest, [...ID_KEYS, ...SECTION_KEYS]);
}

function attachResponseUsage(obj: Record<string, unknown>, log: ConcisePayloadLog): void {
    if (obj.usage != null || obj.usageMetadata != null) return;
    if (!log.input_tokens && !log.output_tokens) return;
    const input = log.input_tokens ?? 0;
    const cached = log.cached_tokens ?? 0;
    const totalIn = cached > input ? input + cached : input;
    const output = log.output_tokens ?? 0;
    const usage: Record<string, unknown> = {
        input_tokens: totalIn,
        output_tokens: output,
        total_tokens: totalIn + output,
    };
    if (log.cached_tokens != null) {
        usage.cached_tokens = log.cached_tokens;
        if (totalIn > 0) {
            const rate = Math.min(100, Math.max(0, (log.cached_tokens / totalIn) * 100));
            usage.cache_hit_rate = `${rate.toFixed(1)}%`;
        }
    }
    obj.usage = usage;
}

export function extractConcisePayload(
    rawStr: string | undefined,
    kind: 'request' | 'upstream' | 'response' = 'request',
    log?: ConcisePayloadLog | null,
): string {
    if (!rawStr) return '';
    let parsed: unknown;
    try {
        parsed = JSON.parse(rawStr);
    } catch {
        return rawStr;
    }
    if (typeof parsed === 'string') {
        try {
            parsed = JSON.parse(parsed);
        } catch {
            return rawStr;
        }
    }
    if (!parsed || typeof parsed !== 'object') return rawStr;

    const slimmed = slimValue(parsed);
    if (kind === 'response' && log && slimmed && typeof slimmed === 'object' && !Array.isArray(slimmed)) {
        attachResponseUsage(slimmed as Record<string, unknown>, log);
    }
    return JSON.stringify(presentPayload(slimmed), null, 2);
}
