/** External dollars -> exact integer cents, without multiplying a float by 100. */
export function parseCents(raw: string): number | null {
  const match = /^(-?)(\d+)(?:\.(\d{1,2}))?$/.exec(raw.trim());
  if (!match) return null;
  const cents = Number(match[2]) * 100 + Number((match[3] ?? "").padEnd(2, "0"));
  return Number.isSafeInteger(cents) ? (match[1] ? -cents : cents) : null;
}

export function orderTotal(items: { price: number; quantity: number }[]): number {
  let cents = 0;
  for (const item of items) {
    const price = parseCents(String(item.price));
    if (price === null || price < 0 || item.quantity < 0 || !Number.isSafeInteger(item.quantity)) throw new Error("Invalid money or quantity");
    const subtotal = price * item.quantity;
    if (!Number.isSafeInteger(subtotal) || !Number.isSafeInteger(cents + subtotal)) throw new Error("Money overflow");
    cents += subtotal;
  }
  return cents / 100;
}
