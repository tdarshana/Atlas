import { describe, expect, it } from 'vitest';
import type { Agent } from '$lib/types';
import { projectAgents, withProjectTag } from './agents';

function agent(name: string, tags: string[]): Agent {
	return {
		id: name,
		name,
		description: '',
		instructions: '',
		model_hint: null,
		tools: [],
		tags,
		version: 1,
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z'
	};
}

describe('projectAgents', () => {
	it('keeps only agents tagged with the project name', () => {
		const all = [agent('a', ['atlas']), agent('b', ['other']), agent('c', ['atlas', 'review'])];
		expect(projectAgents(all, 'atlas').map((a) => a.name)).toEqual(['a', 'c']);
	});
});

describe('withProjectTag', () => {
	it('adds the tag when missing', () => {
		expect(withProjectTag(['review'], 'atlas')).toEqual(['review', 'atlas']);
	});

	it('does not duplicate an existing tag', () => {
		expect(withProjectTag(['atlas', 'review'], 'atlas')).toEqual(['atlas', 'review']);
	});
});
