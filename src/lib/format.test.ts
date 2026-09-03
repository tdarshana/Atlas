import { describe, expect, it } from 'vitest';
import { duration, relativeAge } from './format';

describe('duration', () => {
	it('formats a finished span under a minute as 0m Ns', () => {
		expect(duration('2026-09-03T09:00:00Z', '2026-09-03T09:00:12Z')).toBe('0m 12s');
	});

	it('formats a finished span over a minute as Mm Ns', () => {
		expect(duration('2026-09-03T09:00:00Z', '2026-09-03T09:01:42Z')).toBe('1m 42s');
	});

	it('reads the elapsed time from start to now for a run still in progress', () => {
		const now = new Date('2026-09-03T09:01:42Z').getTime();
		expect(duration('2026-09-03T09:00:00Z', null, now)).toBe('1m 42s');
	});

	it('keeps climbing as `now` advances, one fake poll tick apart', () => {
		const started = '2026-09-03T09:00:00Z';
		const tick1 = new Date('2026-09-03T09:00:12Z').getTime();
		const tick2 = new Date('2026-09-03T09:00:34Z').getTime();
		expect(duration(started, null, tick1)).toBe('0m 12s');
		expect(duration(started, null, tick2)).toBe('0m 34s');
	});

	it('reads n/a only when started_at itself does not parse', () => {
		expect(duration('not a date', null)).toBe('n/a');
		expect(duration('not a date', '2026-09-03T09:00:00Z')).toBe('n/a');
	});

	it('reads n/a for a span that runs backwards', () => {
		expect(duration('2026-09-03T09:01:00Z', '2026-09-03T09:00:00Z')).toBe('n/a');
	});
});

describe('relativeAge with a fixed instant', () => {
	it('reads a whole-hour age against the instant passed in', () => {
		const now = new Date('2026-09-03T10:00:00Z').getTime();
		expect(relativeAge('2026-09-03T08:00:00Z', now)).toBe('2h');
	});
});
