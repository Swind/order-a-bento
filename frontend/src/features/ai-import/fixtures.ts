import type { ImportRun } from "@/shared/api/types";

export const AUTH_CONFIG = {
  aiDemo: false,
  aiImportEnabled: true,
  devLoginEnabled: false,
  oidcEnabled: true,
};

const base = {
  id: "run1",
  status: "proposed" as const,
  mode: "vendor" as const,
  targetRosterId: null,
  targetVendorId: "v1",
  instruction: null,
  summary: "辨識出「好吃便當」的 4 個品項，其中 1 個價格不清楚。",
  usage: { inputTokens: 1, outputTokens: 1 },
  model: "claude-test",
  error: null,
  createdAt: "2026-01-01T00:00:00Z",
  finishedAt: null,
  appliedAt: null,
};

export const VENDOR_RUN: ImportRun = {
  ...base,
  proposal: {
    warnings: ["『雞腿飯』價格被摺痕遮住，請確認"],
    targets: [
      {
        type: "vendor",
        vendor: { kind: "existing", existingId: "v1" },
        info: {
          action: "update",
          before: { name: "好吃便當", phone: null, address: null, note: null },
          after: { name: "好吃便當", phone: "02-1234", address: "台北市", note: null },
        },
        menu: [
          { action: "create", name: "滷肉飯", price: 60, description: null, category: null, confidence: "high", issues: [] },
          {
            action: "update",
            name: "排骨飯",
            before: { price: 100 },
            price: 105,
            description: null,
            category: null,
            confidence: "medium",
            issues: [],
          },
          {
            action: "create",
            name: "雞腿飯",
            price: null,
            description: null,
            category: null,
            confidence: "low",
            issues: ["price_missing"],
          },
          {
            action: "unchanged",
            name: "素食便當",
            before: { price: 90 },
            price: 90,
            description: null,
            category: null,
            confidence: "high",
            issues: [],
          },
        ],
      },
    ],
  },
};

export const ROSTER_RUN: ImportRun = {
  ...base,
  mode: "roster",
  targetVendorId: null,
  proposal: {
    warnings: [],
    targets: [
      {
        type: "roster",
        roster: { kind: "new", name: "業務部" },
        members: [
          { action: "create", name: "王小明", note: "業務", confidence: "high", issues: [] },
          { action: "update", name: "李大華", beforeNote: "會計", note: "財務", confidence: "high", issues: [] },
          { action: "unchanged", name: "陳美麗", note: null, confidence: "high", issues: [] },
        ],
      },
    ],
  },
};
