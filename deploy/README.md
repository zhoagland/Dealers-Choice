# Deploying Dealers Choice (web)

Status: the web server (`dc-server`) is planned, not built yet (`docs/tech/web.md`, milestones W1–W5). Part 1 can
be done now; it gives you the values the server will need. Parts 2–3 are filled in when W5 lands.

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

## Part 2 — Server and DNS (fill in at W5)

- Point an `A` record for your domain at the server's public IP; open ports 80 and 443 (Caddy needs both for
  Let's Encrypt).
- Install Docker Engine + the Compose plugin.
- `git clone` this repo (or copy `deploy/`), `cp deploy/.env.example deploy/.env`, fill in the values from
  Part 1, then `docker compose -f deploy/docker-compose.yml up -d`.
- Check `https://<your-domain>/healthz`, then sign in.

## Part 3 — Updating and backups (fill in at W5)

- Update: `docker compose pull && docker compose up -d` (waits for hands in progress, max 5 min).
- Backups: nightly SQLite snapshots in `data/backups/` (7 kept); restoring one is in the handbook.

## Optional — Play inside Discord (Activity)

If the group wants to launch tables from a Discord voice channel, the same application can also be an
**Activity**. This is a proposal (`docs/tech/web.md` §13), not part of v1; nothing to set up yet.
