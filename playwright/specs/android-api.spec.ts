import { expect, test } from '@playwright/test';
import type { APIRequestContext } from '@playwright/test';
import { user } from '../fixtures/auth';
import { apiAs, uploadPhoto } from '../fixtures/library';

// The Android app calls these endpoints directly; this walks through them as the app does and pins the
// field names and types of the responses. frank is used by no other spec. It runs once (desktop).
test.describe.configure({ mode: 'serial' });
test.skip(({ isMobile }) => isMobile, 'Same requests as the desktop run');

const PHOTO = {
    id: 'number', user_id: 'string', name: 'string', created_at: 'number', file_size: 'number',
    folder_id: 'number', thumb_hash: 'string', trashed_on: 'number',
};

function expectShape(value: Record<string, unknown>, shape: Record<string, string>) {
    expect(Object.keys(value).sort()).toEqual(Object.keys(shape).sort());
    for (const [key, type] of Object.entries(shape)) {
        const actual = value[key] === null ? 'null' : typeof value[key];
        // user_id, folder_id, thumb_hash and trashed_on can be null; every other field has exactly one type
        if (['user_id', 'folder_id', 'thumb_hash', 'trashed_on'].includes(key)) expect(['null', type], key).toContain(actual);
        else expect(actual, key).toBe(type);
    }
}

const json = async (response: Awaited<ReturnType<APIRequestContext['get']>>, status = 200) => {
    expect(response.status(), await response.text()).toBe(status);
    return response.json();
};

