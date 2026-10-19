import { createBrowserRouter, createMemoryRouter, type RouteObject } from "react-router";
import { NotFoundPage } from "./NotFoundPage";
import { PublicLayout } from "./PublicLayout";
import { RouteFallback } from "./RouteFallback";

/** Route-level code splitting: each feature page is its own chunk, loaded on navigation. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const page = (load: () => Promise<any>, name: string) => async () => ({
  Component: (await load())[name],
});

export const routes: RouteObject[] = [
  {
    element: <PublicLayout />,
    HydrateFallback: RouteFallback,
    children: [
      { path: "/login", lazy: page(() => import("@/features/auth/LoginPage"), "LoginPage") },
      { path: "/g/:token", lazy: page(() => import("@/features/public-order/PublicOrderPage"), "PublicOrderPage") },
    ],
  },
  {
    lazy: page(() => import("./AdminLayout"), "AdminLayout"),
    HydrateFallback: RouteFallback,
    children: [
      { path: "/", lazy: page(() => import("@/features/group-orders/DashboardPage"), "DashboardPage") },
      { path: "/rosters", lazy: page(() => import("@/features/rosters/RostersPage"), "RostersPage") },
      { path: "/delivery-addresses", lazy: page(() => import("@/features/delivery-addresses/DeliveryAddressesPage"), "DeliveryAddressesPage") },
      { path: "/rosters/:id", lazy: page(() => import("@/features/rosters/RosterDetailPage"), "RosterDetailPage") },
      { path: "/vendors", lazy: page(() => import("@/features/vendors/VendorsPage"), "VendorsPage") },
      { path: "/vendors/:id", lazy: page(() => import("@/features/vendors/VendorDetailPage"), "VendorDetailPage") },
      { path: "/group-orders", lazy: page(() => import("@/features/group-orders/GroupOrdersPage"), "GroupOrdersPage") },
      { path: "/group-orders/new", lazy: page(() => import("@/features/group-orders/NewGroupOrderPage"), "NewGroupOrderPage") },
      { path: "/group-orders/:id", lazy: page(() => import("@/features/group-orders/GroupOrderDetailPage"), "GroupOrderDetailPage") },
      { path: "/ai-import/:runId", lazy: page(() => import("@/features/ai-import/AiImportPage"), "AiImportPage") },
    ],
  },
  { path: "*", element: <NotFoundPage /> },
];

export const createAppRouter = () => createBrowserRouter(routes);
export const createTestRouter = (initialEntries: string[]) =>
  createMemoryRouter(routes, { initialEntries });
