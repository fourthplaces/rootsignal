.PHONY: schema codegen dev dev-admin

schema:
	cargo run --bin export-schema

codegen: schema
	cd modules/api-client-js && pnpm codegen

dev:
	@echo "Starting Root Signal server on :9081..."
	cargo run --bin rootsignal-server

dev-admin:
	@echo "Starting admin app on :3000..."
	cd modules/admin-app && pnpm dev
