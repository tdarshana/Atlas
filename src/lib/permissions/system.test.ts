// The pure half of the Permissions view: which row a status maps onto, what its badge
// says, and what the counts add up to.

import { describe, expect, it } from 'vitest';
import {
	grantedCount,
	PERMISSIONS,
	summaryText,
	toRows,
	toState,
	type PermissionStatus
} from './system';

const ALL_GRANTED: PermissionStatus[] = PERMISSIONS.map((p) => ({
	id: p.id,
	granted: 'granted',
	detail: null
}));

describe('toState', () => {
	it('takes the five states the Rust side sends', () => {
		expect(toState('granted')).toBe('granted');
		expect(toState('denied')).toBe('denied');
		expect(toState('not_determined')).toBe('not_determined');
		expect(toState('unknown')).toBe('unknown');
		expect(toState('not_applicable')).toBe('not_applicable');
	});

	it('reads anything else as unknown rather than trusting it', () => {
		expect(toState('yes')).toBe('unknown');
		expect(toState('')).toBe('unknown');
		expect(toState(undefined)).toBe('unknown');
		expect(toState(null)).toBe('unknown');
	});
});

describe('toRows', () => {
	it('draws the four permissions in order, whatever order the statuses arrive in', () => {
		const rows = toRows([...ALL_GRANTED].reverse());
		expect(rows.map((r) => r.id)).toEqual([
			'notifications',
			'automation-finder',
			'accessibility',
			'files'
		]);
		expect(rows.every((r) => r.badge === 'Granted' && r.tone === 'success')).toBe(true);
	});

	it('badges each state in sentence case', () => {
		const rows = toRows([
			{ id: 'notifications', granted: 'denied' },
			{ id: 'automation-finder', granted: 'not_determined' },
			{ id: 'accessibility', granted: 'not_applicable' },
			{ id: 'files', granted: 'unknown' }
		]);
		expect(rows.map((r) => r.badge)).toEqual([
			'Denied',
			'Not determined',
			'Not applicable',
			'Unknown'
		]);
		expect(rows.map((r) => r.tone)).toEqual(['danger', 'warning', 'neutral', 'neutral']);
	});

	it('keeps a row the status call said nothing about, as unknown', () => {
		const rows = toRows([{ id: 'notifications', granted: 'granted' }]);
		expect(rows).toHaveLength(4);
		expect(rows[1].state).toBe('unknown');
		expect(rows[1].detail).toBeNull();
	});

	it('carries the detail through so the user learns which path failed', () => {
		const rows = toRows([{ id: 'files', granted: 'denied', detail: '/Users/x/code: denied' }]);
		expect(rows[3].detail).toBe('/Users/x/code: denied');
	});

	it('offers Request where the app can prompt and the answer is still open', () => {
		const open = toRows([
			{ id: 'notifications', granted: 'not_determined' },
			{ id: 'automation-finder', granted: 'unknown' },
			{ id: 'accessibility', granted: 'not_determined' },
			{ id: 'files', granted: 'not_determined' }
		]);
		// The files row has no prompt Atlas can raise at all, whatever its state.
		expect(open.map((r) => r.canRequest)).toEqual([true, true, true, false]);

		const settled = toRows(ALL_GRANTED);
		expect(settled.every((r) => !r.canRequest)).toBe(true);
	});

	/**
	 * `AXIsProcessTrustedWithOptions` raises its "open System Settings" sheet whenever the
	 * process is untrusted, and macOS reports an untrusted process as denied rather than as
	 * undecided, so a denied Accessibility row is exactly where the prompt is useful. The
	 * Apple Event and notification grants answer a second ask with the stored decision, so
	 * asking again there would do nothing.
	 */
	it('keeps Request on a denied Accessibility row, and only that one', () => {
		const denied = toRows([
			{ id: 'notifications', granted: 'denied' },
			{ id: 'automation-finder', granted: 'denied' },
			{ id: 'accessibility', granted: 'denied' },
			{ id: 'files', granted: 'denied' }
		]);
		expect(denied.map((r) => r.canRequest)).toEqual([false, false, true, false]);
	});

	it('never offers Request for a permission this platform does not have', () => {
		const na = toRows([
			{ id: 'notifications', granted: 'granted' },
			{ id: 'automation-finder', granted: 'not_applicable' },
			{ id: 'accessibility', granted: 'not_applicable' },
			{ id: 'files', granted: 'granted' }
		]);
		expect(na.map((r) => r.canRequest)).toEqual([false, false, false, false]);
	});

	it('names why each permission is needed, and warns about the terminal for Finder', () => {
		const rows = toRows(ALL_GRANTED);
		expect(rows.every((r) => r.why.length > 0)).toBe(true);
		expect(rows[1].note).toContain('terminal');
	});
});

describe('grantedCount', () => {
	it('counts the granted rows out of the ones that apply here', () => {
		expect(grantedCount(toRows(ALL_GRANTED))).toEqual({ granted: 4, total: 4 });
	});

	it('leaves a not applicable row out of both halves', () => {
		const rows = toRows([
			{ id: 'notifications', granted: 'granted' },
			{ id: 'automation-finder', granted: 'not_applicable' },
			{ id: 'accessibility', granted: 'not_applicable' },
			{ id: 'files', granted: 'granted' }
		]);
		expect(grantedCount(rows)).toEqual({ granted: 2, total: 2 });
	});
});

describe('summaryText', () => {
	it('reads as one sentence, with the plugin count singular where it should be', () => {
		expect(summaryText(toRows(ALL_GRANTED), 1)).toBe(
			'4 of 4 system permissions granted · 1 plugin with grants'
		);
		expect(summaryText(toRows(ALL_GRANTED), 0)).toBe(
			'4 of 4 system permissions granted · 0 plugins with grants'
		);
	});
});
