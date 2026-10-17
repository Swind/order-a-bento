import { useState, type FormEvent } from "react";
import { Navigate, useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { errorMessage, fieldError, loginErrorMessage } from "@/shared/api/errors";
import { FieldError } from "@/shared/components/FieldError";
import { safeReturnTo } from "@/shared/lib/format";
import { Button } from "@/shared/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/shared/ui/card";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { useAuthConfig, useDevLogin, useMe } from "./hooks";

function DevLoginForm({ returnTo }: { returnTo: string }) {
  const navigate = useNavigate();
  const devLogin = useDevLogin();
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [emailError, setEmailError] = useState<string | null>(null);

  function submit(e: FormEvent) {
    e.preventDefault();
    setEmailError(null);
    devLogin.mutate(
      { email: email.trim(), displayName: displayName.trim() || undefined },
      {
        onSuccess: () => navigate(returnTo, { replace: true }),
        onError: (err) => {
          // The login endpoint reports a bad email as a generic validation failure.
          const f = fieldError(err, ["email"]);
          if (f) setEmailError(f.message);
          else toast.error(errorMessage(err));
        },
      },
    );
  }

  return (
    <form onSubmit={submit} className="space-y-3 border-t pt-4" aria-label="開發用登入">
      <h2 className="text-sm font-medium text-muted-foreground">開發用登入</h2>
      <div className="space-y-1.5">
        <Label htmlFor="dev-email">Email</Label>
        <Input
          id="dev-email"
          type="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          aria-invalid={!!emailError}
        />
        <FieldError message={emailError} />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="dev-name">顯示名稱（選填）</Label>
        <Input id="dev-name" value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
      </div>
      <Button type="submit" variant="outline" className="w-full" disabled={devLogin.isPending}>
        開發用登入
      </Button>
    </form>
  );
}

export function LoginPage() {
  const [params] = useSearchParams();
  const returnTo = safeReturnTo(params.get("returnTo"));
  const errorCode = params.get("error");
  const config = useAuthConfig();
  const me = useMe();

  if (me.data) return <Navigate to={returnTo} replace />;

  return (
    <Card className="mt-8">
      <CardHeader>
        <CardTitle className="text-2xl">🍱 訂便當</CardTitle>
        <CardDescription>請先登入主辦人帳號</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {errorCode && (
          <div role="alert" className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {loginErrorMessage(errorCode)}
          </div>
        )}
        {config.isPending && <Skeleton className="h-9 w-full" />}
        {config.isError && (
          <div className="space-y-2">
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(config.error)}
            </p>
            <Button variant="outline" onClick={() => config.refetch()}>
              重試
            </Button>
          </div>
        )}
        {config.data && (
          <>
            <div className="space-y-2">
              <Button
                className="w-full"
                disabled={!config.data.googleEnabled}
                onClick={() => {
                  window.location.assign(
                    `/auth/google/login?returnTo=${encodeURIComponent(returnTo)}`,
                  );
                }}
              >
                使用 Google 登入
              </Button>
              {!config.data.googleEnabled && (
                <p className="text-sm text-muted-foreground">
                  伺服器尚未設定 Google 登入，請聯絡管理員。
                </p>
              )}
            </div>
            {config.data.devLoginEnabled && <DevLoginForm returnTo={returnTo} />}
          </>
        )}
      </CardContent>
    </Card>
  );
}
