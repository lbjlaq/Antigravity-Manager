/**
 * 简要模式必须保留转出报文层级，只把核心段落提前。
 * Run: npx tsx src/components/proxy/__tests__/concisePayload.test.ts
 */
import { extractConcisePayload } from '../concisePayload';

let passed = 0;
let failed = 0;

function test(description: string, fn: () => void): void {
    try {
        fn();
        passed += 1;
        console.log(`  ok  ${description}`);
    } catch (err) {
        failed += 1;
        console.error(`  FAIL  ${description}`);
        console.error(err);
    }
}

function assertEqual<T>(actual: T, expected: T): void {
    if (actual !== expected) {
        throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

function assertDeepEqual(actual: unknown, expected: unknown): void {
    const left = JSON.stringify(actual);
    const right = JSON.stringify(expected);
    if (left !== right) {
        throw new Error(`expected ${right}, got ${left}`);
    }
}

function keys(value: unknown): string[] {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`expected object, got ${JSON.stringify(value)}`);
    }
    return Object.keys(value);
}

test('官方信封：思考配置留在 generationConfig 内，系统提示词保留 role', () => {
    const raw = {
        project: 'aicode-consumers',
        requestId: 'agent/b0cc58a9212f26fb/1790578373562/9d5055b2/23',
        request: {
            contents: [
                { role: 'user', parts: [{ text: 'hello' }] },
                {
                    role: 'model',
                    parts: [
                        { thought: true, text: 'plan', thoughtSignature: 'sig' },
                        { functionCall: { id: 'c1', name: 'view_file', args: { path: 'a.ts' } } },
                    ],
                },
            ],
            systemInstruction: {
                role: 'user',
                parts: [{ text: 'You are a coding agent.' }],
            },
            tools: [
                {
                    functionDeclarations: [
                        {
                            name: 'view_file',
                            description: 'read a file',
                            parameters: { type: 'OBJECT', properties: { path: { type: 'STRING' } } },
                        },
                    ],
                },
            ],
            labels: {
                last_step_index: '22',
                model_enum: 'MODEL_PLACEHOLDER_M318',
                request_id: '9d5055b2-22',
                trajectory_id: '9d5055b2',
                used_claude: 'false',
                used_claude_conservative: 'false',
                used_non_gemini_model: 'false',
            },
            generationConfig: {
                maxOutputTokens: 65536,
                thinkingConfig: { includeThoughts: true, thinkingBudget: -1 },
            },
            sessionId: 'b0cc58a9212f26fb',
        },
        model: 'gemini-3.8-flash-high',
        userAgent: 'antigravity',
        requestType: 'agent',
    };

    const out = JSON.parse(extractConcisePayload(JSON.stringify(raw), 'upstream'));

    assertDeepEqual(keys(out), [
        'requestId',
        'model',
        'request',
        'project',
        'userAgent',
        'requestType',
    ]);
    assertEqual(out._session_thinking_id, undefined);
    assertEqual(out.thinkingConfig, undefined);
    assertEqual(out.model, 'gemini-3.8-flash-high');
    assertEqual(out.requestId, raw.requestId);

    assertDeepEqual(keys(out.request), [
        'generationConfig',
        'systemInstruction',
        'contents',
        'tools',
        'labels',
        'sessionId',
    ]);
    assertDeepEqual(keys(out.request.generationConfig), ['maxOutputTokens', 'thinkingConfig']);
    assertEqual(out.request.generationConfig.maxOutputTokens, 65536);
    assertDeepEqual(out.request.generationConfig.thinkingConfig, {
        includeThoughts: true,
        thinkingBudget: -1,
    });
    assertDeepEqual(keys(out.request.systemInstruction), ['role', 'parts']);
    assertEqual(out.request.systemInstruction.role, 'user');
    assertEqual(out.request.systemInstruction.parts[0].text, 'You are a coding agent.');
    assertDeepEqual(keys(out.request.labels), [
        'last_step_index',
        'model_enum',
        'request_id',
        'trajectory_id',
        'used_claude',
        'used_claude_conservative',
        'used_non_gemini_model',
    ]);
    assertEqual(out.request.contents[1].parts[0].thoughtSignature, 'sig');
    assertDeepEqual(keys(out.request.contents[1].parts[0]), ['thought', 'text', 'thoughtSignature']);
    assertEqual(out.request.contents[1].parts[1].functionCall.name, 'view_file');
    assertEqual(out.request.tools[0].functionDeclarations[0].parameters.properties.path.type, 'STRING');
    assertEqual(out.project, 'aicode-consumers');
    assertEqual(out.userAgent, 'antigravity');
    assertEqual(out.requestType, 'agent');
});

