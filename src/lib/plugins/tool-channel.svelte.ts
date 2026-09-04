// Whether the daemon's plugin tool channel is actually up, as one store the Plugins page
// and the Settings Plugins card both read.
//
// The channel's own failure mode is quiet: a socket the CSP refuses, or a daemon that is
// not listening, just reconnects forever on the backoff, and the only trace would be a
// console warning nobody sees. So the state is kept here and surfaced in the app. A short
// gap while the socket opens is normal, so nothing is said until the channel has been
// wanted for ten seconds without ever coming up.

/** How long the channel may be down before the app says so. */
export const CHANNEL_GRACE_MS = 10_000;

export const toolChannel = $state({
	/** True while at least one plugin contributes a tool, so a warning would mean something. */
	wanted: false,
	connected: false,
	/** The last failure seen on the socket or on a registration, for the warning line. */
	error: null as string | null,
	/** True once the grace period has passed with the channel still down. */
	late: false
});

let timer: ReturnType<typeof setTimeout> | null = null;

function stopTimer(): void {
	if (timer !== null) clearTimeout(timer);
	timer = null;
}

/** Starts the grace period, unless one is already running or the channel is up. */
function startTimer(): void {
	if (timer !== null || toolChannel.connected || !toolChannel.wanted) return;
	timer = setTimeout(() => {
		timer = null;
		if (!toolChannel.connected && toolChannel.wanted) toolChannel.late = true;
	}, CHANNEL_GRACE_MS);
}

/** Called when the app starts or stops wanting a channel at all. */
export function setChannelWanted(on: boolean): void {
	toolChannel.wanted = on;
	if (!on) {
		stopTimer();
		toolChannel.connected = false;
		toolChannel.late = false;
		toolChannel.error = null;
		return;
	}
	startTimer();
}

/** Called on every open and every close of the socket. */
export function setChannelConnected(on: boolean): void {
	toolChannel.connected = on;
	if (on) {
		stopTimer();
		toolChannel.late = false;
		toolChannel.error = null;
		return;
	}
	startTimer();
}

/** The last thing that went wrong, kept whether or not anything is showing it yet. */
export function reportChannelError(message: string): void {
	toolChannel.error = message;
}

/** The warning to show, or null when there is nothing to say. */
export function channelWarning(): string | null {
	if (!toolChannel.wanted || toolChannel.connected || !toolChannel.late) return null;
	const base = 'Plugin tools channel is not connected';
	return toolChannel.error ? `${base}: ${toolChannel.error}` : base;
}
