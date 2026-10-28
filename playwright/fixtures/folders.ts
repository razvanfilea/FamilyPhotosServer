import { type APIRequestContext, type BrowserContext } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { storageStatePath } from '../harness/run';
import { test as base, type UserId } from './auth';

interface TempFolder {
    id: number;
    owner: UserId;
}

/** Creates folders for a test through the API and removes them (if still empty) afterwards. */
export class TempFolders {
    private readonly contexts = new Map<UserId, BrowserContext>();
    private readonly created: TempFolder[] = [];

    constructor(
        private readonly newContext: (user: UserId) => Promise<BrowserContext>,
        private readonly suffix: string,
    ) {}

    /** A request context logged in as `user` */
    async as(user: UserId): Promise<APIRequestContext> {
        let context = this.contexts.get(user);
        if (!context) {
            context = await this.newContext(user);
            this.contexts.set(user, context);
        }
        return context.request;
    }

    /** A folder name that no other test (or project) uses */
    name(prefix: string): string {
        return `${prefix} ${this.suffix}`;
    }

    async create(owner: UserId, name: string, isPublic = false): Promise<number> {
        const response = await (await this.as(owner)).post('/api/folders', { data: { name, is_public: isPublic } });
        if (!response.ok()) throw new Error(`Creating ${name} failed: ${response.status()}`);
        const { id } = await response.json();
        this.created.push({ id, owner });
        return id;
    }

    /** Uploads a fixture photo into a folder; returns the photo id. */
    async upload(owner: UserId, folderId: number, fixture = 'alice/alice-a.jpg'): Promise<number> {
        // The server reuses a photo with the same content, so make this one unique
        const buffer = Buffer.concat([readFileSync(join(__dirname, 'photos', fixture)), Buffer.from(this.suffix)]);
        const response = await (await this.as(owner)).post(
            `/api/upload?folder_id=${folderId}&time_created=1700000000`,
            { multipart: { file: { name: `e2e-${this.suffix}.jpg`, mimeType: 'image/jpeg', buffer } } },
        );
        if (!response.ok()) throw new Error(`Uploading failed: ${response.status()}`);
        const photo = await response.json();
        if (photo.folder_id !== folderId) throw new Error(`The upload ended up in folder ${photo.folder_id}`);
        return photo.id;
    }

    async dispose() {
        for (const { id, owner } of this.created.reverse()) {
            await (await this.as(owner)).delete(`/api/folders/${id}`);
        }
        for (const context of this.contexts.values()) {
            await context.close();
        }
    }
}

export const test = base.extend<{ folders: TempFolders }>({
    folders: async ({ browser, baseURL }, use, testInfo) => {
        const suffix = `${testInfo.project.name === 'chromium' ? 'D' : 'M'}${Date.now() % 1_000_000}`;
        const folders = new TempFolders(
            (user) => browser.newContext({ baseURL, storageState: storageStatePath(user) }),
            suffix,
        );
        await use(folders);
        await folders.dispose();
    },
});
