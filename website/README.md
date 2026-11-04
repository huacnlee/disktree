# Disktree website

Static Astro site using Bun and GitHub Pages. Fonts are bundled locally. English lives at `/disktree/`, Simplified Chinese at `/disktree/zh-CN/`.

```sh
cd website
bun install --frozen-lockfile
bun run dev
bun run build
bun x playwright install chromium
bun run test
```

The default base is `/disktree`. Set `BASE_PATH` and `SITE_URL` together for another host. The production browser tests deliberately verify the GitHub Pages path.

`.github/workflows/website.yml` builds and tests pull requests, then deploys `main` using GitHub Pages. Repository Settings → Pages → Source must be **GitHub Actions**. No custom domain or external hosting credentials are required.

`public/install.sh` is a symlink to the repository's `install.sh`, so the published installer always matches `main`. `public/favicon.svg` is a copy of `assets/icon.svg`. `public/screenshot.png` is the screenshot used in the README.

The hero treemap is an illustration using the app's size colors, not a screenshot or a browser implementation of Disktree. The site follows the system color scheme until an explicit choice is saved.
