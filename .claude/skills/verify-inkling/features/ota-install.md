# OTA install

`apple/ota.sh` archives the native iPhone app as Inkling Dev (`com.tobygodat.inkling.dev`), signed for
development, uploads it to the Worker and prints a link good for 24 hours. The link opens an install
page whose Install button points iOS at a signed manifest, which points at the signed .ipa.

## Sub-features

- `ota-build` the Release archive and the development export succeed with the local identity and profile.
- `ota-upload` the Worker takes the .ipa with the upload token and answers with a link.
- `ota-page` the link answers 200 with `Install Inkling Dev` and `Cache-Control: no-store`.
- `ota-manifest` the page's manifest answers 200 with bundle ID `com.tobygodat.inkling.dev`.
- `ota-ipa` the manifest's .ipa answers 200 and is byte for byte the one built.
- `ota-signed` a link with a changed signature is refused with 403.

## How to get to it (user POV)

- Toby opens the link from `apple/ota.sh` on his iPhone in Safari and taps Install.

## Driving it with scripts/verify

Preconditions: the baseline in [README.md](README.md); the `Apple Development` identity in the login
Keychain and the `Inkling Dev Development` profile installed (`security find-identity -v -p codesigning`).

- **Build, upload, follow the link.** `scripts/verify ota`. It runs `apple/ota.sh` with
  `INKLING_SERVER` set to the local Worker and its throwaway upload token, then fetches the chain.
  Every line of `ota/checks.txt` is `ok`.
- **What the iPhone shows.** `ota/install-page.png` is the link opened in this run's Simulator's
  Safari, with the Install button.

## Gotchas

- The Simulator can't install an `itms-services` build, so the install itself is proven only on a
  registered iPhone with a real `apple/ota.sh` run, which uploads to production and is outside this skill.
- Safari's first launch on a new Simulator can outlast `simctl openurl`; `verify ota` opens the link
  a second time when the first times out.
- The archive and export land in `target/apple-ota/`, shared with a real `apple/ota.sh` run in the
  same checkout.
