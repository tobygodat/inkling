// iPhone builds installed over the air (TOB-518). `ios/ota.sh` uploads an ad hoc .ipa with the upload
// token, the Worker keeps the newest five in R2 and answers with a signed link to the install page.
// iOS fetches the manifest and the .ipa outside Safari, with no cookies, so every URL carries an
// HMAC of its path and expiry. The page signs the manifest's URL and the manifest signs the .ipa's,
// each with the expiry it was opened with, so the whole chain stops working 24 hours after upload.

import { z } from "zod";
import type { Env } from "./env";
import { Refused, escapeHtml, json, page } from "./http";
import { fromBase64 } from "./secret";

const LINK_HOURS = 24;
const KEPT_BUILDS = 5;
const BUNDLE_ID = "com.tobygodat.inkling";
const ROUTE = /^\/builds\/([0-9a-f]{32})(\/manifest\.plist|\/Inkling\.ipa)?$/;
const LABEL = z.string().regex(/^[0-9A-Za-z.-]{1,32}$/);
const Labels = z.object({ version: LABEL, build: LABEL });

const objectKey = (id: string) => `builds/${id}.ipa`;
const toHex = (bytes: ArrayBuffer | Uint8Array) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
const fromHex = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16));
const signed = (path: string, expires: number) => new TextEncoder().encode(`${path}\n${expires}`);

/** The links' HMAC key, derived from SECRET_KEY so it is never the Readwise token's AES key. */
async function linkKey(secretKey: string): Promise<CryptoKey> {
  const base = await crypto.subtle.importKey("raw", fromBase64(secretKey), "HKDF", false, ["deriveKey"]);

  return crypto.subtle.deriveKey(
    { name: "HKDF", hash: "SHA-256", salt: new Uint8Array(), info: new TextEncoder().encode("inkling build links") },
    base,
    { name: "HMAC", hash: "SHA-256", length: 256 },
    false,
    ["sign", "verify"],
  );
}

/** `path` on the Worker, good until `expires` (seconds since the Unix epoch). */
export async function signUrl(env: Env, path: string, expires: number): Promise<string> {
  const signature = await crypto.subtle.sign("HMAC", await linkKey(env.SECRET_KEY), signed(path, expires));

  return `${env.PUBLIC_URL}${path}?expires=${expires}&signature=${toHex(signature)}`;
}

/** The link's expiry, once its signature matches its path and it hasn't passed. */
async function verify(env: Env, url: URL): Promise<number> {
  const expires = Number(url.searchParams.get("expires"));
  const signature = url.searchParams.get("signature") ?? "";

  if (!Number.isSafeInteger(expires) || !/^[0-9a-f]{64}$/.test(signature)) throw new Refused(403, "That link isn’t signed.");

  if (!(await crypto.subtle.verify("HMAC", await linkKey(env.SECRET_KEY), fromHex(signature), signed(url.pathname, expires)))) {
    throw new Refused(403, "That link was changed, so it doesn’t work.");
  }

  if (expires * 1000 <= Date.now()) throw new Refused(410, "That link has expired. Make a new build for a new link.");

  return expires;
}

async function authorized(request: Request, env: Env): Promise<boolean> {
  const given = request.headers.get("authorization")?.match(/^Bearer (.+)$/)?.[1];

  if (!env.BUILD_UPLOAD_TOKEN || !given) return false;
  const digest = (text: string) => crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));

  return crypto.subtle.timingSafeEqual(await digest(given), await digest(env.BUILD_UPLOAD_TOKEN));
}

