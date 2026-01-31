import { test, expect } from '@playwright/test';
import { login, TEST_USER } from './fixtures/auth';
import * as selectors from './selectors';

test.describe('Authentication', () => {
  test('login page is accessible', async ({ page }) => {
    await page.goto('/login');
    await expect(page).toHaveURL('/login');
    await expect(page.locator(selectors.LOGIN_USER_ID)).toBeVisible();
    await expect(page.locator(selectors.LOGIN_PASSWORD)).toBeVisible();
  });

  test('unauthenticated user is redirected to login', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveURL(/\/login/);
  });

  test('unauthenticated user cannot access favorites', async ({ page }) => {
    await page.goto('/favorites');
    await expect(page).toHaveURL(/\/login/);
  });

  test('unauthenticated user cannot access folders', async ({ page }) => {
    await page.goto('/folders');
    await expect(page).toHaveURL(/\/login/);
  });

  test('unauthenticated user cannot access trash', async ({ page }) => {
    await page.goto('/trash');
    await expect(page).toHaveURL(/\/login/);
  });

  test('successful login redirects to gallery', async ({ page }) => {
    await login(page);
    await expect(page).toHaveURL('/');
  });

  test('invalid credentials shows error', async ({ page }) => {
    await page.goto('/login');
    await page.fill(selectors.LOGIN_USER_ID, TEST_USER.userId);
    await page.fill(selectors.LOGIN_PASSWORD, 'wrong_password');
    await page.click(selectors.LOGIN_SUBMIT);

    await expect(page).toHaveURL(/\/login/);
  });

  test('logout redirects to login', async ({ page }) => {
    await login(page);
    await expect(page).toHaveURL('/');

    const logoutButton = page.locator(selectors.NAV_LOGOUT);
    if (await logoutButton.isVisible()) {
      await logoutButton.click();
      await expect(page).toHaveURL(/\/login/);
    }
  });
});
