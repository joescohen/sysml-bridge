# Docker Infrastructure

The SysML v2 backend services (SysON + SMAPS API + PostgreSQL) run via Docker.

## Quick Start

```sh
cd docker
docker compose up -d
```

This starts three services:

| Port | Service       | Description                        |
|------|---------------|------------------------------------|
| 8080 | SysON         | Web-based SysML v2 modeling tool   |
| 9000 | sysml-api     | SysML v2 SMAPS REST API (Pilot)    |
| 5432 | PostgreSQL    | Model persistence                  |

## Remote Host Setup (optional)

If Docker runs on a different machine (e.g., a Windows desktop), tunnel the ports:

```sh
autossh -N \
  -L 8080:localhost:8080 \
  -L 9000:localhost:9000 \
  <your-remote-host>
```

Then set `SYSON_ENDPOINT=http://localhost:8080` in your `.env`.
