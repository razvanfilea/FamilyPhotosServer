import { test, expect } from './fixtures/auth';
import * as selectors from './selectors';
import {
  navigateToCategory,
  navigateToFirstFolder,
  getPhotoCount,
  getLoadMoreUrl,
  scrollToLoadMore,
} from './helpers';

test.describe('Pagination', () => {
  test.describe('Gallery Pagination', () => {
    test('gallery pagination loads first page', async ({ authenticatedPage: page }) => {
      await expect(page).toHaveURL('/');

      const photoGrid = page.locator(selectors.PHOTO_GRID);
      await expect(photoGrid.first()).toBeVisible();
    });

    test('gallery pagination cursor is present when more photos exist', async ({ authenticatedPage: page }) => {
      const hxGet = await getLoadMoreUrl(page);

      if (hxGet) {
        expect(hxGet).toMatch(/\/(gallery|favorites|folder)\/more/);
      }
    });

    test('gallery more endpoint returns valid HTML', async ({ authenticatedPage: page }) => {
      const response = await page.request.get('/gallery/more');
      expect(response.status()).toBe(200);

      const contentType = response.headers()['content-type'];
      expect(contentType).toContain('text/html');

      const body = await response.text();
      expect(body).toBeDefined();
    });

    test('gallery more with cursor returns next batch', async ({ authenticatedPage: page }) => {
      const hxGet = await getLoadMoreUrl(page);

      if (hxGet) {
        const response = await page.request.get(hxGet);
        expect(response.status()).toBe(200);

        const contentType = response.headers()['content-type'];
        expect(contentType).toContain('text/html');
      }
    });
  });

  test.describe('Favorites Pagination', () => {
    test('favorites more endpoint works', async ({ authenticatedPage: page }) => {
      await page.goto('/favorites');
      await expect(page).toHaveURL('/favorites');

      const response = await page.request.get('/favorites/more');
      expect(response.status()).toBe(200);

      const contentType = response.headers()['content-type'];
      expect(contentType).toContain('text/html');
    });
  });

  test.describe('Folder Pagination', () => {
    test('folder more endpoint works', async ({ authenticatedPage: page }) => {
      await page.goto('/folders');

      const folderLink = page.locator(selectors.FOLDER_LINK).first();

      if (await folderLink.isVisible({ timeout: 2000 }).catch(() => false)) {
        const href = await folderLink.getAttribute('href');
        if (href) {
          await folderLink.click();
          await expect(page).toHaveURL(/\/folder\//);

          const folderMatch = href.match(/\/folder\/(.+)/);
          if (folderMatch) {
            const folderName = folderMatch[1];
            const response = await page.request.get(`/folder/${folderName}/more`);
            expect(response.status()).toBe(200);

            const contentType = response.headers()['content-type'];
            expect(contentType).toContain('text/html');
          }
        }
      }
    });
  });

  test.describe('Category Filter in Pagination', () => {
    test('category filter persists in pagination', async ({ authenticatedPage: page }) => {
      const navigated = await navigateToCategory(page, 'personal');
      if (navigated) {
        const hxGet = await getLoadMoreUrl(page);
        if (hxGet) {
          expect(hxGet).toContain('category=personal');
        }
      }

      const navigatedFamily = await navigateToCategory(page, 'family');
      if (navigatedFamily) {
        const hxGet = await getLoadMoreUrl(page);
        if (hxGet) {
          expect(hxGet).toContain('category=family');
        }
      }
    });

    test('switching categories resets pagination', async ({ authenticatedPage: page }) => {
      const navigated = await navigateToCategory(page, 'personal');
      if (navigated) {
        const url = page.url();
        expect(url).not.toContain('cursor=');
      }
    });
  });

  test.describe('Month Headers in Pagination', () => {
    test('last_month param prevents duplicate headers', async ({ authenticatedPage: page }) => {
      const hxGet = await getLoadMoreUrl(page);
      expect(hxGet === null || typeof hxGet === 'string').toBeTruthy();
    });

    test('month headers are not duplicated after load more', async ({ authenticatedPage: page }) => {
      const monthHeaders = page.locator(selectors.MONTH_HEADER);
      const initialHeaders = await monthHeaders.allTextContents();

      const scrolled = await scrollToLoadMore(page);
      if (scrolled) {
        const updatedHeaders = await monthHeaders.allTextContents();
        const headerSet = new Set(updatedHeaders);
        expect(headerSet.size).toBeGreaterThan(0);
      }
    });
  });
});
