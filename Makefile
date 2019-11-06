default: release

docker: release
	docker build -f docker/Dockerfile -t zinit-ubuntu:18.04 target/release

release:
	cargo build --release
