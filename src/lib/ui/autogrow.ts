// A Svelte action that keeps a `<textarea>`'s height in sync with its content, so
// it grows in place (Jira style) instead of scrolling internally. The caller still
// sets `rows` for a minimum height and `overflow: hidden; resize: none;` in CSS so
// neither the browser's resize handle nor a scrollbar ever appears.
import type { Action } from 'svelte/action';

export const autogrow: Action<HTMLTextAreaElement> = (node) => {
	function resize() {
		node.style.height = 'auto';
		node.style.height = `${node.scrollHeight}px`;
	}

	resize();
	node.addEventListener('input', resize);

	return {
		destroy() {
			node.removeEventListener('input', resize);
		}
	};
};
