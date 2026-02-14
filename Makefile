.PHONY: schema codegen dev

schema:
	cargo run --bin export-schema

codegen: schema
	cd packages/api-client-js && pnpm codegen

dev:
	@echo "Starting taproot dev environment..."
	cargo run --bin taproot-server
