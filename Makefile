default: release

docker: release
	docker build -f docker/Dockerfile -t zinit-ubuntu:18.04 target/release

prepare:
	rustup target  add x86_64-unknown-linux-musl

release: prepare
	cargo build --release --target=x86_64-unknown-linux-musl
