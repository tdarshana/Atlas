import { describe, expect, it } from 'vitest';
import { KIND_META, KIND_OPTIONS, KINDS, kindMeta, SUBTASK_META } from './kind';
import type { TaskKind } from '$lib/types';

describe('task kind marks', () => {
	it('covers every kind the daemon knows, each with a glyph and a colour', () => {
		const wire: TaskKind[] = ['task', 'bug', 'feature', 'chore', 'epic', 'feature_request'];
		for (const k of wire) {
			expect(KIND_META[k].icon).toBeTruthy();
			expect(KIND_META[k].color).toMatch(/^#[0-9A-F]{6}$/i);
		}
		expect(KINDS).toHaveLength(wire.length);
	});

	it('maps the tracker vocabulary: bug, bookmark, up arrow, zap, lightbulb, corner arrow', () => {
		expect(KIND_META.bug.icon).toBe('bug');
		expect(KIND_META.task.icon).toBe('bookmark');
		expect(KIND_META.feature.icon).toBe('square-arrow-up');
		expect(KIND_META.epic.icon).toBe('zap');
		expect(KIND_META.feature_request.icon).toBe('lightbulb');
		expect(SUBTASK_META.icon).toBe('corner-left-up');
		expect(KIND_OPTIONS.find((o) => o.value === 'feature_request')?.label).toBe('Feature request');
	});

	it('falls back to task for an unknown value', () => {
		expect(kindMeta('nope')).toBe(KIND_META.task);
	});
});

describe('stage lozenge colours', () => {
	it('colours the four default stages and greys an unknown one', async () => {
		const { stageColor } = await import('./kind');
		expect(stageColor('Backlog')).toBe('#8993A4');
		expect(stageColor('In Progress')).toBe('#4BADE8');
		expect(stageColor('Testing')).toBe('#904EE2');
		expect(stageColor('Done')).toBe('#65BA43');
		expect(stageColor('Review')).toBe('#8993A4');
	});
});
