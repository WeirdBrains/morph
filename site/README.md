# Morph website

The project site for [Morph](https://github.com/WeirdBrains/morph), built with
[Jaspr](https://jaspr.site) in static mode so the output can be hosted anywhere.

The published site is [https://morph.weirdbrains.com](https://morph.weirdbrains.com).
The custom domain needs a DNS `CNAME` of `morph` to `weirdbrains.github.io`, and
GitHub Pages set to Source: GitHub Actions, custom domain
`morph.weirdbrains.com`, with Enforce HTTPS.

This package uses Jaspr `0.24.0-beta.1`. That is the version that resolves on
Dart 3.14 (the SDK that ships with Flutter 3.50). Jaspr `0.23.5` does not.

## Pages

- `/`: what Morph is, a few screenshots, the latest version and a Coming soon teaser
- `/features`: one card per feature, each with one image (`lib/content/features.dart`)
- `/roadmap`: milestones and statuses from `PRIORITIES.md` and `docs/handoff.md`
- `/download`: the files on the latest GitHub release, TAUR, Nix and building from source

Old one-page anchors (`/#features`, `/#install` and so on) still land on a section of `/`.

## Images

Screenshots in `web/images/screenshot-*.webp` are of the released app (not `main`), taken
in a private Xvfb display with a demo profile and local test pages, never on a live
desktop (collective `AGENTS.md`). `web/images/concept-*.svg` are illustrations of planned
features, labeled Concept; they are not screenshots.

## After a release

Bump `latestVersion` and `latestDate` in `lib/site.dart`, the file sizes in
`lib/pages/download.dart`, and move shipped items in `lib/content/features.dart` and
`lib/pages/roadmap.dart`. Retake screenshots of anything whose look changed.

## Prerequisites

Dart 3.14, or the Dart SDK from Flutter 3.50. Then:

```sh
dart pub global activate jaspr_cli 0.24.0-beta.1
cd site
dart pub get
```

## Development

```sh
cd site
jaspr serve
```

The development server is at `http://localhost:8080`.

## Build

```sh
cd site
jaspr build --sitemap-domain https://morph.weirdbrains.com
```

The static output is written to `site/build/jaspr/`. A `CNAME` file for
`morph.weirdbrains.com` is included from `web/CNAME`. The site uses a `<base href="/">`,
so it is meant to be served at the domain root, not under a `/morph/` path.

To preview the built files:

```sh
python3 -m http.server 8080 --directory build/jaspr
```
