# PostgreSQL persistence

Use this reference before adding or changing persistence. PostgreSQL is the default. Choose SQLite only when the application must be a single local file with no database service; choose one database when the app is scaffolded and do not support both dialects in one application.

## Rules

1. **PostgreSQL by default.** Use SQLite only for a true single-file app. Keep existing applications on their current database unless a migration is explicitly requested.
2. **One application connection setting:** `<APP>_DATABASE_URL`. Read it into typed configuration, validate it before startup, and never log credentials from it. Tests may use a separate test-only `TEST_DATABASE_URL` pointing at a maintenance database.
3. **Startup order:** load configuration → open the pool (create the application database if missing) → apply embedded migrations under a PostgreSQL advisory lock → construct repositories and services → listen. If opening or migrating fails, startup exits without serving.
4. **Forward-only migrations:** keep SQL in `internal/infrastructure/postgres/migrations/NNNN_name.sql` and embed it with `//go:embed`. Never edit an applied migration. Do not add a migration command or Make target; the application applies migrations at startup.
5. **Keep releases rollback-safe.** A release may add tables, nullable columns, and indexes. Defer dropping or renaming schema until a later release so the previous binary can still run after a binary rollback.
6. **Own SQL in the component.** Put the `Repository` interface and `PostgresRepository` in the component's flat `repository.go`, constructed with `NewPostgresRepository(pool *pgxpool.Pool)`. Keep SQL as constants beside their methods. Use `pgx.NamedArgs`, `pgx.CollectRows`, and `pgx.RowToStructByName`; never concatenate input into SQL.
7. **One repository method is one transaction.** Use `pgx.BeginFunc`. A transaction stays inside one component. Put writes that must commit together under the component that owns them; never span components with one transaction.
8. **Translate errors in the repository.** Map `pgx.ErrNoRows` to the component's `ErrNotFound` and PostgreSQL SQLSTATE `23505` to its `ErrConflict`. Do not let `pgconn` error types escape the component. Preserve context cancellation and deadline errors.
9. **Column conventions:** application-generated text IDs, `timestamptz` timestamps, `jsonb` for structured JSON, text plus `CHECK` constraints instead of enum types, pointers for nullable fields, and keyset pagination.
10. **Search:** use a stored generated `tsvector` column with a GIN index, `websearch_to_tsquery`, and `ts_headline`. SQLite uses FTS5 kept current with triggers.
11. **Expiry:** PostgreSQL has no TTL indexes. One in-process sweeper deletes expired rows hourly.
12. **Repository tests:** run against real PostgreSQL. `pgtest.New(t)` gives each test a disposable database cloned from a migrated template and drops it during cleanup. Keep pure business tests independent of the database.

## Dependencies and migration API

For a Go 1.25 application, pin `github.com/pressly/goose/v3` to **v3.27.0**. Goose v3.28.0 and later in this release line declare Go 1.26; upgrade the pin only after the application toolchain moves to Go 1.26 or newer. Use `github.com/jackc/pgx/v5` and `pgxpool` for PostgreSQL access.

The worked module below uses these direct requirements:

### `go.mod`

```go
module example.com/notes

go 1.25.0

require (
	github.com/jackc/pgx/v5 v5.12.0
	github.com/pressly/goose/v3 v3.27.0
)
```

## Worked component: `note`

This example stores one note aggregate. Copy the files into a module with the module path `example.com/notes`; the repository contract test and the concurrent migration test run against PostgreSQL.

### `internal/infrastructure/postgres/migrations/0001_init.sql`

```sql
-- +goose Up
CREATE TABLE notes (
    id         text PRIMARY KEY,
    title      text NOT NULL,
    body       text NOT NULL DEFAULT '',
    version    bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE INDEX notes_updated_at_idx ON notes (updated_at DESC, id DESC);
```

There is no `Down` section: applied schema changes are forward-only.

### `internal/components/note/models.go`

```go
package note

import "time"

type Note struct {
	ID        string    `db:"id" json:"id"`
	Title     string    `db:"title" json:"title"`
	Body      string    `db:"body" json:"body"`
	Version   int64     `db:"version" json:"version"`
	CreatedAt time.Time `db:"created_at" json:"created_at"`
	UpdatedAt time.Time `db:"updated_at" json:"updated_at"`
}
```

### `internal/components/note/repository.go`

