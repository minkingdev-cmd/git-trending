.PHONY: db db-down collect dev-all api admin web test

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
	cd backend && cargo test
	cd frontend && npm test
