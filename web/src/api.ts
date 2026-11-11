import type {
  ActivityToken, AdminAccount, BankrollChange, CreateTable, GameInfo, HandListing, HandReplay, Me,
  SeatResult, ServerConfig, TableListing,
} from "./protocol";

/** Inside the Discord Activity the session travels as a bearer token, not a cookie (web.md §13). */
let bearer: string | null = null;
export function setBearer(token: string | null) {
  bearer = token;
}
export function getBearer(): string | null {
  return bearer;
}

export class HttpError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

/** Resolve an app path against the page's directory, so Discord's proxy mapping still applies. */
export function appUrl(path: string): string {
  return new URL(path.replace(/^\//, ""), document.baseURI).toString();
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (bearer) headers["authorization"] = `Bearer ${bearer}`;
  const r = await fetch(appUrl(path), {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  if (!r.ok) throw new HttpError(r.status, (await r.text()) || r.statusText);
  if (r.status === 204) return undefined as T;
  return (await r.json()) as T;
}

const enc = encodeURIComponent;

export const api = {
  config: () => call<ServerConfig>("GET", "/api/config"),
  me: () => call<Me>("GET", "/api/me"),
  games: () => call<GameInfo[]>("GET", "/api/games"),
  tables: () => call<TableListing[]>("GET", "/api/tables"),
  table: (id: string) => call<TableListing>("GET", `/api/tables/${enc(id)}`),
  createTable: (t: CreateTable) => call<TableListing>("POST", "/api/tables", t),
  closeTable: (id: string) => call<void>("POST", `/api/tables/${enc(id)}/close`),
  tableHands: (id: string) => call<HandListing[]>("GET", `/api/tables/${enc(id)}/hands`),
  settle: (id: string, since?: number) =>
    call<SeatResult[]>("GET", `/api/tables/${enc(id)}/settle${since !== undefined ? `?since=${since}` : ""}`),
  myHands: () => call<HandListing[]>("GET", "/api/hands"),
  hand: (id: number) => call<HandReplay>("GET", `/api/hands/${id}`),
  adminAccounts: () => call<AdminAccount[]>("GET", "/api/admin/accounts"),
  adminBankroll: (c: BankrollChange) => call<AdminAccount>("POST", "/api/admin/bankroll", c),
  logout: () => call<void>("POST", "/auth/logout"),
  activityToken: (code: string, instance: string) =>
    call<ActivityToken>("POST", "/api/discord/token", { code, instance }),
};
