// Dialog cleanup precedes the tab panel's hidden attribute update.
export function restoreOrderFocus(target: HTMLElement | null) {
  queueMicrotask(() => {
    if (!target?.isConnected) return;
    if (target.closest("[hidden]")) {
      target.closest('[data-slot="tabs"]')?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus({ preventScroll: true });
    } else target.focus({ preventScroll: true });
  });
  return false;
}
