# {{crate}}

{{readme}}

## Development

Everything runs inside `nix develop` (or `direnv allow`, which enters it for you).

```bash
just         # list the recipes
just ci      # what CI runs: fmt-check, lint, test, doc, readme-check, build
just test    # the mocked suite; no network, no sleeping
```

`README.md` is generated from the crate documentation, so edit `src/lib.rs` and run
`just readme`. A pre-push hook checks the two agree.

### Testing against the real API

The live suite is `#[ignore]`d and needs credentials for an actual account. Those are
declared in `secretspec.toml` and resolved through [secretspec], so nothing secret is
committed and each developer picks where the values live.

```bash
secretspec config global init      # once: choose a provider, e.g. the system keyring
secretspec set MXROUTE_SERVER      # then each secret secretspec.toml names
secretspec set MXROUTE_USERNAME
secretspec set MXROUTE_API_KEY
just secrets-check                 # reports what is still missing
just live-test
```

Set `MXROUTE_TEST_DOMAIN` to a scratch domain to include the tests that write; they clean
up after themselves. `MXROUTE_TEST_SPAM` and `MXROUTE_TEST_RESELLER` opt into the two
groups that reach beyond one domain.

On Linux the keyring provider talks to the Secret Service over D-Bus, so it needs
gnome-keyring or KWallet running and unlocked. Any other provider works just as well;
`secretspec.toml` does not care which.

[secretspec]: https://secretspec.dev

## License

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))
