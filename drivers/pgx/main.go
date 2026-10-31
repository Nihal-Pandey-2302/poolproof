// poolproof's pgx program: runs a case's steps and prints one JSON line per step.
//
// The protocol is in drivers/README.md. In short: steps come on stdin, one per line
// (client, SQL, comma-separated integer parameters, tab-separated). Each client gets
// one connection, opened at the start and never reopened: no pgxpool. Every step is
// reported, so a driver that recovers on the next call still shows the step that failed.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"runtime/debug"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

type step struct {
	client string
	sql    string
	params []any
}

// A client is a connection, or the error every one of its steps reports.
type client struct {
	conn    *pgx.Conn
	err     string
	connect bool
}

var out = json.NewEncoder(os.Stdout)

func emit(line map[string]any) {
	if err := out.Encode(line); err != nil {
		os.Exit(2)
	}
}

// The SQLSTATE, a timeout, or DRIVER:<type> for an error pgx raised itself.
func code(ctx context.Context, err error) string {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code
	}
	if errors.Is(ctx.Err(), context.DeadlineExceeded) || pgconn.Timeout(err) {
		return "TIMEOUT"
	}
	return "DRIVER:" + strings.TrimPrefix(fmt.Sprintf("%T", err), "*")
}

func readSteps() []step {
	var steps []step
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		parts := strings.Split(scanner.Text(), "\t")
		if len(parts) != 3 {
			fail("bad step line: %q", scanner.Text())
		}
		s := step{client: parts[0], sql: parts[1]}
		for _, p := range strings.Split(parts[2], ",") {
			if p == "" {
				continue
			}
			n, err := strconv.ParseInt(p, 10, 64)
			if err != nil {
				fail("bad parameter %q", p)
			}
			s.params = append(s.params, n)
		}
		steps = append(steps, s)
	}
	return steps
}

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(2)
}

// The pgx version this binary was built with.
func version() string {
	info, ok := debug.ReadBuildInfo()
	if ok {
		for _, dep := range info.Deps {
			if dep.Path == "github.com/jackc/pgx/v5" {
				return strings.TrimPrefix(dep.Version, "v")
			}
		}
	}
	return "unknown"
}

// A keyword/value connection string value, quoted.
func quote(s string) string {
	return "'" + strings.ReplaceAll(strings.ReplaceAll(s, `\`, `\\`), "'", `\'`) + "'"
}

func main() {
	ms, err := strconv.Atoi(os.Getenv("PP_STEP_TIMEOUT_MS"))
	if err != nil {
		fail("bad PP_STEP_TIMEOUT_MS")
	}
	timeout := time.Duration(ms) * time.Millisecond
	steps := readSteps()
	emit(map[string]any{"driver": "pgx", "version": version()})

	cfg, err := pgx.ParseConfig(fmt.Sprintf(
		"host=%s port=%s dbname=%s sslmode=%s",
		quote(os.Getenv("PP_HOST")), quote(os.Getenv("PP_PORT")),
		quote(os.Getenv("PP_DBNAME")), quote(os.Getenv("PP_SSLMODE")),
	))
	if err != nil {
		fail("connection settings: %v", err)
	}
	cfg.User = os.Getenv("PP_USER")
	cfg.Password = os.Getenv("PP_PASSWORD")
	cfg.ConnectTimeout = timeout
	switch variant := os.Getenv("PP_VARIANT"); variant {
	case "default":
	case "exec":
		cfg.DefaultQueryExecMode = pgx.QueryExecModeExec
	default:
		fail("unknown variant %s", variant)
	}

	clients := map[string]*client{}
	attempts := 0
	for _, s := range steps {
		if clients[s.client] != nil {
			continue
		}
		attempts++
		ctx, cancel := context.WithTimeout(context.Background(), timeout)
		conn, err := pgx.ConnectConfig(ctx, cfg)
		if err != nil {
			clients[s.client] = &client{err: code(ctx, err), connect: true}
		} else {
			clients[s.client] = &client{conn: conn}
		}
		cancel()
	}

	for i, s := range steps {
		c := clients[s.client]
		if c.conn == nil {
			line := map[string]any{"step": i + 1, "error": c.err}
			if c.connect {
				line["connect"] = true
			}
			emit(line)
			continue
		}
		ctx, cancel := context.WithTimeout(context.Background(), timeout)
		rows, err := run(ctx, c.conn, s)
		if err != nil {
			got := code(ctx, err)
			emit(map[string]any{"step": i + 1, "error": got})
			if got == "TIMEOUT" {
				// pgx closes a connection whose deadline passed mid-query.
				*c = client{err: "CLOSED"}
			}
		} else {
			emit(map[string]any{"step": i + 1, "rows": rows})
		}
		cancel()
	}

	for _, c := range clients {
		if c.conn != nil {
			ctx, cancel := context.WithTimeout(context.Background(), timeout)
			_ = c.conn.Close(ctx)
			cancel()
		}
	}
	emit(map[string]any{"connections": attempts})
}

func run(ctx context.Context, conn *pgx.Conn, s step) ([][]*string, error) {
	rows, err := conn.Query(ctx, s.sql, s.params...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := [][]*string{}
	for rows.Next() {
		values, err := rows.Values()
		if err != nil {
			return nil, err
		}
		row := make([]*string, len(values))
		for j, v := range values {
			if v != nil {
				text := fmt.Sprint(v)
				row[j] = &text
			}
		}
		result = append(result, row)
	}
	return result, rows.Err()
}
