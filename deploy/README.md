# Deploying Dealers Choice (web)

Status: built (`crates/server`, `web/`; `docs/tech/web.md`). Part 1 creates the Discord application, Part 2 sets
up automatic deploys through your self-hosted GitHub runner (or by hand), Part 3 covers updates and backups, and
Part 4 turns on play inside Discord.

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

## Part 2 — Automatic deploys from GitHub (your self-hosted runner)

Every push to `main` runs CI on GitHub, then the **deploy** workflow (`.github/workflows/deploy.yml`) runs on your
own server through a self-hosted GitHub Actions runner: it backs up the database, builds the image, restarts the
app, checks that the new image is the one running, and smoke-tests it (health, page, sign-in required, no dev
login). It never runs for pull requests, so unreviewed code can't execute on your server. You can also run it by
hand: GitHub → Actions → deploy → **Run workflow**.

This follows the same pattern as your `engineering-gym` deploy on the Docker VM: one runner per repo, a label to
target it, settings in a `.env` in your home directory, and the app on a local port. (Your website container
already holds ports 80/443 on that VM, so the bundled Caddy stays off there.)

### One-time setup on the server

1. **Register a runner for this repo.** Runners are per repo, so the ones registered to `Web-Server` and
   `engineering-gym` can't pick up this repo's jobs. On GitHub: this repo → **Settings → Actions → Runners → New
   self-hosted runner → Linux x64**. On the VM, as the same user that runs the other runners (it must be in the
   `docker` group), follow the page's *Download* commands in a new folder, then configure with the page's token
   and our label:

   ```bash
   mkdir ~/actions-runner-dealers-choice && cd ~/actions-runner-dealers-choice
   # …the page's curl + tar commands…
   ./config.sh --url https://github.com/zhoagland/Dealers-choice --token <TOKEN FROM THE PAGE> \
     --name DockerVM-dealers-choice --labels dealers-choice --unattended
   sudo ./svc.sh install && sudo ./svc.sh start
   ```

   It shows as **Idle** under Settings → Actions → Runners when it's ready.
2. **Settings file:** `mkdir -p ~/dealers-choice` and create `~/dealers-choice/.env` from `deploy/.env.example`:
   `PUBLIC_URL`, the four Discord values from Part 1, and `DC_HTTP_PORT` (default 8090; pick a free port: the
   gym uses 8088). `chmod 600 ~/dealers-choice/.env`. The workflow refuses to deploy with `DEV_MODE=1`.
3. **Make it reachable** (pick one):
   - **Cloudflare Tunnel** (no open ports; HTTPS by Cloudflare; WebSockets work): in Cloudflare Zero Trust →
     Networks → Tunnels, create a tunnel, add a public hostname (e.g. `poker.yourdomain.com`) with service
     `http://app:8080`, and put its token in `.env` as `CLOUDFLARE_TUNNEL_TOKEN=…` plus `DC_PROFILES=tunnel`.
   - **Your existing reverse proxy:** forward the hostname to `127.0.0.1:<DC_HTTP_PORT>`, including WebSocket
     upgrades on `/ws`.
   - **A dedicated host** with nothing on 80/443: `DOMAIN=…` and `DC_PROFILES=caddy` (automatic Let's Encrypt).

   Then set `PUBLIC_URL=https://<that hostname>` and add `https://<that hostname>/auth/discord/callback` as a
   redirect in the Discord Developer Portal (Part 1).
4. **Optional:** a repo secret `DISCORD_WEBHOOK` (a channel webhook URL) posts each deploy's result to Discord.
5. **First deploy:** merge to `main` (or Actions → deploy → Run workflow). Watch it under Actions; when it's green,
   open your hostname, sign in with Discord, create a table and share the invite link.

Everyone starts with $100 of play money. As an admin (`ADMIN_DISCORD_IDS`) you get an **Admin** link in the lobby
to deposit or reset bankrolls. The **settle** tab at a table shows each player's net result over the last 24
hours, so the group can settle up outside the app; the app never moves real money.

### Without the runner (by hand)

On any Linux server with Docker: `git clone <this repo> dealers-choice && cd dealers-choice`,
`cp deploy/.env.example deploy/.env` and fill it in, then
`docker compose -f deploy/docker-compose.yml up -d --build` (add `--profile tunnel` or `--profile caddy`).
For `caddy`, point an `A` record at the server and open ports 80 and 443. Check `https://<your-domain>/healthz`.
Logs: `docker compose -f deploy/docker-compose.yml logs -f app`.

### Trying it on your own computer first

No Discord needed: `DEV_MODE=1 cargo run -p server` in one terminal, `cd web && npm install && npm run dev` in
another, then open the address Vite prints and sign in by name (open a private window for a second player).
Or build once (`cd web && npm run build`) and open <http://localhost:8080> from `DEV_MODE=1 cargo run -p server`.

## Part 3 — Updates and backups

- **Update:** push to `main` (the deploy workflow does the rest), or by hand
  `git pull && docker compose -f deploy/docker-compose.yml up -d --build`. Restarting stops the tables; when the
  server comes back, each table reopens and any game that was interrupted is paid back to the start of the
  interrupted hand (table-stake stacks and carried pots go back to the players, D-WEB-6). Deploy between games if
  you can.
- **Backups:** every deploy first snapshots the database to `/data/backups/pre-deploy-<commit>.sqlite` (newest 10
  kept), and the server writes a nightly snapshot there too (newest 7 kept). Take one any time with
  `docker compose -p dealers-choice exec app dc-server --backup /data/backups/manual.sqlite`, and copy them out
  with `docker compose -p dealers-choice cp app:/data/backups ./backups`.
- **Restore a backup:** stop the app (`docker compose -p dealers-choice stop app`), copy the backup over the live
  database (`docker compose -p dealers-choice cp ./backups/<file>.sqlite app:/data/dc.sqlite`), then start it
  again (`docker compose -p dealers-choice start app`).
- **Roll back a bad deploy:** revert the commit on `main` (the workflow redeploys), or Actions → deploy → Run
  workflow on an older commit.
- If the client secret leaks, reset it (Part 1), update `.env`, and re-run the deploy workflow.

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