```go
package note

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

var (
	ErrNotFound = errors.New("note not found")
	ErrConflict = errors.New("note already exists")
)

type Repository interface {
	Create(context.Context, Note) error
	ByID(context.Context, string) (Note, error)
	List(context.Context, string, int) ([]Note, error)
	Update(context.Context, Note) error
	Delete(context.Context, string) error
}

type PostgresRepository struct {
	pool *pgxpool.Pool
}

var _ Repository = (*PostgresRepository)(nil)

func NewPostgresRepository(pool *pgxpool.Pool) (*PostgresRepository, error) {
	if pool == nil {
		return nil, errors.New("database pool is nil")
	}
	return &PostgresRepository{pool: pool}, nil
}

const createNoteSQL = `
INSERT INTO notes (id, title, body, version, created_at, updated_at)
VALUES (@id, @title, @body, @version, @created_at, @updated_at)`

func (r *PostgresRepository) Create(ctx context.Context, note Note) error {
	err := pgx.BeginFunc(ctx, r.pool, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, createNoteSQL, pgx.NamedArgs{
			"id": note.ID, "title": note.Title, "body": note.Body,
			"version": note.Version, "created_at": note.CreatedAt, "updated_at": note.UpdatedAt,
		})
		return translateError(err)
	})
	if err != nil {
		return fmt.Errorf("create note: %w", translateError(err))
	}
	return nil
}

const getNoteSQL = `
SELECT id, title, body, version, created_at, updated_at
FROM notes
WHERE id = @id`

func (r *PostgresRepository) ByID(ctx context.Context, id string) (Note, error) {
	var note Note
	err := pgx.BeginFunc(ctx, r.pool, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, getNoteSQL, pgx.NamedArgs{"id": id})
		if err != nil {
			return err
		}
		note, err = pgx.CollectOneRow(rows, pgx.RowToStructByName[Note])
		return translateError(err)
	})
	if err != nil {
		return Note{}, fmt.Errorf("find note: %w", translateError(err))
	}
	return note, nil
}

const listNotesSQL = `
SELECT id, title, body, version, created_at, updated_at
FROM notes
WHERE id > @after_id
ORDER BY id
LIMIT @limit`

func (r *PostgresRepository) List(ctx context.Context, afterID string, limit int) ([]Note, error) {
	var notes []Note
	err := pgx.BeginFunc(ctx, r.pool, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, listNotesSQL, pgx.NamedArgs{"after_id": afterID, "limit": limit})
		if err != nil {
			return err
		}
		notes, err = pgx.CollectRows(rows, pgx.RowToStructByName[Note])
		return translateError(err)
	})
	if err != nil {
		return nil, fmt.Errorf("list notes: %w", translateError(err))
	}
	return notes, nil
}

const updateNoteSQL = `
UPDATE notes
SET title = @title, body = @body, version = version + 1, updated_at = @updated_at
WHERE id = @id`

func (r *PostgresRepository) Update(ctx context.Context, note Note) error {
	err := pgx.BeginFunc(ctx, r.pool, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, updateNoteSQL, pgx.NamedArgs{
			"id": note.ID, "title": note.Title, "body": note.Body, "updated_at": note.UpdatedAt,
		})
		if err != nil {
			return translateError(err)
		}
		if tag.RowsAffected() == 0 {
			return ErrNotFound
		}
		return nil
	})
	if err != nil {
		return fmt.Errorf("update note: %w", translateError(err))
	}
	return nil
}

const deleteNoteSQL = `DELETE FROM notes WHERE id = @id`

func (r *PostgresRepository) Delete(ctx context.Context, id string) error {
	err := pgx.BeginFunc(ctx, r.pool, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, deleteNoteSQL, pgx.NamedArgs{"id": id})
		if err != nil {
			return translateError(err)
		}
		if tag.RowsAffected() == 0 {
			return ErrNotFound
		}
		return nil
	})
	if err != nil {
		return fmt.Errorf("delete note: %w", translateError(err))
	}
	return nil
}

func translateError(err error) error {
	if err == nil {
		return nil
	}
	if errors.Is(err, ErrNotFound) {
		return ErrNotFound
	}
	if errors.Is(err, ErrConflict) {
		return ErrConflict
	}
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	if errors.Is(err, context.Canceled) {
		return context.Canceled
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return context.DeadlineExceeded
	}
	var postgresError *pgconn.PgError
	if errors.As(err, &postgresError) {
		if postgresError.Code == "23505" {
			return ErrConflict
		}
		return fmt.Errorf("postgres SQLSTATE %s", postgresError.Code)
	}
	return fmt.Errorf("database operation failed: %v", err)
}
```

`translateError` returns component errors, context errors, or plain wrapped text; it never returns a `pgconn` error identity. The service validates the requested page size before calling `List`.

