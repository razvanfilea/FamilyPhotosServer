import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/** The run directory created by playwright.config.ts (storage, database, seed data, sessions). */
export function runDir(): string {
    const dir = process.env.FP_E2E_RUN_DIR;
    if (!dir) throw new Error('FP_E2E_RUN_DIR is not set; run the tests through playwright.config.ts');
    return dir;
}

export function storageStatePath(userId: string): string {
    return join(runDir(), 'auth', `${userId}.json`);
}

export function readRunJson<T>(name: string): T {
    return JSON.parse(readFileSync(join(runDir(), name), 'utf8')) as T;
}
