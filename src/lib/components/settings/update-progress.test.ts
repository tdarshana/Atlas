import { describe, expect, it } from 'vitest';

import { updateProgressPercent } from './update-progress';

describe('updateProgressPercent', () => {
	it('is 0 before anything is known', () => {
		expect(updateProgressPercent(null)).toBe(0);
	});

	it('is 0 while the server has not sent a content length', () => {
		expect(updateProgressPercent({ downloaded: 1024, total: null })).toBe(0);
	});

	it('rounds to the nearest whole percent', () => {
		expect(updateProgressPercent({ downloaded: 1, total: 3 })).toBe(33);
	});

	it('never exceeds 100 even if downloaded overshoots total', () => {
		expect(updateProgressPercent({ downloaded: 200, total: 100 })).toBe(100);
	});

	it('is 100 once fully downloaded', () => {
		expect(updateProgressPercent({ downloaded: 100, total: 100 })).toBe(100);
	});
});
