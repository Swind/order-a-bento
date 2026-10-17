import { Outlet } from "react-router";

/** Chrome-less layout for public pages (login, participant order page). */
export function PublicLayout() {
  return (
    <div className="mx-auto min-h-dvh w-full max-w-md px-4 py-6 pb-[env(safe-area-inset-bottom)]">
      <Outlet />
    </div>
  );
}
