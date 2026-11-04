import { api, unwrap } from "./client";

export async function listRosters(sharingGroupId?: string, signal?: AbortSignal) {
  return unwrap(await api.GET("/api/v1/rosters", { params: { query: { sharingGroupId } }, signal }));
}

export async function listVendors(sharingGroupId?: string, signal?: AbortSignal) {
  return unwrap(await api.GET("/api/v1/vendors", { params: { query: { sharingGroupId } }, signal }));
}
