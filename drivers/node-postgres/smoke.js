// poolproof's node-postgres program: runs a case's steps and prints one JSON line per
// step.
//
// The protocol is in drivers/README.md. In short: steps come on stdin, one per line
// (client, SQL, comma-separated integer parameters, tab-separated). Each client gets
// one connection, opened at the start and never reopened: no pg.Pool. Every step is
// reported, so a driver that recovers on the next call still shows the step that failed.
'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const { Client, DatabaseError } = require('pg');
const { version } = require('pg/package.json');

const TIMEOUT = Symbol('timeout');

function emit(line) {
  fs.writeSync(1, JSON.stringify(line) + '\n');
}

// The SQLSTATE, or DRIVER:<class> for an error node-postgres raised itself.
function code(err) {
  if (err instanceof DatabaseError && err.code) {
    return err.code;
  }
  return 'DRIVER:' + (err && err.constructor ? err.constructor.name : typeof err);
}

// Resolves to TIMEOUT if `promise` takes longer than `ms`.
function within(promise, ms) {
  let timer;
  const expired = new Promise((resolve) => {
    timer = setTimeout(() => resolve(TIMEOUT), ms);
  });
  // A query left behind after a timeout rejects later; that's expected.
  promise.catch(() => {});
  return Promise.race([promise, expired]).finally(() => clearTimeout(timer));
}

function readSteps() {
  return fs
    .readFileSync(0, 'utf8')
    .split('\n')
    .filter((line) => line !== '')
    .map((line) => {
      const [client, sql, params] = line.split('\t');
      return { client, sql, params: params ? params.split(',').map(Number) : [] };
    });
}

// The query as an app writes it. Without values and a name, node-postgres sends it with
// the simple protocol, as it does for apps.
function query(variant, step) {
  const q = { text: step.sql, rowMode: 'array' };
  if (step.params.length > 0) {
    q.values = step.params;
  }
  if (variant === 'named') {
    q.name = 'pp_' + crypto.createHash('sha1').update(step.sql).digest('hex').slice(0, 12);
  }
  return q;
}

async function main() {
  const variant = process.env.PP_VARIANT;
  if (variant !== 'default' && variant !== 'named') {
    throw new Error(`unknown variant ${variant}`);
  }
  const timeout = Number(process.env.PP_STEP_TIMEOUT_MS);
  const steps = readSteps();
  emit({ driver: 'node-postgres', version });

  const sslmode = process.env.PP_SSLMODE;
  // node-postgres has no sslmode=prefer: it means no TLS here.
  const ssl = sslmode === 'require' ? { rejectUnauthorized: false } : false;
  // A client is a connection, or the error every one of its steps reports.
  const clients = new Map();
  let attempts = 0;
  for (const { client: name } of steps) {
    if (clients.has(name)) {
      continue;
    }
    attempts += 1;
    const client = new Client({
      host: process.env.PP_HOST,
      port: Number(process.env.PP_PORT),
      user: process.env.PP_USER,
      password: process.env.PP_PASSWORD,
      database: process.env.PP_DBNAME,
      ssl,
      connectionTimeoutMillis: timeout,
    });
    // A dropped connection is reported by the step that hits it.
    client.on('error', () => {});
    try {
      const result = await within(client.connect(), timeout);
      clients.set(name, result === TIMEOUT ? { error: 'TIMEOUT', connect: true } : client);
    } catch (err) {
      clients.set(name, { error: code(err), connect: true });
    }
  }

  for (const [i, step] of steps.entries()) {
    const client = clients.get(step.client);
    if (!(client instanceof Client)) {
      emit({ step: i + 1, ...client });
      continue;
    }
    try {
      const result = await within(client.query(query(variant, step)), timeout);
      if (result === TIMEOUT) {
        emit({ step: i + 1, error: 'TIMEOUT' });
        // Its state is unknown now; later steps on it report CLOSED.
        client.connection.stream.destroy();
        clients.set(step.client, { error: 'CLOSED' });
      } else {
        const rows = result.rows.map((row) => row.map((v) => (v === null ? null : String(v))));
        emit({ step: i + 1, rows });
      }
    } catch (err) {
      emit({ step: i + 1, error: code(err) });
    }
  }

  for (const client of clients.values()) {
    if (client instanceof Client) {
      await within(client.end(), timeout);
    }
  }
  emit({ connections: attempts });
}

main().then(
  () => process.exit(0),
  (err) => {
    console.error(err);
    process.exit(2);
  },
);
