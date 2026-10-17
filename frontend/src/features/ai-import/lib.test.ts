import { describe, expect, it } from "vitest";
import { ROSTER_RUN, VENDOR_RUN } from "./fixtures";
import {
  buildApplyBody,
  countChanges,
  hasBlockingIssues,
  initState,
  issueMessage,
  mapServerErrors,
  rowKey,
  type TargetState,
  type VendorTargetState,
} from "./lib";

const vendor = () => initState(VENDOR_RUN.proposal);

function withPrice(state: TargetState[], price: string): TargetState[] {
  const v = state[0] as VendorTargetState;
  return [{ ...v, menu: v.menu.map((m) => (m.name === "雞腿飯" ? { ...m, price } : m)) }];
}

describe("initState / counting", () => {
  it("checks create/update rows, leaves unchanged unchecked", () => {
    const v = vendor()[0] as VendorTargetState;
    expect(v.menu.map((m) => m.checked)).toEqual([true, true, true, false]);
    expect(v.info.checked).toBe(true);
    // info + 3 menu rows
    expect(countChanges(vendor())).toBe(4);
  });

  it("unchecking reduces N", () => {
    const s = vendor();
    const v = s[0] as VendorTargetState;
    const next = [{ ...v, menu: v.menu.map((m, i) => (i === 0 ? { ...m, checked: false } : m)) }];
    expect(countChanges(next)).toBe(3);
  });
});

describe("blocking issues", () => {
  it("blocks while a checked price is missing or out of range", () => {
    expect(hasBlockingIssues(vendor())).toBe(true);
    expect(hasBlockingIssues(withPrice(vendor(), "abc"))).toBe(true);
    expect(hasBlockingIssues(withPrice(vendor(), "0"))).toBe(true);
    expect(hasBlockingIssues(withPrice(vendor(), "100001"))).toBe(true);
    expect(hasBlockingIssues(withPrice(vendor(), "85"))).toBe(false);
  });

  it("unchecking the offending row unblocks", () => {
    const v = vendor()[0] as VendorTargetState;
    const next = [{ ...v, menu: v.menu.map((m) => (m.name === "雞腿飯" ? { ...m, checked: false } : m)) }];
    expect(hasBlockingIssues(next)).toBe(false);
  });

  it("flags empty / too long member names and duplicates", () => {
    const s = initState(ROSTER_RUN.proposal);
    const r = s[0] as Extract<TargetState, { type: "roster" }>;
    expect(hasBlockingIssues(s)).toBe(false);
    const set = (name: string) => [{ ...r, members: r.members.map((m, i) => (i === 0 ? { ...m, name } : m)) }];
    expect(hasBlockingIssues(set(""))).toBe(true);
    expect(hasBlockingIssues(set("x".repeat(51)))).toBe(true);
    expect(hasBlockingIssues(set("李大華"))).toBe(true);
  });
});

describe("buildApplyBody", () => {
  it("sends checked rows with edited values, never unchanged rows", () => {
    const { accepted } = buildApplyBody(withPrice(vendor(), "85"));
    expect(accepted).toEqual([
      {
        type: "vendor",
        vendor: { kind: "existing", existingId: "v1" },
        info: { phone: "02-1234", address: "台北市", note: undefined },
        menu: [
          { name: "滷肉飯", price: 60, description: undefined, category: undefined },
          { name: "排骨飯", price: 105, description: undefined, category: undefined },
          { name: "雞腿飯", price: 85, description: undefined, category: undefined },
        ],
      },
    ]);
  });

  it("omits info when unchecked and skips targets without changes", () => {
    const v = withPrice(vendor(), "85")[0] as VendorTargetState;
    const { accepted } = buildApplyBody([{ ...v, info: { ...v.info, checked: false } }]);
    expect(accepted[0]).not.toHaveProperty("info");
    const none = { ...v, info: { ...v.info, checked: false }, menu: v.menu.map((m) => ({ ...m, checked: false })) };
    expect(buildApplyBody([none]).accepted).toEqual([]);
  });

  it("builds roster changes with the edited new name", () => {
    const s = initState(ROSTER_RUN.proposal);
    const r = s[0] as Extract<TargetState, { type: "roster" }>;
    const { accepted, map } = buildApplyBody([{ ...r, ref: { kind: "new", name: " 新業務部 " } }]);
    expect(accepted).toEqual([
      {
        type: "roster",
        roster: { kind: "new", name: "新業務部" },
        members: [
          { name: "王小明", note: "業務" },
          { name: "李大華", note: "財務" },
        ],
      },
    ]);
    expect(map).toEqual([{ target: 0, members: [0, 1], menu: [] }]);
  });

  it("new vendor sends its name in vendor ref and info", () => {
    const v = vendor()[0] as VendorTargetState;
    const nv: VendorTargetState = {
      ...v,
      ref: { kind: "new", name: "舊名" },
      info: { ...v.info, name: "新店", checked: true },
    };
    const { accepted } = buildApplyBody(withPrice([nv], "85"));
    expect(accepted[0]).toMatchObject({
      vendor: { kind: "new", name: "新店" },
      info: { name: "新店" },
    });
  });
});

describe("mapServerErrors", () => {
  it("maps accepted paths back to UI rows via the body map", () => {
    // only rows 0, 2 of the UI menu were sent -> accepted menu[1] is UI row 2
    const map = [{ target: 0, members: [], menu: [0, 2] }];
    const out = mapServerErrors(
      [
        { path: "accepted[0].menu[1].price", message: "價格不正確" },
        { path: "accepted[0].info.phone", message: "電話太長" },
        { path: "accepted[0].vendor", message: "找不到這間店家" },
        { path: "weird", message: "其他" },
      ],
      map,
    );
    expect(out.rows[rowKey(0, "menu", 2)]).toEqual([{ field: "price", message: "價格不正確" }]);
    expect(out.rows[rowKey(0, "info", 0)]).toEqual([{ field: "phone", message: "電話太長" }]);
    expect(out.rows[rowKey(0, "target", 0)]).toEqual([{ field: "vendor", message: "找不到這間店家" }]);
    expect(out.unmapped).toEqual(["其他"]);
  });
});

describe("issueMessage", () => {
  it("is zh-TW for each issue", () => {
    expect(issueMessage("price_missing")).toBe("請填寫價格");
    expect(issueMessage("duplicate_in_input")).toMatch(/重複/);
    expect(issueMessage("name_too_long")).toMatch(/50/);
  });
});
