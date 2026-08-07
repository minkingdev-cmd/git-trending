.PHONY: db collect dev-all api admin web test stack stack-down docker-build sqlx-prepare

# 本地开发使用本机部署的 PostgreSQL（local-debug 栈，localhost:5432，trust 认证）。
# 禁止用 docker 起 PG 实例；该目标只做连通性检查与建库（幂等）。
PGHOST ?= localhost
PGPORT ?= 5432
PGUSER ?= postgres
PSQL := /opt/pgsql/bin/psql -h $(PGHOST) -p $(PGPORT) -U $(PGUSER) -d postgres

db:
	@$(PSQL) -tAc "SELECT 1;" >/dev/null || { echo "本地 PostgreSQL 未就绪（localhost:5432）"; exit 1; }
	@for db in ghtrending ghtrending_test ghtrending_test_collector ghtrending_test_api; do \
		$(PSQL) -tAc "SELECT 1 FROM pg_database WHERE datname='$$db'" | grep -q 1 \
			|| $(PSQL) -c "CREATE DATABASE $$db"; \
	done
	@echo "本地 PG 就绪，ghtrending* 库已存在"

collect:
	cd backend && cargo run -p ght-collector -- --once

dev-all:
	cd backend && cargo run -p ght-collector

api:
	cd backend && cargo run -p ght-api

admin:
	cd backend && cargo run -p ght-admin -- create-user --username admin --password change-me-now

web:
	cd frontend && npm run dev

test:
	cd backend && \
		DATABASE_URL=postgres://postgres@localhost:5432/ghtrending \
		DATABASE_URL_TEST=postgres://postgres@localhost:5432/ghtrending_test \
		DATABASE_URL_TEST_API=postgres://postgres@localhost:5432/ghtrending_test_api \
		DATABASE_URL_TEST_COLLECTOR=postgres://postgres@localhost:5432/ghtrending_test_collector \
		cargo test
	cd frontend && npm test

# Generate .sqlx offline data (requires local PG + DATABASE_URL)
sqlx-prepare:
	cd backend && DATABASE_URL=postgres://postgres@localhost:5432/ghtrending cargo sqlx prepare --workspace

docker-build:
	docker compose --profile stack build

# App stack via docker (PG 仍用本机实例，容器经 host.docker.internal 连接)
stack:
	docker compose --profile stack up -d --build api
	docker compose --profile stack run --rm collector || true

# Long-running collector (daily COLLECT_TIME) + api
stack-daemon:
	docker compose --profile stack --profile daemon up -d --build api collector-daemon

stack-down:
	docker compose --profile stack --profile daemon down
