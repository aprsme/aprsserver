SHELL := /bin/bash

.PHONY: dev run test

dev:
	@command -v cargo-watch >/dev/null 2>&1 || cargo install cargo-watch
	cargo watch -x run

run:
	cargo run

test:
	cargo test 