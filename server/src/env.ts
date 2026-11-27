import type { GoogleConfig } from "./google";

export interface Env extends GoogleConfig {
  DB: D1Database;
  /** The OAuth provider's own records. */
  OAUTH_KV: KVNamespace;
  /** Parsed books, covers, and Readwise files waiting in the inbox. */
  FILES: KVNamespace;
  /** The newest five iPhone builds, as `builds/<id>.ipa` (TOB-518). */
  BUILDS: R2Bucket;
  /** The Worker's address and the OAuth issuer, with no trailing slash. */
  PUBLIC_URL: string;
  /** 32 random bytes in base64: the key for the Readwise token. */
  SECRET_KEY: string;
  /** The one Google account that may sign in, by its email. Unset, sign-in only shows the email. */
  OWNER_EMAIL?: string;
  /** What `apple/ota.sh` sends as its bearer token to upload a build. Unset, no upload is taken. */
  BUILD_UPLOAD_TOKEN?: string;
  /** Bearer or form token for `POST /save` from a bookmarklet or Shortcut. Unset, `/save` refuses every request. */
  SAVE_TOKEN?: string;
  /** The address Cloudflare Email Routing delivers newsletters to. Defaults to `read@tobygodat.com`. */
  NEWSLETTER_ADDRESS?: string;
}

/** What every grant carries. There is one writer, so this is the same for every token. */
export interface Owner {
  /** The owner's email, as Google verified it. */
  sub: string;
}
