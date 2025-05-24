default: release

docker: release
	docker build -f docker/Dockerfile -t zinit-ubuntu:18.04 target/x86_64-unknown-linux-musl/release

prepare:
	rustup target  add x86_64-unknown-linux-musl

release: prepare
	cargo build --release --target=x86_64-unknown-linux-musl

release-aarch64-musl: prepare-aarch64-musl
	cargo build --release --target=aarch64-unknown-linux-musl

prepare-aarch64-musl:
	rustup target add aarch64-unknown-linux-musl
