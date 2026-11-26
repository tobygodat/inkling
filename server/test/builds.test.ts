import { env } from "cloudflare:workers";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ORIGIN, call, capture } from "./signin";

const TOKEN = "test-upload-token";
const IPA = new TextEncoder().encode("PK stand-in ipa bytes & more");

const DAY = 24 * 60 * 60 * 1000;
const KEPT = 5;

const local = (url: string) => url.slice(ORIGIN.length);

async function upload(token = TOKEN, query = "version=0.1.0&build=20261121.0900", body: BodyInit = IPA) {
  return call(`/builds?${query}`, { method: "POST", headers: { authorization: `Bearer ${token}` }, body });
}

/** The install link's manifest URL, and the manifest's .ipa URL, as iOS reads them. */
async function chain(pageUrl: string) {
  const html = await (await call(local(pageUrl))).text();
  const href = capture(html, /href="(itms-services:[^"]+)"/u).replaceAll("&amp;", "&");
  const manifest = new URL(href).searchParams.get("url")!;
  expect(href).toBe(`itms-services://?action=download-manifest&url=${encodeURIComponent(manifest)}`);
  const plist = await (await call(local(manifest))).text();
  const ipa = capture(plist, /<key>url<\/key><string>([^<]+)<\/string>/u);
  expect(ipa).toContain("&amp;signature=");

  return { manifest, ipa: ipa.replaceAll("&amp;", "&") };
}

const tampered = (url: string, edit: (u: URL) => void) => {
  const u = new URL(url);
  edit(u);

  return local(u.toString());
};

afterEach(() => vi.useRealTimers());

describe("over-the-air builds", () => {
  it("takes an upload only with the token", async () => {
    expect((await call("/builds?version=1&build=1", { method: "POST", body: IPA })).status).toBe(401);
    const wrong = await upload("not-the-token");
    expect(wrong.status).toBe(401);
    expect(wrong.headers.get("cache-control")).toBe("no-store");
    expect((await upload(TOKEN, "version=1")).status).toBe(400);
  });

  it("walks the page, the manifest and the .ipa, each signed by the one before", async () => {
    const res = await upload();
    expect(res.status).toBe(201);
    const { id, url } = await res.json<{ id: string; url: string }>();
    expect(id).toMatch(/^[0-9a-f]{32}$/u);
    expect(url).toMatch(new RegExp(`^${ORIGIN}/builds/${id}\\?expires=\\d+&signature=[0-9a-f]{64}$`, "u"));

    const page = await call(local(url));
    expect(page.status).toBe(200);
    expect(page.headers.get("cache-control")).toBe("no-store");
    expect(await page.clone().text()).toContain("Version 0.1.0 (20261121.0900)");

    const { manifest, ipa } = await chain(url);
    const plist = await call(local(manifest));
    expect(plist.headers.get("cache-control")).toBe("no-store");
    expect(await plist.text()).toContain("<string>com.tobygodat.inkling</string>");

    const file = await call(local(ipa));
    expect(file.status).toBe(200);
    expect(file.headers.get("cache-control")).toBe("no-store");
    expect(new Uint8Array(await file.arrayBuffer())).toEqual(IPA);
  });

  it("refuses a changed link at every step", async () => {
    const other = (await (await upload()).json<{ id: string }>()).id;
    const { url } = await (await upload()).json<{ url: string }>();
    const { manifest, ipa } = await chain(url);

    for (const link of [url, manifest, ipa]) {
      const edits: ((u: URL) => void)[] = [
        (u) => (u.pathname = u.pathname.replace(/[0-9a-f]{32}/u, other)),
        (u) => u.searchParams.set("expires", String(Number(u.searchParams.get("expires")) + 3600)),
        (u) => u.searchParams.set("signature", u.searchParams.get("signature")!.replace(/^./u, (c) => (c === "0" ? "1" : "0"))),
        (u) => u.searchParams.delete("signature"),
      ];

      for (const edit of edits) {
        const res = await call(tampered(link, edit));
        expect(res.status).toBe(403);
        expect(res.headers.get("cache-control")).toBe("no-store");
      }
    }

    // Another file of the same build under this link's signature.
    const swapped = tampered(manifest, (u) => (u.pathname = u.pathname.replace("manifest.plist", "Inkling.ipa")));
    expect((await call(swapped)).status).toBe(403);
  });

  it("stops every link in the chain 24 hours after the upload", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const start = Date.now();
    const { url } = await (await upload()).json<{ url: string }>();
    const { manifest, ipa } = await chain(url);

    vi.setSystemTime(start + DAY - 60_000);
    // A page opened late signs the manifest with its own expiry, not a fresh 24 hours.
    const late = await chain(url);
    expect(new URL(late.manifest).searchParams.get("expires")).toBe(new URL(url).searchParams.get("expires"));

    vi.setSystemTime(start + DAY + 1000);

    for (const link of [url, manifest, ipa, late.ipa]) {
      const res = await call(local(link));
      expect(res.status).toBe(410);
      expect(res.headers.get("cache-control")).toBe("no-store");
    }
  });

  it("keeps only the newest five builds", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const urls: string[] = [];

    for (let i = 0; i < KEPT + 2; i++) {
      vi.setSystemTime(Date.now() + 60_000);
      urls.push((await (await upload(TOKEN, `version=0.1.${i}&build=${i}`)).json<{ url: string }>()).url);
    }

    const kept = (await env.BUILDS.list({ prefix: "builds/" })).objects.map((o) => o.key);
    expect(kept).toHaveLength(KEPT);
    expect(kept.toSorted()).toEqual(urls.slice(2).map((u) => `builds/${new URL(u).pathname.split("/")[2]}.ipa`).toSorted());

    for (const url of urls.slice(0, 2)) {
      const res = await call(local(url));
      expect(res.status).toBe(404);
      expect(res.headers.get("cache-control")).toBe("no-store");
    }

    for (const url of urls.slice(2)) expect((await call(local(url))).status).toBe(200);
  });
});
