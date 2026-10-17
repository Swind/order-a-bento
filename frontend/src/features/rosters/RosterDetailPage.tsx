import { Check, Pencil, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { toast } from "sonner";
import { ApiError } from "@/shared/api/client";
import { errorMessage, fieldError } from "@/shared/api/errors";
import type { Member } from "@/shared/api/types";
import { ImportButton } from "@/features/ai-import/ImportDialog";
import { ConfirmDeleteButton } from "@/shared/components/ConfirmDeleteButton";
import { FieldError } from "@/shared/components/FieldError";
import { ReportSummary, reportText } from "@/shared/components/ReportSummary";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { Textarea } from "@/shared/ui/textarea";
import {
  useDeleteMember,
  useDeleteRoster,
  usePatchMember,
  useRenameRoster,
  useRoster,
  useUpsertMembers,
} from "./hooks";
import { parseMemberLines } from "./parseMemberLines";

function RosterTitle({ id, name }: { id: string; name: string }) {
  const navigate = useNavigate();
  const rename = useRenameRoster(id);
  const del = useDeleteRoster(id);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(name);
  const [error, setError] = useState<string | null>(null);

  function save(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const next = value.trim();
    if (!next) return setError("請輸入名單名稱");
    if (next === name) return setEditing(false);
    rename.mutate(next, {
      onSuccess: () => {
        toast.success("已更新名稱");
        setEditing(false);
      },
      onError: (err) => {
        const f = fieldError(err, ["name"]);
        if (f) setError(f.message);
        else toast.error(errorMessage(err));
      },
    });
  }

  return (
    <div className="space-y-1">
      <div className="flex flex-wrap items-center gap-2">
        {editing ? (
          <form onSubmit={save} className="flex min-w-0 flex-1 items-center gap-1">
            <Input
              aria-label="名單名稱"
              value={value}
              autoFocus
              onChange={(e) => setValue(e.target.value)}
              aria-invalid={!!error}
              className="min-w-0 flex-1"
            />
            <Button type="submit" size="icon" aria-label="儲存名稱" disabled={rename.isPending}>
              <Check />
            </Button>
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label="取消"
              onClick={() => {
                setEditing(false);
                setError(null);
              }}
            >
              <X />
            </Button>
          </form>
        ) : (
          <>
            <h1 className="min-w-0 break-words text-2xl font-bold">{name}</h1>
            <Button
              variant="ghost"
              size="icon"
              aria-label="重新命名"
              onClick={() => {
                setValue(name);
                setEditing(true);
              }}
            >
              <Pencil />
            </Button>
          </>
        )}
        <div className="ml-auto">
          <ConfirmDeleteButton
            label="刪除名單"
            showLabel
            pending={del.isPending}
            onConfirm={() =>
              del.mutate(undefined, {
                onSuccess: () => {
                  toast.success("已刪除名單");
                  navigate("/rosters", { replace: true });
                },
                onError: (err) => toast.error(errorMessage(err)),
              })
            }
          />
        </div>
      </div>
      <FieldError message={error} />
    </div>
  );
}

function MemberRow({
  rosterId,
  member,
  highlighted,
}: {
  rosterId: string;
  member: Member;
  highlighted: boolean;
}) {
  const patch = usePatchMember(rosterId);
  const del = useDeleteMember(rosterId);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(member.name);
  const [note, setNote] = useState(member.note ?? "");
  const [nameError, setNameError] = useState<string | null>(null);

  function save(e: FormEvent) {
    e.preventDefault();
    setNameError(null);
    const nextName = name.trim();
    const nextNote = note.trim();
    if (!nextName) return setNameError("請輸入姓名");
    const body: { name?: string; note?: string | null } = {};
    if (nextName !== member.name) body.name = nextName;
    if (nextNote !== (member.note ?? "")) body.note = nextNote === "" ? null : nextNote;
    if (Object.keys(body).length === 0) return setEditing(false);
    patch.mutate(
      { memberId: member.id, ...body },
      {
        onSuccess: () => {
          toast.success("已更新");
          setEditing(false);
        },
        onError: (err) => {
          const f = fieldError(err, ["name"]);
          if (f) setNameError(f.message);
          else toast.error(errorMessage(err));
        },
      },
    );
  }

  if (editing) {
    return (
      <li className="rounded-lg border p-3">
        <form onSubmit={save} className="space-y-2">
          <div className="space-y-1">
            <Label htmlFor={`m-name-${member.id}`}>姓名</Label>
            <Input
              id={`m-name-${member.id}`}
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={!!nameError}
              autoFocus
            />
            <FieldError message={nameError} />
          </div>
          <div className="space-y-1">
            <Label htmlFor={`m-note-${member.id}`}>備註</Label>
            <Input
              id={`m-note-${member.id}`}
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
          <div className="flex gap-2">
            <Button type="submit" size="sm" disabled={patch.isPending}>
              儲存
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => {
                setEditing(false);
                setName(member.name);
                setNote(member.note ?? "");
                setNameError(null);
              }}
            >
              取消
            </Button>
          </div>
        </form>
      </li>
    );
  }

  return (
    <li
      className={cn(
        "flex items-center gap-2 rounded-lg border px-3 py-2 transition-colors duration-700",
        highlighted && "border-primary bg-primary/10",
      )}
      data-highlighted={highlighted || undefined}
    >
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">{member.name}</p>
        {member.note && <p className="truncate text-sm text-muted-foreground">{member.note}</p>}
      </div>
      <Button
        variant="ghost"
        size="icon"
        aria-label={`編輯 ${member.name}`}
        onClick={() => setEditing(true)}
      >
        <Pencil />
      </Button>
      <ConfirmDeleteButton
        label={`刪除 ${member.name}`}
        pending={del.isPending}
        onConfirm={() =>
          del.mutate(member.id, {
            onSuccess: () => toast.success("已刪除成員"),
            onError: (err) => toast.error(errorMessage(err)),
          })
        }
      />
    </li>
  );
}

