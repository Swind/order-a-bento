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
import { useAuthConfig, useDevLogin, useMe, usePasswordLogin } from "./hooks";

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
    <form onSubmit={submit} className="space-y-3" aria-label="開發用登入">
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

function PasswordLoginForm({ returnTo }: { returnTo: string }) {
  const navigate = useNavigate();
  const login = usePasswordLogin();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    login.mutate(
      { email: email.trim(), password },
      {
        onSuccess: () => navigate(returnTo, { replace: true }),
        // invalid_credentials and rate_limited both map to a friendly zh-TW message.
        onError: (err) => setError(errorMessage(err)),
      },
    );
  }

  return (
    <form onSubmit={submit} className="space-y-3" aria-label="本機帳號登入">
      <div className="space-y-1.5">
        <Label htmlFor="login-email">Email</Label>
        <Input
          id="login-email"
          type="email"
          autoComplete="username"
          inputMode="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          aria-invalid={!!error}
        />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="login-password">密碼</Label>
        <Input
          id="login-password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          aria-invalid={!!error}
        />
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <Button type="submit" className="w-full" disabled={login.isPending}>
        登入
      </Button>
    </form>
  );
}

function OidcLoginButton({ returnTo }: { returnTo: string }) {
  return (
    <Button
      variant="outline"
      className="w-full"
      onClick={() => {
        window.location.assign(`/auth/oidc/login?returnTo=${encodeURIComponent(returnTo)}`);
      }}
    >
      使用單一登入（OIDC）
    </Button>
  );
}

/** Only the enabled methods, separated by dividers; an explanation when none is. */
function LoginMethods({
  returnTo,
  password,
  oidc,
  dev,
}: {
  returnTo: string;
  password: boolean;
  oidc: boolean;
  dev: boolean;
}) {
  const sections = [
    oidc && <OidcLoginButton key="oidc" returnTo={returnTo} />,
    password && (
      <div key="password" className="space-y-3">
        <h2 className="text-sm font-semibold">管理員備援登入</h2>
        <PasswordLoginForm returnTo={returnTo} />
      </div>
    ),
    dev && <DevLoginForm key="dev" returnTo={returnTo} />,
  ].filter(Boolean);
  if (sections.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        目前沒有可用的登入方式，請聯絡系統管理員。
      </p>
    );
  }
  return (
    <>
      {sections.map((section, i) => (
        <div key={i} className={i > 0 ? "border-t pt-4" : undefined}>
          {section}
        </div>
      ))}
    </>
  );
}

function LoginScreen({ admin }: { admin: boolean }) {
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
        <CardDescription>{admin ? "本機管理員備援入口" : "請先登入主辦人帳號"}</CardDescription>
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
          <LoginMethods
            returnTo={returnTo}
            password={admin && config.data.passwordLoginEnabled}
            oidc={!admin && config.data.oidcEnabled}
            dev={!admin && config.data.devLoginEnabled}
          />
        )}
      </CardContent>
    </Card>
  );
}

export function LoginPage() {
  return <LoginScreen admin={false} />;
}

export function AdminLoginPage() {
  return <LoginScreen admin />;
}
