import { describe, expect, it } from 'vitest';
import {
	GLOBAL_SCOPE,
	SYNC_TARGETS,
	chosenKinds,
	syncRequest,
	targetEnabled
} from './sync';

const ALL = SYNC_TARGETS.map((t) => t.kind);
const PROJECT = 'e2f0a1b2-0000-4000-8000-000000000001';

describe('targetEnabled', () => {
	it('disables the two managed-block targets at global scope', () => {
		const off = SYNC_TARGETS.filter((t) => !targetEnabled(t, GLOBAL_SCOPE)).map((t) => t.kind);
		expect(off).toEqual(['agents_md', 'claude_md']);
	});

	it('enables every target at a project scope', () => {
		expect(SYNC_TARGETS.every((t) => targetEnabled(t, PROJECT))).toBe(true);
	});
});

describe('chosenKinds', () => {
	it('drops the disabled targets at global scope', () => {
		expect(chosenKinds(ALL, GLOBAL_SCOPE)).toEqual(['claude', 'codex']);
	});

	it('keeps them at a project scope', () => {
		expect(chosenKinds(ALL, PROJECT)).toEqual(ALL);
	});

	it('keeps the target order rather than the order they were ticked in', () => {
		expect(chosenKinds(['codex', 'claude'], PROJECT)).toEqual(['claude', 'codex']);
	});

	it('is empty when nothing is ticked', () => {
		expect(chosenKinds([], PROJECT)).toEqual([]);
	});
});

describe('syncRequest', () => {
	it('sends no root and global true at global scope', () => {
		expect(syncRequest(GLOBAL_SCOPE, '/home/me/atlas', ALL, true)).toEqual({
			root: null,
			global: true,
			targets: ['claude', 'codex'],
			check_only: true
		});
	});

	it('sends the project root and global false at a project scope', () => {
		expect(syncRequest(PROJECT, '/home/me/atlas', ALL, false)).toEqual({
			root: '/home/me/atlas',
			global: false,
			targets: ALL,
			check_only: false
		});
	});
});
