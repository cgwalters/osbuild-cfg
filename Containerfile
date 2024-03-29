FROM quay.io/centos/centos:stream9 as builder
RUN --mount=type=cache,target=/var/cache/dnf,z dnf -y install cargo
COPY . /src
WORKDIR /src
RUN --mount=type=cache,target=/build/target --mount=type=cache,target=/var/roothome cargo build --release

FROM registry.access.redhat.com/ubi9/ubi:latest
COPY --from=builder /src/target/release/osbuild-cfg /usr/bin
