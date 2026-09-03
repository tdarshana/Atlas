import { describe, expect, it } from 'vitest';
import {
	memoryStatusesFor,
	passesKindFilter,
	toggleMemoryKind,
	type MemoryStateFilter
} from './memories';

describe('toggleMemoryKind', () => {
	it('adds a kind that is not selected', () => {
		expect(toggleMemoryKind([], 'fact')).toEqual(['fact']);
		expect(toggleMemoryKind(['fact'], 'decision')).toEqual(['fact', 'decision']);
	});

	it('removes a kind that is already selected', () => {
		expect(toggleMemoryKind(['fact', 'decision'], 'fact')).toEqual(['decision']);
	});
});

describe('memoryStatusesFor', () => {
	it('sends one status for Accepted and Pending', () => {
		expect(memoryStatusesFor('active')).toEqual(['active']);
		expect(memoryStatusesFor('pending')).toEqual(['pending']);
	});

	it('merges active and pending for All, and leaves out rejected and superseded', () => {
		const statuses = memoryStatusesFor('all' as MemoryStateFilter);
		expect(statuses).toEqual(['active', 'pending']);
	});
});

describe('passesKindFilter', () => {
	it('passes everything when no chip is on', () => {
		expect(passesKindFilter('fact', [])).toBe(true);
		expect(passesKindFilter('todo', [])).toBe(true);
	});

	it('passes only the selected kinds otherwise', () => {
		expect(passesKindFilter('fact', ['fact', 'insight'])).toBe(true);
		expect(passesKindFilter('decision', ['fact', 'insight'])).toBe(false);
	});
});
