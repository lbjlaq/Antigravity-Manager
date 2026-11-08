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

    const makeAccount = (id: string, name: string, geminiWeeklyPct: number, geminiReset: string, claudeWeeklyPct: number, claudeReset: string, disabled = false, priority = 50): Account => ({
        id,
        name,
        email: `${id}@example.com`,
        proxy_disabled: disabled,
        priority,
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

    // Test 2: Activate Pedal Mode & Priority Boost
    await test('2. Activates Pedal Mode, boosts target accounts to priority 1, leaves other accounts active', async () => {
        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        const modal = usePedalModeStore.getState().pendingModal!;
        await store.activatePedalMode(
            modal.model,
            modal.candidates.map(c => c.accountId),
            [acc1, acc2, acc3, acc4],
            mockSetPriority
        );

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode should be active');
        assertEqual(state.targetModel, 'gemini', 'targetModel should be gemini');
        assert(state.targetAccountIds.includes('acc-3') && state.targetAccountIds.includes('acc-4'), 'target accounts should be acc-3 and acc-4');
        assertEqual(state.originalPriorities['acc-3'], 50, 'Original priority for acc-3 should be saved as 50');
        assertEqual(state.originalPriorities['acc-4'], 50, 'Original priority for acc-4 should be saved as 50');

        // Verify priorities boosted to 1
        assert(priorities.some(p => p.id === 'acc-3' && p.priority === 1), 'acc-3 priority was boosted to 1');
        assert(priorities.some(p => p.id === 'acc-4' && p.priority === 1), 'acc-4 priority was boosted to 1');
        // Non-target accounts acc-1 and acc-2 are NOT modified or disabled
        assert(!priorities.some(p => p.id === 'acc-1'), 'acc-1 was not touched');
        assert(!priorities.some(p => p.id === 'acc-2'), 'acc-2 was not touched');
    });

    // Test 3: Partial exhaustion does not exit
    await test('3. Partial exhaustion (one account 0%, another still has quota) keeps Pedal Mode active', async () => {
        const partiallyDrainedAcc3 = makeAccount('acc-3', 'Account 3', 0, soonReset, 80, farReset);
        const stillActiveAcc4 = makeAccount('acc-4', 'Account 4', 15, soonReset, 80, farReset);

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, acc2, partiallyDrainedAcc3, stillActiveAcc4], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode should remain active because acc-4 still has 15%');
        assertEqual(priorities.length, 0, 'No accounts should be restored yet');
    });

    // Test 4: Complete exhaustion triggers auto-recovery
    await test('4. Complete exhaustion (all target accounts at 0%) automatically exits and restores original priorities', async () => {
        const drainedAcc3 = makeAccount('acc-3', 'Account 3', 0, soonReset, 80, farReset);
        const drainedAcc4 = makeAccount('acc-4', 'Account 4', 0, soonReset, 80, farReset);

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, acc2, drainedAcc3, drainedAcc4], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Pedal mode should have exited');
        assertEqual(state.targetAccountIds.length, 0, 'targetAccountIds should be cleared');

        // Verify priority restoration to 50
        assert(priorities.some(p => p.id === 'acc-3' && p.priority === 50), 'acc-3 priority restored to 50');
        assert(priorities.some(p => p.id === 'acc-4' && p.priority === 50), 'acc-4 priority restored to 50');
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
            originalPriorities: { 'acc-ghost': 50 },
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        // Acc-ghost is missing from accounts list
        await store.checkAndAutoRecover([acc1, acc2], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Should automatically exit pedal mode');
        assert(priorities.some(p => p.id === 'acc-ghost' && p.priority === 50), 'acc-ghost priority should be restored to 50');
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
            originalPriorities: { 'acc-3': 50, 'acc-4': 50 },
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, acc2, expiredAcc3, expiredAcc4], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Should automatically exit pedal mode because reset time is reached');
        assert(priorities.some(p => p.id === 'acc-3' && p.priority === 50), 'acc-3 priority restored to 50');
        assert(priorities.some(p => p.id === 'acc-4' && p.priority === 50), 'acc-4 priority restored to 50');
    });

    // Test 9: Quota data loading / undefined protection prevents premature exit
    await test('9. Temporary missing quota data does not trigger premature 0% drain exit', async () => {
        const unreadyAcc3 = {
            id: 'acc-3',
            name: 'Account 3 (Loading)',
            email: 'acc-3@example.com',
            proxy_disabled: false,
            priority: 1,
            quota: undefined,
        } as any;

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3'],
            originalPriorities: { 'acc-3': 50 },
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, unreadyAcc3], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode must stay active while quota data is loading/unavailable');
        assertEqual(priorities.length, 0, 'No account should be toggled');
    });

    // Test 10: Missing reset_time with positive quota must NOT trigger premature allReset exit
    await test('10. Missing or unparseable reset_time with remaining quota (>0%) must NOT trigger premature exit', async () => {
        const accNoResetTime: Account = {
            id: 'acc-3',
            name: 'Account 3 (No Reset Time)',
            email: 'acc-3@example.com',
            proxy_disabled: false,
            priority: 1,
            quota: {
                last_updated: now,
                subscription_tier: 'PRO',
                quota_groups: [
                    {
                        display_name: 'Gemini Models',
                        buckets: [
                            { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.45, reset_time: undefined as any },
                        ],
                    },
                ],
                models: [],
            },
        } as any;

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3'],
            originalPriorities: { 'acc-3': 50 },
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, accNoResetTime], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode must remain active when account has 45% quota even without resetTime');
        assertEqual(priorities.length, 0, 'No priority should be restored while account still has quota');
    });

    // Test 11: Individual account reaches 0% quota restores its priority without killing pedal mode if others remain
    await test('11. Individual account reaching 0% quota restores its own priority while other targets remain active', async () => {
        const drainedAcc3 = makeAccount('acc-3', 'Account 3 Drained', 0, soonReset, 80, farReset, false, 1);
        const activeAcc4 = makeAccount('acc-4', 'Account 4 Still Active', 25, soonReset, 80, farReset, false, 1);

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3', 'acc-4'],
            originalPriorities: { 'acc-3': 50, 'acc-4': 50 },
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, drainedAcc3, activeAcc4], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal mode must stay active because acc-4 still has 25% quota');
        assert(priorities.some(p => p.id === 'acc-3' && p.priority === 50), 'acc-3 priority must be restored to 50 upon draining');
        assert(!priorities.some(p => p.id === 'acc-4'), 'acc-4 priority must NOT be restored while it still has quota');
    });

    // Test 12: Reset timestamp reached ends the cycle even if the quota snapshot still shows < 90%
    await test('12. Reset time reached exits pedal mode even when quota snapshot is still below 90%', async () => {
        const pastReset = new Date(now - 3600 * 1000).toISOString();
        const resetAcc3 = makeAccount('acc-3', 'Account 3 Reset', 50, pastReset, 80, farReset, false, 1);
        const resetAcc4 = makeAccount('acc-4', 'Account 4 Reset', 50, pastReset, 80, farReset, false, 1);

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3', 'acc-4'],
            originalPriorities: { 'acc-3': 50, 'acc-4': 50 },
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        const priorities: Array<{ id: string; priority: number }> = [];
        const mockSetPriority = async (id: string, priority: number) => {
            priorities.push({ id, priority });
        };

        await store.checkAndAutoRecover([acc1, resetAcc3, resetAcc4], mockSetPriority);

        const state = usePedalModeStore.getState();
        assert(state.active === false, 'Pedal mode must exit once the official reset timestamp is reached');
        assert(priorities.some(p => p.id === 'acc-3' && p.priority === 50), 'acc-3 priority must be restored on reset');
        assert(priorities.some(p => p.id === 'acc-4' && p.priority === 50), 'acc-4 priority must be restored on reset');
    });

    // Test 13: Per-model priority isolation during Pedal Mode activation and exit
    await test('13. Scopes Pedal Mode priority boost to target model family while preserving other model family priority', async () => {
        const customAcc = {
            ...makeAccount('acc-custom', 'Custom Priority Account', 40, soonReset, 85, farReset, false, 50),
            model_priorities: { claude: 80 },
        } as Account;

        const updates: Array<{ id: string; priority: number; modelPriorities?: Record<string, number> }> = [];
        const mockSetPriority = async (id: string, priority: number, modelPriorities?: any) => {
            updates.push({ id, priority, modelPriorities });
        };

        await store.activatePedalMode('gemini', ['acc-custom'], [acc1, customAcc], mockSetPriority);

        assertEqual(updates.length, 1, 'Should update target account once on activate');
        assertEqual(updates[0].id, 'acc-custom');
        assertEqual(updates[0].modelPriorities?.gemini, 1, 'Gemini model priority should be boosted to 1');
        assertEqual(updates[0].modelPriorities?.claude, 80, 'Claude model priority should preserve custom 80');

        updates.length = 0;
        await store.manualExit(mockSetPriority);
        assertEqual(updates.length, 1, 'Should restore target account on exit');
        assertEqual(updates[0].priority, 50, 'Base priority restored to 50');
        assertEqual(updates[0].modelPriorities?.claude, 80, 'Original Claude priority 80 restored');
        assertEqual(updates[0].modelPriorities?.gemini, undefined, 'Temporary Gemini override cleared');
    });

    await test('14. checkAndAutoRecover guards against concurrent/re-entrant executions', async () => {
        const store = usePedalModeStore.getState();
        const testDrainedAcc3 = makeAccount('acc-3', 'Account 3', 0, soonReset, 80, farReset);
        let priorityCalls = 0;
        const delayedPriorityUpdater = async (_id: string, _p: number) => {
            priorityCalls++;
            await new Promise(r => setTimeout(r, 50));
        };
        await store.activatePedalMode('gemini', ['acc-3'], [acc1, testDrainedAcc3], delayedPriorityUpdater);
        priorityCalls = 0;

        // Trigger checkAndAutoRecover twice concurrently with testDrainedAcc3 (which should exit)
        const p1 = store.checkAndAutoRecover([acc1, testDrainedAcc3], delayedPriorityUpdater);
        const p2 = store.checkAndAutoRecover([acc1, testDrainedAcc3], delayedPriorityUpdater);
        await Promise.all([p1, p2]);

        assertEqual(priorityCalls, 1, 'Only one auto-recovery process should execute priority restoration');
    });

    await test('15. checkAndAutoRecover skips redundant legacyToggle calls when account is already enabled', async () => {
        const store = usePedalModeStore.getState();
        const testDrainedAcc3 = makeAccount('acc-3', 'Account 3', 0, soonReset, 80, farReset);
        let toggleCalls = 0;
        const mockToggle = async (_id: string, _enable: boolean) => {
            toggleCalls++;
        };
        // Set state with temporarilyDisabledIds where the account in accounts is already proxy_disabled === false
        (usePedalModeStore as any).setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3'],
            originalPriorities: { 'acc-3': 50 },
            temporarilyDisabledIds: ['acc-1'], // acc1 has proxy_disabled: false
        });

        await store.checkAndAutoRecover([acc1, testDrainedAcc3], async () => {}, mockToggle);
        assertEqual(toggleCalls, 0, 'legacyToggle should NOT be called if account is already enabled');
    });

    await test('16. resetPedalModeState cleanly clears state and removes localStorage key', async () => {
        const store = usePedalModeStore.getState();
        (usePedalModeStore as any).setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-3'],
            originalPriorities: { 'acc-3': 50 },
        });
        localStorage.setItem('antigravity_pedal_mode_state', JSON.stringify({ active: true }));

        await store.resetPedalModeState();
        const state = usePedalModeStore.getState();
        assertEqual(state.active, false, 'active should be false');
        assertEqual(state.targetModel, null, 'targetModel should be null');
        assertEqual(localStorage.getItem('antigravity_pedal_mode_state'), null, 'localStorage key should be deleted');
    });

    await test('17. Exposes modelBadge (e.g. Claude 4.6 vs Claude 5.5) on candidates and supports activating a user-selected subset', async () => {
        const store = usePedalModeStore.getState();
        store.clearSuppress();
        usePedalModeStore.setState({ active: false, pendingModal: null });

        const accClaude46: Account = {
            ...makeAccount('acc-c46', 'Legacy 4.6 Account', 90, farReset, 100, soonReset, false, 50),
            quota: {
                last_updated: now,
                subscription_tier: 'PRO',
                quota_groups: [
                    {
                        display_name: 'Claude and GPT models',
                        buckets: [
                            { bucket_id: '3p-weekly', window: 'weekly', remaining_fraction: 1.0, reset_time: soonReset },
                        ],
                    },
                ],
                models: [
                    { name: 'claude-sonnet-4-6', percentage: 100, reset_time: soonReset },
                    { name: 'claude-opus-4-6-thinking', percentage: 100, reset_time: soonReset },
                ],
            },
        } as any;

        const accClaude55: Account = {
            ...makeAccount('acc-c55', 'Flagship 5.5 Account', 90, farReset, 100, soonReset, false, 50),
            quota: {
                last_updated: now,
                subscription_tier: 'PRO',
                quota_groups: [
                    {
                        display_name: 'Claude and GPT models',
                        buckets: [
                            { bucket_id: '3p-weekly', window: 'weekly', remaining_fraction: 1.0, reset_time: soonReset },
                        ],
                    },
                ],
                models: [
                    { name: 'claude-opus-5-5-high', percentage: 100, reset_time: soonReset },
                    { name: 'claude-sonnet-5-5-high', percentage: 100, reset_time: soonReset },
                ],
            },
        } as any;

        store.checkCandidates([accClaude46, accClaude55]);
        const modal = usePedalModeStore.getState().pendingModal;
        assert(modal !== null, 'Should pop up Claude pedal modal');
        assertEqual(modal?.model, 'claude');
        assertEqual(modal?.candidates.length, 2);

        const c46 = modal?.candidates.find(c => c.accountId === 'acc-c46');
        const c55 = modal?.candidates.find(c => c.accountId === 'acc-c55');
        assertEqual(c46?.modelBadge, 'Claude 4.6', 'Candidate 1 must show Claude 4.6 badge');
        assertEqual(c55?.modelBadge, 'Claude 5.5', 'Candidate 2 must show Claude 5.5 badge');

        // User unchecks acc-c46 and only selects acc-c55
        const boostedIds: string[] = [];
        await store.activatePedalMode('claude', ['acc-c55'], [accClaude46, accClaude55], async (id) => {
            boostedIds.push(id);
        });

        const activeState = usePedalModeStore.getState();
        assertEqual(activeState.active, true);
        assertEqual(activeState.targetAccountIds.length, 1, 'Only selected account should be in targetAccountIds');
        assertEqual(activeState.targetAccountIds[0], 'acc-c55');
        assertEqual(boostedIds.length, 1, 'Only selected account should have priority boosted');
        assertEqual(boostedIds[0], 'acc-c55');
    });

    // Test 18: Candidate with >=90% remaining quota does NOT trigger immediate auto-recovery upon activation
    await test('18. Candidate with high remaining quota (e.g. 95%) keeps Pedal Mode active before reset time', async () => {
        const inFiveHours = new Date(now + 5 * 3600 * 1000).toISOString();
        const highQuotaAcc = makeAccount('acc-high', 'High Quota Account', 95, inFiveHours, 95, inFiveHours);

        usePedalModeStore.setState({
            active: true,
            targetModel: 'gemini',
            targetAccountIds: ['acc-high'],
            originalPriorities: { 'acc-high': 50 },
            originalModelPriorities: {},
            snapshotDisabledIds: [],
            temporarilyDisabledIds: [],
        });

        await store.checkAndAutoRecover([acc1, highQuotaAcc], async () => {});

        const state = usePedalModeStore.getState();
        assert(state.active === true, 'Pedal Mode MUST remain active for accounts with high remaining quota (>90%) before reset time');
        assertEqual(state.targetAccountIds.length, 1);
    });

    console.log(`\nPedal Mode Tests Summary: ${passed} passed, ${failed} failed.`);
    if (failed > 0) {
        throw new Error(`Pedal Mode tests failed: ${failed}`);
    }
}

runTests();
