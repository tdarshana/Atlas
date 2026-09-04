import { describe, expect, it } from 'vitest';
import {
	clampDetail,
	enabledSummary,
	filterSkills,
	nextDisabled,
	skillCounts,
	sourceGroup,
	sourceLabel
} from './skills';
import type { SkillSource, SkillSummary } from './types';

function skill(
	name: string,
	source: SkillSource,
	extra: Partial<SkillSummary> = {}
): SkillSummary {
	return {
		id: `${source}:${name}`,
		source,
		name,
		description: `What ${name} does`,
		scope: 'global',
		project_id: null,
		path: `/root/${name}`,
		plugin: null,
		editable: true,
		updated_at: '2026-09-04T10:00:00Z',
		...extra
	};
}

const ITEMS: SkillSummary[] = [
	skill('brainstorming', 'native'),
	skill('commit-helper', 'claude-user'),
	skill('deploy', 'claude-project'),
	skill('review-pr', 'codex-user'),
	skill('gif-maker', 'plugin', { plugin: 'anthropics/example-skills' }),
	skill('poster', 'plugin', { plugin: 'anthropics/example-skills' }),
	skill('rust-rules', 'plugin', { plugin: 'sm/rust' })
];

describe('sourceLabel', () => {
	it('names the tool, not the folder', () => {
		expect(sourceLabel('native')).toBe('Native');
		expect(sourceLabel('claude-project')).toBe('Claude Code');
		expect(sourceLabel('claude-user')).toBe('Claude Code');
		expect(sourceLabel('codex-project')).toBe('Codex');
		expect(sourceLabel('codex-user')).toBe('Codex');
		expect(sourceLabel('plugin')).toBe('Plugin');
	});
});

describe('sourceGroup', () => {
	it('folds both roots of one tool into a single filter value', () => {
		expect(sourceGroup('claude-project')).toBe('claude');
		expect(sourceGroup('claude-user')).toBe('claude');
		expect(sourceGroup('codex-project')).toBe('codex');
		expect(sourceGroup('native')).toBe('native');
		expect(sourceGroup('plugin')).toBe('plugin');
	});
});

describe('filterSkills', () => {
	it('matches name and description, case insensitively', () => {
		expect(filterSkills(ITEMS, 'DEPLOY').map((s) => s.name)).toEqual(['deploy']);
		expect(filterSkills(ITEMS, 'poster does what').map((s) => s.name)).toEqual([]);
		expect(filterSkills(ITEMS, 'What poster does').map((s) => s.name)).toEqual(['poster']);
	});

	it('keeps everything for a blank search', () => {
		expect(filterSkills(ITEMS, '   ')).toHaveLength(ITEMS.length);
	});

	it('narrows to one source group', () => {
		expect(filterSkills(ITEMS, '', 'claude').map((s) => s.name)).toEqual([
			'commit-helper',
			'deploy'
		]);
		expect(filterSkills(ITEMS, '', 'plugin')).toHaveLength(3);
		expect(filterSkills(ITEMS, '', 'native').map((s) => s.name)).toEqual(['brainstorming']);
	});

	it('applies both filters at once', () => {
		expect(filterSkills(ITEMS, 'er', 'plugin').map((s) => s.name)).toEqual([
			'gif-maker',
			'poster'
		]);
	});
});

describe('nextDisabled', () => {
	it('adds an id when the skill is turned off', () => {
		expect(nextDisabled([], 'a', false)).toEqual(['a']);
		expect(nextDisabled(['b'], 'a', false)).toEqual(['b', 'a']);
	});

	it('removes an id when the skill is turned on', () => {
		expect(nextDisabled(['a', 'b'], 'a', true)).toEqual(['b']);
	});

	it('leaves the list alone when nothing changes', () => {
		expect(nextDisabled(['a'], 'a', false)).toEqual(['a']);
		expect(nextDisabled(['b'], 'a', true)).toEqual(['b']);
	});
});

describe('skillCounts', () => {
	it('counts every source group, including the empty ones', () => {
		const counts = skillCounts(ITEMS);
		expect(counts.total).toBe(7);
		expect(counts.bySource).toEqual([
			{ source: 'native', label: 'Native', count: 1 },
			{ source: 'claude', label: 'Claude Code', count: 2 },
			{ source: 'codex', label: 'Codex', count: 1 },
			{ source: 'plugin', label: 'Plugins', count: 3 }
		]);
	});

	it('counts each plugin once, in name order', () => {
		expect(skillCounts(ITEMS).byPlugin).toEqual([
			{ plugin: 'anthropics/example-skills', count: 2 },
			{ plugin: 'sm/rust', count: 1 }
		]);
	});

	it('is all zeroes for an empty list', () => {
		const counts = skillCounts([]);
		expect(counts.total).toBe(0);
		expect(counts.byPlugin).toEqual([]);
		expect(counts.bySource.every((s) => s.count === 0)).toBe(true);
	});
});

describe('enabledSummary', () => {
	it('counts the rows the project has not disabled', () => {
		const rows = [
			skill('a', 'native', { enabled_here: true }),
			skill('b', 'native', { enabled_here: false }),
			skill('c', 'native', { enabled_here: true })
		];
		expect(enabledSummary(rows)).toBe('2 of 3 skills enabled for this project');
	});

	it('reads a row with no flag as enabled, and says "skill" for one', () => {
		expect(enabledSummary([skill('a', 'native')])).toBe(
			'1 of 1 skill enabled for this project'
		);
	});
});

describe('clampDetail', () => {
	it('holds the panel inside its range', () => {
		expect(clampDetail(100)).toBe(300);
		expect(clampDetail(9000)).toBe(640);
		expect(clampDetail(420.4)).toBe(420);
		expect(clampDetail(Number.NaN)).toBe(380);
	});
});
