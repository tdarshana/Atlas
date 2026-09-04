// Theme packs (`ui.theme_pack`): `{ name, base: "dark"|"light", tokens: { "--token":
// "value" } }`. A pack may override colour tokens and the two radius tokens
// (`--radius-sm`, `--radius-md`) only. The allowed token names are read out of the
// same token files the design system ships, `colors.css` and `spacing.css` (via
// Vite's `?raw` import), rather than hand-copied here, so an added or renamed token
// follows automatically; `crates/atlas-core/src/settings.rs` derives the same list
// the same way, independently, since the daemon cannot import this module.

import colorsCss from '$lib/ds/tokens/colors.css?raw';
import spacingCss from '$lib/ds/tokens/spacing.css?raw';
import type { ThemePack } from '$lib/types';

/** The only two length tokens a pack may override. */
const RADIUS_TOKEN_NAMES = ['--radius-sm', '--radius-md'] as const;

/** A pack's JSON text is capped so a client cannot write an unbounded blob into the
 * setting (the token allow list itself is finite, but `name` is a free string).
 * Matches `MAX_THEME_PACK_BYTES` in `crates/atlas-core/src/settings.rs`. */
const MAX_THEME_PACK_BYTES = 16 * 1024;

/** `name`'s own cap, tighter than the whole-pack one since it is shown in the Theme
 * select. Matches `MAX_THEME_PACK_NAME_CHARS` in `crates/atlas-core/src/settings.rs`. */
const MAX_THEME_PACK_NAME_CHARS = 64;

/** Strips CSS comments so a colon inside one (an "AA fix: ..." note) is never mistaken
 * for part of a declaration. */
function stripCssComments(css: string): string {
	let out = '';
	let rest = css;
	for (;;) {
		const start = rest.indexOf('/*');
		if (start === -1) {
			out += rest;
			return out;
		}
		out += rest.slice(0, start);
		const end = rest.indexOf('*/', start + 2);
		rest = end === -1 ? '' : rest.slice(end + 2);
	}
}

/** Every `--name: value;` custom property declared in a CSS file, in source order. */
function parseDeclarations(css: string): [string, string][] {
	const cleaned = stripCssComments(css);
	const out: [string, string][] = [];
	for (const rawChunk of cleaned.split(';')) {
		const chunk = rawChunk.trim();
		if (!chunk.startsWith('--')) continue;
		const idx = chunk.indexOf(':');
		if (idx === -1) continue;
		const name = `--${chunk.slice(2, idx).trim()}`;
		const value = chunk.slice(idx + 1).trim();
		if (name.length > 2 && value) out.push([name, value]);
	}
	return out;
}

/** Whether `value` is a CSS colour: `#rgb`, `#rrggbb`, `#rrggbbaa`, or a
 * `rgb()`/`rgba()`/`hsl()`/`hsla()`/`oklch()` function call. */
export function isCssColor(value: string): boolean {
	const v = value.trim();
	const hex = v.startsWith('#') ? v.slice(1) : null;
	if (hex !== null) {
		return [3, 4, 6, 8].includes(hex.length) && /^[0-9a-fA-F]+$/.test(hex);
	}
	return ['rgb(', 'rgba(', 'hsl(', 'hsla(', 'oklch('].some((p) => v.startsWith(p) && v.endsWith(')'));
}

/** Whether `value` is a plain CSS length (`3px`, `0.5rem`, `0`), the shape the two
 * radius tokens take. */
export function isCssLength(value: string): boolean {
	const v = value.trim();
	if (v === '0') return true;
	return /^\d+(\.\d+)?(px|rem|em|%)$/.test(v);
}

/** The set of custom properties a theme pack may override: every token in
 * `colors.css` whose declared value is a literal colour (not a `var()` alias or a
 * shadow), plus the two named radius tokens, checked present in `spacing.css`. */
export function themeTokenAllowlist(): Set<string> {
	const names = new Set<string>();
	for (const [name, value] of parseDeclarations(colorsCss)) {
		if (isCssColor(value)) names.add(name);
	}
	for (const radius of RADIUS_TOKEN_NAMES) {
		if (parseDeclarations(spacingCss).some(([name]) => name === radius)) names.add(radius);
	}
	return names;
}

