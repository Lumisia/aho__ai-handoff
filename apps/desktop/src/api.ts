import { invoke } from "@tauri-apps/api/core";
import type { CapsuleList, DashboardSnapshot, LogFile, ReadTextResult } from "./types";

export function getDashboardSnapshot(): Promise<DashboardSnapshot> {
  return invoke("get_dashboard_snapshot");
}

export function listCapsules(): Promise<CapsuleList> {
  return invoke("list_capsules");
}

export function readCapsule(path: string): Promise<ReadTextResult> {
  return invoke("read_capsule", { path });
}

export function readLogs(): Promise<LogFile[]> {
  return invoke("read_logs");
}
