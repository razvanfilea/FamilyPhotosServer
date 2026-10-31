import type { Page, Request, Response, TestInfo } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, gotoReady, loginAs } from '../fixtures/auth';
import { test } from '../fixtures/folders';
import { withExifDate } from '../harness/exif.mjs';
import { folderId } from '../harness/seed';

// Uploads change the uploader's timeline, which other specs assert on for alice and bob. So every
// test that uploads does it as ivan (who owns no seeded photos), with dates in the 2040s that no
// other spec uses, with unique file contents (the server deduplicates by hash), and only in the
// desktop project. Tests that upload nothing run on both devices.

const PHOTOS = join(__dirname, '../fixtures/photos');

interface UploadFile {
    name: string;
    mimeType: string;
    buffer: Buffer;
}

/** A fixture JPEG with another EXIF date, made unique by `unique` (appended after the image) */
function photoFile(name: string, exifDate: string, unique: string, fixture = 'public/kite.jpg'): UploadFile {
    let jpeg = readFileSync(join(PHOTOS, fixture));
    // Drop the fixture's own EXIF segment, which sits right after the start of image marker
    if (jpeg[2] === 0xff && jpeg[3] === 0xe1) {
        jpeg = Buffer.concat([jpeg.subarray(0, 2), jpeg.subarray(4 + jpeg.readUInt16BE(4))]);
    }
    return { name, mimeType: 'image/jpeg', buffer: Buffer.concat([withExifDate(jpeg, exifDate), Buffer.from(unique)]) };
}

function fixtureFile(path: string): UploadFile {
    return { name: path.split('/').pop()!, mimeType: 'image/jpeg', buffer: readFileSync(join(PHOTOS, path)) };
}

const unique = (testInfo: TestInfo) => `${testInfo.project.name}-${testInfo.title}-${Date.now()}-${testInfo.repeatEachIndex}-${randomUUID()}`;

const rows = (page: Page) => page.getByTestId('upload-row');
const row = (page: Page, name: string) =>
    rows(page).filter({ has: page.getByTestId('upload-row-name').getByText(name, { exact: true }) });
const picker = (page: Page) => page.getByTestId('destination-picker');
const pickerValue = (page: Page) => picker(page).evaluate((el: any) => el.fpPicker.getValue());
const folderCard = (page: Page, name: string) =>
    picker(page).getByTestId('folder-card').filter({ has: page.getByTestId('folder-card-name').getByText(name, { exact: true }) });

const isUpload = (request: Request) => new URL(request.url()).pathname === '/api/upload';
const uploadRoute = (url: URL) => url.pathname === '/api/upload';

/** Opens the upload page and waits for the destination picker */
async function openUpload(page: Page, url = '/upload') {
    await gotoReady(page, url);
    await expect(picker(page)).toBeAttached();
    await expect.poll(() => pickerValue(page).catch(() => undefined)).not.toBeUndefined();
}

async function addFiles(page: Page, files: UploadFile[]) {
    await page.getByTestId('upload-input-files').setInputFiles(files);
}

