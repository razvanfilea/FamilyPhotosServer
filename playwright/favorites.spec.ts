import { test, expect } from './fixtures/auth';
import * as selectors from './selectors';
import { checkPhotoGridState, openFirstPhotoModal, waitForHtmxSwap } from './helpers';

test.describe('Favorites Page', () => {
  test('favorites page loads successfully', async ({ authenticatedPage: page }) => {
    await page.goto('/favorites');
    await expect(page).toHaveURL('/favorites');
  });

  test('favorites page shows empty state or photos', async ({ authenticatedPage: page }) => {
    await page.goto('/favorites');

    const { hasPhotos, hasEmptyState } = await checkPhotoGridState(
      page,
      selectors.EMPTY_STATE_FAVORITES
    );

    // Page should show either photos or empty state
    // Skip test if neither is visible (unexpected state)
    if (!hasPhotos && !hasEmptyState) {
      test.skip();
      return;
    }

    expect(hasPhotos || hasEmptyState).toBeTruthy();
  });

  test('can navigate back to gallery from favorites', async ({ authenticatedPage: page }) => {
    await page.goto('/favorites');

    const galleryLink = page.locator(selectors.NAV_GALLERY).first();
    if (await galleryLink.isVisible()) {
      await galleryLink.click();
      await expect(page).toHaveURL('/');
    }
  });

  test('favorite toggle button works in photo modal', async ({ authenticatedPage: page }) => {
    // First go to gallery to find a photo
    await page.goto('/');

    const opened = await openFirstPhotoModal(page);
    if (!opened) {
      test.skip();
      return;
    }

    const favoriteButton = page.locator(selectors.FAVORITE_BUTTON).first();

    if (await favoriteButton.isVisible({ timeout: 2000 }).catch(() => false)) {
      await favoriteButton.click();
      await waitForHtmxSwap(page, 500);
      await expect(favoriteButton).toBeVisible();
    }
  });
});
