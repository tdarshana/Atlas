// @vitest-environment jsdom
// What the Skills table and the detail panel actually do: the table names every source
// and hands its `Enabled here` checkbox back with the right flag, and the detail's Edit
// then Save sends the textarea's body to the API.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import type { Skill, SkillSource, SkillSummary } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import SkillDetail from './SkillDetail.svelte';
import SkillTable from './SkillTable.svelte';

afterEach(cleanup);

function summary(
	name: string,
	source: SkillSource,
	extra: Partial<SkillSummary> = {}
): SkillSummary {
	return {
		id: `${source}:${name}`,
		source,
		name,
		description: `What ${name} does`,
		scope: 'global',
		project_id: null,
		path: `/root/${name}`,
		plugin: null,
		editable: true,
		updated_at: '2026-09-04T10:00:00Z',
		enabled_here: null,
		...extra
	};
}

function detail(extra: Partial<Skill> = {}): Skill {
	return {
		...summary('release', 'claude-user'),
		body: '# Release\n\nSteps.',
		files: ['reference.md'],
		...extra
	};
}

describe('SkillTable', () => {
	it('names the tool each row came from', () => {
		render(SkillTable, {
			props: {
				id: 'skills',
				rows: [
					summary('a', 'native'),
					summary('b', 'claude-project'),
					summary('c', 'codex-user'),
					summary('d', 'plugin', { plugin: 'anthropics/example-skills' })
				],
				onopen: () => {}
			}
		});

		expect(screen.getByTestId('skill-source-native:a').textContent).toContain('Native');
		expect(screen.getByTestId('skill-source-claude-project:b').textContent).toContain(
			'Claude Code'
		);
		expect(screen.getByTestId('skill-source-codex-user:c').textContent).toContain('Codex');
		const plugin = screen.getByTestId('skill-source-plugin:d');
		expect(plugin.textContent).toContain('Plugin');
		expect(plugin.getAttribute('title')).toBe('anthropics/example-skills');
	});

	it('draws no Enabled here column unless a toggle is offered', () => {
		render(SkillTable, {
			props: { id: 'skills', rows: [summary('a', 'native')], onopen: () => {} }
		});
		expect(screen.queryByTestId('skill-toggle-native:a')).toBeNull();
	});

	it('reports the new state when Enabled here is unticked', async () => {
		const ontoggle = vi.fn();
		render(SkillTable, {
			props: {
				id: 'project-skills',
				rows: [summary('a', 'native', { enabled_here: true })],
				showScope: true,
				onopen: () => {},
				ontoggle
			}
		});

		const box = screen.getByTestId('skill-toggle-native:a') as HTMLInputElement;
		expect(box.checked).toBe(true);
		await fireEvent.click(box);
		expect(ontoggle).toHaveBeenCalledTimes(1);
		expect(ontoggle.mock.calls[0][0].id).toBe('native:a');
		expect(ontoggle.mock.calls[0][1]).toBe(false);
	});

	it('shows a disabled row as ticked off', () => {
		render(SkillTable, {
			props: {
				id: 'project-skills',
				rows: [summary('a', 'native', { enabled_here: false })],
				onopen: () => {},
				ontoggle: () => {}
			}
		});
		expect((screen.getByTestId('skill-toggle-native:a') as HTMLInputElement).checked).toBe(false);
	});

	it('opens the row that was clicked', async () => {
		const onopen = vi.fn();
		render(SkillTable, {
			props: { id: 'skills', rows: [summary('a', 'native')], onopen }
		});
		await fireEvent.click(screen.getByText('a'));
		expect(onopen).toHaveBeenCalledTimes(1);
		expect(onopen.mock.calls[0][0].id).toBe('native:a');
	});

	it('opens from anywhere on the row, not just the name', async () => {
		const onopen = vi.fn();
		render(SkillTable, {
			props: { id: 'skills', rows: [summary('a', 'native')], onopen }
		});

		// The description cell is the far side of the row from the name.
		await fireEvent.click(screen.getByTestId('skill-description-native:a'));
		expect(onopen).toHaveBeenCalledTimes(1);

		// And the row itself is focusable, so Enter reaches it without a mouse.
		const row = screen.getByTestId('skill-description-native:a').closest('[role="row"]')!;
		expect(row.getAttribute('tabindex')).toBe('0');
		await fireEvent.keyDown(row, { key: 'Enter' });
		expect(onopen).toHaveBeenCalledTimes(2);
	});

	it('clamps the description to two lines and keeps the full text on the title', () => {
		const long =
			'A very long description that would otherwise wrap to several lines and paint ' +
			'over the rows below it, which is exactly what the clamp is here to stop.';
		render(SkillTable, {
			props: {
				id: 'skills',
				rows: [summary('a', 'native', { description: long })],
				onopen: () => {}
			}
		});

		const cell = screen.getByTestId('skill-description-native:a');
		// Svelte appends its scope class, so match the class rather than the whole string.
		expect(cell.classList.contains('description')).toBe(true);
		expect(cell.getAttribute('title')).toBe(long);
	});
});

