// Sign in with Google: the identity behind every grant (ADR 0004). A web client with PKCE and a
// nonce; only the verified email in OWNER_EMAIL gets a grant.
// https://developers.google.com/identity/openid-connect/openid-connect

export interface GoogleConfig {
  /** The OAuth client of type "Web application" in Google Cloud. */
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
}

const AUTHORIZE = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN = "https://oauth2.googleapis.com/token";
const ISSUERS = new Set(["https://accounts.google.com", "accounts.google.com"]);

const base64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");

/** A fresh PKCE verifier or nonce. */
export const random = () => base64url(crypto.getRandomValues(new Uint8Array(32)));

async function challenge(verifier: string): Promise<string> {
  return base64url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
}

/** Where the browser goes to sign in, asking only for the account's email. */
export async function authorizeUrl(config: GoogleConfig, redirectUri: string, state: string, verifier: string, nonce: string): Promise<string> {
  const url = new URL(AUTHORIZE);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("client_id", config.GOOGLE_CLIENT_ID);
  url.searchParams.set("redirect_uri", redirectUri);
  url.searchParams.set("scope", "openid email");
  url.searchParams.set("state", state);
  url.searchParams.set("nonce", nonce);
  url.searchParams.set("code_challenge", await challenge(verifier));
  url.searchParams.set("code_challenge_method", "S256");
  url.searchParams.set("prompt", "select_account");
  return url.toString();
}

/**
 * Exchanges the code Google sent back for the account's verified email. The ID token comes
 * straight from Google's token endpoint over TLS, so its claims are checked but its signature
 * need not be (OpenID Connect Core 3.1.3.7).
 */
export async function emailFor(config: GoogleConfig, code: string, redirectUri: string, verifier: string, nonce: string): Promise<string> {
  const response = await fetch(TOKEN, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: config.GOOGLE_CLIENT_ID,
      client_secret: config.GOOGLE_CLIENT_SECRET,
      code,
      code_verifier: verifier,
      grant_type: "authorization_code",
      redirect_uri: redirectUri,
    }),
  });
  if (!response.ok) throw new Error(`Google refused the sign-in code (${response.status}).`);
  const { id_token } = (await response.json()) as { id_token?: string };
  return checkIdToken(config, id_token ?? "", nonce);
}

export function checkIdToken(config: GoogleConfig, idToken: string, nonce: string, now = Math.floor(Date.now() / 1000)): string {
  const payload = idToken.split(".")[1];
  if (!payload) throw new Error("Google sent no ID token.");
  const claims = JSON.parse(atob(payload.replace(/-/g, "+").replace(/_/g, "/"))) as Record<string, unknown>;
  if (typeof claims.iss !== "string" || !ISSUERS.has(claims.iss)) throw new Error("The ID token isn’t from Google.");
  if (claims.aud !== config.GOOGLE_CLIENT_ID) throw new Error("The ID token is for another app.");
  if (typeof claims.exp !== "number" || claims.exp < now) throw new Error("The ID token has expired.");
  if (claims.nonce !== nonce) throw new Error("The ID token is from another sign-in.");
  if (claims.email_verified !== true || typeof claims.email !== "string" || !claims.email) {
    throw new Error("Google hasn’t verified that account’s email.");
  }
  return claims.email.toLowerCase();
}
