# Documentation site deployment

[The public book](https://cjrh.info/plenty/) defaults to development until the
first published release is admitted in `book/releases.json`. After that, the
root serves the selected stable release. [Development documentation](https://cjrh.info/plenty/dev/)
always follows `main`; admitted releases live under `/plenty/VERSION/`.
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
`book/book`. The complete site helper writes `book/site`. Both show Development
and its source commit, explicitly marking local changes. Each version's
`docs-version.json` records the full revision and Cargo package version;
the package version alone does not identify a development build. Labels are
static HTML. Raw `mdbook serve` previews content without deployment metadata.
To preview the complete site, mount `book/site` at `/plenty/` on a static HTTP
server; configure unknown paths to serve its root `404.html` as GitHub does.

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