describe('SkillDetail', () => {
	const base = {
		loading: false,
		error: null,
		width: 380,
		onclose: () => {},
		onresize: () => {}
	};

	it('sends the edited body to save', async () => {
		const onsave = vi.fn(async () => {});
		render(SkillDetail, { props: { ...base, skill: detail(), onsave } });

		await fireEvent.click(screen.getByTestId('skill-edit'));
		const box = screen.getByTestId('skill-body-editor') as HTMLTextAreaElement;
		expect(box.value).toBe('# Release\n\nSteps.');

		await fireEvent.input(box, { target: { value: '# Release\n\nMore steps.' } });
		await fireEvent.click(screen.getByTestId('skill-save'));

		expect(onsave).toHaveBeenCalledWith('claude-user:release', '# Release\n\nMore steps.');
	});

	it('offers no Edit on a read-only skill', () => {
		render(SkillDetail, {
			props: { ...base, skill: detail({ editable: false }), onsave: async () => {} }
		});
		expect(screen.queryByTestId('skill-edit')).toBeNull();
	});

	it('offers Delete only for a native skill, and only after confirming', async () => {
		const ondelete = vi.fn(async () => {});
		const { unmount } = render(SkillDetail, {
			props: { ...base, skill: detail(), onsave: async () => {}, ondelete }
		});
		expect(screen.queryByTestId('skill-delete')).toBeNull();
		unmount();

		render(SkillDetail, {
			props: {
				...base,
				skill: detail({ id: 'uuid-1', source: 'native', path: null }),
				onsave: async () => {},
				ondelete
			}
		});
		await fireEvent.click(screen.getByTestId('skill-delete'));
		await fireEvent.click(screen.getByTestId('skill-delete-confirm'));
		expect(ondelete).toHaveBeenCalledWith('uuid-1');
	});

	it('renders the body without its frontmatter, and keeps the file whole for Source', async () => {
		const body = '---\nname: deployer\ndescription: Ships it\n---\n\n# Deployer\n\nSteps.';
		render(SkillDetail, {
			props: { ...base, skill: detail({ body }), onsave: async () => {} }
		});

		await fireEvent.click(screen.getByTestId('markdown-mode-preview'));
		expect(screen.getByTestId('skill-detail').textContent).not.toContain('name: deployer');

		await fireEvent.click(screen.getByTestId('markdown-mode-source'));
		expect(screen.getByTestId('skill-detail').textContent).toContain('name: deployer');
	});

	it('lists the other files in the folder', () => {
		render(SkillDetail, { props: { ...base, skill: detail(), onsave: async () => {} } });
		expect(screen.getByTestId('skill-files').textContent).toContain('reference.md');
	});
});
