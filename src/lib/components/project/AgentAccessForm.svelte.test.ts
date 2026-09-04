// @vitest-environment jsdom
// What the shared agent access form draws: a set rule against an inherited one, and the
// action that puts a set rule back on the global default.

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import type { AgentAccess, ProjectAccessReport } from '$lib/types';
import AgentAccessForm from './AgentAccessForm.svelte';
import { accessForm } from './access';

afterEach(cleanup);

const OPEN: AgentAccess = { memory_writers: null, task_movers: null, require_review: false };
const ACTORS = ['claude-code', 'codex'];

function report(access: AgentAccess, defaults: AgentAccess): ProjectAccessReport {
	return {
		access,
		defaults,
		effective: {
			memory_writers: access.memory_writers ?? defaults.memory_writers,
			task_movers: access.task_movers ?? defaults.task_movers,
			require_review: access.require_review || defaults.require_review
		}
	};
}

function mount(access: AgentAccess, defaults: AgentAccess) {
	const r = report(access, defaults);
	const form = $state(accessForm(r.access, r.effective, ACTORS));
	const rendered = render(AgentAccessForm, { props: { form, report: r } });
	return { ...rendered, form };
}

describe('AgentAccessForm', () => {
	it('marks a rule the project does not set as inherited, and names the default', () => {
		const { getByTestId } = mount(OPEN, {
			memory_writers: ['claude-code'],
			task_movers: null,
			require_review: false
		});

		expect(getByTestId('note-memory_writers').textContent).toContain(
			'Inherited from the global default: claude-code'
		);
		expect(getByTestId('rule-memory_writers').textContent).toContain('Inherited');
		// Nothing to put back on the default, so no action is offered.
		expect(() => getByTestId('use-default-memory_writers')).toThrow();
	});

	it('marks a rule the project sets as its own and names the value', () => {
		const access: AgentAccess = { ...OPEN, task_movers: ['codex'] };
		const { getByTestId } = mount(access, OPEN);

		expect(getByTestId('note-task_movers').textContent).toContain('Set on this project: codex');
		expect(getByTestId('rule-task_movers').textContent).toContain('Set here');
		expect(getByTestId('use-default-task_movers')).toBeTruthy();
	});

	it('puts a set rule back on the global default when the action is used', async () => {
		const access: AgentAccess = { ...OPEN, memory_writers: ['codex'] };
		const { getByTestId, form } = mount(access, OPEN);

		expect(form.inheritWriters).toBe(false);
		getByTestId('use-default-memory_writers').click();
		await Promise.resolve();
		expect(form.inheritWriters).toBe(true);
	});

	it('says review is inherited when the global default requires it', () => {
		const { getByTestId } = mount(OPEN, { ...OPEN, require_review: true });
		expect(getByTestId('note-require_review').textContent).toContain(
			'Inherited from the global default'
		);
	});

	it('draws the ticks for a rule the project sets, and none for an inherited one', () => {
		const access: AgentAccess = { ...OPEN, memory_writers: ['codex'] };
		const { getByTestId } = mount(access, OPEN);

		const set = getByTestId('rule-memory_writers');
		expect(set.textContent).toContain('claude-code');
		expect(set.textContent).toContain('codex');

		const inherited = getByTestId('rule-task_movers');
		expect(inherited.textContent).toContain('Set this rule on the project instead');
	});
});
