// Agent names are not known at build time, so the static adapter must not try to
// prerender this route; the SPA fallback serves it.
export const prerender = false;
