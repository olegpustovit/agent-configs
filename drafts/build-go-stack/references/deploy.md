# Deploying a Go service

Use this reference for the release path, binary swap, health checks, rollback, and database backups. Follow the repository's `Makefile`, deploy script, and service-manager configuration instead of building a second deployment system.

## Build and ship from the dev machine

For the normal release, run `make release` from a clean `dev` checkout. It runs `make check`, fast-forwards `production` to `dev`, pushes both branches, then deploys the production checkout. The `make deploy` target calls `scripts/deploy.sh` directly and therefore requires a clean `production` checkout whose `HEAD` matches `production`.

The deploy script builds on the dev machine. Its Linux build target produces the server binary; the web build embeds the frontend and runtime in that binary. The script copies the new server binary to the host as `bin/art.new`, along with the separate downloadable CLI builds and service files. The host does not compile the application.

Before swapping binaries, keep the current `bin/art` as `bin/art.prev`. Move `bin/art.new` to `bin/art`, then restart the existing smdctl service (or create it from `deploy/smdctl.yml` on first install). This keeps the rollback binary available while the new process starts.

## Check health and roll back

Wait for `/healthz` to return success, then check that `/api/config` reports the SHA of the release being deployed. The PostgreSQL service's health response also includes its schema version. Treat a failed restart, health check, or SHA check as a failed deploy: restore `bin/art.prev` and restart the service. The deploy script does this automatically; `scripts/deploy.sh --rollback` requests the same binary swap explicitly.

Schema changes must remain compatible with the previous binary so this swap is safe. See [persistence](persistence.md) for the migration and compatibility rules. The application applies its embedded migrations during startup; **no deploy step runs `migrate` or a migration tool**.

## Schedule PostgreSQL backups

Use the persistent daily backup task in `deploy/smdctl.yml` as the scheduling pattern. For PostgreSQL, configure its backup script to write a dated custom-format archive with `pg_dump -Fc` and remove database archives older than 14 days. Keep the weekly application-file archive for files that are not stored in PostgreSQL.

The deploy script copies the backup script and task configuration, reloads the user systemd units, and enables the task timer. Check task installation and recent runs with `smdctl tasks artifacts`; the task runner and YAML syntax are covered by the [smdctl-deploy skill](../../smdctl-deploy/SKILL.md).
