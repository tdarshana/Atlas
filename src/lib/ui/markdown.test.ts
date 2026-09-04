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
});

describe('highlightSource', () => {
	it('wraps the whole source in a highlighted pre, as markdown', () => {
		const html = highlightSource('# Heading\n\nSome *text* and `code`.');
		expect(html).toContain('<pre class="hljs">');
		expect(html).toMatch(/class="hljs-/);
	});
});
