import { api, setBearer } from "./api";

/** True when Discord is showing this page as an Activity (it adds `frame_id` to the URL). */
export function inDiscord(): boolean {
  return new URLSearchParams(window.location.search).has("frame_id");
}

/**
 * Discord Activity sign-in (web.md §13): the SDK's `authorize` gives a code, our server swaps it
 * for a Discord token (the client secret stays on the server) and a session for this app, and
 * names the table for this Activity instance, so everyone in the voice channel lands together.
 */
export async function startActivity(): Promise<{ table: string }> {
  const { DiscordSDK } = await import("@discord/embedded-app-sdk");
  const cfg = await api.config();
  if (!cfg.discord_client_id) throw new Error("The server has no Discord application configured.");
  const sdk = new DiscordSDK(cfg.discord_client_id);
  await sdk.ready();
  const { code } = await sdk.commands.authorize({
    client_id: cfg.discord_client_id,
    response_type: "code",
    state: "",
    prompt: "none",
    scope: ["identify", "guilds"],
  });
  const tok = await api.activityToken(code, sdk.instanceId);
  await sdk.commands.authenticate({ access_token: tok.access_token });
  setBearer(tok.session);
  return { table: tok.table };
}
