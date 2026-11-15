.PHONY: firefly clear-firefly build test \
        register rotate admin-relink admin-sign resolve-current resolve-history keygen

SHARD := F1R3FLY_RUST_IMAGE=f1r3fly-rust:local docker compose -f ../docker/shard.yml -f docker/quorum.yml

# Spin up a local F1r3fly shard via the workspace's docker compose, from an
# image built from this checkout. The docker/quorum.yml overlay sets a 2-of-3
# identity registry admin quorum. Copies docker/.env from the shipped example
# on first run.
firefly:
	@if [ ! -f ../docker/.env ] && [ -f ../docker/.env.example ]; then \
		cp ../docker/.env.example ../docker/.env; \
	fi
	@if ! docker image inspect f1r3fly-rust:local >/dev/null 2>&1; then \
		echo "Building f1r3fly-rust:local..."; \
		cd .. && bash ./node/docker-commands.sh build-local; \
	fi
	$(SHARD) up

# Stop the shard, wipe all node data volumes and remove the local image, so
# the next `make firefly` rebuilds it. Useful when finality has
# fallen behind block production and the proposer starts refusing new blocks
# because too many unfinalized blocks have piled up above the last finalized
# tip. A fresh genesis resets the finalized tip to block 0 so reruns work
# again. Run between test campaigns when deploys pile up.
clear-firefly:
	$(SHARD) down -v
	-docker image rm f1r3fly-rust:local

build:
	cargo build --release

# Identity registry REG-AC suite. Requires the `make firefly` cluster on
# 127.0.0.1:40422 and F1R3FLY_SIGNING_KEY in the environment, a key with REV
# balance that signs the deploys. The admin keys are the dev keys of
# docker/quorum.conf.
test:
	cargo test --features integration_tests --test identity_registry -- --test-threads=1

# Registry CLI targets. All require F1R3FLY_SIGNING_KEY in the environment.
# IDENTITY_REGISTRY_URI defaults to the genesis registry
# (rho:system:identityRegistry).
# Op-specific args are passed as make variables.
#
#   make keygen
#   make register
#   make register SK=<32-byte hex> NONCE=<32-byte hex>
#   make rotate NAME=<32-byte hex> CURRENT_SK=<32-byte hex> NEW_SK=<32-byte hex> SEQ=<n>
#   make admin-sign ADMIN_SK=<32-byte hex> NAME=<32-byte hex> NEW_PK=<65-byte hex> SEQ=<n> EVIDENCE=<hex>
#   make admin-relink NAME=<32-byte hex> NEW_SK=<32-byte hex> SEQ=<n> EVIDENCE=<hex> \
#        [ADMIN_SKS="<sk1> <sk2>"] [ADMIN_SIGS="<pk1>:<sig1> <pk2>:<sig2>"]
#     ADMIN_SIGS takes the admin-sign output of each admin. Give at least one
#     of ADMIN_SKS and ADMIN_SIGS.
#   make resolve-current NAME=<32-byte hex>
#   make resolve-history NAME=<32-byte hex>

REGISTER_ARGS := $(if $(SK),--sk $(SK)) $(if $(NONCE),--nonce $(NONCE))

register:
	cargo run --bin registry_cli -- register $(REGISTER_ARGS)

rotate:
	cargo run --bin registry_cli -- rotate \
		--name $(NAME) \
		--current-sk $(CURRENT_SK) \
		--new-sk $(NEW_SK) \
		--seq $(SEQ)

admin-relink:
	cargo run --bin registry_cli -- admin-relink \
		--name $(NAME) \
		--new-sk $(NEW_SK) \
		--seq $(SEQ) \
		--evidence $(EVIDENCE) \
		$(foreach sk,$(ADMIN_SKS),--admin-sk $(sk)) \
		$(foreach sig,$(ADMIN_SIGS),--admin-sig $(sig))

admin-sign:
	cargo run --bin registry_cli -- admin-sign \
		--admin-sk $(ADMIN_SK) \
		--name $(NAME) \
		--new-pk $(NEW_PK) \
		--seq $(SEQ) \
		--evidence $(EVIDENCE)

resolve-current:
	cargo run --bin registry_cli -- resolve-current --name $(NAME)

resolve-history:
	cargo run --bin registry_cli -- resolve-history --name $(NAME)

keygen:
	cargo run --bin registry_cli -- keygen
