import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';

const installCommand = 'curl -fsSL https://huacnlee.github.io/disktree/install.sh | sh';

test('English and Chinese routes keep correct links, language and metadata', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('./');
  await expect(page.locator('h1')).toHaveText('See where your disk went.');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://huacnlee.github.io/disktree/');
  await expect(page.locator('link[rel="icon"]')).toHaveAttribute('href', '/disktree/favicon.svg');
  const favicon = await page.request.get('/disktree/favicon.svg');
  expect(favicon.ok()).toBe(true);
  expect(favicon.headers()['content-type']).toContain('image/svg+xml');
  await expect(page.getByRole('link', { name: 'Native Shell' })).toHaveAttribute('href', 'https://nsh.dev');
  await expect(page.getByRole('link', { name: 'tobi/disktree' })).toHaveAttribute('href', 'https://github.com/tobi/disktree');

  await page.locator('#language-toggle').click();
  await page.getByRole('menuitemradio', { name: '简体中文' }).click();
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
  await expect(page.locator('h1')).toHaveText('看清磁盘空间去了哪里');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://huacnlee.github.io/disktree/zh-CN/');
  await expect(page.getByRole('link', { name: 'GitHub', exact: true })).toHaveAttribute('href', 'https://github.com/huacnlee/disktree');
  await expect(page.locator('#command-script')).toHaveText(installCommand);
  expect(errors).toEqual([]);
});

test('hero legend matches the app size colors', async ({ page }) => {
  await page.goto('./');
  await expect(page.locator('.map-legend li')).toHaveText(['1 MB', '10 MB', '100 MB', '1 GB', '4 GB', '10 GB+']);
});

test('published installer is the repository installer', async ({ page }) => {
  const response = await page.request.get('/disktree/install.sh');
  expect(response.ok()).toBe(true);
  expect(await response.text()).toBe(readFileSync(new URL('../../install.sh', import.meta.url), 'utf8'));
});

test('screenshot loads at its declared size', async ({ page }) => {
  await page.goto('./');
  const image = page.locator('.app-screenshot img');
  await image.scrollIntoViewIfNeeded();
  await expect.poll(() => image.evaluate((element: HTMLImageElement) => [element.naturalWidth, element.naturalHeight])).toEqual([2000, 1394]);
});

test('related projects link to the other projects in both languages', async ({ page }) => {
  for (const [route, name] of [['./', 'More projects'], ['./zh-CN/', '更多项目']]) {
    await page.goto(route);
    const related = page.getByRole('region', { name });
    await expect(related.getByRole('link', { name: 'Omasend' })).toHaveAttribute('href', 'https://huacnlee.github.io/omasend/');
    await expect(related.getByRole('link', { name: 'Omamail' })).toHaveAttribute('href', 'https://huacnlee.github.io/omamail/');
    await expect(related.getByRole('link', { name: 'Noop' })).toHaveAttribute('href', 'https://huacnlee.github.io/noop/');
    await expect(related.getByRole('link', { name: 'huacnlee/cargo' })).toHaveAttribute('href', 'https://huacnlee.github.io/cargo/');
    await expect(related.getByRole('link', { name: 'omarchy-mihoro' })).toHaveAttribute('href', 'https://github.com/huacnlee/omarchy-mihoro');
    await expect(related.getByRole('link', { name: 'omarchy-which-key' })).toHaveAttribute('href', 'https://github.com/huacnlee/omarchy-which-key');
    await expect(related.locator('li p')).toHaveCount(6);
  }
});

test('light and dark toggle follows system then persists across reload and language navigation', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('./');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect(page.getByRole('button', { name: 'Dark mode' })).toHaveAttribute('aria-pressed', 'false');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByRole('button', { name: 'Dark mode' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.locator('#language-toggle').click();
  await page.getByRole('menuitemradio', { name: '简体中文' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.getByRole('button', { name: '深色模式' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('language menu handles keyboard selection, escape and outside click', async ({ page }) => {
  await page.goto('./');
  await page.getByRole('button', { name: 'Language' }).focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitemradio', { name: 'English' })).toBeFocused();
  await expect(page.getByRole('menuitemradio', { name: 'English' })).toHaveAttribute('aria-checked', 'true');
  await page.keyboard.press('End');
  await expect(page.getByRole('menuitemradio', { name: '简体中文' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Language' })).toBeFocused();
  await expect(page.getByRole('menu')).toHaveCount(0);
  await page.getByRole('button', { name: 'Language' }).click();
  await page.locator('h1').click();
  await expect(page.getByRole('menu')).toHaveCount(0);
});

test('tabs switch with mouse and keyboard and copy only the active command', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('./');
  const script = page.getByRole('tab', { name: 'Install script', exact: true });
  await expect(script).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('tabpanel')).toHaveCount(1);
  await script.focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: 'mise', exact: true })).toBeFocused();
  await expect(page.getByRole('tabpanel')).toHaveAttribute('id', 'panel-mise');
  await page.getByRole('button', { name: 'Copy', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('Copied');
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('mise use -g github:huacnlee/disktree');
  await page.getByRole('tab', { name: 'mise', exact: true }).focus();
  await page.keyboard.press('Home');
  await expect(script).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(page.getByRole('tab', { name: 'mise', exact: true })).toBeFocused();
  await script.click();
  await page.getByRole('button', { name: 'Copy', exact: true }).click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(installCommand);
});

for (const [width, height] of [[360, 640], [1440, 900]]) {
  test(`installation follows hero in first viewport at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    for (const route of ['./', './zh-CN/']) {
      await page.goto(route);
      expect(await page.locator('.hero').evaluate(element => element.nextElementSibling?.id)).toBe('install');
      const bounds = await page.locator('#command-script').boundingBox();
      expect(bounds).not.toBeNull();
      expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(height);
    }
  });
}

for (const width of [360, 768, 1440]) {
  test(`layout fits ${width}px and reduced motion retains content`, async ({ page }) => {
    await page.setViewportSize({ width, height: 1000 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    for (const route of ['./', './zh-CN/']) {
      await page.goto(route);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      await expect(page.locator('h1')).toBeVisible();
      if (width > 680) await expect(page.locator('.tile-check')).toHaveCSS('opacity', '1');
    }
  });
}

test('keyboard reaches skip link and section link', async ({ page }) => {
  await page.goto('./');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to content' })).toBeFocused();
  await page.keyboard.press('Enter');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'See how it works' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/#workflow$/);
});

test('Chinese headings have no trailing full stop', async ({ page }) => {
  await page.goto('./zh-CN/');
  for (const text of await page.locator('h1, h2, h3, .eyebrow').allTextContents()) {
    expect(text.trim()).not.toMatch(/。/);
  }
});