function isRadiusToken(name: string): boolean {
	return (RADIUS_TOKEN_NAMES as readonly string[]).includes(name);
}

/**
 * Validates an imported pack's shape, its token names against the allow list, and
 * each value's colour/length syntax. Throws a message fit to show the user; never
 * partially applies a pack that fails.
 */
export function validateThemePack(json: unknown): ThemePack {
	if (typeof json !== 'object' || json === null) throw new Error('Theme pack must be a JSON object.');
	if (new TextEncoder().encode(JSON.stringify(json)).length > MAX_THEME_PACK_BYTES) {
		throw new Error(`Theme pack must be at most ${MAX_THEME_PACK_BYTES} bytes of JSON.`);
	}
	const obj = json as Record<string, unknown>;
	if (typeof obj.name !== 'string' || obj.name.trim() === '') {
		throw new Error('Theme pack needs a non-empty "name".');
	}
	if (obj.name.length > MAX_THEME_PACK_NAME_CHARS) {
		throw new Error(`Theme pack "name" must be at most ${MAX_THEME_PACK_NAME_CHARS} characters.`);
	}
	if (obj.base !== 'dark' && obj.base !== 'light') {
		throw new Error('Theme pack "base" must be "dark" or "light".');
	}
	if (typeof obj.tokens !== 'object' || obj.tokens === null || Array.isArray(obj.tokens)) {
		throw new Error('Theme pack "tokens" must be an object.');
	}
	const allowed = themeTokenAllowlist();
	const tokens: Record<string, string> = {};
	for (const [name, rawValue] of Object.entries(obj.tokens as Record<string, unknown>)) {
		if (!allowed.has(name)) throw new Error(`Unknown theme token '${name}'.`);
		if (typeof rawValue !== 'string') throw new Error(`Theme token '${name}' must be a string.`);
		const ok = isRadiusToken(name) ? isCssLength(rawValue) : isCssColor(rawValue);
		if (!ok) throw new Error(`Theme token '${name}' must be a ${isRadiusToken(name) ? 'CSS length' : 'CSS colour'}.`);
		tokens[name] = rawValue;
	}
	return { name: obj.name, base: obj.base, tokens };
}

/** `src/app.html`'s boot script paints pack tokens onto `documentElement` before this
 * module (or the rest of the SvelteKit bundle) ever loads, and leaves the names it set
 * in this dataset attribute. Reading it back here means a later `clearThemePack` (e.g.
 * switching back to Dark or Light and saving) removes those tokens too, not just ones
 * an `applyThemePack` call made during this page's own lifetime. */
function readBootTokenNames(): string[] {
	if (typeof document === 'undefined') return [];
	const raw = document.documentElement.dataset.themePackTokens;
	if (!raw) return [];
	try {
		const parsed = JSON.parse(raw);
		return Array.isArray(parsed) ? parsed.filter((n): n is string => typeof n === 'string') : [];
	} catch {
		return [];
	}
}

/** The token names the last `applyThemePack` call set, so a later call (or
 * `clearThemePack`) knows what to remove first: a token a new pack drops must not be
 * left behind from the previous one. Starts from whatever the boot script already
 * painted, not empty, so the first clear in a session still removes it. */
let appliedTokenNames: string[] = readBootTokenNames();

/** Applies a pack's tokens as inline custom properties on `documentElement`, on top
 * of whatever `data-theme` is already set (the caller sets the base). */
export function applyThemePack(pack: ThemePack | null): void {
	if (typeof document === 'undefined') return;
	const root = document.documentElement;
	for (const name of appliedTokenNames) root.style.removeProperty(name);
	appliedTokenNames = [];
	if (!pack) return;
	for (const [name, value] of Object.entries(pack.tokens)) {
		root.style.setProperty(name, value);
		appliedTokenNames.push(name);
	}
}

/** Removes any pack tokens applied by `applyThemePack`, for switching back to a plain
 * Dark or Light theme. */
export function clearThemePack(): void {
	applyThemePack(null);
}
