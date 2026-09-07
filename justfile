# Default recipe: list available commands
default:
    @just --list

# Format all code (Rust + Nix + Markdown + TOML + YAML)
fmt:
    treefmt

# Check formatting (Rust + Nix + Markdown + TOML + YAML)
fmt-check:
    treefmt --fail-on-change --no-cache

# Run clippy lints
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --all-features

secretspec_reason := "mxroute-rs live API test suite"

# Configure a provider once with `secretspec config global init`, then `secretspec set`
# each secret named in secretspec.toml; `just secrets-check` reports what is still
# missing. The reason is recorded by providers that keep an audit log, and secretspec
# refuses to read anything without one.
#
# Which provider is not decided here. Locally it is whatever the developer configured;
# CI sets SECRETSPEC_PROVIDER=env, since a runner has no keyring but does have the
# workflow's environment. Either way resolution goes through secretspec, so a missing
# secret fails with the same message in both places.
#
# The tests are #[ignore]d so `just test` skips them; --ignored is what opts in.
[doc("Run the tests that talk to the real API, with credentials from secretspec")]
live-test *args='':
    secretspec run --reason {{quote(secretspec_reason)}} -- \
        cargo test --all-features --test live -- --ignored --nocapture --test-threads 2 {{args}}

# Report which live-test credentials are missing from the configured provider
secrets-check:
    secretspec check --reason {{quote(secretspec_reason)}}

# Run the MCP server over stdio, with credentials from secretspec
mcp *args='':
    secretspec run --reason {{quote(secretspec_reason)}} -- \
        cargo run --quiet -p mxroute-mcp -- {{args}}

# Print the MCP tool schemas, which needs no credentials
mcp-tools *args='':
    cargo run --quiet -p mxroute-mcp -- --list-tools {{args}}

# Validate the plugin and marketplace manifests
#
# Kept out of `just ci`: claude is not in the devshell, so CI has no way to run it.
plugin-check:
    claude plugin validate ./plugin --strict
    claude plugin validate . --strict

# Build release
build:
    cargo build --release --all-features

# Generate documentation
doc *args='':
    cargo doc --no-deps --all-features {{args}}

readme_args := "--project-root crates/mxroute --input src/lib.rs --template ../../README.tpl"

# Regenerate README.md from README.tpl and the crate docs
readme:
    cargo readme {{readme_args}} | mdformat - > README.md

# Check README.md is in sync with README.tpl and the crate docs
readme-check:
    cargo readme {{readme_args}} | mdformat - | diff - README.md

# Assert the release tag names the version cargo would publish
check-version version:
    @pkgid="$(cargo pkgid -p mxroute)"; crate="v${pkgid##*#}"; \
    if [ "$crate" != "{{version}}" ]; then \
        echo "tag {{version}} does not match crate version $crate" >&2; exit 1; \
    fi
    @# The plugin carries the version twice more, and neither is inherited from the
    @# workspace. The marketplace copy is what decides whether an installed plugin is
    @# offered an update at all, so forgetting it means users silently never get one.
    @for f in plugin/.claude-plugin/plugin.json .claude-plugin/marketplace.json; do \
        found="v$(jq -r '.version // .plugins[0].version' "$f")"; \
        if [ "$found" != "{{version}}" ]; then \
            echo "tag {{version}} does not match $f version $found" >&2; exit 1; \
        fi; \
    done

# Run CI checks locally
ci: fmt-check lint test doc readme-check build
    @echo "All CI checks passed!"

# Clean build artifacts
clean:
    cargo clean
