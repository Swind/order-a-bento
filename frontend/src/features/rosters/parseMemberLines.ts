export interface ParsedMember {
  name: string;
  note?: string;
}

/**
 * One person per line. An optional note follows the name, separated by a half-width comma,
 * a full-width comma or a tab: `王小明, 業務部` / `王小明，業務部` / `王小明<TAB>業務部`.
 * Blank lines (and lines without a name) are ignored.
 */
export function parseMemberLines(text: string): ParsedMember[] {
  const result: ParsedMember[] = [];
  for (const raw of text.split(/\r\n|\r|\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const idx = line.search(/[,，\t]/);
    const name = (idx === -1 ? line : line.slice(0, idx)).trim();
    if (!name) continue;
    const note = idx === -1 ? "" : line.slice(idx + 1).trim();
    result.push(note ? { name, note } : { name });
  }
  return result;
}
