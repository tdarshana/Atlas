// @vitest-environment jsdom
// The channel status the Plugins page and the Settings Plugins card read. The point of the
// grace period is that the normal gap while a socket opens says nothing, and a channel
// that never comes up does.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import ToolChannelWarning from './ToolChannelWarning.svelte';
import {
	CHANNEL_GRACE_MS,
	channelWarning,
	reportChannelError,
	setChannelConnected,
	setChannelWanted
} from './tool-channel.svelte';

beforeEach(() => {
	vi.useFakeTimers();
	setChannelWanted(false);
});

afterEach(() => {
	cleanup();
	setChannelWanted(false);
	vi.useRealTimers();
});

describe('channelWarning', () => {
	it('says nothing when no plugin wants a channel', () => {
		vi.advanceTimersByTime(CHANNEL_GRACE_MS * 2);

		expect(channelWarning()).toBeNull();
	});

	it('says nothing during the grace period, then names the failure', () => {
		setChannelWanted(true);
		reportChannelError('the plugin channel reported an error');

		vi.advanceTimersByTime(CHANNEL_GRACE_MS - 1);
		expect(channelWarning()).toBeNull();

		vi.advanceTimersByTime(1);
		expect(channelWarning()).toBe(
			'Plugin tools channel is not connected: the plugin channel reported an error'
		);
	});

	it('says the plain sentence when nothing reported a reason', () => {
		setChannelWanted(true);

		vi.advanceTimersByTime(CHANNEL_GRACE_MS);

		expect(channelWarning()).toBe('Plugin tools channel is not connected');
	});

	it('goes quiet the moment the socket comes up, and stays quiet on a short drop', () => {
		setChannelWanted(true);
		vi.advanceTimersByTime(CHANNEL_GRACE_MS);
		expect(channelWarning()).not.toBeNull();

		setChannelConnected(true);
		expect(channelWarning()).toBeNull();

		// A drop restarts the grace period rather than warning at once.
		setChannelConnected(false);
		vi.advanceTimersByTime(CHANNEL_GRACE_MS - 1);
		expect(channelWarning()).toBeNull();
		vi.advanceTimersByTime(1);
		expect(channelWarning()).toBe('Plugin tools channel is not connected');
	});

	it('forgets everything when the last tool-contributing plugin goes', () => {
		setChannelWanted(true);
		reportChannelError('boom');
		vi.advanceTimersByTime(CHANNEL_GRACE_MS);
		expect(channelWarning()).not.toBeNull();

		setChannelWanted(false);

		expect(channelWarning()).toBeNull();
	});
});

describe('ToolChannelWarning', () => {
	it('renders nothing while the channel is healthy', () => {
		setChannelWanted(true);
		setChannelConnected(true);

		const { queryByTestId } = render(ToolChannelWarning);

		expect(queryByTestId('plugin-channel-warning')).toBeNull();
	});

	it('renders the one line, with the last error, once the channel is late', () => {
		setChannelWanted(true);
		reportChannelError('Refused to connect');
		vi.advanceTimersByTime(CHANNEL_GRACE_MS);

		const { getByTestId } = render(ToolChannelWarning);

		expect(getByTestId('plugin-channel-warning').textContent).toContain(
			'Plugin tools channel is not connected: Refused to connect'
		);
	});
});
