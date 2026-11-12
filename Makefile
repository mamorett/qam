BIN_DIR := target/release

.PHONY: all
all: fmt lint test build

.PHONY: fmt
fmt:
	@echo "Formatting with cargo fmt..."
	@cargo fmt

.PHONY: fmt-check
fmt-check:
	@cargo fmt --check

.PHONY: lint
lint:
	@echo "Linting with cargo clippy..."
	@cargo clippy --all-targets -- -D warnings

.PHONY: test
test:
	@echo "Running tests..."
	@cargo test

.PHONY: check-no-ffmpeg
check-no-ffmpeg:
	@./ci/check-no-ffmpeg.sh

.PHONY: build
build:
	@echo "Building release binary..."
	@cargo build --release --bin qam

.PHONY: install
install: build
	@mkdir -p $$HOME/bin
	@cp $(BIN_DIR)/qam $$HOME/bin/qam
	@echo "Installed qam to $$HOME/bin/qam"

.PHONY: clean
clean:
	@cargo clean
