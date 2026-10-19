import { useQueryClient } from "@tanstack/react-query";
import { LogOut, Menu } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, NavLink, Outlet, useLocation, useNavigate } from "react-router";
import { toast } from "sonner";
import { useLogout, useMe } from "@/features/auth/hooks";
import { ApiError } from "@/shared/api/client";
import { errorMessage } from "@/shared/api/errors";
import type { Me } from "@/shared/api/types";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Skeleton } from "@/shared/ui/skeleton";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/shared/ui/sheet";

export const NAV_ITEMS = [
  { to: "/", label: "首頁", end: true },
  { to: "/rosters", label: "名單", end: false },
  { to: "/delivery-addresses", label: "送餐地址", end: false },
  { to: "/vendors", label: "店家", end: false },
  { to: "/sharing-groups", label: "共用群組", end: false },
  { to: "/group-orders", label: "開團紀錄", end: false },
] as const;

function LoginRedirect({ returnTo }: { returnTo: string }) {
  const url = `/auth/login?returnTo=${encodeURIComponent(returnTo)}`;
  useEffect(() => {
    // Full navigation: the server starts OIDC or falls back to /login.
    location.replace(url);
  }, [url]);
  return (
    <div className="mx-auto max-w-md space-y-4 p-6 text-center">
      <p role="status">正在前往登入頁面…</p>
      <Link to={`/login?returnTo=${encodeURIComponent(returnTo)}`} className="underline">
        管理員備援登入
      </Link>
    </div>
  );
}

function NavLinks({ onNavigate, className }: { onNavigate?: () => void; className?: string }) {
  return (
    <>
      {NAV_ITEMS.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          end={item.end}
          onClick={onNavigate}
          className={({ isActive }) =>
            cn(
              "rounded-md px-3 py-2 text-base font-medium transition-colors hover:bg-muted",
              isActive ? "bg-muted text-foreground" : "text-muted-foreground",
              className,
            )
          }
        >
          {item.label}
        </NavLink>
      ))}
    </>
  );
}

function UserMenu({ me }: { me: Me }) {
  const logout = useLogout();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const initial = (me.displayName || me.email).slice(0, 1).toUpperCase();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={<Button variant="ghost" className="h-9 gap-2 px-1.5" aria-label="帳號選單" />}
      >
        {me.avatarUrl ? (
          <img
            src={me.avatarUrl}
            alt=""
            referrerPolicy="no-referrer"
            className="size-7 rounded-full object-cover"
          />
        ) : (
          <span className="flex size-7 items-center justify-center rounded-full bg-primary text-xs text-primary-foreground">
            {initial}
          </span>
        )}
        <span className="hidden max-w-32 truncate sm:inline">{me.displayName}</span>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuGroup>
          <DropdownMenuLabel className="max-w-56 truncate">{me.email}</DropdownMenuLabel>
          <DropdownMenuItem
            onClick={() =>
              logout.mutate(undefined, {
                onSuccess: () => navigate("/login", { replace: true }),
                onError: (e) => {
                  toast.error(errorMessage(e));
                  queryClient.invalidateQueries();
                },
              })
            }
          >
            <LogOut />
            登出
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function LayoutSkeleton() {
  return (
    <div className="min-h-dvh bg-background" aria-busy="true" aria-label="載入中">
      <div className="flex h-14 items-center gap-4 border-b px-4">
        <Skeleton className="h-6 w-24" />
        <Skeleton className="ml-auto h-8 w-24" />
      </div>
      <div className="mx-auto max-w-5xl space-y-4 px-4 py-6">
        <Skeleton className="h-8 w-40" />
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    </div>
  );
}

export function AdminLayout() {
  const [open, setOpen] = useState(false);
  const location = useLocation();
  const me = useMe();

  if (me.isPending) return <LayoutSkeleton />;
  if (me.isError) {
    if (me.error instanceof ApiError && me.error.status === 401) {
      return <LoginRedirect returnTo={location.pathname + location.search} />;
    }
    return (
      <div className="mx-auto flex min-h-dvh max-w-md flex-col items-center justify-center gap-4 p-4 text-center">
        <p>{errorMessage(me.error)}</p>
        <Button onClick={() => me.refetch()}>重試</Button>
      </div>
    );
  }
  return (
    <div className="min-h-dvh bg-background">
      <header className="sticky top-0 z-40 border-b bg-background/95 pt-[env(safe-area-inset-top)] backdrop-blur">
        <div className="mx-auto flex h-14 max-w-5xl items-center gap-4 px-4">
          <span className="text-lg font-bold">🍱 訂便當</span>
          <nav aria-label="主選單" className="hidden items-center gap-1 md:flex">
            <NavLinks />
          </nav>
          <div className="ml-auto flex items-center gap-1">
            <UserMenu me={me.data} />
            <div className="md:hidden">
            <Sheet open={open} onOpenChange={setOpen}>
              <SheetTrigger render={<Button variant="outline" size="icon" aria-label="開啟選單" />}>
                <Menu />
              </SheetTrigger>
              <SheetContent side="right">
                <SheetHeader>
                  <SheetTitle>選單</SheetTitle>
                </SheetHeader>
                <nav aria-label="行動版選單" className="flex flex-col gap-1 px-4">
                  <NavLinks onNavigate={() => setOpen(false)} />
                </nav>
              </SheetContent>
            </Sheet>
            </div>
          </div>
        </div>
      </header>
      <main className="mx-auto w-full max-w-5xl px-4 py-6">
        <Outlet />
      </main>
    </div>
  );
}
