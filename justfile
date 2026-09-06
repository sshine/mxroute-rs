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

# Run the tests that talk to the real API, with credentials from secretspec.
#
# Configure a provider once with `secretspec config global init`, then `secretspec set`
# each secret named in secretspec.toml. `secretspec check --reason=...` reports what is
# still missing. The reason is recorded by providers that keep an audit log, and
# secretspec refuses to read anything without one.
#
# These are #[ignore]d so `just test` and CI skip them; --ignored is what opts in.
live-test *args='':
    secretspec run --reason {{quote(secretspec_reason)}} -- just live-test-inner {{args}}

# Report which live-test credentials are missing from the configured provider
secrets-check:
    secretspec check --reason {{quote(secretspec_reason)}}

# The live suite without the secretspec wrapper, for callers that supply the environment
# themselves. CI does, because a GitHub runner has no keyring to read from.
[private]
live-test-inner *args='':
    cargo test --all-features --test live -- --ignored --nocapture --test-threads 2 {{args}}

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

# Run CI checks locally
ci: fmt-check lint test doc readme-check build
    @echo "All CI checks passed!"

# Clean build artifacts
clean:
    cargo clean
