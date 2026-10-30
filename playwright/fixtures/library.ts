import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { APIRequestContext, PlaywrightWorkerArgs } from '@playwright/test';
import { expect, user, type UserId } from './auth';

/** A request context with its own session, logged in as `id` (dispose it when done). */
export async function apiAs(
    playwright: PlaywrightWorkerArgs['playwright'],
    baseURL: string | undefined,
    id: UserId,
): Promise<APIRequestContext> {
    const request = await playwright.request.newContext({ baseURL });
    const response = await request.post('/api/login', { form: { user_id: id, password: user(id).password } });
    expect(response.ok()).toBe(true);
    return request;
}

/**
 * Uploads a photo (unique content, so the server never reuses another one) and returns its id.
 * It goes to the user's gallery, or into `folderId`.
 */
export async function uploadPhoto(
    request: APIRequestContext,
    name: string,
    time: number,
    folderId?: number,
    paddingBytes = 0,
): Promise<number> {
    const buffer = Buffer.concat([
        readFileSync(join(__dirname, 'photos/public/kite.jpg')),
        Buffer.from(`${name} ${Date.now()} ${Math.random()}`),
        Buffer.alloc(paddingBytes),
    ]);
    const query = folderId === undefined ? '' : `&folder_id=${folderId}`;
    const response = await request.post(`/api/upload?time_created=${time}${query}`, {
        multipart: { file: { name, mimeType: 'image/jpeg', buffer } },
    });
    expect(response.ok(), await response.text()).toBe(true);
    const photo = await response.json();
    if (folderId !== undefined) expect(photo.folder_id).toBe(folderId);
    return photo.id as number;
}

/** `/api/photos/{id}/info` as JSON */
export async function photoInfo(request: APIRequestContext, id: number) {
    const response = await request.get(`/api/photos/${id}/info`);
    expect(response.ok()).toBe(true);
    return response.json();
}
