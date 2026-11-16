/** Rust is authoritative for Kakao window title ↔ client mapping.
 * Window capture, React suggestions, and explicit selection stay in the UI.
 */
import { businessCoreCall } from "./tauri-bridge";
import type { DockWindow, State } from "./types";

export async function nativeRememberedTitleClient(
  state: State, windows: DockWindow[],
): Promise<string | undefined> {
  const { clientId } = await businessCoreCall<{ clientId: string | null }>(
    "catalog_clients", "remembered_title_client", { state, windows },
  );
  return clientId ?? undefined;
}

/** Only Rust may select a dealer from a verified current chat or an
 * explicitly labeled sender in the order. Receiver names are not dealers. */
export async function nativeAutoReceptionClient(
  state: State,
  windows: DockWindow[],
  raw: string,
  activeChatHandle?: string,
): Promise<{clientId: string | null; source: "KAKAO_REMEMBERED" | "KAKAO_TITLE" | "RAW_EXPLICIT" | "NONE" | "CONFLICT"; detail: string; chatTitle?: string; confirmedMatchCount?: number}> {
  return businessCoreCall("catalog_clients", "auto_match_reception_client", {
    state, windows, raw, activeChatHandle: activeChatHandle ?? "",
  });
}

export async function nativeRememberTitle(
  state: State, windows: DockWindow[], handle: string,
  clientId: string, actor: string,
): Promise<State> {
  const { state: next } = await businessCoreCall<{ state: State }>(
    "catalog_clients", "remember_title_client", {
      state, windows, handle, clientId, actor, now: new Date().toISOString(),
    },
  );
  return next;
}
