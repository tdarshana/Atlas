// @vitest-environment jsdom
// The step row's log is collapsed by default and its output pane collapsed again
// behind "Show output" inside that; this pins the two independent expansion states.

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import type { WorkflowStep } from '$lib/types';

import StepLog from './StepLog.svelte';

afterEach(cleanup);

function sampleStep(overrides: Partial<WorkflowStep> = {}): WorkflowStep {
	return {
		id: 'step-1',
		run_id: 'run-1',
		position: 0,
		action_id: 'a',
		name: 'Summarise decisions',
		agent: 'cli/claude-code',
		status: 'success',
		started_at: '2026-09-03T09:00:00Z',
		finished_at: '2026-09-03T09:00:58Z',
		output: 'the reply text',
		log: [{ ts: '2026-09-03T09:00:00Z', level: 'INFO', text: 'request sent' }],
		...overrides
	};
}

describe('StepLog expansion state', () => {
	it('starts collapsed: no log lines and no output shown', () => {
		const { queryByTestId } = render(StepLog, { step: sampleStep() });
		expect(queryByTestId('step-log')).toBeNull();
		expect(queryByTestId('step-output')).toBeNull();
	});

	it('toggling the row shows the log lines, and toggling again hides them', async () => {
		const { getByTestId, queryByTestId } = render(StepLog, { step: sampleStep() });
		await fireEvent.click(getByTestId('step-toggle'));
		expect(getByTestId('step-log').textContent).toContain('request sent');

		await fireEvent.click(getByTestId('step-toggle'));
		expect(queryByTestId('step-log')).toBeNull();
	});

	it('output stays collapsed under "Show output" until clicked, independent of the log', async () => {
		const { getByTestId, queryByTestId } = render(StepLog, { step: sampleStep() });
		await fireEvent.click(getByTestId('step-toggle'));
		expect(queryByTestId('step-output')).toBeNull();
		expect(getByTestId('step-output-toggle').textContent).toBe('Show output');

		await fireEvent.click(getByTestId('step-output-toggle'));
		expect(getByTestId('step-output').textContent).toBe('the reply text');
		expect(getByTestId('step-output-toggle').textContent).toBe('Hide output');
	});

	it('offers no output toggle for a step with no output', async () => {
		const { getByTestId, queryByTestId } = render(StepLog, { step: sampleStep({ output: null }) });
		await fireEvent.click(getByTestId('step-toggle'));
		expect(queryByTestId('step-output-toggle')).toBeNull();
	});
});
