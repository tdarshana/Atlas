// @vitest-environment jsdom
// The skills store's writes carry the scope the list was loaded for. A project-scoped
// discovered skill's id is resolved by the daemon against that project's roots, so a
// save that forgets the project id cannot find the file it is meant to rewrite.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Doc, Skill, SkillSummary } from '$lib/types';

const commits: Doc = {
	id: 'doc-1',
	kind: 'practice',
	name: 'commits',
	body: 'Imperative mood.',
	tags: ['git'],
	project_id: null,
	created_at: '2026-09-01T00:00:00Z',
	updated_at: '2026-09-02T00:00:00Z'
};

const updateSkillBody = vi.fn(async (): Promise<Skill> => saved);
const listSkills = vi.fn(async () => ({ skills: [] as SkillSummary[], warnings: [] }));
const listDocs = vi.fn(async (): Promise<Doc[]> => [commits]);
const saveDoc = vi.fn(async (_kind: string, d: { body: string }): Promise<Doc> => ({ ...commits, body: d.body }));
const deleteDoc = vi.fn(async () => undefined);
const setProjectSkills = vi.fn(async () => ({}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ updateSkillBody, listSkills, listDocs, saveDoc, deleteDoc, setProjectSkills }),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { deleteSkill, loadSkills, openSkill, saveBody, setDisabled, skills } from './skills.svelte';

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
	listDocs.mockClear();
	saveDoc.mockClear();
	deleteDoc.mockClear();
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

// Practices ride in the same list (2026-09-07): fetched beside the skills for the same
// scope, opened without a fetch, and written through the practice routes.
describe('practices in the list', () => {
	it('loads the practices for the same scope and folds them in as rows', async () => {
		await loadSkills('p-1');
		expect(listDocs).toHaveBeenCalledWith('practice', 'p-1');
		expect(skills.items.map((r) => [r.id, r.source])).toEqual([['practice:commits', 'practice']]);
		expect(skills.practices).toEqual([commits]);
	});

	it('opens a practice from the loaded docs, with its body and tags', async () => {
		await loadSkills(null);
		await openSkill('practice:commits', null);
		expect(skills.open?.body).toBe('Imperative mood.');
		expect(skills.open?.tags).toEqual(['git']);
		expect(skills.openError).toBeNull();
		await openSkill('practice:nobody', null);
		expect(skills.open).toBeNull();
		expect(skills.openError).toBe('No practice named nobody');
	});

	it('saves a practice body through the practice route, keeping its tags and scope', async () => {
		await loadSkills(null);
		await saveBody('practice:commits', 'Present tense.');
		expect(saveDoc).toHaveBeenCalledWith('practice', { name: 'commits', body: 'Present tense.', tags: ['git'], project_id: null });
		expect(updateSkillBody).not.toHaveBeenCalled();
		expect(skills.open?.body).toBe('Present tense.');
	});

	it('deletes a practice by name and closes it', async () => {
		await loadSkills(null);
		await openSkill('practice:commits', null);
		await deleteSkill('practice:commits');
		expect(deleteDoc).toHaveBeenCalledWith('practice', 'commits');
		expect(skills.open).toBeNull();
	});
});
