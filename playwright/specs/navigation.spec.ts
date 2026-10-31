import { expect, gotoReady, test } from '../fixtures/auth';

test.describe('in-place tab navigation', () => {
    test('switching tabs swaps the page without a reload and keeps it wired up', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.evaluate(() => { (window as any).__kept = true; });
        const nav = (tab: string) => page.getByTestId(`nav-${tab}`).or(page.getByTestId(`bottom-nav-${tab}`)).locator('visible=true');

        await nav('folders').click();
        await expect(page).toHaveURL(/\/folders$/);
        await expect(page).toHaveTitle('Folders - Family Photos');
        await expect(page.getByTestId('topbar').getByRole('heading', { level: 1 })).toHaveText('Folders');
        await expect(nav('folders')).toHaveAttribute('aria-current', 'page');
        await expect(page.getByTestId('folder-card').first()).toBeVisible();

        await nav('photos').click();
        await expect(page).toHaveURL(/\/$/);
        await expect(page.getByTestId('photo-tile').first()).toBeVisible();
        expect(await page.evaluate(() => scrollY)).toBe(0);
        await page.getByTestId('photo-tile').first().click();
        await expect(page.getByTestId('viewer')).toBeVisible();
        await page.keyboard.press('Escape');
        await expect(page.getByTestId('viewer')).toBeHidden();

        await page.goBack();
        await expect(page).toHaveURL(/\/folders$/);
        await expect(page.getByTestId('folder-card').first()).toBeVisible();

        await page.goForward();
        await expect(page).toHaveURL(/\/$/);
        await expect(page.getByTestId('photo-tile').first()).toBeVisible();

        expect(await page.evaluate(() => (window as any).__kept)).toBe(true);
    });

    test('going back restores the scroll position', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.evaluate(() => scrollTo(0, 300));
        await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(100);
        const before = await page.evaluate(() => scrollY);
        await page.getByTestId('nav-folders').or(page.getByTestId('bottom-nav-folders')).locator('visible=true').click();
        await expect(page).toHaveURL(/\/folders$/);
        await page.goBack();
        await expect(page.getByTestId('photo-tile').first()).toBeVisible();
        await expect.poll(() => page.evaluate(() => scrollY)).toBe(before);
    });

    test('tapping the open tab scrolls to the top instead of reloading', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.evaluate(() => { (window as any).__kept = true; scrollTo(0, 300); });
        await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(100);
        await page.getByTestId('nav-photos').or(page.getByTestId('bottom-nav-photos')).locator('visible=true').click();
        await expect.poll(() => page.evaluate(() => scrollY)).toBe(0);
        expect(await page.evaluate(() => (window as any).__kept)).toBe(true);
    });
});
