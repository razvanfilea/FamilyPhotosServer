/**
 * Shared test helper functions for Playwright tests.
 */

import { Page, expect } from '@playwright/test';
import * as selectors from './selectors';

/**
 * Navigate to a category (personal or family) using category tabs.
 * @returns true if navigation was successful, false if tab not found
 */
export async function navigateToCategory(
  page: Page,
  category: 'personal' | 'family'
): Promise<boolean> {
  const linkSelector = category === 'personal'
    ? selectors.CATEGORY_LINK_PERSONAL
    : selectors.CATEGORY_LINK_FAMILY;

  const link = page.locator(linkSelector).first();

  if (await link.isVisible().catch(() => false)) {
    await link.click();
    await expect(page).toHaveURL(new RegExp(`category=${category}`));
    return true;
  }
  return false;
}

/**
 * Count the number of photo cards on the page.
 */
export async function getPhotoCount(page: Page): Promise<number> {
  return page.locator(selectors.PHOTO_CARD).count();
}

/**
 * Wait for HTMX swap to complete by waiting for network idle.
 */
export async function waitForHtmxSwap(page: Page, timeout = 1000): Promise<void> {
  await page.waitForTimeout(timeout);
}

/**
 * Scroll to the load more trigger to initiate infinite scroll.
 * @returns true if trigger was found and scrolled to, false otherwise
 */
export async function scrollToLoadMore(page: Page): Promise<boolean> {
  const trigger = page.locator(selectors.LOAD_MORE_TRIGGER);

  if (await trigger.isVisible({ timeout: 2000 }).catch(() => false)) {
    await trigger.scrollIntoViewIfNeeded();
    await waitForHtmxSwap(page, 1000);
    return true;
  }
  return false;
}

/**
 * Navigate to a folder from the folders page.
 * @returns true if navigation was successful, false if no folders found
 */
export async function navigateToFirstFolder(page: Page): Promise<boolean> {
  const folderLink = page.locator(selectors.FOLDER_LINK).first();

  if (await folderLink.isVisible({ timeout: 2000 }).catch(() => false)) {
    await folderLink.click();
    await expect(page).toHaveURL(/\/folder\//);
    return true;
  }
  return false;
}

/**
 * Open a photo modal by clicking the first photo card.
 * @deprecated Use openFirstPhotoViewer instead
 * @returns true if modal opened, false if no photos found
 */
export async function openFirstPhotoModal(page: Page): Promise<boolean> {
  return openFirstPhotoViewer(page);
}

/**
 * Close the photo modal using escape key or close button.
 * @deprecated Use closePhotoViewer instead
 */
export async function closePhotoModal(page: Page): Promise<void> {
  return closePhotoViewer(page);
}

/**
 * Open the fullscreen photo viewer by clicking the first photo card.
 * @returns true if viewer opened, false if no photos found
 */
export async function openFirstPhotoViewer(page: Page): Promise<boolean> {
  const photoCard = page.locator(selectors.PHOTO_CARD).first();

  if (await photoCard.isVisible({ timeout: 2000 }).catch(() => false)) {
    await photoCard.click();
    const viewer = page.locator(selectors.PHOTO_VIEWER);
    await expect(viewer).toBeVisible({ timeout: 2000 });
    return true;
  }
  return false;
}

/**
 * Close the fullscreen photo viewer using escape key.
 */
export async function closePhotoViewer(page: Page): Promise<void> {
  const viewer = page.locator(selectors.PHOTO_VIEWER);

  if (await viewer.isVisible().catch(() => false)) {
    await page.keyboard.press('Escape');
    await expect(viewer).not.toBeVisible({ timeout: 1000 });
  }
}

/**
 * Check if the page has photos or an empty state.
 * @returns { hasPhotos: boolean, hasEmptyState: boolean }
 */
export async function checkPhotoGridState(
  page: Page,
  emptyStateSelector = selectors.EMPTY_STATE
): Promise<{ hasPhotos: boolean; hasEmptyState: boolean }> {
  const photoGrid = page.locator(selectors.PHOTO_GRID);
  const emptyState = page.locator(emptyStateSelector);

  const hasPhotos = await photoGrid.first().isVisible({ timeout: 2000 }).catch(() => false);
  const hasEmptyState = await emptyState.first().isVisible({ timeout: 2000 }).catch(() => false);

  return { hasPhotos, hasEmptyState };
}

/**
 * Get the hx-get attribute value from the load more trigger.
 * @returns the hx-get URL or null if no trigger found
 */
export async function getLoadMoreUrl(page: Page): Promise<string | null> {
  const trigger = page.locator(selectors.LOAD_MORE_TRIGGER);

  if (await trigger.isVisible({ timeout: 2000 }).catch(() => false)) {
    return trigger.getAttribute('hx-get');
  }
  return null;
}

/**
 * Check if a navigation link is visible.
 */
export async function isNavLinkVisible(
  page: Page,
  link: 'favorites' | 'folders' | 'trash' | 'gallery'
): Promise<boolean> {
  const selectorMap = {
    favorites: selectors.NAV_FAVORITES,
    folders: selectors.NAV_FOLDERS,
    trash: selectors.NAV_TRASH,
    gallery: selectors.NAV_GALLERY,
  };

  return page.locator(selectorMap[link]).isVisible().catch(() => false);
}
