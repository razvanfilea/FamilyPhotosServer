// Playwright `webServer` command: prepares a fresh, seeded storage + database in the run
// directory, builds the server and runs it. Everything lives in $FP_E2E_RUN_DIR (unique per run,
// created by playwright.config.ts) and the server listens on $FP_E2E_PORT, so several checkouts
// can run the suite at the same time without touching a developer's server or database.
//
// Seeding that needs the HTTP API (sharing, favourites, trash) happens later in global-setup.ts.

import { execFileSync, spawn } from 'node:child_process';
import { cpSync, mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';
import { exifDate, setExifDate } from './exif.mjs';
import { USERS } from './users.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const runDir = process.env.FP_E2E_RUN_DIR;
const port = process.env.FP_E2E_PORT;
if (!runDir || !port) {
    throw new Error('FP_E2E_RUN_DIR and FP_E2E_PORT must be set (see playwright.config.ts)');
}

const storagePath = join(runDir, 'storage');
const previewsPath = join(runDir, 'previews');
const databasePath = join(runDir, 'db.sqlite');

const log = (message) => process.stderr.write(`[e2e] ${message}\n`);

function run(command, args, options = {}) {
    execFileSync(command, args, { cwd: root, stdio: ['ignore', 'ignore', 'inherit'], ...options });
}

/**
 * Creates an SQLite database with node:sqlite (sqlx-cli isn't required): an empty WAL database for
 * the server (which runs the migrations itself), or one with the schema for compiling.
 */
function createDatabase(path, { withSchema }) {
    const db = new DatabaseSync(path);
    if (!withSchema) {
        db.exec('pragma journal_mode = wal');
    } else {
        const migrations = readdirSync(join(root, 'migrations')).filter((f) => f.endsWith('.sql') && !f.endsWith('.down.sql')).sort();
        for (const migration of migrations) {
            db.exec(readFileSync(join(root, 'migrations', migration), 'utf8'));
        }
    }
    db.close();
}

/** 250 tiny photos over March 2022: one month larger than a 200-tile page. */
function generateBulkPhotos(dir) {
    const count = 250;
    const inputs = Array.from({ length: count }, (_, i) => `xc:hsl(${(i * 137) % 360},60%,45%)`);
    // Noise keeps the previews above the server's minimum preview size
    run('magick', [
        '-size', '64x64', ...inputs, '-attenuate', '1.5', '+noise', 'Gaussian',
        '-strip', '-quality', '60', '+adjoin', join(dir, 'bulk-%03d.jpg'),
    ]);

    const start = Date.UTC(2022, 2, 1);
    for (let i = 0; i < count; i++) {
        const file = join(dir, `bulk-${String(i).padStart(3, '0')}.jpg`);
        setExifDate(file, exifDate(new Date(start + i * 10_000 * 1000)));
    }
}

/** "On this day" photos, relative to today: 2 years ago, and 5 years ago + 2 days. */
function generateMemoryPhotos(dir) {
    const today = new Date();
    const memories = [
        { name: 'memory-2y.jpg', years: 2, days: 0 },
        { name: 'memory-5y.jpg', years: 5, days: 2 },
    ];

    for (const [i, { name, years, days }] of memories.entries()) {
        const date = new Date(Date.UTC(today.getUTCFullYear() - years, today.getUTCMonth(), today.getUTCDate() + days, 12));
        const file = join(dir, name);
        run('magick', ['-size', '480x360', '-seed', String(1000 + i), 'plasma:fractal', '-strip', '-quality', '70', file]);
        setExifDate(file, exifDate(date));
    }
}

function buildServer() {
    const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
        cwd: root,
        encoding: 'utf8',
    }));

    const env = { ...process.env };
    // sqlx checks the queries against a database at compile time; use a fresh one unless the
    // developer has already configured theirs. Its path must stay the same across runs, since
    // cargo rebuilds the crate whenever DATABASE_URL changes.
    if (!env.DATABASE_URL) {
        const buildDatabase = join(metadata.target_directory, 'fp-e2e-build.sqlite');
        const temporary = `${buildDatabase}.${process.pid}`;
        createDatabase(temporary, { withSchema: true });
        renameSync(temporary, buildDatabase);
        env.DATABASE_URL = `sqlite:${buildDatabase}`;
    }

    // main.css is gitignored; always rebuild it so a stale file can't hide layout bugs
    log('Building the CSS…');
    run('npx', ['@tailwindcss/cli', '-i', 'templates/input.css', '-o', 'assets/css/main.css', '--minify']);

    log('Building the server (cargo build)…');
    run('cargo', ['build', '--quiet'], { env });
    return join(metadata.target_directory, 'debug', 'familyphotos');
}

const serverEnv = {
    ...process.env,
    STORAGE_PATH: storagePath,
    PREVIEWS_PATH: previewsPath,
    DATABASE_URL: `sqlite:${databasePath}`,
    SERVER_PORT: port,
    SCAN_NEW_FILES: 'false',
    ALLOWED_ORIGINS: `http://localhost:${port}`,
    RUST_LOG: process.env.RUST_LOG ?? 'warn',
};

log(`Preparing ${runDir}`);
mkdirSync(previewsPath, { recursive: true });
cpSync(join(root, 'playwright/fixtures/photos'), storagePath, { recursive: true });
mkdirSync(join(storagePath, 'carol'), { recursive: true });
generateBulkPhotos(join(storagePath, 'public'));
generateMemoryPhotos(join(storagePath, 'public'));
createDatabase(databasePath, { withSchema: false });

const binary = buildServer();
const cli = (...args) => run(binary, args, { env: serverEnv });

log('Seeding users and photos…');
for (const user of USERS) {
    cli('users', 'create', '-u', user.id, '-n', user.name, '-p', user.password);
}
cli('photos', 'scan-photos');
cli('photos', 'generate-previews');
cli('photos', 'generate-thumb-hashes');
cli('photos', 'compute-hashes');
writeFileSync(join(runDir, 'server.json'), JSON.stringify({ storagePath, previewsPath, databasePath, port }));

log(`Starting the server on port ${port}`);
const server = spawn(binary, [], { cwd: root, env: serverEnv, stdio: ['ignore', 'inherit', 'inherit'] });
for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => server.kill(signal));
}
server.on('exit', (code) => process.exit(code ?? 1));
