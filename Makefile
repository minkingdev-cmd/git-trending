.PHONY: db db-down collect dev-all api admin web test stack stack-down docker-build sqlx-prepare

db:
	docker compose up -d postgres
	until docker compose exec -T postgres pg_isready -U ght >/dev/null 2>&1; do sleep 0.5; done

db-down:
	docker compose down

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
	cd backend && DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending cargo test
	cd frontend && npm test

# Generate .sqlx offline data (requires make db + DATABASE_URL)
sqlx-prepare:
	cd backend && DATABASE_URL=postgres://ght:ght@localhost:5433/ghtrending cargo sqlx prepare --workspace

docker-build:
	docker compose --profile stack build

# Full stack: postgres + api + one-shot collector
stack:
	docker compose --profile stack up -d --build postgres api
	docker compose --profile stack run --rm collector || true

stack-down:
	docker compose --profile stack down
