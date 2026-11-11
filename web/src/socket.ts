import { appUrl, getBearer } from "./api";
import type { ClientMsg, ServerMsg } from "./protocol";

export type SocketState = "connecting" | "open" | "closed";

/**
 * One WebSocket per table tab, reconnecting with backoff. The server sends a full snapshot on
 * every (re)join, so a reconnect simply resumes the seat (FR-WB-6).
 */
export class TableSocket {
  private ws: WebSocket | null = null;
  private retry = 0;
  private stopped = false;
  private ping: number | undefined;

  constructor(
    private table: string,
    private onMessage: (m: ServerMsg) => void,
    private onState: (s: SocketState) => void,
  ) {
    this.connect();
  }

  private url(): string {
    const u = new URL(appUrl("/ws"));
    u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
    u.searchParams.set("table", this.table);
    const token = getBearer();
    if (token) u.searchParams.set("token", token);
    return u.toString();
  }

  private connect() {
    if (this.stopped) return;
    this.onState("connecting");
    const ws = new WebSocket(this.url());
    this.ws = ws;
    ws.onopen = () => {
      this.retry = 0;
      this.onState("open");
      this.ping = window.setInterval(() => this.send({ type: "Ping" }), 25_000);
    };
    ws.onmessage = (e) => {
      try {
        this.onMessage(JSON.parse(e.data as string) as ServerMsg);
      } catch {
        /* ignore malformed frames */
      }
    };
    ws.onclose = () => {
      window.clearInterval(this.ping);
      this.onState("closed");
      if (this.stopped) return;
      const delay = Math.min(10_000, 500 * 2 ** this.retry++);
      window.setTimeout(() => this.connect(), delay);
    };
  }

  send(m: ClientMsg) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(m));
  }

  close() {
    this.stopped = true;
    window.clearInterval(this.ping);
    this.ws?.close();
  }
}
