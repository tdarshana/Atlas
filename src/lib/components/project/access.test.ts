// The project Permissions tab's form, as shapes: what a stored rule reads as, what an
// edited form writes back, and what the notes say about set versus inherited.

import { describe, expect, it } from 'vitest';
import { accessForm, accessText, reviewNote, ruleNote, toAccess } from './access';
import type { AgentAccess } from '$lib/types';

const OPEN: AgentAccess = { memory_writers: null, task_movers: null, require_review: false };
const ACTORS = ['claude-code', 'codex'];

describe('accessForm', () => {
	it('reads a project that sets nothing as inheriting both list rules', () => {
		const effective: AgentAccess = {
			memory_writers: ['claude-code'],
			task_movers: null,
			require_review: false
		};
		const form = accessForm(OPEN, effective, ACTORS);

		expect(form.inheritWriters).toBe(true);
		expect(form.inheritMovers).toBe(true);
		// The ticks start from what is in force, so unticking Inherit begins there.
		expect(form.memoryWriters).toEqual({ 'claude-code': true, codex: false });
		expect(form.taskMovers).toEqual({ 'claude-code': true, codex: true });
	});

	it('reads a rule the project does set as its own', () => {
		const access: AgentAccess = {
			memory_writers: ['codex'],
			task_movers: null,
			require_review: true
		};
		const form = accessForm(access, { ...access, task_movers: null }, ACTORS);

		expect(form.inheritWriters).toBe(false);
		expect(form.inheritMovers).toBe(true);
		expect(form.memoryWriters).toEqual({ 'claude-code': false, codex: true });
		expect(form.requireReview).toBe(true);
	});
});

describe('toAccess', () => {
	it('writes an inherited rule as null, which is what makes the daemon fall back', () => {
		const form = accessForm(OPEN, OPEN, ACTORS);
		expect(toAccess(form)).toEqual(OPEN);
	});

	it('writes a set rule out in full, ticks and all', () => {
		const form = accessForm(OPEN, OPEN, ACTORS);
		form.inheritWriters = false;
		form.memoryWriters = { 'claude-code': true, codex: false };
		form.requireReview = true;

		expect(toAccess(form)).toEqual({
			memory_writers: ['claude-code'],
			task_movers: null,
			require_review: true
		});
	});

	it('round trips a project that sets both rules', () => {
		const access: AgentAccess = {
			memory_writers: ['codex'],
			task_movers: ['claude-code'],
			require_review: false
		};
		expect(toAccess(accessForm(access, access, ACTORS))).toEqual(access);
	});

	/** Going back to the global default is `inherit = true`, which writes null; the ticks
	 * it leaves behind must not leak into the saved rule. */
	it('drops the ticks once a rule goes back to the global default', () => {
		const access: AgentAccess = {
			memory_writers: ['codex'],
			task_movers: null,
			require_review: false
		};
		const form = accessForm(access, access, ACTORS);
		form.inheritWriters = true;
		expect(toAccess(form).memory_writers).toBeNull();
	});
});

describe('accessText', () => {
	it('names a null list as any actor and an empty one as none', () => {
		expect(accessText(null)).toBe('Any actor');
		expect(accessText([])).toBe('No actor');
		expect(accessText(['a', 'b'])).toBe('a, b');
	});
});

describe('ruleNote', () => {
	it('says a rule is inherited and names the global default', () => {
		const defaults: AgentAccess = { ...OPEN, memory_writers: ['claude-code'] };
		const note = ruleNote(OPEN, defaults, 'memory_writers');

		expect(note.inherited).toBe(true);
		expect(note.text).toBe('Inherited from the global default: claude-code');
	});

	/**
	 * The note is read against the form as it is edited, so the moment a set rule goes back
	 * on the default it has to name the default rather than the value the project was
	 * setting a click ago.
	 */
	it('names the default, not the project value, once a set rule is cleared', () => {
		const defaults: AgentAccess = { ...OPEN, memory_writers: ['claude-code'] };
		const set: AgentAccess = { ...OPEN, memory_writers: ['codex'] };

		expect(ruleNote(set, defaults, 'memory_writers').text).toBe('Set on this project: codex');
		expect(ruleNote(OPEN, defaults, 'memory_writers').text).toBe(
			'Inherited from the global default: claude-code'
		);
	});

	it('says a rule is set here and names the project value', () => {
		const access: AgentAccess = { ...OPEN, task_movers: ['codex'] };
		const note = ruleNote(access, access, 'task_movers');

		expect(note.inherited).toBe(false);
		expect(note.text).toBe('Set on this project: codex');
	});
});

describe('reviewNote', () => {
	/** The global flag is a floor: a project that leaves it off still requires review when
	 * the default is on, and the note has to say so. */
	it('calls review inherited when the project is off and the default is on', () => {
		const note = reviewNote(OPEN, { ...OPEN, require_review: true });
		expect(note.inherited).toBe(true);
		expect(note.text).toContain('review is required');
	});

	it('calls review set here when the project raises it', () => {
		const note = reviewNote({ ...OPEN, require_review: true }, OPEN);
		expect(note.inherited).toBe(false);
		expect(note.text).toBe('Set on this project: review is required.');
	});

	it('calls review set here when neither the project nor the default asks for it', () => {
		const note = reviewNote(OPEN, OPEN);
		expect(note.inherited).toBe(false);
		expect(note.text).toBe('Set on this project: review is not required.');
	});
});
