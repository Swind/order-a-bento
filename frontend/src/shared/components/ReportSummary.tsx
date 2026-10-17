interface Report {
  created: number;
  updated: number;
  unchanged: number;
}

export function reportText(r: Report): string {
  return `新增 ${r.created} 位、更新 ${r.updated} 位、未變更 ${r.unchanged} 位`;
}

export function itemReportText(r: Report): string {
  return `新增 ${r.created} 項、更新 ${r.updated} 項、未變更 ${r.unchanged} 項`;
}

export function ReportSummary({ text }: { text: string }) {
  return (
    <p role="status" className="rounded-md bg-muted px-3 py-2 text-sm">
      {text}
    </p>
  );
}
