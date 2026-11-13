// Sign in with Apple on the web: the identity behind every grant (ADR 0004).
// https://developer.apple.com/documentation/signinwithapplerestapi

export interface AppleConfig {
  /** The Services ID, such as com.tobygodat.inkling.web. */
  APPLE_CLIENT_ID: string;
  APPLE_TEAM_ID: string;
  /** The Sign in with Apple key's id, and the key itself as a PKCS #8 PEM. */
  APPLE_KEY_ID: string;
  APPLE_PRIVATE_KEY: string;
}

const ISSUER = "https://appleid.apple.com";

/** Where the browser goes to sign in. No scope is asked for, so Apple answers with a GET. */
export function authorizeUrl(config: AppleConfig, redirectUri: string, state: string): string {
  const url = new URL(`${ISSUER}/auth/authorize`);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("response_mode", "query");
  url.searchParams.set("client_id", config.APPLE_CLIENT_ID);
  url.searchParams.set("redirect_uri", redirectUri);
  url.searchParams.set("state", state);
  return url.toString();
}

const base64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const encodeJson = (value: unknown) => base64url(new TextEncoder().encode(JSON.stringify(value)));

function pemBody(pem: string): Uint8Array {
  const body = pem.replace(/-----(BEGIN|END) PRIVATE KEY-----/g, "").replace(/\s+/g, "");
  return Uint8Array.from(atob(body), (c) => c.charCodeAt(0));
}

/** The client secret Apple asks for: a JWT signed with ES256 by the Sign in with Apple key. */
export async function clientSecret(config: AppleConfig, now = Math.floor(Date.now() / 1000)): Promise<string> {
  const key = await crypto.subtle.importKey("pkcs8", pemBody(config.APPLE_PRIVATE_KEY), { name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
  const signingInput = `${encodeJson({ alg: "ES256", kid: config.APPLE_KEY_ID })}.${encodeJson({
    iss: config.APPLE_TEAM_ID,
    iat: now,
    exp: now + 300,
    aud: ISSUER,
    sub: config.APPLE_CLIENT_ID,
  })}`;
  // WebCrypto signs ECDSA in the raw r||s form, which is what JWS uses.
  const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, new TextEncoder().encode(signingInput));
  return `${signingInput}.${base64url(new Uint8Array(signature))}`;
}

/**
 * Exchanges the code Apple sent back for the Apple ID's stable subject. The ID token comes straight
 * from Apple's token endpoint over TLS, so its claims are checked but its signature need not be
 * (OpenID Connect Core 3.1.3.7).
 */
export async function subjectFor(config: AppleConfig, code: string, redirectUri: string): Promise<string> {
  const response = await fetch(`${ISSUER}/auth/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: config.APPLE_CLIENT_ID,
      client_secret: await clientSecret(config),
      code,
      grant_type: "authorization_code",
      redirect_uri: redirectUri,
    }),
  });
  if (!response.ok) throw new Error(`Apple refused the sign-in code (${response.status}).`);
  const { id_token } = (await response.json()) as { id_token?: string };
  return checkIdToken(config, id_token ?? "");
}

export function checkIdToken(config: AppleConfig, idToken: string, now = Math.floor(Date.now() / 1000)): string {
  const payload = idToken.split(".")[1];
  if (!payload) throw new Error("Apple sent no ID token.");
  const claims = JSON.parse(atob(payload.replace(/-/g, "+").replace(/_/g, "/"))) as Record<string, unknown>;
  if (claims.iss !== ISSUER) throw new Error("The ID token isn’t from Apple.");
  if (claims.aud !== config.APPLE_CLIENT_ID) throw new Error("The ID token is for another app.");
  if (typeof claims.exp !== "number" || claims.exp < now) throw new Error("The ID token has expired.");
  if (typeof claims.sub !== "string" || !claims.sub) throw new Error("The ID token names no Apple ID.");
  return claims.sub;
}
