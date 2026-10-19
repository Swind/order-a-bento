import { Plus, Users } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { errorMessage, fieldError } from "@/shared/api/errors";
import { ImportButton } from "@/features/ai-import/ImportDialog";
import { FieldError } from "@/shared/components/FieldError";
import { formatUpdatedAt } from "@/shared/lib/format";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { Textarea } from "@/shared/ui/textarea";
import { useCreateRoster, useRosters } from "./hooks";
import { parseMemberLines } from "./parseMemberLines";
import { ScopeBadge, ScopeSelect } from "@/features/sharing-groups/ScopeSelect";
import { useSharingGroups } from "@/features/sharing-groups/hooks";

function CreateRosterDialog({ sharingGroupId }: { sharingGroupId?: string }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [members, setMembers] = useState("");
  const [nameError, setNameError] = useState<string | null>(null);
  const navigate = useNavigate();
  const create = useCreateRoster();

  function submit(e: FormEvent) {
    e.preventDefault();
    setNameError(null);
    if (!name.trim()) {
      setNameError("請輸入名單名稱");
      return;
    }
    create.mutate(
      { name: name.trim(), members: parseMemberLines(members), ...(sharingGroupId ? { sharingGroupId } : {}) },
      {
        onSuccess: (data) => {
          toast.success("已建立名單");
          setOpen(false);
          setName("");
          setMembers("");
          navigate(`/rosters/${data.id}`);
        },
        onError: (err) => {
          const f = fieldError(err, ["name"]);
          if (f) setNameError(f.message);
          else toast.error(errorMessage(err));
        },
      },
    );
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button />}>
        <Plus />
        新增名單
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>新增名單</DialogTitle>
            <DialogDescription>可一併貼上成員，一行一位。</DialogDescription>
            <ScopeBadge sharingGroupId={sharingGroupId} />
            <p className="text-sm text-muted-foreground">建立後無法變更資料範圍。</p>
          </DialogHeader>
          <div className="space-y-1.5">
            <Label htmlFor="roster-name">名單名稱</Label>
            <Input
              id="roster-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={!!nameError}
              autoComplete="off"
            />
            <FieldError message={nameError} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="roster-members">初始成員（選填）</Label>
            <Textarea
              id="roster-members"
              rows={5}
              value={members}
              onChange={(e) => setMembers(e.target.value)}
              placeholder={"王小明, 業務部\n李大華"}
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={create.isPending}>
              建立
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function RostersPage() {
  const [search, setSearch] = useSearchParams();
  const scope = search.get("sharingGroupId") ?? "";
  const setScope = (value: string) => setSearch(value ? { sharingGroupId: value } : {});
  const groups = useSharingGroups();
  const rosters = useRosters(scope || undefined);
  const canManage = !scope || (!groups.isError && groups.data?.find((group) => group.id === scope)?.role === "admin");
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-2">
        <h1 className="text-2xl font-bold">名單</h1>
        {canManage && <CreateRosterDialog key={scope} sharingGroupId={scope || undefined} />}
      </div>
      <ScopeSelect value={scope} onChange={setScope} />
      {canManage ? <ImportButton key={scope} label="📷 從照片建立名單" mode="roster" sharingGroupId={scope || undefined} /> : <p className="text-sm text-muted-foreground">唯讀：可查看並選用開團，只有群組管理員可修改。</p>}
      {rosters.isPending && (
        <div className="space-y-3" aria-busy="true">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      )}
      {rosters.isError && (
        <div className="space-y-2">
          <p role="alert" className="text-destructive">
            {errorMessage(rosters.error)}
          </p>
          <Button variant="outline" onClick={() => rosters.refetch()}>
            重試
          </Button>
        </div>
      )}
      {rosters.data?.length === 0 && (
        <div className="flex flex-col items-center gap-2 rounded-xl border border-dashed py-12 text-center text-muted-foreground">
          <Users className="size-8" />
          <p>還沒有名單，按「新增名單」開始建立。</p>
        </div>
      )}
      <ul className="grid gap-3 sm:grid-cols-2">
        {!rosters.isError && rosters.data?.map((r) => (
          <li key={r.id}>
            <Link to={`/rosters/${r.id}`} className="block rounded-xl focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50">
              <Card className="transition-colors hover:bg-muted/50">
                <CardContent className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-base font-medium">{r.name}</p>
                    <p className="text-xs text-muted-foreground">{scope ? `共用群組：${groups.data?.find((group) => group.id === scope)?.name ?? scope}` : "個人"}{r.canManage === false ? " · 唯讀" : ""}</p>
                    <p className="text-sm text-muted-foreground">
                      {formatUpdatedAt(r.updatedAt)}更新
                    </p>
                  </div>
                  <span className="shrink-0 text-sm">{r.memberCount} 人</span>
                </CardContent>
              </Card>
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}
