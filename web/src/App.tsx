import { lazy, Suspense, useEffect, useState } from "react";
import { api, HttpError } from "./api";
import { money } from "./cards";
import { HandList, ReplayView } from "./components/Replay";
import { inDiscord, startActivity } from "./discord";
import { Admin } from "./pages/Admin";
import { Lobby } from "./pages/Lobby";
import { TablePage } from "./pages/TablePage";
import type { HandListing, Me, ServerConfig } from "./protocol";

// Dev-only page; not part of production bundles.
const Gallery = import.meta.env.DEV
  ? lazy(() => import("./pages/Gallery").then((m) => ({ default: m.Gallery })))
  : () => null;

function useFourColor(): [boolean, (v: boolean) => void] {
  const [on, setOn] = useState(() => {
    try {
      return localStorage.getItem("fourColor") === "1";
    } catch {
      return false;
    }
  });
  useEffect(() => {
    document.body.classList.toggle("four-color", on);
    try {
      localStorage.setItem("fourColor", on ? "1" : "0");
    } catch {
      /* private mode */
    }
  }, [on]);
  return [on, setOn];
}

/** Hash routes keep every URL relative, which Discord's Activity proxy needs (web.md §13). */
function useRoute(): string {
  const [route, setRoute] = useState(window.location.hash.replace(/^#/, "") || "/");
  useEffect(() => {
    const on = () => setRoute(window.location.hash.replace(/^#/, "") || "/");
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}

function SignIn({ config }: { config: ServerConfig | null }) {
  const [name, setName] = useState("");
  return (
    <div className="signin card-panel">
      <h1>Dealer's Choice</h1>
      <p>Poker night with the group: every game from the Bible, play money only.</p>
      {config?.discord_client_id && (
        <a className="btn primary big" href="auth/discord/login">
          Sign in with Discord
        </a>
      )}
      {config?.dev_mode && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            window.location.href = `auth/dev?name=${encodeURIComponent(name)}`;
          }}
        >
          <p className="muted">Development sign-in (DEV_MODE):</p>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" required />
          <button className="btn">Sign in</button>
        </form>
      )}
    </div>
  );
}

function MyHands() {
  const [hands, setHands] = useState<HandListing[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  useEffect(() => {
    api.myHands().then(setHands, () => {});
  }, []);
  return (
    <div className="my-hands">
      <h2>My hands</h2>
      {open !== null ? <ReplayView id={open} onClose={() => setOpen(null)} /> : <HandList hands={hands} onOpen={setOpen} />}
    </div>
  );
}

export function App() {
  const route = useRoute();
  const [me, setMe] = useState<Me | null>(null);
  const [config, setConfig] = useState<ServerConfig | null>(null);
  const [state, setState] = useState<"loading" | "signin" | "ready" | "error">("loading");
  const [error, setError] = useState<string | null>(null);
  const [activityTable, setActivityTable] = useState<string | null>(null);
  const embedded = inDiscord();
  const [fourColor, setFourColor] = useFourColor();

  useEffect(() => {
    if (import.meta.env.DEV && route === "/dev/views") return;
    (async () => {
      try {
        if (embedded) {
          const { table } = await startActivity();
          setActivityTable(table);
        }
        setMe(await api.me());
        setState("ready");
      } catch (e) {
        if (e instanceof HttpError && e.status === 401) {
          setConfig(await api.config().catch(() => null));
          setState("signin");
        } else {
          setError(e instanceof Error ? e.message : String(e));
          setState("error");
        }
      }
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Refresh the bankroll when the route changes.
  useEffect(() => {
    if (state === "ready") api.me().then(setMe, () => {});
  }, [route, state]);

  if (import.meta.env.DEV && route === "/dev/views")
    return (
      <Suspense fallback={<p className="muted center">Loading…</p>}>
        <Gallery />
      </Suspense>
    );
  if (state === "loading") return <p className="muted center">Loading…</p>;
  if (state === "error") return <p className="error center">{error}</p>;
  if (state === "signin" || !me) return <SignIn config={config} />;

  let page;
  const table = activityTable ?? (route.startsWith("/t/") ? decodeURIComponent(route.slice(3)) : null);
  if (table) page = <TablePage id={table} me={me} embedded={embedded} />;
  else if (route === "/hands") page = <MyHands />;
  else if (route === "/admin" && me.is_admin) page = <Admin />;
  else page = <Lobby me={me} />;

  return (
    <div className={`app${embedded ? " embedded" : ""}`}>
      {!embedded && (
        <header className="top">
          <a href="#/" className="brand">
            Dealer's Choice
          </a>
          <span className="me">
            {me.name} · {money(me.bankroll)}
          </span>
          <label className="toggle" title="Four-colour deck: clubs green, diamonds blue">
            <input type="checkbox" checked={fourColor} onChange={(e) => setFourColor(e.target.checked)} /> 4-colour
          </label>
          <button className="link" onClick={() => void api.logout().then(() => window.location.reload())}>
            Sign out
          </button>
        </header>
      )}
      <main>{page}</main>
    </div>
  );
}
