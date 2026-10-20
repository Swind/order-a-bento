import type { GroupOrderDetail, ParticipantRow } from "@/shared/api/types";

export const GROUP: GroupOrderDetail = {
  canManage: true,
  sharingGroupId: null,
  sharingGroupName: null,
  id: "g1",
  title: "週五午餐",
  deliveryAddress: "台北教室（台北市中正區 3 樓）",
  status: "open",
  deadlineAt: null,
  note: null,
  shareUrl: "https://x.test/g/old",
  vendor: { name: "好吃便當", phone: "02-1234-5678", address: null },
  participants: [
    { id: "p1", name: "王小明", sortOrder: 0 },
    { id: "p2", name: "陳美麗", sortOrder: 1 },
    { id: "p3", name: "李大華", sortOrder: 2 },
  ],
  offeredItems: [
    { id: "i1", name: "排骨飯", price: 100, sortOrder: 0, soldOut: false },
    { id: "i2", name: "雞腿飯", price: 110, sortOrder: 1, soldOut: false },
  ],
  createdAt: "2026-01-01T00:00:00Z",
  closedAt: null,
};

export function row(over: Partial<ParticipantRow>): ParticipantRow {
  return {
    id: "p1",
    name: "王小明",
    sortOrder: 0,
    amountDue: 100,
    items: [{ itemId: "i1", name: "排骨飯", quantity: 1, note: null }],
    orderNote: null,
    submittedAt: "2026-01-01T02:00:00Z",
    submissionCount: 1,
    pickedUp: false,
    pickedUpAt: null,
    paid: false,
    paidAt: null,
    paidAmount: null,
    balance: null,
    suspiciousDevice: false,
    deviceShort: null,
    ...over,
  };
}
