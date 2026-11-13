import { describe, expect, it } from "vitest";
import { REDIRECT, call, rpc, signIn, tool } from "./signin";

describe("signing in", () => {
  it("challenges a request with no token, pointing at the resource's metadata", async () => {
    const response = await call("/mcp", { method: "POST", body: "{}" });
    expect(response.status).toBe(401);
    expect(response.headers.get("www-authenticate")).toContain("/.well-known/oauth-protected-resource/mcp");
    const metadata = await (await call("/.well-known/oauth-authorization-server")).json<Record<string, unknown>>();
    expect(metadata.registration_endpoint).toBe("http://localhost/oauth/register");
    expect(metadata.scopes_supported).toEqual(["library:read", "draft:read", "sync", "offline_access"]);
  });

  it("gives the owner a token with only the scopes they ticked, and the draft needs its own", async () => {
    const signed = await signIn({ scope: "library:read draft:read", tick: ["library:read"] });
    expect(signed.landed.origin + signed.landed.pathname).toBe(REDIRECT);
    expect(signed.landed.searchParams.get("state")).toBe("client-state");
    expect(signed.scope).toBe("library:read");

    const hello = await (await rpc(signed.token!, "initialize", { protocolVersion: "2025-06-18", capabilities: {} })).json<{
      result: { protocolVersion: string };
    }>();
    expect(hello.result.protocolVersion).toBe("2025-06-18");
    expect((await tool(signed.token!, "list_books")).text).toBe("[]");

    const draft = await rpc(signed.token!, "tools/call", { name: "read_draft", arguments: {} });
    expect(draft.status).toBe(403);
    expect(draft.headers.get("www-authenticate")).toContain("draft:read");

    const sync = await call("/v1/library", { headers: { authorization: `Bearer ${signed.token}` } });
    expect(sync.status).toBe(401); // An agent's token is for /mcp, not the devices' API.
  });

  it("sends anyone else back to the client refused", async () => {
    const signed = await signIn({ scope: "library:read", tick: ["library:read"], sub: "000999.stranger" });
    expect(signed.token).toBeUndefined();
    expect(signed.landed.searchParams.get("error")).toBe("access_denied");
  });

  it("answers the protocol's own requests and errors", async () => {
    const { token } = await signIn({ scope: "library:read", tick: ["library:read"] });
    const tools = await (await rpc(token!, "tools/list")).json<{ result: { tools: { name: string }[] } }>();
    expect(tools.result.tools.map((t) => t.name)).toEqual(["list_books", "read_book", "search_library", "read_draft"]);
    const unknown = await (await rpc(token!, "resources/list")).json<{ error: { code: number } }>();
    expect(unknown.error.code).toBe(-32601);
    const notification = await call("/mcp", {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }),
    });
    expect(notification.status).toBe(202);
  });
});
