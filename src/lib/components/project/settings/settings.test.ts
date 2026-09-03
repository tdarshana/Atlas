import { describe, expect, it } from 'vitest';

import type { AgentAccess } from '$lib/types';
import {
	accessChecked,
	changeBaseUrl,
	DEFAULT_ACTORS,
	extractionForm,
	KEY_PREFIX_RE,
	keyPrefixConfirm,
	knownActors,
	MASKED_KEY,
	toAgentAccess,
	toProjectExtraction
} from './settings';

const OPEN: AgentAccess = { memory_writers: null, task_movers: null, require_review: false };

describe('knownActors', () => {
	it('offers the defaults when the log is empty', () => {
		expect(knownActors([], OPEN)).toEqual([...DEFAULT_ACTORS].sort());
	});

	it('adds the actors seen in the log', () => {
		expect(knownActors(['workflow', 'desktop'], OPEN)).toContain('workflow');
		expect(knownActors(['workflow'], OPEN).filter((a) => a === 'desktop')).toHaveLength(1);
	});

	it('keeps an allow-listed actor that has not written yet', () => {
		const access: AgentAccess = {
			memory_writers: ['codex/fixer'],
			task_movers: ['codex/fixer'],
			require_review: false
		};
		expect(knownActors([], access)).toContain('codex/fixer');
	});
});

describe('agent access model', () => {
	const actors = ['cli/claude-code', 'cli/codex', 'desktop'];

	it('ticks everything when the lists are null', () => {
		expect(accessChecked(actors, OPEN)).toEqual({
			'cli/claude-code': true,
			'cli/codex': true,
			desktop: true
		});
	});

	it('ticks only the listed actors', () => {
		const access: AgentAccess = {
			memory_writers: ['cli/codex'],
			task_movers: ['cli/codex'],
			require_review: true
		};
		expect(accessChecked(actors, access)).toEqual({
			'cli/claude-code': false,
			'cli/codex': true,
			desktop: false
		});
	});

	it('writes back nulls when everything is ticked', () => {
		const checked = { 'cli/claude-code': true, 'cli/codex': true, desktop: true };
		expect(toAgentAccess(actors, checked, false)).toEqual(OPEN);
	});

	it('writes the same list to both rules when some are unticked', () => {
		const checked = { 'cli/claude-code': false, 'cli/codex': true, desktop: true };
		expect(toAgentAccess(actors, checked, true)).toEqual({
			memory_writers: ['cli/codex', 'desktop'],
			task_movers: ['cli/codex', 'desktop'],
			require_review: true
		});
	});

	it('round-trips a list', () => {
		const access: AgentAccess = {
			memory_writers: ['desktop'],
			task_movers: ['desktop'],
			require_review: false
		};
		expect(toAgentAccess(actors, accessChecked(actors, access), false)).toEqual(access);
	});

	it('carries the review rule either way', () => {
		const all = { 'cli/claude-code': true, 'cli/codex': true, desktop: true };
		expect(toAgentAccess(actors, all, true).require_review).toBe(true);
	});
});

describe('extraction form', () => {
	it('reads a missing override as "use global"', () => {
		expect(extractionForm(null)).toEqual({
			useGlobal: true,
			baseUrl: '',
			model: '',
			apiKey: '',
			threshold: '',
			storedKey: false
		});
		expect(toProjectExtraction(extractionForm(null))).toBeNull();
	});

	it('reads a stored override, masked key and all', () => {
		const form = extractionForm({
			enabled: true,
			base_url: 'http://127.0.0.1:54321/v1',
			model: 'project-model',
			api_key: MASKED_KEY,
			auto_accept_min_confidence: 0.9
		});
		expect(form).toEqual({
			useGlobal: false,
			baseUrl: 'http://127.0.0.1:54321/v1',
			model: 'project-model',
			apiKey: '',
			threshold: '0.9',
			storedKey: true
		});
	});

	it('sends the mask back for a key it did not touch', () => {
		const form = extractionForm({ base_url: 'https://a', api_key: MASKED_KEY });
		expect(toProjectExtraction(form)?.api_key).toBe(MASKED_KEY);
	});

	it('sends a key the user typed', () => {
		const form = { ...extractionForm({ base_url: 'https://a' }), apiKey: 'sk-new' };
		expect(toProjectExtraction(form)?.api_key).toBe('sk-new');
	});

	it('leaves a blank field null so it still inherits', () => {
		const form = { ...extractionForm(null), useGlobal: false };
		expect(toProjectExtraction(form)).toEqual({
			enabled: true,
			base_url: null,
			model: null,
			api_key: null,
			auto_accept_min_confidence: null
		});
	});

	it('sends the threshold as a number', () => {
		const form = { ...extractionForm(null), useGlobal: false, threshold: '0.85' };
		expect(toProjectExtraction(form)?.auto_accept_min_confidence).toBe(0.85);
	});

	it('clears the stored key when the base URL moves', () => {
		const form = extractionForm({ base_url: 'https://a', api_key: MASKED_KEY });
		const moved = changeBaseUrl({ ...form, apiKey: 'sk-typed' }, 'https://b');
		expect(moved.baseUrl).toBe('https://b');
		expect(moved.apiKey).toBe('');
		expect(moved.storedKey).toBe(false);
		expect(toProjectExtraction(moved)?.api_key).toBeNull();
	});

	it('keeps the key while the base URL is only being retyped the same', () => {
		const form = extractionForm({ base_url: 'https://a', api_key: MASKED_KEY });
		expect(changeBaseUrl(form, ' https://a ').storedKey).toBe(true);
	});
});

describe('key prefix', () => {
	it('accepts 2 to 6 upper-case characters starting with a letter', () => {
		expect(KEY_PREFIX_RE.test('AT')).toBe(true);
		expect(KEY_PREFIX_RE.test('ATLAS1')).toBe(true);
		expect(KEY_PREFIX_RE.test('A')).toBe(false);
		expect(KEY_PREFIX_RE.test('ATLAS12')).toBe(false);
		expect(KEY_PREFIX_RE.test('1TL')).toBe(false);
		expect(KEY_PREFIX_RE.test('atl')).toBe(false);
	});

	it('names the rename it is about to make', () => {
		expect(keyPrefixConfirm(12, 'ATL', 'ZED')).toBe('Rename 12 tasks from ATL- to ZED-?');
		expect(keyPrefixConfirm(1, 'ATL', 'ZED')).toBe('Rename 1 task from ATL- to ZED-?');
		expect(keyPrefixConfirm(0, 'ATL', 'ZED')).toBe('Rename 0 tasks from ATL- to ZED-?');
	});
});
