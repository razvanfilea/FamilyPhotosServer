import { test as base, Page } from '@playwright/test';

export const TEST_USER = {
  userId: 'test_user',
  password: 'test_password_123',
};

export async function login(page: Page, userId = TEST_USER.userId, password = TEST_USER.password) {
  await page.goto('/login');
  await page.fill('input[name="user_id"]', userId);
  await page.fill('input[name="password"]', password);
  await page.click('button[type="submit"]');
  await page.waitForURL('/');
}

type AuthFixtures = {
  authenticatedPage: Page;
};

export const test = base.extend<AuthFixtures>({
  authenticatedPage: async ({ page }, use) => {
    await login(page);
    await use(page);
  },
});

export { expect } from '@playwright/test';
