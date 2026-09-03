import { describe, expect, it } from 'vitest';
import { duration, relativeAge } from './format';

describe('duration', () => {
	it('reads n/a while the run has not finished', () => {
		expect(duration('2026-09-03T09:00:00Z', null)).toBe('n/a');
	});

	it('formats a span under a minute as 0m Ns', () => {
		expect(duration('2026-09-03T09:00:00Z', '2026-09-03T09:00:12Z')).toBe('0m 12s');
	});

	it('formats a span over a minute as Mm Ns', () => {
		expect(duration('2026-09-03T09:00:00Z', '2026-09-03T09:01:42Z')).toBe('1m 42s');
	});

	it('reads n/a for a span that does not parse or runs backwards', () => {
		expect(duration('not a date', '2026-09-03T09:00:00Z')).toBe('n/a');
		expect(duration('2026-09-03T09:01:00Z', '2026-09-03T09:00:00Z')).toBe('n/a');
	});
});

describe('relativeAge with a fixed instant', () => {
	it('reads a whole-hour age against the instant passed in', () => {
		const now = new Date('2026-09-03T10:00:00Z').getTime();
		expect(relativeAge('2026-09-03T08:00:00Z', now)).toBe('2h');
	});
});
