/** Reads a label from one edited attribute of an already admitted opening tag. */
function renamed(source: string, draft: string, offset: number, attribute: string, original: string): string | undefined {
  const element = /<[\w:-]+/y;
  element.lastIndex = offset;
  if (!element.test(source)) return;
  const field = /\s+([\w:.-]+)\s*=\s*(["'])(.*?)\2/gsy;
  field.lastIndex = element.lastIndex;
  let match;
  while ((match = field.exec(source))) {
    if (match[1] !== attribute || match[3] !== original) continue;
    const end = field.lastIndex - 1, start = end - original.length;
    const nextEnd = end + draft.length - source.length;
    if (nextEnd <= start || !draft.startsWith(source.slice(0, start)) || !draft.endsWith(source.slice(end))) return;
    const value = draft.slice(start, nextEnd);
    // Match the AML source-id alphabet. Escaped values defer to the authoritative reader.
    if (value !== original && /^[a-zA-Z_][a-zA-Z0-9_.:/-]*$/.test(value)) return value;
  }
}

/** A selected city's mesh name carries its placement owner and the Rust source location. */
export function renamedSelection(source: string, draft: string, selected?: { id: string; objectId: string; source: { offset: number } }) {
  if (!selected || !selected.objectId.startsWith("city/") || !selected.id.startsWith(selected.objectId + "/")) return;
  const owner = selected.objectId.slice(5);
  // A row has a generated numeric suffix. Read the AML id, which can itself contain a slash.
  const candidates = [owner, owner.replace(/\/\d+$/, "")];
  for (const original of new Set(candidates)) {
    const value = renamed(source, draft, selected.source.offset, "id", original);
    if (value) return "city/" + value + selected.id.slice(5 + original.length);
  }
}
