# syntax = docker/dockerfile:1

FROM nixos/nix
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
RUN mv ./result/bin/blog ./blog

COPY ./init_db.sh .
RUN ./init_db.sh

COPY ./md/ ./md/

FROM alpine:latest
RUN mkdir -p /build/
WORKDIR /build/
COPY --from=0 /build/blog .
RUN chmod +x ./blog
COPY --from=0 /build/static/ ./static/
COPY --from=0 /build/md/ ./md/
COPY --from=0 /build/database.db .

EXPOSE 80
CMD ["./blog"]
