// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import MarkdownView from './MarkdownView.svelte';

afterEach(cleanup);
beforeEach(() => localStorage.clear());

describe('MarkdownView', () => {
	it('defaults to Preview', () => {
		const { container } = render(MarkdownView, { props: { source: '# Title' } });
		expect(container.querySelector('[data-testid="markdown-body"]')!.innerHTML).toContain(
			'<h1>Title</h1>'
		);
		expect(
			container.querySelector('[data-testid="markdown-mode-preview"]')!.getAttribute('aria-selected')
		).toBe('true');
	});

	it('switches to Source, a highlighted pre over the raw text', async () => {
		const { container } = render(MarkdownView, { props: { source: '# Title' } });
		await fireEvent.click(container.querySelector('[data-testid="markdown-mode-source"]')!);

		const body = container.querySelector('[data-testid="markdown-body"]')!;
		expect(body.querySelector('pre.hljs')).toBeTruthy();
		expect(body.textContent).toContain('# Title');
	});

	it('remembers the mode across a fresh instance', async () => {
		const first = render(MarkdownView, { props: { source: '# Title' } });
		await fireEvent.click(first.container.querySelector('[data-testid="markdown-mode-source"]')!);
		first.unmount();

		const second = render(MarkdownView, { props: { source: '# Title' } });
		expect(
			second.container
				.querySelector('[data-testid="markdown-mode-source"]')!
				.getAttribute('aria-selected')
		).toBe('true');
	});

	it('hides the header and always renders Preview when embedded', () => {
		localStorage.setItem('atlas.markdown.mode', 'source');
		const { container } = render(MarkdownView, {
			props: { source: '# Title', showHeader: false }
		});
		expect(container.querySelector('header')).toBeNull();
		expect(container.querySelector('[data-testid="markdown-body"]')!.innerHTML).toContain(
			'<h1>Title</h1>'
		);
	});
});