### `internal/infrastructure/postgres/open.go`

```go
package postgres

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

func Open(ctx context.Context, databaseURL string) (*pgxpool.Pool, error) {
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		return nil, errors.New("invalid PostgreSQL database URL")
	}
	if err = pool.Ping(ctx); err == nil {
		return pool, nil
	}
	pool.Close()
	if !hasSQLState(err, "3D000") {
		return nil, fmt.Errorf("connect to application database: %w", err)
	}

	parsed, err := url.Parse(databaseURL)
	if err != nil || (parsed.Scheme != "postgres" && parsed.Scheme != "postgresql") {
		return nil, errors.New("database URL must use postgres:// or postgresql://")
	}
	databaseName := strings.TrimPrefix(parsed.Path, "/")
	if databaseName == "" || strings.Contains(databaseName, "/") {
		return nil, errors.New("database URL must include a simple database name")
	}
	parsed.Path = "/postgres"
	parsed.RawPath = ""
	adminPool, err := pgxpool.New(ctx, parsed.String())
	if err != nil {
		return nil, errors.New("invalid PostgreSQL maintenance database URL")
	}
	defer adminPool.Close()
	if err := adminPool.Ping(ctx); err != nil {
		return nil, fmt.Errorf("connect to PostgreSQL maintenance database: %w", err)
	}
	_, err = adminPool.Exec(ctx, "CREATE DATABASE "+pgx.Identifier{databaseName}.Sanitize())
	if err != nil && !hasSQLState(err, "42P04") {
		return nil, fmt.Errorf("create application database: %w", err)
	}

	pool, err = pgxpool.New(ctx, databaseURL)
	if err != nil {
		return nil, errors.New("invalid PostgreSQL database URL")
	}
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("connect to created application database: %w", err)
	}
	return pool, nil
}

func hasSQLState(err error, state string) bool {
	var postgresError *pgconn.PgError
	return errors.As(err, &postgresError) && postgresError.Code == state
}
```

The application connects to its configured database first. Only SQLSTATE `3D000` triggers the maintenance connection and `CREATE DATABASE`; the identifier is quoted, creation runs outside a transaction, and SQLSTATE `42P04` handles two instances creating the same database together.

### `internal/infrastructure/postgres/migrate.go`

```go
package postgres

import (
	"context"
	"embed"
	"fmt"
	"io/fs"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/jackc/pgx/v5/stdlib"
	"github.com/pressly/goose/v3"
	"github.com/pressly/goose/v3/lock"
)

//go:embed migrations/*.sql
var migrations embed.FS

func Migrate(ctx context.Context, pool *pgxpool.Pool) error {
	locker, err := lock.NewPostgresSessionLocker()
	if err != nil {
		return fmt.Errorf("create migration lock: %w", err)
	}
	sub, err := fs.Sub(migrations, "migrations")
	if err != nil {
		return fmt.Errorf("open embedded migrations: %w", err)
	}
	db := stdlib.OpenDBFromPool(pool)
	defer db.Close()
	provider, err := goose.NewProvider(goose.DialectPostgres, db, sub, goose.WithSessionLocker(locker))
	if err != nil {
		return fmt.Errorf("create migration provider: %w", err)
	}
	if _, err := provider.Up(ctx); err != nil {
		return fmt.Errorf("apply embedded migrations: %w", err)
	}
	return nil
}
```

`OpenDBFromPool` adapts the existing `pgxpool.Pool` for Goose. Close the `database/sql` adapter after `Up`; the application still owns and closes the original pool.

### `internal/infrastructure/postgres/pgtest/pgtest.go`

