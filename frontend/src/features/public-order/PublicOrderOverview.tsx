import type { usePublicOrders } from "./hooks";
import { formatHHmm } from "@/shared/lib/datetime";
import { errorMessage } from "@/shared/api/errors";
import { Button } from "@/shared/ui/button";

export function PublicOrderOverview({ query }: { query: ReturnType<typeof usePublicOrders> }) {
  return <section aria-label="大家的已送出訂單" className="space-y-4">
    <p className="text-sm text-muted-foreground">持有此訂購連結的人都能查看已送出的姓名與餐點；不顯示備註、繳費及裝置資訊。</p>
    <div className="flex flex-wrap items-center justify-between gap-2">
      {query.dataUpdatedAt > 0 && <p className="text-sm text-muted-foreground">資料最後成功更新：<time dateTime={new Date(query.dataUpdatedAt).toISOString()}>{formatHHmm(new Date(query.dataUpdatedAt).toISOString())}</time></p>}
      <Button variant="outline" className="h-11" disabled={query.isFetching} onClick={() => void query.refresh()}>重新整理</Button>
    </div>
    {query.isFetching && <p role="status">{query.data ? "正在更新，暫顯示上次資料…" : "正在查詢已送出的訂單…"}</p>}
    {query.isError && <p role="alert">更新失敗，{query.data ? "目前顯示上次成功取得的舊資料。" : "請重新整理再試。"}{errorMessage(query.error)}</p>}
    {query.data && (query.data.orders.length === 0
      ? <p className="rounded-xl border bg-card p-4">目前還沒有人訂餐</p>
      : <ul className="space-y-3">{query.data.orders.map((order) => <li key={order.participantId} className="space-y-2 rounded-xl border bg-card p-4">
        <h2 className="break-words text-lg font-semibold">{order.name}</h2>
        <p className="text-xs text-muted-foreground">最後送出：{formatHHmm(order.submittedAt)}</p>
        <ul>{order.lines.map((line) => <li key={line.itemId} className="break-words">{line.name} ×{line.quantity}</li>)}</ul>
      </li>)}</ul>)}
  </section>;
}
