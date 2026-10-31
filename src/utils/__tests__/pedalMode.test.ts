/**
 * Unit verification for Pedal Mode (蹬额度模式) logic and state management.
 * Run: npx tsx src/utils/__tests__/pedalMode.test.ts
 */
import { usePedalModeStore } from '../../stores/usePedalModeStore';
import type { Account } from '../../types/account';

let passed = 0;
let failed = 0;

async function test(description: string, fn: () => void | Promise<void>): Promise<void> {
    try {
        await fn();
        passed++;
        console.log(`  PASS: ${description}`);
    } catch (err) {
        failed++;
        console.error(`  FAIL: ${description} — ${(err as Error).message}`);
    }
}

function assert(condition: boolean, msg: string): void {
    if (!condition) {
        throw new Error(msg);
    }
}

function assertEqual<T>(actual: T, expected: T, msg?: string): void {
    if (actual !== expected) {
        throw new Error(`${msg ? msg + ': ' : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

// Mock localStorage
const mockStorage: Record<string, string> = {};
(globalThis as any).localStorage = {
    getItem: (key: string) => mockStorage[key] || null,
    setItem: (key: string, val: string) => { mockStorage[key] = val; },
    removeItem: (key: string) => { delete mockStorage[key]; },
    clear: () => { Object.keys(mockStorage).forEach(k => delete mockStorage[k]); },
};

async function runTests() {
    console.log('--- Starting Pedal Mode Unit Tests ---');

    // Setup mock accounts
    const now = Date.now();
    const soonReset = new Date(now + 12 * 3600 * 1000).toISOString(); // 12 hours left
    const farReset = new Date(now + 48 * 3600 * 1000).toISOString();  // 48 hours left

    const makeAccount = (id: string, name: string, geminiWeeklyPct: number, geminiReset: string, claudeWeeklyPct: number, claudeReset: string, disabled = false): Account => ({
        id,
        name,
        email: `${id}@example.com`,
        proxy_disabled: disabled,
        quota: {
            last_updated: now,
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 1, reset_time: soonReset },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: geminiWeeklyPct / 100, reset_time: geminiReset },
                    ],
                },
                {
                    display_name: 'Claude & GPT Models',
                    buckets: [
                        { bucket_id: 'claude-5h', window: '5h', remaining_fraction: 1, reset_time: soonReset },
                        { bucket_id: 'claude-weekly', window: 'weekly', remaining_fraction: claudeWeeklyPct / 100, reset_time: claudeReset },
                    ],
                },
            ],
            models: [],
        },
    } as any);

    const acc1 = makeAccount('acc-1', 'Account 1 (Stable)', 80, farReset, 90, farReset);
    const acc2 = makeAccount('acc-2', 'Account 2 (Disabled by user)', 50, farReset, 50, farReset, true);
    const acc3 = makeAccount('acc-3', 'Account 3 (Gemini Imminent)', 45, soonReset, 80, farReset);
    const acc4 = makeAccount('acc-4', 'Account 4 (Gemini Imminent 2)', 30, soonReset, 80, farReset);

    const store = usePedalModeStore.getState();

    // Test 1: Candidate detection
    await test('1. Detects imminent Gemini accounts with >5% weekly quota within 24h', () => {
        usePedalModeStore.setState({ active: false, pendingModal: null, suppressNotice: { gemini: false, claude: false } });
        store.checkCandidates([acc1, acc2, acc3, acc4]);

        const modal = usePedalModeStore.getState().pendingModal;
        assert(modal !== null, 'pendingModal should not be null');
        assertEqual(modal?.model, 'gemini', 'Target model should be gemini');
        assertEqual(modal?.candidates.length, 2, 'Should find 2 imminent candidates');
        assertEqual(modal?.candidates[0].accountId, 'acc-3', 'Candidate 1 should be acc-3');
        assertEqual(modal?.candidates[1].accountId, 'acc-4', 'Candidate 2 should be acc-4');
    });

    // Test 2: Activate Pedal Mode & Snapshot Isolation
    await test('2. Activates Pedal Mode, locks non-target accounts, preserves pre-existing disabled state', async () => {
        const toggled: Array<{ id: string; enable: boolean; reason?: string }> = [];
        const mockToggle = async (id: string, enable: boolean, reason?: string) => {
            toggled.push({ id, enable, reason });
        };

        const modal = usePedalModeStore.getState().pendingModal!;
        await store.activatePedalMode(
            modal.model,
            modal.candidates.map(c => c.accountId),
            [acc1, acc2, acc3, acc4],
            mockToggle
        );

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode should be active');
        assertEqual(state.targetModel, 'gemini', 'targetModel should be gemini');
        assert(state.targetAccountIds.includes('acc-3') && state.targetAccountIds.includes('acc-4'), 'target accounts should be acc-3 and acc-4');
        assertEqual(state.snapshotDisabledIds.length, 1, 'Pre-disabled accounts snapshot should contain acc-2');
        assertEqual(state.snapshotDisabledIds[0], 'acc-2', 'acc-2 should be in snapshotDisabledIds');
        assertEqual(state.temporarilyDisabledIds.length, 1, 'acc-1 should be temporarily disabled');
        assertEqual(state.temporarilyDisabledIds[0], 'acc-1', 'acc-1 should be in temporarilyDisabledIds');

        // Check toggle calls
        assert(toggled.some(t => t.id === 'acc-1' && t.enable === false && t.reason === 'pedal_mode_locked'), 'acc-1 was toggled off with pedal_mode_locked');
        assert(!toggled.some(t => t.id === 'acc-2'), 'acc-2 was not toggled because it was already disabled');
    });

    // Test 3: Partial exhaustion does not exit
    await test('3. Partial exhaustion (one account 0%, another still has quota) keeps Pedal Mode active', async () => {
        const partiallyDrainedAcc3 = makeAccount('acc-3', 'Account 3', 0, soonReset, 80, farReset);
        const stillActiveAcc4 = makeAccount('acc-4', 'Account 4', 15, soonReset, 80, farReset);

        const toggled: Array<{ id: string; enable: boolean }> = [];
        const mockToggle = async (id: string, enable: boolean) => {
            toggled.push({ id, enable });
        };

        await store.checkAndAutoRecover([acc1, acc2, partiallyDrainedAcc3, stillActiveAcc4], mockToggle);

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode should remain active because acc-4 still has 15%');
        assertEqual(toggled.length, 0, 'No accounts should be restored yet');
    });

    // Test 4: Complete exhaustion triggers auto-recovery
    await test('4. Complete exhaustion (all target accounts at 0%) automatically exits and restores only temporarily disabled accounts', async () => {
        const drainedAcc3 = makeAccount('acc-3', 'Account 3', 0, soonReset, 80, farReset);
        const drainedAcc4 = makeAccount('acc-4', 'Account 4', 0, soonReset, 80, farReset);

        const toggled: Array<{ id: string; enable: boolean }> = [];
        const mockToggle = async (id: string, enable: boolean) => {
            toggled.push({ id, enable });
        };

        await store.checkAndAutoRecover([acc1, acc2, drainedAcc3, drainedAcc4], mockToggle);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Pedal mode should have exited');
        assertEqual(state.targetAccountIds.length, 0, 'targetAccountIds should be cleared');

        // Verify restoration: only acc-1 was restored, NOT acc-2
        assert(toggled.some(t => t.id === 'acc-1' && t.enable === true), 'acc-1 was restored');
        assert(!toggled.some(t => t.id === 'acc-2'), 'acc-2 was NOT restored (kept user-disabled)');
    });

    // Test 5: Suppress model prevents re-prompting
    await test('5. Suppressing Gemini model prevents candidate detection for Gemini', () => {
        usePedalModeStore.setState({ active: false, pendingModal: null });
        store.suppressModel('gemini');

        const state = usePedalModeStore.getState();
        assertEqual(state.suppressNotice.gemini, true, 'Gemini suppress should be true');

        store.checkCandidates([acc1, acc2, acc3, acc4]);
        const modal = usePedalModeStore.getState().pendingModal;
        assert(modal === null, 'No modal should appear because Gemini is suppressed');
    });

    // Test 6: Disabled accounts with imminent quota are filtered out
    await test('6. Accounts manually disabled by user are never recommended as pedal candidates', () => {
        store.clearSuppress();
        const disabledImminentAcc = makeAccount('acc-disabled', 'Disabled Imminent', 50, soonReset, 50, soonReset, true);
        usePedalModeStore.setState({ active: false, pendingModal: null });
        store.checkCandidates([acc1, disabledImminentAcc]);

        const modal = usePedalModeStore.getState().pendingModal;
        assert(modal === null, 'Disabled account must not trigger pedal candidate modal');
    });

    // Test 7: Target accounts deleted triggers auto-recovery guard
    await test('7. Automatically exits and recovers when target accounts are deleted from account list', async () => {
        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-ghost'],
            snapshotDisabledIds: ['acc-2'],
            temporarilyDisabledIds: ['acc-1'],
        });

        const toggled: Array<{ id: string; enable: boolean }> = [];
        const mockToggle = async (id: string, enable: boolean) => {
            toggled.push({ id, enable });
        };

        // Acc-ghost is missing from accounts list
        await store.checkAndAutoRecover([acc1, acc2], mockToggle);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Should automatically exit pedal mode');
        assert(toggled.some(t => t.id === 'acc-1' && t.enable === true), 'acc-1 should be restored');
    });

    // Test 8: Official reset time reached (diffHours <= 0) triggers auto-recovery even if quota > 0%
    await test('8. Automatically exits when official reset time is reached (diffHours <= 0) even with remaining quota', async () => {
        const pastReset = new Date(now - 600 * 1000).toISOString(); // 10 minutes ago
        const expiredAcc3 = makeAccount('acc-3', 'Account 3 Expired', 20, pastReset, 80, farReset);
        const expiredAcc4 = makeAccount('acc-4', 'Account 4 Expired', 15, pastReset, 80, farReset);

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3', 'acc-4'],
            snapshotDisabledIds: ['acc-2'],
            temporarilyDisabledIds: ['acc-1'],
        });

        const toggled: Array<{ id: string; enable: boolean }> = [];
        const mockToggle = async (id: string, enable: boolean) => {
            toggled.push({ id, enable });
        };

        await store.checkAndAutoRecover([acc1, acc2, expiredAcc3, expiredAcc4], mockToggle);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Should automatically exit pedal mode because reset time is reached');
        assert(toggled.some(t => t.id === 'acc-1' && t.enable === true), 'acc-1 should be restored');
    });

    // Test 9: Quota data loading / undefined protection prevents premature exit
    await test('9. Temporary missing quota data does not trigger premature 0% drain exit', async () => {
        const unreadyAcc3 = {
            id: 'acc-3',
            name: 'Account 3 (Loading)',
            email: 'acc-3@example.com',
            proxy_disabled: false,
            quota: undefined,
        } as any;

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3'],
            snapshotDisabledIds: ['acc-2'],
            temporarilyDisabledIds: ['acc-1'],
        });

        const toggled: Array<{ id: string; enable: boolean }> = [];
        const mockToggle = async (id: string, enable: boolean) => {
            toggled.push({ id, enable });
        };

        await store.checkAndAutoRecover([acc1, unreadyAcc3], mockToggle);

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode must stay active while quota data is loading/unavailable');
        assertEqual(toggled.length, 0, 'No account should be toggled');
    });

    console.log(`\nPedal Mode Tests Summary: ${passed} passed, ${failed} failed.`);
    if (failed > 0) {
        throw new Error(`Pedal Mode tests failed: ${failed}`);
    }
}

runTests();
