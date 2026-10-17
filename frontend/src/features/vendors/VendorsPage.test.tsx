import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { ME, problem, server } from "@/test/server";
import { renderApp } from "@/test/render";

beforeEach(() =>
  server.use(
    http.get("*/api/v1/me", () => HttpResponse.json(ME)),
    http.get("*/api/v1/vendors", () => HttpResponse.json([])),
  ),
);

describe("VendorsPage create", () => {
  it("shows a link to the existing vendor on 409 vendor_name_taken", async () => {
    server.use(
      http.post("*/api/v1/vendors", () =>
        problem(409, "vendor_name_taken", { field: "name", existingId: "v-existing" }),
      ),
    );
    renderApp("/vendors");
    await userEvent.click(await screen.findByRole("button", { name: "新增店家" }));
    await userEvent.type(await screen.findByLabelText("店家名稱"), "好吃便當");
    await userEvent.click(screen.getByRole("button", { name: "建立" }));
    expect(await screen.findByText("已有同名店家")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "前往既有店家" })).toHaveAttribute(
      "href",
      "/vendors/v-existing",
    );
  });
});
