import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';
import 'package:jaspr_router/jaspr_router.dart';

import 'components/layout.dart';
import 'pages/download.dart';
import 'pages/features.dart';
import 'pages/home.dart';
import 'pages/roadmap.dart';
import 'site.dart';

/// Every page of the site. Static mode renders each route to its own HTML file.
class App extends StatelessComponent {
  const App({super.key});

  @override
  Component build(BuildContext context) {
    return Router(
      routes: [
        Route(
          path: '/',
          title: '$siteTitle: $siteTagline',
          builder: (_, _) => const Layout(
            path: '/',
            title: '$siteTitle: $siteTagline',
            description: siteDescription,
            child: Home(),
          ),
        ),
        Route(
          path: '/features',
          title: 'Features · $siteTitle',
          builder: (_, _) => const Layout(
            path: '/features',
            title: 'Features · $siteTitle',
            description:
                'What Morph $latestVersion does: tabs and the system web view, downloads that resume, bookmarks and '
                'history, one Weirdnet identity, linked devices and sync, relays, and what is coming next.',
            child: FeaturesPage(),
          ),
        ),
        Route(
          path: '/roadmap',
          title: 'Roadmap · $siteTitle',
          builder: (_, _) => const Layout(
            path: '/roadmap',
            title: 'Roadmap · $siteTitle',
            description:
                'Morph’s roadmap: v0.3 peer-to-peer and a password saver, v0.4 local AI and projects, v0.5 '
                'extensions, and platform status.',
            child: RoadmapPage(),
          ),
        ),
        Route(
          path: '/download',
          title: 'Download Morph $latestVersion · $siteTitle',
          builder: (_, _) => const Layout(
            path: '/download',
            title: 'Download Morph $latestVersion · $siteTitle',
            description:
                'Download Morph $latestVersion for Linux, macOS, Windows and Android, install morph-bin from TAUR, '
                'or build from source.',
            child: DownloadPage(),
          ),
        ),
      ],
      errorBuilder: (_, _) => Layout(
        path: '',
        title: 'Not found · $siteTitle',
        description: siteDescription,
        child: section([
          h1([Component.text('Not found')]),
          p([
            Component.text('That page is not here. '),
            a([Component.text('Back home')], href: '/'),
          ], classes: 'lede'),
        ], classes: 'hero page-hero wrap'),
      ),
    );
  }
}
