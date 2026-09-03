// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';

import Badge from './Badge.svelte';
import Button from './Button.svelte';
import Icon from './Icon.svelte';
import Input from './Input.svelte';
import KeyHint from './KeyHint.svelte';

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
