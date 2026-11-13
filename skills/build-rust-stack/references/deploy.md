# Deploying a Rust service

Use this reference for the release path, cross-compilation, binary swap, health checks, rollback, and database backups. Follow the repository's `Makefile`, deploy script, and service-manager configuration instead of building a second deployment system.

## Build and ship from the dev machine

For the normal release, run `make release` from a clean `dev` checkout. It runs the full check, fast-forwards `production` to `dev`, pushes both branches, then deploys the production checkout.

The deploy script builds on the dev machine with the release profile and copies the binary to the host. The host does not compile the application; a Rust release build needs far more memory and time than a small server should spend.

- Build for the host's target triple. When the dev machine and host differ, cross-compile with `cargo zigbuild --release --target <triple>`.
- Default to the glibc target with `mimalloc` as the global allocator. Use `cargo zigbuild` with a glibc version suffix no newer than the host's so the binary loads there.
- Choose the musl target only when a fully static binary is required, and always set a global allocator on it.
- Embed the built frontend in the server binary so the release is one file.
- Stamp the build SHA into the binary at compile time and report it from `/api/config` or the health endpoint.

## Swap, check, roll back

Copy the new binary to the host as `bin/<app>.new`. Before swapping, keep the current `bin/<app>` as `bin/<app>.prev`. Move the new binary into place, then restart the existing smdctl service (or create it from `deploy/smdctl.yml` on first install).

Wait for `/healthz` to return success, then check that the service reports the SHA being deployed and the expected schema version. Treat a failed restart, health check, or SHA check as a failed deploy: restore `bin/<app>.prev` and restart. The deploy script does this automatically and offers `--rollback` for the same swap on request.

Schema changes must remain compatible with the previous binary so this swap is safe. See [persistence.md](persistence.md). The application applies its embedded migrations during startup; no deploy step runs a migration tool.

## Service settings

- Give the service a stop timeout longer than the application's shutdown grace period so in-flight requests drain.
- Run it with `no_new_privileges`, a raised file-descriptor limit, and `restart: always`.
- Bind to loopback and let Caddy terminate TLS and proxy to it.
- Log to stdout in JSON; the service manager collects it.

## Schedule PostgreSQL backups

Use a persistent daily task in `deploy/smdctl.yml`. The backup script writes a dated custom-format archive with `pg_dump -Fc` and removes archives older than 14 days. Check task installation and recent runs with `smdctl tasks <service>`; the task runner and YAML syntax are covered by the [smdctl-deploy skill](../../smdctl-deploy/SKILL.md).
