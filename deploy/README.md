# Deploying Dealers Choice (web)

Status: built (`crates/server`, `web/`; `docs/tech/web.md`). Part 1 creates the Discord application, Part 2 puts
the app on your server, Part 3 covers updates and backups, and the last part turns on play inside Discord.

Plan decisions this guide follows: Discord sign-in only, only members of your Discord server may sign in
(D-WEB-2), a Linux x86-64 server with Docker Compose, HTTPS by a bundled Caddy (OQ-11).

---

## Part 1 — Create the Discord application (do this now)

You need: a Discord account that can manage your Discord server, and the domain you'll host on (you can add the
redirect later if you don't have it yet).

1. Open the Discord Developer Portal: <https://discord.com/developers/applications> → **New Application**.
   Name it (e.g. "Dealers Choice"), accept the terms, **Create**.
2. **General Information**: optionally add an icon and description (people see them on the sign-in screen).
   Copy the **Application ID**. This is `DISCORD_CLIENT_ID`.
3. **OAuth2** (left menu):
   - **Client Secret** → **Reset Secret** → copy it now (Discord shows it once). This is
     `DISCORD_CLIENT_SECRET`. Treat it like a password: it goes only in the server's `.env`, never in git or chat.
   - **Redirects** → **Add Redirect**: `https://<your-domain>/auth/discord/callback` → **Save Changes**.
     For local testing (W2) also add `http://localhost:8080/auth/discord/callback`.
   - You don't need the URL generator or a bot. The app only asks for the `identify` and `guilds` scopes
     (your name and avatar, and the list of servers you're in, to check membership). It never reads messages.
4. Find your Discord server's id (`ALLOWED_GUILD_ID`):
   Discord → **User Settings → Advanced → Developer Mode: on**. Then right-click your server's icon →
   **Copy Server ID**.
5. Find your own Discord user id (`ADMIN_DISCORD_IDS`): with Developer Mode on, right-click your name →
   **Copy User ID**. Add other admins comma-separated.

Keep these five values somewhere private until deploy:

```env
PUBLIC_URL=https://<your-domain>
DISCORD_CLIENT_ID=<application id>
DISCORD_CLIENT_SECRET=<client secret>
ALLOWED_GUILD_ID=<server id>
ADMIN_DISCORD_IDS=<your user id>[,<other admin id>]
```

If the secret ever leaks: Developer Portal → OAuth2 → **Reset Secret**, update `.env`, restart the server.

## Part 2 — Server and DNS

You need: a Linux x86-64 server with a public IP, a domain you control, and Docker.

1. **DNS:** add an `A` record for your domain (e.g. `poker.example.com`) pointing at the server's IP. Open ports
   80 and 443 in the server's firewall; Caddy needs both to get the HTTPS certificate.
2. **Docker:** install Docker Engine and the Compose plugin (<https://docs.docker.com/engine/install/>). Check
   with `docker compose version`.
3. **Code:** `git clone <this repo> dealers-choice && cd dealers-choice`.
4. **Settings:** `cp deploy/.env.example deploy/.env`, then edit `deploy/.env`: set `DOMAIN` and `PUBLIC_URL` to
   your domain and paste the four values from Part 1. Keep `DEV_MODE=0`. The server refuses to start without the
   Discord values and `ALLOWED_GUILD_ID` (D-WEB-2).
5. **Start:** `docker compose -f deploy/docker-compose.yml up -d --build`. The first build takes several
   minutes (it compiles the Rust server and the web app).
6. **Check:** open `https://<your-domain>/healthz` (it says `ok`), then `https://<your-domain>/`, sign in with
   Discord, create a table and copy its invite link into your Discord channel.
7. Logs: `docker compose -f deploy/docker-compose.yml logs -f app`.

Everyone starts with $100 of play money. As an admin (`ADMIN_DISCORD_IDS`) you get an **Admin** link in the lobby
to deposit or reset bankrolls. The **settle** tab at a table shows each player's net result over the last 24
hours, so the group can settle up outside the app; the app never moves real money.

### Trying it on your own computer first

No Discord needed: `DEV_MODE=1 cargo run -p server` in one terminal, `cd web && npm install && npm run dev` in
another, then open the address Vite prints and sign in by name (open a private window for a second player).
Or build once (`cd web && npm run build`) and open <http://localhost:8080> from `DEV_MODE=1 cargo run -p server`.

## Part 3 — Updates and backups

- **Update:** `git pull && docker compose -f deploy/docker-compose.yml up -d --build`. Restarting stops the
  tables; when the server comes back, each table reopens and any game that was interrupted is paid back to the
  start of the interrupted hand (table-stake stacks and carried pots go back to the players, D-WEB-6). Update
  between games if you can.
- **Backups:** the server writes a snapshot of its database every night to `/data/backups` inside the `dc-data`
  volume and keeps the newest 7. To copy them out:
  `docker compose -f deploy/docker-compose.yml cp app:/data/backups ./backups`.
- **Restore a backup:** stop the app (`docker compose -f deploy/docker-compose.yml stop app`), copy the backup
  over the live database (`docker compose -f deploy/docker-compose.yml cp ./backups/dc-dayNNNNNN.sqlite
  app:/data/dc.sqlite`), then start it again (`... start app`).
- If the client secret leaks, reset it (Part 1) and restart the app.

## Part 4 — Play inside Discord (Activity, optional)

The same app runs as a Discord **Activity**: people in a voice channel press the Activities (rocket) button,
pick Dealers Choice, and everyone in that call lands at the same table (`docs/tech/web.md` §13).

1. Developer Portal → your application → **Activities → Settings**: turn on **Enable Activities**, and enable
   the platforms you want (desktop, mobile).
2. **Activities → URL Mappings**: add the root mapping `/` → `<your-domain>` (no `https://`).
3. **OAuth2 → Redirects** must have at least one entry (Part 1's is enough).
4. **Installation**: make sure the app can be added to your server (Guild Install), and add it to your server.
5. Join a voice channel, open Activities, choose the app. The first time, Discord asks each person to authorize.

Inside Discord the session travels as a bearer token rather than a cookie (Discord's iframe blocks third-party
cookies), and the lobby is skipped: one table per Activity instance, hosted by whoever opened it.
