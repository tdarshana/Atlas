// @vitest-environment jsdom
// The skills store's writes carry the scope the list was loaded for. A project-scoped
// discovered skill's id is resolved by the daemon against that project's roots, so a
// save that forgets the project id cannot find the file it is meant to rewrite.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Skill, SkillSummary } from '$lib/types';

const updateSkillBody = vi.fn(async (): Promise<Skill> => saved);
const listSkills = vi.fn(async () => ({ skills: [] as SkillSummary[], warnings: [] }));
const setProjectSkills = vi.fn(async () => ({}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ updateSkillBody, listSkills, setProjectSkills }),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { loadSkills, saveBody, setDisabled, skills } from './skills.svelte';

const saved: Skill = {
	id: 'claude-project:deployer',
	source: 'claude-project',
	name: 'deployer',
	description: 'Ships it',
	scope: 'project',
	project_id: 'p-1',
	path: '/repo/.claude/skills/deployer',
	plugin: null,
	editable: true,
	updated_at: '2026-09-04T10:00:00Z',
	enabled_here: true,
	body: '# Deployer',
	files: []
};

beforeEach(() => {
	updateSkillBody.mockClear();
	listSkills.mockClear();
	setProjectSkills.mockClear();
});

describe('saveBody', () => {
	it('sends the project id the list was loaded for', async () => {
		await loadSkills('p-1');
		expect(skills.projectId).toBe('p-1');

		await saveBody('claude-project:deployer', '# Deployer\n\nMore.');

		expect(updateSkillBody).toHaveBeenCalledWith(
			'claude-project:deployer',
			'# Deployer\n\nMore.',
			'p-1'
		);
	});

	it('sends null from the global list, which is what a native skill needs', async () => {
		await loadSkills(null);
		await saveBody('uuid-1', '# Native');
		expect(updateSkillBody).toHaveBeenCalledWith('uuid-1', '# Native', null);
	});

	it('takes the saved skill from the daemon and reloads the list in the same scope', async () => {
		await loadSkills('p-1');
		listSkills.mockClear();
		await saveBody('claude-project:deployer', '# Deployer');
		expect(skills.open).toEqual(saved);
		expect(listSkills).toHaveBeenCalledWith('p-1');
	});
});

describe('setDisabled', () => {
	it('writes the list and reloads that project', async () => {
		await setDisabled('p-1', ['claude-project:deployer']);
		expect(setProjectSkills).toHaveBeenCalledWith('p-1', ['claude-project:deployer']);
		expect(listSkills).toHaveBeenLastCalledWith('p-1');
	});
});
