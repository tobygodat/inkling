# Vendored anti-slop

Source: [tobygodat/TGstack](https://github.com/tobygodat/TGstack), `skills/install-anti-slop/assets/anti-slop/`
at commit `9050c5466961bfaba6831166f3cedafb13a5689f`. The copy here was checked byte for byte against
`git archive` of that commit when it was installed (TOB-501).

Installed paths:

- `index.ts`, `rules/` and `shared/`: the generic plugin, registered in `server/.oxlintrc.json` as `anti-slop`.
- `effect/`: the Effect plugin, copied with the rest but not registered, since the Worker doesn't
  depend on `effect`.
- `vendor/eslint-stylistic/`: the padding rule behind `require-readable-spacing`, with its own
  `LICENSE` and `UPSTREAM.md`.

Runs on `oxlint` and `@oxlint/plugins` 1.93.0, pinned exactly in `server/package.json`. Upgrade both together.

No local changes.
