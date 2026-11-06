BIN     := bus
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
TAG     ?= v$(VERSION)
TARGET  ?= x86_64-unknown-linux-musl
DIST    := dist
TARBALL := $(DIST)/$(BIN)-$(TAG)-x86_64-linux.tar.gz

.PHONY: help build test fmt lint ci release-build dist check-tag version release clean

help: ## Show targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-14s %s\n", $$1, $$2}'

build: ## Debug build
	cargo build

test: ## Run tests
	cargo test

fmt: ## Format sources
	cargo fmt

lint: ## Check formatting and clippy (as CI does)
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

ci: lint test ## Everything CI checks

release-build: ## Static release binary (needs musl-tools)
	cargo build --release --target $(TARGET)

dist: check-tag release-build ## Package release tarball into dist/
	mkdir -p $(DIST)
	tar -czf $(TARBALL) -C target/$(TARGET)/release $(BIN)
	@echo "Built $(TARBALL)"

check-tag: ## Fail if TAG doesn't match the Cargo.toml version
	@test "$(TAG)" = "v$(VERSION)" || { echo "TAG $(TAG) != Cargo.toml version v$(VERSION)"; exit 1; }

version: ## Print the current version
	@echo $(VERSION)

release: ## Bump (BUMP=patch|minor|major|X.Y.Z), commit, tag and push; no BUMP releases as-is
	@scripts/release.sh $(BUMP)

clean: ## Remove build output
	cargo clean
	rm -rf $(DIST)
