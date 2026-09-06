import { describe, expect, it } from 'vitest';
import { indexForY, isSameSpot, positionBetween, targetPosition } from './dnd.svelte';
import type { Task } from '$lib/types';

function t(key: string, stage: string, position: number): Task {
	return { key, stage, position, id: key, seq: position } as unknown as Task;
}

describe('positionBetween', () => {
	it('lands between neighbours, one past an end, and at 1 in an empty column', () => {
		expect(positionBetween(2, 4)).toBe(3);
		expect(positionBetween(null, 4)).toBe(3);
		expect(positionBetween(2, null)).toBe(3);
		expect(positionBetween(null, null)).toBe(1);
	});
});

describe('targetPosition', () => {
	const col = [t('A', 'Backlog', 1), t('B', 'Backlog', 2), t('C', 'Backlog', 3)];
	it('ignores the moving card when choosing neighbours', () => {
		expect(targetPosition(col, 0, 'C')).toBe(0);
		expect(targetPosition(col, 1, 'C')).toBe(1.5);
		expect(targetPosition(col, 2, 'C')).toBe(3);
		expect(targetPosition(col, 1, 'X')).toBe(1.5);
		expect(targetPosition([], 0, 'X')).toBe(1);
	});
});

describe('isSameSpot', () => {
	const col = [t('A', 'Backlog', 1), t('B', 'Backlog', 2)];
	it('is true only for the card\'s own slot in its own column', () => {
		expect(isSameSpot(col, 'Backlog', 1, 'B')).toBe(true);
		expect(isSameSpot(col, 'Backlog', 0, 'B')).toBe(false);
		expect(isSameSpot(col, 'Testing', 1, 'B')).toBe(false);
	});
});

describe('indexForY', () => {
	it('inserts before the first card whose middle is below the pointer', () => {
		expect(indexForY([50, 150, 250], 10)).toBe(0);
		expect(indexForY([50, 150, 250], 100)).toBe(1);
		expect(indexForY([50, 150, 250], 300)).toBe(3);
		expect(indexForY([], 40)).toBe(0);
	});
});