test.describe('upload page', () => {
    test('has the steps, a keyboard-accessible drop zone and the Personal gallery chosen', async ({ alicePage: page }) => {
        await openUpload(page);
        await expect(page.getByTestId('topbar').getByRole('heading', { level: 1 })).toHaveText('Upload');
        await expect(page.getByTestId('dest-personal')).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('upload-dropzone')).toHaveJSProperty('tagName', 'BUTTON');
        await expect(page.getByTestId('upload-input-files')).toHaveAttribute('accept', 'image/*,video/*,.heic,.heif,.dng');
        await expect(page.getByTestId('upload-input-folder')).toHaveAttribute('webkitdirectory', '');
        // The queue shows up with the first file
        await expect(page.getByTestId('upload-queue-section')).toBeHidden();

        const chooser = page.waitForEvent('filechooser');
        await page.getByTestId('upload-dropzone').focus();
        await page.keyboard.press('Enter');
        expect((await chooser).isMultiple()).toBe(true);
    });

    test('a file that is already on the server is skipped, per destination', async ({ alicePage: page }) => {
        let uploads = 0;
        page.on('request', (request) => {
            if (isUpload(request)) uploads += 1;
        });
        const checks: any[] = [];
        page.on('request', (request) => {
            if (request.url().endsWith('/api/upload/check-hashes')) checks.push(request.postDataJSON());
        });

        await openUpload(page);
        await addFiles(page, [fixtureFile('alice/alice-b.jpg')]);

        const existing = row(page, 'alice-b.jpg');
        await expect(existing.getByTestId('upload-row-status')).toHaveText('Already on server');
        await expect(page.getByTestId('upload-summary')).toHaveText(/^1 file • \d+(\.\d)? KB$/);
        await expect(page.getByTestId('upload-start')).toBeDisabled();
        await expect(page.getByTestId('upload-start')).toHaveText('Upload photos');
        expect(checks[0].hashes[0]).toMatch(/^[0-9a-f]{64}$/);
        expect(checks[0]).toMatchObject({ make_public: false });

        // alice's photo isn't in the Family gallery, so it can go there
        await page.getByTestId('dest-family').click();
        await expect(existing.getByTestId('upload-row-status')).toHaveText('Queued');
        await expect(page.getByTestId('upload-start')).toHaveText('Upload 1 photo');
        expect(checks.at(-1)).toMatchObject({ make_public: true });

        await page.getByTestId('dest-personal').click();
        await expect(existing.getByTestId('upload-row-status')).toHaveText('Already on server');
        expect(uploads).toBe(0);
    });

    test('skips files that are not photos or videos, and files picked twice', async ({ alicePage: page }) => {
        // Paths, not buffers: picking the same file again gives the same modification time
        const input = page.getByTestId('upload-input-files');
        await openUpload(page);
        await input.setInputFiles([join(PHOTOS, 'alice/alice-b.jpg'), join(PHOTOS, '../make-fixtures.sh')]);
        await expect(rows(page)).toHaveCount(1);
        await expect(page.getByTestId('upload-notice')).toHaveText('1 file skipped (not photos or videos)');

        await input.setInputFiles([join(PHOTOS, 'alice/alice-b.jpg')]);
        await expect(rows(page)).toHaveCount(1);
        await expect(page.getByTestId('upload-notice')).toHaveText('1 file already in the list');

        await page.getByTestId('upload-notice').getByRole('button', { name: 'Dismiss' }).click();
        await expect(page.getByTestId('upload-notice')).toHaveCount(0);
        await row(page, 'alice-b.jpg').getByTestId('upload-row-remove').click();
        await expect(rows(page)).toHaveCount(0);
        await expect(page.getByTestId('upload-queue-section')).toBeHidden();
    });

    test('the page stays responsive while hashing hundreds of files', async ({ alicePage: page }, testInfo) => {
        const id = unique(testInfo);
        const files = Array.from({ length: 220 }, (_, i) => photoFile(`many-${i}.jpg`, '2049:01:01 10:00:00', `${id}-${i}`));
        await openUpload(page);
        // The longest gap between two frames while the files are added, hashed and checked
        await page.evaluate(() => {
            const w = window as any;
            w.longestFrame = 0;
            let last = performance.now();
            const tick = (now: number) => {
                w.longestFrame = Math.max(w.longestFrame, now - last);
                last = now;
                if (!w.stopFrames) requestAnimationFrame(tick);
            };
            requestAnimationFrame(tick);
        });

        await addFiles(page, files);
        await expect(page.getByTestId('upload-start')).toHaveText('Upload 220 photos', { timeout: 30_000 });
        await expect(rows(page).filter({ has: page.getByTestId('upload-row-status').getByText('Queued', { exact: true }) }))
            .toHaveCount(220);
        const longestFrame = await page.evaluate(() => {
            (window as any).stopFrames = true;
            return (window as any).longestFrame;
        });
        expect(longestFrame).toBeLessThan(1000);
    });

    test('files can be dropped anywhere on the page', async ({ alicePage: page }) => {
        await openUpload(page);
        const bytes = [...readFileSync(join(PHOTOS, 'alice/alice-c.jpg'))];
        const dataTransfer = await page.evaluateHandle((data) => {
            const transfer = new DataTransfer();
            transfer.items.add(new File([new Uint8Array(data)], 'alice-c.jpg', { type: 'image/jpeg' }));
            return transfer;
        }, bytes);

        await page.dispatchEvent('main', 'dragenter', { dataTransfer });
        await expect(page.getByTestId('upload-drop-overlay')).toBeVisible();
        await expect(page.getByTestId('upload-drop-overlay')).toHaveText('Drop to add files');
        await page.dispatchEvent('main', 'drop', { dataTransfer });
        await expect(page.getByTestId('upload-drop-overlay')).toBeHidden();
        await expect(row(page, 'alice-c.jpg').getByTestId('upload-row-status')).toHaveText('Already on server');
    });

    test("a folder page's Upload button preselects the folder", async ({ alicePage: page }) => {
        await gotoReady(page, `/folders/${folderId('Holiday')}`);
        await page.getByTestId('folder-upload').click();
        await expect(page).toHaveURL(`/upload?folder_id=${folderId('Holiday')}`);
        await expect(page.getByTestId('upload-destination-summary')).toContainText('Uploading to: Holiday');
        await expect(page.getByTestId('upload-destination-summary')).toContainText('Personal');
        await expect(picker(page)).toBeHidden();
        await expect.poll(() => pickerValue(page).catch(() => undefined))
            .toMatchObject({ kind: 'folder', folderId: folderId('Holiday') });

        await page.getByTestId('upload-destination-change').click();
        await expect(page.getByTestId('upload-destination-summary')).toBeHidden();
        await expect(picker(page)).toBeVisible();
        await expect(page.getByTestId('dest-selected-name')).toHaveText('Holiday');
    });

    test("bob can't upload to alice's view-only folder", async ({ bobPage: page }) => {
        await openUpload(page, `/upload?folder_id=${folderId('Holiday')}`);
        await expect(page.getByTestId('upload-destination-summary')).toHaveCount(0);
        expect(await pickerValue(page)).toEqual({ kind: 'root', isPublic: false });
        await expect(folderCard(page, 'Family Trip')).toBeVisible();
        await expect(folderCard(page, 'Holiday')).toHaveCount(0);
    });

    test('a folder shared with upload permission can be picked', async ({ page, folders }) => {
        // ivan is the grantee: a folder shared with bob would show up in bob's picker for other specs
        const name = folders.name('Drop box');
        const id = await folders.create('alice', name);
        const alice = await folders.as('alice');
        const share = await alice.post('/api/sharing', { data: { folder_id: id, grantee_id: 'ivan', can_upload: true } });
        expect(share.ok()).toBe(true);

        await loginAs(page, 'ivan');
        await openUpload(page, `/upload?folder_id=${id}`);
        await expect(page.getByTestId('upload-destination-summary')).toContainText(`Uploading to: ${name}`);
        await expect(page.getByTestId('upload-destination-summary')).toContainText('Shared');
        expect(await pickerValue(page)).toMatchObject({ kind: 'folder', folderId: id });

        await page.getByTestId('upload-destination-change').click();
        await page.getByTestId('dest-clear').click();
        await picker(page).getByTestId('filter-chip-shared').click();
        await expect(folderCard(page, name)).toBeVisible();
        await folderCard(page, name).click();
        expect(await pickerValue(page)).toMatchObject({ kind: 'folder', folderId: id, type: 'shared' });
    });
});

