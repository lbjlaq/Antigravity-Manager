type CapturedFetch = {
  url: string;
  method: string;
  body?: string;
};

function assertEqual(actual: unknown, expected: unknown, label?: string) {
  if (actual !== expected) {
    throw new Error(
      `${label ? `${label}: ` : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}

function assertDeepEqual(actual: unknown, expected: unknown, label?: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(
      `${label ? `${label}: ` : ''}expected ${expectedJson}, got ${actualJson}`,
    );
  }
}

async function runTests() {
  const captured: CapturedFetch[] = [];
  let mockJsonResponse: unknown = {};

  const memoryStorage = {
    getItem: () => null,
    setItem: () => {},
    removeItem: () => {},
  };

  (globalThis as Record<string, unknown>).window = {
    location: { origin: 'http://127.0.0.1:8045', protocol: 'http:' },
    dispatchEvent: () => true,
  };
  (globalThis as Record<string, unknown>).sessionStorage = memoryStorage;
  (globalThis as Record<string, unknown>).localStorage = memoryStorage;
  (globalThis as Record<string, unknown>).fetch = async (
    url: string | URL | Request,
    init?: RequestInit,
  ) => {
    captured.push({
      url: String(url),
      method: init?.method || 'GET',
      body: typeof init?.body === 'string' ? init.body : undefined,
    });
    return {
      ok: true,
      status: 200,
      text: async () => JSON.stringify(mockJsonResponse),
      json: async () => mockJsonResponse,
    } as Response;
  };

  const { request } = await import('../request');

  let passed = 0;
  let failed = 0;

  async function test(name: string, fn: () => Promise<void>) {
    captured.length = 0;
    try {
      await fn();
      console.log(`  ok  ${name}`);
      passed++;
    } catch (error) {
      console.error(`  FAIL ${name}:`, error);
      failed++;
    }
  }

  await test('preserves query params matching URL substrings and substitutes path keys', async () => {
    mockJsonResponse = [];
    await request('get_proxy_logs_paginated', {
      limit: 100,
      offset: 0,
      filter: 'api',
      errors_only: false,
    });
    assertEqual(captured.length, 1, 'request count');
    assertEqual(captured[0].method, 'GET', 'method');
    assertEqual(
      captured[0].url,
      '/api/logs?limit=100&offset=0&filter=api&errors_only=false',
      'url',
    );
  });

  await test('maps minute stats and Droid sync endpoints accurately', async () => {
    mockJsonResponse = { ok: true };
    await request('get_token_stats_minute', { minutes: 60, stepMinutes: 5 });
    await request('get_token_stats_model_trend_minute', { minutes: 60 });
    await request('get_token_stats_account_trend_minute', { minutes: 120 });

    mockJsonResponse = 2;
    const count = await request<number>('execute_droid_sync', { force: true });
    assertEqual(count, 2, 'droid sync count');

    assertDeepEqual(
      captured.map((c) => `${c.method} ${c.url}`),
      [
        'GET /api/stats/token/minute?minutes=60&stepMinutes=5',
        'GET /api/stats/token/model-trend/minute?minutes=60',
        'GET /api/stats/token/account-trend/minute?minutes=120',
        'POST /api/proxy/droid/sync',
      ],
      'mapped requests',
    );
    assertEqual(captured[3].body, JSON.stringify({ force: true }), 'droid sync body');
  });

  await test('substitutes accountId into remove_account_forbidden_model path without duplicating in body', async () => {
    mockJsonResponse = { removed: true };
    await request('remove_account_forbidden_model', {
      accountId: 'acc-123',
      model: 'gemini-4-neon',
    });
    assertEqual(captured.length, 1, 'request count');
    assertEqual(captured[0].method, 'POST', 'method');
    assertEqual(captured[0].url, '/api/accounts/acc-123/remove-forbidden-model', 'url');
    assertEqual(captured[0].body, JSON.stringify({ model: 'gemini-4-neon' }), 'body');
  });

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
  }
}

runTests();
