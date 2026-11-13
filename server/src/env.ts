import type { AppleConfig } from "./apple";

export interface Env extends AppleConfig {
  DB: D1Database;
  /** The OAuth provider's own records. */
  OAUTH_KV: KVNamespace;
  /** Parsed books, covers, and Readwise files waiting in the inbox. */
  FILES: KVNamespace;
  /** The Worker's address and the OAuth issuer, with no trailing slash. */
  PUBLIC_URL: string;
  /** 32 random bytes in base64: the key for the Readwise token and webhook secret. */
  SECRET_KEY: string;
  /** The one Apple ID that may sign in. Unset, sign-in only shows the Apple ID's subject. */
  OWNER_APPLE_SUB?: string;
}

/** What every grant carries. There is one writer, so this is the same for every token. */
export interface Owner {
  sub: string;
}
