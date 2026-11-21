/// Public URLs and copy used across the Morph project site.
library;

const siteUrl = 'https://morph.weirdbrains.com';
const siteTitle = 'Morph';
const siteTagline = 'A private, decentralized browser with local AI built in.';
const siteDescription =
    'Morph is a private, decentralized browser with local AI built in. '
    'The browser is one part of the app, next to chat, notes, projects, '
    'and a peer-to-peer web on Weirdnet.';
const githubUrl = 'https://github.com/WeirdBrains/morph';
const weirdBrainsUrl = 'https://weirdbrains.com';
const weirdnetUrl = 'https://net.weirdbrains.com';
const taurUrl = 'https://pkg.taur.weirdbrains.com';
const ogImageUrl = '$siteUrl/images/morph-icon-512.png';
const ogImageAlt = 'The Morph app icon: a white M with cyan and magenta glitch slices.';

/// The latest published release (GitHub release, not a pre-release).
const latestVersion = '0.2.3';
const latestDate = 'November 20, 2026';
const releasesUrl = '$githubUrl/releases';
const latestReleaseUrl = '$githubUrl/releases/tag/v$latestVersion';
const changelogUrl = '$githubUrl/blob/main/CHANGELOG.md';
const unsignedDocsUrl = '$githubUrl/blob/main/docs/install-macos-windows.md';

String assetUrl(String name) => '$githubUrl/releases/download/v$latestVersion/$name';

/// The newest pre-release. Never the stable recommendation.
const previewVersion = '0.3.0-preview.1';
const previewTag = 'v$previewVersion';
const previewReleaseUrl = '$githubUrl/releases/tag/$previewTag';
String previewAssetUrl(String name) => '$githubUrl/releases/download/$previewTag/$name';
