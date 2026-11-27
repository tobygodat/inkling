import { createExecutionContext, waitOnExecutionContext } from "cloudflare:test";
import { env } from "cloudflare:workers";
import { describe, expect, it } from "vitest";
import worker from "../src/index";
import { matchesNewsletterAddress } from "../src/newsletter";
import { bearer, call, signIn } from "./signin";

function fakeEmail(options: { from: string; to: string; raw: string; headers?: Record<string, string> }) {
  const bytes = new TextEncoder().encode(options.raw);
  let rejected: string | null = null;
  const headers = new Headers(options.headers);

  const message: ForwardableEmailMessage = {
    from: options.from,
    to: options.to,
    headers,
    canBeForwarded: false,
    raw: new ReadableStream({
      start(controller) {
        controller.enqueue(bytes);
        controller.close();
      },
    }),
    rawSize: bytes.byteLength,
    setReject(reason: string) {
      rejected = reason;
    },
    async forward() {
      return { messageId: "" };
    },
    async reply() {
      return { messageId: "" };
    },
  };

  return { message, rejected: () => rejected };
}

async function deliver(message: ForwardableEmailMessage) {
  const ctx = createExecutionContext();
  await worker.email(message, env, ctx);
  await waitOnExecutionContext(ctx);
}

async function syncToken() {
  const { token } = await signIn({ scope: "sync offline_access", tick: ["sync"], resource: "http://localhost/v1" });

  return token!;
}

const MULTIPART_NEWSLETTER = [
  'From: "Stratechery" <email@stratechery.example>',
  "To: read@tobygodat.com",
  "Subject: Aggregation and the Edge",
  "Date: Thu, 26 Nov 2026 12:00:00 +0000",
  "Message-ID: <issue-42@stratechery.example>",
  'Content-Type: multipart/alternative; boundary="b1"',
  "",
  "--b1",
  "Content-Type: text/plain; charset=utf-8",
  "",
  "Plain text edition.",
  "--b1",
  "Content-Type: text/html; charset=utf-8",
  "",
  "<html><body><article><h1>Aggregation and the Edge</h1><p>This week on the web.</p></article></body></html>",
  "--b1--",
  "",
].join("\r\n");

const PLAIN_NEWSLETTER = [
  'From: "Matt Levine" <mlevine@bloomberg.example>',
  "To: read+finance@tobygodat.com",
  "Subject: Money Stuff: Bonds",
  "Date: Fri, 27 Nov 2026 14:00:00 +0000",
  "Message-ID: <issue-99@bloomberg.example>",
  "Content-Type: text/plain; charset=utf-8",
  "",
  "First paragraph of the letter.",
  "",
  "Second paragraph with a <tag> & details.",
  "",
].join("\r\n");

describe("newsletters via Cloudflare Email Routing", () => {
  it("matches the configured address and +tag variants only", () => {
    expect(matchesNewsletterAddress("read@tobygodat.com", "read@tobygodat.com")).toBe(true);
    expect(matchesNewsletterAddress("READ+substack@tobygodat.com", "read@tobygodat.com")).toBe(true);
    expect(matchesNewsletterAddress("other@tobygodat.com", "read@tobygodat.com")).toBe(false);
    expect(matchesNewsletterAddress("read@other.com", "read@tobygodat.com")).toBe(false);
  });

  it("rejects mail to other addresses and stages HTML and plain-text newsletters in the Inbox without duplicates", async () => {
    const wrong = fakeEmail({ from: "spam@example.com", to: "someoneelse@tobygodat.com", raw: PLAIN_NEWSLETTER });
    await deliver(wrong.message);
    expect(wrong.rejected()).toBe("Unknown recipient.");

    const htmlMail = fakeEmail({
      from: "email@stratechery.example",
      to: "read@tobygodat.com",
      raw: MULTIPART_NEWSLETTER,
      headers: { "message-id": "<issue-42@stratechery.example>" },
    });

    await deliver(htmlMail.message);
    expect(htmlMail.rejected()).toBeNull();

    // Redelivery of the same Message-ID is ignored.
    const dupMail = fakeEmail({
      from: "email@stratechery.example",
      to: "read@tobygodat.com",
      raw: MULTIPART_NEWSLETTER,
      headers: { "message-id": "<issue-42@stratechery.example>" },
    });

    await deliver(dupMail.message);

    // Plain-text newsletter addressed with +tag lands cleanly wrapped as HTML.
    const plainMail = fakeEmail({
      from: "mlevine@bloomberg.example",
      to: "read+finance@tobygodat.com",
      raw: PLAIN_NEWSLETTER,
      headers: { "message-id": "<issue-99@bloomberg.example>" },
    });

    await deliver(plainMail.message);
    expect(plainMail.rejected()).toBeNull();

    const token = await syncToken();

    const library = await (await call("/v1/library", bearer(token))).json<{
      incoming: { id: string; source: string; destination: string; title: string; author: string; site: string; state: string; url: string }[];
    }>();

    expect(library.incoming).toHaveLength(2);
    const byTitle = Object.fromEntries(library.incoming.map((item) => [item.title, item]));

    expect(byTitle["Aggregation and the Edge"]).toMatchObject({
      source: "newsletter",
      destination: "inbox",
      author: "Stratechery",
      site: "Stratechery",
      state: "waiting",
    });
    expect(byTitle["Aggregation and the Edge"]!.url).toMatch(/^https:\/\/tobygodat\.com\/newsletters\/[0-9a-f]{64}$/u);

    expect(byTitle["Money Stuff: Bonds"]).toMatchObject({
      source: "newsletter",
      destination: "inbox",
      author: "Matt Levine",
      site: "Matt Levine",
      state: "waiting",
    });

    const htmlStored = await (await call(`/v1/incoming/${byTitle["Aggregation and the Edge"]!.id}/file`, bearer(token))).text();
    expect(htmlStored).toContain("This week on the web.");
    expect(htmlStored).toContain('<meta property="og:site_name" content="Stratechery">');

    const plainStored = await (await call(`/v1/incoming/${byTitle["Money Stuff: Bonds"]!.id}/file`, bearer(token))).text();
    expect(plainStored).toContain("<p>First paragraph of the letter.</p>");
    expect(plainStored).toContain("&lt;tag&gt; &amp; details.");
  });
});
