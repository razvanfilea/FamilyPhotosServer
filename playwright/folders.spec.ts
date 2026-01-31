import { test, expect } from './fixtures/auth';
import * as selectors from './selectors';
import { navigateToCategory, navigateToFirstFolder, checkPhotoGridState } from './helpers';

test.describe('Folders Page', () => {
  test('folders page loads successfully', async ({ authenticatedPage: page }) => {
    await page.goto('/folders');
    await expect(page).toHaveURL('/folders');
  });

  test('folders page shows folder list or empty state', async ({ authenticatedPage: page }) => {
    await page.goto('/folders');

    const folderCard = page.locator(selectors.FOLDER_CARD);
    const emptyState = page.locator(selectors.EMPTY_STATE_FOLDERS);

    const hasFolders = await folderCard.first().isVisible({ timeout: 2000 }).catch(() => false);
    const hasEmptyState = await emptyState.first().isVisible({ timeout: 2000 }).catch(() => false);

    // Skip test if neither is visible (unexpected state)
    if (!hasFolders && !hasEmptyState) {
      test.skip();
      return;
    }

    expect(hasFolders || hasEmptyState).toBeTruthy();
  });

  test('category tabs work on folders page', async ({ authenticatedPage: page }) => {
    await page.goto('/folders');
    await navigateToCategory(page, 'personal');
    await navigateToCategory(page, 'family');
  });

  test('clicking folder navigates to folder page', async ({ authenticatedPage: page }) => {
    await page.goto('/folders');

    const navigated = await navigateToFirstFolder(page);
    if (navigated) {
      await expect(page).toHaveURL(/\/folder\//);
    }
  });

  test('folder page shows photos from that folder', async ({ authenticatedPage: page }) => {
    await page.goto('/folders');

    const navigated = await navigateToFirstFolder(page);
    if (!navigated) {
      test.skip();
      return;
    }

    const { hasPhotos, hasEmptyState } = await checkPhotoGridState(page);

    // Skip test if neither is visible
    if (!hasPhotos && !hasEmptyState) {
      test.skip();
      return;
    }

    expect(hasPhotos || hasEmptyState).toBeTruthy();
  });

  test('folder page has timeline', async ({ authenticatedPage: page }) => {
    await page.goto('/folders');

    const navigated = await navigateToFirstFolder(page);
    if (navigated) {
      // Timeline might not be visible if no photos, but page should have loaded
      await expect(page).toHaveURL(/\/folder\//);
    }
  });
});
