import { describe, expect, it } from "vitest";
import { parseMemberLines } from "./parseMemberLines";

describe("parseMemberLines", () => {
  it("parses names without notes", () => {
    expect(parseMemberLines("王小明\n李大華")).toEqual([{ name: "王小明" }, { name: "李大華" }]);
  });
  it("splits notes on half-width comma, full-width comma and tab", () => {
    expect(parseMemberLines("王小明, 業務部\n李大華，會計\n陳小美\t工程部")).toEqual([
      { name: "王小明", note: "業務部" },
      { name: "李大華", note: "會計" },
      { name: "陳小美", note: "工程部" },
    ]);
  });
  it("ignores blank lines, trims and handles CRLF", () => {
    expect(parseMemberLines("\r\n  王小明  \r\n\r\n   \n")).toEqual([{ name: "王小明" }]);
  });
  it("keeps later separators inside the note and drops empty notes", () => {
    expect(parseMemberLines("A, x, y\nB,   \n,only note")).toEqual([
      { name: "A", note: "x, y" },
      { name: "B" },
    ]);
  });
});