async function upload(request: Request, env: Env): Promise<Response> {
  if (!(await authorized(request, env))) throw new Refused(401, "That upload token isn’t right.");
  const { searchParams } = new URL(request.url);
  const labels = Labels.safeParse({ version: searchParams.get("version"), build: searchParams.get("build") });

  if (!labels.success) throw new Refused(400, "Name the build’s version and build number.");

  if (!request.body) throw new Refused(400, "Send the .ipa as the request body.");
  const id = toHex(crypto.getRandomValues(new Uint8Array(16)));

  await env.BUILDS.put(objectKey(id), request.body, {
    httpMetadata: { contentType: "application/octet-stream" },
    customMetadata: { ...labels.data, at: String(Date.now()) },
  });

  const listed = await env.BUILDS.list({ prefix: "builds/", include: ["customMetadata"] });
  const older = listed.objects.toSorted((a, b) => Number(b.customMetadata?.at) - Number(a.customMetadata?.at)).slice(KEPT_BUILDS);

  if (older.length > 0) await env.BUILDS.delete(older.map((o) => o.key));
  const expires = Math.floor(Date.now() / 1000) + LINK_HOURS * 60 * 60;

  return json({ id, url: await signUrl(env, `/builds/${id}`, expires) }, 201);
}

const gone = () => new Refused(404, "That build is gone: only the newest five are kept.");

function installPage(env: Env, id: string, labels: z.infer<typeof Labels>, manifest: string, expires: number): Response {
  const install = `itms-services://?action=download-manifest&url=${encodeURIComponent(manifest)}`;

  return page(
    "Install Inkling",
    `<style>a.install { display: inline-block; padding: .6rem 1.5rem; border-radius: .5rem; background: #2f6fde; color: #fff; text-decoration: none; font-weight: 600; }</style>
<p>Version ${escapeHtml(labels.version)} (${escapeHtml(labels.build)}), build ${escapeHtml(id.slice(0, 8))}.</p>
<p><a class="install" href="${escapeHtml(install)}">Install</a></p>
<p class="note">Only an iPhone registered on the developer account can install it. This link stops working on ${new Date(expires * 1000).toUTCString()}.</p>`,
  );
}

function manifestPlist(labels: z.infer<typeof Labels>, ipa: string): Response {
  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>items</key>
  <array>
    <dict>
      <key>assets</key>
      <array>
        <dict>
          <key>kind</key><string>software-package</string>
          <key>url</key><string>${escapeHtml(ipa)}</string>
        </dict>
      </array>
      <key>metadata</key>
      <dict>
        <key>bundle-identifier</key><string>${BUNDLE_ID}</string>
        <key>bundle-version</key><string>${escapeHtml(labels.version)}</string>
        <key>kind</key><string>software</string>
        <key>title</key><string>Inkling</string>
      </dict>
    </dict>
  </array>
</dict>
</plist>
`;

  return new Response(plist, { headers: { "content-type": "application/xml" } });
}

async function route(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);

  if (url.pathname === "/builds" && request.method === "POST") return upload(request, env);
  const m = url.pathname.match(ROUTE);

  if (!m || (request.method !== "GET" && request.method !== "HEAD")) throw new Refused(404, "There’s nothing here.");
  const expires = await verify(env, url);
  const [, id, file] = m;

  if (file === "/Inkling.ipa") {
    const object = await env.BUILDS.get(objectKey(id));

    if (!object) throw gone();

    return new Response(object.body, { headers: { "content-type": "application/octet-stream" } });
  }

  const object = await env.BUILDS.head(objectKey(id));

  if (!object) throw gone();
  const labels = Labels.parse(object.customMetadata);

  if (file === "/manifest.plist") return manifestPlist(labels, await signUrl(env, `/builds/${id}/Inkling.ipa`, expires));

  return installPage(env, id, labels, await signUrl(env, `/builds/${id}/manifest.plist`, expires), expires);
}

/** Every answer under /builds, refusals included, says not to keep it. */
export async function serveBuilds(request: Request, env: Env): Promise<Response> {
  let response: Response;

  try {
    response = await route(request, env);
  } catch (e) {
    if (!(e instanceof Refused)) console.error(e);
    response = e instanceof Refused ? json({ error: e.message }, e.status) : json({ error: "Something went wrong on the server." }, 500);
  }

  response.headers.set("cache-control", "no-store");

  return response;
}
