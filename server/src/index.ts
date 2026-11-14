// inkling-server: the OAuth authorization server, the remote MCP endpoint at /mcp, the devices' sync
// API under /v1, and the Readwise inbox (ADR 0004). One Worker on Cloudflare's free plan, so every
// request stays well inside 10 ms of CPU: no book is parsed here.

import { OAuthAuthorizationServer, OAuthResourceServer } from "@cloudflare/workers-oauth-provider";
import { authorize, googleCallback } from "./authorize";
import type { Env, Owner } from "./env";
import { Refused, json } from "./http";
import { serveMcp } from "./mcp";
import { poll, webhook } from "./readwise";
import { serveSync } from "./sync";

/** Everything a client can be granted. `offline_access` lets a client stay signed in. */
export const SCOPES = ["library:read", "draft:read", "sync", "offline_access"];

interface Servers {
  url: string;
  auth: OAuthAuthorizationServer<Env>;
  mcp: OAuthResourceServer<Env, Owner>;
  sync: OAuthResourceServer<Env, Owner>;
}

let built: Servers | null = null;

/** The issuer is the Worker's own address, which comes from its configuration. */
function servers(env: Env): Servers {
  if (built?.url === env.PUBLIC_URL) return built;
  const url = env.PUBLIC_URL;
  const resources = { mcp: `${url}/mcp`, sync: `${url}/v1` };
  const auth = new OAuthAuthorizationServer<Env>({
    issuer: url,
    resources: [resources.mcp, resources.sync],
    // An MCP client that names no resource is an agent.
    defaultResource: resources.mcp,
    scopesSupported: SCOPES,
    clientRegistrationEndpoint: "/oauth/register",
    clientIdMetadataDocumentEnabled: true,
    // The apps sign in through the system browser sheet and come back on inkling://.
    allowPrivateUseRedirectUris: true,
    // A grant lasts while it is used, and ends after 60 idle days.
    refreshTokenIdleTTL: 60 * 24 * 60 * 60,
  });
  const validate = (env: Env) => (resource: string, token: string) => auth.validateToken<Owner>(resource, token, env);
  const errors = (handler: (request: Request, env: Env, ctx: Parameters<typeof serveMcp>[2]) => Promise<Response>) => ({
    fetch: async (request: Request, env: Env, ctx: Parameters<typeof serveMcp>[2]) => {
      try {
        return await handler(request, env, ctx);
      } catch (e) {
        if (e instanceof Refused) return json({ error: e.message }, e.status);
        throw e;
      }
    },
  });
  built = {
    url,
    auth,
    mcp: new OAuthResourceServer<Env, Owner>({
      resourceMetadata: { resource: resources.mcp, authorization_servers: [url], resource_name: "Inkling" },
      requiredScopes: ["library:read"],
      validateToken: validate,
      handler: errors(serveMcp),
    }),
    sync: new OAuthResourceServer<Env, Owner>({
      resourceMetadata: { resource: resources.sync, authorization_servers: [url], resource_name: "Inkling sync" },
      requiredScopes: ["sync"],
      validateToken: validate,
      handler: errors(serveSync),
    }),
  };
  return built;
}

const under = (pathname: string, prefix: string) => pathname === prefix || pathname.startsWith(`${prefix}/`);

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const s = servers(env);
    const { pathname } = new URL(request.url);
    try {
      if (under(pathname, "/mcp") || pathname.startsWith("/.well-known/oauth-protected-resource/mcp")) {
        return await s.mcp.fetch(request, env, ctx);
      }
      if (under(pathname, "/v1") || pathname.startsWith("/.well-known/oauth-protected-resource/v1")) {
        return await s.sync.fetch(request, env, ctx);
      }
      if (pathname === "/authorize") return await authorize(request, env, s.auth.getOAuthApi(env));
      if (pathname === "/google/callback") return await googleCallback(request, env, s.auth.getOAuthApi(env));
      if (pathname === "/readwise/webhook" && request.method === "POST") return await webhook(request, env, ctx);
      if (pathname === "/") return new Response("Inkling's server. Connect to it from the app or an agent at /mcp.\n");
      return await s.auth.fetch(request, env, ctx);
    } catch (e) {
      if (e instanceof Refused) return json({ error: e.message }, e.status);
      console.error(e);
      return json({ error: "Something went wrong on the server." }, 500);
    }
  },

  async scheduled(_controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    ctx.waitUntil(poll(env));
    ctx.waitUntil(servers(env).auth.purgeExpiredData(env).then(() => undefined));
  },
} satisfies ExportedHandler<Env>;