test('扁平 Gemini 报文不把 thinkingConfig 抬到根上，也不丢掉 role', () => {
    const raw = {
        contents: [{ role: 'user', parts: [{ text: 'hi' }] }],
        systemInstruction: { role: 'user', parts: [{ text: 'sys' }] },
        generationConfig: {
            maxOutputTokens: 65536,
            thinkingConfig: { includeThoughts: true, thinkingBudget: -1 },
        },
        model: 'gemini-3.8-flash-high',
        tools: [{ functionDeclarations: [{ name: 'bash' }] }],
    };
    const out = JSON.parse(extractConcisePayload(JSON.stringify(raw), 'request'));
    assertDeepEqual(keys(out), [
        'model',
        'generationConfig',
        'systemInstruction',
        'contents',
        'tools',
    ]);
    assertEqual(out.thinkingConfig, undefined);
    assertEqual(out._session_thinking_id, undefined);
    assertEqual(out.generationConfig.thinkingConfig.thinkingBudget, -1);
    assertEqual(out.systemInstruction.role, 'user');
});

test('根上若本就有 requestId，用原字段名前置，不改写成 _session_thinking_id', () => {
    const raw = {
        model: 'gemini-3.8-flash-high',
        requestId: 'agent/b0cc58a9212f26fb/1790578373562/9d5055b2/23',
        generationConfig: {
            maxOutputTokens: 65536,
            thinkingConfig: { includeThoughts: true, thinkingBudget: -1 },
        },
        systemInstruction: { role: 'user', parts: [{ text: 'sys' }] },
    };
    const out = JSON.parse(extractConcisePayload(JSON.stringify(raw), 'upstream'));
    assertDeepEqual(keys(out), ['requestId', 'model', 'generationConfig', 'systemInstruction']);
    assertEqual(out.requestId, raw.requestId);
});

test('Claude / OpenAI 客户端字段保持各自层级', () => {
    const claude = {
        model: 'claude-sonnet-4-6',
        max_tokens: 64000,
        thinking: { type: 'enabled', budget_tokens: 1024 },
        system: 'you are claude',
        messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }],
        tools: [{ name: 'bash', input_schema: { type: 'object' } }],
    };
    const out = JSON.parse(extractConcisePayload(JSON.stringify(claude), 'request'));
    assertDeepEqual(keys(out), ['model', 'thinking', 'system', 'messages', 'tools', 'max_tokens']);
    assertDeepEqual(out.thinking, { type: 'enabled', budget_tokens: 1024 });
    assertEqual(out.tools[0].input_schema.type, 'object');

    const responses = {
        model: 'gpt-5',
        instructions: 'You are Codex.',
        input: [{ type: 'message', role: 'user', content: 'hi' }],
        max_output_tokens: 1024,
    };
    const responsesOut = JSON.parse(extractConcisePayload(JSON.stringify(responses), 'request'));
    assertDeepEqual(keys(responsesOut), ['model', 'instructions', 'input', 'max_output_tokens']);
    assertEqual(responsesOut.instructions, 'You are Codex.');
    assertEqual(responsesOut.input[0].content, 'hi');
});

test('只替换 inline base64，不改动所在层级', () => {
    const raw = {
        model: 'gemini-3.8-flash-high',
        contents: [
            {
                role: 'user',
                parts: [
                    { text: 'see' },
                    { inlineData: { mimeType: 'image/png', data: 'AAAA' } },
                ],
            },
        ],
    };
    const out = JSON.parse(extractConcisePayload(JSON.stringify(raw), 'request'));
    assertDeepEqual(out.contents[0].parts[1].inlineData, {
        mimeType: 'image/png',
        data: '[base64 image: 4 bytes]',
    });
});

test('响应报文不拆掉 candidates 层级', () => {
    const raw = {
        candidates: [
            {
                content: {
                    role: 'model',
                    parts: [{ thought: true, text: 'think' }, { text: 'answer' }],
                },
                finishReason: 'STOP',
            },
        ],
        usageMetadata: { promptTokenCount: 10, candidatesTokenCount: 2 },
        modelVersion: 'gemini-3.8-flash-high',
    };
    const out = JSON.parse(extractConcisePayload(JSON.stringify(raw), 'response'));
    assertEqual(out.thinking, undefined);
    assertEqual(out.content, undefined);
    assertEqual(out.candidates[0].content.parts[1].text, 'answer');
    assertEqual(out.usageMetadata.promptTokenCount, 10);
    assertDeepEqual(keys(out), ['candidates', 'usageMetadata', 'modelVersion']);
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
