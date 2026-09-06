// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';

import Badge from './Badge.svelte';
import Button from './Button.svelte';
import Checkbox from './Checkbox.svelte';
import Icon from './Icon.svelte';
import IconButton from './IconButton.svelte';
import Input from './Input.svelte';
import KeyHint from './KeyHint.svelte';
import Select from './Select.svelte';
import Typeahead from './Typeahead.svelte';
import Skeleton from './Skeleton.svelte';
import Tooltip from './Tooltip.svelte';

afterEach(cleanup);

/** A children snippet holding literal text, so components can be rendered from plain TS. */
function text(value: string) {
	return createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));
}

describe('Button', () => {
	it('composes the variant and size classes and renders its children', () => {
		const { container } = render(Button, {
			props: { variant: 'danger', size: 'sm', children: text('Drop table…') }
		});

		const button = container.querySelector('button')!;
		expect(button.className).toBe('dbm-btn dbm-btn--danger dbm-btn--sm');
		expect(button.textContent).toContain('Drop table…');
	});
});

describe('Input', () => {
	it('wraps a labelled field and points the label at the input', () => {
		const { container } = render(Input, {
			props: { label: 'Daemon port', hint: 'Loopback only' }
		});

		const field = container.querySelector('.dbm-field')!;
		expect(field).toBeTruthy();

		const label = field.querySelector('label.dbm-field__label')!;
		const input = field.querySelector('input.dbm-input')!;
		expect(label.getAttribute('for')).toBe(input.getAttribute('id'));
		expect(input.getAttribute('id')).toBeTruthy();

		const hint = field.querySelector('.dbm-field__hint')!;
		expect(hint.textContent).toBe('Loopback only');
		expect(hint.className).not.toContain('dbm-field__hint--error');
	});

	it('keeps the caller oninput working alongside the binding', () => {
		const seen: string[] = [];
		const { container } = render(Input, {
			props: { oninput: (e: Event) => seen.push((e.currentTarget as HTMLInputElement).value) }
		});

		const input = container.querySelector('input')!;
		input.value = 'analytics-prod';
		input.dispatchEvent(new Event('input', { bubbles: true }));

		expect(seen).toEqual(['analytics-prod']);
	});
});

describe('Select', () => {
	it('shows the first option when nothing is bound', () => {
		const { container } = render(Select, { props: { options: ['hybrid', 'bm25', 'vector'] } });

		const select = container.querySelector('select')!;
		expect(select.value).toBe('hybrid');
		expect(select.selectedIndex).toBe(0);
		expect(container.querySelectorAll('option')).toHaveLength(3);
	});

	it('honours a given value and labels object options', () => {
		const { container } = render(Select, {
			props: {
				options: [
					{ value: 'bm25', label: 'Keyword' },
					{ value: 'vector', label: 'Semantic' }
				],
				value: 'vector',
				size: 'sm'
			}
		});

		const select = container.querySelector('select')!;
		expect(select.value).toBe('vector');
		expect(select.className).toBe('dbm-select dbm-select--sm');
		expect(container.querySelectorAll('option')[0].textContent).toBe('Keyword');
	});

	it('opens its own menu instead of the native popup, and a pick fires change on the carrier', async () => {
		const seen: string[] = [];
		const { container, queryByRole, getAllByRole } = render(Select, {
			props: {
				options: ['hybrid', 'bm25', 'vector'],
				'data-testid': 'search-mode',
				onchange: (e: Event) => seen.push((e.currentTarget as HTMLSelectElement).value)
			}
		});
		const trigger = container.querySelector('button.dbm-select')!;
		expect(trigger.textContent?.trim()).toBe('hybrid');
		expect(queryByRole('listbox')).toBeNull();

		await fireEvent.click(trigger);
		const options = getAllByRole('option');
		expect(options.map((o) => o.textContent?.trim())).toEqual(['hybrid', 'bm25', 'vector']);
		expect(options[0].getAttribute('aria-selected')).toBe('true');

		await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
		expect(options[1].classList.contains('active')).toBe(true);

		await fireEvent.click(options[2]);
		expect(queryByRole('listbox')).toBeNull();
		expect(container.querySelector('select')!.value).toBe('vector');
		expect(trigger.textContent?.trim()).toBe('vector');
		expect(seen).toEqual(['vector']);
	});
});

