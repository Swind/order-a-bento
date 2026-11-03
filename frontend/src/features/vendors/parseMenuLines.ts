export interface ParsedMenuItem {
  name: string;
  price: number;
  description?: string;
}

export interface MenuLineError {
  /** 1-based line number in the pasted text. */
  line: number;
  reason: string;
}

export const MAX_PRICE = 100_000;

/** Parse `85`, `$85`, `NT$85`, `85元`, `１２０` (full-width). Returns null when not a price. */
export function parsePrice(raw: string): number | null {
  const s = raw.normalize("NFKC").trim().replace(/,/g, "");
  const m = /^(?:nt\s*\$?|\$)?\s*(\d+(?:\.\d{1,2})?)\s*(?:元|塊)?$/i.exec(s);
  const cents = m ? parseCents(m[1]) : null;
  return cents === null ? null : cents / 100;
}

/**
 * One item per line: `品名 價格` (last whitespace-separated token is the price) or
 * `品名,價格,說明` (comma / full-width comma / tab separated). Blank lines are skipped.
 */
export function parseMenuLines(text: string): { items: ParsedMenuItem[]; errors: MenuLineError[] } {
  const items: ParsedMenuItem[] = [];
  const errors: MenuLineError[] = [];
  text.split(/\r\n|\r|\n/).forEach((raw, i) => {
    const line = raw.trim();
    if (!line) return;
    const lineNo = i + 1;
    let name: string;
    let priceText: string;
    let description = "";
    if (/[,，\t]/.test(line)) {
      const parts = line.split(/[,，\t]/);
      name = parts[0].trim();
      priceText = (parts[1] ?? "").trim();
      description = parts.slice(2).join(",").trim();
    } else {
      const m = /^(.*\S)\s+(\S+)$/.exec(line);
      if (!m) {
        errors.push({ line: lineNo, reason: "缺少價格" });
        return;
      }
      name = m[1].trim();
      priceText = m[2];
    }
    if (!name) {
      errors.push({ line: lineNo, reason: "缺少品名" });
      return;
    }
    if (!priceText) {
      errors.push({ line: lineNo, reason: "缺少價格" });
      return;
    }
    const price = parsePrice(priceText);
    if (price === null) {
      errors.push({ line: lineNo, reason: `價格格式不正確：${priceText}` });
      return;
    }
    if (price < 0.01 || price > MAX_PRICE) {
      errors.push({ line: lineNo, reason: `價格需介於 0.01 到 ${MAX_PRICE.toLocaleString("en-US")}，最多小數兩位` });
      return;
    }
    items.push(description ? { name, price, description } : { name, price });
  });
  return { items, errors };
}
import { parseCents } from "@/shared/lib/money";
