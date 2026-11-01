.DEFAULT_GOAL := check

BINARY_NAME := ai-config-tui
INSTALL_PATH ?= /usr/local/bin
CARGO_FLAGS ?= --locked
BIN_SRC := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)/release/$(BINARY_NAME)
INSTALL_DEPS := $(if $(filter 0,$(shell id -u)),,build)
export INSTALL_PATH BIN_SRC

require_abs = case "$$INSTALL_PATH" in /*) ;; *) printf '%s\n' 'INSTALL_PATH must be an absolute path' >&2; exit 2;; esac

.PHONY: check fmt test build install uninstall help catalog

check:
	cargo fmt --all -- --check
	cargo clippy --all-targets $(CARGO_FLAGS) -- -D warnings
	cargo test $(CARGO_FLAGS)

fmt:
	cargo fmt --all

test:
	cargo test $(CARGO_FLAGS)

build:
	cargo build --release $(CARGO_FLAGS)

# Replace through a temporary file to avoid macOS code-signing cache conflicts.
install: $(INSTALL_DEPS)
	@set -eu; \
	$(require_abs); \
	test -f "$$BIN_SRC" || { printf '%s\n' 'Release binary not found. Run make build first.' >&2; exit 1; }; \
	mkdir -p "$$INSTALL_PATH"; \
	if [ -d "$$INSTALL_PATH/$(BINARY_NAME)" ]; then \
		printf '%s\n' "Install destination is a directory: $$INSTALL_PATH/$(BINARY_NAME)" >&2; \
		exit 1; \
	fi; \
	install_tmp=$$(mktemp "$$INSTALL_PATH/.$(BINARY_NAME).XXXXXX"); \
	trap 'rm -f "$$install_tmp"' 0; \
	trap 'exit 1' HUP INT TERM; \
	cp "$$BIN_SRC" "$$install_tmp"; \
	chmod 755 "$$install_tmp"; \
	mv -f "$$install_tmp" "$$INSTALL_PATH/$(BINARY_NAME)"; \
	printf '%s\n' "Installed $$INSTALL_PATH/$(BINARY_NAME)"

uninstall:
	@set -eu; $(require_abs); rm -f "$$INSTALL_PATH/$(BINARY_NAME)"

catalog:
	python3 scripts/update_catalog.py

help:
	@printf '%s\n' \
		'make / make check  Check formatting, run clippy and tests' \
		'make build         Build the release binary' \
		'make test          Run tests' \
		'make fmt           Format the code' \
		'make install       Build and install (default: /usr/local/bin)' \
		'make uninstall     Remove the installed binary' \
		'make help          Show this help' \
		'make catalog       Refresh settings from official references (network required)' \
		'' \
		'Override the install directory: make install INSTALL_PATH="$$HOME/.local/bin"'
