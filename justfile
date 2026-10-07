# Common tasks. `just --list` for the full set.

# `just deploy web`, `just deploy --dry-run`; secrets come from `.env.deploy`, and the script's
# header has the rest.
# Build every platform and push it to itch.io from this machine.
deploy *args="":
    ./scripts/deploy-itch.sh {{args}}
