import { type APIRequestContext, type BrowserContext } from '@playwright/test';
import { storageStatePath } from '../harness/run';
import { test as base, type UserId } from './auth';
import { uploadPhoto } from './library';

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

    /** Uploads a photo into a folder; returns the photo id. */
    async upload(owner: UserId, folderId: number): Promise<number> {
        return uploadPhoto(await this.as(owner), `e2e-${this.suffix}.jpg`, 1700000000, folderId);
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
        const prefix = ({ chromium: 'D', firefox: 'F' } as Record<string, string>)[testInfo.project.name] ?? 'M';
        const suffix = `${prefix}${Date.now() % 1_000_000}`;
        const folders = new TempFolders(
            (user) => browser.newContext({ baseURL, storageState: storageStatePath(user) }),
            suffix,
        );
        await use(folders);
        await folders.dispose();
    },
});
