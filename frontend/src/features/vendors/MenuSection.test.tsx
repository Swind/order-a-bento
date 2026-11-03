import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { expect, it } from "vitest";
import { server } from "@/test/server";
import { MenuSection } from "./MenuSection";

it("manually edits a menu price to 7.5 without truncation and rejects excess precision", async () => {
  let sent: unknown;
  const item = { id: "i1", name: "水餃", price: 8, description: null, category: null, archived: false, sortOrder: 0 };
  server.use(http.patch("*/api/v1/vendors/v1/menu-items/i1", async ({ request }) => { sent = await request.json(); return HttpResponse.json({ ...item, price: 7.5 }); }));
  const qc = new QueryClient();
  render(<QueryClientProvider client={qc}><MenuSection vendorId="v1" items={[item]} /></QueryClientProvider>);
  await userEvent.click(screen.getAllByRole("button", { name: "編輯 水餃" })[0]);
  const price = screen.getByLabelText("價格");
  expect(price).toHaveAttribute("inputmode", "decimal");
  expect(price).toHaveAttribute("step", "0.01");
  await userEvent.clear(price); await userEvent.type(price, "7.501");
  await userEvent.click(screen.getByRole("button", { name: "儲存" }));
  expect(await screen.findByText(/最多小數兩位/)).toBeVisible(); expect(sent).toBeUndefined();
  await userEvent.clear(price); await userEvent.type(price, "7.5");
  await userEvent.click(screen.getByRole("button", { name: "儲存" }));
  await waitFor(() => expect(sent).toEqual({ price: 7.5 }));
});
