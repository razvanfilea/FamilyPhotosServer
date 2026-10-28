import { rmSync } from 'node:fs';
import { runDir } from './run';

/** Deletes the run directory, unless FP_E2E_KEEP is set (to inspect the storage and database). */
export default async function globalTeardown() {
    if (!process.env.FP_E2E_KEEP) {
        rmSync(runDir(), { recursive: true, force: true });
    }
}
