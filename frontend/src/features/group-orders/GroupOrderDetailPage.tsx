import { useParams, useSearchParams } from "react-router";
import { errorMessage } from "@/shared/api/errors";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { ChecklistTab } from "./ChecklistTab";
import { DetailsTab } from "./DetailsTab";
import { GroupHeader } from "./GroupHeader";
import { useGroupOrder } from "./hooks";
import { parseTab } from "./lib";
import { SharePanel } from "./SharePanel";
import { StatsTab } from "./StatsTab";

export function GroupOrderDetailPage() {
  const { id = "" } = useParams();
  const query = useGroupOrder(id);
  const [params, setParams] = useSearchParams();
  const tab = parseTab(params.get("tab"));

  if (query.isPending)
    return (
      <div className="space-y-3" aria-busy="true">
        <Skeleton className="h-8 w-2/3" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  if (query.isError)
    return (
      <div className="space-y-2">
        <p role="alert" className="text-destructive">
          {errorMessage(query.error)}
        </p>
        <Button variant="outline" className="h-11" onClick={() => query.refetch()}>
          重試
        </Button>
      </div>
    );

  const group = query.data;
  const triggerClass = "h-11 min-w-24 flex-none px-4 text-base";
  return (
    <div className="mx-auto max-w-2xl space-y-5">
      <GroupHeader group={group} />
      <SharePanel title={group.title} shareUrl={group.shareUrl} />

      <Tabs
        value={tab}
        onValueChange={(v) =>
          setParams(
            (prev) => {
              const next = new URLSearchParams(prev);
              next.set("tab", String(v));
              return next;
            },
            { replace: true },
          )
        }
      >
        {/* Own scroll container: the tab row never widens the page. */}
        <div className="max-w-full overflow-x-auto">
          <TabsList className="h-12 w-max min-w-full justify-start">
            <TabsTrigger value="stats" className={triggerClass}>
              統計
            </TabsTrigger>
            <TabsTrigger value="details" className={triggerClass}>
              明細
            </TabsTrigger>
            <TabsTrigger value="checklist" className={triggerClass}>
              領取＆繳費
            </TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="stats" className="text-base">
          <StatsTab group={group} />
        </TabsContent>
        <TabsContent value="details" className="text-base">
          <DetailsTab group={group} />
        </TabsContent>
        <TabsContent value="checklist" className="text-base">
          <ChecklistTab group={group} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
