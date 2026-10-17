/** LINE share deep link carrying 「標題 + 換行 + 連結」. */
export function lineShareUrl(title: string, shareUrl: string): string {
  return `https://line.me/R/share?text=${encodeURIComponent(`${title}\n${shareUrl}`)}`;
}
