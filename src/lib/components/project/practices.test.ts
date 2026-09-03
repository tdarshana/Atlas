import { describe, expect, it } from 'vitest';
import type { Doc } from '$lib/types';
import { splitPractices } from './practices';

function doc(name: string, projectId: string | null): Doc {
	return {
		id: name,
		kind: 'practice',
		name,
		body: '',
		tags: [],
		project_id: projectId,
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z'
	};
}

describe('splitPractices', () => {
	const PROJECT = 'proj-1';

	it('puts this project\'s own docs in `project`', () => {
		const docs = [doc('a', PROJECT), doc('b', PROJECT)];
		const { project, inherited } = splitPractices(docs, PROJECT);
		expect(project.map((d) => d.name)).toEqual(['a', 'b']);
		expect(inherited).toEqual([]);
	});

	it('puts global docs (null project_id) in `inherited`', () => {
		const docs = [doc('commit-hygiene', null), doc('no-secrets', null)];
		const { project, inherited } = splitPractices(docs, PROJECT);
		expect(project).toEqual([]);
		expect(inherited.map((d) => d.name)).toEqual(['commit-hygiene', 'no-secrets']);
	});

	it('drops a doc scoped to a different project', () => {
		const docs = [doc('mine', PROJECT), doc('theirs', 'proj-2'), doc('global', null)];
		const { project, inherited } = splitPractices(docs, PROJECT);
		expect(project.map((d) => d.name)).toEqual(['mine']);
		expect(inherited.map((d) => d.name)).toEqual(['global']);
	});
});
