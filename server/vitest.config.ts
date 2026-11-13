import { cloudflareTest, readD1Migrations } from "@cloudflare/vitest-plugin";
import { generateKeyPairSync } from "node:crypto";
import { defineConfig } from "vitest/config";

// A throwaway Sign in with Apple key for each run; the real one is a Worker secret.
const appleKey = generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey.export({ type: "pkcs8", format: "pem" }).toString();

export default defineConfig(async () => ({
  plugins: [
    cloudflareTest({
      wrangler: { configPath: "./wrangler.jsonc" },
      miniflare: {
        bindings: {
          PUBLIC_URL: "http://localhost",
          TEST_MIGRATIONS: await readD1Migrations("./migrations"),
          // Test values only. Production secrets are set with `wrangler secret put`.
          SECRET_KEY: "dGVzdC1rZXktdGVzdC1rZXktdGVzdC1rZXktdGVzdC0=",
          OWNER_APPLE_SUB: "000123.owner",
          APPLE_CLIENT_ID: "com.tobygodat.inkling.web",
          APPLE_TEAM_ID: "Q6NAWA42Y2",
          APPLE_KEY_ID: "TESTKEY123",
          APPLE_PRIVATE_KEY: appleKey,
        },
      },
    }),
  ],
  test: { setupFiles: ["./test/setup.ts"] },
}));
