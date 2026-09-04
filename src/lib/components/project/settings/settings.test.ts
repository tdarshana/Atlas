import { describe, expect, it } from 'vitest';

import type { AgentAccess } from '$lib/types';
import {
	alwaysAllowed,
	boardKeyBase,
	DEFAULT_ACTORS,
	extractionForm,
	KEY_PREFIX_RE,
	keyPrefixConfirm,
	keyWillDrop,
	knownActors,
	MASKED_KEY,
	SYSTEM_ACTORS,
	toProjectExtraction
} from './settings';

const OPEN: AgentAccess = { memory_writers: null, task_movers: null, require_review: false };

describe('knownActors', () => {
	it('offers the defaults when the log is empty', () => {
		expect(knownActors([], OPEN)).toEqual([...DEFAULT_ACTORS].sort());
	});

	it('adds the agent labels seen in the log', () => {
		expect(knownActors(['codex/fixer'], OPEN)).toContain('codex/fixer');
		expect(knownActors(['cli/claude-code'], OPEN).filter((a) => a === 'cli/claude-code')).toHaveLength(1);
	});

	it('hides Atlas own system labels, however often they write', () => {
		const actors = knownActors(['api', 'cli', 'sync', 'desktop', 'workflow', 'codex'], OPEN);
		for (const system of SYSTEM_ACTORS) expect(actors).not.toContain(system);
		expect(actors).toContain('codex');
	});

	it('keeps an allow-listed actor that has not written yet', () => {
		const access: AgentAccess = {
			memory_writers: ['codex/fixer'],
			task_movers: ['codex/fixer'],
			require_review: false
		};
		expect(knownActors([], access)).toContain('codex/fixer');
	});

	it('reads both rules, not only the memory one', () => {
		const access: AgentAccess = {
			memory_writers: null,
			task_movers: ['codex/fixer'],
			require_review: false
		};
		expect(knownActors([], access)).toContain('codex/fixer');
	});
});

describe('extraction form', () => {
	it('reads a missing override as "use global"', () => {
		expect(extractionForm(null)).toEqual({
			useGlobal: true,
			enabled: null,
			baseUrl: '',
			loadedBaseUrl: '',
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
			enabled: true,
			baseUrl: 'http://127.0.0.1:54321/v1',
			loadedBaseUrl: 'http://127.0.0.1:54321/v1',
			model: 'project-model',
			apiKey: '',
			threshold: '0.9',
			storedKey: true
		});
	});

	it('leaves `enabled` inherited when the override says nothing about it', () => {
		const form = { ...extractionForm({ base_url: 'https://a' }), useGlobal: false };
		expect(form.enabled).toBeNull();
		expect(toProjectExtraction(form)?.enabled).toBeNull();
	});

	it('carries a stored `enabled: false` rather than turning extraction on', () => {
		const form = extractionForm({ enabled: false, model: 'm' });
		expect(form.enabled).toBe(false);
		expect(toProjectExtraction(form)?.enabled).toBe(false);
	});

	it('sends the switch the user set', () => {
		const form = { ...extractionForm(null), useGlobal: false, enabled: true };
		expect(toProjectExtraction(form)?.enabled).toBe(true);
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
			enabled: null,
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

	it('drops the stored key when the base URL is saved away from the loaded one', () => {
		const form = extractionForm({ base_url: 'https://a', api_key: MASKED_KEY });
		const moved = { ...form, baseUrl: 'https://b' };
		expect(keyWillDrop(moved)).toBe(true);
		expect(toProjectExtraction(moved)?.api_key).toBeNull();
	});

	it('keeps the key when an edit to the base URL is undone', () => {
		const form = extractionForm({ base_url: 'https://a', api_key: MASKED_KEY });
		const typed = { ...form, baseUrl: 'https://ab' };
		const undone = { ...typed, baseUrl: ' https://a ' };
		expect(keyWillDrop(undone)).toBe(false);
		expect(toProjectExtraction(undone)?.api_key).toBe(MASKED_KEY);
	});

	it('does not warn about a dropped key once a new one is typed', () => {
		const form = extractionForm({ base_url: 'https://a', api_key: MASKED_KEY });
		const moved = { ...form, baseUrl: 'https://b', apiKey: 'sk-new' };
		expect(keyWillDrop(moved)).toBe(false);
		expect(toProjectExtraction(moved)?.api_key).toBe('sk-new');
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

describe('alwaysAllowed', () => {
	it('names the actors the daemon exempts, and only those', () => {
		for (const actor of ['desktop', 'api', 'cli', 'cli/ann', 'cli/claude-code']) {
			expect(alwaysAllowed(actor)).toBe(true);
		}
		for (const actor of ['cline', 'codex', 'claude-code/reviewer', 'workflow']) {
			expect(alwaysAllowed(actor)).toBe(false);
		}
	});
});

describe('boardKeyBase', () => {
	it('mirrors the daemon: three alphanumerics, uppercased and padded', () => {
		expect(boardKeyBase('atlas')).toBe('ATL');
		expect(boardKeyBase('my-app')).toBe('MYA');
		expect(boardKeyBase('go')).toBe('GOX');
		expect(boardKeyBase('a1-b2')).toBe('A1B');
	});

	it('falls back to the global board key when the name has nothing to take', () => {
		expect(boardKeyBase('---')).toBe('ATLAS');
	});
});
