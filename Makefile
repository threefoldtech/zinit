docker: release
	docker build -f Dockerfile -t zinit target/release

release:
	cargo build --release
