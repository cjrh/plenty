# Documentation versions

The [documentation site](https://cjrh.info/plenty/) is assembled from an explicit
manifest, `book/releases.json`, and the current checkout. There are no published
versions in the initial manifest: a Git tag alone does not establish a release.

| Path | Documentation |
| --- | --- |
| `/plenty/` | Selected stable release, or development while `stable` is null |
| `/plenty/dev/` | Current `main` |
| `/plenty/VERSION/` | Admitted release from its pinned source commit |

Development pages show **Development**, their source commit, and any local
changes. Released pages show **Plenty VERSION**. Each book includes a version
menu and `docs-version.json`; `versions.json` at the site root lists the entire
catalog. Selecting another version retains the chapter if it exists there,
otherwise opens the homepage. A heading may not exist in an older chapter;
the browser still opens that chapter even when the fragment has no target.

## Admit a published release

After a stable GitHub Release is published successfully:

1. Verify the published tag and record `git rev-parse 'vX.Y.Z^{commit}'`.
   Do not admit drafts, prereleases, or a tag whose release job failed.
2. Add a manifest entry with `version`, `tag`, and the full `commit` hash. For
   example, the structure is `{ "version": "0.2.0", "tag": "v0.2.0",
   "commit": "FULL_40_CHARACTER_COMMIT_HASH" }`. Set `stable` to the version
   that should be the default; never infer it from main's Cargo version.
3. Keep all previously admitted entries. Archive URLs are permanent; do not
   rewrite tag/commit pins. The helper rejects moved tags, version mismatches,
   duplicates and unsafe version names. Removing an entry would remove its
   deployed archive, so retention changes require an explicit review decision.
4. Run `python3 -B scripts/build_docs_site.py`. It uses the manifest's exact
   mdBook version, currently 0.5.4. When changing that pin, also change the
   workflow installation pin and validate every archive with the new renderer.
5. Check version switching, search, custom highlighting, print, nested 404
   handling, and existing root links. Submit the manifest change through the
   normal local PR workflow. Its eventual push to `main` rebuilds the site;
   publishing a release by itself does not change the documentation default.

Historical books use their own prose, configuration, highlighting and included
files from the pinned tree. The renderer and navigation layer come from the
current site builder. Thus source revisions stay fixed, while presentation can
receive fixes. No old compiler or repository script runs during extraction;
mdBook may execute preprocessors declared in a historical book configuration,
so review such additions when admitting releases.

The v0.1.1 tag predates a table-of-contents indentation fix. If that version is
eventually published and admitted, its entry needs `summary_indent_repairs`
with `tutorial/11-concurrency/stop-on-signal.md` and
`tutorial/11-concurrency/bounded-io.md`. This explicitly recorded, rendering-only
repair restores omitted chapters; it does not replace historical prose. The
build fails if the repair no longer matches. Metadata records applied repairs.

## Deployment and existing links

The helper builds all books in temporary directories and checks their HTML
links and assets before replacing `book/site`. Includes can refer outside
`book/`, so releases are extracted from their complete tracked tree. Only
generated books enter the artifact; no source trees or issues database are
uploaded. Logged mdBook errors are fatal even if the renderer exits successfully.

The first stable admission deliberately changes the root's content from
development to stable. Existing chapter paths found in stable continue to work.
Development-only paths get small forwarding pages to `/plenty/dev/`, preserving
queries and fragments with JavaScript and offering an ordinary link without it.
The builder creates these for development pages present at build time; it is
not a historical redirect registry for subsequently deleted or renamed chapters.

GitHub supplies the site's root 404 page for missing URLs in any version.
JavaScript forwards known version prefixes to their corresponding 404 page;
without JavaScript the root 404 and version menu remain usable. No server-side
rewrite rules, new domain settings, token, or extra Pages environment are needed.

Local admission experiments can use `--manifest /tmp/releases.json` and
`--output /tmp/docs-preview` without changing the reviewed manifest. The output
directory must not exist, or must contain a previous builder's `versions.json`.
Follow [the deployment recovery procedure](workflow.md#first-publication-and-recovery)
for failures. No archive is retained solely in short-lived Actions artifacts:
each deployment rebuilds the complete admitted catalog from Git.
