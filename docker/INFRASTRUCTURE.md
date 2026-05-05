# Docker Infrastructure

All Docker services run on the Windows desktop (`10.0.0.111`).

The compose file lives at `C:\Users\jsc61\syson\docker-compose.yml` on that machine.

## Port Tunnel (Mac → Windows)

```sh
autossh -N \
  -L 8080:localhost:8080 \
  -L 9000:localhost:9000 \
  -L 8888:localhost:8888 \
  winserver
```

| Local port | Service   |
|------------|-----------|
| 8080       | SysON     |
| 9000       | sysml-api |
| 8888       | Jupyter   |
