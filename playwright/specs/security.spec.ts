import { expect, test } from '@playwright/test';
import { apiAs } from '../fixtures/library';
import { folderId, photo } from '../harness/seed';

// carol owns nothing and nothing is shared with her, so every private id of alice's must look missing.
test.describe('private photos stay private', () => {
    const privatePhotos = [photo('alice/Private/private-a.jpg'), photo('alice/alice-a.jpg')];

    test('another user gets 404 for every way to reach them', async ({ playwright, baseURL }) => {
        const carol = await apiAs(playwright, baseURL, 'carol');
        const alice = await apiAs(playwright, baseURL, 'alice');
        const privateFolder = folderId('Private');
        const urls = [
            ...privatePhotos.flatMap(({ id }) => [
                `/api/preview/${id}`,
                `/api/screen/${id}`,
                `/api/download/${id}`,
                `/api/photos/${id}/info`,
                `/api/exif/${id}`,
                `/ui/viewer/info/${id}`,
            ]),
            `/ui/grid/month?ctx=folder:${privateFolder}&month=2024-01`,
            `/api/context/ids?ctx=folder:${privateFolder}`,
            `/folders/${privateFolder}`,
            `/folders/${privateFolder}?photo=${privatePhotos[0].id}`,
        ];
        try {
            for (const url of urls) {
                expect((await alice.get(url)).status(), `alice ${url}`).toBe(200);
                expect((await carol.get(url)).status(), `carol ${url}`).toBe(404);
            }
        } finally {
            await carol.dispose();
            await alice.dispose();
        }
    });

    test('the folders page of another user does not link the private folder', async ({ playwright, baseURL }) => {
        const link = `/folders/${folderId('Private')}"`;
        const alice = await apiAs(playwright, baseURL, 'alice');
        const carol = await apiAs(playwright, baseURL, 'carol');
        try {
            expect(await (await alice.get('/folders')).text()).toContain(link);
            expect(await (await carol.get('/folders')).text()).not.toContain(link);
        } finally {
            await alice.dispose();
            await carol.dispose();
        }
    });
});
