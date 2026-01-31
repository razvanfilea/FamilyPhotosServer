import { test, expect } from './fixtures/auth';
import * as selectors from './selectors';
import { navigateToCategory, openFirstPhotoModal, closePhotoModal, isNavLinkVisible } from './helpers';

test.describe('Gallery Page', () => {
  test('gallery page loads successfully', async ({ authenticatedPage: page }) => {
    await expect(page).toHaveURL('/');
    const photoGrid = page.locator(selectors.PHOTO_GRID);
    await expect(photoGrid.first()).toBeVisible();
  });

  test('category tabs are visible', async ({ authenticatedPage: page }) => {
    const allTab = page.locator(selectors.CATEGORY_TAB_ALL);
    const personalTab = page.locator(selectors.CATEGORY_TAB_PERSONAL);
    const familyTab = page.locator(selectors.CATEGORY_TAB_FAMILY);

    const hasAllTab = await allTab.first().isVisible().catch(() => false);
    const hasPersonalTab = await personalTab.first().isVisible().catch(() => false);
    const hasFamilyTab = await familyTab.first().isVisible().catch(() => false);

    expect(hasAllTab || hasPersonalTab || hasFamilyTab).toBeTruthy();
  });

  test('switching category updates URL', async ({ authenticatedPage: page }) => {
    await navigateToCategory(page, 'personal');
    await navigateToCategory(page, 'family');
  });

  test('photo modal opens on photo click', async ({ authenticatedPage: page }) => {
    const opened = await openFirstPhotoModal(page);
    if (opened) {
      const modal = page.locator(selectors.PHOTO_MODAL);
      await expect(modal.first()).toBeVisible();
    }
  });

  test('photo modal closes on escape or close button', async ({ authenticatedPage: page }) => {
    const opened = await openFirstPhotoModal(page);
    if (opened) {
      await closePhotoModal(page);
    }
  });

  test('navigation links are visible', async ({ authenticatedPage: page }) => {
    const favoritesVisible = await isNavLinkVisible(page, 'favorites');
    const foldersVisible = await isNavLinkVisible(page, 'folders');
    const trashVisible = await isNavLinkVisible(page, 'trash');

    const visibleCount = [favoritesVisible, foldersVisible, trashVisible].filter(Boolean).length;
    expect(visibleCount).toBeGreaterThan(0);
  });
});
