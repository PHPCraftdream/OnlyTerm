.PHONY: all fmt build check test docs servedocs

all: build

test:
	cargo nextest run
	cargo nextest run -p onlyterm-escape-parser # no_std by default

check:
	cargo check
	cargo check -p onlyterm-escape-parser
	cargo check -p onlyterm-cell
	cargo check -p onlyterm-surface

build:
	cargo build $(BUILD_OPTS) -p onlyterm
	cargo build $(BUILD_OPTS) -p onlyterm-gui
	cargo build $(BUILD_OPTS) -p onlyterm-mux-server
	cargo build $(BUILD_OPTS) -p strip-ansi-escapes

fmt:
	cargo +nightly fmt

docs:
	ci/build-docs.sh

servedocs:
	ci/build-docs.sh serve
