# Documentation site deployment

[The public book](https://plentylang.org/docs/) defaults to development until the
first published release is admitted in `book/releases.json`. After that, the
documentation root serves the selected stable release. [Development documentation](https://plentylang.org/docs/dev/)
always follows `main`; admitted releases live under `/docs/VERSION/`.
Every page identifies its version and offers a version menu. Matching chapters
are preserved when switching versions; missing chapters fall back to that
version's homepage. The links work without JavaScript; JavaScript also preserves
the current query and heading fragment when the chapter exists.

`.github/workflows/docs.yml` builds every push to `main`, PR targeting `main`
(including forks), and manual dispatch. Only a main push or main dispatch can
deploy. It does not publish packages or push branches. Release admission and
retention are described in [versioned documentation](documentation-versions.md).

See the [development workflow](workflow.md#documentation-site) for pinned tools and local preview commands.

For only the current checkout, `python3 -B scripts/build_book.py` writes
`book/book`. The complete site helper writes the homepage and all books to
`book/site`, with documentation under `book/site/docs`. Books show Development and its source
commit, explicitly marking local changes. Each version's
`docs-version.json` records the full revision and Cargo package version;
the package version alone does not identify a development build. Labels are
static HTML. Raw `mdbook serve` previews content without deployment metadata.
To preview the complete site with Live Server, run these commands from the
repository root and open `http://127.0.0.1:8081/`.

```sh
python3 -B scripts/build_docs_site.py
live-server --host 127.0.0.1 --port 8081 --poll book/site
```

Run the build command again after edits. To rebuild automatically while editing
the landing page or its artwork, run this in another terminal.

```sh
watchexec --postpone --watch website --watch logos -- \
  python3 -B scripts/build_docs_site.py
```

Live Server reloads when the output changes. To test missing routes as GitHub
serves them, use a static HTTP server configured to serve the root `404.html`
for unknown paths.

The build needs no Plenty compiler. Do not use `mdbook test` for Plenty fences;
the [tutorial integration tests](workflow.md#runnable-documentation) execute those examples. The output checker
verifies rendered local links stay inside the artifact and reach existing files,
and checks search, print, 404 and custom highlighting assets. Source checking
alone cannot catch a malformed `SUMMARY.md` that leaves a chapter unrendered.
Neither checker validates external URLs or heading anchors.

Book links must target published pages or assets. A relative link to a repository
file outside `book/src` is rejected by the source checker even when it exists,
because mdBook does not publish that file. Refer to such files with a repository-relative
code path (for example, `experiments/rust-crates/README.md`), or include the needed
content in a book page. Run the complete site helper after adding links; a
successful `mdbook build` alone does not validate generated links, including
those in `print.html`.

The Pages artifact is only `book/site`, which remains ignored. It contains all
admitted versions together, so development updates preserve older books. Each
build receives its own `site-url` prefix for nested 404 assets. Check navigation,
anchors, search, print, version switching and Plenty highlighting under each
prefix; repeat those checks on the public site after deployment.

## Homepage and documentation paths

`website/index.html` and `website/assets/site.css` provide the homepage at `/`.
The rounded-square artwork lives in `logos/`. The builder publishes its SVG and PNG
under `assets/`. The book uses the same mark and a matching dark-blue palette
for its default Plenty theme. Readers can select any bundled mdBook theme
from the theme menu. When updating the logo, keep the mdBook favicon copies
in `book/theme/` in sync with the source artwork.

Documentation lives at `/docs/`, development documentation at `/docs/dev/`, and
admitted releases at `/docs/VERSION/`. Every book's version banner includes a
**Plenty home** link. Known chapter and version URLs from the former root layout
get forwarding pages to their new locations, preserving queries and fragments
with JavaScript and providing an ordinary link without it. The root homepage
and site-wide 404 page are reserved; the former root book index is now `/docs/`.
Redirects cover pages present in the books built by the current manifest, not
a registry of previously removed pages.

GitHub serves the root `404.html` even for missing documentation paths. It links
to the homepage and docs without JavaScript; with JavaScript, recognized docs
and legacy version prefixes open the corresponding book's 404 page. The complete
site check covers the homepage, logo, styles, redirects, and all book links.

## Repository transfer and custom domain

The compiler repository is `plentylang/plenty`. Its Pages site uses
`plentylang.org`, with the homepage at `/`, documentation at `/docs/`, and
development documentation at `/docs/dev/`. Verify the domain under the
organization, then set that hostname in
**Settings > Pages > Custom domain** before pointing the domain's apex DNS to
GitHub Pages. Enable HTTPS when its certificate is ready. Follow GitHub's
[custom-domain setup](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site)
and [domain verification](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/verifying-your-custom-domain-for-github-pages).

GitHub redirects the old repository address after a transfer, but does not
redirect its Pages site. Keeping `cjrh.info/plenty/` working requires a redirect
on that host, preserving the chapter path, query, and fragment. The new site's
layout replaces the old `/plenty/` prefix with `/docs/`. A repository transfer
alone does not publish local documentation changes or configure DNS.

## First publication and recovery

After review, merge and authorization to publish, an administrator must:

1. Check account/repository visibility and organization policies allow Pages
   and the pinned actions. Choose **Settings > Pages > Build and deployment >
   Source: GitHub Actions**. The workflow does not enable Pages automatically.
2. Set the `github-pages` environment's deployment branch policy to `main`
   only, preserving any required reviewers.
3. Run **Documentation** from current `main`, approve its environment if
   required, and inspect the deployed URL and source commit in the run summary.

Build jobs have only `contents: read`; PRs do not configure/upload Pages or use
the deployment environment. The deploy job has `pages: write`, `id-token: write`
and `contents: read` for a main-tip API check. It uses the built-in token, with
no PAT. Actions are pinned by SHA; the upload action also pins its nested action.

Deployment jobs share one concurrency group and do not cancel a running
deployment. After acquiring that slot and any environment approval, a run checks
the live main SHA and skips if superseded. Queue ordering is not guaranteed:
rerun current main if its pending deployment was replaced by an older run that
then skipped. A commit arriving after the check can briefly leave the previous
revision live until the next successful run; serialization prevents an older
deployment finishing after a newer deployment. API failures fail closed.

Failed checks or deployments leave the last successful site available. Rerun
current main for transient failures; fix or revert bad content through review
and deploy the resulting main commit. Do not roll back by dispatching an old
branch or deleting the site. Tool/action upgrades require another complete book
build and theme/navigation check. See the official
[Pages workflow guide](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
and [mdBook HTML configuration](https://rust-lang.github.io/mdBook/format/configuration/renderers.html).
