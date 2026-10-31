import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';
import { expect, gotoReady, test } from '../fixtures/auth';
import { folderId, photo } from '../harness/seed';

/** Fails on serious and critical violations, listing them readably. */
async function expectAccessible(page: Page, scope?: string) {
    let axe = new AxeBuilder({ page });
    if (scope) axe = axe.include(scope);
    const { violations } = await axe.analyze();
    const serious = violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
    expect(
        serious.map((v) => `${v.id} (${v.impact}): ${v.nodes.map((n) => n.target.join(' ')).slice(0, 5).join(' | ')}`),
    ).toEqual([]);
}

const grid = (page: Page) => expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');

test.describe('pages', () => {
    const pages = [
        ['timeline', '/'],
        ['folders', '/folders'],
        ['folder', `/folders/${folderId('Holiday')}`],
        ['favorites', '/favorites'],
        ['utilities', '/utilities'],
        ['trash', '/trash'],
        ['duplicates', '/duplicates'],
        ['large files', '/large-files'],
        ['settings', '/settings'],
        ['upload', '/upload'],
    ];
    for (const [name, url] of pages) {
        test(name, async ({ alicePage: page }) => {
            await gotoReady(page, url);
            await expectAccessible(page);
        });
    }

    test('login', async ({ page }) => {
        await page.goto('/login');
        await expectAccessible(page);
    });

    test('not found', async ({ alicePage: page }) => {
        const response = await page.goto('/no-such-page');
        expect(response?.status()).toBe(404);
        await expectAccessible(page);
    });
});

test.describe('open states', () => {
    // Axe samples colours while dialogs fade in; reduced motion makes them settle at once
    test.beforeEach(({ alicePage: page }) => page.emulateMedia({ reducedMotion: 'reduce' }));

    async function openViewer(page: Page) {
        const target = photo('alice/alice-a.jpg');
        await gotoReady(page, '/');
        await grid(page);
        await page.locator(`[data-testid="photo-tile"][data-photo-id="${target.id}"]`).click();
        await expect(page.getByTestId('viewer')).toHaveAttribute('data-photo-id', String(target.id));
    }

    test('viewer and delete dialog', async ({ alicePage: page }) => {
        await openViewer(page);
        await expectAccessible(page);

        await page.getByTestId('viewer-delete').click();
        await expect(page.locator('dialog[open]')).toBeVisible();
        await expectAccessible(page);
    });

    test('info panel', async ({ alicePage: page }) => {
        await openViewer(page);
        await page.getByTestId('viewer-info').click();
        await expect(page.getByTestId('viewer-info-content')).toBeVisible();
        await expectAccessible(page);
    });

    test('selection bar and move dialog', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await grid(page);
        await page.getByTestId('photo-tile').first().locator('.photo-tile__check').click();
        await expect(page.getByTestId('selection-bar')).toBeVisible();
        await expectAccessible(page);

        await page.getByTestId('selection-move').click();
        await expect(page.getByTestId('move-dialog')).toBeVisible();
        await expectAccessible(page);
    });

    test('month picker', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await grid(page);
        await page.getByTestId('month-picker-open').first().click();
        await expect(page.locator('dialog[open]')).toBeVisible();
        await expectAccessible(page);
    });

    test('sharing dialog', async ({ alicePage: page }) => {
        await gotoReady(page, `/folders/${folderId('Holiday')}`);
        await page.getByTestId('folder-share').click();
        await expect(page.locator('dialog[open]')).toBeVisible();
        await expectAccessible(page);
    });
});
