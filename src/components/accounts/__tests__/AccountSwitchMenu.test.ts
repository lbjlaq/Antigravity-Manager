/**
 * Unit tests for AccountSwitchMenu portal positioning and clipping boundary logic.
 * Run: npx tsx src/components/accounts/__tests__/AccountSwitchMenu.test.ts
 */
import {
    computeAccountSwitchMenuPosition,
    isTriggerVisibleInClipBounds,
    type RectLike,
} from '../accountSwitchMenuPosition';

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

function assertTrue(condition: boolean, message: string): void {
    if (!condition) {
        throw new Error(message);
    }
}

const VIEWPORT = { width: 1280, height: 800 };
const MENU_SIZE = { width: 220, height: 56 };

test('flips downward on first table row right below sticky table header', () => {
    // Navbar + toolbar + thead ends at y=206; first row trigger starts at y=214
    const triggerRect: RectLike = { top: 214, bottom: 238, left: 1120, right: 1180 };
    const clipBounds: RectLike = { top: 206, bottom: 740, left: 40, right: 1250 };

    const pos = computeAccountSwitchMenuPosition({
        triggerRect,
        menuSize: MENU_SIZE,
        viewport: VIEWPORT,
        clipBounds,
    });

    assertEqual(pos.placement, 'bottom');
    assertEqual(pos.top, 238 + 6);
    assertEqual(pos.left, 1180 - 220);
});

test('opens upward on middle/lower rows when sufficient space exists below header', () => {
    const triggerRect: RectLike = { top: 340, bottom: 364, left: 1120, right: 1180 };
    const clipBounds: RectLike = { top: 206, bottom: 740, left: 40, right: 1250 };

    const pos = computeAccountSwitchMenuPosition({
        triggerRect,
        menuSize: MENU_SIZE,
        viewport: VIEWPORT,
        clipBounds,
    });

    assertEqual(pos.placement, 'top');
    assertEqual(pos.top, 340 - 6 - 56);
    assertEqual(pos.left, 1180 - 220);
});

test('opens upward on bottom row even when bottom clip space is tight', () => {
    const triggerRect: RectLike = { top: 710, bottom: 734, left: 1120, right: 1180 };
    const clipBounds: RectLike = { top: 206, bottom: 740, left: 40, right: 1250 };

    const pos = computeAccountSwitchMenuPosition({
        triggerRect,
        menuSize: MENU_SIZE,
        viewport: VIEWPORT,
        clipBounds,
    });

    assertEqual(pos.placement, 'top');
    assertEqual(pos.top, 710 - 6 - 56);
});

test('falls back to viewport space when container is tight on both top and bottom', () => {
    // 1-row short table near bottom of a small window: header directly above, container bottom directly below,
    // but viewport still has plenty of room above
    const triggerRect: RectLike = { top: 250, bottom: 274, left: 400, right: 460 };
    const clipBounds: RectLike = { top: 242, bottom: 280, left: 40, right: 600 };

    const pos = computeAccountSwitchMenuPosition({
        triggerRect,
        menuSize: MENU_SIZE,
        viewport: { width: 640, height: 300 },
        clipBounds,
    });

    assertEqual(pos.placement, 'top');
    assertEqual(pos.top, 250 - 6 - 56);
});

test('clamps horizontally inside viewport when trigger is near left edge', () => {
    const triggerRect: RectLike = { top: 300, bottom: 324, left: 40, right: 100 };
    const clipBounds: RectLike = { top: 150, bottom: 740, left: 20, right: 1250 };

    const pos = computeAccountSwitchMenuPosition({
        triggerRect,
        menuSize: MENU_SIZE,
        viewport: VIEWPORT,
        clipBounds,
    });

    assertTrue(pos.left >= 8, `expected left >= 8, got ${pos.left}`);
    assertEqual(pos.left, 8);
});

test('aligns with trigger left edge in RTL mode and stays within viewport', () => {
    const triggerRect: RectLike = { top: 300, bottom: 324, left: 40, right: 100 };
    const clipBounds: RectLike = { top: 150, bottom: 740, left: 20, right: 1250 };

    const pos = computeAccountSwitchMenuPosition({
        triggerRect,
        menuSize: MENU_SIZE,
        viewport: VIEWPORT,
        clipBounds,
        isRtl: true,
    });

    assertEqual(pos.left, 40);
});

test('detects when trigger scrolls out of clipping container', () => {
    const clipBounds: RectLike = { top: 206, bottom: 740, left: 40, right: 1250 };

    assertEqual(
        isTriggerVisibleInClipBounds({ top: 210, bottom: 234, left: 1120, right: 1180 }, clipBounds),
        true
    );
    assertEqual(
        isTriggerVisibleInClipBounds({ top: 170, bottom: 200, left: 1120, right: 1180 }, clipBounds),
        false
    );
    assertEqual(
        isTriggerVisibleInClipBounds({ top: 745, bottom: 770, left: 1120, right: 1180 }, clipBounds),
        false
    );
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
