import { cloudflareTest, readD1Migrations } from "@cloudflare/vitest-plugin";
import { defineConfig } from "vitest/config";

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
          // Mixed case, as someone might type it: Google's email is compared in lower case.
          OWNER_EMAIL: "Owner@Example.com",
          GOOGLE_CLIENT_ID: "test-client.apps.googleusercontent.com",
          GOOGLE_CLIENT_SECRET: "test-secret",
        },
      },
    }),
  ],
  test: { setupFiles: ["./test/setup.ts"] },
}));
