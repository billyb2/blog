FROM nixos/nix as builder
WORKDIR /build
RUN nix-channel --update
RUN mkdir -p ~/.config/nix/
RUN echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
COPY . .
RUN nix build && cp ./result/bin/blog

FROM debian:bullseye-slim
RUN mkdir -p /app/
WORKDIR /app/
COPY --from=builder /build/result/bin/blog .
COPY --from=builder /build/blog .
COPY --from=builder /build/md .
COPY --from=builder /build/static .

EXPOSE 80
ENTRYPOINT ["./blog"]