```go
package pgtest

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"example.com/notes/internal/infrastructure/postgres"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

func New(t *testing.T) *pgxpool.Pool {
	t.Helper()
	maintenanceURL := testDatabaseURL(t)
	templateURL := createDatabase(t, maintenanceURL, "")
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	templatePool, err := postgres.Open(ctx, templateURL)
	if err != nil {
		t.Fatalf("open migration template: %v", err)
	}
	t.Cleanup(templatePool.Close)
	if err := postgres.Migrate(ctx, templatePool); err != nil {
		t.Fatalf("migrate template database: %v", err)
	}
	templatePool.Close()

	testURL := createDatabase(t, maintenanceURL, databaseName(t, templateURL))
	pool, err := pgxpool.New(ctx, testURL)
	if err != nil {
		t.Fatalf("open test database: %v", err)
	}
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		t.Fatalf("connect to test database: %v", err)
	}
	t.Cleanup(pool.Close)
	return pool
}

// NewEmptyURL is for migration tests that need an unmigrated database.
func NewEmptyURL(t *testing.T) string {
	t.Helper()
	return createDatabase(t, testDatabaseURL(t), "")
}

func testDatabaseURL(t *testing.T) string {
	t.Helper()
	databaseURL := os.Getenv("TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set TEST_DATABASE_URL to a PostgreSQL maintenance database")
	}
	return databaseURL
}

func createDatabase(t *testing.T, maintenanceURL, template string) string {
	t.Helper()
	name := "note_test_" + randomSuffix(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	conn, err := pgx.Connect(ctx, maintenanceURL)
	if err != nil {
		t.Fatalf("connect to test maintenance database: %v", err)
	}
	query := "CREATE DATABASE " + pgx.Identifier{name}.Sanitize()
	if template != "" {
		query += " TEMPLATE " + pgx.Identifier{template}.Sanitize()
	}
	if _, err := conn.Exec(ctx, query); err != nil {
		conn.Close(ctx)
		t.Fatalf("create test database: %v", err)
	}
	t.Cleanup(func() { dropDatabase(t, maintenanceURL, name) })
	if err := conn.Close(ctx); err != nil {
		t.Fatalf("close maintenance connection: %v", err)
	}
	return urlForDatabase(t, maintenanceURL, name)
}

func dropDatabase(t *testing.T, maintenanceURL, name string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	conn, err := pgx.Connect(ctx, maintenanceURL)
	if err != nil {
		t.Errorf("connect to drop test database: %v", err)
		return
	}
	defer conn.Close(ctx)
	query := "DROP DATABASE IF EXISTS " + pgx.Identifier{name}.Sanitize()
	if _, err := conn.Exec(ctx, query); err != nil {
		t.Errorf("drop test database: %v", err)
	}
}

func randomSuffix(t *testing.T) string {
	t.Helper()
	var value [8]byte
	if _, err := rand.Read(value[:]); err != nil {
		t.Fatalf("generate test database name: %v", err)
	}
	return hex.EncodeToString(value[:])
}

func databaseName(t *testing.T, databaseURL string) string {
	t.Helper()
	parsed, err := url.Parse(databaseURL)
	if err != nil {
		t.Fatalf("parse test database URL: %v", err)
	}
	name := strings.TrimPrefix(parsed.Path, "/")
	if name == "" || strings.Contains(name, "/") {
		t.Fatalf("test database URL has no simple database name")
	}
	return name
}

func urlForDatabase(t *testing.T, databaseURL, name string) string {
	t.Helper()
	parsed, err := url.Parse(databaseURL)
	if err != nil {
		t.Fatalf("parse test database URL: %v", err)
	}
	parsed.Path = "/" + name
	parsed.RawPath = ""
	return parsed.String()
}
```

`TEST_DATABASE_URL` points at an existing maintenance database such as `postgres`. The helper creates uniquely named databases, migrates a disposable template once for the test, clones each repository test database from it, and drops both databases during cleanup. Migration tests use `NewEmptyURL` so they start with no migration ledger.

### `internal/components/note/repository_test.go`

```go
package note

import (
	"context"
	"errors"
	"testing"
	"time"

	"example.com/notes/internal/infrastructure/postgres/pgtest"
)

func TestPostgresRepositoryContract(t *testing.T) {
	pool := pgtest.New(t)
	repository, err := NewPostgresRepository(pool)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	createdAt := time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)
	first := Note{ID: "n1", Title: "First", Body: "One", Version: 1, CreatedAt: createdAt, UpdatedAt: createdAt}
	second := Note{ID: "n2", Title: "Second", Body: "Two", Version: 1, CreatedAt: createdAt, UpdatedAt: createdAt}

	if err := repository.Create(ctx, first); err != nil {
		t.Fatal(err)
	}
	if err := repository.Create(ctx, first); !errors.Is(err, ErrConflict) {
		t.Fatalf("duplicate create error = %v, want ErrConflict", err)
	}
	if err := repository.Create(ctx, second); err != nil {
		t.Fatal(err)
	}

	got, err := repository.ByID(ctx, first.ID)
	if err != nil {
		t.Fatal(err)
	}
	if got.ID != first.ID || got.Title != first.Title || got.Body != first.Body || got.Version != 1 || !got.CreatedAt.Equal(createdAt) {
		t.Fatalf("ByID() = %#v, want %#v", got, first)
	}
	if _, err := repository.ByID(ctx, "missing"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("missing ByID() error = %v, want ErrNotFound", err)
	}

	page, err := repository.List(ctx, first.ID, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(page) != 1 || page[0].ID != second.ID {
		t.Fatalf("List() after %q = %#v, want only %q", first.ID, page, second.ID)
	}

	updatedAt := createdAt.Add(time.Minute)
	if err := repository.Update(ctx, Note{ID: first.ID, Title: "Changed", Body: "Updated", UpdatedAt: updatedAt}); err != nil {
		t.Fatal(err)
	}
	got, err = repository.ByID(ctx, first.ID)
	if err != nil {
		t.Fatal(err)
	}
	if got.Title != "Changed" || got.Body != "Updated" || got.Version != 2 || !got.UpdatedAt.Equal(updatedAt) {
		t.Fatalf("updated note = %#v", got)
	}
	if err := repository.Delete(ctx, first.ID); err != nil {
		t.Fatal(err)
	}
	if err := repository.Delete(ctx, first.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("missing Delete() error = %v, want ErrNotFound", err)
	}
}
```

