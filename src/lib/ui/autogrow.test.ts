// @vitest-environment jsdom
// The autogrow action: sets the textarea's height from its scrollHeight on mount and
// on every input, and stops listening once destroyed.

import { describe, expect, it } from 'vitest';
import { autogrow } from './autogrow';

function textareaWithScrollHeight(px: number): HTMLTextAreaElement {
	const node = document.createElement('textarea');
	Object.defineProperty(node, 'scrollHeight', { value: px, configurable: true });
	return node;
}

describe('autogrow', () => {
	it('sets the height from scrollHeight as soon as it is attached', () => {
		const node = textareaWithScrollHeight(48);

		autogrow(node);

		expect(node.style.height).toBe('48px');
	});

	it('resizes on every input, tracking a growing or shrinking scrollHeight', () => {
		const node = textareaWithScrollHeight(20);
		autogrow(node);

		Object.defineProperty(node, 'scrollHeight', { value: 96, configurable: true });
		node.dispatchEvent(new Event('input'));
		expect(node.style.height).toBe('96px');

		Object.defineProperty(node, 'scrollHeight', { value: 32, configurable: true });
		node.dispatchEvent(new Event('input'));
		expect(node.style.height).toBe('32px');
	});

	it('stops resizing once destroyed', () => {
		const node = textareaWithScrollHeight(20);
		const action = autogrow(node);

		action?.destroy?.();

		Object.defineProperty(node, 'scrollHeight', { value: 200, configurable: true });
		node.dispatchEvent(new Event('input'));
		expect(node.style.height).toBe('20px');
	});
});
