import { describe, expect, it } from "vitest";
import { authorizeUrl, checkIdToken } from "../src/google";

const config = { GOOGLE_CLIENT_ID: "test-client.apps.googleusercontent.com", GOOGLE_CLIENT_SECRET: "test-secret" };
const base64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");

describe("Sign in with Google", () => {
  it("asks only for the email, with PKCE and a nonce", async () => {
    const url = new URL(await authorizeUrl(config, "https://s/google/callback", "st", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "n1"));
    expect(url.origin + url.pathname).toBe("https://accounts.google.com/o/oauth2/v2/auth");
    expect(url.searchParams.get("scope")).toBe("openid email");
    // RFC 7636's own example pair.
    expect(url.searchParams.get("code_challenge")).toBe("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    expect([url.searchParams.get("state"), url.searchParams.get("nonce")]).toEqual(["st", "n1"]);
  });

  it("takes the email only from an unexpired token Google issued for this sign-in", () => {
    const token = (claims: Record<string, unknown>) => `e30.${base64url(new TextEncoder().encode(JSON.stringify(claims)))}.sig`;
    const good = { iss: "https://accounts.google.com", aud: config.GOOGLE_CLIENT_ID, exp: 2_000, nonce: "n1", email: "Owner@Example.com", email_verified: true };
    expect(checkIdToken(config, token(good), "n1", 1_000)).toBe("owner@example.com");
    expect(checkIdToken(config, token({ ...good, iss: "accounts.google.com" }), "n1", 1_000)).toBe("owner@example.com");
    expect(() => checkIdToken(config, token({ ...good, aud: "other.apps.googleusercontent.com" }), "n1", 1_000)).toThrow();
    expect(() => checkIdToken(config, token({ ...good, iss: "https://evil.example" }), "n1", 1_000)).toThrow();
    expect(() => checkIdToken(config, token(good), "n1", 3_000)).toThrow();
    expect(() => checkIdToken(config, token(good), "n2", 1_000)).toThrow();
    expect(() => checkIdToken(config, token({ ...good, email_verified: false }), "n1", 1_000)).toThrow();
  });
});
