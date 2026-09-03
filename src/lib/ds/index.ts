/* The Atlas port of the DBMan design system. Import components from here; the stylesheet
   is `$lib/ds/index.css`, pulled in once by `src/app.css`. */

export { default as Badge } from './Badge.svelte';
export { default as Button } from './Button.svelte';
export { default as Checkbox } from './Checkbox.svelte';
export { default as Icon } from './Icon.svelte';
export { default as IconButton } from './IconButton.svelte';
export { default as Input } from './Input.svelte';
export { default as KeyHint, resolvePlatform, comboKeys } from './KeyHint.svelte';
export { default as Select } from './Select.svelte';
export { default as Skeleton } from './Skeleton.svelte';
export { default as Tooltip } from './Tooltip.svelte';

export type { Platform } from './KeyHint.svelte';
export type { SelectOption } from './Select.svelte';
export { icons, resolveIcon, type IconName } from './icons';
