import { describe, expect, it } from "vitest";
import { checkIdToken, clientSecret } from "../src/apple";

const config = { APPLE_CLIENT_ID: "com.tobygodat.inkling.web", APPLE_TEAM_ID: "Q6NAWA42Y2", APPLE_KEY_ID: "KEY1", APPLE_PRIVATE_KEY: "" };
const base64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const fromBase64url = (s: string) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));

describe("Sign in with Apple", () => {
  it("signs a client secret Apple can check with the key's public half", async () => {
    const pair = (await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"])) as CryptoKeyPair;
    const der = new Uint8Array((await crypto.subtle.exportKey("pkcs8", pair.privateKey)) as ArrayBuffer);
    const pem = `-----BEGIN PRIVATE KEY-----\n${btoa(String.fromCharCode(...der))}\n-----END PRIVATE KEY-----`;
    const jwt = await clientSecret({ ...config, APPLE_PRIVATE_KEY: pem }, 1_000);
    const [header, claims, signature] = jwt.split(".");
    expect(JSON.parse(new TextDecoder().decode(fromBase64url(header)))).toEqual({ alg: "ES256", kid: "KEY1" });
    expect(JSON.parse(new TextDecoder().decode(fromBase64url(claims)))).toEqual({
      iss: "Q6NAWA42Y2", iat: 1_000, exp: 1_300, aud: "https://appleid.apple.com", sub: "com.tobygodat.inkling.web",
    });
    const valid = await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, pair.publicKey, fromBase64url(signature), new TextEncoder().encode(`${header}.${claims}`));
    expect(valid).toBe(true);
  });

  it("takes the subject only from an unexpired token Apple issued for this app", () => {
    const token = (claims: Record<string, unknown>) => `e30.${base64url(new TextEncoder().encode(JSON.stringify(claims)))}.sig`;
    const good = { iss: "https://appleid.apple.com", aud: config.APPLE_CLIENT_ID, exp: 2_000, sub: "000123.owner" };
    expect(checkIdToken(config, token(good), 1_000)).toBe("000123.owner");
    expect(() => checkIdToken(config, token({ ...good, aud: "com.other.app" }), 1_000)).toThrow();
    expect(() => checkIdToken(config, token({ ...good, iss: "https://evil.example" }), 1_000)).toThrow();
    expect(() => checkIdToken(config, token(good), 3_000)).toThrow();
    expect(() => checkIdToken(config, token({ ...good, sub: "" }), 1_000)).toThrow();
  });
});
