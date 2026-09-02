// The two pieces of the settings store that carry real rules: the auto-accept
// default, and the diff that decides what a Save actually sends.

import { beforeEach, describe, expect, it } from 'vitest';
import {
	DEFAULT_MIN_CONFIDENCE,
	MASKED,
	changedSettings,
	draftFromSettings,
	minConfidence,
	settings,
	type SettingsDraft
} from './settings.svelte';

/** The daemon's shape when nothing has been configured: every key is null. */
const UNSET = {
	'daemon.port': null,
	'embedding.model': null,
	'extraction.api_key': null,
	'extraction.auto_accept_min_confidence': null,
	'extraction.base_url': null,
	'extraction.enabled': null,
	'extraction.model': null
};

beforeEach(() => {
	settings.values = { ...UNSET };
});

describe('minConfidence', () => {
	it('defaults to 1.0, which auto-accepts nothing', () => {
		expect(DEFAULT_MIN_CONFIDENCE).toBe(1.0);
		expect(minConfidence()).toBe(1.0);
	});

	it('defaults to 1.0 when the key is missing entirely', () => {
		settings.values = {};
		expect(minConfidence()).toBe(1.0);
	});

	it('returns the stored number', () => {
		settings.values = { ...UNSET, 'extraction.auto_accept_min_confidence': 0.75 };
		expect(minConfidence()).toBe(0.75);
	});
});

describe('changedSettings', () => {
	/** A stored key and a filled-in extraction block. */
	function configured() {
		settings.values = {
			...UNSET,
			'extraction.enabled': true,
			'extraction.base_url': 'https://api.deepseek.com',
			'extraction.api_key': MASKED,
			'extraction.model': 'deepseek-chat',
			'extraction.auto_accept_min_confidence': 0.9
		};
		return draftFromSettings();
	}

	it('sends nothing when the draft matches the server', () => {
		expect(changedSettings(configured())).toEqual({});
	});

	it('never sends the masked API key back when the box was left blank', () => {
		const draft: SettingsDraft = { ...configured(), model: 'deepseek-reasoner' };
		const out = changedSettings(draft);
		expect(out).toEqual({ 'extraction.model': 'deepseek-reasoner' });
		expect(out).not.toHaveProperty('extraction.api_key');
	});

	it('sends the API key the user typed', () => {
		const draft: SettingsDraft = { ...configured(), apiKey: 'sk-new' };
		expect(changedSettings(draft)).toEqual({ 'extraction.api_key': 'sk-new' });
	});

	it('sends the other changed keys and only those', () => {
		const draft: SettingsDraft = {
			...configured(),
			enabled: false,
			baseUrl: 'http://localhost:11434/v1',
			threshold: 0.5
		};
		expect(changedSettings(draft)).toEqual({
			'extraction.enabled': false,
			'extraction.base_url': 'http://localhost:11434/v1',
			'extraction.auto_accept_min_confidence': 0.5
		});
	});
});
