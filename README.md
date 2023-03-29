# zinit [![Rust](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml/badge.svg)](https://github.com/threefoldtech/zinit/actions/workflows/rust.yml)
A POC PID 1 replacement that feels like runit written in rust+tokio

## Goal
A PID replacement that is very lightweight and provide the following requirements
- Make sure that configured services are up and running at all times
- Support service dependencies during the boot process
- Provide a simple command line interface to add, start, stop and reload services

## Test docker image
To play with zinit, we have a testing docker image you can build easily by typing `make docker`.
The test image currently auto starts redis and open-sshd, it doesn't create key or change passwords (please check [Dockerfile](Dockerfile)).

## Local build notes
- To build locally you can use `make` 
- Build requires `rust` and `musl`, `musl-tools` installed
- Preferred to build with rust version `cargo 1.46.0 (149022b1d 2020-07-17)`
