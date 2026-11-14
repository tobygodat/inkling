import type { GoogleConfig } from "./google";

export interface Env extends GoogleConfig {
  DB: D1Database;
  /** The OAuth provider's own records. */
  OAUTH_KV: KVNamespace;
  /** Parsed books, covers, and Readwise files waiting in the inbox. */
  FILES: KVNamespace;
  /** The Worker's address and the OAuth issuer, with no trailing slash. */
  PUBLIC_URL: string;
  /** 32 random bytes in base64: the key for the Readwise token and webhook secret. */
  SECRET_KEY: string;
  /** The one Google account that may sign in, by its email. Unset, sign-in only shows the email. */
  OWNER_EMAIL?: string;
}

/** What every grant carries. There is one writer, so this is the same for every token. */
export interface Owner {
  /** The owner's email, as Google verified it. */
  sub: string;
}
