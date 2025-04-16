# Zinit Service Configuration Examples

This document provides practical examples of Zinit service configurations for common use cases. These examples can be used as templates for your own services.

## Basic Services

### Simple Background Service

A basic service that runs in the background:

```yaml
# /etc/zinit/simple.yaml
exec: "/usr/bin/myservice --daemon"
```

### Web Server

A typical web server configuration:

```yaml
# /etc/zinit/nginx.yaml
exec: "/usr/sbin/nginx -g 'daemon off;'"
test: "curl -s http://localhost > /dev/null"
```

### Database Server

A database server with health check:

```yaml
# /etc/zinit/postgres.yaml
exec: "/usr/lib/postgresql/13/bin/postgres -D /var/lib/postgresql/13/main"
test: "pg_isready -q"
dir: "/var/lib/postgresql"
env:
  PGDATA: "/var/lib/postgresql/13/main"
```

## One-shot Services

### Initialization Service

A service that runs once during startup:

```yaml
# /etc/zinit/init-system.yaml
exec: "sh -c 'echo System initialization started; sleep 2; echo Done'"
oneshot: true
```

### Database Migration

Run database migrations once:

```yaml
# /etc/zinit/db-migrate.yaml
exec: "/usr/local/bin/migrate-database"
oneshot: true
after:
  - postgres
env:
  DB_HOST: "localhost"
  DB_USER: "postgres"
```

### Cleanup Service

A cleanup service that runs once:

```yaml
# /etc/zinit/cleanup.yaml
exec: "/usr/local/bin/cleanup-temp-files"
oneshot: true
```

## Services with Dependencies

### API Service Depending on Database

An API service that depends on a database:

```yaml
# /etc/zinit/api.yaml
exec: "/usr/local/bin/api-server --port 8080"
test: "curl -s http://localhost:8080/health > /dev/null"
after:
  - postgres
  - redis
```

### Multi-tier Application

A complex application with multiple tiers:

```yaml
# /etc/zinit/database.yaml
exec: "/usr/bin/mysqld"
test: "mysqladmin ping -h localhost"
```

```yaml
# /etc/zinit/cache.yaml
exec: "/usr/bin/redis-server /etc/redis/redis.conf"
test: "redis-cli ping"
```

```yaml
# /etc/zinit/backend.yaml
exec: "/usr/local/bin/backend-app"
test: "curl -s http://localhost:5000/health"
after:
  - database
  - cache
```

```yaml
# /etc/zinit/frontend.yaml
exec: "/usr/local/bin/frontend-app"
test: "curl -s http://localhost:3000 > /dev/null"
after:
  - backend
```

## Logging Configuration

### Service with Stdout Logging

Direct service output to Zinit's stdout:

```yaml
# /etc/zinit/logging-stdout.yaml
exec: "/usr/local/bin/verbose-service"
log: stdout
```

### Service with Ring Buffer Logging

Store logs in the kernel ring buffer (default):

```yaml
# /etc/zinit/logging-ring.yaml
exec: "/usr/local/bin/service-with-logs"
log: ring
```

### Service with No Logging

Discard all service output:

```yaml
# /etc/zinit/silent.yaml
exec: "/usr/local/bin/quiet-service"
log: null
```

## Custom Signal Handling

### Service with Custom Stop Signal

Use a specific signal to stop a service:

```yaml
# /etc/zinit/graceful.yaml
exec: "/usr/local/bin/graceful-app"
signal:
  stop: SIGINT  # Use SIGINT instead of SIGTERM
```

### Service with Long Shutdown Time

Allow a service more time to shut down:

```yaml
# /etc/zinit/slow-shutdown.yaml
exec: "/usr/local/bin/slow-to-stop-app"
shutdown_timeout: 60  # 60 seconds instead of default 10
```

## Working Directory and Environment

### Service with Custom Working Directory

Set a specific working directory:

```yaml
# /etc/zinit/custom-dir.yaml
exec: "./start.sh"
dir: "/opt/myapp"
```

### Service with Environment Variables

Provide environment variables to a service:

```yaml
# /etc/zinit/env-vars.yaml
exec: "/usr/local/bin/configurable-app"
env:
  PORT: "8080"
  DEBUG: "true"
  CONFIG_PATH: "/etc/myapp/config.json"
  NODE_ENV: "production"
```

## Container Services

### Main Container Application

The primary application in a container:

```yaml
# /etc/zinit/app.yaml
exec: "/app/entrypoint.sh"
log: stdout
```

### Sidecar Services

Additional services in the same container:

```yaml
# /etc/zinit/sidecar.yaml
exec: "/usr/local/bin/sidecar-service"
after:
  - app
log: stdout
```

## System Services

### Network Setup

Configure networking:

```yaml
# /etc/zinit/networking.yaml
exec: "sh -c 'ip link set eth0 up && dhclient eth0'"
oneshot: true
```

### SSH Server

Run SSH daemon:

```yaml
# /etc/zinit/sshd.yaml
exec: "/usr/sbin/sshd -D"
after:
  - networking
```

### Cron Service

Run cron daemon:

```yaml
# /etc/zinit/cron.yaml
exec: "/usr/sbin/cron -f"
```

## Testing in Containers

### Redis Test Instance

A Redis instance for testing:

```yaml
# /etc/zinit/redis-test.yaml
exec: "redis-server --port 7777"
test: "redis-cli -p 7777 PING"
```

### Initialization and Population

```yaml
# /etc/zinit/redis-init.yaml
exec: "sh -c 'echo Prepare DB files for Redis'"
oneshot: true
```

```yaml
# /etc/zinit/redis.yaml
exec: "redis-server --port 7777"
test: "redis-cli -p 7777 PING"
after:
  - redis-init
```

```yaml
# /etc/zinit/redis-after.yaml
exec: "sh -c 'echo Populating Redis with seed data; redis-cli -p 7777 SET key value'"
oneshot: true
after:
  - redis
```

## Practical Applications

### Node.js API Server

```yaml
# /etc/zinit/nodejs-api.yaml
exec: "node /app/server.js"
test: "curl -s http://localhost:3000/health > /dev/null"
dir: "/app"
env:
  PORT: "3000"
  NODE_ENV: "production"
  DB_URL: "mongodb://localhost:27017/mydb"
```

### Python Web Application

```yaml
# /etc/zinit/python-app.yaml
exec: "gunicorn --workers 4 --bind 0.0.0.0:5000 app:app"
test: "curl -s http://localhost:5000/health"
dir: "/app/python"
env:
  FLASK_ENV: "production"
  DATABASE_URL: "postgresql://user:pass@localhost/db"
```

### Golang Service

```yaml
# /etc/zinit/go-service.yaml
exec: "/app/bin/service"
test: "curl -s http://localhost:8080/ping"
env:
  CONFIG_FILE: "/etc/app/config.json"
  LOG_LEVEL: "info"
```

## Advanced Configurations

### Service with Health Check Script

Using an external script for health checking:

```yaml
# /etc/zinit/health-script.yaml
exec: "/usr/local/bin/complex-service"
test: "/usr/local/bin/health-check.sh"
```

### Dependency Chain with Timeouts

A series of services with custom timeouts:

```yaml
# /etc/zinit/service1.yaml
exec: "/bin/service1"
shutdown_timeout: 5
```

```yaml
# /etc/zinit/service2.yaml
exec: "/bin/service2"
after:
  - service1
shutdown_timeout: 10
```

```yaml
# /etc/zinit/service3.yaml
exec: "/bin/service3"
after:
  - service2
shutdown_timeout: 15