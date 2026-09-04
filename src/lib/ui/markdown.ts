// The shared Markdown pipeline behind `MarkdownView`. One place owns marked's setup so the
// fenced-block language subset, the disabled images and relative links, and the DOMPurify pass
// all live together instead of being repeated at each of the app's four Markdown surfaces.
// `{@html}` in the component is fed only by `renderMarkdown` or `highlightSource`, never by
// marked's or hljs's raw output.

import { Marked } from 'marked';
import DOMPurify from 'dompurify';
import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import ini from 'highlight.js/lib/languages/ini'; // registers the `toml` alias
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import markdownLang from 'highlight.js/lib/languages/markdown';
import rust from 'highlight.js/lib/languages/rust';
import shell from 'highlight.js/lib/languages/shell';
import sql from 'highlight.js/lib/languages/sql';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

hljs.registerLanguage('bash', bash);
hljs.registerLanguage('css', css);
hljs.registerLanguage('diff', diff);
hljs.registerLanguage('ini', ini);
hljs.registerLanguage('javascript', javascript);
hljs.registerLanguage('json', json);
hljs.registerLanguage('markdown', markdownLang);
hljs.registerLanguage('rust', rust);
hljs.registerLanguage('shell', shell);
hljs.registerLanguage('sql', sql);
hljs.registerLanguage('typescript', typescript);
hljs.registerLanguage('xml', xml);
hljs.registerLanguage('yaml', yaml);

function escapeHtml(text: string): string {
	return text
		.replace(/&/g, '&amp;')
		.replace(/</g, '&lt;')
		.replace(/>/g, '&gt;')
		.replace(/"/g, '&quot;')
		.replace(/'/g, '&#39;');
}

/** Highlights one fenced block against the registered language subset; an unregistered or
    missing language falls back to escaped plain text rather than guessing at one. */
function highlightBlock(text: string, lang: string | undefined): { html: string; language: string } {
	const requested = lang?.trim().split(/\s+/)[0]?.toLowerCase();
	if (requested && hljs.getLanguage(requested)) {
		return { html: hljs.highlight(text, { language: requested }).value, language: requested };
	}
	return { html: escapeHtml(text), language: 'plaintext' };
}

function isHttpUrl(href: string): boolean {
	return /^https?:\/\//i.test(href);
}

// Documents come from local repos: an image reference has nothing to fetch, so it renders as
// its alt text, and a link that is not a plain http(s) URL (a relative path, a `file:` link)
// renders as plain text rather than a dead or unsafe anchor.
const marked = new Marked({
	gfm: true,
	renderer: {
		code({ text, lang }) {
			const { html, language } = highlightBlock(text, lang);
			return `<pre><code class="hljs language-${language}">${html}</code></pre>`;
		},
		link({ href, title, tokens }) {
			const label = this.parser.parseInline(tokens);
			if (!href || !isHttpUrl(href)) return label;
			const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
			return `<a href="${escapeHtml(href)}"${titleAttr}>${label}</a>`;
		},
		image({ text }) {
			return text ? escapeHtml(text) : '';
		}
	}
});

// An explicit allowlist rather than the default profile minus a few tags: DOMPurify's default
// profile includes `form`, `input`, `button`, `select`, `option`, `textarea`, `svg` and `image`,
// none of which a rendered document should ever produce. Everything not listed here (forms,
// media embeds, `iframe`, `svg`, MathML, `script`, `style`) is stripped regardless of what a
// document's raw HTML or `marked`'s own output contains.
const ALLOWED_TAGS = [
	'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
	'p', 'br', 'hr', 'blockquote',
	'ul', 'ol', 'li',
	'pre', 'code', 'span',
	'strong', 'em', 'del',
	'a',
	'table', 'thead', 'tbody', 'tr', 'th', 'td',
	'sup', 'sub',
	'details', 'summary',
	'input'
];

const ALLOWED_ATTR = [
	'href',
	'title',
	'class',
	'start',
	'align',
	'colspan',
	'rowspan',
	'type',
	'checked',
	'disabled'
];

// An isolated instance rather than the shared global `DOMPurify` object: the hooks below
// are specific to this module's rules, and registering them on the global would let them
// silently apply to any other `sanitize()` call added elsewhere later, and stack a second
// copy of both on an HMR re-import.
const purify = DOMPurify(typeof window === 'undefined' ? undefined : window);

// `input` is allowed only for a GFM task-list checkbox (`marked`'s own output, always
// `type="checkbox" disabled`); an `<a>` is allowed only with an http(s) `href`, the same rule
// the custom `link()` renderer above applies to Markdown-syntax links, so a raw HTML anchor a
// document's own markup contains cannot smuggle a `javascript:`, `data:` or relative link past
// it. `ALLOWED_URI_REGEXP` on the `sanitize()` call already strips a disallowed `href`; this
// hook additionally drops the anchor itself, so the two paths behave identically.
purify.addHook('uponSanitizeElement', (node, event) => {
	if (event.tagName === 'a') {
		const href = (node as Element).getAttribute('href');
		if (href && !isHttpUrl(href)) (node as Element).remove();
		return;
	}
	if (event.tagName === 'input') {
		const el = node as Element;
		const isTaskCheckbox = el.getAttribute('type') === 'checkbox' && el.hasAttribute('disabled');
		if (!isTaskCheckbox) el.remove();
	}
});

// DOMPurify checks every attribute value against `ALLOWED_URI_REGEXP` unless the attribute
// name is on its own URI-safe list (`class` and `title` already are): `type`, `checked`,
// `disabled`, `start`, `align`, `colspan` and `rowspan` never hold a URI, so without this a
// checkbox's `type="checkbox"` or a table cell's `colspan="2"` would fail the http(s) test and
// be stripped as if it were an unsafe link.
const URI_SAFE_ATTR = ['type', 'checked', 'disabled', 'start', 'align', 'colspan', 'rowspan'];

function sanitize(html: string): string {
	return purify.sanitize(html, {
		ALLOWED_TAGS,
		ALLOWED_ATTR,
		ADD_URI_SAFE_ATTR: URI_SAFE_ATTR,
		ALLOWED_URI_REGEXP: /^https?:\/\//i
	});
}

/** Markdown source to sanitised, highlighted HTML for Preview mode. */
export function renderMarkdown(source: string): string {
	return sanitize(marked.parse(source, { async: false }) as string);
}

/** The raw source, highlighted as Markdown, for Source mode. */
export function highlightSource(source: string): string {
	const { value } = hljs.highlight(source, { language: 'markdown' });
	return sanitize(`<pre class="hljs"><code>${value}</code></pre>`);
}
