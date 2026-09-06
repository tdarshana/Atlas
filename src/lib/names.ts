// The name rule the daemon enforces for practice and workflow documents, shared by
// the editors that let the user type one.

/** Mirrors the backend rule for practice and workflow names. */
export const NAME_PATTERN = /^[a-z0-9][a-z0-9-_]*$/;
export const NAME_MAX = 64;

/** The reason `name` is not a legal name, or null when it is one. */
export function nameError(name: string): string | null {
	if (name === '') return 'A name is required';
	if (name.length > NAME_MAX) return `A name is at most ${NAME_MAX} characters`;
	if (!NAME_PATTERN.test(name)) {
		return 'Use lowercase letters, digits, - and _, starting with a letter or digit';
	}
	return null;
}

/** Splits a comma-separated field into trimmed, non-empty entries. */
export function parseList(text: string): string[] {
	return text
		.split(',')
		.map((s) => s.trim())
		.filter((s) => s !== '');
}
