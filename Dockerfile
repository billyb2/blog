# syntax = docker/dockerfile:1

FROM nixos/nix as builder
RUN mkdir -p /build/
WORKDIR /build/
RUN nix-channel --update
RUN mkdir -p ~/.config/nix/
RUN echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
COPY ./Cargo.lock .
COPY ./Cargo.toml .
COPY ./flake.nix .
COPY ./flake.lock .
COPY ./rust-toolchain .
COPY ./.cargo/ ./.cargo/
COPY ./static/ ./static/
COPY ./src ./src/
RUN nix build

COPY ./init_db.sh .
RUN ./init_db.sh

COPY ./md/ ./md/

RUN nix-collect-garbage

EXPOSE 80
CMD ["./result/bin/blog"]