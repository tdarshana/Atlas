// WebKit's `autocorrect` attribute is not in Svelte's element typings yet; every text box
// in the app sets it (with autocapitalize and spellcheck) to keep macOS from offering
// replacements while typing keys, names and code.
declare namespace svelteHTML {
	interface HTMLAttributes<T> {
		autocorrect?: 'on' | 'off' | null;
	}
}

export {};
