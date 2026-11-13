// The sign-in page every client goes through: the consent form, then Sign in with Apple, then back
// to the client with a code. Only the Apple ID in OWNER_APPLE_SUB gets that far.

import {
  AuthorizationError,
  CimdFetchError,
  authorizationErrorRedirect,
  type ConsentDescription,
  type OAuthHelpers,
} from "@cloudflare/workers-oauth-provider";
import { authorizeUrl, subjectFor } from "./apple";
import type { Env, Owner } from "./env";
import { escapeHtml, page } from "./http";

/** What each scope lets a client do, as the consent form says it. */
const SCOPES: Record<string, { label: string; ticked: boolean }> = {
  "library:read": { label: "Read your library: the books, their text and where you are in each", ticked: true },
  "draft:read": { label: "Read your draft", ticked: false },
  sync: { label: "Keep this device’s library and draft in step with your others", ticked: true },
};
/** Scopes every MCP client is offered, whatever it asked for. */
const OFFERED_TO_AGENTS = ["library:read", "draft:read"];

export const callbackUrl = (env: Env) => `${env.PUBLIC_URL}/apple/callback`;

export async function authorize(request: Request, env: Env, oauth: OAuthHelpers): Promise<Response> {
  try {
    if (request.method === "GET") return await showConsent(request, oauth);
    if (request.method === "POST") return await decide(request, env, oauth);
    return new Response(null, { status: 405, headers: { allow: "GET, POST" } });
  } catch (error) {
    return explain(error);
  }
}

async function showConsent(request: Request, oauth: OAuthHelpers): Promise<Response> {
  const authRequest = await oauth.parseAuthRequest(request);
  const details = await oauth.describeConsent(authRequest);
  const consent = await oauth.beginConsent(authRequest);
  return page(`Allow ${details.clientName} to use Inkling?`, consentForm(details, consent.handle), 200, consent.headers);
}

function consentForm(details: ConsentDescription, handle: string): string {
  const who = details.clientDomain
    ? `Published by <strong>${escapeHtml(details.clientDomain)}</strong>.`
    : "This app registered itself, so its name isn’t verified.";
  const local = details.redirectIsLoopback
    ? "<p><strong>This sends access to an app on this computer.</strong> Go on only if you just started signing in from it.</p>"
    : "";
  const offered = details.scope.includes("sync") ? ["sync"] : OFFERED_TO_AGENTS;
  const boxes = offered
    .map((scope) => {
      const { label, ticked } = SCOPES[scope];
      const checked = ticked || (scope !== "draft:read" && details.scope.includes(scope)) ? " checked" : "";
      return `<label><input type="checkbox" name="scope" value="${scope}"${checked}> ${escapeHtml(label)}</label>`;
    })
    .join("\n");
  return `<p>${who} Access goes to <strong>${escapeHtml(details.redirectHost)}</strong>.</p>
${local}
<form method="post">
  <input type="hidden" name="handle" value="${escapeHtml(handle)}">
  ${boxes}
  <p class="note">It can only read. Nothing it does changes your books or your draft.</p>
  <p><button name="decision" value="allow">Sign in with Apple and allow</button><button name="decision" value="deny">Don’t allow</button></p>
</form>`;
}

async function decide(request: Request, env: Env, oauth: OAuthHelpers): Promise<Response> {
  const form = await request.formData();
  const handle = String(form.get("handle") ?? "");
  if (form.get("decision") !== "allow") {
    const denied = await oauth.denyConsent(request, handle);
    return new Response(null, { status: 302, headers: denied.headers });
  }
  if (!env.APPLE_PRIVATE_KEY) {
    return page("Sign-in isn’t set up yet", "<p>The server has no Sign in with Apple key. Add it, then try again.</p>", 503);
  }
  const ticked = form.getAll("scope").map(String).filter((s) => s in SCOPES);
  const approved = await oauth.approveConsent(request, handle, { scope: ticked });
  // Staying signed in is the client's to ask for, not a box on the form.
  const scope = approved.request.scope;
  const upstream = await oauth.beginUpstream(
    { ...approved.request, scope: [...new Set([...ticked, ...scope.filter((s) => s === "offline_access")])] },
    { headers: approved.headers },
  );
  upstream.headers.set("location", authorizeUrl(env, callbackUrl(env), upstream.state));
  return new Response(null, { status: 302, headers: upstream.headers });
}

/** GET /apple/callback: Apple sends the browser back here with a code, or an error. */
export async function appleCallback(request: Request, env: Env, oauth: OAuthHelpers): Promise<Response> {
  try {
    const resumed = await oauth.finishUpstream(request);
    const params = new URL(request.url).searchParams;
    const code = params.get("code");
    const refuse = () => {
      resumed.headers.set("location", authorizationErrorRedirect(resumed.request, "access_denied"));
      return new Response(null, { status: 302, headers: resumed.headers });
    };
    if (params.get("error") || !code) return refuse();

    const sub = await subjectFor(env, code, callbackUrl(env));
    if (!env.OWNER_APPLE_SUB) {
      return page(
        "One more step",
        `<p>Signing in works. To make this Apple ID the only one that can sign in, set the Worker secret
<code>OWNER_APPLE_SUB</code> to:</p><p><code>${escapeHtml(sub)}</code></p><p>Then sign in again.</p>`,
        403,
        resumed.headers,
      );
    }
    if (sub !== env.OWNER_APPLE_SUB) return refuse();

    const props: Owner = { sub };
    const { redirectTo } = await oauth.completeAuthorization({
      request: resumed.request,
      userId: "owner",
      metadata: {},
      scope: resumed.request.scope,
      props,
    });
    resumed.headers.set("location", redirectTo);
    return new Response(null, { status: 302, headers: resumed.headers });
  } catch (error) {
    return explain(error);
  }
}

/** Errors the consent docs say to send back to the client, or to show here. */
function explain(error: unknown): Response {
  if (error instanceof AuthorizationError && error.redirectTo) return Response.redirect(error.redirectTo, 302);
  if (error instanceof AuthorizationError) {
    return page("That sign-in can’t go on", `<p>${escapeHtml(error.description)}</p><p>Start signing in again from the app.</p>`, 400);
  }
  if (error instanceof CimdFetchError) return page("That app couldn’t be checked", "<p>Its description couldn’t be fetched.</p>", 400);
  throw error;
}
