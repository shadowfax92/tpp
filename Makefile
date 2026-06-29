BIN := tpp
CARGO ?= cargo
LOCAL_BINDIR ?= bin
PREFIX ?= $(HOME)
INSTALL_BINDIR ?= $(PREFIX)/bin
FISH_DIR ?= $(HOME)/.config/fish/completions
SRC := $(shell find src -type f -name '*.rs')

.PHONY: all build install fish completions test fmt lint clean

all: build

build: $(LOCAL_BINDIR)/$(BIN)

$(LOCAL_BINDIR)/$(BIN): Cargo.toml Cargo.lock $(SRC)
	$(CARGO) build --release
	mkdir -p "$(LOCAL_BINDIR)"
	cp "target/release/$(BIN)" "$@"

# Build, install to PATH, and codesign so macOS doesn't kill a re-signed binary mid-run.
install: build
	install -d "$(INSTALL_BINDIR)"
	install -m 755 "$(LOCAL_BINDIR)/$(BIN)" "$(INSTALL_BINDIR)/$(BIN)"
	@codesign --force --sign - "$(INSTALL_BINDIR)/$(BIN)" 2>/dev/null || true
	@echo "installed $(BIN) -> $(INSTALL_BINDIR)/$(BIN)"

# Generate + install fish completions from the binary itself.
fish: build
	install -d "$(FISH_DIR)"
	"$(LOCAL_BINDIR)/$(BIN)" completions fish > "$(FISH_DIR)/$(BIN).fish"
	@echo "installed fish completions -> $(FISH_DIR)/$(BIN).fish"

test:
	$(CARGO) test

fmt:
	$(CARGO) fmt

lint:
	$(CARGO) clippy --all-targets -- -D warnings

clean:
	rm -rf "$(LOCAL_BINDIR)"
	$(CARGO) clean
