# Getting started
The build system is heavily based around [Nix](https://nixos.org/). To get started, [install nix](https://nixos.org/download.html) and [enable flakes](https://nixos.wiki/wiki/Flakes). Next, run `nix develop` to get a working development environment. Finally, run `./init_db.sh` to initialize the database. If you want to run a release build of the server, run `nix run`.

