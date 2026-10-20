import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StatusBadge } from "./GroupCard";

afterEach(() => vi.useRealTimers());

describe("StatusBadge", () => {
  it.each([
    ["open", null, "進行中"],
    ["open", "2026-01-01T11:01:00+08:00", "進行中"],
    ["open", "2026-01-01T11:00:00+08:00", "已截止"],
    ["open", "2026-01-01T10:59:00+08:00", "已截止"],
    ["closed", "2026-01-01T10:59:00+08:00", "已關閉"],
    ["archived", "2026-01-01T10:59:00+08:00", "已封存"],
  ])("renders %s with deadline %s as %s", (status, deadlineAt, label) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T03:00:00Z"));
    render(<StatusBadge status={status} deadlineAt={deadlineAt} />);
    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it("updates after the deadline without reloading", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T03:00:00Z"));
    render(<StatusBadge status="open" deadlineAt="2026-01-01T11:00:30+08:00" />);
    expect(screen.getByText("進行中")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(30_000));
    expect(screen.getByText("已截止")).toBeInTheDocument();
  });
});
