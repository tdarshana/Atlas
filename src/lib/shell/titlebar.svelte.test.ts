// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte';

import TitleBar from './TitleBar.svelte';

describe('TitleBar', () => {
	it('names the view in the command box', () => {
		const { getByTestId } = render(TitleBar, { platform: 'mac', title: 'Dashboard' });
		expect(getByTestId('titlebar-command').textContent).toContain('Atlas · Dashboard');
	});

	it('leaves the window controls to macOS', () => {
		const { queryByLabelText } = render(TitleBar, { platform: 'mac', title: 'Dashboard' });
		expect(queryByLabelText('Close')).toBeNull();
		expect(queryByLabelText('Minimise')).toBeNull();
	});

	it('draws menus and three window controls on windows', () => {
		const { getByLabelText, getByText } = render(TitleBar, {
			platform: 'windows',
			title: 'Memories'
		});
		for (const menu of ['File', 'Edit', 'View', 'Window', 'Help']) {
			expect(getByText(menu)).toBeTruthy();
		}
		for (const control of ['Minimise', 'Maximise', 'Close']) {
			expect(getByLabelText(control)).toBeTruthy();
		}
	});
});