/** What a test created through the page as ivan (photos from `/api/upload`, folders from `/api/folders`) */
interface Created {
    photos: Promise<number | undefined>[];
    folders: Promise<number | undefined>[];
}

const idOf = (response: Response) => response.ok() ? response.json().then((json) => json.id as number, () => undefined) : undefined;

/** Records the photos and folders that the page creates, so `afterEach` can remove them */
function trackCreated(page: Page, created: Created) {
    page.on('response', (response) => {
        const { pathname } = new URL(response.url());
        const method = response.request().method();
        if (method !== 'POST') return;
        if (pathname === '/api/upload') created.photos.push(idOf(response)!);
        if (pathname === '/api/folders') created.folders.push(idOf(response)!);
    });
}

test.describe('uploading', () => {
    test.describe.configure({ mode: 'serial' });
    test.beforeEach(({ isMobile }) => {
        test.skip(isMobile, 'Uploads change ivan\'s photos');
    });

    let created: Created;
    test.beforeEach(() => {
        created = { photos: [], folders: [] };
    });

    // Leaves nothing behind: trashes and deletes the uploaded photos, then their folders
    // ("Uploads" is made through the `folders` fixture, which deletes it once it is empty)
    test.afterEach(async ({ page, isMobile }) => {
        if (isMobile) return;
        // A fresh session of the test's own: the test may have cleared its cookies
        await loginAs(page, 'ivan');
        const ivan = page.request;
        const photoIds = (await Promise.all(created.photos)).filter((id): id is number => id !== undefined);
        if (photoIds.length > 0) {
            expect((await ivan.post('/api/trash', { data: photoIds })).ok()).toBe(true);
            for (const id of photoIds) {
                expect((await ivan.delete(`/api/delete/${id}`)).ok()).toBe(true);
            }
        }
        for (const id of (await Promise.all(created.folders)).filter((id): id is number => id !== undefined)) {
            expect([200, 204]).toContain((await ivan.delete(`/api/folders/${id}`)).status());
        }
    });

    test('new photos go to the Personal gallery with their EXIF dates', async ({ page }, testInfo) => {
        const id = unique(testInfo);
        const uploads: URL[] = [];
        const checked: string[] = [];
        const warnings: string[] = [];
        page.on('request', (request) => {
            if (isUpload(request)) uploads.push(new URL(request.url()));
            if (request.url().endsWith('/api/upload/check-hashes')) checked.push(...request.postDataJSON().hashes);
        });
        page.on('console', (message) => {
            if (message.type() === 'warning') warnings.push(message.text());
        });

        trackCreated(page, created);
        await loginAs(page, 'ivan');
        await openUpload(page);
        await addFiles(page, [
            photoFile('march-a.jpg', '2045:03:10 09:00:00', id),
            photoFile('march-b.jpg', '2045:03:20 18:30:00', id),
            photoFile('may.jpg', '2045:05:02 12:00:00', id),
        ]);
        await expect(rows(page).getByTestId('upload-row-status')).toHaveText(['Queued', 'Queued', 'Queued']);
        await expect(page.getByTestId('upload-start')).toHaveText('Upload 3 photos');
        await page.getByTestId('upload-start').click();

        await expect(page.getByTestId('upload-done')).toBeVisible();
        await expect(page.getByTestId('upload-done-summary')).toHaveText('3 uploaded');
        await expect(rows(page).getByTestId('upload-row-status')).toHaveText(['Uploaded', 'Uploaded', 'Uploaded']);
        await expect(page.getByTestId('upload-progress')).toHaveJSProperty('value', 1);

        // The scoped CSP lets the hash worker compile WebAssembly: a silent fallback to "no hash"
        // must fail here
        expect(warnings.filter((text) => text.includes('Hashing failed'))).toEqual([]);
        expect(uploads).toHaveLength(3);
        expect(uploads.map((url) => url.searchParams.get('hash')).sort()).toEqual(checked.sort());
        for (const url of uploads) {
            expect(url.searchParams.get('make_public')).toBe('false');
            expect(url.searchParams.get('hash')).toMatch(/^[0-9a-f]{64}$/);
            expect(Number(url.searchParams.get('fallback_time'))).toBeGreaterThan(1_600_000_000);
            expect(url.searchParams.has('time_created')).toBe(false);
            expect(url.searchParams.has('folder_id')).toBe(false);
        }

        await expect(page.getByTestId('upload-view-photos')).toHaveAttribute('href', '/?filter=personal');
        await page.getByTestId('upload-view-photos').click();
        await expect(page.locator('[data-month="2045-05"]').getByTestId('month-title')).toHaveText('May 2045');
        // By id, so that a parallel run of this test (--repeat-each) with the same dates doesn't count
        const photoIds = await Promise.all(created.photos);
        expect(photoIds).toHaveLength(3);
        const tile = (month: string, photoId: number | undefined) =>
            page.locator(`[data-month="${month}"] [data-photo-id="${photoId}"]`);
        // Uploads finish in any order, so find each photo's month by trying both
        for (const photoId of photoIds) {
            await expect(page.locator(`[data-photo-id="${photoId}"]`)).toHaveCount(1);
        }
        const inMay = await Promise.all(photoIds.map((photoId) => tile('2045-05', photoId).count()));
        expect(inMay.reduce((sum, n) => sum + n, 0)).toBe(1);
        const inMarch = await Promise.all(photoIds.map((photoId) => tile('2045-03', photoId).count()));
        expect(inMarch.reduce((sum, n) => sum + n, 0)).toBe(2);
    });

    test('photos go into an existing folder and into a new one', async ({ page, folders }, testInfo) => {
        const id = unique(testInfo);
        const existingName = folders.name('Uploads');
        const existingId = await folders.create('ivan', existingName);

        trackCreated(page, created);
        await loginAs(page, 'ivan');
        await openUpload(page);
        await folderCard(page, existingName).click();
        await expect(page.getByTestId('dest-selected-name')).toHaveText(existingName);
        await addFiles(page, [photoFile('in-folder.jpg', '2046:01:05 10:00:00', id)]);
        await page.getByTestId('upload-start').click();
        await expect(page.getByTestId('upload-done-summary')).toHaveText('1 uploaded');
        await expect(page.getByTestId('upload-view-photos')).toHaveAttribute('href', `/folders/${existingId}`);

        // "Upload more" keeps the destination, which is changed to a new folder
        await page.getByTestId('upload-more').click();
        await expect(rows(page)).toHaveCount(0);
        await page.getByTestId('dest-clear').click();
        const newName = folders.name('Fresh');
        await page.getByTestId('dest-new').click();
        await page.getByTestId('folder-name-input').fill(newName);
        await page.getByTestId('folder-name-confirm').click();
        await expect(page.getByTestId('dest-selected-name')).toHaveText(newName);

        await addFiles(page, [
            photoFile('new-a.jpg', '2046:02:05 10:00:00', id),
            photoFile('new-b.jpg', '2046:02:06 10:00:00', id),
        ]);
        await expect(page.getByTestId('upload-start')).toHaveText('Upload 2 photos');
        await page.getByTestId('upload-start').click();
        await expect(page.getByTestId('upload-done-summary')).toHaveText('2 uploaded');
        await expect(page.getByTestId('upload-view-photos')).toHaveAttribute('href', /^\/folders\/\d+$/);

        await page.getByTestId('upload-view-photos').click();
        await expect(page.getByTestId('folder-title')).toHaveText(newName);
        await expect(page.getByTestId('photo-tile')).toHaveCount(2);

        await gotoReady(page, `/folders/${existingId}`);
        await expect(page.getByTestId('photo-tile')).toHaveCount(1);
    });

    test('server errors are retried, client errors fail with their message', async ({ page }, testInfo) => {
        const id = unique(testInfo);
        trackCreated(page, created);
        await loginAs(page, 'ivan');
        await openUpload(page);

        let attempts = 0;
        await page.route(uploadRoute, (route) => {
            attempts += 1;
            return attempts === 1 ? route.fulfill({ status: 500, body: 'Internal server error' }) : route.continue();
        });
        await addFiles(page, [photoFile('flaky.jpg', '2047:01:10 10:00:00', id)]);
        await page.getByTestId('upload-start').click();
        await expect(page.getByTestId('upload-done-summary')).toHaveText('1 uploaded');
        expect(attempts).toBe(2);

        await page.unrouteAll();
        attempts = 0;
        await page.route(uploadRoute, (route) => {
            attempts += 1;
            return route.fulfill({ status: 403, contentType: 'text/plain', body: "You can't upload here" });
        });
        await page.getByTestId('upload-more').click();
        await addFiles(page, [photoFile('refused.jpg', '2047:01:11 10:00:00', id)]);
        await page.getByTestId('upload-start').click();

        const refused = row(page, 'refused.jpg');
        await expect(refused.getByTestId('upload-row-status')).toHaveText('Failed');
        await expect(refused.getByTestId('upload-row-error')).toHaveText("You can't upload here");
        await expect(page.getByTestId('upload-done-summary')).toHaveText('0 uploaded • 1 failed');
        expect(attempts).toBe(1);

        // Retrying works once the server accepts it
        await page.unrouteAll();
        await page.getByTestId('upload-retry-failed').click();
        await expect(refused.getByTestId('upload-row-status')).toHaveText('Uploaded');
        await expect(page.getByTestId('upload-done-summary')).toHaveText('1 uploaded');
        await expect(page.getByTestId('upload-retry-failed')).toBeHidden();
    });

    test('cancelling stops the queue and the summary shows what was uploaded', async ({ page }, testInfo) => {
        const id = unique(testInfo);
        trackCreated(page, created);
        await loginAs(page, 'ivan');
        await openUpload(page);

        // Only "first.jpg" gets through; the other uploads hang until they are cancelled
        await page.route(uploadRoute, async (route) => {
            if (route.request().postDataBuffer()?.includes('filename="first.jpg"')) {
                await route.continue();
            }
        });
        await addFiles(page, ['first', 'second', 'third', 'fourth'].map((name, i) =>
            photoFile(`${name}.jpg`, `2048:01:0${i + 1} 10:00:00`, id)));
        await page.getByTestId('upload-start').click();

        await expect(row(page, 'first.jpg').getByTestId('upload-row-status')).toHaveText('Uploaded');
        await expect(rows(page).getByTestId('upload-row-status')).toHaveText(['Uploaded', /^Uploading/, /^Uploading/, /^Uploading/]);
        await expect(page.getByTestId('upload-start')).toBeHidden();
        await page.getByTestId('upload-cancel').click();

        await expect(page.getByTestId('upload-done-summary')).toHaveText('1 uploaded • 3 cancelled');
        await expect(rows(page).getByTestId('upload-row-status')).toHaveText(['Uploaded', 'Cancelled', 'Cancelled', 'Cancelled']);
        await expect(page.getByTestId('upload-retry-failed')).toBeVisible();
        await page.unrouteAll({ behavior: 'ignoreErrors' });
    });

    for (const leave of [false, true]) {
        test(`an expired session cancels the queue and redirects to the login page once (${leave ? 'leaving' : 'staying'})`, async ({ page }, testInfo) => {
            const id = unique(testInfo);
            trackCreated(page, created);
            await loginAs(page, 'ivan');
            await openUpload(page);

            const dialogs: string[] = [];
            page.on('dialog', async (dialog) => {
                dialogs.push(dialog.type());
                await (leave ? dialog.accept() : dialog.dismiss());
            });
            let profileChecks = 0;
            let loginLoads = 0;
            page.on('request', (request) => {
                const { pathname } = new URL(request.url());
                if (pathname === '/api/profile') profileChecks += 1;
                if (pathname === '/login' && request.resourceType() === 'document') loginLoads += 1;
            });

            await addFiles(page, ['a', 'b', 'c', 'd', 'e'].map((name, i) =>
                photoFile(`${name}.jpg`, `2049:01:0${i + 1} 10:00:00`, id)));
            await expect(rows(page).getByTestId('upload-row-status')).toHaveText(Array(5).fill('Queued'));

            // The session ends while the files wait
            await page.context().clearCookies();
            await page.getByTestId('upload-start').click();

            if (leave) {
                await expect(page).toHaveURL('/login?next=%2Fupload');
            } else {
                // Staying: nothing is sent any more, and the rows say why
                await expect(page.getByTestId('upload-done')).toBeVisible();
                await expect(page.getByTestId('upload-row-error').first()).toHaveText('Session expired. Log in and retry');
                await expect(page.getByTestId('upload-row-error').filter({ hasText: "can't upload" })).toHaveCount(0);
                await expect(page.getByTestId('upload-start')).toBeHidden();
                expect(page.url()).toContain('/upload');
            }
            await page.waitForTimeout(500);
            expect(dialogs).toHaveLength(1);
            expect(profileChecks).toBe(1);
            expect(loginLoads).toBe(leave ? 1 : 0);
        });
    }
});
