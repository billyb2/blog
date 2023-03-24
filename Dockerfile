FROM nixos/nix

RUN nix-channel --update
RUN mkdir -p ~/.config/nix/
RUN echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf

EXPOSE 80/tcp

CMD nix run