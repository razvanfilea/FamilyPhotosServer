// Seeds what needs the HTTP API (the server was started and filled with photos by serve.mjs),
// saves a logged-in session per user and writes the seed manifest used by the specs.

import { chromium, type APIRequestContext, type FullConfig } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { USERS } from './users.mjs';
import { runDir, storageStatePath } from './run';
import type { Seed, SeedPhoto } from './seed';

interface PhotoJson {
    id: number;
    user_id: string | null;
    name: string;
    created_at: number;
    folder_id: number | null;
    trashed_on: number | null;
}

async function ok(response: Awaited<ReturnType<APIRequestContext['get']>>) {
    if (!response.ok()) {
        throw new Error(`${response.url()} failed: ${response.status()} ${await response.text()}`);
    }
    return response;
}

async function upsertFolder(request: APIRequestContext, name: string, isPublic: boolean): Promise<number> {
    const response = await ok(await request.post('/api/folders', { data: { name, is_public: isPublic } }));
    return (await response.json()).id;
}

export default async function globalSetup(config: FullConfig) {
    const baseURL = config.projects.find((p) => p.name === 'chromium')!.use.baseURL!;
    const browser = await chromium.launch();
    const requests: Record<string, APIRequestContext> = {};

    for (const user of USERS) {
        const context = await browser.newContext({ baseURL });
        await ok(await context.request.post('/api/login', { form: { user_id: user.id, password: user.password } }));
        const path = storageStatePath(user.id);
        mkdirSync(dirname(path), { recursive: true });
        await context.storageState({ path });
        requests[user.id] = context.request;
    }

    const alice = requests.alice;
    const folders: Record<string, number> = {
        'Holiday': await upsertFolder(alice, 'Holiday', false),
        'Private': await upsertFolder(alice, 'Private', false),
        'Family Trip': await upsertFolder(alice, 'Family Trip', true),
    };

    await ok(await alice.post('/api/sharing', {
        data: { folder_id: folders.Holiday, grantee_id: 'bob', can_upload: false, can_delete: false },
    }));

    // Everything alice or bob can see: own + public photos
    const folderNames = new Map(Object.entries(folders).map(([name, id]) => [id, name]));
    const photos: Record<string, SeedPhoto> = {};
    for (const user of ['alice', 'bob']) {
        const response = await ok(await requests[user].get('/api/sync/full'));
        for (const p of (await response.json()).photos as PhotoJson[]) {
            const folder = p.folder_id === null ? '' : `${folderNames.get(p.folder_id)}/`;
            const path = `${p.user_id ?? 'public'}/${folder}${p.name}`;
            photos[path] = {
                id: p.id,
                path,
                name: p.name,
                userId: p.user_id,
                folderId: p.folder_id,
                createdAt: p.created_at,
                trashed: p.trashed_on !== null,
            };
        }
    }

    // The scanner must have read the EXIF dates of the fixtures
    const expected = Date.UTC(2024, 5, 8, 14, 3, 29) / 1000;
    if (photos['public/tie-a.jpg']?.createdAt !== expected) {
        throw new Error(`public/tie-a.jpg has the wrong date: ${photos['public/tie-a.jpg']?.createdAt}`);
    }

    const aliceFavorites = ['alice/alice-a.jpg', 'public/lake.jpg'];
    for (const path of aliceFavorites) {
        await ok(await alice.post(`/api/favorite/${photos[path].id}`));
    }

    const trashed = ['alice/alice-f.jpg'];
    await ok(await alice.post('/api/trash', { data: trashed.map((path) => photos[path].id) }));
    for (const path of trashed) {
        photos[path].trashed = true;
    }

    const seed: Seed = { photos, folders, aliceFavorites, trashed };
    writeFileSync(join(runDir(), 'seed.json'), JSON.stringify(seed, null, 2));

    await browser.close();
}