describe('Checkbox', () => {
	it('renders the radio variant with its dot when checked', () => {
		const { container } = render(Checkbox, {
			props: { radio: true, checked: true, label: 'Production' }
		});

		expect(container.querySelector('label')!.className).toBe('dbm-check dbm-radio');
		expect(container.querySelector('input')!.getAttribute('type')).toBe('radio');
		expect(container.querySelector('.dbm-check__box.dbm-radio__box')).toBeTruthy();
		expect(container.querySelector('.dbm-radio__dot')).toBeTruthy();
	});

	it('draws the dash instead of the tick when indeterminate', () => {
		const { container } = render(Checkbox, { props: { indeterminate: true } });

		const path = container.querySelector('.dbm-check__box svg path')!;
		expect(path.getAttribute('d')).toBe('M2 5h6');
	});
});

describe('Skeleton', () => {
	it('renders one placeholder per cell and reports itself as a status', () => {
		const { container } = render(Skeleton, { props: { rows: 3, columns: 2 } });

		const root = container.querySelector('.dbm-skeleton-rows')!;
		expect(root.getAttribute('role')).toBe('status');
		expect(root.querySelectorAll(':scope > div')).toHaveLength(3);
		expect(root.querySelectorAll('span.dbm-skeleton')).toHaveLength(6);
	});
});

describe('IconButton', () => {
	it('labels the button and marks it pressed when active', () => {
		const { container } = render(IconButton, {
			props: { icon: 'refresh-cw', label: 'Reload', active: true }
		});

		const button = container.querySelector('button')!;
		expect(button.className).toBe('dbm-iconbtn dbm-iconbtn--active');
		expect(button.getAttribute('aria-label')).toBe('Reload');
		expect(button.getAttribute('title')).toBe('Reload');
		expect(button.getAttribute('aria-pressed')).toBe('true');
	});

	it('lets a caller title win over the label', () => {
		const { container } = render(IconButton, {
			props: { icon: 'refresh-cw', label: 'Reload', title: 'Reload memories ⌘R' }
		});

		const button = container.querySelector('button')!;
		expect(button.getAttribute('title')).toBe('Reload memories ⌘R');
		expect(button.getAttribute('aria-label')).toBe('Reload');
		expect(button.getAttribute('aria-pressed')).toBe(null);
	});
});

describe('Tooltip', () => {
	it('renders the label and the shortcut in the popover', () => {
		const { container } = render(Tooltip, {
			props: { label: 'Execute statement', combo: 'Mod+Enter', children: text('Run') }
		});

		expect(container.querySelector('.dbm-tip')!.textContent).toContain('Run');

		const pop = container.querySelector('.dbm-tip__pop')!;
		expect(pop.getAttribute('role')).toBe('tooltip');
		expect(pop.className).toBe('dbm-tip__pop dbm-tip__pop--top');
		expect(pop.textContent).toContain('Execute statement');
		expect(pop.querySelector('.dbm-tip__kbd .dbm-keyhint')).toBeTruthy();
	});
});

describe('Badge', () => {
	it('maps outline to its own key', () => {
		const { container } = render(Badge, {
			props: { variant: 'outline', children: text('moved') }
		});
		expect(container.querySelector('span.dbm-badge')!.className).toBe(
			'dbm-badge dbm-badge--outline'
		);
	});

	it('adds the mono modifier', () => {
		const { container } = render(Badge, { props: { mono: true, children: text('ATL-2') } });
		expect(container.querySelector('span.dbm-badge')!.className).toContain('dbm-badge--mono');
	});

	it('maps a solid danger badge to the solid-danger key', () => {
		const { container } = render(Badge, {
			props: { variant: 'solid', tone: 'danger', children: text('failed') }
		});
		expect(container.querySelector('span.dbm-badge')!.className).toBe(
			'dbm-badge dbm-badge--solid-danger'
		);
	});
});