test('the endpoints the Android app uses keep their shapes', async ({ playwright, baseURL }) => {
    const anonymous = await playwright.request.newContext({ baseURL });
    const frank = await playwright.request.newContext({ baseURL });
    const bob = await apiAs(playwright, baseURL, 'bob');
    try {
        // Login is form encoded; logout and profile
        const { password } = user('frank');
        const login = await frank.post('/api/login', { form: { user_id: 'frank', password } });
        expect(await json(login)).toEqual({ user_id: 'frank', display_name: 'Frank' });
        expect(await json(await frank.get('/api/profile'))).toEqual({ user_id: 'frank', display_name: 'Frank' });
        const members = await json(await frank.get('/api/members'));
        expect(members.map((m: { user_id: string }) => m.user_id)).toEqual(
            expect.arrayContaining(['alice', 'bob', 'frank']),
        );
        expect(Object.keys(members[0]).sort()).toEqual(['display_name', 'user_id']);
        expect((await anonymous.get('/api/profile')).status()).toBe(401);

        // Full sync gives the cursor to the partial sync
        const before = await json(await frank.get('/api/sync/full'));
        expect(typeof before.event_log_id).toBe('number');
        const own = (photos: { user_id: string | null }[]) => photos.filter((p) => p.user_id === 'frank');
        expect(own(before.photos)).toEqual([]);

        // Upload with time_created and hash, to the root and to a new folder
        const folder = await json(await frank.post('/api/folders', { data: { name: 'Android', is_public: false } }));
        expect(Object.keys(folder).sort()).toEqual(['id', 'name', 'owner_id']);
        expect(folder).toMatchObject({ name: 'Android', owner_id: 'frank' });
        const again = await json(await frank.post('/api/folders', { data: { name: 'Android', is_public: false } }));
        expect(again.id).toBe(folder.id);

        const rootId = await uploadPhoto(frank, 'root.jpg', 1_700_000_000);
        const inFolderId = await uploadPhoto(frank, 'in-folder.jpg', 1_700_000_100, folder.id);
        const info = await json(await frank.get(`/api/photos/${inFolderId}/info`));
        expect(info).toBeTruthy();

        const downloaded = await frank.get(`/api/download/${rootId}`);
        expect(downloaded.status()).toBe(200);
        const ranged = await frank.get(`/api/download/${rootId}`, { headers: { Range: 'bytes=0-9' } });
        expect(ranged.status()).toBe(206);
        expect((await ranged.body()).length).toBe(10);
        expect((await frank.get(`/api/preview/${rootId}`)).status()).toBe(200);
        const exif = await frank.get(`/api/exif/${rootId}`);
        const fields = await json(exif);
        expect(fields.length).toBeGreaterThan(0);
        expect(Object.keys(fields[0]).sort()).toEqual(['tag', 'value']);

        const synced = await json(await frank.get('/api/sync/full'));
        expect(own(synced.photos)).toHaveLength(2);
        for (const p of synced.photos) expectShape(p, PHOTO);
        const partial = await json(await frank.get(`/api/sync/partial?last_synced_event_id=${before.event_log_id}`));
        expect(partial.event_log_id).toBeGreaterThan(before.event_log_id);
        expect(partial.events.length).toBeGreaterThanOrEqual(2);
        for (const e of partial.events) {
            expect(Object.keys(e).sort()).toEqual(['data', 'photo_id']);
            expect(JSON.parse(Buffer.from(e.data, 'base64').toString())).toHaveProperty('name');
        }
        expect((await frank.get('/api/sync/partial?last_synced_event_id=999999999')).status()).toBe(409);

        const folders = await json(await frank.post('/api/sync/folders', { data: { [folder.id]: 0 } }));
        const synced0 = folders.find((f: { id: number }) => f.id === folder.id);
        expect(synced0).toMatchObject({ name: 'Android', owner_id: 'frank' });
        expect(Object.keys(synced0).sort()).toEqual(['id', 'name', 'owner_id']);

        // Timestamp, favourites, duplicates
        const stamped = await json(await frank.post(`/api/timestamp/${rootId}?time_created=1700001000`));
        expect(stamped.created_at).toBe(1_700_001_000);
        // Bulk date change: partial success returns only the updated photos
        const dated = await json(await frank.post('/api/photos/date', {
            data: { photo_ids: [rootId, 999_999_999], created_at: 1_700_002_000 },
        }));
        expect(dated.map((p: { id: number }) => p.id)).toEqual([rootId]);
        expect(dated[0].created_at).toBe(1_700_002_000);
        expectShape(dated[0], PHOTO);
        expect((await frank.post('/api/photos/date', { data: { photo_ids: [rootId] } })).status()).toBe(400);
        expect((await frank.post(`/api/favorite/${rootId}`)).status()).toBe(200);
        expect(await json(await frank.get('/api/favorite'))).toEqual([rootId]);
        expect((await frank.delete(`/api/favorite/${rootId}`)).status()).toBe(200);
        expect(await json(await frank.get('/api/favorite'))).toEqual([]);
        expect(Array.isArray(await json(await frank.get('/api/duplicates')))).toBe(true);

        // Rename the folder, move the root photo into it (partial success returns only the moved ones)
        const renamed = await json(await frank.patch(`/api/folders/${folder.id}`, { data: { name: 'Android 2' } }));
        expect(renamed).toMatchObject({ id: folder.id, name: 'Android 2' });
        const moved = await json(await frank.post(`/api/move?target_folder_id=${folder.id}`, { data: [rootId, 999_999_999] }));
        expect(moved.map((p: { id: number }) => p.id)).toEqual([rootId]);
        expectShape(moved[0], PHOTO);

        // Sharing CRUD without expires_at
        const share = await json(await frank.post('/api/sharing', {
            data: { folder_id: folder.id, grantee_id: 'bob', can_upload: false, can_delete: false },
        }));
        expect(Object.keys(share).sort()).toEqual(
            ['can_delete', 'can_upload', 'created_at', 'expires_at', 'folder_id', 'folder_name', 'grantee_id', 'id', 'token'],
        );
        expect(share).toMatchObject({ folder_id: folder.id, grantee_id: 'bob', can_upload: false, can_delete: false });
        const updated = await json(await frank.put(`/api/sharing/${share.id}`, { data: { can_upload: true, can_delete: false } }));
        expect(updated.can_upload).toBe(true);
        expect((await json(await bob.get(`/api/sharing/folder/${folder.id}`))).map((s: { id: number }) => s.id)).toEqual([share.id]);
        expect((await json(await frank.get('/api/sharing'))).length).toBe(1);
        expect((await frank.delete(`/api/sharing/${share.id}`)).status()).toBe(200);
        expect(await json(await frank.get('/api/sharing'))).toEqual([]);

        // Trash, restore and delete forever (which needs the photo to be trashed first)
        expect((await frank.delete(`/api/delete/${inFolderId}`)).status()).toBe(400);
        const trashed = await json(await frank.post('/api/trash', { data: [inFolderId] }));
        expect(trashed).toHaveLength(1);
        expect(typeof trashed[0].trashed_on).toBe('number');
        const restored = await json(await frank.post('/api/trash/restore', { data: [inFolderId] }));
        expect(restored[0].trashed_on).toBeNull();
        await json(await frank.post('/api/trash', { data: [inFolderId] }));
        expect((await frank.delete(`/api/delete/${inFolderId}`)).status()).toBe(200);
        expect((await frank.get(`/api/download/${inFolderId}`)).status()).toBe(404);

        expect(await json(await frank.post('/api/logout'))).toEqual({ message: 'Logged out' });
        expect((await frank.get('/api/profile')).status()).toBe(401);
    } finally {
        await anonymous.dispose();
        await frank.dispose();
        await bob.dispose();
    }
});
