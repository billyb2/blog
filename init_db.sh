#!/bin/sh
touch database.db
nix shell nixpkgs#sqlite -c sqlite3 database.db "create table visits ( path string primary key unique not null, num_visits int not null);"
