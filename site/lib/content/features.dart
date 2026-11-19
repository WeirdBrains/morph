/// Morph's features, from CHANGELOG.md (shipped) and PRIORITIES.md (planned).
library;

import '../components/layout.dart';

class Feature {
  const Feature({
    required this.id,
    required this.title,
    required this.body,
    required this.image,
    required this.alt,
    required this.caption,
    required this.status,
    this.when = '',
  });

  /// Anchor on /features.
  final String id;
  final String title;
  final String body;

  /// File in web/images. `concept-*.svg` files are illustrations, not screenshots.
  final String image;
  final String alt;
  final String caption;
  final Status status;

  /// The release it shipped in, or the milestone it is planned for.
  final String when;

  bool get concept => image.startsWith('concept-');
}

const features = <Feature>[
  Feature(
    id: 'browser',
    title: 'A standards-based browser',
    body:
        'Tabs, an address bar that searches when you type a phrase, back and forward, find in page, '
        'and a page per crashed tab while the rest keep running. Pages render in the system web view: '
        'WebKitGTK on Linux, WKWebView on macOS, WebView2 on Windows and Android WebView. Since 0.2.1: '
        'more than one window (Ctrl+N, ⌘N on macOS), Move to new window from the tab menu, and Morph '
        'reopens the windows and tabs that were open when it quit.',
    image: 'screenshot-main-tabs.webp',
    alt: 'Morph on Linux with four tabs open and a tide pool field guide page loaded.',
    caption: 'Morph 0.2.0 on Linux with four tabs open.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'downloads',
    title: 'Downloads that resume',
    body:
        'A downloads panel next to the menu (Ctrl+Shift+Y) with progress, speed and time left, and Pause, '
        'Resume, Cancel, Retry and Show in folder. Downloads behind a login resume with the page’s session, '
        'which is kept encrypted and deleted when the download ends. Since 0.2.1 that includes sites that sign '
        'you in with JavaScript: Morph retries with the page’s sign-in token, and never logs it.',
    image: 'screenshot-downloads.webp',
    alt: 'The Morph downloads panel showing a file at 24.2 of 41.9 MB with Pause and Cancel buttons.',
    caption: 'The downloads panel during a download.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'library',
    title: 'Bookmarks and history',
    body:
        'Bookmarks with folders, tags and search, and your history, open beside the page so you can keep '
        'reading. Both sync between your linked devices.',
    image: 'screenshot-bookmarks.webp',
    alt: 'The bookmarks panel beside a page, with tag chips and the folders Outdoors, Kitchen and Space.',
    caption: 'Bookmarks with folders and tags, beside the page.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'identity',
    title: 'One identity, yours to keep',
    body:
        'Each profile gets a Weirdnet identity, the same kind Muninn uses, so one person has one key across '
        'Weirdnet apps. Back it up as 24 recovery words, 3-of-5 paper shards or an encrypted file. Creating it '
        'also encrypts the profile’s bookmarks and history.',
    image: 'screenshot-identity.webp',
    alt: 'The Identity and devices panel offering to create an identity, restore from recovery words or a backup file, or link from another device.',
    caption: 'Setting up an identity for a profile.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'sync',
    title: 'Link your devices and sync',
    body:
        'Link another device with a QR code or a link and a short code to compare. Bookmarks, history, '
        'search engine, home page and theme then sync between your devices over Weirdnet, soon after unlock '
        'and every 30 seconds. Settings > Linked devices lists them and revokes one.',
    image: 'screenshot-link-device.webp',
    alt: 'The identity panel showing a QR code and a weirdnet-morph link for linking a new device, connected to the WeirdBrains public relay.',
    caption: 'Linking a new device with a QR code.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'relays',
    title: 'Relays, discovery and manual peers',
    body:
        'Morph reaches your other devices through WeirdBrains relays or your own. Settings > Relays shows the '
        'relay and checks it, turns Weirdnet discovery off for networks where you want it off, and adds peers '
        'by hand. An admin policy can lock discovery off.',
    image: 'screenshot-relays.webp',
    alt: 'Morph settings showing the Linked devices and Relays sections, with the WeirdBrains public relay, Weirdnet discovery and manual peers.',
    caption: 'Settings > Linked devices and Relays.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'look',
    title: 'Your own look and layout',
    body:
        'Light or dark, tabs on the top, bottom, left or right, and a collapsible side tab list. Each '
        'profile keeps its own. A drag-and-drop layout mode for placing components is planned.',
    image: 'screenshot-light-side.webp',
    alt: 'Morph in the light theme with tabs in a list on the left side.',
    caption: 'The light theme with tabs on the left.',
    status: Status.shipped,
    when: '0.2.0',
  ),
  Feature(
    id: 'p2p',
    title: 'A peer-to-peer web',
    body:
        'Sites served from your own devices at weirdnet-morph:// addresses, found through relays and '
        'Grindvakt dynamic DNS, with signed scheme records so a link always opens the right app. Today '
        'weirdnet- links already call the app over Weirdnet; real peer content comes next.',
    image: 'concept-p2p-web.svg',
    alt: 'Concept diagram: a Morph device hosting a site, a relay, and a peer opening a weirdnet-morph link.',
    caption: 'How peer sites are meant to work.',
    status: Status.planned,
    when: 'v0.3',
  ),
  Feature(
    id: 'passwords',
    title: 'Password saver',
    body: 'Save and fill passwords in Morph, kept encrypted on your own devices.',
    image: 'concept-passwords.svg',
    alt: 'Concept mock-up of a sign-in form with a Save this password prompt.',
    caption: 'An idea of the save prompt.',
    status: Status.planned,
    when: 'v0.3',
  ),
  Feature(
    id: 'ai',
    title: 'Local AI',
    body:
        'Local models, on by default with one switch to turn them off, and one action that clears the AI’s '
        'stored context. It starts as a chat partner and rubber duck; later a Student that learns only from '
        'browsing you opt into, and a Researcher that keeps research journals. Morph is MCP-native: its AI runs '
        'on the shared weirdnet-agent runtime, where Model Context Protocol servers plug in as tool sources '
        'beside Morph’s own, each with its own switch and the same per-site approvals, and outside agents that '
        'drive Morph over MCP have to be approved first.',
    image: 'concept-local-ai.svg',
    alt: 'Concept diagram: a web page beside a local model panel with MCP tools; AI runs on the device and can be switched off.',
    caption: 'Local AI beside the page, with MCP tools.',
    status: Status.planned,
    when: 'v0.4',
  ),
  Feature(
    id: 'projects',
    title: 'Projects and code spaces',
    body:
        'Projects with templates, notes, a code editor and code spaces: groups of repos and folders, Rust '
        'first. A project is private or public, gated or open, and public ones can be served over HTTPS at '
        'your own name.',
    image: 'concept-projects.svg',
    alt: 'Concept mock-up of a project with notes, templates and a code space beside a code editor.',
    caption: 'A project with a code space.',
    status: Status.planned,
    when: 'v0.4',
  ),
  Feature(
    id: 'extensions',
    title: 'Sandboxed extensions',
    body:
        'Chrome and Firefox extensions running unchanged, with the WebExtensions API in Rust, plus native '
        'extensions. Each one is signed by its author, installed only with your permission, and runs in its '
        'own sandboxed process.',
    image: 'concept-extensions.svg',
    alt: 'Concept diagram: an extension running in a sandbox and talking to Morph over gRPC with permission checks.',
    caption: 'Extensions in their own sandbox.',
    status: Status.planned,
    when: 'v0.5',
  ),
  Feature(
    id: 'social',
    title: 'Notes, a journal, chat and forums',
    body:
        'Write notes and a journal, chat with the peers you know, and run forums that are public or private, '
        'gated or open.',
    image: 'concept-notes-chat.svg',
    alt: 'Concept illustration of notes, journal, chat and forum columns.',
    caption: 'Notes, journal, chat and forums.',
    status: Status.planned,
    when: 'Later',
  ),
  Feature(
    id: 'updates',
    title: 'Signed updates, never forced',
    body:
        'Signed updates through Salty Gull, announced over Weirdnet so devices behind NAT hear about them. '
        'You opt in to each update, or to all of them.',
    image: 'concept-updates.svg',
    alt: 'Concept diagram: a signed release passed through relays to Morph, which asks before updating.',
    caption: 'Signed updates that ask first.',
    status: Status.planned,
    when: 'Later',
  ),
];
