// Signs in the way an MCP client or the app does: register, consent, Sign in with Google, then a
// code for a token. Google's token endpoint is faked; everything else is the real Worker.

import { exports } from "cloudflare:workers";
import { vi } from "vitest";

export const ORIGIN = "http://localhost";
export const REDIRECT = "https://claude.ai/api/mcp/auth_callback";

const worker = () => (exports as unknown as { default: Fetcher }).default;
export const call = (path: string, init?: RequestInit) => worker().fetch(`${ORIGIN}${path}`, { redirect: "manual", ...init });

const base64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");

/** The cookies a response set, as a Cookie header for the next request. */
const cookies = (response: Response) =>
  response.headers
    .getSetCookie()
    .map((c) => c.split(";")[0])
    .join("; ");

export const GOOGLE_CLIENT = "test-client.apps.googleusercontent.com";

/**
 * Google's token endpoint: it hands out an ID token for `email` only for the PKCE verifier that
 * matches the challenge the browser was sent with, as Google does. The signature isn't checked.
 */
export function fakeGoogle(email: string, sent: URL) {
  const claims = {
    iss: "https://accounts.google.com",
    aud: GOOGLE_CLIENT,
    exp: Math.floor(Date.now() / 1000) + 600,
    sub: "1234567890",
    email,
    email_verified: true,
    nonce: sent.searchParams.get("nonce"),
  };
  const idToken = `e30.${base64url(new TextEncoder().encode(JSON.stringify(claims)))}.sig`;
  const real = globalThis.fetch;
  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url !== "https://oauth2.googleapis.com/token") return real(input, init);
    const form = new URLSearchParams(String(init?.body));
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(form.get("code_verifier") ?? "")));
    if (base64url(digest) !== sent.searchParams.get("code_challenge") || form.get("client_secret") !== "test-secret") {
      return Response.json({ error: "invalid_grant" }, { status: 400 });
    }
    return Response.json({ id_token: idToken });
  });
}

export interface Signed {
  /** Where the client was sent back to, with its query. */
  landed: URL;
  token?: string;
  scope?: string;
}

export async function signIn(options: { scope: string; tick: string[]; resource?: string; email?: string }): Promise<Signed> {
  const registered = await call("/oauth/register", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ client_name: "Test agent", redirect_uris: [REDIRECT], token_endpoint_auth_method: "none" }),
  });
  const { client_id } = (await registered.json()) as { client_id: string };

  const verifier = base64url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = base64url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
  const query = new URLSearchParams({
    response_type: "code",
    client_id,
    redirect_uri: REDIRECT,
    code_challenge: challenge,
    code_challenge_method: "S256",
    state: "client-state",
    scope: options.scope,
    resource: options.resource ?? `${ORIGIN}/mcp`,
  });
  const consent = await call(`/authorize?${query}`);
  if (consent.status !== 200) throw new Error(`consent page: ${consent.status} ${await consent.text()}`);
  const handle = (await consent.text()).match(/name="handle" value="([^"]+)"/)![1];

  const form = new URLSearchParams({ handle, decision: "allow" });
  for (const scope of options.tick) form.append("scope", scope);
  const toGoogle = await call("/authorize", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: cookies(consent) },
    body: form,
  });
  const google = new URL(toGoogle.headers.get("location")!);
  if (google.origin !== "https://accounts.google.com") throw new Error(`expected Google, went to ${google}`);

  const spy = fakeGoogle(options.email ?? "owner@example.com", google);
  const back = await call(`/google/callback?code=google-code&state=${encodeURIComponent(google.searchParams.get("state")!)}`, {
    headers: { cookie: cookies(toGoogle) },
  });
  spy.mockRestore();
  const landed = new URL(back.headers.get("location") ?? "about:blank");
  const code = landed.searchParams.get("code");
  if (!code) return { landed };

  const tokens = await call("/oauth/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "authorization_code", code, client_id, redirect_uri: REDIRECT, code_verifier: verifier }),
  });
  const { access_token, scope } = (await tokens.json()) as { access_token: string; scope: string };
  return { landed, token: access_token, scope };
}

/** One JSON-RPC request to /mcp. */
export async function rpc(token: string, method: string, params?: unknown) {
  const response = await call("/mcp", {
    method: "POST",
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json", accept: "application/json, text/event-stream" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  return response;
}

/** A tool's text, from a tools/call that succeeded at the protocol level. */
export async function tool(token: string, name: string, args: Record<string, unknown> = {}) {
  const response = await rpc(token, "tools/call", { name, arguments: args });
  const body = (await response.json()) as { result: { content: { text: string }[]; isError?: boolean } };
  return { text: body.result.content[0].text, isError: body.result.isError === true };
}

export const bearer = (token: string, init: RequestInit = {}): RequestInit => ({
  ...init,
  headers: { authorization: `Bearer ${token}`, "content-type": "application/json", ...(init.headers ?? {}) },
});
