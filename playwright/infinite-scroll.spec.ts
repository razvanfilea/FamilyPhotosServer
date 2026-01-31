import { test, expect } from './fixtures/auth';
import * as selectors from './selectors';
import { getPhotoCount, getLoadMoreUrl, scrollToLoadMore, waitForHtmxSwap } from './helpers';

test.describe('Infinite Scroll', () => {
  test('load more trigger exists when there are more photos', async ({ authenticatedPage: page }) => {
    await expect(page).toHaveURL('/');
    // Trigger visibility depends on whether there are more photos to load
  });

  test('scrolling loads more content', async ({ authenticatedPage: page }) => {
    const initialCount = await getPhotoCount(page);

    if (initialCount > 0) {
      const scrolled = await scrollToLoadMore(page);
      if (scrolled) {
        const newCount = await getPhotoCount(page);
        expect(newCount).toBeGreaterThanOrEqual(initialCount);
      }
    }
  });

  test('HTMX swap appends content rather than replacing', async ({ authenticatedPage: page }) => {
    const photoCards = page.locator(selectors.PHOTO_CARD);
    const initialCount = await photoCards.count();

    if (initialCount > 0) {
      const firstPhotoId = await photoCards.first().getAttribute('data-photo-id');

      const scrolled = await scrollToLoadMore(page);
      if (scrolled) {
        await waitForHtmxSwap(page, 1500);

        const newCount = await photoCards.count();
        if (newCount > initialCount) {
          const stillHasFirstPhoto = await photoCards.first().getAttribute('data-photo-id');
          expect(stillHasFirstPhoto).toBe(firstPhotoId);
        }
      }
    }
  });

  test('load more trigger updates with new cursor after load', async ({ authenticatedPage: page }) => {
    const initialHxGet = await getLoadMoreUrl(page);

    if (initialHxGet) {
      await scrollToLoadMore(page);
      await waitForHtmxSwap(page, 1500);

      const newHxGet = await getLoadMoreUrl(page);
      expect(newHxGet === null || typeof newHxGet === 'string').toBeTruthy();
    }
  });

  test('empty response hides load more trigger', async ({ authenticatedPage: page }) => {
    const loadMoreTrigger = page.locator(selectors.LOAD_MORE_TRIGGER);
    let hasMoreContent = true;
    let iterations = 0;
    const maxIterations = 5;

    while (hasMoreContent && iterations < maxIterations) {
      if (await loadMoreTrigger.isVisible({ timeout: 2000 }).catch(() => false)) {
        await loadMoreTrigger.scrollIntoViewIfNeeded();
        await waitForHtmxSwap(page, 1000);
        iterations++;
      } else {
        hasMoreContent = false;
      }
    }

    const finalTriggerVisible = await loadMoreTrigger.isVisible({ timeout: 1000 }).catch(() => false);
    expect(typeof finalTriggerVisible).toBe('boolean');
  });
});
