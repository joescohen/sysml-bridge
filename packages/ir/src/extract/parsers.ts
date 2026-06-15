/** Parse the "Relevant Need(s)" cell into an array of need IDs. */
export function parseNeeds(raw: string | undefined | null): string[] {
  if (!raw) return [];
  // Split on commas and/or whitespace, trim, keep tokens matching /^N\d+$/
  return String(raw)
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter((t) => /^N\d+$/.test(t));
}

/**
 * Parse the token before the first colon as a function/activity id.
 * "F3.3: Initiate Fuel Transfer" → "F3.3"
 * "F3.3" (no colon) → "F3.3"
 */
export function parseActivityId(raw: string): string {
  const colonIdx = raw.indexOf(":");
  return colonIdx >= 0 ? raw.slice(0, colonIdx).trim() : raw.trim();
}

/**
 * Return the substring after the first colon, trimmed (strips the id prefix).
 * "F1.1: Receive & Authenticate Request" → "Receive & Authenticate Request"
 * "Receive & Authenticate Request" (no colon) → "Receive & Authenticate Request"
 */
export function stripIdPrefix(raw: string): string {
  const colonIdx = raw.indexOf(":");
  return colonIdx >= 0 ? raw.slice(colonIdx + 1).trim() : raw.trim();
}