function BatchAdd({ id, onAffected }: { id: string; onAffected: (ids: string[]) => void }) {
  const upsert = useUpsertMembers(id);
  const [text, setText] = useState("");
  const [summary, setSummary] = useState<string | null>(null);
  const parsed = parseMemberLines(text);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (parsed.length === 0) return;
    upsert.mutate(parsed, {
      onSuccess: ({ report }) => {
        setSummary(reportText(report));
        setText("");
        onAffected(
          report.outcomes.filter((o) => o.outcome !== "unchanged").map((o) => o.memberId),
        );
      },
      onError: (err) => {
        setSummary(null);
        toast.error(errorMessage(err));
      },
    });
  }

  return (
    <form onSubmit={submit} className="space-y-2">
      <Label htmlFor="batch-members">批次新增成員</Label>
      <Textarea
        id="batch-members"
        rows={5}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={"一行一位，備註用逗號或 Tab 分隔\n王小明, 業務部\n李大華，會計"}
      />
      <div className="flex flex-wrap items-center gap-2">
        <Button type="submit" disabled={parsed.length === 0 || upsert.isPending}>
          新增 {parsed.length} 位
        </Button>
        <ImportButton label="📷 從照片匯入" mode="roster" targetRosterId={id} />
      </div>
      {summary && <ReportSummary text={summary} />}
    </form>
  );
}

export function RosterDetailPage() {
  const { id = "" } = useParams();
  const roster = useRoster(id);
  const [highlight, setHighlight] = useState<Set<string>>(new Set());
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  function flash(ids: string[]) {
    setHighlight(new Set(ids));
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHighlight(new Set()), 3000);
  }

  if (roster.isPending) {
    return (
      <div className="space-y-3" aria-busy="true">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-32 w-full" />
      </div>
    );
  }
  if (roster.isError) {
    const notFound = roster.error instanceof ApiError && roster.error.status === 404;
    return (
      <div className="space-y-2">
        <p role="alert" className="text-destructive">
          {errorMessage(roster.error)}
        </p>
        {notFound ? (
          <Link to="/rosters" className="text-primary underline">
            回到名單列表
          </Link>
        ) : (
          <Button variant="outline" onClick={() => roster.refetch()}>
            重試
          </Button>
        )}
      </div>
    );
  }

  const data = roster.data;
  const members = [...data.members].sort((a, b) => a.sortOrder - b.sortOrder);
  return (
    <div className="space-y-6">
      <Link to="/rosters" className="text-sm text-muted-foreground hover:underline">
        ← 名單
      </Link>
      <RosterTitle key={data.name} id={id} name={data.name} />
      <BatchAdd id={id} onAffected={flash} />
      <section className="space-y-2" aria-label="成員">
        <h2 className="text-lg font-semibold">成員（{members.length}）</h2>
        {members.length === 0 ? (
          <p className="rounded-xl border border-dashed py-8 text-center text-muted-foreground">
            還沒有成員，從上方貼上名單吧。
          </p>
        ) : (
          <ul className="space-y-2">
            {members.map((m) => (
              <MemberRow
                key={`${m.id}-${m.name}-${m.note ?? ""}`}
                rosterId={id}
                member={m}
                highlighted={highlight.has(m.id)}
              />
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