### `internal/infrastructure/postgres/migrate_test.go`

```go
package postgres_test

import (
	"context"
	"sync"
	"testing"
	"time"

	"example.com/notes/internal/infrastructure/postgres"
	"example.com/notes/internal/infrastructure/postgres/pgtest"
	"github.com/jackc/pgx/v5/pgxpool"
)

func TestConcurrentMigrateRecordsEachVersionOnce(t *testing.T) {
	databaseURL := pgtest.NewEmptyURL(t)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	pools := make([]*pgxpool.Pool, 5)
	for i := range pools {
		pool, err := pgxpool.New(ctx, databaseURL)
		if err != nil {
			t.Fatal(err)
		}
		if err := pool.Ping(ctx); err != nil {
			pool.Close()
			t.Fatal(err)
		}
		pools[i] = pool
		t.Cleanup(pool.Close)
	}

	var wait sync.WaitGroup
	errors := make(chan error, len(pools))
	for _, pool := range pools {
		wait.Add(1)
		go func(pool *pgxpool.Pool) {
			defer wait.Done()
			errors <- postgres.Migrate(ctx, pool)
		}(pool)
	}
	wait.Wait()
	close(errors)
	for err := range errors {
		if err != nil {
			t.Error(err)
		}
	}

	var applied int
	err := pools[0].QueryRow(ctx, `
		SELECT count(*)
		FROM goose_db_version
		WHERE version_id = 1 AND is_applied`).Scan(&applied)
	if err != nil {
		t.Fatal(err)
	}
	if applied != 1 {
		t.Fatalf("migration version 1 was recorded %d times, want once", applied)
	}
}
```

The five migrations share one empty database but use five independent pools. The assertion proves that version 1 appears once in Goose's ledger after concurrent startup.

## SQLite for single-file applications

Use [`modernc.org/sqlite`](https://pkg.go.dev/modernc.org/sqlite), a pure-Go driver that keeps `CGO_ENABLED=0`. Open it with `database/sql` and use `goose.DialectSQLite3`; do not use PostgreSQL's session locker for SQLite. Keep one process responsible for the file and set `db.SetMaxOpenConns(1)` on the writer pool. Enable WAL mode once when opening the file:

```go
import _ "modernc.org/sqlite"

db, err := sql.Open("sqlite", "notes.db")
if err != nil {
	return err
}
db.SetMaxOpenConns(1)
if _, err := db.ExecContext(ctx, "PRAGMA journal_mode=WAL"); err != nil {
	return err
}
provider, err := goose.NewProvider(goose.DialectSQLite3, db, migrations)
```

Use SQLite-specific migrations and maintain an FTS5 virtual table with triggers when full-text search is needed. Do not share PostgreSQL SQL or migrations with the SQLite app.

## Verify the example

Copy the `note` files into a temporary module, fetch its pinned dependencies, then run the tests against a real PostgreSQL maintenance database:

```sh
go mod tidy
TEST_DATABASE_URL=postgres://snowbear@oracle-vm:5432/postgres go vet ./...
TEST_DATABASE_URL=postgres://snowbear@oracle-vm:5432/postgres go test ./...
```

The contract tests create isolated databases and remove them afterward. Use a disposable database role with `CREATEDB` permission; never point this helper at a production application database.

Primary sources: [pgx](https://pkg.go.dev/github.com/jackc/pgx/v5), [Goose](https://pkg.go.dev/github.com/pressly/goose/v3), [Goose migration locking](https://github.com/pressly/goose/tree/v3.27.0/lock), and [modernc.org/sqlite](https://pkg.go.dev/modernc.org/sqlite).
