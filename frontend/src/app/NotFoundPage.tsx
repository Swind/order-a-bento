import { Link } from "react-router";

export function NotFoundPage() {
  return (
    <div className="mx-auto flex min-h-dvh max-w-md flex-col items-center justify-center gap-4 p-4 text-center">
      <h1 className="text-2xl font-bold">找不到頁面</h1>
      <Link to="/" className="text-primary underline">回首頁</Link>
    </div>
  );
}