describe('KeyHint', () => {
	it('resolves Mod to the command glyph on mac', () => {
		const { container } = render(KeyHint, { props: { combo: 'Mod+K', platform: 'mac' } });
		expect(container.querySelector('.dbm-keyhint')!.textContent).toBe('⌘K');
	});

	it('resolves Mod to Ctrl and joins with a plus on windows', () => {
		const { container } = render(KeyHint, { props: { combo: 'Mod+K', platform: 'windows' } });
		expect(container.querySelector('.dbm-keyhint')!.textContent).toBe('Ctrl+K');
	});
});

describe('Icon', () => {
	it('renders an svg at the requested size for a known name', () => {
		const { container } = render(Icon, { props: { name: 'database', size: 18 } });

		const svg = container.querySelector('svg')!;
		expect(svg.getAttribute('width')).toBe('18');
		expect(svg.getAttribute('height')).toBe('18');
		expect(svg.classList.contains('lucide-database')).toBe(true);
	});

	it('falls back to circle and warns once for an unknown name', () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

		render(Icon, { props: { name: 'no-such-glyph' } });
		render(Icon, { props: { name: 'no-such-glyph' } });

		expect(document.querySelector('svg.lucide-circle')).toBeTruthy();
		expect(warn).toHaveBeenCalledTimes(1);
		warn.mockRestore();
	});
});

describe('Typeahead', () => {
	const rows = [
		{ value: 'ATL-9', label: 'Newer daemon fix', hint: 'ATL-9' },
		{ value: 'ATL-3', label: 'Older daemon fix', hint: 'ATL-3' }
	];

	it('shows the rows under the box while typing, moves with the arrows and picks on Enter', async () => {
		const picked: string[] = [];
		const { container, queryByRole, getAllByRole } = render(Typeahead, {
			props: { rows, value: '', onpick: (r: { value: string }) => picked.push(r.value), 'data-testid': 'search' }
		});
		const input = container.querySelector('input')!;
		expect(queryByRole('listbox')).toBeNull();

		await fireEvent.focus(input);
		await fireEvent.input(input, { target: { value: 'daemon' } });
		expect(queryByRole('listbox')).not.toBeNull();
		const options = getAllByRole('option');
		expect(options.map((o) => o.textContent?.replace(/\s+/g, ' ').trim())).toEqual(['Newer daemon fix ATL-9', 'Older daemon fix ATL-3']);
		expect(options[0].getAttribute('aria-selected')).toBe('true');

		await fireEvent.keyDown(input, { key: 'ArrowDown' });
		expect(options[1].getAttribute('aria-selected')).toBe('true');
		await fireEvent.keyDown(input, { key: 'Enter' });
		expect(picked).toEqual(['ATL-3']);
		expect(queryByRole('listbox')).toBeNull();
	});

	it('closes on Escape and stays closed for that text, and clicking a row picks it', async () => {
		const picked: string[] = [];
		const { container, queryByRole, getByText } = render(Typeahead, {
			props: { rows, value: '', onpick: (r: { value: string }) => picked.push(r.value) }
		});
		const input = container.querySelector('input')!;
		await fireEvent.focus(input);
		await fireEvent.input(input, { target: { value: 'daemon' } });
		expect(queryByRole('listbox')).not.toBeNull();
		await fireEvent.keyDown(input, { key: 'Escape' });
		expect(queryByRole('listbox')).toBeNull();
		await fireEvent.input(input, { target: { value: 'daemon f' } });
		expect(queryByRole('listbox')).not.toBeNull();
		await fireEvent.click(getByText('Newer daemon fix'));
		expect(picked).toEqual(['ATL-9']);
	});
});
