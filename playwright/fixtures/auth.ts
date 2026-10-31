import { test as base, expect, type Browser, type Page, type TestInfo } from '@playwright/test';
import { USERS } from '../harness/users.mjs';
import { storageStatePath } from '../harness/run';

export type UserId = 'alice' | 'bob' | 'carol' | 'dave' | 'erin' | 'frank' | 'grace' | 'hank' | 'heidi' | 'ivan' | 'judy';

export function user(id: UserId) {
    return USERS.find((u) => u.id === id)!;
}

/**
 * Logs in with a new session through the API. Use it (instead of the shared sessions of the
 * `alicePage`/`bobPage` fixtures) in tests that log out, since logging out ends the session.
 */
export async function loginAs(page: Page, id: UserId) {
    const { password } = user(id);
    const response = await page.request.post('/api/login', { form: { user_id: id, password } });
    expect(response.ok()).toBe(true);
}

/** Waits until app.js has booted, so the page's components and shortcuts are live. */
export async function waitForApp(page: Page) {
    await expect(page.locator('html')).toHaveAttribute('data-app-ready', '');
}

/** Navigates and waits until the page is interactive. */
export async function gotoReady(page: Page, url: string) {
    const response = await page.goto(url);
    await waitForApp(page);
    return response;
}

/** A page in a new context of the current project's device, logged in with the user's saved session. */
async function userPage(browser: Browser, testInfo: TestInfo, id: UserId) {
    const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch, baseURL } = testInfo.project.use;
    const context = await browser.newContext({
        viewport, userAgent, deviceScaleFactor, isMobile, hasTouch, baseURL,
        storageState: storageStatePath(id),
    });
    return context.newPage();
}

type Fixtures = {
    alicePage: Page;
    bobPage: Page;
};

export const test = base.extend<Fixtures>({
    alicePage: async ({ browser }, use, testInfo) => {
        const page = await userPage(browser, testInfo, 'alice');
        await use(page);
        await page.context().close();
    },
    bobPage: async ({ browser }, use, testInfo) => {
        const page = await userPage(browser, testInfo, 'bob');
        await use(page);
        await page.context().close();
    },
});

export { expect };
