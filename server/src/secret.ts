// The Readwise token and webhook secret are kept as AES-256-GCM ciphertext under SECRET_KEY, a
// Worker secret of 32 random bytes in base64. Reading D1 alone reveals neither.

const fromBase64 = (text: string) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
const toBase64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));

async function key(secretKey: string): Promise<CryptoKey> {
  const raw = fromBase64(secretKey);
  if (raw.length !== 32) throw new Error("SECRET_KEY must be 32 bytes in base64.");
  return crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["encrypt", "decrypt"]);
}

/** `<iv>.<ciphertext>`, both base64. */
export async function seal(secretKey: string, plain: string): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const sealed = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, await key(secretKey), new TextEncoder().encode(plain));
  return `${toBase64(iv)}.${toBase64(new Uint8Array(sealed))}`;
}

export async function open(secretKey: string, sealed: string): Promise<string> {
  const [iv, data] = sealed.split(".");
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: fromBase64(iv) }, await key(secretKey), fromBase64(data));
  return new TextDecoder().decode(plain);
}

/** Compares two secrets without the time taken saying where they first differ. */
export async function sameSecret(a: string, b: string): Promise<boolean> {
  const digest = async (s: string) => new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s)));
  const [x, y] = await Promise.all([digest(a), digest(b)]);
  let difference = 0;
  for (let i = 0; i < x.length; i++) difference |= x[i] ^ y[i];
  return difference === 0;
}
